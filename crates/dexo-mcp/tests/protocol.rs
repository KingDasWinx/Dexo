use std::time::Duration;

use dexo_app::mcp::{Effect, McpProfile, McpService, QueryMode, SelectorRule};
use dexo_driver_api::{CatalogObject, ObjectId, ObjectKind, QualifiedName};
use dexo_mcp::resources::{ResultStore, list_resources, read_resource};
use dexo_mcp::tools_read::call_tool;
use serde_json::json;

fn table(name: &str) -> CatalogObject {
    CatalogObject::new(
        ObjectId::new(name),
        ObjectKind::Table,
        QualifiedName::new(Some("db"), Some("public"), name),
        None,
    )
}

fn service() -> McpService {
    let mut profile = McpProfile::new("assistant");
    profile.query_mode = QueryMode::RawReadSql;
    profile.selectors = vec![
        SelectorRule::parse(Effect::Allow, "db.public.*").unwrap(),
        SelectorRule::parse(Effect::Deny, "db.public.secrets").unwrap(),
    ];
    McpService::new(profile, vec![table("users"), table("secrets")])
}

#[test]
fn denied_targets_are_absent_from_resource_list() {
    let service = service();
    let store = ResultStore::new("assistant");
    let listed = list_resources(&service, &store);
    let blob = listed
        .iter()
        .map(|resource| format!("{} {}", resource.uri, resource.name))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(blob.contains("users"));
    assert!(!blob.contains("secrets"));
    assert!(read_resource(&service, &store, "dexo://object/secrets").is_err());
}

#[test]
fn expired_result_is_generic_not_found() {
    let mut store = ResultStore::new("assistant");
    store.insert(
        "dexo://result/expired".into(),
        "secret-rows".into(),
        Duration::from_millis(1),
    );
    std::thread::sleep(Duration::from_millis(5));
    let error = store.get("dexo://result/expired").unwrap_err();
    assert_eq!(error, "not found");
}

#[test]
fn mutating_sql_is_rejected_before_data() {
    let service = service();
    let result = call_tool(
        &service,
        "query_execute_read",
        json!({"sql": "WITH x AS (SELECT 1) DELETE FROM users"})
            .as_object()
            .cloned()
            .unwrap(),
    );
    let text = format!("{result:?}");
    assert!(text.contains("statement rejected") || text.contains("is_error: Some(true)"));
    let denied = call_tool(
        &service,
        "query_execute_read",
        json!({"sql": "SELECT 1 FROM secrets"})
            .as_object()
            .cloned()
            .unwrap(),
    );
    let denied_text = format!("{denied:?}");
    assert!(denied_text.contains("not found") || denied_text.contains("statement rejected"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialize_stdout_is_jsonrpc() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let mut profile = McpProfile::new("conformance-fixture");
    profile.enabled = true;
    profile.selectors = vec![SelectorRule::parse(Effect::Allow, "db.public.*").unwrap()];
    let service = McpService::new(profile, vec![table("users"), table("secrets")]);
    let (client, server_io) = tokio::io::duplex(64 * 1024);
    let (server_read, server_write) = tokio::io::split(server_io);
    let (client_read, mut client_write) = tokio::io::split(client);
    let handle =
        tokio::spawn(async move { dexo_mcp::serve_io(service, server_read, server_write).await });
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0.0.1"}
        }
    });
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    client_write
        .write_all(format!("{init}\n{initialized}\n").as_bytes())
        .await
        .unwrap();
    client_write.flush().await.unwrap();
    let mut lines = BufReader::new(client_read).lines();
    let line = tokio::time::timeout(std::time::Duration::from_secs(5), lines.next_line())
        .await
        .expect("timeout waiting for initialize result")
        .unwrap()
        .expect("line");
    let value: serde_json::Value = serde_json::from_str(&line).expect("json-rpc");
    assert_eq!(value["jsonrpc"], "2.0");
    assert!(value.get("result").is_some() || value.get("error").is_some());
    let ping = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"ping"});
    let tools = serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/list"});
    client_write
        .write_all(format!("{ping}\n{tools}\n").as_bytes())
        .await
        .unwrap();
    let ping_line = tokio::time::timeout(std::time::Duration::from_secs(5), lines.next_line())
        .await
        .expect("timeout ping")
        .unwrap()
        .expect("ping line");
    let ping_value: serde_json::Value = serde_json::from_str(&ping_line).unwrap();
    assert_eq!(ping_value["jsonrpc"], "2.0");
    drop(client_write);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
}

#[test]
fn disconnect_clears_result_pages() {
    let server = dexo_mcp::DexoMcpServer::new(service());
    let store = server.store();
    store.lock().unwrap().insert(
        "dexo://result/x".into(),
        "rows".into(),
        std::time::Duration::from_secs(60),
    );
    assert!(store.lock().unwrap().get("dexo://result/x").is_ok());
    drop(server);
    assert_eq!(
        store.lock().unwrap().get("dexo://result/x").unwrap_err(),
        "not found"
    );
}
