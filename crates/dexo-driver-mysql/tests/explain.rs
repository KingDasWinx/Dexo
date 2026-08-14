use dexo_driver_mysql::{parse_explain_json, parse_explain_tree};

#[test]
fn json_and_tree_goldens_and_unavailable_metrics() {
    let scan = parse_explain_json(include_str!("fixtures/explain/scan.json")).unwrap();
    assert_eq!(scan.root.kind, "Table scan");
    assert_eq!(scan.root.relation.as_deref(), Some("items"));
    assert_eq!(scan.root.estimates.rows, Some(1000.0));
    assert!(scan.root.actual.rows.is_none(), "json actual must not be zeroed");
    assert!(scan.root.loops.is_none());

    let join = parse_explain_json(include_str!("fixtures/explain/join.json")).unwrap();
    assert_eq!(join.root.kind, "Nested loop");
    assert_eq!(join.root.children.len(), 2);
    assert_eq!(join.root.children[0].relation.as_deref(), Some("orders"));
    assert_eq!(join.root.children[1].kind, "Index lookup");

    let tree = parse_explain_tree(include_str!("fixtures/explain/tree.txt")).unwrap();
    assert_eq!(tree.root.kind, "Sort: items.name");
    assert_eq!(tree.root.children[0].kind, "Table scan");
    assert_eq!(tree.root.children[0].relation.as_deref(), Some("items"));
    assert!(tree.root.actual.time_ms.is_none());

    let analyzed = parse_explain_tree(include_str!("fixtures/explain/tree_analyze.txt")).unwrap();
    assert_eq!(analyzed.root.kind, "Aggregate: count(0)");
    assert_eq!(analyzed.root.actual.rows, Some(10.0));
    assert_eq!(analyzed.root.loops, Some(1));
    assert!(analyzed.root.actual.time_ms.is_some());
    assert!(analyzed.root.children[0].actual.rows.is_some());
}

#[test]
fn capability_fallback_prefers_json_then_tree() {
    use dexo_driver_mysql::{MysqlExplainCaps, NativeExplainFormat, select_format};
    let full = MysqlExplainCaps {
        json: true,
        tree: true,
        tree_analyze: true,
    };
    assert_eq!(select_format(false, full).unwrap(), NativeExplainFormat::Json);
    assert_eq!(select_format(true, full).unwrap(), NativeExplainFormat::Tree);
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
        .connect(ConnectRequest {
            endpoint: pair.mysql_endpoint().to_string(),
            database: Some("dexo".into()),
            username: "dexo".into(),
            secret: SecretString::from("dexo_test_only"),
            read_only: false,
        })
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
    assert!(analyzed.raw.contains("->") || analyzed.root.loops.is_some() || !analyzed.root.kind.is_empty());
}
