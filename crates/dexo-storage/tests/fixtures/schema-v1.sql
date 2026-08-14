-- sanitized
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
