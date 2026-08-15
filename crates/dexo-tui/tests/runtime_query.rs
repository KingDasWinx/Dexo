use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use dexo_app::{DriverRegistry, ScriptPolicy, statements_for};
use dexo_driver_api::{
    ColumnMeta, DbValue, DriverError, QueryEvent, QueryId, QueryRequest, QueryStream, RowBatch,
    Session, TransactionControl, TransactionMode, TransactionState,
};
use dexo_tui::action::{Action, ScriptRequest};
use dexo_tui::runtime::session_registry::SessionRegistry;
use dexo_tui::runtime::storage_worker::StorageWorker;
use dexo_tui::runtime::{OperationId, OperationKey, WorkbenchRuntime};

#[test]
fn operation_key_rejects_a_stale_session_generation() {
    let operation = OperationKey::new(OperationId::new(), "session-a", "doc-a", 4);
    assert!(operation.belongs_to("session-a", "doc-a", 4));
    assert!(!operation.belongs_to("session-a", "doc-a", 3));
    assert!(!operation.belongs_to("session-b", "doc-a", 4));
}

#[tokio::test]
async fn storage_worker_creates_and_loads_the_default_project() {
    let dir = tempfile::tempdir().unwrap();
    let worker =
        dexo_tui::runtime::storage_worker::StorageWorker::start(dir.path().join("dexo.db")).unwrap();
    let bootstrap = worker.bootstrap().await.unwrap();
    assert_eq!(bootstrap.active_project.name, "Default");
    assert!(bootstrap.connections.is_empty());
    drop(worker);
}

struct FakeSession {
    commits: AtomicU64,
    cancels: AtomicU64,
    tx: Mutex<TransactionState>,
    remaining: Mutex<VecDeque<usize>>,
}

impl Default for FakeSession {
    fn default() -> Self {
        Self {
            commits: AtomicU64::new(0),
            cancels: AtomicU64::new(0),
            tx: Mutex::new(TransactionState::Idle),
            remaining: Mutex::new(VecDeque::new()),
        }
    }
}

impl FakeSession {
    fn with_rows(counts: Vec<usize>) -> Self {
        Self {
            remaining: Mutex::new(counts.into()),
            ..Self::default()
        }
    }
}

#[async_trait::async_trait]
impl Session for FakeSession {
    fn capabilities(&self) -> &[dexo_driver_api::CapabilityState] {
        &[]
    }

    async fn execute(&self, _request: QueryRequest) -> Result<QueryStream, DriverError> {
        let count = self
            .remaining
            .lock()
            .expect("fake rows")
            .pop_front()
            .unwrap_or(0);
        let mut events = vec![
            QueryEvent::ResultSetStarted { index: 0 },
            QueryEvent::Columns(vec![ColumnMeta {
                name: "n".into(),
                type_name: "int8".into(),
                nullable: false,
            }]),
        ];
        if count > 0 {
            events.push(QueryEvent::Rows(RowBatch {
                rows: (1..=count as i64).map(|n| vec![DbValue::I64(n)]).collect(),
            }));
        }
        events.push(QueryEvent::ResultSetFinished {
            index: 0,
            rows_affected: Some(count as u64),
        });
        events.push(QueryEvent::Finished {
            rows_affected: Some(count as u64),
        });
        Ok(Box::pin(futures_util::stream::iter(
            events.into_iter().map(Ok),
        )))
    }

    async fn cancel(&self, _query: QueryId) -> Result<(), DriverError> {
        self.cancels.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn close(self: Box<Self>) -> Result<(), DriverError> {
        Ok(())
    }

    fn transactions(&self) -> Option<&dyn TransactionControl> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl TransactionControl for FakeSession {
    async fn begin(&self, _mode: TransactionMode) -> Result<(), DriverError> {
        *self.tx.lock().expect("fake tx") = TransactionState::Active;
        Ok(())
    }

    async fn commit(&self) -> Result<(), DriverError> {
        self.commits.fetch_add(1, Ordering::SeqCst);
        *self.tx.lock().expect("fake tx") = TransactionState::Idle;
        Ok(())
    }

    async fn rollback(&self) -> Result<(), DriverError> {
        *self.tx.lock().expect("fake tx") = TransactionState::Idle;
        Ok(())
    }

    async fn savepoint(&self, _name: &str) -> Result<(), DriverError> {
        Ok(())
    }

    async fn rollback_to(&self, _name: &str) -> Result<(), DriverError> {
        Ok(())
    }

    async fn release_savepoint(&self, _name: &str) -> Result<(), DriverError> {
        Ok(())
    }

    fn state(&self) -> TransactionState {
        *self.tx.lock().expect("fake tx")
    }
}

#[tokio::test]
async fn connected_session_survives_and_commit_reaches_the_driver() {
    let fake = Arc::new(FakeSession::default());
    let mut registry = SessionRegistry::default();
    let id = registry.insert("connection-a", fake.clone());
    registry.commit(id).await.unwrap();
    assert_eq!(fake.commits.load(Ordering::SeqCst), 1);
    assert!(registry.get(id).is_some());
}

#[tokio::test]
async fn reconnect_is_refused_for_unknown_transaction() {
    let mut registry = SessionRegistry::default();
    let id = registry.insert("connection-a", Arc::new(FakeSession::default()));
    registry
        .set_transaction(id, TransactionState::Unknown)
        .unwrap();
    assert!(registry.can_reconnect(id, true).is_err());
}

async fn runtime_with_session(
    fake: Arc<FakeSession>,
) -> (
    tempfile::TempDir,
    WorkbenchRuntime,
    tokio::sync::mpsc::Receiver<Action>,
) {
    let dir = tempfile::tempdir().unwrap();
    let worker = StorageWorker::start(dir.path().join("dexo.db")).unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    let mut runtime = WorkbenchRuntime::new(tx, worker, DriverRegistry::new());
    runtime.sessions_mut().insert("session-a", fake);
    (dir, runtime, rx)
}

fn script_request(sql: &str) -> ScriptRequest {
    ScriptRequest {
        key: OperationKey::new(OperationId::new(), "session-a", "doc-a", 1),
        statements: statements_for(sql, dexo_app::ExecutionTarget::Document, 0, None),
        policy: ScriptPolicy::StopOnError,
        parameters: Vec::new(),
        timeout: std::time::Duration::from_secs(5),
    }
}

async fn collect_until_finished(actions: &mut tokio::sync::mpsc::Receiver<Action>) -> Vec<Action> {
    let mut received = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(
            std::time::Duration::from_millis(200),
            actions.recv(),
        )
        .await
        {
            Ok(Some(action)) => {
                let done = matches!(
                    action,
                    Action::ScriptFinished { .. } | Action::OperationFailed { .. }
                );
                received.push(action);
                if done {
                    break;
                }
            }
            _ => break,
        }
    }
    received
}

fn result_set_indexes(actions: &[Action]) -> Vec<usize> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::QueryResultSetStarted { index, .. } => Some(*index),
            _ => None,
        })
        .collect()
}

fn active_operation(actions: &[Action]) -> OperationId {
    actions
        .iter()
        .find_map(|action| match action {
            Action::OperationStarted(key) | Action::ScriptFinished { key } => Some(key.operation),
            _ => None,
        })
        .unwrap_or_else(OperationId::new)
}

#[tokio::test]
async fn script_streams_two_real_tabs_and_cancel_reaches_session() {
    let fake = Arc::new(FakeSession::with_rows(vec![1, 2]));
    let (_dir, mut runtime, mut actions) = runtime_with_session(fake.clone()).await;
    runtime
        .start_script(script_request("select 1; select 2;"))
        .await
        .unwrap();
    let received = collect_until_finished(&mut actions).await;
    assert_eq!(result_set_indexes(&received), vec![0, 1]);
    runtime.cancel(active_operation(&received)).await;
    assert_eq!(fake.cancels.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stale_generation_is_ignored_by_the_reducer() {
    let mut model = dexo_tui::Model::default();
    model.session_generation = 4;
    let stale = OperationKey::new(OperationId::new(), "", "scratch", 3);
    dexo_tui::update(
        &mut model,
        Action::QueryRows {
            key: stale,
            rows: vec![vec![DbValue::I64(1)]],
        },
    );
    assert_eq!(model.results.row_count(), 0);
}
