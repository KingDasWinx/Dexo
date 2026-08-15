use dexo_driver_api::{
    CatalogListOptions, ColumnSpec, ConnectRequest, ConnectionFactory, DdlOutcome, GeneratedSpec,
    IdentitySpec, IndexDef, ObjectKind, PartitionSpec, PrivilegeDef, QualifiedName, RoutineDef,
    RoutineKind, SchemaChange, Session, TableDef, TableShape, ViewDef,
};
use dexo_driver_postgres::{PostgresFactory, render_ddl};
use dexo_test_support::DatabasePair;
use secrecy::SecretString;

struct Fixture {
    _pair: DatabasePair,
    endpoint: String,
    session: Box<dyn Session>,
}

async fn connect() -> Fixture {
    let pair = DatabasePair::start().await.unwrap();
    let endpoint = pair.postgres_endpoint().to_string();
    let session = PostgresFactory
        .connect(ConnectRequest::new(
            endpoint.clone(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    Fixture {
        _pair: pair,
        endpoint,
        session,
    }
}

fn ident(name: &str) -> QualifiedName {
    QualifiedName::new(None::<String>, None::<String>, name)
}

fn q(schema: &str, object: &str) -> QualifiedName {
    QualifiedName::new(None::<String>, Some(schema), object)
}

fn col(name: &str, data_type: &str) -> ColumnSpec {
    ColumnSpec {
        name: ident(name),
        data_type: data_type.into(),
        nullable: false,
        default_sql: None,
        identity: None,
        auto_increment: false,
        generated: None,
        primary_key: false,
    }
}

async fn apply(session: &dyn Session, change: SchemaChange) -> DdlOutcome {
    let plan = render_ddl(&change).unwrap();
    session.ddl().unwrap().apply_ddl(&plan).await.unwrap()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_ddl_round_trip_matches_introspected_shape() {
    let fixture = connect().await;
    let ddl = fixture.session.ddl().unwrap();
    let schema_sql = "CREATE SCHEMA dexo_ddl";
    assert_eq!(
        ddl.apply_ddl(&dexo_driver_api::DdlPlan {
            statements: vec![dexo_driver_api::DdlStatement {
                sql: schema_sql.into(),
                implicit_commit: false,
            }],
            rollback: vec![],
            warnings: vec![],
            transactional: true,
        })
        .await
        .unwrap(),
        DdlOutcome::Committed
    );

    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::CreateTable {
                target: q("dexo_ddl", "mood"),
                def: TableDef {
                    shape: TableShape::Enum {
                        labels: vec!["sad".into(), "ok".into(), "happy".into()],
                    },
                    columns: vec![],
                    constraints: vec![],
                    partition: None,
                    engine: None,
                    charset: None,
                    collation: None,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::CreateTable {
                target: q("dexo_ddl", "posint"),
                def: TableDef {
                    shape: TableShape::Domain {
                        base_type: "integer".into(),
                        check: Some("VALUE > 0".into()),
                    },
                    columns: vec![],
                    constraints: vec![],
                    partition: None,
                    engine: None,
                    charset: None,
                    collation: None,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );

    let mut id = col("id", "bigint");
    id.identity = Some(IdentitySpec { always: true });
    id.primary_key = true;
    let mut status = col("status", "dexo_ddl.mood");
    status.nullable = true;
    status.primary_key = false;
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::CreateTable {
                target: q("dexo_ddl", "orders"),
                def: TableDef {
                    shape: TableShape::Table,
                    columns: vec![id, status],
                    constraints: vec![],
                    partition: Some(PartitionSpec {
                        method: "range".into(),
                        columns: vec![ident("id")],
                    }),
                    engine: None,
                    charset: None,
                    collation: None,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );

    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::CreateView {
                target: q("dexo_ddl", "orders_mv"),
                def: ViewDef {
                    sql: "SELECT id FROM dexo_ddl.orders".into(),
                    materialized: true,
                    replace: false,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::CreateIndex {
                target: ident("orders_status_idx"),
                def: IndexDef {
                    table: q("dexo_ddl", "orders"),
                    columns: vec![ident("status")],
                    unique: false,
                    concurrently: false,
                    method: Some("btree".into()),
                    include: vec![],
                    predicate: None,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::AlterRoutine {
                target: q("dexo_ddl", "add1"),
                def: RoutineDef {
                    kind: RoutineKind::Function,
                    arguments: "n integer".into(),
                    language: "sql".into(),
                    body: "SELECT n + 1".into(),
                    returns: Some("integer".into()),
                    volatility: Some("IMMUTABLE".into()),
                    table: None,
                    timing: None,
                    schedule: None,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::Grant {
                target: ident("dexo_ddl_reporter"),
                def: PrivilegeDef {
                    principal: ident("dexo_ddl_reporter"),
                    privileges: vec![],
                    with_grant_option: false,
                    role_membership: false,
                    create_principal: true,
                    login: false,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::Grant {
                target: q("dexo_ddl", "orders"),
                def: PrivilegeDef {
                    principal: ident("dexo_ddl_reporter"),
                    privileges: vec!["SELECT".into()],
                    with_grant_option: false,
                    role_membership: false,
                    create_principal: false,
                    login: false,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );

    let catalog = fixture.session.catalog().unwrap();
    let roots = catalog
        .list_children(None, &CatalogListOptions::default())
        .await
        .unwrap();
    let children = catalog
        .list_children(Some(&roots.objects[0].id), &CatalogListOptions::default())
        .await
        .unwrap();
    let schema = children
        .objects
        .iter()
        .find(|object| object.qualified_name.object() == "dexo_ddl")
        .unwrap();
    let listed = catalog
        .list_children(Some(&schema.id), &CatalogListOptions::default())
        .await
        .unwrap();
    let objects = &listed.objects;
    assert!(objects.iter().any(|object| {
        object.kind == ObjectKind::Table && object.qualified_name.object() == "orders"
    }));
    assert!(objects.iter().any(|object| {
        object.kind == ObjectKind::MaterializedView && object.qualified_name.object() == "orders_mv"
    }));
    assert!(objects.iter().any(|object| {
        object.kind == ObjectKind::Function && object.qualified_name.object() == "add1"
    }));
    assert!(objects.iter().any(|object| {
        matches!(&object.kind, ObjectKind::DriverSpecific(kind) if kind == "enum")
            && object.qualified_name.object() == "mood"
    }));
    assert!(objects.iter().any(|object| {
        matches!(&object.kind, ObjectKind::DriverSpecific(kind) if kind == "domain")
            && object.qualified_name.object() == "posint"
    }));
    let table = objects
        .iter()
        .find(|object| object.kind == ObjectKind::Table)
        .unwrap();
    assert!(
        table
            .attributes
            .contains_key("driver.postgres.partition_key")
    );
    let table_children = catalog
        .list_children(Some(&table.id), &CatalogListOptions::default())
        .await
        .unwrap();
    assert!(
        table_children
            .objects
            .iter()
            .any(|object| object.kind == ObjectKind::Index)
    );
    let grants = fixture
        .session
        .security()
        .unwrap()
        .list_grants(Some(&ident("dexo_ddl_reporter")))
        .await
        .unwrap();
    assert!(
        grants
            .iter()
            .any(|grant| grant.privileges.iter().any(|privs| privs == "SELECT"))
    );
}

#[test]
fn generated_column_sql_is_typed() {
    let mut full_name = col("full_name", "text");
    full_name.generated = Some(GeneratedSpec {
        expression: "id::text".into(),
        stored: true,
    });
    full_name.nullable = true;
    let sql = render_ddl(&SchemaChange::CreateTable {
        target: q("public", "t"),
        def: TableDef {
            shape: TableShape::Table,
            columns: vec![full_name],
            constraints: vec![],
            partition: None,
            engine: None,
            charset: None,
            collation: None,
        },
    })
    .unwrap()
    .statements[0]
        .sql
        .clone();
    assert!(sql.contains("GENERATED ALWAYS AS (id::text) STORED"));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_least_privilege_grant_revoke() {
    use dexo_driver_api::QueryRequest;
    use futures_util::StreamExt;

    async fn drain(session: &dyn Session, sql: &str) -> Result<(), String> {
        let mut stream = session
            .execute(QueryRequest::write(sql))
            .await
            .map_err(|error| error.to_string())?;
        while let Some(event) = stream.next().await {
            event.map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    let fixture = connect().await;
    drain(fixture.session.as_ref(), "CREATE SCHEMA dexo_lp")
        .await
        .unwrap();
    drain(
        fixture.session.as_ref(),
        "CREATE TABLE dexo_lp.allowed (id int PRIMARY KEY)",
    )
    .await
    .unwrap();
    drain(
        fixture.session.as_ref(),
        "CREATE TABLE dexo_lp.denied (id int PRIMARY KEY)",
    )
    .await
    .unwrap();
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::Grant {
                target: ident("dexo_lp_role"),
                def: PrivilegeDef {
                    principal: ident("dexo_lp_role"),
                    privileges: vec![],
                    with_grant_option: false,
                    role_membership: false,
                    create_principal: true,
                    login: true,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );
    fixture
        .session
        .security()
        .unwrap()
        .set_password(
            &ident("dexo_lp_role"),
            &SecretString::from("dexo_test_only"),
        )
        .await
        .unwrap();
    drain(
        fixture.session.as_ref(),
        "GRANT USAGE ON SCHEMA dexo_lp TO dexo_lp_role",
    )
    .await
    .unwrap();
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::Grant {
                target: q("dexo_lp", "allowed"),
                def: PrivilegeDef {
                    principal: ident("dexo_lp_role"),
                    privileges: vec!["SELECT".into()],
                    with_grant_option: false,
                    role_membership: false,
                    create_principal: false,
                    login: false,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );
    let limited = PostgresFactory
        .connect(ConnectRequest::new(
            fixture.endpoint.clone(),
            Some("dexo".into()),
            "dexo_lp_role".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    drain(limited.as_ref(), "SELECT * FROM dexo_lp.allowed")
        .await
        .unwrap();
    assert!(
        drain(limited.as_ref(), "SELECT * FROM dexo_lp.denied")
            .await
            .is_err()
    );
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::Revoke {
                target: q("dexo_lp", "allowed"),
                def: PrivilegeDef {
                    principal: ident("dexo_lp_role"),
                    privileges: vec!["SELECT".into()],
                    with_grant_option: false,
                    role_membership: false,
                    create_principal: false,
                    login: false,
                },
            }
        )
        .await,
        DdlOutcome::Committed
    );
    assert!(
        drain(limited.as_ref(), "SELECT * FROM dexo_lp.allowed")
            .await
            .is_err()
    );
}
