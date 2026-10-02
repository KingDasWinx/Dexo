# Line-based CLI clients vs Dexo: pgcli, mycli, litecli, usql, sq

Research date: 2026-10-02. Clones: `/home/winx/Documents/dexo-examples-projects/tui/{pgcli,mycli,litecli,usql,sq}`.
Dexo baseline: `development` @ 642fb76 (1.4.2). Each "Dexo lacks" claim below was checked against `docs/src/` and `crates/` (paths cited).

## 1. Projects

| Project | Stars | Last activity | Lang | What it is |
|---|---|---|---|---|
| [dbcli/pgcli](https://github.com/dbcli/pgcli) | 13,412 | v4.7.1, 2026-09-20 | Python (prompt_toolkit) | A Postgres REPL with the best context-aware completion in the field: FK-aware JOINs, `*` expansion, ranking by how often you use a name. It uses `pgspecial` for psql `\` commands. |
| [dbcli/mycli](https://github.com/dbcli/mycli) | 11,979 | v2.28.0, 2026-10-02 (very active, ~weekly releases) | Python | The MySQL/MariaDB/TiDB sibling. It now goes further than pgcli: frecency completion, enum completion, Jinja favorites, shell redirects (`$>`, `$|`), fzf history/output explorer, Polars transforms/plots, `/llm`, and Vault/Boundary/kubectl tunnels. |
| [dbcli/litecli](https://github.com/dbcli/litecli) | 3,310 | last push 2026-06-18; PyPI 1.19.0 (2026-01-30) | Python | The SQLite sibling, with dot commands (`.tables`, `.schema`, `.import`, `.load`, `.once`), sqlean extensions and `\llm`. It is slowing down. |
| [xo/usql](https://github.com/xo/usql) | 10,135 | v0.21.6, 2026-09-22 | Go | A "universal psql" with ~50 drivers through `dburl` URL schemes. It is psql-compatible (`\set`, `\if`, `\gexec`, `\watch`, `\crosstab`) and adds a cross-database `\copy`, `\chart` (Kitty/iTerm/Sixel) and `.usqlpass`. |
| [neilotoole/sq](https://github.com/neilotoole/sq) | 2,571 | v0.55.0, 2026-09-10; nightly site commits | Go | "jq for databases": you add sources (DBs, CSV, XLSX, JSON, remote URLs) by handle. It has a jq-like query language plus native SQL, joins across sources, `--insert @src.table`, `diff` of schema *and data*, and ~12 output formats including Markdown/HTML/XLSX and a Mermaid ERD. |

## 2. Features Dexo lacks

Effort is for Dexo's codebase: S ≈ a day or two, M ≈ a week, L = more.

### 2a. Connecting (first five minutes)

| Feature | Who has it (evidence) | Value for Dexo's users | Effort | Notes |
|---|---|---|---|---|
| **Read libpq's own config: `~/.pgpass`, `pg_service.conf` (`service=` / `PGSERVICE`), `PGHOST/PGUSER/PGDATABASE/PGPASSWORD/PGSSLMODE`** | pgcli: README "environment variables", `pgcli/main.py:2238` `parse_service_info`, changelog 4.6.0 (.pgpass over SSH via `hostaddr`). psql users already have these files. | High. Every psql user already has these files and doesn't want to "define them twice" ([pgcli#1624](https://github.com/dbcli/pgcli/issues/1624), [#1474](https://github.com/dbcli/pgcli/issues/1474)). Dexo uses `tokio-postgres`, which reads none of them; nothing in `crates/` or `docs/` mentions them (only `native_tool.rs` writes a temp pgpass for pg_dump). | S–M | Apply to `dexo <url>` and as a fallback secret for Postgres profiles without one. Also add an "Import from pg_service.conf" action that creates saved connections. Enforce the 0600 permission check like libpq. |
| **MySQL `~/.my.cnf [client]` / `mysql_config_editor` login-path** | mycli: `--login-path` (TIPS), `password_sources = … login_path …` (`mycli/myclirc`) | Medium | S (my.cnf), M (login-path is an AES-obfuscated file) | Same idea as pgpass for MySQL users. |
| **`dexo <saved-name>`, bare paths (`dexo shop.db`, `dexo sales.parquet`), short schemes (`pg://`, `my://`, `sq:`)** | usql: README "Database Connection Strings", "Paths on Disk", "Short Aliases" (dburl). `litecli FILE`. `sq add ./sakila.db` sniffs the type. | High. These are first-five-minutes wins. Today `connection_url.rs:36-46` rejects anything without `scheme://` and accepts 6 schemes, and `crates/dexo/src/main.rs:35` treats the argument only as a URL, so `dexo prod` fails. | S | Order: saved name → existing file (sniff SQLite header / extension) → URL. |
| **CLI subcommands accept a URL, not only a saved name** (`dexo query --connection postgres://…`) | `usql DSN -c "…"`, `pgcli postgres://… -c`, `sq --src` | High for CI, containers and one-off scripts, where nothing is saved. Every subcommand resolves `--connection` with `get_by_name` only (`run.rs:285, 404, 627, 738, 1110, 1211, 1325, 1347, 1535`). | S | Reuse `temporary_connection`. A URL has no environment, so keep `--confirm` for destructive statements and allow `?mode=ro`. |
| **Open a CSV/TSV/JSON file as a table in the release binary** | sq (csv/tsv/json/jsonl/xlsx drivers, `README.md` "Drivers"), usql `csvq`, litecli `.import` | High. "Query a CSV" is sq's whole pitch and a classic Show-HN demo. Dexo can do it only through DuckDB, which release binaries leave out (`docs/src/install.md:45`). | M | Cheapest path: an in-memory SQLite plus Dexo's existing import pipeline (`transfer/detect.rs` already sniffs delimiters and headers), or rusqlite's `csvtab` feature. |
| **Per-connection startup SQL** (`SET search_path`, `SET ROLE`, `statement_timeout`, `sql_mode`) | pgcli `[init-commands]`, `[alias_dsn.init-commands]`, `--init-command` (`pgcli/pgclirc`, changelog 4.4.0). mycli `--init-command`. usql `init:` in config.yaml. | Medium–high (multi-schema Postgres, RLS roles) | S | Hold it to the connection policy: a write on a read-only connection is refused. List it in config-import review the way `pre_connect` is. |

### 2b. Completion (pgcli's signature)

| Feature | Who has it (evidence) | Value | Effort | Notes |
|---|---|---|---|---|
| **Whole JOIN clause after `JOIN`**: offer `orders ON orders.customer_id = c.id` for tables that have an FK to a table already in scope, ranked first | pgcli `pgcompleter.py:571-610` (`get_join_matches`) | High. This is the most-typed boilerplate in SQL. Dexo offers join conditions only after `ON` (`dexo-sql/src/completion.rs:272` `push_join_conditions`); after `JOIN` it lists all tables unranked. | S–M | Product rule: automatic aliases were removed for good. Insert the table name, or reuse an alias only when the user typed one. |
| **Name-based join conditions when no FK is declared** (same name and type, with integer key columns boosted) | pgcli `pgcompleter.py:652-660` (`"name join"`) | Medium–high. Many real schemas have no declared FKs (Rails/Django apps, MyISAM, analytics marts), and there Dexo offers nothing. | S | Rank below FK joins (pgcli uses 2000 vs 1000). |
| **`*` expansion**: Tab on `SELECT *` or `t.*` writes the explicit column list; in INSERT it skips columns with defaults | pgcli `pgcompleter.py:536-552`, `asterisk_column_order` in `pgclirc` | Medium | S | Dexo has nothing for `*`. |
| **Ranking by use, learned from executed SQL** (frecency of table and column names) | pgcli `packages/prioritization.py` (`PrevalenceCounter`, fed by `extend_query_history` at `pgcompleter.py:306`). mycli `completion_tiebreaker = frecency` (`myclirc`; changelog "Sort completion candidates by frecency from history"). | Medium–high | S–M | Dexo's `rank.rs` boosts favorites, recently *opened* tables, and a `_id` name heuristic. It never learns from the queries you run, and it has no column frequency. `sql_history` is already stored. |
| **Enum literal completion** (`WHERE status = '` lists the enum labels) | mycli `packages/completion_engine.py:396,643`, `sqlcompleter.py:1105` | Medium | M | Needs `pg_enum` / MySQL `ENUM(...)` labels in the catalog. |
| **Real index/PK/FK knowledge in ranking** (mycli marks indexed columns with a suffix) | mycli `indexed_column_suffix` (`myclirc`, `client.py:201`) | Low–medium | S | Dexo guesses key columns from names (`rank.rs` `is_key_column`). |
| **Keyword casing preference** (auto = follow what you type, lower, upper) | pgcli/mycli `keyword_casing = auto` | Low–medium. Users who write lowercase SQL get uppercase keywords pushed into their text (`completion.rs` `push_keywords`: "Keywords always go in capitals"). | S | One setting. |
| **Datatype completion** (`CAST(x AS `, `::`, column types in CREATE TABLE) | pgcli `get_datatype_matches` (`pgcompleter.py:858`) | Low–medium | S | No `Intent` for types in `dexo-sql/src/context.rs`. |
| **Inline history suggestion** (fish-style ghost text from history, accepted with →) | pgcli `auto_suggest = True` (`pgclirc`) | Low–medium | S–M | Dexo has a history search popup, but no inline suggestion. |

### 2c. CLI output and scripting

| Feature | Who has it (evidence) | Value | Effort | Notes |
|---|---|---|---|---|
| **Correct, readable CLI output.** CSV with RFC 4180 quoting, NULL distinct from empty, an aligned table, vertical/expanded output (`-x`, plus *auto* when wider than the terminal), `--no-header`/`-t`, Markdown/HTML | pgcli: 25 `table_format`s including `sql-insert`/`sql-update`, `-t` (4.7.0), auto-expand (`main.py:1355`). usql: `-A -H -J -C -G -x -t`, `\pset`. sq: text/json/jsona/jsonl/csv/tsv/markdown/html/xml/xlsx/yaml/raw (`cli/output/*w`). | **High, and a correctness bug today.** `dexo-cli/src/presenter.rs:84-104`: `write_delimited` joins raw values with `,`, so a value containing a comma, quote or newline corrupts `--format csv`. `table` is the same code with `\|`: no alignment, no header rule. NULL prints as an empty string in both. The `csv` crate is already used correctly in `dexo-app/src/transfer/codec.rs:167`. | S | Reuse the transfer CSV writer. Add an aligned table writer (unicode-width is already a dependency) with auto-vertical when the table is wider than the terminal. |
| **`--single-transaction` / `-1` for `dexo run`** (all or nothing) | usql `-1` (README "Command-line Options"), psql `-1` | High for migration scripts | S | Nothing similar in `args.rs` or `run.rs`. Wrap the script with the transaction service, and on error roll back and report which statement failed. |
| **Short flags and positional SQL** (`-c conn`, `-f file`, `-o out`, `dexo query shop "select 1"`) | pgcli `-c/-f/-y/-t` (changelog 4.7.0), usql, sq | Medium (ergonomics, copy-paste examples) | S | `args.rs` has 0 `short` flags. The help golden test (`crates/dexo-cli/tests/help.rs`) must be updated. |
| **Named CLI parameters / variables** | mycli favorites with Jinja `{{ kv.name }}` and `$1` (`packages/special/favoritequeries.py`). usql `\set` and `-v NAME=VALUE` with `:NAME` / `:'NAME'` interpolation (README "Variable Interpolation"). Asked for in [sq#583](https://github.com/neilotoole/sq/issues/583). | Medium | S | `run.rs:1371-1380` `parse_params` **drops the name** from `--param name=value` and binds positionally. The editor already knows `:name` placeholders (`dexo-sql/src/parameter.rs` `named_parameters`). Binding by name would match the editor and avoid wrong-order surprises. |
| **Streaming CLI output** (print rows as they arrive) | mycli `--unbuffered` (changelog 2.28: "streaming" state) | Medium | M | `run_query` gets every event back from `execute_script` before printing (`run.rs:999-1015`), capped by `max_rows`. |
| **MySQL `DELIMITER` in scripts** | mycli `/delimiter` (`packages/special/delimitercommand.py`, [mycli#383](https://github.com/dbcli/mycli/issues/383), 8👍) | Medium (mysqldump output with routines and triggers uses `DELIMITER ;;`) | S | Dexo's splitter handles `BEGIN…END` (`dexo-sql/src/statement.rs:43`) but has no `DELIMITER` client command, so `dexo run --file dump.sql` sends `DELIMITER ;;` to the server. |

### 2d. Saved queries, editor commands, data movement

| Feature | Who has it (evidence) | Value | Effort | Notes |
|---|---|---|---|---|
| **Saved queries outside the TUI.** `dexo query --saved NAME --param k=v`, `\n name [args]` in the editor, list/run from the CLI, a version-controlled shared file, and exposure to MCP agents as curated read tools/prompts | pgcli `\n`, `\ns`, `\nd`, `\ne` (`main.py:406-421`, changelog 4.6.0). mycli `/f`, `/fs`, `/favorite run\|eval\|edit` with Jinja and `shared_favorites_file` (`myclirc`, `favoritequeries.py`). usql's top open request is [usql#469 "Defined Queries"](https://github.com/xo/usql/issues/469) (6👍). | High. Dexo's saved queries are TUI-only (`docs/src/workbench.md` "Saved queries") and live in the SQLite store. | S (CLI + `\n`), M (file sync + MCP) | MCP "saved query as tool" gives agents vetted queries instead of raw SQL. It fits Dexo's guardrail story. |
| **`\watch N` / auto-refresh a result** | pgcli `\watch` (`main.py:1168`), mycli `/watch`, usql `\watch` (`metacmd/cmds.go:291`) | Medium (queue depth, replication lag, migration progress) | S–M | Reads only; stop on error or on a write. |
| **Surface MySQL server warnings** (`SHOW WARNINGS` when `warning_count > 0`) | mycli `show_warnings` / `\W` (`sqlexecute.py:467`, `client_query.py:123`), [mycli#555](https://github.com/dbcli/mycli/issues/555) (10👍) | Medium–high (data integrity: silent truncation under non-strict `sql_mode`) | S | No warning handling anywhere in `crates/dexo-driver-mysql/src`. Postgres notices already reach the Messages view. |
| **Send a result to an external program or pager** (`$\|`, `/once`, `\o \|cmd`, fzf/csvlens explorer) | mycli `packages/hybrid_redirection.py`, `iocommands.py:1002` (`set_pipe_once`), `doc/cookbook.md` (`pager = csvlens`). usql `\g \|cmd`, `\o \|cmd`. [pgcli#558](https://github.com/dbcli/pgcli/issues/558) `\copy … TO PROGRAM` (30👍, closed). | Medium (VisiData / csvlens / `jq` users) | S | Same suspend/resume mechanism as `edit_externally` (`dexo-tui/src/event.rs:230`). Write CSV or JSONL to the process's stdin. |
| **Copy a query result from connection A into a table on connection B** (create it if missing) | usql `\copy SRC DST QUERY TABLE(cols)` (README "Copying Between Databases", `metacmd/cmds.go:571`). sq `sql … --insert @handle.table` (`cli/cmd_sql.go:41`). | High (seed dev/staging from a prod subset, move SQLite → Postgres) | M | Reuse the import pipeline with a row-stream source. Hold it to the *destination* policy. Learn from usql's bugs: NULL conversion ([#397](https://github.com/xo/usql/issues/397)), batching ([#458](https://github.com/xo/usql/issues/458)), large inputs ([#322](https://github.com/xo/usql/issues/322)). |
| **Schema docs: `inspect --format markdown\|html\|mermaid`** with a Mermaid ER diagram | sq `inspect --markdown/--html/mermaid-erd` (`site/content/en/docs/inspect/index.md:99-175`, `cli/output/mermaidw`, `erdimgw`) | High. It closes Dexo's "no ER diagram" gap cheaply, renders on GitHub and in PRs, and Dexo's table and column **notes** become documentation. | S–M | Catalog and FKs are already there. No TUI rendering is needed. |
| **Row-level data diff** (rows added/removed/changed by PK) between tables or connections | sq `diff --data` (`site/content/en/docs/diff/index.md`) | Medium (verify ETL, compare staging and prod) | M | Dexo diffs schema only. |
| **Column profile** (nulls, distinct, min/max, top-k) | usql `\ss[+]` (`metacmd/cmds.go:939`) | Medium (exploration) | M | Nothing in Dexo. |
| **More psql commands**: `\du`, `\dx`, `\ds`, `\dT`, `\dp`, `\conninfo`, `\timing`, `\c db`, `\i file`, `\sf` | pgspecial via pgcli. usql `\?` (README "Backslash Commands"). | Low–medium (muscle memory) | S each, when the catalog has the data | Dexo answers 9 (`dexo-app/src/meta_command.rs:37-48`). |
| **SQLite extensions** (`.load`, sqlean, spatialite, sqlite-vec) and **SQLCipher** | litecli `.load`, sqlean (CHANGELOG 1.16.0). [litecli#42](https://github.com/dbcli/litecli/issues/42) SQLCipher, [#127](https://github.com/dbcli/litecli/issues/127) spatialite. | Medium | S (extension list per connection, loaded through the API, never via SQL `load_extension()`), M (SQLCipher build) | rusqlite is built `bundled` only (`Cargo.toml:25`). |
| **Display settings**: null text, number grouping, date/time format, local time zone | pgcli `null_string`, `[data_formats]`, `[column_date_formats]`, `use_local_timezone`. usql `\pset time`. sq `--format.datetime`. | Low–medium | S | No such settings found in Dexo. |
| **Notify when a long query finishes** | mycli `beep_after_seconds` (`client.py:120`) | Low–medium | S | BEL or OSC 9/777 after N seconds. Dexo shows the elapsed time only at the end (`update.rs:6565`). |
| **XLSX export** | sq `cli/output/xlsxw` | Medium (handing data to business users) | M | `rust_xlsxwriter`. |

## 3. Where they do better even though Dexo has the feature

- **Completion ranking.** Both tools are fuzzy, like Dexo. pgcli ranks by match quality, then type priority, then how often *you* used the name. mycli's match order is configurable (perfect, regex, under_words, camelCase, rapidfuzz), with a frecency tiebreak. Dexo's tiers (`rank.rs`) are good, but its boosts are static (favorite, recently opened, `_id`). After `JOIN`, pgcli writes the whole clause; Dexo waits for `ON`.
- **Connection strings.** usql's `dburl` understands ~50 schemes plus aliases, fills in defaults (`pg://` means `$USER` over `/var/run/postgresql`) and sniffs file paths. pgcli takes a URI *or* libpq `key=value` conninfo *or* `service=`. Dexo takes 6 schemes, needs `://`, and its CLI subcommands accept only saved names.
- **Scripting ergonomics.** pgcli 4.7 added `-c` (repeatable), `-f` (repeatable), `-y` and `-t`. usql has `-1`, `-v`, `-o` and `-q`. Dexo needs `--connection NAME --sql …`. `query` cannot read stdin (only `run` can), and there are no short flags.
- **Output.** Every tool in this group prints an aligned, correctly quoted table. Dexo's CLI table is pipe-joined text, and its CSV is unquoted (§2c).
- **Auto-expand.** pgcli and mycli switch to vertical output automatically when a row is wider than the terminal. Dexo's `\x` takes only `on|off`, with no `auto`, and the CLI has no vertical mode at all.
- **Saved queries.** pgcli and mycli favorites are reachable from any prompt, take positional and named arguments, can be edited in `$EDITOR`, and can live in a shared file. Dexo's are TUI-only and parameterless at save time.
- **Optional extra safety.** Dexo is already stronger overall: read-only is enforced on the server, production writes need the connection's name typed, and grid edits go through a change set. pgcli has two ideas worth considering as policy options:
  - `destructive_statements_require_transaction`: destructive statements run only inside an explicit transaction, so you can inspect the affected row count and roll back.
  - `destructive_warning_restarts_connection`: declining a destructive statement rolls back the open transaction.
- **History.** mycli searches history with fzf and a highlighted preview, and pgcli shows fish-style inline suggestions. Dexo's history search covers the core use, and `sql_history` stores SQL, connection and time only.

Where Dexo is already ahead, so don't copy backwards:
- `\d` and friends are answered from Dexo's own catalog on every database. usql's `\d` is missing on many drivers ([#375](https://github.com/xo/usql/issues/375), [#440](https://github.com/xo/usql/issues/440)).
- Secrets: keychain plus `password_command` covers mycli's Vault/Boundary integrations generically, and `pre_connect` covers kubectl and Boundary tunnels.
- Native backup and restore, schema diff and apply, EXPLAIN tools, the MCP guardrails, Vim mode (usql's #2 open request, [#236](https://github.com/xo/usql/issues/236), 5👍), and transactions with savepoints.

## 4. Most-wanted requests in their trackers

Open trackers are thin. pgcli's highest open item has 2👍. mycli has **0 open issues** because its maintainers close everything. usql peaks at 6. So the table includes the all-time top closed requests as the real demand signal. Only items Dexo does **not** fully answer are listed.

| Repo | Request | 👍 | State | Dexo today |
|---|---|---|---|---|
| usql | [Defined Queries](https://github.com/xo/usql/issues/469) | 6 | open | Partial: saved queries exist in the TUI only. No CLI, no `\n`, no MCP. |
| litecli | [`--wrap` / `--wordwrap` for wide text](https://github.com/dbcli/litecli/issues/136) | 5 | open | No: the CLI table neither aligns nor wraps. |
| pgcli | [`pgcli service=NAME`](https://github.com/dbcli/pgcli/issues/1474) | 2 | open | No `pg_service.conf` support. |
| litecli | [SQLCipher](https://github.com/dbcli/litecli/issues/42) | 2 | open | No. |
| litecli | [`.separator`](https://github.com/dbcli/litecli/issues/123) | 2 | open | No: the CLI has no field separator option. |
| pgcli | [Edit named queries in an external editor](https://github.com/dbcli/pgcli/issues/1430) | 2 | open | Partial: open the saved query, press Ctrl+E, save again. |
| usql | [Markdown table output](https://github.com/xo/usql/issues/482) | 1 | open | Partial: "Copy as Markdown" exists in the TUI; no CLI or export format. |
| litecli | [spatialite](https://github.com/dbcli/litecli/issues/127) / sq [#86](https://github.com/neilotoole/sq/issues/86) | 1 / 0 | open | No extension loading. |
| litecli | [`.eqp` (auto EXPLAIN every query)](https://github.com/dbcli/litecli/issues/144) | 1 | open | No: explain is on demand (F7). |
| sq | [Import large `.sql` files (SQLite)](https://github.com/neilotoole/sq/issues/430) | 1 | open | Partial: `dexo run --file` reads the whole file into memory (`run.rs:1390`). |
| sq | [Vertical mode output](https://github.com/neilotoole/sq/issues/530) | 0 (2 comments) | open | Partial: the TUI has a record view; the CLI has none. |
| sq | [Templated / parameterized raw SQL with env vars](https://github.com/neilotoole/sq/issues/583) | 0 | open | No: CLI `--param` ignores names. |
| pgcli | [Show service name in prompt](https://github.com/dbcli/pgcli/issues/1624), [env vars for SSH](https://github.com/dbcli/pgcli/issues/1433) | 0 (5 comments) | open | No service or env support. |
| pgcli | [`\copy … TO PROGRAM`](https://github.com/dbcli/pgcli/issues/558) | 30 | closed (shipped) | No pipe-to-program for results. |
| pgcli | [`\set` variables](https://github.com/dbcli/pgcli/issues/829) / [#1269](https://github.com/dbcli/pgcli/issues/1269) | 26 / 11 | closed (shipped) | Partial: the editor prompts for `:name` parameters; there are no session variables and no CLI names. |
| mycli | [`\w` / `\W` warnings](https://github.com/dbcli/mycli/issues/555) | 10 | closed (shipped) | No MySQL warnings shown. |
| mycli | [Set statement delimiter](https://github.com/dbcli/mycli/issues/383) | 8 | closed (shipped) | No `DELIMITER`. |

Already answered by Dexo, among the all-time top requests: SSH tunnel (pgcli#459, 31👍), transactions (pgcli#410, 15), piping a file in (pgcli#307, 11, via `dexo run`), autocomplete (usql#41, 13), pg_dump (usql#39, 7, via native backup), fzf-like history (mycli#726, 7), pretty JSON (mycli#342, 7), vi bindings (usql#236, 5).

## 5. Not worth copying

- **Automatic table aliases** (pgcli `generate_aliases`, `alias_map_file`): Dexo removed automatic aliases for good, as a settled product rule.
- **usql's breadth of ~50 drivers**: Dexo's value is per-driver depth (server-side read-only, explain, DDL, admin, MCP policy). A thin driver that cannot honor the guardrails dilutes the promise. usql's tracker is full of driver-specific breakage (Oracle, Firebird, Athena `\d`).
- **sq's SLQ query language**: a second query language to learn and maintain. Native SQL plus good completion serves Dexo's audience better.
- **psql scripting metalanguage** (`\if/\elif`, backtick shell expansion, `\gexec`, `\setenv`): it is complex, and `\gexec` and backticks execute generated or shell text, which works against the guardrail and audit model. Named parameters plus `--single-transaction` cover the real scripting need.
- **In-REPL LLM** (`/llm` in mycli, `\llm` in litecli): Dexo's bet is MCP with grants and audit. An in-TUI chat duplicates the agent and is already a known non-goal. litecli users even complained about the forced LLM dependencies ([litecli#239](https://github.com/dbcli/litecli/issues/239), [mycli#1332](https://github.com/dbcli/mycli/issues/1332)).
- **mycli's Polars `.|` transforms and Altair plots**: Python-only. The Rust analogue would be "query the result with DuckDB", which is L effort and not in release builds. Revisit only if charts become a goal.
- **Vendor-specific tunnels** (mycli Vault, Boundary, kubectl): Dexo's generic `password_command` and `pre_connect` already cover them, and the docs name Boundary and kubectl.
- **Shell-redirect syntax inside SQL** (`$>`, `$>>`, `$|`): in a TUI an action ("send result to command…", see §2d) is clearer than a grammar extension on SQL text.
- **Terminal-graphics charts** (usql `\chart` adds ~12 MiB and a JS engine behind a build tag): not before a charts strategy exists.

## 6. Top 5 recommendations (ranked by value / effort)

1. **Fix and finish the CLI output (S, high).** RFC 4180 CSV through the existing `csv` writer, NULL that differs from empty, an aligned table with auto-vertical when too wide, `--no-header`, Markdown. Today `dexo query --format csv` corrupts values that contain commas, quotes or newlines (`presenter.rs:84-104`). This is a correctness bug in the most-used scripting path, and it is cheap.
2. **Let users in the way their other tools do (S–M, high, first five minutes).**
   - `dexo <saved-name>`, `dexo file.db` / `file.parquet` (sniff the type), `pg://` / `my://` aliases.
   - A URL accepted wherever `--connection` is.
   - Read `~/.pgpass`, `pg_service.conf` and the `PG*` env vars (plus `~/.my.cnf`), with an "import from pg_service.conf" action.

   This removes the "re-enter everything" barrier for every psql/mysql user trying Dexo, and every new user meets it in the first five minutes.
3. **Port pgcli's completion tricks (S–M, high daily value).**
   - The whole JOIN clause after `JOIN` for FK-related tables (no invented alias).
   - Name-based join conditions when no FK exists.
   - `*` expansion to the column list.
   - Frecency from `sql_history` in `rank.rs`.

   This is what pgcli is loved for, and Dexo already has the FK data and the history table.
4. **Make saved queries a first-class, shareable asset (S for CLI, M for MCP).** `dexo query --saved NAME --param k=v`, with `--param` bound **by name** to `:name` (today the name is dropped, `run.rs:1371`), plus `\n NAME` in the editor. Then expose saved queries to MCP profiles as vetted read tools. This answers usql's top open request and strengthens the agent-guardrail story.
5. **Schema docs with an ER diagram: `dexo inspect --format markdown|mermaid` (S–M, high).** Tables, columns, keys, indexes, Dexo **notes**, and a Mermaid `erDiagram` that renders on GitHub. It fills the "no ER diagram" gap without TUI graphics, and it is shareable and launch-friendly (sq's version is the model).

Next in line, all S unless noted:
- `dexo run --single-transaction`.
- MySQL warnings shown in Messages, and `DELIMITER` support in scripts.
- Per-connection startup SQL.
- `\watch N` (S–M).
- Cross-connection copy, query → table on another connection (M).
- Opening CSV files in the release binary without DuckDB (M).
