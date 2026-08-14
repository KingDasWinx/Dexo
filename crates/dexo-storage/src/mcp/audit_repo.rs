use dexo_app::mcp::audit::AuditEvent;
use rusqlite::{Connection, params};
use uuid::Uuid;

pub fn insert(conn: &Connection, event: &AuditEvent) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO mcp_audit (id, timestamp, json) VALUES (?1, ?2, ?3)",
        params![
            Uuid::new_v4().to_string(),
            event.timestamp,
            serde_json::to_string(event)?
        ],
    )?;
    Ok(())
}

pub fn list(conn: &Connection) -> anyhow::Result<Vec<AuditEvent>> {
    let mut stmt = conn.prepare("SELECT json FROM mcp_audit ORDER BY timestamp")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut events = Vec::new();
    for row in rows {
        if let Ok(event) = serde_json::from_str(&row?) {
            events.push(event);
        }
    }
    Ok(events)
}

pub fn prune(conn: &Connection, older_than: i64) -> anyhow::Result<()> {
    conn.execute(
        "DELETE FROM mcp_audit WHERE timestamp < ?1",
        params![older_than],
    )?;
    Ok(())
}
