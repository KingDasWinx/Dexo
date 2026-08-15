use dexo_app::schema_diff::{
    OrderedChange, SchemaDifference, generate_script, infer_edges, order_changes,
};
use dexo_driver_api::{
    AlterOp, CatalogListOptions, CatalogObject, ColumnSpec, ConnectRequest, ConnectionFactory,
    DdlPlan, ObjectId, ObjectKind, PrivilegeDef, QualifiedName, SchemaChange, Session, TableDef,
    TableShape,
};
use dexo_driver_mysql::{MysqlFactory, plan_ddl, render_ddl};
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

fn mysql_render(change: &SchemaChange) -> Result<DdlPlan, String> {
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
    QualifiedName::new(Some(schema), None::<String>, object)
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
                auto_increment: true,
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
    assert!(plan.statements[0].sql.contains("`Sales`.`Order`"));
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
            .contains("DROP TABLE `Sales`.`Order`")
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
            .contains("ALTER TABLE `Sales`.`Order`")
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

    let domain = SchemaChange::CreateTable {
        target: q("Sales", "posint"),
        def: TableDef {
            shape: TableShape::Domain {
                base_type: "int".into(),
                check: None,
            },
            columns: vec![],
            constraints: vec![],
            partition: None,
            engine: None,
            charset: None,
            collation: None,
        },
    };
    let err = ddl().plan_change(&domain).unwrap_err();
    assert!(err.to_string().contains("mysql"));
    assert!(err.to_string().contains("CreateTable"));
}

#[test]
fn mysql_script_golden_drop_and_create() {
    let drop = OrderedChange {
        difference: SchemaDifference::Removed(table("dexo", "gone")),
        manual: false,
    };
    let script = generate_script(&[drop], mysql_render);
    assert!(script.forward.contains("DROP TABLE `dexo`.`gone`"));
    assert!(script.forward.contains("destructive=true"));
    assert!(script.reverse.is_none());

    let added = OrderedChange {
        difference: SchemaDifference::Added(table("dexo", "orders")),
        manual: false,
    };
    let created = generate_script(&[added], mysql_render);
    assert!(created.forward.contains("CREATE TABLE `dexo`.`orders`"));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mysql_forward_script_reaches_empty_diff() {
    let pair = DatabasePair::start().await.unwrap();
    let session = MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "root".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    let from = list_tables(session.as_ref(), "dexo").await;
    let changes = vec![SchemaDifference::Added(table("dexo", "mig_added"))];
    let edges = infer_edges(&changes);
    let ordered = order_changes(changes, &edges);
    let script = generate_script(&ordered, mysql_render);
    apply_sql(session.as_ref(), &script.forward).await;
    let after = list_tables(session.as_ref(), "dexo").await;
    assert!(
        after
            .iter()
            .any(|object| object.qualified_name.object() == "mig_added")
    );
    let reverse = script.reverse.expect("added table is reversible");
    apply_sql(session.as_ref(), &reverse).await;
    let back = list_tables(session.as_ref(), "dexo").await;
    assert_eq!(names(&back), names(&from));
}

async fn list_tables(session: &dyn Session, schema: &str) -> Vec<CatalogObject> {
    let catalog = session.catalog().unwrap();
    let roots = catalog
        .list_children(None, &CatalogListOptions::default())
        .await
        .unwrap();
    let db = roots
        .objects
        .iter()
        .find(|object| object.qualified_name.object() == schema)
        .or_else(|| roots.objects.first())
        .unwrap();
    catalog
        .list_children(Some(&db.id), &CatalogListOptions::default())
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

async fn apply_sql(session: &dyn Session, sql: &str) {
    let mut plan = DdlPlan::default();
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
        plan.push(statement, true);
    }
    if plan.statements.is_empty() {
        return;
    }
    session.ddl().unwrap().apply_ddl(&plan).await.unwrap();
}
