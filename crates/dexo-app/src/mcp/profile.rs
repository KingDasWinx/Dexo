use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, ErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PersistentAccess {
    ReadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum QueryMode {
    StructuredOnly,
    RawReadSql,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct McpLimits {
    pub max_rows: u64,
    pub max_bytes: u64,
    pub timeout_secs: u64,
    pub max_concurrency: u32,
}

impl Default for McpLimits {
    fn default() -> Self {
        Self {
            max_rows: 1_000,
            max_bytes: 1_048_576,
            timeout_secs: 30,
            max_concurrency: 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ToolRule {
    pub tool: String,
    pub allowed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct McpProfile {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub persistent_access: PersistentAccess,
    pub limits: McpLimits,
    pub query_mode: QueryMode,
    pub audit_retention_days: u32,
    pub connections: Vec<String>,
    pub selectors: Vec<crate::mcp::selector::SelectorRule>,
    pub tool_rules: Vec<ToolRule>,
}

impl McpProfile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            enabled: false,
            persistent_access: PersistentAccess::ReadOnly,
            limits: McpLimits::default(),
            query_mode: QueryMode::StructuredOnly,
            audit_retention_days: 30,
            connections: Vec::new(),
            selectors: Vec::new(),
            tool_rules: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.name.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::Configuration,
                "profile name is required",
            ));
        }
        validate_limits(&self.limits)?;
        if self.audit_retention_days == 0 || self.audit_retention_days > 3650 {
            return Err(AppError::new(
                ErrorCategory::Configuration,
                "audit retention must be 1..=3650 days",
            ));
        }
        for rule in &self.tool_rules {
            if rule.tool == "*" || rule.tool.eq_ignore_ascii_case("all") {
                return Err(AppError::new(
                    ErrorCategory::McpPolicy,
                    "wildcard capabilities are not allowed",
                ));
            }
        }
        Ok(())
    }

    pub fn column_isolation(&self) -> bool {
        self.selectors
            .iter()
            .any(|rule| rule.selector.column.is_some())
    }

    pub fn tool_allowed(&self, name: &str) -> bool {
        if name == "*" {
            return false;
        }
        match self.tool_rules.iter().find(|rule| rule.tool == name) {
            Some(rule) => rule.allowed,
            None => true,
        }
    }
}

fn validate_limits(limits: &McpLimits) -> Result<(), AppError> {
    let overflow = limits.max_rows > i64::MAX as u64
        || limits.max_bytes > i64::MAX as u64
        || u64::from(limits.max_concurrency) > i64::MAX as u64;
    if overflow
        || limits.max_rows == 0
        || limits.max_bytes == 0
        || limits.timeout_secs == 0
        || limits.max_concurrency == 0
        || limits.max_rows > 1_000_000
        || limits.max_bytes > 32 * 1024 * 1024
        || limits.timeout_secs > 3600
        || limits.max_concurrency > 32
    {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            "MCP limits are invalid",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{McpLimits, McpProfile, PersistentAccess, ToolRule};

    #[test]
    fn new_profile_is_disabled_and_read_only() {
        let profile = McpProfile::new("assistant");
        assert!(!profile.enabled);
        assert_eq!(profile.persistent_access, PersistentAccess::ReadOnly);
    }

    #[test]
    fn rejects_wildcard_capabilities_and_zero_limits() {
        let mut profile = McpProfile::new("bad");
        profile.tool_rules.push(ToolRule {
            tool: "*".into(),
            allowed: true,
        });
        assert!(profile.validate().is_err());
        profile.tool_rules.clear();
        profile.limits = McpLimits {
            max_rows: 0,
            ..McpLimits::default()
        };
        assert!(profile.validate().is_err());
    }
}
