use dexo_driver_api::{
    ColumnId, ConnectRequest, ConnectionFactory, DataRequest, DbValue, Filter, Mutation, Page,
    QualifiedName, Session, Sort,
};
use dexo_driver_mysql::MysqlFactory;
use dexo_driver_postgres::PostgresFactory;
use dexo_test_support::DatabasePair;
use dexo_test_support::containers::TEST_PASSWORD;
use futures_util::StreamExt;
use secrecy::SecretString;

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
}

async fn postgres() -> (DatabasePair, Box<dyn Session>) {
    let pair = DatabasePair::start().await.unwrap();
    let session = PostgresFactory
        .connect(ConnectRequest::new(
            pair.postgres_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from(TEST_PASSWORD),
            false,
        ))
        .await
        .unwrap();
    (pair, session)
}

async fn mysql(pair: &DatabasePair) -> Box<dyn Session> {
    MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from(TEST_PASSWORD),
            false,
        ))
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_data_page_and_filter() {
    let (_pair, session) = postgres().await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "CREATE TABLE live_items (id int PRIMARY KEY, n int NOT NULL)",
            ))
            .await
            .unwrap(),
    )
    .await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "INSERT INTO live_items VALUES (1, 10), (2, 20)",
            ))
            .await
            .unwrap(),
    )
    .await;
    let data = session.data().expect("data");
    let page = data
        .fetch(DataRequest {
            object: QualifiedName::new(None::<String>, Some("public"), "live_items"),
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
    assert_eq!(page.rows.len(), 1);
    assert!(!page.has_more);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_changes_apply_insert_update_delete() {
    let (_pair, session) = postgres().await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "CREATE TABLE live_edit (id int PRIMARY KEY, n int NOT NULL)",
            ))
            .await
            .unwrap(),
    )
    .await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "INSERT INTO live_edit VALUES (1, 10)",
            ))
            .await
            .unwrap(),
    )
    .await;
    let data = session.data().expect("data");
    let table = QualifiedName::new(None::<String>, Some("public"), "live_edit");
    data.apply(&[
        Mutation::Insert {
            table: table.clone(),
            columns: vec![ColumnId("id".into()), ColumnId("n".into())],
            values: vec![DbValue::I64(2), DbValue::I64(20)],
        },
        Mutation::Update {
            table: table.clone(),
            identity: vec![(ColumnId("id".into()), DbValue::I64(1))],
            original: vec![(ColumnId("n".into()), DbValue::I64(10))],
            changes: vec![(ColumnId("n".into()), DbValue::I64(11))],
        },
    ])
    .await
    .unwrap();
    data.apply(&[Mutation::Delete {
        table: table.clone(),
        identity: vec![(ColumnId("id".into()), DbValue::I64(2))],
        original: vec![(ColumnId("n".into()), DbValue::I64(20))],
    }])
    .await
    .unwrap();
    let page = data
        .fetch(DataRequest {
            object: table,
            columns: vec![ColumnId("id".into()), ColumnId("n".into())],
            filter: None,
            sort: vec![],
            page: Page::new(0, 10).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0][1], DbValue::I64(11));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn postgres_foreign_key_composite_and_simple() {
    let (_pair, session) = postgres().await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "CREATE TABLE live_users (
                    org int NOT NULL,
                    id int NOT NULL,
                    PRIMARY KEY (org, id)
                )",
            ))
            .await
            .unwrap(),
    )
    .await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "CREATE TABLE live_orders (
                    org_id int NOT NULL,
                    user_id int NOT NULL,
                    FOREIGN KEY (org_id, user_id) REFERENCES live_users(org, id)
                )",
            ))
            .await
            .unwrap(),
    )
    .await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "INSERT INTO live_users VALUES (7, 3)",
            ))
            .await
            .unwrap(),
    )
    .await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "INSERT INTO live_orders VALUES (7, 3)",
            ))
            .await
            .unwrap(),
    )
    .await;
    let fk = dexo_app::data::ForeignKey {
        local: vec!["org_id".into(), "user_id".into()],
        referenced_table: QualifiedName::new(None::<String>, Some("public"), "live_users"),
        referenced: vec!["org".into(), "id".into()],
    };
    let filter = dexo_app::data::related_filter(
        &fk,
        &[
            ("org_id".into(), Some(DbValue::I64(7))),
            ("user_id".into(), Some(DbValue::I64(3))),
        ],
    )
    .unwrap();
    let page = session
        .data()
        .unwrap()
        .fetch(DataRequest {
            object: fk.referenced_table,
            columns: vec![],
            filter: Some(filter),
            sort: vec![],
            page: Page::new(0, 10).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn mysql_foreign_key_simple() {
    let pair = DatabasePair::start().await.unwrap();
    let session = mysql(&pair).await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "CREATE TABLE live_users (id int PRIMARY KEY) ENGINE=InnoDB",
            ))
            .await
            .unwrap(),
    )
    .await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "CREATE TABLE live_orders (
                    user_id int NOT NULL,
                    FOREIGN KEY (user_id) REFERENCES live_users(id)
                ) ENGINE=InnoDB",
            ))
            .await
            .unwrap(),
    )
    .await;
    drain(
        session
            .execute(dexo_driver_api::QueryRequest::write(
                "INSERT INTO live_users VALUES (9); INSERT INTO live_orders VALUES (9)",
            ))
            .await
            .unwrap(),
    )
    .await;
    let fk = dexo_app::data::ForeignKey {
        local: vec!["user_id".into()],
        referenced_table: QualifiedName::new(Some("dexo"), None::<String>, "live_users"),
        referenced: vec!["id".into()],
    };
    let filter =
        dexo_app::data::related_filter(&fk, &[("user_id".into(), Some(DbValue::I64(9)))]).unwrap();
    let page = session
        .data()
        .unwrap()
        .fetch(DataRequest {
            object: fk.referenced_table,
            columns: vec![],
            filter: Some(filter),
            sort: vec![],
            page: Page::new(0, 10).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
}
