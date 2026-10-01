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

#[tokio::test]
async fn data_read_pages_and_says_where_to_continue() {
    let rows = (1..=5).map(|id| vec![DbValue::I64(id)]).collect();
    let (mut client, _, _) = client_with(FakeBackend::with_session(
        "local",
        FakeSession::with_rows(&["id"], rows),
    ))
    .await;
    let first = client
        .call("data_read", json!({"table": "users", "limit": 2}))
        .await;
    assert_eq!(first["structuredContent"]["rows"], json!([["1"], ["2"]]));
    assert_eq!(first["structuredContent"]["next_offset"], json!(2));
    let last = client
        .call(
            "data_read",
            json!({"table": "users", "offset": 4, "limit": 2}),
        )
        .await;
    assert_eq!(last["structuredContent"]["rows"], json!([["5"]]));
    assert_eq!(last["structuredContent"]["next_offset"], json!(null));
    let hidden = client.call("data_read", json!({"table": "secrets"})).await;
    assert_eq!(text(&hidden), "Error [NOT_FOUND]: not found");
}

#[tokio::test]
async fn schema_diff_hides_denied_objects() {
    use dexo_app::schema_diff::SchemaSnapshot;
    let table = |name: &str| {
        dexo_driver_api::CatalogObject::new(
            dexo_driver_api::ObjectId::new(name),
            dexo_driver_api::ObjectKind::Table,
            dexo_driver_api::QualifiedName::new(Some("db"), Some("public"), name),
            None,
        )
    };
    let snapshot =
        |objects| SchemaSnapshot::capture("postgres", "16", "2026-09-23T00:00:00Z", "db", objects);
    let mut backend = FakeBackend::with_session("local", users());
    backend
        .snapshots
        .insert("before".into(), snapshot(vec![table("users")]));
    backend.snapshots.insert(
        "after".into(),
        snapshot(vec![table("users"), table("orders"), table("secrets")]),
    );
    let (mut client, _, _) = client_with(backend).await;
    let diff = client
        .call(
            "schema_diff",
            json!({"from_snapshot": "before", "to_snapshot": "after"}),
        )
        .await;
    assert!(
        text(&diff).contains("| added | db.public.orders |"),
        "{}",
        text(&diff)
    );
    assert!(!text(&diff).contains("secrets"));
    let missing = client
        .call(
            "schema_diff",
            json!({"from_snapshot": "nope", "to_snapshot": "after"}),
        )
        .await;
    assert_eq!(text(&missing), "Error [NOT_FOUND]: not found");
}

#[tokio::test]
async fn admin_reads_need_an_explicit_rule() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    assert!(
        !client
            .tool_names()
            .await
            .contains(&"admin_list_sessions".to_string())
    );
    let hidden = client.call("admin_list_sessions", json!({})).await;
    assert_eq!(text(&hidden), "Error [NOT_FOUND]: not found");

    let mut allowed = profile();
    allowed.tool_rules.push(dexo_app::mcp::ToolRule {
        tool: "admin_list_sessions".into(),
        allowed: true,
    });
    let mut client = Client::start(
        allowed,
        vec![connection("local")],
        Arc::new(FakeBackend::with_session("local", users())),
        Arc::new(MemoryGrantLedger::default()),
    )
    .await;
    assert!(
        client
            .tool_names()
            .await
            .contains(&"admin_list_sessions".to_string())
    );
    let unsupported = client.call("admin_list_sessions", json!({})).await;
    assert!(text(&unsupported).starts_with("Error [UNSUPPORTED]"));
}

#[tokio::test]
async fn every_call_is_audited_without_its_sql() {
    let (mut client, _, ledger) = client_with(FakeBackend::with_session("local", users())).await;
    client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    client.call("grant_create", json!({})).await;
    let events = ledger.audits();
    let read = events
        .iter()
        .find(|event| event.request == "tools/call query_execute_read")
        .expect("the read is audited");
    assert_eq!(read.decision, "allow");
    assert_eq!(read.status, "ok");
    assert_eq!(read.rows, 1);
    assert!(
        read.sql
            .as_deref()
            .is_some_and(|hash| !hash.contains("select"))
    );
    let denied = events
        .iter()
        .find(|event| event.request == "tools/call grant_create")
        .expect("the denied call is audited");
    assert_eq!(denied.decision, "deny");
    assert_eq!(denied.status, "NOT_FOUND");
}

#[tokio::test]
async fn resources_and_prompts_do_not_leak_policy_or_sql() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    let listed = client.request("resources/list", json!({})).await;
    assert_eq!(
        listed["result"]["resources"].as_array().map(Vec::len),
        Some(1)
    );
    let capabilities = client
        .request(
            "resources/read",
            json!({"uri": "dexo://profile/capabilities"}),
        )
        .await;
    let body = capabilities["result"]["contents"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(body.contains("query_execute_read"));
    assert!(
        !body.contains("secrets"),
        "the allowlist is not published: {body}"
    );
    for name in ["explore_schema", "review_migration", "analyze_plan"] {
        let prompt = client.request("prompts/get", json!({"name": name})).await;
        let text = prompt["result"]["messages"][0]["content"]["text"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();
        assert!(!text.is_empty(), "{name}");
        assert!(!text.contains("select "), "{name}");
    }
    let unknown = client
        .request("resources/read", json!({"uri": "dexo://object/secrets"}))
        .await;
    assert!(unknown.get("error").is_some());
}

/// MCP-020 / QUALITY-015: any change to a tool's name, schema or annotations shows up
/// here. Review it with `cargo insta review`, bump TOOL_SCHEMA_VERSION and rename the
/// snapshot to match.
#[tokio::test]
async fn tool_contract_is_versioned() {
    assert_eq!(
        dexo_mcp::TOOL_SCHEMA_VERSION,
        2,
        "rename the snapshot below with the new version"
    );
    let mut everything = profile();
    for tool in ["admin_list_sessions", "data_execute_sql"] {
        everything.tool_rules.push(dexo_app::mcp::ToolRule {
            tool: tool.into(),
            allowed: true,
        });
    }
    let ledger = Arc::new(MemoryGrantLedger::default());
    let now = dexo_mcp::tools_write::now_secs();
    for (capability, tools) in [
        (
            GrantCapability::DataWrite,
            vec![
                "data_insert",
                "data_update",
                "data_delete",
                "data_execute_sql",
            ],
        ),
        (GrantCapability::Ddl, vec!["schema_apply_ddl"]),
        (
            GrantCapability::Admin,
            vec!["admin_cancel_query", "admin_terminate_session"],
        ),
    ] {
        ledger
            .insert_grant(
                Grant::new(
                    &everything,
                    "local",
                    capability,
                    tools.into_iter().map(String::from).collect(),
                    vec![SelectorRule::parse(Effect::Allow, "db.public.users").unwrap()],
                    now,
                    DEFAULT_TTL_SECS,
                )
                .unwrap(),
            )
            .unwrap();
    }
    let mut client = Client::start(
        everything,
        vec![connection("local")],
        Arc::new(FakeBackend::with_session("local", users())),
        ledger,
    )
    .await;
    insta::assert_json_snapshot!("tools_v2", client.tools().await);
}

/// MCP-002 / MCP-003: a structured-only profile exposes no raw SQL, no write and no
/// grant tool, and calling one anyway reads as "not found".
#[tokio::test]
async fn a_structured_profile_exposes_no_raw_sql_or_writes() {
    let mut structured = profile();
    structured.query_mode = dexo_app::mcp::QueryMode::StructuredOnly;
    let mut client = Client::start(
        structured,
        vec![connection("local")],
        Arc::new(FakeBackend::with_session("local", users())),
        Arc::new(MemoryGrantLedger::default()),
    )
    .await;
    let names = client.tool_names().await;
    for hidden in [
        "query_execute_read",
        "query_explain",
        "data_insert",
        "grant_create",
    ] {
        assert!(!names.contains(&hidden.to_string()), "{hidden}");
        let result = client.call(hidden, json!({"sql": "select 1"})).await;
        assert_eq!(text(&result), "Error [NOT_FOUND]: not found", "{hidden}");
    }
    assert!(names.contains(&"data_read".to_string()));
}

/// QUALITY-019: every bypass found in the 2026-09-23 review, over the protocol. None of
/// them may reach the session.
#[tokio::test]
async fn review_bypasses_never_reach_the_database() {
    let session = users();
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", session.clone())).await;
    for sql in [
        "select count(*) from users u join secrets s on true",
        "select count(*) from users, secrets",
        "select count(*) from\nsecrets",
        "select * into leaked from users",
        "select * from users for update",
        "select pg_terminate_backend(1) from users",
        "explain analyze delete from users",
        "with x as (delete from users returning *) select * from x",
        "select query_to_xml('select * from secrets', true, true, '')",
        "select 1; select 2",
    ] {
        let result = client.call("query_execute_read", json!({"sql": sql})).await;
        assert!(is_error(&result), "{sql} was accepted: {result}");
    }
    assert!(
        !session
            .log()
            .iter()
            .any(|entry| entry.starts_with("execute")),
        "{:?}",
        session.log()
    );
}

/// MCP-014: a cancel names one request and stops only that one. `data_read` needs the
/// same connection lock the hanging read holds, so it only answers if the cancelled
/// request really let go.
#[tokio::test]
async fn a_cancel_notification_only_stops_its_own_request() {
    let session = FakeSession::hanging();
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", session.clone())).await;
    let hanging = client
        .send_request(
            "tools/call",
            json!({"name": "query_execute_read", "arguments": {"sql": "select id from users"}}),
        )
        .await;
    client
        .notify(
            "notifications/cancelled",
            json!({"requestId": hanging, "reason": "test"}),
        )
        .await;
    let next = client.call("data_read", json!({"table": "users"})).await;
    assert!(!is_error(&next), "{next}");
    assert!(
        session.log().contains(&"rollback".to_string()),
        "{:?}",
        session.log()
    );
    let listed = client.call("list_connections", json!({})).await;
    assert!(!is_error(&listed), "{listed}");
}

/// MCP-019: with two connections the name is required, and each call reaches its own.
#[tokio::test]
async fn several_connections_need_an_explicit_name() {
    let sales = users();
    let reports = users();
    let mut backend = FakeBackend::with_session("sales", sales.clone());
    backend.sessions.insert("reports".into(), reports.clone());
    let mut two = profile();
    two.connections = vec!["sales".into(), "reports".into()];
    let mut client = Client::start(
        two,
        vec![connection("sales"), connection("reports")],
        Arc::new(backend),
        Arc::new(MemoryGrantLedger::default()),
    )
    .await;
    let ambiguous = client
        .call("query_execute_read", json!({"sql": "select id from users"}))
        .await;
    assert!(text(&ambiguous).contains("list_connections"), "{ambiguous}");
    let routed = client
        .call(
            "query_execute_read",
            json!({"connection": "reports", "sql": "select id from users"}),
        )
        .await;
    assert!(!is_error(&routed), "{routed}");
    assert!(sales.log().is_empty());
    assert!(
        reports
            .log()
            .iter()
            .any(|entry| entry.starts_with("execute"))
    );
    let unknown = client
        .call(
            "query_execute_read",
            json!({"connection": "prod", "sql": "select 1"}),
        )
        .await;
    assert_eq!(text(&unknown), "Error [NOT_FOUND]: not found");
}

/// MCP-015: production connections never accept an MCP write, grant or not.
#[tokio::test]
async fn production_connections_refuse_writes_over_the_protocol() {
    let mut production = connection("local");
    production.environment = dexo_app::Environment::Production;
    let ledger = Arc::new(MemoryGrantLedger::default());
    ledger
        .insert_grant(
            Grant::new(
                &profile(),
                "local",
                GrantCapability::DataWrite,
                vec!["data_insert".into()],
                vec![SelectorRule::parse(Effect::Allow, "db.public.users").unwrap()],
                dexo_mcp::tools_write::now_secs(),
                DEFAULT_TTL_SECS,
            )
            .unwrap(),
        )
        .unwrap();
    let mut client = Client::start(
        profile(),
        vec![production],
        Arc::new(FakeBackend::with_session("local", users())),
        Arc::clone(&ledger),
    )
    .await;
    let refused = client
        .call(
            "data_insert",
            json!({"operation_id": "op-prod", "target": "users", "values": {"id": 1}}),
        )
        .await;
    assert!(
        text(&refused).starts_with("Error [POLICY_DENIED]"),
        "{refused}"
    );
    assert_eq!(
        ledger
            .active_grants("assistant", dexo_mcp::tools_write::now_secs())
            .len(),
        1
    );
}

/// MCP-016: every listed tool rejects a call without its required arguments, as a
/// protocol error or a tool error, never as a silent default.
#[tokio::test]
async fn missing_required_arguments_are_rejected() {
    let (mut client, _, _) = client_with(FakeBackend::with_session("local", users())).await;
    for tool in client.tools().await {
        let required = tool["inputSchema"]["required"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if required.is_empty() {
            continue;
        }
        let name = tool["name"].as_str().unwrap_or_default().to_string();
        let response = client
            .request("tools/call", json!({"name": name, "arguments": {}}))
            .await;
        let rejected = response.get("error").is_some() || is_error(&response["result"]);
        assert!(
            rejected,
            "{name} accepted a call without {required:?}: {response}"
        );
    }
}
