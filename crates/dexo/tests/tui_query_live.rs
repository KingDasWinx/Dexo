use std::sync::Arc;
use std::time::Duration;

use dexo_app::{DriverRegistry, NewConnection, ScriptPolicy};
use dexo_driver_api::TransactionMode;
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_storage::AppPaths;
use dexo_test_support::DatabasePair;
use dexo_test_support::containers::TEST_PASSWORD;
use dexo_tui::action::{Action, Effect, ScriptRequest};
use dexo_tui::runtime::document_io::save_sql_atomic;
use dexo_tui::runtime::storage_worker::StorageWorker;
use dexo_tui::runtime::{OperationId, OperationKey, WorkbenchRuntime};

#[tokio::test]
#[ignore = "requires Docker"]
async fn tui_runtime_postgres_and_mysql_query_tx_cancel_and_scratch() {
    let pair = DatabasePair::start().await.unwrap();
    run_driver("postgres", pair.postgres_endpoint(), "select pg_sleep(8)").await;
    run_driver("mysql", pair.mysql_endpoint(), "select sleep(8)").await;
}

async fn run_driver(driver: &str, endpoint: &str, sleep_sql: &str) {
    let dir = tempfile::tempdir().unwrap();
    unsafe { std::env::set_var("DEXO_DATA_HOME", dir.path()) }
    let paths = AppPaths::from_data_home(dir.path().to_path_buf());
    let worker = StorageWorker::start(paths.database.clone()).unwrap();
    let mut registry = DriverRegistry::new();
    registry.register(Arc::new(PostgresFactory));
    registry.register(Arc::new(MysqlFactory));
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let mut runtime = WorkbenchRuntime::new(tx, worker, registry);
    let (host, port) = endpoint.rsplit_once(':').expect("host:port");
    runtime
        .dispatch(Effect::CreateConnection {
            input: NewConnection {
                name: format!("{driver}-live"),
                driver: driver.into(),
                host: host.into(),
                port: Some(port.parse().unwrap()),
                database: "dexo".into(),
                username: "dexo".into(),
                environment: "local".into(),
                ..NewConnection::default()
            },
            password: TEST_PASSWORD.into(),
        })
        .await;
    let session = wait_session(&mut rx).await;
    let key = OperationKey::new(OperationId::new(), session.0.to_string(), "scratch", 1);
    runtime
        .dispatch(Effect::StartScript(ScriptRequest {
            key: key.clone(),
            statements: vec!["select 1".into()],
            policy: ScriptPolicy::StopOnError,
            parameters: Vec::new(),
            timeout: Duration::from_secs(10),
        }))
        .await;
    assert!(wait_rows(&mut rx).await, "{driver} select 1 returned rows");
    runtime
        .dispatch(Effect::BeginTransaction {
            session,
            mode: TransactionMode::ReadWrite,
        })
        .await;
    runtime
        .dispatch(Effect::RollbackTransaction { session })
        .await;
    let sleep_key = OperationKey::new(OperationId::new(), session.0.to_string(), "scratch", 1);
    runtime
        .dispatch(Effect::StartScript(ScriptRequest {
            key: sleep_key.clone(),
            statements: vec![sleep_sql.into()],
            policy: ScriptPolicy::StopOnError,
            parameters: Vec::new(),
            timeout: Duration::from_secs(30),
        }))
        .await;
    runtime
        .dispatch(Effect::CancelOperation(sleep_key.operation))
        .await;
    let scratch = dir.path().join("scratch.sql");
    save_sql_atomic(&scratch, "select 1").await.unwrap();
    assert_eq!(
        tokio::fs::read_to_string(&scratch).await.unwrap(),
        "select 1"
    );
    runtime.dispatch(Effect::Shutdown).await;
    let db_bytes = std::fs::read(&paths.database).unwrap_or_default();
    assert!(
        !dexo_app::diagnostic_service::contains_sentinel(&db_bytes),
        "{driver} sqlite must not contain the secret sentinel"
    );
}

async fn wait_session(
    rx: &mut tokio::sync::mpsc::Receiver<Action>,
) -> dexo_tui::runtime::SessionId {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let action = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("timed out waiting for connection")
            .expect("runtime closed");
        if let Action::ConnectionChanged {
            session: Some(session),
            ..
        } = action
        {
            return session;
        }
    }
}

async fn wait_rows(rx: &mut tokio::sync::mpsc::Receiver<Action>) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let action = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .ok()
            .flatten();
        match action {
            Some(Action::QueryRows { rows, .. }) if !rows.is_empty() => return true,
            Some(Action::ScriptFinished { .. }) => return false,
            Some(Action::OperationFailed { .. }) => return false,
            None => return false,
            _ => {}
        }
    }
}
