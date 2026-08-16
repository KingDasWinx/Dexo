-- sanitized
BEGIN;
CREATE TABLE object_usage(
  project_id TEXT NOT NULL,
  connection_id TEXT NOT NULL,
  object_id TEXT NOT NULL,
  favorite INTEGER NOT NULL DEFAULT 0,
  opened_count INTEGER NOT NULL DEFAULT 0,
  last_opened_at TEXT,
  PRIMARY KEY(project_id,connection_id,object_id)
);
INSERT INTO schema_migrations(version,applied_at) VALUES(10,datetime('now'));
COMMIT;
