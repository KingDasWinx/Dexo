//! What the runtime reads for the screen is never waited for on the loop that draws it
//! and reads the keys. The check for waiting agent writes, every two seconds on every
//! screen, waited for the storage worker -- and the worker for a lock another process
//! held on the database: the screen froze for seconds while typing.
use std::time::{Duration, Instant};

use dexo_app::DriverRegistry;
use dexo_storage::Database;
use dexo_tui::action::{Action, Effect, RecoveryCheckpointRequest};
use dexo_tui::runtime::WorkbenchRuntime;
use dexo_tui::runtime::storage_worker::StorageWorker;

#[tokio::test(flavor = "multi_thread")]
async fn the_approval_check_does_not_wait_for_a_busy_database() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("dexo.db");
    let worker = StorageWorker::start(db_path.clone()).unwrap();
    worker.bootstrap().await.unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let mut runtime = WorkbenchRuntime::new(tx, worker, DriverRegistry::new());
    // Another writer -- the MCP server, another Dexo -- holds the database, and the
    // worker's next write waits for it.
    let other = Database::open(&db_path).unwrap();
    other
        .connection()
        .execute_batch("BEGIN IMMEDIATE;")
        .unwrap();
    runtime
        .dispatch(Effect::CheckpointRecovery(RecoveryCheckpointRequest {
            document: "doc".into(),
            project_id: String::new(),
            title: "query-1.sql".into(),
            content: "select 1".into(),
        }))
        .await;

    let started = Instant::now();
    runtime.dispatch(Effect::CheckApprovals).await;

    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the loop waited {:?}",
        started.elapsed()
    );
    other.connection().execute_batch("COMMIT;").unwrap();
    let answered = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(Action::ApprovalsWaiting(_)) = rx.recv().await {
                return;
            }
        }
    })
    .await;
    assert!(answered.is_ok(), "the check never answered");
}
