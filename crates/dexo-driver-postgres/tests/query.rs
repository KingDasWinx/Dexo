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

/// Without hypopg a hypothetical index says how to get it; with ANALYZE, or with text
/// that is not an index definition, it is refused before anything reaches the server.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_hypothetical_index_needs_hypopg_and_an_estimated_plan() {
    let fixture = connect_postgres_fixture().await;
    let explain = fixture.session.explain().unwrap();
    let error = explain
        .explain(dexo_driver_api::ExplainRequest::with_indexes(
            "select 1",
            vec!["CREATE INDEX ON pg_class (relname)".into()],
        ))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("CREATE EXTENSION hypopg"), "{error}");
    let mut analyzed = dexo_driver_api::ExplainRequest::with_indexes(
        "select 1",
        vec!["CREATE INDEX ON t (a)".into()],
    );
    analyzed.analyze = true;
    assert!(explain.explain(analyzed).await.is_err());
    assert!(
        explain
            .explain(dexo_driver_api::ExplainRequest::with_indexes(
                "select 1",
                vec!["DROP TABLE t".into()],
            ))
            .await
            .unwrap_err()
            .to_string()
            .contains("not an index definition")
    );
}

/// A session on a server with hypopg installed, named by DEXO_HYPOPG_ENDPOINT (user and
/// database `dexo`, password in DEXO_HYPOPG_PASSWORD), with `table` made afresh: a
/// hundred thousand rows in its one column `a`. `None` when no such server is named.
async fn hypopg_session(table: &str) -> Option<Box<dyn Session>> {
    let endpoint = std::env::var("DEXO_HYPOPG_ENDPOINT").ok()?;
    let password = std::env::var("DEXO_HYPOPG_PASSWORD").unwrap_or_default();
    let session = PostgresFactory
        .connect(ConnectRequest::new(
            endpoint,
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from(password),
            false,
        ))
        .await
        .unwrap();
    for sql in [
        "CREATE EXTENSION IF NOT EXISTS hypopg".to_string(),
        format!("DROP TABLE IF EXISTS {table}"),
        format!("CREATE TABLE {table} AS SELECT g AS a FROM generate_series(1, 100000) g"),
        format!("ANALYZE {table}"),
    ] {
        collect(session.execute(QueryRequest::write(sql)).await.unwrap()).await;
    }
    Some(session)
}

/// With hypopg the plan uses the index as if it were built, and the next plan on the
/// session does not: it never outlives its EXPLAIN.
#[tokio::test]
#[ignore = "requires a Postgres with hypopg"]
async fn a_hypothetical_index_is_planned_with_and_then_gone() {
    let Some(session) = hypopg_session("hypo_probe").await else {
        return;
    };
    let explain = session.explain().unwrap();
    let sql = "select * from hypo_probe where a = 42";
    let with = explain
        .explain(dexo_driver_api::ExplainRequest::with_indexes(
            sql,
            vec!["CREATE INDEX ON hypo_probe (a) /* ; */ WHERE a < 1000;".into()],
        ))
        .await
        .unwrap();
    assert!(with.raw.contains("Index"), "{}", with.raw);
    let without = explain
        .explain(dexo_driver_api::ExplainRequest::estimated(sql))
        .await
        .unwrap();
    assert!(!without.raw.contains("Index"), "{}", without.raw);
}

/// Inside the user's transaction, an index that fails after another was made leaves
/// neither behind: the transaction is not aborted by it, and once it is rolled back the
/// next plain plan uses no index.
#[tokio::test]
#[ignore = "requires a Postgres with hypopg"]
async fn a_failed_hypothetical_index_in_a_transaction_leaves_none_behind() {
    let Some(session) = hypopg_session("hypo_in_tx").await else {
        return;
    };
    let tx = session.transactions().unwrap();
    tx.begin(dexo_driver_api::TransactionMode::ReadWrite)
        .await
        .unwrap();
    let explain = session.explain().unwrap();
    let sql = "select * from hypo_in_tx where a = 42";
    assert!(
        explain
            .explain(dexo_driver_api::ExplainRequest::with_indexes(
                sql,
                vec![
                    "CREATE INDEX ON hypo_in_tx (a)".into(),
                    "CREATE INDEX ON hypo_in_tx (no_such_column)".into(),
                ],
            ))
            .await
            .is_err()
    );
    collect(
        session
            .execute(QueryRequest::read("select 1", 1))
            .await
            .unwrap(),
    )
    .await;
    tx.rollback().await.unwrap();
    let plain = explain
        .explain(dexo_driver_api::ExplainRequest::estimated(sql))
        .await
        .unwrap();
    assert!(!plain.raw.contains("Index"), "{}", plain.raw);
}

/// Trying an index drops only what the try made: a hypothetical index the user made on
/// the session is still there after it.
#[tokio::test]
#[ignore = "requires a Postgres with hypopg"]
async fn trying_an_index_keeps_the_users_own_hypothetical_ones() {
    let Some(session) = hypopg_session("hypo_own").await else {
        return;
    };
    let names = async |session: &dyn Session| {
        collect(
            session
                .execute(QueryRequest::write(
                    "select coalesce(string_agg(index_name, ','), '') from hypopg_list_indexes",
                ))
                .await
                .unwrap(),
        )
        .await
        .into_iter()
        .find_map(|event| match event {
            QueryEvent::Rows(batch) => batch.rows.into_iter().next(),
            _ => None,
        })
    };
    collect(
        session
            .execute(QueryRequest::write(
                "select * from hypopg_create_index('CREATE INDEX ON hypo_own (a)')",
            ))
            .await
            .unwrap(),
    )
    .await;
    let before = names(session.as_ref()).await;
    let explain = session.explain().unwrap();
    explain
        .explain(dexo_driver_api::ExplainRequest::with_indexes(
            "select * from hypo_own where a + 1 = 42",
            vec!["CREATE INDEX ON hypo_own ((a + 1))".into()],
        ))
        .await
        .unwrap();
    assert_eq!(names(session.as_ref()).await, before);
    assert_ne!(
        before,
        Some(vec![dexo_driver_api::DbValue::Text(String::new())])
    );
}

/// Extension and built-in types this driver had no decoder for came out as hex.
#[tokio::test]
#[ignore = "requires Docker"]
async fn extension_and_numeric_types_read_as_postgres_prints_them() {
    let fixture = connect_postgres_fixture().await;
    for extension in ["citext", "ltree"] {
        let setup = QueryRequest::write(format!("create extension if not exists {extension}"));
        collect(fixture.session.execute(setup).await.unwrap()).await;
    }
    let read = QueryRequest::read(
        "select 'MiXed'::citext, 'a.b.c'::ltree, '*.b.*'::lquery, '$.a[*] ? (@ > 1)'::jsonpath,
                '42'::xid, '(3,7)'::tid, '08:00:2b:01:02:03:04:05'::macaddr8,
                '[(1,2),(3,4)]'::lseg, '(3,4),(1,2)'::box, '{1,-1,0}'::line, '<(1,2),3>'::circle,
                '[(0,0),(1,1)]'::path, '((0,0),(1,1),(1,0))'::polygon,
                'the fat cats'::tsvector, 'pg_catalog'::regnamespace",
        0,
    );
    let texts = first_row_texts(&*fixture.session, read).await;
    assert_eq!(
        texts[..13],
        [
            "MiXed",
            "a.b.c",
            "*.b.*",
            "$.\"a\"[*]?(@ > 1)",
            "42",
            "(3,7)",
            "08:00:2b:01:02:03:04:05",
            "[(1,2),(3,4)]",
            "(3,4),(1,2)",
            "{1,-1,0}",
            "<(1,2),3>",
            "[(0,0),(1,1)]",
            "((0,0),(1,1),(1,0))",
        ]
    );
    assert_eq!(texts[13], "'cats' 'fat' 'the'");
    assert_eq!(texts[14], "11");

    // A composite type of the user's is not pgvector's for its name: it read as `[]`.
    let setup = QueryRequest::write("create type vector as (a int, b int)");
    collect(fixture.session.execute(setup).await.unwrap()).await;
    let texts = first_row_texts(
        &*fixture.session,
        QueryRequest::read("select row(1, 2)::vector", 0),
    )
    .await;
    assert_ne!(texts[0], "[]");
}

/// The first row's cells as text: what the grid shows for each.
async fn first_row_texts(session: &dyn Session, request: QueryRequest) -> Vec<String> {
    let events = collect(session.execute(request).await.unwrap()).await;
    let row = events
        .iter()
        .find_map(|event| match event {
            QueryEvent::Rows(batch) => batch.rows.first().cloned(),
            _ => None,
        })
        .unwrap();
    row.iter()
        .map(|cell| match cell {
            dexo_driver_api::DbValue::Text(text)
            | dexo_driver_api::DbValue::Decimal(text)
            | dexo_driver_api::DbValue::Json(text)
            | dexo_driver_api::DbValue::Native { text, .. } => text.clone(),
            dexo_driver_api::DbValue::U64(value) => value.to_string(),
            dexo_driver_api::DbValue::I64(value) => value.to_string(),
            other => format!("{other:?}"),
        })
        .collect()
}
