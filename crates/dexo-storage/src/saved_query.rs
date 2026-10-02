use rusqlite::{Connection, ErrorCode, params};

/// Named SQL kept for a project and a connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedQuery {
    pub id: String,
    pub project_id: String,
    pub connection_id: String,
    pub name: String,
    pub sql: String,
    pub updated_at: String,
}

pub struct SavedQueryRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SavedQueryRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Saves `sql` under `name`. A query of that name for the same project and
    /// connection is replaced, keeping its id.
    pub fn save(
        &self,
        project_id: &str,
        connection_id: &str,
        name: &str,
        sql: &str,
    ) -> anyhow::Result<SavedQuery> {
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO saved_queries (id, project_id, connection_id, name, sql, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))
             ON CONFLICT(project_id, connection_id, name)
             DO UPDATE SET sql = excluded.sql, updated_at = excluded.updated_at",
            params![id, project_id, connection_id, name, sql],
        )?;
        self.conn
            .query_row(
                "SELECT id, project_id, connection_id, name, sql, updated_at FROM saved_queries
                 WHERE project_id = ?1 AND connection_id = ?2 AND name = ?3",
                params![project_id, connection_id, name],
                row_to_query,
            )
            .map_err(Into::into)
    }

    /// The project's queries, for every connection, by name.
    pub fn list_for_project(&self, project_id: &str) -> anyhow::Result<Vec<SavedQuery>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, connection_id, name, sql, updated_at FROM saved_queries
             WHERE project_id = ?1 ORDER BY name COLLATE NOCASE, connection_id",
        )?;
        let rows = stmt.query_map(params![project_id], row_to_query)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Renames the project's query `id`. A name another query of its connection holds is
    /// refused; another project's query is never touched.
    pub fn rename(&self, project_id: &str, id: &str, name: &str) -> anyhow::Result<()> {
        let changed = self
            .conn
            .execute(
                "UPDATE saved_queries SET name = ?1, updated_at = datetime('now')
                 WHERE id = ?2 AND project_id = ?3",
                params![name, id, project_id],
            )
            .map_err(|error| match error.sqlite_error_code() {
                Some(ErrorCode::ConstraintViolation) => {
                    anyhow::anyhow!("a saved query of this connection is already called {name}")
                }
                _ => error.into(),
            })?;
        if changed == 0 {
            anyhow::bail!("the saved query is gone");
        }
        Ok(())
    }

    pub fn delete(&self, project_id: &str, id: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM saved_queries WHERE id = ?1 AND project_id = ?2",
            params![id, project_id],
        )?;
        Ok(())
    }
}

fn row_to_query(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedQuery> {
    Ok(SavedQuery {
        id: row.get(0)?,
        project_id: row.get(1)?,
        connection_id: row.get(2)?,
        name: row.get(3)?,
        sql: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::SavedQueryRepository;

    fn database() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        for project in ["p1", "p2"] {
            conn.execute(
                "INSERT INTO projects (id, name, created_at) VALUES (?1, ?1, datetime('now'))",
                [project],
            )
            .unwrap();
        }
        conn
    }

    /// A query stays with its project and connection: the same name elsewhere is
    /// another query, saving again under a name replaces it, and renaming or deleting
    /// through another project touches nothing.
    #[test]
    fn saved_queries_stay_with_their_project_and_connection() {
        let conn = database();
        let repo = SavedQueryRepository::new(&conn);
        let first = repo.save("p1", "c1", "Top customers", "select 1").unwrap();
        let again = repo.save("p1", "c1", "Top customers", "select 2").unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(again.sql, "select 2");
        repo.save("p1", "c2", "Top customers", "select 3").unwrap();
        repo.save("p2", "c1", "Top customers", "select 4").unwrap();
        assert_eq!(repo.list_for_project("p1").unwrap().len(), 2);

        let other = repo.save("p1", "c1", "Late orders", "select 5").unwrap();
        assert!(repo.rename("p1", &other.id, "Top customers").is_err());
        assert!(repo.rename("p2", &other.id, "Stolen").is_err());
        repo.rename("p1", &other.id, "Overdue orders").unwrap();
        repo.delete("p2", &other.id).unwrap();
        let names: Vec<String> = repo
            .list_for_project("p1")
            .unwrap()
            .into_iter()
            .map(|query| query.name)
            .collect();
        assert_eq!(names, ["Overdue orders", "Top customers", "Top customers"]);
        repo.delete("p1", &other.id).unwrap();
        assert_eq!(repo.list_for_project("p1").unwrap().len(), 2);
        assert_eq!(repo.list_for_project("p2").unwrap()[0].sql, "select 4");
    }
}
