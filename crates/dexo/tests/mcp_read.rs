use dexo_app::mcp::{Effect, McpProfile, McpService, QueryMode, SelectorRule};
use dexo_driver_api::{
    CatalogObject, ConnectRequest, ConnectionFactory, ObjectId, ObjectKind, QualifiedName, Session,
};
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_test_support::DatabasePair;
use secrecy::SecretString;

fn table(name: &str) -> CatalogObject {
    CatalogObject::new(
        ObjectId::new(name),
        ObjectKind::Table,
        QualifiedName::new(Some("dexo"), Some("public"), name),
        None,
    )
}

fn read_profile() -> McpProfile {
    let mut profile = McpProfile::new("reader");
    profile.query_mode = QueryMode::RawReadSql;
    profile.selectors = vec![SelectorRule::parse(Effect::Allow, "dexo.public.items").unwrap()];
    profile
}

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    use futures_util::StreamExt;
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_and_mysql_reject_mutating_mcp_reads() {
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
        let service = McpService::new(read_profile(), vec![table("items")]);
        assert!(
            service
                .execute_read(session, "WITH x AS (SELECT 1) DELETE FROM items")
                .await
                .is_err()
        );
        assert!(
            service
                .execute_read(session, "SELECT 1 FROM secrets")
                .await
                .is_err()
        );
        let ok = service.execute_read(session, "SELECT 1").await;
        assert!(ok.is_ok() || ok.unwrap_err().to_string().contains("not found"));
    }
}
