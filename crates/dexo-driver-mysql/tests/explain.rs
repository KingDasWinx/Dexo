use dexo_driver_mysql::{parse_explain_json, parse_explain_tree};

#[test]
fn json_and_tree_goldens_and_unavailable_metrics() {
    let scan = parse_explain_json(include_str!("fixtures/explain/scan.json")).unwrap();
    assert_eq!(scan.root.kind, "Table scan");
    assert_eq!(scan.root.relation.as_deref(), Some("items"));
    assert_eq!(scan.root.estimates.rows, Some(1000.0));
    assert!(
        scan.root.actual.rows.is_none(),
        "json actual must not be zeroed"
    );
    assert!(scan.root.loops.is_none());

    let join = parse_explain_json(include_str!("fixtures/explain/join.json")).unwrap();
    assert_eq!(join.root.kind, "Nested loop");
    assert_eq!(join.root.children.len(), 2);
    assert_eq!(join.root.children[0].relation.as_deref(), Some("orders"));
    assert_eq!(join.root.children[1].kind, "Index lookup");

    let tree = parse_explain_tree(include_str!("fixtures/explain/tree.txt")).unwrap();
    assert_eq!(tree.root.kind, "Sort");
    assert_eq!(tree.root.detail.as_deref(), Some("items.name"));
    assert_eq!(tree.root.children[0].kind, "Table scan");
    assert_eq!(tree.root.children[0].relation.as_deref(), Some("items"));
    assert!(tree.root.actual.time_ms.is_none());

    let analyzed = parse_explain_tree(include_str!("fixtures/explain/tree_analyze.txt")).unwrap();
    assert_eq!(analyzed.root.kind, "Aggregate");
    assert_eq!(analyzed.root.detail.as_deref(), Some("count(0)"));
    assert_eq!(analyzed.root.actual.rows, Some(10.0));
    assert_eq!(analyzed.root.loops, Some(1));
    assert!(analyzed.root.actual.time_ms.is_some());
    assert!(analyzed.root.children[0].actual.rows.is_some());
}

#[test]
fn capability_fallback_prefers_json_then_tree() {
    use dexo_driver_mysql::{MysqlExplainCaps, NativeExplainFormat, select_format};
    let full = MysqlExplainCaps::mysql();
    assert_eq!(
        select_format(false, full).unwrap(),
        NativeExplainFormat::Json
    );
    assert_eq!(
        select_format(true, full).unwrap(),
        NativeExplainFormat::Tree
    );
    // MariaDB has no FORMAT=TREE; its EXPLAIN ANALYZE is ANALYZE FORMAT=JSON.
    let mariadb = MysqlExplainCaps::mariadb();
    assert_eq!(
        select_format(true, mariadb).unwrap(),
        NativeExplainFormat::Json
    );
    assert!(
        dexo_driver_mysql::wrap_explain("select 1", NativeExplainFormat::Json, true)
            .starts_with("ANALYZE FORMAT=JSON")
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn container_explain_json_and_analyze_tree() {
    use dexo_driver_api::{ConnectRequest, ConnectionFactory, ExplainRequest};
    use dexo_driver_mysql::MysqlFactory;
    use dexo_test_support::DatabasePair;
    use secrecy::SecretString;

    let pair = DatabasePair::start().await.unwrap();
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
    let estimated = session
        .explain()
        .unwrap()
        .explain(ExplainRequest::estimated("select 1"))
        .await
        .unwrap();
    assert!(!estimated.raw.is_empty());
    let analyzed = session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("select 1"))
        .await
        .unwrap();
    assert!(
        analyzed.raw.contains("->")
            || analyzed.root.loops.is_some()
            || !analyzed.root.kind.is_empty()
    );
}

/// Every table read shows in the plan, the ones a subquery, a derived table or a CTE
/// reads too.
#[tokio::test]
#[ignore = "requires Docker"]
async fn subqueries_derived_tables_and_ctes_keep_their_tables() {
    use dexo_driver_api::{ConnectRequest, ConnectionFactory, ExplainRequest, PlanNode};
    use dexo_driver_mysql::MysqlFactory;
    use dexo_test_support::DatabasePair;
    use futures_util::StreamExt;
    use secrecy::SecretString;

    fn relations(node: &PlanNode, out: &mut Vec<String>) {
        out.extend(node.relation.clone());
        for child in &node.children {
            relations(child, out);
        }
    }
    let pair = DatabasePair::start().await.unwrap();
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
    for sql in [
        "create table c (id int primary key, name varchar(20))",
        "create table o (id int primary key, customer_id int, total int, key (customer_id))",
        "insert into c values (1, 'a'), (2, 'b'), (3, 'c')",
        "insert into o values (1, 1, 10), (2, 1, 20), (3, 2, 30), (4, 3, 40)",
        "analyze table c, o",
    ] {
        let mut stream = session
            .execute(dexo_driver_api::QueryRequest::write(sql))
            .await
            .unwrap();
        while let Some(event) = stream.next().await {
            event.unwrap();
        }
    }
    for sql in [
        "select c.id, (select count(*) from o where o.customer_id = c.id) n from c",
        "select * from (select customer_id, sum(total) s from o group by customer_id) t \
         join c on c.id = t.customer_id",
        "with t as (select customer_id, sum(total) s from o group by customer_id) \
         select * from t join c on c.id = t.customer_id",
    ] {
        let plan = session
            .explain()
            .unwrap()
            .explain(ExplainRequest::estimated(sql))
            .await
            .unwrap();
        let mut found = Vec::new();
        relations(&plan.root, &mut found);
        assert!(
            found.iter().any(|name| name == "o") && found.iter().any(|name| name == "c"),
            "{sql}: {found:?}"
        );
    }
}
