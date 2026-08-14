-- sanitized
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
