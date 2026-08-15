-- sanitized
BEGIN;
ALTER TABLE snippets ADD COLUMN project_id TEXT;
ALTER TABLE sql_history ADD COLUMN project_id TEXT;
CREATE TABLE recent_items(
  project_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  item_id TEXT NOT NULL,
  opened_at TEXT NOT NULL,
  PRIMARY KEY(project_id,kind,item_id),
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE TABLE project_state(
  project_id TEXT PRIMARY KEY,
  active_document_id TEXT,
  active_connection_id TEXT,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);
UPDATE snippets SET project_id = (
  SELECT id FROM projects WHERE name = 'Default' ORDER BY created_at LIMIT 1
) WHERE project_id IS NULL;
UPDATE sql_history SET project_id = (
  SELECT id FROM projects WHERE name = 'Default' ORDER BY created_at LIMIT 1
) WHERE project_id IS NULL;
INSERT INTO schema_migrations(version,applied_at) VALUES(9,datetime('now'));
COMMIT;
