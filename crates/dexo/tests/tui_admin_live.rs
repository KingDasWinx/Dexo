//! Inspect Sessions reads on a connection of its own, in a task: the loop that draws the
//! screen never waits on a busy session, and a failure reaches the dialog.
use std::sync::Arc;
use std::time::Duration;

use dexo_app::DriverRegistry;
use dexo_driver_sqlite::SqliteFactory;
use dexo_storage::AppPaths;
use dexo_tui::action::{Action, Effect};
use dexo_tui::runtime::storage_worker::StorageWorker;
use dexo_tui::runtime::{SessionId, WorkbenchRuntime};

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

/// SQLite has no sessions to list: the answer is an `AdminFailed` the dialog shows, not
/// a message nobody reads. It comes from the connection dialled for the list.
#[tokio::test(flavor = "multi_thread")]
async fn the_list_of_sessions_reports_why_it_cannot_load() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_home(dir.path().to_path_buf());
    let worker = StorageWorker::start(paths.database.clone()).unwrap();
    let mut registry = DriverRegistry::new();
    registry.register(Arc::new(SqliteFactory));
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let mut runtime = WorkbenchRuntime::new(tx, worker, registry);
    let session = connected(&mut runtime, &mut rx, &dir.path().join("shop.db")).await;

    runtime
        .dispatch(Effect::LoadAdminSessions {
            session,
            generation: 1,
        })
        .await;
    let message = next_matching(&mut rx, Duration::from_secs(10), |action| match action {
        Action::AdminFailed { message } => Some(message),
        _ => None,
    })
    .await
    .expect("the list answered");
    assert!(message.contains("no session administration"), "{message}");
}
