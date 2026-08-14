use dexo_app::mcp::{McpService, advertised_tools};
use rmcp::model::{Prompt, PromptMessage, Role};

pub fn list_prompts(service: &McpService) -> Vec<Prompt> {
    let _ = service;
    vec![
        Prompt::new(
            "explore_schema",
            Some("Explore allowed schema objects"),
            None,
        ),
        Prompt::new(
            "review_migration",
            Some("Review a schema diff using allowed tools"),
            None,
        ),
        Prompt::new("analyze_plan", Some("Analyze an explain plan"), None),
    ]
}

pub fn get_prompt(service: &McpService, name: &str) -> Result<Vec<PromptMessage>, String> {
    let tools = advertised_tools(&service.profile).join(", ");
    let text = match name {
        "explore_schema" => format!(
            "Use catalog_search and object_describe on allowed URIs only. Tools: {tools}. Resource: dexo://profile/capabilities"
        ),
        "review_migration" => format!("Use schema_diff on allowed snapshots only. Tools: {tools}"),
        "analyze_plan" => format!("Use query_explain without ANALYZE. Tools: {tools}"),
        _ => return Err(crate::error::hidden_error().into()),
    };
    Ok(vec![PromptMessage::new_text(Role::User, text)])
}
