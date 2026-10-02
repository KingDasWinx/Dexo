# Editor plugins and SQL language servers vs Dexo

Research date: 2026-10-02. Sources: shallow clones in `~/Documents/dexo-examples-projects/tui/{nvim-dbee,vim-dadbod-ui}`, `gh api` (READMEs, source files, issue search sorted by +1), Dexo `docs/src/cli.md` ("Language server") and `crates/dexo-cli/src/lsp.rs`.

## 0. What `dexo lsp` does today (baseline)

From `crates/dexo-cli/src/lsp.rs` (772 lines, hand-written JSON-RPC):

- Capabilities: `textDocumentSync: Full`, `completionProvider` (trigger `.`), `documentFormattingProvider`. Everything else returns `-32601`.
- Diagnostics are pushed on open/change: parse errors plus unknown schema/table/column (`Diagnoser` + `KnownObjects`). Every diagnostic is severity 1 (Error).
- Connection: `--connection NAME`, or a first line `-- dexo: connection=NAME`. No `initializationOptions`, no `workspace/didChangeConfiguration`, no per-directory binding.
- Catalog: only what Dexo has already cached in its SQLite store, which is opened read-only. The catalog is re-read when that file changes. The server never dials the database. If the connection is unknown or nothing is cached, it **says nothing**: you get no completions and no catalog diagnostics, with no message.
- Formatting: whole document only; honours `tabSize` and `insertSpaces`. Keywords are always uppercased.

Building blocks in `dexo-sql` and `dexo-storage` that the LSP does not use yet:
- `definition_at` (navigation.rs, used by the TUI's Go To Definition).
- `statement_at_in` and `split_statements_in` (statement.rs).
- Function signatures (`FunctionInfo.signature`, `BUILTINS` in completion.rs). Today they only fill completion `detail`.
- `statement_guard::destructive` and `is_read`.
- `named_parameters` (parameter.rs).
- Column `type` and `comment` attributes, plus `fk_*` attributes, on cached `CatalogObject`s, from all four drivers.
- `ObjectNoteRepository::for_connection` (the notes on tables and columns).

## 1. Projects

| Project | Stars | Last push | Open issues | One line |
|---|---|---|---|---|
| [kndndrj/nvim-dbee](https://github.com/kndndrj/nvim-dbee) | 1,318 | 2025-07-25 (about 14 months idle) | 79 | Neovim DB client: a Go backend (a remote plugin over msgpack-rpc) with a Lua UI made of a drawer, a scratchpad editor, a paged result split and a call log. 12 adapters (PG, MySQL, SQLite, DuckDB, SQL Server, Oracle, ClickHouse, BigQuery, Redshift, Databricks, Mongo, Redis). Self-described "alpha". |
| [kristijanhusak/vim-dadbod-ui](https://github.com/kristijanhusak/vim-dadbod-ui) | 2,053 | 2026-06-19 | 101 | Drawer UI over tpope's vim-dadbod (4,445★), which shells out to psql, mysql and the like. Features: saved queries, table helpers, bind parameters, jump to FK from output, execute on `:w`, statusline. It is the SQL story in LazyVim's `lang.sql` extra. |
| [tpope/vim-dadbod](https://github.com/tpope/vim-dadbod) (context) | 4,445 | 2026-01-07 | 58 | `:DB` command that pipes a range or buffer to the DB's CLI and shows the output in a preview window. |
| [kristijanhusak/vim-dadbod-completion](https://github.com/kristijanhusak/vim-dadbod-completion) (context) | 799 | 2025-03-19 | 18 | Completion source (omnifunc, cmp, blink) that queries the live DB through dadbod. |
| [sqls-server/sqls](https://github.com/sqls-server/sqls) | 1,335 | 2026-08-20 | 47 | Go SQL LSP that connects live to MySQL, PG, SQLite, MSSQL, H2 or Vertica. Has completion (including FK join completion), hover, signature help, definition, rename, range formatting, and code actions: execute query, switch connection, switch database, show tables. No diagnostics. Credentials go in a YAML or LSP config. "No stable release." |
| [joe-re/sql-language-server](https://github.com/joe-re/sql-language-server) | 777 | 2026-09-28 | 66 | TypeScript LSP for MySQL, PG, SQLite and BigQuery. Has completion, sqlint lint diagnostics with quick fixes, rename, and `executeCommand` switchDatabaseConnection and fixAllFixableProblems. Connections are bound to projects by `projectPaths`, with `${env:VAR}` substitution. |
| [supabase-community/postgres-language-server](https://github.com/supabase-community/postgres-language-server) (pgls, formerly postgres_lsp) | 5,260 | 2026-10-02 (active daily) | 21 | Rust, built on libpg_query, Postgres only. Has completion and hover (tables with column types, columns with nullability, function signatures), syntax diagnostics, type checking via `EXPLAIN` against the live DB, a migration-safety linter (squawk-style), a database linter (Splinter), PL/pgSQL support, and document and range formatting. Code actions: **Execute statement** and **Invalidate schema cache**. Available in VS Code, Neovim, Zed and Sublime. |

Distribution facts relevant to Dexo:
- nvim-lspconfig ships `lsp/sqls.lua`, `lsp/sqlls.lua` and `lsp/postgres_lsp.lua`. mason-registry ships `sqls`, `sqlls` and `postgres-language-server`. Dexo is in neither.
- Helix's default `languages.toml` has a `sql` language with **no language server**.
- LazyVim's `lang.sql` extra uses dadbod, dadbod-ui and dadbod-completion, with sqlfluff for lint and format. It uses **no SQL LSP at all**.

### Capability matrix (LSP)

| Capability | sqls | sql-language-server | pgls | dexo lsp |
|---|---|---|---|---|
| Completion | ✓ (+ FK joins) | ✓ | ✓ | ✓ (+ FK join conditions) |
| Diagnostics | ✗ | lint (sqlint) | syntax, type check, lint, DB lint | parse + unknown objects |
| Hover | ✓ | ✗ | ✓ | ✗ |
| Signature help | ✓ | ✗ | ✗ | ✗ |
| Go to definition | ✓ | ✗ | ✗ | ✗ (the TUI has it) |
| Rename | ✓ (aliases) | ✓ | ✗ | ✗ |
| Formatting | doc + range (buggy, see §3) | via lint fix | doc + range | doc only |
| Code actions / commands | execute, switch conn/db, show tables | fix all, switch conn | execute stmt, invalidate cache | ✗ |
| Needs a live DB | yes | yes | yes (for schema features) | **no** (cached catalog) |
| Credentials in editor config | yes | yes (or env) | yes (or env) | **no** (keychain or password command) |
| Engines | 6 | 4 | PG only | PG, MySQL, MariaDB, SQLite, DuckDB |

Dexo's real differentiators are four. It needs no credentials in the editor. A slow DB never blocks a keystroke. The TUI, CLI, LSP and MCP share one connection list and one catalog; nvim-dbee#42 asks for exactly this sharing between dbee and sqlls. And it covers five engines with one server. Its gaps are the interactive features that every competitor has: hover, execute, and switching connections.

## 2. Features Dexo (TUI or LSP) lacks

LSP rows first, then editor integration, then TUI. Effort: S is under a day, M is a few days, L is a week or more.

| Feature | Who has it (evidence) | Value | Effort | Notes |
|---|---|---|---|---|
| **Hover** on table, column or function: columns with types, comment, Dexo note, FK target, signature | pgls ([editor_features.md](https://github.com/supabase-community/postgres-language-server/blob/main/docs/features/editor_features.md)), sqls (`HoverProvider: true` in handler.go) | High | S–M | The data is already cached: the `type` and `comment` attributes, `fk_*`, and `ObjectNoteRepository::for_connection`. Resolve the token with `definition_at`, which already handles aliases. Dexo's notes in hovers are unique: no competitor has user notes. |
| **Execute statement** from the buffer (code action → command) | pgls `pgls.executeStatement` (code_actions.rs), sqls `executeQuery` (execute_command.go), dadbod `:DB`, dbee `BB` | High | M | Keep the server non-dialing by spawning `dexo query --connection X --sql … --format table --non-interactive`, which already enforces read-only, production and destructive policy. Write the output to `$XDG_CACHE_HOME/dexo/results/<doc>.txt` and send `window/showDocument`: a results split with no plugin, in Neovim and VS Code (check Helix). Offer the action only when `is_read` holds, and refuse writes with "run it in Dexo". The M is for moving off the single-threaded loop: run it on a thread that shares a locked writer. |
| **Connection switching and workspace binding** | sqls `switchConnections` and `switchDatabase`; sql-language-server `switchDatabaseConnection` + `projectPaths`; sqls#59 (+11) and #101 (+11) | High | S | (a) Read `initializationOptions.connection` and `workspace/didChangeConfiguration`. (b) Add a `dexo.useConnection` command that writes or replaces the `-- dexo: connection=` header through `workspace/applyEdit`, with a code action listing the saved connections. (c) Complete connection names on the header line. (d) Optional: walk up to a `.dexo` or `dexo.toml` file (`connection = "shop"`) so a migrations directory needs no header per file. |
| **Say why nothing works**, plus a refresh command | pgls `pgls.invalidateSchemaCache`; sqls#59 ("no database connection" on start, +11) | High | S | Today an unknown connection or an empty cache leaves the server silent. Send one `window/showMessage` per connection ("no catalog cached for shop: run `dexo inspect --connection shop --refresh`") and add `dexo.refreshCatalog`, which spawns that command. The existing file-stamp reload then picks up the result. |
| **Signature help** | sqls (`SignatureHelpProvider`) | Medium | S | The signatures exist (`BUILTINS` and catalog `FunctionInfo`). Find the call at the cursor and its active parameter by counting commas at depth 0. |
| **Destructive-statement warnings** | pgls safety linter (`banDropColumn`, `banDropTable`…, [linting.md](https://github.com/supabase-community/postgres-language-server/blob/main/docs/features/linting.md)) | Medium | S | Call `statement_guard::destructive` per statement and publish it as a Warning ("DELETE without WHERE touches every row"). This is the same rule the TUI, CLI and MCP enforce, and it extends Dexo's "safety" story into the editor. |
| **Diagnostic severity** | All the others distinguish severities | Low–Med | S | An unknown table or column against a possibly stale cache should be a Warning, not an Error. |
| **Range formatting** | sqls, pgls (`document_range_formatting_provider`) | Medium | S | Run `format_sql_with` on the selected range, so one statement in a long migration can be formatted. |
| **Keyword-case option** for format and completion | sqls `lowercaseKeywords`; sql-language-server#48, #204 | Medium | S | The formatter always uppercases, and a forced style blocks adoption of a formatter. Take it from `initializationOptions` or a Dexo setting. |
| Go to definition on a table or column | sqls (definition.go) | Low–Med | M | LSP needs a file location. Write the object's DDL to `$XDG_CACHE_HOME/dexo/ddl/<conn>/<obj>.sql` and return it. Hover covers most of the need. |
| Code lens "▶ Run" above each statement | Common in VS Code SQL extensions | Medium | M | Comes almost free once execute exists (`split_statements_in`). Neovim users often find lenses noisy, so make it opt-in. |
| Document symbols (statement outline) and folding | — | Low | S | `split_statements_in`. |
| Bind-parameter prompt when executing from the editor | dadbod-ui `:contactId` with remembered values; nvim-dbee#229 (+4) | Medium | M | Pass `--param`. Needs `window/showMessageRequest` or a plugin to ask for values. |
| SQL embedded in other languages (Rust, Go, TS, PHP strings) | Wanted: vim-dadbod-completion#59 (+4), sql-language-server#180, pgls#176/#177 | Medium | L | Skip for now. Tree-sitter injections already give Neovim users highlighting. |
| **Neovim plugin driving Dexo** (the lazygit.nvim pattern: a float with the TUI on the current file and connection) | nvim-dbee and dadbod-ui are this audience; lazygit.nvim proves the pattern | High | S–M | Needs one CLI addition: `dexo --connection NAME [FILE.sql]` opens the workbench with that document bound. The plugin is about 60 lines of Lua (float terminal, read the header, `:Dexo`). This puts grid editing, explain, schema diff and the rest inside Neovim. |
| Editor distribution: nvim-lspconfig `lsp/dexo.lua`, mason-registry package, Helix `languages.toml` entry, Zed extension | sqls, sqlls and pgls are in lspconfig and mason; pgls has a Zed extension (pgls#435) | High (reach) | S each (Zed M) | Mason can point at the GitHub release tarballs dist already builds. Helix ships no SQL LS, so `dexo lsp` with no arguments still gives parse diagnostics and formatting. |
| Zero-code recipes in the docs | dadbod is essentially this | Medium | S | Vim and Neovim: `:'<,'>w !dexo run --connection shop --non-interactive`. `dexo run` already reads stdin, and the output appears in the message area. |
| TUI: user-defined **table and database helpers** (templated queries on catalog nodes: `{table}`, `{schema}`) | dadbod-ui `g:db_ui_table_helpers`; dbee `extra_helpers` (Go templates); dadbod-ui#92 (+4) | Medium | S–M | Dexo has snippets and saved queries but no per-object templated actions in Object Actions. |
| TUI: **call log with archived results** (reopen a past result without re-running) | nvim-dbee call log (`archived` state, `show_result`) | Medium | M | Dexo keeps the SQL history but not the results ("no result pinning" is listed as missing). |
| TUI/CLI: connections from `DATABASE_URL` or `.env` | dadbod-ui `$DBUI_URL` and `DB_UI_*` from `.env`; dbee `EnvSource`; sqls#101 (+11) | Low–Med | S | `dexo "$DATABASE_URL"` already works, so this is mostly docs. Auto-reading `.env` could surprise users, so make it opt-in. |
| Drivers: SQL Server, Oracle, ClickHouse, BigQuery, Redshift, Snowflake, Mongo, Redis | nvim-dbee adapters; sqls (MSSQL, Vertica); MSSQL is the top request in sql-language-server#222 (+12) and sqls#184 (+6) | High (market) | L | A known gap. MSSQL is the most-requested engine across every tracker surveyed. |

## 3. Most-wanted requests in their trackers that Dexo could answer

The +N is the 👍 count on the open issue unless noted. "Dexo" says whether Dexo already answers the request or what would.

| Request | Where | Dexo |
|---|---|---|
| Edit result cells and propagate them to the DB | dadbod-ui [#233](https://github.com/kristijanhusak/vim-dadbod-ui/issues/233) (+8, 8c); dbee [#140](https://github.com/kndndrj/nvim-dbee/issues/140) (+4) | **Has it** (staged change set with review). Reachable from Neovim through the float plugin (§4 #5). |
| List all databases on the server | dadbod-ui [#205](https://github.com/kristijanhusak/vim-dadbod-ui/issues/205) (+9) | The drivers have `databases()` (PG `pg_database`). Likely covered; verify in the TUI. |
| List stored procedures and functions | dadbod-ui [#245](https://github.com/kristijanhusak/vim-dadbod-ui/issues/245) (+7) | `ObjectKind::Function/Procedure` are in the catalog. Has it. |
| Password from a password store | dadbod-ui [#185](https://github.com/kristijanhusak/vim-dadbod-ui/issues/185) (+5) | **Has it** (password command: `pass`, `op read`). |
| Export results as CSV | dadbod-ui [#354](https://github.com/kristijanhusak/vim-dadbod-ui/issues/354) (+5); vim-dadbod [#181](https://github.com/tpope/vim-dadbod/issues/181) (+4) | **Has it** (TUI export, `dexo query --format csv`). |
| Read-only connections | dadbod-ui [#273](https://github.com/kristijanhusak/vim-dadbod-ui/issues/273) (+2, 8c) and PR [#346](https://github.com/kristijanhusak/vim-dadbod-ui/pull/346) | **Has it**, enforced on the server too. |
| Open the UI over my own file instead of a note | dbee [#132](https://github.com/kndndrj/nvim-dbee/issues/132) (+5, its top issue) | The TUI opens and saves `.sql` files. `dexo --connection X file.sql` (§4 #5) would answer it exactly. |
| Bind parameters | dbee [#229](https://github.com/kndndrj/nvim-dbee/issues/229) (+4) | **Has it** in the TUI and CLI (`--param`). |
| Query timeout | dbee [#85](https://github.com/kndndrj/nvim-dbee/issues/85) (+2) | **Has it** (environment policy timeout). |
| Multiple queries shouldn't merge into one output | dbee [#118](https://github.com/kndndrj/nvim-dbee/issues/118) (+2) | **Has it** (result tabs). |
| SSH tunnel (MySQL) | dbee [#162](https://github.com/kndndrj/nvim-dbee/issues/162) (7c) | **Has it**, with `known_hosts` checked. |
| Disconnect the active connection | dbee [#69](https://github.com/kndndrj/nvim-dbee/issues/69) | **Has it**. |
| One connection config for the client and the LSP | dbee [#42](https://github.com/kndndrj/nvim-dbee/issues/42) | **Has it by design**: the TUI, CLI, LSP and MCP share one store. Market this. |
| Formatter must not destroy SQL | sqls [#149](https://github.com/sqls-server/sqls/issues/149) (**+31**, closed; the most-reacted sqls issue ever), [#105](https://github.com/sqls-server/sqls/issues/105) (+8) | Dexo's formatter keeps statements it cannot parse (`kept_any` in format.rs). Keep that guarantee and say so in the docs. |
| No connection on start / config via workspace settings fails | sqls [#59](https://github.com/sqls-server/sqls/issues/59) (+11, 8c) | Partly. Dexo needs no credentials in the editor, but it is silent when nothing is cached. Fix with §2 "say why nothing works". |
| `DATABASE_URL` as the connection source | sqls [#101](https://github.com/sqls-server/sqls/issues/101) (+11) | `dexo "$DATABASE_URL"` works for the TUI. The LSP needs a saved connection. Document it. |
| Completions outside the `public` schema | sqls [#99](https://github.com/sqls-server/sqls/issues/99) (+5); sql-language-server [#231](https://github.com/joe-re/sql-language-server/issues/231) (+3) | Dexo's catalog is schema-qualified; worth a test to claim it. |
| DuckDB support | sql-language-server [#229](https://github.com/joe-re/sql-language-server/issues/229) (+2) | **Has it** (optional build). |
| Prefer DB-specific completions | vim-dadbod-completion [#54](https://github.com/kristijanhusak/vim-dadbod-completion/issues/54) (+7) | **Has it** (dialect-aware completion). |
| SQL Server | sql-language-server [#222](https://github.com/joe-re/sql-language-server/issues/222) (+12); sqls [#184](https://github.com/sqls-server/sqls/issues/184) (+6); dbee #141, #160 | Missing (L). This is the recurring #1 engine request. |
| PL/pgSQL bodies | pgls [#179](https://github.com/supabase-community/postgres-language-server/issues/179) (+5) | Missing, and niche for Dexo. |
| Lowercase keywords | sql-language-server [#48](https://github.com/joe-re/sql-language-server/issues/48), [#204](https://github.com/joe-re/sql-language-server/issues/204) | Missing; the keyword-case option is S. |
| Freeze the header row while scrolling | dbee [#217](https://github.com/kndndrj/nvim-dbee/issues/217), [#148](https://github.com/kndndrj/nvim-dbee/issues/148); dadbod-ui [#187](https://github.com/kristijanhusak/vim-dadbod-ui/issues/187) (7c) | The TUI grid has a fixed header. |

**Takeaway:** most of the top requests in the two Neovim trackers are features Dexo's TUI already ships: edit results, read-only connections, password commands, export, parameters, result tabs, SSH, a shared config. What those users lack is a way into Dexo **from Neovim**. The LSP trackers want, in order: a connection setup that just works, a formatter that doesn't mangle SQL, MSSQL, and schema-aware completion.

## 4. Top 5 recommendations (ranked by value/effort)

1. **LSP hover** (S–M, high). On a table: qualified name, then the columns with types, then the comment and the Dexo note. On a column: type, FK target, comment, note. On a function: its signature. Everything comes from the cached catalog and notes, and `definition_at` already resolves aliases. This matches pgls and sqls, and notes in hovers are something no competitor has. Ship **signature help** in the same pass (S), since the signatures are already there.
2. **Connection ergonomics and the silent-failure fix** (S, high). Accept `initializationOptions.connection` and `workspace/didChangeConfiguration`. Add a `dexo.useConnection` code action and command that writes the `-- dexo: connection=` header, and complete connection names on that line. When nothing is cached, send one `showMessage`, and add a `dexo.refreshCatalog` command that spawns `dexo inspect --refresh`. This targets the most-upvoted LSP complaints (sqls#59, sqls#101) and the first-run cliff of Dexo's own design. Add **Warning severity** for unknown objects and **destructive-statement warnings** from `statement_guard::destructive` here too (S each).
3. **Get listed where editor users look** (S each, high reach). Submit `lsp/dexo.lua` to nvim-lspconfig, a package to mason-registry (from the dist release assets), and a `dexo` entry in Helix's `languages.toml`, where SQL has no default server today. Add a Zed extension later. Do this after #2 so first contact doesn't fail silently.
4. **Run from the editor, reads only** (M, high). Add an "Execute statement in Dexo" code action, offered when `is_read` holds. It spawns `dexo query … --format table --non-interactive`, writes a results file, and opens it with `window/showDocument`. That gives Neovim and VS Code a results split with no plugin, and Dexo's existing policy decides what runs. Writes answer "open it in Dexo". In the meantime, document `:'<,'>w !dexo run --connection shop` (zero code). Code lens and range formatting can follow.
5. **`dexo.nvim`, the lazygit.nvim pattern** (S–M, high). Add `dexo --connection NAME [FILE.sql]` to open the workbench bound to a file. Then a ~60-line Lua plugin that floats `dexo` on the current buffer and connection (from the header or `vim.b.dexo_connection`). That brings everything nvim-dbee and dadbod-ui users ask for (editing results, explain, read-only connections, export, schema diff) into Neovim without rebuilding it in Lua. It answers dbee#132, dbee#140 and dadbod-ui#233.

Deliberately not in the top 5:
- MSSQL is the most-requested engine everywhere, but it is a driver project (L), not editor work.
- Embedded SQL in other languages (L).
- Go to definition to a generated DDL file (M; hover covers most of it).
- Result history in the TUI (M).
- Table helpers in the TUI (S–M; medium value).
