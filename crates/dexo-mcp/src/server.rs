use std::sync::{Arc, Mutex};
use std::time::Duration;

use dexo_app::mcp::McpService;
use dexo_driver_api::Session;
use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, CancelledNotificationParam,
    ContentBlock, GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation,
    ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer};
use tokio_util::sync::CancellationToken;

use crate::error::hidden_error;
use crate::prompts;
use crate::resources::{ResultStore, list_resources, read_resource};
use crate::tools_read;

pub struct DexoMcpServer {
    pub service: Arc<McpService>,
    store: Arc<Mutex<ResultStore>>,
    cancel: CancellationToken,
    session: Option<Arc<dyn Session>>,
}

impl DexoMcpServer {
    pub fn new(service: McpService) -> Self {
        let profile = service.profile.name.clone();
        Self {
            service: Arc::new(service),
            store: Arc::new(Mutex::new(ResultStore::new(profile))),
            cancel: CancellationToken::new(),
            session: None,
        }
    }

    pub fn with_session(mut self, session: Arc<dyn Session>) -> Self {
        self.session = Some(session);
        self
    }

    pub fn store(&self) -> Arc<Mutex<ResultStore>> {
        Arc::clone(&self.store)
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn clear_results(&self) {
        self.store.lock().expect("result store").clear();
    }
}

impl Drop for DexoMcpServer {
    fn drop(&mut self) {
        self.clear_results();
        self.cancel.cancel();
    }
}

impl ServerHandler for DexoMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
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
        Ok(ListToolsResult::with_all_items(tools_read::list_tools(
            &self.service,
        )))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if self.cancel.is_cancelled() {
            return Ok(CallToolResult::error(vec![ContentBlock::text("cancelled")]).into());
        }
        let arguments = request.arguments.unwrap_or_default();
        if request.name == "query_execute_read" {
            return execute_read_tool(self, arguments).await;
        }
        if request.name == "query_explain" {
            return explain_tool(self, arguments).await;
        }
        Ok(tools_read::call_tool(&self.service, &request.name, arguments).into())
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
            Ok(body) => Ok(ReadResourceResult::new(vec![ResourceContents::text(
                body,
                request.uri,
            )])
            .into()),
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

    async fn on_cancelled(
        &self,
        _notification: CancelledNotificationParam,
        _context: NotificationContext<RoleServer>,
    ) {
        self.cancel.cancel();
        self.clear_results();
    }
}

async fn execute_read_tool(
    server: &DexoMcpServer,
    arguments: serde_json::Map<String, serde_json::Value>,
) -> Result<CallToolResponse, McpError> {
    let sql = arguments
        .get("sql")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if let Err(error) = server.service.validate_sql(sql) {
        return Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into());
    }
    let Some(session) = &server.session else {
        let uri = dexo_app::mcp::new_result_uri();
        let body = serde_json::json!({ "status": "authorized", "sql": sql }).to_string();
        server.store.lock().expect("result store").insert(
            uri.clone(),
            body,
            Duration::from_secs(60),
        );
        return Ok(CallToolResult::success(vec![ContentBlock::text(uri)]).into());
    };
    match server.service.execute_read(session.as_ref(), sql).await {
        Ok(value) => {
            let uri = dexo_app::mcp::new_result_uri();
            server.store.lock().expect("result store").insert(
                uri.clone(),
                value.to_string(),
                Duration::from_secs(60),
            );
            Ok(CallToolResult::success(vec![ContentBlock::text(uri)]).into())
        }
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
    if let Err(error) = server.service.validate_sql(sql) {
        return Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into());
    }
    let Some(session) = &server.session else {
        return Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "explain estimated: {sql}"
        ))])
        .into());
    };
    match server.service.explain(session.as_ref(), sql, false).await {
        Ok(plan) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string(&plan).unwrap_or_else(|_| hidden_error().into()),
        )])
        .into()),
        Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into()),
    }
}
