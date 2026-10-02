use dexo_app::mcp::approval::{Approval, ApprovalDecision};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

pub fn insert(conn: &Connection, approval: &Approval) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO mcp_approvals (
            id, profile_name, connection_name, tool, statement, targets_json,
            created_at, deadline, decision
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
/// decided, and only before its deadline is it approved.
pub fn settle(
    conn: &Connection,
    id: Uuid,
    decision: ApprovalDecision,
    now: i64,
) -> anyhow::Result<bool> {
    let changed = conn.execute(
        "UPDATE mcp_approvals SET decision = ?1, statement = ''
         WHERE id = ?2 AND decision = 'pending' AND (?1 <> 'approved' OR deadline > ?3)",
        params![decision.as_str(), id.to_string(), now],
    )?;
    Ok(changed > 0)
}

pub fn pending(conn: &Connection, now: i64) -> anyhow::Result<Vec<Approval>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT} WHERE decision = 'pending' AND deadline > ?1 ORDER BY created_at"
    ))?;
    let rows = stmt.query_map(params![now], row_to_approval)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Requests that were never decided -- their server stopped while it waited -- have
/// their statement blanked once their deadline has passed.
pub fn expire_stale(conn: &Connection, now: i64) -> anyhow::Result<usize> {
    conn.execute(
        "UPDATE mcp_approvals SET decision = 'expired', statement = ''
         WHERE decision = 'pending' AND deadline <= ?1",
        params![now],
    )
    .map_err(Into::into)
}

const SELECT: &str = "SELECT id, profile_name, connection_name, tool, statement, targets_json,
        created_at, deadline, decision FROM mcp_approvals";

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
        assert!(super::settle(&conn, first.id, ApprovalDecision::Approved, 105).unwrap());
        assert!(!super::settle(&conn, first.id, ApprovalDecision::Denied, 106).unwrap());
        let decided = super::get(&conn, first.id).unwrap().unwrap();
        assert_eq!(decided.decision, ApprovalDecision::Approved);
        assert!(decided.statement.is_empty());

        let late = request(100);
        super::insert(&conn, &late).unwrap();
        assert!(!super::settle(&conn, late.id, ApprovalDecision::Approved, 110).unwrap());
        assert!(super::pending(&conn, 110).unwrap().is_empty());
        assert_eq!(super::expire_stale(&conn, 110).unwrap(), 1);
        let expired = super::get(&conn, late.id).unwrap().unwrap();
        assert_eq!(expired.decision, ApprovalDecision::Expired);
        assert!(expired.statement.is_empty());
    }
}
