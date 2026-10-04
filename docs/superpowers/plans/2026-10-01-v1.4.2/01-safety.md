# 1.4.2 Section 1: Safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The SQL editor never runs a write on a read-only connection, asks for the connection's name before any write on production, and asks before a destructive statement anywhere the policy says so; the grid review and the schema editor stop leaking past the same policy; MySQL enforces read-only on the server; the welcome says how to add a connection.

**Architecture:** `dexo-sql` learns two judgements on one statement, `is_read` and `destructive`, on top of the sqlparser inspection it already has for MCP. `dexo-app` turns those plus the connection's policy into one verdict for a script (`run_guard::judge`), so the CLI can reuse it later. The TUI asks that verdict in `start_query`, the single place every editor run passes, and shows a dialog built on the existing footer and text-input widgets. The MySQL factory sends `SET SESSION TRANSACTION READ ONLY` on every connection of a read-only session, as the Postgres factory already sets `default_transaction_read_only`.

**Tech Stack:** Rust 1.93 (edition 2024), sqlparser 0.62 (`Visitor`), mysql_async 0.37 (`OptsBuilder::init`), ratatui, crossterm.

**Spec:** `docs/superpowers/specs/2026-10-01-v1.4.2-competitive-release-design.md`, section 1 (A1, A4, A5). Index and release-wide rules: `docs/superpowers/plans/2026-10-01-v1.4.2/README.md`.

## Global Constraints

- Every Submit/Cancel dialog: arrows walk the buttons, Esc cancels (`widgets::form::footer_key`).
- Secrets never go in SQLite, TOML, argv, logs or panic reports.
- TUI, CLI and MCP go through `dexo-app`; drivers do not import UI crates.
- No new dependencies in this section.
- CI gates before each commit: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- Tests: only the ones written out below. They pin safety properties that have no other way to be checked; do not add others.
- Unknown statements count as writes. An unknown environment label counts as production (`Environment::parse_strict`). A profile whose policy cannot be found or resolved confirms destructive statements.
- On production, the name typed must equal the connection name exactly: no trimming, no case folding.
- Commit subjects are user-facing sentences; the CHANGELOG is generated from them.

## Review Focus

1. **Comments and casing before the keyword.** `-- note\nDELETE FROM t` and `DeLeTe from t` are judged like `delete from t`. Pinned in Task 1 (`destructive_statements_say_why`).
2. **A script where only a later statement writes.** On production, `select 1; insert …; delete …` lists statements 2 and 3, and Run sends all three as one script. Pinned in Task 2 (`production_asks_for_the_name_on_any_write`) and Task 3 (`run_on_a_destructive_prompt_sends_exactly_what_was_shown`).
3. **Custom environment labels.** A profile labelled `prod` gets the production rules, even though `Environment::parse` would call it local. Pinned in Task 3 (`an_unknown_label_like_prod_counts_as_production`).
4. **The connection changing under an open dialog.** A connect that lands while the dialog is open closes it, so an answer given for one connection never runs on another. Pinned in Task 3 (`a_connection_change_closes_the_run_prompt`).
5. **Running the statement under the cursor, not only the document.** Ctrl+Enter takes the same guard as Run Document. Pinned in Task 3 (`the_statement_under_the_cursor_is_guarded_too`).

## File Structure

**`dexo-sql`**
- Modify `crates/dexo-sql/src/statement_guard.rs`: add `Destructive`, `is_read`, `destructive`, and the visitor that finds a `DELETE`/`UPDATE` without `WHERE` or an `ALTER … DROP`.
- Modify `crates/dexo-sql/src/statement.rs`: `first_keyword` becomes `pub(crate)`.
- Modify `crates/dexo-sql/src/lib.rs`: export the new names.

**`dexo-app`**
- Create `crates/dexo-app/src/run_guard.rs`: `RunPolicy`, `Flagged`, `RunVerdict`, `judge`.
- Modify `crates/dexo-app/src/connection_policy.rs`: `Environment::parse_strict`, moved from `mcp/connection.rs`.
- Modify `crates/dexo-app/src/mcp/connection.rs`: use `Environment::parse_strict`; drop `mcp_environment`.
- Modify `crates/dexo-app/src/lib.rs`: `pub mod run_guard;`.

**`dexo-tui`**
- Create `crates/dexo-tui/src/screens/run_prompt.rs`: the dialog's state and lines.
- Modify `crates/dexo-tui/src/screens/mod.rs`, `model.rs`, `mouse.rs`, `render.rs`, `update.rs`, `screens/editor.rs`.
- Create `crates/dexo-tui/tests/editor_write_guard.rs`.

**`dexo-driver-mysql`**
- Modify `crates/dexo-driver-mysql/src/factory.rs`; add a test to `crates/dexo-driver-mysql/tests/query.rs`.

**Docs**
- Modify `docs/src/connections.md`.

---

### Task 1: Reads, writes and destructive statements in `dexo-sql`

**Files:**
- Modify: `crates/dexo-sql/src/statement.rs:298`
- Modify: `crates/dexo-sql/src/statement_guard.rs` (new items after `inspect_schema_write`, tests in its `mod tests`)
- Modify: `crates/dexo-sql/src/lib.rs:36-38`

**Interfaces:**
- Produces:
  - `dexo_sql::Destructive` (`Clone, Copy, Debug, Eq, PartialEq`): `DeleteWithoutWhere`, `UpdateWithoutWhere`, `Drop`, `Truncate`, `AlterDrop`, `Unrecognized`; `Destructive::describe(self) -> &'static str`.
  - `dexo_sql::is_read(sql: &str, dialect: Dialect) -> bool`
  - `dexo_sql::destructive(sql: &str, dialect: Dialect) -> Option<Destructive>`
  - Both take one statement, as `planned_statements` hands them out.

- [ ] **Step 1: Let the guard read the first keyword**

In `crates/dexo-sql/src/statement.rs`, change the signature at line 298:

```rust
pub(crate) fn first_keyword(sql: &str) -> Option<String> {
```

It already skips whitespace and comments (`skip_ws`), so `-- note\nDELETE` yields `DELETE`.

- [ ] **Step 2: Add the judgements to `statement_guard.rs`**

Add to the imports at the top:

```rust
use crate::statement::first_keyword;
```

Add after `inspect_schema_write` (before `fn parse_one`):

```rust
/// Why the editor asks before it runs a statement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Destructive {
    DeleteWithoutWhere,
    UpdateWithoutWhere,
    Drop,
    Truncate,
    AlterDrop,
    /// Neither parsed nor plainly a read, so what it does cannot be told.
    Unrecognized,
}

impl Destructive {
    pub fn describe(self) -> &'static str {
        match self {
            Self::DeleteWithoutWhere => "DELETE without WHERE removes every row",
            Self::UpdateWithoutWhere => "UPDATE without WHERE changes every row",
            Self::Drop => "DROP removes the object and what it holds",
            Self::Truncate => "TRUNCATE removes every row",
            Self::AlterDrop => "ALTER ... DROP removes part of the table",
            Self::Unrecognized => "Dexo could not read this statement",
        }
    }
}

/// Whether one statement only reads. SHOW and DESCRIBE count, and so does a plain
/// EXPLAIN, which never runs what it explains. A read that writes on the side --
/// `SELECT INTO`, `FOR UPDATE`, `set_config()`, a data-modifying CTE -- does not, nor
/// does anything sqlparser cannot parse.
pub fn is_read(sql: &str, dialect: Dialect) -> bool {
    let keyword = first_keyword(sql);
    if matches!(keyword.as_deref(), Some("SHOW" | "DESCRIBE" | "DESC")) {
        return true;
    }
    match inspect_read(sql, dialect) {
        Ok(_) => true,
        Err(GuardRejection::WrongKind) => keyword.as_deref() == Some("EXPLAIN"),
        Err(_) => false,
    }
}

/// What makes one statement destructive, if anything does. A statement that neither
/// parses nor is plainly a read is `Unrecognized`: unknown counts against it.
pub fn destructive(sql: &str, dialect: Dialect) -> Option<Destructive> {
    match first_keyword(sql).as_deref() {
        Some("DROP") => return Some(Destructive::Drop),
        Some("TRUNCATE") => return Some(Destructive::Truncate),
        _ => {}
    }
    let Ok(statement) = parse_one(sql, dialect) else {
        return (!is_read(sql, dialect)).then_some(Destructive::Unrecognized);
    };
    let mut finder = DestructiveFinder::default();
    let _ = statement.visit(&mut finder);
    finder.found
}

/// Visits nested statements too, so a `DELETE` inside a CTE is found.
#[derive(Default)]
struct DestructiveFinder {
    found: Option<Destructive>,
}

impl Visitor for DestructiveFinder {
    type Break = ();

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<()> {
        self.found = match statement {
            Statement::Delete(delete) if delete.selection.is_none() => {
                Some(Destructive::DeleteWithoutWhere)
            }
            Statement::Update(update) if update.selection.is_none() => {
                Some(Destructive::UpdateWithoutWhere)
            }
            // Every drop operation displays as `DROP ...`; matching the text covers
            // columns, constraints, keys, indexes and partitions alike.
            Statement::AlterTable(alter)
                if alter
                    .operations
                    .iter()
                    .any(|operation| operation.to_string().starts_with("DROP")) =>
            {
                Some(Destructive::AlterDrop)
            }
            _ => return ControlFlow::Continue(()),
        };
        ControlFlow::Break(())
    }
}
```

- [ ] **Step 3: Export them**

In `crates/dexo-sql/src/lib.rs`, replace the `statement_guard` re-export:

```rust
pub use statement_guard::{
    Destructive, GuardRejection, Inspection, destructive, inspect_data_write, inspect_read,
    inspect_schema_write, is_read,
};
```

- [ ] **Step 4: Pin the judgements**

In `statement_guard.rs`'s `mod tests`, replace the `use super::…` line with:

```rust
    use super::{
        Destructive, GuardRejection, destructive, inspect_data_write, inspect_read,
        inspect_schema_write, is_read,
    };
```

and add at the end of the module:

```rust
    #[test]
    fn plain_reads_are_reads() {
        for sql in [
            "select 1",
            "with t as (select 1 as n) select n from t",
            "-- note\nselect * from items",
            "show tables",
            "SHOW search_path",
            "describe items",
            "explain select * from items",
        ] {
            assert!(is_read(sql, Dialect::Postgres), "{sql}");
        }
    }

    #[test]
    fn writes_hidden_in_reads_are_not_reads() {
        for sql in [
            "select * into backup from items",
            "select * from items for update",
            "select set_config('default_transaction_read_only', 'off', false)",
            "with d as (delete from items returning *) select * from d",
            "explain analyze delete from items",
            "set default_transaction_read_only = off",
            "begin",
            "insert into items values (1)",
            "not sql at all",
        ] {
            assert!(!is_read(sql, Dialect::Postgres), "{sql}");
        }
    }

    #[test]
    fn destructive_statements_say_why() {
        let pg = |sql: &str| destructive(sql, Dialect::Postgres);
        assert_eq!(pg("delete from items"), Some(Destructive::DeleteWithoutWhere));
        assert_eq!(pg("DeLeTe from items"), Some(Destructive::DeleteWithoutWhere));
        assert_eq!(
            pg("-- clean up\nDELETE FROM items"),
            Some(Destructive::DeleteWithoutWhere)
        );
        assert_eq!(pg("update items set n = 0"), Some(Destructive::UpdateWithoutWhere));
        assert_eq!(
            pg("update items set n = (select max(n) from items where id = 1)"),
            Some(Destructive::UpdateWithoutWhere)
        );
        assert_eq!(pg("drop table items"), Some(Destructive::Drop));
        assert_eq!(pg("DROP INDEX items_n"), Some(Destructive::Drop));
        assert_eq!(pg("truncate items"), Some(Destructive::Truncate));
        assert_eq!(pg("alter table items drop column n"), Some(Destructive::AlterDrop));
        assert_eq!(
            pg("alter table items drop constraint items_pkey"),
            Some(Destructive::AlterDrop)
        );
        assert!(pg("with d as (delete from items returning id) select * from d").is_some());
        assert_eq!(pg("frobnicate items"), Some(Destructive::Unrecognized));
        assert_eq!(
            destructive("delete from items limit 10", Dialect::Mysql),
            Some(Destructive::DeleteWithoutWhere)
        );
    }

    #[test]
    fn ordinary_writes_and_reads_are_not_destructive() {
        for sql in [
            "delete from items where id = 1",
            "update items set n = 0 where id in (select id from items)",
            "insert into items values (1, 2)",
            "alter table items add column m int",
            "alter table items alter column n drop not null",
            "create table t (id int)",
            "select 1",
            "show tables",
        ] {
            assert_eq!(destructive(sql, Dialect::Postgres), None, "{sql}");
        }
    }
```

- [ ] **Step 5: Run them**

Run: `cargo test -p dexo-sql statement_guard`
Expected: PASS, including the existing guard tests.

- [ ] **Step 6: CI gates and commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/dexo-sql/src/statement.rs crates/dexo-sql/src/statement_guard.rs crates/dexo-sql/src/lib.rs
git commit -m "feat(sql): tell plain reads from writes and destructive statements"
```

---

### Task 2: One verdict for a script in `dexo-app`

**Files:**
- Create: `crates/dexo-app/src/run_guard.rs`
- Modify: `crates/dexo-app/src/lib.rs` (module list)
- Modify: `crates/dexo-app/src/connection_policy.rs` (`impl Environment`, `mod tests`)
- Modify: `crates/dexo-app/src/mcp/connection.rs:47`, `:111-119`, `:123`, `:158-164`

**Interfaces:**
- Consumes: `dexo_sql::{Destructive, Dialect, destructive, is_read}` (Task 1).
- Produces:
  - `dexo_app::Environment::parse_strict(label: &str) -> Environment`: known labels as themselves, empty or `local` as `Local`, anything else as `Production`.
  - `dexo_app::run_guard::RunPolicy { connection: String, read_only: bool, confirm_destructive: bool, production: bool }` (`Clone, Debug, Eq, PartialEq`).
  - `dexo_app::run_guard::Flagged { index: usize, sql: String, reason: &'static str }` (`Clone, Debug, Eq, PartialEq`).
  - `dexo_app::run_guard::RunVerdict` (`Clone, Debug, Eq, PartialEq`): `Run`, `Refuse { index: usize, sql: String }`, `Confirm { flagged: Vec<Flagged>, typed: Option<String> }`.
  - `dexo_app::run_guard::judge(statements: &[String], dialect: Dialect, policy: &RunPolicy) -> RunVerdict`.

- [ ] **Step 1: Move the fail-closed environment reading**

In `crates/dexo-app/src/connection_policy.rs`, add to `impl Environment` after `known`:

```rust
    /// `parse` maps any unknown label to `Local`. Anything that guards a write fails
    /// closed instead, so `prod`, `PRD` or `live` count as production.
    pub fn parse_strict(label: &str) -> Self {
        match Self::known(label) {
            Some(environment) => environment,
            None if label.is_empty() || label.eq_ignore_ascii_case("local") => Self::Local,
            None => Self::Production,
        }
    }
```

and add to its `mod tests`:

```rust
    #[test]
    fn unknown_environment_labels_fail_closed() {
        assert_eq!(Environment::parse_strict("prod"), Environment::Production);
        assert_eq!(Environment::parse_strict("PRD"), Environment::Production);
        assert_eq!(Environment::parse_strict(""), Environment::Local);
        assert_eq!(Environment::parse_strict("local"), Environment::Local);
        assert_eq!(Environment::parse_strict("staging"), Environment::Staging);
    }
```

(add `Environment` to that module's `use super::…` if it is not imported there).

In `crates/dexo-app/src/mcp/connection.rs`: line 47 becomes `environment: Environment::parse_strict(&profile.environment),`; delete the doc comment and `fn mcp_environment` (lines 111-119); in its `mod tests`, change `use super::{McpConnection, mcp_environment};` to `use super::McpConnection;` and delete the test `unknown_environment_labels_fail_closed` (it now lives next to `parse_strict`).

- [ ] **Step 2: Write `run_guard.rs`**

Create `crates/dexo-app/src/run_guard.rs`:

```rust
//! What the SQL editor has to ask before it runs a script on a connection.

use dexo_sql::{Destructive, Dialect, destructive, is_read};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunPolicy {
    pub connection: String,
    pub read_only: bool,
    pub confirm_destructive: bool,
    pub production: bool,
}

/// A statement the user has to see before it runs, and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Flagged {
    pub index: usize,
    pub sql: String,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunVerdict {
    Run,
    /// Read-only connection: the first statement that is not a read, and nothing is sent.
    Refuse { index: usize, sql: String },
    /// `typed` is the connection name that has to be typed, on production.
    Confirm {
        flagged: Vec<Flagged>,
        typed: Option<String>,
    },
}

const NOT_A_READ: &str = "not a read-only statement";

/// Read-only refuses any write; production asks for the connection's name before any
/// write; elsewhere, `confirm_destructive` asks before the destructive ones. Unknown
/// statements count as writes.
pub fn judge(statements: &[String], dialect: Dialect, policy: &RunPolicy) -> RunVerdict {
    let writes: Vec<usize> = statements
        .iter()
        .enumerate()
        .filter(|(_, sql)| !is_read(sql, dialect))
        .map(|(index, _)| index)
        .collect();
    if policy.read_only {
        return match writes.first() {
            Some(&index) => RunVerdict::Refuse {
                index,
                sql: statements[index].clone(),
            },
            None => RunVerdict::Run,
        };
    }
    let flag = |index: usize, reason: &'static str| Flagged {
        index,
        sql: statements[index].clone(),
        reason,
    };
    if policy.production && !writes.is_empty() {
        let flagged = writes
            .iter()
            .map(|&index| {
                let reason = destructive(&statements[index], dialect)
                    .map_or(NOT_A_READ, Destructive::describe);
                flag(index, reason)
            })
            .collect();
        return RunVerdict::Confirm {
            flagged,
            typed: Some(policy.connection.clone()),
        };
    }
    if policy.confirm_destructive {
        let flagged: Vec<Flagged> = writes
            .iter()
            .filter_map(|&index| {
                destructive(&statements[index], dialect)
                    .map(|found| flag(index, found.describe()))
            })
            .collect();
        if !flagged.is_empty() {
            return RunVerdict::Confirm {
                flagged,
                typed: None,
            };
        }
    }
    RunVerdict::Run
}

#[cfg(test)]
mod tests {
    use super::{RunPolicy, RunVerdict, judge};
    use dexo_sql::{Destructive, Dialect};

    fn policy(read_only: bool, confirm_destructive: bool, production: bool) -> RunPolicy {
        RunPolicy {
            connection: "shop".into(),
            read_only,
            confirm_destructive,
            production,
        }
    }

    fn script(statements: &[&str]) -> Vec<String> {
        statements.iter().map(|sql| sql.to_string()).collect()
    }

    #[test]
    fn reads_run_everywhere() {
        for policy in [
            policy(true, true, true),
            policy(false, true, true),
            policy(false, false, false),
        ] {
            assert_eq!(
                judge(&script(&["select 1", "show tables"]), Dialect::Postgres, &policy),
                RunVerdict::Run
            );
        }
    }

    #[test]
    fn a_read_only_connection_refuses_the_first_write() {
        let statements = script(&[
            "select 1",
            "set default_transaction_read_only = off",
            "delete from items",
        ]);
        assert_eq!(
            judge(&statements, Dialect::Postgres, &policy(true, false, false)),
            RunVerdict::Refuse {
                index: 1,
                sql: "set default_transaction_read_only = off".into()
            }
        );
    }

    #[test]
    fn production_asks_for_the_name_on_any_write() {
        let statements = script(&["select 1", "insert into items values (1)", "delete from items"]);
        let verdict = judge(&statements, Dialect::Postgres, &policy(false, false, true));
        let RunVerdict::Confirm { flagged, typed } = verdict else {
            panic!("{verdict:?}");
        };
        assert_eq!(typed.as_deref(), Some("shop"));
        assert_eq!(
            flagged.iter().map(|flagged| flagged.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(flagged[0].reason, "not a read-only statement");
        assert_eq!(flagged[1].reason, Destructive::DeleteWithoutWhere.describe());
    }

    #[test]
    fn elsewhere_only_destructive_statements_ask() {
        let ask = policy(false, true, false);
        assert_eq!(
            judge(&script(&["delete from items where id = 1"]), Dialect::Postgres, &ask),
            RunVerdict::Run
        );
        let verdict = judge(&script(&["update items set n = 0"]), Dialect::Postgres, &ask);
        let RunVerdict::Confirm { flagged, typed } = verdict else {
            panic!("{verdict:?}");
        };
        assert_eq!(typed, None);
        assert_eq!(flagged.len(), 1);
    }

    #[test]
    fn turning_confirmation_off_runs_destructive_statements_off_production() {
        assert_eq!(
            judge(&script(&["drop table items"]), Dialect::Postgres, &policy(false, false, false)),
            RunVerdict::Run
        );
    }
}
```

In `crates/dexo-app/src/lib.rs`, add `pub mod run_guard;` after `pub mod recovery_service;`.

- [ ] **Step 3: Run them**

Run: `cargo test -p dexo-app run_guard` then `cargo test -p dexo-app connection_policy` then `cargo test -p dexo-app mcp::connection`
Expected: PASS.

- [ ] **Step 4: CI gates and commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/dexo-app/src/run_guard.rs crates/dexo-app/src/lib.rs crates/dexo-app/src/connection_policy.rs crates/dexo-app/src/mcp/connection.rs
git commit -m "feat(app): one rule for what the editor may run on a connection"
```

---

### Task 3: The editor asks before it writes

**Files:**
- Create: `crates/dexo-tui/src/screens/run_prompt.rs`
- Modify: `crates/dexo-tui/src/screens/mod.rs`
- Modify: `crates/dexo-tui/src/screens/editor.rs:179`
- Modify: `crates/dexo-tui/src/model.rs` (field after `explain_prompt`, ~1626; default, ~1726)
- Modify: `crates/dexo-tui/src/mouse.rs:87-90` (enum), `:209-211` (`top_overlay`)
- Modify: `crates/dexo-tui/src/render.rs:180-185`, `:285`, new `render_run_prompt`
- Modify: `crates/dexo-tui/src/update.rs` (`ConnectionChanged` ~65-100, mouse ~1901, keys ~2952, `start_query` 4537-4600, new functions)
- Create: `crates/dexo-tui/tests/editor_write_guard.rs`
- Modify: `docs/src/connections.md`

**Interfaces:**
- Consumes: `dexo_app::run_guard::{RunPolicy, RunVerdict, Flagged, judge}`, `dexo_app::Environment::parse_strict` (Task 2).
- Produces:
  - `Model.run_prompt: Option<crate::screens::run_prompt::RunPrompt>` (pub).
  - `RunPrompt { statements: Vec<String>, flagged: Vec<Flagged>, expected: Option<String>, typed: TextInput, footer: FooterFocus, error: Option<String> }`, `RunPrompt::new`, `title`, `accepted`, `lines`.
  - `OverlayKind::RunPrompt`.
  - `update.rs`: `launch_script(model, statements) -> Vec<Effect>` (private), `run_policy(model) -> RunPolicy` (private).

- [ ] **Step 1: The dialog's state**

Create `crates/dexo-tui/src/screens/run_prompt.rs`:

```rust
//! Asked before the SQL editor runs a write on production, or a destructive statement
//! anywhere the connection's policy confirms them.

use dexo_app::run_guard::Flagged;

use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

/// Flagged statements listed before the dialog says how many more there are.
const LISTED: usize = 5;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunPrompt {
    /// Exactly what Run sends: the statements that were judged, not a fresh read of
    /// the document.
    pub statements: Vec<String>,
    pub flagged: Vec<Flagged>,
    /// On production, the connection name that has to be typed before Run does anything.
    pub expected: Option<String>,
    pub typed: TextInput,
    pub footer: FooterFocus,
    pub error: Option<String>,
}

impl RunPrompt {
    pub fn new(statements: Vec<String>, flagged: Vec<Flagged>, expected: Option<String>) -> Self {
        // With nothing to type, Cancel has the focus, so an Enter pressed out of habit
        // runs nothing.
        let footer = if expected.is_some() {
            FooterFocus::Input
        } else {
            FooterFocus::Cancel
        };
        Self {
            statements,
            flagged,
            expected,
            typed: TextInput::default(),
            footer,
            error: None,
        }
    }

    pub fn title(&self) -> &'static str {
        if self.expected.is_some() {
            "Run on production"
        } else {
            "Run destructive statements"
        }
    }

    /// Nothing to type, or the connection name typed exactly.
    pub fn accepted(&self) -> bool {
        self.expected
            .as_deref()
            .is_none_or(|name| self.typed.as_str() == name)
    }

    pub fn lines(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for flagged in self.flagged.iter().take(LISTED) {
            lines.push(format!(
                "{}. {}",
                flagged.index + 1,
                preview(&flagged.sql, width.saturating_sub(4))
            ));
            lines.push(format!("   {}", flagged.reason));
        }
        if self.flagged.len() > LISTED {
            lines.push(format!("... and {} more", self.flagged.len() - LISTED));
        }
        lines.push(String::new());
        if let Some(name) = &self.expected {
            lines.push(format!("Type {name} to run this on production."));
            lines.push(
                self.typed
                    .inline_line("name: ", self.footer == FooterFocus::Input),
            );
        }
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line("Run", self.footer));
        lines
    }
}

/// The statement's first line, cut to `width` characters with an ellipsis when there
/// is more of it.
fn preview(sql: &str, width: usize) -> String {
    let sql = sql.trim();
    let first = sql.lines().next().unwrap_or_default();
    if first.chars().count() <= width && sql.lines().nth(1).is_none() {
        return first.to_string();
    }
    let mut cut: String = first.chars().take(width.saturating_sub(1)).collect();
    cut.push('…');
    cut
}
```

In `crates/dexo-tui/src/screens/mod.rs`, add `pub mod run_prompt;` after `pub mod recovery;`.

- [ ] **Step 2: Model, overlay and dialect**

In `crates/dexo-tui/src/model.rs`, after the `explain_prompt` field:

```rust
    /// Asked before the editor runs a write on production or a destructive statement.
    pub run_prompt: Option<crate::screens::run_prompt::RunPrompt>,
```

and in `impl Default for Model`, after `explain_prompt: None,`: `run_prompt: None,`.

In `crates/dexo-tui/src/mouse.rs`, add `RunPrompt,` after `ClosePrompt,` in `enum OverlayKind`, and in `top_overlay`, right after the `close_prompt` row:

```rust
        (model.run_prompt.is_some(), OverlayKind::RunPrompt),
```

In `crates/dexo-tui/src/screens/editor.rs:179`, make the dialect reachable from `update.rs`:

```rust
pub(crate) fn editor_dialect(model: &Model) -> Dialect {
```

- [ ] **Step 3: Guard `start_query`**

In `crates/dexo-tui/src/update.rs`, replace `start_query` (4537-4600) with the guard, and move its tail into `launch_script` unchanged:

```rust
fn start_query(model: &mut Model) -> Vec<Effect> {
    crate::screens::editor::end_typing(model);
    if model.active_document().text().trim().is_empty() {
        return Vec::new();
    }
    if !model.editor.parameters.is_empty()
        && model
            .editor
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.value, DbValue::Null))
    {
        model.editor.parameter_prompt = true;
        return Vec::new();
    }
    let statements = crate::screens::workbench::planned_statements(model);
    if statements.is_empty() {
        return Vec::new();
    }
    // Statement, selection, document and history runs all pass here, so this is the
    // one place the connection's policy is held to.
    let dialect = crate::screens::editor::editor_dialect(model);
    match dexo_app::run_guard::judge(&statements, dialect, &run_policy(model)) {
        dexo_app::run_guard::RunVerdict::Run => launch_script(model, statements),
        dexo_app::run_guard::RunVerdict::Refuse { index, sql } => {
            let first = sql.trim().lines().next().unwrap_or_default().to_string();
            model.messages.error(format!(
                "Not run: {} is read-only, and statement {} is not a read: {first}",
                model.connection.name,
                index + 1,
            ));
            Vec::new()
        }
        dexo_app::run_guard::RunVerdict::Confirm { flagged, typed } => {
            model.run_prompt = Some(crate::screens::run_prompt::RunPrompt::new(
                statements, flagged, typed,
            ));
            Vec::new()
        }
    }
}

/// The connection's policy as the guard needs it. A profile that cannot be found or
/// resolved -- a temporary connection, a custom label without its policy -- fails
/// closed: destructive statements are confirmed.
fn run_policy(model: &Model) -> dexo_app::run_guard::RunPolicy {
    let confirm_destructive = model
        .connections
        .profiles
        .iter()
        .map(|row| &row.profile)
        .find(|profile| profile.name == model.connection.name)
        .and_then(|profile| {
            dexo_app::ConnectionPolicy::resolve(&profile.environment, &profile.policy).ok()
        })
        .is_none_or(|policy| policy.confirm_destructive);
    dexo_app::run_guard::RunPolicy {
        connection: model.connection.name.clone(),
        read_only: model.connection.read_only,
        confirm_destructive,
        production: dexo_app::Environment::parse_strict(&model.connection.environment)
            == dexo_app::Environment::Production,
    }
}

fn launch_script(model: &mut Model, statements: Vec<String>) -> Vec<Effect> {
    let operation = crate::runtime::OperationId::new();
    let session = model
        .active_session
        .map(|id| id.0.to_string())
        .unwrap_or_default();
    let document = model.active_document().id.clone();
    let key = crate::runtime::OperationKey::new(
        operation,
        session.clone(),
        document.clone(),
        model.session_generation.max(1),
    );
    model.results.tabs = statements
        .iter()
        .enumerate()
        .map(|(index, sql)| {
            let mut tab = crate::model::ResultTab::new(
                crate::model::ResultKey {
                    operation: key.clone(),
                    index,
                },
                format!("result {}", index + 1),
            );
            tab.source_sql = Some(sql.clone());
            tab
        })
        .collect();
    model.results.active = 0;
    let request = QueryRequest::read(statements[0].clone(), 10_000);
    model.active_query = Some(request.id);
    model.active_operation = Some(operation);
    let mut effects = checkpoint_dirty(model);
    effects.push(Effect::StartScript(crate::action::ScriptRequest {
        key,
        statements,
        policy: model.script_policy,
        parameters: model
            .editor
            .parameters
            .iter()
            .map(|parameter| parameter.value.clone())
            .collect(),
        timeout: std::time::Duration::from_secs(30),
    }));
    effects
}
```

- [ ] **Step 4: Keys, mouse, and a connection that changes underneath**

In `handle_key`, right after the `close_prompt` check:

```rust
    if model.run_prompt.is_some() {
        return handle_run_prompt_key(model, key);
    }
```

In `handle_mouse_down`, before the `OverlayKind::ExplainPrompt` arm:

```rust
        Some(OverlayKind::RunPrompt) => match hit {
            Some(HitTarget::FormField(_)) => {
                if let Some(prompt) = model.run_prompt.as_mut()
                    && prompt.expected.is_some()
                {
                    prompt.footer = crate::widgets::form::FooterFocus::Input;
                }
                Vec::new()
            }
            Some(HitTarget::FooterSubmit) => submit_run_prompt(model),
            Some(HitTarget::FooterCancel) => {
                model.run_prompt = None;
                Vec::new()
            }
            _ => Vec::new(),
        },
```

Add next to `handle_explain_prompt_key`:

```rust
fn handle_run_prompt_key(model: &mut Model, key: KeyEvent) -> Vec<Effect> {
    use crate::widgets::form::{FooterFocus, FooterKey, footer_key};
    let Some(prompt) = model.run_prompt.as_mut() else {
        return Vec::new();
    };
    match footer_key(&mut prompt.footer, &key) {
        FooterKey::Submit => submit_run_prompt(model),
        FooterKey::Cancel => {
            model.run_prompt = None;
            Vec::new()
        }
        FooterKey::Moved => {
            // Nothing to type: the walk skips the input stop it would land on.
            if prompt.expected.is_none() && prompt.footer == FooterFocus::Input {
                prompt.footer = if matches!(key.code, KeyCode::BackTab | KeyCode::Up) {
                    FooterFocus::Cancel
                } else {
                    FooterFocus::Submit
                };
            }
            Vec::new()
        }
        FooterKey::Pass => {
            if prompt.expected.is_some() && prompt.footer == FooterFocus::Input {
                let _ = prompt.typed.handle_key(key);
                prompt.error = None;
            }
            Vec::new()
        }
    }
}

fn submit_run_prompt(model: &mut Model) -> Vec<Effect> {
    let Some(prompt) = model.run_prompt.as_mut() else {
        return Vec::new();
    };
    if !prompt.accepted() {
        prompt.error = Some("The name does not match; nothing was run.".into());
        return Vec::new();
    }
    let statements = std::mem::take(&mut prompt.statements);
    model.run_prompt = None;
    launch_script(model, statements)
}
```

In the `Action::ConnectionChanged` arm, just before `model.connection.name = name.clone();`:

```rust
            // An answer given for one connection never runs on another.
            if model.connection.name != name || model.active_session != session {
                model.run_prompt = None;
            }
```

- [ ] **Step 5: Draw it**

In `crates/dexo-tui/src/render.rs`, after the `explain_prompt` block in `render` (so it sits above it and below the close prompt):

```rust
    if let Some(prompt) = &model.run_prompt {
        render_run_prompt(frame, model, prompt, hits);
    }
```

and next to `render_explain_prompt`:

```rust
fn render_run_prompt(
    frame: &mut Frame,
    model: &Model,
    prompt: &crate::screens::run_prompt::RunPrompt,
    hits: &mut HitMap,
) {
    let width = 72.min(frame.area().width);
    let lines = prompt.lines(width.saturating_sub(2) as usize);
    let popup = centered(frame.area(), width, lines.len() as u16 + 2);
    paint_popup(
        frame,
        model,
        popup,
        Block::bordered().title(prompt.title()),
        lines.join("\n"),
    );
    register_overlay(hits, popup);
    for_popup_lines(popup, &lines, |_, line, rect| {
        if line.starts_with("name:") {
            hits.register(HitTarget::FormField(0), rect);
        }
        if line.contains("[Cancel]") {
            crate::widgets::form::register_footer(hits, rect, line, "Run");
        }
    });
}
```

In `render_explain_prompt` (line 285), read production the same strict way:

```rust
    let production = dexo_app::Environment::parse_strict(&model.connection.environment)
        == dexo_app::Environment::Production;
```

- [ ] **Step 6: Pin the guard end to end**

Create `crates/dexo-tui/tests/editor_write_guard.rs`:

```rust
//! The SQL editor holds every run to the connection's policy: a read-only connection
//! refuses writes, production asks for its name, destructive statements ask first.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_driver_api::TransactionState;
use dexo_tui::Effect;
use dexo_tui::action::Action;
use dexo_tui::model::{EditorDocument, Model};
use dexo_tui::runtime::SessionId;
use dexo_tui::screens::connections::SessionRow;
use dexo_tui::update::update;

/// `shop` live and active, with one document bound to it holding `sql`.
fn live(environment: &str, read_only: bool, sql: &str) -> Model {
    let mut model = Model::default();
    model.apply_size(120, 30);
    model.connections.load_profiles(vec![ConnectionProfile::new(
        ConnectionId(uuid::Uuid::from_u128(1)),
        None,
        "shop",
        "postgres",
        environment,
        serde_json::json!({"host":"h","port":5432,"username":"u","database":"d"}),
        SecretRef::new("r1"),
    )]);
    let session = SessionId(uuid::Uuid::from_u128(101));
    model.connections.upsert_session(SessionRow {
        id: session,
        connection: "shop".into(),
        transaction: TransactionState::Idle,
        generation: 1,
        environment: environment.into(),
        read_only,
        driver: "postgres".into(),
    });
    model.connection.name = "shop".into();
    model.connection.ready = true;
    model.connection.environment = environment.into();
    model.connection.read_only = read_only;
    model.connection.driver = "postgres".into();
    model.active_session = Some(session);
    model.session_generation = 1;
    model.documents.push(EditorDocument::new_unique(
        "shop.sql",
        None,
        Some(uuid::Uuid::from_u128(1).to_string()),
    ));
    model.active_document = model.documents.len() - 1;
    model.documents[model.active_document].sql = dexo_sql::SqlDocument::new(sql);
    model
}

fn ran(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, Effect::StartScript(_)))
}

fn press(model: &mut Model, code: KeyCode) -> Vec<Effect> {
    update(model, Action::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn type_text(model: &mut Model, text: &str) {
    for ch in text.chars() {
        press(model, KeyCode::Char(ch));
    }
}

#[test]
fn a_read_only_connection_refuses_a_write_and_sends_nothing() {
    let mut model = live("local", true, "delete from orders");
    assert!(!ran(&update(&mut model, Action::ExecuteDocument)));
    assert!(model.run_prompt.is_none());
    let message = model.messages.last().map(|entry| entry.message.clone());
    assert!(
        message.as_deref().is_some_and(|text| text.contains("read-only")),
        "{message:?}"
    );
}

#[test]
fn the_server_side_guard_cannot_be_switched_off_from_the_editor() {
    let mut model = live("local", true, "set default_transaction_read_only = off");
    assert!(!ran(&update(&mut model, Action::ExecuteDocument)));
}

#[test]
fn reads_run_without_asking_on_production() {
    let mut model = live("production", false, "select * from orders");
    assert!(ran(&update(&mut model, Action::ExecuteDocument)));
    assert!(model.run_prompt.is_none());
}

#[test]
fn production_runs_a_write_only_after_its_name_is_typed() {
    let mut model = live("production", false, "update orders set paid = true where id = 7");
    assert!(!ran(&update(&mut model, Action::ExecuteDocument)));
    assert!(model.run_prompt.is_some());
    type_text(&mut model, "Shop");
    assert!(!ran(&press(&mut model, KeyCode::Enter)), "a wrong-case name ran it");
    assert!(model.run_prompt.is_some());
    for _ in 0..4 {
        press(&mut model, KeyCode::Backspace);
    }
    type_text(&mut model, "shop");
    assert!(ran(&press(&mut model, KeyCode::Enter)));
    assert!(model.run_prompt.is_none());
}

#[test]
fn an_unknown_label_like_prod_counts_as_production() {
    let mut model = live("prod", false, "insert into orders values (1)");
    update(&mut model, Action::ExecuteDocument);
    assert_eq!(
        model
            .run_prompt
            .as_ref()
            .and_then(|prompt| prompt.expected.clone())
            .as_deref(),
        Some("shop")
    );
}

#[test]
fn a_destructive_statement_asks_and_esc_runs_nothing() {
    let mut model = live("local", false, "delete from orders");
    assert!(!ran(&update(&mut model, Action::ExecuteDocument)));
    assert!(
        model
            .run_prompt
            .as_ref()
            .is_some_and(|prompt| prompt.expected.is_none())
    );
    assert!(!ran(&press(&mut model, KeyCode::Esc)));
    assert!(model.run_prompt.is_none());
}

#[test]
fn enter_on_the_default_focus_cancels_a_destructive_run() {
    let mut model = live("local", false, "drop table orders");
    update(&mut model, Action::ExecuteDocument);
    assert!(!ran(&press(&mut model, KeyCode::Enter)));
    assert!(model.run_prompt.is_none());
}

#[test]
fn run_on_a_destructive_prompt_sends_exactly_what_was_shown() {
    let mut model = live("local", false, "select 1;\ndelete from orders");
    update(&mut model, Action::ExecuteDocument);
    press(&mut model, KeyCode::Left);
    let effects = press(&mut model, KeyCode::Enter);
    let script = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::StartScript(request) => Some(request),
            _ => None,
        })
        .expect("Run sent nothing");
    assert_eq!(script.statements.len(), 2);
    assert!(script.statements[1].contains("delete from orders"));
}

#[test]
fn the_statement_under_the_cursor_is_guarded_too() {
    let mut model = live("local", true, "delete from orders");
    assert!(!ran(&update(&mut model, Action::ExecuteStatement)));
}

#[test]
fn a_connection_change_closes_the_run_prompt() {
    let mut model = live("production", false, "delete from orders");
    update(&mut model, Action::ExecuteDocument);
    assert!(model.run_prompt.is_some());
    update(
        &mut model,
        Action::ConnectionChanged {
            name: "other".into(),
            ready: true,
            environment: "local".into(),
            session: Some(SessionId(uuid::Uuid::from_u128(202))),
            generation: 1,
            token: 0,
            read_only: false,
            driver: "postgres".into(),
        },
    );
    assert!(model.run_prompt.is_none());
}
```

Run: `cargo test -p dexo-tui --test editor_write_guard`
Expected: PASS. Then `cargo test -p dexo-tui` to check that no other flow broke (no existing test runs a write in the editor).

- [ ] **Step 7: Document it**

Append to `docs/src/connections.md`:

```markdown
## Environments and the SQL editor

Each connection has an environment: local, development, staging or production. A label Dexo does not know, such as `prod`, counts as production.

- On a read-only connection, the editor refuses any statement that is not a read and sends nothing. `SET`, transaction commands and anything Dexo cannot parse count as writes.
- On production, any write asks for the connection's name, typed exactly, before it runs.
- Elsewhere, `DELETE` or `UPDATE` without `WHERE`, `DROP`, `TRUNCATE` and `ALTER ... DROP` ask first. Turn this off with the connection's `confirm_destructive` setting.
```

- [ ] **Step 8: CI gates and commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/dexo-tui/src crates/dexo-tui/tests/editor_write_guard.rs docs/src/connections.md
git commit -m "fix(tui): the editor asks before it writes on production or destroys data"
```

---

### Task 4: The grid review and the schema editor keep to the same policy

**Files:**
- Modify: `crates/dexo-tui/src/update.rs:969-972` (`Action::OpenReview`), `apply_ddl` (~5794)

**Interfaces:**
- Consumes: `dexo_app::Environment::parse_strict` (Task 2).

- [ ] **Step 1: The review learns where it is**

`DataScreen.environment` is set nowhere outside a test, so `ReviewModal.production` is always false and production edits apply without the `y` confirmation. Replace the `Action::OpenReview` arm:

```rust
        Action::OpenReview => {
            // The review asks on production only if it knows it is on production, and
            // nothing outside a test told it.
            model.data.environment =
                dexo_app::Environment::parse_strict(&model.connection.environment);
            model.data.open_review();
            Vec::new()
        }
```

- [ ] **Step 2: The schema editor refuses on read-only**

At the top of `fn apply_ddl`, before reading the preview, the same refusal `apply_changes` has:

```rust
    if model.connection.read_only {
        model.messages.warn("connection is read-only".into());
        return Vec::new();
    }
```

- [ ] **Step 3: Validate**

Run: `cargo build -p dexo-tui` and `cargo test -p dexo-tui --lib data` (the existing `review_states_require_production_confirm` still passes).

- [ ] **Step 4: CI gates and commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/dexo-tui/src/update.rs
git commit -m "fix(tui): grid edits on production wait for confirmation again"
```

---

### Task 5: MySQL refuses writes on a read-only connection

**Files:**
- Modify: `crates/dexo-driver-mysql/src/factory.rs:31-43`
- Modify: `crates/dexo-driver-mysql/tests/query.rs` (new test at the end)
- Modify: `docs/src/connections.md`

**Interfaces:**
- Consumes: `ConnectRequest.read_only` (exists, `crates/dexo-driver-api/src/connection.rs:69`).

- [ ] **Step 1: Send the access mode on every connection**

In `MysqlFactory::connect`, after the `OptsBuilder` is built and before the TLS block:

```rust
        if request.read_only {
            // `init` runs on every connection these options open, the cancel connection
            // and reconnects included, so the server refuses the write, not only Dexo.
            builder = builder.init(vec!["SET SESSION TRANSACTION READ ONLY"]);
        }
```

- [ ] **Step 2: Pin it against a real server**

Append to `crates/dexo-driver-mysql/tests/query.rs`:

```rust
async fn write_fails(session: &dyn Session, sql: &str) -> bool {
    match session.execute(QueryRequest::write(sql)).await {
        Err(_) => true,
        Ok(mut stream) => {
            let mut failed = false;
            while let Some(event) = stream.next().await {
                failed |= event.is_err();
            }
            failed
        }
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_read_only_session_refuses_writes_on_the_server() {
    let pair = DatabasePair::start().await.unwrap();
    let request = |read_only| {
        ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            read_only,
        )
    };
    let writer = MysqlFactory.connect(request(false)).await.unwrap();
    assert!(
        !write_fails(
            &*writer,
            "create table if not exists ro_probe (id int primary key)"
        )
        .await
    );
    let reader = MysqlFactory.connect(request(true)).await.unwrap();
    assert!(write_fails(&*reader, "insert into ro_probe values (1)").await);
    assert!(write_fails(&*reader, "delete from ro_probe").await);
    assert!(write_fails(&*reader, "create table ro_probe_2 (id int)").await);
    let mut stream = reader
        .execute(QueryRequest::read("select count(*) from ro_probe", 1))
        .await
        .unwrap();
    first_value(&mut stream).await;
}
```

Run: `cargo test -p dexo-driver-mysql --test query -- --ignored a_read_only_session_refuses_writes_on_the_server`
Expected: PASS (needs Docker). If the `create table ro_probe_2` assertion fails, the server accepts DDL in a read-only session: drop that assertion, and add to the doc text in Step 3 that on MySQL the editor's refusal is what stops DDL.

- [ ] **Step 3: Document it**

Append to the "Environments and the SQL editor" section of `docs/src/connections.md`:

```markdown
A read-only connection is also enforced by the server: Postgres sessions start with `default_transaction_read_only`, MySQL sessions with `SET SESSION TRANSACTION READ ONLY`.
```

- [ ] **Step 4: CI gates and commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/dexo-driver-mysql/src/factory.rs crates/dexo-driver-mysql/tests/query.rs docs/src/connections.md
git commit -m "fix(mysql): a read-only connection refuses writes on the server"
```

---

### Task 6: The welcome says how to add a connection

**Files:**
- Modify: `crates/dexo-tui/src/render.rs:358-380` (onboarding lines)
- Modify: `crates/dexo-tui/tests/onboarding.rs:24`

- [ ] **Step 1: One more line in both layouts**

In the compact branch, after `lines.push("F1  help".into());`:

```rust
        lines.push("n  new connection".into());
```

In the full branch, after `lines.push("F1 opens help.".into());`:

```rust
        lines.push("n in the explorer adds a connection.".into());
```

(`n` is `connection.new` in the `[explorer]` section of every keymap profile, `keymap.rs:435`, `:547`, `:633`.)

- [ ] **Step 2: Extend the existing check**

In `onboarding_explains_the_first_steps`, after `assert!(screen.contains("F1"));`:

```rust
    assert!(screen.contains("adds a connection"));
```

Run: `cargo test -p dexo-tui --test onboarding`
Expected: PASS.

- [ ] **Step 3: CI gates and commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
git add crates/dexo-tui/src/render.rs crates/dexo-tui/tests/onboarding.rs
git commit -m "fix(tui): the welcome says how to add a connection"
```

---

## Traceability

| Spec | Task |
|---|---|
| A1 editor guard: read-only refuses, production typed name, destructive confirm, `SET`/unknown as writes, guard in `start_query` | 1, 2, 3 |
| A1 unknown labels count as production | 2 (`parse_strict`), 3 |
| A1 grid review production confirmation, schema editor read-only | 4 |
| A4 MySQL read-only on the server; Postgres not switchable from the editor | 5, 3 (`the_server_side_guard_cannot_be_switched_off_from_the_editor`) |
| A4 a driver that cannot enforce read-only refuses to connect | Postgres and MySQL both enforce after Task 5; the rule binds the SQLite and DuckDB drivers (README, "What later plans must honour") |
| A5 welcome names `n` | 6 |
