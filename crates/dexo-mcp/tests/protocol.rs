mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use common::{Client, FakeBackend, connection, is_error, profile, text};
use dexo_app::mcp::grant::{DEFAULT_TTL_SECS, Grant, GrantCapability};
use dexo_app::mcp::{Effect, GrantLedger, MemoryGrantLedger, SelectorRule};
use dexo_driver_api::DbValue;
use dexo_test_support::FakeSession;
use serde_json::json;

fn users() -> FakeSession {
    FakeSession::with_rows(&["id"], vec![vec![DbValue::I64(1)]])
}

async fn client_with(backend: FakeBackend) -> (Client, Arc<FakeBackend>, Arc<MemoryGrantLedger>) {
    let backend = Arc::new(backend);
    let ledger = Arc::new(MemoryGrantLedger::default());
    let client = Client::start(
        profile(),
        vec![connection("local")],
        Arc::clone(&backend),
        Arc::clone(&ledger),
    )
    .await;
    (client, backend, ledger)
}

#[tokio::test]
async fn tools_are_typed_and_described() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    let tools = client.tools().await;
    let read = tools
        .iter()
        .find(|tool| tool["name"] == "query_execute_read")
        .expect("a raw-read profile lists query_execute_read");
    assert_eq!(read["inputSchema"]["required"], json!(["sql"]));
    assert_eq!(read["annotations"]["readOnlyHint"], json!(true));
    for tool in &tools {
        let description = tool["description"].as_str().unwrap_or_default();
        assert!(
            description.len() > 20,
            "{} needs a real description",
            tool["name"]
        );
    }
}

#[tokio::test]
async fn a_read_returns_a_table_with_its_columns() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    let result = client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    assert!(!is_error(&result), "{result}");
    assert!(text(&result).starts_with("| id |"), "{}", text(&result));
    assert_eq!(result["structuredContent"]["rows"], json!([["1"]]));
}

#[tokio::test]
async fn a_failed_connect_is_retried_on_the_next_call() {
    let backend = FakeBackend::with_session("local", users());
    backend.fail_next_connect.store(true, Ordering::SeqCst);
    let (mut client, backend, _) = client_with(backend).await;
    let first = client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    assert!(
        text(&first).starts_with("Error [CONNECTION_FAILED]"),
        "{first}"
    );
    let second = client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    assert!(!is_error(&second), "{second}");
    assert_eq!(*backend.connects.lock().unwrap(), ["local", "local"]);
}

#[tokio::test]
async fn a_grant_publishes_its_tool_and_revoking_removes_it() {
    let (mut client, _, ledger) = client_with(FakeBackend::with_session("local", users())).await;
    assert!(
        !client
            .tool_names()
            .await
            .contains(&"data_insert".to_string())
    );
    let grant = Grant::new(
        &profile(),
        "local",
        GrantCapability::DataWrite,
        vec!["data_insert".into()],
        vec![SelectorRule::parse(Effect::Allow, "db.public.users").unwrap()],
        dexo_mcp::tools_write::now_secs(),
        DEFAULT_TTL_SECS,
    )
    .unwrap();
    let id = grant.id;
    ledger.insert_grant(grant).unwrap();
    assert!(
        client
            .saw_notification("notifications/tools/list_changed")
            .await
    );
    let names = client.tool_names().await;
    assert!(names.contains(&"data_insert".to_string()));
    assert!(!names.iter().any(|name| name.starts_with("grant_")));
    ledger.revoke(id).unwrap();
    client.notifications.clear();
    assert!(
        client
            .saw_notification("notifications/tools/list_changed")
            .await
    );
    assert!(
        !client
            .tool_names()
            .await
            .contains(&"data_insert".to_string())
    );
}
