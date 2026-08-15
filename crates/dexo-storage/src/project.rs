use dexo_app::{Project, ProjectId};
use rusqlite::{Connection, OptionalExtension, params};

pub struct ProjectRepository<'a> {
    conn: &'a Connection,
}

impl<'a> ProjectRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn create(&self, name: &str) -> anyhow::Result<Project> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("project name is required");
        }
        if self.get_by_name(name)?.is_some() {
            anyhow::bail!("project '{name}' already exists");
        }
        let project = Project {
            id: ProjectId(uuid::Uuid::new_v4()),
            name: name.to_string(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs().to_string())
                .unwrap_or_else(|_| "0".into()),
        };
        self.save(&project)?;
        Ok(project)
    }

    pub fn save(&self, project: &Project) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO projects (id, name, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, created_at = excluded.created_at",
            params![project.id.0.to_string(), project.name, project.created_at],
        )?;
        Ok(())
    }

    pub fn get(&self, id: ProjectId) -> anyhow::Result<Option<Project>> {
        self.conn
            .query_row(
                "SELECT id, name, created_at FROM projects WHERE id = ?1",
                params![id.0.to_string()],
                |row| {
                    let id: String = row.get(0)?;
                    Ok(Project {
                        id: ProjectId(parse_uuid(&id)?),
                        name: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list(&self) -> anyhow::Result<Vec<Project>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, created_at FROM projects ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            Ok(Project {
                id: ProjectId(parse_uuid(&id)?),
                name: row.get(1)?,
                created_at: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_by_name(&self, name: &str) -> anyhow::Result<Option<Project>> {
        self.conn
            .query_row(
                "SELECT id, name, created_at FROM projects WHERE name = ?1",
                params![name],
                |row| {
                    let id: String = row.get(0)?;
                    Ok(Project {
                        id: ProjectId(parse_uuid(&id)?),
                        name: row.get(1)?,
                        created_at: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn rename(&self, id: ProjectId, name: &str) -> anyhow::Result<()> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("project name is required");
        }
        if let Some(existing) = self.get_by_name(name)?
            && existing.id != id
        {
            anyhow::bail!("project '{name}' already exists");
        }
        let changed = self.conn.execute(
            "UPDATE projects SET name = ?1 WHERE id = ?2",
            params![name, id.0.to_string()],
        )?;
        if changed == 0 {
            anyhow::bail!("unknown project {}", id.0);
        }
        Ok(())
    }

    pub fn preview_delete(&self, id: ProjectId) -> anyhow::Result<ProjectDeletePreview> {
        let pid = id.0.to_string();
        Ok(ProjectDeletePreview {
            connections: count(
                self.conn,
                "SELECT COUNT(*) FROM connections WHERE project_id = ?1",
                &pid,
            )?,
            documents: count(
                self.conn,
                "SELECT COUNT(*) FROM documents WHERE project_id = ?1",
                &pid,
            )?,
            snippets: count(
                self.conn,
                "SELECT COUNT(*) FROM snippets WHERE project_id = ?1",
                &pid,
            )?,
            external_paths: {
                let mut stmt = self.conn.prepare(
                    "SELECT path FROM documents WHERE project_id = ?1 AND path IS NOT NULL AND path <> ''",
                )?;
                let rows = stmt.query_map(params![pid], |row| row.get::<_, String>(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            },
        })
    }

    pub fn delete(&self, id: ProjectId) -> anyhow::Result<()> {
        let pid = id.0.to_string();
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let result = (|| {
            self.conn.execute(
                "UPDATE connections SET project_id = NULL WHERE project_id = ?1",
                params![&pid],
            )?;
            self.conn
                .execute("DELETE FROM documents WHERE project_id = ?1", params![&pid])?;
            self.conn
                .execute("DELETE FROM snippets WHERE project_id = ?1", params![&pid])?;
            self.conn.execute(
                "DELETE FROM sql_history WHERE project_id = ?1",
                params![&pid],
            )?;
            self.conn.execute(
                "DELETE FROM recent_items WHERE project_id = ?1",
                params![&pid],
            )?;
            self.conn.execute(
                "DELETE FROM object_usage WHERE project_id = ?1",
                params![&pid],
            )?;
            self.conn.execute(
                "DELETE FROM project_state WHERE project_id = ?1",
                params![&pid],
            )?;
            self.conn.execute(
                "DELETE FROM recovery_documents WHERE project_id = ?1",
                params![&pid],
            )?;
            self.conn.execute(
                "DELETE FROM workbench_layouts WHERE project_id = ?1",
                params![&pid],
            )?;
            let deleted = self
                .conn
                .execute("DELETE FROM projects WHERE id = ?1", params![&pid])?;
            if deleted == 0 {
                anyhow::bail!("unknown project {}", id.0);
            }
            Ok(())
        })();
        match result {
            Ok(()) => self.conn.execute_batch("COMMIT;")?,
            Err(_) => {
                let _ = self.conn.execute_batch("ROLLBACK;");
            }
        }
        result
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectDeletePreview {
    pub connections: usize,
    pub documents: usize,
    pub snippets: usize,
    pub external_paths: Vec<String>,
}

fn count(conn: &Connection, sql: &str, id: &str) -> anyhow::Result<usize> {
    let n: i64 = conn.query_row(sql, params![id], |row| row.get(0))?;
    Ok(n as usize)
}

fn parse_uuid(value: &str) -> rusqlite::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
