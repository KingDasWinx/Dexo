-- sanitized
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
