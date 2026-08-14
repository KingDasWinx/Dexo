use dexo_app::mcp::McpService;
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::server::DexoMcpServer;

pub fn init_mcp_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
}

pub async fn serve_profile(service: McpService) -> anyhow::Result<()> {
    serve_with_session(service, None).await
}

pub async fn serve_with_session(
    service: McpService,
    session: Option<std::sync::Arc<dyn dexo_driver_api::Session>>,
) -> anyhow::Result<()> {
    init_mcp_tracing();
    let mut server = DexoMcpServer::new(service);
    if let Some(session) = session {
        server = server.with_session(session);
    }
    let running = server.serve(rmcp::transport::io::stdio()).await?;
    running.waiting().await?;
    Ok(())
}

pub async fn serve_io<R, W>(service: McpService, read: R, write: W) -> anyhow::Result<()>
where
    R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
{
    init_mcp_tracing();
    let server = DexoMcpServer::new(service);
    let running = server.serve((read, write)).await?;
    running.waiting().await?;
    Ok(())
}
