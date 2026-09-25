use dexo_app::{AppError, ErrorCategory};
use rmcp::model::{CallToolResult, ContentBlock};

pub const HIDDEN: &str = "not found";

pub fn hidden_error() -> &'static str {
    HIDDEN
}

pub fn hidden() -> AppError {
    AppError::new(ErrorCategory::McpPolicy, HIDDEN)
}

/// `Error [CODE]: message`, a shape a model can branch on without parsing prose.
pub fn tool_error(code: &str, message: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!(
        "Error [{code}]: {message}"
    ))])
}

pub fn app_error(error: &AppError) -> CallToolResult {
    let message = error.to_string();
    let code = match error.category() {
        ErrorCategory::McpPolicy if message == HIDDEN => "NOT_FOUND",
        ErrorCategory::McpPolicy => "POLICY_DENIED",
        ErrorCategory::Permission => "PERMISSION_DENIED",
        ErrorCategory::Configuration | ErrorCategory::Syntax => "INVALID_INPUT",
        ErrorCategory::Timeout => "TIMEOUT",
        ErrorCategory::Cancelled => "CANCELLED",
        ErrorCategory::Capability => "UNSUPPORTED",
        ErrorCategory::Conflict => "CONFLICT",
        ErrorCategory::Authentication | ErrorCategory::Network | ErrorCategory::Transport => {
            "CONNECTION_FAILED"
        }
        ErrorCategory::Storage | ErrorCategory::ExternalTool | ErrorCategory::Internal => {
            "INTERNAL"
        }
    };
    tool_error(code, &message)
}
