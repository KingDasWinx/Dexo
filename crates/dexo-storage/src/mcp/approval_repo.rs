use dexo_app::mcp::approval::{
    Approval, ApprovalDecision, HEARTBEAT_GRACE_SECS, KEEP_SETTLED_SECS,
};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

pub fn insert(conn: &Connection, approval: &Approval) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO mcp_approvals (
            id, profile_name, connection_name, tool, statement, targets_json,
            created_at, deadline, decision, grant_id, heartbeat
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            approval.id.to_string(),
            approval.profile,
            approval.connection,
            approval.tool,
            approval.statement,
            serde_json::to_string(&approval.targets)?,
            approval.created_at,
            approval.deadline,
            approval.decision.as_str(),
            approval.grant.to_string(),
            approval.heartbeat,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: Uuid) -> anyhow::Result<Option<Approval>> {
    conn.query_row(
        &format!("{SELECT} WHERE id = ?1"),
        params![id.to_string()],
        row_to_approval,
    )
    .optional()
    .map_err(Into::into)
}

/// One statement, so a decision and the deadline cannot cross: only a pending request is
/// decided, and only before its deadline, while its call still waits, is it approved.
pub fn settle(
    conn: &Connection,
    id: Uuid,
    decision: ApprovalDecision,
    now: i64,
) -> anyhow::Result<bool> {
    let changed = conn.execute(
        "UPDATE mcp_approvals SET decision = ?1, statement = ''
         WHERE id = ?2 AND decision = 'pending'
           AND (?1 <> 'approved' OR (deadline > ?3 AND heartbeat >= ?3 - ?4))",
        params![decision.as_str(), id.to_string(), now, HEARTBEAT_GRACE_SECS],
    )?;
    Ok(changed > 0)
}

/// The waiting call says it is still there.
pub fn touch(conn: &Connection, id: Uuid, now: i64) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE mcp_approvals SET heartbeat = ?2 WHERE id = ?1 AND decision = 'pending'",
        params![id.to_string(), now],
    )?;
    Ok(())
}

pub fn pending(conn: &Connection, now: i64) -> anyhow::Result<Vec<Approval>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE decision = 'pending' AND deadline > ?1 ORDER BY created_at"
    ))?;
    let rows = stmt.query_map(params![now], row_to_approval)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Requests nobody will decide have their statement blanked: past its deadline a request
/// expires, and one whose call went silent -- its server stopped while it waited -- is
/// cancelled. Decided requests are deleted after a day.
pub fn sweep(conn: &Connection, now: i64) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE mcp_approvals SET decision = 'expired', statement = ''
         WHERE decision = 'pending' AND deadline <= ?1",
        params![now],
    )?;
    conn.execute(
        "UPDATE mcp_approvals SET decision = 'cancelled', statement = ''
         WHERE decision = 'pending' AND heartbeat < ?1 - ?2",
        params![now, HEARTBEAT_GRACE_SECS],
    )?;
    conn.execute(
        "DELETE FROM mcp_approvals WHERE decision <> 'pending' AND deadline < ?1 - ?2",
        params![now, KEEP_SETTLED_SECS],
    )?;
    Ok(())
}

/// A revoked grant's waiting writes are denied, so no one approves one of them.
pub fn deny_revoked(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE mcp_approvals SET decision = 'denied', statement = ''
         WHERE decision = 'pending'
           AND grant_id IN (SELECT id FROM mcp_grants WHERE revoked = 1)",
        [],
    )?;
    Ok(())
}

const SELECT: &str = "SELECT id, profile_name, connection_name, tool, statement, targets_json,
        created_at, deadline, decision, grant_id, heartbeat FROM mcp_approvals";

fn row_to_approval(row: &rusqlite::Row<'_>) -> rusqlite::Result<Approval> {
    Ok(Approval {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
        profile: row.get(1)?,
        connection: row.get(2)?,
        tool: row.get(3)?,
        statement: row.get(4)?,
        targets: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
        created_at: row.get(6)?,
        deadline: row.get(7)?,
        decision: ApprovalDecision::parse(&row.get::<_, String>(8)?),
        grant: Uuid::parse_str(&row.get::<_, String>(9)?).unwrap_or_default(),
        heartbeat: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use dexo_app::mcp::approval::{Approval, ApprovalDecision};

    /// A request is decided once; approval after the deadline is refused; a request
    /// whose server went away is expired; a decided request keeps no SQL.
    #[test]
    fn requests_are_decided_once_and_keep_no_sql() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        let arguments = serde_json::json!({"sql": "DELETE FROM orders WHERE id = 7"})
            .as_object()
            .cloned()
            .unwrap();
        let request = |now| {
            Approval::pending(
                "p",
                "c",
                "data_execute_sql",
                &arguments,
                vec!["db.orders".into()],
                now,
                10,
            )
        };
        let first = request(100);
        super::insert(&conn, &first).unwrap();
        assert_eq!(
            super::pending(&conn, 105).unwrap()[0].statement,
            "DELETE FROM orders WHERE id = 7"
        );
        super::touch(&conn, first.id, 105).unwrap();
        assert!(super::settle(&conn, first.id, ApprovalDecision::Approved, 105).unwrap());
        assert!(!super::settle(&conn, first.id, ApprovalDecision::Denied, 106).unwrap());
        let decided = super::get(&conn, first.id).unwrap().unwrap();
        assert_eq!(decided.decision, ApprovalDecision::Approved);
        assert!(decided.statement.is_empty());

        let late = request(100);
        super::insert(&conn, &late).unwrap();
        super::touch(&conn, late.id, 110).unwrap();
        assert!(!super::settle(&conn, late.id, ApprovalDecision::Approved, 110).unwrap());
        super::sweep(&conn, 110).unwrap();
        assert!(super::pending(&conn, 110).unwrap().is_empty());
        let expired = super::get(&conn, late.id).unwrap().unwrap();
        assert_eq!(expired.decision, ApprovalDecision::Expired);
        assert!(expired.statement.is_empty());
    }

    /// A request whose call stopped saying it waits -- its server was killed -- cannot be
    /// approved and loses its SQL at the next sweep; a revoked grant's request is denied;
    /// decided requests are deleted after a day.
    #[test]
    fn orphaned_requests_are_settled_and_pruned() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        let arguments = serde_json::json!({"sql": "UPDATE orders SET paid = true"})
            .as_object()
            .cloned()
            .unwrap();
        let request = || {
            let request =
                Approval::pending("p", "c", "data_execute_sql", &arguments, vec![], 100, 60);
            super::insert(&conn, &request).unwrap();
            request
        };
        let orphan = request();
        assert!(!super::settle(&conn, orphan.id, ApprovalDecision::Approved, 110).unwrap());
        super::sweep(&conn, 110).unwrap();
        let swept = super::get(&conn, orphan.id).unwrap().unwrap();
        assert_eq!(swept.decision, ApprovalDecision::Cancelled);
        assert!(swept.statement.is_empty());

        let grant = uuid::Uuid::new_v4();
        conn.execute(
            "INSERT INTO mcp_grants (id, profile_name, connection_name, capability, tools_json,
                selectors_json, expires_at, remaining_uses, revision, revoked)
             VALUES (?1, 'p', 'c', 'data_write', '[]', '[]', 1000, 1, 1, 1)",
            rusqlite::params![grant.to_string()],
        )
        .unwrap();
        let mut revoked = Approval::pending("p", "c", "data_insert", &arguments, vec![], 100, 60);
        revoked.grant = grant;
        super::insert(&conn, &revoked).unwrap();
        super::deny_revoked(&conn).unwrap();
        let denied = super::get(&conn, revoked.id).unwrap().unwrap();
        assert_eq!(denied.decision, ApprovalDecision::Denied);
        assert!(denied.statement.is_empty());

        super::sweep(&conn, 160 + dexo_app::mcp::approval::KEEP_SETTLED_SECS + 1).unwrap();
        assert!(super::get(&conn, orphan.id).unwrap().is_none());
        assert!(super::get(&conn, revoked.id).unwrap().is_none());
    }
}
