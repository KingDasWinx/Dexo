use dexo_app::mcp::grant::Grant;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

/// Saves `grant`, revoking first what its profile had on its connection: one write access
/// per profile and connection. Returns how many it replaced.
pub fn insert(conn: &Connection, grant: &Grant) -> anyhow::Result<usize> {
    let replaced = revoke_connection(conn, &grant.profile, &grant.connection)?;
    conn.execute(
        "INSERT INTO mcp_grants (
            id, profile_name, connection_name, tools_json, expires_at, revision, revoked,
            ask_secs, bypass_json, production
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            grant.id.to_string(),
            grant.profile,
            grant.connection,
            serde_json::to_string(&grant.tools)?,
            grant.expires_at,
            grant.revision as i64,
            if grant.revoked { 1 } else { 0 },
            grant.ask_secs,
            serde_json::to_string(&grant.bypass)?,
            grant.production,
        ],
    )?;
    Ok(replaced)
}

/// Every grant not revoked, of any profile, expired or not.
pub fn live(conn: &Connection) -> anyhow::Result<Vec<Grant>> {
    let mut stmt = conn.prepare(
        "SELECT id, profile_name, connection_name, tools_json, expires_at, revision, revoked,
                ask_secs, bypass_json, production
         FROM mcp_grants
         WHERE revoked = 0",
    )?;
    let rows = stmt.query_map([], row_to_grant)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn list_active(conn: &Connection, profile: &str, now: i64) -> anyhow::Result<Vec<Grant>> {
    let mut stmt = conn.prepare(
        "SELECT id, profile_name, connection_name, tools_json, expires_at, revision, revoked,
                ask_secs, bypass_json, production
         FROM mcp_grants
         WHERE profile_name = ?1 AND expires_at > ?2 AND revoked = 0",
    )?;
    let rows = stmt.query_map(params![profile, now], row_to_grant)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn set_bypass(
    conn: &Connection,
    id: Uuid,
    bypass: &[dexo_app::mcp::bypass::Operation],
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE mcp_grants SET bypass_json = ?2, revision = revision + 1 WHERE id = ?1",
        params![id.to_string(), serde_json::to_string(bypass)?],
    )?;
    Ok(())
}

pub fn revoke(conn: &Connection, id: Uuid) -> anyhow::Result<()> {
    let changed = conn.execute(
        "UPDATE mcp_grants SET revoked = 1, revision = revision + 1 WHERE id = ?1 AND revoked = 0",
        params![id.to_string()],
    )?;
    anyhow::ensure!(changed == 1, "no live grant has the id {id}");
    Ok(())
}

pub fn revoke_connection(
    conn: &Connection,
    profile: &str,
    connection: &str,
) -> anyhow::Result<usize> {
    conn.execute(
        "UPDATE mcp_grants SET revoked = 1, revision = revision + 1
         WHERE profile_name = ?1 AND connection_name = ?2 AND revoked = 0",
        params![profile, connection],
    )
    .map_err(Into::into)
}

pub fn revoke_profile(conn: &Connection, profile: &str) -> anyhow::Result<usize> {
    let revoked = conn.execute(
        "UPDATE mcp_grants SET revoked = 1, revision = revision + 1
         WHERE profile_name = ?1 AND revoked = 0",
        params![profile],
    )?;
    Ok(revoked)
}

/// Takes back every write access; how many stopped, one per profile and connection --
/// not counting access already out of time, which was never going to write again.
pub fn revoke_all(conn: &Connection) -> anyhow::Result<usize> {
    let live: i64 = conn.query_row(
        "SELECT COUNT(*) FROM (
            SELECT DISTINCT profile_name, connection_name FROM mcp_grants
            WHERE revoked = 0 AND expires_at > CAST(strftime('%s', 'now') AS INTEGER)
         )",
        [],
        |row| row.get(0),
    )?;
    conn.execute(
        "UPDATE mcp_grants SET revoked = 1, revision = revision + 1 WHERE revoked = 0",
        [],
    )?;
    Ok(live as usize)
}

pub fn is_revoked(conn: &Connection, id: Uuid) -> anyhow::Result<bool> {
    let revoked: Option<i64> = conn
        .query_row(
            "SELECT revoked FROM mcp_grants WHERE id = ?1",
            params![id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(revoked.unwrap_or(0) != 0)
}

fn row_to_grant(row: &rusqlite::Row<'_>) -> rusqlite::Result<Grant> {
    let tools: Vec<String> = serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default();
    Ok(Grant {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).expect("uuid"),
        profile: row.get(1)?,
        connection: row.get(2)?,
        tools,
        expires_at: row.get(4)?,
        revision: row.get::<_, i64>(5)? as u64,
        revoked: row.get::<_, i64>(6)? != 0,
        ask_secs: row.get(7)?,
        bypass: serde_json::from_str(&row.get::<_, String>(8)?).unwrap_or_default(),
        production: row.get::<_, i64>(9)? != 0,
    })
}
