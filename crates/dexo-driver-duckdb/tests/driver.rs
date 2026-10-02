use std::path::Path;
use std::time::{Duration, Instant};

use dexo_driver_api::{
    CatalogListOptions, ColumnId, ConnectRequest, ConnectionFactory, DataRequest, DbValue,
    DriverErrorCategory, ExplainRequest, Mutation, ObjectKind, Page, QualifiedName, QueryEvent,
    QueryRequest, Session, TransactionMode,
};
use dexo_driver_duckdb::DuckdbFactory;
use futures_util::StreamExt;
use secrecy::SecretString;

const SCHEMA: &str = "
    CREATE TABLE customers (id INTEGER PRIMARY KEY, name VARCHAR NOT NULL UNIQUE, score DOUBLE);
    CREATE TABLE orders (
        id INTEGER PRIMARY KEY,
        customer_id INTEGER NOT NULL REFERENCES customers (id),
        total DECIMAL(10,2)
    );
    CREATE INDEX orders_customer ON orders (customer_id);
    CREATE VIEW big_orders AS SELECT * FROM orders WHERE total > 100;
    CREATE TABLE notes (body VARCHAR);
    INSERT INTO customers VALUES (1, 'Ada', 9.5), (2, 'Grace', NULL), (3, 'Linus', 7.25);
    INSERT INTO orders VALUES (10, 1, 250.00), (11, 1, 20.50), (12, 3, 99.00);
    INSERT INTO notes VALUES ('first'), ('second');
";

fn request(endpoint: &str, read_only: bool) -> ConnectRequest {
    ConnectRequest::new(
        endpoint,
        None,
        String::new(),
        SecretString::from(String::new()),
        read_only,
    )
}

async fn open(path: &Path, read_only: bool) -> Box<dyn Session> {
    DuckdbFactory
        .connect(request(&path.to_string_lossy(), read_only))
        .await
        .expect("connect")
}

async fn seeded() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shop.duckdb");
    let session = open(&path, false).await;
    run(&*session, QueryRequest::write(SCHEMA)).await.unwrap();
    session.close().await.unwrap();
    (dir, path)
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

fn texts(events: &[QueryEvent]) -> Vec<Vec<String>> {
    rows(events)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| match cell {
                    DbValue::Null => "NULL".into(),
                    DbValue::Bool(value) => value.to_string(),
                    DbValue::I64(value) => value.to_string(),
                    DbValue::U64(value) => value.to_string(),
                    DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Json(text) => text,
                    DbValue::Bytes(bytes) => format!("{bytes:?}"),
                    DbValue::Native { text, .. } => text,
                })
                .collect()
        })
        .collect()
}

fn table(name: &str) -> QualifiedName {
    QualifiedName::new(Some("shop"), Some("main"), name)
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
    assert_eq!(columns[1].type_name, "VARCHAR");
    assert_eq!(columns[2].type_name, "DOUBLE");
    assert_eq!(
        rows(&events),
        [
            vec![
                DbValue::I64(1),
                DbValue::Text("Ada".into()),
                DbValue::Native {
                    type_name: "DOUBLE".into(),
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
    assert!(events.contains(&QueryEvent::ResultSetFinished {
        index: 0,
        rows_affected: None,
        truncated: true
    }));

    let missing = run(&*session, QueryRequest::read("select ?", 0))
        .await
        .unwrap_err();
    assert_eq!(missing.category(), DriverErrorCategory::Syntax);

    // Two statements, two result sets: DuckDB's own prepare would run the first unseen.
    let events = run(
        &*session,
        QueryRequest::write(
            "update orders set total = total + 1 where customer_id = 1; select count(*) as \"Count\" from orders",
        ),
    )
    .await
    .unwrap();
    assert!(events.contains(&QueryEvent::ResultSetFinished {
        index: 0,
        rows_affected: Some(2),
        truncated: false
    }));
    assert_eq!(texts(&events), [["3"]]);

    let events = run(&*session, QueryRequest::write("create table t2 (x int)"))
        .await
        .unwrap();
    assert!(events.contains(&QueryEvent::Columns(Vec::new())));
}

#[tokio::test(flavor = "multi_thread")]
async fn duckdb_types_read_as_duckdb_writes_them() {
    let session = DuckdbFactory
        .connect(request(":memory:", false))
        .await
        .unwrap();
    let events = run(
        &*session,
        QueryRequest::read(
            "select 170141183460469231731687303715884105727::HUGEINT h, 1.50::DECIMAL(10,2) d,
                    [1, NULL]::INTEGER[] l, {'a': 1} s, MAP {'k': 1} m,
                    '6ea0f862-6093-4e90-8e8d-7b3d3d9d0416'::UUID u, INTERVAL 1 DAY i,
                    '2020-01-01 10:00:00+00'::TIMESTAMPTZ tz, '2020-01-01 10:00:00.5'::TIMESTAMP ts,
                    '2020-01-02'::DATE dt, '\\xAA'::BLOB b, '0101'::BIT bits, 3::UBIGINT ub,
                    'a'::ENUM('a', 'b') e, '{\"a\": 1}'::JSON j, 0.1::FLOAT f",
            0,
        ),
    )
    .await
    .unwrap();
    let QueryEvent::Columns(columns) = &events[1] else {
        panic!("columns follow the start: {events:?}");
    };
    let types: Vec<&str> = columns
        .iter()
        .map(|column| column.type_name.as_str())
        .collect();
    assert_eq!(
        types,
        [
            "HUGEINT",
            "DECIMAL(10,2)",
            "INTEGER[]",
            "STRUCT(a INTEGER)",
            "MAP(VARCHAR, INTEGER)",
            "UUID",
            "INTERVAL",
            "TIMESTAMP WITH TIME ZONE",
            "TIMESTAMP",
            "DATE",
            "BLOB",
            "BIT",
            "UBIGINT",
            "ENUM",
            "JSON",
            "FLOAT"
        ]
    );
    let row = &rows(&events)[0];
    assert_eq!(
        row[0],
        DbValue::Decimal("170141183460469231731687303715884105727".into())
    );
    assert_eq!(row[1], DbValue::Decimal("1.50".into()));
    assert_eq!(
        row[5],
        DbValue::Text("6ea0f862-6093-4e90-8e8d-7b3d3d9d0416".into())
    );
    assert_eq!(row[10], DbValue::Bytes(vec![0xAA]));
    assert_eq!(row[12], DbValue::U64(3));
    assert_eq!(row[13], DbValue::Text("a".into()));
    assert_eq!(row[14], DbValue::Json("{\"a\": 1}".into()));
    let text = &texts(&events)[0];
    assert_eq!(text[2], "[1, NULL]");
    assert_eq!(text[3], "{a: 1}");
    assert_eq!(text[4], "{k: 1}");
    assert_eq!(text[7], "2020-01-01 10:00:00+00:00");
    assert_eq!(text[8], "2020-01-01 10:00:00.500");
    assert_eq!(text[9], "2020-01-02");
    assert_eq!(text[11], "0101");
    assert_eq!(text[15], "0.1");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_query_is_cancelled_promptly() {
    let session = DuckdbFactory
        .connect(request(":memory:", false))
        .await
        .unwrap();
    let query = QueryRequest::read("select count(*) from range(100000000000) a", 0);
    let id = query.id;
    let started = Instant::now();
    let mut stream = session.execute(query).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    session.cancel(id).await.unwrap();
    let mut cancelled = false;
    while let Some(event) = stream.next().await {
        if let Err(error) = event {
            cancelled = error.category() == DriverErrorCategory::Cancelled;
        }
    }
    assert!(cancelled);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_catalog_lists_databases_schemas_tables_columns_indexes_and_keys() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let catalog = session.catalog().unwrap();
    let options = CatalogListOptions::default();
    let top = catalog.list_children(None, &options).await.unwrap().objects;
    assert_eq!(top.len(), 1, "system and temp only on request: {top:?}");
    assert_eq!(top[0].qualified_name.object(), "shop");
    let schemas = catalog
        .list_children(Some(&top[0].id), &options)
        .await
        .unwrap()
        .objects;
    assert_eq!(schemas[0].qualified_name.object(), "main");
    let relations = catalog
        .list_children(Some(&schemas[0].id), &options)
        .await
        .unwrap()
        .objects;
    let names: Vec<(&str, &ObjectKind)> = relations
        .iter()
        .map(|object| (object.qualified_name.object(), &object.kind))
        .collect();
    assert_eq!(
        names,
        [
            ("big_orders", &ObjectKind::View),
            ("customers", &ObjectKind::Table),
            ("notes", &ObjectKind::Table),
            ("orders", &ObjectKind::Table),
        ]
    );
    let orders = relations
        .iter()
        .find(|object| object.qualified_name.object() == "orders")
        .unwrap();
    assert_eq!(orders.qualified_name, table("orders"));
    let children = catalog
        .list_children(Some(&orders.id), &options)
        .await
        .unwrap()
        .objects;
    let column = children
        .iter()
        .find(|object| object.qualified_name.object() == "orders.total")
        .unwrap();
    assert_eq!(column.attributes["type"], "DECIMAL(10,2)");
    assert!(
        children
            .iter()
            .any(|object| object.kind == ObjectKind::Index
                && object.qualified_name.object() == "orders_customer")
    );
    let key = children
        .iter()
        .find(|object| object.attributes.contains_key("fk_table"))
        .unwrap();
    assert_eq!(key.attributes["fk_table"], "customers");
    assert_eq!(
        key.attributes["fk_local"],
        serde_json::json!(["customer_id"])
    );
    assert_eq!(key.attributes["fk_referenced"], serde_json::json!(["id"]));

    let found = catalog.object(&column.id).await.unwrap().unwrap();
    assert_eq!(found.id, column.id);
    let ddl = catalog.ddl(&orders.id).await.unwrap().sql;
    assert!(ddl.contains("CREATE TABLE orders"), "{ddl}");
    assert!(ddl.contains("CREATE INDEX orders_customer"), "{ddl}");

    let keys = catalog.foreign_keys(&table("customers")).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].from, table("orders"));
    assert_eq!(keys[0].to_columns, ["id"]);
    let customers = relations
        .iter()
        .find(|object| object.qualified_name.object() == "customers")
        .unwrap();
    assert_eq!(
        catalog.dependents(&customers.id).await.unwrap(),
        std::slice::from_ref(&orders.id)
    );
    assert_eq!(
        catalog.dependencies(&orders.id).await.unwrap(),
        std::slice::from_ref(&customers.id)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rows_are_paged_and_edited_by_their_key_or_rowid() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let data = session.data().unwrap();

    let page = data
        .fetch(DataRequest {
            object: table("customers"),
            columns: Vec::new(),
            filter: None,
            sort: Vec::new(),
            page: Page::new(0, 2).unwrap(),
            clauses: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    assert!(page.has_more);
    assert_eq!(page.columns[0].name, "id");

    let keys = data.table_columns(&table("customers")).await.unwrap();
    assert!(keys[0].primary_key && keys[0].name == "id");
    assert!(keys[1].unique && !keys[1].primary_key);

    data.apply(&[Mutation::Update {
        table: table("customers"),
        identity: vec![(ColumnId("id".into()), DbValue::I64(2))],
        original: Vec::new(),
        changes: vec![(
            ColumnId("score".into()),
            DbValue::Native {
                type_name: "DOUBLE".into(),
                bytes: Vec::new(),
                text: "3.5".into(),
            },
        )],
    }])
    .await
    .unwrap();
    let events = run(
        &*session,
        QueryRequest::read("select score from customers where id = 2", 0),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["3.5"]]);

    // No primary key: the rowid keys the page, first.
    let notes = data
        .fetch(DataRequest {
            object: table("notes"),
            columns: Vec::new(),
            filter: None,
            sort: Vec::new(),
            page: Page::new(0, 10).unwrap(),
            clauses: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(notes.columns[0].name, "rowid");
    let keys = data.table_columns(&table("notes")).await.unwrap();
    assert_eq!(keys[0].name, "rowid");
    let first = notes.rows[0][0].clone();
    data.apply(&[Mutation::Delete {
        table: table("notes"),
        identity: vec![(ColumnId("rowid".into()), first)],
        original: Vec::new(),
    }])
    .await
    .unwrap();

    // A conflict leaves nothing behind.
    let conflict = data
        .apply(&[
            Mutation::Insert {
                table: table("notes"),
                columns: vec![ColumnId("body".into())],
                values: vec![DbValue::Text("third".into())],
            },
            Mutation::Delete {
                table: table("notes"),
                identity: vec![(ColumnId("body".into()), DbValue::Text("nope".into()))],
                original: Vec::new(),
            },
        ])
        .await
        .unwrap_err();
    assert_eq!(conflict.category(), DriverErrorCategory::Conflict);
    let events = run(&*session, QueryRequest::read("select body from notes", 0))
        .await
        .unwrap();
    assert_eq!(texts(&events), [["second"]]);

    let estimate = data.estimate_rows(&table("orders")).await.unwrap();
    assert_eq!(estimate, Some(3));
}

#[tokio::test(flavor = "multi_thread")]
async fn explain_draws_estimated_and_analyzed_plans() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let explain = session.explain().unwrap();
    let sql = "select c.name, sum(o.total) from orders o join customers c on c.id = o.customer_id group by c.name";
    let plan = explain
        .explain(ExplainRequest::estimated(sql))
        .await
        .unwrap();
    fn kinds(node: &dexo_driver_api::PlanNode, out: &mut Vec<String>) {
        out.push(node.kind.clone());
        for child in &node.children {
            kinds(child, out);
        }
    }
    let mut found = Vec::new();
    kinds(&plan.root, &mut found);
    assert!(found.iter().any(|kind| kind == "HASH_JOIN"), "{found:?}");
    assert!(found.iter().any(|kind| kind == "SEQ_SCAN"), "{found:?}");

    let analyzed = explain
        .explain(ExplainRequest::analyzed(sql))
        .await
        .unwrap();
    assert!(analyzed.execution_ms.is_some());
    assert!(analyzed.root.actual.rows.is_some());

    // ANALYZE runs a write, and leaves nothing of it.
    explain
        .explain(ExplainRequest::analyzed("delete from notes"))
        .await
        .unwrap();
    let events = run(
        &*session,
        QueryRequest::read("select count(*) from notes", 0),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["2"]]);

    // Inside the user's transaction there is nothing to undo it with.
    let transactions = session.transactions().unwrap();
    transactions
        .begin(TransactionMode::ReadWrite)
        .await
        .unwrap();
    let refused = explain
        .explain(ExplainRequest::analyzed("delete from notes"))
        .await
        .unwrap_err();
    assert_eq!(refused.category(), DriverErrorCategory::Capability);
    transactions.rollback().await.unwrap();

    for request in [
        ExplainRequest::estimated("select * from orders where id = ?"),
        ExplainRequest::analyzed("select * from orders where id = $1"),
    ] {
        let refused = explain.explain(request).await.unwrap_err();
        assert_eq!(refused.category(), DriverErrorCategory::Capability);
        assert!(refused.to_string().contains("parameters"), "{refused}");
    }

    let hypothetical = explain
        .explain(ExplainRequest::with_indexes(
            sql,
            vec!["create index on orders (total)".into()],
        ))
        .await
        .unwrap_err();
    assert_eq!(hypothetical.category(), DriverErrorCategory::Capability);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_connection_cannot_write_the_file_or_anything_else() {
    let (dir, path) = seeded().await;
    let session = open(&path, true).await;
    let out = dir.path().join("out.csv");
    for sql in [
        "insert into notes values ('x')".to_string(),
        format!("copy notes to '{}'", out.display()),
        format!(
            "attach '{}' as other",
            dir.path().join("other.duckdb").display()
        ),
    ] {
        let error = run(&*session, QueryRequest::write(sql.clone()))
            .await
            .unwrap_err();
        assert_eq!(error.category(), DriverErrorCategory::Permission, "{sql}");
    }
    assert!(!out.exists());
    let refused = session
        .data()
        .unwrap()
        .apply(&[Mutation::Insert {
            table: table("notes"),
            columns: vec![ColumnId("body".into())],
            values: vec![DbValue::Text("x".into())],
        }])
        .await
        .unwrap_err();
    assert_eq!(refused.category(), DriverErrorCategory::Permission);
    let events = run(
        &*session,
        QueryRequest::read("select count(*) from notes", 0),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["2"]]);

    let missing = DuckdbFactory
        .connect(request(
            &dir.path().join("nope.duckdb").to_string_lossy(),
            true,
        ))
        .await
        .err()
        .unwrap();
    assert_eq!(missing.category(), DriverErrorCategory::Configuration);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_request_cannot_write() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let mut write = QueryRequest::read("delete from notes", 0);
    write.read_only = true;
    let error = run(&*session, write).await.unwrap_err();
    assert_eq!(error.category(), DriverErrorCategory::Permission);
    // A function that writes is refused by DuckDB's read-only transaction.
    let mut read = QueryRequest::read("select count(*) from notes", 0);
    read.read_only = true;
    assert_eq!(texts(&run(&*session, read).await.unwrap()), [["2"]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_csv_file_opens_as_a_view_and_is_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("sales.csv");
    std::fs::write(&csv, "id,amount\n1,2.5\n2,3\n").unwrap();
    let session = open(&csv, false).await;
    let events = run(
        &*session,
        QueryRequest::read("select * from sales order by id", 0),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["1", "2.5"], ["2", "3.0"]]);
    let error = run(
        &*session,
        QueryRequest::write(format!("copy (select 1) to '{}'", csv.display())),
    )
    .await
    .unwrap_err();
    assert_eq!(error.category(), DriverErrorCategory::Permission);
    assert_eq!(
        std::fs::read_to_string(&csv).unwrap(),
        "id,amount\n1,2.5\n2,3\n"
    );
    let missing = DuckdbFactory
        .connect(request(
            &dir.path().join("gone.parquet").to_string_lossy(),
            false,
        ))
        .await
        .err()
        .unwrap();
    assert_eq!(missing.category(), DriverErrorCategory::Configuration);
}

#[tokio::test(flavor = "multi_thread")]
async fn transactions_commit_and_roll_back() {
    let (_dir, path) = seeded().await;
    let session = open(&path, false).await;
    let transactions = session.transactions().unwrap();
    transactions
        .begin(TransactionMode::ReadWrite)
        .await
        .unwrap();
    run(&*session, QueryRequest::write("delete from notes"))
        .await
        .unwrap();
    transactions.rollback().await.unwrap();
    let events = run(
        &*session,
        QueryRequest::read("select count(*) from notes", 0),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["2"]]);

    transactions.begin(TransactionMode::ReadOnly).await.unwrap();
    let error = run(&*session, QueryRequest::write("delete from notes"))
        .await
        .unwrap_err();
    assert_eq!(error.category(), DriverErrorCategory::Permission);
    transactions.rollback().await.unwrap();
    assert!(transactions.savepoint("a").await.is_err());
}

/// DuckDB ends a `--` comment at a bare carriage return. Read to the next line feed, a
/// statement after it went unchecked: a COPY replaced the CSV a read-only session had
/// open, and an estimated EXPLAIN ran it.
#[tokio::test(flavor = "multi_thread")]
async fn a_carriage_return_hides_no_statement() {
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("sales.csv");
    std::fs::write(&csv, "id\n1\n").unwrap();
    let out = dir.path().join("out.csv");
    let hidden = |target: &Path| {
        format!(
            "select 1 --\r; copy (select 42) to '{}' --\n",
            target.display()
        )
    };

    let session = open(&csv, false).await;
    let refused = run(&*session, QueryRequest::write(hidden(&csv)))
        .await
        .unwrap_err();
    assert_eq!(refused.category(), DriverErrorCategory::Permission);
    assert_eq!(std::fs::read_to_string(&csv).unwrap(), "id\n1\n");

    let (_seeded, path) = seeded().await;
    let writable = open(&path, false).await;
    let read_only = open(&path, true).await;
    run(&*read_only, QueryRequest::write(hidden(&out)))
        .await
        .unwrap_err();
    let mut asked = QueryRequest::read(hidden(&out), 0);
    asked.read_only = true;
    run(&*writable, asked).await.unwrap_err();
    writable
        .explain()
        .unwrap()
        .explain(ExplainRequest::estimated(hidden(&out)))
        .await
        .unwrap_err();
    assert!(!out.exists());

    // Where writing is allowed, the two statements are two result sets, as DuckDB reads them.
    let events = run(&*writable, QueryRequest::write("select 1 --\r; select 2"))
        .await
        .unwrap();
    assert_eq!(texts(&events), [["1"], ["2"]]);
}

/// A second engine on a file deleted, as it closed, the write-ahead log the first was
/// still writing to. Every session on a file shares one database.
#[tokio::test(flavor = "multi_thread")]
async fn sessions_on_one_file_share_its_database() {
    let (_dir, path) = seeded().await;
    let first = open(&path, false).await;
    run(
        &*first,
        QueryRequest::write("insert into notes values ('third')"),
    )
    .await
    .unwrap();
    let wal = path.with_extension("duckdb.wal");
    assert!(wal.exists(), "the insert is in the log");
    let second = open(&path, false).await;
    let events = run(
        &*second,
        QueryRequest::read("select count(*) from notes", 0),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["3"]]);
    second.close().await.unwrap();
    assert!(
        wal.exists(),
        "closing one session leaves the log to the database"
    );

    // Read-only shares a database open for writing; writing cannot share a read-only one.
    let reader = open(&path, true).await;
    let refused = run(&*reader, QueryRequest::write("delete from notes"))
        .await
        .unwrap_err();
    assert_eq!(refused.category(), DriverErrorCategory::Permission);
    drop(reader);
    first.close().await.unwrap();
    let reader = open(&path, true).await;
    let writer = DuckdbFactory
        .connect(request(&path.to_string_lossy(), false))
        .await
        .err()
        .unwrap();
    assert_eq!(writer.category(), DriverErrorCategory::Configuration);
    // A read-only database still profiles a query, and fetches no extension by itself.
    reader
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("select * from notes"))
        .await
        .unwrap();
    let events = run(
        &*reader,
        QueryRequest::read("select current_setting('autoinstall_known_extensions')", 0),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["false"]]);
    let refused = run(
        &*reader,
        QueryRequest::write("set autoinstall_known_extensions = true"),
    )
    .await
    .unwrap_err();
    assert_eq!(refused.category(), DriverErrorCategory::Permission);
}
