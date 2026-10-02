use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, ErrorCategory};
use crate::mcp::policy::{Decision, ObjectPolicy};
use crate::mcp::profile::McpProfile;
use crate::mcp::selector::{Effect, ObjectRef, Segment, Selector, SelectorRule};

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
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DataWrite => "data_write",
            Self::Ddl => "ddl",
            Self::Admin => "admin",
        }
    }

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
    /// Above zero, the grant asks: each write it covers waits up to this many seconds
    /// for a person to approve it, and the grant is not spent by one.
    #[serde(default)]
    pub ask_secs: u32,
}

impl Grant {
    /// The grant made to ask, every write it covers waiting up to `timeout_secs` (1 s to
    /// an hour) for a person's decision. An asking grant lasts until it expires.
    pub fn asking(mut self, timeout_secs: u32) -> Self {
        self.ask_secs = timeout_secs.clamp(1, crate::mcp::approval::MAX_TIMEOUT_SECS);
        self.remaining_uses = u32::MAX;
        self
    }

    pub fn asks(&self) -> bool {
        self.ask_secs > 0
    }

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
            ask_secs: 0,
        })
    }

    pub fn active(&self, now: i64) -> bool {
        self.remaining_uses > 0 && self.expires_at > now && !self.revoked
    }

    /// A grant narrows the profile; it can never widen it. The profile's deny rules still
    /// apply inside the grant's scope, and a grant issued for one connection is useless
    /// on another. Admin actions target a server session, not an object, so an admin
    /// grant is bounded by its connection alone.
    pub fn authorizes(
        &self,
        tool: &str,
        connection: &str,
        target: &ObjectRef,
        profile: &ObjectPolicy,
        now: i64,
    ) -> bool {
        self.active(now)
            && self.connection == connection
            && self.tools.iter().any(|name| name == tool)
            && self.capability.allows_tool(tool)
            && (self.capability == GrantCapability::Admin
                || (ObjectPolicy::new(self.selectors.clone()).decide(target) == Decision::Allow
                    && profile.decide(target) == Decision::Allow))
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

/// A grant as a person asks for one: `dexo mcp grant create` and the TUI's New MCP Grant
/// take the same fields and make the same grant.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GrantRequest {
    pub connection: String,
    pub capability: String,
    pub tools: Vec<String>,
    pub selector: String,
    /// How long the grant lasts, as `15m`, `2h` or seconds.
    pub expires: String,
    /// The connection or the selector, typed again to confirm.
    pub confirm_target: String,
    /// Each write the grant covers waits up to this many seconds for a person to
    /// approve it; without it, the grant is spent by one write.
    pub ask_secs: Option<u32>,
}

impl GrantRequest {
    /// The grant, once the target was typed again and the connection is one the profile
    /// may use and one that accepts writes at all. `saved` is the connection named.
    pub fn issue(
        &self,
        profile: &McpProfile,
        saved: &crate::connection_profile::ConnectionProfile,
        now: i64,
    ) -> Result<Grant, AppError> {
        if self.confirm_target != self.connection && self.confirm_target != self.selector {
            return Err(AppError::new(
                ErrorCategory::Configuration,
                "type the connection or the selector to confirm",
            ));
        }
        if !profile.connections.is_empty()
            && !profile
                .connections
                .iter()
                .any(|name| name == &self.connection)
        {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "connection is not allowed for this profile",
            ));
        }
        crate::mcp::McpConnection::from_profile(saved)?.accepts_writes()?;
        let grant = Grant::new(
            profile,
            self.connection.clone(),
            GrantCapability::parse(&self.capability)?,
            self.tools.clone(),
            vec![SelectorRule::parse(Effect::Allow, &self.selector)?],
            now,
            parse_ttl(&self.expires)?,
        )?;
        Ok(match self.ask_secs {
            Some(secs) => grant.asking(secs),
            None => grant,
        })
    }
}

fn sample_object(selector: &Selector) -> ObjectRef {
    ObjectRef {
        path: selector
            .segments
            .iter()
            .map(|segment| match segment {
                Segment::Exact(name) => name.clone(),
                Segment::Star => "probe".into(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_TTL_SECS, Grant, GrantCapability, MAX_TTL_SECS, parse_ttl};
    use crate::mcp::policy::ObjectPolicy;
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

    /// The CLI's and the TUI's grant: confirmed by typing the target, inside the
    /// profile's connections, and asking before each write when told to.
    #[test]
    fn a_grant_request_makes_the_grant_the_cli_makes() {
        use super::GrantRequest;
        let saved = crate::connection_profile::ConnectionProfile::new(
            crate::ConnectionId(uuid::Uuid::from_u128(1)),
            None,
            "local",
            "postgres",
            "local",
            serde_json::json!({"host": "h", "port": 5432, "username": "u", "database": "db"}),
            crate::SecretRef::new("r".into()),
        );
        let request = GrantRequest {
            connection: "local".into(),
            capability: "data_write".into(),
            tools: vec!["data_insert".into()],
            selector: "db.public.items".into(),
            expires: "15m".into(),
            confirm_target: "local".into(),
            ask_secs: Some(90),
        };
        let asking = request.issue(&profile(), &saved, 0).unwrap();
        assert!(asking.asks());
        assert_eq!(asking.ask_secs, 90);
        assert_eq!(asking.expires_at, DEFAULT_TTL_SECS);
        let once = GrantRequest {
            ask_secs: None,
            ..request.clone()
        }
        .issue(&profile(), &saved, 0)
        .unwrap();
        assert!(!once.asks());
        assert_eq!(once.remaining_uses, 1);
        let unconfirmed = GrantRequest {
            confirm_target: "loca".into(),
            ..request.clone()
        };
        assert!(unconfirmed.issue(&profile(), &saved, 0).is_err());
        let mut elsewhere = profile();
        elsewhere.connections = vec!["staging".into()];
        assert!(request.issue(&elsewhere, &saved, 0).is_err());
    }

    /// An asking grant waits at least a second and at most an hour per write.
    #[test]
    fn the_approval_wait_is_bounded() {
        let grant = || {
            Grant::new(
                &profile(),
                "local",
                GrantCapability::DataWrite,
                vec!["data_insert".into()],
                vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
                0,
                DEFAULT_TTL_SECS,
            )
            .unwrap()
        };
        assert_eq!(grant().asking(0).ask_secs, 1);
        assert_eq!(grant().asking(120).ask_secs, 120);
        assert_eq!(grant().asking(u32::MAX).ask_secs, 3600);
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
        let policy = ObjectPolicy::new(profile().selectors);
        assert!(grant.authorizes("data_insert", "local", &target, &policy, 10));
        assert!(!grant.authorizes("schema_apply_ddl", "local", &target, &policy, 10));
        assert!(!grant.authorizes(
            "data_insert",
            "local",
            &ObjectRef::parse("db.public.secrets"),
            &policy,
            10
        ));
        assert!(!grant.authorizes("data_insert", "local", &target, &policy, grant.expires_at));
        assert!(!grant.authorizes("data_insert", "other", &target, &policy, 10));
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
