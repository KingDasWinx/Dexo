use dexo_driver_api::{
    CatalogListOptions, ColumnSpec, ConnectRequest, ConnectionFactory, DdlOutcome, GeneratedSpec,
    IndexDef, ObjectKind, PrivilegeDef, QualifiedName, RoutineDef, RoutineKind, SchemaChange,
    Session, TableDef, TableShape,
};
use dexo_driver_mysql::{MysqlFactory, render_ddl};
use dexo_test_support::DatabasePair;
use secrecy::SecretString;

struct Fixture {
    _pair: DatabasePair,
    endpoint: String,
    session: Box<dyn Session>,
}

async fn connect_root() -> Fixture {
    let pair = DatabasePair::start().await.unwrap();
    let endpoint = pair.mysql_endpoint().to_string();
    let session = MysqlFactory
        .connect(ConnectRequest::new(
            endpoint.clone(),
            Some("dexo".into()),
            "root".into(),
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
    QualifiedName::new(Some(schema), None::<String>, object)
}

async fn apply(session: &dyn Session, change: SchemaChange) -> DdlOutcome {
    let plan = render_ddl(&change).unwrap();
    session
        .ddl()
        .unwrap()
        .apply_ddl(&plan)
        .await
        .unwrap_or_else(|error| panic!("ddl failed for {:?}: {error}", plan.statements))
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mysql_ddl_round_trip_matches_introspected_shape() {
    let fixture = connect_root().await;
    let trust = dexo_driver_api::DdlPlan {
        statements: vec![dexo_driver_api::DdlStatement {
            sql: "SET GLOBAL log_bin_trust_function_creators = 1".into(),
            implicit_commit: false,
        }],
        rollback: vec![],
        warnings: vec![],
        transactional: false,
    };
    fixture
        .session
        .ddl()
        .unwrap()
        .apply_ddl(&trust)
        .await
        .unwrap();

    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::CreateTable {
                target: q("dexo", "ddl_orders"),
                def: TableDef {
                    shape: TableShape::Table,
                    columns: vec![
                        ColumnSpec {
                            name: ident("id"),
                            data_type: "bigint".into(),
                            nullable: false,
                            default_sql: None,
                            identity: None,
                            auto_increment: true,
                            generated: None,
                            primary_key: true,
                        },
                        ColumnSpec {
                            name: ident("base"),
                            data_type: "int".into(),
                            nullable: false,
                            default_sql: Some("0".into()),
                            identity: None,
                            auto_increment: false,
                            generated: None,
                            primary_key: false,
                        },
                        ColumnSpec {
                            name: ident("label"),
                            data_type: "int".into(),
                            nullable: true,
                            default_sql: None,
                            identity: None,
                            auto_increment: false,
                            generated: Some(GeneratedSpec {
                                expression: "`base` * 2".into(),
                                stored: true,
                            }),
                            primary_key: false,
                        },
                    ],
                    constraints: vec![],
                    partition: None,
                    engine: Some("InnoDB".into()),
                    charset: Some("utf8mb4".into()),
                    collation: Some("utf8mb4_0900_ai_ci".into()),
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
                target: ident("ddl_orders_label_idx"),
                def: IndexDef {
                    table: q("dexo", "ddl_orders"),
                    columns: vec![ident("base")],
                    unique: false,
                    concurrently: false,
                    method: None,
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
                target: ident("ddl_add1"),
                def: RoutineDef {
                    kind: RoutineKind::Function,
                    arguments: "n INT".into(),
                    language: "sql".into(),
                    body: "RETURN n + 1".into(),
                    returns: Some("INT".into()),
                    volatility: None,
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
                target: ident("ddl_reader"),
                def: PrivilegeDef {
                    principal: ident("ddl_reader"),
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
                target: q("dexo", "ddl_orders"),
                def: PrivilegeDef {
                    principal: ident("ddl_reader"),
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
    assert!(children.objects.iter().any(|object| {
        object.kind == ObjectKind::Table && object.qualified_name.object() == "ddl_orders"
    }));
    assert!(children.objects.iter().any(|object| {
        object.kind == ObjectKind::Function && object.qualified_name.object() == "ddl_add1"
    }));
    let table = children
        .objects
        .iter()
        .find(|object| object.qualified_name.object() == "ddl_orders")
        .unwrap();
    let table_children = catalog
        .list_children(Some(&table.id), &CatalogListOptions::default())
        .await
        .unwrap();
    assert!(table_children.objects.iter().any(|object| {
        object.kind == ObjectKind::Column && object.qualified_name.object().ends_with("base")
    }));
    let grants = fixture
        .session
        .security()
        .unwrap()
        .list_grants(Some(&ident("ddl_reader")))
        .await
        .unwrap();
    assert!(
        grants
            .iter()
            .any(|grant| grant.privileges.iter().any(|item| item == "SELECT"))
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mysql_least_privilege_grant_revoke() {
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

    let fixture = connect_root().await;
    drain(
        fixture.session.as_ref(),
        "CREATE TABLE dexo.lp_allowed (id int PRIMARY KEY)",
    )
    .await
    .unwrap();
    drain(
        fixture.session.as_ref(),
        "CREATE TABLE dexo.lp_denied (id int PRIMARY KEY)",
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
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::Grant {
                target: q("dexo", "lp_allowed"),
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
    let limited = MysqlFactory
        .connect(ConnectRequest::new(
            fixture.endpoint.clone(),
            Some("dexo".into()),
            "dexo_lp_role".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    drain(limited.as_ref(), "SELECT * FROM dexo.lp_allowed")
        .await
        .unwrap();
    assert!(
        drain(limited.as_ref(), "SELECT * FROM dexo.lp_denied")
            .await
            .is_err()
    );
    assert_eq!(
        apply(
            fixture.session.as_ref(),
            SchemaChange::Revoke {
                target: q("dexo", "lp_allowed"),
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
        drain(limited.as_ref(), "SELECT * FROM dexo.lp_allowed")
            .await
            .is_err()
    );
}
