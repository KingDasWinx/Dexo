# 1.4.2 Section 4: Results and Data Implementation Plan

> **For agentic workers:** executed natively on `development`, one commit per task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The grid stops losing to the basics: free WHERE and ORDER BY, header sort, row counts that say what they know, the rows that point at a row, and saved queries.

**Architecture:** A table document and a query result share one pair of input bars and one sort. The text in the ORDER BY bar *is* the sort: a header click rewrites it, and the header reads its markers back from it. Both run server-side -- a table document through `DataRequest`, which gains raw clauses (`RawClauses`, never filled by MCP), a result through `derive_page_in`. Counts are a driver estimate plus an exact `COUNT(*)` on a runner of its own, so it never takes the one live-query slot. Relations come from a new `CatalogReader::foreign_keys` that lists both directions. Saved queries are a `dexo-storage` table and a picker overlay.

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`, section 4 (C1, C2, A3, C3, C4). Index and release-wide rules: `README.md` here.

## Global Constraints

- Every Submit/Cancel dialog: arrows walk the buttons, Esc cancels (`widgets::form::footer_key`).
- Menus and the palette show each action's hotkey; new actions are in the palette.
- Every document belongs to a connection; offline actions connect by themselves.
- Secrets never go in SQLite, TOML, argv, logs or panic reports.
- TUI, CLI and MCP go through `dexo-app`; drivers do not import UI crates.
- New dependencies come from crates.io and pass `cargo deny check`.
- CI gates before each commit: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- From section 1: anything Dexo runs again on its own is a read; a clause that is not is refused before it is sent.
- Tests where a safety property (a raw clause that writes, a count that leaks into the live slot) or a driver contract has no other check.

## Rulings made from the code

- **Raw clauses are a field, not a `Filter`.** `DataRequest.clauses: RawClauses { where_sql, order_by }`, default empty; MCP builds `DataRequest`s with typed filters only and never sets it. Each driver puts `WHERE (raw) AND typed` and `ORDER BY raw` in its fetch. Before either is sent, the TUI checks it: no `;`, and `SELECT * FROM t WHERE <raw> ORDER BY <raw>` must pass `dexo_sql::inspect_read` (which also refuses side-effecting functions). Refused text stays in the bar with the reason in Messages; the last good page stays on screen.
- **One bar row.** `WHERE [...]  ORDER BY [...]` share one row above the grid, shown whenever the grid can re-run (a table document, or a result with a source statement). `w` focuses WHERE and `o` ORDER BY (free in every profile's results context); Enter applies, Esc reverts to the last applied text. "Filter" and "Sort" in the palette focus the bars; the old one-column prompt goes.
- **Header sort.** Click a header, or `s` on the focused column: ascending, descending, none. Shift-click or `S` adds the column after the others. Headers show `▲1 ▼2` (`^1 v2` without Unicode). An ORDER BY Dexo cannot read back as a column list shows no markers, and a header click replaces it.
- **Counts.** A table document's page says `843 rows` when the page holds them all, `~4.3M rows` from the driver's estimate (`pg_class.reltuples`, `information_schema.TABLES.TABLE_ROWS`, `sqlite_stat1`) when nothing filters it, and `300+ rows` otherwise. `t` runs the exact count in the background; `t` again cancels it. Drivers report a result cut at the row limit (`ResultSetFinished.truncated`, found by reading one row past it), shown as `10,000+ rows, limit reached`.
- **Relations.** `CatalogReader::foreign_keys(table)` returns every foreign key from or to the table with its columns (`dexo_driver_api::ForeignKeyRef`). "Related…" (`f` on a row) lists both directions -- `→ customers (customer_id)`, `← order_items (order_id)` -- and opens the chosen table filtered to the row, with `b` back. This also revives Open Related, whose foreign key was never set outside tests.
- **Saved queries.** Migration 14 adds `saved_queries(id, project_id, connection_id, name, sql, created_at, updated_at)`, unique by project, connection and name. Save Query As takes the selection or the document; Open Saved Query is a picker filtered as you type, with the highlighted query's SQL beside it, Enter opening it in a new document of that connection, F2 renaming and Delete asking first.

## Tasks

### Task 1 (C1): WHERE and ORDER BY bars
- `RawClauses` on `DataRequest`; the three drivers' fetches; `derive_page_in` taking the clauses; the bars, their keys and palette entries; the read check.
- Commit: `feat(results): WHERE and ORDER BY bars over the grid`.

### Task 2 (C2): Header sort
- Click and `s`/`S` cycle the sort through the ORDER BY text; markers in the header.
- Commit: `feat(results): sort by clicking a column header`.

### Task 3 (A3): Honest row counts
- `ResultSetFinished.truncated` in the three drivers; `DataMutator::estimate_rows`; the count runner and `t`; the counts in the results title.
- Commit: `feat(results): row counts say whether they are exact, estimated or open`.

### Task 4 (C3): Referenced by
- `CatalogReader::foreign_keys` in the three drivers; the Related picker; opening and Back.
- Commit: `feat(results): open the rows a row points at, or that point at it`.

### Task 5 (C4): Saved queries
- Migration 14 and `SavedQueryRepository`; storage worker commands; Save Query As and the picker.
- Commit: `feat(editor): saved queries`.

## Review Focus

1. A raw WHERE or ORDER BY never runs anything but a read, on any driver, and never reaches MCP.
2. A count never cancels or replaces a running query, and cancelling it stops the server's work.
3. "10,000+" appears only when there were more rows; `843 rows` only when it is exact.
4. Related opens the right rows for composite and NULL keys, and Back returns to the row it came from.
5. Saved queries stay with their project and connection; renaming and deleting never touch another's.
