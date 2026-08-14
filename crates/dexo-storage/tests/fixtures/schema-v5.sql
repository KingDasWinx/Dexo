-- sanitized
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
