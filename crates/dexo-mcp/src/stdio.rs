use std::sync::Arc;

use dexo_app::mcp::{GrantLedger, McpConnection, McpService};
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::backend::McpBackend;
use crate::server::DexoMcpServer;

pub fn init_mcp_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
}

pub async fn serve_stdio(
    service: McpService,
    connections: Vec<McpConnection>,
    backend: Arc<dyn McpBackend>,
    ledger: Arc<dyn GrantLedger>,
) -> anyhow::Result<()> {
    init_mcp_tracing();
    let server = DexoMcpServer::new(service, connections, backend, ledger);
    let stop = server.stop_token();
    let running = server.serve(rmcp::transport::io::stdio()).await?;
    let outcome = running.waiting().await;
    stop.cancel();
    outcome?;
    Ok(())
}

pub async fn serve_io<R, W>(
    service: McpService,
    connections: Vec<McpConnection>,
    backend: Arc<dyn McpBackend>,
    ledger: Arc<dyn GrantLedger>,
    read: R,
    write: W,
) -> anyhow::Result<()>
where
    R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
{
    init_mcp_tracing();
    let server = DexoMcpServer::new(service, connections, backend, ledger);
    let stop = server.stop_token();
    let running = server.serve((read, write)).await?;
    let outcome = running.waiting().await;
    stop.cancel();
    outcome?;
    Ok(())
}
