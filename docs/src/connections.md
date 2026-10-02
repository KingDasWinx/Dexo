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

Dexo runs it through `sh -c` (`cmd /C` on Windows) on every connect, off the screen's thread, and uses what it prints, without the trailing line break. On the command line it can ask on the terminal (a GPG passphrase, say); in the workbench, which owns the terminal, it runs without one, so a manager that has to ask should ask in a window, or be unlocked in a shell first. The output stays in memory; nothing goes to the keychain. A command that fails, prints nothing or takes longer than 30 seconds fails the connection with a message naming the command, never its output, and what it prints to stderr is discarded.

## Pre-connect commands

Some databases are only reachable through a tunnel another tool opens: `kubectl port-forward`, `cloud-sql-proxy`, a Teleport or Boundary tunnel. Set `pre_connect` under the form's advanced options and Dexo runs it before every connect:

```sh
kubectl port-forward -n shop svc/postgres ${port}:5432
```

`${port}` becomes a free port on this machine, and the connection dials `127.0.0.1` on it; verified TLS still checks the certificate against the host the profile names. Without `${port}`, Dexo waits for the profile's own host and port to open. It waits 30 seconds; a command that exits first fails the connect with its exit status and the last line it printed on stderr.

The command runs through the shell in a process group of its own, and lives as long as the session: closing the session, a failed connect, quitting Dexo, or Dexo being told to stop (`SIGTERM`, `SIGHUP`, and Ctrl+C on the command line) stops it and everything it started. It has to stay in the foreground -- `ssh -N`, not `ssh -fN`, and no trailing `&` -- or Dexo refuses it, since nothing would be left to stop. A tunnel that dies mid-session is what the next query says. It cannot be combined with an SSH tunnel or a proxy on the same profile.

An MCP agent is told which command failed, never the command line or what it printed; `dexo connections test <name>` shows you the rest. A config file you import lists every command its connections would run before you apply it.

## Databases running in Docker

The connections screen lists the Postgres, MySQL and MariaDB containers running in Docker that publish their port, under "Running in Docker", with their driver and `host:port`; `r` looks again, and "Find Databases in Docker" in the palette opens the screen. Enter on one opens the connection form filled in from the container's environment -- user, database and password -- and nothing is saved until you save it. A container made with only a root password gets the server's `mysql` schema; one that lets its user in without a password (`MYSQL_ALLOW_EMPTY_PASSWORD`, `POSTGRES_HOST_AUTH_METHOD=trust`) saves with an empty one. A container a saved connection already dials is not listed again.

Dexo only reads Docker (`docker ps`, `docker inspect`), with three seconds for all of it; no `docker` on the PATH, or a daemon that does not answer, is an empty list. With a remote daemon (`DOCKER_HOST`, or a remote `docker context`), the host is the daemon's.

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
