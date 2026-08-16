use dexo_driver_api::{
    CatalogListOptions, CatalogObject, ConnectRequest, ConnectionFactory, ObjectId, Session,
};
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_storage::{CatalogCache, Database};
use dexo_test_support::DatabasePair;
use dexo_test_support::containers::TEST_PASSWORD;
use futures_util::StreamExt;
use secrecy::SecretString;

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
}

async fn child_named(
    catalog: &dyn dexo_driver_api::CatalogReader,
    parent: Option<&ObjectId>,
    name: &str,
) -> CatalogObject {
    catalog
        .list_children(parent, &CatalogListOptions::default())
        .await
        .unwrap()
        .objects
        .into_iter()
        .find(|object| object.qualified_name.object() == name)
        .unwrap_or_else(|| panic!("missing catalog object {name}"))
}

async fn live_catalog_round_trip(
    session: Box<dyn Session>,
    table_sql: &str,
    schema: &str,
    table_name: &str,
) {
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(table_sql))
            .await
            .unwrap(),
    )
    .await;
    let catalog = session.catalog().expect("catalog");
    let roots = catalog
        .list_children(None, &CatalogListOptions::default())
        .await
        .unwrap();
    assert!(!roots.objects.is_empty());
    let schema_object = {
        let mut found = None;
        for root in &roots.objects {
            if root.qualified_name.object() == schema {
                found = Some(root.clone());
                break;
            }
            if let Ok(list) = catalog
                .list_children(Some(&root.id), &CatalogListOptions::default())
                .await
                && let Some(object) = list
                    .objects
                    .into_iter()
                    .find(|object| object.qualified_name.object() == schema)
            {
                found = Some(object);
                break;
            }
        }
        found.expect("schema")
    };
    let table = child_named(catalog, Some(&schema_object.id), table_name).await;
    let ddl = catalog.ddl(&table.id).await.unwrap();
    assert!(ddl.sql.to_ascii_uppercase().contains("CREATE TABLE"));
    let _ = catalog.dependencies(&table.id).await;
    let db = Database::open_in_memory().unwrap();
    CatalogCache::new(db.connection())
        .replace_snapshot("c1", "dexo", std::slice::from_ref(&table))
        .unwrap();
    let loaded = CatalogCache::new(db.connection())
        .load_latest("c1", "dexo")
        .unwrap();
    assert!(
        loaded
            .iter()
            .any(|object| object.qualified_name.object() == table_name)
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_live_catalog_round_trip() {
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
    live_catalog_round_trip(
        session,
        "CREATE TABLE live_orders (id int PRIMARY KEY)",
        "public",
        "live_orders",
    )
    .await;
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mysql_live_catalog_round_trip() {
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
    live_catalog_round_trip(
        session,
        "CREATE TABLE live_orders (id int PRIMARY KEY)",
        "dexo",
        "live_orders",
    )
    .await;
}
