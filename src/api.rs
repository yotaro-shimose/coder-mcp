use crate::models::{BashEvent, ExecuteBashRequest};
use crate::runtime::bash::BashEventService;
use crate::service::StrReplaceArgs;
use crate::tools::file_tools::{run_str_replace, run_view_file, ViewFileArgs};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

#[derive(Clone)]
pub struct AppState {
    pub bash: Arc<BashEventService>,
    pub workspace_dir: PathBuf,
    pub editor_history: Arc<Mutex<HashMap<PathBuf, Vec<String>>>>,
    pub truncation_limit: usize,
}

#[derive(Deserialize)]
pub struct RunPayload {
    // No arguments needed for now, but good to have a struct for future extensibility
}

#[derive(Deserialize)]
pub struct StrReplacePayload {
    pub old_str: String,
    pub new_str: String,
}

#[derive(Deserialize)]
pub struct ViewFilePayload {
    pub path: String,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
}

#[derive(Serialize)]
pub struct CommandOutput {
    pub output: String,
}

fn truncate_output(text: String, limit: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= limit {
        return text;
    }

    let half = 3000; // Keep 3000 chars from start and end
    if char_count <= half * 2 {
        return text;
    }

    let start: String = text.chars().take(half).collect();
    let end: String = text
        .chars()
        .rev()
        .take(half)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{}...[truncated]...{}", start, end)
}

pub async fn run_handler(
    State(state): State<Arc<AppState>>,
    Json(_payload): Json<RunPayload>,
) -> impl IntoResponse {
    tracing::info!("Executing cargo run (REST)");
    let req = ExecuteBashRequest {
        command: "cargo run".to_string(),
        cwd: None,
        timeout: None,
    };

    let cmd = state.bash.start_bash_command(req);
    tracing::info!("Started cargo run with ID: {}", cmd.id);

    let mut attempts = 0;
    loop {
        sleep(Duration::from_millis(100)).await;
        let page = state.bash.search_bash_events(Some(cmd.id));
        if let Some(last_item) = page.items.last() {
            if let BashEvent::BashOutput(out) = last_item {
                let mut result_str = String::new();
                if let Some(stdout) = &out.stdout {
                    result_str.push_str(stdout);
                }
                if let Some(stderr) = &out.stderr {
                    if !result_str.is_empty() {
                        result_str.push('\n');
                    }
                    result_str.push_str(stderr);
                }
                if let Some(exit_code) = out.exit_code {
                    if !result_str.is_empty() {
                        result_str.push('\n');
                    }
                    result_str
                        .push_str(&format!("[Command finished with exit code {}]", exit_code));
                }
                return (
                    StatusCode::OK,
                    Json(CommandOutput {
                        output: truncate_output(result_str, state.truncation_limit),
                    }),
                );
            }
        }

        attempts += 1;
        if attempts > 3000 {
            return (
                StatusCode::GATEWAY_TIMEOUT,
                Json(CommandOutput {
                    output: "Polling timed out".to_string(),
                }),
            );
        }
    }
}

pub async fn str_replace_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StrReplacePayload>,
) -> impl IntoResponse {
    let full_args = StrReplaceArgs {
        path: "src/main.rs".to_string(),
        old_str: payload.old_str,
        new_str: payload.new_str,
    };

    match run_str_replace(&full_args, &state.workspace_dir, &state.editor_history).await {
        Ok(output) => (
            StatusCode::OK,
            Json(CommandOutput {
                output: truncate_output(output, state.truncation_limit),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CommandOutput {
                output: format!("Error: {}", e.message),
            }),
        ),
    }
}

pub async fn view_file_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ViewFilePayload>,
) -> impl IntoResponse {
    let full_args = ViewFileArgs {
        path: payload.path,
        start_line: payload.start_line,
        end_line: payload.end_line,
    };

    match run_view_file(&full_args, &state.workspace_dir).await {
        Ok(output) => (
            StatusCode::OK,
            Json(CommandOutput {
                output: truncate_output(output, state.truncation_limit),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CommandOutput {
                output: format!("Error: {}", e.message),
            }),
        ),
    }
}
