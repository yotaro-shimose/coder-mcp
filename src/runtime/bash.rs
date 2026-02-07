use crate::models::{BashCommand, BashEvent, BashEventPage, BashOutput, ExecuteBashRequest};
use crate::runtime::terminal::TerminalSession;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub struct BashEventService {
    pub db: Arc<Mutex<Connection>>,
    pub terminal_session: Arc<Mutex<TerminalSession>>,
}

impl BashEventService {
    pub fn new(bash_events_dir: PathBuf, workdir: Option<PathBuf>) -> Self {
        fs::create_dir_all(&bash_events_dir).expect("Failed to create bash events dir");
        let db_path = bash_events_dir.join("bash_events.db");
        let conn = Connection::open(db_path).expect("Failed to open SQLite database");

        // Initialize table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bash_events (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                command_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                json_data TEXT NOT NULL
            )",
            [],
        )
        .expect("Failed to create tables");

        // indexes
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_bash_events_command_id ON bash_events (command_id)",
            [],
        )
        .expect("Failed to create index on command_id");
        
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_bash_events_timestamp ON bash_events (timestamp)",
            [],
        )
        .expect("Failed to create index on timestamp");

        let terminal_session =
            TerminalSession::new(workdir).expect("Failed to initialize terminal session");

        Self {
            db: Arc::new(Mutex::new(conn)),
            terminal_session: Arc::new(Mutex::new(terminal_session)),
        }
    }

    fn save_event(&self, event: &BashEvent) {
        let (id, command_id, event_type) = match event {
            BashEvent::BashCommand(c) => (c.id, c.id, "BashCommand"),
            BashEvent::BashOutput(o) => (o.id, o.command_id, "BashOutput"),
        };

        let json = serde_json::to_string(event).expect("Failed to serialize event");
        let timestamp_str = event.timestamp().to_rfc3339();

        let conn = self.db.lock().unwrap();
        conn.execute(
            "INSERT INTO bash_events (id, timestamp, command_id, event_type, json_data) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id.simple().to_string(), timestamp_str, command_id.simple().to_string(), event_type, json],
        )
        .expect("Failed to insert event info db");
    }

    pub fn start_bash_command(&self, req: ExecuteBashRequest) -> BashCommand {
        let command_id = Uuid::new_v4();
        let bash_command = BashCommand {
            id: command_id,
            timestamp: Utc::now(),
            command: req.command.clone(),
            cwd: req.cwd.clone(),
            timeout: req.timeout.unwrap_or(300),
        };

        // Save initial command event synchronously
        self.save_event(&BashEvent::BashCommand(bash_command.clone()));

        let service = self.clone();
        let cmd_clone = bash_command.clone();

        // Spawn background task
        tokio::spawn(async move {
            service.execute_bash_command_background(cmd_clone).await;
        });

        bash_command
    }

    async fn execute_bash_command_background(&self, command: BashCommand) {
        let terminal_session = self.terminal_session.clone();
        let cmd_text = command.command.clone();
        let timeout_val = command.timeout;

        let result = tokio::task::spawn_blocking(move || {
            let mut session = terminal_session.lock().unwrap();
            session.execute(&cmd_text, timeout_val * 1000) // ms
        })
        .await;

        match result {
            Ok(Ok((output, exit_code))) => {
                let out = BashOutput {
                    id: Uuid::new_v4(),
                    timestamp: Utc::now(),
                    command_id: command.id,
                    order: 0,
                    exit_code: Some(exit_code),
                    stdout: Some(output),
                    stderr: None, // We merged everything into stdout in this simple PTY model
                };
                self.save_event(&BashEvent::BashOutput(out));
            }
            Ok(Err(e)) => {
                // Error executing
                let out = BashOutput {
                    id: Uuid::new_v4(),
                    timestamp: Utc::now(),
                    command_id: command.id,
                    order: 0,
                    exit_code: Some(-1),
                    stdout: None,
                    stderr: Some(format!("Error executing command: {}", e)),
                };
                self.save_event(&BashEvent::BashOutput(out));
            }
            Err(join_err) => {
                let out = BashOutput {
                    id: Uuid::new_v4(),
                    timestamp: Utc::now(),
                    command_id: command.id,
                    order: 0,
                    exit_code: Some(-1),
                    stdout: None,
                    stderr: Some(format!("Task execution panicked: {}", join_err)),
                };
                self.save_event(&BashEvent::BashOutput(out));
            }
        }
    }

    pub fn search_bash_events(&self, command_id: Option<Uuid>) -> BashEventPage {
        let conn = self.db.lock().unwrap();
        let mut stmt;
        let mut rows = if let Some(cid) = command_id {
            stmt = conn.prepare("SELECT json_data FROM bash_events WHERE command_id = ? ORDER BY timestamp ASC").unwrap();
            stmt.query(params![cid.simple().to_string()]).unwrap()
        } else {
            stmt = conn.prepare("SELECT json_data FROM bash_events ORDER BY timestamp ASC").unwrap();
            stmt.query([]).unwrap()
        };

        let mut events = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            let json_data: String = row.get(0).unwrap();
            if let Ok(event) = serde_json::from_str(&json_data) {
                events.push(event);
            }
        }

        BashEventPage {
            items: events,
            next_page_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_bash_event_service_execution() {
        let dir = tempdir().unwrap();
        let service = BashEventService::new(dir.path().to_path_buf(), None);

        let req = ExecuteBashRequest {
            command: "echo test_bash_service".to_string(),
            cwd: None,
            timeout: Some(5),
        };

        let cmd = service.start_bash_command(req);

        // Wait for execution
        let mut attempts = 0;
        let mut found_output = false;

        while attempts < 20 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let page = service.search_bash_events(Some(cmd.id));
            if let Some(last) = page.items.last() {
                if let BashEvent::BashOutput(out) = last {
                    found_output = true;
                    assert_eq!(out.exit_code, Some(0));
                    let output = out.stdout.as_ref().unwrap();
                    assert!(
                        output.contains("test_bash_service"),
                        "Output did not contain expected string. Got: '{}'",
                        output
                    );
                    break;
                }
            }
            attempts += 1;
        }

        assert!(found_output, "Did not find bash output");
    }
}
