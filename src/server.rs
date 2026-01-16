use crate::logger;
use crate::runtime::bash::BashEventService;
use crate::service::{CoderMcpReadOnlyService, CoderMcpService};
use axum::Router;
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
    let bash_service = BashEventService::new(cwd.join("bash_events"), Some(workspace_path.clone()));

    // Create the MCP service
    let coder_mcp_service = CoderMcpService::new(bash_service, workspace_path.clone());
    let coder_mcp_service_ro = CoderMcpReadOnlyService::new(workspace_path);

    // Wrap in StreamableHttpService
    let mcp_service: StreamableHttpService<CoderMcpService, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(coder_mcp_service.clone()),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );

    // Wrap read-only service
    let mcp_service_ro: StreamableHttpService<CoderMcpReadOnlyService, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(coder_mcp_service_ro.clone()),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );

    // Build our application with routes
    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "OK" }))
        .nest_service("/mcp", mcp_service)
        .nest_service("/mcp-readonly", mcp_service_ro);

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
