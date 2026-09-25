use dexo_app::mcp::{McpConnection, McpService};
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

pub async fn serve_with_ledger(
    service: McpService,
    target: Option<(McpConnection, std::sync::Arc<dyn dexo_driver_api::Session>)>,
    ledger: Option<std::sync::Arc<dyn dexo_app::mcp::GrantLedger>>,
) -> anyhow::Result<()> {
    init_mcp_tracing();
    let mut server = DexoMcpServer::new(service);
    if let Some((connection, session)) = target {
        server = server.with_session(connection, session);
    }
    if let Some(ledger) = ledger {
        server = server.with_ledger(ledger);
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
    serve_io_with_ledger(service, read, write, None).await
}

pub async fn serve_io_with_ledger<R, W>(
    service: McpService,
    read: R,
    write: W,
    ledger: Option<std::sync::Arc<dyn dexo_app::mcp::GrantLedger>>,
) -> anyhow::Result<()>
where
    R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
{
    init_mcp_tracing();
    let mut server = DexoMcpServer::new(service);
    if let Some(ledger) = ledger {
        server = server.with_ledger(ledger);
    }
    let running = server.serve((read, write)).await?;
    running.waiting().await?;
    Ok(())
}
