use dexo_driver_postgres::parse_explain_json;

fn parse_fixture(name: &str) -> dexo_driver_api::ExplainPlan {
    let raw = match name {
        "scan" => include_str!("fixtures/explain/scan.json"),
        "join" => include_str!("fixtures/explain/join.json"),
        "sort" => include_str!("fixtures/explain/sort.json"),
        "aggregate" => include_str!("fixtures/explain/aggregate.json"),
        "parallel" => include_str!("fixtures/explain/parallel.json"),
        _ => panic!("unknown fixture {name}"),
    };
    parse_explain_json(raw).unwrap()
}

#[test]
fn goldens_cover_scan_join_sort_aggregate_parallel() {
    let scan = parse_fixture("scan");
    assert_eq!(scan.root.kind, "Seq Scan");
    assert_eq!(scan.root.relation.as_deref(), Some("items"));
    assert_eq!(scan.root.estimates.cost, Some(22.5));
    assert_eq!(scan.root.actual.time_ms, Some(0.180));
    assert!(scan.raw.contains("Seq Scan"));

    let join = parse_fixture("join");
    assert_eq!(join.root.kind, "Hash Join");
    assert_eq!(join.root.children.len(), 2);
    assert_eq!(join.root.children[0].relation.as_deref(), Some("orders"));
    assert_eq!(
        join.root.children[1].children[0].relation.as_deref(),
        Some("users")
    );

    let sort = parse_fixture("sort");
    assert_eq!(sort.root.kind, "Sort");
    assert_eq!(sort.root.children[0].kind, "Seq Scan");

    let aggregate = parse_fixture("aggregate");
    assert_eq!(aggregate.root.kind, "Aggregate");
    assert_eq!(aggregate.root.estimates.rows, Some(10.0));
    assert_eq!(aggregate.root.actual.rows, Some(10.0));

    let parallel = parse_fixture("parallel");
    assert_eq!(parallel.root.kind, "Gather");
    assert_eq!(parallel.root.loops, Some(1));
    assert_eq!(parallel.root.children[0].loops, Some(3));
    assert_eq!(parallel.root.children[0].relation.as_deref(), Some("big"));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn container_explain_estimated_and_analyze() {
    use dexo_driver_api::{ConnectRequest, ConnectionFactory, ExplainRequest};
    use dexo_driver_postgres::PostgresFactory;
    use dexo_test_support::DatabasePair;
    use secrecy::SecretString;

    let pair = DatabasePair::start().await.unwrap();
    let session = PostgresFactory
        .connect(ConnectRequest {
            endpoint: pair.postgres_endpoint().to_string(),
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
    assert!(estimated.root.kind.contains("Result") || estimated.root.kind.contains("Scan"));
    assert!(estimated.execution_ms.is_none());
    assert!(estimated.raw.contains("Plan"));
    let analyzed = session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("select 1"))
        .await
        .unwrap();
    assert!(analyzed.execution_ms.is_some());
    assert!(analyzed.root.actual.time_ms.is_some() || analyzed.root.loops.is_some());
}
