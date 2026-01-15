use glob::glob;
use rmcp::schemars;
use rmcp::ErrorData as McpError;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GlobArgs {
    pub pattern: String,
    pub path: Option<String>,
}

pub fn run_glob(args: &GlobArgs, workspace_dir: &Path) -> Result<String, McpError> {
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

    let pattern_str = if Path::new(&args.pattern).is_absolute() {
        args.pattern.clone()
    } else {
        base_path.join(&args.pattern).to_string_lossy().to_string()
    };

    let mut matches = Vec::new();
    // glob returns Result<Paths, PatternError>
    let paths = match glob(&pattern_str) {
        Ok(p) => p,
        Err(e) => {
            return Ok(format!(
                "Error: Invalid glob pattern '{}': {}",
                args.pattern, e
            ))
        }
    };

    for entry in paths {
        match entry {
            Ok(path) => {
                matches.push(path.to_string_lossy().to_string());
                if matches.len() >= 100 {
                    break;
                }
            }
            Err(e) => {
                return Ok(format!("Error while iterating glob matches: {}", e));
            }
        }
    }

    let truncated = matches.len() >= 100;
    let count = matches.len();
    let matches_str = matches.join("\n");
    let mut output = format!(
        "Found {} file(s) matching pattern '{}' in '{}':\n{}",
        count,
        args.pattern,
        base_path.display(),
        matches_str
    );

    if truncated {
        output.push_str(
            "\n\n[Results truncated to first 100 files. Consider using a more specific pattern.]",
        );
    }

    if count == 0 {
        output = format!(
            "No files found matching pattern '{}' in directory '{}'",
            args.pattern,
            base_path.display()
        );
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_glob_basic() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        File::create(&file_path).unwrap();

        let args = GlobArgs {
            pattern: "*.txt".to_string(),
            path: Some(dir.path().to_string_lossy().to_string()),
        };

        let result = run_glob(&args, dir.path()).unwrap();
        assert!(result.contains("Found 1 file(s)"));
        assert!(result.contains("test.txt"));
    }

    #[test]
    fn test_glob_no_matches() {
        let dir = tempdir().unwrap();
        let args = GlobArgs {
            pattern: "*.rs".to_string(),
            path: Some(dir.path().to_string_lossy().to_string()),
        };

        let result = run_glob(&args, dir.path()).unwrap();
        assert!(result.contains("No files found"));
    }

    #[test]
    fn test_glob_recursive() {
        let dir = tempdir().unwrap();
        let sub_dir = dir.path().join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        File::create(sub_dir.join("test.json")).unwrap();

        let args = GlobArgs {
            pattern: "**/*.json".to_string(),
            path: Some(dir.path().to_string_lossy().to_string()),
        };

        let result = run_glob(&args, dir.path()).unwrap();
        assert!(result.contains("Found 1 file(s)"));
        assert!(result.contains("test.json"));
    }
}
