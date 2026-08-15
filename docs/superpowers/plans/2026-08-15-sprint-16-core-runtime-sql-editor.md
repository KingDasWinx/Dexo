# Dexo Sprint 16: Core Runtime and SQL Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the TUI into a real SQL workbench that owns live sessions, edits and persists SQL documents, streams queries, cancels work, and controls transactions against PostgreSQL and MySQL.

**Architecture:** Keep `Model`/`update` pure and introduce an async `WorkbenchRuntime` that owns sessions, a SQLite worker, cancellable tasks, and an action channel. The terminal loop selects between terminal input, runtime actions, timers, and shutdown; every async result is correlated by operation, session, document, and generation.

**Tech Stack:** Rust 1.93, Tokio 1.53, Ratatui 0.30, Crossterm 0.29, rusqlite 0.40, Ropey, tree-sitter-sequel, tokio-postgres 0.7.18, mysql_async 0.37, secrecy, testcontainers.

---

## Preconditions and file map

Read first:

- `docs/superpowers/specs/2026-08-15-dexo-functional-completion-design.md`
- `crates/dexo-tui/src/event.rs`
- `crates/dexo-tui/src/action.rs`
- `crates/dexo-tui/src/model.rs`
- `crates/dexo-tui/src/update.rs`
- `crates/dexo-app/src/query_service.rs`
- `crates/dexo-sql/src/document.rs`

Create:

- `crates/dexo-tui/src/runtime/mod.rs` — runtime bootstrap and exhaustive effect dispatch.
- `crates/dexo-tui/src/runtime/storage_worker.rs` — single-owner SQLite worker.
- `crates/dexo-tui/src/runtime/session_registry.rs` — live session ownership and generations.
- `crates/dexo-tui/src/runtime/query_runner.rs` — script streaming, timeout, cancellation, and result-set correlation.
- `crates/dexo-tui/src/runtime/document_io.rs` — atomic SQL file I/O and fingerprints.
- `crates/dexo-tui/src/screens/editor.rs` — editor commands and parser/completion state.
- `crates/dexo-tui/tests/runtime_query.rs` — fake-session runtime tests.
- `crates/dexo-tui/tests/editor_flow.rs` — keyboard and file persistence tests.
- `crates/dexo/tests/tui_query_live.rs` — ignored PostgreSQL/MySQL acceptance tests.

Modify:

- `crates/dexo-tui/Cargo.toml` — add `dexo-sql`, `sha2`, and test-support dependencies.
- `crates/dexo-tui/src/lib.rs` — export runtime and editor modules.
- `crates/dexo-tui/src/event.rs` — replace sequential `apply_effect` with `tokio::select!`.
- `crates/dexo-tui/src/action.rs` — typed async actions and effects.
- `crates/dexo-tui/src/model.rs` — documents, tabs, operation states, and IDs.
- `crates/dexo-tui/src/update.rs` — editor commands and real effect transitions.
- `crates/dexo-tui/src/widgets/editor.rs` — cursor, selection, highlights, diagnostics, and completion popup.
- `crates/dexo-driver-api/src/query.rs` — result-set and session-event contracts.
- `crates/dexo-driver-api/src/connection.rs` — optional session event stream.
- `crates/dexo-driver-postgres/src/factory.rs` and `src/session.rs` — notices, parameters, rows affected.
- `crates/dexo-driver-mysql/src/session.rs` — parameters, rows affected, multiple results.
- `crates/dexo-app/src/query_service.rs` and `src/script.rs` — task handle and timeout/cancel semantics.
- `crates/dexo-storage/src/document.rs`, `history.rs`, `recovery.rs`, `session_recovery.rs` — missing reads/lists and default-project recovery.

Do not implement the full connection browser, TLS/SSH/proxy form, projects UI, catalog, editable data, schema tools, or admin here. Sprint 16 keeps the existing create-connection form and makes that vertical real.

### Task 1: Define correlated operations and an exhaustive runtime boundary

**Files:**
- Modify: `crates/dexo-tui/src/action.rs`
- Create: `crates/dexo-tui/src/runtime/mod.rs`
- Modify: `crates/dexo-tui/src/lib.rs`
- Test: `crates/dexo-tui/tests/runtime_query.rs`

- [x] **Step 1: Write the failing operation-correlation test**

```rust
use dexo_tui::runtime::{OperationId, OperationKey};

#[test]
fn operation_key_rejects_a_stale_session_generation() {
    let operation = OperationKey::new(OperationId::new(), "session-a", "doc-a", 4);
    assert!(operation.belongs_to("session-a", "doc-a", 4));
    assert!(!operation.belongs_to("session-a", "doc-a", 3));
    assert!(!operation.belongs_to("session-b", "doc-a", 4));
}
```

- [x] **Step 2: Run the test and verify the missing runtime API**

Run: `cargo test -p dexo-tui --test runtime_query operation_key_rejects_a_stale_session_generation`

Expected: FAIL with unresolved import `dexo_tui::runtime`.

- [x] **Step 3: Add operation types and typed completion actions**

```rust
// crates/dexo-tui/src/runtime/mod.rs
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OperationId(pub Uuid);

impl OperationId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationKey {
    pub operation: OperationId,
    pub session: String,
    pub document: String,
    pub generation: u64,
}

impl OperationKey {
    pub fn new(operation: OperationId, session: impl Into<String>, document: impl Into<String>, generation: u64) -> Self {
        Self { operation, session: session.into(), document: document.into(), generation }
    }

    pub fn belongs_to(&self, session: &str, document: &str, generation: u64) -> bool {
        self.session == session && self.document == document && self.generation == generation
    }
}
```

Add `Action::{OperationStarted, OperationFailed, OperationCancelled}` carrying `OperationKey`; add `Effect::{ConnectProfile, StartScript, CancelOperation, BeginTransaction, CommitTransaction, RollbackTransaction, Savepoint, RollbackToSavepoint, ReleaseSavepoint, LoadDocument, SaveDocument, CheckpointRecovery, PersistHistory, Shutdown}`. Remove `StartQuery` if no reducer can emit it. Keep `CreateConnection` until Sprint 17 replaces the form.

- [x] **Step 4: Make dispatch exhaustive and keep I/O out of the reducer**

```rust
pub struct WorkbenchRuntime {
    action_tx: tokio::sync::mpsc::Sender<crate::Action>,
}

impl WorkbenchRuntime {
    pub async fn dispatch(&mut self, effect: crate::Effect) {
        match effect {
            crate::Effect::CreateConnection { input, password } => self.create_connection(input, password).await,
            crate::Effect::ConnectProfile { profile } => self.connect_profile(profile).await,
            crate::Effect::StartScript(request) => self.start_script(request),
            crate::Effect::CancelOperation(id) => self.cancel_operation(id).await,
            crate::Effect::BeginTransaction { session, mode } => self.begin(session, mode).await,
            crate::Effect::CommitTransaction { session } => self.commit(session).await,
            crate::Effect::RollbackTransaction { session } => self.rollback(session).await,
            crate::Effect::Savepoint { session, name } => self.savepoint(session, name).await,
            crate::Effect::RollbackToSavepoint { session, name } => self.rollback_to(session, name).await,
            crate::Effect::ReleaseSavepoint { session, name } => self.release_savepoint(session, name).await,
            crate::Effect::LoadDocument(request) => self.load_document(request).await,
            crate::Effect::SaveDocument(request) => self.save_document(request).await,
            crate::Effect::CheckpointRecovery(request) => self.checkpoint_recovery(request).await,
            crate::Effect::PersistHistory(request) => self.persist_history(request).await,
            crate::Effect::PersistLayout => self.persist_layout().await,
            crate::Effect::Shutdown => self.shutdown().await,
            crate::Effect::Quit => self.shutdown().await,
        }
    }
}
```

Every called method must be defined in this sprint; methods that delegate to later domains must not be added yet.

- [x] **Step 5: Run tests and commit**

Run: `cargo test -p dexo-tui --test runtime_query operation_key_rejects_a_stale_session_generation`

Expected: PASS.

```powershell
git add crates/dexo-tui/src/action.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-tui/src/lib.rs crates/dexo-tui/tests/runtime_query.rs
git commit -m "feat(tui): define correlated runtime effects"
```

### Task 2: Add the SQLite worker and real bootstrap

**Files:**
- Create: `crates/dexo-tui/src/runtime/storage_worker.rs`
- Modify: `crates/dexo-tui/src/runtime/mod.rs`
- Modify: `crates/dexo-tui/src/event.rs`
- Test: `crates/dexo-tui/tests/runtime_query.rs`

- [x] **Step 1: Write the failing worker round-trip test**

```rust
#[tokio::test]
async fn storage_worker_creates_and_loads_the_default_project() {
    let dir = tempfile::tempdir().unwrap();
    let worker = dexo_tui::runtime::storage_worker::StorageWorker::start(
        dir.path().join("dexo.db"),
    ).unwrap();
    let bootstrap = worker.bootstrap().await.unwrap();
    assert_eq!(bootstrap.active_project.name, "Default");
    assert!(bootstrap.connections.is_empty());
}
```

- [x] **Step 2: Verify the test fails**

Run: `cargo test -p dexo-tui --test runtime_query storage_worker_creates_and_loads_the_default_project`

Expected: FAIL because `StorageWorker` is missing.

- [x] **Step 3: Implement a single-owner worker with typed commands**

```rust
pub enum StorageCommand {
    Bootstrap { reply: tokio::sync::oneshot::Sender<anyhow::Result<BootstrapState>> },
    Shutdown,
}

pub struct StorageWorker {
    tx: std::sync::mpsc::Sender<StorageCommand>,
}

impl StorageWorker {
    pub fn start(path: std::path::PathBuf) -> anyhow::Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("dexo-storage".into()).spawn(move || {
            let db = dexo_storage::Database::open(path).expect("open local Dexo database");
            while let Ok(command) = rx.recv() {
                match command {
                    StorageCommand::Bootstrap { reply } => {
                        let result = bootstrap_state(db.connection());
                        let _ = reply.send(result);
                    }
                    StorageCommand::Shutdown => break,
                }
            }
        })?;
        Ok(Self { tx })
    }

    pub async fn bootstrap(&self) -> anyhow::Result<BootstrapState> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.tx.send(StorageCommand::Bootstrap { reply })?;
        receive.await?
    }
}
```

`bootstrap_state()` must create one persisted `Project { name: "Default" }` only when the table is empty, then load profiles, recovery state, and layout for its UUID.

- [x] **Step 4: Replace `Model::default()`-only startup**

Construct the worker and runtime before entering raw mode where possible. Send `Action::Bootstrapped` into `update()` before the first interactive frame. If bootstrap fails, restore the terminal and return `TuiError`; do not continue with an in-memory fake state.

- [x] **Step 5: Run, inspect, and commit**

Run: `cargo test -p dexo-tui --test runtime_query storage_worker_creates_and_loads_the_default_project`

Expected: PASS and the worker thread exits when dropped/shutdown.

```powershell
git add crates/dexo-tui/src/runtime/storage_worker.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-tui/src/event.rs crates/dexo-tui/tests/runtime_query.rs
git commit -m "feat(tui): bootstrap through local storage worker"
```

### Task 3: Retain live sessions and make transaction control real

**Files:**
- Create: `crates/dexo-tui/src/runtime/session_registry.rs`
- Modify: `crates/dexo-tui/src/runtime/mod.rs`
- Modify: `crates/dexo-tui/src/model.rs`
- Modify: `crates/dexo-tui/src/update.rs`
- Test: `crates/dexo-tui/tests/runtime_query.rs`

- [x] **Step 1: Write failing lifecycle tests with a fake session**

```rust
#[tokio::test]
async fn connected_session_survives_and_commit_reaches_the_driver() {
    let fake = std::sync::Arc::new(FakeSession::default());
    let mut registry = SessionRegistry::default();
    let id = registry.insert("connection-a", fake.clone());
    registry.commit(id).await.unwrap();
    assert_eq!(fake.commits.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(registry.get(id).is_some());
}

#[tokio::test]
async fn reconnect_is_refused_for_unknown_transaction() {
    let mut registry = SessionRegistry::default();
    let id = registry.insert("connection-a", std::sync::Arc::new(FakeSession::default()));
    registry.set_transaction(id, dexo_driver_api::TransactionState::Unknown).unwrap();
    assert!(registry.can_reconnect(id, true).is_err());
}
```

- [x] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test runtime_query connected_session_survives_and_commit_reaches_the_driver`

Expected: FAIL because `SessionRegistry` is missing.

- [x] **Step 3: Implement the registry and convert driver boxes to `Arc`**

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SessionId(pub uuid::Uuid);

pub struct ActiveSession {
    pub id: SessionId,
    pub connection: String,
    pub generation: u64,
    pub transaction: dexo_driver_api::TransactionState,
    pub session: std::sync::Arc<dyn dexo_driver_api::Session>,
}

#[derive(Default)]
pub struct SessionRegistry {
    sessions: std::collections::HashMap<SessionId, ActiveSession>,
}

impl SessionRegistry {
    pub fn insert(&mut self, connection: impl Into<String>, session: std::sync::Arc<dyn dexo_driver_api::Session>) -> SessionId {
        let id = SessionId(uuid::Uuid::new_v4());
        self.sessions.insert(id, ActiveSession { id, connection: connection.into(), generation: 1, transaction: dexo_driver_api::TransactionState::Idle, session });
        id
    }

    pub fn get(&self, id: SessionId) -> Option<&ActiveSession> { self.sessions.get(&id) }

    pub fn can_reconnect(&self, id: SessionId, read_only: bool) -> Result<(), String> {
        let active = self.get(id).ok_or_else(|| "session is closed".to_string())?;
        if !read_only || active.transaction != dexo_driver_api::TransactionState::Idle {
            return Err("unsafe reconnect requires an idle read-only operation".into());
        }
        Ok(())
    }
}
```

Implement begin/commit/rollback/savepoint methods by calling `TransactionService` and updating state only after the driver result. Convert `Box<dyn Session>` with `Arc::<dyn Session>::from(boxed)`.

- [x] **Step 4: Wire transaction effects and status actions**

`Action::TransactionChanged` must include `SessionId` and generation. `update()` changes the active document state only when IDs match. Add palette actions for begin and savepoint; commit/rollback remain disabled unless the driver-reported state permits them.

- [x] **Step 5: Run and commit**

Run: `cargo test -p dexo-tui --test runtime_query session -- --nocapture`

Expected: all session lifecycle tests PASS.

```powershell
git add crates/dexo-tui/src/runtime/session_registry.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/update.rs crates/dexo-tui/tests/runtime_query.rs
git commit -m "feat(tui): retain sessions and control transactions"
```

### Task 4: Fix the query contracts in both drivers

**Files:**
- Modify: `crates/dexo-driver-api/src/query.rs`
- Modify: `crates/dexo-driver-api/src/connection.rs`
- Create: `crates/dexo-driver-postgres/src/params.rs`
- Modify: `crates/dexo-driver-postgres/src/lib.rs`
- Modify: `crates/dexo-driver-postgres/src/factory.rs`
- Modify: `crates/dexo-driver-postgres/src/session.rs`
- Modify: `crates/dexo-driver-mysql/src/session.rs`
- Test: `crates/dexo-driver-postgres/tests/query.rs`
- Test: `crates/dexo-driver-mysql/tests/query.rs`

- [x] **Step 1: Add ignored failing contracts for parameters, affected rows, notices, timeout, and result sets**

```rust
#[tokio::test]
#[ignore = "requires Docker"]
async fn parameters_rows_affected_and_result_sets_are_observable() {
    let fixture = connect_postgres_fixture().await;
    let mut request = QueryRequest::write("insert into dexo_params(value) values ($1)");
    request.parameters = vec![DbValue::Text("bound-value".into())];
    let events = collect(fixture.session.execute(request).await.unwrap()).await;
    assert!(events.iter().any(|event| matches!(event, QueryEvent::Finished { rows_affected: Some(1), .. })));
}
```

Add the MySQL equivalent with `?`, plus a two-result script/procedure contract and a timeout contract that runs `pg_sleep`/`sleep` and expects `DriverErrorCategory::Timeout` or cancellation.

- [ ] **Step 2: Run the contracts and verify real failures**

Run: `cargo test -p dexo-driver-postgres -p dexo-driver-mysql --test query parameters_rows_affected_and_result_sets_are_observable -- --ignored --nocapture`

Expected: FAIL because parameters are ignored and affected rows are absent.

- [x] **Step 3: Extend the event contract without coupling driver-api to Tokio**

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum QueryEvent {
    ResultSetStarted { index: usize },
    Columns(Vec<ColumnMeta>),
    Rows(RowBatch),
    Notice { message: String },
    ResultSetFinished { index: usize, rows_affected: Option<u64> },
    Finished { rows_affected: Option<u64> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    Notice { severity: Option<String>, message: String },
}

pub type SessionEventStream = std::pin::Pin<Box<dyn futures_core::Stream<Item = SessionEvent> + Send>>;
```

Add `fn events(&self) -> Option<SessionEventStream> { None }` to `Session`.

- [x] **Step 4: Bind parameters and emit accurate completion**

PostgreSQL: implement `PgParam` in `params.rs`, including an explicit all-types NULL implementation, build references matching `statement.params()`, use `query_raw`, then call `rows.rows_affected()` after exhaustion. MySQL: use `exec_iter(sql, mysql_async::Params::Positional(values))` when parameters are non-empty, iterate `QueryResult::iter()` result sets, and capture `affected_rows()` before advancing.

```rust
let params = crate::params::bind(&statement, &request.parameters)?;
let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
    params.iter().map(|value| value.as_ref()).collect();
let rows = client.query_raw(&statement, refs).await.map_err(map_error)?;
```

Drive the PostgreSQL connection with `poll_message` and forward `AsyncMessage::Notice` through the session event stream. Keep notifications out of query rows.

- [ ] **Step 5: Run Docker contracts and commit**

Run: `cargo test -p dexo-driver-postgres -p dexo-driver-mysql --test query -- --ignored --nocapture`

Expected: parameter, affected-row, result-set, cancellation, stream, and transaction contracts PASS for both drivers.

```powershell
git add crates/dexo-driver-api/src/query.rs crates/dexo-driver-api/src/connection.rs crates/dexo-driver-postgres/src crates/dexo-driver-postgres/tests/query.rs crates/dexo-driver-mysql/src/session.rs crates/dexo-driver-mysql/tests/query.rs
git commit -m "feat(drivers): complete query event and parameter contracts"
```

### Task 5: Stream scripts through the runtime with timeout and cancellation

**Files:**
- Create: `crates/dexo-tui/src/runtime/query_runner.rs`
- Modify: `crates/dexo-runtime/src/task.rs`
- Modify: `crates/dexo-app/src/query_service.rs`
- Modify: `crates/dexo-app/src/script.rs`
- Modify: `crates/dexo-tui/src/runtime/mod.rs`
- Modify: `crates/dexo-tui/src/action.rs`
- Modify: `crates/dexo-tui/src/update.rs`
- Test: `crates/dexo-tui/tests/runtime_query.rs`

- [x] **Step 1: Write failing streaming and stale-event tests**

```rust
#[tokio::test]
async fn script_streams_two_real_tabs_and_cancel_reaches_session() {
    let fake = std::sync::Arc::new(FakeSession::with_rows(vec![1, 2]));
    let (runtime, mut actions) = runtime_with_session(fake.clone()).await;
    runtime.start_script(script_request("select 1; select 2;")).await.unwrap();
    let received = collect_until_finished(&mut actions).await;
    assert_eq!(result_set_indexes(&received), vec![0, 1]);
    runtime.cancel(active_operation(&received)).await.unwrap();
    assert_eq!(fake.cancels.load(std::sync::atomic::Ordering::SeqCst), 1);
}
```

- [x] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test runtime_query script_streams_two_real_tabs_and_cancel_reaches_session`

Expected: FAIL because no query runner exists.

- [x] **Step 3: Return a cancellable task handle from the app service**

```rust
pub struct QueryTask {
    pub task: dexo_runtime::RuntimeTaskId,
    pub query: dexo_driver_api::QueryId,
    pub events: tokio::sync::mpsc::Receiver<Result<dexo_driver_api::QueryEvent, dexo_driver_api::DriverError>>,
}
```

`QueryService::start()` returns `QueryTask`. `TaskRegistry::cancel()` cancels the token; runner cancellation also calls `Session::cancel(query)` and waits for a terminal event.

- [x] **Step 4: Implement script execution and bounded action delivery**

For each statement, classify with `dexo_sql::statement_effect`, construct `QueryRequest::read` or `write`, bind the parameter values, and wrap the full stream in `tokio::time::timeout`. Emit `QueryResultSetStarted`, meta, row batches, notices, result-set finished, and script finished actions containing the same `OperationKey`.

Use `ScriptPolicy::StopOnError` to stop only after the failing statement action has been delivered. Never replay a mutating statement after network failure.

- [x] **Step 5: Replace the event loop with `tokio::select!`**

```rust
loop {
    terminal.draw(|frame| crate::render::render(frame, &model))?;
    tokio::select! {
        terminal_event = events.next() => handle_terminal_event(terminal_event, &mut model, &mut runtime).await?,
        runtime_action = action_rx.recv() => {
            let Some(action) = runtime_action else { break };
            dispatch_effects(&mut runtime, crate::update::update(&mut model, action));
        }
        _ = checkpoint.tick() => dispatch_effects(&mut runtime, crate::update::update(&mut model, Action::CheckpointTick)),
    }
}
```

`dispatch_effects` calls `runtime.dispatch` without awaiting long-running query tasks.

- [x] **Step 6: Run and commit**

Run: `cargo test -p dexo-tui --test runtime_query`

Expected: streaming, stale generation, timeout, cancellation, and script policy tests PASS.

```powershell
git add crates/dexo-runtime/src/task.rs crates/dexo-app/src/query_service.rs crates/dexo-app/src/script.rs crates/dexo-tui/src/runtime/query_runner.rs crates/dexo-tui/src/runtime/mod.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/event.rs crates/dexo-tui/tests/runtime_query.rs
git commit -m "feat(tui): stream and cancel SQL scripts"
```

### Task 6: Replace the String editor with `SqlDocument`

**Files:**
- Modify: `crates/dexo-tui/Cargo.toml`
- Create: `crates/dexo-tui/src/screens/editor.rs`
- Modify: `crates/dexo-tui/src/model.rs`
- Modify: `crates/dexo-tui/src/update.rs`
- Modify: `crates/dexo-tui/src/widgets/editor.rs`
- Test: `crates/dexo-tui/tests/editor_flow.rs`

- [ ] **Step 1: Write keyboard editing tests**

```rust
#[test]
fn editor_types_unicode_moves_and_undoes() {
    let mut model = Model::default();
    send_text(&mut model, "select 'ação'");
    assert_eq!(model.active_document().text(), "select 'ação'");
    update(&mut model, ctrl('z'));
    assert_eq!(model.active_document().text(), "");
}
```

Add tests for arrows, Home/End, word motion, Backspace/Delete, Shift selection, Ctrl+A, newline indentation, Tab insertion, redo, and cursor visibility after scrolling.

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test editor_flow editor_types_unicode_moves_and_undoes`

Expected: FAIL because editor keys are ignored.

- [ ] **Step 3: Introduce document state**

```rust
pub struct EditorDocument {
    pub id: String,
    pub title: String,
    pub path: Option<std::path::PathBuf>,
    pub sql: dexo_sql::SqlDocument,
    pub saved_revision: u64,
    pub session: Option<crate::runtime::session_registry::SessionId>,
    pub viewport_line: usize,
    pub viewport_column: usize,
}

impl EditorDocument {
    pub fn is_dirty(&self) -> bool { self.sql.revision() != self.saved_revision }
    pub fn text(&self) -> String { self.sql.text() }
}
```

Replace `Model.sql`, `cursor`, and `selection` with `documents: Vec<EditorDocument>` and `active_document: usize`. Provide accessors so script planning reads the active document.

- [ ] **Step 4: Implement editor key commands**

Map printable characters, deletion, motion, selection, undo/redo, and newline indentation in `screens/editor.rs`. Group contiguous typing into one undo group, ending the group on motion, paste, execution, or focus change.

Render line numbers, selection, cursor, current statement marker, horizontal/vertical viewport, and dirty tab marker. Use `Frame::set_cursor_position` only while editor focus is active.

- [ ] **Step 5: Run snapshots and commit**

Run: `cargo test -p dexo-tui --test editor_flow && cargo test -p dexo-tui --test snapshots`

Expected: editor behavior PASS; review and accept only intentional snapshot changes with `cargo insta review`.

```powershell
git add crates/dexo-tui/Cargo.toml crates/dexo-tui/src/screens/editor.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/widgets/editor.rs crates/dexo-tui/tests/editor_flow.rs crates/dexo-tui/tests/snapshots
git commit -m "feat(tui): add a unicode safe SQL editor"
```

### Task 7: Integrate SQL intelligence, parameters, snippets, and history

**Files:**
- Modify: `crates/dexo-tui/src/screens/editor.rs`
- Modify: `crates/dexo-tui/src/model.rs`
- Modify: `crates/dexo-tui/src/action.rs`
- Modify: `crates/dexo-tui/src/update.rs`
- Modify: `crates/dexo-tui/src/widgets/editor.rs`
- Modify: `crates/dexo-tui/src/runtime/storage_worker.rs`
- Modify: `crates/dexo-storage/src/history.rs`
- Modify: `crates/dexo-storage/src/snippet.rs`
- Test: `crates/dexo-tui/tests/editor_flow.rs`

- [ ] **Step 1: Write failing intelligence tests**

```rust
#[test]
fn editor_highlights_formats_completes_and_prompts_for_parameters() {
    let mut model = model_with_sql("select * from users where id = :id");
    update(&mut model, Action::RefreshSqlIntelligence);
    assert!(model.editor.highlights.iter().any(|span| span.kind == dexo_sql::Highlight::Keyword));
    assert_eq!(model.editor.parameters.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["id"]);
    assert!(model.editor.completions.iter().any(|item| item.label == "users"));
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test editor_flow editor_highlights_formats_completes_and_prompts_for_parameters`

Expected: FAIL because parser state is not connected.

- [ ] **Step 3: Add incremental parser and completion state**

`EditorState` owns one `ParserService` per document dialect, the latest `ParsedSql`, completion popup, diagnostics, parameter form, and format preview. Reparse after edits using the old tree and `InputEdit`; debounce completion but not highlight.

```rust
pub struct ParameterValue {
    pub name: String,
    pub value: dexo_driver_api::DbValue,
    pub sensitive: bool,
}
```

Sensitive parameter values never enter history or recovery.

- [ ] **Step 4: Wire commands to real storage and query requests**

Implement palette/keymap commands for completion accept, format preview/apply, snippet insert/manage, parameter submit, and history search/rerun. Add `HistoryRepository::{list, clear_for_connection}` and `SnippetRepository::{list, delete}`. Persist SQL only after query completion; apply `HistoryPolicy::SqlOnly` by default.

- [ ] **Step 5: Run and commit**

Run: `cargo test -p dexo-sql -p dexo-storage -p dexo-tui --test editor_flow`

Expected: parser, completion, format, snippet, parameter, and history tests PASS.

```powershell
git add crates/dexo-tui/src/screens/editor.rs crates/dexo-tui/src/model.rs crates/dexo-tui/src/action.rs crates/dexo-tui/src/update.rs crates/dexo-tui/src/widgets/editor.rs crates/dexo-tui/src/runtime/storage_worker.rs crates/dexo-storage/src/history.rs crates/dexo-storage/src/snippet.rs crates/dexo-tui/tests/editor_flow.rs
git commit -m "feat(tui): connect SQL intelligence and history"
```

### Task 8: Add atomic SQL files, scratch recovery, and the live acceptance gate

**Files:**
- Create: `crates/dexo-tui/src/runtime/document_io.rs`
- Modify: `crates/dexo-storage/src/document.rs`
- Modify: `crates/dexo-storage/src/recovery.rs`
- Modify: `crates/dexo-storage/src/session_recovery.rs`
- Modify: `crates/dexo-tui/src/runtime/storage_worker.rs`
- Modify: `crates/dexo-tui/src/runtime/mod.rs`
- Modify: `crates/dexo-tui/src/action.rs`
- Modify: `crates/dexo-tui/src/update.rs`
- Test: `crates/dexo-tui/tests/editor_flow.rs`
- Create: `crates/dexo/tests/tui_query_live.rs`

- [ ] **Step 1: Write failing atomic-save, conflict, and crash-recovery tests**

```rust
#[tokio::test]
async fn save_is_atomic_and_external_change_requires_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("query.sql");
    save_sql_atomic(&path, "select 1").await.unwrap();
    let first = fingerprint(&path).await.unwrap();
    tokio::fs::write(&path, "select 2").await.unwrap();
    let error = save_if_unchanged(&path, &first, "select 3").await.unwrap_err();
    assert!(matches!(error, DocumentIoError::ExternalConflict { .. }));
    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), "select 2");
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test -p dexo-tui --test editor_flow save_is_atomic_and_external_change_requires_resolution`

Expected: FAIL because document I/O is missing.

- [ ] **Step 3: Implement atomic save and complete repository CRUD**

Write to a unique `.dexo-part-<uuid>` in the same directory, `sync_all`, then rename. On Windows, handle existing target with the safest replace supported by `std::fs`; never delete the original before the replacement is durable. Compute SHA-256 plus mtime fingerprint. Add `DocumentRepository::{get, list_for_project, delete}`.

- [ ] **Step 4: Wire checkpoint ticks and clean shutdown**

Every dirty scratch is checkpointed after debounce and before execution/focus loss/shutdown. Startup loads `SessionRecoveryState` and emits a real recovery action. Clean shutdown is marked only after documents and layout flush. Parameter values and session handles are excluded.

- [ ] **Step 5: Add ignored live TUI runtime acceptance tests**

The test must start both testcontainers, create profiles in a temporary `DEXO_DATA_HOME`, instantiate `WorkbenchRuntime`, connect, type/execute `select`, verify returned rows, begin/rollback a transaction, cancel sleep, save/reopen a scratch, and assert no sentinel appears in SQLite/logs.

Run: `cargo test -p dexo --test tui_query_live -- --ignored --nocapture`

Expected: PostgreSQL and MySQL cases PASS.

- [ ] **Step 6: Run the sprint gate and commit**

Run:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --no-fail-fast
cargo test -p dexo-driver-postgres -p dexo-driver-mysql --test query -- --ignored --nocapture
cargo test -p dexo --test tui_query_live -- --ignored --nocapture
```

Expected: all non-Docker tests PASS; all query/live Docker contracts PASS; no production path still discards a connected session.

```powershell
git add crates/dexo-tui/src/runtime/document_io.rs crates/dexo-storage/src/document.rs crates/dexo-storage/src/recovery.rs crates/dexo-storage/src/session_recovery.rs crates/dexo-tui/src/runtime crates/dexo-tui/src/action.rs crates/dexo-tui/src/update.rs crates/dexo-tui/tests/editor_flow.rs crates/dexo/tests/tui_query_live.rs
git commit -m "feat(tui): persist SQL work and complete core runtime"
```

## Sprint 16 exit checklist

- [ ] Typing, cursor, selection, undo/redo, highlight, completion, format, snippets, parameters, and history work in the TUI.
- [ ] Create connection retains a live session.
- [ ] F5 streams real PostgreSQL/MySQL rows and notices into the correct result tabs.
- [ ] Cancellation reaches the same transported session generation.
- [ ] Begin/commit/rollback/savepoints reach the driver and status follows the response.
- [ ] `.sql` files and scratches survive restart; conflicts never overwrite silently.
- [ ] Runtime actions reject stale session/document generations.
- [ ] The complete sprint gate is green.
