use dexo_driver_api::{ConnectRequest, ConnectionFactory, QueryEvent, QueryRequest, Session};
use dexo_driver_postgres::PostgresFactory;
use dexo_test_support::DatabasePair;
use futures_util::StreamExt;
use secrecy::SecretString;

struct Fixture {
    _pair: DatabasePair,
    session: Box<dyn Session>,
}

async fn connect_postgres_fixture() -> Fixture {
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
    Fixture {
        _pair: pair,
        session,
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn streams_postgres_rows_without_collecting_all() {
    let fixture = connect_postgres_fixture().await;
    let mut stream = fixture
        .session
        .execute(QueryRequest::read("select generate_series(1, 513)", 1000))
        .await
        .unwrap();
    let mut batches = 0;
    while let Some(event) = StreamExt::next(&mut stream).await {
        if matches!(event.unwrap(), QueryEvent::Rows(_)) {
            batches += 1;
        }
    }
    assert!(batches >= 3);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn cancel_pg_sleep() {
    let fixture = connect_postgres_fixture().await;
    let request = QueryRequest::read("select pg_sleep(30)", 1);
    let id = request.id;
    let mut stream = fixture.session.execute(request).await.unwrap();
    let consume = tokio::spawn(async move {
        let mut cancelled = false;
        while let Some(event) = stream.next().await {
            if event.is_err() {
                cancelled = true;
                break;
            }
        }
        cancelled
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    fixture.session.cancel(id).await.unwrap();
    assert!(consume.await.unwrap());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn transaction_contract() {
    let fixture = connect_postgres_fixture().await;
    drain(
        fixture
            .session
            .execute(QueryRequest::write(
                "create table if not exists tx_test (id int primary key)",
            ))
            .await
            .unwrap(),
    )
    .await;
    drain(
        fixture
            .session
            .execute(QueryRequest::write("delete from tx_test"))
            .await
            .unwrap(),
    )
    .await;
    let tx = fixture
        .session
        .transactions()
        .expect("transaction capability");
    tx.begin(dexo_driver_api::TransactionMode::ReadWrite)
        .await
        .unwrap();
    tx.savepoint("before_insert").await.unwrap();
    drain(
        fixture
            .session
            .execute(QueryRequest::write("insert into tx_test values (1)"))
            .await
            .unwrap(),
    )
    .await;
    tx.rollback_to("before_insert").await.unwrap();
    tx.commit().await.unwrap();
    let mut stream = fixture
        .session
        .execute(QueryRequest::read("select count(*) from tx_test", 10))
        .await
        .unwrap();
    let mut count = None;
    while let Some(event) = stream.next().await {
        if let QueryEvent::Rows(batch) = event.unwrap() {
            count = Some(batch.rows[0][0].clone());
        }
    }
    assert_eq!(count, Some(dexo_driver_api::DbValue::I64(0)));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn parameters_rows_affected_and_result_sets_are_observable() {
    let fixture = connect_postgres_fixture().await;
    drain(
        fixture
            .session
            .execute(QueryRequest::write(
                "create table if not exists dexo_params(value text)",
            ))
            .await
            .unwrap(),
    )
    .await;
    let mut request = QueryRequest::write("insert into dexo_params(value) values ($1)");
    request.parameters = vec![dexo_driver_api::DbValue::Text("bound-value".into())];
    let events = collect(fixture.session.execute(request).await.unwrap()).await;
    assert!(events.iter().any(|event| {
        matches!(
            event,
            QueryEvent::Finished {
                rows_affected: Some(1),
            }
        )
    }));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn timeout_or_cancel_pg_sleep() {
    let fixture = connect_postgres_fixture().await;
    let mut request = QueryRequest::read("select pg_sleep(30)", 1);
    request.timeout = std::time::Duration::from_millis(200);
    let events = collect_results(fixture.session.execute(request).await.unwrap()).await;
    assert!(events.iter().any(|event| match event {
        Err(error) => matches!(
            error.category(),
            dexo_driver_api::DriverErrorCategory::Timeout
                | dexo_driver_api::DriverErrorCategory::Cancelled
        ),
        Ok(_) => false,
    }));
}

/// A result cut at the row limit says so; one that holds exactly the limit does not.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_result_cut_at_the_row_limit_says_so() {
    let fixture = connect_postgres_fixture().await;
    for (limit, cut) in [(3, true), (5, false), (0, false)] {
        let stream = fixture
            .session
            .execute(QueryRequest::read("select generate_series(1, 5)", limit))
            .await
            .unwrap();
        let truncated = collect(stream)
            .await
            .into_iter()
            .find_map(|event| match event {
                QueryEvent::ResultSetFinished { truncated, .. } => Some(truncated),
                _ => None,
            });
        assert_eq!(truncated, Some(cut), "limit {limit}");
    }
}

async fn collect(mut stream: dexo_driver_api::QueryStream) -> Vec<QueryEvent> {
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event.unwrap());
    }
    events
}

async fn collect_results(
    mut stream: dexo_driver_api::QueryStream,
) -> Vec<Result<QueryEvent, dexo_driver_api::DriverError>> {
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event);
    }
    events
}

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
}

/// A query that times out is stopped on the server, not only no longer waited for.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn a_timed_out_query_stops_on_the_server() {
    let fixture = connect_postgres_fixture().await;
    let mut request = QueryRequest::read("select pg_sleep(30) /* dexo-timeout-probe */", 1);
    request.timeout = std::time::Duration::from_secs(1);
    let events = collect_results(fixture.session.execute(request).await.unwrap()).await;
    assert!(events.iter().any(|event| matches!(
        event,
        Err(error) if error.category() == dexo_driver_api::DriverErrorCategory::Timeout
    )));
    let watcher = PostgresFactory
        .connect(ConnectRequest::new(
            fixture._pair.postgres_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let stream = watcher
            .execute(QueryRequest::read(
                "select count(*)::text from pg_stat_activity \
                 where query like '%dexo-timeout-' || 'probe%' and state = 'active' \
                 and pid <> pg_backend_pid()",
                1,
            ))
            .await
            .unwrap();
        let still = collect(stream)
            .await
            .into_iter()
            .find_map(|event| match event {
                QueryEvent::Rows(batch) => batch.rows.into_iter().next(),
                _ => None,
            });
        if still == Some(vec![dexo_driver_api::DbValue::Text("0".into())]) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the query still runs on the server: {still:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// Text asked to only read runs where it cannot write: alone, in a read-only transaction
/// of its own; inside the user's transaction, behind a savepoint rolled back after it.
/// Either way the session is as it was afterwards.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn a_read_only_request_cannot_write() {
    let fixture = connect_postgres_fixture().await;
    let run = |sql: &str, read_only: bool| {
        let mut request = QueryRequest::write(sql);
        request.read_only = read_only;
        let session = &fixture.session;
        async move { collect_results(session.execute(request).await.unwrap()).await }
    };
    run("create table ro_probe (id int)", false).await;
    let refused = run("insert into ro_probe values (1)", true).await;
    assert!(refused.iter().any(Result::is_err), "{refused:?}");
    assert!(
        run("insert into ro_probe values (2)", false)
            .await
            .iter()
            .all(Result::is_ok),
        "the session stayed read-only"
    );
    let transactions = fixture.session.transactions().unwrap();
    transactions
        .begin(dexo_driver_api::TransactionMode::ReadWrite)
        .await
        .unwrap();
    run("insert into ro_probe values (3)", false).await;
    run("insert into ro_probe values (4)", true).await;
    transactions.commit().await.unwrap();
    let rows = collect(
        fixture
            .session
            .execute(QueryRequest::read(
                "select string_agg(id::text, ',' order by id) from ro_probe",
                1,
            ))
            .await
            .unwrap(),
    )
    .await
    .into_iter()
    .find_map(|event| match event {
        QueryEvent::Rows(batch) => batch.rows.into_iter().next(),
        _ => None,
    });
    assert_eq!(
        rows,
        Some(vec![dexo_driver_api::DbValue::Text("2,3".into())])
    );
}
