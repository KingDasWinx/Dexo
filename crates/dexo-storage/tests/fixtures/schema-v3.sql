-- sanitized
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
