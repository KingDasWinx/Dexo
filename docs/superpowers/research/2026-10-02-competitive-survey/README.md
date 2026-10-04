# Competitive survey, 2026-10-02

Nine groups of projects were read (README, docs, source, issue trackers) against what Dexo 1.4.2 does. Clones live in `~/Documents/dexo-examples-projects/tui`.

| Report | Projects |
| --- | --- |
| [rust-tuis](rust-tuis.md) | rainfrog, lazydb, gobang, tabiew |
| [other-tuis](other-tuis.md) | lazysql, sqlit, harlequin, dblab, vi-mongo |
| [cli-clients](cli-clients.md) | pgcli, mycli, litecli, usql, sq |
| [data-exploration](data-exploration.md) | visidata, pspg, tabiew |
| [monitoring](monitoring.md) | pg_activity, pgcenter |
| [editor-plugins](editor-plugins.md) | nvim-dbee, vim-dadbod-ui, sqls, sql-language-server, postgres-language-server |
| [gui-clients](gui-clients.md) | Beekeeper Studio, DbGate |
| [mcp-servers](mcp-servers.md) | MCP Toolbox for Databases, dbhub, Postgres MCP Pro, Supabase MCP, mcp-server-mysql |
| [drivers](drivers.md) | which database to add next, demand and feasibility |

Demand signals in the trackers are weak everywhere (the top open request in most repos has under 15 👍), so the order below weighs the first five minutes and reach over vote counts.

## Defects the survey found, fixed before 1.4.2

- The Sessions screen ended the first session listed on Enter, unpicked and unconfirmed, on any connection.
- MySQL's blocking graph queried a column `data_locks` does not have; MariaDB has no `data_locks`; lock ids were not the ones `KILL` takes.
- Postgres server variables showed compiled-in defaults; Dexo's sessions had no application name.
- `dexo sessions cancel|terminate` ignored read-only connections; the docs promised locks and sizes the screen does not show.
- `dexo query --format csv` did not quote values.
- Quitting dropped an open transaction and unapplied grid edits without asking.
- `:name` parameters reached Postgres and MySQL as typed (a syntax error), and every statement of a script got every value.
- Ctrl+Enter, the only run key, arrives as Enter on terminals without the kitty keyboard protocol (tmux by default): Ctrl+J now runs too.
- The README said the grid edits rows; it inserts and deletes them.

## What to build next, in order

### 1. The basics a first session hits

| Item | Effort | Evidence |
| --- | --- | --- |
| Edit a cell in the grid (inline, NULL/DEFAULT, `$EDITOR` for long values), through the existing change set and review | M | other-tuis; lazysql, sqlit, vi-mongo have it; harlequin #653 |
| Wide results keep their widths and scroll sideways; freeze and hide columns (the model already has both) | S–M | data-exploration; today 30 columns squeeze to 3 characters each |
| `/` search in the catalog tree, then a "Go to object" picker (the filter code exists) | S–M | other-tuis; every SQL rival has it |
| Start without setup: `dexo shop.db`, `dexo data.csv`, `dexo <saved name>`, `DATABASE_URL` and `.env`, `~/.pgpass`, `pg_service.conf`, `PG*`, `~/.my.cnf` | S–M | rust-tuis, cli-clients |
| Search inside results, selection stats, filter to the cell's value | S–M | data-exploration |

### 2. Reach

| Item | Effort | Evidence |
| --- | --- | --- |
| Static musl Linux builds: glibc 2.35 shuts out RHEL 9, Debian 11, Ubuntu 20.04, Amazon Linux 2023 | M | rust-tuis; rainfrog ships glibc 2.28 and musl |
| DuckDB in the release binaries (opens CSV/Parquet too) | M | lazysql's top issue (13 👍); rainfrog ships it in 15–19 MB |
| winget first submission, nixpkgs, crates.io; the AUR already has a community `dexo-bin` | S each | rust-tuis, other-tuis |
| Listed where agents and editors look: MCP Registry `server.json`, nvim-lspconfig, Mason, Helix | S each | mcp-servers, editor-plugins |

### 3. What sets Dexo apart, deepened

| Item | Effort | Evidence |
| --- | --- | --- |
| Activity screen: live sessions with wait events and ages, blocking tree with the root blocker, top queries (pg_stat_statements / statement digests), a health check; the same checks as `dexo health` and read-only MCP tools | M | monitoring, mcp-servers |
| MCP: smaller default results and an untrusted-data fence around rows; saved queries as vetted tools; column-level deny, then masking | S–M | mcp-servers |
| LSP: hover with column types and notes, a connection setup that explains itself, warnings for destructive statements, run a read from the editor | S–M | editor-plugins |
| Foreign-key and enum lookup when inserting or editing rows | M | gui-clients |
| Schema docs and ER diagrams as text: `dexo inspect --format markdown|mermaid` with the notes | S–M | cli-clients, gui-clients |

### 4. Drivers

1. **Compatibility pack, no new crate:** Postgres 18 and MariaDB 11.8 in CI; clear errors for Neon's pooled endpoint and Supabase's transaction pooler; TiDB (rejects `SET SESSION TRANSACTION READ ONLY`, has no JSON explain) and CockroachDB (no `EXPLAIN (FORMAT JSON)`) made to work and named in the form.
2. **SQL Server** with `tiberius` (L): the biggest SQL database Dexo lacks, shipped by 10 of 12 rivals. Cancel ends the connection, and read-only rests on the parser guard.
3. **ClickHouse** with the official crate (M): asked for in 6 of 9 trackers; `readonly=1` gives real server-side read-only.

Hold: Oracle (needs Oracle's client libraries), libSQL/D1 (low demand), Redis and MongoDB (not SQL), Snowflake and BigQuery (paid queries behind an agent).

### Not worth copying

An AI chat inside the TUI (MCP is the stronger position), driver auto-installers and a long tail of thin drivers, visual query designers, pivots and SQL over loaded rows, hundreds of bundled themes, plugin systems, SQL tabs without a connection, bypassing the SQL parser.
