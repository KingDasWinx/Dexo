use dexo_app::schema_diff::{
    OrderedChange, SchemaDifference, generate_script, infer_edges, order_changes,
};
use dexo_driver_api::{
    CatalogListOptions, CatalogObject, ConnectRequest, ConnectionFactory, DdlPlan, ObjectId,
    ObjectKind, QualifiedName, SchemaChange, Session,
};
use dexo_driver_mysql::{MysqlFactory, render_ddl};
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
        .connect(ConnectRequest {
            endpoint: pair.mysql_endpoint().to_string(),
            database: Some("dexo".into()),
            username: "root".into(),
            secret: SecretString::from("dexo_test_only"),
            read_only: false,
        })
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
        .join("\n");
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
