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
    assert_eq!(aggregate.root.kind, "HashAggregate");
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
        .connect(ConnectRequest::new(
            pair.postgres_endpoint().to_string(),
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

    // A parameter has no value: Postgres 16 plans for any, an older server refuses
    // clearly, and ANALYZE, which runs the statement, refuses everywhere.
    let generic = session
        .explain()
        .unwrap()
        .explain(ExplainRequest::estimated(
            "select * from pg_class where oid = $1 and relname = $2;",
        ))
        .await;
    let version: i64 = {
        let mut stream = session
            .execute(dexo_driver_api::QueryRequest::read(
                "select current_setting('server_version_num')::int",
                1,
            ))
            .await
            .unwrap();
        let mut found = 0;
        while let Some(event) = futures_util::StreamExt::next(&mut stream).await {
            if let Ok(dexo_driver_api::QueryEvent::Rows(batch)) = event
                && let dexo_driver_api::DbValue::I64(value) = batch.rows[0][0]
            {
                found = value;
            }
        }
        found
    };
    if version >= 160_000 {
        assert!(generic.unwrap().raw.contains("Plan"));
    } else {
        let refused = generic.unwrap_err();
        assert_eq!(
            refused.category(),
            dexo_driver_api::DriverErrorCategory::Capability
        );
    }
    let refused = session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("select $1::int + 1"))
        .await
        .unwrap_err();
    assert_eq!(
        refused.category(),
        dexo_driver_api::DriverErrorCategory::Capability
    );
    assert!(refused.to_string().contains("parameters"), "{refused}");
    // The fence closed: the session still runs a plain plan.
    session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("select 1"))
        .await
        .unwrap();
}

/// A trailing `--` comment on the explained statement used to comment out the fence's
/// ROLLBACK, leaving the delete pending in an open transaction the next commit kept.
#[tokio::test]
#[ignore = "requires Docker"]
async fn analyze_rolls_back_a_statement_that_ends_in_a_line_comment() {
    use dexo_driver_api::{ConnectRequest, ConnectionFactory, ExplainRequest, QueryRequest};
    use dexo_driver_postgres::PostgresFactory;
    use dexo_test_support::DatabasePair;
    use futures_util::StreamExt;
    use secrecy::SecretString;

    async fn run(session: &dyn dexo_driver_api::Session, sql: &str) -> Vec<String> {
        let mut stream = session.execute(QueryRequest::write(sql)).await.unwrap();
        let mut values = Vec::new();
        while let Some(event) = stream.next().await {
            if let dexo_driver_api::QueryEvent::Rows(batch) = event.unwrap() {
                for row in batch.rows {
                    values.extend(row.into_iter().map(|value| format!("{value:?}")));
                }
            }
        }
        values
    }

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
    run(&*session, "create table fence_probe (id int)").await;
    run(
        &*session,
        "insert into fence_probe select generate_series(1, 5)",
    )
    .await;
    session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed(
            "delete from fence_probe -- clean up",
        ))
        .await
        .unwrap();
    let count = run(&*session, "select count(*) from fence_probe").await;
    assert!(count.iter().any(|value| value.contains('5')), "{count:?}");
    let open = run(&*session, "select txid_current_if_assigned() is not null").await;
    assert!(open.iter().any(|value| value.contains("false")), "{open:?}");
}
