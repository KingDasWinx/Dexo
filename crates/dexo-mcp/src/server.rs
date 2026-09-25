use std::sync::{Arc, Mutex};
use std::time::Duration;

use dexo_app::mcp::grant::WRITE_TOOLS;
use dexo_app::mcp::ledger::GrantLedger;
use dexo_app::mcp::{McpConnection, McpService, advertised_tools};
use dexo_driver_api::Session;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, GetPromptRequestParams,
    GetPromptResponse, GetPromptResult, Implementation, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, ResourceContents,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer};
use tokio_util::sync::CancellationToken;

use crate::error::hidden_error;
use crate::prompts;
use crate::resources::{ResultStore, list_resources, read_resource};
use crate::tools_read;
use crate::tools_write;

pub struct DexoMcpServer {
    pub service: Arc<McpService>,
    store: Arc<Mutex<ResultStore>>,
    stop: CancellationToken,
    target: Option<(McpConnection, Arc<dyn Session>)>,
    session_lock: Arc<tokio::sync::Mutex<()>>,
    ledger: Option<Arc<dyn GrantLedger>>,
    session_id: String,
    last_revision: Arc<Mutex<u64>>,
}

impl DexoMcpServer {
    pub fn new(service: McpService) -> Self {
        let profile = service.profile.name.clone();
        Self {
            service: Arc::new(service),
            store: Arc::new(Mutex::new(ResultStore::new(profile))),
            stop: CancellationToken::new(),
            target: None,
            session_lock: Arc::new(tokio::sync::Mutex::new(())),
            ledger: None,
            session_id: uuid::Uuid::new_v4().to_string(),
            last_revision: Arc::new(Mutex::new(0)),
        }
    }

    pub fn with_session(mut self, connection: McpConnection, session: Arc<dyn Session>) -> Self {
        self.target = Some((connection, session));
        self
    }

    pub fn with_ledger(mut self, ledger: Arc<dyn GrantLedger>) -> Self {
        self.ledger = Some(ledger);
        self
    }

    pub fn store(&self) -> Arc<Mutex<ResultStore>> {
        Arc::clone(&self.store)
    }

    pub fn clear_results(&self) {
        self.store.lock().expect("result store").clear();
    }
}

impl Drop for DexoMcpServer {
    fn drop(&mut self) {
        self.clear_results();
        self.stop.cancel();
    }
}

impl ServerHandler for DexoMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("dexo", env!("CARGO_PKG_VERSION")))
        .with_instructions("Dexo read-only catalog and query MCP")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let mut tools = tools_read::list_tools(&self.service);
        if let Some(ledger) = &self.ledger {
            for name in tools_write::write_tool_names(
                ledger.as_ref(),
                &self.service.profile.name,
                tools_write::now_secs(),
            ) {
                tools.push(rmcp::model::Tool::new(
                    name.clone(),
                    name,
                    tools_read::input_schema(),
                ));
            }
        }
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let arguments = request.arguments.unwrap_or_default();
        let is_write = WRITE_TOOLS
            .iter()
            .any(|name| *name == request.name.as_ref());
        if !is_write && !advertised_tools(&self.service.profile).contains(&request.name.as_ref()) {
            return Ok(CallToolResult::error(vec![ContentBlock::text(hidden_error())]).into());
        }
        if is_write {
            let Some(ledger) = &self.ledger else {
                return Ok(CallToolResult::error(vec![ContentBlock::text(hidden_error())]).into());
            };
            match tools_write::call_write_tool(
                &self.service,
                ledger.as_ref(),
                self.target
                    .as_ref()
                    .map(|(connection, session)| (connection, session.as_ref())),
                &self.session_id,
                &request.name,
                arguments,
                tools_write::now_secs(),
            )
            .await
            {
                Ok(text) => {
                    return Ok(CallToolResult::success(vec![ContentBlock::text(text)]).into());
                }
                Err(error) => {
                    return Ok(
                        CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into(),
                    );
                }
            }
        }
        if request.name == "query_execute_read" {
            return execute_read_tool(self, arguments, context.ct.clone()).await;
        }
        if request.name == "query_explain" {
            return explain_tool(self, arguments).await;
        }
        Ok(tools_read::call_tool(
            &self.service,
            self.target.as_ref().map(|(connection, _)| connection),
            &request.name,
            arguments,
        )
        .into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let store = self.store.lock().expect("result store");
        Ok(ListResourcesResult::with_all_items(list_resources(
            &self.service,
            &store,
        )))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let store = self.store.lock().expect("result store");
        match read_resource(&self.service, &store, &request.uri) {
            Ok(body) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(body, request.uri)]).into())
            }
            Err(_) => Err(McpError::resource_not_found(
                hidden_error(),
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
            &self.service,
        )))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        match prompts::get_prompt(&self.service, &request.name) {
            Ok(messages) => Ok(GetPromptResult::new(messages).into()),
            Err(_) => Err(McpError::invalid_params(hidden_error(), None)),
        }
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        let Some(ledger) = self.ledger.clone() else {
            return;
        };
        let last_revision = Arc::clone(&self.last_revision);
        let cancel = self.stop.clone();
        let peer = context.peer.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(150)) => {}
                }
                let revision = ledger.revision();
                let changed = {
                    let mut last = last_revision.lock().expect("revision");
                    if revision != *last {
                        *last = revision;
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    let _ = peer.notify_tool_list_changed().await;
                }
            }
        });
    }
}

async fn execute_read_tool(
    server: &DexoMcpServer,
    arguments: serde_json::Map<String, serde_json::Value>,
    cancel: CancellationToken,
) -> Result<CallToolResponse, McpError> {
    let sql = arguments
        .get("sql")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let Some((connection, session)) = &server.target else {
        return Ok(no_connection().into());
    };
    let _serialized = server.session_lock.lock().await;
    match server
        .service
        .execute_read(session.as_ref(), connection, sql, &cancel)
        .await
    {
        Ok(result) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string(&result).unwrap_or_default(),
        )])
        .into()),
        Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into()),
    }
}

async fn explain_tool(
    server: &DexoMcpServer,
    arguments: serde_json::Map<String, serde_json::Value>,
) -> Result<CallToolResponse, McpError> {
    let sql = arguments
        .get("sql")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let Some((connection, session)) = &server.target else {
        return Ok(no_connection().into());
    };
    let _serialized = server.session_lock.lock().await;
    match server
        .service
        .explain(session.as_ref(), connection, sql)
        .await
    {
        Ok(plan) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string(&plan).unwrap_or_else(|_| hidden_error().into()),
        )])
        .into()),
        Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into()),
    }
}

fn no_connection() -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(
        "no connection is configured for this profile",
    )])
}
