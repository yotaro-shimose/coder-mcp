use rmcp::ErrorData as McpError;
use rmcp::model::ErrorCode;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

use crate::tools::utils;

// Re-export argument types from service
pub use crate::service::{
    CreateFileArgs, DeleteFileArgs, InsertLinesArgs, ListDirectoryArgs, StrReplaceArgs,
    TreeArgs, UndoEditArgs, ViewFileArgs,
};

const SNIPPET_CONTEXT_WINDOW: usize = 4;

fn make_output(snippet_content: &str, _snippet_description: &str, start_line: usize) -> String {
    utils::make_numbered_output(snippet_content, start_line)
}

pub async fn run_view_file(args: &ViewFileArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    if !path.exists() {
        return Ok(format!(
            "Error: The path {} does not exist. Please provide a valid path.",
            path.display()
        ));
    }

    match fs::read_to_string(&path) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let num_lines = lines.len();

            let (start_line, end_line) = match (args.start_line, args.end_line) {
                (Some(s), Some(e)) => {
                    let s = s as usize;
                    let e = e as usize;
                    if s < 1 || s > num_lines {
                        return Ok(format!(
                            "Error: start_line {} should be within the range [1, {}].",
                            s, num_lines
                        ));
                    }
                    if e < s {
                        return Ok(format!(
                            "Error: end_line {} should be greater than or equal to start_line {}.",
                            e, s
                        ));
                    }
                    (s, e)
                }
                (Some(s), None) => {
                    let s = s as usize;
                    if s < 1 || s > num_lines {
                        return Ok(format!(
                            "Error: start_line {} should be within the range [1, {}].",
                            s, num_lines
                        ));
                    }
                    (s, num_lines)
                }
                (None, Some(e)) => {
                    let e = e as usize;
                    (1, e)
                }
                (None, None) => (1, num_lines),
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

pub async fn run_list_directory(
    args: &ListDirectoryArgs,
    workspace_dir: &Path,
) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    if !path.exists() {
        return Ok(format!(
            "Error: The path {} does not exist. Please provide a valid path.",
            path.display()
        ));
    }

    if !path.is_dir() {
        return Ok(format!(
            "Error: The path {} is not a directory.",
            path.display()
        ));
    }

    match fs::read_dir(&path) {
        Ok(entries) => {
            let mut formatted_paths = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') {
                    if entry.path().is_dir() {
                        formatted_paths.push(format!("{}/", name));
                    } else {
                        let line_count_str = if let Ok(content) = fs::read_to_string(entry.path()) {
                            let count = content.lines().count();
                            format!(" ({} line{})", count, if count == 1 { "" } else { "s" })
                        } else {
                            "".to_string()
                        };
                        formatted_paths.push(format!("{}{}", name, line_count_str));
                    }
                }
            }
            formatted_paths.sort();
            Ok(formatted_paths.join("\n"))
        }
        Err(e) => Ok(format!(
            "Error: Failed to list directory {}: {}",
            path.display(),
            e
        )),
    }
}

pub async fn run_create_file(
    args: &CreateFileArgs,
    workspace_dir: &Path,
) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    if path.exists() {
        return Ok(format!(
            "Error: File already exists at: {}. Cannot overwrite files using create_file.",
            path.display()
        ));
    }

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

    if let Err(e) = fs::write(&path, &args.content) {
        return Ok(format!(
            "Error: Failed to write to {}: {}",
            path.display(),
            e
        ));
    }

    Ok(format!("File created successfully at: {}", path.display()))
}

pub async fn run_str_replace(
    args: &StrReplaceArgs,
    workspace_dir: &Path,
    editor_history: &Mutex<HashMap<PathBuf, Vec<String>>>,
) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    if !path.exists() {
        return Ok(format!(
            "Error: The path {} does not exist. Please check the file path.",
            path.display()
        ));
    }

    if args.old_str == args.new_str {
        return Ok(
            "Error: No replacement was performed. new_str and old_str must be different."
                .to_string(),
        );
    }

    let content = fs::read_to_string(&path).map_err(|e| McpError {
        code: ErrorCode(-32603),
        message: format!("Failed to read file: {}", e).into(),
        data: None,
    })?;

    // Find occurrences logic
    let occurrences: Vec<_> = content.match_indices(&args.old_str).collect();

    if occurrences.is_empty() {
        return Ok(format!(
            "Error: No replacement was performed, old_str `{}` did not appear verbatim in {}.",
            args.old_str,
            path.display()
        ));
    }
    if occurrences.len() > 1 {
        let line_numbers: Vec<usize> = occurrences
            .iter()
            .map(|(idx, _)| content[..*idx].chars().filter(|&c| c == '\n').count() + 1)
            .collect();
        return Ok(format!(
            "Error: No replacement was performed. Multiple occurrences of old_str `{}` in lines {:?}. Please provide more context to make the match unique.",
            args.old_str, line_numbers
        ));
    }

    let (idx, matched_text) = occurrences[0];
    let replacement_line = content[..idx].chars().filter(|&c| c == '\n').count() + 1;

    let new_content = format!(
        "{}{}{}",
        &content[..idx],
        args.new_str,
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
    let start_line = replacement_line.saturating_sub(SNIPPET_CONTEXT_WINDOW);
    let end_line = replacement_line + SNIPPET_CONTEXT_WINDOW + args.new_str.matches('\n').count();

    let lines: Vec<&str> = new_content.lines().collect();
    let snippet_display_start_line = start_line + 1;

    let s_idx = start_line;
    let output_snippet = lines
        .iter()
        .skip(s_idx)
        .take(end_line - s_idx)
        .cloned()
        .collect::<Vec<&str>>()
        .join("\n");

    Ok(format!(
        "The file {} has been edited. {}Review the changes and make sure they are as expected.",
        path.display(),
        make_output(
            &output_snippet,
            &format!("a snippet of {}", path.display()),
            snippet_display_start_line
        )
    ))
}

pub async fn run_insert_lines(
    args: &InsertLinesArgs,
    workspace_dir: &Path,
    editor_history: &Mutex<HashMap<PathBuf, Vec<String>>>,
) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    if !path.exists() {
        return Ok(format!(
            "Error: The path {} does not exist.",
            path.display()
        ));
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            return Ok(format!(
                "Error: Failed to read file {}: {}",
                path.display(),
                e
            ));
        }
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
    let idx = (args.insert_line as usize).saturating_sub(1);

    if idx > lines.len() {
        return Ok(format!(
            "Error: insert_line {} should be within the range [0, {}]",
            args.insert_line,
            lines.len()
        ));
    }

    let inserted_lines_count = args.content.lines().count();

    if idx == lines.len() {
        lines.push(args.content.clone());
    } else {
        lines.insert(idx, args.content.clone());
    }

    let new_content = lines.join("\n");
    if let Err(e) = fs::write(&path, &new_content) {
        return Ok(format!(
            "Error: Failed to write file {}: {}",
            path.display(),
            e
        ));
    }

    // Snippet
    let start_line = (args.insert_line as usize).saturating_sub(SNIPPET_CONTEXT_WINDOW);
    let end_line = args.insert_line as usize + SNIPPET_CONTEXT_WINDOW + inserted_lines_count;

    let new_lines: Vec<&str> = new_content.lines().collect();
    let output_snippet = new_lines
        .iter()
        .skip(start_line)
        .take(end_line - start_line)
        .cloned()
        .collect::<Vec<&str>>()
        .join("\n");

    Ok(format!(
        "The file {} has been edited. {}Review the changes and make sure they are as expected.",
        path.display(),
        make_output(
            &output_snippet,
            "a snippet of the edited file",
            start_line + 1
        )
    ))
}

pub async fn run_delete_file(
    args: &DeleteFileArgs,
    workspace_dir: &Path,
) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    if !path.exists() {
        return Ok(format!(
            "Error: The path {} does not exist.",
            path.display()
        ));
    }

    if let Err(e) = fs::remove_file(&path) {
        return Ok(format!(
            "Error: Failed to delete file {}: {}",
            path.display(),
            e
        ));
    }

    Ok(format!("File deleted successfully: {}", path.display()))
}

pub async fn run_undo_edit(
    args: &UndoEditArgs,
    workspace_dir: &Path,
    editor_history: &Mutex<HashMap<PathBuf, Vec<String>>>,
) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    let mut history = editor_history.lock().await;
    if let Some(versions) = history.get_mut(&path) {
        if let Some(prev_content) = versions.pop() {
            if let Err(e) = fs::write(&path, &prev_content) {
                return Ok(format!(
                    "Error: Failed to restore file {}: {}",
                    path.display(),
                    e
                ));
            }
            return Ok(format!(
                "Last edit to {} undone successfully. {}",
                path.display(),
                make_output(&prev_content, &path.to_string_lossy(), 1)
            ));
        }
    }
    Ok(format!(
        "Error: No edit history found for {}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ========== str_replace tests ==========

    #[tokio::test]
    async fn test_str_replace_basic() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        let args = StrReplaceArgs {
            path: "test.txt".to_string(),
            old_str: "world".to_string(),
            new_str: "rust".to_string(),
        };

        let result = run_str_replace(&args, dir.path(), &history).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello rust");
    }

    #[tokio::test]
    async fn test_str_replace_not_found() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        let args = StrReplaceArgs {
            path: "test.txt".to_string(),
            old_str: "nonexistent".to_string(),
            new_str: "replacement".to_string(),
        };

        let result = run_str_replace(&args, dir.path(), &history).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("did not appear verbatim"));
    }

    #[tokio::test]
    async fn test_str_replace_multiple_occurrences() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello hello hello").unwrap();

        let args = StrReplaceArgs {
            path: "test.txt".to_string(),
            old_str: "hello".to_string(),
            new_str: "world".to_string(),
        };

        let result = run_str_replace(&args, dir.path(), &history).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("Multiple occurrences"));
    }

    #[tokio::test]
    async fn test_str_replace_same_string() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        let args = StrReplaceArgs {
            path: "test.txt".to_string(),
            old_str: "world".to_string(),
            new_str: "world".to_string(),
        };

        let result = run_str_replace(&args, dir.path(), &history).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("must be different"));
    }

    #[tokio::test]
    async fn test_str_replace_file_not_found() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());

        let args = StrReplaceArgs {
            path: "nonexistent.txt".to_string(),
            old_str: "old".to_string(),
            new_str: "new".to_string(),
        };

        let result = run_str_replace(&args, dir.path(), &history).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("does not exist"));
    }

    #[tokio::test]
    async fn test_str_replace_multiline() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").unwrap();

        let args = StrReplaceArgs {
            path: "test.txt".to_string(),
            old_str: "line2".to_string(),
            new_str: "modified".to_string(),
        };

        let result = run_str_replace(&args, dir.path(), &history).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\nmodified\nline3");
    }

    // ========== view_file tests ==========

    #[tokio::test]
    async fn test_view_file_basic() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3\nline4\nline5").unwrap();

        let args = ViewFileArgs {
            path: "test.txt".to_string(),
            start_line: None,
            end_line: None,
        };

        let result = run_view_file(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("line1"));
        assert!(output.contains("line5"));
    }

    #[tokio::test]
    async fn test_view_file_with_range() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3\nline4\nline5").unwrap();

        let args = ViewFileArgs {
            path: "test.txt".to_string(),
            start_line: Some(2),
            end_line: Some(4),
        };

        let result = run_view_file(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.contains("line1"));
        assert!(output.contains("line2"));
        assert!(output.contains("line4"));
        assert!(!output.contains("line5"));
    }

    #[tokio::test]
    async fn test_view_file_invalid_start_line() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").unwrap();

        let args = ViewFileArgs {
            path: "test.txt".to_string(),
            start_line: Some(10),
            end_line: None,
        };

        let result = run_view_file(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("start_line"));
    }

    #[tokio::test]
    async fn test_view_file_invalid_range() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").unwrap();

        let args = ViewFileArgs {
            path: "test.txt".to_string(),
            start_line: Some(3),
            end_line: Some(1),
        };

        let result = run_view_file(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("greater than or equal to"));
    }

    #[tokio::test]
    async fn test_view_file_not_found() {
        let dir = tempdir().unwrap();

        let args = ViewFileArgs {
            path: "nonexistent.txt".to_string(),
            start_line: None,
            end_line: None,
        };

        let result = run_view_file(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("does not exist"));
    }

    // ========== create_file tests ==========

    #[tokio::test]
    async fn test_create_file_basic() {
        let dir = tempdir().unwrap();

        let args = CreateFileArgs {
            path: "new_file.txt".to_string(),
            content: "hello world".to_string(),
        };

        let result = run_create_file(&args, dir.path()).await;
        assert!(result.is_ok());

        let file_path = dir.path().join("new_file.txt");
        assert!(file_path.exists());
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_create_file_already_exists() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("existing.txt");
        fs::write(&file_path, "existing content").unwrap();

        let args = CreateFileArgs {
            path: "existing.txt".to_string(),
            content: "new content".to_string(),
        };

        let result = run_create_file(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("already exists"));
    }

    #[tokio::test]
    async fn test_create_file_with_parent_dirs() {
        let dir = tempdir().unwrap();

        let args = CreateFileArgs {
            path: "subdir/nested/file.txt".to_string(),
            content: "nested content".to_string(),
        };

        let result = run_create_file(&args, dir.path()).await;
        assert!(result.is_ok());

        let file_path = dir.path().join("subdir/nested/file.txt");
        assert!(file_path.exists());
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "nested content");
    }

    #[tokio::test]
    async fn test_create_file_empty_content() {
        let dir = tempdir().unwrap();

        let args = CreateFileArgs {
            path: "empty.txt".to_string(),
            content: "".to_string(),
        };

        let result = run_create_file(&args, dir.path()).await;
        assert!(result.is_ok());

        let file_path = dir.path().join("empty.txt");
        assert!(file_path.exists());
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "");
    }

    // ========== insert_lines tests ==========

    #[tokio::test]
    async fn test_insert_lines_basic() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3").unwrap();

        let args = InsertLinesArgs {
            path: "test.txt".to_string(),
            insert_line: 2,
            content: "inserted".to_string(),
        };

        let result = run_insert_lines(&args, dir.path(), &history).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&file_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "inserted");
        assert_eq!(lines[2], "line2");
    }

    #[tokio::test]
    async fn test_insert_lines_at_beginning() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2").unwrap();

        let args = InsertLinesArgs {
            path: "test.txt".to_string(),
            insert_line: 1,
            content: "first".to_string(),
        };

        let result = run_insert_lines(&args, dir.path(), &history).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&file_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[0], "first");
        assert_eq!(lines[1], "line1");
    }

    #[tokio::test]
    async fn test_insert_lines_at_end() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2").unwrap();

        let args = InsertLinesArgs {
            path: "test.txt".to_string(),
            insert_line: 3,
            content: "last".to_string(),
        };

        let result = run_insert_lines(&args, dir.path(), &history).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&file_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines[2], "last");
    }

    #[tokio::test]
    async fn test_insert_lines_invalid_line() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2").unwrap();

        let args = InsertLinesArgs {
            path: "test.txt".to_string(),
            insert_line: 100,
            content: "invalid".to_string(),
        };

        let result = run_insert_lines(&args, dir.path(), &history).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("insert_line"));
    }

    #[tokio::test]
    async fn test_insert_lines_file_not_found() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());

        let args = InsertLinesArgs {
            path: "nonexistent.txt".to_string(),
            insert_line: 1,
            content: "content".to_string(),
        };

        let result = run_insert_lines(&args, dir.path(), &history).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("does not exist"));
    }

    // ========== delete_file tests ==========

    #[tokio::test]
    async fn test_delete_file_basic() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("to_delete.txt");
        fs::write(&file_path, "content").unwrap();
        assert!(file_path.exists());

        let args = DeleteFileArgs {
            path: "to_delete.txt".to_string(),
        };

        let result = run_delete_file(&args, dir.path()).await;
        assert!(result.is_ok());
        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn test_delete_file_not_found() {
        let dir = tempdir().unwrap();

        let args = DeleteFileArgs {
            path: "nonexistent.txt".to_string(),
        };

        let result = run_delete_file(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("does not exist"));
    }

    // ========== undo_edit tests ==========

    #[tokio::test]
    async fn test_undo_edit_after_str_replace() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        // First, do a str_replace
        let replace_args = StrReplaceArgs {
            path: "test.txt".to_string(),
            old_str: "world".to_string(),
            new_str: "rust".to_string(),
        };
        run_str_replace(&replace_args, dir.path(), &history)
            .await
            .unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello rust");

        // Now undo
        let undo_args = UndoEditArgs {
            path: "test.txt".to_string(),
        };
        let result = run_undo_edit(&undo_args, dir.path(), &history).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_undo_edit_after_insert_lines() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2").unwrap();

        // Insert a line
        let insert_args = InsertLinesArgs {
            path: "test.txt".to_string(),
            insert_line: 2,
            content: "inserted".to_string(),
        };
        run_insert_lines(&insert_args, dir.path(), &history)
            .await
            .unwrap();

        // Undo
        let undo_args = UndoEditArgs {
            path: "test.txt".to_string(),
        };
        let result = run_undo_edit(&undo_args, dir.path(), &history).await;
        assert!(result.is_ok());

        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\nline2");
    }

    #[tokio::test]
    async fn test_undo_edit_no_history() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "content").unwrap();

        let undo_args = UndoEditArgs {
            path: "test.txt".to_string(),
        };
        let result = run_undo_edit(&undo_args, dir.path(), &history).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("No edit history"));
    }

    #[tokio::test]
    async fn test_undo_edit_multiple_times() {
        let dir = tempdir().unwrap();
        let history = Mutex::new(HashMap::new());
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "original").unwrap();

        // First edit
        let replace_args1 = StrReplaceArgs {
            path: "test.txt".to_string(),
            old_str: "original".to_string(),
            new_str: "edit1".to_string(),
        };
        run_str_replace(&replace_args1, dir.path(), &history)
            .await
            .unwrap();

        // Second edit
        let replace_args2 = StrReplaceArgs {
            path: "test.txt".to_string(),
            old_str: "edit1".to_string(),
            new_str: "edit2".to_string(),
        };
        run_str_replace(&replace_args2, dir.path(), &history)
            .await
            .unwrap();

        // Undo once
        let undo_args = UndoEditArgs {
            path: "test.txt".to_string(),
        };
        run_undo_edit(&undo_args, dir.path(), &history)
            .await
            .unwrap();
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "edit1");

        // Undo again
        run_undo_edit(&undo_args, dir.path(), &history)
            .await
            .unwrap();
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "original");
    }

    // ========== list_directory tests ==========

    #[tokio::test]
    async fn test_list_directory_basic() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("file1.txt"), "content1").unwrap();
        fs::write(dir.path().join("file2.txt"), "content2").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();

        let args = ListDirectoryArgs {
            path: ".".to_string(),
        };

        let result = run_list_directory(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("file1.txt"));
        assert!(output.contains("file2.txt"));
        assert!(output.contains("subdir/"));
    }

    #[tokio::test]
    async fn test_list_directory_empty() {
        let dir = tempdir().unwrap();

        let args = ListDirectoryArgs {
            path: ".".to_string(),
        };

        let result = run_list_directory(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.is_empty());
    }

    #[tokio::test]
    async fn test_list_directory_hidden_files_excluded() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("visible.txt"), "content").unwrap();
        fs::write(dir.path().join(".hidden"), "secret").unwrap();

        let args = ListDirectoryArgs {
            path: ".".to_string(),
        };

        let result = run_list_directory(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("visible.txt"));
        assert!(!output.contains(".hidden"));
    }

    #[tokio::test]
    async fn test_list_directory_not_found() {
        let dir = tempdir().unwrap();

        let args = ListDirectoryArgs {
            path: "nonexistent".to_string(),
        };

        let result = run_list_directory(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("does not exist"));
    }

    #[tokio::test]
    async fn test_list_directory_on_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("file.txt"), "content").unwrap();

        let args = ListDirectoryArgs {
            path: "file.txt".to_string(),
        };

        let result = run_list_directory(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("not a directory"));
    }

    #[tokio::test]
    async fn test_list_directory_with_line_counts() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("file1.txt"), "line1\nline2\nline3").unwrap();
        fs::write(dir.path().join("file2.txt"), "hello").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();

        let args = ListDirectoryArgs {
            path: ".".to_string(),
        };

        let result = run_list_directory(&args, dir.path()).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("file1.txt (3 lines)"));
        assert!(output.contains("file2.txt (1 line)"));
        assert!(output.contains("subdir/"));
    }
}

pub fn run_tree(
    args: &TreeArgs,
    workspace_dir: &Path,
) -> Result<String, McpError> {
    let rel_path = args.path.as_deref().unwrap_or(".");
    let root_path = workspace_dir.join(rel_path);

    if !root_path.exists() {
        return Err(McpError {
            code: ErrorCode(-32602),
            message: format!("Path does not exist: {}", root_path.display()).into(),
            data: None,
        });
    }

    let max_depth = args.max_depth.unwrap_or(usize::MAX);
    let truncate = args.truncate.unwrap_or(10);
    
    let exclude_vec: Vec<String> = args.exclude.as_deref().unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mut output = String::new();
    // Add root
    output.push_str(&format!("{}\n", rel_path));

    visit_dirs(
        &root_path,
        &mut output,
        "",
        0,
        max_depth,
        truncate,
        &exclude_vec,
    )?;

    Ok(output)
}

fn visit_dirs(
    dir: &Path,
    output: &mut String,
    prefix: &str,
    current_depth: usize,
    max_depth: usize,
    truncate: usize,
    exclude: &[String],
) -> Result<(), McpError> {
    if current_depth >= max_depth {
        return Ok(());
    }

    let entries = fs::read_dir(dir).map_err(|e| McpError {
        code: ErrorCode(-32603),
        message: format!("Failed to read directory: {}", e).into(),
        data: None,
    })?;

    let mut entries_vec = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| McpError {
            code: ErrorCode(-32603),
            message: format!("Failed to read entry: {}", e).into(),
            data: None,
        })?;
        let name = entry.file_name().to_string_lossy().to_string();

        // Filter excludes and hidden files
        // Note: exclude matches exact name here.
        if !name.starts_with('.') && !exclude.contains(&name) {
            entries_vec.push((name, entry.path()));
        }
    }

    entries_vec.sort_by(|a, b| a.0.cmp(&b.0));

    let total_count = entries_vec.len();
    let mut display_entries = entries_vec;
    let mut remaining = 0;

    if total_count > truncate {
        remaining = total_count - truncate;
        display_entries.truncate(truncate);
    }

    for (i, (name, path)) in display_entries.iter().enumerate() {
        let is_last_entry = i == display_entries.len() - 1;
        let show_more = is_last_entry && remaining > 0;

        // connector depends on whether this is logically the last thing printed.
        // If we show more, this is NOT the last thing strings-wise.
        let connector = if !show_more && is_last_entry {
            "└── "
        } else {
            "├── "
        };

        output.push_str(&format!("{}{}{}\n", prefix, connector, name));

        if path.is_dir() {
            let new_prefix = if !show_more && is_last_entry {
                format!("{}    ", prefix)
            } else {
                format!("{}│   ", prefix)
            };
            visit_dirs(
                path,
                output,
                &new_prefix,
                current_depth + 1,
                max_depth,
                truncate,
                exclude,
            )?;
        }

        if show_more {
            output.push_str(&format!("{}{}... ({} more)\n", prefix, "└── ", remaining));
        }
    }

    Ok(())
}
