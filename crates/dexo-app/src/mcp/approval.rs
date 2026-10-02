//! A write an agent asked to make under an asking grant, waiting for a person. The MCP
//! server writes it and waits; the TUI's Agent Activity screen reads it and decides.
//! The two processes meet only in the shared database.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;
use crate::mcp::ledger::GrantLedger;

/// How long a write waits for a decision when the grant does not say.
pub const DEFAULT_TIMEOUT_SECS: u32 = 120;

/// The longest a write may wait for a decision: an agent's call stays open all along.
pub const MAX_TIMEOUT_SECS: u32 = 3600;

/// A waiting call says it is still there every second. One silent this long has gone
/// -- its server was killed -- and its request is taken off the list.
pub const HEARTBEAT_GRACE_SECS: i64 = 5;

/// How long a decided request stays in the database before it is deleted.
pub const KEEP_SETTLED_SECS: i64 = 24 * 60 * 60;

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
    /// The asking grant the write would run under; revoking it denies the request.
    #[serde(default)]
    pub grant: Uuid,
    /// When the waiting call last said it is still there, in Unix seconds.
    #[serde(default)]
    pub heartbeat: i64,
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
            grant: Uuid::nil(),
            heartbeat: now,
        }
    }

    /// Whole seconds left to decide, at `now`.
    pub fn seconds_left(&self, now: i64) -> i64 {
        (self.deadline - now).max(0)
    }

    /// Whether the call that asked has been silent long enough to look gone: it says it is
    /// there every second.
    pub fn seems_gone(&self, now: i64) -> bool {
        self.heartbeat < now.saturating_sub(2)
    }

    /// Whether the call that asked still waits for the answer at `now`.
    pub fn waited_on(&self, now: i64) -> bool {
        self.heartbeat >= now.saturating_sub(HEARTBEAT_GRACE_SECS)
    }
}

/// What a person's answer came to, for the screen that gave it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Answered {
    /// Taken: an approved write runs now, a denied one is refused.
    Taken,
    /// Someone answered first, or its grant was revoked, which denies it.
    AlreadyDecided,
    /// Its time ran out.
    TimedOut,
    /// The agent stopped waiting: its call was cancelled, or its server went away.
    NobodyWaiting,
}

/// Answers a waiting request. An approval is taken only while the agent's call still
/// waits for it, so "the write runs now" is never said of a write nobody will run; a
/// request found that way is settled on the spot, which takes it off every list.
pub fn answer(
    ledger: &dyn GrantLedger,
    id: Uuid,
    approve: bool,
    now: i64,
) -> Result<Answered, AppError> {
    let decision = if approve {
        ApprovalDecision::Approved
    } else {
        ApprovalDecision::Denied
    };
    if ledger.settle_approval(id, decision, now)? {
        return Ok(Answered::Taken);
    }
    let Some(request) = ledger.approval(id) else {
        return Ok(Answered::NobodyWaiting);
    };
    Ok(match request.decision {
        ApprovalDecision::Pending if request.deadline <= now => {
            ledger.settle_approval(id, ApprovalDecision::Expired, now)?;
            Answered::TimedOut
        }
        ApprovalDecision::Pending => {
            ledger.settle_approval(id, ApprovalDecision::Cancelled, now)?;
            Answered::NobodyWaiting
        }
        ApprovalDecision::Expired => Answered::TimedOut,
        ApprovalDecision::Cancelled => Answered::NobodyWaiting,
        ApprovalDecision::Approved | ApprovalDecision::Denied => Answered::AlreadyDecided,
    })
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
    use super::{
        Answered, Approval, ApprovalDecision, HEARTBEAT_GRACE_SECS, KEEP_SETTLED_SECS, answer,
    };
    use crate::mcp::grant::{Grant, GrantCapability};
    use crate::mcp::ledger::{GrantLedger, MemoryGrantLedger};
    use crate::mcp::profile::McpProfile;
    use crate::mcp::selector::{Effect, SelectorRule};

    /// An approval is taken only while the agent's call still waits; otherwise the
    /// person is told what became of the request, and it leaves the list with its SQL.
    /// A revoked grant's request cannot be approved, and decided ones are deleted later.
    #[test]
    fn an_answer_says_what_it_came_to() {
        let ledger = MemoryGrantLedger::default();
        let arguments = serde_json::json!({"sql": "UPDATE t SET a = 1"})
            .as_object()
            .cloned()
            .unwrap();
        let request = || {
            let request =
                Approval::pending("p", "c", "data_execute_sql", &arguments, vec![], 100, 60);
            ledger.request_approval(&request).unwrap();
            request
        };
        let live = request();
        assert_eq!(
            answer(&ledger, live.id, true, 101).unwrap(),
            Answered::Taken
        );
        assert_eq!(
            answer(&ledger, live.id, false, 102).unwrap(),
            Answered::AlreadyDecided
        );

        let silent = request();
        let later = 100 + HEARTBEAT_GRACE_SECS + 1;
        assert_eq!(
            answer(&ledger, silent.id, true, later).unwrap(),
            Answered::NobodyWaiting
        );
        let settled = ledger.approval(silent.id).unwrap();
        assert_eq!(settled.decision, ApprovalDecision::Cancelled);
        assert!(settled.statement.is_empty());

        let touched = request();
        ledger.touch_approval(touched.id, later);
        assert_eq!(
            answer(&ledger, touched.id, true, later).unwrap(),
            Answered::Taken
        );

        let late = request();
        ledger.touch_approval(late.id, 160);
        assert_eq!(
            answer(&ledger, late.id, true, 160).unwrap(),
            Answered::TimedOut
        );

        let mut profile = McpProfile::new("p");
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
        let grant = Grant::new(
            &profile,
            "c",
            GrantCapability::DataWrite,
            vec!["data_insert".into()],
            vec![SelectorRule::parse(Effect::Allow, "db.public.t").unwrap()],
            100,
            900,
        )
        .unwrap()
        .asking(60);
        ledger.insert_grant(grant.clone()).unwrap();
        let mut revoked = Approval::pending("p", "c", "data_insert", &arguments, vec![], 100, 60);
        revoked.grant = grant.id;
        ledger.request_approval(&revoked).unwrap();
        assert_eq!(ledger.pending_approvals(101).len(), 1);
        ledger.revoke(grant.id).unwrap();
        assert!(ledger.pending_approvals(101).is_empty());
        assert_eq!(
            answer(&ledger, revoked.id, true, 101).unwrap(),
            Answered::AlreadyDecided
        );

        ledger.sweep_approvals(160 + KEEP_SETTLED_SECS + 1);
        assert!(ledger.approval(live.id).is_none());
    }

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
