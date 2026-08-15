use rusqlite::{Connection, params};

pub struct SnippetRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SnippetRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn upsert(&self, id: &str, name: &str, body: &str) -> anyhow::Result<()> {
        self.upsert_for_project(id, None, name, body)
    }

    pub fn upsert_for_project(
        &self,
        id: &str,
        project_id: Option<&str>,
        name: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO snippets (id, name, body, created_at, project_id)
             VALUES (?1, ?2, ?3, datetime('now'), ?4)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, body = excluded.body, project_id = excluded.project_id",
            params![id, name, body, project_id],
        )?;
        Ok(())
    }

    pub fn get_body(&self, id: &str) -> anyhow::Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT body FROM snippets WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
    }

    pub fn list(&self) -> anyhow::Result<Vec<(String, String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, body FROM snippets ORDER BY name")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_for_project(&self, project_id: &str) -> anyhow::Result<Vec<(String, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, body FROM snippets WHERE project_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn rename(&self, id: &str, name: &str) -> anyhow::Result<()> {
        let changed = self
            .conn
            .execute("UPDATE snippets SET name = ?1 WHERE id = ?2", params![name, id])?;
        if changed == 0 {
            anyhow::bail!("unknown snippet {id}");
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
        Ok(())
    }
}
