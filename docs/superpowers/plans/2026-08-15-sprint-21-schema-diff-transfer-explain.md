# Dexo Sprint 21: Schema, Diff, Transfer, Backup, and Explain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace fixture-backed schema, diff, transfer, native backup/restore, and explain screens with cancelable database operations whose previews and outcomes come from the selected driver and session.

**Architecture:** `WorkbenchRuntime` owns schema, transfer, native-tool, and explain operation managers. Domain services build typed requests and deterministic previews; driver capabilities render or execute database-specific work. Every long operation has an `OperationId`, progress events, cancellation, a terminal outcome, and targeted catalog invalidation.

**Tech Stack:** Tokio tasks and cancellation tokens, dexo-driver-api capability traits, PostgreSQL/MySQL DDL and EXPLAIN implementations, serde snapshots, tempfile, tokio::process, Ratatui overlays, Docker-backed integration tests.

---

## File map

Create `crates/dexo-tui/src/runtime/schema_manager.rs`, `transfer_manager.rs`, `native_tool_manager.rs`, `explain_manager.rs`, `screens/file_picker.rs`, `crates/dexo-storage/src/explain_plan.rs`, `crates/dexo-tui/tests/schema_transfer_explain_flow.rs`, and `crates/dexo/tests/tui_advanced_live.rs`.

Modify driver API DDL/explain contracts, both drivers' renderers and explain providers, app schema/diff/transfer/native-tool/explain services, storage migrations and exports, TUI action/model/update/render and affected screens, CLI transfer/explain paths, and integration workflows.

Requires Sprints 16–20 green.

### Task 1: Make DDL rendering a public driver capability

**Files:** `crates/dexo-driver-api/src/ddl.rs`, `connection.rs`, `lib.rs`; `crates/dexo-driver-postgres/src/ddl/{mod.rs,render.rs,execute.rs}`; `crates/dexo-driver-mysql/src/ddl/{mod.rs,render.rs,execute.rs}`; tests `crates/dexo-driver-{postgres,mysql}/tests/schema_diff.rs`.

- [ ] **Step 1: Write failing renderer contract tests**

```rust
#[test]
fn plan_change_uses_driver_quoting_and_reports_risk() {
    let plan = ddl().plan_change(&create_table("Sales", "Order")).unwrap();
    assert!(plan.statements[0].sql.contains("\"Sales\".\"Order\""));
    assert!(!plan.statements[0].sql.contains("Sales.Order"));
    assert_eq!(plan.risk.destructive, false);
}
```

Add the MySQL equivalent expecting backticks, plus drop/alter/grant cases for both drivers.

- [ ] **Step 2: Run the focused tests**

Run: `cargo test -p dexo-driver-postgres --test schema_diff && cargo test -p dexo-driver-mysql --test schema_diff`

Expected: FAIL because rendering is not exposed through `DdlExecutor`.

- [ ] **Step 3: Extend the capability without duplicating SQL generation**

```rust
#[async_trait]
pub trait DdlExecutor: Send + Sync {
    fn plan_change(&self, change: &SchemaChange) -> Result<DdlPlan, DriverError>;
    async fn apply_ddl(&self, plan: DdlPlan) -> Result<DdlOutcome, DriverError>;
}
```

Make each executor delegate `plan_change` to its existing renderer. Delete the generic `render_unquoted` production fallback from `dexo-app`; retain only test-local render helpers.

- [ ] **Step 4: Cover every supported `SchemaChange` variant**

Add table, view, index, rename, alter, grant, revoke, and raw-SQL cases. Unsupported variants must return `DriverError::Unsupported` naming the driver and change kind; they must never emit guessed SQL.

- [ ] **Step 5: Run driver and API suites, then commit**

Run: `cargo test -p dexo-driver-api -p dexo-driver-postgres -p dexo-driver-mysql`

Expected: PASS.

Commit: `feat(driver): expose database-specific ddl planning`

### Task 2: Execute schema forms through a protected operation

**Files:** `crates/dexo-app/src/schema/{change.rs,preview.rs,apply.rs,security.rs}`; create `crates/dexo-tui/src/runtime/schema_manager.rs`; modify `crates/dexo-tui/src/{action.rs,model.rs,update.rs,event.rs,render.rs}`, `screens/schema_editor.rs`, `modals/ddl_preview.rs`; test `crates/dexo-tui/tests/schema_transfer_explain_flow.rs`.

- [ ] **Step 1: Test the preview/apply/action lifecycle**

```rust
#[tokio::test]
async fn confirmed_schema_change_uses_selected_session_and_invalidates_scope() {
    let runtime = runtime_with_recording_ddl();
    let op = runtime.preview_schema(session_id(), add_column()).await.unwrap();
    assert!(matches!(op.confirmation, Confirmation::TypeTarget { .. }));
    runtime.apply_schema(op.operation_id, typed_target()).await.unwrap();
    assert_eq!(runtime.ddl_calls(), 1);
    assert_eq!(runtime.invalidations(), vec![CatalogScope::Table(table_id())]);
}
```

- [ ] **Step 2: Verify the current reducer-only behavior fails**

Run: `cargo test -p dexo-tui --test schema_transfer_explain_flow confirmed_schema_change_uses_selected_session_and_invalidates_scope`

Expected: FAIL.

- [ ] **Step 3: Add the runtime protocol**

```rust
pub enum SchemaEffect {
    Preview { operation_id: OperationId, session_id: SessionId, change: SchemaChange },
    Apply { operation_id: OperationId, confirmation: ConfirmationAnswer },
    Cancel { operation_id: OperationId },
}

pub enum SchemaAction {
    PreviewReady { operation_id: OperationId, plan: DdlPlan },
    Progress { operation_id: OperationId, completed: usize, total: usize },
    Finished { operation_id: OperationId, outcome: DdlOutcome },
    Failed { operation_id: OperationId, error: UiError },
}
```

The manager must revalidate session identity, connection policy, transaction state, and typed confirmation immediately before apply.

- [ ] **Step 4: Wire forms and preview overlay**

Convert form fields to typed `SchemaChange`; display exact driver SQL, risk, affected objects, implicit-commit warning, and reversibility. Disable Apply when the capability is unavailable, the session is read-only, or validation fails.

- [ ] **Step 5: Invalidate only affected catalog scopes**

On complete success, invalidate the changed object and parents. On partial or uncertain outcome, mark the entire connection cache stale and require refresh. Preserve the generated script and driver error in the operation log.

- [ ] **Step 6: Test and commit**

Run: `cargo test -p dexo-app schema -p dexo-tui --test schema_transfer_explain_flow`

Expected: PASS.

Commit: `feat(tui): execute protected schema changes`

### Task 3: Load and compare real schema snapshot sources

**Files:** `crates/dexo-app/src/schema_diff/{snapshot.rs,normalize.rs,diff.rs,graph.rs,script.rs,mod.rs}`; `crates/dexo-storage/src/schema_snapshot.rs`; create `crates/dexo-tui/src/runtime/schema_manager.rs`; modify `screens/schema_diff.rs`, TUI action/model/update; test `schema_transfer_explain_flow.rs`.

- [ ] **Step 1: Add a failing live-versus-saved diff test**

```rust
#[tokio::test]
async fn diff_loads_both_selected_sources_instead_of_fixture_objects() {
    let runtime = runtime_with_catalog_and_snapshot();
    let result = runtime.diff(DiffRequest {
        left: DiffSource::SavedSnapshot(snapshot_id()),
        right: DiffSource::Live(session_id()),
        filters: DiffFilters::all(),
        renames: vec![],
    }).await.unwrap();
    assert_eq!(result.changes[0].object_name(), "public.orders");
}
```

- [ ] **Step 2: Run and observe the fixture-backed failure**

Run: `cargo test -p dexo-tui --test schema_transfer_explain_flow diff_loads_both_selected_sources_instead_of_fixture_objects`

Expected: FAIL.

- [ ] **Step 3: Define explicit sources and normalized snapshot envelopes**

```rust
pub enum DiffSource {
    Live(SessionId),
    SavedSnapshot(SnapshotId),
    JsonFile(PathBuf),
}

pub struct SnapshotEnvelope {
    pub format_version: u32,
    pub driver: DriverId,
    pub server_version: String,
    pub captured_at: DateTime<Utc>,
    pub objects: Vec<CatalogObject>,
}
```

Reject unknown future versions and driver-incompatible comparisons with a precise message. Never silently coerce unsupported object kinds.

- [ ] **Step 4: Connect source selectors, filters, refresh, and rename mapping**

The overlay must load each source asynchronously, show capture timestamps and stale/offline state, filter by object kind/schema, and let users explicitly map renames before recomputing dependency order.

- [ ] **Step 5: Persist and import snapshots atomically**

Use `SchemaSnapshotRepository` for local snapshots and the atomic file helper from Sprint 16 for JSON import/export. Validate the entire envelope before replacing UI state.

- [ ] **Step 6: Test and commit**

Run: `cargo test -p dexo-app schema_diff -p dexo-storage schema_snapshot -p dexo-tui --test schema_transfer_explain_flow`

Expected: PASS.

Commit: `feat(schema): compare live saved and file snapshots`

### Task 4: Apply a schema diff with ordering, confirmation, and recovery evidence

**Files:** `crates/dexo-app/src/schema_diff/{graph.rs,risk.rs,script.rs,mod.rs}`, `schema/apply.rs`; `crates/dexo-tui/src/runtime/schema_manager.rs`, `screens/schema_diff.rs`, `action.rs`, `update.rs`; test `schema_transfer_explain_flow.rs`.

- [ ] **Step 1: Test partial completion and uncertain state**

```rust
#[tokio::test]
async fn failed_diff_records_completed_statement_and_marks_cache_uncertain() {
    let outcome = apply_diff_with_failure_on_statement(2).await;
    assert_eq!(outcome.completed, vec![statement_id(1)]);
    assert_eq!(outcome.failed, Some(statement_id(2)));
    assert!(outcome.catalog_state.is_uncertain());
}
```

- [ ] **Step 2: Add a typed `MigrationOperation`**

Store source fingerprints, ordered changes, rendered SQL, confirmations, completed statement IDs, failure, start/end timestamps, and whether the driver executed atomically. Do not promise rollback when the server performs implicit DDL commits.

- [ ] **Step 3: Require a fresh preview before apply**

Hash the source fingerprints and rendered plan. If either live source changes between preview and apply, refuse execution and request a re-diff. Destructive changes require typed target confirmation.

- [ ] **Step 4: Stream statement progress and preserve a rerunnable remainder**

Dispatch `SchemaAction::Progress` after each statement. On failure, generate a new script containing only unapplied statements, clearly marked as requiring review rather than safe automatic resume.

- [ ] **Step 5: Test and commit**

Run: `cargo test -p dexo-app schema_diff -p dexo-tui --test schema_transfer_explain_flow failed_diff_records_completed_statement_and_marks_cache_uncertain`

Expected: PASS.

Commit: `feat(schema): apply reviewed schema diffs safely`

### Task 5: Add a real terminal file picker and streaming transfer operations

**Files:** create `crates/dexo-tui/src/screens/file_picker.rs`, `runtime/transfer_manager.rs`; modify `crates/dexo-app/src/transfer/{export.rs,import.rs,codec.rs,map.rs,rejects.rs}`, `crates/dexo-tui/src/screens/transfer.rs`, TUI action/model/update/render, `crates/dexo-cli/src/run.rs`; test `schema_transfer_explain_flow.rs` and `crates/dexo-cli/tests/transfer.rs`.

- [ ] **Step 1: Test path selection and bounded streaming**

```rust
#[tokio::test]
async fn export_writes_batches_without_buffering_the_dataset() {
    let sink = RecordingSink::new();
    export_rows(three_batches_of(1_000), sink.clone()).await.unwrap();
    assert_eq!(sink.max_rows_held(), 1_000);
    assert_eq!(sink.rows_written(), 3_000);
}
```

Add picker tests for parent navigation, hidden-file toggle, drive roots on Windows, absolute-path entry, overwrite confirmation, and inaccessible directories.

- [ ] **Step 2: Run focused failures**

Run: `cargo test -p dexo-app transfer -p dexo-tui --test schema_transfer_explain_flow file_picker`

Expected: FAIL.

- [ ] **Step 3: Implement the terminal-native path browser**

Use `std::fs::read_dir` in a blocking worker. Return a validated absolute `PathBuf`; do not invoke platform GUI dialogs. Keep current directory, selection, sort, filter, and error in screen state.

- [ ] **Step 4: Stream export and import through the selected live session**

Export must consume driver row batches and write CSV, TSV, JSON, JSONL, or SQL incrementally to a temporary sibling file, then atomically replace the destination. Import must stream decoded batches into `BulkWriter`, honor stop/continue/reject policies, and write a structured rejects file.

- [ ] **Step 5: Expose truthful progress and cancellation**

Report bytes, rows, elapsed time, throughput, rejects, current phase, and whether totals are known. Cancellation must stop fetching/writing, remove incomplete temporary output, and leave the database outcome explicit when a batch was already committed.

- [ ] **Step 6: Reuse the same service from CLI and TUI**

Move orchestration out of `dexo-cli/src/run.rs`; both adapters construct `TransferRequest` and consume identical `TransferEvent` values.

- [ ] **Step 7: Test and commit**

Run: `cargo test -p dexo-app transfer -p dexo-cli --test transfer -p dexo-tui --test schema_transfer_explain_flow`

Expected: PASS.

Commit: `feat(transfer): stream real imports and exports`

### Task 6: Replace production fake native tools with a secure process runner

**Files:** `crates/dexo-app/src/transfer/native_tool.rs`; create `crates/dexo-tui/src/runtime/native_tool_manager.rs`; modify `screens/transfer.rs`, TUI action/model/update; test `crates/dexo-app/tests/native_tool.rs`, `crates/dexo/tests/tui_advanced_live.rs`.

- [ ] **Step 1: Write runner tests against temporary executable scripts**

```rust
#[tokio::test]
async fn cancellation_kills_the_child_and_removes_secret_material() {
    let temp = TempDir::new().unwrap();
    let runner = test_runner(&temp, slow_tool_script(&temp));
    let handle = runner.start(backup_request()).await.unwrap();
    handle.cancel().await.unwrap();
    assert!(!handle.secret_file().exists());
    assert_eq!(handle.outcome().await.unwrap().status, NativeStatus::Cancelled);
}
```

Cover executable discovery, version parsing, stderr progress, non-zero exit, output overwrite, paths with spaces, and secret cleanup on panic/drop for Windows and Unix test helpers.

- [ ] **Step 2: Run and prove production fakes are insufficient**

Run: `cargo test -p dexo-app --test native_tool`

Expected: FAIL.

- [ ] **Step 3: Introduce an injectable process boundary**

```rust
#[async_trait]
pub trait ProcessRunner: Send + Sync {
    async fn spawn(&self, spec: ProcessSpec) -> Result<Box<dyn RunningProcess>, NativeToolError>;
}

pub struct NativeToolRunner<R: ProcessRunner> {
    process: R,
    resolver: NativeToolResolver,
}
```

Production uses `tokio::process::Command`; unit tests inject a recording runner. Remove `fake_pg_dump`, `FakeChild`, and any production branch that manufactures success.

- [ ] **Step 4: Secure credentials and command construction**

For PostgreSQL use a permission-restricted temporary passfile; for MySQL use a permission-restricted temporary option file. Never place passwords in argv, logs, diagnostic bundles, or progress events. Resolve `pg_dump`/`pg_restore`/`mysqldump`/`mysql`, run `--version`, and warn on major-version incompatibility before execution.

- [ ] **Step 5: Implement backup and restore forms**

Support schema/data selection, format, compression where the tool supports it, destination/source, extra safe options from an enumerated allowlist, exact command preview with secrets redacted, typed confirmation for restore, progress, cancellation, and terminal outcome.

- [ ] **Step 6: Add opt-in live tests**

Run: `cargo test -p dexo --test tui_advanced_live native_backup_restore -- --ignored --nocapture`

Expected: PASS when Docker databases and native clients are installed; otherwise the test must report the documented missing prerequisite, never silently pass.

- [ ] **Step 7: Commit**

Commit: `feat(transfer): run secure native backup and restore`

### Task 7: Execute EXPLAIN for the current statement and persist plans

**Files:** `crates/dexo-app/src/explain_service.rs`; `crates/dexo-driver-api/src/explain.rs`; both driver `src/explain.rs`; create `crates/dexo-storage/src/explain_plan.rs`, `crates/dexo-tui/src/runtime/explain_manager.rs`; modify storage migrations/lib, TUI `screens/explain.rs`, action/model/update/render; test `schema_transfer_explain_flow.rs`.

- [ ] **Step 1: Add migration 11 for saved plans**

```sql
CREATE TABLE explain_plans (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    connection_id TEXT REFERENCES connections(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    driver TEXT NOT NULL,
    server_version TEXT NOT NULL,
    sql_fingerprint TEXT NOT NULL,
    analyzed INTEGER NOT NULL,
    plan_json TEXT NOT NULL,
    captured_at TEXT NOT NULL
);
```

Test migration from a version-10 database and CRUD isolation by project.

- [ ] **Step 2: Test selection, confirmation, and correlation**

```rust
#[tokio::test]
async fn explain_uses_statement_at_editor_cursor() {
    let runtime = runtime_with_recording_explainer();
    runtime.explain(editor_with_cursor_in_second_statement(), false).await.unwrap();
    assert_eq!(runtime.explain_sql(), "SELECT * FROM orders");
}
```

Also prove `analyze=true` does not execute until the dedicated confirmation is accepted.

- [ ] **Step 3: Normalize complete PostgreSQL and MySQL plan trees**

Populate node type, relation, alias, estimated/actual rows, costs, loops, timing, buffers where available, warnings, planning/execution time, and raw driver JSON. Unknown fields remain in raw JSON so newer servers do not lose information.

- [ ] **Step 4: Wire real views and cancellation**

Replace `OpenExplain` fixtures with `Effect::Explain`; render tree, table, summary, raw JSON, loading, unsupported, error, and canceled states. Correlate results by operation ID and selected session.

- [ ] **Step 5: Save and compare plans**

Compare nodes by stable path plus normalized relation identity. Show added/removed nodes and deltas in cost, estimates, actual rows, time, loops, and buffers. Warn when SQL fingerprint, driver, or major server version differs.

- [ ] **Step 6: Test and commit**

Run: `cargo test -p dexo-driver-postgres explain -p dexo-driver-mysql explain -p dexo-storage explain_plan -p dexo-tui --test schema_transfer_explain_flow explain`

Expected: PASS.

Commit: `feat(explain): inspect save and compare real plans`

### Task 8: Prove advanced operations against PostgreSQL and MySQL

**Files:** `crates/dexo/tests/tui_advanced_live.rs`, `.github/workflows/integration.yml`, `docs/testing.md`, `docs/user-guide/schema-and-transfer.md`.

- [ ] **Step 1: Add ignored live scenarios**

Cover create/alter/drop with refresh, diff live-to-snapshot, partial failure reporting, CSV/JSONL export/import, cancel mid-transfer, EXPLAIN, EXPLAIN ANALYZE confirmation, and plan comparison for both databases.

- [ ] **Step 2: Make CI run the ignored scenarios explicitly**

Use service health checks and execute:

```bash
cargo test -p dexo --test tui_advanced_live -- --ignored --test-threads=1
```

Expected: PostgreSQL and MySQL jobs both execute tests rather than reporting zero tests.

- [ ] **Step 3: Add fixture/fake regression checks**

Run in CI:

```bash
rg -n "fixture_|fake_pg_dump|FakeChild" crates/dexo-tui/src crates/dexo-app/src
```

Expected: no production matches. Test-only builders remain under `tests/` or `#[cfg(test)]` and use names describing recorded behavior.

- [ ] **Step 4: Run the sprint gate**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace --no-fail-fast`

Expected: PASS.

- [ ] **Step 5: Update the user guide and commit**

Document DDL confirmation, snapshot sources, transfer reject handling, native-tool prerequisites, secret redaction, EXPLAIN ANALYZE side effects, and plan comparison limits.

Commit: `test: verify advanced database operations end to end`

## Sprint 21 exit criteria

- [ ] Schema forms render and execute dialect-correct DDL through the chosen live session.
- [ ] Schema diff compares live, saved, and imported sources and applies only a freshly reviewed plan.
- [ ] Export/import streams real data with truthful progress, rejects, cleanup, and cancellation.
- [ ] Backup/restore invokes real native tools without exposing secrets.
- [ ] EXPLAIN uses the statement under the cursor; analyzed plans require confirmation and can be saved/compared.
- [ ] No production schema, diff, transfer, or explain path creates fixture data or synthetic success.
- [ ] Unit, integration, ignored-live, clippy, and formatting gates pass.
