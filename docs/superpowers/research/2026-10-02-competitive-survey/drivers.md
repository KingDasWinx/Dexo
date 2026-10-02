# Which database should Dexo support next?

Research date: 2026-10-02. Dexo at `development` (642fb76). Read-only: nothing was installed, built or run against a live server. Every claim about a compatible database's behaviour is from code reading plus vendor docs or competitor code. Claims marked *(verify)* need a container run.

---

## 1. What a driver costs in Dexo

### The API (`crates/dexo-driver-api/src/`)

The required part is small:

| Trait | Required? | What it is |
|---|---|---|
| `ConnectionFactory` (`connection.rs`) | **yes** | `descriptor()` and `connect(ConnectRequest) -> Box<dyn Session>` |
| `Session` (`connection.rs`) | **yes**, but only 4 methods | `capabilities()`, `execute(QueryRequest) -> QueryStream` (a stream of `QueryEvent`: result-set start, columns, row batches, notices, finish with `truncated`), `cancel(QueryId)`, `close()` |
| `TransactionControl` (`transaction.rs`) | optional (`None` by default) | begin RW/RO, commit, rollback, savepoints, state |
| `CatalogReader` (`catalog.rs`) | optional | `list_children`, `object`, `ddl`, `dependencies`, `dependents`, `foreign_keys`; `relations_named` and `databases` have defaults |
| `DataMutator` (`mutation.rs`) | optional | paged table reads with typed filters, sorts and raw WHERE/ORDER BY text, row estimates, keyed row edits with conflict detection |
| `BulkWriter` (`transfer.rs`) | optional | `insert_batch` (CSV/JSON import) |
| `ExplainProvider` (`explain.rs`) | optional | estimated or analyzed plan mapped to a `PlanNode` tree (`kind`, `relation`, `detail`, cost/rows/time, `native` JSON) |
| `DdlExecutor` + `SecurityAdmin` (`ddl.rs`, `schema_change.rs`) | optional | plan and apply `SchemaChange`s (schema editor), grants, passwords |
| `AdministrationProvider` (`admin.rs`) | optional | sessions, locks, blocking graph, sizes, stats, variables, cancel/kill/vacuum/analyze |
| `events()` | optional | server notices outside a query |

`Capability` (`capability.rs`) has 12 flags (Catalog, Query, Cancel, Transactions, DataWrite, Ddl, Explain, ExplainAnalyze, Admin, Import, Export, Backup). Each one can be `unavailable(reason)`, and the UI shows the reason. SQLite and DuckDB already leave out Ddl, Admin, Backup and SecurityAdmin (`crates/dexo-driver-sqlite/src/factory.rs:69`, `crates/dexo-driver-duckdb/src/factory.rs:187`). DuckDB has no savepoints. So a useful minimum is: **Query + Cancel + Catalog + Transactions + DataWrite + Explain**. The schema editor and the admin screens can wait.

Driver sizes today (src / test lines): SQLite 1,714 / 885, DuckDB 3,393 / 1,114, MySQL 4,549 / 2,521, Postgres 5,491 / 3,178. DuckDB landed as one ~3.6k-line commit plus about 20 fix commits on 2026-10-02 (`git log -i --grep=duckdb`). For comparison, the Rust rival lazydb's SQL Server driver is 5,033 lines and its Oracle driver 2,472 lines.

### What the driver crate does not cover (the hidden cost)

A new SQL family is more than a crate. Each item below was touched by DuckDB (`grep -rln Duckdb crates`):

1. **`dexo-sql`**:
   - a `Dialect` variant (`dialect.rs`: name, quote char, `needs_quotes`);
   - the **read/write guard** `statement_guard.rs`, which maps the dialect to a sqlparser dialect (`:387`, `:534`), finds the first keyword (`:481`) and holds dialect-only side-effect rules (`:258-321`);
   - lexing of comments and strings (`lex.rs`), statement splitting (`statement.rs::split_statements_in`; T-SQL would need `GO`), `format.rs`, `order.rs`, `derived.rs` (placeholder style `?` / `$1` / `@p1`), `diagnose.rs` keyword tables and `completion.rs`.

   About **36 `match dialect` sites** across dexo-sql/app/tui/mcp/cli, and 21 non-test files outside dexo-sql name `Dialect`. Exhaustive matches mean the compiler finds them all.
2. **sqlparser 0.62 already ships** `MsSqlDialect`, `ClickHouseDialect`, `OracleDialect`, `SnowflakeDialect`, `BigQueryDialect`, `RedshiftSqlDialect`, `DatabricksDialect` (`~/.cargo/registry/.../sqlparser-0.62.0/src/dialect/`). The guard can extend to any of them. Anything that fails to parse counts as a write ("Unknown statements count as writes", `crates/dexo-app/src/run_guard.rs`), so weak T-SQL or PL/SQL coverage fails safe: it refuses, it does not leak.
3. **`dexo-app`**:
   - `script.rs::dialect_for_driver`
   - `connection_url.rs` (URL scheme and query parameters)
   - `driver_registry.rs` (`FEATURE_GATED` when heavy)
   - `data/copy.rs` (literal rendering: booleans, bytes)
   - `transfer/codec.rs`
   - `mcp/connection.rs`. MCP serves **only Postgres/MySQL/MariaDB** today (`:25-31`); SQLite and DuckDB are refused. A new driver reaches agents only if this qualified-name mapping is extended.
4. **`dexo-driver-api` descriptor (`connection.rs`)**: `DriverDescriptor::<x>()`, `for_id`, `family`.
5. **TUI connection form** (`dexo-tui/src/screens/connection.rs`), **`dexo/src/main.rs`** registration, cargo feature if heavy (`docs/src/drivers.md`).
6. **CI**: `dexo-test-support/src/containers.rs` picks a testcontainers module by `DEXO_IT_IMAGE` prefix (`postgres:`, `mysql:`, `mariadb:`). `.github/workflows/integration.yml` runs the image matrix. `testcontainers-modules` 0.15 already has `mssql_server`, `oracle`, `clickhouse`, `cockroach_db`, `redis`, `mongo`, `scylladb` (no TiDB/Yugabyte; those need `GenericImage`).
7. **Product rules** (memory/spec): read-only profiles must be enforced **server-side as well as by the parser guard**. Postgres uses `default_transaction_read_only` (`crates/dexo-driver-postgres/src/factory.rs:35`), MySQL `SET SESSION TRANSACTION READ ONLY` (`crates/dexo-driver-mysql/src/factory.rs:41`), SQLite `query_only`. A database with **no server-side read-only mode** (SQL Server, Snowflake, BigQuery, Cassandra) can enforce it only with the parser guard plus a rolled-back transaction, plus a hint to use a read-only login. That is a real product cost, because "read-only you can trust" is part of Dexo's MCP pitch.

**What is free:** SSH tunnels, proxies and the secret store live in `dexo-app`/`dexo-transport` (`ConnectRequest.transport`). Any TCP driver that can dial a local endpoint gets them. TLS is per driver: Postgres has its own `tls.rs`, MySQL uses `mysql_async` `default-rustls`.

**Spec context:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md:107,120` already says "Not in 1.4.2: SQL Server" and "Non-goals: Thirty databases … SQL Server later". This report agrees with that direction.

---

## 2. Demand

### 2a. General popularity (Stack Overflow Developer Survey 2025, all respondents, fetched from survey.stackoverflow.co/2025/technology)

PostgreSQL 55.6, MySQL 40.5, SQLite 37.5, **SQL Server 30.1**, Redis 28.0, MongoDB 24.0, MariaDB 22.5, Elasticsearch 16.7, **Oracle 10.6**, DynamoDB 9.8, BigQuery 6.5, **Supabase 6.0**, Firestore 5.7, Snowflake 4.1, DuckDB 3.3, Cassandra 2.9, **ClickHouse 2.4**, Redshift 2.3, CockroachDB 1.0. Professional developers: SQL Server 30.9, Oracle 10.4.

DB-Engines (my background knowledge, not fetched) has long ranked Oracle, MySQL, SQL Server, PostgreSQL as the top four relational systems, with Snowflake, ClickHouse and Databricks the fastest risers. Dexo already covers 4 of the top 7 survey entries; **SQL Server is by far the largest uncovered SQL database**.

### 2b. Competitor support

Source: clones in `/home/winx/Documents/dexo-examples-projects/tui/`. 1P = first-party, P = paid/premium edition only, plug = plugin/extra.

| Competitor (language) | Supports |
|---|---|
| rainfrog (Rust) | Postgres, MySQL, SQLite (sqlx); DuckDB (feature, default on); **Oracle** (`oracle` 0.6.3, feature, default on, needs Instant Client); Redshift "via wire protocol" |
| lazydb (Rust) | Postgres, MySQL, MariaDB, SQLite (sqlx 0.9); **SQL Server** (`tiberius` 0.12, rustls); **Oracle** (`oracle`, feature `driver-oracle`, default on); **Redis** (`redis`) |
| gobang (Rust) | Postgres, MySQL, SQLite (sqlx 0.5) |
| lazysql (Go) | Postgres, MySQL, SQLite, **SQL Server**, **ClickHouse**. Redis planned (issues #317-319 closed "moving to GitHub Projects"); MongoDB unchecked in README |
| dblab (Go) | Postgres, MySQL, SQLite, **Oracle** (go-ora), **SQL Server** |
| usql (Go) | base tag: Postgres, MySQL, SQL Server, Oracle, SQLite, DuckDB, ClickHouse, CockroachDB, CrateDB, Redshift, MemSQL, TiDB, Vitess. `most`/`all`: ~40 more (Snowflake, BigQuery, Trino, Cassandra, Spanner, Databricks …) |
| harlequin (Python) | DuckDB, SQLite bundled; plug: postgres, mysql, odbc, bigquery, trino, databricks, adbc, cassandra, nebulagraph, chdb |
| sqlit (Python) | Postgres, **CockroachDB**, **Supabase**, MySQL, MariaDB, SQL Server, Oracle, Redshift, DuckDB/MotherDuck, SQLite, **Turso (libsql)**, **Cloudflare D1** (HTTP), ClickHouse, Snowflake, Databricks, BigQuery, Spanner, Athena, Trino, Presto, Db2, HANA, Teradata, … |
| beekeeper-studio (Electron) | Community: Postgres, **CockroachDB**, Redshift, MySQL, MariaDB, **TiDB**, SQLite, **SQL Server**, BigQuery, Redis. **Paid**: Oracle, Cassandra/Scylla, ClickHouse, MongoDB, DuckDB, Trino, Snowflake, LibSQL, DynamoDB, Firebird, SurrealDB |
| dbgate (Electron) | Postgres (+CockroachDB, Redshift P), MySQL, MariaDB, **SQL Server**, Oracle, SQLite (+LibSQL P, D1 P), MongoDB, Redis, ClickHouse, Cassandra, DuckDB, Firebird |
| mcp-toolbox (Go, MCP) | Postgres, **CockroachDB**, AlloyDB/Cloud SQL, **YugabyteDB**, MySQL, **TiDB**, SingleStore, OceanBase, **SQL Server**, Oracle, SQLite, Firebird, ClickHouse, Snowflake, Trino, Spanner, BigQuery, Bigtable, Firestore, MongoDB, Redis, Valkey, Cassandra, Scylla, Couchbase, Elasticsearch, Neo4j, … |
| dbhub (TS, MCP) | Postgres, MySQL, MariaDB, **SQL Server**, **Oracle**, SQLite (also handles TiDB as a MySQL flavour) |

Count across these 12: **SQL Server 10/12** (all but rainfrog and gobang), **Oracle 9/12** (one paid), **ClickHouse 7/12** (one paid), CockroachDB 5 (all through Postgres code), Cassandra 5, Trino 5, BigQuery 5, Snowflake 4, Redis 4, TiDB 4 (all through MySQL code), MongoDB 3, libSQL/Turso 3 (two paid), D1 2 (one paid).

Both MCP competitors (dbhub, mcp-toolbox) ship SQL Server and Oracle. In the MCP market, SQL Server is the line between "toy" and "enterprise".

### 2c. Issue tracker evidence

How the search was run:
- 207 searches through the REST search API: `gh api search/issues -f q='"<term>" repo:<repo>' -f sort=reactions-+1`. `gh search issues --json reactionGroups` is not a valid field, so the REST API's `reactions["+1"]` was used instead.
- Repos: rainfrog, lazysql, sqlit, harlequin, dblab, gobang, beekeeper-studio, dbgate, yelog/lazydb.
- Terms: 23, covering SQL Server/mssql, Oracle, ClickHouse, Redis, MongoDB, CockroachDB, libsql/Turso, Cassandra/ScyllaDB, Snowflake, BigQuery, TiDB, Neon, Supabase, D1, Trino/Presto, Elasticsearch/OpenSearch, YugabyteDB and PlanetScale.
- Raw output: `research/gh/results.jsonl`.

What the results look like:
- The two Electron trackers (beekeeper, dbgate) are where 👍 accumulate. The TUI trackers rarely pass 3👍.
- yelog/lazydb (54★) and Dexo (11★, no issues) have no evidence at all.

| Database | Competitors that support it (of 12) | Issue evidence (👍, URL) | Notes |
|---|---|---|---|
| **SQL Server** | **10**: lazydb, lazysql, dblab, usql, sqlit, beekeeper (community), dbgate, mcp-toolbox, dbhub; harlequin via its odbc plugin | Beekeeper shipped it on day one, so demand shows up as **auth** requests: Windows integrated login 12👍 [bks#48](https://github.com/beekeeper-studio/beekeeper-studio/issues/48); Windows auth connect help 7👍 [bks#534](https://github.com/beekeeper-studio/beekeeper-studio/issues/534); Azure SQL + Azure AD auth 5👍 open [bks#16](https://github.com/beekeeper-studio/beekeeper-studio/issues/16); pass-through Windows auth 3👍 open [bks#1186](https://github.com/beekeeper-studio/beekeeper-studio/issues/1186). Also lazysql "MSSQL support" 2👍 [#12](https://github.com/jorgerojas26/lazysql/issues/12) (shipped), dblab [#188](https://github.com/danvergara/dblab/issues/188) (shipped), harlequin 1👍 [#932](https://github.com/tconbeer/harlequin/issues/932) | Survey 30.1% (#4 overall, the largest uncovered). Users of tools that have it file schema and Azure bugs (lazysql [#342](https://github.com/jorgerojas26/lazysql/issues/342), [#290](https://github.com/jorgerojas26/lazysql/issues/290)) |
| **Oracle** | **9**: rainfrog, lazydb, dblab, usql, sqlit, dbgate, mcp-toolbox, dbhub; beekeeper (paid) | **35👍** [bks#20](https://github.com/beekeeper-studio/beekeeper-studio/issues/20) (largest SQL-engine request found); dbgate 1👍 [#95](https://github.com/dbgate/dbgate/issues/95); harlequin 1👍 [#814](https://github.com/tconbeer/harlequin/issues/814); rainfrog has 14 Oracle hits, all on its shipped driver | Survey 10.6%. Both Rust rivals ship it |
| **ClickHouse** | **7**: lazysql, usql, sqlit, dbgate, mcp-toolbox, harlequin (chdb plugin); beekeeper (paid) | 14👍 [bks#1335](https://github.com/beekeeper-studio/beekeeper-studio/issues/1335) + 5👍 [bks#470](https://github.com/beekeeper-studio/beekeeper-studio/issues/470); 13👍 [dbgate#104](https://github.com/dbgate/dbgate/issues/104); **6👍 open** [harlequin#230](https://github.com/tconbeer/harlequin/issues/230); 3👍 [lazysql#200](https://github.com/jorgerojas26/lazysql/issues/200); **3👍 open** [gobang#184](https://github.com/TaKO8Ki/gobang/issues/184) | Survey only 2.4%, but requested in **6 of the 9 trackers** (also sqlit [#17](https://github.com/Maxteabag/sqlit/issues/17)), the most of any engine. Users of terminal and analytics tools ask for it |
| MongoDB | 3: dbgate, mcp-toolbox; beekeeper (paid). nvim-dbee too | **46👍** [bks#258](https://github.com/beekeeper-studio/beekeeper-studio/issues/258) (most-👍 driver request overall); 2👍 open [lazysql#235](https://github.com/jorgerojas26/lazysql/issues/235) | Survey 24%. **Not SQL**: does not fit Dexo's editor, guard, grid or MCP-SQL model |
| Redis | 4: lazydb, beekeeper (community), dbgate, mcp-toolbox. nvim-dbee too; lazysql planned it but has not shipped | 10👍 [bks#80](https://github.com/beekeeper-studio/beekeeper-studio/issues/80); 5👍 open [dbgate#602](https://github.com/dbgate/dbgate/issues/602) (TLS) | Survey 28%. **Not SQL**. Needs a key browser |
| Snowflake | 4: usql, sqlit, mcp-toolbox; beekeeper (paid); harlequin only via ADBC | 17👍 [bks#443](https://github.com/beekeeper-studio/beekeeper-studio/issues/443); 5👍 [sqlit#127](https://github.com/Maxteabag/sqlit/issues/127) (auth); **3👍 open** [rainfrog#233](https://github.com/achristmascarl/rainfrog/issues/233); open [lazysql#347](https://github.com/jorgerojas26/lazysql/issues/347) | Survey 4.1%. Auth-heavy (key-pair, SSO, MFA) |
| libSQL / Turso | 3: sqlit; beekeeper (paid); dbgate (premium) | 17👍 [bks#1762](https://github.com/beekeeper-studio/beekeeper-studio/issues/1762); [dbgate#849](https://github.com/dbgate/dbgate/issues/849); [lazysql#49](https://github.com/jorgerojas26/lazysql/issues/49) | Local libSQL files are SQLite files |
| BigQuery | 5: usql, sqlit, beekeeper (community), mcp-toolbox, harlequin (plugin). nvim-dbee too | Only bug reports (bks#3009 1👍, harlequin [#363](https://github.com/tconbeer/harlequin/issues/363) shipped) | Survey 6.5%. Every query costs money |
| Trino / Presto | 5: usql, sqlit, mcp-toolbox, harlequin (plugin); beekeeper (paid) | 4👍 open [dbgate#621](https://github.com/dbgate/dbgate/issues/621); 3👍 [bks#2013](https://github.com/beekeeper-studio/beekeeper-studio/issues/2013); 1👍 [bks#344](https://github.com/beekeeper-studio/beekeeper-studio/issues/344) | Niche |
| Cassandra / Scylla | 5: usql, dbgate, mcp-toolbox, harlequin (plugin); beekeeper (paid) | 4👍 [dbgate#258](https://github.com/dbgate/dbgate/issues/258) | Survey 2.9%. CQL, no joins |
| Cloudflare D1 | 2: sqlit; dbgate (premium) | 3👍 open [bks#4260](https://github.com/beekeeper-studio/beekeeper-studio/issues/4260) | SQLite over REST |
| CockroachDB | 5, all through Postgres code: usql, sqlit, beekeeper, dbgate, mcp-toolbox (rainfrog: "should work") | 1👍 [dbgate#112](https://github.com/dbgate/dbgate/issues/112); open bug "not showing all tables" [bks#1177](https://github.com/beekeeper-studio/beekeeper-studio/issues/1177) | Survey 1.0% |
| TiDB | 4, all through MySQL code: usql, beekeeper, mcp-toolbox, dbhub (flavour detection) | 2👍 [bks#2010](https://github.com/beekeeper-studio/beekeeper-studio/issues/2010) | |
| Supabase / Neon | Every Postgres tool; sqlit has a Supabase host helper | Supabase connect bugs open, 0👍 ([bks#1942](https://github.com/beekeeper-studio/beekeeper-studio/issues/1942), [#1943](https://github.com/beekeeper-studio/beekeeper-studio/issues/1943)); no Neon-specific issues | Supabase is 6.0% of the survey, more than ClickHouse, Snowflake and DuckDB. These are Postgres users who hit pooler problems |
| YugabyteDB, PlanetScale/Vitess | mcp-toolbox (Yugabyte); usql (Vitess) | Only a question, [bks#1652](https://github.com/beekeeper-studio/beekeeper-studio/issues/1652) | |
| Elasticsearch / OpenSearch | mcp-toolbox | none | Survey 16.7%, but nobody asks a SQL TUI for it |

---

## 3. Feasibility in Rust

Crate data from the crates.io API on 2026-10-02 (total downloads / last 90 days / latest stable / last release / license).

Licences: every crate below passes `deny.toml`'s allow list, including `oracle`. Its `UPL-1.0/Apache-2.0` is a dual licence, so Apache-2.0 applies. `redis` is BSD-3-Clause, which is on the list.

Effort is relative: S = days, M ≈ DuckDB-sized (~3k lines), L ≈ MySQL-sized, XL = L plus a new UI model or a distribution problem.

| Database | Rust crate(s) | Maturity | Native deps | Server read-only / cancel / EXPLAIN | Effort | Risks |
|---|---|---|---|---|---|---|
| **SQL Server** | `tiberius` 5.33M / 728k, **0.13.0 (2026-09-25)**, MIT/Apache | Prisma's TDS driver. No release from 2024-07 to 0.13, which revived it: rustls 0.23 (the same as Dexo's), command and handshake timeouts, NTLM via pure-Rust `sspi-rs`, AAD token, client certs, TVPs. 444★, not archived | **None** (pure Rust); Kerberos (`integrated-auth-gssapi`) would need libgssapi | **RO: none on the server.** `ApplicationIntent=ReadOnly` only routes to AG secondaries; lazydb's `config.readonly()` (`lazydb/src/db/mssql.rs:172`) enforces nothing on a primary. Dexo would rely on the guard (sqlparser `MsSqlDialect`), on guarded reads inside `BEGIN TRAN … ROLLBACK`, and on telling users to use `db_datareader`. **Cancel: no TDS Attention in tiberius.** lazydb says so (`mssql.rs:3353`: "Tiberius exposes no TDS attention/cancel primitive. Closing the dedicated transport is the only safe way"). Cancel means dropping the connection and reconnecting, or `KILL <spid>`; either ends the session. **EXPLAIN: good.** `SET SHOWPLAN_XML ON` gives the estimate, `SET STATISTICS XML ON` the actuals. The XML `RelOp` carries PhysicalOp, EstimateRows, subtree cost and actual rows/time, which map to `PlanNode`. `quick-xml` is already in Cargo.lock | **L** | T-SQL in dexo-sql: `GO` splitting, `[ident]`, `TOP`, several result sets per batch (`QueryEvent` already indexes result sets), `@p1` params. Table DDL has to be synthesised (no SHOW CREATE). Paging needs ORDER BY. Types: datetimeoffset, money, sql_variant, uniqueidentifier, hierarchyid/geography (CLR). `rowversion` makes conflict detection easy. Auth expectations are high (see the bks auth 👍). Test image `mcr.microsoft.com/mssql/server` is x86_64 only, fine on ubuntu CI; `testcontainers_modules::mssql_server` exists. Upstream Attention support could be contributed |
| **Oracle** | `oracle` (rust-oracle) 3.16M / 1.11M, 0.6.3 (2025-01), UPL/Apache. `oracle-rs` 14k / 9.8k, 0.1.7 (2026-03), pure-Rust "thin", one maintainer, 4 months old. `sibyl` 59k (OCI) | rust-oracle is stable but slow-moving. It is the one rainfrog and lazydb use | **ODPI-C** (C, compiled by `cc`); **Oracle Instant Client loaded at runtime** (≈100+ MB, Oracle licence, cannot ship in Homebrew/Scoop/winget/deb). lazydb has a client locator (`src/db/oracle_client.rs`) | RO: `SET TRANSACTION READ ONLY` ✓. Cancel: `Connection::break_execution` ✓. EXPLAIN: `EXPLAIN PLAN FOR` + `PLAN_TABLE` rows (id/parent_id/operation/cost/cardinality) map well; actuals need `V$SQL_PLAN_STATISTICS_ALL` privileges. DDL: `DBMS_METADATA.GET_DDL` ✓ | **XL** | Sync API, so the `spawn_blocking` pattern like SQLite/DuckDB. PL/SQL blocks with `/` terminators; `''` = NULL; NUMBER/DATE/TIMESTAMP TZ/INTERVAL/LOB types; identifiers fold to upper case (`needs_quotes` logic flips). sqlparser `OracleDialect` is new and thin. "Install Instant Client" is the top support issue in rivals (sqlit [#300](https://github.com/Maxteabag/sqlit/issues/300) thick mode, dbgate [#843](https://github.com/dbgate/dbgate/issues/843)) |
| **ClickHouse** | `clickhouse` (official) **14.4M / 4.92M**, 0.15.2 (2026-08-28), MIT/Apache. `klickhouse` 282k (native TCP), `clickhouse-arrow` 1.35M (native, Arrow) | Official and very active (6 releases in 2026) | None. HTTP on hyper 1 (already in Dexo's lockfile), rustls feature | **RO: best of any candidate.** `readonly=1` per query or session is enforced by the server. **Cancel:** set `query_id` per request, then `KILL QUERY WHERE query_id=…` ✓. **EXPLAIN:** `EXPLAIN PLAN json=1, actions=1, indexes=1` ✓; no EXPLAIN ANALYZE, so ExplainAnalyze is unavailable (actuals live in `system.query_log`) | **M** | `fetch_bytes("JSONCompactEachRowWithNamesAndTypes")` or `RowBinaryWithNamesAndTypes` gives untyped rows for an ad-hoc editor (`Query::fetch_bytes`, docs.rs). No transactions; rows have no identity, and UPDATE/DELETE are async mutations, so grid edits and Ddl are unavailable (like DuckDB's Ddl). Catalog from `system.databases/tables/columns`, `SHOW CREATE TABLE` ✓. Admin is cheap: `system.processes`, `system.parts`, KILL. sqlparser `ClickHouseDialect` ✓; testcontainers `clickhouse` ✓ |
| Redis | `redis` 109.5M / 27.9M, 1.7.1, BSD-3. `fred` 9.4M, MIT | Excellent | None | n/a (no SQL) | **XL** | `Session::execute` takes SQL, and the catalog, guard, grid edits, EXPLAIN and MCP SQL tools have no meaning here. It needs its own browser UI, as lazydb built. Off-thesis |
| MongoDB | `mongodb` (official) 18.7M / 4.68M, 3.9.1, Apache-2.0 | Excellent | None | n/a | **XL** | Same mismatch: documents and pipelines, not SQL |
| libSQL / Turso | `libsql` 2.22M / 861k, 0.9.30 (0.10.0-pre in 2026), MIT. `turso` 1.03M / 573k, 0.8.1 with daily pre-releases (the Rust rewrite). `libsql-client` dead (2024) | libsql works but is in transition; turso is pre-1.0 churn | `libsql` with `default-features=false, features=["remote"]` is a pure-Rust Hrana/HTTP client. The local core bundles libSQL's SQLite fork, and linking it next to rusqlite's bundled SQLite is a symbol-clash risk *(verify)* | RO: a Turso read-only token is server-side ✓. Cancel: drop the request (weak). EXPLAIN: SQLite's `EXPLAIN QUERY PLAN` ✓ | **M** (S if the SQLite driver's SQL catalog and mutation code, which is plain SQL over `sqlite_master` and `pragma_*` (`crates/dexo-driver-sqlite/src/catalog.rs`), is lifted behind an executor) | Local libSQL files are SQLite files and should open in the existing SQLite driver today *(verify: vector columns, ALTER COLUMN)*. The gap is remote Turso/sqld only |
| Cloudflare D1 | No crate for use outside Workers; REST `POST /accounts/{a}/d1/database/{id}/query` | n/a | None | RO by token scope only; no interactive transactions (atomic batch only); no cancel | **S–M** after libSQL | The same "SQLite over HTTP" core as Turso remote |
| Cassandra / Scylla | `scylla` (official) 9.23M / 1.67M, 1.9.0, MIT/Apache. `cdrs-tokio` 831k | Excellent, tokio-native | None (rustls or openssl) | RO: roles only. Cancel: drop the stream. EXPLAIN: none (tracing) | **L** | CQL has no sqlparser dialect, so a small CQL classifier is needed for the guard. No joins or transactions. Low demand |
| Snowflake | `snowflake-connector-rs` 4.3M / 1.04M, 1.1.0 (2026-07), MIT (estie). `snowflake-api` 7.4M, 0.14.0 (2025-10), Apache | Community, not official | None | RO: role-based only. Cancel ✓ (abort request). `EXPLAIN USING JSON` ✓ | **L** | Auth matrix (password, key-pair JWT, OAuth, SSO browser, MFA). Every query burns credits, a hazard for MCP agents |
| BigQuery | `gcp-bigquery-client` 7.75M / 1.53M, 0.28.0, MIT/Apache. `google-cloud-bigquery` 257k (official google-cloud-rust, young) | Community, plus a young official crate | None | RO: none. Cancel: job cancel ✓. Plan only after the run; a dry run gives bytes scanned | **L** | ADC, service-account and OAuth auth; per-byte cost; projects and datasets do not map to catalog/schema 1:1 |
| Trino / Presto | `trino-rust-client` 456k / 353k, 0.12.0 (2026-08), MIT. `prusto` 394k (2024) | Community | None (HTTP) | `START TRANSACTION READ ONLY` ✓. Cancel via `DELETE nextUri` ✓. `EXPLAIN (FORMAT JSON)` and `EXPLAIN ANALYZE` ✓ | **M–L** | Catalogs of catalogs; low demand |
| Elasticsearch / OpenSearch | `elasticsearch` 23.4M but **9.1.0-alpha.1** (the official client is still alpha). `opensearch` 3.36M, 2.4.0 | Alpha / OK | None | SQL plugin (`_sql`) is a SELECT-only subset | M | No demand from SQL-tool users |
| *(long tail)* ODBC | `odbc-api` 1.69M / 256k, 29.1.1, MIT | Excellent | **unixODBC plus vendor drivers at runtime** | Generic only | M | An escape hatch (harlequin and usql have one), not a "next driver" |

**Binary size.** `target/release/dexo` is 57.9 MB with DuckDB; `~/.cargo/bin/dexo` from 2026-09-24 is 41.5 MB. TDS (tiberius) and the HTTP clients (clickhouse, libsql-remote) reuse rustls 0.23, tokio-rustls 0.26, hyper 1 and tokio-util, which are already locked. My estimate, not measured, is +1–3 MB each. Oracle adds little to the binary, but users need the separate Instant Client download.

---

## 4. Postgres- and MySQL-compatible databases through today's drivers

### What the existing drivers assume

**Postgres** (`crates/dexo-driver-postgres/src/`):
- Read-only profiles use the startup parameter `options=-c default_transaction_read_only=on` (`factory.rs:35-36`).
- Every query goes through `client.prepare`, a **named** prepared statement (`session.rs:240`).
- Guarded reads use `SAVEPOINT dexo_read_only` / `BEGIN READ ONLY` (`session.rs:79-104`).
- EXPLAIN is `EXPLAIN (FORMAT JSON)` and `(ANALYZE, FORMAT JSON)`, plus `GENERIC_PLAN` from server version 16 (`explain.rs:13-15,189,294`).
- Row estimates use `pg_partition_tree` (`mutation.rs:293`).
- The catalog uses `pg_get_*def`, `pg_depend`, `pg_rewrite`, `pg_policy`, `pg_publication`, `pg_inherits` and `pg_get_partkeydef` (`catalog/mod.rs`).
- Admin uses `pg_stat_activity`, `pg_locks`, `pg_cancel_backend` and `pg_terminate_backend` (`admin.rs`).

**MySQL** (`crates/dexo-driver-mysql/src/`):
- Read-only profiles run `SET SESSION TRANSACTION READ ONLY` at connect (`factory.rs:41`).
- Every guarded read (WHERE bar, re-run ORDER BY, counts) runs `SELECT @@session.transaction_read_only`, then `SET SESSION TRANSACTION READ ONLY`, then `SAVEPOINT` (`session.rs:76-120`).
- EXPLAIN is `FORMAT=JSON` / `FORMAT=TREE` (`explain.rs:75-78`).
- Admin reads `performance_schema.data_locks` (`admin.rs:106`).
- The driver already detects a **flavour from `version()`**, for MariaDB (`factory.rs:52-58`, `MysqlExplainCaps::mariadb()`). That is the hook for TiDB.

`DriverDescriptor::family()` (`dexo-driver-api/src/connection.rs`) already maps `"mariadb" => "mysql"`. A "CockroachDB" or "TiDB" entry in the connection form costs one descriptor and one match arm.

### Per database

| Database | Driver | Works today? | What breaks (code reading + vendor docs) | Cost to make it official |
|---|---|---|---|---|
| RDS / Aurora PostgreSQL, Cloud SQL, AlloyDB, Azure Flexible, Crunchy, DO, Timescale/Tiger | postgres | **Yes**: genuine PostgreSQL | Nothing specific. Gap: CI tests PG 14/16/17 (`.github/workflows/integration.yml:19-23`), and **PG 18 (GA 2025-09) is not in the matrix**. `postgres_matrix_status` (`support.rs`) still calls 18 unverified, though nothing outside the API calls it | **S**: add `postgres:18` to the matrix; one docs page "hosted Postgres" |
| **Neon** | postgres | **Direct endpoint: yes. Pooled (`-pooler`, PgBouncer transaction mode): partly** | (1) A read-only profile's startup `options` is refused: PgBouncer accepts only tracked startup parameters, so Neon's pooler answers "unsupported startup parameter in options" (Neon connection-errors doc; PgBouncer `ignore_startup_parameters`/`track_extra_parameters` docs). (2) Named prepared statements work only if the pooler tracks them (`max_prepared_statements`, default 200 in current PgBouncer) *(verify on Neon)*. (3) Session state (SET, temp tables, hypopg's session indexes, notices) is unreliable between transactions. Scale-to-zero adds cold-start latency to the first connect | **S**: recognise those two error texts and say "use the direct (non-pooler) host"; optionally, when `options` is refused, reconnect without it and rely on per-statement `BEGIN READ ONLY` plus the guard. Docs |
| **Supabase** | postgres | **Direct and Supavisor session mode (5432): yes. Transaction mode (6543): no** | The direct host `db.<ref>.supabase.co` is IPv6-only without the IPv4 add-on. Supavisor transaction mode **disables startup options and prepared statements** (Supabase docs, Supavisor FAQ; named statements only behind a per-tenant flag), so read-only profiles fail to connect and **every query fails** (`prepare`). Pooler user is `postgres.<ref>` | **S**: the same error recognition plus docs ("use session mode, port 5432"). An optional URL helper like sqlit's (`providers/supabase/adapter.py:21-32`) is not needed |
| **CockroachDB** | postgres | **Querying, catalog basics, cancel, read-only: probably yes. Explain: no** | Reports server_version 13.x (Postgres-compatible number; usql overrides version reading for this). **`EXPLAIN (FORMAT JSON)` does not exist** (docs list VERBOSE/TYPES/OPT/VEC/DISTSQL/…), so Explain fails. `pg_partition_tree` in row estimates and some `pg_get_*def`/`pg_policy`/`pg_publication` lookups are likely missing or stubbed *(verify)*. `reltuples` estimates are unreliable (beekeeper `postgresql.ts:1533`: "doesn't work in redshift or cockroach"). Admin: `pg_terminate_backend` becomes `CANCEL SESSION`, `pg_locks` is thin. Schema changes are online and async, so a `DdlPlan` must not claim to be transactional. pgwire cancel ✓, `default_transaction_read_only` ✓, savepoints ✓. Competitors: beekeeper overrides columns, indexes, partitions, triggers and DDL in a 242-line `clients/cockroach.ts`; sqlit disables procedures and triggers; dbgate and beekeeper detect it from `version()` | **M** (a few hundred lines): flavour flag from `version()` ILIKE '%CockroachDB%'; Explain parsed from `EXPLAIN (VERBOSE)` rows, or `unavailable(reason)` first; catalog and estimate fallbacks; CRDB admin verbs; a "CockroachDB" descriptor (port 26257) with family postgres; `testcontainers_modules::cockroach_db` in the matrix |
| YugabyteDB (YSQL) | postgres | **Mostly** | It is a Postgres fork: 2.20 LTS is PG 11.2, 2.25+ is PG 15. On PG11-based releases `pg_partition_tree` (PG 12+) is missing, so estimates fail; `GENERIC_PLAN` is already version-gated. FORMAT JSON ✓ | **S** once Cockroach-style fallbacks exist; GenericImage `yugabytedb/yugabyte` |
| Redshift | postgres | **Largely no** | A PG 8.0.2 fork: no FORMAT JSON explain, different system views (`svv_*`), most `pg_get_*` and `pg_depend` paths fail; beekeeper and dbgate have separate clients | M–L. Not now (survey 2.3%) |
| RDS / Aurora MySQL, Cloud SQL, Azure MySQL | mysql | **Yes** | Nothing specific. The matrix tests MySQL 8.0/8.4/9.3 and MariaDB 10.11/11.4; MariaDB **11.8 LTS** is missing | **S**: add `mariadb:11.8` |
| **TiDB** (and TiDB Cloud) | mysql | **Connects, but read-only and guarded reads fail; Explain fails** | (1) TiDB rejects `START/SET [SESSION] TRANSACTION READ ONLY` unless `tidb_enable_noop_functions` is on, and when it is on the statement is a no-op, so nothing is enforced. dbhub detects TiDB from `version()` (`8.0.11-TiDB-v7.5.0`) and skips it (`dbhub/src/utils/server-flavor.ts:6-12`, `src/connectors/mysql/index.ts:165-189`). Dexo issues that statement both at connect for read-only profiles and **for every guarded read on any TiDB connection** (`session.rs:85-103`), so the WHERE bar, re-sorts and counts break *(verify)*. (2) TiDB supports neither `FORMAT=JSON` nor `FORMAT=TREE` (docs.pingcap.com: "TiDB does not support the FORMAT=JSON or FORMAT=TREE options"); it has `FORMAT='tidb_json'` and the row format with `estRows`/`actRows`. (3) No `performance_schema.data_locks` (TiDB has `information_schema.data_lock_waits`). Savepoints since v6.2 ✓; KILL ✓ (global kill). mcp-toolbox forces TLS for `*.tidbcloud.com` | **S–M**: flavour flag next to MariaDB's; guarded reads fall back to the parser guard plus a transaction rolled back after it (TiDB's real read-only switches are global and admin-only); a `tidb_json` explain mapper; admin restrictions; a "TiDB" descriptor (port 4000); GenericImage `pingcap/tidb` (standalone, mocktikv) |
| PlanetScale (Vitess MySQL) / Vitess | mysql | **Mostly** *(verify)* | TLS is mandatory (supported). Vitess has savepoints (v12+) and KILL (v18+). `SET SESSION TRANSACTION READ ONLY` may need reserved connections *(verify)*. EXPLAIN FORMAT=JSON passes through on unsharded keyspaces; sharded keyspaces need `VEXPLAIN`. No `performance_schema`, so admin locks degrade to the existing restriction path (`admin.rs:114`). PlanetScale Postgres is real Postgres | **S** to test with `vitess/vttestserver`; fixes depend on findings |
| SingleStore, OceanBase, StarRocks, Doris | mysql | Partly | Divergent catalogs and EXPLAIN | Not worth it now |

**Verdict.** "Tested compatibility" is far cheaper than any new driver:
- Neon, Supabase, Aurora/RDS, Cloud SQL, AlloyDB and Azure are genuine engines. They need **error messages, docs and two CI images** (PG 18, MariaDB 11.8), not code paths.
- TiDB and CockroachDB need a **flavour flag** of the kind MySQL already has for MariaDB, plus fixes measured in hundreds of lines, and they then appear as named engines in the connection form, as they do in beekeeper, dbgate, usql, sqlit and mcp-toolbox.

None of this has been run against a live server. The TiDB guarded-read and the CockroachDB catalog/estimate findings are the first thing to confirm with containers.

---

## 5. Recommendation

### 1st: the "hosted and compatible" pack (no new driver). Effort S–M

1. CI matrix: add `postgres:18` and `mariadb:11.8`. PG 18 has been GA for a year and is what new Neon, Supabase and RDS projects get.
2. Pooler-aware errors for Neon and Supabase: "unsupported startup parameter … options" and "prepared statement … does not exist". Each should say "use the direct or session-mode endpoint". Add a docs page on hosted Postgres and MySQL (Supabase, Neon, RDS/Aurora, Cloud SQL/AlloyDB, Azure, PlanetScale).
3. **TiDB flavour** next to MariaDB's in the MySQL driver:
   - read-only and guarded reads that do not use `READ ONLY` syntax;
   - a `tidb_json` explain;
   - admin restrictions;
   - a "TiDB" descriptor with family `mysql`;
   - a `pingcap/tidb` image in CI.
4. **CockroachDB flavour** in the Postgres driver:
   - Explain from CRDB's own output, or unavailable with a reason;
   - catalog and estimate fallbacks;
   - CANCEL QUERY/SESSION admin;
   - a "CockroachDB" descriptor with family `postgres`;
   - a `cockroach_db` container in CI.

Why first:
- Supabase alone (6.0% in the survey) outnumbers ClickHouse, Snowflake and DuckDB users.
- The r/PostgreSQL and Show HN launch audience is mostly on Neon, Supabase and RDS.
- Today two of those setups fail in confusing ways: read-only profiles on pooled endpoints, and every query on Supavisor transaction mode.
- TiDB currently breaks Dexo's guarded reads, its read-only story and Explain *(verify)*.
- The fixes are hundreds of lines, they reuse the `version()` flavour pattern already in the MySQL driver, and they add four named engines to the connection form for marketing. Every competitor lists CockroachDB/TiDB as named engines.
- It fits "few big releases": one release, no new crate.

### 2nd: SQL Server (new driver, `tiberius` 0.13). Effort L

Why:
- It is the largest SQL database Dexo lacks: #4 in the survey at 30.1%.
- 10 of 12 competitors ship it, including the Rust rival lazydb and both MCP rivals (dbhub, mcp-toolbox).
- The spec already queues it ("SQL Server later").
- Feasibility is good:
  - pure Rust and maintained again, sharing rustls 0.23;
  - no runtime dependency and an official test container;
  - sqlparser's `MsSqlDialect` for the guard;
  - showplan XML maps cleanly onto `PlanNode`;
  - `rowversion` gives honest conflict detection.

Two things must be decided up front and documented:
- **Cancel ends the session.** Tiberius has no TDS Attention. The options are to drop the connection and reconnect (lazydb does this), or to contribute Attention upstream.
- **SQL Server has no server-side read-only mode.** Read-only profiles rest on the parser guard plus reads rolled back inside a transaction, and docs telling users to connect with a `db_datareader` login. MCP docs must say this plainly.

Other notes:
- Auth is where SQL Server users complain: bks#48 12👍, #534 7👍, #16 5👍. Ship SQL login, NTLM via `sspi-rs` and an Entra ID access token in v1. Defer Kerberos and interactive Entra.
- Wire MCP's qualified-name mapping (`dexo-app/src/mcp/connection.rs`) in the same release so agents get it. That is the enterprise unlock for the MCP pitch.

### 3rd: ClickHouse (new driver, official `clickhouse` crate). Effort M

Why ClickHouse over Oracle:
- Demand is the broadest of any missing engine in terminal and analytics trackers: requested in 6 of the 9 trackers, with 14👍, 13👍, 6👍 and two open 3👍 requests.
- It is half the cost, a DuckDB-shaped read-mostly driver.
- It has the **best server-enforced read-only** of any candidate (`readonly=1`) and clean cancel by `query_id`. That makes it a safe MCP analytics target and extends the DuckDB "analytics" story.
- The official crate is very active, and `fetch_bytes` gives untyped rows.
- Grid edits, DDL and Analyze start as `unavailable(reason)`.

### After that, only on evidence

- **Oracle.** Its demand is real: bks#20 35👍, and both rainfrog and lazydb ship it. But it is XL: ODPI-C plus an Instant Client that users install by hand, PL/SQL, and type quirks. Do it behind a cargo feature once Dexo's own tracker asks for it. Watch `oracle-rs` (pure-Rust thin): if it matures, the distribution problem disappears.
- **libSQL/Turso remote and D1.** One "SQLite over HTTP" driver reusing the SQLite driver's SQL catalog: cheap (M) and low risk, but small demand (bks#1762 17👍, bks#4260 3👍). First confirm that local libSQL files already open in the SQLite driver; a docs line may be enough.
- **Not recommended:**
  - Redis and MongoDB: not SQL, they need a second UI, and they dilute the SQL-workbench and guard identity despite high 👍 (46 for MongoDB).
  - Snowflake and BigQuery: auth sprawl, community crates, and per-query cost that is dangerous behind an MCP agent.
  - Cassandra, Trino, Elasticsearch: low demand.
  - Redshift: a separate catalog dialect for 2.3% of developers.
