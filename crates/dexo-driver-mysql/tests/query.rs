use dexo_driver_api::{
    ConnectRequest, ConnectionFactory, DbValue, QueryEvent, QueryRequest, Session,
};
use dexo_driver_mysql::MysqlFactory;
use dexo_test_support::DatabasePair;
use futures_util::StreamExt;
use secrecy::SecretString;

struct Fixture {
    _pair: DatabasePair,
    session: Box<dyn Session>,
}

async fn connect_mysql_fixture() -> Fixture {
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
    Fixture {
        _pair: pair,
        session,
    }
}

async fn first_value(stream: &mut dexo_driver_api::QueryStream) -> DbValue {
    while let Some(event) = stream.next().await {
        if let QueryEvent::Rows(batch) = event.unwrap() {
            return batch
                .rows
                .into_iter()
                .next()
                .unwrap()
                .into_iter()
                .next()
                .unwrap();
        }
    }
    panic!("no value");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn streams_mysql_rows_without_collecting_all() {
    let fixture = connect_mysql_fixture().await;
    let mut stream = fixture
        .session
        .execute(QueryRequest::read(
            "WITH RECURSIVE seq AS (SELECT 1 AS n UNION ALL SELECT n + 1 FROM seq WHERE n < 513) SELECT n FROM seq",
            1000,
        ))
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

#[tokio::test]
#[ignore = "requires Docker"]
async fn streams_mysql_rows_and_unsigned_values() {
    let fixture = connect_mysql_fixture().await;
    let mut stream = fixture
        .session
        .execute(QueryRequest::read(
            "SELECT CAST(18446744073709551615 AS UNSIGNED)",
            10,
        ))
        .await
        .unwrap();
    assert_eq!(first_value(&mut stream).await, DbValue::U64(u64::MAX));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn cancel_mysql_sleep() {
    let fixture = connect_mysql_fixture().await;
    let request = QueryRequest::read("SELECT SLEEP(30)", 1);
    let id = request.id;
    let mut stream = fixture.session.execute(request).await.unwrap();
    let consume = tokio::spawn(async move {
        let mut cancelled = false;
        while let Some(event) = stream.next().await {
            match event {
                Err(_) => {
                    cancelled = true;
                    break;
                }
                Ok(QueryEvent::Rows(batch)) => {
                    // MySQL SLEEP returns 1 when KILL QUERY interrupts, not a protocol error.
                    if batch
                        .rows
                        .iter()
                        .flatten()
                        .any(|value| matches!(value, DbValue::I64(1) | DbValue::U64(1)))
                    {
                        cancelled = true;
                        break;
                    }
                }
                _ => {}
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
    let fixture = connect_mysql_fixture().await;
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
    assert!(matches!(
        count,
        Some(DbValue::I64(0) | DbValue::U64(0) | DbValue::Decimal(_))
    ));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn parameters_rows_affected_and_result_sets_are_observable() {
    let fixture = connect_mysql_fixture().await;
    drain(
        fixture
            .session
            .execute(QueryRequest::write(
                "create table if not exists dexo_params(value varchar(64))",
            ))
            .await
            .unwrap(),
    )
    .await;
    let mut request = QueryRequest::write("insert into dexo_params(value) values (?)");
    request.parameters = vec![DbValue::Text("bound-value".into())];
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
async fn two_result_sets_are_indexed() {
    let fixture = connect_mysql_fixture().await;
    let events = collect(
        fixture
            .session
            .execute(QueryRequest::read("select 1; select 2;", 10))
            .await
            .unwrap(),
    )
    .await;
    let indexes: Vec<usize> = events
        .iter()
        .filter_map(|event| match event {
            QueryEvent::ResultSetStarted { index } => Some(*index),
            _ => None,
        })
        .collect();
    assert_eq!(indexes, vec![0, 1]);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn timeout_or_cancel_mysql_sleep() {
    let fixture = connect_mysql_fixture().await;
    let mut request = QueryRequest::read("SELECT SLEEP(30)", 1);
    request.timeout = std::time::Duration::from_millis(200);
    let events = collect_results(fixture.session.execute(request).await.unwrap()).await;
    assert!(events.iter().any(|event| {
        match event {
            Err(error) => matches!(
                error.category(),
                dexo_driver_api::DriverErrorCategory::Timeout
                    | dexo_driver_api::DriverErrorCategory::Cancelled
            ),
            Ok(QueryEvent::Rows(batch)) => batch
                .rows
                .iter()
                .flatten()
                .any(|value| matches!(value, DbValue::I64(1) | DbValue::U64(1))),
            Ok(_) => false,
        }
    }));
}

/// A result cut at the row limit says so, and the next statement still runs on the
/// connection; one that holds exactly the limit does not say so.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_result_cut_at_the_row_limit_says_so() {
    let fixture = connect_mysql_fixture().await;
    // Up to 100,000 rows from five joined digits; no recursion limit to raise.
    let numbers = |count: u64| {
        format!(
            "with d(i) as (select 0 union all select 1 union all select 2 union all select 3 \
             union all select 4 union all select 5 union all select 6 union all select 7 \
             union all select 8 union all select 9) \
             select a.i + 10 * b.i + 100 * c.i + 1000 * e.i + 10000 * f.i as n \
             from d a, d b, d c, d e, d f \
             where a.i + 10 * b.i + 100 * c.i + 1000 * e.i + 10000 * f.i < {count}"
        )
    };
    for (count, limit) in [
        (5, 3),
        (5, 5),
        (5, 0),
        (10_000, 10_000),
        (10_001, 10_000),
        (25_000, 10_000),
    ] {
        let stream = fixture
            .session
            .execute(QueryRequest::read(numbers(count), limit))
            .await
            .unwrap();
        // Every event, not only the first set's: the rest of a cut set once came back
        // as a second one, which the first `ResultSetFinished` never showed.
        let events = collect(stream).await;
        let started = events
            .iter()
            .filter(|event| matches!(event, QueryEvent::ResultSetStarted { .. }))
            .count();
        let rows: u64 = events
            .iter()
            .map(|event| match event {
                QueryEvent::Rows(batch) => batch.rows.len() as u64,
                _ => 0,
            })
            .sum();
        let finished: Vec<bool> = events
            .iter()
            .filter_map(|event| match event {
                QueryEvent::ResultSetFinished { truncated, .. } => Some(*truncated),
                _ => None,
            })
            .collect();
        let cut = limit > 0 && count > limit;
        let shown = if limit == 0 { count } else { count.min(limit) };
        assert_eq!(started, 1, "{count} rows, limit {limit}");
        assert_eq!(rows, shown, "{count} rows, limit {limit}");
        assert_eq!(finished, [cut], "{count} rows, limit {limit}");
    }
    // A cut set does not eat the one after it.
    let stream = fixture
        .session
        .execute(QueryRequest::read(format!("{}; select 42", numbers(5)), 3))
        .await
        .unwrap();
    let events = collect(stream).await;
    let finished: Vec<bool> = events
        .iter()
        .filter_map(|event| match event {
            QueryEvent::ResultSetFinished { truncated, .. } => Some(*truncated),
            _ => None,
        })
        .collect();
    assert_eq!(finished, [true, false]);
    let last = events.iter().rev().find_map(|event| match event {
        QueryEvent::Rows(batch) => batch.rows.first().cloned(),
        _ => None,
    });
    assert!(
        matches!(last.as_deref(), Some([DbValue::I64(42) | DbValue::U64(42)])),
        "{last:?}"
    );
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

async fn write_fails(session: &dyn Session, sql: &str) -> bool {
    match session.execute(QueryRequest::write(sql)).await {
        Err(_) => true,
        Ok(mut stream) => {
            let mut failed = false;
            while let Some(event) = stream.next().await {
                failed |= event.is_err();
            }
            failed
        }
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_read_only_session_refuses_writes_on_the_server() {
    let pair = DatabasePair::start().await.unwrap();
    let request = |read_only| {
        ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            read_only,
        )
    };
    let writer = MysqlFactory.connect(request(false)).await.unwrap();
    assert!(
        !write_fails(
            &*writer,
            "create table if not exists ro_probe (id int primary key)"
        )
        .await
    );
    let reader = MysqlFactory.connect(request(true)).await.unwrap();
    assert!(write_fails(&*reader, "insert into ro_probe values (1)").await);
    assert!(write_fails(&*reader, "delete from ro_probe").await);
    assert!(write_fails(&*reader, "create table ro_probe_2 (id int)").await);
    let mut stream = reader
        .execute(QueryRequest::read("select count(*) from ro_probe", 1))
        .await
        .unwrap();
    first_value(&mut stream).await;
}

/// A query that times out is stopped on the server, not only no longer waited for.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires Docker"]
async fn a_timed_out_query_stops_on_the_server() {
    let fixture = connect_mysql_fixture().await;
    let mut request = QueryRequest::read("SELECT SLEEP(30) /* dexo-timeout-probe */", 1);
    request.timeout = std::time::Duration::from_secs(1);
    let mut stream = fixture.session.execute(request).await.unwrap();
    let mut timed_out = false;
    while let Some(event) = stream.next().await {
        if let Err(error) = event {
            timed_out |= error.category() == dexo_driver_api::DriverErrorCategory::Timeout;
        }
    }
    assert!(timed_out);
    let watcher = MysqlFactory
        .connect(ConnectRequest::new(
            fixture._pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let mut stream = watcher
            .execute(QueryRequest::read(
                "SELECT CAST(COUNT(*) AS CHAR) FROM information_schema.PROCESSLIST \
                 WHERE INFO LIKE CONCAT('%dexo-timeout-', 'probe%') AND ID <> CONNECTION_ID()",
                1,
            ))
            .await
            .unwrap();
        let still = first_value(&mut stream).await;
        if still == DbValue::Text("0".into()) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the query still runs on the server: {still:?}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// MariaDB runs what `/*M! … */` holds, so the read check takes it for code: the ORDER
/// BY that wrote a file past it is refused, and so is a statement holding one.
#[tokio::test]
#[ignore = "requires Docker"]
async fn executable_comments_are_read_as_the_server_runs_them() {
    let fixture = connect_mysql_fixture().await;
    let mut stream = fixture
        .session
        .execute(QueryRequest::read("select version()", 1))
        .await
        .unwrap();
    let mariadb = matches!(
        first_value(&mut stream).await,
        DbValue::Text(version) if version.to_ascii_lowercase().contains("mariadb")
    );
    drain(stream).await;
    let mut stream = fixture
        .session
        .execute(QueryRequest::read("select 1 /*M!100100 + 1 */", 1))
        .await
        .unwrap();
    let value = first_value(&mut stream).await;
    drain(stream).await;
    let ran = matches!(value, DbValue::I64(2) | DbValue::U64(2));
    assert_eq!(ran, mariadb, "{value:?}");
    let clauses = dexo_driver_api::RawClauses {
        where_sql: None,
        order_by: Some("id /*M! LIMIT 1 INTO OUTFILE '/tmp/x' */ -- x".into()),
    };
    assert!(dexo_sql::clauses_read(&clauses, dexo_sql::Dialect::Mysql).is_err());
    assert!(!dexo_sql::is_read(
        "select 1 /*M!100100 into outfile '/tmp/x' */",
        dexo_sql::Dialect::Mysql
    ));
}

/// An exported INSERT replays its values as they were: a backslash is a backslash, and
/// `\'` in a value does not end the literal and run what follows it.
#[tokio::test]
#[ignore = "requires Docker"]
async fn an_exported_insert_replays_backslashes_as_themselves() {
    let fixture = connect_mysql_fixture().await;
    for sql in [
        "create table victim2 (id int)",
        "create table dest (v text, path text, c text)",
    ] {
        drain(
            fixture
                .session
                .execute(QueryRequest::write(sql))
                .await
                .unwrap(),
        )
        .await;
    }
    let values = [
        "a\\'); DROP TABLE victim2; -- ",
        "C:\\new\\table",
        "O'Brien\0\n\r\x1a",
    ];
    let options = dexo_app::transfer::codec::FormatOptions {
        dialect: dexo_app::data::SqlDialect::Mysql,
        ..Default::default()
    };
    let exported = dexo_app::transfer::codec::encode_document(
        dexo_app::transfer::codec::TransferFormat::Sql,
        &options,
        &["v".into(), "path".into(), "c".into()],
        &[values
            .iter()
            .map(|value| DbValue::Text(value.to_string()))
            .collect()],
    )
    .unwrap();
    let insert = String::from_utf8(exported).unwrap();
    drain(
        fixture
            .session
            .execute(QueryRequest::write(insert))
            .await
            .unwrap(),
    )
    .await;
    let mut stream = fixture
        .session
        .execute(QueryRequest::read(
            "select hex(v), hex(path), hex(c), \
             (select count(*) from information_schema.tables where table_name = 'victim2') \
             from dest",
            10,
        ))
        .await
        .unwrap();
    let mut row = Vec::new();
    while let Some(event) = stream.next().await {
        if let QueryEvent::Rows(batch) = event.unwrap() {
            row = batch.rows.into_iter().next().unwrap();
        }
    }
    let hex = |text: &str| {
        text.bytes()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>()
    };
    for (value, want) in row.iter().zip(values) {
        assert_eq!(value, &DbValue::Text(hex(want)), "{want:?}");
    }
    assert!(
        matches!(row[3], DbValue::I64(1) | DbValue::U64(1)),
        "{:?}",
        row[3]
    );
}
