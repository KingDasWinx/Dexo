use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dexo_app::mcp::audit::{AuditEvent, SqlAuditMode};
use dexo_app::mcp::ledger::GrantLedger;
use dexo_app::mcp::{McpConnection, McpService, WRITE_TOOLS, advertised_tools};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, GetPromptRequestParams, GetPromptResponse,
    GetPromptResult, Implementation, JsonObject, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, ResourceContents,
    ServerCapabilities, ServerInfo, SubscriptionFilter,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer, SubscriptionContext};
use serde_json::Value;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::backend::McpBackend;
use crate::error::{HIDDEN, busy, tool_error};
use crate::router::McpConnectionRouter;
use crate::tools_write::{now_secs, write_tool_names};
use crate::{prompts, resources};

/// Bumped whenever a tool's name, input schema or annotations change; the snapshot test
/// in `tests/protocol.rs` fails until it is (MCP-020).
pub const TOOL_SCHEMA_VERSION: u32 = 4;

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
    pub calls: Semaphore,
    /// The tools the client was last shown, by `tools/list` or a change notification.
    shown: Mutex<Vec<String>>,
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
        // Requests left by a server that was killed while it waited lose their SQL now,
        // not when someone next opens Approvals.
        ledger.sweep_approvals(now_secs());
        let server = Self {
            inner: Arc::new(Inner {
                router: McpConnectionRouter::new(connections, backend, connect_timeout),
                ledger,
                session_id: uuid::Uuid::new_v4().to_string(),
                calls: Semaphore::new(concurrency),
                shown: Mutex::new(Vec::new()),
                stop: CancellationToken::new(),
                service,
            }),
            tool_router: Self::read_tools() + Self::write_tools(),
        };
        *server.inner.shown.lock().expect("tools") = server.inner.visible_tools();
        server
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

    /// What this client may see right now; `tools/call` is gated by the same list.
    pub fn visible_tools(&self) -> Vec<String> {
        self.inner.visible_tools()
    }
}

impl Inner {
    /// The profile's reads, plus the writes an active grant publishes on a connection that
    /// takes them (MCP-002, MCP-004), and the sessions list while a grant lets sessions be
    /// ended.
    fn visible_tools(&self) -> Vec<String> {
        let profile = &self.service.profile;
        let now = now_secs();
        let mut tools: Vec<String> = advertised_tools(profile)
            .into_iter()
            .map(str::to_string)
            .collect();
        let accepts_writes = |name: &str| {
            self.router
                .connections()
                .find(|connection| connection.name == name)
                .is_some_and(|connection| connection.accepts_writes().is_ok())
        };
        tools.extend(
            write_tool_names(self.ledger.as_ref(), &profile.name, now, &accepts_writes)
                .into_iter()
                .filter(|tool| profile.tool_allowed(tool)),
        );
        let lists_sessions = self
            .ledger
            .active_grants(&profile.name, now)
            .iter()
            .any(|grant| grant.lists_sessions() && accepts_writes(&grant.connection));
        if lists_sessions
            && profile.tool_allowed("admin_list_sessions")
            && !tools.iter().any(|tool| tool == "admin_list_sessions")
        {
            tools.push("admin_list_sessions".into());
        }
        tools
    }

    /// Calls `notify` each time the visible tools differ from what the client was last
    /// shown, until the server stops, `until` resolves, or `notify` says the client is gone.
    /// The list is compared, not a counter: a revoke left the maximum revision where it was,
    /// and the client kept tools it had lost.
    // ponytail: one `shown` per connection, so two listens on one connection split the
    // notifications between them; keep one per subscription if a client ever opens two.
    async fn watch_tools<F: Future<Output = bool>>(
        &self,
        until: impl Future<Output = ()>,
        notify: impl Fn() -> F,
    ) {
        let mut until = std::pin::pin!(until);
        loop {
            tokio::select! {
                () = self.stop.cancelled() => break,
                () = &mut until => break,
                () = tokio::time::sleep(Duration::from_millis(150)) => {}
            }
            let tools = self.visible_tools();
            let changed = {
                let mut shown = self.shown.lock().expect("tools");
                let changed = tools != *shown;
                *shown = tools;
                changed
            };
            if changed && !notify().await {
                break;
            }
        }
    }
}

/// An argument the client got wrong, in words: the deserializer's own text is
/// `failed to deserialize parameters: missing field `name``, which names the Rust side.
pub(crate) fn plain_params_error(message: &str) -> String {
    let Some(detail) = message.strip_prefix("failed to deserialize parameters: ") else {
        return message.to_string();
    };
    let quoted = |text: &str| {
        text.split('`')
            .nth(1)
            .map(str::to_string)
            .unwrap_or_default()
    };
    if detail.starts_with("missing field") {
        format!(
            "the required argument `{}` is missing; the tool's inputSchema lists its arguments",
            quoted(detail)
        )
    } else if detail.starts_with("unknown field") {
        format!(
            "`{}` is not an argument of this tool; the tool's inputSchema lists its arguments",
            quoted(detail)
        )
    } else {
        format!("an argument has the wrong type or value: {detail}")
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
             Write tools appear only while a person has given this profile write access to a \
             connection, and every write waits for that person's approval on Dexo's Agents \
             screen: it is refused when they deny it or do not answer in time.",
            profile.name, profile.limits.max_rows, profile.limits.max_bytes
        ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let visible = self.visible_tools();
        self.inner.shown.lock().expect("tools").clone_from(&visible);
        Ok(ListToolsResult::with_all_items(
            self.tool_router
                .list_all()
                .into_iter()
                .filter(|tool| visible.iter().any(|name| name == tool.name.as_ref()))
                .collect(),
        )
        .with_ttl_ms(0)
        .with_cache_scope(CacheScope::Private))
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
        // A write takes its permit itself, once it may run: one that waits for a person
        // holds none meanwhile.
        let _permit = if WRITE_TOOLS.contains(&tool.as_str()) {
            None
        } else {
            let Ok(permit) = self.inner.calls.try_acquire() else {
                let busy: CallToolResponse = busy().into();
                self.audit(&tool, &arguments, &request_id, "deny", Some(&busy), started);
                return Ok(busy);
            };
            Some(permit)
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
        // The deserializer's refusal of a call's arguments arrives as an error result.
        let wrong_arguments = match &response {
            Ok(CallToolResponse::Complete(result)) if result.is_error == Some(true) => result
                .content
                .first()
                .and_then(|block| block.as_text())
                .map(|text| text.text.as_str())
                .filter(|text| text.contains("failed to deserialize parameters: "))
                .map(|text| plain_params_error(text.trim_start_matches("Error [INVALID_INPUT]: "))),
            _ => None,
        };
        match wrong_arguments {
            Some(message) => Ok(tool_error("INVALID_INPUT", &message).into()),
            None => response,
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(
            ListResourcesResult::with_all_items(resources::list_resources())
                .with_ttl_ms(0)
                .with_cache_scope(CacheScope::Private),
        )
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
            Some(body) => Ok(ReadResourceResult::new(vec![ResourceContents::text(
                body,
                request.uri,
            )])
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private)
            .into()),
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
        Ok(ListResourceTemplatesResult::with_all_items(Vec::new())
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(prompts::list_prompts())
            .with_ttl_ms(0)
            .with_cache_scope(CacheScope::Private))
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

    /// A client before protocol 2026-07-28 hears of tool changes unasked; a later one
    /// subscribes with `subscriptions/listen` and hears of them there.
    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        let listens = context
            .peer
            .peer_info()
            .is_some_and(|info| info.protocol_version >= ProtocolVersion::V_2026_07_28);
        if listens {
            return;
        }
        let inner = Arc::clone(&self.inner);
        let peer = context.peer.clone();
        tokio::spawn(async move {
            inner
                .watch_tools(std::future::pending(), || {
                    let peer = peer.clone();
                    async move {
                        let _ = peer.notify_tool_list_changed().await;
                        true
                    }
                })
                .await;
        });
    }

    /// Every category a client asks for is accepted; the SDK keeps what the capabilities
    /// advertise, which is the tool list. Unanswered, each listen was method-not-found.
    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        Some(requested.clone())
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), McpError> {
        if context.accepted().tools_list_changed != Some(true) {
            context.cancelled().await;
            return Ok(());
        }
        let sink = context.sink().clone();
        self.inner
            .watch_tools(context.cancelled(), || {
                let sink = sink.clone();
                async move { sink.notify_tool_list_changed().await.is_ok() }
            })
            .await;
        Ok(())
    }
}
