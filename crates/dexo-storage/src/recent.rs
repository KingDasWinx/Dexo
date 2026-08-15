use rusqlite::{Connection, params};

pub struct RecentItemsRepository<'a> {
    conn: &'a Connection,
}

impl<'a> RecentItemsRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn touch(&self, project_id: &str, kind: &str, item_id: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO recent_items (project_id, kind, item_id, opened_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(project_id, kind, item_id) DO UPDATE SET opened_at = excluded.opened_at",
            params![project_id, kind, item_id],
        )?;
        Ok(())
    }

    pub fn list(&self, project_id: &str) -> anyhow::Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, item_id, opened_at FROM recent_items WHERE project_id = ?1 ORDER BY opened_at DESC",
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn clear(&self, project_id: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM recent_items WHERE project_id = ?1",
            params![project_id],
        )?;
        Ok(())
    }
}
