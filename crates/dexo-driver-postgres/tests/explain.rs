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

    // Inside a transaction the user typed, which the session's state never hears of,
    // the fence's ROLLBACK used to end it and take the user's own work with it.
    run(&*session, "begin").await;
    run(&*session, "insert into fence_probe values (6)").await;
    session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("delete from fence_probe"))
        .await
        .unwrap();
    let count = run(&*session, "select count(*) from fence_probe").await;
    assert_eq!(count, ["I64(6)"]);
    let open = run(&*session, "select txid_current_if_assigned() is not null").await;
    assert_eq!(open, ["Bool(true)"]);
    run(&*session, "rollback").await;
}

/// A plan asked inside the user's transaction -- begun from Dexo or typed -- of a
/// statement with parameters used to abort it: every statement after failed with
/// "current transaction is aborted", and Postgres 16 reported that instead of the
/// parameters. The transaction now goes on as it was.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_plan_with_parameters_leaves_the_transaction_usable() {
    use dexo_driver_api::{
        ConnectRequest, ConnectionFactory, DriverErrorCategory, ExplainRequest, QueryRequest,
        TransactionMode,
    };
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
    run(
        &*session,
        "create table plan_orders (id int, customer_id int)",
    )
    .await;
    let version = run(
        &*session,
        "select current_setting('server_version_num')::int",
    )
    .await;
    let generic = version[0]
        .trim_start_matches("I64(")
        .trim_end_matches(')')
        .parse::<i64>()
        .unwrap()
        >= 160_000;
    for typed in [false, true] {
        if typed {
            run(&*session, "begin").await;
        } else {
            session
                .transactions()
                .unwrap()
                .begin(TransactionMode::ReadWrite)
                .await
                .unwrap();
        }
        run(&*session, "insert into plan_orders values (1, 1)").await;
        let planned = session
            .explain()
            .unwrap()
            .explain(ExplainRequest::estimated(
                "select * from plan_orders where customer_id = $1",
            ))
            .await;
        if generic {
            assert!(planned.unwrap().raw.contains("Plan"));
        } else {
            let refused = planned.unwrap_err();
            assert_eq!(refused.category(), DriverErrorCategory::Capability);
        }
        // No type to plan with: the extended protocol refuses it before 16, and 16 plans
        // it for any value.
        let untyped = session
            .explain()
            .unwrap()
            .explain(ExplainRequest::estimated(
                "select * from plan_orders where $1 is null",
            ))
            .await;
        if generic {
            assert!(untyped.unwrap().raw.contains("Plan"));
        } else {
            let refused = untyped.unwrap_err();
            assert_eq!(refused.category(), DriverErrorCategory::Capability);
            assert!(refused.to_string().contains("parameters"), "{refused}");
        }
        // The transaction is alive, and holds the user's row.
        let count = run(&*session, "select count(*) from plan_orders").await;
        assert_eq!(count, ["I64(1)"], "typed: {typed}");
        if typed {
            run(&*session, "rollback").await;
        } else {
            session.transactions().unwrap().rollback().await.unwrap();
        }
    }
}
