use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryDocument {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub content: String,
    pub updated_at: String,
}

pub struct RecoveryRepository<'a> {
    conn: &'a Connection,
}

impl<'a> RecoveryRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn checkpoint(
        &self,
        id: &str,
        project_id: &str,
        title: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        // ponytail: persist document SQL only; callers must not pass secrets or parameter values.
        self.conn.execute(
            "INSERT INTO recovery_documents (id, project_id, title, content, updated_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
               project_id = excluded.project_id,
               title = excluded.title,
               content = excluded.content,
               updated_at = excluded.updated_at",
            params![id, project_id, title, content],
        )?;
        Ok(())
    }

    pub fn load(&self, id: &str) -> anyhow::Result<Option<RecoveryDocument>> {
        self.conn
            .query_row(
                "SELECT id, project_id, title, content, updated_at
                 FROM recovery_documents WHERE id = ?1",
                params![id],
                row_to_document,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_for_project(&self, project_id: &str) -> anyhow::Result<Vec<RecoveryDocument>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, title, content, updated_at
             FROM recovery_documents WHERE project_id = ?1 ORDER BY title",
        )?;
        let rows = stmt.query_map(params![project_id], row_to_document)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn clear(&self, id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM recovery_documents WHERE id = ?1", params![id])?;
        Ok(())
    }
}

fn row_to_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecoveryDocument> {
    Ok(RecoveryDocument {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        content: row.get(3)?,
        updated_at: row.get(4)?,
    })
}
