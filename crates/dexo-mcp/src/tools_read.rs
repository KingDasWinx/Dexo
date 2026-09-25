use std::sync::Arc;

use dexo_app::mcp::{McpConnection, McpService, advertised_tools};
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, Tool};
use serde_json::{Map, Value};

use crate::schema::{ObjectInput, QueryInput, SearchInput};

pub fn list_tools(service: &McpService) -> Vec<Tool> {
    advertised_tools(&service.profile)
        .into_iter()
        .map(|name| Tool::new(name, name, input_schema()))
        .collect()
}

pub fn call_tool(
    service: &McpService,
    connection: Option<&McpConnection>,
    name: &str,
    arguments: Map<String, Value>,
) -> CallToolResult {
    if !advertised_tools(&service.profile).contains(&name) {
        return CallToolResult::error(vec![ContentBlock::text(crate::error::hidden_error())]);
    }
    let value = Value::Object(arguments);
    let outcome: Result<String, String> = match name {
        "catalog_search" => serde_json::from_value::<SearchInput>(value)
            .map_err(|error| error.to_string())
            .map(|input| serde_json::to_string(&service.search(&input.query)).unwrap_or_default()),
        "object_describe" => serde_json::from_value::<ObjectInput>(value)
            .map_err(|error| error.to_string())
            .and_then(|input| {
                service
                    .describe(&input.id)
                    .map_err(|error| error.to_string())
            })
            .map(|object| serde_json::to_string(&object).unwrap_or_default()),
        "object_get_ddl" => serde_json::from_value::<ObjectInput>(value)
            .map_err(|error| error.to_string())
            .and_then(|input| service.ddl(&input.id).map_err(|error| error.to_string())),
        "object_relationships" => serde_json::from_value::<ObjectInput>(value)
            .map_err(|error| error.to_string())
            .and_then(|input| {
                service
                    .relationships(&input.id)
                    .map_err(|error| error.to_string())
            })
            .map(|items| serde_json::to_string(&items).unwrap_or_default()),
        "query_validate" => match connection {
            None => Err("no connection is configured for this profile".into()),
            Some(connection) => serde_json::from_value::<QueryInput>(value)
                .map_err(|error| error.to_string())
                .and_then(|input| {
                    service
                        .validate_sql(connection, &input.sql)
                        .map(|()| "ok".to_string())
                        .map_err(|error| error.to_string())
                }),
        },
        _ => Err(crate::error::hidden_error().into()),
    };
    match outcome {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(text) => CallToolResult::error(vec![ContentBlock::text(text)]),
    }
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
