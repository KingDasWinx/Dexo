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

/// A server far away: every catalog read takes its time, the first one longest.
struct FarSession {
    first: std::sync::atomic::AtomicBool,
}

#[async_trait::async_trait]
impl dexo_driver_api::Session for FarSession {
    fn capabilities(&self) -> &[dexo_driver_api::CapabilityState] {
        &[]
    }

    async fn execute(
        &self,
        _request: dexo_driver_api::QueryRequest,
    ) -> Result<dexo_driver_api::QueryStream, dexo_driver_api::DriverError> {
        Err(dexo_driver_api::DriverError::new(
            dexo_driver_api::DriverErrorCategory::Network,
            "not here",
        ))
    }

    async fn cancel(
        &self,
        _query: dexo_driver_api::QueryId,
    ) -> Result<(), dexo_driver_api::DriverError> {
        Ok(())
    }

    async fn close(self: Box<Self>) -> Result<(), dexo_driver_api::DriverError> {
        Ok(())
    }

    fn catalog(&self) -> Option<&dyn dexo_driver_api::CatalogReader> {
        Some(self)
    }
}

#[async_trait::async_trait]
impl dexo_driver_api::CatalogReader for FarSession {
    async fn list_children(
        &self,
        _parent: Option<&dexo_driver_api::ObjectId>,
        _options: &dexo_driver_api::CatalogListOptions,
    ) -> Result<dexo_driver_api::CatalogList, dexo_driver_api::DriverError> {
        let wait = if self.first.swap(false, std::sync::atomic::Ordering::SeqCst) {
            800
        } else {
            50
        };
        tokio::time::sleep(Duration::from_millis(wait)).await;
        Ok(dexo_driver_api::CatalogList {
            objects: Vec::new(),
            restrictions: Vec::new(),
        })
    }

    async fn object(
        &self,
        _id: &dexo_driver_api::ObjectId,
    ) -> Result<Option<dexo_driver_api::CatalogObject>, dexo_driver_api::DriverError> {
        Ok(None)
    }

    async fn ddl(
        &self,
        id: &dexo_driver_api::ObjectId,
    ) -> Result<dexo_driver_api::ObjectDdl, dexo_driver_api::DriverError> {
        Ok(dexo_driver_api::ObjectDdl {
            object_id: id.clone(),
            sql: String::new(),
        })
    }

    async fn dependencies(
        &self,
        _id: &dexo_driver_api::ObjectId,
    ) -> Result<Vec<dexo_driver_api::ObjectId>, dexo_driver_api::DriverError> {
        Ok(Vec::new())
    }

    async fn dependents(
        &self,
        _id: &dexo_driver_api::ObjectId,
    ) -> Result<Vec<dexo_driver_api::ObjectId>, dexo_driver_api::DriverError> {
        Ok(Vec::new())
    }

    async fn foreign_keys(
        &self,
        _table: &dexo_driver_api::QualifiedName,
    ) -> Result<Vec<dexo_driver_api::ForeignKeyRef>, dexo_driver_api::DriverError> {
        Ok(Vec::new())
    }
}

/// Expanding a node on a server far away read its children on the loop: the screen
/// froze for as long as the round trips took -- seconds on a remote database. Read off
/// it, the answers still come in the order they were asked for.
#[tokio::test(flavor = "multi_thread")]
async fn a_catalog_read_does_not_hold_the_screen_and_answers_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let worker = StorageWorker::start(dir.path().join("dexo.db")).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let mut runtime = WorkbenchRuntime::new(tx, worker, DriverRegistry::new());
    let session = runtime.sessions_mut().insert(
        "far",
        std::sync::Arc::new(FarSession {
            first: std::sync::atomic::AtomicBool::new(true),
        }),
    );
    let load = |parent: &str| Effect::LoadCatalogChildren {
        parent: Some(dexo_driver_api::ObjectId::new(parent)),
        operation: dexo_tui::runtime::OperationId::new(),
        session,
        generation: 1,
        replace_roots: false,
        include_system: false,
    };

    let started = Instant::now();
    runtime.dispatch(load("pg:schema:first")).await;
    runtime.dispatch(load("pg:schema:second")).await;
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "the loop waited {:?}",
        started.elapsed()
    );

    let mut answered = Vec::new();
    while answered.len() < 2 {
        let action = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("an answer")
            .expect("the channel");
        if let Action::CatalogLoaded {
            parent: Some(parent),
            ..
        } = action
        {
            answered.push(parent.as_str().to_string());
        }
    }
    assert_eq!(answered, ["pg:schema:first", "pg:schema:second"]);
}
