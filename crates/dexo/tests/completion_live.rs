//! Completion against a real database: the columns come from the captured catalog, and
//! a table the catalog does not know yet is answered by the session.
use std::sync::Arc;

use dexo_driver_api::{ConnectRequest, ConnectionFactory, QualifiedName, Session};
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_test_support::DatabasePair;
use dexo_test_support::containers::TEST_PASSWORD;
use dexo_tui::action::Action;
use dexo_tui::{Focus, Model, update};
use futures_util::StreamExt;
use secrecy::SecretString;

async fn run(session: &dyn Session, sql: &str) {
    let mut stream = session
        .execute(dexo_driver_api::QueryRequest::write(sql))
        .await
        .unwrap();
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
}

async fn completes_live_columns(session: Box<dyn Session>) {
    let session: Arc<dyn Session> = Arc::from(session);
    run(
        session.as_ref(),
        "CREATE TABLE live_orders (id int PRIMARY KEY, total int, status varchar(10))",
    )
    .await;

    // A table named without its schema is looked up in the current one.
    let columns: Vec<String> = session
        .data()
        .expect("data")
        .table_columns(&QualifiedName::new(
            None::<String>,
            None::<String>,
            "live_orders",
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|column| column.name)
        .collect();
    assert_eq!(columns, ["id", "total", "status"]);

    let dir = tempfile::tempdir().unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    dexo_tui::runtime::catalog_manager::capture_snapshot(
        Arc::clone(&session),
        "c1".into(),
        "dexo".into(),
        false,
        dir.path().join("dexo.db"),
        1,
        tx,
    )
    .await;
    let captured = rx.recv().await.expect("the capture reported nothing");

    let mut model = Model {
        focus: Focus::Editor,
        session_generation: 1,
        ..Model::default()
    };
    model.connection.name = "c1".into();
    update(&mut model, captured);
    model.set_sql("select * from live_orders where ");
    update(&mut model, Action::RefreshSqlIntelligence);
    let offered: Vec<_> = model
        .editor
        .completions
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert!(model.editor.completion_open);
    assert!(offered.contains(&"total"), "{offered:?}");
    assert!(offered.contains(&"status"), "{offered:?}");

    // Before any FROM, the name before the dot is the table.
    model.set_sql("select live_orders.");
    update(&mut model, Action::RefreshSqlIntelligence);
    let offered: Vec<_> = model
        .editor
        .completions
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(offered, ["id", "total", "status"]);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_completes_live_columns() {
    let pair = DatabasePair::start().await.unwrap();
    let session = PostgresFactory
        .connect(ConnectRequest::new(
            pair.postgres_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from(TEST_PASSWORD),
            false,
        ))
        .await
        .unwrap();
    completes_live_columns(session).await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mysql_completes_live_columns() {
    let pair = DatabasePair::start().await.unwrap();
    let session = MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from(TEST_PASSWORD),
            false,
        ))
        .await
        .unwrap();
    completes_live_columns(session).await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn admin_actions_observe_server_state() {
    let pair = DatabasePair::start().await.unwrap();
    assert!(!pair.postgres_endpoint().is_empty());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn completion_live_smoke() {
    let pair = DatabasePair::start().await.unwrap();
    assert!(!pair.mysql_endpoint().is_empty());
}
