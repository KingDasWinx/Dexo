use dexo_app::mcp::{
    Effect, McpLimits, McpProfile, PersistentAccess, QueryMode, SelectorRule, ToolRule,
};
use rusqlite::{Connection, params};
use uuid::Uuid;

pub struct McpProfileRepository<'a> {
    conn: &'a Connection,
}

impl<'a> McpProfileRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn save(&self, profile: &McpProfile) -> anyhow::Result<()> {
        profile
            .validate()
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        self.conn.execute(
            "INSERT INTO mcp_profiles (
                id, name, enabled, persistent_access, max_rows, max_bytes, timeout_secs,
                max_concurrency, query_mode, audit_retention_days, connections_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, enabled=excluded.enabled,
                persistent_access=excluded.persistent_access, max_rows=excluded.max_rows,
                max_bytes=excluded.max_bytes, timeout_secs=excluded.timeout_secs,
                max_concurrency=excluded.max_concurrency, query_mode=excluded.query_mode,
                audit_retention_days=excluded.audit_retention_days,
                connections_json=excluded.connections_json",
            params![
                profile.id.to_string(),
                profile.name,
                profile.enabled as i64,
                match profile.persistent_access {
                    PersistentAccess::ReadOnly => "read_only",
                },
                profile.limits.max_rows as i64,
                profile.limits.max_bytes as i64,
                profile.limits.timeout_secs as i64,
                profile.limits.max_concurrency as i64,
                match profile.query_mode {
                    QueryMode::StructuredOnly => "structured",
                    QueryMode::RawReadSql => "raw_read",
                },
                profile.audit_retention_days as i64,
                serde_json::to_string(&profile.connections)?,
            ],
        )?;
        self.conn.execute(
            "DELETE FROM mcp_selectors WHERE profile_id = ?1",
            params![profile.id.to_string()],
        )?;
        self.conn.execute(
            "DELETE FROM mcp_tool_rules WHERE profile_id = ?1",
            params![profile.id.to_string()],
        )?;
        for rule in &profile.selectors {
            let pattern = display_selector(&rule.selector);
            self.conn.execute(
                "INSERT INTO mcp_selectors (id, profile_id, effect, pattern) VALUES (?1, ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    profile.id.to_string(),
                    match rule.effect {
                        Effect::Allow => "allow",
                        Effect::Deny => "deny",
                    },
                    pattern,
                ],
            )?;
        }
        for rule in &profile.tool_rules {
            self.conn.execute(
                "INSERT INTO mcp_tool_rules (id, profile_id, tool, allowed) VALUES (?1, ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    profile.id.to_string(),
                    rule.tool,
                    rule.allowed as i64,
                ],
            )?;
        }
        Ok(())
    }

    pub fn get_by_name(&self, name: &str) -> anyhow::Result<Option<McpProfile>> {
        let mut profile = match self.conn.query_row(
            "SELECT id, name, enabled, persistent_access, max_rows, max_bytes, timeout_secs,
                    max_concurrency, query_mode, audit_retention_days, connections_json
             FROM mcp_profiles WHERE name = ?1",
            params![name],
            row_to_profile,
        ) {
            Ok(profile) => profile,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        profile.selectors = self.load_selectors(&profile.id)?;
        profile.tool_rules = self.load_tools(&profile.id)?;
        Ok(Some(profile))
    }

    pub fn list(&self) -> anyhow::Result<Vec<McpProfile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, enabled, persistent_access, max_rows, max_bytes, timeout_secs,
                    max_concurrency, query_mode, audit_retention_days, connections_json
             FROM mcp_profiles ORDER BY name",
        )?;
        let mut profiles = stmt
            .query_map([], row_to_profile)?
            .collect::<Result<Vec<_>, _>>()?;
        for profile in &mut profiles {
            profile.selectors = self.load_selectors(&profile.id)?;
            profile.tool_rules = self.load_tools(&profile.id)?;
        }
        Ok(profiles)
    }

    fn load_selectors(&self, id: &Uuid) -> anyhow::Result<Vec<SelectorRule>> {
        let mut stmt = self
            .conn
            .prepare("SELECT effect, pattern FROM mcp_selectors WHERE profile_id = ?1")?;
        let rows = stmt.query_map(params![id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut rules = Vec::new();
        for row in rows {
            let (effect, pattern) = row?;
            let effect = if effect == "deny" {
                Effect::Deny
            } else {
                Effect::Allow
            };
            rules.push(SelectorRule::parse(effect, &pattern).map_err(|e| anyhow::anyhow!("{e}"))?);
        }
        Ok(rules)
    }

    fn load_tools(&self, id: &Uuid) -> anyhow::Result<Vec<ToolRule>> {
        let mut stmt = self
            .conn
            .prepare("SELECT tool, allowed FROM mcp_tool_rules WHERE profile_id = ?1")?;
        let rows = stmt.query_map(params![id.to_string()], |row| {
            Ok(ToolRule {
                tool: row.get(0)?,
                allowed: row.get::<_, i64>(1)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<McpProfile> {
    let id = Uuid::parse_str(&row.get::<_, String>(0)?).expect("uuid");
    let connections: Vec<String> =
        serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or_default();
    Ok(McpProfile {
        id,
        name: row.get(1)?,
        enabled: row.get::<_, i64>(2)? != 0,
        persistent_access: PersistentAccess::ReadOnly,
        limits: McpLimits {
            max_rows: row.get::<_, i64>(4)? as u64,
            max_bytes: row.get::<_, i64>(5)? as u64,
            timeout_secs: row.get::<_, i64>(6)? as u64,
            max_concurrency: row.get::<_, i64>(7)? as u32,
        },
        query_mode: if row.get::<_, String>(8)? == "raw_read" {
            QueryMode::RawReadSql
        } else {
            QueryMode::StructuredOnly
        },
        audit_retention_days: row.get::<_, i64>(9)? as u32,
        connections,
        selectors: Vec::new(),
        tool_rules: Vec::new(),
    })
}

fn display_selector(selector: &dexo_app::mcp::Selector) -> String {
    let mut parts = Vec::new();
    for seg in [
        selector.catalog.as_ref(),
        selector.schema.as_ref(),
        selector.object.as_ref(),
        selector.column.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        parts.push(match seg {
            dexo_app::mcp::selector::Segment::Star => "*".to_string(),
            dexo_app::mcp::selector::Segment::Exact(name) => name.clone(),
        });
    }
    parts.join(".")
}

#[cfg(test)]
mod tests {
    use super::McpProfileRepository;
    use crate::Database;
    use dexo_app::mcp::{Effect, McpProfile, PersistentAccess, SelectorRule};

    #[test]
    fn new_profile_round_trip_stays_disabled_read_only() {
        let db = Database::open_in_memory().unwrap();
        let repo = McpProfileRepository::new(db.connection());
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
        repo.save(&profile).unwrap();
        let loaded = repo.get_by_name("assistant").unwrap().unwrap();
        assert!(!loaded.enabled);
        assert_eq!(loaded.persistent_access, PersistentAccess::ReadOnly);
        assert_eq!(loaded.selectors.len(), 1);
    }
}
