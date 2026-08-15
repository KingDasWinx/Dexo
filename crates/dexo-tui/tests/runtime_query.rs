use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use dexo_driver_api::{
    DriverError, QueryEvent, QueryId, QueryRequest, QueryStream, Session, TransactionControl,
    TransactionMode, TransactionState,
};
use dexo_tui::runtime::session_registry::SessionRegistry;
use dexo_tui::runtime::{OperationId, OperationKey};

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
}

impl Default for FakeSession {
    fn default() -> Self {
        Self {
            commits: AtomicU64::new(0),
            cancels: AtomicU64::new(0),
            tx: Mutex::new(TransactionState::Idle),
        }
    }
}

#[async_trait::async_trait]
impl Session for FakeSession {
    fn capabilities(&self) -> &[dexo_driver_api::CapabilityState] {
        &[]
    }

    async fn execute(&self, _request: QueryRequest) -> Result<QueryStream, DriverError> {
        Ok(Box::pin(futures_util::stream::iter([Ok(
            QueryEvent::Finished {
                rows_affected: Some(0),
            },
        )])))
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
