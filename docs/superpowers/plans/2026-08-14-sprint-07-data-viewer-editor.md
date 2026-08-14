# Dexo Sprint 07: Data Viewer and Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Completar visualização paginada e edição segura de linhas com change sets, conflitos e navegação por chaves.

**Architecture:** `DataSource` describes server-side paging/filter/sort; `ChangeSet` is pure domain state. Drivers render bound mutations from trusted metadata; the TUI never constructs SQL by string concatenation.

**Tech Stack:** existing driver/runtime/TUI stack, serde_json, csv formatting utilities, proptest.

---

## File map

- Create: `dexo-app/src/data/{mod.rs,source.rs,filter.rs,change_set.rs,apply.rs,foreign_key.rs}`
- Create: `dexo-driver-api/src/mutation.rs`
- Create: driver `mutation.rs` modules
- Extend: TUI grid and data screen
- Test: shared mutation contracts and large-value snapshots.

### Task 1: Add server paging, sorting and filtering model

**Files:** data source/filter and driver API.

- [x] **Step 1:** Write a failing test proving filters are typed AST nodes, not raw SQL: `Filter::Eq(ColumnId("age"), DbValue::I64(18))`.
- [x] **Step 2:** Run target; expect missing types.
- [x] **Step 3:** Implement `DataRequest { object, columns, filter, sort, page }`, `Page { offset, limit<=10_000 }`, boolean filter AST and null-aware operators. Each driver quotes catalog metadata and binds values.
- [x] **Step 4:** Run property tests for identifiers/values and Docker paging contracts.
- [x] **Step 5:** Commit with `git commit -m "feat(data): add typed server paging and filters"`.

### Task 2: Complete grid selection and copy formats

**Files:** TUI grid/data screen/tests.

- [x] **Step 1:** Add snapshots/tests for cell/row/column/range selection, frozen/hidden columns and copy as text/CSV/TSV/JSON/Markdown/SQL.
- [x] **Step 2:** Run tests; expect failures.
- [x] **Step 3:** Implement formatters that distinguish NULL, empty text and empty bytes; SQL copy delegates quoting/literals to driver dialect; never copy truncated bytes as complete.
- [x] **Step 4:** Run snapshot and formatter golden tests.
- [x] **Step 5:** Commit with `git commit -m "feat(data): add complete grid selection and copy"`.

### Task 3: Render and fetch large/native values safely

**Files:** value viewer widget and app value loader.

- [x] **Step 1:** Add tests for JSON pretty view, text, XML, array, binary hex, recognized image metadata and explicit `Truncated { loaded,total }`.
- [x] **Step 2:** Run target; expect viewer absent.
- [x] **Step 3:** Implement inline byte threshold, on-demand fetch token, max download policy and atomic save chosen locally by the user. Do not infer image by extension; inspect bounded magic bytes.
- [x] **Step 4:** Run large-value tests with 100MB fixture while asserting bounded cache bytes.
- [x] **Step 5:** Commit with `git commit -m "feat(data): inspect large and native values safely"`.

### Task 4: Build immutable row identities and change sets

**Files:** `change_set.rs`, driver mutation contract.

- [x] **Step 1: Write failing identity test**

```rust
#[test]
fn table_without_unique_identity_is_read_only() {
    assert_eq!(RowIdentity::from_table(&table_without_keys()), None);
    assert_eq!(ChangeSet::for_table(table_without_keys()).mode(), EditMode::ReadOnly);
}
```

- [x] **Step 2:** Run target; expect FAIL.
- [x] **Step 3:** Choose primary key, else non-null unique key; snapshot original identity values. Implement pending insert/update/delete states, validation errors and discard/revert without mutating loaded rows.
- [x] **Step 4:** Run change-set property tests for add/edit/delete/revert sequences.
- [x] **Step 5:** Commit with `git commit -m "feat(data): stage edits in safe change sets"`.

### Task 5: Render and apply bound mutations with conflict detection

**Files:** driver mutation modules, `apply.rs`, Docker contracts.

- [x] **Step 1:** Add shared contract: concurrent update changes original row, applying stale change returns `MutationConflict` and commits zero Dexo changes.
- [x] **Step 2:** Run both driver tests; expect FAIL.
- [x] **Step 3:** Generate INSERT/UPDATE/DELETE with bound parameters, quoted metadata identifiers, identity predicates and optional original-value version predicate. Execute one change set transactionally; validate affected rows exactly one; rollback on error.
- [x] **Step 4:** Run conflict, partial failure, cancel and successful batch tests on both databases.
- [x] **Step 5:** Commit with `git commit -m "feat(data): apply edits with optimistic conflicts"`.

### Task 6: Navigate foreign-key relationships

**Files:** `foreign_key.rs`, data screen.

- [x] **Step 1:** Add test mapping composite FK local columns to referenced columns and producing a typed target filter.
- [x] **Step 2:** Run target; expect missing navigator.
- [x] **Step 3:** Implement inbound/outbound relationship list, NULL handling, composite key order and open-related-data action in a new tab.
- [x] **Step 4:** Run navigation tests and TUI snapshot.
- [x] **Step 5:** Commit with `git commit -m "feat(data): navigate foreign-key records"`.

### Task 7: Expose review/apply/revert in TUI

**Files:** data screen, change review modal, status.

- [x] **Step 1:** Add reducer/snapshot tests for pending/applied/reverted/failed states, preview SQL and production confirmation.
- [x] **Step 2:** Run tests; expect FAIL.
- [x] **Step 3:** Implement review modal showing fully qualified target, operations, parameter-safe preview, affected row policy and transaction choice. Keep failed and unsubmitted changes editable.
- [x] **Step 4:** Run full sprint gate including both mutation contract suites.
- [x] **Step 5:** Commit with `git commit -m "feat(tui): review and apply data edits"`.

## Sprint exit

- [x] Grid supports all selection/copy/viewer requirements.
- [x] No identity means read-only by default.
- [x] Every mutation uses bound values and validated metadata identifiers.
- [x] Conflict or partial failure never silently commits unexpected rows.
- [x] Foreign-key navigation handles composite keys.
