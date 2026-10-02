//! The exact row count runs on a connection of its own: it answers, it does not hold up
//! the session's own queries, and a cancel stops it.
use std::sync::Arc;
use std::time::Duration;

use dexo_app::{DriverRegistry, ScriptPolicy};
use dexo_driver_sqlite::SqliteFactory;
use dexo_storage::AppPaths;
use dexo_tui::action::{Action, Effect, ScriptRequest};
use dexo_tui::runtime::storage_worker::StorageWorker;
use dexo_tui::runtime::{OperationId, OperationKey, SessionId, WorkbenchRuntime};

async fn next_matching<T>(
    rx: &mut tokio::sync::mpsc::Receiver<Action>,
    within: Duration,
    mut pick: impl FnMut(Action) -> Option<T>,
) -> Option<T> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        let action = tokio::time::timeout_at(deadline, rx.recv()).await.ok()??;
        if let Some(found) = pick(action) {
            return Some(found);
        }
    }
}

async fn connected(
    runtime: &mut WorkbenchRuntime,
    rx: &mut tokio::sync::mpsc::Receiver<Action>,
    file: &std::path::Path,
) -> SessionId {
    let connection = dexo_app::connection_url::file("sqlite", file).unwrap();
    runtime
        .dispatch(Effect::ConnectProfile {
            profile: connection.profile,
            token: 1,
        })
        .await;
    next_matching(rx, Duration::from_secs(10), |action| match action {
        Action::SessionOpened { token } => Some(token),
        _ => None,
    })
    .await
    .expect("dialled");
    runtime.dispatch(Effect::AdoptSession { token: 1 }).await;
    next_matching(rx, Duration::from_secs(10), |action| match action {
        Action::ConnectionChanged {
            session: Some(session),
            ..
        } => Some(session),
        _ => None,
    })
    .await
    .expect("adopted")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_count_answers_beside_the_session_and_stops_when_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_home(dir.path().to_path_buf());
    let worker = StorageWorker::start(paths.database.clone()).unwrap();
    let mut registry = DriverRegistry::new();
    registry.register(Arc::new(SqliteFactory));
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mut runtime = WorkbenchRuntime::new(tx, worker, registry);
    let file = dir.path().join("shop.db");
    let session = connected(&mut runtime, &mut rx, &file).await;

    let script = |sql: &str| {
        Effect::StartScript(ScriptRequest {
            key: OperationKey::new(OperationId::new(), session.0.to_string(), "doc", 1),
            statements: vec![sql.into()],
            dialect: dexo_sql::Dialect::Sqlite,
            policy: ScriptPolicy::StopOnError,
            parameters: Vec::new(),
            timeout: Duration::from_secs(10),
        })
    };
    runtime
        .dispatch(script(
            "create table t (n integer); insert into t values (1), (2), (3), (4)",
        ))
        .await;
    next_matching(&mut rx, Duration::from_secs(10), |action| {
        matches!(action, Action::ScriptFinished { .. }).then_some(())
    })
    .await
    .expect("seeded");

    let operation = OperationId::new();
    runtime
        .dispatch(Effect::CountRows {
            session,
            operation,
            sql:
                "SELECT COUNT(*) FROM (SELECT * FROM \"main\".\"t\") AS _dexo_derived WHERE (n > ?)"
                    .into(),
            parameters: vec![dexo_driver_api::DbValue::I64(1)],
            on_session: false,
        })
        .await;
    let counted = next_matching(&mut rx, Duration::from_secs(10), |action| match action {
        Action::RowsCounted {
            operation: answered,
            result,
        } if answered == operation => Some(result),
        _ => None,
    })
    .await
    .expect("counted");
    assert_eq!(counted, Ok(3));

    // A count that would take minutes: the session still answers at once beside it,
    // and a cancel stops it without an answer.
    let slow = OperationId::new();
    runtime
        .dispatch(Effect::CountRows {
            session,
            operation: slow,
            sql: "SELECT COUNT(*) FROM (WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL \
                  SELECT i + 1 FROM n WHERE i < 2000000000) SELECT i FROM n) AS _dexo_derived"
                .into(),
            parameters: Vec::new(),
            on_session: false,
        })
        .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    runtime.dispatch(script("select 41 + 1")).await;
    let rows = next_matching(&mut rx, Duration::from_secs(5), |action| match action {
        Action::QueryRows { rows, .. } => Some(rows),
        _ => None,
    })
    .await
    .expect("the session answered while the count ran");
    assert_eq!(rows, vec![vec![dexo_driver_api::DbValue::I64(42)]]);
    runtime
        .dispatch(Effect::CancelCount { operation: slow })
        .await;
    let late = next_matching(&mut rx, Duration::from_secs(2), |action| match action {
        Action::RowsCounted { operation, .. } if operation == slow => Some(()),
        _ => None,
    })
    .await;
    assert!(late.is_none(), "a cancelled count still answered");
}
