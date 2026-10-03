//! Reads and writes of Dexo's own database run off the loop that draws the screen. In a
//! file of its own: it points DEXO_DATA_HOME at a scratch folder for its whole process, so
//! nothing reaches the real database, and no other test shares the process.
use std::time::{Duration, Instant};

use dexo_app::DriverRegistry;
use dexo_storage::Database;
use dexo_tui::action::{Action, Effect};
use dexo_tui::runtime::WorkbenchRuntime;
use dexo_tui::runtime::storage_worker::StorageWorker;

/// Agents reads its audit every second, and the favorites are read after every catalog
/// node: each opened the database on the loop, and waited out a lock another process
/// held on it.
#[tokio::test(flavor = "multi_thread")]
async fn reads_of_dexos_database_do_not_hold_the_screen() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("dexo.db");
    let worker = StorageWorker::start(db_path.clone()).unwrap();
    worker.bootstrap().await.unwrap();
    // SAFETY: the one test in this process, before anything reads the environment.
    unsafe { std::env::set_var("DEXO_DATA_HOME", dir.path()) };
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let mut runtime = WorkbenchRuntime::new(tx, worker, DriverRegistry::new());
    // Another process writing: the MCP server's audit, say.
    let other = Database::open(&db_path).unwrap();
    other
        .connection()
        .execute_batch("BEGIN IMMEDIATE;")
        .unwrap();

    let started = Instant::now();
    runtime
        .dispatch(Effect::PersistFavorite {
            project_id: "p".into(),
            connection_id: "c".into(),
            object_id: "o".into(),
            favorite: true,
        })
        .await;
    runtime.dispatch(Effect::LoadMcpAudit).await;
    runtime.dispatch(Effect::LoadMcpProfiles).await;
    runtime
        .dispatch(Effect::LoadObjectUsage {
            project_id: "p".into(),
            connection_id: "c".into(),
        })
        .await;

    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the loop waited {:?}",
        started.elapsed()
    );
    other.connection().execute_batch("COMMIT;").unwrap();
    let answered = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(Action::McpAuditLoaded { .. }) = rx.recv().await {
                return;
            }
        }
    })
    .await;
    assert!(answered.is_ok(), "the audit never came");
}
