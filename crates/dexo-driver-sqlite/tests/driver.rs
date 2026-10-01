use std::path::Path;
use std::time::{Duration, Instant};

use dexo_driver_api::{
    Capability, CatalogListOptions, ColumnId, ConnectRequest, ConnectionFactory, DataRequest,
    DbValue, DriverErrorCategory, ExplainRequest, Filter, Mutation, ObjectKind, Page,
    QualifiedName, QueryEvent, QueryRequest, Session, Sort, TransactionMode,
};
use dexo_driver_sqlite::SqliteFactory;
use futures_util::StreamExt;
use secrecy::SecretString;

const SCHEMA: &str = "
    CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, score REAL);
    CREATE TABLE orders (
        id INTEGER PRIMARY KEY,
        customer_id INTEGER NOT NULL REFERENCES customers,
        total REAL
    );
    CREATE INDEX orders_customer ON orders (customer_id);
    CREATE VIEW big_orders AS SELECT * FROM orders WHERE total > 100;
    CREATE TRIGGER orders_touch AFTER UPDATE ON orders BEGIN SELECT 1; END;
    CREATE TABLE notes (body TEXT);
    INSERT INTO customers (id, name, score) VALUES (1, 'Ada', 9.5), (2, 'Grace', NULL), (3, 'Linus', 7.25);
    INSERT INTO orders (id, customer_id, total) VALUES (10, 1, 250.0), (11, 1, 20.5), (12, 3, 99.0);
    INSERT INTO notes (body) VALUES ('first'), ('second');
";

fn request(path: &Path, read_only: bool) -> ConnectRequest {
    ConnectRequest::new(
        path.to_string_lossy(),
        None,
        String::new(),
        SecretString::from(String::new()),
        read_only,
    )
}

async fn open(path: &Path, read_only: bool) -> Box<dyn Session> {
    SqliteFactory
        .connect(request(path, read_only))
        .await
        .expect("connect")
}

async fn seeded() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shop.db");
    seed(&path, SCHEMA).await;
    (dir, path)
}

/// Seeds through the driver itself: a writable open creates the file.
async fn seed(path: &Path, sql: &str) {
    let session = open(path, false).await;
    run(&*session, QueryRequest::write(sql)).await.unwrap();
}

async fn run(
    session: &dyn Session,
    request: QueryRequest,
) -> Result<Vec<QueryEvent>, dexo_driver_api::DriverError> {
    let mut stream = session.execute(request).await?;
    let mut events = Vec::new();
    while let Some(event) = stream.next().await {
        events.push(event?);
    }
    Ok(events)
}

fn rows(events: &[QueryEvent]) -> Vec<Vec<DbValue>> {
    events
        .iter()
        .filter_map(|event| match event {
            QueryEvent::Rows(batch) => Some(batch.rows.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn table(name: &str) -> QualifiedName {
    QualifiedName::new(None::<String>, Some("main"), name)
}

#[tokio::test(flavor = "multi_thread")]
async fn queries_bind_parameters_honour_the_row_limit_and_report_writes() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;

    let mut read = QueryRequest::read(
        "select id, name, score from customers where id >= ? order by id",
        2,
    );
    read.parameters = vec![DbValue::I64(1)];
    let events = run(&*session, read).await.unwrap();
    let QueryEvent::Columns(columns) = &events[1] else {
        panic!("columns follow the start: {events:?}");
    };
    assert_eq!(columns[1].name, "name");
    assert_eq!(columns[1].type_name, "TEXT");
    assert_eq!(
        rows(&events),
        [
            vec![
                DbValue::I64(1),
                DbValue::Text("Ada".into()),
                DbValue::Native {
                    type_name: "real".into(),
                    bytes: vec![],
                    text: "9.5".into()
                }
            ],
            vec![
                DbValue::I64(2),
                DbValue::Text("Grace".into()),
                DbValue::Null
            ],
        ]
    );

    let mut named = QueryRequest::read("select name from customers where name = :who", 0);
    named.parameters = vec![DbValue::Text("Linus".into())];
    let events = run(&*session, named).await.unwrap();
    assert_eq!(rows(&events), [vec![DbValue::Text("Linus".into())]]);

    let missing = run(&*session, QueryRequest::read("select ?", 0))
        .await
        .unwrap_err();
    assert_eq!(missing.category(), DriverErrorCategory::Syntax);

    let events = run(
        &*session,
        QueryRequest::write("update orders set total = total + 1 where customer_id = 1"),
    )
    .await
    .unwrap();
    assert!(events.contains(&QueryEvent::Finished {
        rows_affected: Some(2)
    }));
    let events = run(&*session, QueryRequest::write("create table t (x)"))
        .await
        .unwrap();
    assert!(events.contains(&QueryEvent::Finished {
        rows_affected: Some(0)
    }));

    let syntax = run(&*session, QueryRequest::read("select * frm customers", 0))
        .await
        .unwrap_err();
    assert_eq!(syntax.category(), DriverErrorCategory::Syntax);
    assert!(syntax.position().is_some(), "{syntax:?}");
}

/// A recursive CTE that would count for minutes stops when cancelled, and a short
/// timeout ends one the same way.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_query_is_cancelled_promptly() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let endless = "with recursive n(i) as (select 1 union all select i + 1 from n) \
                   select count(*) from n";
    let request = QueryRequest::read(endless, 0);
    let query = request.id;
    let mut stream = session.execute(request).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let started = Instant::now();
    session.cancel(query).await.unwrap();
    let mut error = None;
    while let Some(event) = stream.next().await {
        if let Err(failure) = event {
            error = Some(failure);
            break;
        }
    }
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        error.expect("cancelled").category(),
        DriverErrorCategory::Cancelled
    );

    let mut timed = QueryRequest::read(endless, 0);
    timed.timeout = Duration::from_millis(200);
    let error = run(&*session, timed).await.unwrap_err();
    assert_eq!(error.category(), DriverErrorCategory::Timeout);

    let events = run(&*session, QueryRequest::read("select 1", 0))
        .await
        .unwrap();
    assert_eq!(rows(&events), [vec![DbValue::I64(1)]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_catalog_lists_tables_columns_indexes_keys_and_triggers() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let catalog = session.catalog().unwrap();
    let options = CatalogListOptions::default();
    let roots = catalog.list_children(None, &options).await.unwrap();
    assert_eq!(roots.objects.len(), 1);
    let main = &roots.objects[0];
    assert_eq!(main.kind, ObjectKind::Catalog);
    assert_eq!(main.qualified_name.object(), "main");

    let relations = catalog
        .list_children(Some(&main.id), &options)
        .await
        .unwrap()
        .objects;
    let names: Vec<_> = relations
        .iter()
        .map(|object| {
            (
                object.qualified_name.display_unquoted(),
                object.kind.clone(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            ("main.big_orders".to_string(), ObjectKind::View),
            ("main.customers".to_string(), ObjectKind::Table),
            ("main.notes".to_string(), ObjectKind::Table),
            ("main.orders".to_string(), ObjectKind::Table),
        ]
    );

    let orders = relations
        .iter()
        .find(|object| object.qualified_name.object() == "orders")
        .unwrap();
    let children = catalog
        .list_children(Some(&orders.id), &options)
        .await
        .unwrap()
        .objects;
    let column = children
        .iter()
        .find(|object| object.qualified_name.object() == "orders.customer_id")
        .unwrap();
    assert_eq!(column.attributes["type"], "INTEGER");
    assert_eq!(column.attributes["driver.sqlite.not_null"], true);
    assert!(
        children
            .iter()
            .any(|object| object.kind == ObjectKind::Index
                && object.qualified_name.object() == "orders_customer")
    );
    assert!(
        children
            .iter()
            .any(|object| object.kind == ObjectKind::Trigger
                && object.qualified_name.object() == "orders_touch")
    );
    let key = children
        .iter()
        .find(|object| object.kind == ObjectKind::Constraint)
        .unwrap();
    assert_eq!(key.attributes["fk_table"], "customers");
    assert_eq!(
        key.attributes["fk_local"],
        serde_json::json!(["customer_id"])
    );
    // `REFERENCES customers` names no column: it is the parent's primary key.
    assert_eq!(key.attributes["fk_referenced"], serde_json::json!(["id"]));

    let found = catalog.object(&column.id).await.unwrap().unwrap();
    assert_eq!(found, *column);
    let ddl = catalog.ddl(&orders.id).await.unwrap().sql;
    assert!(ddl.starts_with("CREATE TABLE orders"), "{ddl}");
    assert!(ddl.contains("CREATE INDEX orders_customer"), "{ddl}");
    assert!(ddl.contains("CREATE TRIGGER orders_touch"), "{ddl}");

    let customers = relations
        .iter()
        .find(|object| object.qualified_name.object() == "customers")
        .unwrap();
    assert_eq!(
        catalog.dependencies(&orders.id).await.unwrap(),
        std::slice::from_ref(&customers.id)
    );
    assert!(
        catalog
            .dependents(&customers.id)
            .await
            .unwrap()
            .contains(&orders.id)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rows_are_paged_filtered_and_edited_by_their_key() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let data = session.data().unwrap();

    let page = data
        .fetch(DataRequest {
            object: table("customers"),
            columns: vec![],
            filter: Some(Filter::Gt(ColumnId("id".into()), DbValue::I64(1))),
            sort: vec![Sort {
                column: ColumnId("name".into()),
                descending: true,
            }],
            page: Page::new(0, 1).unwrap(),
        })
        .await
        .unwrap();
    assert!(page.has_more);
    assert_eq!(
        page.rows,
        [vec![
            DbValue::I64(3),
            DbValue::Text("Linus".into()),
            DbValue::Native {
                type_name: "real".into(),
                bytes: vec![],
                text: "7.25".into()
            }
        ]]
    );

    let keys = data.table_columns(&table("customers")).await.unwrap();
    assert!(keys[0].primary_key && keys[0].name == "id");
    assert!(keys[1].unique && !keys[1].primary_key);

    let identity = vec![(ColumnId("id".into()), DbValue::I64(3))];
    data.apply(&[
        Mutation::Insert {
            table: table("customers"),
            columns: vec![ColumnId("id".into()), ColumnId("name".into())],
            values: vec![DbValue::I64(4), DbValue::Text("Barbara".into())],
        },
        Mutation::Update {
            table: table("customers"),
            identity: identity.clone(),
            original: vec![(ColumnId("score".into()), page.rows[0][2].clone())],
            changes: vec![(ColumnId("name".into()), DbValue::Text("Torvalds".into()))],
        },
        Mutation::Delete {
            table: table("customers"),
            identity: vec![(ColumnId("id".into()), DbValue::I64(2))],
            original: vec![(ColumnId("score".into()), DbValue::Null)],
        },
    ])
    .await
    .unwrap();
    let events = run(
        &*session,
        QueryRequest::read("select id, name from customers order by id", 0),
    )
    .await
    .unwrap();
    assert_eq!(
        rows(&events),
        [
            vec![DbValue::I64(1), DbValue::Text("Ada".into())],
            vec![DbValue::I64(3), DbValue::Text("Torvalds".into())],
            vec![DbValue::I64(4), DbValue::Text("Barbara".into())],
        ]
    );

    // A row that changed under the edit matches nothing, and nothing of the batch lands.
    let conflict = data
        .apply(&[
            Mutation::Insert {
                table: table("customers"),
                columns: vec![ColumnId("name".into())],
                values: vec![DbValue::Text("Edsger".into())],
            },
            Mutation::Delete {
                table: table("customers"),
                identity: vec![(ColumnId("id".into()), DbValue::I64(2))],
                original: vec![],
            },
        ])
        .await
        .unwrap_err();
    assert_eq!(conflict.category(), DriverErrorCategory::Conflict);
    let events = run(
        &*session,
        QueryRequest::read("select count(*) from customers", 0),
    )
    .await
    .unwrap();
    assert_eq!(rows(&events), [vec![DbValue::I64(3)]]);

    // No primary key: the rowid keys the row, and the page carries it.
    let keys = data.table_columns(&table("notes")).await.unwrap();
    assert_eq!(keys[0].name, "rowid");
    assert!(keys[0].primary_key);
    let notes = data
        .fetch(DataRequest {
            object: table("notes"),
            columns: vec![],
            filter: None,
            sort: vec![],
            page: Page::new(0, 10).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(notes.columns[0].name, "rowid");
    data.apply(&[Mutation::Update {
        table: table("notes"),
        identity: vec![(ColumnId("rowid".into()), notes.rows[1][0].clone())],
        original: vec![],
        changes: vec![(ColumnId("body".into()), DbValue::Text("edited".into()))],
    }])
    .await
    .unwrap();
    let events = run(
        &*session,
        QueryRequest::read("select body from notes where rowid = 2", 0),
    )
    .await
    .unwrap();
    assert_eq!(rows(&events), [vec![DbValue::Text("edited".into())]]);

    let written = session
        .bulk()
        .unwrap()
        .insert_batch(
            &table("notes"),
            &["body".to_string()],
            &[
                vec![DbValue::Text("a".into())],
                vec![DbValue::Text("b".into())],
            ],
        )
        .await
        .unwrap();
    assert_eq!(written, 2);
    let events = run(
        &*session,
        QueryRequest::read("select count(*) from notes", 0),
    )
    .await
    .unwrap();
    assert_eq!(rows(&events), [vec![DbValue::I64(4)]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn explain_draws_the_query_plan_and_refuses_analyze() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let explain = session.explain().unwrap();
    let plan = explain
        .explain(ExplainRequest::estimated(
            "select * from orders where customer_id = 1 order by total;",
        ))
        .await
        .unwrap();
    assert_eq!(plan.root.kind, "QUERY PLAN");
    let search = &plan.root.children[0];
    assert_eq!(search.kind, "SEARCH");
    assert_eq!(search.relation.as_deref(), Some("orders"));
    assert!(
        search
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("orders_customer")),
        "{search:?}"
    );
    assert!(
        plan.root
            .children
            .iter()
            .any(|node| node.kind == "USE TEMP B-TREE")
    );

    let analyze = explain
        .explain(ExplainRequest::analyzed("select * from orders"))
        .await
        .unwrap_err();
    assert_eq!(analyze.category(), DriverErrorCategory::Capability);
    let unavailable: Vec<_> = session
        .capabilities()
        .iter()
        .filter(|state| !state.available)
        .map(|state| state.capability)
        .collect();
    assert_eq!(
        unavailable,
        [
            Capability::Ddl,
            Capability::ExplainAnalyze,
            Capability::Admin
        ]
    );
    assert!(session.ddl().is_none() && session.admin().is_none());
}

/// The file is opened read-only, so SQLite refuses a write whatever route it takes: a
/// plain INSERT, a write into an ATTACHed file, a write after `PRAGMA query_only = 0`,
/// a VACUUM INTO, and the grid's own edits.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_connection_cannot_write() {
    let (dir, path) = seeded().await;
    let other = dir.path().join("other.db");
    seed(&other, "create table t (x); insert into t values (1);").await;
    let session = open(&path, true).await;

    let events = run(
        &*session,
        QueryRequest::read("select count(*) from orders", 0),
    )
    .await
    .unwrap();
    assert_eq!(rows(&events), [vec![DbValue::I64(3)]]);

    let refused = |error: dexo_driver_api::DriverError| {
        assert_eq!(error.category(), DriverErrorCategory::Permission, "{error}");
    };
    refused(
        run(
            &*session,
            QueryRequest::write("insert into notes (body) values ('x')"),
        )
        .await
        .unwrap_err(),
    );
    let attach = format!("attach database '{}' as other", other.display());
    run(&*session, QueryRequest::write(attach)).await.unwrap();
    refused(
        run(
            &*session,
            QueryRequest::write("insert into other.t values (2)"),
        )
        .await
        .unwrap_err(),
    );
    let _ = run(&*session, QueryRequest::write("pragma query_only = 0")).await;
    refused(
        run(&*session, QueryRequest::write("delete from orders"))
            .await
            .unwrap_err(),
    );
    let copy = dir.path().join("copy.db");
    refused(
        run(
            &*session,
            QueryRequest::write(format!("vacuum into '{}'", copy.display())),
        )
        .await
        .unwrap_err(),
    );
    assert!(!copy.exists());
    let uri = format!("attach database 'file:{}?mode=rw' as rw", other.display());
    assert!(run(&*session, QueryRequest::write(uri)).await.is_err());

    // The grid's edits do not pass the statement check; SQLite refuses them itself.
    let denied = session
        .data()
        .unwrap()
        .apply(&[Mutation::Insert {
            table: table("notes"),
            columns: vec![ColumnId("body".into())],
            values: vec![DbValue::Text("x".into())],
        }])
        .await
        .unwrap_err();
    refused(denied);
    let events = run(
        &*session,
        QueryRequest::read("select count(*) from notes", 0),
    )
    .await
    .unwrap();
    assert_eq!(rows(&events), [vec![DbValue::I64(2)]]);

    let missing = SqliteFactory
        .connect(request(&dir.path().join("absent.db"), true))
        .await
        .err()
        .expect("a missing file is not created read-only");
    assert_eq!(missing.category(), DriverErrorCategory::Configuration);
    assert!(!dir.path().join("absent.db").exists());
}

/// What MCP reads in: `begin(ReadOnly)` refuses writes until the transaction ends.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_transaction_refuses_writes_until_it_ends() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let transactions = session.transactions().unwrap();
    transactions.begin(TransactionMode::ReadOnly).await.unwrap();
    let error = run(&*session, QueryRequest::write("delete from notes"))
        .await
        .unwrap_err();
    assert_eq!(error.category(), DriverErrorCategory::Permission);
    transactions.rollback().await.unwrap();
    let events = run(&*session, QueryRequest::write("delete from notes"))
        .await
        .unwrap();
    assert!(events.contains(&QueryEvent::Finished {
        rows_affected: Some(2)
    }));

    transactions
        .begin(TransactionMode::ReadWrite)
        .await
        .unwrap();
    transactions.savepoint("before").await.unwrap();
    run(
        &*session,
        QueryRequest::write("insert into notes values ('x')"),
    )
    .await
    .unwrap();
    transactions.rollback_to("before").await.unwrap();
    transactions.release_savepoint("before").await.unwrap();
    transactions.commit().await.unwrap();
    let events = run(
        &*session,
        QueryRequest::read("select count(*) from notes", 0),
    )
    .await
    .unwrap();
    assert_eq!(rows(&events), [vec![DbValue::I64(0)]]);
}

/// A name with `/` in it still finds its children, and a keyless table whose column is
/// called `rowid` is keyed by `_rowid_`, the real row id, not by the user's column.
#[tokio::test]
async fn odd_names_keep_their_ids_and_their_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("odd.db");
    seed(
        &path,
        "CREATE TABLE \"a/b%\" (id INTEGER PRIMARY KEY, v TEXT);
         CREATE TABLE logs (rowid TEXT, body TEXT);
         INSERT INTO logs VALUES ('same', 'one'), ('same', 'two');",
    )
    .await;
    let session = open(&path, false).await;
    let catalog = session.catalog().unwrap();
    let options = CatalogListOptions::default();
    let main = catalog.list_children(None, &options).await.unwrap().objects;
    let tables = catalog
        .list_children(Some(&main[0].id), &options)
        .await
        .unwrap()
        .objects;
    let odd = tables
        .iter()
        .find(|object| object.qualified_name.object() == "a/b%")
        .expect("listed");
    let columns = catalog
        .list_children(Some(&odd.id), &options)
        .await
        .unwrap()
        .objects;
    let column = columns
        .iter()
        .find(|object| object.kind == ObjectKind::Column)
        .expect("a column");
    assert!(catalog.object(&column.id).await.unwrap().is_some());

    let data = session.data().unwrap();
    let keys = data.table_columns(&table("logs")).await.unwrap();
    assert_eq!(keys[0].name, "_rowid_");
    let page = data
        .fetch(DataRequest {
            object: table("logs"),
            columns: vec![],
            filter: None,
            sort: vec![],
            page: Page::new(0, 10).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows[0][0], DbValue::I64(1));
    assert_eq!(page.rows[1][0], DbValue::I64(2));
}
