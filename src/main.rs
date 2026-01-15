use coder_mcp::server::run_server;
use std::env;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    let cwd = std::env::current_dir().unwrap();

    // Use WORKSPACE_DIR env var if set, otherwise default to current_dir/workspace
    let workspace_path = env::var("WORKSPACE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| cwd.join("workspace"));

    let port = 3000;
    run_server(workspace_path, port).await;
}
