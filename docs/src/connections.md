# Connections, TLS, SSH, and keychain

Dexo connects to PostgreSQL, MySQL and MariaDB. MariaDB goes through the MySQL driver; pick `mariadb` in the form so the connection says what it is, though a MariaDB server reached as `mysql` behaves the same.

Connections store host, port, database, user, and driver options in SQLite. The password lives in the native keychain behind an opaque `secret_ref`.

TLS verifies certificates by default. Custom CA and client certificates are supported. Disabling verification is explicit and shown as a persistent warning.

SSH tunnels verify known hosts. A new or changed host key requires confirmation and is never accepted automatically.

If the keychain is missing or locked, Dexo asks for the secret for the current session. It does not write a file vault.

## Environments and the SQL editor

Each connection has an environment: local, development, staging or production. A label Dexo does not know, such as `prod`, counts as production.

- On a read-only connection, the editor refuses any statement that is not a read and sends nothing. `SET`, transaction commands and anything Dexo cannot parse count as writes.
- On production, any write asks for the connection's name, typed exactly, before it runs.
- Elsewhere, `DELETE` or `UPDATE` without `WHERE`, `DROP`, `TRUNCATE` and `ALTER ... DROP` ask first. Turn this off with the connection's `confirm_destructive` setting.

A read-only connection is also enforced by the server: Postgres sessions start with `default_transaction_read_only`, MySQL and MariaDB sessions with `SET SESSION TRANSACTION READ ONLY`.

A statement Dexo cannot read counts as a write: production asks for the name, and elsewhere it asks before running, like a destructive statement. Maintenance it knows -- `VACUUM`, `ANALYZE`, `REINDEX`, `CLUSTER`, `REFRESH MATERIALIZED VIEW`, `CHECKPOINT`, `OPTIMIZE` -- asks only on production.
