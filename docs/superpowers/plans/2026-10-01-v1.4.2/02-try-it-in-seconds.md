# 1.4.2 Section 2: Try It in Seconds Implementation Plan

> **For agentic workers:** executed natively on `development`, one commit per task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Anyone can try Dexo without a server (SQLite, `dexo --demo`), connect from a URL without saving a profile, use MariaDB as a supported database, and keep passwords in a password manager.

**Architecture:** A new `dexo-driver-sqlite` crate on the workspace's bundled `rusqlite`, run on blocking threads. `DriverDescriptor` learns which drivers open a file instead of a host. A temporary connection is a `ConnectionProfile` that lives only in the TUI model, with its password in the session's in-memory secret store; the demo is a temporary SQLite connection on a seeded temp file. MariaDB is the MySQL driver under its own descriptor. A password command is a profile field the connection manager runs instead of reading the keyring.

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`, section 2 (D2, D4, A2, D1, D7). Index and release-wide rules: `README.md` here.

## Global Constraints

- Every Submit/Cancel dialog: arrows walk the buttons, Esc cancels.
- Secrets never go in SQLite, TOML, argv, logs or panic reports. A URL password and a password command's output live in memory only.
- TUI, CLI and MCP go through `dexo-app`; drivers do not import UI crates.
- New dependencies only from crates.io and only if unavoidable; `cargo deny check` passes.
- From section 1: a new driver enforces read-only on the server side or refuses a read-only profile (SQLite opens `SQLITE_OPEN_READ_ONLY`); a new `dexo_sql::Dialect` variant gets arms in `is_read`, `destructive` and the splitter, and `screens::editor::editor_dialect` maps its driver id.
- From section 1: a temporary connection has no saved profile, so `run_policy` fails closed (destructive statements confirmed); that is intended.
- CI gates before each commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Tests only where a safety or security property, or a driver contract, has no other check.

## Order

D2 → A2 → D4 → D1 → D7. Ruling: A2 goes before D4 because the demo is a temporary connection, so the temporary-connection mechanism lands first.

## Interfaces fixed up front

- `dexo_driver_api::DriverDescriptor` gains `pub file: bool` (`true` for SQLite and, later, DuckDB) and `DriverDescriptor::sqlite()` (`id: "sqlite"`, `display_name: "SQLite"`, `default_port: 0`, no TLS/SSH/proxy options). `for_id` knows it.
- A file-based profile's `config` is `{"path": "<absolute path>"}`. `ConnectionProfile::connect_request` for a file driver needs no host, user or secret: `endpoint` is the path, `database` is `None`, `username` is empty, the secret is empty.
- The TUI's secret lookup (`runtime/connection_manager.rs`) asks nothing for a file driver.
- `dexo_sql::Dialect::Sqlite` (sqlparser `SQLiteDialect`); the TUI maps driver `sqlite` to it.
- `dexo_driver_sqlite::SqliteFactory`, registered in `crates/dexo/src/main.rs`.

## Tasks

### Task 1 (D2): SQLite driver
- Crate `crates/dexo-driver-sqlite`: factory; session with `execute` (streamed result sets, `row_limit`, parameters), `cancel` through `InterruptHandle`, `close`; `TransactionControl`; `CatalogReader` (one catalog `main` → tables and views → columns, indexes, triggers, foreign keys; DDL from `sqlite_master.sql`; dependencies from foreign keys); `DataMutator` (fetch with typed filters and sort, fetch_value, apply with primary keys or rowid, table_columns); `ExplainProvider` (`EXPLAIN QUERY PLAN` as a tree; ANALYZE unavailable); `BulkWriter`; capabilities that hide what it cannot do (Admin, Ddl planning, ExplainAnalyze).
- `Dialect::Sqlite` in `dexo-sql` and every exhaustive match on `Dialect` and on `dexo_app::data::SqlDialect`.
- Connection form: driver `sqlite` shows a file path field instead of host, port, user and password.
- Contract and integration tests on a temp file.
- Commit: `feat(drivers): SQLite`.

### Task 2 (A2): Temporary connections from a URL
- `dexo <url>` with `postgres://`, `postgresql://`, `mysql://`, `mariadb://`, `sqlite:///path`; `duckdb:///path` says DuckDB arrives with its driver. Percent-decoded user and password; `--password-prompt` asks on the terminal instead.
- The password is put in the session's in-memory secret store only; stderr notes that the URL may be in shell history.
- The TUI starts connected to a profile that exists only in the model, marked temporary in the explorer, with a palette action "Save Connection…" that opens the connection form prefilled, and saves through the normal path (password to the keyring).
- Commit: `feat(cli): dexo <url> opens a temporary connection`.

### Task 3 (D4): Demo mode
- `dexo --demo`: a temp SQLite file seeded from an embedded script (customers, products, orders, order_items with foreign keys, a few hundred deterministic rows), opened as a temporary connection named `demo`.
- Commit: `feat(cli): dexo --demo opens a seeded store to try every screen`.

### Task 4 (D1): MariaDB
- `DriverDescriptor::mariadb()`; the MySQL factory serves both ids; version detection; catalog and explain differences fixed (MariaDB has no `EXPLAIN ANALYZE FORMAT=TREE`); a MariaDB container in the integration test support and CI; README compatibility updated.
- Commit: `feat(drivers): MariaDB is supported`.

### Task 5 (D7): Password command
- Profile config `password_command`; the connection manager runs it through the platform shell (`sh -c`, `cmd /C`) with a 30-second timeout; trimmed stdout is the secret, in memory only; a non-zero exit or timeout fails the connection with a message naming the command, never its output. Connection form field; CLI `connections add --password-command`.
- Commit: `feat(connections): take a password from a command`.

## Review Focus

1. A URL password never reaches SQLite, recovery checkpoints, logs or the panic report.
2. A read-only SQLite profile cannot write, even through `ATTACH` or `PRAGMA`.
3. A password command that hangs or prints to stderr does not block the UI or leak its output.
4. Documents opened on a temporary connection survive a restart without a dangling connection.
5. MariaDB catalog and explain on a real server.
