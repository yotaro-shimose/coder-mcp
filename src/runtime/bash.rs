use crate::models::{BashCommand, BashOutput, ExecuteBashRequest};
use crate::runtime::terminal::TerminalSession;
use chrono::Utc;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Clone)]
pub struct BashEventService {
    pub terminal_session: Arc<Mutex<TerminalSession>>,
}

impl BashEventService {
    pub fn new(workdir: Option<PathBuf>) -> Self {
        let terminal_session =
            TerminalSession::new(workdir).expect("Failed to initialize terminal session");

        Self {
            terminal_session: Arc::new(Mutex::new(terminal_session)),
        }
    }

    pub fn start_bash_command(
        &self,
        req: ExecuteBashRequest,
    ) -> (BashCommand, oneshot::Receiver<BashOutput>) {
        let command_id = Uuid::new_v4();
        let bash_command = BashCommand {
            id: command_id,
            timestamp: Utc::now(),
            command: req.command.clone(),
            cwd: req.cwd.clone(),
            timeout: req.timeout.unwrap_or(300),
        };

        let (tx, rx) = oneshot::channel();
        let service = self.clone();
        let cmd_clone = bash_command.clone();

        tokio::spawn(async move {
            let out = service.execute_bash_command_background(cmd_clone).await;
            let _ = tx.send(out);
        });

        (bash_command, rx)
    }

    async fn execute_bash_command_background(&self, command: BashCommand) -> BashOutput {
        let terminal_session = self.terminal_session.clone();
        let cmd_text = command.command.clone();
        let timeout_val = command.timeout;

        let result = tokio::task::spawn_blocking(move || {
            let mut session = terminal_session.lock().unwrap();
            session.execute(&cmd_text, timeout_val * 1000) // ms
        })
        .await;

        match result {
            Ok(Ok((output, exit_code))) => BashOutput {
                id: Uuid::new_v4(),
                timestamp: Utc::now(),
                command_id: command.id,
                order: 0,
                exit_code: Some(exit_code),
                stdout: Some(output),
                stderr: None,
            },
            Ok(Err(e)) => BashOutput {
                id: Uuid::new_v4(),
                timestamp: Utc::now(),
                command_id: command.id,
                order: 0,
                exit_code: Some(-1),
                stdout: None,
                stderr: Some(format!("Error executing command: {}", e)),
            },
            Err(join_err) => BashOutput {
                id: Uuid::new_v4(),
                timestamp: Utc::now(),
                command_id: command.id,
                order: 0,
                exit_code: Some(-1),
                stdout: None,
                stderr: Some(format!("Task execution panicked: {}", join_err)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_bash_event_service_execution() {
        let service = BashEventService::new(None);

        let req = ExecuteBashRequest {
            command: "echo test_bash_service".to_string(),
            cwd: None,
            timeout: Some(5),
        };

        let (_cmd, rx) = service.start_bash_command(req);

        let out = tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .expect("Timed out waiting for bash output")
            .expect("Bash sender was dropped");

        assert_eq!(out.exit_code, Some(0));
        let output = out.stdout.as_ref().unwrap();
        assert!(
            output.contains("test_bash_service"),
            "Output did not contain expected string. Got: '{}'",
            output
        );
    }
}
