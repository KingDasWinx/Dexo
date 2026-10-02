# Rust TUI rivals: rainfrog, lazydb, gobang, tabiew

Research date: 2026-10-02. Clones: `/home/winx/Documents/dexo-examples-projects/tui/<project>` (paths below are relative to each clone). Dexo was checked against `docs/src/*.md`, `crates/dexo-tui/src/palette/registry.rs`, `crates/dexo-tui/src/keymap.rs` and the driver/app code before anything was called missing.

## 1. Projects

| Project | Stars | Last activity | Lang | What it is |
| --- | --- | --- | --- | --- |
| achristmascarl/rainfrog | 5,356 | last commit 2026-09-29; v0.4.6 2026-09-24 (8 releases since June) | Rust (ratatui, sqlx) | A lightweight vim-style DB TUI: menu of schemas/tables, a query editor with autocomplete, history and favorites, and a results grid. Postgres (tier 1), MySQL/SQLite/DuckDB/Redshift (tier 2), Oracle (tier 3). In homebrew-core, with 151 installs in the last 30 days (formulae.brew.sh analytics). |
| yelog/lazydb | 54 | created 2026-08-27, 981 commits in the last 30 days, v0.1.7 2026-09-23 (10 releases in 4 weeks), quiet since 09-23 | Rust | A "keyboard-first database workspace": PG/MySQL/MariaDB/Oracle/SQL Server/SQLite/Redis, Omni Bar, catalog editor, Users & Roles, monitoring dashboard, MCP + JSON agent CLI, an LSP, a Neovim plugin, an in-app updater. It ships very fast. |
| TaKO8Ki/gobang | 3,324 | last commit 2023-01-25, last release v0.1.0-alpha.5 (2021) | Rust | An abandoned MySQL/PG/SQLite browser. It fails to build on Rust >= 1.73 (#179, the top issue). Useful only as a demand signal (57 open issues). |
| shshemi/tabiew | 3,129 | last commit 2026-10-02, v0.15.1 2026-09-09, 116 commits in 30 days | Rust (polars) | A viewer for tabular *files* (CSV/TSV/Parquet/JSON/Arrow/Avro/Excel/SQLite/FWF/logfmt/HTML/MD tables). It has SQL over loaded frames, fuzzy search, histogram/scatter plots and 421 themes. In homebrew-core (52 installs/30d) and Arch extra. |

## 2. Features Dexo lacks

Each row was checked in Dexo's docs and code. "Dexo today" says what is there instead.

| Feature | Who has it (evidence) | Value for Dexo's users | Effort | Notes / Dexo today |
| --- | --- | --- | --- | --- |
| **Row-count confirmation before COMMIT**: UPDATE/DELETE run inside a transaction, the dialog says "DELETE 1,234 rows? [Y]es / [N]o", and Y commits while N rolls back | rainfrog `src/popups/confirm_tx.rs`, `src/database/mod.rs:269-285` (Delete/Update → `ExecutionType::Transaction`) | High. It shows the real blast radius instead of a guess. It is the natural next step for Dexo's guardrail story and works well in a GIF. | M | Dexo asks *before* running (`dexo_app::run_guard::judge`, `crates/dexo-tui/src/update.rs:5609`) and never shows a count. It could reuse the Analyze rollback fence (savepoint inside a user tx). MySQL non-transactional engines would fall back to today's prompt. Apply it to flagged statements and every production write. |
| **Quit guard**: when a manual transaction is open or grid edits are not applied, quitting offers Commit / Rollback / Cancel (and Apply / Discard) | lazydb v0.1.1 notes ("review pending transactions before quit"), `docs/keybindings.md` "Transaction Exit" (a abandon / r rollback / c commit) | High. Today the work is lost silently. | S | `Action::Quit` (`crates/dexo-tui/src/update.rs:2265`) saves documents and goes straight to `Effect::Shutdown`, with no check for an open tx or a staged change set. Verify in the app before fixing. |
| **Bare file path at launch**: `dexo shop.db`, `dexo sales.parquet`, `dexo data.csv` | tabiew `tw <file>` (README "Usage"); gobang #92 "open any database, e.g. an sqlite file" (6 👍, open) | High for the first 5 minutes | S | `connection_url::parse` requires `scheme://` (`crates/dexo-app/src/connection_url.rs:29`), so a bare path errors. Pick the driver by extension. Handle Windows drive letters (tabiew #112 is exactly that bug). |
| **`DATABASE_URL` / `.env` auto-detect** | rainfrog `src/main.rs:60,135` (env var, then `.env` in cwd or a parent; README "with environment variables") | High for app developers (`cd myapp && dexo`) | S | No `DATABASE_URL` anywhere in Dexo. Offer it as a temporary connection, which already fails closed on destructive statements. Never persist the password. |
| **Open a saved connection at launch**: `dexo --connection NAME` | lazydb `--profile NAME` (`docs/configuration.md` "Startup Selection"); rainfrog `default = true` in `[db]` | Medium (shell aliases, nvim/tmux launchers) | S | `TuiStart` is only Workbench / Url / Demo (`crates/dexo-cli/src/args.rs:30`). |
| **URL field in the connection form**: paste `postgres://`, `mysql://` or `jdbc:…` and every field is filled, with the password moved to the secret field | lazydb `docs/configuration.md` "Ad-hoc Connections" (last paragraph); rainfrog #225 (JDBC URLs, shipped) | High. Supabase, Neon, Railway and Heroku all hand out URLs. | S | The form has only name/driver/path/host/port/database/username/password (`crates/dexo-tui/src/screens/connection.rs:8`). The parser already exists. |
| **Every database of a Postgres server from one connection** (database switcher or tree level) | lazydb `Space D` "workspace database selector" (`config/default.toml` `open-database-selector`), Explorer database level; rainfrog #126 "Switch db in Postgres" (open, 4 comments) | Medium-high for PG dev servers with many DBs | M-L | The PG catalog root is `WHERE datname = current_database()` (`crates/dexo-driver-postgres/src/catalog/mod.rs:81`). `databases()` is used only by `\l`. It needs a session per database. |
| **Go-to-object Omni search**: one box for commands (`>`), connections (`@`), open documents and catalog objects (tables/views) across connections, with server-side catalog search for huge schemas | lazydb `docs/omni-bar.md`, Explorer `f` catalog search (`docs/keybindings.md` Explorer) | High daily value on big schemas | M | Dexo's palette is commands only. The explorer filter matches only loaded nodes (`crates/dexo-tui/src/screens/explorer.rs:757`). Cached catalogs already exist for the LSP. |
| **Find in results**: `/` over the loaded rows with n/N and highlight, plus jump to a column by name | tabiew `/` fuzzy search (README keybindings, `src/misc/search.rs`) | Medium-high (wide tables, eyeballing a value) | S | There is no `/` in any Dexo keymap section. The WHERE bar is server-side and a different tool. |
| **Freeze the first N columns** (keep the id visible while scrolling right) | tabiew #125 (open request) | Medium | S | `GridState.frozen_columns` already exists (`crates/dexo-tui/src/model.rs:335`), but no command ever sets it. |
| **Monitoring dashboard**: TPS, connections/max, active, cache-hit %, deadlocks, temp files, uptime; a process list with filter; charts; pause/refresh interval | lazydb `src/ui/dashboard.rs` (Overview/Processes/Charts), `docs/database-capabilities.md` "Monitoring Dashboard", `[dashboard] refresh_interval_seconds` | Medium | M | Dexo admin = sessions, locks, sizes (`crates/dexo-driver-api/src/admin.rs:96`). There is no `pg_stat_database` or `pg_stat_statements`. |
| **Column profile**: name, type, nulls, distinct, min, max for a result or table | tabiew `src/tui/schema/data_frame_field_info.rs:67` (Name/Type/Size/Null Count/Min/Max), `data_frame_meta_info.rs` | Medium (quick data sanity) | M | Nothing in Dexo. It could be done as one aggregate query over the result's SQL, inside a read-only tx like the WHERE bar. |
| **Charts**: histogram of a column (buckets) and X/Y scatter with color-by | tabiew `src/tui/plots/histogram_plot.rs`, `scatter_plot.rs`, `src/tui/popups/histogram_builder.rs`, `scatter_plot_builder.rs` | Medium-low (analyst use, DuckDB/CSV) | M | Listed as known-missing in Dexo. Bucket server-side (GROUP BY) to stay bounded. ratatui has BarChart/Chart. |
| **MySQL `DELIMITER` client command** in pasted scripts | rainfrog #282 (open, 4 comments) is the demand; lazydb splits SQL Server `GO` (`docs/database-capabilities.md`) as precedent | Medium for MySQL users (dumps, tutorials, procedure scripts) | S | The Dexo splitter handles BEGIN…END bodies (`crates/dexo-sql/src/statement.rs:43`) but not `DELIMITER //` lines. |
| **Mouse drag**: resize panes by dragging borders, resize a grid column by dragging its separator, drag-select text to copy | lazydb `docs/keybindings.md` "## Mouse" | Low-medium | M | Dexo `mouse.rs` handles clicks and wheel only. Keyboard resize exists (`layout.*_grow/shrink`). |
| **More drivers** | SQL Server: lazydb `src/db/mssql.rs`. Oracle: rainfrog `src/database/oracle/`, lazydb. Redis: lazydb `src/db/redis/`. Redshift via PG wire: rainfrog README | SQL Server is the most-asked across trackers (gobang #95, shipped by lazydb) | L each | See section 5 for Oracle and Redis. |
| **Release binaries for older Linux**: GNU built against glibc 2.28, plus static musl | rainfrog v0.4.6 "Build GNU releases against glibc 2.28" (PR #341). Release assets include `x86_64/aarch64/i686-unknown-linux-musl` | High reach. Dexo needs glibc 2.35 (`docs/src/install.md`), so RHEL/Rocky/Alma 9 and Amazon Linux 2023 (2.34), Debian 11 and Ubuntu 20.04 (2.31) cannot run it, and those are exactly the server boxes a DB TUI is used on. | S-M | `dist-workspace.toml` runners are `ubuntu-22.04`. Add a musl target or an older runner. |
| **DuckDB in the release binaries** | rainfrog `Cargo.toml:93,124` (`duckdb` with `bundled` in the default features). Prebuilt gnu/darwin/windows tarballs are 15-19 MB | High for "try without a server" (CSV/Parquet) | M (CI minutes) | Dexo ships DuckDB only from source (`docs/src/install.md`: +11 min, 56→120 MB). Rainfrog proves the CI cost is acceptable. A separate `dexo-full` asset is the cheap middle ground. |
| **crates.io, binstall, AUR, nixpkgs, conda, homebrew-core, Arch extra** | rainfrog README "installation" (all of them); tabiew (homebrew-core, Arch extra); gobang (Scoop main bucket, nixpkgs, NetBSD pkgsrc, AUR) | Medium-high reach | S each (crates.io: M, since every workspace crate must publish) | crates.io answers "crate `dexo` does not exist". Dexo has its own tap/bucket, winget, deb/rpm and MSI. gobang #97 "Upload to AUR" (5 👍) shows the demand from Arch users. |
| **stdin piping**: `curl …csv \| dexo` | tabiew README ("Open a URL using curl"), `src/misc/stdin.rs` | Low-medium | S once DuckDB ships | — |
| **Export to Parquet / Markdown file** | tabiew `src/io/writer/parquet.rs`, `markdown.rs`, `arrow.rs`, `avro.rs` | Low | S-M (DuckDB `COPY TO`) | Dexo exports CSV/TSV/JSON/JSONL/SQL and copies Markdown. |
| **Neovim plugin** (DB TUI in a floating terminal, `:checkhealth`) | lazydb `yelog/lazydb.nvim` (README "Neovim Integration") | Low-medium (nvim crowd, which Dexo's LSP already courts) | S | — |
| **In-app updater** (Update Center, applies without interrupting, then "Restart now") | lazydb `docs/keybindings.md` "Update Center", `lazydb update` | Low | M | Dexo notifies and prints the command (`crates/dexo-tui/src/runtime/update_check.rs`). |
| **Installer offers MCP setup right after install** | lazydb README ("After an interactive installation, the installer can open the LazyDB MCP setup wizard") | Low-medium (agent users) | S | Dexo has `dexo mcp setup`, but nothing points to it at install time. |

## 3. Where they do better even though Dexo has the feature

- **Distribution.** rainfrog is in homebrew-core, Arch `extra`, nixpkgs, conda-forge, crates.io (+binstall), Termux and Docker Hub (`achristmascarl/rainfrog`). tabiew is in homebrew-core and Arch `extra`. gobang made it into the Scoop main bucket and nixpkgs even as an alpha. Dexo depends on its own tap and bucket. A user who types `brew install dexo` or `pacman -S dexo` gets nothing.
- **Binary portability.** rainfrog's glibc floor is 2.28, with musl variants; Dexo's is 2.35 (see table).
- **DuckDB out of the box.** rainfrog's default download opens Parquet and CSV through DuckDB. Dexo's "try with a file" story exists only for people who compile.
- **Starting without typing a URL.** rainfrog takes `DATABASE_URL` and `.env`. It prompts for any missing option and falls back to the environment, then asks for the password and offers to keep it in the keychain (`src/database/postgresql.rs:463`). `dexo postgres://host/db` without a user fails with "the URL needs a user" (`connection_url.rs`).
- **Confirmations say how many rows.** rainfrog's dialog names the statement type and the actual affected count. Dexo's names the statement but not its effect.
- **Saved queries as plain `.sql` files.** rainfrog writes favorites as one `.sql` file each, in a folder you can point with `RAINFROG_FAVORITES` and commit to git. Dexo's saved queries live inside its SQLite database, so they cannot be shared with a team or diffed.
- **Per-process flags.** lazydb has `--read-only`, `--profile`, `--mouse off`, `--icons ascii|unicode`, `--motion reduced` and `--theme-file` (watched and hot-reloaded). Dexo keeps these in Settings only. A one-off `--read-only` session on a writable profile is a useful safety knob that Dexo lacks.
- **Huge catalogs.** lazydb's catalog search goes to the server (debounced), refresh is scoped to the selected node, stale rows stay visible while reloading, and a PG table renamed outside the tool is re-bound by OID (`docs/keybindings.md` Explorer section).
- **Honest support matrix.** rainfrog's README tier table says which databases are tier 1-4 and which go over a wire protocol. lazydb has a capability matrix per driver (`docs/database-capabilities.md`).
- **Instant start on a file.** tabiew's `tw big.csv --max-rows 1000` loads a sample of a multi-GB file to check its schema first.
- **Velocity (a threat, not a feature).** lazydb landed about 1,000 commits and 10 releases in its first 4 weeks: SQL Server, Oracle, Redis, users and roles, a dashboard, MCP. It already overlaps Dexo's MCP and LSP pitch, so "check lazydb before calling something unique" stays true.

## 4. Most-wanted requests that Dexo also does not answer

Top issues by 👍 that Dexo does not cover. Most trackers are tiny, and the maximum anywhere is 13 👍 (rainfrog's "Edit columns directly", which Dexo already has).

| Request | 👍 / comments | URL | Dexo |
| --- | --- | --- | --- |
| Command-line parameter to open any database, e.g. an sqlite file | 6 / 2 | https://github.com/TaKO8Ki/gobang/issues/92 | Partly: `dexo sqlite:///path` works, but a bare path does not |
| Upload to AUR | 5 / 1 | https://github.com/TaKO8Ki/gobang/issues/97 | No AUR package |
| Add support for Snowflake | 3 / 1 | https://github.com/achristmascarl/rainfrog/issues/233 | No |
| Support ClickHouse connector | 3 / 0 | https://github.com/TaKO8Ki/gobang/issues/184 | No |
| Add flatpak or snap support | 2 / 1 | https://github.com/TaKO8Ki/gobang/issues/72 | No |
| Switch db in Postgres | 1 / 4 | https://github.com/achristmascarl/rainfrog/issues/126 | No (one DB per connection) |
| Databricks adapter | 1 / 0 | https://github.com/TaKO8Ki/gobang/issues/183 | No |
| Build for older OS (CentOS 7/8) | 1 / 0 | https://github.com/shshemi/tabiew/issues/81 | No (glibc 2.35) |
| Delimiter statements not processed | 0 / 4 | https://github.com/achristmascarl/rainfrog/issues/282 | No `DELIMITER` |
| Readline bindings in the command window | 0 / 3 | https://github.com/shshemi/tabiew/issues/53 | Partly: Dexo fields take Ctrl+W/Home/End, but Ctrl+A selects all and there is no Ctrl+E/U/K |
| SQL Server support | 0 / 2 | https://github.com/TaKO8Ki/gobang/issues/95 | No (lazydb has it) |
| First column freezing | 0 / 0 | https://github.com/shshemi/tabiew/issues/125 | No (field exists, unused) |
| Allow a console with no connection configured | 0 / 1 | https://github.com/yelog/lazydb/issues/9 | No, by design (see §5) |

Requests already answered by Dexo, for calibration: edit cells (rainfrog #124, 13 👍), keybinding and color customization (#72), Ctrl+hjkl/Alt+n pane focus (#168), delete N selected rows (#226), SSH tunnel (gobang #83), prompt for password (gobang #162), SQL completion (gobang #87), query history (tabiew #114), run SQL from the terminal (tabiew #124), copy one row (tabiew #105).

## 5. Not worth copying

- **Unbound/offline SQL consoles** (lazydb v0.1.7, issue #9). This contradicts Dexo's rule that every document belongs to a connection.
- **"LOCAL ENCRYPTED" credential file with its key beside it** (lazydb's default, `docs/configuration.md` "Password Storage"). A same-user attacker reads both files. Dexo's keychain, `password_command` or per-session prompt is the honest model, and it already covers headless boxes.
- **Bypass-parser execution, F7** (rainfrog). It is an escape hatch around guardrails. Dexo's rule that an unparsed statement counts as a write and is confirmed is better.
- **Kitty remote-control pane sync** (lazydb `contrib/kitty`, `docs/kitty-integration.md`). It serves one terminal, needs remote-control setup, and is high maintenance.
- **MyBatis-XML LSP** (lazydb `docs/sql-language-server.md`). A niche Java ecosystem.
- **Redis browser** (lazydb). A different product with its own value editors. Dexo's SQL, guardrail and MCP model doesn't transfer, and it is large.
- **Oracle** (rainfrog tier 3, lazydb partial). It needs the Oracle Instant Client at runtime, makes packaging painful, and serves a small TUI audience. SQL Server is the better next driver if one is added.
- **421 bundled themes** (tabiew `src/tui/themes/`). Bloat. Dexo's TOML themes and five presets are enough.
- **Exotic file readers** (Excel, Avro, Arrow, FWF, logfmt, HTML/Markdown tables; tabiew `src/io/reader/`). Dexo is a DB workbench, and DuckDB already covers CSV/Parquet/JSON once it ships.
- **Android/Termux and i686 builds** (rainfrog). The audience is tiny. musl, however, *is* worth it (§2).
- **`uninstall` / `migrate-home` commands** (lazydb). Package managers handle this.
- **gobang as a whole.** It is dead: it does not build on current Rust, and its last release is from 2021. Its records/columns/constraints/FKs/indexes tabs (keys 1-5) are already covered by Dexo's inspector.

## 6. Top 5 for Dexo (value ÷ effort)

1. **Zero-config start (S).** `dexo <path>` picks SQLite or DuckDB by extension (including Windows paths). `dexo --connection NAME` opens a saved connection. `DATABASE_URL` from the environment or `.env` is offered as a temporary connection. A URL field in the connection form fills every field. The parser already exists in `crates/dexo-app/src/connection_url.rs`. This targets the first 5 minutes, where the stars come from (rainfrog `src/main.rs:60`, lazydb profile URL field, tabiew `tw <file>`, gobang #92).
2. **Quit guard for an open transaction or unapplied grid edits (S).** Ctrl+Q today discards both silently (`update.rs:2265`). A Commit / Rollback / Apply / Discard / Cancel dialog with Cancel focused fits Dexo's safety brand (lazydb "Transaction Exit").
3. **Row-count confirmation before COMMIT (M).** Run flagged UPDATE/DELETE statements, and every production write, inside a transaction or savepoint. Then show "DELETE 1,234 rows on shop-prod — type the name to commit / Esc rolls back". This is rainfrog's `confirm_tx` with Dexo's typed confirmation on top. It is the best upgrade to the guardrail GIF.
4. **Reach (S-M).** Add a musl or glibc-2.28 Linux build (so RHEL 9 and Amazon Linux 2023 can run Dexo), DuckDB in the release binaries (or a `-full` asset), and publish to crates.io + binstall and AUR `dexo-bin`. Then apply to homebrew-core and nixpkgs. rainfrog has all of these.
5. **Find in results + freeze columns (S).** Add `/` with n/N over loaded rows plus jump-to-column (tabiew), and expose the existing `frozen_columns` (tabiew #125). Both are small, used daily, and need no driver work.

Next in line: the go-to-object Omni palette (M, lazydb), browsing every database of a Postgres server (M-L, rainfrog #126), the MySQL `DELIMITER` command (S), a monitoring overview with rates (M, lazydb), and a column profile followed by a histogram (M, tabiew).
