# MCP Hardening and Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Dexo's MCP server safe and real: reads that cannot write, an allowlist that cannot be bypassed, grants that cannot reach past the profile, typed tools that talk to the live database, and a protocol test suite that proves each spec requirement MCP-001..020.

**Architecture:** Part A (Tasks 1–5) fixes the P0 security holes found in the 2026-09-23 review inside `dexo-sql` and `dexo-app`, with only the adapter glue needed to keep `dexo-mcp` compiling. Part B (Tasks 6–16) rewrites the `dexo-mcp` adapter on rmcp's `#[tool_router]` macros: a backend trait supplied by the CLI, a per-connection router with lazy sessions, typed inputs, Markdown results with `structuredContent`, audit on every call, and a protocol suite. Part C lists the requirements that stay blocked on other spec work.

**Tech Stack:** Rust 1.93 (edition 2024), rmcp 3.1.2 (`server`, `transport-io`, `macros`, `schemars`), schemars 1.2, sqlparser 0.62 (`visitor` feature, already declared in the workspace, unused until now), tokio / tokio-util, rusqlite (via `dexo-storage`), insta 1.48.

**Spec:** `docs/superpowers/specs/2026-08-31-dexo-professional-parity-program-design.md` — §22 (MCP-001..020), FLOW-014, SEC-002/004/007/008/010/011/012/015/016/026, QUALITY-009/015/019. Review evidence (live probes against `pg-commerce-lab`) is summarized in "Findings this plan fixes" below.

## Global Constraints

- MCP stays stdio-only and server-only; no HTTP listener, no embedded client (MCP-001; spec §"Fora de escopo": "Servidor MCP HTTP ou daemon de rede", "Cliente MCP embutido").
- Profiles start disabled and read-only; bootstrap exposes no write tool (MCP-002).
- The MCP process never creates, lists or revokes grants (MCP-003). Grants come from `dexo mcp grant create`.
- A denied object and a missing object produce the same error text, `not found` (existing `hidden_error()` contract, SEC-012 "reason" is given only for policy refusals that do not reveal an object).
- CLI and MCP reuse app services and policies; no parallel DML or DDL builders in the adapter (spec §30 item 9, MCP-008).
- No secret, connection password or result row ever reaches stdout outside a JSON-RPC message, and no SQL text ever reaches the audit log (only its hash) (SEC-002, MCP-012).
- CI gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo deny check`. Run all three before each commit.
- Config files (`Cargo.toml` and friends) get the setting only, never an explanatory comment block.
- Tests: this repo's standing preference is "no tests while implementing plan tasks". This plan deviates only where the spec demands tests (MCP-016, QUALITY-009/015/019) or where a security property has no other way to be checked (the statement guard, selector matching, transaction ordering, grant scope). Every such test is written out in full below; do not add others.

## Review Focus

These are the inputs most likely to hurt someone that no happy-path test would catch. Each has a test in the task named after it.

1. **Identifier case and quoting on Postgres** — `SELECT * FROM Stores` must resolve to `stores`, `"Mixed"` must stay `Mixed`; a case mismatch must never turn a denied table into an allowed one (Task 1, `postgres_folds_unquoted_identifiers`).
2. **MySQL two-part names** — `shop.items` must match the selector `shop.*` and never `shop.public.*` (Task 2, `two_part_mysql_names_match_catalog_selectors`).
3. **Unknown environment labels** — a connection saved with environment `prod` or `PRD` must be treated as production and refuse MCP writes (Task 3, `unknown_environment_labels_fail_closed`).
4. **A client cancelling mid-query** — the read transaction must be rolled back and the next call on the same connection must work (Task 3, `a_cancelled_read_cancels_the_query_and_rolls_back`; Task 15, `a_cancel_notification_only_stops_its_own_request`).
5. **A connection that is down or has no keychain secret** — the call reports `CONNECTION_FAILED`, the server keeps serving other connections, and the next call retries the connect (Task 7, `a_failed_connect_is_retried_on_the_next_call`).

## Findings this plan fixes

| # | Finding (live-verified unless noted) | Fixed in |
| --- | --- | --- |
| F1 | Reads run in a normal read-write session (`transaction_read_only = off`) | Task 3 |
| F2 | First-keyword classifier: `SELECT … INTO`, `SELECT … FOR UPDATE`, `SELECT pg_terminate_backend()` pass as reads; `EXPLAIN ANALYZE DELETE` passes (code-read) | Task 1, 3 |
| F3 | Allowlist checks only the first `" FROM "`: `JOIN`, comma joins and a newline after `FROM` leak denied tables | Task 1, 3 |
| F4 | One `notifications/cancelled` (any request id) bricks the server for good | Task 3 |
| F5 | `Grant::authorizes` ignores the profile's deny rules and the grant's connection | Task 4 |
| F6 | DDL typed confirmation is auto-filled with the target | Task 5 |
| F7 | `tool_rules` denials are ignored for `query_execute_read` / `query_explain` | Task 5, 7 |
| F8 | Writes without a session are recorded as `Succeeded` and spend the grant | Task 4 |
| F9 | `data_insert/update/delete` invent table metadata (first column = PK) | Task 11 |
| F10 | `schema_diff`, `query_explain` (no session) and `query_execute_read` (no session) return fabricated text | Task 3, 10 |
| F11 | No CLI way to set a profile's connections, query mode, limits or tool rules | Task 6 |
| F12 | `mcp serve` loads the newest catalog snapshot of *any* connection, connects only the first profile connection and swallows connect errors | Task 3, 7 |
| F13 | Selector and object parsing disagree for two-part (MySQL) names; column objects of a denied table are visible | Task 2 |
| F14 | Every tool has description = name and one shared schema with no `required` | Task 7 |
| F15 | Result rows are Rust `Debug` strings without column names; byte limit is estimated as `len * 8` | Task 3, 7 |
| F16 | Only writes are audited; `duration_ms/rows/bytes` are always 0; `mcp audit` prunes with a hard-coded 30 days | Task 13 |
| F17 | Dead modules: `router.rs` (test-only), `limits.rs`, `cancellation.rs`, `audit.rs::record`, `TextOut` | Task 7, 14 |

## File Structure

**`dexo-sql`**
- Create `crates/dexo-sql/src/statement_guard.rs` — sqlparser-based inspection: what kind of statement it is and every relation it names.

**`dexo-app`**
- Rewrite `crates/dexo-app/src/mcp/selector.rs` — path-based selectors and object references.
- Create `crates/dexo-app/src/mcp/connection.rs` — `McpConnection`: dialect, defaults and write eligibility of one saved connection.
- Modify `crates/dexo-app/src/mcp/service.rs` — authorization against real relations, read-only transactional reads, paged reads, writes, visibility.
- Modify `crates/dexo-app/src/mcp/grant.rs`, `policy.rs`, `profile.rs`, `mod.rs`.
- Modify `crates/dexo-app/src/data/copy.rs` (export `display_value`), `crates/dexo-app/src/data/change_set.rs` (`TableMeta::from_keys`).

**`dexo-test-support`**
- Create `crates/dexo-test-support/src/session.rs` — `FakeSession`, an in-memory `Session` that logs every call.

**`dexo-mcp`** (Part B rewrite)
- `src/backend.rs` — `McpBackend` trait the host binary implements.
- `src/router.rs` — per-connection lazy sessions and name resolution.
- `src/schema.rs` — typed tool inputs.
- `src/render.rs` — Markdown tables plus `structuredContent`.
- `src/error.rs` — error codes.
- `src/server.rs` — `DexoMcpServer`, gating, concurrency, audit.
- `src/tools_read.rs`, `src/tools_write.rs` — tools on `#[tool_router]`.
- `src/resources.rs`, `src/prompts.rs`, `src/stdio.rs`.
- Delete `src/limits.rs`, `src/cancellation.rs`, `src/audit.rs`.
- Tests: `tests/common/mod.rs`, `tests/protocol.rs`, `tests/snapshots/`; delete `tests/multi_connection.rs`.

**`dexo-cli`**
- Create `crates/dexo-cli/src/mcp_backend.rs` — `CliMcpBackend` (keychain, drivers, SQLite).
- Modify `crates/dexo-cli/src/args.rs`, `crates/dexo-cli/src/run.rs`, `crates/dexo-cli/src/lib.rs`.

**Other**
- Modify `crates/dexo-storage/src/mcp_profile.rs`, `crates/dexo-tui/src/runtime/mod.rs`, `crates/dexo-tui/src/update.rs`.
- Rewrite `crates/dexo/tests/mcp_read.rs`, `crates/dexo/tests/mcp_write.rs`; modify `crates/dexo/tests/mcp_stdio.rs`.
- Rewrite `docs/src/mcp.md`; modify `docs/src/cli.md`.

---

# Part A — Security (P0)

### Task 1: Statement guard in `dexo-sql` (SEC-010, SEC-026, MCP-002, MCP-006)

The old check reads the first keyword (`dexo-sql/src/statement.rs:77`) and the first `" FROM "` (`dexo-app/src/mcp/service.rs:215`). This task replaces both, for MCP, with a real parse. The code below was compiled and run against every live bypass from the review before this plan was written.

**Files:**
- Modify: `crates/dexo-sql/Cargo.toml`
- Create: `crates/dexo-sql/src/statement_guard.rs`
- Modify: `crates/dexo-sql/src/lib.rs`

**Interfaces:**
- Produces: `dexo_sql::Inspection { pub relations: Vec<Vec<String>> }` (sorted, deduplicated identifier paths).
- Produces: `dexo_sql::GuardRejection` (`thiserror` enum, `Display` is the user-facing reason).
- Produces: `dexo_sql::inspect_read(sql: &str, dialect: Dialect) -> Result<Inspection, GuardRejection>`
- Produces: `dexo_sql::inspect_data_write(sql: &str, dialect: Dialect) -> Result<Inspection, GuardRejection>`
- Produces: `dexo_sql::inspect_schema_write(sql: &str, dialect: Dialect) -> Result<Inspection, GuardRejection>`

- [ ] **Step 1: Add the parser dependency**

In `crates/dexo-sql/Cargo.toml`, under `[dependencies]`, add:

```toml
sqlparser = { workspace = true, features = ["visitor"] }
```

- [ ] **Step 2: Create the guard**

Create `crates/dexo-sql/src/statement_guard.rs`:

```rust
use std::ops::ControlFlow;

use sqlparser::ast::{Expr, ObjectName, ObjectNamePart, Query, Select, Statement, Visit, Visitor};
use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect};
use sqlparser::parser::Parser;

use crate::Dialect;

/// What a statement touches, as identifier paths such as `["db", "public", "orders"]`.
/// Unquoted Postgres identifiers are folded to lowercase, as the server folds them, so
/// the allowlist compares the name the server will actually resolve.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inspection {
    pub relations: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum GuardRejection {
    #[error("statement could not be parsed: {0}")]
    Unparsed(String),
    #[error("exactly one statement is required")]
    NotSingleStatement,
    #[error("statement is not allowed for this tool")]
    WrongKind,
    #[error("EXPLAIN ANALYZE executes the statement")]
    ExplainAnalyze,
    #[error("locking clauses (FOR UPDATE/SHARE) are not reads")]
    LockingClause,
    #[error("SELECT INTO creates a table")]
    SelectInto,
    #[error("nested data-modifying statements are not allowed")]
    NestedStatement,
    #[error("function {0} is not allowed")]
    Function(String),
}

/// Functions with side effects, or that run SQL passed as text and so would slip past the
/// relation allowlist. The read-only transaction is the real guarantee; this list turns
/// the known escapes into a clear refusal before anything reaches the server.
const DENIED_FUNCTIONS: &[&str] = &[
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_promote",
    "pg_switch_wal",
    "pg_create_restore_point",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "pg_ls_logdir",
    "pg_ls_waldir",
    "pg_stat_file",
    "lo_import",
    "lo_export",
    "lo_unlink",
    "lo_create",
    "lo_from_bytea",
    "set_config",
    "pg_advisory_lock",
    "pg_advisory_xact_lock",
    "pg_advisory_lock_shared",
    "pg_try_advisory_lock",
    "pg_try_advisory_xact_lock",
    "pg_sleep",
    "pg_sleep_for",
    "pg_sleep_until",
    "nextval",
    "setval",
    "pg_notify",
    "dblink",
    "dblink_exec",
    "dblink_connect",
    "dblink_send_query",
    "query_to_xml",
    "query_to_xml_and_xmlschema",
    "query_to_xmlschema",
    "cursor_to_xml",
    "table_to_xml",
    "table_to_xml_and_xmlschema",
    "schema_to_xml",
    "database_to_xml",
    "ts_stat",
    "ts_rewrite",
    "sleep",
    "benchmark",
    "get_lock",
    "release_lock",
    "release_all_locks",
    "load_file",
    "sys_exec",
    "sys_eval",
];

/// A single `SELECT`/`VALUES`/`TABLE`/`WITH … SELECT`, or a plain `EXPLAIN` of one.
pub fn inspect_read(sql: &str, dialect: Dialect) -> Result<Inspection, GuardRejection> {
    let statement = parse_one(sql, dialect)?;
    let query = match &statement {
        Statement::Query(query) => query.as_ref(),
        Statement::Explain {
            analyze,
            statement,
            options,
            ..
        } => {
            let analyze_option = options.iter().flatten().any(|option| {
                option.name.value.eq_ignore_ascii_case("analyze")
                    && !matches!(&option.arg, Some(Expr::Value(value)) if value.to_string().eq_ignore_ascii_case("false"))
            });
            if *analyze || analyze_option {
                return Err(GuardRejection::ExplainAnalyze);
            }
            match statement.as_ref() {
                Statement::Query(query) => query.as_ref(),
                _ => return Err(GuardRejection::WrongKind),
            }
        }
        _ => return Err(GuardRejection::WrongKind),
    };
    let mut guard = Guard::new(dialect, 0);
    let _ = query.visit(&mut guard);
    guard.finish(Vec::new())
}

/// A single top-level `INSERT`, `UPDATE` or `DELETE`. Every table it reads or writes is
/// returned, so the caller can hold all of them to the grant.
pub fn inspect_data_write(sql: &str, dialect: Dialect) -> Result<Inspection, GuardRejection> {
    let statement = parse_one(sql, dialect)?;
    if !matches!(
        statement,
        Statement::Insert(_) | Statement::Update(_) | Statement::Delete(_)
    ) {
        return Err(GuardRejection::WrongKind);
    }
    let mut guard = Guard::new(dialect, 1);
    let _ = statement.visit(&mut guard);
    guard.finish(Vec::new())
}

/// A single `CREATE TABLE`, `ALTER TABLE`, `DROP`, `CREATE INDEX` or `CREATE VIEW`.
/// sqlparser does not visit `DROP` names or a view's own name as relations, so those are
/// added here explicitly.
pub fn inspect_schema_write(sql: &str, dialect: Dialect) -> Result<Inspection, GuardRejection> {
    let statement = parse_one(sql, dialect)?;
    let named: Vec<ObjectName> = match &statement {
        Statement::CreateTable(_) | Statement::AlterTable(_) | Statement::CreateIndex(_) => {
            Vec::new()
        }
        Statement::Drop { names, .. } => names.clone(),
        Statement::CreateView(view) => vec![view.name.clone()],
        _ => return Err(GuardRejection::WrongKind),
    };
    let mut guard = Guard::new(dialect, 1);
    let _ = statement.visit(&mut guard);
    let extra = named.iter().map(|name| guard.path(name)).collect();
    guard.finish(extra)
}

fn parse_one(sql: &str, dialect: Dialect) -> Result<Statement, GuardRejection> {
    let mut statements = match dialect {
        Dialect::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
        Dialect::Mysql => Parser::parse_sql(&MySqlDialect {}, sql),
    }
    .map_err(|error| GuardRejection::Unparsed(error.to_string()))?;
    if statements.len() != 1 {
        return Err(GuardRejection::NotSingleStatement);
    }
    Ok(statements.remove(0))
}

struct Guard {
    dialect: Dialect,
    statements_allowed: usize,
    statements_seen: usize,
    ctes: Vec<String>,
    relations: Vec<Vec<String>>,
    rejection: Option<GuardRejection>,
}

impl Guard {
    fn new(dialect: Dialect, statements_allowed: usize) -> Self {
        Self {
            dialect,
            statements_allowed,
            statements_seen: 0,
            ctes: Vec::new(),
            relations: Vec::new(),
            rejection: None,
        }
    }

    fn reject(&mut self, rejection: GuardRejection) -> ControlFlow<()> {
        self.rejection = Some(rejection);
        ControlFlow::Break(())
    }

    fn path(&self, name: &ObjectName) -> Vec<String> {
        name.0
            .iter()
            .map(|part| match part {
                ObjectNamePart::Identifier(ident)
                    if ident.quote_style.is_none() && self.dialect == Dialect::Postgres =>
                {
                    ident.value.to_lowercase()
                }
                ObjectNamePart::Identifier(ident) => ident.value.clone(),
                ObjectNamePart::Function(function) => function.name.value.clone(),
            })
            .collect()
    }

    fn finish(self, extra: Vec<Vec<String>>) -> Result<Inspection, GuardRejection> {
        if let Some(rejection) = self.rejection {
            return Err(rejection);
        }
        let ctes = self.ctes;
        let mut relations: Vec<Vec<String>> = self
            .relations
            .into_iter()
            .chain(extra)
            .filter(|path| !(path.len() == 1 && ctes.contains(&path[0])))
            .collect();
        relations.sort();
        relations.dedup();
        Ok(Inspection { relations })
    }
}

impl Visitor for Guard {
    type Break = ();

    fn pre_visit_statement(&mut self, _statement: &Statement) -> ControlFlow<()> {
        self.statements_seen += 1;
        if self.statements_seen > self.statements_allowed {
            return self.reject(GuardRejection::NestedStatement);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        if !query.locks.is_empty() {
            return self.reject(GuardRejection::LockingClause);
        }
        if let Some(with) = &query.with {
            for cte in &with.cte_tables {
                let name = self.path(&ObjectName::from(vec![cte.alias.name.clone()]));
                self.ctes.extend(name);
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<()> {
        if select.into.is_some() {
            return self.reject(GuardRejection::SelectInto);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<()> {
        let path = self.path(relation);
        self.relations.push(path);
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        if let Expr::Function(function) = expr {
            let name = self
                .path(&function.name)
                .last()
                .map(|name| name.to_lowercase())
                .unwrap_or_default();
            if DENIED_FUNCTIONS.contains(&name.as_str()) {
                return self.reject(GuardRejection::Function(name));
            }
        }
        ControlFlow::Continue(())
    }
}
```

- [ ] **Step 3: Export it**

In `crates/dexo-sql/src/lib.rs`, add `pub mod statement_guard;` after `pub mod statement;`, and after the `pub use statement::…` line add:

```rust
pub use statement_guard::{
    GuardRejection, Inspection, inspect_data_write, inspect_read, inspect_schema_write,
};
```

- [ ] **Step 4: Pin every bypass from the review with tests**

Append to `crates/dexo-sql/src/statement_guard.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::{GuardRejection, inspect_data_write, inspect_read, inspect_schema_write};
    use crate::Dialect;

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    fn reads(sql: &str) -> Vec<Vec<String>> {
        inspect_read(sql, Dialect::Postgres).unwrap().relations
    }

    fn refused(sql: &str) -> GuardRejection {
        inspect_read(sql, Dialect::Postgres).unwrap_err()
    }

    #[test]
    fn postgres_folds_unquoted_identifiers() {
        assert_eq!(reads("select id from Stores"), vec![path(&["stores"])]);
        assert_eq!(reads("select * from \"Mixed\""), vec![path(&["Mixed"])]);
    }

    #[test]
    fn every_joined_table_is_reported() {
        assert_eq!(
            reads("select count(*) from db1.public.stores s join db1.public.employees e on true"),
            vec![
                path(&["db1", "public", "employees"]),
                path(&["db1", "public", "stores"])
            ]
        );
        assert_eq!(reads("select * from stores s, employees e").len(), 2);
        assert_eq!(reads("select count(*) from\nemployees"), vec![path(&["employees"])]);
        assert_eq!(
            reads("select (select max(id) from employees) from stores").len(),
            2
        );
    }

    #[test]
    fn cte_names_are_not_tables_but_their_bodies_are() {
        assert_eq!(
            reads("with e as (select * from employees) select * from e"),
            vec![path(&["employees"])]
        );
        assert!(reads("select 1").is_empty());
        assert_eq!(reads("explain select * from stores"), vec![path(&["stores"])]);
    }

    #[test]
    fn reads_that_write_or_lock_are_refused() {
        assert_eq!(
            refused("select * into copy_of_stores from stores"),
            GuardRejection::SelectInto
        );
        assert_eq!(
            refused("select * from stores for update"),
            GuardRejection::LockingClause
        );
        assert_eq!(
            refused("select pg_terminate_backend(1) from stores"),
            GuardRejection::Function("pg_terminate_backend".into())
        );
        assert_eq!(
            refused("select query_to_xml('select * from employees', true, true, '')"),
            GuardRejection::Function("query_to_xml".into())
        );
        assert_eq!(
            refused("explain analyze delete from stores"),
            GuardRejection::ExplainAnalyze
        );
        assert_eq!(
            refused("explain (analyze) select 1"),
            GuardRejection::ExplainAnalyze
        );
        assert_eq!(refused("explain delete from stores"), GuardRejection::WrongKind);
        assert_eq!(
            refused("with x as (delete from stores returning *) select * from x"),
            GuardRejection::NestedStatement
        );
        assert_eq!(refused("delete from stores"), GuardRejection::WrongKind);
        assert_eq!(
            refused("select 1; select 2"),
            GuardRejection::NotSingleStatement
        );
        assert!(matches!(refused("selec 1"), GuardRejection::Unparsed(_)));
    }

    #[test]
    fn mysql_names_and_functions() {
        let read = |sql: &str| inspect_read(sql, Dialect::Mysql);
        assert_eq!(
            read("select * from shop.Items").unwrap().relations,
            vec![path(&["shop", "Items"])]
        );
        assert_eq!(
            read("select sleep(10)").unwrap_err(),
            GuardRejection::Function("sleep".into())
        );
        assert!(read("select * from items into outfile '/tmp/x'").is_err());
    }

    #[test]
    fn data_writes_report_every_table_they_touch() {
        let write = |sql: &str| inspect_data_write(sql, Dialect::Postgres);
        assert_eq!(
            write("insert into db1.public.stores (id) select id from employees")
                .unwrap()
                .relations,
            vec![path(&["db1", "public", "stores"]), path(&["employees"])]
        );
        assert_eq!(
            write("update stores set name = 'x' where id = 1")
                .unwrap()
                .relations,
            vec![path(&["stores"])]
        );
        assert_eq!(
            write("delete from stores using employees where stores.id = employees.id")
                .unwrap()
                .relations
                .len(),
            2
        );
        assert_eq!(
            write("with x as (select id from employees) delete from stores").unwrap_err(),
            GuardRejection::WrongKind
        );
        assert_eq!(write("drop table stores").unwrap_err(), GuardRejection::WrongKind);
        assert_eq!(
            write("update stores set id = nextval('s')").unwrap_err(),
            GuardRejection::Function("nextval".into())
        );
    }

    #[test]
    fn schema_writes_name_their_targets() {
        let ddl = |sql: &str| inspect_schema_write(sql, Dialect::Postgres);
        assert_eq!(
            ddl("drop table public.stores, public.employees")
                .unwrap()
                .relations,
            vec![path(&["public", "employees"]), path(&["public", "stores"])]
        );
        assert_eq!(
            ddl("create view v as select * from employees")
                .unwrap()
                .relations,
            vec![path(&["employees"]), path(&["v"])]
        );
        assert_eq!(
            ddl("create index idx on public.stores (name)")
                .unwrap()
                .relations,
            vec![path(&["public", "stores"])]
        );
        assert_eq!(
            ddl("create table t3 as select * from employees")
                .unwrap()
                .relations,
            vec![path(&["employees"]), path(&["t3"])]
        );
        assert_eq!(ddl("truncate public.stores").unwrap_err(), GuardRejection::WrongKind);
        assert_eq!(
            inspect_schema_write("drop table shop.items", Dialect::Mysql)
                .unwrap()
                .relations,
            vec![path(&["shop", "items"])]
        );
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p dexo-sql statement_guard`
Expected: 7 tests pass.

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo clippy -p dexo-sql --all-targets --all-features -- -D warnings && cargo deny check
git add crates/dexo-sql Cargo.lock
git commit -m "fix(sql): parse MCP statements instead of reading the first keyword"
```

---

### Task 2: Path-based selectors (MCP-006, MCP-004)

Today a selector with two segments means `catalog.schema`, while an object name with two segments means `schema.object`. On MySQL, where names are `database.table`, no selector can ever match. Column objects are stored as `table.column` in the object slot, so the column of a denied table slips through. This task matches selectors against the object's path, segment by segment, from the front.

**Files:**
- Rewrite: `crates/dexo-app/src/mcp/selector.rs`
- Modify: `crates/dexo-app/src/mcp/policy.rs:188-203` (specificity type only)
- Modify: `crates/dexo-app/src/mcp/grant.rs:180-192` (`sample_object`)
- Modify: `crates/dexo-app/src/mcp/profile.rs:396-401` (delete `column_isolation`)
- Modify: `crates/dexo-app/src/mcp/service.rs` (`new` filter, drop `column_isolation` uses)
- Modify: `crates/dexo-storage/src/mcp_profile.rs:60,182-198`
- Modify: `crates/dexo-tui/src/runtime/mod.rs:1547`
- Modify: `crates/dexo-cli/src/run.rs:1240,1289`

**Interfaces:**
- Produces: `Selector { pub segments: Vec<Segment> }`, `Selector::parse(&str) -> Result<Selector, AppError>` (1..=3 segments), `Selector::matches(&ObjectRef) -> bool`, `Selector::specificity() -> usize`, `impl Display for Selector`.
- Produces: `impl Display for SelectorRule` → `"allow db.public.*"` / `"deny db.public.secrets"`.
- Produces: `ObjectRef { pub path: Vec<String> }` with `ObjectRef::parse(&str)`, `ObjectRef::from_qualified(&QualifiedName)`, `ObjectRef::from_catalog_object(&CatalogObject)`, `impl Display`.
- Removes: `Selector::{catalog, schema, object, column}` fields, `ObjectRef::{catalog, schema, name, column}` fields, `McpProfile::column_isolation`.

Column-level selectors (four segments) are dropped. Their only effect was to switch off raw SQL for the whole profile; the spec asks for connection and object allowlists (MCP-006), not column ones. A profile saved with a four-segment selector will fail to load with "selector must have 1..=3 segments" — recreate it.

- [ ] **Step 1: Rewrite `selector.rs`**

Replace the whole file `crates/dexo-app/src/mcp/selector.rs` with:

```rust
use std::fmt;

use dexo_driver_api::{CatalogObject, ObjectKind, QualifiedName};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    Exact(String),
    Star,
}

impl Segment {
    fn parse(part: &str) -> Result<Self, AppError> {
        if part == "*" {
            return Ok(Self::Star);
        }
        if part.is_empty() || part.contains('*') {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "selectors allow exact names or explicit * only",
            ));
        }
        Ok(Self::Exact(part.to_string()))
    }

    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Star => true,
            Self::Exact(name) => name == value,
        }
    }
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Star => f.write_str("*"),
            Self::Exact(name) => f.write_str(name),
        }
    }
}

/// A dotted pattern matched segment by segment against the front of an object's path.
/// `db.public.*` covers every object in schema `public` of catalog `db`, and `db.*`
/// covers the whole catalog. Postgres paths are `catalog.schema.object` and MySQL paths
/// `catalog.object`, so one rule shape serves both without guessing which slot a name
/// belongs in.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Selector {
    pub segments: Vec<Segment>,
}

impl Selector {
    pub fn parse(pattern: &str) -> Result<Self, AppError> {
        let segments = pattern
            .split('.')
            .map(Segment::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if segments.is_empty() || segments.len() > 3 {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "selector must have 1..=3 segments (catalog.schema.object)",
            ));
        }
        Ok(Self { segments })
    }

    pub fn matches(&self, object: &ObjectRef) -> bool {
        self.segments.len() <= object.path.len()
            && self
                .segments
                .iter()
                .zip(&object.path)
                .all(|(segment, part)| segment.matches(part))
    }

    pub fn specificity(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| matches!(segment, Segment::Exact(_)))
            .count()
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.segments.iter().map(ToString::to_string).collect();
        f.write_str(&parts.join("."))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SelectorRule {
    pub effect: Effect,
    pub selector: Selector,
}

impl SelectorRule {
    pub fn parse(effect: Effect, pattern: &str) -> Result<Self, AppError> {
        Ok(Self {
            effect,
            selector: Selector::parse(pattern)?,
        })
    }
}

impl fmt::Display for SelectorRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let effect = match self.effect {
            Effect::Allow => "allow",
            Effect::Deny => "deny",
        };
        write!(f, "{effect} {}", self.selector)
    }
}

/// An object's position in the catalog tree, outermost segment first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectRef {
    pub path: Vec<String>,
}

impl ObjectRef {
    pub fn parse(qualified: &str) -> Self {
        Self {
            path: qualified
                .split('.')
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn from_qualified(name: &QualifiedName) -> Self {
        let mut path: Vec<String> = name
            .catalog()
            .into_iter()
            .chain(name.schema())
            .map(str::to_string)
            .collect();
        path.push(name.object().to_string());
        Self { path }
    }

    /// Drivers repeat a node's own name in the object slot (a Postgres schema is
    /// `db.public.public`) and store columns as `table.column`; both are mapped to the
    /// path the node really occupies, so a column belongs to its table.
    pub fn from_catalog_object(object: &CatalogObject) -> Self {
        let name = &object.qualified_name;
        match &object.kind {
            ObjectKind::Catalog => Self {
                path: vec![name.object().to_string()],
            },
            ObjectKind::Schema => Self {
                path: name
                    .catalog()
                    .into_iter()
                    .map(str::to_string)
                    .chain([name.object().to_string()])
                    .collect(),
            },
            ObjectKind::Column => {
                let mut reference = Self::from_qualified(name);
                if let Some(last) = reference.path.last_mut()
                    && let Some((table, _)) = last.split_once('.')
                {
                    *last = table.to_string();
                }
                reference
            }
            _ => Self::from_qualified(name),
        }
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::{Effect, ObjectRef, Selector, SelectorRule};
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};

    fn object(kind: ObjectKind, name: QualifiedName) -> CatalogObject {
        CatalogObject::new(ObjectId::new("x"), kind, name, None)
    }

    #[test]
    fn two_part_mysql_names_match_catalog_selectors() {
        let items =
            ObjectRef::from_qualified(&QualifiedName::new(Some("shop"), None::<String>, "items"));
        assert!(Selector::parse("shop.*").unwrap().matches(&items));
        assert!(Selector::parse("shop.items").unwrap().matches(&items));
        assert!(!Selector::parse("shop.public.*").unwrap().matches(&items));
    }

    #[test]
    fn a_column_belongs_to_its_table() {
        let column = object(
            ObjectKind::Column,
            QualifiedName::new(Some("db"), Some("public"), "secrets.email"),
        );
        assert_eq!(
            ObjectRef::from_catalog_object(&column).path,
            ["db", "public", "secrets"]
        );
    }

    #[test]
    fn container_nodes_do_not_repeat_their_name() {
        let schema = object(
            ObjectKind::Schema,
            QualifiedName::new(Some("db"), Some("public"), "public"),
        );
        let catalog = object(
            ObjectKind::Catalog,
            QualifiedName::new(Some("db"), None::<String>, "db"),
        );
        assert_eq!(ObjectRef::from_catalog_object(&schema).path, ["db", "public"]);
        assert_eq!(ObjectRef::from_catalog_object(&catalog).path, ["db"]);
    }

    #[test]
    fn column_and_partial_wildcard_selectors_are_rejected() {
        assert!(Selector::parse("db.public.t.col").is_err());
        assert!(Selector::parse("db.pub*").is_err());
        assert!(Selector::parse("").is_err());
    }

    #[test]
    fn rules_render_as_they_are_typed() {
        let rule = SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap();
        assert_eq!(rule.to_string(), "deny db.public.secrets");
    }
}
```

- [ ] **Step 2: Update the policy's sort key**

In `crates/dexo-app/src/mcp/policy.rs`, `decide` keeps its body; `specificity()` now returns `usize`, which `std::cmp::Reverse` accepts unchanged. No edit is needed unless the compiler flags one. The existing tests keep using `ObjectRef::parse`, which still exists.

- [ ] **Step 3: Update grant scope sampling**

In `crates/dexo-app/src/mcp/grant.rs`, change the import line to

```rust
use crate::mcp::selector::{ObjectRef, Segment, Selector, SelectorRule};
```

and replace `fn sample_object` with:

```rust
fn sample_object(selector: &Selector) -> ObjectRef {
    ObjectRef {
        path: selector
            .segments
            .iter()
            .map(|segment| match segment {
                Segment::Exact(name) => name.clone(),
                Segment::Star => "probe".into(),
            })
            .collect(),
    }
}
```

- [ ] **Step 4: Drop column isolation**

In `crates/dexo-app/src/mcp/profile.rs`, delete `pub fn column_isolation(&self) -> bool { … }`.

In `crates/dexo-app/src/mcp/service.rs`:
- in `McpService::new`, replace `policy.decide(&ObjectRef::parse(&object.qualified_name.display_unquoted()))` with `policy.decide(&ObjectRef::from_catalog_object(object))`;
- in `authorize_read_sql`, change the first condition to `if self.profile.query_mode != QueryMode::RawReadSql {`;
- in `advertised_tools`, change `if profile.query_mode == QueryMode::RawReadSql && !profile.column_isolation() {` to `if profile.query_mode == QueryMode::RawReadSql {`.

- [ ] **Step 5: Render rules with `Display` everywhere**

In `crates/dexo-storage/src/mcp_profile.rs`:
- in `save`, replace `let pattern = display_selector(&rule.selector);` with `let pattern = rule.selector.to_string();`;
- delete `fn display_selector`.

In `crates/dexo-tui/src/runtime/mod.rs` (inside `load_mcp_profiles`), replace `.map(|rule| format!("{rule:?}"))` with `.map(ToString::to_string)`.

In `crates/dexo-cli/src/run.rs`:
- in `McpProfileCommand::Enable`, replace `println!("  {:?}", rule.effect);` with `println!("  {rule}");`;
- in `mcp_policy`, replace `println!("selector {:?}", rule.effect);` with `println!("selector {rule}");`.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p dexo-app mcp:: && cargo test -p dexo-storage mcp && cargo test -p dexo --test threat`
Expected: all pass, including the 5 new selector tests.

- [ ] **Step 7: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-app/src/mcp crates/dexo-storage/src/mcp_profile.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-cli/src/run.rs
git commit -m "fix(mcp): match selectors against the object's real path"
```

---

### Task 3: Read path — connection-aware, read-only, cancellable (SEC-010, SEC-011, MCP-002, MCP-006, MCP-013, MCP-014)

**Files:**
- Create: `crates/dexo-app/src/mcp/connection.rs`
- Modify: `crates/dexo-app/src/mcp/mod.rs`, `crates/dexo-app/src/mcp/service.rs`
- Modify: `crates/dexo-app/src/data/copy.rs`, `crates/dexo-app/src/data/mod.rs`
- Modify: `crates/dexo-app/Cargo.toml`
- Create: `crates/dexo-test-support/src/session.rs`; modify `crates/dexo-test-support/src/lib.rs`, `crates/dexo-test-support/Cargo.toml`
- Modify: `crates/dexo-mcp/src/server.rs`, `crates/dexo-mcp/src/tools_read.rs`, `crates/dexo-mcp/src/stdio.rs`, `crates/dexo-mcp/src/lib.rs`, `crates/dexo-mcp/tests/protocol.rs`
- Modify: `crates/dexo-cli/src/run.rs:1354-1393` (`mcp_serve`)
- Rewrite: `crates/dexo/tests/mcp_read.rs`

**Interfaces:**
- Produces: `dexo_app::mcp::McpConnection { pub name: String, pub driver: String, pub dialect: dexo_sql::Dialect, pub database: Option<String>, pub default_schema: Option<String>, pub environment: Environment, pub read_only: bool }`, `McpConnection::from_profile(&ConnectionProfile) -> Result<Self, AppError>`, `McpConnection::qualify(&self, path: &[String]) -> ObjectRef`, `McpConnection::qualified_name(&self, reference: &ObjectRef) -> QualifiedName`.
- Produces: `dexo_app::mcp::ReadResult { pub columns: Vec<String>, pub rows: Vec<Vec<String>>, pub truncated: bool, pub bytes: u64 }` (`Serialize`).
- Produces: `McpService::policy(&self) -> ObjectPolicy`, `McpService::validate_sql(&self, &McpConnection, &str) -> Result<(), AppError>`, `McpService::authorize_relations(&self, &McpConnection, &[Vec<String>]) -> Result<Vec<ObjectRef>, AppError>`, `McpService::execute_read(&self, &dyn Session, &McpConnection, &str, &CancellationToken) -> Result<ReadResult, AppError>`, `McpService::explain(&self, &dyn Session, &McpConnection, &str) -> Result<ExplainPlan, AppError>`.
- Produces: `dexo_app::data::display_value(&DbValue) -> String`.
- Produces: `dexo_test_support::FakeSession` with `with_rows(&[&str], Vec<Vec<DbValue>>)`, `hanging()`, `with_keys(Vec<ColumnKeyInfo>)`, `log() -> Vec<String>`. Log entries: `"begin ReadOnly"`, `"execute <sql>"`, `"cancel"`, `"rollback"`, `"commit"`, `"fetch <table> <offset>+<limit>"`, `"apply <n>"`, `"columns <table>"`, `"ddl <sql>"`.
- Changes: `DexoMcpServer::with_session(self, McpConnection, Arc<dyn Session>)`; `serve_with_ledger(service, Option<(McpConnection, Arc<dyn Session>)>, Option<Arc<dyn GrantLedger>>)`; `tools_read::call_tool(service, Option<&McpConnection>, name, arguments)`.
- Removes: `serve_profile`, `serve_with_session`.

- [ ] **Step 1: Dependencies**

`crates/dexo-app/Cargo.toml`: under `[dependencies]` add `tokio-util.workspace = true`; under `[dev-dependencies]` add `dexo-test-support = { path = "../dexo-test-support" }`.

`crates/dexo-test-support/Cargo.toml`: under `[dependencies]` add

```toml
async-trait.workspace = true
dexo-driver-api = { path = "../dexo-driver-api" }
futures-util = "0.3"
```

- [ ] **Step 2: `FakeSession`**

Create `crates/dexo-test-support/src/session.rs`:

```rust
use std::sync::{Arc, Mutex};

use dexo_driver_api::{
    CapabilityState, ColumnKeyInfo, ColumnMeta, DataMutator, DataPage, DataRequest, DbValue,
    DdlExecutor, DdlOutcome, DdlPlan, DriverError, Mutation, QualifiedName, QueryEvent, QueryId,
    QueryRequest, QueryStream, RemoteValueRef, RowBatch, SchemaChange, Session,
    TransactionControl, TransactionMode, TransactionState,
};

/// A session that answers from memory and writes down every call, so a test can assert
/// what reached the "server" and in which order.
#[derive(Clone, Default)]
pub struct FakeSession {
    log: Arc<Mutex<Vec<String>>>,
    columns: Vec<String>,
    rows: Vec<Vec<DbValue>>,
    keys: Vec<ColumnKeyInfo>,
    hang: bool,
}

impl FakeSession {
    pub fn with_rows(columns: &[&str], rows: Vec<Vec<DbValue>>) -> Self {
        Self {
            columns: columns.iter().map(|name| name.to_string()).collect(),
            rows,
            ..Self::default()
        }
    }

    /// `execute` returns a stream that never yields, for cancellation tests.
    pub fn hanging() -> Self {
        Self {
            hang: true,
            ..Self::default()
        }
    }

    pub fn with_keys(mut self, keys: Vec<ColumnKeyInfo>) -> Self {
        self.keys = keys;
        self
    }

    pub fn log(&self) -> Vec<String> {
        self.log.lock().expect("fake session log").clone()
    }

    fn record(&self, entry: impl Into<String>) {
        self.log.lock().expect("fake session log").push(entry.into());
    }

    fn column_meta(&self) -> Vec<ColumnMeta> {
        self.columns
            .iter()
            .map(|name| ColumnMeta {
                name: name.clone(),
                type_name: "text".into(),
                nullable: true,
            })
            .collect()
    }
}

#[async_trait::async_trait]
impl Session for FakeSession {
    fn capabilities(&self) -> &[CapabilityState] {
        &[]
    }

    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError> {
        self.record(format!("execute {}", request.sql));
        if self.hang {
            return Ok(Box::pin(futures_util::stream::pending::<
                Result<QueryEvent, DriverError>,
            >()));
        }
        let limit = if request.row_limit == 0 {
            usize::MAX
        } else {
            request.row_limit as usize
        };
        let events = vec![
            Ok(QueryEvent::Columns(self.column_meta())),
            Ok(QueryEvent::Rows(RowBatch {
                rows: self.rows.iter().take(limit).cloned().collect(),
            })),
            Ok(QueryEvent::Finished {
                rows_affected: Some(self.rows.len() as u64),
            }),
        ];
        Ok(Box::pin(futures_util::stream::iter(events)))
    }

    async fn cancel(&self, _query: QueryId) -> Result<(), DriverError> {
        self.record("cancel");
        Ok(())
    }

    async fn close(self: Box<Self>) -> Result<(), DriverError> {
        Ok(())
    }

    fn transactions(&self) -> Option<&dyn TransactionControl> {
        Some(self)
    }

    fn data(&self) -> Option<&dyn DataMutator> {
        Some(self)
    }

    fn ddl(&self) -> Option<&dyn DdlExecutor> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl TransactionControl for FakeSession {
    async fn begin(&self, mode: TransactionMode) -> Result<(), DriverError> {
        self.record(format!("begin {mode:?}"));
        Ok(())
    }

    async fn commit(&self) -> Result<(), DriverError> {
        self.record("commit");
        Ok(())
    }

    async fn rollback(&self) -> Result<(), DriverError> {
        self.record("rollback");
        Ok(())
    }

    async fn savepoint(&self, name: &str) -> Result<(), DriverError> {
        self.record(format!("savepoint {name}"));
        Ok(())
    }

    async fn rollback_to(&self, name: &str) -> Result<(), DriverError> {
        self.record(format!("rollback_to {name}"));
        Ok(())
    }

    async fn release_savepoint(&self, name: &str) -> Result<(), DriverError> {
        self.record(format!("release {name}"));
        Ok(())
    }

    fn state(&self) -> TransactionState {
        TransactionState::Idle
    }
}

#[async_trait::async_trait]
impl DataMutator for FakeSession {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError> {
        self.record(format!(
            "fetch {} {}+{}",
            request.object.display_unquoted(),
            request.page.offset,
            request.page.limit
        ));
        let rows = self
            .rows
            .iter()
            .skip(request.page.offset as usize)
            .take(request.page.limit as usize + 1)
            .cloned()
            .collect();
        Ok(DataPage::from_fetched(
            self.column_meta(),
            rows,
            request.page.offset,
            request.page.limit,
        ))
    }

    async fn fetch_value(
        &self,
        _value: &RemoteValueRef,
        _offset: u64,
        _limit: u32,
    ) -> Result<Vec<u8>, DriverError> {
        Ok(Vec::new())
    }

    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError> {
        self.record(format!("apply {}", mutations.len()));
        Ok(())
    }

    async fn table_columns(
        &self,
        target: &QualifiedName,
    ) -> Result<Vec<ColumnKeyInfo>, DriverError> {
        self.record(format!("columns {}", target.display_unquoted()));
        Ok(self.keys.clone())
    }
}

#[async_trait::async_trait]
impl DdlExecutor for FakeSession {
    fn plan_change(&self, _change: &SchemaChange) -> Result<DdlPlan, DriverError> {
        Ok(DdlPlan::default())
    }

    async fn apply_ddl(&self, plan: &DdlPlan) -> Result<DdlOutcome, DriverError> {
        for statement in &plan.statements {
            self.record(format!("ddl {}", statement.sql));
        }
        Ok(DdlOutcome::Committed)
    }
}
```

In `crates/dexo-test-support/src/lib.rs` add `mod session;` and `pub use session::FakeSession;`.

- [ ] **Step 3: `McpConnection`**

Create `crates/dexo-app/src/mcp/connection.rs`:

```rust
use dexo_driver_api::QualifiedName;
use dexo_sql::Dialect;

use crate::connection_policy::{ConnectionPolicy, Environment};
use crate::connection_profile::ConnectionProfile;
use crate::error::{AppError, ErrorCategory};
use crate::mcp::selector::ObjectRef;

/// What MCP needs to know about one saved connection a profile may use: how to parse its
/// SQL, how to complete a partial name the way the server would, and whether it may be
/// written to at all.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpConnection {
    pub name: String,
    pub driver: String,
    pub dialect: Dialect,
    pub database: Option<String>,
    pub default_schema: Option<String>,
    pub environment: Environment,
    pub read_only: bool,
}

impl McpConnection {
    pub fn from_profile(profile: &ConnectionProfile) -> Result<Self, AppError> {
        let dialect = match profile.driver.as_str() {
            "postgres" => Dialect::Postgres,
            "mysql" => Dialect::Mysql,
            other => {
                return Err(AppError::new(
                    ErrorCategory::Capability,
                    format!("MCP does not support the {other} driver"),
                ));
            }
        };
        let policy = ConnectionPolicy::resolve(&profile.environment, &profile.policy)?;
        let database = ["database", "dbname"]
            .iter()
            .find_map(|key| profile.config.get(*key))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        Ok(Self {
            name: profile.name.clone(),
            driver: profile.driver.clone(),
            dialect,
            database,
            default_schema: (dialect == Dialect::Postgres).then(|| "public".to_string()),
            environment: mcp_environment(&profile.environment),
            read_only: policy.read_only,
        })
    }

    /// Completes a name the way the server resolves it: a bare Postgres table is
    /// `database.public.table`, a bare MySQL table is `database.table`. Without a known
    /// database the name is left as typed, which matches no three-part selector and is
    /// therefore denied.
    pub fn qualify(&self, path: &[String]) -> ObjectRef {
        let Some(database) = &self.database else {
            return ObjectRef {
                path: path.to_vec(),
            };
        };
        let defaults: Vec<String> = std::iter::once(database.clone())
            .chain(self.default_schema.clone())
            .collect();
        let missing = defaults.len().saturating_sub(path.len().saturating_sub(1));
        ObjectRef {
            path: defaults[..missing]
                .iter()
                .cloned()
                .chain(path.iter().cloned())
                .collect(),
        }
    }

    pub fn qualified_name(&self, reference: &ObjectRef) -> QualifiedName {
        let path = &reference.path;
        match (self.dialect, path.as_slice()) {
            (Dialect::Postgres, [catalog, schema, object]) => {
                QualifiedName::new(Some(catalog), Some(schema), object)
            }
            (Dialect::Postgres, [schema, object]) => {
                QualifiedName::new(None::<String>, Some(schema), object)
            }
            (Dialect::Mysql, [catalog, object]) => {
                QualifiedName::new(Some(catalog), None::<String>, object)
            }
            (_, [.., object]) => QualifiedName::new(None::<String>, None::<String>, object),
            (_, []) => QualifiedName::new(None::<String>, None::<String>, ""),
        }
    }
}

/// `Environment::parse` maps any unknown label to `Local`; MCP must fail closed instead,
/// so `prod`, `PRD` or `live` count as production.
fn mcp_environment(label: &str) -> Environment {
    match Environment::known(label) {
        Some(environment) => environment,
        None if label.is_empty() || label.eq_ignore_ascii_case("local") => Environment::Local,
        None => Environment::Production,
    }
}

#[cfg(test)]
mod tests {
    use super::{McpConnection, mcp_environment};
    use crate::connection_policy::Environment;
    use dexo_sql::Dialect;

    fn postgres() -> McpConnection {
        McpConnection {
            name: "local".into(),
            driver: "postgres".into(),
            dialect: Dialect::Postgres,
            database: Some("db".into()),
            default_schema: Some("public".into()),
            environment: Environment::Local,
            read_only: false,
        }
    }

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| part.to_string()).collect()
    }

    #[test]
    fn bare_names_complete_to_the_server_default() {
        let pg = postgres();
        assert_eq!(pg.qualify(&path(&["t"])).path, ["db", "public", "t"]);
        assert_eq!(pg.qualify(&path(&["s", "t"])).path, ["db", "s", "t"]);
        assert_eq!(pg.qualify(&path(&["x", "s", "t"])).path, ["x", "s", "t"]);
        let mysql = McpConnection {
            dialect: Dialect::Mysql,
            default_schema: None,
            ..postgres()
        };
        assert_eq!(mysql.qualify(&path(&["t"])).path, ["db", "t"]);
        assert_eq!(mysql.qualify(&path(&["shop", "t"])).path, ["shop", "t"]);
    }

    #[test]
    fn unknown_environment_labels_fail_closed() {
        assert_eq!(mcp_environment("prod"), Environment::Production);
        assert_eq!(mcp_environment("PRD"), Environment::Production);
        assert_eq!(mcp_environment("local"), Environment::Local);
        assert_eq!(mcp_environment("staging"), Environment::Staging);
    }
}
```

In `crates/dexo-app/src/mcp/mod.rs` add `pub mod connection;` and `pub use connection::McpConnection;`, and extend the service re-export to `pub use service::{McpService, ReadResult, advertised_tools, new_result_uri};`.

- [ ] **Step 4: Export the value renderer**

In `crates/dexo-app/src/data/copy.rs` rename `fn display(value: &DbValue) -> String` to `pub fn display_value(value: &DbValue) -> String` and update its three call sites in the same file (`Ok(display(value))`, `other => display(other)`, `display(&DbValue::Bytes(v.clone()))`). In `crates/dexo-app/src/data/mod.rs` change the copy re-export to `pub use copy::{CopyFormat, SqlDialect, copy_selection, display_value};`.

- [ ] **Step 5: Authorize against real relations and read inside a read-only transaction**

In `crates/dexo-app/src/mcp/service.rs`:

Replace the imports at the top with:

```rust
use std::time::Duration;

use dexo_driver_api::{
    CatalogObject, ExplainPlan, ExplainRequest, QueryEvent, QueryRequest, Session,
    TransactionMode,
};
use dexo_sql::{GuardRejection, inspect_read};
use futures_util::StreamExt;
use serde::Serialize;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::data::display_value;
use crate::error::{AppError, ErrorCategory};
use crate::mcp::connection::McpConnection;
use crate::mcp::policy::{Decision, ObjectPolicy};
use crate::mcp::profile::{McpProfile, QueryMode};
use crate::mcp::selector::ObjectRef;
use crate::query_service::map_driver_error;
use crate::search_service::SearchService;
```

Add, after `const HIDDEN`:

```rust
/// Rows as the user would see them in the grid, with the limits that cut them short.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ReadResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
    pub bytes: u64,
}
```

Replace `validate_sql`, `authorize_read_sql`, `execute_read` and `explain` with the block below. Leave `authorize_write_sql` as it is; Task 5 replaces it.

```rust
    pub fn policy(&self) -> ObjectPolicy {
        ObjectPolicy::new(self.profile.selectors.clone())
    }

    pub fn validate_sql(&self, connection: &McpConnection, sql: &str) -> Result<(), AppError> {
        self.authorize_read_sql(connection, sql)
    }

    pub fn authorize_read_sql(&self, connection: &McpConnection, sql: &str) -> Result<(), AppError> {
        if self.profile.query_mode != QueryMode::RawReadSql {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "raw SQL is not enabled for this profile",
            ));
        }
        let inspection = inspect_read(sql, connection.dialect).map_err(guard_error)?;
        self.authorize_relations(connection, &inspection.relations)
            .map(|_| ())
    }

    /// Every relation a statement names must be allowed after completing it the way the
    /// server would; one denied or unknown name hides the whole statement.
    pub fn authorize_relations(
        &self,
        connection: &McpConnection,
        relations: &[Vec<String>],
    ) -> Result<Vec<ObjectRef>, AppError> {
        let policy = self.policy();
        let targets: Vec<ObjectRef> = relations
            .iter()
            .map(|path| connection.qualify(path))
            .collect();
        if targets
            .iter()
            .all(|target| policy.decide(target) == Decision::Allow)
        {
            Ok(targets)
        } else {
            Err(hidden())
        }
    }

    /// Runs one authorized read inside `BEGIN READ ONLY … ROLLBACK`. The parse above keeps
    /// out what it can recognise; the transaction is what stops a function that writes.
    pub async fn execute_read(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        sql: &str,
        cancel: &CancellationToken,
    ) -> Result<ReadResult, AppError> {
        self.authorize_read_sql(connection, sql)?;
        let transactions = session.transactions().ok_or_else(|| {
            AppError::new(
                ErrorCategory::Capability,
                "this connection cannot open a read-only transaction",
            )
        })?;
        transactions
            .begin(TransactionMode::ReadOnly)
            .await
            .map_err(map_driver_error)?;
        let outcome = self.collect_rows(session, sql, cancel).await;
        let rolled_back = transactions.rollback().await.map_err(map_driver_error);
        let result = outcome?;
        rolled_back?;
        Ok(result)
    }

    async fn collect_rows(
        &self,
        session: &dyn Session,
        sql: &str,
        cancel: &CancellationToken,
    ) -> Result<ReadResult, AppError> {
        let limits = &self.profile.limits;
        let mut request = QueryRequest::read(sql, limits.max_rows.saturating_add(1));
        request.timeout = Duration::from_secs(limits.timeout_secs);
        let query = request.id;
        let mut stream = session.execute(request).await.map_err(map_driver_error)?;
        let mut result = ReadResult::default();
        while !result.truncated {
            let event = tokio::select! {
                () = cancel.cancelled() => {
                    let _ = session.cancel(query).await;
                    return Err(AppError::new(ErrorCategory::Cancelled, "cancelled by the client"));
                }
                event = stream.next() => event,
            };
            let Some(event) = event else {
                break;
            };
            match event.map_err(map_driver_error)? {
                QueryEvent::Columns(columns) if result.columns.is_empty() => {
                    result.columns = columns.into_iter().map(|column| column.name).collect();
                }
                QueryEvent::Rows(batch) => {
                    for row in batch.rows {
                        let cells: Vec<String> = row.iter().map(display_value).collect();
                        let size: u64 = cells.iter().map(|cell| cell.len() as u64).sum();
                        if result.rows.len() as u64 >= limits.max_rows
                            || result.bytes + size > limits.max_bytes
                        {
                            result.truncated = true;
                            break;
                        }
                        result.bytes += size;
                        result.rows.push(cells);
                    }
                }
                _ => {}
            }
        }
        Ok(result)
    }

    pub async fn explain(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        sql: &str,
    ) -> Result<ExplainPlan, AppError> {
        self.authorize_read_sql(connection, sql)?;
        session
            .explain()
            .ok_or_else(|| AppError::new(ErrorCategory::Capability, "explain is unavailable"))?
            .explain(ExplainRequest::estimated(sql))
            .await
            .map_err(map_driver_error)
    }
```

Delete `fn referenced_name` and the dead `for object in &self.objects { let _ = object; }` loop. Add below `fn hidden()`:

```rust
fn guard_error(rejection: GuardRejection) -> AppError {
    AppError::new(ErrorCategory::McpPolicy, rejection.to_string())
}
```

Replace the `tests` module's second test and add the transaction tests:

```rust
    fn raw_service(max_rows: u64, max_bytes: u64) -> McpService {
        let mut profile = McpProfile::new("assistant");
        profile.query_mode = QueryMode::RawReadSql;
        profile.limits.max_rows = max_rows;
        profile.limits.max_bytes = max_bytes;
        profile.selectors = vec![
            SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
            SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
        ];
        McpService::new(profile, Vec::new())
    }

    fn pg() -> McpConnection {
        McpConnection {
            name: "local".into(),
            driver: "postgres".into(),
            dialect: dexo_sql::Dialect::Postgres,
            database: Some("db".into()),
            default_schema: Some("public".into()),
            environment: crate::connection_policy::Environment::Local,
            read_only: false,
        }
    }

    #[test]
    fn denied_tables_cannot_ride_along_in_a_join() {
        let service = raw_service(10, 1024);
        assert!(service.validate_sql(&pg(), "select * from users").is_ok());
        for sql in [
            "select * from users u join secrets s on true",
            "select * from users, secrets",
            "select * from\nsecrets",
            "select (select 1 from db.public.secrets) from users",
            "WITH x AS (SELECT 1) DELETE FROM users",
            "select * into leaked from users",
        ] {
            assert!(service.validate_sql(&pg(), sql).is_err(), "{sql}");
        }
    }

    #[tokio::test]
    async fn reads_run_in_a_read_only_transaction_that_is_rolled_back() {
        let session = FakeSession::with_rows(&["id"], vec![vec![DbValue::I64(1)]]);
        let result = raw_service(10, 1024)
            .execute_read(&session, &pg(), "select id from users", &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.columns, ["id"]);
        assert_eq!(result.rows, vec![vec!["1".to_string()]]);
        assert_eq!(
            session.log(),
            ["begin ReadOnly", "execute select id from users", "rollback"]
        );
    }

    #[tokio::test]
    async fn rows_past_the_limits_are_cut_and_flagged() {
        let rows = vec![
            vec![DbValue::Text("ab".into())],
            vec![DbValue::Text("cd".into())],
            vec![DbValue::Text("ef".into())],
        ];
        let by_rows = raw_service(2, 1024)
            .execute_read(
                &FakeSession::with_rows(&["v"], rows.clone()),
                &pg(),
                "select v from users",
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(by_rows.rows.len(), 2);
        assert!(by_rows.truncated);
        let by_bytes = raw_service(10, 3)
            .execute_read(
                &FakeSession::with_rows(&["v"], rows),
                &pg(),
                "select v from users",
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(by_bytes.rows.len(), 1);
        assert!(by_bytes.truncated);
    }

    #[tokio::test]
    async fn a_cancelled_read_cancels_the_query_and_rolls_back() {
        let session = FakeSession::hanging();
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            trigger.cancel();
        });
        let error = raw_service(10, 1024)
            .execute_read(&session, &pg(), "select id from users", &cancel)
            .await
            .unwrap_err();
        assert_eq!(error.category(), ErrorCategory::Cancelled);
        assert_eq!(
            session.log(),
            ["begin ReadOnly", "execute select id from users", "cancel", "rollback"]
        );
    }
```

and extend the test module's `use` block with:

```rust
    use super::McpConnection;
    use crate::error::ErrorCategory;
    use dexo_driver_api::DbValue;
    use dexo_test_support::FakeSession;
    use tokio_util::sync::CancellationToken;
```

Delete the old `mutating_and_unknown_sql_are_rejected` test (its hedged `||` assertions are replaced by `denied_tables_cannot_ride_along_in_a_join`).

- [ ] **Step 6: Adapter glue — stop inventing results and stop the global cancel**

In `crates/dexo-mcp/src/server.rs`:
- Replace the field `session: Option<Arc<dyn Session>>` with `target: Option<(McpConnection, Arc<dyn Session>)>` and add `session_lock: Arc<tokio::sync::Mutex<()>>` (initialized with `Arc::new(tokio::sync::Mutex::new(()))`). Rename the field `cancel` to `stop`; it now only stops the `list_changed` poller. Update `Drop` to call `self.stop.cancel()`, point the poller in `on_initialized` at `self.stop`, and delete `cancelled()` (it has no callers).
- Replace `with_session` with:

```rust
    pub fn with_session(mut self, connection: McpConnection, session: Arc<dyn Session>) -> Self {
        self.target = Some((connection, session));
        self
    }
```

- In `call_tool`: delete the `if self.cancel.is_cancelled()` block; pass `context.ct.clone()` to `execute_read_tool`; replace `self.session.as_deref()` in the write branch with `self.target.as_ref().map(|(connection, session)| (connection, session.as_ref()))` (this becomes the `target` argument of `call_write_tool` in Task 4 — until then keep passing `self.target.as_ref().map(|(_, session)| session.as_ref())`); change the fallback to `tools_read::call_tool(&self.service, self.target.as_ref().map(|(connection, _)| connection), &request.name, arguments)`.
- Delete `on_cancelled` entirely. rmcp already cancels `context.ct` for the one request a `notifications/cancelled` names.
- Replace `execute_read_tool` and `explain_tool` with:

```rust
async fn execute_read_tool(
    server: &DexoMcpServer,
    arguments: serde_json::Map<String, serde_json::Value>,
    cancel: CancellationToken,
) -> Result<CallToolResponse, McpError> {
    let sql = arguments
        .get("sql")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let Some((connection, session)) = &server.target else {
        return Ok(no_connection().into());
    };
    let _serialized = server.session_lock.lock().await;
    match server
        .service
        .execute_read(session.as_ref(), connection, sql, &cancel)
        .await
    {
        Ok(result) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string(&result).unwrap_or_default(),
        )])
        .into()),
        Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into()),
    }
}

async fn explain_tool(
    server: &DexoMcpServer,
    arguments: serde_json::Map<String, serde_json::Value>,
) -> Result<CallToolResponse, McpError> {
    let sql = arguments
        .get("sql")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let Some((connection, session)) = &server.target else {
        return Ok(no_connection().into());
    };
    let _serialized = server.session_lock.lock().await;
    match server.service.explain(session.as_ref(), connection, sql).await {
        Ok(plan) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string(&plan).unwrap_or_else(|_| hidden_error().into()),
        )])
        .into()),
        Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(error.to_string())]).into()),
    }
}

fn no_connection() -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(
        "no connection is configured for this profile",
    )])
}
```

Also, at the top of `call_tool`, refuse tools the profile does not advertise before any dispatch, so a denied `tool_rules` entry is honoured for the special-cased reads too:

```rust
        let is_write = WRITE_TOOLS.iter().any(|name| *name == request.name.as_ref());
        if !is_write && !advertised_tools(&self.service.profile).contains(&request.name.as_ref()) {
            return Ok(CallToolResult::error(vec![ContentBlock::text(hidden_error())]).into());
        }
```

(add `use dexo_app::mcp::advertised_tools;` and `use dexo_app::mcp::McpConnection;`).

In `crates/dexo-mcp/src/tools_read.rs`, change `call_tool` to take `connection: Option<&McpConnection>`; make `query_validate` use it, return `CallToolResult::error` for every failure, and delete the fabricated branches:

```rust
pub fn call_tool(
    service: &McpService,
    connection: Option<&McpConnection>,
    name: &str,
    arguments: Map<String, Value>,
) -> CallToolResult {
    if !advertised_tools(&service.profile).contains(&name) {
        return CallToolResult::error(vec![ContentBlock::text(crate::error::hidden_error())]);
    }
    let value = Value::Object(arguments);
    let outcome: Result<String, String> = match name {
        "catalog_search" => serde_json::from_value::<SearchInput>(value)
            .map_err(|error| error.to_string())
            .map(|input| serde_json::to_string(&service.search(&input.query)).unwrap_or_default()),
        "object_describe" => serde_json::from_value::<ObjectInput>(value)
            .map_err(|error| error.to_string())
            .and_then(|input| service.describe(&input.id).map_err(|error| error.to_string()))
            .map(|object| serde_json::to_string(&object).unwrap_or_default()),
        "object_get_ddl" => serde_json::from_value::<ObjectInput>(value)
            .map_err(|error| error.to_string())
            .and_then(|input| service.ddl(&input.id).map_err(|error| error.to_string())),
        "object_relationships" => serde_json::from_value::<ObjectInput>(value)
            .map_err(|error| error.to_string())
            .and_then(|input| service.relationships(&input.id).map_err(|error| error.to_string()))
            .map(|items| serde_json::to_string(&items).unwrap_or_default()),
        "query_validate" => match connection {
            None => Err("no connection is configured for this profile".into()),
            Some(connection) => serde_json::from_value::<QueryInput>(value)
                .map_err(|error| error.to_string())
                .and_then(|input| {
                    service
                        .validate_sql(connection, &input.sql)
                        .map(|()| "ok".to_string())
                        .map_err(|error| error.to_string())
                }),
        },
        _ => Err(crate::error::hidden_error().into()),
    };
    match outcome {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(text) => CallToolResult::error(vec![ContentBlock::text(text)]),
    }
}
```

Change its `dexo_app::mcp` import to `use dexo_app::mcp::{McpConnection, McpService, advertised_tools};` and remove the now-unused `ExplainInput` / `DiffInput` imports. In `crates/dexo-app/src/mcp/service.rs::advertised_tools`, remove `"schema_diff"` from the list (it only ever returned `"diff a -> b"`; Task 10 brings back a real one).

In `crates/dexo-mcp/src/stdio.rs`: delete `serve_profile` and `serve_with_session`; change `serve_with_ledger`'s second parameter to `target: Option<(McpConnection, std::sync::Arc<dyn dexo_driver_api::Session>)>` and use `if let Some((connection, session)) = target { server = server.with_session(connection, session); }`. In `crates/dexo-mcp/src/lib.rs` drop `serve_profile` and `serve_with_session` from the `pub use stdio::…` list.

In `crates/dexo-mcp/tests/protocol.rs`, `mutating_sql_is_rejected_before_data`: pass a connection as the new second argument of `call_tool` and assert on `is_error`:

```rust
#[test]
fn mutating_sql_is_rejected_before_data() {
    let service = service();
    let connection = dexo_app::mcp::McpConnection {
        name: "local".into(),
        driver: "postgres".into(),
        dialect: dexo_sql::Dialect::Postgres,
        database: Some("db".into()),
        default_schema: Some("public".into()),
        environment: dexo_app::Environment::Local,
        read_only: false,
    };
    for sql in ["DELETE FROM users", "SELECT 1 FROM secrets"] {
        let result = call_tool(
            &service,
            Some(&connection),
            "query_validate",
            json!({ "sql": sql }).as_object().cloned().unwrap(),
        );
        assert_eq!(result.is_error, Some(true), "{sql}");
    }
}
```

and add `dexo-sql = { path = "../dexo-sql" }` to `[dev-dependencies]` of `crates/dexo-mcp/Cargo.toml`.

- [ ] **Step 7: `mcp serve` — the profile's own catalog, a loud connect**

In `crates/dexo-cli/src/run.rs`, replace the body of `mcp_serve` after `if !profile.enabled { … }` with:

```rust
    let target = match profile.connections.first() {
        None => None,
        Some(name) => {
            let saved = ConnectionRepository::new(db.connection())
                .get_by_name(name)?
                .ok_or_else(|| anyhow::anyhow!("profile names unknown connection '{name}'"))?;
            Some(saved)
        }
    };
    let objects = match &target {
        Some(saved) => CatalogCache::new(db.connection())
            .load_latest(&saved.id.0.to_string(), &catalog_database_name(saved))?,
        None => Vec::new(),
    };
    let service = McpService::new(profile, objects);
    let target = match target {
        Some(saved) => {
            let connection = dexo_app::mcp::McpConnection::from_profile(&saved)?;
            let session = connect_session(&registry, &saved).await?;
            Some((connection, std::sync::Arc::from(session)))
        }
        None => None,
    };
    dexo_mcp::serve_with_ledger(
        service,
        target,
        Some(std::sync::Arc::new(SqliteGrantLedger::open(&paths.database)?)),
    )
    .await
```

and add, next to `refresh_catalog`, the helper that `run_inspect` already inlines. Replace the inline block at `run.rs:516-521` with `let database_name = catalog_database_name(&profile);` and pass `&database_name` where the old `&str` was used:

```rust
pub(crate) fn catalog_database_name(profile: &dexo_app::ConnectionProfile) -> String {
    profile
        .config
        .get("database")
        .or_else(|| profile.config.get("dbname"))
        .and_then(|value| value.as_str())
        .unwrap_or("default")
        .to_string()
}
```

This drops `load_latest_any` (another connection's catalog) and the `.ok()` that turned a failed connect into a server that silently answered nothing.

- [ ] **Step 8: Live test against real servers**

Replace `crates/dexo/tests/mcp_read.rs` with:

```rust
use dexo_app::Environment;
use dexo_app::mcp::{Effect, McpConnection, McpProfile, McpService, QueryMode, SelectorRule};
use dexo_driver_api::{ConnectRequest, ConnectionFactory, QueryRequest, Session};
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_sql::Dialect;
use dexo_test_support::DatabasePair;
use futures_util::StreamExt;
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;

fn read_profile() -> McpProfile {
    let mut profile = McpProfile::new("reader");
    profile.query_mode = QueryMode::RawReadSql;
    profile.selectors = vec![
        SelectorRule::parse(Effect::Allow, "dexo.public.items").unwrap(),
        SelectorRule::parse(Effect::Allow, "dexo.items").unwrap(),
    ];
    profile
}

fn connection(dialect: Dialect) -> McpConnection {
    McpConnection {
        name: "local".into(),
        driver: dialect.name().into(),
        dialect,
        database: Some("dexo".into()),
        default_schema: (dialect == Dialect::Postgres).then(|| "public".to_string()),
        environment: Environment::Local,
        read_only: false,
    }
}

async fn run(session: &dyn Session, sql: &str) {
    let mut stream = session.execute(QueryRequest::write(sql)).await.unwrap();
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
}

async fn connect(factory: &dyn ConnectionFactory, endpoint: &str) -> Box<dyn Session> {
    factory
        .connect(ConnectRequest::new(
            endpoint.to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mcp_reads_cannot_write_on_postgres_or_mysql() {
    let pair = DatabasePair::start().await.unwrap();
    let pg = connect(&PostgresFactory, pair.postgres_endpoint()).await;
    run(&*pg, "CREATE TABLE IF NOT EXISTS items (id int primary key)").await;
    run(
        &*pg,
        "CREATE OR REPLACE FUNCTION sneaky() RETURNS int LANGUAGE sql AS $$ INSERT INTO items VALUES (99) RETURNING 1 $$",
    )
    .await;
    let mysql = connect(&MysqlFactory, pair.mysql_endpoint()).await;
    run(&*mysql, "CREATE TABLE IF NOT EXISTS items (id int primary key)").await;

    let service = McpService::new(read_profile(), Vec::new());
    let cancel = CancellationToken::new();
    for (session, dialect) in [(&*pg, Dialect::Postgres), (&*mysql, Dialect::Mysql)] {
        let connection = connection(dialect);
        assert!(
            service
                .execute_read(session, &connection, "select count(*) from items", &cancel)
                .await
                .is_ok()
        );
        assert!(
            service
                .execute_read(session, &connection, "select * from items for update", &cancel)
                .await
                .is_err()
        );
        assert!(
            service
                .execute_read(session, &connection, "select 1 from secrets", &cancel)
                .await
                .is_err()
        );
    }
    let written = service
        .execute_read(&*pg, &connection(Dialect::Postgres), "select sneaky() from items", &cancel)
        .await
        .unwrap_err();
    assert!(written.to_string().contains("read-only"), "{written}");
}
```

Add `tokio-util.workspace = true` and `dexo-sql = { path = "../dexo-sql" }` to `[dev-dependencies]` of `crates/dexo/Cargo.toml` if they are missing.

- [ ] **Step 9: Run**

Run: `cargo test -p dexo-app mcp:: && cargo test -p dexo-mcp && cargo test -p dexo --test mcp_read -- --include-ignored` (the last one needs Docker; skip it if Docker is not running and say so in the task report).
Expected: all pass; the live test shows Postgres refusing `sneaky()` with "cannot execute INSERT in a read-only transaction".

- [ ] **Step 10: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo deny check
git add -A crates/dexo-app crates/dexo-test-support crates/dexo-mcp crates/dexo-cli/src/run.rs crates/dexo/tests/mcp_read.rs crates/dexo/Cargo.toml Cargo.lock
git commit -m "fix(mcp): read inside a read-only transaction and cancel only the request asked"
```

---

### Task 4: Grants bound to the profile and to their connection (MCP-004, MCP-003, MCP-018)

**Files:**
- Modify: `crates/dexo-app/src/mcp/grant.rs:152-159` (`authorizes`) and its tests
- Modify: `crates/dexo-mcp/src/tools_write.rs` (signature, grant lookup, tests)
- Modify: `crates/dexo-mcp/src/server.rs` (write branch)
- Rewrite: `crates/dexo/tests/mcp_write.rs`

**Interfaces:**
- Changes: `Grant::authorizes(&self, tool: &str, connection: &str, target: &ObjectRef, profile: &ObjectPolicy, now: i64) -> bool`. Admin grants are connection-wide: selectors are not consulted for `GrantCapability::Admin`.
- Changes: `tools_write::call_write_tool(service, ledger, target: Option<(&McpConnection, &dyn Session)>, session_id, name, arguments, now)`. `None` fails before any operation is reserved or any grant is spent.

- [ ] **Step 1: Intersect grant scope with the profile and the connection**

In `crates/dexo-app/src/mcp/grant.rs` replace `authorizes` with:

```rust
    /// A grant narrows the profile; it can never widen it. The profile's deny rules still
    /// apply inside the grant's scope, and a grant issued for one connection is useless
    /// on another. Admin actions target a server session, not an object, so an admin
    /// grant is bounded by its connection alone.
    pub fn authorizes(
        &self,
        tool: &str,
        connection: &str,
        target: &ObjectRef,
        profile: &ObjectPolicy,
        now: i64,
    ) -> bool {
        self.active(now)
            && self.connection == connection
            && self.tools.iter().any(|name| name == tool)
            && self.capability.allows_tool(tool)
            && (self.capability == GrantCapability::Admin
                || (ObjectPolicy::new(self.selectors.clone()).decide(target) == Decision::Allow
                    && profile.decide(target) == Decision::Allow))
    }
```

Update the calls to `authorizes` in this file's tests to pass `"local"` and `&ObjectPolicy::new(profile().selectors)` (add a local `profile()` helper if the test module lacks one).

- [ ] **Step 2: Carry the connection through the write path**

In `crates/dexo-mcp/src/tools_write.rs`:
- change the signature of `call_write_tool` to take `target: Option<(&McpConnection, &dyn Session)>` instead of `session: Option<&dyn Session>`;
- right after the `is_grant_management` block, add:

```rust
    let Some((connection, session)) = target else {
        return Err(AppError::new(
            ErrorCategory::Capability,
            "no connection is open for this profile",
        ));
    };
```

- rename the local `target` string to `target_name` and build the object with the connection's defaults: `let object = connection.qualify(&ObjectRef::parse(target_name).path);`
- replace the grant lookup with:

```rust
    let profile_policy = service.policy();
    let grants = ledger.active_grants(&service.profile.name, now);
    let grant = grants
        .iter()
        .find(|grant| grant.authorizes(name, &connection.name, &object, &profile_policy, now))
        .cloned()
```

- change `execute`, `apply_mutations`, `apply_ddl` and `apply_admin` to take `session: &dyn Session` (not `Option`) and delete every `let Some(session) = session else { return Ok((SideEffect::Unknown, "session required".into())); };` block. The fake-success path is gone: without a session nothing is reserved and nothing is spent.

In `crates/dexo-mcp/src/server.rs`, pass `self.target.as_ref().map(|(connection, session)| (connection, session.as_ref()))` as the `target` argument.

- [ ] **Step 3: Tests use a real (fake) session and pin the new scope rules**

In `crates/dexo-mcp/src/tools_write.rs` tests, add `dexo-test-support = { path = "../dexo-test-support" }` to `crates/dexo-mcp/Cargo.toml` `[dev-dependencies]`, and replace the helpers and the two tests that asserted fabricated outcomes:

```rust
    fn connection(name: &str) -> McpConnection {
        McpConnection {
            name: name.into(),
            driver: "postgres".into(),
            dialect: dexo_sql::Dialect::Postgres,
            database: Some("db".into()),
            default_schema: Some("public".into()),
            environment: dexo_app::Environment::Local,
            read_only: false,
        }
    }

    async fn call_on(
        ledger: &MemoryGrantLedger,
        session: &FakeSession,
        connection_name: &str,
        tool: &str,
        payload: serde_json::Value,
    ) -> Result<String, dexo_app::AppError> {
        let connection = connection(connection_name);
        call_write_tool(
            &service(),
            ledger,
            Some((&connection, session as &dyn dexo_driver_api::Session)),
            "s",
            tool,
            payload.as_object().cloned().unwrap(),
            0,
        )
        .await
    }

    async fn call(
        ledger: &MemoryGrantLedger,
        tool: &str,
        payload: serde_json::Value,
    ) -> Result<String, dexo_app::AppError> {
        call_on(ledger, &FakeSession::default(), "local", tool, payload).await
    }

    #[tokio::test]
    async fn same_operation_and_payload_executes_once() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(GrantCapability::DataWrite, "data_insert", "db.public.items"))
            .unwrap();
        let session = FakeSession::default().with_keys(vec![dexo_driver_api::ColumnKeyInfo {
            name: "id".into(),
            primary_key: true,
            unique: true,
        }]);
        let payload = json!({"operation_id":"op-1","target":"db.public.items","values":{"id":7}});
        let first = call_on(&ledger, &session, "local", "data_insert", payload.clone())
            .await
            .unwrap();
        let replay = call_on(&ledger, &session, "local", "data_insert", payload)
            .await
            .unwrap();
        assert_eq!(first, replay);
        assert!(first.contains("Committed"), "{first}");
        assert_eq!(
            session.log().iter().filter(|entry| entry.starts_with("apply")).count(),
            1
        );
        let conflict = call(
            &ledger,
            "data_insert",
            json!({"operation_id":"op-1","target":"db.public.items","values":{"id":8}}),
        )
        .await
        .unwrap_err();
        assert!(conflict.to_string().contains("different payload"));
    }

    #[tokio::test]
    async fn writes_without_an_open_connection_fail_before_the_grant_is_spent() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(GrantCapability::DataWrite, "data_insert", "db.public.items"))
            .unwrap();
        let error = call_write_tool(
            &service(),
            &ledger,
            None,
            "s",
            "data_insert",
            json!({"operation_id":"op-x","target":"db.public.items","values":{"id":1}})
                .as_object()
                .cloned()
                .unwrap(),
            0,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("no connection"));
        assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
    }

    #[tokio::test]
    async fn a_grant_cannot_reach_what_the_profile_denies() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(GrantCapability::DataWrite, "data_insert", "db.public.*"))
            .unwrap();
        let error = call(
            &ledger,
            "data_insert",
            json!({"operation_id":"op-s","target":"db.public.secrets","values":{"id":1}}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "not found");
    }

    #[tokio::test]
    async fn a_grant_is_bound_to_its_connection() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(GrantCapability::DataWrite, "data_insert", "db.public.items"))
            .unwrap();
        let error = call_on(
            &ledger,
            &FakeSession::default(),
            "other",
            "data_insert",
            json!({"operation_id":"op-c","target":"db.public.items","values":{"id":1}}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "not found");
    }
```

`mysql_ddl_commit_is_not_claimed_reversed` moves to Task 5 (it needs the DDL changes). Delete it here. Keep `catalog_has_no_grant_management_tools`, `data_write_cannot_ddl_and_ddl_cannot_terminate`, `revoke_before_dispatch_hides_grant`, `audit_omits_results_and_secret_sentinel` and `data_execute_sql_rejects_ddl`; they compile against the new `call()` helper unchanged. The `service()` helper keeps the deny rule `db.public.secrets` it already has.

Add to the test module's imports: `use dexo_app::mcp::McpConnection;` and `use dexo_test_support::FakeSession;`.

- [ ] **Step 4: Live write test**

In `crates/dexo/tests/mcp_write.rs`, replace `Some(session)` with `Some((&connection, session))`, where `connection` is built like `connection(dialect)` in `mcp_read.rs` (copy that function into this file), and iterate `[(&*pg, Dialect::Postgres), (&*mysql, Dialect::Mysql)]`. Keep the assertion that a `data_write` grant cannot run `schema_apply_ddl`.

- [ ] **Step 5: Run, gate, commit**

Run: `cargo test -p dexo-app grant && cargo test -p dexo-mcp`
Expected: all pass.

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-app/src/mcp/grant.rs crates/dexo-mcp crates/dexo/tests/mcp_write.rs
git commit -m "fix(mcp): grants cannot widen the profile or leave their connection"
```

---

### Task 5: Writes fail closed (MCP-008 partial, MCP-009, MCP-015, MCP-018, SEC-004, SEC-008)

**Files:**
- Modify: `crates/dexo-app/src/mcp/connection.rs` (`accepts_writes`)
- Modify: `crates/dexo-app/src/mcp/service.rs` (`data_write_targets`, `schema_write_targets`, `execute_write`; delete `authorize_write_sql`)
- Modify: `crates/dexo-mcp/src/tools_write.rs`
- Modify: `crates/dexo-mcp/Cargo.toml`

**Interfaces:**
- Produces: `McpConnection::accepts_writes(&self) -> Result<(), AppError>`.
- Produces: `McpService::data_write_targets(&self, &McpConnection, &str) -> Result<Vec<ObjectRef>, AppError>`, `McpService::schema_write_targets(&self, &McpConnection, &str) -> Result<Vec<ObjectRef>, AppError>`, `McpService::execute_write(&self, &dyn Session, &McpConnection, &str) -> Result<u64, AppError>` (rows affected).

- [ ] **Step 1: Connection-level write eligibility**

In `crates/dexo-app/src/mcp/connection.rs`, inside `impl McpConnection`:

```rust
    /// MCP never writes to production (MCP-015) or to a connection whose own policy keeps
    /// it read-only (SEC-011); a grant cannot override either.
    pub fn accepts_writes(&self) -> Result<(), AppError> {
        if self.environment == Environment::Production {
            return Err(AppError::new(
                ErrorCategory::McpPolicy,
                "writes to production connections are not available over MCP",
            ));
        }
        if self.read_only {
            return Err(AppError::new(
                ErrorCategory::Permission,
                "connection is read-only",
            ));
        }
        Ok(())
    }
```

- [ ] **Step 2: Bind SQL writes to every table they touch, and run them through the app**

In `crates/dexo-app/src/mcp/service.rs`, extend the `dexo_sql` import to `use dexo_sql::{GuardRejection, inspect_data_write, inspect_read, inspect_schema_write};`, delete `authorize_write_sql`, and add:

```rust
    pub fn data_write_targets(
        &self,
        connection: &McpConnection,
        sql: &str,
    ) -> Result<Vec<ObjectRef>, AppError> {
        let inspection = inspect_data_write(sql, connection.dialect).map_err(guard_error)?;
        self.authorize_relations(connection, &inspection.relations)
    }

    pub fn schema_write_targets(
        &self,
        connection: &McpConnection,
        sql: &str,
    ) -> Result<Vec<ObjectRef>, AppError> {
        let inspection = inspect_schema_write(sql, connection.dialect).map_err(guard_error)?;
        self.authorize_relations(connection, &inspection.relations)
    }

    /// Runs one authorized INSERT/UPDATE/DELETE to completion. The old adapter dropped
    /// the stream unread, so a constraint violation was reported as "sql applied".
    pub async fn execute_write(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        sql: &str,
    ) -> Result<u64, AppError> {
        self.data_write_targets(connection, sql)?;
        let mut request = QueryRequest::write(sql);
        request.timeout = Duration::from_secs(self.profile.limits.timeout_secs);
        let mut stream = session.execute(request).await.map_err(map_driver_error)?;
        let mut affected = 0;
        while let Some(event) = stream.next().await {
            match event.map_err(map_driver_error)? {
                QueryEvent::ResultSetFinished {
                    rows_affected: Some(rows),
                    ..
                }
                | QueryEvent::Finished {
                    rows_affected: Some(rows),
                } => affected = rows,
                _ => {}
            }
        }
        Ok(affected)
    }
```

- [ ] **Step 3: The write path refuses before it reserves**

In `crates/dexo-mcp/src/tools_write.rs`, after the `let Some((connection, session)) = target else { … };` block from Task 4, add:

```rust
    if !service.profile.tool_allowed(name) {
        return Err(AppError::new(ErrorCategory::McpPolicy, "not found"));
    }
    connection.accepts_writes()?;
    let sql = arguments.get("sql").and_then(Value::as_str).unwrap_or_default();
```

and replace `let object = …` plus the grant lookup with:

```rust
    let object = connection.qualify(&ObjectRef::parse(target_name).path);
    let targets = match name {
        "data_execute_sql" => service.data_write_targets(connection, sql)?,
        "schema_apply_ddl" => service.schema_write_targets(connection, sql)?,
        _ => vec![object],
    };
    let profile_policy = service.policy();
    let grants = ledger.active_grants(&service.profile.name, now);
    let grant = grants
        .iter()
        .find(|grant| {
            targets
                .iter()
                .all(|target| grant.authorizes(name, &connection.name, target, &profile_policy, now))
        })
        .cloned()
```

(the `.ok_or_else(|| { audit(…); … })?` tail stays as it is).

Pass `connection` into `execute` and replace its `"data_execute_sql"` and `"schema_apply_ddl"` arms:

```rust
        "data_execute_sql" => {
            if cancelled() {
                return Ok((SideEffect::RolledBack, "rolled_back".into()));
            }
            let sql = value.get("sql").and_then(Value::as_str).unwrap_or_default();
            let affected = service.execute_write(session, connection, sql).await?;
            Ok((SideEffect::Committed, format!("{affected} rows affected")))
        }
        "schema_apply_ddl" => apply_ddl(value, session, connection, cancelled()).await,
```

Replace `apply_ddl` with:

```rust
/// Raw DDL from a client cannot be turned into a structured `SchemaChange`, so the risk
/// comes from the driver-api classifier and the typed confirmation is required from the
/// client, never filled in from the target it already sent. MySQL commits DDL implicitly;
/// that is a property of the connection, not something the client gets to claim.
async fn apply_ddl(
    value: &Value,
    session: &dyn Session,
    connection: &McpConnection,
    cancelled: bool,
) -> Result<(SideEffect, String), AppError> {
    let target = value.get("target").and_then(Value::as_str).unwrap_or_default();
    let sql = value.get("sql").and_then(Value::as_str).unwrap_or_default();
    let risk = classify_raw_sql(sql);
    if (risk.destructive || risk.data_loss)
        && value.get("confirm_target").and_then(Value::as_str) != Some(target)
    {
        return Err(AppError::new(
            ErrorCategory::Permission,
            format!("type {target} as confirm_target to confirm"),
        ));
    }
    if cancelled {
        return Ok((SideEffect::RolledBack, "rolled_back".into()));
    }
    let mut plan = DdlPlan::default();
    plan.push(sql, connection.dialect == dexo_sql::Dialect::Mysql);
    let outcome = session
        .ddl()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "ddl unavailable"))?
        .apply_ddl(&plan)
        .await
        .map_err(map_driver_error)?;
    Ok(map_ddl_outcome(outcome))
}
```

Fix the imports: add `classify_raw_sql` to the `dexo_driver_api` list and `use dexo_app::mcp::McpConnection;`; remove `ApplyRequest`, `apply_change`, `drop_table`, `production_policy`, `ObjectKind`, `SchemaChange`, `QueryRequest`, `DdlOutcome` only if the compiler reports them unused. Add `dexo-sql = { path = "../dexo-sql" }` to `[dependencies]` of `crates/dexo-mcp/Cargo.toml` (it was a dev-dependency since Task 3; move it).

- [ ] **Step 4: Tests for every refusal**

Append to the `tools_write` test module:

```rust
    #[tokio::test]
    async fn mysql_ddl_commit_is_not_claimed_reversed() {
        let service = service_with("db.*");
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant_on(
                &service.profile,
                GrantCapability::Ddl,
                "schema_apply_ddl",
                "db.items",
            ))
            .unwrap();
        let session = FakeSession::default();
        let mysql = McpConnection {
            dialect: dexo_sql::Dialect::Mysql,
            default_schema: None,
            ..connection("local")
        };
        let result = call_write_tool(
            &service,
            &ledger,
            Some((&mysql, &session as &dyn dexo_driver_api::Session)),
            "s",
            "schema_apply_ddl",
            json!({
                "operation_id":"op-mysql",
                "target":"db.items",
                "sql":"DROP TABLE items",
                "confirm_target":"db.items"
            })
            .as_object()
            .cloned()
            .unwrap(),
            0,
        )
        .await
        .unwrap();
        assert!(result.contains("Committed") && !result.contains("RolledBack"), "{result}");
        assert!(session.log().contains(&"ddl DROP TABLE items".to_string()));
    }

    #[tokio::test]
    async fn destructive_ddl_needs_the_target_typed_by_the_client() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(GrantCapability::Ddl, "schema_apply_ddl", "db.public.items"))
            .unwrap();
        let error = call(
            &ledger,
            "schema_apply_ddl",
            json!({"operation_id":"op-d","target":"db.public.items","sql":"DROP TABLE items"}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("type db.public.items"), "{error}");
    }

    #[tokio::test]
    async fn sql_writes_must_stay_inside_the_grant() {
        let mut profile = profile();
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
        profile.tool_rules.push(dexo_app::mcp::profile::ToolRule {
            tool: "data_execute_sql".into(),
            allowed: true,
        });
        let service = McpService::new(profile.clone(), Vec::new());
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant_on(&profile, GrantCapability::DataWrite, "data_execute_sql", "db.public.items"))
            .unwrap();
        let session = FakeSession::default();
        let error = call_write_tool(
            &service,
            &ledger,
            Some((&connection("local"), &session as &dyn dexo_driver_api::Session)),
            "s",
            "data_execute_sql",
            json!({
                "operation_id":"op-sub",
                "target":"db.public.items",
                "sql":"update items set x = 1 where id in (select id from orders)"
            })
            .as_object()
            .cloned()
            .unwrap(),
            0,
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "not found");
        assert!(session.log().is_empty());
    }

    #[tokio::test]
    async fn production_connections_refuse_writes_and_keep_the_grant() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(GrantCapability::DataWrite, "data_insert", "db.public.items"))
            .unwrap();
        let production = McpConnection {
            environment: dexo_app::Environment::Production,
            ..connection("local")
        };
        let error = call_write_tool(
            &service(),
            &ledger,
            Some((&production, &FakeSession::default() as &dyn dexo_driver_api::Session)),
            "s",
            "data_insert",
            json!({"operation_id":"op-p","target":"db.public.items","values":{"id":1}})
                .as_object()
                .cloned()
                .unwrap(),
            0,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("production"));
        assert_eq!(ledger.active_grants("assistant", 0).len(), 1);
    }
```

with these helpers next to `grant`:

```rust
    fn service_with(allow: &str) -> McpService {
        let mut profile = McpProfile::new("assistant");
        profile.selectors = vec![SelectorRule::parse(Effect::Allow, allow).unwrap()];
        McpService::new(profile, Vec::new())
    }

    fn grant_on(
        profile: &McpProfile,
        capability: GrantCapability,
        tool: &str,
        selector: &str,
    ) -> Grant {
        Grant::new(
            profile,
            "local",
            capability,
            vec![tool.into()],
            vec![SelectorRule::parse(Effect::Allow, selector).unwrap()],
            0,
            DEFAULT_TTL_SECS,
        )
        .unwrap()
    }
```

Update `data_execute_sql_rejects_ddl`: pass `Some((&connection("local"), &FakeSession::default() as &dyn dexo_driver_api::Session))` instead of `None`, and assert `error.to_string().contains("not allowed for this tool")`.

- [ ] **Step 5: Run, gate, commit**

Run: `cargo test -p dexo-mcp && cargo test -p dexo-app mcp::`
Expected: all pass.

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-app/src/mcp crates/dexo-mcp
git commit -m "fix(mcp): writes refuse production, stay inside the grant and need typed confirmation"
```

**End of Part A.** The server is still the old adapter, but every P0 finding (F1–F8, F12, F13) is closed and pinned by a test.

---
# Part B — Adapter rewrite

### Task 6: CLI profile configuration (MCP-002, MCP-015, MCP-019, F11)

Before this task a profile could not be pointed at a connection, switched to raw reads, re-limited or given tool rules without editing SQLite by hand.

**Files:**
- Modify: `crates/dexo-cli/src/args.rs` (`McpProfileCommand::Set`, `McpCommand::Allow { remove }`)
- Modify: `crates/dexo-cli/src/run.rs` (`run_mcp`, `run_mcp_profile`, `mcp_allow`, `mcp_policy`, `mcp_config_print`, `run_mcp_grant`)
- Modify: `crates/dexo-app/src/mcp/service.rs` (`known_tools`), `crates/dexo-app/src/mcp/mod.rs`
- Modify: `crates/dexo-cli/tests/mcp.rs`, `crates/dexo-cli/Cargo.toml`

**Interfaces:**
- Produces: `dexo mcp profile set --name N [--connection C]… [--clear-connections] [--query-mode structured|raw-read] [--max-rows N] [--max-bytes N] [--timeout-secs N] [--max-concurrency N] [--allow-tool T]… [--deny-tool T]…`
- Produces: `dexo mcp allow --profile P --selector S [--deny] [--remove]`
- Produces: `dexo mcp config print --profile P [--client json|claude-code]` with the absolute path of the running binary.
- Produces: `dexo_app::mcp::known_tools() -> Vec<&'static str>`.
- Changes: `dexo mcp grant create` refuses unknown connections and connections that do not accept writes (production, read-only).

- [ ] **Step 1: Arguments**

In `crates/dexo-cli/src/args.rs`, add to `McpProfileCommand`:

```rust
    Set {
        #[arg(long)]
        name: String,
        #[arg(long = "connection")]
        connections: Vec<String>,
        #[arg(long)]
        clear_connections: bool,
        #[arg(long, value_parser = ["structured", "raw-read"])]
        query_mode: Option<String>,
        #[arg(long)]
        max_rows: Option<u64>,
        #[arg(long)]
        max_bytes: Option<u64>,
        #[arg(long)]
        timeout_secs: Option<u64>,
        #[arg(long)]
        max_concurrency: Option<u32>,
        #[arg(long = "allow-tool")]
        allow_tools: Vec<String>,
        #[arg(long = "deny-tool")]
        deny_tools: Vec<String>,
    },
```

and to `McpCommand::Allow` add `#[arg(long)] remove: bool,` after `deny`.

- [ ] **Step 2: `known_tools`**

In `crates/dexo-app/src/mcp/service.rs`, after `advertised_tools`:

```rust
/// Every tool name a profile rule may mention, so a typo is refused instead of silently
/// matching nothing.
pub fn known_tools() -> Vec<&'static str> {
    let mut probe = McpProfile::new("probe");
    probe.query_mode = QueryMode::RawReadSql;
    let mut tools = advertised_tools(&probe);
    tools.extend(crate::mcp::grant::WRITE_TOOLS);
    tools
}
```

Re-export it: `pub use service::{McpService, ReadResult, advertised_tools, known_tools, new_result_uri};`.

- [ ] **Step 3: Handlers**

In `crates/dexo-cli/src/run.rs`, extend the `dexo_app::mcp` import with `McpConnection, QueryMode, ToolRule, known_tools`. In `run_mcp`, pass `remove` through: `McpCommand::Allow { profile, selector, deny, remove } => mcp_allow(&profile, &selector, deny, remove)?,`.

In `run_mcp_profile`, add the arm:

```rust
        McpProfileCommand::Set {
            name,
            connections,
            clear_connections,
            query_mode,
            max_rows,
            max_bytes,
            timeout_secs,
            max_concurrency,
            allow_tools,
            deny_tools,
        } => {
            let mut profile = load_profile(&repo, &name)?;
            if clear_connections {
                profile.connections.clear();
            }
            let saved = ConnectionRepository::new(db.connection());
            for connection in connections {
                let found = saved
                    .get_by_name(&connection)?
                    .ok_or_else(|| anyhow::anyhow!("unknown connection '{connection}'"))?;
                McpConnection::from_profile(&found)?;
                if !profile.connections.contains(&connection) {
                    profile.connections.push(connection);
                }
            }
            if let Some(mode) = query_mode {
                profile.query_mode = if mode == "raw-read" {
                    QueryMode::RawReadSql
                } else {
                    QueryMode::StructuredOnly
                };
            }
            if let Some(value) = max_rows {
                profile.limits.max_rows = value;
            }
            if let Some(value) = max_bytes {
                profile.limits.max_bytes = value;
            }
            if let Some(value) = timeout_secs {
                profile.limits.timeout_secs = value;
            }
            if let Some(value) = max_concurrency {
                profile.limits.max_concurrency = value;
            }
            let known = known_tools();
            for (tools, allowed) in [(allow_tools, true), (deny_tools, false)] {
                for tool in tools {
                    anyhow::ensure!(known.contains(&tool.as_str()), "unknown MCP tool '{tool}'");
                    profile.tool_rules.retain(|rule| rule.tool != tool);
                    profile.tool_rules.push(ToolRule { tool, allowed });
                }
            }
            repo.save(&profile)?;
            mcp_policy(&name)?;
        }
```

Replace `mcp_allow` with:

```rust
fn mcp_allow(name: &str, selector: &str, deny: bool, remove: bool) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let repo = McpProfileRepository::new(db.connection());
    let mut profile = load_profile(&repo, name)?;
    let effect = if deny { Effect::Deny } else { Effect::Allow };
    let rule = SelectorRule::parse(effect, selector)?;
    if remove {
        let before = profile.selectors.len();
        profile.selectors.retain(|existing| existing != &rule);
        anyhow::ensure!(
            profile.selectors.len() < before,
            "profile '{name}' has no rule '{rule}'"
        );
        println!("removed {rule}");
    } else {
        if !profile.selectors.contains(&rule) {
            profile.selectors.push(rule.clone());
        }
        println!("{rule}");
    }
    repo.save(&profile)?;
    Ok(())
}
```

Replace `mcp_policy`'s body after `load_profile` with:

```rust
    println!(
        "name={} enabled={} access=read_only query_mode={:?} max_rows={} max_bytes={} timeout_secs={} max_concurrency={} audit_retention_days={}",
        profile.name,
        profile.enabled,
        profile.query_mode,
        profile.limits.max_rows,
        profile.limits.max_bytes,
        profile.limits.timeout_secs,
        profile.limits.max_concurrency,
        profile.audit_retention_days
    );
    if profile.connections.is_empty() {
        println!(
            "connections: none (dexo mcp profile set --name {} --connection <name>)",
            profile.name
        );
    } else {
        println!("connections: {}", profile.connections.join(", "));
    }
    for rule in &profile.selectors {
        println!("selector {rule}");
    }
    for rule in &profile.tool_rules {
        let effect = if rule.allowed { "allow" } else { "deny" };
        println!("tool {effect} {}", rule.tool);
    }
    println!("tools: {}", advertised_tools(&profile).join(", "));
    Ok(())
```

Replace the `match` in `mcp_config_print` with:

```rust
    let exe = std::env::current_exe()?.display().to_string();
    match client.unwrap_or("json") {
        "claude-code" => println!("claude mcp add dexo -- {exe} mcp serve --profile {name}"),
        _ => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "dexo": { "command": exe, "args": ["mcp", "serve", "--profile", name] }
                }
            }))?
        ),
    }
    Ok(())
```

In `run_mcp_grant`'s `Create` arm, after the membership check, add:

```rust
            let saved = ConnectionRepository::new(db.connection())
                .get_by_name(&connection)?
                .ok_or_else(|| anyhow::anyhow!("unknown connection '{connection}'"))?;
            McpConnection::from_profile(&saved)?.accepts_writes()?;
```

- [ ] **Step 4: CLI contract tests (QUALITY-009)**

In `crates/dexo-cli/Cargo.toml` `[dev-dependencies]` add `uuid.workspace = true`. In `crates/dexo-cli/tests/mcp.rs`, add:

```rust
#[test]
fn mcp_profile_set_args_parse() {
    let set = Args::parse_from([
        "dexo",
        "mcp",
        "profile",
        "set",
        "--name",
        "assistant",
        "--connection",
        "local",
        "--connection",
        "reports",
        "--query-mode",
        "raw-read",
        "--max-rows",
        "200",
        "--deny-tool",
        "data_execute_sql",
    ]);
    assert!(matches!(
        set.command,
        Some(dexo_cli::args::Command::Mcp {
            command: McpCommand::Profile {
                command: McpProfileCommand::Set {
                    ref connections,
                    ref query_mode,
                    max_rows: Some(200),
                    ref deny_tools,
                    ..
                }
            }
        }) if connections == &["local", "reports"]
            && query_mode.as_deref() == Some("raw-read")
            && deny_tools == &["data_execute_sql"]
    ));
}
```

In `mcp_grant_lifecycle_and_audit_export`, the grant now needs a real connection. Right after `set_var("DEXO_DATA_HOME", …)` save one:

```rust
    {
        let paths = dexo_storage::AppPaths::from_data_home(dir.path().to_path_buf());
        let db = dexo_storage::Database::open(&paths.database).unwrap();
        dexo_storage::ConnectionRepository::new(db.connection())
            .save(&dexo_app::ConnectionProfile::new(
                dexo_app::ConnectionId(uuid::Uuid::new_v4()),
                None,
                "local",
                "postgres",
                "local",
                serde_json::json!({"host": "127.0.0.1", "port": 5432, "database": "db", "username": "u"}),
                dexo_app::SecretRef::new("unused".into()),
            ))
            .unwrap();
    }
```

- [ ] **Step 5: Run, gate, commit**

Run: `cargo test -p dexo-cli --test mcp`
Expected: all pass.

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-cli crates/dexo-app/src/mcp
git commit -m "feat(cli): configure MCP profiles without editing SQLite"
```

---

### Task 7: Backend, connection router and typed tool surface (MCP-001, MCP-005, MCP-013, MCP-014, MCP-019, F7, F12, F14, F15, F17)

This task replaces the adapter's plumbing. The tool set shrinks to what is real today: `list_connections`, `query_validate`, `query_execute_read`, `query_explain` and the seven grant-gated writes. Tasks 8–12 each add their own tools on top.

**Files:**
- Create: `crates/dexo-mcp/src/backend.rs`, `crates/dexo-mcp/src/render.rs`
- Rewrite: `crates/dexo-mcp/src/router.rs`, `crates/dexo-mcp/src/error.rs`, `crates/dexo-mcp/src/schema.rs`, `crates/dexo-mcp/src/server.rs`, `crates/dexo-mcp/src/tools_read.rs`, `crates/dexo-mcp/src/resources.rs`, `crates/dexo-mcp/src/stdio.rs`, `crates/dexo-mcp/src/lib.rs`
- Modify: `crates/dexo-mcp/src/tools_write.rs` (add the tool router block), `crates/dexo-mcp/Cargo.toml`
- Delete: `crates/dexo-mcp/src/limits.rs`, `crates/dexo-mcp/src/cancellation.rs`, `crates/dexo-mcp/src/audit.rs`, `crates/dexo-mcp/tests/multi_connection.rs`
- Rewrite: `crates/dexo-mcp/tests/protocol.rs`; create `crates/dexo-mcp/tests/common/mod.rs`
- Modify: `crates/dexo-app/src/mcp/service.rs` (`McpService::new(profile)`; delete snapshot methods, `new_result_uri`), `crates/dexo-app/src/mcp/mod.rs`
- Create: `crates/dexo-cli/src/mcp_backend.rs`; modify `crates/dexo-cli/src/lib.rs`, `crates/dexo-cli/src/run.rs`, `crates/dexo-cli/Cargo.toml`
- Modify: `crates/dexo/tests/mcp_read.rs`, `crates/dexo/tests/mcp_write.rs`

**Interfaces:**
- Produces: `dexo_mcp::McpBackend` (async trait): `connect(&self, &str) -> Result<Box<dyn Session>, AppError>`, `catalog_snapshot(&self, &str) -> Result<Vec<CatalogObject>, AppError>`, `schema_snapshot(&self, &str) -> Result<Option<SchemaSnapshot>, AppError>`.
- Produces: `router::{McpConnectionRouter, ConnectionSlot, SessionLease}`; `McpConnectionRouter::new(Vec<McpConnection>, Arc<dyn McpBackend>, Duration)`, `resolve(Option<&str>) -> Result<&ConnectionSlot, AppError>`, `lease(&ConnectionSlot) -> Result<SessionLease<'_>, AppError>`, `connections()`, `backend()`; `SessionLease { pub meta: &McpConnection }`, `session() -> &dyn Session`, `discard_if_broken(&AppError)`.
- Produces: `render::{RowsPage, rows_result, text_result, markdown_table}`; `RowsPage { columns, rows, truncated, bytes, elapsed, next_offset, title }`, `RowsPage::new(Vec<String>, Vec<Vec<String>>)`.
- Produces: `error::{HIDDEN, hidden, hidden_error, tool_error, app_error}`.
- Produces: `DexoMcpServer::new(McpService, Vec<McpConnection>, Arc<dyn McpBackend>, Arc<dyn GrantLedger>)`, `DexoMcpServer::visible_tools() -> Vec<String>`, `DexoMcpServer::stop_token() -> CancellationToken`, `TOOL_SCHEMA_VERSION`.
- Produces: `dexo_mcp::serve_stdio(service, connections, backend, ledger)`, `dexo_mcp::serve_io(service, connections, backend, ledger, read, write)`.
- Produces (crate-internal, used by Tasks 8–12): `DexoMcpServer::slot(Option<&str>) -> Result<&ConnectionSlot, CallToolResult>`, `DexoMcpServer::open(Option<&str>) -> Result<SessionLease<'_>, CallToolResult>`, `tools_read::finish(&mut SessionLease, Result<CallToolResult, AppError>) -> CallToolResult`.
- Changes: `McpService::new(profile: McpProfile) -> McpService` (no catalog snapshot); `advertised_tools` returns only tools that exist, from `pub const READ_TOOLS`.

- [ ] **Step 1: Dependencies and module list**

`crates/dexo-mcp/Cargo.toml` becomes:

```toml
[package]
name = "dexo-mcp"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
anyhow.workspace = true
async-trait.workspace = true
dexo-app = { path = "../dexo-app" }
dexo-driver-api = { path = "../dexo-driver-api" }
dexo-sql = { path = "../dexo-sql" }
rmcp.workspace = true
schemars.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tracing-subscriber.workspace = true
uuid.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "io-util"] }
dexo-test-support = { path = "../dexo-test-support" }
```

Delete `src/limits.rs`, `src/cancellation.rs`, `src/audit.rs` and `tests/multi_connection.rs`. Replace `src/lib.rs` with:

```rust
pub mod backend;
pub mod error;
pub mod prompts;
pub mod render;
pub mod resources;
pub mod router;
pub mod schema;
pub mod server;
pub mod stdio;
pub mod tools_read;
pub mod tools_write;

pub use backend::McpBackend;
pub use error::hidden_error;
pub use server::{DexoMcpServer, TOOL_SCHEMA_VERSION};
pub use stdio::{init_mcp_tracing, serve_io, serve_stdio};
```

- [ ] **Step 2: The backend seam**

Create `crates/dexo-mcp/src/backend.rs`:

```rust
use dexo_app::AppError;
use dexo_app::schema_diff::SchemaSnapshot;
use dexo_driver_api::{CatalogObject, Session};

/// What the server needs from the host binary. The keychain, the drivers and the local
/// SQLite belong to `dexo`; the adapter crate reaches them only through this.
#[async_trait::async_trait]
pub trait McpBackend: Send + Sync {
    async fn connect(&self, connection: &str) -> Result<Box<dyn Session>, AppError>;
    /// The connection's indexed catalog; the first call on a connection may build it.
    async fn catalog_snapshot(&self, connection: &str) -> Result<Vec<CatalogObject>, AppError>;
    fn schema_snapshot(&self, name: &str) -> Result<Option<SchemaSnapshot>, AppError>;
}
```

- [ ] **Step 3: The router**

Replace `crates/dexo-mcp/src/router.rs` with:

```rust
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use dexo_app::mcp::McpConnection;
use dexo_app::{AppError, ErrorCategory};
use dexo_driver_api::Session;
use tokio::sync::{Mutex, MutexGuard};

use crate::backend::McpBackend;
use crate::error::hidden;

/// One connection the profile may use. The session opens on first use and stays open;
/// the mutex serializes calls on it, because a read-only transaction and a write cannot
/// share one server session at the same time.
pub struct ConnectionSlot {
    pub meta: McpConnection,
    session: Mutex<Option<Box<dyn Session>>>,
}

pub struct SessionLease<'a> {
    pub meta: &'a McpConnection,
    guard: MutexGuard<'a, Option<Box<dyn Session>>>,
}

impl SessionLease<'_> {
    pub fn session(&self) -> &dyn Session {
        self.guard
            .as_deref()
            .expect("a lease always holds an open session")
    }

    /// After a network or authentication failure the session is useless; dropping it
    /// makes the next call reconnect instead of failing forever.
    pub fn discard_if_broken(&mut self, error: &AppError) {
        if matches!(
            error.category(),
            ErrorCategory::Network | ErrorCategory::Transport | ErrorCategory::Authentication
        ) {
            *self.guard = None;
        }
    }
}

pub struct McpConnectionRouter {
    slots: BTreeMap<String, ConnectionSlot>,
    backend: Arc<dyn McpBackend>,
    connect_timeout: Duration,
}

impl McpConnectionRouter {
    pub fn new(
        connections: Vec<McpConnection>,
        backend: Arc<dyn McpBackend>,
        connect_timeout: Duration,
    ) -> Self {
        let slots = connections
            .into_iter()
            .map(|meta| {
                (
                    meta.name.clone(),
                    ConnectionSlot {
                        meta,
                        session: Mutex::new(None),
                    },
                )
            })
            .collect();
        Self {
            slots,
            backend,
            connect_timeout,
        }
    }

    pub fn connections(&self) -> impl Iterator<Item = &McpConnection> {
        self.slots.values().map(|slot| &slot.meta)
    }

    pub fn backend(&self) -> &dyn McpBackend {
        self.backend.as_ref()
    }

    /// Explicit routing (MCP-019): a name must be one of the profile's connections, and
    /// leaving it out is only allowed when there is exactly one.
    pub fn resolve(&self, requested: Option<&str>) -> Result<&ConnectionSlot, AppError> {
        match requested {
            Some(name) => self.slots.get(name).ok_or_else(hidden),
            None if self.slots.len() == 1 => Ok(self.slots.values().next().expect("one slot")),
            None => Err(AppError::new(
                ErrorCategory::Configuration,
                "this profile has several connections; pass `connection` (see list_connections)",
            )),
        }
    }

    pub async fn lease<'a>(&'a self, slot: &'a ConnectionSlot) -> Result<SessionLease<'a>, AppError> {
        let mut guard = slot.session.lock().await;
        if guard.is_none() {
            let session = tokio::time::timeout(
                self.connect_timeout,
                self.backend.connect(&slot.meta.name),
            )
            .await
            .map_err(|_| {
                AppError::new(
                    ErrorCategory::Timeout,
                    format!("connecting to {} timed out", slot.meta.name),
                )
            })??;
            *guard = Some(session);
        }
        Ok(SessionLease {
            meta: &slot.meta,
            guard,
        })
    }
}
```

- [ ] **Step 4: Errors and rendering**

Replace `crates/dexo-mcp/src/error.rs` with:

```rust
use dexo_app::{AppError, ErrorCategory};
use rmcp::model::{CallToolResult, ContentBlock};

pub const HIDDEN: &str = "not found";

pub fn hidden_error() -> &'static str {
    HIDDEN
}

pub fn hidden() -> AppError {
    AppError::new(ErrorCategory::McpPolicy, HIDDEN)
}

/// `Error [CODE]: message`, a shape a model can branch on without parsing prose.
pub fn tool_error(code: &str, message: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!("Error [{code}]: {message}"))])
}

pub fn app_error(error: &AppError) -> CallToolResult {
    let message = error.to_string();
    let code = match error.category() {
        ErrorCategory::McpPolicy if message == HIDDEN => "NOT_FOUND",
        ErrorCategory::McpPolicy => "POLICY_DENIED",
        ErrorCategory::Permission => "PERMISSION_DENIED",
        ErrorCategory::Configuration | ErrorCategory::Syntax => "INVALID_INPUT",
        ErrorCategory::Timeout => "TIMEOUT",
        ErrorCategory::Cancelled => "CANCELLED",
        ErrorCategory::Capability => "UNSUPPORTED",
        ErrorCategory::Conflict => "CONFLICT",
        ErrorCategory::Authentication | ErrorCategory::Network | ErrorCategory::Transport => {
            "CONNECTION_FAILED"
        }
        ErrorCategory::Storage | ErrorCategory::ExternalTool | ErrorCategory::Internal => {
            "INTERNAL"
        }
    };
    tool_error(code, &message)
}
```

Create `crates/dexo-mcp/src/render.rs`:

```rust
use std::time::Duration;

use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Value, json};

/// Cells longer than this are cut in the Markdown view; `structuredContent` keeps them
/// whole, within the profile's byte limit.
const CELL_CHARS: usize = 200;

#[derive(Clone, Debug, Default)]
pub struct RowsPage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
    pub bytes: u64,
    pub elapsed: Duration,
    pub next_offset: Option<u64>,
    pub title: Option<String>,
}

impl RowsPage {
    pub fn new(columns: Vec<String>, rows: Vec<Vec<String>>) -> Self {
        let bytes = rows.iter().flatten().map(|cell| cell.len() as u64).sum();
        Self {
            columns,
            rows,
            bytes,
            ..Self::default()
        }
    }
}

/// A Markdown table for the model to read, and the same rows as `structuredContent` for
/// clients that parse. The result always says whether it was cut short (MCP-013).
pub fn rows_result(page: &RowsPage) -> CallToolResult {
    let mut text = String::new();
    if let Some(title) = &page.title {
        text.push_str(title);
        text.push_str("\n\n");
    }
    text.push_str(&markdown_table(&page.columns, &page.rows));
    text.push_str(&format!(
        "\n({} rows, {} ms)",
        page.rows.len(),
        page.elapsed.as_millis()
    ));
    if page.truncated {
        text.push_str("\nResult truncated at the profile's row or byte limit; narrow the query or select fewer columns.");
    }
    if let Some(offset) = page.next_offset {
        text.push_str(&format!("\nMore rows: call again with offset={offset}."));
    }
    text_result(
        text,
        json!({
            "columns": page.columns,
            "rows": page.rows,
            "row_count": page.rows.len(),
            "truncated": page.truncated,
            "bytes": page.bytes,
            "elapsed_ms": page.elapsed.as_millis() as u64,
            "next_offset": page.next_offset,
        }),
    )
}

pub fn text_result(text: impl Into<String>, data: Value) -> CallToolResult {
    let mut result = CallToolResult::success(vec![ContentBlock::text(text.into())]);
    result.structured_content = Some(data);
    result
}

pub fn markdown_table(columns: &[String], rows: &[Vec<String>]) -> String {
    let mut out = String::from("|");
    for column in columns {
        out.push_str(&format!(" {} |", cell(column)));
    }
    out.push_str("\n|");
    for _ in columns {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in rows {
        out.push('|');
        for value in row {
            out.push_str(&format!(" {} |", cell(value)));
        }
        out.push('\n');
    }
    out
}

fn cell(text: &str) -> String {
    let flat = text.replace('|', "\\|").replace(['\n', '\r'], " ");
    if flat.chars().count() > CELL_CHARS {
        format!("{}…", flat.chars().take(CELL_CHARS).collect::<String>())
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::markdown_table;

    #[test]
    fn cells_cannot_break_the_table() {
        let table = markdown_table(
            &["a".into()],
            &[vec!["x|y\nz".into()], vec!["w".repeat(300)]],
        );
        assert!(table.contains("x\\|y z"));
        assert!(table.contains('…'));
        assert_eq!(table.lines().count(), 4);
    }
}
```

- [ ] **Step 5: Typed inputs**

Replace `crates/dexo-mcp/src/schema.rs` with:

```rust
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SqlInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// One read-only statement: SELECT, WITH … SELECT, VALUES, TABLE, or EXPLAIN without ANALYZE.
    pub sql: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataInsertInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// Table to write, e.g. `public.orders`; a bare name uses the connection's default database and schema.
    pub target: String,
    /// Column name to value for the new row.
    pub values: Map<String, Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataUpdateInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// Table to write, e.g. `public.orders`.
    pub target: String,
    /// Primary-key (or unique) column to value, naming exactly one row.
    pub identity: Map<String, Value>,
    /// Column name to new value.
    pub values: Map<String, Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataDeleteInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// Table to write, e.g. `public.orders`.
    pub target: String,
    /// Primary-key (or unique) column to value, naming exactly one row.
    pub identity: Map<String, Value>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataSqlInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// The table the grant was issued for. Every table the statement touches must be inside the grant.
    pub target: String,
    /// One INSERT, UPDATE or DELETE.
    pub sql: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DdlInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this change. Retrying with the same id and payload returns the first result instead of writing twice.
    pub operation_id: String,
    /// The object the grant was issued for.
    pub target: String,
    /// One CREATE TABLE, ALTER TABLE, DROP, CREATE INDEX or CREATE VIEW.
    pub sql: String,
    /// Required for destructive DDL: repeat `target` exactly to confirm.
    pub confirm_target: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AdminActionInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Unique id for this action. Retrying with the same id and payload returns the first result.
    pub operation_id: String,
    /// Server session to act on, from `admin_list_sessions`.
    pub session_id: String,
    /// Required to terminate a session: repeat `session_id` exactly to confirm.
    pub confirm_target: Option<String>,
}
```

- [ ] **Step 6: The server**

Replace `crates/dexo-mcp/src/server.rs` with:

```rust
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dexo_app::mcp::ledger::GrantLedger;
use dexo_app::mcp::{McpConnection, McpService, advertised_tools};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, GetPromptRequestParams, GetPromptResponse,
    GetPromptResult, Implementation, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, ResourceContents, ServerCapabilities, ServerInfo,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::backend::McpBackend;
use crate::error::{HIDDEN, tool_error};
use crate::router::McpConnectionRouter;
use crate::tools_write::{now_secs, write_tool_names};
use crate::{prompts, resources};

/// Bumped whenever a tool's name, input schema or annotations change; the snapshot test
/// in `tests/protocol.rs` fails until it is (MCP-020).
pub const TOOL_SCHEMA_VERSION: u32 = 2;

#[derive(Clone)]
pub struct DexoMcpServer {
    pub(crate) inner: Arc<Inner>,
    tool_router: ToolRouter<Self>,
}

pub(crate) struct Inner {
    pub service: McpService,
    pub router: McpConnectionRouter,
    pub ledger: Arc<dyn GrantLedger>,
    pub session_id: String,
    calls: Semaphore,
    last_revision: Mutex<u64>,
    stop: CancellationToken,
}

impl DexoMcpServer {
    pub fn new(
        service: McpService,
        connections: Vec<McpConnection>,
        backend: Arc<dyn McpBackend>,
        ledger: Arc<dyn GrantLedger>,
    ) -> Self {
        let concurrency = service.profile.limits.max_concurrency as usize;
        let connect_timeout = Duration::from_secs(service.profile.limits.timeout_secs);
        Self {
            inner: Arc::new(Inner {
                router: McpConnectionRouter::new(connections, backend, connect_timeout),
                ledger,
                session_id: uuid::Uuid::new_v4().to_string(),
                calls: Semaphore::new(concurrency),
                last_revision: Mutex::new(0),
                stop: CancellationToken::new(),
                service,
            }),
            tool_router: Self::read_tools() + Self::write_tools(),
        }
    }

    /// Stops the `list_changed` poller; `serve_io` cancels it when the client goes away.
    pub fn stop_token(&self) -> CancellationToken {
        self.inner.stop.clone()
    }

    /// What this client may see right now: the profile's reads, plus the writes an active
    /// grant has published (MCP-002, MCP-004). `tools/call` is gated by the same list.
    pub fn visible_tools(&self) -> Vec<String> {
        let profile = &self.inner.service.profile;
        let mut tools: Vec<String> = advertised_tools(profile)
            .into_iter()
            .map(str::to_string)
            .collect();
        tools.extend(
            write_tool_names(self.inner.ledger.as_ref(), &profile.name, now_secs())
                .into_iter()
                .filter(|tool| profile.tool_allowed(tool)),
        );
        tools
    }
}

impl ServerHandler for DexoMcpServer {
    fn get_info(&self) -> ServerInfo {
        let profile = &self.inner.service.profile;
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("dexo", env!("CARGO_PKG_VERSION")))
        .with_instructions(format!(
            "Dexo database workbench, MCP profile '{}' (tool schema v{TOOL_SCHEMA_VERSION}). \
             Start with list_connections; every database tool takes an optional `connection`. \
             Reads run inside read-only transactions and return at most {} rows / {} bytes. \
             Write tools appear only while a grant created with `dexo mcp grant create` is active.",
            profile.name, profile.limits.max_rows, profile.limits.max_bytes
        ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let visible = self.visible_tools();
        Ok(ListToolsResult::with_all_items(
            self.tool_router
                .list_all()
                .into_iter()
                .filter(|tool| visible.iter().any(|name| name == tool.name.as_ref()))
                .collect(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        if !self
            .visible_tools()
            .iter()
            .any(|name| name == request.name.as_ref())
        {
            return Ok(tool_error("NOT_FOUND", HIDDEN).into());
        }
        let Ok(_permit) = self.inner.calls.try_acquire() else {
            return Ok(tool_error(
                "BUSY",
                "too many calls in flight for this profile; retry shortly",
            )
            .into());
        };
        self.tool_router
            .call(ToolCallContext::new(self, request, context))
            .await
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(
            resources::list_resources(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        match resources::read_resource(
            &self.inner.service,
            self.inner.router.connections(),
            &request.uri,
        ) {
            Some(body) => {
                Ok(ReadResourceResult::new(vec![ResourceContents::text(body, request.uri)]).into())
            }
            None => Err(McpError::resource_not_found(
                HIDDEN,
                Some(serde_json::json!({ "uri": request.uri })),
            )),
        }
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(Vec::new()))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(prompts::list_prompts(
            &self.inner.service,
        )))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        match prompts::get_prompt(&self.inner.service, &request.name) {
            Ok(messages) => Ok(GetPromptResult::new(messages).into()),
            Err(_) => Err(McpError::invalid_params(HIDDEN, None)),
        }
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        let inner = Arc::clone(&self.inner);
        let peer = context.peer.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = inner.stop.cancelled() => break,
                    () = tokio::time::sleep(Duration::from_millis(150)) => {}
                }
                let revision = inner.ledger.revision();
                let changed = {
                    let mut last = inner.last_revision.lock().expect("revision");
                    let changed = revision != *last;
                    *last = revision;
                    changed
                };
                if changed {
                    let _ = peer.notify_tool_list_changed().await;
                }
            }
        });
    }
}
```

The poller holds an `Arc<Inner>`, so it cannot rely on `Drop` to stop; `serve_io` cancels `stop` once the client disconnects (Step 9).

- [ ] **Step 7: Read tools**

Replace `crates/dexo-mcp/src/tools_read.rs` with:

```rust
use std::time::Instant;

use dexo_app::AppError;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::error::app_error;
use crate::render::{RowsPage, rows_result, text_result};
use crate::router::{ConnectionSlot, SessionLease};
use crate::schema::SqlInput;
use crate::server::DexoMcpServer;

impl DexoMcpServer {
    pub(crate) fn slot(&self, connection: Option<&str>) -> Result<&ConnectionSlot, CallToolResult> {
        self.inner
            .router
            .resolve(connection)
            .map_err(|error| app_error(&error))
    }

    pub(crate) async fn open(
        &self,
        connection: Option<&str>,
    ) -> Result<SessionLease<'_>, CallToolResult> {
        let slot = self.slot(connection)?;
        self.inner
            .router
            .lease(slot)
            .await
            .map_err(|error| app_error(&error))
    }
}

/// Turns an app result into a tool result, first dropping the session if the error says
/// the connection is gone.
pub(crate) fn finish(
    lease: &mut SessionLease<'_>,
    outcome: Result<CallToolResult, AppError>,
) -> CallToolResult {
    match outcome {
        Ok(result) => result,
        Err(error) => {
            lease.discard_if_broken(&error);
            app_error(&error)
        }
    }
}

#[tool_router(router = read_tools, vis = "pub(crate)")]
impl DexoMcpServer {
    /// The saved connections this profile may use: driver, database, environment, and whether writes are possible at all. Pass a name as `connection` to the other tools.
    #[tool(annotations(read_only_hint = true))]
    async fn list_connections(&self) -> CallToolResult {
        let rows = self
            .inner
            .router
            .connections()
            .map(|connection| {
                vec![
                    connection.name.clone(),
                    connection.driver.clone(),
                    connection.database.clone().unwrap_or_default(),
                    format!("{:?}", connection.environment).to_lowercase(),
                    match connection.accepts_writes() {
                        Ok(()) => "with a grant".to_string(),
                        Err(error) => error.to_string(),
                    },
                ]
            })
            .collect();
        let columns = ["name", "driver", "database", "environment", "writes"]
            .map(String::from)
            .to_vec();
        rows_result(&RowsPage::new(columns, rows))
    }

    /// Check whether query_execute_read would accept a statement, without running it. Says why when it would not.
    #[tool(annotations(read_only_hint = true))]
    async fn query_validate(&self, Parameters(input): Parameters<SqlInput>) -> CallToolResult {
        let slot = match self.slot(input.connection.as_deref()) {
            Ok(slot) => slot,
            Err(result) => return result,
        };
        match self.inner.service.validate_sql(&slot.meta, &input.sql) {
            Ok(()) => text_result(
                "ok: one read-only statement, and every table it names is allowed",
                json!({ "valid": true }),
            ),
            Err(error) => app_error(&error),
        }
    }

    /// Run one read-only statement inside a read-only transaction and return the rows as a table. Every table it names must be inside the profile's allowlist.
    #[tool(annotations(read_only_hint = true))]
    async fn query_execute_read(
        &self,
        Parameters(input): Parameters<SqlInput>,
        cancel: CancellationToken,
    ) -> CallToolResult {
        let slot = match self.slot(input.connection.as_deref()) {
            Ok(slot) => slot,
            Err(result) => return result,
        };
        if let Err(error) = self.inner.service.validate_sql(&slot.meta, &input.sql) {
            return app_error(&error);
        }
        let mut lease = match self.inner.router.lease(slot).await {
            Ok(lease) => lease,
            Err(error) => return app_error(&error),
        };
        let started = Instant::now();
        let outcome = self
            .inner
            .service
            .execute_read(lease.session(), lease.meta, &input.sql, &cancel)
            .await
            .map(|result| {
                rows_result(&RowsPage {
                    columns: result.columns,
                    rows: result.rows,
                    truncated: result.truncated,
                    bytes: result.bytes,
                    elapsed: started.elapsed(),
                    next_offset: None,
                    title: None,
                })
            });
        finish(&mut lease, outcome)
    }

    /// Estimated plan for one read-only statement. Runs EXPLAIN without ANALYZE, so nothing is executed.
    #[tool(annotations(read_only_hint = true))]
    async fn query_explain(&self, Parameters(input): Parameters<SqlInput>) -> CallToolResult {
        let slot = match self.slot(input.connection.as_deref()) {
            Ok(slot) => slot,
            Err(result) => return result,
        };
        if let Err(error) = self.inner.service.validate_sql(&slot.meta, &input.sql) {
            return app_error(&error);
        }
        let mut lease = match self.inner.router.lease(slot).await {
            Ok(lease) => lease,
            Err(error) => return app_error(&error),
        };
        let outcome = self
            .inner
            .service
            .explain(lease.session(), lease.meta, &input.sql)
            .await
            .map(|plan| {
                text_result(
                    serde_json::to_string_pretty(&plan).unwrap_or_default(),
                    serde_json::to_value(&plan).unwrap_or_default(),
                )
            });
        finish(&mut lease, outcome)
    }
}
```

- [ ] **Step 8: Write tools on the router**

In `crates/dexo-mcp/src/tools_write.rs`, add these imports:

```rust
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde::Serialize;
use serde_json::json;

use crate::render::text_result;
use crate::schema::{
    AdminActionInput, DataDeleteInput, DataInsertInput, DataSqlInput, DataUpdateInput, DdlInput,
};
use crate::server::DexoMcpServer;
use crate::tools_read::finish;
```

and add, above `pub fn write_tool_names`:

```rust
impl DexoMcpServer {
    async fn write(
        &self,
        name: &str,
        connection: Option<String>,
        input: &impl Serialize,
    ) -> CallToolResult {
        let arguments = serde_json::to_value(input)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        let mut lease = match self.open(connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let outcome = call_write_tool(
            &self.inner.service,
            self.inner.ledger.as_ref(),
            Some((lease.meta, lease.session())),
            &self.inner.session_id,
            name,
            arguments,
            now_secs(),
        )
        .await
        .map(|text| text_result(text.clone(), json!({ "outcome": text })));
        finish(&mut lease, outcome)
    }
}

#[tool_router(router = write_tools, vis = "pub(crate)")]
impl DexoMcpServer {
    /// Insert one row. Appears only while a data_write grant covering `target` is active; one successful call spends the grant.
    #[tool(annotations(read_only_hint = false, destructive_hint = false))]
    async fn data_insert(&self, Parameters(input): Parameters<DataInsertInput>) -> CallToolResult {
        self.write("data_insert", input.connection.clone(), &input).await
    }

    /// Update the one row named by `identity`. Appears only while a data_write grant covering `target` is active.
    #[tool(annotations(read_only_hint = false, destructive_hint = true))]
    async fn data_update(&self, Parameters(input): Parameters<DataUpdateInput>) -> CallToolResult {
        self.write("data_update", input.connection.clone(), &input).await
    }

    /// Delete the one row named by `identity`. Appears only while a data_write grant covering `target` is active.
    #[tool(annotations(read_only_hint = false, destructive_hint = true))]
    async fn data_delete(&self, Parameters(input): Parameters<DataDeleteInput>) -> CallToolResult {
        self.write("data_delete", input.connection.clone(), &input).await
    }

    /// Run one INSERT, UPDATE or DELETE. Every table it touches must be inside the grant; needs the profile rule `--allow-tool data_execute_sql`.
    #[tool(annotations(read_only_hint = false, destructive_hint = true))]
    async fn data_execute_sql(&self, Parameters(input): Parameters<DataSqlInput>) -> CallToolResult {
        self.write("data_execute_sql", input.connection.clone(), &input).await
    }

    /// Apply one DDL statement. Destructive DDL needs `confirm_target` equal to `target`. MySQL commits DDL implicitly.
    #[tool(annotations(read_only_hint = false, destructive_hint = true))]
    async fn schema_apply_ddl(&self, Parameters(input): Parameters<DdlInput>) -> CallToolResult {
        self.write("schema_apply_ddl", input.connection.clone(), &input).await
    }

    /// Cancel the running query of one server session. Needs an admin grant for this connection.
    #[tool(annotations(read_only_hint = false, destructive_hint = false))]
    async fn admin_cancel_query(&self, Parameters(input): Parameters<AdminActionInput>) -> CallToolResult {
        self.write("admin_cancel_query", input.connection.clone(), &input).await
    }

    /// Terminate one server session. Needs an admin grant for this connection and `confirm_target` equal to `session_id`.
    #[tool(annotations(read_only_hint = false, destructive_hint = true))]
    async fn admin_terminate_session(&self, Parameters(input): Parameters<AdminActionInput>) -> CallToolResult {
        self.write("admin_terminate_session", input.connection.clone(), &input).await
    }
}
```

`call_write_tool` keeps its `Map<String, Value>` signature, so the payload hash covers exactly the typed input. Admin actions carry no `target`; Task 4 made admin grants connection-wide, so the empty object path is never consulted for them.

- [ ] **Step 9: Resources, stdio**

Replace `crates/dexo-mcp/src/resources.rs` with:

```rust
use dexo_app::mcp::{McpConnection, McpService};
use rmcp::model::Resource;

pub const CAPABILITIES_URI: &str = "dexo://profile/capabilities";

pub fn list_resources() -> Vec<Resource> {
    vec![Resource::new(
        CAPABILITIES_URI,
        "Active MCP profile: tools, limits and connections",
    )]
}

pub fn read_resource<'a>(
    service: &McpService,
    connections: impl Iterator<Item = &'a McpConnection>,
    uri: &str,
) -> Option<String> {
    if uri != CAPABILITIES_URI {
        return None;
    }
    let mut capabilities = service.capabilities();
    capabilities["connections"] = connections
        .map(|connection| {
            serde_json::json!({
                "name": connection.name,
                "driver": connection.driver,
                "environment": format!("{:?}", connection.environment).to_lowercase(),
            })
        })
        .collect();
    Some(capabilities.to_string())
}
```

Replace `crates/dexo-mcp/src/stdio.rs` with:

```rust
use std::sync::Arc;

use dexo_app::mcp::{GrantLedger, McpConnection, McpService};
use rmcp::ServiceExt;
use tracing_subscriber::EnvFilter;

use crate::backend::McpBackend;
use crate::server::DexoMcpServer;

pub fn init_mcp_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
}

pub async fn serve_stdio(
    service: McpService,
    connections: Vec<McpConnection>,
    backend: Arc<dyn McpBackend>,
    ledger: Arc<dyn GrantLedger>,
) -> anyhow::Result<()> {
    init_mcp_tracing();
    let server = DexoMcpServer::new(service, connections, backend, ledger);
    let stop = server.stop_token();
    let running = server.serve(rmcp::transport::io::stdio()).await?;
    let outcome = running.waiting().await;
    stop.cancel();
    outcome?;
    Ok(())
}

pub async fn serve_io<R, W>(
    service: McpService,
    connections: Vec<McpConnection>,
    backend: Arc<dyn McpBackend>,
    ledger: Arc<dyn GrantLedger>,
    read: R,
    write: W,
) -> anyhow::Result<()>
where
    R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
    W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
{
    init_mcp_tracing();
    let server = DexoMcpServer::new(service, connections, backend, ledger);
    let stop = server.stop_token();
    let running = server.serve((read, write)).await?;
    let outcome = running.waiting().await;
    stop.cancel();
    outcome?;
    Ok(())
}
```

- [ ] **Step 10: The app service loses its snapshot**

In `crates/dexo-app/src/mcp/service.rs`:
- change the struct to `pub struct McpService { pub profile: McpProfile }` and `new` to `pub fn new(profile: McpProfile) -> Self { Self { profile } }`;
- delete `search`, `describe`, `ddl`, `relationships`, `find`, `new_result_uri` and the `SearchService`, `CatalogObject` and `Uuid` imports that only they used;
- in `capabilities`, replace the hard-coded `"persistent_access": "read_only"` with `"persistent_access": format!("{:?}", self.profile.persistent_access)`;
- replace `advertised_tools` with:

```rust
/// Read tools that exist. Tasks 8–12 add their names here as they land.
pub const READ_TOOLS: &[&str] = &[
    "list_connections",
    "query_validate",
    "query_explain",
    "query_execute_read",
];

const RAW_SQL_TOOLS: &[&str] = &["query_validate", "query_explain", "query_execute_read"];

pub fn advertised_tools(profile: &McpProfile) -> Vec<&'static str> {
    READ_TOOLS
        .iter()
        .copied()
        .filter(|name| {
            profile.query_mode == QueryMode::RawReadSql || !RAW_SQL_TOOLS.contains(name)
        })
        .filter(|name| profile.tool_allowed(name))
        .collect()
}
```

- delete the `denied_objects_are_absent_from_search_and_describe` test (it exercises the removed snapshot search; Task 8's `denied_tables_and_their_columns_are_invisible_but_their_schema_is_not` replaces it), and change every `McpService::new(profile, Vec::new())` in the file's tests to `McpService::new(profile)`.

In `crates/dexo-app/src/mcp/mod.rs` change the re-export to `pub use service::{McpService, READ_TOOLS, ReadResult, advertised_tools, known_tools};`.

Everywhere else `McpService::new(x, …)` appears (`crates/dexo-mcp/src/tools_write.rs` tests, `crates/dexo/tests/mcp_read.rs`, `crates/dexo/tests/mcp_write.rs`), drop the second argument. In `crates/dexo/tests/mcp_write.rs` delete the `service.describe("secrets")` assertion; Task 8's tests cover catalog visibility.

- [ ] **Step 11: The CLI backend and `mcp serve`**

In `crates/dexo-cli/Cargo.toml` `[dependencies]` add `async-trait.workspace = true`. In `crates/dexo-cli/src/run.rs` make `connect_session` and `refresh_catalog` `pub(crate)`. Create `crates/dexo-cli/src/mcp_backend.rs`:

```rust
use std::path::PathBuf;

use dexo_app::schema_diff::SchemaSnapshot;
use dexo_app::{AppError, ConnectionProfile, DriverRegistry, ErrorCategory};
use dexo_driver_api::{CatalogObject, Session};
use dexo_mcp::McpBackend;
use dexo_storage::{CatalogCache, ConnectionRepository, Database, SchemaSnapshotStore};

use crate::run::{catalog_database_name, connect_session, refresh_catalog};

/// Hands the MCP adapter the keychain, the drivers and SQLite. The database file is
/// reopened per call because a SQLite connection cannot be held across an await.
pub struct CliMcpBackend {
    registry: DriverRegistry,
    database: PathBuf,
}

impl CliMcpBackend {
    pub fn new(registry: DriverRegistry, database: PathBuf) -> Self {
        Self { registry, database }
    }

    fn saved(&self, name: &str) -> Result<ConnectionProfile, AppError> {
        let db = Database::open(&self.database).map_err(storage)?;
        ConnectionRepository::new(db.connection())
            .get_by_name(name)
            .map_err(storage)?
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::Configuration,
                    format!("unknown connection '{name}'"),
                )
            })
    }
}

#[async_trait::async_trait]
impl McpBackend for CliMcpBackend {
    async fn connect(&self, connection: &str) -> Result<Box<dyn Session>, AppError> {
        let saved = self.saved(connection)?;
        connect_session(&self.registry, &saved)
            .await
            .map_err(from_anyhow)
    }

    async fn catalog_snapshot(&self, connection: &str) -> Result<Vec<CatalogObject>, AppError> {
        let saved = self.saved(connection)?;
        let key = saved.id.0.to_string();
        let database = catalog_database_name(&saved);
        let cached = {
            let db = Database::open(&self.database).map_err(storage)?;
            CatalogCache::new(db.connection())
                .load_latest(&key, &database)
                .map_err(storage)?
        };
        if !cached.is_empty() {
            return Ok(cached);
        }
        let objects = refresh_catalog(&self.registry, &saved)
            .await
            .map_err(from_anyhow)?;
        let db = Database::open(&self.database).map_err(storage)?;
        CatalogCache::new(db.connection())
            .replace_snapshot(&key, &database, &objects)
            .map_err(storage)?;
        Ok(objects)
    }

    fn schema_snapshot(&self, name: &str) -> Result<Option<SchemaSnapshot>, AppError> {
        let db = Database::open(&self.database).map_err(storage)?;
        SchemaSnapshotStore::new(db.connection())
            .load_by_name(name)
            .map_err(storage)
    }
}

fn storage(error: anyhow::Error) -> AppError {
    AppError::new(ErrorCategory::Storage, error.to_string())
}

fn from_anyhow(error: anyhow::Error) -> AppError {
    match error.downcast::<AppError>() {
        Ok(error) => error,
        Err(error) => AppError::new(ErrorCategory::Internal, error.to_string()),
    }
}
```

Add `pub mod mcp_backend;` to `crates/dexo-cli/src/lib.rs`. Replace `mcp_serve` with:

```rust
async fn mcp_serve(registry: DriverRegistry, name: String) -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    let db = Database::open(&paths.database)?;
    let profile = load_profile(&McpProfileRepository::new(db.connection()), &name)?;
    if !profile.enabled {
        anyhow::bail!("profile '{name}' is disabled");
    }
    if profile.connections.is_empty() {
        anyhow::bail!(
            "profile '{name}' has no connections; run `dexo mcp profile set --name {name} --connection <connection>`"
        );
    }
    let saved = ConnectionRepository::new(db.connection());
    let mut connections = Vec::new();
    for connection in &profile.connections {
        let found = saved.get_by_name(connection)?.ok_or_else(|| {
            anyhow::anyhow!("profile '{name}' names unknown connection '{connection}'")
        })?;
        connections.push(McpConnection::from_profile(&found)?);
    }
    drop(db);
    let backend = std::sync::Arc::new(crate::mcp_backend::CliMcpBackend::new(
        registry,
        paths.database.clone(),
    ));
    let ledger = std::sync::Arc::new(SqliteGrantLedger::open(&paths.database)?);
    dexo_mcp::serve_stdio(McpService::new(profile), connections, backend, ledger).await
}
```

Connections now open lazily, on the first tool call that needs one. A server whose database is down still starts, lists its connections and reports `CONNECTION_FAILED` per call. Delete the `CatalogCache` import from `run.rs` if `mcp_serve` was its last user outside `run_inspect`.

- [ ] **Step 12: Protocol harness**

In `crates/dexo-mcp/Cargo.toml` `[dev-dependencies]` add `async-trait.workspace = true` and `dexo-sql = { path = "../dexo-sql" }`. Create `crates/dexo-mcp/tests/common/mod.rs`:

```rust
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dexo_app::mcp::{
    Effect, McpConnection, McpProfile, McpService, MemoryGrantLedger, QueryMode, SelectorRule,
};
use dexo_app::schema_diff::SchemaSnapshot;
use dexo_app::{AppError, Environment, ErrorCategory};
use dexo_driver_api::{CatalogObject, Session};
use dexo_mcp::McpBackend;
use dexo_sql::Dialect;
use dexo_test_support::FakeSession;
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

#[derive(Default)]
pub struct FakeBackend {
    pub sessions: BTreeMap<String, FakeSession>,
    pub catalog: Vec<CatalogObject>,
    pub snapshots: BTreeMap<String, SchemaSnapshot>,
    pub connects: Mutex<Vec<String>>,
    pub fail_next_connect: AtomicBool,
}

impl FakeBackend {
    pub fn with_session(name: &str, session: FakeSession) -> Self {
        let mut backend = Self::default();
        backend.sessions.insert(name.into(), session);
        backend
    }
}

#[async_trait::async_trait]
impl McpBackend for FakeBackend {
    async fn connect(&self, connection: &str) -> Result<Box<dyn Session>, AppError> {
        self.connects.lock().unwrap().push(connection.into());
        if self.fail_next_connect.swap(false, Ordering::SeqCst) {
            return Err(AppError::new(ErrorCategory::Network, "connection refused"));
        }
        self.sessions
            .get(connection)
            .cloned()
            .map(|session| Box::new(session) as Box<dyn Session>)
            .ok_or_else(|| AppError::new(ErrorCategory::Network, "connection refused"))
    }

    async fn catalog_snapshot(&self, _connection: &str) -> Result<Vec<CatalogObject>, AppError> {
        Ok(self.catalog.clone())
    }

    fn schema_snapshot(&self, name: &str) -> Result<Option<SchemaSnapshot>, AppError> {
        Ok(self.snapshots.get(name).cloned())
    }
}

pub fn connection(name: &str) -> McpConnection {
    McpConnection {
        name: name.into(),
        driver: "postgres".into(),
        dialect: Dialect::Postgres,
        database: Some("db".into()),
        default_schema: Some("public".into()),
        environment: Environment::Local,
        read_only: false,
    }
}

/// Enabled, raw reads on, `db.public.*` allowed except `db.public.secrets`.
pub fn profile() -> McpProfile {
    let mut profile = McpProfile::new("assistant");
    profile.enabled = true;
    profile.query_mode = QueryMode::RawReadSql;
    profile.connections = vec!["local".into()];
    profile.selectors = vec![
        SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
        SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
    ];
    profile
}

pub struct Client {
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    write: WriteHalf<DuplexStream>,
    next_id: u64,
    pub notifications: Vec<Value>,
}

impl Client {
    pub async fn start(
        profile: McpProfile,
        connections: Vec<McpConnection>,
        backend: Arc<FakeBackend>,
        ledger: Arc<MemoryGrantLedger>,
    ) -> Self {
        let (client, server) = tokio::io::duplex(1 << 20);
        let (server_read, server_write) = tokio::io::split(server);
        tokio::spawn(dexo_mcp::serve_io(
            McpService::new(profile),
            connections,
            backend,
            ledger,
            server_read,
            server_write,
        ));
        let (read, write) = tokio::io::split(client);
        let mut client = Self {
            lines: BufReader::new(read).lines(),
            write,
            next_id: 0,
            notifications: Vec::new(),
        };
        let init = client
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "test", "version": "0"}
                }),
            )
            .await;
        assert!(init.get("result").is_some(), "{init}");
        client.notify("notifications/initialized", json!({})).await;
        client
    }

    pub async fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await;
    }

    pub async fn send_request(&mut self, method: &str, params: Value) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        id
    }

    /// Every line the server writes must be one JSON-RPC 2.0 message (MCP-001).
    pub async fn read_message(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), self.lines.next_line())
            .await
            .expect("the server answered in time")
            .unwrap()
            .expect("the server is still open");
        let message: Value = serde_json::from_str(&line).expect("stdout carries only JSON-RPC");
        assert_eq!(message["jsonrpc"], "2.0", "{message}");
        message
    }

    pub async fn response(&mut self, id: u64) -> Value {
        loop {
            let message = self.read_message().await;
            if message["id"] == json!(id) {
                return message;
            }
            self.notifications.push(message);
        }
    }

    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.send_request(method, params).await;
        self.response(id).await
    }

    pub async fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
            .await["result"]
            .clone()
    }

    pub async fn tools(&mut self) -> Vec<Value> {
        self.request("tools/list", json!({})).await["result"]["tools"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub async fn tool_names(&mut self) -> Vec<String> {
        self.tools()
            .await
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect()
    }

    /// Waits up to three seconds for a notification with this method.
    pub async fn saw_notification(&mut self, method: &str) -> bool {
        if self
            .notifications
            .iter()
            .any(|message| message["method"] == method)
        {
            return true;
        }
        for _ in 0..20 {
            let next =
                tokio::time::timeout(Duration::from_millis(150), self.lines.next_line()).await;
            if let Ok(Ok(Some(line))) = next {
                let message: Value = serde_json::from_str(&line).unwrap();
                if message["method"] == method {
                    return true;
                }
                self.notifications.push(message);
            }
        }
        false
    }

    async fn send(&mut self, message: Value) {
        self.write
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        self.write.flush().await.unwrap();
    }
}

pub fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

pub fn is_error(result: &Value) -> bool {
    result["isError"] == json!(true)
}
```

Replace `crates/dexo-mcp/tests/protocol.rs` with its first four tests; Tasks 8–15 append the rest:

```rust
mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use common::{Client, FakeBackend, connection, is_error, profile, text};
use dexo_app::mcp::grant::{DEFAULT_TTL_SECS, Grant, GrantCapability};
use dexo_app::mcp::{Effect, GrantLedger, MemoryGrantLedger, SelectorRule};
use dexo_driver_api::DbValue;
use dexo_test_support::FakeSession;
use serde_json::json;

fn users() -> FakeSession {
    FakeSession::with_rows(&["id"], vec![vec![DbValue::I64(1)]])
}

async fn client_with(backend: FakeBackend) -> (Client, Arc<FakeBackend>, Arc<MemoryGrantLedger>) {
    let backend = Arc::new(backend);
    let ledger = Arc::new(MemoryGrantLedger::default());
    let client = Client::start(
        profile(),
        vec![connection("local")],
        Arc::clone(&backend),
        Arc::clone(&ledger),
    )
    .await;
    (client, backend, ledger)
}

#[tokio::test]
async fn tools_are_typed_and_described() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    let tools = client.tools().await;
    let read = tools
        .iter()
        .find(|tool| tool["name"] == "query_execute_read")
        .expect("a raw-read profile lists query_execute_read");
    assert_eq!(read["inputSchema"]["required"], json!(["sql"]));
    assert_eq!(read["annotations"]["readOnlyHint"], json!(true));
    for tool in &tools {
        let description = tool["description"].as_str().unwrap_or_default();
        assert!(description.len() > 20, "{} needs a real description", tool["name"]);
    }
}

#[tokio::test]
async fn a_read_returns_a_table_with_its_columns() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    let result = client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    assert!(!is_error(&result), "{result}");
    assert!(text(&result).starts_with("| id |"), "{}", text(&result));
    assert_eq!(result["structuredContent"]["rows"], json!([["1"]]));
}

#[tokio::test]
async fn a_failed_connect_is_retried_on_the_next_call() {
    let backend = FakeBackend::with_session("local", users());
    backend.fail_next_connect.store(true, Ordering::SeqCst);
    let (mut client, backend, _) = client_with(backend).await;
    let first = client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    assert!(text(&first).starts_with("Error [CONNECTION_FAILED]"), "{first}");
    let second = client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    assert!(!is_error(&second), "{second}");
    assert_eq!(*backend.connects.lock().unwrap(), ["local", "local"]);
}

#[tokio::test]
async fn a_grant_publishes_its_tool_and_revoking_removes_it() {
    let (mut client, _, ledger) = client_with(FakeBackend::with_session("local", users())).await;
    assert!(!client.tool_names().await.contains(&"data_insert".to_string()));
    let grant = Grant::new(
        &profile(),
        "local",
        GrantCapability::DataWrite,
        vec!["data_insert".into()],
        vec![SelectorRule::parse(Effect::Allow, "db.public.users").unwrap()],
        dexo_mcp::tools_write::now_secs(),
        DEFAULT_TTL_SECS,
    )
    .unwrap();
    let id = grant.id;
    ledger.insert_grant(grant).unwrap();
    assert!(client.saw_notification("notifications/tools/list_changed").await);
    let names = client.tool_names().await;
    assert!(names.contains(&"data_insert".to_string()));
    assert!(!names.iter().any(|name| name.starts_with("grant_")));
    ledger.revoke(id).unwrap();
    client.notifications.clear();
    assert!(client.saw_notification("notifications/tools/list_changed").await);
    assert!(!client.tool_names().await.contains(&"data_insert".to_string()));
}
```

- [ ] **Step 13: Run, gate, commit**

Run: `cargo test -p dexo-mcp && cargo test -p dexo-app mcp:: && cargo test -p dexo-cli --test mcp && cargo build -p dexo`
Expected: all pass.

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo deny check
git add -A crates/dexo-mcp crates/dexo-app/src/mcp crates/dexo-cli crates/dexo/tests Cargo.lock
git commit -m "feat(mcp): typed tools on a per-connection router"
```

---

### Task 8: Live catalog tools (MCP-006)

**Files:**
- Modify: `crates/dexo-app/src/mcp/selector.rs` (`Selector::reaches`), `crates/dexo-app/src/mcp/policy.rs` (`reveals`), `crates/dexo-app/src/mcp/service.rs` (`visible`, `READ_TOOLS`)
- Modify: `crates/dexo-test-support/src/session.rs` (catalog reader)
- Modify: `crates/dexo-mcp/src/schema.rs`, `crates/dexo-mcp/src/tools_read.rs`, `crates/dexo-mcp/tests/protocol.rs`

**Interfaces:**
- Produces: `Selector::reaches(&ObjectRef) -> bool`, `ObjectPolicy::reveals(&ObjectRef) -> bool`, `McpService::visible(&CatalogObject) -> bool`.
- Produces: `FakeSession::with_catalog(Vec<CatalogObject>) -> Self`.
- Produces tools: `catalog_list { connection?, parent_id? }`, `catalog_search { connection?, query, limit? }`, `object_describe { connection?, name }`, `object_get_ddl { connection?, name }`, `object_relationships { connection?, name }`.

- [ ] **Step 1: Containers are visible when an allow rule reaches below them**

In `selector.rs`, inside `impl Selector`:

```rust
    /// True when the selector could match something at or below `container`; for
    /// example `db.public.*` reaches `db` and `db.public`.
    pub fn reaches(&self, container: &ObjectRef) -> bool {
        self.segments
            .iter()
            .zip(&container.path)
            .all(|(segment, part)| segment.matches(part))
    }
```

In `policy.rs`, inside `impl ObjectPolicy`:

```rust
    /// A catalog or schema is listed when some allow rule reaches below it and no deny
    /// rule covers it whole.
    pub fn reveals(&self, container: &ObjectRef) -> bool {
        !self
            .rules
            .iter()
            .any(|rule| rule.effect == Effect::Deny && rule.selector.matches(container))
            && self
                .rules
                .iter()
                .any(|rule| rule.effect == Effect::Allow && rule.selector.reaches(container))
    }
```

In `service.rs`, inside `impl McpService` (import `dexo_driver_api::{CatalogObject, ObjectKind}`):

```rust
    pub fn visible(&self, object: &CatalogObject) -> bool {
        let reference = ObjectRef::from_catalog_object(object);
        let policy = self.policy();
        match &object.kind {
            ObjectKind::Catalog | ObjectKind::Schema => policy.reveals(&reference),
            _ => policy.decide(&reference) == Decision::Allow,
        }
    }
```

and add `"catalog_list", "catalog_search", "object_describe", "object_get_ddl", "object_relationships"` to `READ_TOOLS`, after `"list_connections"`.

Add to the `service.rs` tests:

```rust
    #[test]
    fn denied_tables_and_their_columns_are_invisible_but_their_schema_is_not() {
        use dexo_driver_api::ObjectKind::{Column, Schema, Table};
        use dexo_driver_api::{CatalogObject, ObjectId, QualifiedName};
        let service = raw_service(10, 1024);
        let object = |kind, name: &str| {
            CatalogObject::new(
                ObjectId::new(name),
                kind,
                QualifiedName::new(Some("db"), Some("public"), name),
                None,
            )
        };
        assert!(service.visible(&object(Schema, "public")));
        assert!(service.visible(&object(Table, "users")));
        assert!(!service.visible(&object(Table, "secrets")));
        assert!(!service.visible(&object(Column, "secrets.email")));
        assert!(!service.visible(&CatalogObject::new(
            ObjectId::new("other"),
            Schema,
            QualifiedName::new(Some("db"), Some("other"), "other"),
            None,
        )));
    }
```

- [ ] **Step 2: `FakeSession` answers catalog calls**

In `crates/dexo-test-support/src/session.rs`, add a field `catalog: Vec<CatalogObject>` to `FakeSession`, extend the `dexo_driver_api` import with `CatalogList, CatalogListOptions, CatalogObject, CatalogReader, ObjectDdl, ObjectId`, add to `impl FakeSession`:

```rust
    pub fn with_catalog(mut self, catalog: Vec<CatalogObject>) -> Self {
        self.catalog = catalog;
        self
    }
```

add `fn catalog(&self) -> Option<&dyn CatalogReader> { Some(self) }` to `impl Session for FakeSession`, and add:

```rust
#[async_trait::async_trait]
impl CatalogReader for FakeSession {
    async fn list_children(
        &self,
        parent: Option<&ObjectId>,
        _options: &CatalogListOptions,
    ) -> Result<CatalogList, DriverError> {
        Ok(CatalogList {
            objects: self
                .catalog
                .iter()
                .filter(|object| object.parent.as_ref() == parent)
                .cloned()
                .collect(),
            restrictions: Vec::new(),
        })
    }

    async fn object(&self, id: &ObjectId) -> Result<Option<CatalogObject>, DriverError> {
        Ok(self.catalog.iter().find(|object| &object.id == id).cloned())
    }

    async fn ddl(&self, id: &ObjectId) -> Result<ObjectDdl, DriverError> {
        Ok(ObjectDdl {
            object_id: id.clone(),
            sql: format!("-- ddl of {}", id.as_str()),
        })
    }

    async fn dependencies(&self, _id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        Ok(Vec::new())
    }

    async fn dependents(&self, _id: &ObjectId) -> Result<Vec<ObjectId>, DriverError> {
        Ok(Vec::new())
    }
}
```

- [ ] **Step 3: Inputs**

Append to `crates/dexo-mcp/src/schema.rs`:

```rust
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CatalogListInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// `id` of the node whose children to list, from an earlier catalog_list or catalog_search; omit for the roots.
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CatalogSearchInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Part of a table, view or column name.
    pub query: String,
    /// At most this many hits (default 50, at most 200).
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ObjectInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Qualified name such as `public.orders` or `db.public.orders`; a bare name uses the connection's default database and schema.
    pub name: String,
}
```

- [ ] **Step 4: Tools**

In `crates/dexo-mcp/src/tools_read.rs`, extend the imports:

```rust
use dexo_app::mcp::selector::ObjectRef;
use dexo_app::mcp::{Decision, McpService};
use dexo_app::{CatalogService, ErrorCategory, SearchService, map_driver_error};
use dexo_driver_api::{
    CatalogListOptions, CatalogObject, CatalogReader, ObjectId, ObjectKind, Session,
};
use serde_json::Value;

use crate::error::hidden;
use crate::schema::{CatalogListInput, CatalogSearchInput, ObjectInput};
```

Add these free functions below `finish`:

```rust
fn objects_result(objects: &[CatalogObject]) -> CallToolResult {
    let rows = objects
        .iter()
        .map(|object| {
            vec![
                object.id.as_str().to_string(),
                format!("{:?}", object.kind),
                object.qualified_name.display_unquoted(),
            ]
        })
        .collect();
    rows_result(&RowsPage::new(
        ["id", "kind", "name"].map(String::from).to_vec(),
        rows,
    ))
}

fn catalog_of(session: &dyn Session) -> Result<&dyn CatalogReader, AppError> {
    session.catalog().ok_or_else(|| {
        AppError::new(
            ErrorCategory::Capability,
            "this connection has no catalog reader",
        )
    })
}

async fn list(
    service: &McpService,
    session: &dyn Session,
    parent: Option<ObjectId>,
) -> Result<CallToolResult, AppError> {
    let reader = catalog_of(session)?;
    let listed =
        CatalogService::list_children(reader, parent.as_ref(), &CatalogListOptions::default())
            .await?;
    let visible: Vec<CatalogObject> = listed
        .objects
        .into_iter()
        .filter(|object| service.visible(object))
        .collect();
    Ok(objects_result(&visible))
}

/// Finds an object by the name the client typed, completed with the connection's
/// defaults. A denied object and a missing one answer the same way.
async fn find_visible(
    service: &McpService,
    reader: &dyn CatalogReader,
    target: &ObjectRef,
) -> Result<CatalogObject, AppError> {
    if service.policy().decide(target) != Decision::Allow {
        return Err(hidden());
    }
    CatalogService::find_by_qualified_name(
        reader,
        &target.to_string(),
        &CatalogListOptions::default(),
    )
    .await?
    .filter(|object| service.visible(object))
    .ok_or_else(hidden)
}

async fn describe(
    service: &McpService,
    session: &dyn Session,
    target: &ObjectRef,
) -> Result<CallToolResult, AppError> {
    let reader = catalog_of(session)?;
    let object = find_visible(service, reader, target).await?;
    let children =
        CatalogService::list_children(reader, Some(&object.id), &CatalogListOptions::default())
            .await?;
    let keys = match session.data() {
        Some(data) => data
            .table_columns(&object.qualified_name)
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let rows = children
        .objects
        .iter()
        .filter(|child| child.kind == ObjectKind::Column)
        .map(|column| {
            let full = column.qualified_name.object();
            let name = full.split_once('.').map_or(full, |(_, name)| name).to_string();
            let role = match keys.iter().find(|key| key.name == name) {
                Some(key) if key.primary_key => "primary key",
                Some(key) if key.unique => "unique",
                _ => "",
            };
            let kind = column
                .attributes
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            vec![name, kind, role.to_string()]
        })
        .collect();
    let mut page = RowsPage::new(["column", "type", "key"].map(String::from).to_vec(), rows);
    page.title = Some(format!(
        "### {} ({:?})",
        object.qualified_name.display_unquoted(),
        object.kind
    ));
    Ok(rows_result(&page))
}

async fn ddl(
    service: &McpService,
    session: &dyn Session,
    target: &ObjectRef,
) -> Result<CallToolResult, AppError> {
    let reader = catalog_of(session)?;
    let object = find_visible(service, reader, target).await?;
    let ddl = CatalogService::ddl(reader, &object.id).await?;
    Ok(text_result(ddl.sql.clone(), json!({ "sql": ddl.sql })))
}

async fn relationships(
    service: &McpService,
    session: &dyn Session,
    target: &ObjectRef,
) -> Result<CallToolResult, AppError> {
    let reader = catalog_of(session)?;
    let object = find_visible(service, reader, target).await?;
    let dependencies = reader
        .dependencies(&object.id)
        .await
        .map_err(map_driver_error)?;
    let dependents = reader
        .dependents(&object.id)
        .await
        .map_err(map_driver_error)?;
    let mut rows = Vec::new();
    for (direction, ids) in [("depends on", dependencies), ("used by", dependents)] {
        for id in ids {
            if let Some(related) = CatalogService::object(reader, &id).await?
                && service.visible(&related)
            {
                rows.push(vec![
                    direction.to_string(),
                    format!("{:?}", related.kind),
                    related.qualified_name.display_unquoted(),
                ]);
            }
        }
    }
    Ok(rows_result(&RowsPage::new(
        ["relation", "kind", "name"].map(String::from).to_vec(),
        rows,
    )))
}
```

and these tools inside the `#[tool_router(router = read_tools, …)]` block:

```rust
    /// Children of a catalog node, live from the server: databases, then schemas, then tables and views, then columns and indexes. Omit `parent_id` for the roots. Objects outside the allowlist are not listed.
    #[tool(annotations(read_only_hint = true))]
    async fn catalog_list(&self, Parameters(input): Parameters<CatalogListInput>) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let parent = input.parent_id.map(ObjectId::new);
        let outcome = list(&self.inner.service, lease.session(), parent).await;
        finish(&mut lease, outcome)
    }

    /// Search table, view and column names in the connection's indexed catalog. The first search on a connection indexes it, which can take a while on a large server.
    #[tool(annotations(read_only_hint = true))]
    async fn catalog_search(&self, Parameters(input): Parameters<CatalogSearchInput>) -> CallToolResult {
        let slot = match self.slot(input.connection.as_deref()) {
            Ok(slot) => slot,
            Err(result) => return result,
        };
        let objects = match self
            .inner
            .router
            .backend()
            .catalog_snapshot(&slot.meta.name)
            .await
        {
            Ok(objects) => objects,
            Err(error) => return app_error(&error),
        };
        let visible: Vec<CatalogObject> = objects
            .into_iter()
            .filter(|object| self.inner.service.visible(object))
            .collect();
        let limit = input.limit.unwrap_or(50).clamp(1, 200) as usize;
        let hits: Vec<CatalogObject> = SearchService::from_objects(visible)
            .search(&input.query)
            .into_iter()
            .map(|hit| hit.object)
            .take(limit)
            .collect();
        objects_result(&hits)
    }

    /// Columns (with type and key role) of one table or view.
    #[tool(annotations(read_only_hint = true))]
    async fn object_describe(&self, Parameters(input): Parameters<ObjectInput>) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let target = lease.meta.qualify(&ObjectRef::parse(&input.name).path);
        let outcome = describe(&self.inner.service, lease.session(), &target).await;
        finish(&mut lease, outcome)
    }

    /// The CREATE statement of one object, as the server reports it.
    #[tool(annotations(read_only_hint = true))]
    async fn object_get_ddl(&self, Parameters(input): Parameters<ObjectInput>) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let target = lease.meta.qualify(&ObjectRef::parse(&input.name).path);
        let outcome = ddl(&self.inner.service, lease.session(), &target).await;
        finish(&mut lease, outcome)
    }

    /// Objects this one depends on and objects that depend on it, limited to what the allowlist shows.
    #[tool(annotations(read_only_hint = true))]
    async fn object_relationships(&self, Parameters(input): Parameters<ObjectInput>) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let target = lease.meta.qualify(&ObjectRef::parse(&input.name).path);
        let outcome = relationships(&self.inner.service, lease.session(), &target).await;
        finish(&mut lease, outcome)
    }
```

- [ ] **Step 5: Protocol test for MCP-006**

Append to `crates/dexo-mcp/tests/protocol.rs`:

```rust
fn catalog() -> Vec<dexo_driver_api::CatalogObject> {
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    let node = |id: &str, kind, name: QualifiedName, parent: Option<&str>| {
        CatalogObject::new(ObjectId::new(id), kind, name, parent.map(ObjectId::new))
    };
    let public = |object: &str| QualifiedName::new(Some("db"), Some("public"), object);
    vec![
        node(
            "db",
            ObjectKind::Catalog,
            QualifiedName::new(Some("db"), None::<String>, "db"),
            None,
        ),
        node("public", ObjectKind::Schema, public("public"), Some("db")),
        node("users", ObjectKind::Table, public("users"), Some("public")),
        node("secrets", ObjectKind::Table, public("secrets"), Some("public")),
        node("secrets.email", ObjectKind::Column, public("secrets.email"), Some("secrets")),
        node("users.id", ObjectKind::Column, public("users.id"), Some("users"))
            .with_attribute("type", json!("int4")),
    ]
}

#[tokio::test]
async fn a_denied_table_is_invisible_to_every_catalog_tool() {
    let mut backend = FakeBackend::with_session("local", users().with_catalog(catalog()));
    backend.catalog = catalog();
    let (mut client, _, _) = client_with(backend).await;
    let listed = client.call("catalog_list", json!({"parent_id": "public"})).await;
    assert!(text(&listed).contains("db.public.users"));
    assert!(!text(&listed).contains("secrets"), "{}", text(&listed));
    let columns = client.call("catalog_list", json!({"parent_id": "secrets"})).await;
    assert!(!text(&columns).contains("email"));
    let search = client.call("catalog_search", json!({"query": "secr"})).await;
    assert!(!text(&search).contains("secrets"));
    for tool in ["object_describe", "object_get_ddl", "object_relationships"] {
        let hidden = client.call(tool, json!({"name": "secrets"})).await;
        assert_eq!(text(&hidden), "Error [NOT_FOUND]: not found", "{tool}");
    }
    let described = client.call("object_describe", json!({"name": "users"})).await;
    assert!(text(&described).contains("| id | int4 |"), "{}", text(&described));
}
```

- [ ] **Step 6: Run, gate, commit**

Run: `cargo test -p dexo-mcp && cargo test -p dexo-app mcp::`
Expected: all pass.

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-app/src/mcp crates/dexo-test-support crates/dexo-mcp
git commit -m "feat(mcp): live catalog tools behind the allowlist"
```

---

### Task 9: Paginated `data_read` (MCP-007, MCP-013)

This tool works in structured-only profiles, so an assistant can look at data without raw SQL.

**Files:**
- Modify: `crates/dexo-app/src/mcp/service.rs` (`read_page`, `ReadResult::next_offset`, `READ_TOOLS`)
- Modify: `crates/dexo-mcp/src/schema.rs`, `crates/dexo-mcp/src/tools_read.rs`, `crates/dexo-mcp/tests/protocol.rs`

**Interfaces:**
- Produces: `ReadResult.next_offset: Option<u64>`.
- Produces: `McpService::read_page(&self, session: &dyn Session, connection: &McpConnection, target: &ObjectRef, offset: u64, limit: Option<u32>, cancel: &CancellationToken) -> Result<ReadResult, AppError>`.
- Produces tool: `data_read { connection?, table, offset?, limit? }`.

- [ ] **Step 1: The app service pages through the driver**

In `service.rs`, add `pub next_offset: Option<u64>,` to `ReadResult`, extend the `dexo_driver_api` import with `DataRequest, Page`, add `"data_read"` to `READ_TOOLS` (not to `RAW_SQL_TOOLS`), and add to `impl McpService`:

```rust
    /// One page of a table through the driver's typed paging, the same path the grid
    /// uses. `next_offset` is set when there is more to read or a limit cut the page.
    pub async fn read_page(
        &self,
        session: &dyn Session,
        connection: &McpConnection,
        target: &ObjectRef,
        offset: u64,
        limit: Option<u32>,
        cancel: &CancellationToken,
    ) -> Result<ReadResult, AppError> {
        if self.policy().decide(target) != Decision::Allow {
            return Err(hidden());
        }
        let data = session.data().ok_or_else(|| {
            AppError::new(
                ErrorCategory::Capability,
                "this connection cannot page table data",
            )
        })?;
        let cap = u32::try_from(self.profile.limits.max_rows)
            .unwrap_or(Page::MAX_LIMIT)
            .min(Page::MAX_LIMIT);
        let page =
            Page::new(offset, limit.unwrap_or(100).clamp(1, cap)).map_err(map_driver_error)?;
        let request = DataRequest {
            object: connection.qualified_name(target),
            columns: Vec::new(),
            filter: None,
            sort: Vec::new(),
            page,
        };
        let fetched = tokio::select! {
            () = cancel.cancelled() => {
                return Err(AppError::new(ErrorCategory::Cancelled, "cancelled by the client"));
            }
            fetched = data.fetch(request) => fetched.map_err(map_driver_error)?,
        };
        let mut result = ReadResult {
            columns: fetched.columns.into_iter().map(|column| column.name).collect(),
            ..ReadResult::default()
        };
        for row in fetched.rows {
            let cells: Vec<String> = row.iter().map(display_value).collect();
            let size: u64 = cells.iter().map(|cell| cell.len() as u64).sum();
            if result.bytes + size > self.profile.limits.max_bytes {
                result.truncated = true;
                break;
            }
            result.bytes += size;
            result.rows.push(cells);
        }
        result.next_offset = (fetched.has_more || result.truncated)
            .then(|| offset + result.rows.len() as u64);
        Ok(result)
    }
```

`DataRequest` with empty `columns` selects every column; the Postgres driver already does this (`crates/dexo-driver-postgres/src/mutation.rs:103`).

- [ ] **Step 2: Tool**

Append to `schema.rs`:

```rust
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DataReadInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
    /// Table or view to read, qualified like object_describe.
    pub table: String,
    /// Row offset of the page (default 0); use the `next_offset` of the previous page.
    pub offset: Option<u64>,
    /// Rows per page (default 100, capped by the profile's max_rows).
    pub limit: Option<u32>,
}
```

Add to the read tool router block (import `DataReadInput`):

```rust
    /// One page of a table or view, without writing SQL. Works in structured-only profiles. Follow `next_offset` for the next page.
    #[tool(annotations(read_only_hint = true))]
    async fn data_read(
        &self,
        Parameters(input): Parameters<DataReadInput>,
        cancel: CancellationToken,
    ) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let target = lease.meta.qualify(&ObjectRef::parse(&input.table).path);
        let started = Instant::now();
        let outcome = self
            .inner
            .service
            .read_page(
                lease.session(),
                lease.meta,
                &target,
                input.offset.unwrap_or(0),
                input.limit,
                &cancel,
            )
            .await
            .map(|result| {
                rows_result(&RowsPage {
                    columns: result.columns,
                    rows: result.rows,
                    truncated: result.truncated,
                    bytes: result.bytes,
                    elapsed: started.elapsed(),
                    next_offset: result.next_offset,
                    title: None,
                })
            });
        finish(&mut lease, outcome)
    }
```

- [ ] **Step 3: Protocol test**

Append to `protocol.rs`:

```rust
#[tokio::test]
async fn data_read_pages_and_says_where_to_continue() {
    let rows = (1..=5).map(|id| vec![DbValue::I64(id)]).collect();
    let (mut client, _, _) = client_with(FakeBackend::with_session(
        "local",
        FakeSession::with_rows(&["id"], rows),
    ))
    .await;
    let first = client.call("data_read", json!({"table": "users", "limit": 2})).await;
    assert_eq!(first["structuredContent"]["rows"], json!([["1"], ["2"]]));
    assert_eq!(first["structuredContent"]["next_offset"], json!(2));
    let last = client
        .call("data_read", json!({"table": "users", "offset": 4, "limit": 2}))
        .await;
    assert_eq!(last["structuredContent"]["rows"], json!([["5"]]));
    assert_eq!(last["structuredContent"]["next_offset"], json!(null));
    let hidden = client.call("data_read", json!({"table": "secrets"})).await;
    assert_eq!(text(&hidden), "Error [NOT_FOUND]: not found");
}
```

- [ ] **Step 4: Run, gate, commit**

Run: `cargo test -p dexo-mcp && cargo test -p dexo-app mcp::`

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-app/src/mcp crates/dexo-mcp
git commit -m "feat(mcp): page table data without raw SQL"
```

---

### Task 10: `schema_diff` on saved snapshots (MCP-009, F10)

A read-only comparison of two snapshots saved with `dexo schema snapshot --connection C --name N`. Applying a sync stays out: COMPARE-023 is absent (Part C).

**Files:**
- Modify: `crates/dexo-app/src/mcp/service.rs` (`READ_TOOLS`)
- Modify: `crates/dexo-mcp/src/schema.rs`, `crates/dexo-mcp/src/tools_read.rs`, `crates/dexo-mcp/tests/protocol.rs`

**Interfaces:**
- Produces tool: `schema_diff { from_snapshot, to_snapshot }`.

- [ ] **Step 1: Tool**

Add `"schema_diff"` to `READ_TOOLS`. Append to `schema.rs`:

```rust
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DiffInput {
    /// Name of the older snapshot, saved with `dexo schema snapshot --connection C --name N`.
    pub from_snapshot: String,
    /// Name of the newer snapshot.
    pub to_snapshot: String,
}
```

In `tools_read.rs` import `dexo_app::schema_diff::{SchemaDifference, plan_migration, render_unquoted}` and `crate::schema::DiffInput`, and add to the read router:

```rust
    /// Differences between two saved schema snapshots, limited to objects this profile may see. Read-only: nothing is applied.
    #[tool(annotations(read_only_hint = true))]
    async fn schema_diff(&self, Parameters(input): Parameters<DiffInput>) -> CallToolResult {
        let backend = self.inner.router.backend();
        let load = |name: &str| {
            backend
                .schema_snapshot(name)
                .and_then(|snapshot| snapshot.ok_or_else(hidden))
        };
        let (from, to) = match (load(&input.from_snapshot), load(&input.to_snapshot)) {
            (Ok(from), Ok(to)) => (from, to),
            (Err(error), _) | (_, Err(error)) => return app_error(&error),
        };
        let (changes, _, _) = plan_migration(&from, &to, &[], render_unquoted);
        let service = &self.inner.service;
        let rows = changes
            .iter()
            .filter(|change| match change {
                SchemaDifference::Added(object) | SchemaDifference::Removed(object) => {
                    service.visible(object)
                }
                SchemaDifference::Changed { before, after } => {
                    service.visible(before) && service.visible(after)
                }
            })
            .map(|change| {
                let kind = match change {
                    SchemaDifference::Added(_) => "added",
                    SchemaDifference::Removed(_) => "removed",
                    SchemaDifference::Changed { .. } => "changed",
                };
                vec![kind.to_string(), change.object_name()]
            })
            .collect();
        rows_result(&RowsPage::new(
            ["change", "object"].map(String::from).to_vec(),
            rows,
        ))
    }
```

The migration script is deliberately not returned: it would carry DDL for objects the filter hides.

- [ ] **Step 2: Protocol test**

`SchemaSnapshot::capture` takes `(driver, server_version, captured_at, scope, objects)` (`crates/dexo-app/src/schema_diff/snapshot.rs:38`).

```rust
#[tokio::test]
async fn schema_diff_hides_denied_objects() {
    use dexo_app::schema_diff::SchemaSnapshot;
    let table = |name: &str| {
        dexo_driver_api::CatalogObject::new(
            dexo_driver_api::ObjectId::new(name),
            dexo_driver_api::ObjectKind::Table,
            dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    };
    let snapshot =
        |objects| SchemaSnapshot::capture("postgres", "16", "2026-09-23T00:00:00Z", "db", objects);
    let mut backend = FakeBackend::with_session("local", users());
    backend
        .snapshots
        .insert("before".into(), snapshot(vec![table("users")]));
    backend.snapshots.insert(
        "after".into(),
        snapshot(vec![table("users"), table("orders"), table("secrets")]),
    );
    let (mut client, _, _) = client_with(backend).await;
    let diff = client
        .call(
            "schema_diff",
            json!({"from_snapshot": "before", "to_snapshot": "after"}),
        )
        .await;
    assert!(text(&diff).contains("| added | db.public.orders |"), "{}", text(&diff));
    assert!(!text(&diff).contains("secrets"));
    let missing = client
        .call(
            "schema_diff",
            json!({"from_snapshot": "nope", "to_snapshot": "after"}),
        )
        .await;
    assert_eq!(text(&missing), "Error [NOT_FOUND]: not found");
}
```

- [ ] **Step 3: Run, gate, commit**

Run: `cargo test -p dexo-mcp`

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-app/src/mcp crates/dexo-mcp
git commit -m "feat(mcp): compare saved schema snapshots"
```

---

### Task 11: Row writes use the table's real keys (MCP-008, META-001, F9)

**Files:**
- Modify: `crates/dexo-app/src/data/change_set.rs` (`TableMeta::from_keys`)
- Modify: `crates/dexo-tui/src/update.rs:814-831` (use it)
- Modify: `crates/dexo-mcp/src/tools_write.rs` (`data_mutations`)

**Interfaces:**
- Produces: `TableMeta::from_keys(Vec<ColumnKeyInfo>) -> TableMeta`, the one adapter-neutral way to build editing metadata.
- Changes: `data_mutations` becomes `async fn data_mutations(name: &str, value: &Value, session: &dyn Session, connection: &McpConnection) -> Result<Vec<Mutation>, AppError>`.

- [ ] **Step 1: One editing model for every adapter**

In `crates/dexo-app/src/data/change_set.rs` (import `dexo_driver_api::ColumnKeyInfo`):

```rust
impl TableMeta {
    /// Editing metadata from the driver's key report. The TUI grid and MCP writes both
    /// build it here, so they agree on which columns identify a row (META-001).
    pub fn from_keys(keys: Vec<ColumnKeyInfo>) -> Self {
        Self {
            columns: keys
                .into_iter()
                .map(|key| ColumnDef {
                    name: key.name,
                    primary_key: key.primary_key,
                    unique: key.unique,
                    nullable: true,
                })
                .collect(),
        }
    }
}
```

In `crates/dexo-tui/src/update.rs`, in the `Action::TableColumnsLoaded` arm, replace the literal `dexo_app::data::TableMeta { columns: … }` with `dexo_app::data::TableMeta::from_keys(columns)`.

- [ ] **Step 2: MCP asks the table for its keys**

In `crates/dexo-mcp/src/tools_write.rs`, replace `data_mutations` with:

```rust
/// Builds the mutations from the table's real primary or unique key, never from the
/// order the client happened to list its columns in.
async fn data_mutations(
    name: &str,
    value: &Value,
    session: &dyn Session,
    connection: &McpConnection,
) -> Result<Vec<Mutation>, AppError> {
    let target = value
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let table = connection.qualified_name(&connection.qualify(&ObjectRef::parse(target).path));
    let data = session
        .data()
        .ok_or_else(|| AppError::new(ErrorCategory::Capability, "data writer unavailable"))?;
    let meta = TableMeta::from_keys(data.table_columns(&table).await.map_err(map_driver_error)?);
    let key = RowIdentity::from_table(&meta).ok_or_else(|| {
        AppError::new(
            ErrorCategory::Capability,
            format!("{target} has no primary key or unique identity; it cannot be edited over MCP"),
        )
    })?;
    let values = object_pairs(value.get("values"));
    let identity = object_pairs(value.get("identity"));
    let mut changes = ChangeSet::for_table(&meta);
    match name {
        "data_insert" => changes.insert(values),
        "data_update" | "data_delete" => {
            let mut supplied: Vec<&str> =
                identity.iter().map(|(column, _)| column.as_str()).collect();
            let mut expected: Vec<&str> = key.iter().map(String::as_str).collect();
            supplied.sort_unstable();
            expected.sort_unstable();
            if supplied != expected {
                return Err(AppError::new(
                    ErrorCategory::Configuration,
                    format!("identity must name exactly the key columns: {}", key.join(", ")),
                ));
            }
            let row = RowIdentity {
                columns: identity.iter().map(|(column, _)| column.clone()).collect(),
                values: identity.iter().map(|(_, value)| value.clone()).collect(),
            };
            if name == "data_update" {
                changes.update(row, identity.clone(), values);
            } else {
                changes.delete(row, identity);
            }
        }
        _ => {}
    }
    if let Some(error) = changes.errors().first() {
        return Err(AppError::new(ErrorCategory::Configuration, error.clone()));
    }
    mutations_for(table, &changes)
        .map_err(|error| AppError::new(ErrorCategory::Configuration, error.to_string()))
}
```

In `execute`, change the data arm to `let mutations = data_mutations(name, value, session, connection).await?;`. Remove `ColumnDef` and `parse_qualified` from the imports if the compiler reports them unused.

- [ ] **Step 3: Tests**

Append to the `tools_write` test module:

```rust
    fn keyed() -> FakeSession {
        FakeSession::default().with_keys(vec![dexo_driver_api::ColumnKeyInfo {
            name: "id".into(),
            primary_key: true,
            unique: true,
        }])
    }

    #[tokio::test]
    async fn updates_must_name_the_real_key() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(GrantCapability::DataWrite, "data_update", "db.public.items"))
            .unwrap();
        let session = keyed();
        let wrong = call_on(
            &ledger,
            &session,
            "local",
            "data_update",
            json!({"operation_id":"op-u1","target":"db.public.items","identity":{"name":"x"},"values":{"name":"y"}}),
        )
        .await
        .unwrap_err();
        assert!(wrong.to_string().contains("key columns: id"), "{wrong}");
        assert!(!session.log().iter().any(|entry| entry.starts_with("apply")));
    }

    #[tokio::test]
    async fn keyless_tables_are_not_editable() {
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(grant(GrantCapability::DataWrite, "data_insert", "db.public.items"))
            .unwrap();
        let error = call(
            &ledger,
            "data_insert",
            json!({"operation_id":"op-k","target":"db.public.items","values":{"name":"x"}}),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("no primary key"), "{error}");
    }
```

- [ ] **Step 4: Run, gate, commit**

Run: `cargo test -p dexo-mcp && cargo test -p dexo-app data:: && cargo build -p dexo-tui`

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-app/src/data crates/dexo-tui/src/update.rs crates/dexo-mcp
git commit -m "fix(mcp): row writes use the table's real keys"
```

---

### Task 12: Admin read, separate from admin write (MCP-011, MCP-017)

**Files:**
- Modify: `crates/dexo-app/src/mcp/service.rs` (`OPT_IN_TOOLS`, `advertised_tools`, `known_tools`, `READ_TOOLS`)
- Modify: `crates/dexo-mcp/src/schema.rs`, `crates/dexo-mcp/src/tools_read.rs`, `crates/dexo-mcp/tests/protocol.rs`

**Interfaces:**
- Produces tool: `admin_list_sessions { connection? }`, listed only when the profile has the rule `--allow-tool admin_list_sessions`. Query text is never returned.

- [ ] **Step 1: Opt-in tools**

In `service.rs`, add `"admin_list_sessions"` to `READ_TOOLS`, add

```rust
/// Read tools that show other people's activity; listed only with an explicit allow rule.
const OPT_IN_TOOLS: &[&str] = &["admin_list_sessions"];
```

and change the last filter of `advertised_tools` to:

```rust
        .filter(|name| {
            if OPT_IN_TOOLS.contains(name) {
                profile
                    .tool_rules
                    .iter()
                    .any(|rule| rule.tool == *name && rule.allowed)
            } else {
                profile.tool_allowed(name)
            }
        })
```

In `known_tools`, add `tools.extend(OPT_IN_TOOLS);` before the write tools: its probe profile has no rules, so `advertised_tools` alone would leave opt-in names out.

- [ ] **Step 2: Tool**

Append to `schema.rs`:

```rust
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AdminListInput {
    /// Connection name from `list_connections`; optional when the profile has exactly one.
    pub connection: Option<String>,
}
```

In `tools_read.rs`, add:

```rust
async fn sessions(session: &dyn Session) -> Result<CallToolResult, AppError> {
    let admin = session.admin().ok_or_else(|| {
        AppError::new(
            ErrorCategory::Capability,
            "administration is unavailable on this connection",
        )
    })?;
    let list = admin.list_sessions().await.map_err(map_driver_error)?;
    let rows = list
        .items
        .into_iter()
        .map(|item| {
            vec![
                item.id,
                item.user.unwrap_or_default(),
                item.database.unwrap_or_default(),
                item.state,
                item.duration_ms
                    .map(|ms| ms.to_string())
                    .unwrap_or_default(),
            ]
        })
        .collect();
    let mut page = RowsPage::new(
        ["session_id", "user", "database", "state", "duration_ms"]
            .map(String::from)
            .to_vec(),
        rows,
    );
    page.title = list
        .restriction
        .map(|reason| format!("Partial list: {reason}"));
    Ok(rows_result(&page))
}
```

and in the read router (import `AdminListInput`):

```rust
    /// Server sessions: id, user, database, state and duration. Query text is not returned. Listed only when the profile allows it explicitly.
    #[tool(annotations(read_only_hint = true))]
    async fn admin_list_sessions(&self, Parameters(input): Parameters<AdminListInput>) -> CallToolResult {
        let mut lease = match self.open(input.connection.as_deref()).await {
            Ok(lease) => lease,
            Err(result) => return result,
        };
        let outcome = sessions(lease.session()).await;
        finish(&mut lease, outcome)
    }
```

- [ ] **Step 3: Protocol test**

```rust
#[tokio::test]
async fn admin_reads_need_an_explicit_rule() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    assert!(!client.tool_names().await.contains(&"admin_list_sessions".to_string()));
    let hidden = client.call("admin_list_sessions", json!({})).await;
    assert_eq!(text(&hidden), "Error [NOT_FOUND]: not found");

    let mut allowed = profile();
    allowed.tool_rules.push(dexo_app::mcp::ToolRule {
        tool: "admin_list_sessions".into(),
        allowed: true,
    });
    let mut client = Client::start(
        allowed,
        vec![connection("local")],
        Arc::new(FakeBackend::with_session("local", users())),
        Arc::new(MemoryGrantLedger::default()),
    )
    .await;
    assert!(client.tool_names().await.contains(&"admin_list_sessions".to_string()));
    let unsupported = client.call("admin_list_sessions", json!({})).await;
    assert!(text(&unsupported).starts_with("Error [UNSUPPORTED]"));
}
```

`FakeSession` has no administration provider, so the last call proves the gate opened and the tool reached the session.

- [ ] **Step 4: Run, gate, commit**

Run: `cargo test -p dexo-mcp && cargo test -p dexo-cli --test mcp`

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-app/src/mcp crates/dexo-mcp
git commit -m "feat(mcp): opt-in admin session list, separate from admin actions"
```

---

### Task 13: Audit every call (MCP-012, SEC-015, SEC-016, F16)

**Files:**
- Modify: `crates/dexo-mcp/src/server.rs` (`call_tool`, `new`)
- Modify: `crates/dexo-mcp/src/tools_write.rs` (grant events are tagged `grant <tool>`)
- Modify: `crates/dexo-cli/src/run.rs` (`mcp_audit` stops pruning)
- Modify: `crates/dexo-mcp/tests/protocol.rs`

**Interfaces:**
- Every `tools/call` writes one `AuditEvent { request: "tools/call <tool>", decision: "allow" | "deny", status: "ok" | <error code>, duration_ms, rows, bytes, target: "<connection>:<object>", operation_id: <operation_id or "rpc:<id>">, sql: <hash> }`. Grant decisions inside write tools keep their own events, with `request: "grant <tool>"` and the grant id; the operation id correlates both (SEC-016).
- The server prunes events older than the profile's `audit_retention_days` when it starts.

- [ ] **Step 1: Wrap the dispatch**

In `server.rs`, import `std::time::Instant`, `dexo_app::mcp::audit::{AuditEvent, SqlAuditMode}`, `rmcp::model::JsonObject` and `serde_json::Value`. Replace `call_tool` with:

```rust
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let tool = request.name.to_string();
        let arguments = request.arguments.clone().unwrap_or_default();
        let request_id = serde_json::to_string(&context.id).unwrap_or_default();
        let started = Instant::now();
        if !self.visible_tools().contains(&tool) {
            let denied: CallToolResponse = tool_error("NOT_FOUND", HIDDEN).into();
            self.audit(&tool, &arguments, &request_id, "deny", Some(&denied), started);
            return Ok(denied);
        }
        let Ok(_permit) = self.inner.calls.try_acquire() else {
            let busy: CallToolResponse = tool_error(
                "BUSY",
                "too many calls in flight for this profile; retry shortly",
            )
            .into();
            self.audit(&tool, &arguments, &request_id, "deny", Some(&busy), started);
            return Ok(busy);
        };
        let response = self
            .tool_router
            .call(ToolCallContext::new(self, request, context))
            .await;
        self.audit(
            &tool,
            &arguments,
            &request_id,
            "allow",
            response.as_ref().ok(),
            started,
        );
        response
    }
```

and add to `impl DexoMcpServer`:

```rust
    fn audit(
        &self,
        tool: &str,
        arguments: &JsonObject,
        request_id: &str,
        decision: &str,
        response: Option<&CallToolResponse>,
        started: Instant,
    ) {
        let result = match response {
            Some(CallToolResponse::Complete(result)) => Some(result),
            _ => None,
        };
        let data = result.and_then(|result| result.structured_content.as_ref());
        let count = |key: &str| {
            data.and_then(|data| data.get(key))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let status = match result {
            Some(result) if result.is_error == Some(true) => result
                .content
                .first()
                .and_then(|block| block.as_text())
                .and_then(|text| text.text.strip_prefix("Error ["))
                .and_then(|rest| rest.split_once(']'))
                .map_or("error", |(code, _)| code)
                .to_string(),
            Some(_) => "ok".to_string(),
            None => "incomplete".to_string(),
        };
        let field = |key: &str| arguments.get(key).and_then(Value::as_str);
        let target = [
            field("connection"),
            field("target").or(field("table")).or(field("name")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(":");
        self.inner.ledger.record_audit(
            AuditEvent {
                timestamp: now_secs(),
                request: format!("tools/call {tool}"),
                operation_id: field("operation_id")
                    .map(str::to_string)
                    .or_else(|| Some(format!("rpc:{request_id}"))),
                profile: self.inner.service.profile.name.clone(),
                client: "mcp".into(),
                target,
                decision: decision.into(),
                grant_id: None,
                duration_ms: started.elapsed().as_millis() as u64,
                rows: count("row_count"),
                bytes: count("bytes"),
                status,
                sql: None,
            }
            .sanitize(SqlAuditMode::Hash, field("sql")),
        );
    }
```

In `DexoMcpServer::new`, before building `Inner`:

```rust
        let retention = i64::from(service.profile.audit_retention_days).saturating_mul(86_400);
        ledger.prune_audits(now_secs().saturating_sub(retention));
```

- [ ] **Step 2: Grant events say they are grant events**

In `tools_write.rs::audit`, change `request: format!("tools/call {tool}")` to `request: format!("grant {tool}")`.

- [ ] **Step 3: Reading the audit does not delete it**

In `crates/dexo-cli/src/run.rs::mcp_audit`, delete `ledger.prune_audits(now_minus_retention(30));` and the now-unused `now_minus_retention`. Retention is applied by `mcp serve`, with the profile's own setting.

- [ ] **Step 4: Protocol test**

```rust
#[tokio::test]
async fn every_call_is_audited_without_its_sql() {
    let (mut client, _, ledger) = client_with(FakeBackend::with_session("local", users())).await;
    client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    client.call("grant_create", json!({})).await;
    let events = ledger.audits();
    let read = events
        .iter()
        .find(|event| event.request == "tools/call query_execute_read")
        .expect("the read is audited");
    assert_eq!(read.decision, "allow");
    assert_eq!(read.status, "ok");
    assert_eq!(read.rows, 1);
    assert!(read.sql.as_deref().is_some_and(|hash| !hash.contains("select")));
    let denied = events
        .iter()
        .find(|event| event.request == "tools/call grant_create")
        .expect("the denied call is audited");
    assert_eq!(denied.decision, "deny");
    assert_eq!(denied.status, "NOT_FOUND");
}
```

- [ ] **Step 5: Run, gate, commit**

Run: `cargo test -p dexo-mcp && cargo test -p dexo-cli --test mcp`

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-mcp crates/dexo-cli/src/run.rs
git commit -m "feat(mcp): audit every tool call with timing, size and a hashed statement"
```

---

### Task 14: Prompts, resources and dead code (MCP-017, F17)

**Files:**
- Rewrite: `crates/dexo-mcp/src/prompts.rs`
- Modify: `crates/dexo-mcp/src/server.rs` (prompt calls)
- Modify: `crates/dexo-mcp/tests/protocol.rs`

- [ ] **Step 1: Prompts name real tools and carry no SQL**

Replace `crates/dexo-mcp/src/prompts.rs` with:

```rust
use dexo_app::mcp::{McpService, advertised_tools};
use rmcp::model::{Prompt, PromptMessage, Role};

use crate::error::HIDDEN;

pub fn list_prompts() -> Vec<Prompt> {
    vec![
        Prompt::new(
            "explore_schema",
            Some("Walk the allowed catalog of a connection"),
            None,
        ),
        Prompt::new(
            "review_migration",
            Some("Review the difference between two saved schema snapshots"),
            None,
        ),
        Prompt::new(
            "analyze_plan",
            Some("Read the estimated plan of a query"),
            None,
        ),
    ]
}

pub fn get_prompt(service: &McpService, name: &str) -> Result<Vec<PromptMessage>, String> {
    let tools = advertised_tools(&service.profile).join(", ");
    let text = match name {
        "explore_schema" => format!(
            "Call list_connections, then catalog_list without parent_id, and follow the ids down to tables; use object_describe for columns. Available tools: {tools}."
        ),
        "review_migration" => format!(
            "Call schema_diff with the two snapshot names the user gives you and explain each added, removed or changed object. Available tools: {tools}."
        ),
        "analyze_plan" => format!(
            "Call query_explain with the user's query and explain the most expensive nodes. Never ask for ANALYZE. Available tools: {tools}."
        ),
        _ => return Err(HIDDEN.into()),
    };
    Ok(vec![PromptMessage::new_text(Role::User, text)])
}
```

In `server.rs`, change `prompts::list_prompts(&self.inner.service)` to `prompts::list_prompts()`.

- [ ] **Step 2: Sweep what is left**

Run: `grep -rn "ResultStore\|new_result_uri\|LimitTracker\|CancellationRegistry\|TextOut\|serve_with_ledger\|tools_read::call_tool\|input_schema" crates`
Expected: no output. Delete anything it finds.

- [ ] **Step 3: Protocol test**

```rust
#[tokio::test]
async fn resources_and_prompts_do_not_leak_policy_or_sql() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    let listed = client.request("resources/list", json!({})).await;
    assert_eq!(
        listed["result"]["resources"].as_array().map(Vec::len),
        Some(1)
    );
    let capabilities = client
        .request(
            "resources/read",
            json!({"uri": "dexo://profile/capabilities"}),
        )
        .await;
    let body = capabilities["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(body.contains("query_execute_read"));
    assert!(!body.contains("secrets"), "the allowlist is not published: {body}");
    for name in ["explore_schema", "review_migration", "analyze_plan"] {
        let prompt = client.request("prompts/get", json!({"name": name})).await;
        let text = prompt["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();
        assert!(!text.is_empty(), "{name}");
        assert!(!text.contains("select "), "{name}");
    }
    let unknown = client
        .request("resources/read", json!({"uri": "dexo://object/secrets"}))
        .await;
    assert!(unknown.get("error").is_some());
}
```

- [ ] **Step 4: Run, gate, commit**

Run: `cargo test -p dexo-mcp`

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings
git add -A crates/dexo-mcp
git commit -m "fix(mcp): prompts point at real tools; drop dead adapter code"
```

---

### Task 15: Protocol, contract and live suites (MCP-001, MCP-014, MCP-015, MCP-016, MCP-019, MCP-020, QUALITY-015, QUALITY-019)

**Files:**
- Modify: `crates/dexo-mcp/tests/protocol.rs`, `crates/dexo-mcp/Cargo.toml`
- Create: `crates/dexo-mcp/tests/snapshots/protocol__tools_v2.snap` (generated by insta)
- Modify: `crates/dexo/tests/mcp_stdio.rs`

- [ ] **Step 1: Snapshot dependency**

In `crates/dexo-mcp/Cargo.toml` `[dev-dependencies]` add `insta = { workspace = true, features = ["json"] }`.

- [ ] **Step 2: The remaining protocol tests**

Append to `crates/dexo-mcp/tests/protocol.rs`:

```rust
/// MCP-020 / QUALITY-015: any change to a tool's name, schema or annotations shows up
/// here. Review it with `cargo insta review`, bump TOOL_SCHEMA_VERSION and rename the
/// snapshot to match.
#[tokio::test]
async fn tool_contract_is_versioned() {
    assert_eq!(
        dexo_mcp::TOOL_SCHEMA_VERSION,
        2,
        "rename the snapshot below with the new version"
    );
    let mut everything = profile();
    for tool in ["admin_list_sessions", "data_execute_sql"] {
        everything.tool_rules.push(dexo_app::mcp::ToolRule {
            tool: tool.into(),
            allowed: true,
        });
    }
    let ledger = Arc::new(MemoryGrantLedger::default());
    let now = dexo_mcp::tools_write::now_secs();
    for (capability, tools) in [
        (
            GrantCapability::DataWrite,
            vec!["data_insert", "data_update", "data_delete", "data_execute_sql"],
        ),
        (GrantCapability::Ddl, vec!["schema_apply_ddl"]),
        (
            GrantCapability::Admin,
            vec!["admin_cancel_query", "admin_terminate_session"],
        ),
    ] {
        ledger
            .insert_grant(
                Grant::new(
                    &everything,
                    "local",
                    capability,
                    tools.into_iter().map(String::from).collect(),
                    vec![SelectorRule::parse(Effect::Allow, "db.public.users").unwrap()],
                    now,
                    DEFAULT_TTL_SECS,
                )
                .unwrap(),
            )
            .unwrap();
    }
    let mut client = Client::start(
        everything,
        vec![connection("local")],
        Arc::new(FakeBackend::with_session("local", users())),
        ledger,
    )
    .await;
    insta::assert_json_snapshot!("tools_v2", client.tools().await);
}

/// MCP-002 / MCP-003: a structured-only profile exposes no raw SQL, no write and no
/// grant tool, and calling one anyway reads as "not found".
#[tokio::test]
async fn a_structured_profile_exposes_no_raw_sql_or_writes() {
    let mut structured = profile();
    structured.query_mode = dexo_app::mcp::QueryMode::StructuredOnly;
    let mut client = Client::start(
        structured,
        vec![connection("local")],
        Arc::new(FakeBackend::with_session("local", users())),
        Arc::new(MemoryGrantLedger::default()),
    )
    .await;
    let names = client.tool_names().await;
    for hidden in ["query_execute_read", "query_explain", "data_insert", "grant_create"] {
        assert!(!names.contains(&hidden.to_string()), "{hidden}");
        let result = client.call(hidden, json!({"sql": "select 1"})).await;
        assert_eq!(text(&result), "Error [NOT_FOUND]: not found", "{hidden}");
    }
    assert!(names.contains(&"data_read".to_string()));
}

/// QUALITY-019: every bypass found in the 2026-09-23 review, over the protocol. None of
/// them may reach the session.
#[tokio::test]
async fn review_bypasses_never_reach_the_database() {
    let session = users();
    let (mut client, _, _) =
        client_with(FakeBackend::with_session("local", session.clone())).await;
    for sql in [
        "select count(*) from users u join secrets s on true",
        "select count(*) from users, secrets",
        "select count(*) from\nsecrets",
        "select * into leaked from users",
        "select * from users for update",
        "select pg_terminate_backend(1) from users",
        "explain analyze delete from users",
        "with x as (delete from users returning *) select * from x",
        "select query_to_xml('select * from secrets', true, true, '')",
        "select 1; select 2",
    ] {
        let result = client.call("query_execute_read", json!({"sql": sql})).await;
        assert!(is_error(&result), "{sql} was accepted: {result}");
    }
    assert!(
        !session.log().iter().any(|entry| entry.starts_with("execute")),
        "{:?}",
        session.log()
    );
}

/// MCP-014: a cancel names one request and stops only that one. `data_read` needs the
/// same connection lock the hanging read holds, so it only answers if the cancelled
/// request really let go.
#[tokio::test]
async fn a_cancel_notification_only_stops_its_own_request() {
    let session = FakeSession::hanging();
    let (mut client, _, _) =
        client_with(FakeBackend::with_session("local", session.clone())).await;
    let hanging = client
        .send_request(
            "tools/call",
            json!({"name": "query_execute_read", "arguments": {"sql": "select id from users"}}),
        )
        .await;
    client
        .notify(
            "notifications/cancelled",
            json!({"requestId": hanging, "reason": "test"}),
        )
        .await;
    let next = client.call("data_read", json!({"table": "users"})).await;
    assert!(!is_error(&next), "{next}");
    assert!(session.log().contains(&"rollback".to_string()), "{:?}", session.log());
    let listed = client.call("list_connections", json!({})).await;
    assert!(!is_error(&listed), "{listed}");
}

/// MCP-019: with two connections the name is required, and each call reaches its own.
#[tokio::test]
async fn several_connections_need_an_explicit_name() {
    let sales = users();
    let reports = users();
    let mut backend = FakeBackend::with_session("sales", sales.clone());
    backend.sessions.insert("reports".into(), reports.clone());
    let mut two = profile();
    two.connections = vec!["sales".into(), "reports".into()];
    let mut client = Client::start(
        two,
        vec![connection("sales"), connection("reports")],
        Arc::new(backend),
        Arc::new(MemoryGrantLedger::default()),
    )
    .await;
    let ambiguous = client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    assert!(text(&ambiguous).contains("list_connections"), "{ambiguous}");
    let routed = client
        .call(
            "query_execute_read",
            json!({"connection": "reports", "sql": "select id from users"}),
        )
        .await;
    assert!(!is_error(&routed), "{routed}");
    assert!(sales.log().is_empty());
    assert!(reports.log().iter().any(|entry| entry.starts_with("execute")));
    let unknown = client
        .call(
            "query_execute_read",
            json!({"connection": "prod", "sql": "select 1"}),
        )
        .await;
    assert_eq!(text(&unknown), "Error [NOT_FOUND]: not found");
}

/// MCP-015: production connections never accept an MCP write, grant or not.
#[tokio::test]
async fn production_connections_refuse_writes_over_the_protocol() {
    let mut production = connection("local");
    production.environment = dexo_app::Environment::Production;
    let ledger = Arc::new(MemoryGrantLedger::default());
    ledger
        .insert_grant(
            Grant::new(
                &profile(),
                "local",
                GrantCapability::DataWrite,
                vec!["data_insert".into()],
                vec![SelectorRule::parse(Effect::Allow, "db.public.users").unwrap()],
                dexo_mcp::tools_write::now_secs(),
                DEFAULT_TTL_SECS,
            )
            .unwrap(),
        )
        .unwrap();
    let mut client = Client::start(
        profile(),
        vec![production],
        Arc::new(FakeBackend::with_session("local", users())),
        Arc::clone(&ledger),
    )
    .await;
    let refused = client
        .call(
            "data_insert",
            json!({"operation_id": "op-prod", "target": "users", "values": {"id": 1}}),
        )
        .await;
    assert!(text(&refused).starts_with("Error [POLICY_DENIED]"), "{refused}");
    assert_eq!(
        ledger
            .active_grants("assistant", dexo_mcp::tools_write::now_secs())
            .len(),
        1
    );
}

/// MCP-016: every listed tool rejects a call without its required arguments, as a
/// protocol error or a tool error, never as a silent default.
#[tokio::test]
async fn missing_required_arguments_are_rejected() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    for tool in client.tools().await {
        let required = tool["inputSchema"]["required"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if required.is_empty() {
            continue;
        }
        let name = tool["name"].as_str().unwrap_or_default().to_string();
        let response = client
            .request("tools/call", json!({"name": name, "arguments": {}}))
            .await;
        let rejected = response.get("error").is_some() || is_error(&response["result"]);
        assert!(rejected, "{name} accepted a call without {required:?}: {response}");
    }
}
```

- [ ] **Step 3: Accept the snapshot**

Run: `cargo test -p dexo-mcp tool_contract_is_versioned; cargo insta accept`
Then open `crates/dexo-mcp/tests/snapshots/protocol__tools_v2.snap` and read it: every tool has a description, `connection` is optional everywhere, and each input lists its `required` fields. Fix any schema that reads wrong before accepting.

- [ ] **Step 4: The stdio smoke test serves a real profile**

`mcp serve` refuses a profile without connections since Task 7, and the current test would then pass on an empty stdout. In `crates/dexo/tests/mcp_stdio.rs`, before saving the profile, save a connection and point the profile at it:

```rust
    dexo_storage::ConnectionRepository::new(db.connection())
        .save(&dexo_app::ConnectionProfile::new(
            dexo_app::ConnectionId(uuid::Uuid::new_v4()),
            None,
            "local",
            "postgres",
            "local",
            serde_json::json!({"host": "127.0.0.1", "port": 1, "database": "db", "username": "u"}),
            dexo_app::SecretRef::new("unused".into()),
        ))
        .unwrap();
    profile.connections = vec!["local".into()];
```

and, after the loop over `stdout.lines()`, require the `initialize` answer:

```rust
    assert!(
        stdout.lines().any(|line| line.contains("\"id\":1")),
        "no initialize response on stdout\nstderr={stderr}"
    );
```

Port 1 never listens; the server starts anyway because connections open lazily.

- [ ] **Step 5: Run everything**

Run: `cargo test -p dexo-mcp && cargo test -p dexo --test mcp_stdio --test threat && cargo test -p dexo --test mcp_read --test mcp_write -- --include-ignored`
Expected: all pass. The last command needs Docker; if Docker is not running, say so in the task report instead of claiming it passed.

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo deny check
git add -A crates/dexo-mcp crates/dexo/tests/mcp_stdio.rs Cargo.lock
git commit -m "test(mcp): protocol, contract and adversarial suites for MCP-001..020"
```

---

### Task 16: Documentation

**Files:**
- Rewrite: `docs/src/mcp.md`
- Modify: `docs/src/cli.md` (the `dexo mcp` section)

- [ ] **Step 1: `docs/src/mcp.md`**

Replace the page with these sections, in this order, written from the code as it now stands:

1. **What it is** — stdio-only server; no HTTP listener; tools, one resource, three prompts.
2. **Set up** — the exact command sequence:
   ```sh
   dexo mcp profile create --name assistant
   dexo mcp profile set --name assistant --connection local --query-mode raw-read
   dexo mcp allow --profile assistant --selector 'app.public.*'
   dexo mcp allow --profile assistant --selector 'app.public.secrets' --deny
   dexo mcp profile enable --name assistant --confirm
   dexo mcp config print --profile assistant --client claude-code
   ```
3. **Selectors** — matched from the front of the object's path; Postgres `catalog.schema.object`, MySQL `catalog.object`; deny wins; bare names in SQL are completed with the connection's database and `public`.
4. **Tools** — a table built from `crates/dexo-mcp/tests/snapshots/protocol__tools_v2.snap`: name, what it does, when it is listed (always / raw-read / opt-in rule / grant).
5. **Reads** — parsed allowlist check, then `BEGIN READ ONLY … ROLLBACK`; row and byte limits; the truncation notice; `next_offset`.
6. **Writes** — `dexo mcp grant create …`; one use; TTL; bound to one connection and inside the profile; production and read-only connections never accept MCP writes; destructive DDL needs `confirm_target`.
7. **Audit** — one event per call, SQL hashed, retention from the profile, `dexo mcp audit`.
8. **Limits of the allowlist** — the parse sees table names, not what a function or a view reads inside the server. For hard isolation, give the connection a database user that can read only the allowed objects.

- [ ] **Step 2: `docs/src/cli.md`**

Next to the existing `dexo mcp serve --profile assistant` line, add one line each for `dexo mcp profile set`, `dexo mcp allow --remove` and `dexo mcp config print --client claude-code`.

- [ ] **Step 3: Check and commit**

Run: `cargo test -p dexo --test docs`

```bash
git add docs/src/mcp.md docs/src/cli.md
git commit -m "docs(mcp): setup, selectors, tools, grants and the allowlist's limits"
```

---

# Part C — Blocked or out of scope

| Requirement | Why it is not in this plan | Unblocks when |
| --- | --- | --- |
| MCP-010 task status/cancel with scope | MCP calls are request/response, and `notifications/cancelled` already cancels the one request it names (Tasks 3, 15). A status/cancel surface for operations *outside* an MCP call (TUI queries, transfers) needs the shared task runtime. | TASK-001 (P) and TASK-024 (A) land. |
| MCP-009 compare **sync apply** | `schema_diff` is read-only by design. Applying a sync needs the protected apply flow. | COMPARE-023 (A) lands; then add `schema_sync_apply` behind a `ddl` grant, reusing that flow. |
| Grant creation and profile editing in the TUI | The old docs claimed grants were "created in the TUI"; the TUI only enables, disables and revokes. An editor is TUI design work. | A separate plan, through `superpowers:brainstorming` + `tui-design:tui-design`. |
| Streamable HTTP transport | Out of scope by the spec (§ Fora de escopo; MCP-001). | Only if the spec changes. |
| `citext` and other unknown Postgres types render as hex | Seen in the review's live probe (`\x6c6f6a…` for an e-mail address). A driver value-decoding bug in `dexo-driver-postgres`, not an MCP one. | A separate driver fix. |

## Traceability

| Spec ID | Task(s) | Test(s) |
| --- | --- | --- |
| MCP-001 | 7, 15 | `Client::read_message` asserts JSON-RPC on every line; `mcp_stdio.rs` |
| MCP-002 | 3, 5, 7 | `a_structured_profile_exposes_no_raw_sql_or_writes`, `reads_run_in_a_read_only_transaction_that_is_rolled_back` |
| MCP-003 | existing, 15 | `a_structured_profile_exposes_no_raw_sql_or_writes` (`grant_create`), `catalog_has_no_grant_management_tools` |
| MCP-004 | 4, 7 | `a_grant_cannot_reach_what_the_profile_denies`, `a_grant_is_bound_to_its_connection`, `a_grant_publishes_its_tool_and_revoking_removes_it` |
| MCP-005 | 7, 15 | `tools_are_typed_and_described`, `tool_contract_is_versioned` |
| MCP-006 | 1, 2, 3, 8 | `every_joined_table_is_reported`, `a_column_belongs_to_its_table`, `a_denied_table_is_invisible_to_every_catalog_tool` |
| MCP-007 | 9 | `data_read_pages_and_says_where_to_continue` |
| MCP-008 | 5, 11 | `updates_must_name_the_real_key`, `sql_writes_must_stay_inside_the_grant` |
| MCP-009 | 5, 10 | `data_write_cannot_ddl_and_ddl_cannot_terminate`, `destructive_ddl_needs_the_target_typed_by_the_client`, `schema_diff_hides_denied_objects` |
| MCP-010 | Part C | — |
| MCP-011 | 12 | `admin_reads_need_an_explicit_rule` |
| MCP-012 | 13 | `every_call_is_audited_without_its_sql` |
| MCP-013 | 3, 7, 9 | `rows_past_the_limits_are_cut_and_flagged`, `cells_cannot_break_the_table`, `data_read_pages_and_says_where_to_continue` |
| MCP-014 | 3, 15 | `a_cancelled_read_cancels_the_query_and_rolls_back`, `a_cancel_notification_only_stops_its_own_request` |
| MCP-015 | 3, 5, 6, 15 | `unknown_environment_labels_fail_closed`, `production_connections_refuse_writes_and_keep_the_grant`, `production_connections_refuse_writes_over_the_protocol` |
| MCP-016 | 7–15 | one protocol test per tool family, `missing_required_arguments_are_rejected` |
| MCP-017 | 12, 14 | `resources_and_prompts_do_not_leak_policy_or_sql`; `admin_list_sessions` omits query text |
| MCP-018 | existing, 4 | `revoke_before_dispatch_hides_grant`, `writes_without_an_open_connection_fail_before_the_grant_is_spent` |
| MCP-019 | 7, 15 | `several_connections_need_an_explicit_name`, `a_failed_connect_is_retried_on_the_next_call` |
| MCP-020 | 7, 15 | `tool_contract_is_versioned` |
