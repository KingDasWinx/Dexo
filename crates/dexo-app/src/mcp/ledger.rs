use std::collections::HashMap;
use std::sync::Mutex;

use uuid::Uuid;

use crate::error::{AppError, ErrorCategory};
use crate::mcp::audit::AuditEvent;
use crate::mcp::grant::Grant;
use crate::mcp::operation::{
    OperationRecord, OperationState, SideEffect, payload_hash, replay_or_conflict,
};

pub trait GrantLedger: Send + Sync {
    fn active_grants(&self, profile: &str, now: i64) -> Vec<Grant>;
    fn revision(&self) -> u64;
    fn insert_grant(&self, grant: Grant) -> Result<(), AppError>;
    fn consume(&self, id: Uuid, now: i64) -> Result<Grant, AppError>;
    fn revoke(&self, id: Uuid) -> Result<(), AppError>;
    fn revoke_profile(&self, profile: &str) -> Result<(), AppError>;
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
    fn prune_audits(&self, older_than: i64);
    fn is_revoked(&self, id: Uuid) -> bool;
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
    revision: u64,
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

    fn revision(&self) -> u64 {
        self.inner.lock().expect("ledger").revision
    }

    fn insert_grant(&self, grant: Grant) -> Result<(), AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        inner.revision += 1;
        let mut grant = grant;
        grant.revision = inner.revision;
        inner.grants.push(grant);
        Ok(())
    }

    fn consume(&self, id: Uuid, now: i64) -> Result<Grant, AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        let cloned = {
            let grant = inner
                .grants
                .iter_mut()
                .find(|grant| grant.id == id)
                .ok_or_else(|| AppError::new(ErrorCategory::McpPolicy, "not found"))?;
            if !grant.active(now) {
                return Err(AppError::new(ErrorCategory::McpPolicy, "not found"));
            }
            grant.remaining_uses = grant.remaining_uses.saturating_sub(1);
            grant.clone()
        };
        inner.revision += 1;
        Ok(cloned)
    }

    fn revoke(&self, id: Uuid) -> Result<(), AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        if let Some(grant) = inner.grants.iter_mut().find(|grant| grant.id == id) {
            grant.remaining_uses = 0;
            grant.revoked = true;
            inner.revision += 1;
            Ok(())
        } else {
            Err(AppError::new(ErrorCategory::McpPolicy, "not found"))
        }
    }

    fn revoke_profile(&self, profile: &str) -> Result<(), AppError> {
        let mut inner = self.inner.lock().expect("ledger");
        for grant in &mut inner.grants {
            if grant.profile == profile {
                grant.remaining_uses = 0;
                grant.revoked = true;
            }
        }
        inner.revision += 1;
        Ok(())
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
}

pub fn hash_payload(value: &serde_json::Value) -> String {
    payload_hash(value)
}

#[cfg(test)]
mod tests {
    use super::{GrantLedger, MemoryGrantLedger};
    use crate::mcp::grant::{DEFAULT_TTL_SECS, Grant, GrantCapability};
    use crate::mcp::operation::{OperationRecord, OperationState, SideEffect};
    use crate::mcp::profile::McpProfile;
    use crate::mcp::selector::{Effect, SelectorRule};
    use serde_json::json;

    #[test]
    fn consume_is_one_use_and_expiry_hides_grant() {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
        let grant = Grant::new(
            &profile,
            "local",
            GrantCapability::DataWrite,
            vec!["data_insert".into()],
            vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
            0,
            DEFAULT_TTL_SECS,
        )
        .unwrap();
        let id = grant.id;
        let ledger = MemoryGrantLedger::default();
        ledger.insert_grant(grant).unwrap();
        assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
        ledger.consume(id, 0).unwrap();
        assert!(ledger.active_grants("assistant", 0).is_empty());
        assert!(ledger.consume(id, 0).is_err());
    }

    #[test]
    fn fake_clock_hides_expired_grant() {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
        let grant = Grant::new(
            &profile,
            "local",
            GrantCapability::DataWrite,
            vec!["data_insert".into()],
            vec![SelectorRule::parse(Effect::Allow, "db.public.items").unwrap()],
            0,
            DEFAULT_TTL_SECS,
        )
        .unwrap();
        let ledger = MemoryGrantLedger::default();
        ledger.insert_grant(grant).unwrap();
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
