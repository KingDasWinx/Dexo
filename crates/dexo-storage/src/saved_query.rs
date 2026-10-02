use rusqlite::{Connection, ErrorCode, OptionalExtension, params};

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

    /// Saves `sql` under `name`, and says whether it replaced a query: one of that name,
    /// in any case, for the same project and connection, keeping its id.
    pub fn save(
        &self,
        project_id: &str,
        connection_id: &str,
        name: &str,
        sql: &str,
    ) -> anyhow::Result<(SavedQuery, bool)> {
        let find = || {
            self.conn
                .query_row(
                    "SELECT id, project_id, connection_id, name, sql, updated_at FROM saved_queries
                     WHERE project_id = ?1 AND connection_id = ?2 AND name = ?3",
                    params![project_id, connection_id, name],
                    row_to_query,
                )
                .optional()
        };
        let replaced = find()?.is_some();
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO saved_queries (id, project_id, connection_id, name, sql, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), datetime('now'))
             ON CONFLICT(project_id, connection_id, name)
             DO UPDATE SET name = excluded.name, sql = excluded.sql, updated_at = excluded.updated_at",
            params![id, project_id, connection_id, name, sql],
        )?;
        let saved = find()?.ok_or_else(|| anyhow::anyhow!("the saved query is gone"))?;
        Ok((saved, replaced))
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
        let deleted = self.conn.execute(
            "DELETE FROM saved_queries WHERE id = ?1 AND project_id = ?2",
            params![id, project_id],
        )?;
        if deleted == 0 {
            anyhow::bail!("the saved query is gone");
        }
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
        for connection in ["c1", "c2"] {
            conn.execute(
                "INSERT INTO connections (id, name, driver, environment, config_json, secret_ref)
                 VALUES (?1, ?1, 'postgres', 'local', '{}', 'ref')",
                [connection],
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
        let (first, replaced) = repo.save("p1", "c1", "Top customers", "select 1").unwrap();
        assert!(!replaced);
        // The name is the same in another case.
        let (again, replaced) = repo.save("p1", "c1", "top CUSTOMERS", "select 2").unwrap();
        assert!(replaced);
        assert_eq!(first.id, again.id);
        assert_eq!(again.sql, "select 2");
        repo.save("p1", "c2", "Top customers", "select 3").unwrap();
        repo.save("p2", "c1", "Top customers", "select 4").unwrap();
        assert_eq!(repo.list_for_project("p1").unwrap().len(), 2);

        let (other, _) = repo.save("p1", "c1", "Late orders", "select 5").unwrap();
        assert!(repo.rename("p1", &other.id, "TOP customers").is_err());
        assert!(repo.rename("p2", &other.id, "Stolen").is_err());
        repo.rename("p1", &other.id, "Overdue orders").unwrap();
        assert!(repo.delete("p2", &other.id).is_err());
        let names: Vec<String> = repo
            .list_for_project("p1")
            .unwrap()
            .into_iter()
            .map(|query| query.name)
            .collect();
        assert_eq!(names, ["Overdue orders", "top CUSTOMERS", "Top customers"]);
        repo.delete("p1", &other.id).unwrap();
        assert!(
            repo.delete("p1", &other.id).is_err(),
            "a stale id is not deleted"
        );
        assert_eq!(repo.list_for_project("p1").unwrap().len(), 2);
        assert_eq!(repo.list_for_project("p2").unwrap()[0].sql, "select 4");
    }

    /// A query belongs to a saved connection, and goes with it.
    #[test]
    fn a_deleted_connection_takes_its_queries() {
        let conn = database();
        let repo = SavedQueryRepository::new(&conn);
        assert!(repo.save("p1", "not-saved", "q", "select 1").is_err());
        repo.save("p1", "c1", "q", "select 1").unwrap();
        repo.save("p1", "c2", "q", "select 1").unwrap();
        conn.execute("DELETE FROM connections WHERE id = 'c1'", [])
            .unwrap();
        let left = repo.list_for_project("p1").unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].connection_id, "c2");
    }
}
