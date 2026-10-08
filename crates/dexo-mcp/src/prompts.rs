use dexo_app::mcp::{McpService, advertised_tools};
use rmcp::model::{Prompt, PromptMessage, Role};

use crate::error::HIDDEN;

pub fn list_prompts() -> Vec<Prompt> {
    vec![
        Prompt::new(
            "explore_schema",
            Some("Walk the allowed catalog of a connection"),
            None,
        ),
        Prompt::new(
            "review_migration",
            Some("Review the difference between two saved schema snapshots"),
            None,
        ),
        Prompt::new(
            "analyze_plan",
            Some("Read the estimated plan of a query"),
            None,
        ),
    ]
}

pub fn get_prompt(service: &McpService, name: &str) -> Result<Vec<PromptMessage>, String> {
    let tools = advertised_tools(&service.profile).join(", ");
    let text = match name {
        "explore_schema" => format!(
            "Call list_connections, then schema_overview for every table of a schema at once -- columns, defaults, keys, foreign keys and row counts; catalog_list without parent_id walks the catalog down by its ids, and object_describe gives one table and its notes. Available tools: {tools}."
        ),
        "review_migration" => format!(
            "Call schema_diff with the two snapshot names the user gives you and explain each added, removed or changed object. Available tools: {tools}."
        ),
        "analyze_plan" => format!(
            "Call query_explain with the user's query and explain the most expensive nodes. Never ask for ANALYZE. Available tools: {tools}."
        ),
        _ => return Err(HIDDEN.into()),
    };
    Ok(vec![PromptMessage::new_text(Role::User, text)])
}
