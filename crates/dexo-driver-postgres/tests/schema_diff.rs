use dexo_app::schema_diff::{
    OrderedChange, SchemaDifference, generate_script, infer_edges, order_changes,
};
use dexo_driver_api::{
    CatalogListOptions, CatalogObject, ConnectRequest, ConnectionFactory, DdlPlan, ObjectId,
    ObjectKind, QualifiedName, SchemaChange, Session,
};
use dexo_driver_postgres::{PostgresFactory, render_ddl};
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
