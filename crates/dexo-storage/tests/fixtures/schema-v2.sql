-- sanitized
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
