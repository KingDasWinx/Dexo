use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, ErrorCategory};
use crate::mcp::policy::{Decision, ObjectPolicy};
use crate::mcp::profile::McpProfile;
use crate::mcp::selector::ObjectRef;

pub const WRITE_TOOLS: &[&str] = &[
    "data_insert",
    "data_update",
    "data_delete",
    "data_execute_sql",
    "schema_apply_ddl",
    "admin_cancel_query",
    "admin_terminate_session",
];

/// What a write access may change: rows, the schema, or server sessions. Each kind
/// stands for its own tools, so DDL and admin writes need their own access (MCP-009,
/// MCP-011).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WriteKind {
    Rows,
    Schema,
    Sessions,
}

impl WriteKind {
    pub const ALL: [WriteKind; 3] = [Self::Rows, Self::Schema, Self::Sessions];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rows => "rows",
            Self::Schema => "schema",
            Self::Sessions => "sessions",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value.trim() {
            "rows" => Ok(Self::Rows),
            "schema" => Ok(Self::Schema),
            "sessions" => Ok(Self::Sessions),
            "all" | "*" => Err(AppError::new(
                ErrorCategory::McpPolicy,
                "name what it may change: rows, schema or sessions",
            )),
            other => Err(AppError::new(
                ErrorCategory::Configuration,
                format!("'{other}' is not something to change: use rows, schema or sessions"),
            )),
        }
    }

    pub fn tools(self) -> &'static [&'static str] {
        match self {
            Self::Rows => &[
                "data_insert",
                "data_update",
                "data_delete",
                "data_execute_sql",
            ],
            Self::Schema => &["schema_apply_ddl"],
            Self::Sessions => &["admin_cancel_query", "admin_terminate_session"],
        }
    }

    pub fn of_tool(tool: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.tools().contains(&tool))
    }
}

/// Why a write access cannot give `profile` the write tool `tool`, or `None` when it can:
/// the profile denies it, so the server never offers it.
pub fn why_not_grantable(profile: &McpProfile, tool: &str) -> Option<String> {
    (!profile.tool_allowed(tool)).then(|| {
        format!(
            "{} denies it: dexo mcp profile set --name {} --allow-tool {tool}",
            profile.name, profile.name
        )
    })
}

/// The write tools a write access can give `profile`, in order.
pub fn grantable_tools(profile: &McpProfile) -> Vec<&'static str> {
    WRITE_TOOLS
        .iter()
        .copied()
        .filter(|tool| why_not_grantable(profile, tool).is_none())
        .collect()
}

pub const DEFAULT_TTL_SECS: i64 = 15 * 60;
pub const MAX_TTL_SECS: i64 = 24 * 60 * 60;
/// A grant that lasts until it is revoked: `--expires never`. Its `expires_at` is the
/// furthest time there is, so every check that a grant has not expired holds.
pub const UNTIL_REVOKED: i64 = i64::MAX;

/// A profile's write access to one connection: the tools of the kinds it was given, until
/// it expires or is revoked. Every write it covers waits up to `ask_secs` for a person to
/// approve it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub id: Uuid,
    pub profile: String,
    pub connection: String,
    pub tools: Vec<String>,
    pub expires_at: i64,
    pub revision: u64,
    pub revoked: bool,
    pub ask_secs: u32,
}

impl Grant {
    /// Write access for `profile` to `connection`, of `kinds`, for `ttl_secs` (or until
    /// revoked), each write waiting up to `ask_secs` (1 s to an hour) for a person. A tool
    /// the profile denies is left out; a kind left with none is refused.
    pub fn new(
        profile: &McpProfile,
        connection: impl Into<String>,
        kinds: &[WriteKind],
        now: i64,
        ttl_secs: i64,
        ask_secs: u32,
    ) -> Result<Self, AppError> {
        if kinds.is_empty() {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "name what it may change: rows, schema or sessions",
            ));
        }
        let mut tools = Vec::new();
        for kind in kinds {
            let allowed: Vec<String> = kind
                .tools()
                .iter()
                .filter(|tool| profile.tool_allowed(tool))
                .map(|tool| tool.to_string())
                .collect();
            if allowed.is_empty() {
                return Err(AppError::new(
                    ErrorCategory::McpPolicy,
                    format!(
                        "{} denies every tool that changes {}",
                        profile.name,
                        kind.as_str()
                    ),
                ));
            }
            for tool in allowed {
                if !tools.contains(&tool) {
                    tools.push(tool);
                }
            }
        }
        if ttl_secs <= 0 || (ttl_secs > MAX_TTL_SECS && ttl_secs != UNTIL_REVOKED) {
            return Err(AppError::new(
                ErrorCategory::Configuration,
                "a grant lasts from 1 second to 24 hours, or until revoked: write the time as 90s, 15m, 2h, 1h30m or never",
            ));
        }
        Ok(Self {
            id: Uuid::new_v4(),
            profile: profile.name.clone(),
            connection: connection.into(),
            tools,
            expires_at: now.saturating_add(ttl_secs),
            revision: 1,
            revoked: false,
            ask_secs: ask_secs.clamp(1, crate::mcp::approval::MAX_TIMEOUT_SECS),
        })
    }

    /// Made with `--expires never`: it ends only when it is revoked.
    pub fn until_revoked(&self) -> bool {
        self.expires_at == UNTIL_REVOKED
    }

    /// The kinds its tools are of, in order.
    pub fn kinds(&self) -> Vec<WriteKind> {
        WriteKind::ALL
            .into_iter()
            .filter(|kind| {
                kind.tools()
                    .iter()
                    .any(|tool| self.tools.iter().any(|t| t == tool))
            })
            .collect()
    }

    pub fn active(&self, now: i64) -> bool {
        self.expires_at > now && !self.revoked
    }

    /// A grant narrows the profile; it can never widen it. It covers what the profile sees
    /// on its one connection, the profile's deny rules included; a grant for one connection
    /// is useless on another. Ending a session targets a server session, not an object,
    /// so it is bounded by the connection alone.
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
            && (WriteKind::of_tool(tool) == Some(WriteKind::Sessions)
                || profile.decide(target) == Decision::Allow)
    }
}

/// How long a grant lasts, as `90s`, `15m`, `2h`, `1d` or joined, `1h30m`, or `never`:
/// until it is revoked. A number with no unit is refused: `15` read as seconds gave a
/// grant that ended before it was used to someone who meant minutes.
pub fn parse_ttl(spec: &str) -> Result<i64, AppError> {
    let bad = || {
        AppError::new(
            ErrorCategory::Configuration,
            format!("'{spec}' is not a time: write it as 90s, 15m, 2h, 1h30m or never"),
        )
    };
    if spec.trim().eq_ignore_ascii_case("never") {
        return Ok(UNTIL_REVOKED);
    }
    let mut total: i64 = 0;
    let mut digits = String::new();
    for ch in spec.trim().chars() {
        let unit = match ch {
            '0'..='9' => {
                digits.push(ch);
                continue;
            }
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86_400,
            _ => return Err(bad()),
        };
        let count: i64 = digits.parse().map_err(|_| bad())?;
        digits.clear();
        total = total.saturating_add(count.saturating_mul(unit));
    }
    if !digits.is_empty() || total == 0 {
        return Err(bad());
    }
    Ok(total)
}

/// Write access as a person asks for it: `dexo mcp grant create` and the TUI's Writes
/// take the same fields and make the same grant.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GrantRequest {
    pub connection: String,
    pub kinds: Vec<WriteKind>,
    /// How long it lasts, as `15m` or `2h`, or `never`: until it is revoked.
    pub expires: String,
    /// How long each write waits for a person; the default without it.
    pub ask_secs: Option<u32>,
}

impl GrantRequest {
    /// The grant, once the connection is one the profile may use and one that accepts
    /// writes at all. `saved` is the connection named.
    pub fn issue(
        &self,
        profile: &McpProfile,
        saved: &crate::connection_profile::ConnectionProfile,
        now: i64,
    ) -> Result<Grant, AppError> {
        if self.connection.trim().is_empty() {
            return Err(AppError::new(
                ErrorCategory::Configuration,
                "name the connection it may write to",
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
                format!(
                    "the profile {} does not use the connection {} (it uses {})",
                    profile.name,
                    self.connection,
                    profile.connections.join(", ")
                ),
            ));
        }
        crate::mcp::McpConnection::from_profile(saved)?.accepts_writes()?;
        Grant::new(
            profile,
            self.connection.clone(),
            &self.kinds,
            now,
            parse_ttl(&self.expires)?,
            self.ask_secs
                .unwrap_or(crate::mcp::approval::DEFAULT_TIMEOUT_SECS),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_TTL_SECS, Grant, MAX_TTL_SECS, WriteKind, parse_ttl};
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

    fn rows(now: i64, ttl: i64) -> Result<Grant, crate::AppError> {
        Grant::new(&profile(), "local", &[WriteKind::Rows], now, ttl, 120)
    }

    #[test]
    fn default_ttl_is_15m_and_hard_max_24h() {
        assert_eq!(DEFAULT_TTL_SECS, 15 * 60);
        assert_eq!(MAX_TTL_SECS, 24 * 60 * 60);
        assert_eq!(parse_ttl("15m").unwrap(), DEFAULT_TTL_SECS);
        assert_eq!(parse_ttl("90s").unwrap(), 90);
        assert_eq!(parse_ttl("1d").unwrap(), MAX_TTL_SECS);
        assert_eq!(parse_ttl("1h30m").unwrap(), 5400);
        assert_eq!(parse_ttl("never").unwrap(), super::UNTIL_REVOKED);
        for refused in ["15", "", "m", "1.5h", "15 min", "abc"] {
            assert!(parse_ttl(refused).is_err(), "{refused:?}");
        }
        assert!(rows(0, MAX_TTL_SECS + 1).is_err());
    }

    /// The CLI's and the TUI's write access: inside the profile's connections, every write
    /// asking, for a while or until revoked.
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
            kinds: vec![WriteKind::Rows, WriteKind::Schema],
            expires: "15m".into(),
            ask_secs: Some(90),
        };
        let grant = request.issue(&profile(), &saved, 0).unwrap();
        assert_eq!(grant.ask_secs, 90);
        assert_eq!(grant.expires_at, DEFAULT_TTL_SECS);
        assert_eq!(grant.kinds(), [WriteKind::Rows, WriteKind::Schema]);
        assert!(grant.tools.iter().any(|tool| tool == "data_execute_sql"));
        assert!(grant.tools.iter().any(|tool| tool == "schema_apply_ddl"));
        let default_wait = GrantRequest {
            ask_secs: None,
            ..request.clone()
        }
        .issue(&profile(), &saved, 0)
        .unwrap();
        assert_eq!(default_wait.ask_secs, 120);
        let mut elsewhere = profile();
        elsewhere.connections = vec!["staging".into()];
        assert!(request.issue(&elsewhere, &saved, 0).is_err());
        let nothing = GrantRequest {
            kinds: Vec::new(),
            ..request.clone()
        };
        assert!(nothing.issue(&profile(), &saved, 0).is_err());
        let forever = GrantRequest {
            expires: "never".into(),
            ..request.clone()
        }
        .issue(&profile(), &saved, 0)
        .unwrap();
        assert!(forever.until_revoked() && forever.active(i64::MAX - 1));
    }

    /// Each write waits at least a second and at most an hour.
    #[test]
    fn the_approval_wait_is_bounded() {
        let wait = |secs| {
            Grant::new(&profile(), "local", &[WriteKind::Rows], 0, 60, secs)
                .unwrap()
                .ask_secs
        };
        assert_eq!(wait(0), 1);
        assert_eq!(wait(120), 120);
        assert_eq!(wait(u32::MAX), 3600);
    }

    #[test]
    fn kinds_are_independent_and_named() {
        assert_eq!(WriteKind::of_tool("data_insert"), Some(WriteKind::Rows));
        assert_eq!(
            WriteKind::of_tool("schema_apply_ddl"),
            Some(WriteKind::Schema)
        );
        assert_eq!(
            WriteKind::of_tool("admin_terminate_session"),
            Some(WriteKind::Sessions)
        );
        assert!(WriteKind::parse("all").is_err());
        assert!(WriteKind::parse("*").is_err());
        assert!(WriteKind::parse("ddl").is_err());
        let grant = rows(0, DEFAULT_TTL_SECS).unwrap();
        let target = ObjectRef::parse("db.public.items");
        let policy = ObjectPolicy::new(profile().selectors);
        assert!(!grant.authorizes("schema_apply_ddl", "local", &target, &policy, 0));
        assert!(!grant.authorizes("admin_cancel_query", "local", &target, &policy, 0));
    }

    /// It covers what the profile sees on its connection, while it lasts, and no more.
    #[test]
    fn a_grant_reaches_only_what_the_profile_sees() {
        let grant = rows(10, DEFAULT_TTL_SECS).unwrap();
        let target = ObjectRef::parse("db.public.items");
        let policy = ObjectPolicy::new(profile().selectors);
        assert!(grant.authorizes("data_insert", "local", &target, &policy, 10));
        assert!(!grant.authorizes(
            "data_insert",
            "local",
            &ObjectRef::parse("db.public.secrets"),
            &policy,
            10
        ));
        assert!(!grant.authorizes(
            "data_insert",
            "local",
            &ObjectRef::parse("db.other.t"),
            &policy,
            10
        ));
        assert!(!grant.authorizes("data_insert", "local", &target, &policy, grant.expires_at));
        assert!(!grant.authorizes("data_insert", "other", &target, &policy, 10));
        let mut revoked = grant.clone();
        revoked.revoked = true;
        assert!(!revoked.authorizes("data_insert", "local", &target, &policy, 10));
    }

    /// A tool the profile denies is left out of the grant; a kind with none left is
    /// refused.
    #[test]
    fn a_grant_gives_only_what_the_profile_lets_it() {
        use super::{grantable_tools, why_not_grantable};
        let mut profile = profile();
        assert_eq!(grantable_tools(&profile).len(), super::WRITE_TOOLS.len());
        profile.tool_rules.push(ToolRule {
            tool: "data_delete".into(),
            allowed: false,
        });
        assert!(!grantable_tools(&profile).contains(&"data_delete"));
        assert!(why_not_grantable(&profile, "data_delete").is_some());
        let grant = Grant::new(&profile, "local", &[WriteKind::Rows], 0, 60, 120).unwrap();
        assert!(!grant.tools.iter().any(|tool| tool == "data_delete"));
        profile.tool_rules.push(ToolRule {
            tool: "schema_apply_ddl".into(),
            allowed: false,
        });
        assert!(Grant::new(&profile, "local", &[WriteKind::Schema], 0, 60, 120).is_err());
    }
}
