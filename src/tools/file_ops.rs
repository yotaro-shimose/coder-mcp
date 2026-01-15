use rmcp::schemars;
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ReadFileArgs {
    pub path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct WriteFileArgs {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ListFilesArgs {
    pub path: String,
    pub recursive: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DeleteFileArgs {
    pub path: String,
}

const MAX_LINES_PER_READ: usize = 1000;

fn make_numbered_output(content: &str, start_line: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let numbered_lines: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:6}\t{}", i + start_line, line))
        .collect();
    numbered_lines.join("\n")
}

pub fn run_read_file(args: &ReadFileArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);
    if !path.exists() {
        return Ok(format!(
            "Error: File not found: {}. Please check the path and try again.",
            path.display()
        ));
    }
    if path.is_dir() {
        return Ok(format!(
            "Error: Path is a directory, not a file: {}. Use list_files instead.",
            path.display()
        ));
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => return Ok(format!("Error reading file {}: {}", path.display(), e)),
    };

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let offset = args.offset.unwrap_or(0);

    if offset >= total_lines && total_lines > 0 {
        return Ok(format!(
            "Error: Offset {} is beyond file length ({} lines). Use a smaller offset.",
            offset, total_lines
        ));
    }

    let limit = args.limit.unwrap_or(MAX_LINES_PER_READ);
    let end = std::cmp::min(offset + limit, total_lines);

    let lines_to_show = &lines[offset..end];
    let content_to_show = lines_to_show.join("\n");
    let numbered_content = make_numbered_output(&content_to_show, offset + 1);

    let is_truncated = end < total_lines;
    let mut header = format!("Read file: {}", path.display());
    if is_truncated {
        header.push_str(&format!(
            " (showing lines {}-{} of {})",
            offset + 1,
            end,
            total_lines
        ));
        header.push_str(&format!(
            "\nTo read more, use: read_file(path='{}', offset={}, limit={})",
            args.path, end, limit
        ));
    }

    Ok(format!("{}\n\n{}", header, numbered_content))
}

pub fn run_write_file(args: &WriteFileArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);

    if path.exists() && path.is_dir() {
        return Ok(format!(
            "Error: Path is a directory, not a file: {}. Cannot write to a directory.",
            path.display()
        ));
    }

    let is_new_file = !path.exists();

    // Create parent dirs
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return Ok(format!(
                "Error creating parent directory for {}: {}",
                path.display(),
                e
            ));
        }
    }

    if let Err(e) = fs::write(&path, &args.content) {
        return Ok(format!("Error writing file {}: {}", path.display(), e));
    }

    let action_verb = if is_new_file { "Created" } else { "Updated" };
    Ok(format!("{} file: {}", action_verb, path.display()))
}

pub fn run_list_files(args: &ListFilesArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);
    if !path.exists() {
        return Ok(format!(
            "Error: Directory not found: {}. Please check the path.",
            path.display()
        ));
    }
    if !path.is_dir() {
        return Ok(format!(
            "Error: Path is not a directory: {}. Use read_file for files.",
            path.display()
        ));
    }

    let mut entries = Vec::new();
    let recursive = args.recursive.unwrap_or(false);

    if recursive {
        // Simple recursive implementation (limit to 2 levels deep like Python)
        for entry in walkdir::WalkDir::new(&path)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let rel_path = entry.path().strip_prefix(&path).unwrap_or(entry.path());
            if rel_path.as_os_str().is_empty() {
                continue;
            }

            let name = rel_path.to_string_lossy().to_string();
            let type_str = if entry.file_type().is_dir() {
                "dir"
            } else {
                "file"
            };
            entries.push(format!("{} ({})", name, type_str));
            if entries.len() >= 1000 {
                break;
            }
        }
    } else {
        let read_dir = match fs::read_dir(&path) {
            Ok(rd) => rd,
            Err(e) => {
                return Ok(format!(
                    "Error: Failed to list directory {}: {}",
                    path.display(),
                    e
                ))
            }
        };
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let type_str = if entry.path().is_dir() { "dir" } else { "file" };
            entries.push(format!("{} ({})", name, type_str));
            if entries.len() >= 1000 {
                break;
            }
        }
    }

    entries.sort();
    let total_count = entries.len();
    let truncated = total_count >= 1000;

    let mut header = format!(
        "Listed directory: {} ({} entries",
        path.display(),
        total_count
    );
    if truncated {
        header.push_str(", truncated to 1000");
    }
    header.push_str(")");

    Ok(format!("{}\n{}", header, entries.join("\n")))
}

pub fn run_delete_file(args: &DeleteFileArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let path = workspace_dir.join(&args.path);
    if !path.exists() {
        return Ok(format!(
            "Error: File not found: {}. Cannot delete a file that doesn't exist.",
            path.display()
        ));
    }

    if path.is_dir() {
        if let Err(e) = fs::remove_dir_all(&path) {
            return Ok(format!(
                "Error deleting directory {}: {}",
                path.display(),
                e
            ));
        }
        Ok(format!("Deleted directory: {}", path.display()))
    } else {
        if let Err(e) = fs::remove_file(&path) {
            return Ok(format!("Error deleting file {}: {}", path.display(), e));
        }
        Ok(format!("Deleted file: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_read_file_with_pagination() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3\nline4\n").unwrap();

        let args = ReadFileArgs {
            path: "test.txt".to_string(),
            offset: Some(1),
            limit: Some(2),
        };

        let result = run_read_file(&args, dir.path()).unwrap();
        assert!(result.contains("Read file:"));
        assert!(result.contains("showing lines 2-3 of 4"));
        assert!(result.contains("     2\tline2"));
        assert!(result.contains("     3\tline3"));
        assert!(!result.contains("line1"));
    }

    #[test]
    fn test_write_file_new_and_update() {
        let dir = tempdir().unwrap();

        let args_create = WriteFileArgs {
            path: "new.txt".to_string(),
            content: "hello".to_string(),
        };
        let res1 = run_write_file(&args_create, dir.path()).unwrap();
        assert!(res1.contains("Created file"));

        let args_update = WriteFileArgs {
            path: "new.txt".to_string(),
            content: "world".to_string(),
        };
        let res2 = run_write_file(&args_update, dir.path()).unwrap();
        assert!(res2.contains("Updated file"));
    }

    #[test]
    fn test_list_files_basic() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("f1.txt"), "").unwrap();
        fs::create_dir(dir.path().join("d1")).unwrap();

        let args = ListFilesArgs {
            path: ".".to_string(),
            recursive: Some(false),
        };
        let result = run_list_files(&args, dir.path()).unwrap();
        assert!(result.contains("f1.txt (file)"));
        assert!(result.contains("d1 (dir)"));
    }

    #[test]
    fn test_delete_file_and_dir() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("f.txt");
        let sub_dir = dir.path().join("d");
        fs::write(&file_path, "").unwrap();
        fs::create_dir(&sub_dir).unwrap();

        let args_f = DeleteFileArgs {
            path: "f.txt".to_string(),
        };
        run_delete_file(&args_f, dir.path()).unwrap();
        assert!(!file_path.exists());

        let args_d = DeleteFileArgs {
            path: "d".to_string(),
        };
        run_delete_file(&args_d, dir.path()).unwrap();
        assert!(!sub_dir.exists());
    }

    // Error handling tests - verify errors return Ok with error message

    #[test]
    fn test_read_file_not_found_returns_ok() {
        let dir = tempdir().unwrap();
        let args = ReadFileArgs {
            path: "nonexistent.txt".to_string(),
            offset: None,
            limit: None,
        };
        let result = run_read_file(&args, dir.path()).unwrap();
        assert!(result.contains("Error: File not found"));
    }

    #[test]
    fn test_read_file_is_directory_returns_ok() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        let args = ReadFileArgs {
            path: "subdir".to_string(),
            offset: None,
            limit: None,
        };
        let result = run_read_file(&args, dir.path()).unwrap();
        assert!(result.contains("Error: Path is a directory"));
    }

    #[test]
    fn test_read_file_offset_out_of_bounds_returns_ok() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.txt"), "line1\nline2\n").unwrap();
        let args = ReadFileArgs {
            path: "test.txt".to_string(),
            offset: Some(100),
            limit: None,
        };
        let result = run_read_file(&args, dir.path()).unwrap();
        assert!(result.contains("Error: Offset"));
        assert!(result.contains("beyond file length"));
    }

    #[test]
    fn test_list_files_not_found_returns_ok() {
        let dir = tempdir().unwrap();
        let args = ListFilesArgs {
            path: "nonexistent".to_string(),
            recursive: None,
        };
        let result = run_list_files(&args, dir.path()).unwrap();
        assert!(result.contains("Error: Directory not found"));
    }

    #[test]
    fn test_delete_file_not_found_returns_ok() {
        let dir = tempdir().unwrap();
        let args = DeleteFileArgs {
            path: "nonexistent.txt".to_string(),
        };
        let result = run_delete_file(&args, dir.path()).unwrap();
        assert!(result.contains("Error: File not found"));
    }
}
