use dexo_driver_api::{CatalogListOptions, ConnectRequest, ConnectionFactory, ObjectKind, Session};
use dexo_driver_postgres::PostgresFactory;
use dexo_test_support::DatabasePair;
use futures_util::StreamExt;
use secrecy::SecretString;

struct Fixture {
    _pair: DatabasePair,
    session: Box<dyn Session>,
}

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
}

async fn connect_seeded() -> Fixture {
    let pair = DatabasePair::start().await.unwrap();
    let session = PostgresFactory
        .connect(ConnectRequest::new(
            pair.postgres_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    let statements = [
        "CREATE SCHEMA dexo_catalog",
        "CREATE TYPE dexo_catalog.mood AS ENUM ('sad', 'ok', 'happy')",
        "CREATE DOMAIN dexo_catalog.posint AS int CHECK (VALUE > 0)",
        "CREATE SEQUENCE dexo_catalog.order_seq",
        "CREATE TABLE dexo_catalog.orders (
  id int PRIMARY KEY DEFAULT nextval('dexo_catalog.order_seq'),
  status dexo_catalog.mood,
  qty dexo_catalog.posint
) PARTITION BY RANGE (id)",
        "CREATE TABLE dexo_catalog.orders_p0 PARTITION OF dexo_catalog.orders FOR VALUES FROM (0) TO (1000)",
        "CREATE MATERIALIZED VIEW dexo_catalog.orders_mv AS SELECT id FROM dexo_catalog.orders",
        "CREATE EXTENSION IF NOT EXISTS postgres_fdw",
        "CREATE PUBLICATION dexo_catalog_pub FOR TABLE dexo_catalog.orders",
        "CREATE FUNCTION dexo_catalog.add1(n int) RETURNS int LANGUAGE sql IMMUTABLE AS 'SELECT n + 1'",
        "CREATE PROCEDURE dexo_catalog.noop() LANGUAGE plpgsql AS 'BEGIN NULL; END;'",
        "CREATE FUNCTION dexo_catalog.tg_fn() RETURNS trigger LANGUAGE plpgsql AS 'BEGIN RETURN NEW; END;'",
        "CREATE TRIGGER orders_tg BEFORE INSERT ON dexo_catalog.orders FOR EACH ROW EXECUTE FUNCTION dexo_catalog.tg_fn()",
        "ALTER TABLE dexo_catalog.orders ENABLE ROW LEVEL SECURITY",
        "CREATE POLICY orders_all ON dexo_catalog.orders USING (true)",
        "GRANT SELECT ON dexo_catalog.orders TO PUBLIC",
    ];
    for statement in statements {
        drain(
            session
                .execute(dexo_driver_api::QueryRequest::write(statement))
                .await
                .unwrap(),
        )
        .await;
    }
    Fixture {
        _pair: pair,
        session,
    }
}

fn has_kind_named(
    objects: &[dexo_driver_api::CatalogObject],
    kind: ObjectKind,
    name: &str,
) -> bool {
    objects
        .iter()
        .any(|object| object.kind == kind && object.qualified_name.object().ends_with(name))
}

fn has_specific(objects: &[dexo_driver_api::CatalogObject], kind: &str, name: &str) -> bool {
    objects.iter().any(|object| {
        matches!(&object.kind, ObjectKind::DriverSpecific(value) if value == kind)
            && object.qualified_name.object().ends_with(name)
    })
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_catalog_contract() {
    let fixture = connect_seeded().await;
    let catalog = fixture.session.catalog().expect("catalog capability");
    let roots = catalog
        .list_children(None, &CatalogListOptions::default())
        .await
        .unwrap();
    assert!(has_kind_named(&roots.objects, ObjectKind::Catalog, "dexo"));
    let database = &roots.objects[0];

    let children = catalog
        .list_children(Some(&database.id), &CatalogListOptions::default())
        .await
        .unwrap();
    assert!(
        !children
            .objects
            .iter()
            .any(|object| object.qualified_name.object() == "pg_catalog")
    );
    assert!(has_kind_named(
        &children.objects,
        ObjectKind::Schema,
        "dexo_catalog"
    ));
    assert!(has_specific(&children.objects, "extension", "postgres_fdw"));
    assert!(has_specific(&children.objects, "fdw", "postgres_fdw"));
    assert!(has_specific(
        &children.objects,
        "publication",
        "dexo_catalog_pub"
    ));
    assert!(
        children
            .objects
            .iter()
            .any(|object| object.kind == ObjectKind::User || object.kind == ObjectKind::Role)
    );

    let with_system = catalog
        .list_children(
            Some(&database.id),
            &CatalogListOptions {
                include_system: true,
            },
        )
        .await
        .unwrap();
    assert!(
        with_system
            .objects
            .iter()
            .any(|object| object.qualified_name.object() == "pg_catalog")
    );

    let schema = children
        .objects
        .iter()
        .find(|object| {
            object.kind == ObjectKind::Schema && object.qualified_name.object() == "dexo_catalog"
        })
        .unwrap();
    let schema_children = catalog
        .list_children(Some(&schema.id), &CatalogListOptions::default())
        .await
        .unwrap();
    assert!(has_kind_named(
        &schema_children.objects,
        ObjectKind::Table,
        "orders"
    ));
    assert!(has_kind_named(
        &schema_children.objects,
        ObjectKind::Sequence,
        "order_seq"
    ));
    assert!(has_kind_named(
        &schema_children.objects,
        ObjectKind::MaterializedView,
        "orders_mv"
    ));
    assert!(has_kind_named(
        &schema_children.objects,
        ObjectKind::Function,
        "add1"
    ));
    assert!(has_kind_named(
        &schema_children.objects,
        ObjectKind::Procedure,
        "noop"
    ));
    assert!(has_specific(&schema_children.objects, "enum", "mood"));
    assert!(has_specific(&schema_children.objects, "domain", "posint"));

    let table = schema_children
        .objects
        .iter()
        .find(|object| object.kind == ObjectKind::Table)
        .unwrap();
    assert!(
        table
            .attributes
            .contains_key("driver.postgres.partition_key")
    );
    assert!(table.attributes.contains_key("driver.postgres.oid"));
    assert_ne!(table.id.as_str(), table.qualified_name.display_unquoted());

    let table_children = catalog
        .list_children(Some(&table.id), &CatalogListOptions::default())
        .await
        .unwrap();
    assert!(has_kind_named(
        &table_children.objects,
        ObjectKind::Column,
        "orders.id"
    ));
    assert!(has_kind_named(
        &table_children.objects,
        ObjectKind::Constraint,
        "orders_pkey"
    ));
    assert!(has_kind_named(
        &table_children.objects,
        ObjectKind::Trigger,
        "orders_tg"
    ));
    assert!(has_specific(
        &table_children.objects,
        "partition",
        "orders_p0"
    ));
    assert!(has_specific(
        &table_children.objects,
        "policy",
        "orders_all"
    ));

    let ddl = catalog.ddl(&table.id).await.unwrap();
    assert!(ddl.sql.to_ascii_uppercase().contains("CREATE TABLE"));
    let deps = catalog.dependents(&table.id).await.unwrap();
    assert!(!deps.is_empty());
}
