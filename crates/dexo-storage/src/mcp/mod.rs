mod audit_repo;
mod grant_repo;
mod operation_repo;

use std::path::Path;
use std::sync::Mutex;

use dexo_app::error::{AppError, ErrorCategory};
use dexo_app::mcp::audit::AuditEvent;
use dexo_app::mcp::grant::Grant;
use dexo_app::mcp::ledger::GrantLedger;
use dexo_app::mcp::operation::{OperationRecord, OperationState, SideEffect};
use rusqlite::Connection;
use uuid::Uuid;

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

    pub fn revoke_str(&self, id: &str) -> Result<(), AppError> {
        let id = Uuid::parse_str(id)
            .map_err(|error| AppError::new(ErrorCategory::Configuration, error.to_string()))?;
        self.revoke(id)
    }

    pub fn revoke_all(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().expect("sqlite");
        grant_repo::revoke_all(&conn)
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
        let conn = self.conn.lock().expect("sqlite");
        grant_repo::insert(&conn, &grant).map_err(sql_err)
    }

    fn consume(&self, id: Uuid, now: i64) -> Result<Grant, AppError> {
        let conn = self.conn.lock().expect("sqlite");
        let tx = conn.unchecked_transaction().map_err(sql_err)?;
        let grant = grant_repo::consume(&tx, id, now)?;
        tx.commit().map_err(sql_err)?;
        Ok(grant)
    }

    fn revoke(&self, id: Uuid) -> Result<(), AppError> {
        let conn = self.conn.lock().expect("sqlite");
        grant_repo::revoke(&conn, id).map_err(sql_err)
    }

    fn revoke_profile(&self, profile: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().expect("sqlite");
        grant_repo::revoke_profile(&conn, profile).map_err(sql_err)
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

    fn prune_audits(&self, older_than: i64) {
        let conn = self.conn.lock().expect("sqlite");
        let _ = audit_repo::prune(&conn, older_than);
    }

    fn is_revoked(&self, id: Uuid) -> bool {
        let conn = self.conn.lock().expect("sqlite");
        grant_repo::is_revoked(&conn, id).unwrap_or(false)
    }
}

fn sql_err(error: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCategory::Storage, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::SqliteGrantLedger;
    use dexo_app::mcp::grant::{DEFAULT_TTL_SECS, Grant, GrantCapability};
    use dexo_app::mcp::ledger::GrantLedger;
    use dexo_app::mcp::profile::McpProfile;
    use dexo_app::mcp::selector::{Effect, SelectorRule};

    #[test]
    fn consume_is_transactional_one_use() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        let ledger = SqliteGrantLedger {
            conn: std::sync::Mutex::new(conn),
        };
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
        ledger.insert_grant(grant).unwrap();
        ledger.consume(id, 0).unwrap();
        assert!(ledger.active_grants("assistant", 0).is_empty());
    }

    #[test]
    fn concurrent_consume_only_one_succeeds() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        let ledger = std::sync::Arc::new(SqliteGrantLedger {
            conn: std::sync::Mutex::new(conn),
        });
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
        ledger.insert_grant(grant).unwrap();
        let a = {
            let ledger = std::sync::Arc::clone(&ledger);
            std::thread::spawn(move || ledger.consume(id, 0).is_ok())
        };
        let b = {
            let ledger = std::sync::Arc::clone(&ledger);
            std::thread::spawn(move || ledger.consume(id, 0).is_ok())
        };
        let wins = usize::from(a.join().unwrap()) + usize::from(b.join().unwrap());
        assert_eq!(wins, 1);
    }

    #[test]
    fn grant_expiry_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        let id;
        {
            let _db = crate::Database::open(&path).unwrap();
            let ledger = SqliteGrantLedger::open(&path).unwrap();
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
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        let ledger = SqliteGrantLedger {
            conn: std::sync::Mutex::new(conn),
        };
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
        ledger.insert_grant(grant).unwrap();
        assert_eq!(ledger.revoke_all().unwrap(), 1);
        assert!(ledger.active_grants("assistant", 0).is_empty());
    }
}
