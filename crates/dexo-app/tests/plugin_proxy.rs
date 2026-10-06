//! What the proxy adds over the wire: a result dropped midway frees its session, and
//! the transaction state follows what the plugin reports.
use std::sync::Arc;

use dexo_app::plugin::host::{PluginHost, Spawned};
use dexo_app::plugin::proxy::PluginFactory;
use dexo_driver_api::{
    ConnectRequest, ConnectionFactory, DbValue, DriverDescriptor, QueryEvent, QueryRequest,
    Session, TransactionMode, TransactionState,
};
use dexo_driver_sqlite::SqliteFactory;
use futures_util::StreamExt;
use secrecy::SecretString;

async fn connect(dir: &tempfile::TempDir) -> Box<dyn Session> {
    let host = PluginHost::new(
        "SQLite",
        vec!["sqlite".into()],
        Box::new(|| {
            let (reader, writer) =
                dexo_plugin_sdk::serve_in_process(vec![Arc::new(SqliteFactory)])?;
            Ok(Spawned {
                reader: Box::new(reader),
                writer: Box::new(writer),
                stderr: None,
                child: None,
            })
        }),
    );
    PluginFactory::new(DriverDescriptor::sqlite(), Arc::new(host))
        .connect(ConnectRequest::new(
            dir.path().join("a.db").to_string_lossy(),
            None,
            String::new(),
            SecretString::from(String::new()),
            false,
        ))
        .await
        .unwrap()
}

async fn rows(session: &dyn Session, sql: &str) -> Vec<Vec<DbValue>> {
    let mut stream = session.execute(QueryRequest::read(sql, 0)).await.unwrap();
    let mut rows = Vec::new();
    while let Some(event) = stream.next().await {
        if let QueryEvent::Rows(batch) = event.unwrap() {
            rows.extend(batch.rows);
        }
    }
    rows
}

#[tokio::test]
async fn dropping_a_result_midway_frees_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let session = connect(&dir).await;
    let sql = "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1200) SELECT i FROM n";
    let mut stream = session.execute(QueryRequest::read(sql, 0)).await.unwrap();
    stream.next().await.unwrap().unwrap();
    drop(stream);
    assert_eq!(
        rows(&*session, "SELECT 7").await,
        vec![vec![DbValue::I64(7)]]
    );
}

#[tokio::test]
async fn the_transaction_state_follows_the_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let session = connect(&dir).await;
    assert!(session.catalog().is_some() && session.data().is_some());
    assert!(session.bulk().is_some() && session.explain().is_some());
    let control = session.transactions().unwrap();
    assert_eq!(control.state(), TransactionState::Idle);
    control.begin(TransactionMode::ReadWrite).await.unwrap();
    assert_eq!(control.state(), TransactionState::Active);
    control.rollback().await.unwrap();
    assert_eq!(control.state(), TransactionState::Idle);
    session.close().await.unwrap();
}

/// A plugin whose `session/open` takes `slow` to answer.
fn slow_open(slow: std::time::Duration) -> dexo_app::plugin::host::Spawner {
    use dexo_plugin_protocol::{Request, Response, read_line, write_line};
    Box::new(move || {
        let (host_reader, mut plugin_writer) = std::io::pipe()?;
        let (plugin_reader, host_writer) = std::io::pipe()?;
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(plugin_reader);
            let mut line = Vec::new();
            while read_line(&mut reader, &mut line).unwrap_or(false) {
                let request: Request = serde_json::from_slice(&line).unwrap();
                let reply = match request.method.as_str() {
                    "initialize" => serde_json::json!({
                        "protocol": 1, "driver_version": "1", "drivers": ["sqlite"]}),
                    "session/open" => {
                        std::thread::sleep(slow);
                        serde_json::json!({"session": "s1", "capabilities": [], "families": []})
                    }
                    _ => serde_json::Value::Null,
                };
                let _ = write_line(&mut plugin_writer, &Response::ok(request.id, reply));
            }
        });
        Ok(Spawned {
            reader: Box::new(host_reader),
            writer: Box::new(host_writer),
            stderr: None,
            child: None,
        })
    })
}

#[tokio::test]
async fn a_session_opening_when_the_plugin_goes_idle_keeps_it() {
    let host = Arc::new(
        PluginHost::new(
            "Slow",
            vec!["sqlite".into()],
            slow_open(std::time::Duration::from_millis(300)),
        )
        .with_idle(std::time::Duration::from_millis(50)),
    );
    host.live().await.unwrap();
    // The last session closes: the plugin stops in 50 ms unless one opens.
    host.session_opened();
    host.session_closed();
    let opened = PluginFactory::new(DriverDescriptor::sqlite(), host)
        .connect(ConnectRequest::new(
            "x",
            None,
            String::new(),
            SecretString::from(String::new()),
            false,
        ))
        .await;
    assert!(opened.is_ok(), "{:?}", opened.err());
}
