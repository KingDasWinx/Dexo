use dexo_app::error::{AppError, ErrorCategory};
use dexo_app::mcp::grant::{Grant, GrantCapability};
use dexo_app::mcp::selector::SelectorRule;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

pub fn insert(conn: &Connection, grant: &Grant) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO mcp_grants (
            id, profile_name, connection_name, capability, tools_json, selectors_json,
            expires_at, remaining_uses, revision, revoked
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            grant.id.to_string(),
            grant.profile,
            grant.connection,
            capability_name(grant.capability),
            serde_json::to_string(&grant.tools)?,
            serde_json::to_string(&grant.selectors)?,
            grant.expires_at,
            grant.remaining_uses as i64,
            grant.revision as i64,
            if grant.revoked { 1 } else { 0 },
        ],
    )?;
    Ok(())
}

pub fn list_active(conn: &Connection, profile: &str, now: i64) -> anyhow::Result<Vec<Grant>> {
    let mut stmt = conn.prepare(
        "SELECT id, profile_name, connection_name, capability, tools_json, selectors_json,
                expires_at, remaining_uses, revision, revoked
         FROM mcp_grants
         WHERE profile_name = ?1 AND remaining_uses > 0 AND expires_at > ?2 AND revoked = 0",
    )?;
    let rows = stmt.query_map(params![profile, now], row_to_grant)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn revision(conn: &Connection) -> anyhow::Result<u64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(revision), 0) FROM mcp_grants",
        [],
        |row| row.get::<_, i64>(0),
    )? as u64)
}

pub fn consume(conn: &Connection, id: Uuid, now: i64) -> Result<Grant, AppError> {
    let mut grant = conn
        .query_row(
            "SELECT id, profile_name, connection_name, capability, tools_json, selectors_json,
                    expires_at, remaining_uses, revision, revoked
             FROM mcp_grants WHERE id = ?1",
            params![id.to_string()],
            row_to_grant,
        )
        .optional()
        .map_err(|error| AppError::new(ErrorCategory::Storage, error.to_string()))?
        .ok_or_else(|| AppError::new(ErrorCategory::McpPolicy, "not found"))?;
    if !grant.active(now) {
        return Err(AppError::new(ErrorCategory::McpPolicy, "not found"));
    }
    grant.remaining_uses = grant.remaining_uses.saturating_sub(1);
    conn.execute(
        "UPDATE mcp_grants SET remaining_uses = ?1, revision = revision + 1 WHERE id = ?2",
        params![grant.remaining_uses as i64, id.to_string()],
    )
    .map_err(|error| AppError::new(ErrorCategory::Storage, error.to_string()))?;
    Ok(grant)
}

pub fn revoke(conn: &Connection, id: Uuid) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE mcp_grants SET remaining_uses = 0, revoked = 1, revision = revision + 1 WHERE id = ?1",
        params![id.to_string()],
    )?;
    Ok(())
}

pub fn revoke_profile(conn: &Connection, profile: &str) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE mcp_grants SET remaining_uses = 0, revoked = 1, revision = revision + 1 WHERE profile_name = ?1",
        params![profile],
    )?;
    Ok(())
}

pub fn revoke_all(conn: &Connection) -> anyhow::Result<usize> {
    conn.execute(
        "UPDATE mcp_grants SET remaining_uses = 0, revoked = 1, revision = revision + 1 WHERE revoked = 0",
        [],
    )
    .map_err(Into::into)
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

fn capability_name(capability: GrantCapability) -> &'static str {
    match capability {
        GrantCapability::DataWrite => "data_write",
        GrantCapability::Ddl => "ddl",
        GrantCapability::Admin => "admin",
    }
}

fn row_to_grant(row: &rusqlite::Row<'_>) -> rusqlite::Result<Grant> {
    let capability = match row.get::<_, String>(3)?.as_str() {
        "ddl" => GrantCapability::Ddl,
        "admin" => GrantCapability::Admin,
        _ => GrantCapability::DataWrite,
    };
    let tools: Vec<String> = serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default();
    let selectors: Vec<SelectorRule> =
        serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default();
    Ok(Grant {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).expect("uuid"),
        profile: row.get(1)?,
        connection: row.get(2)?,
        capability,
        tools,
        selectors,
        expires_at: row.get(6)?,
        remaining_uses: row.get::<_, i64>(7)? as u32,
        revision: row.get::<_, i64>(8)? as u64,
        revoked: row.get::<_, i64>(9)? != 0,
    })
}
