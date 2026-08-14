# Dexo Sprint 05: SQL Editor Intelligence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Entregar documentos SQL completos, parsing tolerante, autocomplete contextual, parâmetros, snippets, histórico e execução de scripts.

**Architecture:** `dexo-sql` owns text and language services with dialect adapters. Editor state stays independent from TUI widgets; catalog lookup is an injected read-only interface and may use offline snapshots.

**Tech Stack:** Ropey 1.6.1, Tree-sitter 0.26.12, tree-sitter-sequel 0.3.11, sqlparser 0.62 as best-effort, Serde 1.0, rusqlite 0.40.

---

## File map

- Create: `crates/dexo-sql/src/{document.rs,edit.rs,parse.rs,statement.rs,dialect.rs,completion.rs,format.rs,diagnostic.rs,parameter.rs,snippet.rs,lib.rs}`
- Create: `crates/dexo-storage/src/{history.rs,snippet.rs,document.rs}`
- Create: `crates/dexo-tui/src/widgets/editor.rs`, `crates/dexo-tui/src/screens/workbench.rs`
- Test: SQL unit/property tests, editor snapshots, storage round-trips.

### Task 1: Build Unicode-safe document editing

**Files:** `document.rs`, `edit.rs`.

- [ ] **Step 1: Write failing Unicode undo test**

```rust
#[test]
fn unicode_edit_undo_redo_is_lossless() {
    let mut doc = SqlDocument::new("select 'ação'\n");
    doc.replace_chars(8..12, "café").unwrap();
    assert_eq!(doc.text(), "select 'café'\n");
    doc.undo().unwrap(); assert_eq!(doc.text(), "select 'ação'\n");
    doc.redo().unwrap(); assert_eq!(doc.text(), "select 'café'\n");
}
```

- [ ] **Step 2:** Run `cargo test -p dexo-sql unicode_edit_undo_redo_is_lossless`; expect FAIL.

- [ ] **Step 3:** Implement rope-backed char-indexed edits, cursor/selection, grouped undo/redo and monotonically increasing document revision. Reject ranges not on char boundaries.

- [ ] **Step 4:** Run unit and proptest sequences of insert/delete/undo; expect exact string model parity.

- [ ] **Step 5:** Commit with `git commit -m "feat(sql): add Unicode-safe document model"`.

### Task 2: Parse incomplete SQL incrementally

**Files:** `parse.rs`, `dialect.rs`, tests.

- [ ] **Step 1: Add failing incomplete-input test**

```rust
#[test]
fn incomplete_select_still_highlights_keywords() {
    let parsed = ParserService::postgres().parse("select * fro");
    assert!(parsed.highlights.iter().any(|h| h.kind == Highlight::Keyword && h.text == "select"));
    assert!(!parsed.regions.is_empty());
}
```

- [ ] **Step 2:** Run targeted test; expect missing parser.

- [ ] **Step 3:** Wire Tree-sitter incremental edits and highlight queries. Wrap best-effort `sqlparser` AST as `Option`; tree errors create local diagnostics but never make text unexecutable.

- [ ] **Step 4:** Run parser corpus for PostgreSQL/MySQL valid, invalid and incomplete fixtures; expect no panic.

- [ ] **Step 5:** Commit with `git commit -m "feat(sql): parse incomplete SQL incrementally"`.

### Task 3: Select statements and classify risk conservatively

**Files:** `statement.rs`, property tests.

- [ ] **Step 1: Add tests** for semicolons inside strings/comments/dollar quotes, current cursor statement and `WITH ... DELETE` classification.

```rust
#[test]
fn cte_delete_is_mutating() {
    let s = statement_at("WITH x AS (SELECT 1) DELETE FROM t WHERE id=1", 10).unwrap();
    assert_eq!(s.effect, StatementEffect::DataWrite);
}
```

- [ ] **Step 2:** Run tests; expect FAIL.

- [ ] **Step 3:** Implement `StatementSpan { byte_range, effect, understood }`; unknown syntax maps to `StatementEffect::Unknown`, never `ReadOnly`.

- [ ] **Step 4:** Run proptest ensuring concatenated spans cover non-whitespace input without overlap.

- [ ] **Step 5:** Commit with `git commit -m "feat(sql): split and classify statements safely"`.

### Task 4: Add contextual completion and navigation

**Files:** `completion.rs`, `dialect.rs`.

- [ ] **Step 1: Write failing alias completion test**

```rust
#[test]
fn completes_columns_for_alias() {
    let catalog = FakeCatalog::table("public.users", ["id", "email"]);
    let items = complete("select u. from public.users u", 9, &catalog, Dialect::Postgres);
    assert_eq!(labels(items), ["email", "id"]);
}
```

- [ ] **Step 2:** Run target; expect FAIL.

- [ ] **Step 3:** Implement context extraction for aliases, CTEs, SELECT/FROM/JOIN/WHERE, functions and keywords. Rank local aliases, current schema, favorites, recency, then lexical order. Add definition target IDs and signature help.

- [ ] **Step 4:** Run tests with online and offline fake catalogs; expect same deterministic ranking.

- [ ] **Step 5:** Commit with `git commit -m "feat(sql): add contextual completion and navigation"`.

### Task 5: Format and diagnose without corrupting SQL

**Files:** `format.rs`, `diagnostic.rs`.

- [ ] **Step 1: Add failing idempotence test**: `format(format(sql)) == format(sql)` and literals/comments unchanged.

- [ ] **Step 2:** Run test; expect formatter absent.

- [ ] **Step 3:** Implement dialect-aware token formatter with preview diff. Emit local parse diagnostics as `Local`; map server code/position as `Server`. Refuse format when token round-trip differs.

- [ ] **Step 4:** Run golden corpus; expect idempotence and exact literal preservation.

- [ ] **Step 5:** Commit with `git commit -m "feat(sql): format and diagnose safely"`.

### Task 6: Persist parameters, snippets and history safely

**Files:** `parameter.rs`, `snippet.rs`, storage repositories/migration 2.

- [ ] **Step 1: Write failing history privacy test**

```rust
#[test]
fn history_excludes_parameter_values_by_default() {
    let entry = HistoryEntry::new("select * from users where email=:email", [("email", "secret@example.com")]);
    let stored = entry.for_storage(HistoryPolicy::SqlOnly);
    assert!(stored.sql.contains(":email"));
    assert!(!serde_json::to_string(&stored).unwrap().contains("secret@example.com"));
}
```

- [ ] **Step 2:** Run target; expect FAIL.

- [ ] **Step 3:** Add migration 2 tables `sql_history`, `snippets`, `documents`; repositories with retention by age/count; typed parameter editor; snippet placeholders `${1:name}`; external file mtime/hash conflict detection.

- [ ] **Step 4:** Run storage migration v1->v2 and privacy tests.

- [ ] **Step 5:** Commit with `git commit -m "feat(workbench): persist documents snippets and safe history"`.

### Task 7: Execute selections, statements and scripts from TUI/CLI

**Files:** `dexo-app/query_service.rs`, workbench screen, CLI run.

- [ ] **Step 1: Add failing script test** proving three statements produce three result tabs in order and a failure stops sequential execution unless `--continue-on-error` is explicit.

- [ ] **Step 2:** Run app/CLI/TUI tests; expect FAIL.

- [ ] **Step 3:** Implement `ExecutionTarget::{Selection,CurrentStatement,Document}` and `ScriptPolicy::{StopOnError,ContinueOnError}`; bind parameters separately; expose commit/rollback/savepoints persistently in workbench status.

- [ ] **Step 4:** Run full sprint gate plus driver integration tests.

- [ ] **Step 5:** Commit with `git commit -m "feat(workbench): execute SQL documents and scripts"`.

## Sprint exit

- [ ] Unicode edit/undo property tests pass.
- [ ] Parser corpus never panics and highlights incomplete input.
- [ ] Unknown statements are never classified read-only.
- [ ] Autocomplete works from offline catalog.
- [ ] History privacy and v1->v2 migrations pass.
- [ ] TUI/CLI execute selection/current/document consistently.
