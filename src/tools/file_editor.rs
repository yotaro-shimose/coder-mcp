use rmcp::model::ErrorCode;
use rmcp::schemars;
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

const SNIPPET_CONTEXT_WINDOW: usize = 4;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct FileEditorArgs {
    pub command: String, // view, create, str_replace, insert, undo_edit
    pub path: String,
    pub file_text: Option<String>,
    pub view_range: Option<Vec<u64>>,
    pub old_str: Option<String>,
    pub new_str: Option<String>,
    pub insert_line: Option<u64>,
}

fn make_output(snippet_content: &str, snippet_description: &str, start_line: usize) -> String {
    let lines: Vec<&str> = snippet_content.lines().collect();
    let numbered_lines: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:6}\t{}", i + start_line, line))
        .collect();

    format!(
        "Here's the result of running `cat -n` on {}:\n{}\n",
        snippet_description,
        numbered_lines.join("\n")
    )
}

pub async fn run_file_editor(
    args: &FileEditorArgs,
    workspace_dir: &Path,
    editor_history: &Mutex<HashMap<PathBuf, Vec<String>>>,
) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    match args.command.as_str() {
        "view" => {
            if !path.exists() {
                return Ok(format!(
                    "Error: The path {} does not exist. Please provide a valid path.",
                    path.display()
                ));
            }
            if path.is_dir() {
                let mut formatted_paths = Vec::new();
                // Read dir up to depth 2 (simulated basic logic)
                // For now, simpler implementation than full recursive walk with exclude hidden
                // just to show intent.
                // Actually, let's just list 1 level for simplicity as walkdir is cleaner but maybe overkill if not already used.
                // But wait, we imported WalkDir in grep.rs, so we can use it if we add it to Cargo.toml or just use fs::read_dir.
                // Original impl uses `find ... -maxdepth 2`.
                // Let's stick to simple flat list for now to avoid complexity unless requested.
                // User said "If original tool also is not that much verbose it is okay".
                // But for directories, verbosity IS helpful.
                // Let's rely on standard message for now.

                // Using fs::read_dir
                match fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if !name.starts_with('.') {
                                if entry.path().is_dir() {
                                    formatted_paths.push(format!("{}/", name));
                                } else {
                                    formatted_paths.push(name);
                                }
                            }
                        }
                        formatted_paths.sort();
                        Ok(format!(
                            "Here's the files and directories in {}, excluding hidden items:\n{}",
                            path.display(),
                            formatted_paths.join("\n")
                        ))
                    }
                    Err(e) => Ok(format!(
                        "Error: Failed to list directory {}: {}",
                        path.display(),
                        e
                    )),
                }
            } else {
                match fs::read_to_string(&path) {
                    Ok(content) => {
                        let lines: Vec<&str> = content.lines().collect();
                        let num_lines = lines.len();
                        let (start_line, end_line) = if let Some(range) = &args.view_range {
                            if range.len() != 2 {
                                return Ok("Error: view_range should be a list of two integers."
                                    .to_string());
                            }
                            let s = range[0] as usize;
                            let e = range[1] as usize;
                            if s < 1 || s > num_lines {
                                return Ok(format!("Error: Its first element `{}` should be within the range of lines of the file: [1, {}].", s, num_lines));
                            }
                            if e < s {
                                return Ok(format!("Error: Its second element `{}` should be greater than or equal to the first element `{}`.", e, s));
                            }
                            (s, e)
                        } else {
                            (1, num_lines)
                        };

                        let end_line = std::cmp::min(end_line, num_lines);
                        let snippet_lines = lines
                            .iter()
                            .skip(start_line - 1)
                            .take(end_line - start_line + 1)
                            .cloned()
                            .collect::<Vec<&str>>()
                            .join("\n");

                        Ok(make_output(
                            &snippet_lines,
                            &path.to_string_lossy(),
                            start_line,
                        ))
                    }
                    Err(e) => Ok(format!(
                        "Error: Failed to read file {}: {}",
                        path.display(),
                        e
                    )),
                }
            }
        }
        "create" => {
            if path.exists() {
                return Ok(format!("Error: File already exists at: {}. Cannot overwrite files using command `create`. Use `str_replace` to edit the file instead.", path.display()));
            }
            let content = match args.file_text.clone() {
                Some(c) => c,
                None => {
                    return Ok("Error: Missing file_text parameter for create command.".to_string())
                }
            };
            // Create parent directories if they don't exist
            if let Some(parent) = path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Ok(format!(
                        "Error: Failed to create parent directories for {}: {}",
                        path.display(),
                        e
                    ));
                }
            }
            if let Err(e) = fs::write(&path, &content) {
                return Ok(format!(
                    "Error: Failed to write to {}: {}",
                    path.display(),
                    e
                ));
            }
            Ok(format!("File created successfully at: {}", path.display()))
        }
        "str_replace" => {
            if !path.exists() {
                return Ok(format!(
                    "Error: The path {} does not exist. Please check the file path.",
                    path.display()
                ));
            }
            let old_str = match args.old_str.clone() {
                Some(s) => s,
                None => {
                    return Ok(
                        "Error: Missing old_str parameter for str_replace command.".to_string()
                    )
                }
            };
            let new_str = match args.new_str.clone() {
                Some(s) => s,
                None => {
                    return Ok(
                        "Error: Missing new_str parameter for str_replace command.".to_string()
                    )
                }
            };

            if old_str == new_str {
                return Ok(
                    "Error: No replacement was performed. `new_str` and `old_str` must be different. Please provide different values.".to_string()
                );
            }

            let content = fs::read_to_string(&path).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to read file: {}", e).into(),
                data: None,
            })?;

            // Find occurrences logic
            let occurrences: Vec<_> = content.match_indices(&old_str).collect();

            if occurrences.is_empty() {
                return Ok(format!(
                    "Error: No replacement was performed, old_str `{}` did not appear verbatim in {}. Please check the file content and try again with the correct string.",
                    old_str,
                    path.display()
                ));
            }
            if occurrences.len() > 1 {
                let line_numbers: Vec<usize> = occurrences
                    .iter()
                    .map(|(idx, _)| content[..*idx].chars().filter(|&c| c == '\n').count() + 1)
                    .collect();
                return Ok(format!("Error: No replacement was performed. Multiple occurrences of old_str `{}` in lines {:?}. Please provide more context to make the match unique.", old_str, line_numbers));
            }

            let (idx, matched_text) = occurrences[0];
            let replacement_line = content[..idx].chars().filter(|&c| c == '\n').count() + 1;

            let new_content = format!(
                "{}{}{}",
                &content[..idx],
                new_str,
                &content[idx + matched_text.len()..]
            );

            // Save history
            {
                let mut history = editor_history.lock().await;
                history
                    .entry(path.clone())
                    .or_default()
                    .push(content.clone());
            }

            fs::write(&path, &new_content).map_err(|e| McpError {
                code: ErrorCode(-32603),
                message: format!("Failed to write file: {}", e).into(),
                data: None,
            })?;

            // Create snippet
            // Create snippet
            let start_line = replacement_line.saturating_sub(SNIPPET_CONTEXT_WINDOW);
            let end_line =
                replacement_line + SNIPPET_CONTEXT_WINDOW + new_str.matches('\n').count();

            let lines: Vec<&str> = new_content.lines().collect();
            // Adjust for make_output
            let snippet_display_start_line = start_line + 1; // if 0 -> 1

            // Slicing logic on vector:
            let s_idx = start_line; // 0-based index to start reading from
            let output_snippet = lines
                .iter()
                .skip(s_idx)
                .take(end_line - s_idx)
                .cloned()
                .collect::<Vec<&str>>()
                .join("\n");

            Ok(format!(
                "The file {} has been edited. {}Review the changes and make sure they are as expected. Edit the file again if necessary.",
                 path.display(),
                 make_output(&output_snippet, &format!("a snippet of {}", path.display()), snippet_display_start_line)
            ))
        }
        "insert" => {
            let insert_line = match args.insert_line {
                Some(l) => l,
                None => return Ok("Error: Missing insert_line parameter for insert command.".to_string()),
            };
            let text_to_insert = match args.new_str.clone().or(args.file_text.clone()) {
                Some(t) => t,
                None => return Ok("Error: Missing new_str (or file_text) for insert command.".to_string()),
            };

            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => return Ok(format!("Error: Failed to read file {}: {}", path.display(), e)),
            };

            // Save history
            {
                let mut history = editor_history.lock().await;
                history
                    .entry(path.clone())
                    .or_default()
                    .push(content.clone());
            }

            let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let idx = (insert_line as usize).saturating_sub(1);

            if idx > lines.len() {
                return Ok(format!("Error: insert_line {} should be within the range of allowed values: [0, {}]", insert_line, lines.len()));
            }

            let inserted_lines_count = text_to_insert.lines().count();

            if idx == lines.len() {
                lines.push(text_to_insert.clone());
            } else {
                lines.insert(idx, text_to_insert.clone());
            }

            let new_content = lines.join("\n");
            if let Err(e) = fs::write(&path, &new_content) {
                return Ok(format!("Error: Failed to write file {}: {}", path.display(), e));
            }

            // Snippet
            let start_line = (insert_line as usize).saturating_sub(SNIPPET_CONTEXT_WINDOW);
            let end_line = insert_line as usize + SNIPPET_CONTEXT_WINDOW + inserted_lines_count;

            let new_lines: Vec<&str> = new_content.lines().collect();
            let output_snippet = new_lines
                .iter()
                .skip(start_line)
                .take(end_line - start_line)
                .cloned()
                .collect::<Vec<&str>>()
                .join("\n");

            Ok(format!(
                "The file {} has been edited. {}Review the changes and make sure they are as expected (correct indentation, no duplicate lines, etc). Edit the file again if necessary.",
                path.display(),
                make_output(&output_snippet, "a snippet of the edited file", start_line + 1)
            ))
        }
        "undo_edit" => {
            let mut history = editor_history.lock().await;
            if let Some(versions) = history.get_mut(&path) {
                if let Some(prev_content) = versions.pop() {
                    if let Err(e) = fs::write(&path, &prev_content) {
                        return Ok(format!("Error: Failed to restore file {}: {}", path.display(), e));
                    }
                    return Ok(format!(
                        "Last edit to {} undone successfully. {}",
                        path.display(),
                        make_output(&prev_content, &path.to_string_lossy(), 1)
                    ));
                }
            }
            Ok(format!("Error: No edit history found for {}", path.display()))
        }
        _ => Ok(format!("Error: Unrecognized command '{}'. Use view, create, str_replace, insert, or undo_edit.", args.command)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_file_editor_create_and_view() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());

        let args_create = FileEditorArgs {
            command: "create".to_string(),
            path: "test.txt".to_string(),
            file_text: Some("hello world".to_string()),
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };

        let result = run_file_editor(&args_create, dir.path(), &history)
            .await
            .unwrap();
        assert!(result.contains("created successfully"));

        let args_view = FileEditorArgs {
            command: "view".to_string(),
            path: "test.txt".to_string(),
            file_text: None,
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };

        let content = run_file_editor(&args_view, dir.path(), &history)
            .await
            .unwrap();
        assert!(content.contains("hello world"));
        assert!(content.contains("cat -n"));
    }

    #[tokio::test]
    async fn test_file_editor_replace_and_undo() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        // Replace
        let args_replace = FileEditorArgs {
            command: "str_replace".to_string(),
            path: "test.txt".to_string(),
            old_str: Some("world".to_string()),
            new_str: Some("rust".to_string()),
            file_text: None,
            view_range: None,
            insert_line: None,
        };

        let res = run_file_editor(&args_replace, dir.path(), &history)
            .await
            .unwrap();
        assert!(res.contains("edited"));
        // {:6} padding for "1" gives "     1"
        assert!(res.contains("     1\thello rust"));

        // Verify content
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello rust");

        // Undo
        let args_undo = FileEditorArgs {
            command: "undo_edit".to_string(),
            path: "test.txt".to_string(),
            file_text: None,
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };

        let undo_res = run_file_editor(&args_undo, dir.path(), &history)
            .await
            .unwrap();
        assert!(undo_res.contains("undone successfully"));

        // Verify undo
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello world");
    }

    // Error handling tests - verify errors return Ok with error message

    #[tokio::test]
    async fn test_view_file_not_found_returns_ok() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let args = FileEditorArgs {
            command: "view".to_string(),
            path: "nonexistent.txt".to_string(),
            file_text: None,
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };
        let result = run_file_editor(&args, dir.path(), &history).await.unwrap();
        assert!(result.contains("Error:"));
        assert!(result.contains("does not exist"));
    }

    #[tokio::test]
    async fn test_create_file_exists_returns_ok() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        fs::write(dir.path().join("test.txt"), "existing content").unwrap();

        let args = FileEditorArgs {
            command: "create".to_string(),
            path: "test.txt".to_string(),
            file_text: Some("new content".to_string()),
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };
        let result = run_file_editor(&args, dir.path(), &history).await.unwrap();
        assert!(result.contains("Error:"));
        assert!(result.contains("already exists"));
    }

    #[tokio::test]
    async fn test_str_replace_not_found_returns_ok() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        let args = FileEditorArgs {
            command: "str_replace".to_string(),
            path: "test.txt".to_string(),
            old_str: Some("nonexistent".to_string()),
            new_str: Some("replacement".to_string()),
            file_text: None,
            view_range: None,
            insert_line: None,
        };
        let result = run_file_editor(&args, dir.path(), &history).await.unwrap();
        assert!(result.contains("Error:"));
        assert!(result.contains("did not appear verbatim"));
    }

    #[tokio::test]
    async fn test_str_replace_multiple_occurrences_returns_ok() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        fs::write(dir.path().join("test.txt"), "hello hello hello").unwrap();

        let args = FileEditorArgs {
            command: "str_replace".to_string(),
            path: "test.txt".to_string(),
            old_str: Some("hello".to_string()),
            new_str: Some("world".to_string()),
            file_text: None,
            view_range: None,
            insert_line: None,
        };
        let result = run_file_editor(&args, dir.path(), &history).await.unwrap();
        assert!(result.contains("Error:"));
        assert!(result.contains("Multiple occurrences"));
    }

    #[tokio::test]
    async fn test_file_editor_unknown_command_returns_ok() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let args = FileEditorArgs {
            command: "unknown".to_string(),
            path: "test.txt".to_string(),
            file_text: None,
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };
        let result = run_file_editor(&args, dir.path(), &history).await.unwrap();
        assert!(result.contains("Error: Unrecognized command"));
    }

    #[tokio::test]
    async fn test_file_editor_missing_parameters_returns_ok() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());

        // Missing file_text for create
        let args_create = FileEditorArgs {
            command: "create".to_string(),
            path: "test.txt".to_string(),
            file_text: None,
            view_range: None,
            old_str: None,
            new_str: None,
            insert_line: None,
        };
        let res_create = run_file_editor(&args_create, dir.path(), &history)
            .await
            .unwrap();
        assert!(res_create.contains("Error: Missing file_text"));

        // Create the file first so we can test other commands
        fs::write(dir.path().join("test.txt"), "hello world").unwrap();

        // Missing old_str for str_replace
        let args_replace = FileEditorArgs {
            command: "str_replace".to_string(),
            path: "test.txt".to_string(),
            old_str: None,
            new_str: Some("new".to_string()),
            file_text: None,
            view_range: None,
            insert_line: None,
        };
        let res_replace = run_file_editor(&args_replace, dir.path(), &history)
            .await
            .unwrap();
        assert!(res_replace.contains("Error: Missing old_str"));

        // Missing insert_line for insert
        let args_insert = FileEditorArgs {
            command: "insert".to_string(),
            path: "test.txt".to_string(),
            insert_line: None,
            new_str: Some("new".to_string()),
            file_text: None,
            view_range: None,
            old_str: None,
        };
        let res_insert = run_file_editor(&args_insert, dir.path(), &history)
            .await
            .unwrap();
        assert!(res_insert.contains("Error: Missing insert_line"));
    }
}
