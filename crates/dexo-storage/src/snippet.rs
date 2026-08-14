use rusqlite::{Connection, params};

pub struct SnippetRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SnippetRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn upsert(&self, id: &str, name: &str, body: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO snippets (id, name, body, created_at)
             VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, body = excluded.body",
            params![id, name, body],
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
}
