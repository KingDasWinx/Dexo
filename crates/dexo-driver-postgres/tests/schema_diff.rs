use dexo_app::schema_diff::{
    OrderedChange, SchemaDifference, generate_script, infer_edges, order_changes,
};
use dexo_driver_api::{
    AlterOp, CatalogListOptions, CatalogObject, ColumnSpec, ConnectRequest, ConnectionFactory,
    DdlPlan, IndexDef, ObjectId, ObjectKind, PrivilegeDef, QualifiedName, SchemaChange, Session,
    TableDef, TableShape, ViewDef,
};
use dexo_driver_postgres::{PostgresFactory, plan_ddl, render_ddl};
use dexo_test_support::DatabasePair;
use secrecy::SecretString;

fn table(schema: &str, name: &str) -> CatalogObject {
    CatalogObject::new(
        ObjectId::new(name),
        ObjectKind::Table,
        QualifiedName::new(None::<String>, Some(schema), name),
        None,
    )
}

fn pg_render(change: &SchemaChange) -> Result<DdlPlan, String> {
    render_ddl(change).map_err(|error| error.to_string())
}

struct Planner;

impl Planner {
    fn plan_change(&self, change: &SchemaChange) -> Result<DdlPlan, dexo_driver_api::DriverError> {
        plan_ddl(change)
    }
}

fn ddl() -> Planner {
    Planner
}

fn ident(name: &str) -> QualifiedName {
    QualifiedName::new(None::<String>, None::<String>, name)
}

fn q(schema: &str, object: &str) -> QualifiedName {
    QualifiedName::new(None::<String>, Some(schema), object)
}

fn create_table(schema: &str, name: &str) -> SchemaChange {
    SchemaChange::CreateTable {
        target: q(schema, name),
        def: TableDef {
            shape: TableShape::Table,
            columns: vec![ColumnSpec {
                name: ident("id"),
                data_type: "int".into(),
                nullable: false,
                default_sql: None,
                identity: None,
                auto_increment: false,
                generated: None,
                primary_key: true,
            }],
            constraints: vec![],
            partition: None,
            engine: None,
            charset: None,
            collation: None,
        },
    }
}

#[test]
fn plan_change_uses_driver_quoting_and_reports_risk() {
    let plan = ddl().plan_change(&create_table("Sales", "Order")).unwrap();
    assert!(plan.statements[0].sql.contains("\"Sales\".\"Order\""));
    assert!(!plan.statements[0].sql.contains("Sales.Order"));
    assert!(!plan.risk.destructive);
}

#[test]
fn plan_change_covers_drop_alter_grant_and_unsupported() {
    let drop = ddl()
        .plan_change(&SchemaChange::DropObject {
            target: q("Sales", "Order"),
            kind: ObjectKind::Table,
        })
        .unwrap();
    assert!(
        drop.statements[0]
            .sql
            .contains("DROP TABLE \"Sales\".\"Order\"")
    );
    assert!(drop.risk.destructive);

    let alter = ddl()
        .plan_change(&SchemaChange::AlterTable {
            target: q("Sales", "Order"),
            ops: vec![AlterOp::AddColumn(ColumnSpec {
                name: ident("qty"),
                data_type: "int".into(),
                nullable: true,
                default_sql: None,
                identity: None,
                auto_increment: false,
                generated: None,
                primary_key: false,
            })],
        })
        .unwrap();
    assert!(
        alter.statements[0]
            .sql
            .contains("ALTER TABLE \"Sales\".\"Order\"")
    );

    let grant = ddl()
        .plan_change(&SchemaChange::Grant {
            target: q("Sales", "Order"),
            def: PrivilegeDef {
                principal: ident("reporter"),
                privileges: vec!["SELECT".into()],
                with_grant_option: false,
                role_membership: false,
                create_principal: false,
                login: false,
            },
        })
        .unwrap();
    assert!(grant.statements[0].sql.contains("GRANT SELECT"));

    let view = ddl()
        .plan_change(&SchemaChange::CreateView {
            target: q("Sales", "v_order"),
            def: ViewDef {
                sql: "SELECT 1".into(),
                materialized: false,
                replace: false,
            },
        })
        .unwrap();
    assert!(view.statements[0].sql.contains("CREATE"));

    let index = ddl()
        .plan_change(&SchemaChange::CreateIndex {
            target: ident("order_idx"),
            def: IndexDef {
                table: q("Sales", "Order"),
                columns: vec![ident("id")],
                unique: false,
                concurrently: false,
                method: None,
                include: vec![],
                predicate: None,
            },
        })
        .unwrap();
    assert!(index.statements[0].sql.contains("CREATE"));

    let event = SchemaChange::AlterRoutine {
        target: ident("tick"),
        def: dexo_driver_api::RoutineDef {
            kind: dexo_driver_api::RoutineKind::Event,
            arguments: String::new(),
            language: "sql".into(),
            body: "SELECT 1".into(),
            returns: None,
            volatility: None,
            table: None,
            timing: None,
            schedule: None,
        },
    };
    let err = ddl().plan_change(&event).unwrap_err();
    assert!(err.to_string().contains("postgres"));
    assert!(err.to_string().contains("AlterRoutine"));
}

#[test]
fn postgres_script_golden_drop_and_create() {
    let drop = OrderedChange {
        difference: SchemaDifference::Removed(table("public", "gone")),
        manual: false,
    };
    let script = generate_script(&[drop], pg_render);
    assert!(script.forward.contains("DROP TABLE \"public\".\"gone\""));
    assert!(script.forward.contains("destructive=true"));
    assert!(script.forward.contains("lock=AccessExclusive"));
    assert!(script.reverse.is_none());

    let added = OrderedChange {
        difference: SchemaDifference::Added(table("public", "orders")),
        manual: false,
    };
    let created = generate_script(&[added], pg_render);
    assert!(
        created
            .forward
            .contains("CREATE TABLE \"public\".\"orders\"")
    );
    assert!(created.reverse.is_some());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_forward_script_reaches_empty_diff() {
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
    apply_sql(session.as_ref(), "CREATE SCHEMA dexo_mig", true).await;
    let from = list_tables(session.as_ref(), "dexo_mig").await;
    let to = vec![table("dexo_mig", "added")];
    let changes = vec![SchemaDifference::Added(table("dexo_mig", "added"))];
    let edges = infer_edges(&changes);
    let ordered = order_changes(changes, &edges);
    let script = generate_script(&ordered, pg_render);
    apply_sql(session.as_ref(), &script.forward, true).await;
    let after = list_tables(session.as_ref(), "dexo_mig").await;
    assert!(
        after
            .iter()
            .any(|object| object.qualified_name.object() == "added")
    );
    let leftover: Vec<_> = after
        .iter()
        .filter(|object| {
            !from
                .iter()
                .any(|before| before.qualified_name.object() == object.qualified_name.object())
                && object.qualified_name.object() != "added"
        })
        .collect();
    assert!(leftover.is_empty(), "forward apply left unexpected tables");
    let reverse = script.reverse.expect("added table is reversible");
    apply_sql(session.as_ref(), &reverse, true).await;
    let back = list_tables(session.as_ref(), "dexo_mig").await;
    assert_eq!(names(&back), names(&from));
    let _ = to;
}

async fn list_tables(session: &dyn Session, schema: &str) -> Vec<CatalogObject> {
    let catalog = session.catalog().unwrap();
    let roots = catalog
        .list_children(None, &CatalogListOptions::default())
        .await
        .unwrap();
    let children = catalog
        .list_children(Some(&roots.objects[0].id), &CatalogListOptions::default())
        .await
        .unwrap();
    let schema_obj = children
        .objects
        .iter()
        .find(|object| object.qualified_name.object() == schema)
        .unwrap();
    catalog
        .list_children(Some(&schema_obj.id), &CatalogListOptions::default())
        .await
        .unwrap()
        .objects
        .into_iter()
        .filter(|object| object.kind == ObjectKind::Table)
        .collect()
}

fn names(objects: &[CatalogObject]) -> Vec<String> {
    let mut names: Vec<_> = objects
        .iter()
        .map(|object| object.qualified_name.object().to_string())
        .collect();
    names.sort();
    names
}

async fn apply_sql(session: &dyn Session, sql: &str, transactional: bool) {
    let mut plan = DdlPlan {
        transactional,
        ..DdlPlan::default()
    };
    let body = sql
        .lines()
        .filter(|line| !line.trim().starts_with("--"))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    for statement in body
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        plan.push(statement, false);
    }
    if plan.statements.is_empty() {
        return;
    }
    session.ddl().unwrap().apply_ddl(&plan).await.unwrap();
}
