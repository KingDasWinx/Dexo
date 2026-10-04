# Monitoring and diagnosis: pg_activity, pgcenter vs Dexo

Group: monitoring / "the database is slow". Sources: shallow clones in
`/home/winx/Documents/dexo-examples-projects/tui/{pg_activity,pgcenter}` (README, docs, SQL),
`gh` for repo stats and issues, Dexo `development` at 642fb76. Date: 2026-10-02.

## 1. Projects

| Project | Stars | Last activity | One line |
|---|---|---|---|
| [dalibo/pg_activity](https://github.com/dalibo/pg_activity) | 3,050 | pushed 2026-09-21; release 3.6.2 (2026-06) | Python `top` for Postgres: a live list of running, waiting and blocking queries, with an instance header (TPS, cache hit, sessions by state, workers) and per-process CPU/MEM/IO when run on the DB host. |
| [lesovsky/pgcenter](https://github.com/lesovsky/pgcenter) | 1,630 | pushed 2026-09-10; release 0.11.0 (2026-06), 0.12.0 in progress | Go, Linux-only admin TUI: about 20 screens over every `pg_stat_*` view (activity, databases, tables, indexes, sizes, statements x7, progress x6, replication, slots, WAL, bgwriter, pg_stat_io) shown as per-interval deltas, plus host procfs stats, a wait-event profiler and record/report for incident forensics. |

What users ask for (reaction counts are low in both repos, so the roadmap and the issue text say more than the votes):
- pgcenter's 0.12.0 roadmap (`docs/roadmap-0.12.0.md`) calls **"who blocks whom"** its "biggest functional gap for incident work" (feature [020]). It also covers the xmin-horizon holder ([013], shipped), autovacuum scores (PG 19), pause, and colour for idle-in-transaction and waiting rows. The all-time top issue was #14, narrow screens and horizontal scroll.
- Open pg_activity requests: #399 detail view for one process (its locks, who it waits for, progress), #316 lock **tree**, #448 historical logging mode, #274 show idle sessions.

## 0. Baseline: what Dexo does today (verified in code)

- Driver API `AdministrationProvider` (`crates/dexo-driver-api/src/admin.rs`) has these methods: `list_sessions`, `list_locks`, `blocking_graph`, `sizes`, `statistics`, `variables`, `preview`, `execute_action`. The actions are cancel, terminate, VACUUM, ANALYZE, REINDEX and OPTIMIZE. Only Postgres and MySQL/MariaDB implement it. SQLite and DuckDB do not.
- Postgres (`crates/dexo-driver-postgres/src/admin.rs`):
  - Sessions come from `pg_stat_activity` with pid, user, db, state, duration and query. They do **not** include wait_event, xact_start, application_name, client, backend_type or xmin. Idle sessions, background processes and Dexo's own sessions are all included.
  - Blocking uses the classic `pg_locks` self-join.
  - `sizes` uses `pg_total_relation_size`.
  - `statistics` returns only `n_live_tup`.
  - `variables` reads `pg_settings`.
- MySQL (`crates/dexo-driver-mysql/src/admin.rs`): sessions come from `information_schema.PROCESSLIST`, locks and blocking from `performance_schema.data_locks/data_lock_waits`, sizes from `information_schema.tables`, and variables from `SHOW VARIABLES`.
- TUI: the palette command `admin.sessions` opens an 80x16 popup titled "Sessions". It is plain text lines of sessions plus blocking edges, loaded once when the popup opens. The `p`/`r` keys toggle a `paused` flag, but **no refresh loop exists**, so the flag does nothing. `AdminView::{Locks, Statistics, Sizes, Variables}` in `runtime/admin_manager.rs` is defined but never used, so locks, sizes, statistics and variables cannot be reached from the TUI.
- CLI: `dexo sessions` (list, cancel, terminate). MCP: `admin_list_sessions` (opt-in, no query text), plus grant-gated cancel and terminate.

### Defects found while reading (fix before building on top)

| # | Defect | Where |
|---|---|---|
| B1 | **Pressing Enter in the Sessions popup terminates a session nobody picked.** `ConfirmAdmin` fills `confirm_target` with the *first* listed session; Postgres orders by pid, so that is the lowest pid, whoever owns it. It then sends `AdminTerminate`. There is no row selection and no typed confirmation. The path never calls `admin_service::evaluate`, so read-only and production policies are ignored. Clicking the "confirm" line does the same. Success is reported through `Action::OperationFailed`. | `crates/dexo-tui/src/update.rs:1567-1586, 8916-8926`; `runtime/mod.rs:673`; `runtime/admin_manager.rs::terminate_live` |
| B2 | **MySQL `blocking_graph` selects `OWNER_THREAD_ID` from `performance_schema.data_locks`.** That column belongs to `metadata_locks`; `data_locks` has `THREAD_ID`. By the documented schema the query fails on every MySQL (I did not run it: no MySQL container was available). The TUI swallows the error (`.unwrap_or_default()`) and shows "no blocking". The ids are also performance-schema `THREAD_ID`s, not the processlist ids that `KILL` and the session list use, and `list_locks` has the same mismatch. MariaDB has no `performance_schema.data_locks` at all, and the failure is not a permission error, so `list_locks` errors there. No test covers `list_locks` or `blocking_graph` on MySQL. The session already knows `is_mariadb()`, but `admin.rs` never branches on it. | `crates/dexo-driver-mysql/src/admin.rs:98-200`; `tests/admin.rs` |
| B3 | Postgres `variables()` reports `boot_val` (the compiled-in default) as the "server" value, which is misleading. `setting`, `reset_val`, `source`, `sourcefile` and `pending_restart` are the useful columns. | `crates/dexo-driver-postgres/src/admin.rs::variables` |
| B4 | The docs say "Admin lists sessions, locks, and sizes". In fact the TUI shows sessions and blocking only, and the CLI and MCP show sessions only. | `docs/src/data-schema.md:7` |
| B5 | CLI `sessions cancel/terminate` asks for `--confirm`/`--confirm-target` but skips `admin_service::evaluate` (read-only and production). Only MCP applies that policy. | `crates/dexo-cli/src/run.rs:1338-1366` |
| B6 | Dexo never sets `application_name` (Postgres) or a program name. Its own connections look anonymous in other tools and show up in its own session list. pg_activity sets `application_name='pg_activity'` and filters out `pg_backend_pid()`. | `crates/dexo-driver-{postgres,mysql}/src` (no matches) |

## 2. Monitoring and diagnosis features Dexo lacks

Value: H/M/L for a developer or DBA using a workbench. Effort is for Dexo, given the existing `AdministrationProvider`, `AdminList{restriction}` and admin policy.

| Feature | Who has it (evidence) | Value | Effort | MySQL / MariaDB mapping | Notes |
|---|---|---|---|---|---|
| **Live auto-refresh** of the activity view (interval, pause, manual refresh) | pg_activity `--refresh 0.5–5`, `+/-`, `Space` pause, `R`; pgcenter `z` (refresh interval) | H | S | same | The `paused` flag already exists. Pause should stop **rendering, not collecting**, or deltas are wrong after resume (pgcenter roadmap [016]). |
| **Filters**: hide idle (default), minimum duration, database filter | pg_activity `--min-duration`, `--filter dbname:REGEX`, shows non-idle only (#274 asks for idle); pgcenter `I`, `A` age threshold, `/` filter, `\` clear | H | S | same | Without these, the session list is mostly idle-connection-pool noise. |
| **Richer session columns**: wait_event_type/wait_event, xact age vs query age vs backend age, application_name, client, backend_type | pg_activity `get_pg_activity_post_140000.sql` (wait, xmin, `T` duration mode query/transaction/backend); pgcenter `internal/query/activity.go` (xact_age, query_age, change_age, wait_etype) | H | S | MySQL: `performance_schema.threads` (PROCESSLIST_ID, PROCESSLIST_STATE), `sys.session` (trx_state, trx_latency, statement_latency, lock_latency, last_wait, progress), `information_schema.INNODB_TRX.trx_started` for transaction age. MariaDB: `information_schema.PROCESSLIST` (TIME_MS, STAGE, PROGRESS) + `INNODB_TRX` | Add `Option` fields to `SessionInfo`. `information_schema.PROCESSLIST` is deprecated in MySQL 8.0.35+; `performance_schema.processlist` exists since 8.0.22. |
| **xmin horizon / long transactions**: who blocks vacuum, `backend_xid` blank means cheap to kill | pgcenter [013] (`horizon_xacts = age(backend_xmin)`, `backend_xid`, `leader`); pg_activity `xmin` column (3.6.0) | H | S | No xmin. The analogue is the oldest `INNODB_TRX` row plus the InnoDB **history list length** (`information_schema.INNODB_METRICS` `trx_rseg_history_len`) as purge lag | Also check `pg_prepared_xacts` and replication slots `xmin`/`catalog_xmin` (pgcenter backlog: "full horizon aggregate"). |
| **Blocking tree with root blocker** | pg_activity F2 waiting / F3 blocking (`get_blocking_post_140000.sql`, 3 lock kinds); pgcenter [020] contention screen (planned, `pg_blocking_pids()` gated to waiters); pg_activity #316 asks for a tree | H | S–M | MySQL 8: `sys.innodb_lock_waits` (waiting_pid, blocking_pid, wait_age, locked_table, `sql_kill_blocking_connection`), or `data_lock_waits.REQUESTING_THREAD_ID/BLOCKING_THREAD_ID` joined to `performance_schema.threads` to get PROCESSLIST_ID. **Metadata locks** (the "ALTER waiting for table metadata lock" incident): `sys.schema_table_lock_waits` / `performance_schema.metadata_locks`. MariaDB: `information_schema.INNODB_LOCK_WAITS` joined to `INNODB_TRX` (trx_mysql_thread_id); MDL via the `METADATA_LOCK_INFO` plugin | Dexo's Postgres self-join misses transaction-id chains for indirect waiters and gives no root. Use `pg_blocking_pids(pid)` **only for pids with `wait_event_type='Lock'`**. pgcenter documents that calling it per backend amplifies an outage. |
| **Cancel or terminate the selected row; group kill by state and age** | pg_activity navigation `C`/`K` + `Space` to tag several; pgcenter `-`/`_` by pid, `k`/`K` by mask (state + age) | M | S (row) / M (group) | `KILL QUERY id` / `KILL id`, one id at a time | The row action replaces B1. A group kill needs a typed count ("terminate 14 sessions"). |
| **Process detail**: locks held, what it waits for, progress, parallel workers | pg_activity #399 (open request); pgcenter `leader` column groups parallel workers | M | S once the rows above exist | MySQL `sys.session` row + `data_locks` for that thread | A natural "Enter on row" view in a TUI. |
| **Copy query / open in editor / EXPLAIN it** | pg_activity `y` copies the query (OSC 52) | H | S | same | This is **where a workbench wins**: pg_activity can only copy. Dexo can open the query in a document and EXPLAIN it, try a hypopg index and compare plans, all features it already has. |
| **Instance summary header**: uptime, sessions by state vs `max_connections`, TPS, ins/upd/del per second, **cache hit ratio**, rollback ratio, DB size + growth, temp files, waiting count, autovacuum workers, WAL senders/slots | pg_activity `get_server_info_post_110000.sql` + `views.header`; pgcenter summary panel + `v` verbose (`internal/query/overview.go`: deadlocks, conflicts, checksum failures, replication lag, archiving backlog) | H | M | `SHOW GLOBAL STATUS` (or `performance_schema.global_status` / MariaDB `information_schema.GLOBAL_STATUS`): Threads_connected/running, Max_used_connections, Questions and Com_* per second, Innodb_buffer_pool_read_requests vs Innodb_buffer_pool_reads (hit ratio), Innodb_row_lock_waits, Created_tmp_disk_tables, Slow_queries, Uptime | Rates need two snapshots. Show `collecting…` on the first tick and `n/a` (never 0) when a source is unavailable (pgcenter [010]). Do not call `pg_database_size()` on every tick (pg_activity has `--no-db-size` and `D` to refresh). |
| **Top queries** (normalized statements by total/mean time, calls, rows, IO, temp) | pgcenter `x`/`X`: 7 sub-screens (timings, general, IO, temp, local, WAL, JIT) + `G` per-query report (`internal/query/statements.go`), shown as deltas per interval; postgres-mcp `get_top_queries` (adjacent) | H | M | `performance_schema.events_statements_summary_by_digest` / `sys.statement_analysis` (DIGEST_TEXT, COUNT_STAR, SUM_TIMER_WAIT in ps, SUM_ROWS_EXAMINED, SUM_NO_INDEX_USED, SUM_CREATED_TMP_DISK_TABLES), `sys.statements_with_full_table_scans`. MariaDB: same digest table, but **performance_schema is OFF by default** (needs a restart), so show a hint | Postgres needs the `pg_stat_statements` extension (`shared_preload_libraries`). Detect it and show a setup hint instead of an error. PG 13 renamed `total_time`; PG 17 split `blk_*_time` (pgcenter keeps three query variants). Start with cumulative values plus "since stats_reset"; deltas can come later. |
| **Table stats**: seq vs idx scans, dead tuples, last (auto)vacuum/analyze, `n_mod_since_analyze`, HOT ratio, heap/idx cache hit | pgcenter `t` (`tables.go`, joins `pg_stat_*_tables` with `pg_statio_*_tables`) | H | S | `sys.schema_table_statistics` (rows fetched/changed, IO latency), `mysql.innodb_table_stats.last_update` (persistent stats age), `information_schema.TABLES.UPDATE_TIME`. MariaDB: `information_schema.TABLE_STATISTICS` with `userstat=1` | Dexo `statistics()` returns only `n_live_tup` today. Show these in the **object inspector** too, which beats a separate screen in a workbench. |
| **Index usage: unused, invalid, duplicate** | pgcenter `i` (raw `idx_scan`, sortable); postgres-mcp `index_health_calc.py` (invalid, duplicate, bloat, unused) | H | S | `sys.schema_unused_indexes`, `sys.schema_redundant_indexes` (MySQL 8; MariaDB has `sys` since 10.6); MariaDB `information_schema.INDEX_STATISTICS` (userstat) | `idx_scan` counts only since the last stats reset and only on this node, because replicas keep their own counters. Show the reset age and exclude PK/unique. The suggested `DROP INDEX CONCURRENTLY` goes through Dexo's existing DDL preview. |
| **Missing-index hints** | Neither tool; Dexo already has hypopg + plan compare | M–H | M | `sys.statements_with_full_table_scans`, digest `SUM_NO_INDEX_USED` | Heuristic: large tables with high `seq_scan` and `seq_tup_read/seq_scan`. Pair each with its top statements and feed them to the existing hypothetical-index explain. |
| **Bloat estimate** (tables and indexes) | Neither of the two; postgres-mcp has an index-bloat estimate (statistics-based SQL) | M | M | `information_schema.TABLES.DATA_FREE` (fragmentation), which Dexo's existing `Optimize` action fixes | The estimate SQL is approximate. `pgstattuple` is exact but scans the table, so it must not run on a refresh tick. |
| **Vacuum / analyze / index build progress** | pgcenter `p`/`P`: `pg_stat_progress_{vacuum,analyze,create_index,cluster,copy,basebackup}`; PG 19 adds `started_by` and `mode` | M | S | `performance_schema.events_stages_current` WORK_COMPLETED/WORK_ESTIMATED (ALTER TABLE; needs stage instruments); MariaDB `PROCESSLIST.PROGRESS` | Useful right after Dexo runs its own VACUUM/REINDEX action. |
| **Wraparound risk** (`age(datfrozenxid)`, `mxid_age(datminmxid)` vs `autovacuum_freeze_max_age`) | pgcenter counts "to prevent wraparound" autovacuums; postgres-mcp `vacuum_health_calc.transaction_id_danger_check` | H (rare, catastrophic) | S | Not applicable (no xid wraparound); show history list length instead | One cheap query; a natural health check. |
| **Replication lag and slots** (bytes/time per standby, inactive slot retaining WAL) | pgcenter `r` (`replication.go` write/flush/replay lag), `o` slots (`retained,KiB`, `wal_status`); pg_activity header counts | M | S | MySQL `SHOW REPLICA STATUS` (8.0.22+; Seconds_Behind_Source), `performance_schema.replication_applier_status_by_worker`; MariaDB `SHOW ALL REPLICAS STATUS` / `SHOW SLAVE STATUS` | An inactive slot is the classic disk-fill incident. Worth one health check line. |
| **Config inspection**: non-default settings, source file, pending restart | pgcenter `C` show config (`pg_settings` by category), `E` edit files, `R` reload | M | S | MySQL `performance_schema.variables_info` (VARIABLE_SOURCE, SET_TIME, SET_USER) joined to `global_variables`; MariaDB `information_schema.SYSTEM_VARIABLES` (GLOBAL_VALUE_ORIGIN, DEFAULT_VALUE) | `variables()` already exists but is unused and mislabelled (B3). Show it read-only; leave editing files and reloading to dedicated tools. |
| **Deadlock detail** | Neither (Postgres reports deadlocks only as a counter in `pg_stat_database` and in the log) | M (MySQL users) | S | `SHOW ENGINE INNODB STATUS` → "LATEST DETECTED DEADLOCK" section (both MySQL and MariaDB); MySQL `innodb_print_all_deadlocks` | Cheap to show the raw section; parsing is optional. |
| **Wait-event profile of one query** | `pgcenter profile` (samples a pid's `wait_event` at high frequency: "72% IO.DataFileRead") | M | M | `performance_schema.events_waits_history` per thread (instruments off by default) | Workbench twist: while an editor query runs, sample its backend from a second connection and show "time by wait event" next to the result. It answers "is this CPU, IO or a lock?", which EXPLAIN cannot. |
| Server log view / tail | pgcenter `l`/`L` (`pg_current_logfile()`, needs file read rights) | L | M | `performance_schema.error_log` (MySQL 8.0.22+) | Usually blocked on managed services. |
| Reset statistics counters | pgcenter `Q` (`pg_stat_reset`, `pg_stat_statements_reset`) | L | S | `TRUNCATE performance_schema.events_statements_summary_by_digest` | A write; put it behind the admin grant or skip it. |
| Hide monitoring queries from server logs | pg_activity `--hide-queries-in-logs` (`SET log_min_duration_statement=-1`, superuser) | L | S | `SET SESSION sql_log_off` (needs SUPER) | Tagging connections (B6) is the cheaper courtesy. |
| Host OS stats, per-backend CPU/MEM/IO | pg_activity header + columns (psutil, local only); pgcenter sysstat, `B`/`N`/`F` panels, `Shift+S` procfs, PL/Perl functions for remote | L for a workbench | L | none | Needs local access to the DB host. Useless for RDS or containers over SSH tunnels. |
| pg_stat_io, bgwriter/checkpointer, WAL, archiver, JIT screens | pgcenter `j`, `b`, `w`, JIT sub-screen | L | M | InnoDB metrics / `SHOW ENGINE INNODB STATUS` | DBA tuning territory. |
| Record/replay history ("poor man's monitoring") | pgcenter `record`/`report`; pg_activity #448 asks for it | M | L | same | A collector product. Dexo's answer: `dexo health --format json` from cron. |

## 3. Inside a workbench vs left to dedicated tools

**Put in Dexo**: one **Activity** screen that replaces the Sessions popup, using the `AdminView` tabs already sketched:

```
Activity — prod-db  ↻2s  12 conn/100 · 3 active · 2 idle-in-tx · 1 waiting · 845 tps · hit 99.2% · oldest tx 14m
[Sessions] [Blocking] [Top queries] [Health] [Settings]
```

- **Sessions**: live and filterable (idle hidden, minimum duration). Highlight idle-in-transaction and waiting rows. `Enter` opens the detail view, `e` opens the query in the editor, `x` runs EXPLAIN, and `c`/`k` cancel or terminate **the selected row** through `admin_service::evaluate`.
- **Blocking**: a tree with root blockers first. Show the blocker's state and transaction age, because idle-in-transaction is the usual culprit. One action: terminate the root, with a typed pid.
- **Top queries**: from `pg_stat_statements` or the digest table. `Enter` puts the query in the editor; from there EXPLAIN, hypopg and plan compare already exist.
- **Health**: a one-shot report (no refresh loop) that lists findings by severity. Each finding links to the object inspector, and each suggested fix is DDL shown through the existing preview and confirm path.
- **Settings**: non-default settings with source and pending_restart, read-only.
- **Per-table stats** belong in the **object inspector**: scans, dead tuples, last vacuum/analyze, index usage. That is where a developer already looks.
- **Why it fits**: every answer leads to something Dexo already does (editor, explain, hypothetical index, DDL preview, schema forms, maintenance actions), and the same SQL can feed the CLI and MCP through `dexo-app`.

**Leave to pgcenter, pg_activity, Prometheus/pgwatch/PMM or pganalyze**:
- host and procfs metrics, per-process CPU and IO;
- pg_stat_io, bgwriter, WAL and JIT internals;
- continuous recording and history, alerting, charts (Dexo has no charts);
- tailing server logs, editing config files and reloading;
- high-frequency server-wide sampling.

These need local host access or superuser, need a long-running collector, or serve DBA tuning rather than "why is my app slow".

**Design rules to copy** (lessons both tools paid for):
1. "Do not make an incident worse":
   - gate `pg_blocking_pids()` to waiting pids;
   - no `pg_database_size()` or `sys.innodb_buffer_stats_by_table` on every tick;
   - a short `statement_timeout`/`lock_timeout` on monitoring queries.
2. Pause stops the render, not the collector (deltas stay honest).
3. Show `n/a` and the `restriction` text when a source is unavailable, never 0. Dexo already has `AdminList.restriction`.
4. Exclude Dexo's own pid and tag Dexo's connections.
5. Monochrome stays readable; colour only marks state (idle-in-tx, waiting, long).
6. Required privileges, stated in the restriction text: Postgres `pg_monitor`/`pg_read_all_stats`; MySQL `PROCESS` plus `SELECT` on `performance_schema` and `sys`.

**SQLite/DuckDB**: small, optional "health" via PRAGMAs:
- SQLite `freelist_count`/`page_count` (fragmentation, which VACUUM fixes), `integrity_check`, `quick_check`;
- DuckDB `pragma database_size`, `duckdb_memory()`.

## 4. Serving Dexo's MCP server (read-only diagnosis tools)

Dexo's MCP already has the right frame: profiles, opt-in tools that expose other people's activity (`OPT_IN_TOOLS`), row/byte/timeout limits, audit, and grant-gated cancel/terminate with `--ask` approval in the TUI. Diagnosis tools fit as read-only, `read_only_hint = true` tools over the same `dexo-app` functions as the TUI tabs:

| Tool | Returns | Safety |
|---|---|---|
| `admin_activity` (extends `admin_list_sessions`) | Sessions with state, wait event, transaction and query age, blocked_by. Optional `include_query`. | Opt-in, like today. On PG 14+ return the normalized text via `query_id` → `pg_stat_statements`, not raw `pg_stat_activity.query`, which can hold literals and PII. |
| `admin_blocking_tree` | Root blockers and their waiters: pid, state, xact age, lock mode, relation. | Opt-in. Gated query. No query text by default. |
| `admin_top_queries {sort_by: total\|mean\|calls\|rows\|io\|temp, limit≤50}` | Normalized statements with stats, plus "since stats_reset". | Digests are already normalized (`$1`/`?`), which makes them safer than activity text. Still opt-in, since they show other users' workload. |
| `admin_health_check {checks?: [...]}` | Findings with severity, evidence numbers, and suggested SQL as **text**, never executed. | Catalog and stats only. Each check is bounded and timed out. Safe to allow by default in read profiles. |
| `admin_table_health {table}` | Scans, dead tuples, last vacuum/analyze, size, per-index usage. | Same exposure as `object_describe`. Gives the agent loop: `query_explain` → `admin_table_health` → hypothetical index in `query_explain`. |
| `admin_settings {names?, non_default_only}` | Setting, value, source, pending_restart. | Reveals infrastructure details, so opt-in. |

Typical agent flow for "the app is slow":
1. `admin_health_check` and `admin_blocking_tree` find a root blocker that is idle in transaction for 40 minutes.
2. The agent proposes `admin_terminate_session`, which needs a grant; with `--ask`, the user approves it in Agent Activity.
3. `admin_top_queries` shows one statement taking 60% of total time.
4. `query_explain` with a hypothetical index shows the fix.
5. The agent proposes DDL through the grant path.

No new safety machinery is needed. The same functions give `dexo health|top|activity --format json` for scripts, and run from cron they answer pg_activity #448 ("history") without building a recorder.

## 5. Top recommendations (value / effort)

0. **Fix B1–B6 first.** B1 is a safety bug (Enter kills a session nobody chose, bypassing read-only and production policy). B2 means MySQL blocking never works, and MariaDB locks error out. All are S.
1. **Live Activity screen** (H / S–M):
   - Turn the popup into a refreshing, selectable table with wait event, xact/query age, app and client columns.
   - Hide idle by default, add a minimum-duration filter, exclude its own pid.
   - Cancel/terminate the selected row through `admin_service::evaluate`.
   - Open in editor / EXPLAIN on the row.
   - The `AdminView` enum and `paused` flag are already there.
2. **Blocking tree with root blockers, including MySQL metadata locks** (H / S):
   - Postgres: `pg_blocking_pids()` gated to waiters.
   - MySQL 8: `sys.innodb_lock_waits` + `sys.schema_table_lock_waits`.
   - MariaDB: `INNODB_LOCK_WAITS` ⋈ `INNODB_TRX`.
   - Terminate root with a typed pid. Expose it as MCP `admin_blocking_tree`.
3. **Top queries** (H / M):
   - Postgres: `pg_stat_statements`, with a setup hint when it is missing.
   - MySQL/MariaDB: `events_statements_summary_by_digest`, with a hint when performance_schema is off.
   - Rows lead into editor → EXPLAIN → hypopg → plan compare, a chain no TUI rival offers.
   - Also as MCP `admin_top_queries` and CLI.
4. **Health check report** (H / M), one set of SQL checks shared by the TUI tab, `dexo health` and MCP `admin_health_check`:
   - long and idle-in-transaction transactions, xmin horizon;
   - wraparound age;
   - connections vs max;
   - unused, duplicate and invalid indexes;
   - stale vacuum/analyze and dead-tuple ratio;
   - cache hit;
   - inactive slots and replication lag;
   - sequences or auto_increment near max;
   - MySQL history list length and DATA_FREE.
5. **Instance summary header + Settings tab + table stats in the object inspector** (M / S–M):
   - Header: sessions by state vs max, TPS, cache hit (deltas between ticks), oldest transaction, waiting count.
   - Settings tab: non-default settings with source and pending_restart (fixes B3).
   - Object inspector: per-table scans, dead tuples, last vacuum/analyze, index usage.
