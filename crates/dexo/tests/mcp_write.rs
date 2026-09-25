use dexo_app::Environment;
use dexo_app::mcp::grant::{DEFAULT_TTL_SECS, Grant, GrantCapability};
use dexo_app::mcp::ledger::{GrantLedger, MemoryGrantLedger};
use dexo_app::mcp::{Effect, McpConnection, McpProfile, McpService, SelectorRule};
use dexo_driver_api::{ConnectRequest, ConnectionFactory, Session};
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_mcp::tools_write::call_write_tool;
use dexo_sql::Dialect;
use dexo_test_support::DatabasePair;
use secrecy::SecretString;
use serde_json::json;

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    use futures_util::StreamExt;
    while let Some(event) = stream.next().await {
        let _ = event;
    }
}

fn connection(dialect: Dialect) -> McpConnection {
    McpConnection {
        name: "local".into(),
        driver: dialect.name().into(),
        dialect,
        database: Some("dexo".into()),
        default_schema: (dialect == Dialect::Postgres).then(|| "public".to_string()),
        environment: Environment::Local,
        read_only: false,
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
        .connect(ConnectRequest::new(
            pair.postgres_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
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
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
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

    for (session, dialect) in [
        (&*pg as &dyn Session, Dialect::Postgres),
        (&*mysql as &dyn Session, Dialect::Mysql),
    ] {
        let connection = connection(dialect);
        let profile = write_profile();
        let service = McpService::new(profile.clone());
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
            Some((&connection, session)),
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
    }
}
