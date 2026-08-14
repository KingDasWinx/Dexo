use dexo_app::{Project, ProjectId};
use rusqlite::{Connection, OptionalExtension, params};

pub struct ProjectRepository<'a> {
    conn: &'a Connection,
}

impl<'a> ProjectRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
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
}

fn parse_uuid(value: &str) -> rusqlite::Result<uuid::Uuid> {
    uuid::Uuid::parse_str(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}
