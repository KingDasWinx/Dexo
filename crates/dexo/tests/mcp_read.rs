use dexo_app::Environment;
use dexo_app::mcp::{Effect, McpConnection, McpProfile, McpService, QueryMode, SelectorRule};
use dexo_driver_api::{ConnectRequest, ConnectionFactory, QueryRequest, Session};
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_sql::Dialect;
use dexo_test_support::DatabasePair;
use futures_util::StreamExt;
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;

fn read_profile() -> McpProfile {
    let mut profile = McpProfile::new("reader");
    profile.query_mode = QueryMode::RawReadSql;
    profile.selectors = vec![
        SelectorRule::parse(Effect::Allow, "dexo.public.items").unwrap(),
        SelectorRule::parse(Effect::Allow, "dexo.items").unwrap(),
    ];
    profile
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

async fn run(session: &dyn Session, sql: &str) {
    let mut stream = session.execute(QueryRequest::write(sql)).await.unwrap();
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
}

async fn connect(factory: &dyn ConnectionFactory, endpoint: &str) -> Box<dyn Session> {
    factory
        .connect(ConnectRequest::new(
            endpoint.to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mcp_reads_cannot_write_on_postgres_or_mysql() {
    let pair = DatabasePair::start().await.unwrap();
    let pg = connect(&PostgresFactory, pair.postgres_endpoint()).await;
    run(
        &*pg,
        "CREATE TABLE IF NOT EXISTS items (id int primary key)",
    )
    .await;
    // `sneaky()` runs once per row: with an empty table it would never try to write.
    run(&*pg, "INSERT INTO items VALUES (1) ON CONFLICT DO NOTHING").await;
    run(
        &*pg,
        "CREATE OR REPLACE FUNCTION sneaky() RETURNS int LANGUAGE sql AS $$ INSERT INTO items VALUES (99) RETURNING 1 $$",
    )
    .await;
    let mysql = connect(&MysqlFactory, pair.mysql_endpoint()).await;
    run(
        &*mysql,
        "CREATE TABLE IF NOT EXISTS items (id int primary key)",
    )
    .await;

    let service = McpService::new(read_profile());
    let cancel = CancellationToken::new();
    for (session, dialect) in [(&*pg, Dialect::Postgres), (&*mysql, Dialect::Mysql)] {
        let connection = connection(dialect);
        assert!(
            service
                .execute_read(session, &connection, "select count(*) from items", &cancel)
                .await
                .is_ok()
        );
        assert!(
            service
                .execute_read(
                    session,
                    &connection,
                    "select * from items for update",
                    &cancel
                )
                .await
                .is_err()
        );
        assert!(
            service
                .execute_read(session, &connection, "select 1 from secrets", &cancel)
                .await
                .is_err()
        );
    }
    let written = service
        .execute_read(
            &*pg,
            &connection(Dialect::Postgres),
            "select sneaky() from items",
            &cancel,
        )
        .await
        .unwrap_err();
    assert!(written.to_string().contains("read-only"), "{written}");
}
