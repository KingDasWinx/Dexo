mod approval_repo;
mod audit_repo;
mod grant_repo;
mod operation_repo;

use std::path::Path;
use std::sync::Mutex;

use dexo_app::error::{AppError, ErrorCategory};
use dexo_app::mcp::approval::{Approval, ApprovalDecision};
use dexo_app::mcp::audit::AuditEvent;
use dexo_app::mcp::grant::Grant;
use dexo_app::mcp::ledger::GrantLedger;
use dexo_app::mcp::operation::{OperationRecord, OperationState, SideEffect};
use rusqlite::Connection;
use uuid::Uuid;

/// The requests waiting at `now`, for a screen that asks every few seconds: nothing is
/// written unless one is pending, so someone who never used MCP pays one indexed read.
pub fn waiting_approvals(conn: &Connection, now: i64) -> anyhow::Result<Vec<Approval>> {
    if !approval_repo::any_pending(conn)? {
        return Ok(Vec::new());
    }
    approval_repo::sweep(conn, now)?;
    approval_repo::pending(conn, now)
}

pub struct SqliteGrantLedger {
    conn: Mutex<Connection>,
}

impl SqliteGrantLedger {
    pub fn open(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn revoke_all(&self) -> anyhow::Result<usize> {
        self.revoking(grant_repo::revoke_all)
    }

    /// Revokes -- or replaces -- and denies the revoked grants' waiting writes in the same
    /// transaction, so no one approves a write its grant no longer allows.
    fn revoking<T>(
        &self,
        revoke: impl FnOnce(&Connection) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let conn = self.conn.lock().expect("sqlite");
        let tx = conn.unchecked_transaction()?;
        let revoked = revoke(&tx)?;
        approval_repo::deny_revoked(&tx)?;
        tx.commit()?;
        Ok(revoked)
    }
}

impl GrantLedger for SqliteGrantLedger {
    fn active_grants(&self, profile: &str, now: i64) -> Vec<Grant> {
        let conn = self.conn.lock().expect("sqlite");
        grant_repo::list_active(&conn, profile, now).unwrap_or_default()
    }

    fn revision(&self) -> u64 {
        let conn = self.conn.lock().expect("sqlite");
        grant_repo::revision(&conn).unwrap_or(0)
    }

    fn insert_grant(&self, grant: Grant) -> Result<(), AppError> {
        self.revoking(|conn| grant_repo::insert(conn, &grant))
            .map(drop)
            .map_err(sql_err)
    }

    fn revoke(&self, id: Uuid) -> Result<(), AppError> {
        self.revoking(|conn| grant_repo::revoke(conn, id))
            .map_err(sql_err)
    }

    fn revoke_connection(&self, profile: &str, connection: &str) -> Result<usize, AppError> {
        self.revoking(|conn| grant_repo::revoke_connection(conn, profile, connection))
            .map_err(sql_err)
    }

    fn revoke_profile(&self, profile: &str) -> Result<usize, AppError> {
        self.revoking(|conn| grant_repo::revoke_profile(conn, profile))
            .map_err(sql_err)
    }

    fn reserve_operation(&self, record: OperationRecord) -> Result<OperationRecord, AppError> {
        let conn = self.conn.lock().expect("sqlite");
        operation_repo::reserve(&conn, record)
    }

    fn lookup_operation(
        &self,
        profile: &str,
        session: &str,
        operation_id: &str,
    ) -> Option<OperationRecord> {
        let conn = self.conn.lock().expect("sqlite");
        operation_repo::lookup(&conn, profile, session, operation_id)
            .ok()
            .flatten()
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
        let conn = self.conn.lock().expect("sqlite");
        operation_repo::finish(
            &conn,
            profile,
            session,
            operation_id,
            state,
            side_effect,
            result,
        )
        .map_err(sql_err)
    }

    fn record_audit(&self, event: AuditEvent) {
        let conn = self.conn.lock().expect("sqlite");
        let _ = audit_repo::insert(&conn, &event);
    }

    fn audits(&self) -> Vec<AuditEvent> {
        let conn = self.conn.lock().expect("sqlite");
        audit_repo::list(&conn).unwrap_or_default()
    }

    fn recent_audits(&self, limit: usize) -> Vec<AuditEvent> {
        let conn = self.conn.lock().expect("sqlite");
        audit_repo::recent(&conn, limit).unwrap_or_default()
    }

    fn prune_audits(&self, older_than: i64) {
        let conn = self.conn.lock().expect("sqlite");
        let _ = audit_repo::prune(&conn, older_than);
    }

    fn is_revoked(&self, id: Uuid) -> bool {
        let conn = self.conn.lock().expect("sqlite");
        grant_repo::is_revoked(&conn, id).unwrap_or(false)
    }

    fn request_approval(&self, approval: &Approval) -> Result<(), AppError> {
        let conn = self.conn.lock().expect("sqlite");
        approval_repo::insert(&conn, approval).map_err(sql_err)
    }

    fn approval(&self, id: Uuid) -> Option<Approval> {
        let conn = self.conn.lock().expect("sqlite");
        approval_repo::get(&conn, id).ok().flatten()
    }

    fn settle_approval(
        &self,
        id: Uuid,
        decision: ApprovalDecision,
        now: i64,
    ) -> Result<bool, AppError> {
        let conn = self.conn.lock().expect("sqlite");
        approval_repo::settle(&conn, id, decision, now).map_err(sql_err)
    }

    fn touch_approval(&self, id: Uuid, now: i64) {
        let conn = self.conn.lock().expect("sqlite");
        let _ = approval_repo::touch(&conn, id, now);
    }

    fn sweep_approvals(&self, now: i64) {
        let conn = self.conn.lock().expect("sqlite");
        let _ = approval_repo::sweep(&conn, now);
    }

    fn pending_approvals(&self, now: i64) -> Vec<Approval> {
        let conn = self.conn.lock().expect("sqlite");
        let _ = approval_repo::sweep(&conn, now);
        approval_repo::pending(&conn, now).unwrap_or_default()
    }
}

fn sql_err(error: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCategory::Storage, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::SqliteGrantLedger;
    use dexo_app::mcp::grant::{DEFAULT_TTL_SECS, Grant, WriteKind};
    use dexo_app::mcp::ledger::GrantLedger;
    use dexo_app::mcp::profile::McpProfile;
    use dexo_app::mcp::selector::{Effect, SelectorRule};

    /// The TUI asks every two seconds: with nothing pending it writes nothing, and with
    /// a request pending it sweeps and lists.
    #[test]
    fn waiting_approvals_writes_nothing_when_nothing_waits() {
        use dexo_app::mcp::approval::{Approval, ApprovalDecision};
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        let changes = |conn: &rusqlite::Connection| -> i64 {
            conn.query_row("SELECT total_changes()", [], |row| row.get(0))
                .unwrap()
        };
        let arguments = serde_json::json!({"sql": "DELETE FROM orders"})
            .as_object()
            .cloned()
            .unwrap();
        let decided = Approval {
            decision: ApprovalDecision::Denied,
            ..Approval::pending("p", "c", "data_execute_sql", &arguments, vec![], 0, 10)
        };
        super::approval_repo::insert(&conn, &decided).unwrap();
        let before = changes(&conn);
        assert!(
            super::waiting_approvals(&conn, 1_000_000)
                .unwrap()
                .is_empty()
        );
        assert_eq!(changes(&conn), before);

        let waiting = Approval::pending("p", "c", "data_execute_sql", &arguments, vec![], 100, 60);
        super::approval_repo::insert(&conn, &waiting).unwrap();
        let listed = super::waiting_approvals(&conn, 101).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, waiting.id);
    }

    /// A request whose agent was killed is withdrawn, and the audit says so: its "waiting"
    /// line was the last word on it for ever.
    #[test]
    fn a_request_whose_agent_went_silent_is_audited_as_withdrawn() {
        use dexo_app::mcp::approval::Approval;
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        let arguments = serde_json::json!({"sql": "DELETE FROM orders"})
            .as_object()
            .cloned()
            .unwrap();
        let waiting = Approval::pending(
            "pg-dev",
            "pg-dev",
            "data_update",
            &arguments,
            vec!["public.orders".into()],
            100,
            600,
        );
        super::approval_repo::insert(&conn, &waiting).unwrap();
        super::approval_repo::sweep(&conn, 200).unwrap();
        let events = super::audit_repo::list(&conn).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].profile, "pg-dev");
        assert!(events[0].status.starts_with("withdrawn"), "{:?}", events[0]);
        // Swept again, it is not written twice.
        super::approval_repo::sweep(&conn, 201).unwrap();
        assert_eq!(super::audit_repo::list(&conn).unwrap().len(), 1);
    }

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

    fn ledger() -> SqliteGrantLedger {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        SqliteGrantLedger {
            conn: std::sync::Mutex::new(conn),
        }
    }

    /// One write access per profile and connection, in the database too: the new one
    /// replaces the old, and what waited under the old one is denied.
    #[test]
    fn a_new_grant_replaces_the_one_on_its_connection() {
        use dexo_app::mcp::approval::{Approval, ApprovalDecision};
        let ledger = ledger();
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
        assert!(ledger.is_revoked(first.id));
        assert_eq!(ledger.active_grants("assistant", 0).len(), 2);
        assert_eq!(
            ledger.approval(waiting.id).unwrap().decision,
            ApprovalDecision::Denied
        );
        assert_eq!(ledger.revoke_connection("assistant", "local").unwrap(), 1);
        assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
    }

    /// Write access given before the redesign was under other terms: its own tables, no
    /// approval. Migration 19 dropped those terms; migration 20 revokes what it widened,
    /// so a person gives it again on the new terms.
    #[test]
    fn migrations_19_and_20_revoke_every_grant_made_before_the_redesign() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrations::apply_up_to(&conn, 18).unwrap();
        for (id, uses, ask) in [
            ("00000000-0000-0000-0000-000000000001", 0i64, 0i64),
            ("00000000-0000-0000-0000-000000000002", 1, 0),
            ("00000000-0000-0000-0000-000000000003", 4_294_967_295, 120),
        ] {
            conn.execute(
                "INSERT INTO mcp_grants (id, profile_name, connection_name, capability, tools_json,
                    selectors_json, expires_at, remaining_uses, revision, revoked, ask_secs)
                 VALUES (?1, 'assistant', 'local', 'data_write', '[\"data_insert\"]',
                    '[\"db.public.items\"]', 9223372036854775807, ?2, 1, 0, ?3)",
                rusqlite::params![id, uses, ask],
            )
            .unwrap();
        }
        crate::migrations::apply_pending(&conn).unwrap();
        assert!(
            super::grant_repo::list_active(&conn, "assistant", 0)
                .unwrap()
                .is_empty()
        );
        let revoked: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mcp_grants WHERE revoked = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revoked, 3);
    }

    #[test]
    fn grant_expiry_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        let id;
        {
            let _db = crate::Database::open(&path).unwrap();
            let ledger = SqliteGrantLedger::open(&path).unwrap();
            let grant = grant("local");
            id = grant.id;
            ledger.insert_grant(grant).unwrap();
            assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
        }
        let ledger = SqliteGrantLedger::open(&path).unwrap();
        assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
        assert!(
            ledger
                .active_grants("assistant", DEFAULT_TTL_SECS)
                .is_empty()
        );
        assert!(!ledger.is_revoked(id));
    }

    #[test]
    fn revoke_all_clears_active_grants() {
        let ledger = ledger();
        ledger.insert_grant(grant("local")).unwrap();
        assert_eq!(ledger.revoke_all().unwrap(), 1);
        assert!(ledger.active_grants("assistant", 0).is_empty());
    }
}
