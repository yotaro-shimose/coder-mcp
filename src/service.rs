use crate::models::ExecuteBashRequest;
use crate::runtime::bash::BashEventService;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use std::path::PathBuf;
use std::sync::Arc;

use crate::tools::file_tools::*;
use crate::tools::glob::{run_glob, GlobArgs};
use crate::tools::grep::{run_grep, GrepArgs};

#[derive(Clone)]
pub struct CoderMcpService {
    bash: Arc<BashEventService>,
    workspace_dir: PathBuf,
    tool_router: ToolRouter<CoderMcpService>,
    truncation_limit: usize,
}

// Bash tool arguments
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct BashArgs {
    pub command: String,
    pub cwd: Option<String>,
    pub timeout: Option<u64>,
}

// File tool arguments
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ViewFileArgs {
    pub path: String,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct ListDirectoryArgs {
    pub path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct CreateFileArgs {
    pub path: String,
    pub content: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct StrReplaceArgs {
    pub path: String,
    pub old_str: String,
    pub new_str: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct InsertLinesArgs {
    pub path: String,
    pub insert_line: u64,
    pub content: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct DeleteFileArgs {
    pub path: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct TreeArgs {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub exclude: Option<String>,
    pub max_depth: Option<usize>,
    #[serde(default)]
    pub truncate: Option<usize>,
}

#[tool_router]
impl CoderMcpService {
    pub fn new(bash: BashEventService, workspace_dir: PathBuf, truncation_limit: usize) -> Self {
        Self {
            bash: Arc::new(bash),
            workspace_dir,
            tool_router: Self::tool_router(),
            truncation_limit,
        }
    }

    fn truncate_output(&self, text: String) -> String {
        let char_limit = self.truncation_limit;
        let char_count = text.chars().count();
        if char_count <= char_limit {
            return text;
        }

        let half = 3000;
        if char_count <= half * 2 {
            // Cannot take distinct first and last 3000, so return whole text
            // even if it exceeds the configured limit.
            return text;
        }

        let start: String = text.chars().take(half).collect();
        let end: String = text.chars().rev().take(half).collect::<String>().chars().rev().collect();
        format!("{}...[truncated]...{}", start, end)
    }

    #[tool(
        name = "search_filenames",
        description = "Fast file pattern matching tool. Finds files by name patterns (e.g. '**/*.js'). Returns matching file paths."
    )]
    async fn search_filenames(
        &self,
        Parameters(args): Parameters<GlobArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_glob(&args, &self.workspace_dir)?;
        Ok(CallToolResult::success(vec![Content::text(self.truncate_output(output))]))
    }

    #[tool(
        name = "search_content",
        description = "Fast content search tool. Searches file contents using regex. Returns matching file paths."
    )]
    async fn search_content(
        &self,
        Parameters(args): Parameters<GrepArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_grep(&args, &self.workspace_dir)?;
        Ok(CallToolResult::success(vec![Content::text(self.truncate_output(output))]))
    }

    #[tool(
        name = "bash",
        description = "Execute a bash command in a stateful terminal session. State (environment variables, working directory) persists across calls."
    )]
    async fn bash(
        &self,
        Parameters(args): Parameters<BashArgs>,
    ) -> Result<CallToolResult, McpError> {
        tracing::info!("Executing bash command: {}", args.command);
        let req = ExecuteBashRequest {
            command: args.command,
            cwd: args.cwd,
            timeout: args.timeout,
        };

        let (cmd, rx) = self.bash.start_bash_command(req);
        tracing::info!("Started bash command with ID: {}", cmd.id);

        let out = rx.await.map_err(|_| McpError {
            code: ErrorCode(0),
            message: "Bash execution task ended without sending result"
                .to_string()
                .into(),
            data: None,
        })?;

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
            result_str.push_str(&format!("[Command finished with exit code {}]", exit_code));
        }

        Ok(CallToolResult::success(vec![Content::text(
            self.truncate_output(result_str),
        )]))
    }

    #[tool(
        name = "view_file",
        description = "Read file contents with optional line range. Returns file content with line numbers."
    )]
    async fn view_file(
        &self,
        Parameters(args): Parameters<ViewFileArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_view_file(&args, &self.workspace_dir).await?;
        Ok(CallToolResult::success(vec![Content::text(self.truncate_output(output))]))
    }

    #[tool(
        name = "list_directory",
        description = "List contents of a directory, excluding hidden files."
    )]
    async fn list_directory(
        &self,
        Parameters(args): Parameters<ListDirectoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_list_directory(&args, &self.workspace_dir).await?;
        Ok(CallToolResult::success(vec![Content::text(self.truncate_output(output))]))
    }

    #[tool(
        name = "create_file",
        description = "Create a new file with content. Returns error if file already exists."
    )]
    async fn create_file(
        &self,
        Parameters(args): Parameters<CreateFileArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_create_file(&args, &self.workspace_dir).await?;
        Ok(CallToolResult::success(vec![Content::text(self.truncate_output(output))]))
    }

    #[tool(
        name = "str_replace",
        description = "Find and replace exact string in file. Returns error if string not found or multiple matches. Shows context snippet after edit."
    )]
    async fn str_replace(
        &self,
        Parameters(args): Parameters<StrReplaceArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_str_replace(&args, &self.workspace_dir).await?;
        Ok(CallToolResult::success(vec![Content::text(self.truncate_output(output))]))
    }

    #[tool(
        name = "insert_lines",
        description = "Insert content at a specific line number. Shows context snippet after edit."
    )]
    async fn insert_lines(
        &self,
        Parameters(args): Parameters<InsertLinesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_insert_lines(&args, &self.workspace_dir).await?;
        Ok(CallToolResult::success(vec![Content::text(self.truncate_output(output))]))
    }

    #[tool(
        name = "delete_file",
        description = "Delete a file from the workspace."
    )]
    async fn delete_file(
        &self,
        Parameters(args): Parameters<DeleteFileArgs>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_delete_file(&args, &self.workspace_dir).await?;
        Ok(CallToolResult::success(vec![Content::text(self.truncate_output(output))]))
    }
}

#[tool_handler]
impl ServerHandler for CoderMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Coder MCP Server providing Bash and File tools".to_string()),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        Ok(self.get_info().into())
    }
}

// ===================================
// Read-Only Service Implementation
// ===================================

