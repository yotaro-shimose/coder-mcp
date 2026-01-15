use crate::models::{BashCommand, BashEvent, BashEventPage, BashOutput, ExecuteBashRequest};
use crate::runtime::terminal::TerminalSession;
use chrono::Utc;
use glob::glob;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub struct BashEventService {
    pub bash_events_dir: PathBuf,
    pub terminal_session: Arc<Mutex<TerminalSession>>,
}

impl BashEventService {
    pub fn new(bash_events_dir: PathBuf, workdir: Option<PathBuf>) -> Self {
        fs::create_dir_all(&bash_events_dir).expect("Failed to create bash events dir");
        let terminal_session =
            TerminalSession::new(workdir).expect("Failed to initialize terminal session");

        Self {
            bash_events_dir,
            terminal_session: Arc::new(Mutex::new(terminal_session)),
        }
    }

    fn save_event(&self, event: &BashEvent) {
        let timestamp_str = event.timestamp().format("%Y%m%d%H%M%S");
        let kind = match event {
            BashEvent::BashCommand(_) => "BashCommand",
            BashEvent::BashOutput(_) => "BashOutput",
        };

        let filename = match event {
            BashEvent::BashCommand(c) => format!("{}_{}_{}", timestamp_str, kind, c.id.simple()),
            BashEvent::BashOutput(o) => format!(
                "{}_{}_{}_{}",
                timestamp_str,
                kind,
                o.command_id.simple(),
                o.id.simple()
            ),
        };

        let path = self.bash_events_dir.join(filename);
        let json = serde_json::to_string_pretty(event).expect("Failed to serialize event");
        fs::write(path, json).expect("Failed to write event file");
    }

    fn load_event(path: PathBuf) -> Option<BashEvent> {
        let content = fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
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
        let mut events = Vec::new();

        let pattern = if let Some(cid) = command_id {
            format!("*{}*", cid.simple())
        } else {
            "*".to_string()
        };

        let full_pattern = self.bash_events_dir.join(pattern);

        if let Ok(entries) = glob(full_pattern.to_str().unwrap_or("")) {
            for entry in entries.filter_map(Result::ok) {
                if let Some(event) = Self::load_event(entry) {
                    let match_cmd = match command_id {
                        Some(cid) => match &event {
                            BashEvent::BashCommand(c) => c.id == cid,
                            BashEvent::BashOutput(o) => o.command_id == cid,
                        },
                        None => true,
                    };

                    if match_cmd {
                        events.push(event);
                    }
                }
            }
        }

        events.sort_by_key(|e| e.timestamp());

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
