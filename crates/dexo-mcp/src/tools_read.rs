use std::sync::Arc;

use dexo_app::mcp::{McpService, advertised_tools};
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool};
use serde_json::{Map, Value};

use crate::schema::{DiffInput, ExplainInput, ObjectInput, QueryInput, SearchInput};

pub fn list_tools(service: &McpService) -> Vec<Tool> {
    advertised_tools(&service.profile)
        .into_iter()
        .map(|name| Tool::new(name, name, input_schema()))
        .collect()
}

pub fn call_tool(
    service: &McpService,
    name: &str,
    arguments: Map<String, Value>,
) -> CallToolResult {
    if !advertised_tools(&service.profile).contains(&name) {
        return CallToolResult::error(vec![ContentBlock::text(crate::error::hidden_error())]);
    }
    let value = Value::Object(arguments);
    let text = match name {
        "catalog_search" => serde_json::from_value::<SearchInput>(value)
            .map(|input| serde_json::to_string(&service.search(&input.query)).unwrap_or_default())
            .unwrap_or_else(|error| error.to_string()),
        "object_describe" => serde_json::from_value::<ObjectInput>(value)
            .ok()
            .and_then(|input| service.describe(&input.id).ok())
            .map(|object| serde_json::to_string(&object).unwrap_or_default())
            .unwrap_or_else(|| crate::error::hidden_error().into()),
        "object_get_ddl" => serde_json::from_value::<ObjectInput>(value)
            .ok()
            .and_then(|input| service.ddl(&input.id).ok())
            .unwrap_or_else(|| crate::error::hidden_error().into()),
        "object_relationships" => serde_json::from_value::<ObjectInput>(value)
            .ok()
            .and_then(|input| service.relationships(&input.id).ok())
            .map(|items| serde_json::to_string(&items).unwrap_or_default())
            .unwrap_or_else(|| crate::error::hidden_error().into()),
        "query_validate" => serde_json::from_value::<QueryInput>(value)
            .ok()
            .and_then(|input| service.validate_sql(&input.sql).ok().map(|_| "ok".into()))
            .unwrap_or_else(|| "statement rejected".into()),
        "query_explain" => serde_json::from_value::<ExplainInput>(value)
            .map(|input| format!("explain estimated: {}", input.sql))
            .unwrap_or_else(|error| error.to_string()),
        "schema_diff" => serde_json::from_value::<DiffInput>(value)
            .map(|input| format!("diff {} -> {}", input.from, input.to))
            .unwrap_or_else(|error| error.to_string()),
        "query_execute_read" => serde_json::from_value::<QueryInput>(value)
            .ok()
            .and_then(|input| {
                service
                    .validate_sql(&input.sql)
                    .ok()
                    .map(|_| "queued".into())
            })
            .unwrap_or_else(|| "statement rejected".into()),
        _ => crate::error::hidden_error().into(),
    };
    CallToolResult::success(vec![ContentBlock::text(text)])
}

pub(crate) fn input_schema() -> Arc<JsonObject> {
    let value = serde_json::json!({
        "type": "object",
        "properties": {
            "query": {"type": "string"},
            "id": {"type": "string"},
            "sql": {"type": "string"},
            "from": {"type": "string"},
            "to": {"type": "string"}
        }
    });
    Arc::new(value.as_object().cloned().unwrap_or_default())
}
