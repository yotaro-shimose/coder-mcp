use crate::logger;
use crate::runtime::bash::BashEventService;
use crate::service::CoderMcpService;
use crate::tools::file_tools::{run_tree, TreeArgs};
use axum::{extract::Query, Router};
use rmcp::transport::{
    StreamableHttpServerConfig,
    streamable_http_server::{session::local::LocalSessionManager, tower::StreamableHttpService},
};
use std::path::PathBuf;
use tokio::net::TcpListener;

pub async fn run_server(
    workspace_path: PathBuf,
    port: u16,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    // Set up tracing using the local logger
    logger::init_logging();

    let cwd = std::env::current_dir().unwrap();
    let bash_service = BashEventService::new(cwd.join(".coder_mcp"), Some(workspace_path.clone()));

    // Create the MCP service
    let coder_mcp_service = CoderMcpService::new(bash_service, workspace_path.clone());

    // Wrap in StreamableHttpService
    let mcp_service: StreamableHttpService<CoderMcpService, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(coder_mcp_service.clone()),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );

    // Build our application with routes
    let tree_workspace = workspace_path.clone();
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "OK" }))
        .route(
            "/tree",
            axum::routing::get(move |Query(args): Query<TreeArgs>| async move {
                match run_tree(&args, &tree_workspace) {
                    Ok(tree) => tree,
                    Err(e) => format!("Error: {}", e.message),
                }
            }),
        )
        .nest_service("/mcp", mcp_service);

    // Run it
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
            tracing::info!("Server shutting down");
        })
        .await
        .unwrap();
}
