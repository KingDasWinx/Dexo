pub const LATEST_SCHEMA_VERSION: u32 = 4;

pub const MIGRATION_1: &str = r#"
BEGIN;
CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
CREATE TABLE projects(id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE TABLE connections(id TEXT PRIMARY KEY, project_id TEXT, name TEXT NOT NULL,
  driver TEXT NOT NULL, environment TEXT NOT NULL, config_json TEXT NOT NULL,
  secret_ref TEXT NOT NULL, FOREIGN KEY(project_id) REFERENCES projects(id));
CREATE TABLE recovery_documents(id TEXT PRIMARY KEY, project_id TEXT NOT NULL,
  title TEXT NOT NULL, content TEXT NOT NULL, updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id));
INSERT INTO schema_migrations(version, applied_at) VALUES(1, datetime('now'));
COMMIT;
"#;

pub const MIGRATION_2: &str = r#"
BEGIN;
CREATE TABLE sql_history(
  id TEXT PRIMARY KEY,
  connection_id TEXT,
  sql TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE snippets(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE documents(
  id TEXT PRIMARY KEY,
  project_id TEXT,
  title TEXT NOT NULL,
  content TEXT NOT NULL,
  path TEXT,
  mtime TEXT,
  content_hash TEXT,
  updated_at TEXT NOT NULL
);
INSERT INTO schema_migrations(version, applied_at) VALUES(2, datetime('now'));
COMMIT;
"#;

pub const MIGRATION_3: &str = r#"
BEGIN;
CREATE TABLE catalog_snapshots(
  id TEXT PRIMARY KEY,
  connection_id TEXT NOT NULL,
  database_name TEXT NOT NULL,
  complete INTEGER NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE catalog_objects(
  snapshot_id TEXT NOT NULL,
  object_id TEXT NOT NULL,
  parent_id TEXT,
  kind TEXT NOT NULL,
  qualified_name TEXT NOT NULL,
  json TEXT NOT NULL,
  stale INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (snapshot_id, object_id),
  FOREIGN KEY (snapshot_id) REFERENCES catalog_snapshots(id) ON DELETE CASCADE
);
INSERT INTO schema_migrations(version, applied_at) VALUES(3, datetime('now'));
COMMIT;
"#;

pub const MIGRATION_4: &str = r#"
BEGIN;
CREATE TABLE schema_diff_snapshots(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  driver TEXT NOT NULL,
  json TEXT NOT NULL,
  digest TEXT NOT NULL,
  created_at TEXT NOT NULL
);
INSERT INTO schema_migrations(version, applied_at) VALUES(4, datetime('now'));
COMMIT;
"#;

pub fn read_schema_version(conn: &rusqlite::Connection) -> u32 {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get::<_, i64>(0),
    )
    .ok()
    .and_then(|v| u32::try_from(v).ok())
    .unwrap_or(0)
}

pub fn apply_pending(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    let mut current = read_schema_version(conn);
    if current < 1 {
        conn.execute_batch(MIGRATION_1)?;
        current = 1;
    }
    if current < 2 {
        conn.execute_batch(MIGRATION_2)?;
        current = 2;
    }
    if current < 3 {
        conn.execute_batch(MIGRATION_3)?;
        current = 3;
    }
    if current < 4 {
        conn.execute_batch(MIGRATION_4)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MIGRATION_1, MIGRATION_2, apply_pending, read_schema_version};

    #[test]
    fn migrates_v1_to_v2() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATION_1).unwrap();
        assert_eq!(read_schema_version(&conn), 1);
        apply_pending(&conn).unwrap();
        assert_eq!(read_schema_version(&conn), 4);
        let name: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='sql_history'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "sql_history");
    }

    #[test]
    fn migrates_v2_to_v3() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATION_1).unwrap();
        conn.execute_batch(MIGRATION_2).unwrap();
        assert_eq!(read_schema_version(&conn), 2);
        apply_pending(&conn).unwrap();
        assert_eq!(read_schema_version(&conn), 4);
        let name: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='catalog_snapshots'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "catalog_snapshots");
    }

    #[test]
    fn migrates_v3_to_v4() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATION_1).unwrap();
        conn.execute_batch(MIGRATION_2).unwrap();
        conn.execute_batch(super::MIGRATION_3).unwrap();
        assert_eq!(read_schema_version(&conn), 3);
        apply_pending(&conn).unwrap();
        assert_eq!(read_schema_version(&conn), 4);
        let name: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='schema_diff_snapshots'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "schema_diff_snapshots");
    }
}
