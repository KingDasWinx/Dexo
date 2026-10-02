# 1.4.2 Section 7: DuckDB Implementation Plan

> **For agentic workers:** executed natively on `development`, one commit per task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** DuckDB is a fourth driver: open a DuckDB file, or a CSV, Parquet or JSON file directly, browse its catalog, run queries, read and edit rows, and see its plans in the Explain view.

**Architecture:** A `dexo-driver-duckdb` crate on `duckdb` with the bundled engine and its Parquet extension, shaped like the SQLite driver (a file, one connection behind a mutex, blocking calls on `spawn_blocking`). `dexo` registers it behind its `duckdb` cargo feature. `dexo_sql::Dialect` gains `Duckdb`, parsed with sqlparser's `DuckDbDialect` (FROM-first queries, `EXCLUDE`, `FROM 'file.csv'`) and quoted like Postgres.

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`, section 7 (D3). Index and release-wide rules: `README.md` here.

## Global Constraints

- Every Submit/Cancel dialog: arrows walk the buttons, Esc cancels (`widgets::form::footer_key`).
- Menus and the palette show each action's hotkey; new actions are in the palette.
- Every document belongs to a connection; offline actions connect by themselves.
- Secrets never go in SQLite, TOML, argv, logs or panic reports.
- TUI, CLI and MCP go through `dexo-app`; drivers do not import UI crates.
- New dependencies come from crates.io and pass `cargo deny check`.
- CI gates before each commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- From section 1: a read-only profile opens DuckDB with `AccessMode::ReadOnly`; the new dialect has its arm in `is_read` and `destructive`, and `editor_dialect` maps `duckdb`.

## Rulings made from the code

- **A DuckDB connection is a path,** like SQLite (`DriverDescriptor::file`), or `:memory:`. A path ending in `.csv`, `.tsv`, `.parquet`, `.json` or `.jsonl` opens an in-memory database with a view named after the file (`read_csv_auto`, `read_parquet`, `read_json_auto`), so the file browses like a table; `dexo duckdb:///data/sales.parquet` opens it straight from the command line. Any query can also read files itself (`SELECT * FROM 'other.csv'`).
- **Read-only:** a read-only profile opens with `AccessMode::ReadOnly` (a CSV/Parquet view is read-only by nature). A request asked to only read (`QueryRequest::read_only`) runs, outside a transaction, inside one rolled back after it; inside the user's, as part of it (DuckDB has no savepoints).
- **Catalog** from `duckdb_databases()`, `duckdb_schemas()`, `duckdb_tables()`, `duckdb_views()`, `duckdb_columns()`, `duckdb_indexes()` and `duckdb_constraints()` (foreign keys); system schemas only when asked. Row estimates from `duckdb_tables().estimated_size`.
- **Explain:** `EXPLAIN (FORMAT JSON)`, and `EXPLAIN ANALYZE (FORMAT JSON)` with timings and actual rows, mapped onto `PlanNode` (`name`, `extra_info`, `children`); hypothetical indexes are refused like the other non-Postgres drivers.
- **Rows** are edited by primary key, else by DuckDB's `rowid`.
- **Release targets:** the feature is built where CI shows the build time and binary size acceptable; the README lists those targets. Until measured, release builds leave it off and the README says how to build with it.

## Tasks

### Task 1: The dialect
- `Dialect::Duckdb` in `dexo-sql` with every match arm (parse, quote, guard, split, format, order, derived), `dialect_for_driver("duckdb")`, `editor_dialect`.
- Commit: `feat(sql): DuckDB's SQL is read as DuckDB writes it`.

### Task 2: The driver
- The crate: factory (files, `:memory:`, read-only), session (queries with the row limit, cancel, transactions), catalog, explain, data (fetch, apply, estimates), `DriverDescriptor::duckdb()`; `dexo`'s `duckdb` feature registers it; connection URLs take `duckdb://`.
- Commit: `feat(duckdb): open DuckDB files, CSV, Parquet and JSON, with catalog, rows and plans`.

### Task 3: Docs and release targets
- `docs/src/connections.md`, `docs/src/drivers.md`, README's target list; the measured build time and size.
- Commit: `docs: DuckDB, and the release targets that have it`.

## Review Focus

1. A read-only profile can never write a DuckDB file, through any path (queries, the grid, the bars, the schema editor).
2. A CSV/Parquet file is never written or replaced.
3. The catalog, rows and plans are right for DuckDB's own types (HUGEINT, DECIMAL, LIST, STRUCT, MAP, UUID, INTERVAL, TIMESTAMP WITH TIME ZONE).
4. Nothing DuckDB-specific leaks into the other drivers' behaviour, and the binary without the feature is unchanged.
