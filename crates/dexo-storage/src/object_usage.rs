use rusqlite::{Connection, params};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectUsage {
    pub project_id: String,
    pub connection_id: String,
    pub object_id: String,
    pub favorite: bool,
    pub opened_count: i64,
    pub last_opened_at: Option<String>,
}

pub struct ObjectUsageRepository<'a> {
    conn: &'a Connection,
}

impl<'a> ObjectUsageRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn set_favorite(
        &self,
        project_id: &str,
        connection_id: &str,
        object_id: &str,
        favorite: bool,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO object_usage(project_id, connection_id, object_id, favorite, opened_count, last_opened_at)
             VALUES (?1, ?2, ?3, ?4, 0, NULL)
             ON CONFLICT(project_id, connection_id, object_id)
             DO UPDATE SET favorite = excluded.favorite",
            params![project_id, connection_id, object_id, i64::from(favorite)],
        )?;
        Ok(())
    }

    pub fn touch(
        &self,
        project_id: &str,
        connection_id: &str,
        object_id: &str,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO object_usage(project_id, connection_id, object_id, favorite, opened_count, last_opened_at)
             VALUES (?1, ?2, ?3, 0, 1, datetime('now'))
             ON CONFLICT(project_id, connection_id, object_id)
             DO UPDATE SET opened_count = opened_count + 1, last_opened_at = datetime('now')",
            params![project_id, connection_id, object_id],
        )?;
        Ok(())
    }

    pub fn list_for_connection(
        &self,
        project_id: &str,
        connection_id: &str,
    ) -> anyhow::Result<Vec<ObjectUsage>> {
        let mut stmt = self.conn.prepare(
            "SELECT project_id, connection_id, object_id, favorite, opened_count, last_opened_at
             FROM object_usage
             WHERE project_id = ?1 AND connection_id = ?2
             ORDER BY object_id",
        )?;
        let rows = stmt.query_map(params![project_id, connection_id], |row| {
            Ok(ObjectUsage {
                project_id: row.get(0)?,
                connection_id: row.get(1)?,
                object_id: row.get(2)?,
                favorite: row.get::<_, i64>(3)? != 0,
                opened_count: row.get(4)?,
                last_opened_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::ObjectUsageRepository;
    use crate::{ConnectionRepository, Database, ProjectRepository};
    use dexo_app::{ConnectionId, ConnectionProfile, Project, ProjectId, SecretRef};

    fn seed() -> (Database, String, String) {
        let db = Database::open_in_memory().unwrap();
        let project_id = uuid::Uuid::new_v4();
        let connection_id = uuid::Uuid::new_v4();
        ProjectRepository::new(db.connection())
            .save(&Project {
                id: ProjectId(project_id),
                name: "p".into(),
                created_at: "now".into(),
            })
            .unwrap();
        ConnectionRepository::new(db.connection())
            .save(&ConnectionProfile::new(
                ConnectionId(connection_id),
                Some(project_id),
                "c",
                "postgres",
                "local",
                serde_json::json!({"host": "localhost"}),
                SecretRef::new("ref-1".into()),
            ))
            .unwrap();
        (db, project_id.to_string(), connection_id.to_string())
    }

    #[test]
    fn set_favorite_and_touch_roundtrip() {
        let (db, project_id, connection_id) = seed();
        let repo = ObjectUsageRepository::new(db.connection());
        repo.set_favorite(&project_id, &connection_id, "obj:orders", true)
            .unwrap();
        repo.touch(&project_id, &connection_id, "obj:orders")
            .unwrap();
        repo.touch(&project_id, &connection_id, "obj:users")
            .unwrap();
        let rows = repo
            .list_for_connection(&project_id, &connection_id)
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].favorite);
        assert_eq!(rows[0].opened_count, 1);
        assert!(rows[0].last_opened_at.is_some());
        assert!(!rows[1].favorite);
        assert_eq!(rows[1].opened_count, 1);
    }

    #[test]
    fn project_delete_cleans_object_usage() {
        let (db, project_id, connection_id) = seed();
        let repo = ObjectUsageRepository::new(db.connection());
        repo.set_favorite(&project_id, &connection_id, "obj:orders", true)
            .unwrap();
        ProjectRepository::new(db.connection())
            .delete(ProjectId(uuid::Uuid::parse_str(&project_id).unwrap()))
            .unwrap();
        assert!(
            repo.list_for_connection(&project_id, &connection_id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn connection_delete_cleans_object_usage() {
        let (db, project_id, connection_id) = seed();
        let repo = ObjectUsageRepository::new(db.connection());
        repo.touch(&project_id, &connection_id, "obj:orders")
            .unwrap();
        ConnectionRepository::new(db.connection())
            .delete(ConnectionId(uuid::Uuid::parse_str(&connection_id).unwrap()))
            .unwrap();
        assert!(
            repo.list_for_connection(&project_id, &connection_id)
                .unwrap()
                .is_empty()
        );
    }
}
