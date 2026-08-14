use dexo_app::mcp::grant::{DEFAULT_TTL_SECS, Grant, GrantCapability};
use dexo_app::mcp::ledger::{GrantLedger, MemoryGrantLedger};
use dexo_app::mcp::{Effect, McpProfile, McpService, SelectorRule};
use dexo_driver_api::{
    CatalogObject, ConnectRequest, ConnectionFactory, ObjectId, ObjectKind, QualifiedName, Session,
};
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_mcp::tools_write::call_write_tool;
use dexo_test_support::DatabasePair;
use secrecy::SecretString;
use serde_json::json;

fn table(name: &str) -> CatalogObject {
    CatalogObject::new(
        ObjectId::new(name),
        ObjectKind::Table,
        QualifiedName::new(Some("dexo"), Some("public"), name),
        None,
    )
}

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    use futures_util::StreamExt;
    while let Some(event) = stream.next().await {
        let _ = event;
    }
}

fn write_profile() -> McpProfile {
    let mut profile = McpProfile::new("writer");
    profile.selectors = vec![SelectorRule::parse(Effect::Allow, "dexo.public.items").unwrap()];
    profile
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_and_mysql_keep_mcp_capabilities_isolated() {
    let pair = DatabasePair::start().await.unwrap();
    let pg = PostgresFactory
        .connect(ConnectRequest {
            endpoint: pair.postgres_endpoint().to_string(),
            database: Some("dexo".into()),
            username: "dexo".into(),
            secret: SecretString::from("dexo_test_only"),
            read_only: false,
        })
        .await
        .unwrap();
    drain(
        pg.execute(dexo_driver_api::QueryRequest::write(
            "CREATE TABLE IF NOT EXISTS items (id int primary key)",
        ))
        .await
        .unwrap(),
    )
    .await;
    let mysql = MysqlFactory
        .connect(ConnectRequest {
            endpoint: pair.mysql_endpoint().to_string(),
            database: Some("dexo".into()),
            username: "dexo".into(),
            secret: SecretString::from("dexo_test_only"),
            read_only: false,
        })
        .await
        .unwrap();
    drain(
        mysql
            .execute(dexo_driver_api::QueryRequest::write(
                "CREATE TABLE IF NOT EXISTS items (id int primary key)",
            ))
            .await
            .unwrap(),
    )
    .await;

    for session in [&*pg as &dyn Session, &*mysql as &dyn Session] {
        let profile = write_profile();
        let service = McpService::new(profile.clone(), vec![table("items")]);
        let ledger = MemoryGrantLedger::default();
        ledger
            .insert_grant(
                Grant::new(
                    &profile,
                    "local",
                    GrantCapability::DataWrite,
                    vec!["data_insert".into()],
                    vec![SelectorRule::parse(Effect::Allow, "dexo.public.items").unwrap()],
                    0,
                    DEFAULT_TTL_SECS,
                )
                .unwrap(),
            )
            .unwrap();
        let denied = call_write_tool(
            &service,
            &ledger,
            Some(session),
            "s",
            "schema_apply_ddl",
            json!({
                "operation_id":"op-ddl",
                "target":"dexo.public.items",
                "sql":"DROP TABLE items",
                "confirm_target":"dexo.public.items"
            })
            .as_object()
            .cloned()
            .unwrap(),
            0,
        )
        .await
        .unwrap_err();
        assert!(denied.to_string().contains("not found"));
        let hidden = service.describe("secrets");
        assert!(hidden.is_err());
    }
}
