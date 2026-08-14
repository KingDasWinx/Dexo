use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, ErrorCategory};
use crate::mcp::policy::{Decision, ObjectPolicy};
use crate::mcp::profile::McpProfile;
use crate::mcp::selector::{ObjectRef, SelectorRule};

pub const WRITE_TOOLS: &[&str] = &[
    "data_insert",
    "data_update",
    "data_delete",
    "data_execute_sql",
    "schema_apply_ddl",
    "admin_cancel_query",
    "admin_terminate_session",
];

pub const DEFAULT_TTL_SECS: i64 = 15 * 60;
pub const MAX_TTL_SECS: i64 = 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum GrantCapability {
    DataWrite,
    Ddl,
    Admin,
}

impl GrantCapability {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "data_write" => Ok(Self::DataWrite),
            "ddl" => Ok(Self::Ddl),
            "admin" => Ok(Self::Admin),
            "all" => Err(AppError::new(
                ErrorCategory::McpPolicy,
                "wildcard capabilities are not allowed",
            )),
            _ => Err(AppError::new(
                ErrorCategory::Configuration,
                "unknown grant capability",
            )),
        }
    }

    pub fn allows_tool(self, tool: &str) -> bool {
        matches!(
            (self, tool),
            (
                Self::DataWrite,
                "data_insert" | "data_update" | "data_delete" | "data_execute_sql",
            ) | (Self::Ddl, "schema_apply_ddl")
                | (
                    Self::Admin,
                    "admin_cancel_query" | "admin_terminate_session"
                )
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub id: Uuid,
    pub profile: String,
    pub connection: String,
    pub selectors: Vec<SelectorRule>,
    pub tools: Vec<String>,
    pub capability: GrantCapability,
    pub expires_at: i64,
    pub remaining_uses: u32,
    pub revision: u64,
    pub revoked: bool,
}

impl Grant {
    pub fn new(
        profile: &McpProfile,
        connection: impl Into<String>,
        capability: GrantCapability,
        tools: Vec<String>,
        selectors: Vec<SelectorRule>,
        now: i64,
        ttl_secs: i64,
    ) -> Result<Self, AppError> {
        if tools.is_empty() || tools.iter().any(|tool| tool == "all" || tool == "*") {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "grant tools must be explicit",
            ));
        }
        for tool in &tools {
            if !WRITE_TOOLS.contains(&tool.as_str()) || !capability.allows_tool(tool) {
                return Err(AppError::new(
                    ErrorCategory::McpPolicy,
                    format!("tool {tool} is not valid for this capability"),
                ));
            }
            if tool == "data_execute_sql"
                && !profile
                    .tool_rules
                    .iter()
                    .any(|rule| rule.tool == "data_execute_sql" && rule.allowed)
            {
                return Err(AppError::new(
                    ErrorCategory::McpPolicy,
                    "data_execute_sql requires an explicit profile tool rule",
                ));
            }
        }
        if ttl_secs <= 0 || ttl_secs > MAX_TTL_SECS {
            return Err(AppError::new(
                ErrorCategory::Configuration,
                "grant ttl must be 1s..=24h",
            ));
        }
        let policy = ObjectPolicy::new(profile.selectors.clone());
        for rule in &selectors {
            let sample = sample_object(&rule.selector);
            if policy.decide(&sample) != Decision::Allow {
                return Err(AppError::new(
                    ErrorCategory::McpPolicy,
                    "grant scope cannot be broader than the profile",
                ));
            }
        }
        Ok(Self {
            id: Uuid::new_v4(),
            profile: profile.name.clone(),
            connection: connection.into(),
            selectors,
            tools,
            capability,
            expires_at: now.saturating_add(ttl_secs),
            remaining_uses: 1,
            revision: 1,
            revoked: false,
        })
    }

    pub fn active(&self, now: i64) -> bool {
        self.remaining_uses > 0 && self.expires_at > now && !self.revoked
    }

    pub fn authorizes(&self, tool: &str, target: &ObjectRef, now: i64) -> bool {
        self.active(now)
            && self.tools.iter().any(|name| name == tool)
            && self.capability.allows_tool(tool)
            && ObjectPolicy::new(self.selectors.clone()).decide(target) == Decision::Allow
    }
}

pub fn parse_ttl(spec: &str) -> Result<i64, AppError> {
    if let Some(mins) = spec.strip_suffix('m') {
        let mins: i64 = mins
            .parse()
            .map_err(|_| AppError::new(ErrorCategory::Configuration, "invalid ttl"))?;
        return Ok(mins.saturating_mul(60));
    }
    if let Some(hours) = spec.strip_suffix('h') {
        let hours: i64 = hours
            .parse()
            .map_err(|_| AppError::new(ErrorCategory::Configuration, "invalid ttl"))?;
        return Ok(hours.saturating_mul(3600));
    }
    spec.parse::<i64>()
        .map_err(|_| AppError::new(ErrorCategory::Configuration, "invalid ttl"))
}

fn sample_object(selector: &crate::mcp::selector::Selector) -> ObjectRef {
    fn part(seg: Option<&crate::mcp::selector::Segment>) -> String {
        match seg {
            Some(crate::mcp::selector::Segment::Exact(name)) => name.clone(),
            _ => "probe".into(),
        }
    }
    ObjectRef {
        catalog: Some(part(selector.catalog.as_ref())),
        schema: Some(part(selector.schema.as_ref())),
        name: part(selector.object.as_ref()),
        column: selector.column.as_ref().map(|seg| part(Some(seg))),
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_TTL_SECS, Grant, GrantCapability, MAX_TTL_SECS, parse_ttl};
    use crate::mcp::profile::{McpProfile, ToolRule};
    use crate::mcp::selector::{Effect, ObjectRef, SelectorRule};

    fn profile() -> McpProfile {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![
            SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
            SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
        ];
        profile
    }

    #[test]
    fn default_ttl_is_15m_and_hard_max_24h() {
        assert_eq!(DEFAULT_TTL_SECS, 15 * 60);
        assert_eq!(MAX_TTL_SECS, 24 * 60 * 60);
        assert_eq!(parse_ttl("15m").unwrap(), DEFAULT_TTL_SECS);
        assert!(
            Grant::new(
                &profile(),
                "local",
                GrantCapability::DataWrite,
                vec!["data_insert".into()],
                vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
                0,
                MAX_TTL_SECS + 1,
            )
            .is_err()
        );
    }

    #[test]
    fn capabilities_are_independent() {
        assert!(GrantCapability::DataWrite.allows_tool("data_insert"));
        assert!(!GrantCapability::DataWrite.allows_tool("schema_apply_ddl"));
        assert!(!GrantCapability::Ddl.allows_tool("admin_terminate_session"));
        assert!(!GrantCapability::Admin.allows_tool("data_insert"));
    }

    #[test]
    fn rejects_all_empty_tools_and_broader_scope() {
        let profile = profile();
        assert!(
            Grant::new(
                &profile,
                "local",
                GrantCapability::DataWrite,
                vec![],
                vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
                0,
                DEFAULT_TTL_SECS,
            )
            .is_err()
        );
        assert!(
            Grant::new(
                &profile,
                "local",
                GrantCapability::DataWrite,
                vec!["all".into()],
                vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
                0,
                DEFAULT_TTL_SECS,
            )
            .is_err()
        );
        assert!(
            Grant::new(
                &profile,
                "local",
                GrantCapability::DataWrite,
                vec!["data_insert".into()],
                vec![SelectorRule::parse(Effect::Allow, "db.*").unwrap()],
                0,
                DEFAULT_TTL_SECS,
            )
            .is_err()
        );
    }

    #[test]
    fn one_use_grant_authorizes_until_consumed() {
        let grant = Grant::new(
            &profile(),
            "local",
            GrantCapability::DataWrite,
            vec!["data_insert".into()],
            vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
            10,
            DEFAULT_TTL_SECS,
        )
        .unwrap();
        assert_eq!(grant.remaining_uses, 1);
        let target = ObjectRef::parse("db.public.items");
        assert!(grant.authorizes("data_insert", &target, 10));
        assert!(!grant.authorizes("schema_apply_ddl", &target, 10));
        assert!(!grant.authorizes("data_insert", &ObjectRef::parse("db.public.secrets"), 10));
        assert!(!grant.authorizes("data_insert", &target, grant.expires_at));
    }

    #[test]
    fn data_execute_sql_needs_profile_tool_rule() {
        let mut profile = profile();
        assert!(
            Grant::new(
                &profile,
                "local",
                GrantCapability::DataWrite,
                vec!["data_execute_sql".into()],
                vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
                0,
                DEFAULT_TTL_SECS,
            )
            .is_err()
        );
        profile.tool_rules.push(ToolRule {
            tool: "data_execute_sql".into(),
            allowed: true,
        });
        assert!(
            Grant::new(
                &profile,
                "local",
                GrantCapability::DataWrite,
                vec!["data_execute_sql".into()],
                vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
                0,
                DEFAULT_TTL_SECS,
            )
            .is_ok()
        );
    }
}
