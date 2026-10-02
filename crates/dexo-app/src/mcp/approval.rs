//! A write an agent asked to make under an asking grant, waiting for a person. The MCP
//! server writes it and waits; the TUI's Agent Activity screen reads it and decides.
//! The two processes meet only in the shared database.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How long a write waits for a decision when the grant does not say.
pub const DEFAULT_TIMEOUT_SECS: u32 = 120;

/// The longest a write may wait for a decision: an agent's call stays open all along.
pub const MAX_TIMEOUT_SECS: u32 = 3600;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ApprovalDecision {
    Pending,
    Approved,
    Denied,
    Expired,
    /// The agent stopped waiting: it cancelled the call.
    Cancelled,
}

impl ApprovalDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }

    /// A value Dexo never writes reads as denied: read as pending, it could never be
    /// settled, and the server asked again forever.
    pub fn parse(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "approved" => Self::Approved,
            "expired" => Self::Expired,
            "cancelled" => Self::Cancelled,
            _ => Self::Denied,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    pub id: Uuid,
    pub profile: String,
    pub connection: String,
    pub tool: String,
    /// What the write would do, for the person deciding: the SQL or DDL, or the call's
    /// arguments for a structured write. Blanked once the request is decided.
    pub statement: String,
    pub targets: Vec<String>,
    pub created_at: i64,
    pub deadline: i64,
    pub decision: ApprovalDecision,
}

impl Approval {
    pub fn pending(
        profile: &str,
        connection: &str,
        tool: &str,
        arguments: &serde_json::Map<String, serde_json::Value>,
        targets: Vec<String>,
        now: i64,
        timeout_secs: u32,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            profile: profile.into(),
            connection: connection.into(),
            tool: tool.into(),
            statement: statement_of(arguments),
            targets,
            created_at: now,
            deadline: now.saturating_add(i64::from(timeout_secs)),
            decision: ApprovalDecision::Pending,
        }
    }

    /// Whole seconds left to decide, at `now`.
    pub fn seconds_left(&self, now: i64) -> i64 {
        (self.deadline - now).max(0)
    }
}

/// The SQL a call carries, or its arguments in one line when it carries none.
fn statement_of(arguments: &serde_json::Map<String, serde_json::Value>) -> String {
    if let Some(sql) = arguments.get("sql").and_then(serde_json::Value::as_str) {
        return sql.trim().to_string();
    }
    let shown: serde_json::Map<String, serde_json::Value> = arguments
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "operation_id" | "connection"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    serde_json::Value::Object(shown).to_string()
}

#[cfg(test)]
mod tests {
    use super::ApprovalDecision;

    /// Only what Dexo writes reads back as itself; anything else stops the wait as denied.
    #[test]
    fn an_unknown_decision_is_a_denial() {
        for decision in [
            ApprovalDecision::Pending,
            ApprovalDecision::Approved,
            ApprovalDecision::Denied,
            ApprovalDecision::Expired,
            ApprovalDecision::Cancelled,
        ] {
            assert_eq!(ApprovalDecision::parse(decision.as_str()), decision);
        }
        assert_eq!(ApprovalDecision::parse("maybe"), ApprovalDecision::Denied);
        assert_eq!(ApprovalDecision::parse(""), ApprovalDecision::Denied);
    }
}
