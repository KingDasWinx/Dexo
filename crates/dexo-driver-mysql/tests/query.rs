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
        .connect(ConnectRequest {
            endpoint: pair.mysql_endpoint().to_string(),
            database: Some("dexo".into()),
            username: "dexo".into(),
            secret: SecretString::from("dexo_test_only"),
            read_only: false,
        })
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
    assert!(events.iter().any(|event| match event {
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
    }));
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
