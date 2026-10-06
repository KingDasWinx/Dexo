//! The host against a real plugin process: the testbin, copied under the name of the
//! failure it plays.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dexo_app::plugin::host::{PluginHost, process_spawner};
use dexo_app::plugin::proxy::PluginFactory;
use dexo_driver_api::{
    ConnectRequest, ConnectionFactory, DbValue, DriverDescriptor, DriverError, QueryEvent,
    QueryRequest, Session,
};
use futures_util::StreamExt;
use secrecy::SecretString;

/// The testbin in `dir`, named `mode`.
fn testbin(dir: &Path, mode: &str) -> PathBuf {
    let exe = dir.join(format!("{mode}{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(env!("CARGO_BIN_EXE_dexo-plugin-testbin"), &exe).unwrap();
    exe
}

fn plugin(dir: &Path, mode: &str) -> PluginFactory {
    let host = PluginHost::new(
        "Testbin",
        vec!["testbin".into()],
        process_spawner(testbin(dir, mode)),
    );
    PluginFactory::new(
        DriverDescriptor::plugin_driver("testbin", "Testbin", true, "testbin", true),
        Arc::new(host),
    )
}

async fn open(plugin: &PluginFactory, dir: &Path) -> Result<Box<dyn Session>, DriverError> {
    plugin
        .connect(ConnectRequest::new(
            dir.join("t.db").to_string_lossy(),
            None,
            String::new(),
            SecretString::from(String::new()),
            false,
        ))
        .await
}

async fn run(session: &dyn Session, sql: &str) -> Result<Vec<Vec<DbValue>>, DriverError> {
    let mut stream = session.execute(QueryRequest::read(sql, 0)).await?;
    let mut rows = Vec::new();
    while let Some(event) = stream.next().await {
        if let QueryEvent::Rows(batch) = event? {
            rows.extend(batch.rows);
        }
    }
    Ok(rows)
}

#[tokio::test]
async fn a_plugin_process_answers_queries() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = plugin(dir.path(), "plain");
    let session = open(&plugin, dir.path()).await.unwrap();
    assert_eq!(
        run(&*session, "SELECT 1").await.unwrap(),
        vec![vec![DbValue::I64(1)]]
    );
    session.close().await.unwrap();
}

/// Review focus 1.
#[tokio::test]
async fn a_killed_plugin_fails_open_sessions_and_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = plugin(dir.path(), "crash");
    let session = open(&plugin, dir.path()).await.unwrap();
    let crashed = run(&*session, "SELECT 'crash'").await.unwrap_err();
    assert_eq!(
        crashed.to_string(),
        "the Testbin plugin stopped: crashing on purpose"
    );
    let after = run(&*session, "SELECT 1").await.unwrap_err();
    assert!(
        after.to_string().starts_with("the Testbin plugin stopped"),
        "{after}"
    );
    let again = open(&plugin, dir.path()).await.unwrap();
    assert_eq!(
        run(&*again, "SELECT 1").await.unwrap(),
        vec![vec![DbValue::I64(1)]]
    );
}

#[tokio::test]
async fn the_restart_budget_holds() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = plugin(dir.path(), "crash");
    for _ in 0..4 {
        let session = open(&plugin, dir.path()).await.unwrap();
        let _ = run(&*session, "SELECT 'crash'").await;
    }
    let refused = open(&plugin, dir.path()).await.err().unwrap();
    assert!(
        refused.to_string().contains("stopped 3 times in a minute"),
        "{refused}"
    );
}

#[tokio::test]
async fn a_plugin_that_ignores_a_cancel_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = plugin(dir.path(), "ignore-cancel");
    let session = open(&plugin, dir.path()).await.unwrap();
    let request = QueryRequest::read(
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT count(*) FROM n",
        0,
    );
    let id = request.id;
    let started = Instant::now();
    let (ran, cancelled) = tokio::join!(
        async {
            let mut stream = session.execute(request).await?;
            while let Some(event) = stream.next().await {
                event?;
            }
            Ok::<_, DriverError>(())
        },
        async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            session.cancel(id).await
        },
    );
    assert_eq!(
        cancelled.unwrap_err().to_string(),
        "the Testbin plugin stopped: it did not answer a cancel within 5 seconds"
    );
    assert!(
        ran.unwrap_err()
            .to_string()
            .contains("did not answer a cancel")
    );
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[tokio::test]
async fn a_handshake_with_another_protocol_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let refused = open(&plugin(dir.path(), "bad-protocol"), dir.path())
        .await
        .err()
        .unwrap();
    assert!(
        refused.to_string().contains("it speaks protocol 99"),
        "{refused}"
    );
}

#[tokio::test]
async fn the_plugin_gets_a_minimal_environment() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = plugin(dir.path(), "print-env");
    let session = open(&plugin, dir.path()).await.unwrap();
    let names = std::fs::read_to_string(dir.path().join("print-env.env")).unwrap();
    let allowed = [
        "PATH",
        "HOME",
        "USERPROFILE",
        "TMPDIR",
        "TMP",
        "TEMP",
        "LANG",
        "LANGUAGE",
        "SYSTEMROOT",
    ];
    for name in names.lines() {
        let upper = name.to_ascii_uppercase();
        assert!(
            allowed.contains(&upper.as_str()) || upper.starts_with("LC_"),
            "{name} reached the plugin"
        );
    }
    assert!(names.lines().any(|name| name.eq_ignore_ascii_case("PATH")));
    session.close().await.unwrap();
}
