-- sanitized
BEGIN;
ALTER TABLE connections ADD COLUMN group_path TEXT;
ALTER TABLE connections ADD COLUMN policy_json TEXT NOT NULL DEFAULT '{}';
CREATE TABLE connection_secret_refs(
  connection_id TEXT NOT NULL,
  purpose TEXT NOT NULL,
  secret_ref TEXT NOT NULL,
  PRIMARY KEY(connection_id, purpose),
  FOREIGN KEY(connection_id) REFERENCES connections(id) ON DELETE CASCADE
);
INSERT INTO connection_secret_refs(connection_id,purpose,secret_ref)
  SELECT id,'database_password',secret_ref FROM connections;
INSERT INTO schema_migrations(version,applied_at) VALUES(8,datetime('now'));
COMMIT;
