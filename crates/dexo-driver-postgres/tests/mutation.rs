use dexo_driver_api::{
    ColumnId, ConnectRequest, ConnectionFactory, DataRequest, DbValue, DriverErrorCategory, Filter,
    Mutation, Page, QualifiedName, Session, Sort,
};
use dexo_driver_postgres::PostgresFactory;
use dexo_test_support::DatabasePair;
use futures_util::StreamExt;
use secrecy::SecretString;

struct Fixture {
    _pair: DatabasePair,
    session: Box<dyn Session>,
}

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
}

async fn connect() -> Fixture {
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
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "create table if not exists items (id int primary key, n int not null)",
            ))
            .await
            .unwrap(),
    )
    .await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write("delete from items"))
            .await
            .unwrap(),
    )
    .await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "insert into items values (1, 10), (2, 20), (3, 30)",
            ))
            .await
            .unwrap(),
    )
    .await;
    Fixture {
        _pair: pair,
        session,
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_paging_and_typed_filter() {
    let fixture = connect().await;
    let data = fixture.session.data().unwrap();
    let page = data
        .fetch(DataRequest {
            object: QualifiedName::new(None::<String>, Some("public"), "items"),
            columns: vec![ColumnId("id".into()), ColumnId("n".into())],
            filter: Some(Filter::Gt(ColumnId("n".into()), DbValue::I64(10))),
            sort: vec![Sort {
                column: ColumnId("id".into()),
                descending: true,
            }],
            page: Page::new(0, 10).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.rows[0][0], DbValue::I64(3));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_mutation_conflict_commits_zero() {
    let fixture = connect().await;
    let data = fixture.session.data().unwrap();
    let table = QualifiedName::new(None::<String>, Some("public"), "items");
    drain(
        fixture
            .session
            .execute(dexo_driver_api::QueryRequest::write(
                "update items set n = 99 where id = 1",
            ))
            .await
            .unwrap(),
    )
    .await;
    let error = data
        .apply(&[Mutation::Update {
            table: table.clone(),
            identity: vec![(ColumnId("id".into()), DbValue::I64(1))],
            original: vec![(ColumnId("n".into()), DbValue::I64(10))],
            changes: vec![(ColumnId("n".into()), DbValue::I64(11))],
        }])
        .await
        .unwrap_err();
    assert_eq!(error.category(), DriverErrorCategory::Conflict);
    let page = data
        .fetch(DataRequest {
            object: table,
            columns: vec![ColumnId("n".into())],
            filter: Some(Filter::Eq(ColumnId("id".into()), DbValue::I64(1))),
            sort: vec![],
            page: Page::new(0, 1).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows[0][0], DbValue::I64(99));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_batch_insert_update_delete() {
    let fixture = connect().await;
    let data = fixture.session.data().unwrap();
    let table = QualifiedName::new(None::<String>, Some("public"), "items");
    data.apply(&[
        Mutation::Insert {
            table: table.clone(),
            columns: vec![ColumnId("id".into()), ColumnId("n".into())],
            values: vec![DbValue::I64(4), DbValue::I64(40)],
        },
        Mutation::Update {
            table: table.clone(),
            identity: vec![(ColumnId("id".into()), DbValue::I64(2))],
            original: vec![(ColumnId("n".into()), DbValue::I64(20))],
            changes: vec![(ColumnId("n".into()), DbValue::I64(21))],
        },
        Mutation::Delete {
            table: table.clone(),
            identity: vec![(ColumnId("id".into()), DbValue::I64(3))],
            original: vec![(ColumnId("n".into()), DbValue::I64(30))],
        },
    ])
    .await
    .unwrap();
    let page = data
        .fetch(DataRequest {
            object: table,
            columns: vec![ColumnId("id".into())],
            filter: None,
            sort: vec![Sort {
                column: ColumnId("id".into()),
                descending: false,
            }],
            page: Page::new(0, 10).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 3);
    assert_eq!(page.rows[2][0], DbValue::I64(4));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_partial_failure_rolls_back() {
    let fixture = connect().await;
    let data = fixture.session.data().unwrap();
    let table = QualifiedName::new(None::<String>, Some("public"), "items");
    let error = data
        .apply(&[
            Mutation::Insert {
                table: table.clone(),
                columns: vec![ColumnId("id".into()), ColumnId("n".into())],
                values: vec![DbValue::I64(4), DbValue::I64(40)],
            },
            Mutation::Update {
                table: table.clone(),
                identity: vec![(ColumnId("id".into()), DbValue::I64(1))],
                original: vec![(ColumnId("n".into()), DbValue::I64(999))],
                changes: vec![(ColumnId("n".into()), DbValue::I64(11))],
            },
        ])
        .await
        .unwrap_err();
    assert_eq!(error.category(), DriverErrorCategory::Conflict);
    let page = data
        .fetch(DataRequest {
            object: table,
            columns: vec![ColumnId("id".into())],
            filter: Some(Filter::Eq(ColumnId("id".into()), DbValue::I64(4))),
            sort: vec![],
            page: Page::new(0, 1).unwrap(),
        })
        .await
        .unwrap();
    assert!(page.rows.is_empty());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_empty_apply_is_cancel() {
    let fixture = connect().await;
    let data = fixture.session.data().unwrap();
    data.apply(&[]).await.unwrap();
    let page = data
        .fetch(DataRequest {
            object: QualifiedName::new(None::<String>, Some("public"), "items"),
            columns: vec![ColumnId("id".into())],
            filter: None,
            sort: vec![],
            page: Page::new(0, 10).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 3);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_bulk_insert_batch() {
    let fixture = connect().await;
    let writer = fixture.session.bulk().unwrap();
    let table = QualifiedName::new(None::<String>, Some("public"), "items");
    writer
        .insert_batch(
            &table,
            &["id".into(), "n".into()],
            &[vec![DbValue::I64(20), DbValue::I64(200)]],
        )
        .await
        .unwrap();
    let page = fixture
        .session
        .data()
        .unwrap()
        .fetch(DataRequest {
            object: table,
            columns: vec![ColumnId("id".into())],
            filter: Some(Filter::Eq(ColumnId("id".into()), DbValue::I64(20))),
            sort: vec![],
            page: Page::new(0, 1).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
}
