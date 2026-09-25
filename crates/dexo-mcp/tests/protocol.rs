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

fn catalog() -> Vec<dexo_driver_api::CatalogObject> {
    use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
    let node = |id: &str, kind, name: QualifiedName, parent: Option<&str>| {
        CatalogObject::new(ObjectId::new(id), kind, name, parent.map(ObjectId::new))
    };
    let public = |object: &str| QualifiedName::new(Some("db"), Some("public"), object);
    vec![
        node(
            "db",
            ObjectKind::Catalog,
            QualifiedName::new(Some("db"), None::<String>, "db"),
            None,
        ),
        node("public", ObjectKind::Schema, public("public"), Some("db")),
        node("users", ObjectKind::Table, public("users"), Some("public")),
        node(
            "secrets",
            ObjectKind::Table,
            public("secrets"),
            Some("public"),
        ),
        node(
            "secrets.email",
            ObjectKind::Column,
            public("secrets.email"),
            Some("secrets"),
        ),
        node(
            "users.id",
            ObjectKind::Column,
            public("users.id"),
            Some("users"),
        )
        .with_attribute("type", json!("int4")),
    ]
}

#[tokio::test]
async fn a_denied_table_is_invisible_to_every_catalog_tool() {
    let mut backend = FakeBackend::with_session("local", users().with_catalog(catalog()));
    backend.catalog = catalog();
    let (mut client, _, _) = client_with(backend).await;
    let listed = client
        .call("catalog_list", json!({"parent_id": "public"}))
        .await;
    assert!(text(&listed).contains("db.public.users"));
    assert!(!text(&listed).contains("secrets"), "{}", text(&listed));
    let columns = client
        .call("catalog_list", json!({"parent_id": "secrets"}))
        .await;
    assert!(!text(&columns).contains("email"));
    let search = client
        .call("catalog_search", json!({"query": "secr"}))
        .await;
    assert!(!text(&search).contains("secrets"));
    for tool in ["object_describe", "object_get_ddl", "object_relationships"] {
        let hidden = client.call(tool, json!({"name": "secrets"})).await;
        assert_eq!(text(&hidden), "Error [NOT_FOUND]: not found", "{tool}");
    }
    let described = client
        .call("object_describe", json!({"name": "users"}))
        .await;
    assert!(
        text(&described).contains("| id | int4 |"),
        "{}",
        text(&described)
    );
}
