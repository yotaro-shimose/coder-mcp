use regex::Regex;
use rmcp::schemars;
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GrepArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub include: Option<String>,
}

pub fn run_grep(args: &GrepArgs, workspace_dir: &Path) -> Result<String, McpError> {
    let base_path = if let Some(p) = &args.path {
        PathBuf::from(p)
    } else {
        workspace_dir.to_path_buf()
    };

    if !base_path.is_dir() {
        return Ok(format!(
            "Path '{}' is not a valid directory",
            base_path.display()
        ));
    }

    let re = match Regex::new(&args.pattern) {
        Ok(r) => r,
        Err(e) => {
            return Ok(format!(
                "Error: Invalid regex pattern '{}': {}",
                args.pattern, e
            ))
        }
    };

    let include_pattern = args.include.as_deref();
    let include_glob = if let Some(p) = include_pattern {
        match glob::Pattern::new(p) {
            Ok(pat) => Some(pat),
            Err(e) => {
                return Ok(format!(
                    "Error: Invalid include glob pattern '{}': {}",
                    p, e
                ))
            }
        }
    } else {
        None
    };

    let mut matches = Vec::new();
    let walker = WalkDir::new(&base_path).follow_links(true).into_iter();

    for entry in walker.filter_map(|e| e.ok()) {
        if matches.len() >= 100 {
            break;
        }
        if !entry.file_type().is_file() {
            continue;
        }

        if let Some(ref pat) = include_glob {
            if !pat.matches_path(Path::new(entry.file_name())) {
                continue;
            }
        }

        let path = entry.path();
        if let Ok(content) = std::fs::read_to_string(path) {
            if re.is_match(&content) {
                matches.push(path.to_string_lossy().to_string());
            }
        }
    }

    let truncated = matches.len() >= 100;
    let count = matches.len();
    let matches_str = matches.join("\n");
    let mut output = format!(
        "Found {} file(s) containing pattern '{}' in '{}'",
        count,
        args.pattern,
        base_path.display()
    );
    if let Some(inc) = include_pattern {
        output.push_str(&format!(" (filtered by '{}')", inc));
    }
    output.push_str(":\n");
    output.push_str(&matches_str);

    if truncated {
        output.push_str(
            "\n\n[Results truncated to first 100 files. Consider using a more specific pattern.]",
        );
    }

    if count == 0 {
        output = format!(
            "No files found containing pattern '{}' in directory '{}'",
            args.pattern,
            base_path.display()
        );
        if let Some(inc) = include_pattern {
            output.push_str(&format!(" (filtered by '{}')", inc));
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_grep_basic() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "hello world").unwrap();

        let args = GrepArgs {
            pattern: "world".to_string(),
            path: Some(dir.path().to_string_lossy().to_string()),
            include: None,
        };

        let result = run_grep(&args, dir.path()).unwrap();
        assert!(result.contains("Found 1 file(s)"));
        assert!(result.contains("test.txt"));
    }

    #[test]
    fn test_grep_regex() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "abc 123 xyz").unwrap();

        let args = GrepArgs {
            pattern: r"\d+".to_string(),
            path: Some(dir.path().to_string_lossy().to_string()),
            include: None,
        };

        let result = run_grep(&args, dir.path()).unwrap();
        assert!(result.contains("Found 1 file(s)"));
    }

    #[test]
    fn test_grep_case_insensitive() {
        // Rust Regex is case sensitive by default, unless using (?i)
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "HELLO").unwrap();

        let args = GrepArgs {
            pattern: "(?i)hello".to_string(),
            path: Some(dir.path().to_string_lossy().to_string()),
            include: None,
        };

        let result = run_grep(&args, dir.path()).unwrap();
        assert!(result.contains("Found 1 file(s)"));
    }

    #[test]
    fn test_grep_with_include_filter() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("test.txt");
        let file2 = dir.path().join("test.rs");
        writeln!(File::create(&file1).unwrap(), "match").unwrap();
        writeln!(File::create(&file2).unwrap(), "match").unwrap();

        let args = GrepArgs {
            pattern: "match".to_string(),
            path: Some(dir.path().to_string_lossy().to_string()),
            include: Some("*.rs".to_string()),
        };

        let result = run_grep(&args, dir.path()).unwrap();
        assert!(result.contains("Found 1 file(s)"));
        assert!(!result.contains("test.txt"));
        assert!(result.contains("test.rs"));
    }

    #[test]
    fn test_grep_invalid_regex_returns_ok() {
        let dir = tempdir().unwrap();
        let args = GrepArgs {
            pattern: "[".to_string(), // Invalid regex
            path: None,
            include: None,
        };
        let result = run_grep(&args, dir.path()).unwrap();
        assert!(result.contains("Error: Invalid regex pattern"));
    }

    #[test]
    fn test_grep_invalid_include_glob_returns_ok() {
        let dir = tempdir().unwrap();
        let args = GrepArgs {
            pattern: "test".to_string(),
            path: None,
            include: Some("[".to_string()), // Invalid glob
        };
        let result = run_grep(&args, dir.path()).unwrap();
        assert!(result.contains("Error: Invalid include glob pattern"));
    }
}
