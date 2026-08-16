use dexo_driver_api::{CatalogListOptions, ConnectRequest, ConnectionFactory, ObjectKind, Session};
use dexo_driver_mysql::MysqlFactory;
use dexo_test_support::DatabasePair;
use futures_util::StreamExt;
use secrecy::SecretString;

struct Fixture {
    pair: DatabasePair,
    session: Box<dyn Session>,
}

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    while let Some(event) = stream.next().await {
        event.expect("seed statement event");
    }
}

async fn connect_seeded() -> Fixture {
    let pair = DatabasePair::start().await.unwrap();
    let root = MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "root".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    let statements = [
        "SET GLOBAL log_bin_trust_function_creators = 1",
        "CREATE TABLE orders (
            id INT PRIMARY KEY AUTO_INCREMENT,
            note VARCHAR(16) CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci
        ) ENGINE=InnoDB
        PARTITION BY RANGE (id) (
            PARTITION p0 VALUES LESS THAN (1000),
            PARTITION p1 VALUES LESS THAN MAXVALUE
        )",
        "CREATE TABLE customers (id INT PRIMARY KEY)",
        "CREATE TABLE order_items (
            id INT PRIMARY KEY,
            customer_id INT NOT NULL,
            CONSTRAINT order_items_customer_fk FOREIGN KEY (customer_id) REFERENCES customers(id)
        ) ENGINE=InnoDB",
        "CREATE TABLE generated_demo (
            id INT PRIMARY KEY,
            total INT GENERATED ALWAYS AS (id * 2) STORED
        ) ENGINE=InnoDB",
        "CREATE VIEW orders_v AS SELECT id FROM orders",
        "CREATE FUNCTION add1(n INT) RETURNS INT DETERMINISTIC RETURN n + 1",
        "CREATE PROCEDURE noop() BEGIN SELECT 1; END",
        "CREATE TRIGGER orders_tg BEFORE INSERT ON orders FOR EACH ROW SET NEW.note = COALESCE(NEW.note, 'x')",
        "CREATE EVENT IF NOT EXISTS tick ON SCHEDULE EVERY 1 DAY DO SELECT 1",
        "CREATE ROLE IF NOT EXISTS dexo_reader",
        "GRANT SELECT ON dexo.orders TO dexo",
        "CREATE USER 'catalog_restricted'@'%' IDENTIFIED BY 'dexo_test_only'",
        "GRANT SELECT ON dexo.customers TO 'catalog_restricted'@'%'",
    ];
    for statement in statements {
        let stream = root
            .execute(dexo_driver_api::QueryRequest::write(statement))
            .await
            .unwrap_or_else(|error| panic!("seed failed for {statement}: {error}"));
        drain(stream).await;
    }
    let session = MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    Fixture { pair, session }
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

#[tokio::test]
#[ignore = "requires Docker"]
async fn mysql_catalog_contract() {
    let fixture = connect_seeded().await;
    let catalog = fixture.session.catalog().expect("catalog capability");
    let roots = catalog
        .list_children(None, &CatalogListOptions::default())
        .await
        .unwrap();
    assert!(has_kind_named(&roots.objects, ObjectKind::Catalog, "dexo"));
    let database = &roots.objects[0];
    assert!(database.attributes.contains_key("driver.mysql.charset"));

    let children = catalog
        .list_children(Some(&database.id), &CatalogListOptions::default())
        .await
        .unwrap();
    assert!(
        !children
            .objects
            .iter()
            .any(|object| object.qualified_name.object() == "mysql")
    );
    assert!(has_kind_named(
        &children.objects,
        ObjectKind::Table,
        "orders"
    ));
    assert!(has_kind_named(
        &children.objects,
        ObjectKind::View,
        "orders_v"
    ));
    assert!(has_kind_named(
        &children.objects,
        ObjectKind::Function,
        "add1"
    ));
    assert!(has_kind_named(
        &children.objects,
        ObjectKind::Procedure,
        "noop"
    ));
    let _ = children
        .objects
        .iter()
        .any(|object| matches!(&object.kind, ObjectKind::DriverSpecific(kind) if kind == "event"));
    if children.restrictions.is_empty() {
        assert!(
            children
                .objects
                .iter()
                .any(|object| object.kind == ObjectKind::User || object.kind == ObjectKind::Role)
        );
    } else {
        assert!(
            children
                .restrictions
                .iter()
                .any(|restriction| restriction.capability.starts_with("mysql."))
        );
    }

    let table = children
        .objects
        .iter()
        .find(|object| {
            object.kind == ObjectKind::Table && object.qualified_name.object() == "orders"
        })
        .unwrap();
    assert_eq!(
        table.attributes.get("driver.mysql.engine"),
        Some(&serde_json::json!("InnoDB"))
    );
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
    let generated = children
        .objects
        .iter()
        .find(|object| object.qualified_name.object() == "generated_demo")
        .unwrap();
    let generated_children = catalog
        .list_children(Some(&generated.id), &CatalogListOptions::default())
        .await
        .unwrap();
    let generated_col = generated_children
        .objects
        .iter()
        .find(|object| object.qualified_name.object().ends_with("total"))
        .unwrap();
    assert!(
        generated_col
            .attributes
            .contains_key("driver.mysql.generation_expression")
    );
    assert!(has_kind_named(
        &table_children.objects,
        ObjectKind::Trigger,
        "orders_tg"
    ));
    assert!(table_children.objects.iter().any(
        |object| matches!(&object.kind, ObjectKind::DriverSpecific(kind) if kind == "partition")
    ));

    let ddl = catalog.ddl(&table.id).await.unwrap();
    assert!(ddl.sql.to_ascii_uppercase().contains("CREATE TABLE"));

    let customers = children
        .objects
        .iter()
        .find(|object| object.qualified_name.object() == "customers")
        .expect("customers table");
    let items = children
        .objects
        .iter()
        .find(|object| object.qualified_name.object() == "order_items")
        .expect("order_items table");
    let view = children
        .objects
        .iter()
        .find(|object| object.qualified_name.object() == "orders_v")
        .expect("orders_v view");
    let deps = catalog.dependencies(&items.id).await.unwrap();
    assert!(
        deps.iter().any(|id| id == &customers.id),
        "order_items should depend on customers, got {deps:?}"
    );
    let dependents = catalog.dependents(&customers.id).await.unwrap();
    assert!(
        dependents.iter().any(|id| id == &items.id),
        "customers should have order_items as dependent, got {dependents:?}"
    );
    let order_dependents = catalog.dependents(&table.id).await.unwrap();
    assert!(
        order_dependents.iter().any(|id| id == &view.id),
        "orders should have orders_v as dependent, got {order_dependents:?}"
    );
    let view_deps = catalog.dependencies(&view.id).await.unwrap();
    assert!(
        view_deps.iter().any(|id| id == &table.id),
        "orders_v should depend on orders, got {view_deps:?}"
    );
    let trigger = table_children
        .objects
        .iter()
        .find(|object| object.kind == ObjectKind::Trigger)
        .expect("orders trigger");
    assert!(
        order_dependents.iter().any(|id| id == &trigger.id),
        "orders should have trigger as dependent, got {order_dependents:?}"
    );

    let restricted = MysqlFactory
        .connect(ConnectRequest::new(
            fixture.pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "catalog_restricted".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    let restricted_catalog = restricted.catalog().expect("catalog capability");
    let restricted_roots = restricted_catalog
        .list_children(None, &CatalogListOptions::default())
        .await
        .unwrap();
    let restricted_children = restricted_catalog
        .list_children(
            Some(&restricted_roots.objects[0].id),
            &CatalogListOptions::default(),
        )
        .await
        .unwrap();
    assert!(
        !restricted_children.restrictions.is_empty()
            || restricted_catalog.ddl(&table.id).await.is_err_and(
                |error| error.category() == dexo_driver_api::DriverErrorCategory::Permission
            ),
        "least-privilege user must get a restriction or permission error, not empty success"
    );
}
