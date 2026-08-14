pub const LATEST_SCHEMA_VERSION: u32 = 7;

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

pub const MIGRATION_5: &str = r#"
BEGIN;
CREATE TABLE mcp_profiles(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  enabled INTEGER NOT NULL DEFAULT 0,
  persistent_access TEXT NOT NULL,
  max_rows INTEGER NOT NULL,
  max_bytes INTEGER NOT NULL,
  timeout_secs INTEGER NOT NULL,
  max_concurrency INTEGER NOT NULL,
  query_mode TEXT NOT NULL,
  audit_retention_days INTEGER NOT NULL,
  connections_json TEXT NOT NULL
);
CREATE TABLE mcp_selectors(
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL,
  effect TEXT NOT NULL,
  pattern TEXT NOT NULL,
  FOREIGN KEY(profile_id) REFERENCES mcp_profiles(id) ON DELETE CASCADE
);
CREATE TABLE mcp_tool_rules(
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL,
  tool TEXT NOT NULL,
  allowed INTEGER NOT NULL,
  FOREIGN KEY(profile_id) REFERENCES mcp_profiles(id) ON DELETE CASCADE
);
INSERT INTO schema_migrations(version, applied_at) VALUES(5, datetime('now'));
COMMIT;
"#;

pub const MIGRATION_6: &str = r#"
BEGIN;
CREATE TABLE mcp_grants(
  id TEXT PRIMARY KEY,
  profile_name TEXT NOT NULL,
  connection_name TEXT NOT NULL,
  capability TEXT NOT NULL,
  tools_json TEXT NOT NULL,
  selectors_json TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  remaining_uses INTEGER NOT NULL,
  revision INTEGER NOT NULL,
  revoked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE mcp_operations(
  profile_name TEXT NOT NULL,
  session_id TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  tool TEXT NOT NULL,
  payload_hash TEXT NOT NULL,
  state TEXT NOT NULL,
  side_effect TEXT NOT NULL,
  result TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  PRIMARY KEY (profile_name, session_id, operation_id)
);
CREATE TABLE mcp_audit(
  id TEXT PRIMARY KEY,
  timestamp INTEGER NOT NULL,
  json TEXT NOT NULL
);
INSERT INTO schema_migrations(version, applied_at) VALUES(6, datetime('now'));
COMMIT;
"#;

pub const MIGRATION_7: &str = r#"
BEGIN;
CREATE TABLE workbench_layouts(
  project_id TEXT PRIMARY KEY,
  version INTEGER NOT NULL,
  json TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id)
);
CREATE TABLE session_recovery(
  id INTEGER PRIMARY KEY CHECK (id = 1),
  clean_shutdown INTEGER NOT NULL DEFAULT 1,
  layout_json TEXT,
  tx_state TEXT NOT NULL DEFAULT 'idle',
  updated_at TEXT NOT NULL
);
INSERT INTO schema_migrations(version, applied_at) VALUES(7, datetime('now'));
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
        current = 4;
    }
    if current < 5 {
        conn.execute_batch(MIGRATION_5)?;
        current = 5;
    }
    if current < 6 {
        conn.execute_batch(MIGRATION_6)?;
        current = 6;
    }
    if current < 7 {
        conn.execute_batch(MIGRATION_7)?;
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
        assert_eq!(read_schema_version(&conn), 7);
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
        assert_eq!(read_schema_version(&conn), 7);
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
        assert_eq!(read_schema_version(&conn), 7);
        let name: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='schema_diff_snapshots'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "schema_diff_snapshots");
    }

    #[test]
    fn migrates_v4_to_v5() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATION_1).unwrap();
        conn.execute_batch(MIGRATION_2).unwrap();
        conn.execute_batch(super::MIGRATION_3).unwrap();
        conn.execute_batch(super::MIGRATION_4).unwrap();
        assert_eq!(read_schema_version(&conn), 4);
        apply_pending(&conn).unwrap();
        assert_eq!(read_schema_version(&conn), 7);
        let name: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='mcp_profiles'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "mcp_profiles");
    }

    #[test]
    fn migrates_v5_to_v6() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATION_1).unwrap();
        conn.execute_batch(MIGRATION_2).unwrap();
        conn.execute_batch(super::MIGRATION_3).unwrap();
        conn.execute_batch(super::MIGRATION_4).unwrap();
        conn.execute_batch(super::MIGRATION_5).unwrap();
        assert_eq!(read_schema_version(&conn), 5);
        apply_pending(&conn).unwrap();
        assert_eq!(read_schema_version(&conn), 7);
        let name: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='mcp_grants'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "mcp_grants");
    }

    #[test]
    fn migrates_v6_to_v7() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATION_1).unwrap();
        conn.execute_batch(MIGRATION_2).unwrap();
        conn.execute_batch(super::MIGRATION_3).unwrap();
        conn.execute_batch(super::MIGRATION_4).unwrap();
        conn.execute_batch(super::MIGRATION_5).unwrap();
        conn.execute_batch(super::MIGRATION_6).unwrap();
        assert_eq!(read_schema_version(&conn), 6);
        apply_pending(&conn).unwrap();
        assert_eq!(read_schema_version(&conn), 7);
        let name: String = conn
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='workbench_layouts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "workbench_layouts");
    }
}
