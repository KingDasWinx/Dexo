use dexo_app::mcp::{McpConnection, McpService};
use rmcp::model::Resource;

pub const CAPABILITIES_URI: &str = "dexo://profile/capabilities";

pub fn list_resources() -> Vec<Resource> {
    vec![Resource::new(
        CAPABILITIES_URI,
        "Active MCP profile: tools, limits and connections",
    )]
}

pub fn read_resource<'a>(
    service: &McpService,
    connections: impl Iterator<Item = &'a McpConnection>,
    uri: &str,
) -> Option<String> {
    if uri != CAPABILITIES_URI {
        return None;
    }
    let mut capabilities = service.capabilities();
    capabilities["connections"] = connections
        .map(|connection| {
            serde_json::json!({
                "name": connection.name,
                "driver": connection.driver,
                "environment": format!("{:?}", connection.environment).to_lowercase(),
            })
        })
        .collect();
    Some(capabilities.to_string())
}
