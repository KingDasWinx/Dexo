use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dexo_app::mcp::audit::{AuditEvent, SqlAuditMode};
use dexo_app::mcp::ledger::GrantLedger;
use dexo_app::mcp::{McpConnection, McpService, advertised_tools};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, GetPromptRequestParams, GetPromptResponse,
    GetPromptResult, Implementation, JsonObject, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer};
use serde_json::Value;
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
        let retention = i64::from(service.profile.audit_retention_days).saturating_mul(86_400);
        ledger.prune_audits(now_secs().saturating_sub(retention));
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

    fn audit(
        &self,
        tool: &str,
        arguments: &JsonObject,
        request_id: &str,
        decision: &str,
        response: Option<&CallToolResponse>,
        started: Instant,
    ) {
        let result = match response {
            Some(CallToolResponse::Complete(result)) => Some(result),
            _ => None,
        };
        let data = result.and_then(|result| result.structured_content.as_ref());
        let count = |key: &str| {
            data.and_then(|data| data.get(key))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let status = match result {
            Some(result) if result.is_error == Some(true) => result
                .content
                .first()
                .and_then(|block| block.as_text())
                .and_then(|text| text.text.strip_prefix("Error ["))
                .and_then(|rest| rest.split_once(']'))
                .map_or("error", |(code, _)| code)
                .to_string(),
            Some(_) => "ok".to_string(),
            None => "incomplete".to_string(),
        };
        let field = |key: &str| arguments.get(key).and_then(Value::as_str);
        let target = [
            field("connection"),
            field("target").or(field("table")).or(field("name")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(":");
        self.inner.ledger.record_audit(
            AuditEvent {
                timestamp: now_secs(),
                request: format!("tools/call {tool}"),
                operation_id: field("operation_id")
                    .map(str::to_string)
                    .or_else(|| Some(format!("rpc:{request_id}"))),
                profile: self.inner.service.profile.name.clone(),
                client: "mcp".into(),
                target,
                decision: decision.into(),
                grant_id: None,
                duration_ms: started.elapsed().as_millis() as u64,
                rows: count("row_count"),
                bytes: count("bytes"),
                status,
                sql: None,
            }
            .sanitize(SqlAuditMode::Hash, field("sql")),
        );
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
        let tool = request.name.to_string();
        let arguments = request.arguments.clone().unwrap_or_default();
        let request_id = serde_json::to_string(&context.id).unwrap_or_default();
        let started = Instant::now();
        if !self.visible_tools().contains(&tool) {
            let denied: CallToolResponse = tool_error("NOT_FOUND", HIDDEN).into();
            self.audit(
                &tool,
                &arguments,
                &request_id,
                "deny",
                Some(&denied),
                started,
            );
            return Ok(denied);
        }
        let Ok(_permit) = self.inner.calls.try_acquire() else {
            let busy: CallToolResponse = tool_error(
                "BUSY",
                "too many calls in flight for this profile; retry shortly",
            )
            .into();
            self.audit(&tool, &arguments, &request_id, "deny", Some(&busy), started);
            return Ok(busy);
        };
        let response = self
            .tool_router
            .call(ToolCallContext::new(self, request, context))
            .await;
        self.audit(
            &tool,
            &arguments,
            &request_id,
            "allow",
            response.as_ref().ok(),
            started,
        );
        response
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
        Ok(ListPromptsResult::with_all_items(prompts::list_prompts()))
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
