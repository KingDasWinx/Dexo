-- sanitized
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
