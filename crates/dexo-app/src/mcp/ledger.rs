use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;

use crate::error::{AppError, ErrorCategory};
use crate::mcp::approval::{Approval, ApprovalDecision, KEEP_SETTLED_SECS};
use crate::mcp::audit::AuditEvent;
use crate::mcp::grant::Grant;
use crate::mcp::operation::{
    OperationRecord, OperationState, SideEffect, payload_hash, replay_or_conflict,
};

pub trait GrantLedger: Send + Sync {
    fn active_grants(&self, profile: &str, now: i64) -> Vec<Grant>;
    /// Saves `grant` in place of what its profile had on its connection -- one write access
    /// per profile and connection -- denying the writes that waited under the old one.
    fn insert_grant(&self, grant: Grant) -> Result<(), AppError>;
    fn revoke(&self, id: Uuid) -> Result<(), AppError>;
    /// Takes back a profile's write access to one connection; how many grants went.
    fn revoke_connection(&self, profile: &str, connection: &str) -> Result<usize, AppError>;
    fn revoke_profile(&self, profile: &str) -> Result<usize, AppError>;
    fn reserve_operation(&self, record: OperationRecord) -> Result<OperationRecord, AppError>;
    fn lookup_operation(
        &self,
        profile: &str,
        session: &str,
        operation_id: &str,
    ) -> Option<OperationRecord>;
    fn finish_operation(
        &self,
        profile: &str,
        session: &str,
        operation_id: &str,
        state: OperationState,
        side_effect: SideEffect,
        result: String,
    ) -> Result<(), AppError>;
    fn record_audit(&self, event: AuditEvent);
    fn audits(&self) -> Vec<AuditEvent>;
    /// The `limit` latest events, newest first.
    fn recent_audits(&self, limit: usize) -> Vec<AuditEvent> {
        let mut events = self.audits();
        events.reverse();
        events.truncate(limit);
        events
    }
    fn prune_audits(&self, older_than: i64);
    fn is_revoked(&self, id: Uuid) -> bool;
    /// A write waiting for a person, as the server records it.
    fn request_approval(&self, approval: &Approval) -> Result<(), AppError>;
    fn approval(&self, id: Uuid) -> Option<Approval>;
    /// Decides a request that is still pending, its statement blanked; false when it was
    /// decided already. Approval is refused once the deadline has passed, or once the
    /// call that asked stopped saying it waits (see [`Approval::waited_on`]).
    fn settle_approval(
        &self,
        id: Uuid,
        decision: ApprovalDecision,
        now: i64,
    ) -> Result<bool, AppError>;
    /// The call that asked still waits for its answer, at `now`.
    fn touch_approval(&self, id: Uuid, now: i64);
    /// Settles what nobody will decide: a request past its deadline expires, one whose
    /// call went silent is cancelled -- its statement blanked either way -- and decided
    /// requests older than [`KEEP_SETTLED_SECS`] are deleted.
    fn sweep_approvals(&self, now: i64);
    /// The requests still waiting at `now`, oldest first, after a sweep.
    fn pending_approvals(&self, now: i64) -> Vec<Approval>;
}

#[derive(Default)]
pub struct MemoryGrantLedger {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    grants: Vec<Grant>,
    operations: HashMap<String, OperationRecord>,
    audits: Vec<AuditEvent>,
    approvals: Vec<Approval>,
    revision: u64,
}

impl Inner {
    /// A revoked grant's waiting writes are denied, so no one approves one of them.
    fn deny_revoked(&mut self) {
        let revoked: Vec<Uuid> = self
            .grants
            .iter()
            .filter(|grant| grant.revoked)
            .map(|grant| grant.id)
            .collect();
        for approval in &mut self.approvals {
            if approval.decision == ApprovalDecision::Pending && revoked.contains(&approval.grant) {
                approval.decision = ApprovalDecision::Denied;
                approval.statement.clear();
            }
        }
    }
}

fn op_key(profile: &str, session: &str, operation_id: &str) -> String {
    format!("{profile}\0{session}\0{operation_id}")
}

impl GrantLedger for MemoryGrantLedger {
    fn active_grants(&self, profile: &str, now: i64) -> Vec<Grant> {
        self.inner
            .lock()
            .expect("ledger")
            .grants
            .iter()
            .filter(|grant| grant.profile == profile && grant.active(now))
            .cloned()
            .collect()
    }

    fn insert_grant(&self, grant: Grant) -> Result<(), AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        for old in &mut inner.grants {
            if old.profile == grant.profile && old.connection == grant.connection {
                old.revoked = true;
            }
        }
        inner.revision += 1;
        let mut grant = grant;
        grant.revision = inner.revision;
        inner.grants.push(grant);
        inner.deny_revoked();
        Ok(())
    }

    fn revoke(&self, id: Uuid) -> Result<(), AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        if let Some(grant) = inner.grants.iter_mut().find(|grant| grant.id == id) {
            grant.revoked = true;
            inner.revision += 1;
            inner.deny_revoked();
            Ok(())
        } else {
            Err(AppError::new(ErrorCategory::McpPolicy, "not found"))
        }
    }

    fn revoke_connection(&self, profile: &str, connection: &str) -> Result<usize, AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        let mut revoked = 0;
        for grant in &mut inner.grants {
            if grant.profile == profile && grant.connection == connection && !grant.revoked {
                grant.revoked = true;
                revoked += 1;
            }
        }
        inner.revision += 1;
        inner.deny_revoked();
        Ok(revoked)
    }

    fn revoke_profile(&self, profile: &str) -> Result<usize, AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        let mut revoked = 0;
        for grant in &mut inner.grants {
            if grant.profile == profile && !grant.revoked {
                grant.revoked = true;
                revoked += 1;
            }
        }
        inner.revision += 1;
        inner.deny_revoked();
        Ok(revoked)
    }

    fn reserve_operation(&self, record: OperationRecord) -> Result<OperationRecord, AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        let key = op_key(&record.profile, &record.session, &record.operation_id);
        if let Some(existing) = inner.operations.get(&key) {
            return replay_or_conflict(existing, &record.payload_hash);
        }
        inner.operations.insert(key, record.clone());
        Ok(record)
    }

    fn lookup_operation(
        &self,
        profile: &str,
        session: &str,
        operation_id: &str,
    ) -> Option<OperationRecord> {
        self.inner
            .lock()
            .expect("ledger")
            .operations
            .get(&op_key(profile, session, operation_id))
            .cloned()
    }

    fn finish_operation(
        &self,
        profile: &str,
        session: &str,
        operation_id: &str,
        state: OperationState,
        side_effect: SideEffect,
        result: String,
    ) -> Result<(), AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        if let Some(record) = inner
            .operations
            .get_mut(&op_key(profile, session, operation_id))
        {
            record.state = state;
            record.side_effect = side_effect;
            record.result = result;
        }
        Ok(())
    }

    fn record_audit(&self, event: AuditEvent) {
        self.inner.lock().expect("ledger").audits.push(event);
    }

    fn audits(&self) -> Vec<AuditEvent> {
        self.inner.lock().expect("ledger").audits.clone()
    }

    fn prune_audits(&self, older_than: i64) {
        self.inner
            .lock()
            .expect("ledger")
            .audits
            .retain(|event| event.timestamp >= older_than);
    }

    fn is_revoked(&self, id: Uuid) -> bool {
        self.inner
            .lock()
            .expect("ledger")
            .grants
            .iter()
            .any(|grant| grant.id == id && grant.revoked)
    }

    fn request_approval(&self, approval: &Approval) -> Result<(), AppError> {
        self.inner
            .lock()
            .expect("ledger")
            .approvals
            .push(approval.clone());
        Ok(())
    }

    fn approval(&self, id: Uuid) -> Option<Approval> {
        self.inner
            .lock()
            .expect("ledger")
            .approvals
            .iter()
            .find(|approval| approval.id == id)
            .cloned()
    }

    fn settle_approval(
        &self,
        id: Uuid,
        decision: ApprovalDecision,
        now: i64,
    ) -> Result<bool, AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        let Some(approval) = inner
            .approvals
            .iter_mut()
            .find(|approval| approval.id == id)
        else {
            return Ok(false);
        };
        if approval.decision != ApprovalDecision::Pending
            || (decision == ApprovalDecision::Approved
                && (now >= approval.deadline || !approval.waited_on(now)))
        {
            return Ok(false);
        }
        approval.decision = decision;
        approval.statement.clear();
        Ok(true)
    }

    fn touch_approval(&self, id: Uuid, now: i64) {
        let mut inner = self.inner.lock().expect("ledger");
        if let Some(approval) = inner
            .approvals
            .iter_mut()
            .find(|approval| approval.id == id && approval.decision == ApprovalDecision::Pending)
        {
            approval.heartbeat = now;
        }
    }

    fn sweep_approvals(&self, now: i64) {
        let mut inner = self.inner.lock().expect("ledger");
        for approval in &mut inner.approvals {
            if approval.decision != ApprovalDecision::Pending {
                continue;
            }
            if approval.deadline <= now {
                approval.decision = ApprovalDecision::Expired;
            } else if !approval.waited_on(now) {
                approval.decision = ApprovalDecision::Cancelled;
            } else {
                continue;
            }
            approval.statement.clear();
        }
        inner.approvals.retain(|approval| {
            approval.decision == ApprovalDecision::Pending
                || approval.deadline >= now.saturating_sub(KEEP_SETTLED_SECS)
        });
    }

    fn pending_approvals(&self, now: i64) -> Vec<Approval> {
        self.sweep_approvals(now);
        self.inner
            .lock()
            .expect("ledger")
            .approvals
            .iter()
            .filter(|approval| approval.decision == ApprovalDecision::Pending)
            .cloned()
            .collect()
    }
}

pub fn hash_payload(value: &serde_json::Value) -> String {
    payload_hash(value)
}

#[cfg(test)]
mod tests {
    use super::{GrantLedger, MemoryGrantLedger};
    use crate::mcp::grant::{DEFAULT_TTL_SECS, Grant, WriteKind};
    use crate::mcp::operation::{OperationRecord, OperationState, SideEffect};
    use crate::mcp::profile::McpProfile;
    use crate::mcp::selector::{Effect, SelectorRule};
    use serde_json::json;

    fn grant(connection: &str) -> Grant {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
        Grant::new(
            &profile,
            connection,
            &[WriteKind::Rows],
            0,
            DEFAULT_TTL_SECS,
            120,
        )
        .unwrap()
    }

    /// One write access per profile and connection: a new one replaces the old, and the
    /// writes waiting under the old one are denied.
    #[test]
    fn a_new_grant_replaces_the_one_on_its_connection() {
        use crate::mcp::approval::{Approval, ApprovalDecision};
        let ledger = MemoryGrantLedger::default();
        let first = grant("local");
        ledger.insert_grant(first.clone()).unwrap();
        ledger.insert_grant(grant("other")).unwrap();
        let mut waiting = Approval::pending(
            "assistant",
            "local",
            "data_insert",
            &serde_json::Map::new(),
            vec![],
            0,
            60,
        );
        waiting.grant = first.id;
        ledger.request_approval(&waiting).unwrap();
        ledger.insert_grant(grant("local")).unwrap();
        let active = ledger.active_grants("assistant", 0);
        assert_eq!(active.len(), 2);
        assert!(active.iter().all(|grant| grant.id != first.id));
        assert_eq!(
            ledger.approval(waiting.id).unwrap().decision,
            ApprovalDecision::Denied
        );
        assert_eq!(ledger.revoke_connection("assistant", "local").unwrap(), 1);
        assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
    }

    #[test]
    fn fake_clock_hides_expired_grant() {
        let ledger = MemoryGrantLedger::default();
        ledger.insert_grant(grant("local")).unwrap();
        assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
        assert!(
            ledger
                .active_grants("assistant", DEFAULT_TTL_SECS)
                .is_empty()
        );
    }

    #[tokio::test]
    async fn same_operation_and_payload_executes_once() {
        let ledger = MemoryGrantLedger::default();
        let record = |payload: serde_json::Value| OperationRecord {
            profile: "assistant".into(),
            session: "s".into(),
            operation_id: "op-1".into(),
            tool: "data_insert".into(),
            payload_hash: crate::mcp::operation::payload_hash(&payload),
            state: OperationState::Succeeded,
            side_effect: SideEffect::Committed,
            result: "once".into(),
        };
        let first = ledger.reserve_operation(record(json!({"id": 7}))).unwrap();
        let replay = ledger.reserve_operation(record(json!({"id": 7}))).unwrap();
        assert_eq!(first, replay);
        assert!(
            ledger
                .reserve_operation(record(json!({"id": 8})))
                .unwrap_err()
                .to_string()
                .contains("different payload")
        );
        let unknown = OperationRecord {
            operation_id: "op-unknown".into(),
            state: OperationState::Unknown,
            ..record(json!({"id": 9}))
        };
        ledger.reserve_operation(unknown.clone()).unwrap();
        assert!(
            ledger
                .reserve_operation(unknown)
                .unwrap_err()
                .to_string()
                .contains("must not be retried")
        );
    }
}
