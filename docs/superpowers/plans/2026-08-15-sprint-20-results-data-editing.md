# Dexo Sprint 20: Results and Data Editing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver real result-set tabs, server-backed data browsing, cross-platform copying, large-value viewers, protected row editing, and foreign-key navigation.

**Architecture:** Query tabs remain immutable bounded streams; table-data tabs use typed `DataRequest` for remote paging/sort/filter and stable row identities. Grid cells may be inline, remote-deferred, or locally spooled; mutations are applied through `DataMutator` only after review and policy checks.

**Tech Stack:** Ratatui virtual grid, dexo-driver-api data capabilities, arboard, ratatui-image, image crate, tempfile, Tokio, PostgreSQL/MySQL prepared mutations.

---

## File map

Create `crates/dexo-tui/src/runtime/data_manager.rs`, `result_spool.rs`, `screens/data_browser.rs`, `screens/value_viewer.rs`, `widgets/image_viewer.rs`, `tests/data_flow.rs`, and `crates/dexo/tests/tui_data_live.rs`.

Modify driver API mutation/value contracts, both driver mutation implementations, app data modules, TUI model/grid/data/action/update/render, and clipboard runtime.

Requires Sprints 16–19 green.

### Task 1: Model independent result sets and bounded grid cells

**Files:** `crates/dexo-tui/src/model.rs`, `widgets/grid.rs`, `screens/workbench.rs`, `action.rs`, `update.rs`, tests `data_flow.rs`.

- [x] **Step 1: Write a failing multi-result isolation test**

```rust
#[test]
fn batches_update_only_the_correlated_result_set() {
    let mut model = model_with_two_running_results();
    update(&mut model, rows_action(result_key(1), vec![vec![DbValue::I64(2)]]));
    assert_eq!(model.results.tabs[0].grid.row_count(), 0);
    assert_eq!(model.results.tabs[1].grid.row_count(), 1);
}
```

- [x] **Step 2: Run and verify current global-grid failure**

Run: `cargo test -p dexo-tui --test data_flow batches_update_only_the_correlated_result_set`

Expected: FAIL.

- [x] **Step 3: Add typed result tabs and cells**

```rust
pub enum GridCell { Inline(DbValue), Spool { id: uuid::Uuid, loaded: u64, total: u64 }, Remote(RemoteValueRef) }
pub struct ResultTab {
    pub key: ResultKey,
    pub title: String,
    pub grid: GridModel,
    pub status: OperationStatus,
    pub rows_affected: Option<u64>,
    pub notices: Vec<String>,
}
```

Every meta/rows/finish action contains `ResultKey`; stale or closed tabs ignore it. Keep viewport rendering O(visible rows × visible columns).

- [x] **Step 4: Test and commit**

Run: `cargo test -p dexo-tui --test data_flow result && cargo bench -p dexo-tui grid_viewport`

Expected: isolation PASS and viewport budget unchanged.

```powershell
git add crates/dexo-tui/src/{model.rs,action.rs,update.rs} crates/dexo-tui/src/widgets/grid.rs crates/dexo-tui/src/screens/workbench.rs crates/dexo-tui/tests/data_flow.rs
git commit -m "feat(tui): isolate real result set tabs"
```

### Task 2: Fetch table data with remote paging, sorting, and typed filters

**Files:** driver API mutation, both mutation drivers, app data source/filter, runtime/data manager, screen/data browser, tests and live tests.

- [x] **Step 1: Add live paging/sort/filter contracts**

Seed 250 rows and assert page 2, descending sort, `And` typed filters, NULL, decimal, Unicode, and invalid columns. Assert values are bound and identifiers dialect-quoted.

Run: `cargo test -p dexo-driver-postgres -p dexo-driver-mysql --test mutation paging -- --ignored --nocapture`

Expected: baseline paging may pass; total/count and invalid-column cases fail.

- [x] **Step 2: Extend `DataPage` and request validation**

```rust
pub struct DataPage {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<DbValue>>,
    pub offset: u64,
    pub has_more: bool,
    pub estimated_total: Option<u64>,
}
```

Validate requested columns/filter/sort against introspected table columns before rendering SQL. Fetch `limit + 1` to set `has_more` without mandatory count.

- [x] **Step 3: Implement TUI data-load effects**

`OpenObjectData`, `ChangeDataPage`, `ApplyRemoteSort`, and `ApplyRemoteFilter` call the active session's `DataMutator::fetch`. Keep the previous page visible while loading; only matching generation replaces it. Display exact filter AST as chips.

Expected: PASS.

```powershell
git add crates/dexo-driver-api/src/mutation.rs crates/dexo-driver-postgres/src/mutation.rs crates/dexo-driver-mysql/src/mutation.rs crates/dexo-app/src/data crates/dexo-tui/src/runtime/data_manager.rs crates/dexo-tui/src/screens/data_browser.rs crates/dexo-tui/src crates/dexo-tui/tests/data_flow.rs
git commit -m "feat(data): browse tables with remote paging and filters"
```

### Task 3: Support safe remote sorting/filtering for arbitrary SELECT results

**Files:** `dexo-sql` new derived query module, app script/data, TUI result actions/tests.

- [x] **Step 1: Write conservative rewrite tests**

```rust
#[test]
fn wraps_only_one_read_only_select_without_locking_or_terminator() {
    assert!(derive_page("select id,name from users", &sort(), &filter(), page()).is_ok());
    assert!(derive_page("update users set name='x'", &sort(), &filter(), page()).is_err());
    assert!(derive_page("select * from users for update", &sort(), &filter(), page()).is_err());
}
```

- [x] **Step 2: Run and verify missing API**

Run: `cargo test -p dexo-sql wraps_only_one_read_only_select_without_locking_or_terminator`

Expected: FAIL.

- [x] **Step 3: Implement dialect-safe derived queries**

Parse exactly one read-only statement, reject unknown syntax, locks, side-effecting functions when detectable, and existing multi-statements. Quote output-column identifiers and bind filter values. Mark unsupported query tabs `local-only` with a disabled reason rather than concatenating raw text.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-sql derived && cargo test -p dexo-tui --test data_flow arbitrary_select`

Expected: accepted/rejected corpus PASS.

```powershell
git add crates/dexo-sql/src crates/dexo-app/src crates/dexo-tui/src crates/dexo-tui/tests/data_flow.rs
git commit -m "feat(results): rerun validated select results remotely"
```

### Task 4: Copy selections to the real OS clipboard

**Files:** app copy module, TUI grid/data manager/clipboard, tests.

- [ ] **Step 1: Add format and clipboard-result tests**

Test cell/row/column/range for CSV, TSV, JSON, Markdown, SQL; distinguish NULL, empty text, empty bytes; assert clipboard failure yields `OperationFailed` and success follows adapter confirmation.

Run: `cargo test -p dexo-tui --test data_flow clipboard`

Expected: FAIL because copy only writes `model.data.clipboard`.

- [ ] **Step 2: Complete serializers**

```rust
pub enum CopyFormat { Csv, Tsv, Json, Markdown, SqlInsert }
```

SQL export requires a selected target table and uses dialect quoting/placeholders rendered to literals only for clipboard output. Cap clipboard bytes; larger selections offer file export.

- [ ] **Step 3: Dispatch clipboard I/O**

Move `arboard::Clipboard::set_text` to a blocking runtime task. Update toast/status only from `ClipboardWritten`/`OperationFailed`.

- [ ] **Step 4: Test and commit**

Run: `cargo test -p dexo-app data::copy && cargo test -p dexo-tui --test data_flow clipboard`

Expected: PASS, with headless adapter faked only in tests.

```powershell
git add crates/dexo-app/src/data/copy.rs crates/dexo-tui/src crates/dexo-tui/tests/data_flow.rs
git commit -m "feat(results): copy real selections to system clipboard"
```

### Task 5: Fetch, spool, inspect, render, and save large values

**Files:** driver API mutation/value, both drivers, app value, runtime result spool/data manager, value/image widgets, Cargo dependencies, tests.

- [ ] **Step 1: Add large text/blob contracts**

Seed a 40 MiB blob and JSON/XML/image values. Assert a table-data page returns a bounded remote reference, range fetch works, arbitrary-query ingestion spools above threshold, cancellation deletes partial files, and the grid never owns 40 MiB.

Run: `cargo test -p dexo-tui --test data_flow large_value`

Expected: FAIL.

- [ ] **Step 2: Add remote value capability**

```rust
pub struct RemoteValueRef { pub object: QualifiedName, pub identity: Vec<(ColumnId, DbValue)>, pub column: ColumnId, pub total: u64 }

#[async_trait::async_trait]
pub trait DataMutator: Send + Sync {
    async fn fetch(&self, request: DataRequest) -> Result<DataPage, DriverError>;
    async fn fetch_value(&self, value: &RemoteValueRef, offset: u64, limit: u32) -> Result<Vec<u8>, DriverError>;
    async fn apply(&self, mutations: &[Mutation]) -> Result<(), DriverError>;
}
```

Drivers use bound identity predicates and database substring functions. Only create a remote ref when stable identity exists; otherwise runtime spools the already-streamed value to a project temp directory.

- [ ] **Step 3: Implement real viewers**

Pretty-print JSON/XML, hex/UTF-8 bytes, arrays, image metadata, and progressive load. Decode images with bounded dimensions and use `ratatui_image::picker::Picker::from_query_stdio()` once; fall back to halfblocks/text metadata. Saving uses atomic output and refuses above configured limit until confirmed.

- [ ] **Step 4: Test memory/cleanup and commit**

Run: `cargo test -p dexo-tui --test data_flow large_value && cargo test -p dexo-app data::value`

Expected: PASS; temp directory empty after close/cancel.

```powershell
git add Cargo.toml crates/dexo-driver-api/src crates/dexo-driver-postgres/src/mutation.rs crates/dexo-driver-mysql/src/mutation.rs crates/dexo-app/src/data/value.rs crates/dexo-tui/src/runtime/{result_spool.rs,data_manager.rs} crates/dexo-tui/src/screens/value_viewer.rs crates/dexo-tui/src/widgets/image_viewer.rs crates/dexo-tui/tests/data_flow.rs
git commit -m "feat(data): inspect bounded large and image values"
```

### Task 6: Apply real change sets with conflict recovery

**Files:** app change set/apply, runtime data manager, screen data, grid/update, driver mutation tests, TUI/live tests.

- [ ] **Step 1: Add TUI-to-database change tests**

Test insert/update/delete, review SQL placeholders, production confirmation, read-only denial, no unique identity, concurrent modification conflict, partial failure rollback, reload/retry/revert.

Run: `cargo test -p dexo --test tui_data_live changes -- --ignored --nocapture`

Expected: FAIL because TUI apply changes only reducer state.

- [ ] **Step 2: Preserve stable identities and originals**

```rust
pub struct EditableRow {
    pub identity: Vec<(ColumnId, DbValue)>,
    pub original: Vec<DbValue>,
    pub current: Vec<DbValue>,
    pub state: RowEditState,
}
```

Introspect primary/unique keys. No safe identity means read-only. Build `Mutation` with original values for optimistic conflict detection.

- [ ] **Step 3: Dispatch protected apply**

Review lists exact operations but never secrets. `ApplyChanges` evaluates connection policy, asks typed confirmation when required, calls `DataMutator::apply`, then reloads the page. On conflict, retain edits and show reload/merge/retry/revert choices.

- [ ] **Step 4: Run and commit**

Run: `cargo test -p dexo-driver-postgres -p dexo-driver-mysql --test mutation -- --ignored --nocapture && cargo test -p dexo --test tui_data_live changes -- --ignored --nocapture`

Expected: PASS.

```powershell
git add crates/dexo-app/src/data crates/dexo-tui/src/runtime/data_manager.rs crates/dexo-tui/src/screens/data.rs crates/dexo-tui/src/{model.rs,action.rs,update.rs} crates/dexo-tui/src/widgets/grid.rs crates/dexo-tui/tests/data_flow.rs crates/dexo/tests/tui_data_live.rs
git commit -m "feat(data): review and apply protected row changes"
```

### Task 7: Navigate foreign keys with real queries

**Files:** app foreign key, catalog drivers if metadata missing, runtime/screen data, tests.

- [ ] **Step 1: Add composite-FK navigation tests**

Assert NULL FK disables navigation, composite mapping creates typed `And(Eq...)`, destination opens a separate data tab, back/forward breadcrumb restores page/filter, and permission failure is visible.

Run: `cargo test -p dexo-tui --test data_flow foreign_key`

Expected: FAIL because `open_related` only appends an empty tab.

- [ ] **Step 2: Load FK metadata and dispatch destination fetch**

Use catalog constraint attributes to build `ForeignKey`; call `related_filter`, then `DataMutator::fetch` for referenced table. Correlate the new tab and preserve origin breadcrumb.

- [ ] **Step 3: Run live tests and commit**

Run: `cargo test -p dexo --test tui_data_live foreign_key -- --ignored --nocapture`

Expected: PostgreSQL/MySQL composite and simple FK cases PASS.

```powershell
git add crates/dexo-app/src/data/foreign_key.rs crates/dexo-driver-postgres/src/catalog crates/dexo-driver-mysql/src/catalog crates/dexo-tui/src crates/dexo-tui/tests/data_flow.rs crates/dexo/tests/tui_data_live.rs
git commit -m "feat(data): navigate foreign keys through live data tabs"
```

### Task 8: Run the results/data sprint gate

- [ ] **Step 1: Run gates**

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --no-fail-fast
cargo test -p dexo-driver-postgres -p dexo-driver-mysql --test mutation -- --ignored --nocapture
cargo test -p dexo --test tui_data_live -- --ignored --nocapture
cargo bench -p dexo-tui grid_viewport
```

Expected: PASS and memory/viewport budgets hold.

- [ ] **Step 2: Confirm fixture grid is not used in production**

Run: `rg -n "fixture_rows|model\.data\.clipboard\s*=" crates/dexo-tui/src`

Expected: `fixture_rows` appears only under tests/benches; copy success is runtime-driven.

- [ ] **Step 3: Commit verified state**

```powershell
git add .
git commit -m "test(data): verify live result and editing workflows"
```

## Sprint 20 exit checklist

- [ ] Real rows fill independent result tabs.
- [ ] Server paging/sort/filter is typed and safe.
- [ ] Clipboard formats reach the OS.
- [ ] Large/native/image values are bounded, inspectable, and cleanly disposed.
- [ ] Change sets modify the database with conflict handling.
- [ ] FK navigation issues a real query.
