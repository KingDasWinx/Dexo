# Connections, TLS, SSH, and keychain

Dexo connects to PostgreSQL, MySQL, MariaDB and SQLite. MariaDB goes through the MySQL driver; pick `mariadb` in the form so the connection says what it is, though a MariaDB server reached as `mysql` behaves the same.

Connections store host, port, database, user, and driver options in SQLite. The password lives in the native keychain behind an opaque `secret_ref`.

TLS verifies certificates by default. Custom CA and client certificates are supported. Disabling verification is explicit and shown as a persistent warning.

SSH tunnels verify known hosts. A new or changed host key requires confirmation and is never accepted automatically.

If the keychain is missing or locked, Dexo asks for the secret for the current session. It does not write a file vault.

## Password managers

A connection can take its password from a password manager's command instead of the keychain: set `password_command` under the form's advanced options, or pass `--password-command` to `dexo connections add`.

```sh
dexo connections add --name shop --driver postgres --host db --username ana --database shop \
  --password-command 'op read op://dev/shop/password'
```

Dexo runs it through `sh -c` (`cmd /C` on Windows) on every connect, off the screen's thread, and uses what it prints, without the trailing line break. The output stays in memory; nothing goes to the keychain. A command that fails, prints nothing or takes longer than 30 seconds fails the connection with a message naming the command, never its output, and what it prints to stderr is discarded.

## Temporary connections

`dexo <url>` opens the workbench connected to a URL without saving it -- `postgres://user:password@host:5432/db`, `postgresql://`, `mysql://`, `mariadb://` or `sqlite:///path/to/file` -- and `dexo --demo` opens a sample shop in SQLite. The connection is marked temporary and is gone when Dexo closes; its password is kept in memory only. "Save Connection…" in the palette, or editing it, opens the connection form filled in, and saving keeps it like any other, with the password going to the keychain.

`sslmode` (Postgres) and `ssl-mode` (MySQL) in the URL set the TLS mode. `--password-prompt` asks for the password on the terminal, which keeps it out of your shell history.

## SQLite

A SQLite connection is a file. Pick the `SQLite` driver in the connection form and give the file's path; there is no host, port, user or password, and nothing goes to the keychain. From the command line:

```sh
dexo connections add --name shop --driver sqlite --path ./shop.db
```

The path is stored absolute. Opening a file that does not exist creates it, except on a read-only connection, which refuses. The catalog shows `main` and any database you `ATTACH`, with tables, views, columns, indexes, foreign keys and triggers. Rows are edited by primary key, or by `rowid` when a table has none. Explain shows `EXPLAIN QUERY PLAN`; SQLite has no `EXPLAIN ANALYZE`, and the schema editor and administration screens do not apply to it.

## Environments and the SQL editor

Each connection has an environment: local, development, staging or production. A label Dexo does not know, such as `prod`, counts as production.

- On a read-only connection, the editor refuses any statement that is not a read and sends nothing. `SET`, transaction commands and anything Dexo cannot parse count as writes.
- On production, any write asks for the connection's name, typed exactly, before it runs.
- Elsewhere, `DELETE` or `UPDATE` without `WHERE`, `DROP`, `TRUNCATE` and `ALTER ... DROP` ask first. Turn this off with the connection's `confirm_destructive` setting.

A read-only connection is also enforced by the server: Postgres sessions start with `default_transaction_read_only`, MySQL and MariaDB sessions with `SET SESSION TRANSACTION READ ONLY`, and SQLite opens the file read-only, which covers anything it attaches, so neither `PRAGMA query_only = 0` nor an `ATTACH` can write.

A statement Dexo cannot read counts as a write: production asks for the name, and elsewhere it asks before running, like a destructive statement. Maintenance it knows -- `VACUUM`, `ANALYZE`, `REINDEX`, `CLUSTER`, `REFRESH MATERIALIZED VIEW`, `CHECKPOINT`, `OPTIMIZE` -- asks only on production.
