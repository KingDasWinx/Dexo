use dexo_app::schema_diff::{SchemaSnapshot, SnapshotError};
use rusqlite::{Connection, OptionalExtension, params};

pub struct SchemaSnapshotStore<'a> {
    conn: &'a Connection,
}

impl<'a> SchemaSnapshotStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn save(&self, name: &str, snapshot: &SchemaSnapshot) -> anyhow::Result<String> {
        snapshot.verify()?;
        let id = uuid::Uuid::new_v4().to_string();
        // ponytail: store uncompressed JSON until snapshot size is measured; add deflate when payloads exceed a few MB.
        self.conn.execute(
            "INSERT INTO schema_diff_snapshots(id, name, driver, json, digest, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![
                id,
                name,
                snapshot.driver,
                serde_json::to_string(snapshot)?,
                snapshot.digest,
            ],
        )?;
        Ok(id)
    }

    pub fn load_by_name(&self, name: &str) -> anyhow::Result<Option<SchemaSnapshot>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT json FROM schema_diff_snapshots WHERE name = ?1 ORDER BY created_at DESC LIMIT 1",
                params![name],
                |row| row.get(0),
            )
            .optional()?;
        let Some(json) = json else {
            return Ok(None);
        };
        let snapshot: SchemaSnapshot = serde_json::from_str(&json)?;
        snapshot.verify().map_err(anyhow::Error::from)?;
        Ok(Some(snapshot))
    }

    pub fn load_json(json: &str) -> Result<SchemaSnapshot, SnapshotError> {
        let snapshot: SchemaSnapshot =
            serde_json::from_str(json).map_err(|_| SnapshotError::Tampered)?;
        snapshot.verify()?;
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::SchemaSnapshotStore;
    use dexo_app::schema_diff::SchemaSnapshot;
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::migrations::apply_pending(&conn).unwrap();
        conn
    }

    #[test]
    fn persists_and_rejects_tampered_payload() {
        let conn = db();
        let store = SchemaSnapshotStore::new(&conn);
        let snapshot = SchemaSnapshot::capture(
            "postgres",
            "16",
            "2026-08-14T00:00:00Z",
            "db",
            vec![CatalogObject::new(
                ObjectId::new("t"),
                ObjectKind::Table,
                QualifiedName::new(Some("db"), Some("public"), "t"),
                None,
            )],
        );
        store.save("prod", &snapshot).unwrap();
        let loaded = store.load_by_name("prod").unwrap().unwrap();
        assert_eq!(loaded.digest, snapshot.digest);
        assert!(SchemaSnapshotStore::load_json(r#"{"format_version":1,"driver":"postgres","server_version":"16","captured_at":"x","scope":"db","objects":[],"digest":"deadbeef"}"#).is_err());
    }
}
