use rusqlite::{Connection, params};

pub struct HistoryRepository<'a> {
    conn: &'a Connection,
}

impl<'a> HistoryRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn insert(&self, id: &str, connection_id: Option<&str>, sql: &str) -> anyhow::Result<()> {
        self.insert_scoped(id, None, connection_id, sql)
    }

    pub fn insert_scoped(
        &self,
        id: &str,
        project_id: Option<&str>,
        connection_id: Option<&str>,
        sql: &str,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO sql_history (id, connection_id, sql, created_at, project_id)
             VALUES (?1, ?2, ?3, datetime('now'), ?4)",
            params![id, connection_id, sql, project_id],
        )?;
        Ok(())
    }

    pub fn prune(&self, max_count: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM sql_history WHERE id NOT IN (
                SELECT id FROM sql_history ORDER BY created_at DESC LIMIT ?1
             )",
            params![max_count],
        )?;
        Ok(())
    }

    pub fn count(&self) -> anyhow::Result<i64> {
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM sql_history", [], |row| row.get(0))?;
        Ok(count)
    }

    pub fn list(&self, connection_id: Option<&str>) -> anyhow::Result<Vec<(String, String)>> {
        if let Some(connection_id) = connection_id {
            let mut stmt = self.conn.prepare(
                "SELECT id, sql FROM sql_history WHERE connection_id = ?1 ORDER BY created_at DESC",
            )?;
            let rows =
                stmt.query_map(params![connection_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let mut stmt = self
                .conn
                .prepare("SELECT id, sql FROM sql_history ORDER BY created_at DESC")?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    pub fn clear_for_connection(&self, connection_id: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM sql_history WHERE connection_id = ?1",
            params![connection_id],
        )?;
        Ok(())
    }

    pub fn list_for_project(&self, project_id: &str) -> anyhow::Result<Vec<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, sql FROM sql_history WHERE project_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![project_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn clear_for_project(&self, project_id: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM sql_history WHERE project_id = ?1",
            params![project_id],
        )?;
        Ok(())
    }
}
