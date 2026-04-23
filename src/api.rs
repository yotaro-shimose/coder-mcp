use crate::service::StrReplaceArgs;
use crate::tools::file_tools::{run_str_replace, run_view_file, ViewFileArgs};
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// REST handlers are intentionally stateless: each request spawns its own
/// `cargo run` subprocess and the file-editing handlers go through `std::fs`
/// directly. They do NOT share the PTY-backed `TerminalSession` that the
/// MCP `bash` tool uses, because a hung PTY session would otherwise stall
/// every subsequent `/run` request up to the 5-minute timeout. MCP side
/// (service.rs) still uses the stateful terminal for its `bash` tool.
const RUN_TIMEOUT: Duration = Duration::from_secs(270);

#[derive(Clone)]
pub struct AppState {
    pub workspace_dir: PathBuf,
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
pub struct SetContentPayload {
    pub path: String,
    pub content: String,
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
    pub exit_code: Option<i32>,
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

    let mut cmd = Command::new("cargo");
    cmd.arg("run")
        .current_dir(&state.workspace_dir)
        .kill_on_drop(true);

    let spawn_future = cmd.output();
    let result = match timeout(RUN_TIMEOUT, spawn_future).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CommandOutput {
                    output: format!("Failed to spawn `cargo run`: {e}"),
                    exit_code: None,
                }),
            );
        }
        Err(_) => {
            // kill_on_drop takes care of the child process when the future
            // returned by Command::output is dropped on timeout.
            let msg = format!(
                "[cargo run timed out after {}s]",
                RUN_TIMEOUT.as_secs()
            );
            return (
                StatusCode::OK,
                Json(CommandOutput {
                    output: msg,
                    exit_code: Some(-1),
                }),
            );
        }
    };

    let exit_code = result.status.code();
    let mut output = String::from_utf8_lossy(&result.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&result.stderr);
    if !stderr.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&stderr);
    }
    if let Some(code) = exit_code {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&format!("[Command finished with exit code {}]", code));
    }

    (
        StatusCode::OK,
        Json(CommandOutput {
            output: truncate_output(output, state.truncation_limit),
            exit_code,
        }),
    )
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

    match run_str_replace(&full_args, &state.workspace_dir).await {
        Ok(output) => (
            StatusCode::OK,
            Json(CommandOutput {
                output: truncate_output(output, state.truncation_limit),
                exit_code: Some(0),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CommandOutput {
                output: format!("Error: {}", e.message),
                exit_code: Some(1),
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
                exit_code: Some(0),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CommandOutput {
                output: format!("Error: {}", e.message),
                exit_code: Some(1),
            }),
        ),
    }
}

pub async fn set_content_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SetContentPayload>,
) -> impl IntoResponse {
    let path = state.workspace_dir.join(&payload.path);

    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
    }

    match tokio::fs::write(&path, &payload.content).await {
        Ok(_) => (
            StatusCode::OK,
            Json(CommandOutput {
                // Return truncated content as output, or just a success message
                output: truncate_output(payload.content, state.truncation_limit),
                exit_code: Some(0),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(CommandOutput {
                output: format!("Error: {}", e),
                exit_code: Some(1),
            }),
        ),
    }
}
