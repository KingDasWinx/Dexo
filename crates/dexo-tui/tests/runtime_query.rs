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
    let worker = dexo_tui::runtime::storage_worker::StorageWorker::start(
        dir.path().join("dexo.db"),
    )
    .unwrap();
    let bootstrap = worker.bootstrap().await.unwrap();
    assert_eq!(bootstrap.active_project.name, "Default");
    assert!(bootstrap.connections.is_empty());
    drop(worker);
}
