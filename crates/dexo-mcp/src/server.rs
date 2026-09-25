use std::sync::{Arc, Mutex};
use std::time::Duration;

use dexo_app::mcp::ledger::GrantLedger;
use dexo_app::mcp::{McpConnection, McpService, advertised_tools};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, GetPromptRequestParams, GetPromptResponse,
    GetPromptResult, Implementation, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::backend::McpBackend;
use crate::error::{HIDDEN, tool_error};
use crate::router::McpConnectionRouter;
use crate::tools_write::{now_secs, write_tool_names};
use crate::{prompts, resources};

/// Bumped whenever a tool's name, input schema or annotations change; the snapshot test
/// in `tests/protocol.rs` fails until it is (MCP-020).
pub const TOOL_SCHEMA_VERSION: u32 = 2;

#[derive(Clone)]
pub struct DexoMcpServer {
    pub(crate) inner: Arc<Inner>,
    tool_router: ToolRouter<Self>,
}

pub(crate) struct Inner {
    pub service: McpService,
    pub router: McpConnectionRouter,
    pub ledger: Arc<dyn GrantLedger>,
    pub session_id: String,
    calls: Semaphore,
    last_revision: Mutex<u64>,
    stop: CancellationToken,
}

impl DexoMcpServer {
    pub fn new(
        service: McpService,
        connections: Vec<McpConnection>,
        backend: Arc<dyn McpBackend>,
        ledger: Arc<dyn GrantLedger>,
    ) -> Self {
        let concurrency = service.profile.limits.max_concurrency as usize;
        let connect_timeout = Duration::from_secs(service.profile.limits.timeout_secs);
        Self {
            inner: Arc::new(Inner {
                router: McpConnectionRouter::new(connections, backend, connect_timeout),
                ledger,
                session_id: uuid::Uuid::new_v4().to_string(),
                calls: Semaphore::new(concurrency),
                last_revision: Mutex::new(0),
                stop: CancellationToken::new(),
                service,
            }),
            tool_router: Self::read_tools() + Self::write_tools(),
        }
    }

    /// Stops the `list_changed` poller; `serve_io` cancels it when the client goes away.
    pub fn stop_token(&self) -> CancellationToken {
        self.inner.stop.clone()
    }

    /// What this client may see right now: the profile's reads, plus the writes an active
    /// grant has published (MCP-002, MCP-004). `tools/call` is gated by the same list.
    pub fn visible_tools(&self) -> Vec<String> {
        let profile = &self.inner.service.profile;
        let mut tools: Vec<String> = advertised_tools(profile)
            .into_iter()
            .map(str::to_string)
            .collect();
        tools.extend(
            write_tool_names(self.inner.ledger.as_ref(), &profile.name, now_secs())
                .into_iter()
                .filter(|tool| profile.tool_allowed(tool)),
        );
        tools
    }
}

impl ServerHandler for DexoMcpServer {
    fn get_info(&self) -> ServerInfo {
        let profile = &self.inner.service.profile;
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("dexo", env!("CARGO_PKG_VERSION")))
        .with_instructions(format!(
            "Dexo database workbench, MCP profile '{}' (tool schema v{TOOL_SCHEMA_VERSION}). \
             Start with list_connections; every database tool takes an optional `connection`. \
             Reads run inside read-only transactions and return at most {} rows / {} bytes. \
             Write tools appear only while a grant created with `dexo mcp grant create` is active.",
            profile.name, profile.limits.max_rows, profile.limits.max_bytes
        ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let visible = self.visible_tools();
        Ok(ListToolsResult::with_all_items(
            self.tool_router
                .list_all()
                .into_iter()
                .filter(|tool| visible.iter().any(|name| name == tool.name.as_ref()))
                .collect(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if !self
            .visible_tools()
            .iter()
            .any(|name| name == request.name.as_ref())
        {
            return Ok(tool_error("NOT_FOUND", HIDDEN).into());
        }
        let Ok(_permit) = self.inner.calls.try_acquire() else {
            return Ok(tool_error(
                "BUSY",
                "too many calls in flight for this profile; retry shortly",
            )
            .into());
        };
        self.tool_router
            .call(ToolCallContext::new(self, request, context))
            .await
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(
            resources::list_resources(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        match resources::read_resource(
            &self.inner.service,
            self.inner.router.connections(),
            &request.uri,
        ) {
            Some(body) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(body, request.uri)]).into())
            }
            None => Err(McpError::resource_not_found(
                HIDDEN,
                Some(serde_json::json!({ "uri": request.uri })),
            )),
        }
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(Vec::new()))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(prompts::list_prompts(
            &self.inner.service,
        )))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        match prompts::get_prompt(&self.inner.service, &request.name) {
            Ok(messages) => Ok(GetPromptResult::new(messages).into()),
            Err(_) => Err(McpError::invalid_params(HIDDEN, None)),
        }
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        let inner = Arc::clone(&self.inner);
        let peer = context.peer.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = inner.stop.cancelled() => break,
                    () = tokio::time::sleep(Duration::from_millis(150)) => {}
                }
                let revision = inner.ledger.revision();
                let changed = {
                    let mut last = inner.last_revision.lock().expect("revision");
                    let changed = revision != *last;
                    *last = revision;
                    changed
                };
                if changed {
                    let _ = peer.notify_tool_list_changed().await;
                }
            }
        });
    }
}
