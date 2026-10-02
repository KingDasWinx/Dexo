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
    // A CREATE changes no rows; DuckDB's count of them is not one.
    assert!(events.contains(&QueryEvent::ResultSetFinished {
        index: 0,
        rows_affected: None,
        truncated: false
    }));
    // A write returning a column named like DuckDB's own count keeps it.
    let events = run(
        &*session,
        QueryRequest::write("insert into t2 values (5) returning x::BIGINT as \"Count\""),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["5"]]);
    let events = run(&*session, QueryRequest::write("drop table t2"))
        .await
        .unwrap();
    assert!(events.contains(&QueryEvent::Columns(Vec::new())));
}

/// Every value reads as DuckDB writes it cast to VARCHAR -- which DuckDB reads back as
/// the same value. Arrow's formatter wrote a struct as `{a: 1}`, strings in a list
/// unquoted, a UHUGEINT past 2^127 negative, and failed on an infinite date.
#[tokio::test(flavor = "multi_thread")]
async fn duckdb_types_read_as_duckdb_writes_them() {
    let session = DuckdbFactory
        .connect(request(":memory:", false))
        .await
        .unwrap();
    let values = [
        "true",
        "170141183460469231731687303715884105727::HUGEINT",
        "(-170141183460469231731687303715884105727)::HUGEINT - 1",
        "340282366920938463463374607431768211455::UHUGEINT",
        "'-123456789012345678901234567890'::BIGNUM",
        "1.50::DECIMAL(10,2)",
        "['a', 'a b', 'a,b', 'NULL', NULL, '', ' x', 'it''s', '[x]', 'a:b']",
        "{'a': 'x y', 'b': NULL, 'c': [1, 2]}",
        "MAP {'k': 'v,1', 'z': NULL}",
        "union_value(str := 'a b,c')::UNION(num INT, str VARCHAR)",
        "[1, 2]::INTEGER[2]",
        "'6ea0f862-6093-4e90-8e8d-7b3d3d9d0416'::UUID",
        "INTERVAL '1 year 2 months 3 days 04:05:06.5'",
        "INTERVAL '-36 hours'",
        "'2020-01-01 10:00:00.5+00'::TIMESTAMPTZ",
        "'2020-01-01 10:00:00.5'::TIMESTAMP",
        "'2020-01-01 10:00:00.123456789'::TIMESTAMP_NS",
        "'infinity'::TIMESTAMP",
        "'0044-03-15 (BC)'::DATE",
        "'12345-01-01'::DATE",
        "'-infinity'::DATE",
        "'12:00:00.25'::TIME",
        "'12:00:00+05:30'::TIMETZ",
        "'0101'::BIT",
        "3::UBIGINT",
        "'a'::ENUM('a', 'b')",
        "'{\"a\": 1}'::JSON",
        "['{\"a\": 1}'::JSON]",
        "0.1::FLOAT",
        "1e300::DOUBLE",
        "1e-7::DOUBLE",
        "'nan'::DOUBLE",
        "[1.5::DOUBLE, 1e16]",
        "['2020-01-01'::DATE, '0044-03-15 (BC)'::DATE]",
        "[{'a': 'x,y'}]",
    ];
    let select = values
        .iter()
        .map(|value| format!("{value}, ({value})::VARCHAR"))
        .collect::<Vec<_>>()
        .join(", ");
    let events = run(&*session, QueryRequest::read(format!("select {select}"), 0))
        .await
        .unwrap();
    let row = &rows(&events)[0];
    for (index, value) in values.iter().enumerate() {
        let read = match &row[index * 2] {
            DbValue::Null => "NULL".to_string(),
            DbValue::Bool(value) => value.to_string(),
            DbValue::I64(value) => value.to_string(),
            DbValue::U64(value) => value.to_string(),
            DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Json(text) => text.clone(),
            DbValue::Native { text, .. } => text.clone(),
            DbValue::Bytes(bytes) => format!("{bytes:?}"),
        };
        let DbValue::Text(written) = &row[index * 2 + 1] else {
            panic!("{value}: DuckDB wrote {:?}", row[index * 2 + 1]);
        };
        assert_eq!(&read, written, "{value}");
    }
    let QueryEvent::Columns(columns) = &events[1] else {
        panic!("columns follow the start: {events:?}");
    };
    assert_eq!(columns[2].type_name, "HUGEINT");
    assert_eq!(columns[12].type_name, "VARCHAR[]");
    assert_eq!(
        columns[14].type_name,
        "STRUCT(a VARCHAR, b INTEGER, c INTEGER[])"
    );
    assert_eq!(row[0], DbValue::Bool(true));
    assert_eq!(row[48], DbValue::U64(3));
    // A union holding a NULL member is NULL.
    let events = run(
        &*session,
        QueryRequest::read("select NULL::UNION(num INT, str VARCHAR)", 0),
    )
    .await
    .unwrap();
    assert_eq!(rows(&events), [vec![DbValue::Null]]);
}

/// A row of every awkward type is found again by the values it was read with: an edit
/// is keyed by them, and one that missed its row reported a conflict.
#[tokio::test(flavor = "multi_thread")]
async fn rows_of_every_type_are_found_by_the_values_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("types.duckdb");
    let session = open(&path, false).await;
    run(
        &*session,
        QueryRequest::write(
            "create table t (
                l VARCHAR[], s STRUCT(a INTEGER, b VARCHAR), m MAP(VARCHAR, INTEGER),
                u UNION(num INTEGER, str VARCHAR), z TIMETZ, h UHUGEINT, b BIGNUM,
                d DATE, ts TIMESTAMP, f DOUBLE, i INTERVAL, a INTEGER[2], e ENUM('x', 'y'));
             insert into t values (
                ['a, b', 'NULL', NULL], {'a': 1, 'b': 'x:y'}, MAP {'k': 1},
                union_value(str := 'a b'), '12:00:00+05:30',
                340282366920938463463374607431768211455, '-123456789012345678901234567890',
                'infinity', '2020-01-01 10:00:00.5', 0.1, INTERVAL '1 day -01:00:00',
                [1, 2], 'y')",
        ),
    )
    .await
    .unwrap();
    let data = session.data().unwrap();
    let page = data
        .fetch(DataRequest {
            object: QualifiedName::new(Some("types"), Some("main"), "t"),
            columns: Vec::new(),
            filter: None,
            sort: Vec::new(),
            page: Page::new(0, 10).unwrap(),
            clauses: Default::default(),
        })
        .await
        .unwrap();
    assert_eq!(page.columns[0].name, "rowid");
    assert_eq!(page.columns[12].type_name, "INTEGER[2]");
    let identity: Vec<(ColumnId, DbValue)> = page
        .columns
        .iter()
        .zip(&page.rows[0])
        .skip(1)
        .map(|(column, value)| (ColumnId(column.name.clone()), value.clone()))
        .collect();
    data.apply(&[Mutation::Delete {
        table: QualifiedName::new(Some("types"), Some("main"), "t"),
        identity,
        original: Vec::new(),
    }])
    .await
    .unwrap();
    let events = run(&*session, QueryRequest::read("select count(*) from t", 0))
        .await
        .unwrap();
    assert_eq!(texts(&events), [["0"]]);
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
    // DuckDB itself has the file read-only, whatever Dexo's own checks let through.
    let events = run(
        &*session,
        QueryRequest::read(
            "select readonly from duckdb_databases() where database_name = 'shop'",
            0,
        ),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["true"]]);
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
    // A read runs in a read-only transaction of its own, rolled back after it: none is
    // left open for the user's BEGIN to trip on.
    let read = || {
        let mut read = QueryRequest::read("select count(*) from notes", 0);
        read.read_only = true;
        read
    };
    assert_eq!(texts(&run(&*session, read()).await.unwrap()), [["2"]]);
    let transactions = session.transactions().unwrap();
    transactions
        .begin(TransactionMode::ReadWrite)
        .await
        .unwrap();
    // Inside the user's own, it runs as part of it, and sees what it has not committed.
    run(
        &*session,
        QueryRequest::write("insert into notes values ('third')"),
    )
    .await
    .unwrap();
    assert_eq!(texts(&run(&*session, read()).await.unwrap()), [["3"]]);
    transactions.rollback().await.unwrap();
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

/// DuckDB clears an interrupt as each statement begins: a cancel that came between two
/// statements of a script was lost, and the script ran to its end.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_script_runs_no_further_statement() {
    let session = DuckdbFactory
        .connect(request(":memory:", false))
        .await
        .unwrap();
    run(&*session, QueryRequest::write("create table t (i int)"))
        .await
        .unwrap();
    let script = (0..3000)
        .map(|i| format!("insert into t values ({i});"))
        .collect::<Vec<_>>()
        .join("\n");
    let query = QueryRequest::write(script);
    let id = query.id;
    let mut stream = session.execute(query).await.unwrap();
    let mut seen = 0;
    let mut cancelled = false;
    while let Some(event) = stream.next().await {
        match event {
            Ok(_) => {
                seen += 1;
                if seen == 50 {
                    session.cancel(id).await.unwrap();
                }
            }
            Err(error) => cancelled = error.category() == DriverErrorCategory::Cancelled,
        }
    }
    assert!(cancelled);
    let events = run(&*session, QueryRequest::read("select count(*) from t", 0))
        .await
        .unwrap();
    let count: u64 = texts(&events)[0][0].parse().unwrap();
    assert!(count < 3000, "{count} rows: the script ran on");
}

/// A caller that stops waiting for an EXPLAIN ANALYZE stops it: the session is free for
/// the next query at once.
#[tokio::test(flavor = "multi_thread")]
async fn an_abandoned_analyze_is_interrupted() {
    let session = DuckdbFactory
        .connect(request(":memory:", false))
        .await
        .unwrap();
    let slow = session.explain().unwrap().explain(ExplainRequest::analyzed(
        "select count(*) from range(100000000000) a",
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(300), slow)
            .await
            .is_err()
    );
    let started = Instant::now();
    let events = run(&*session, QueryRequest::read("select 1", 0))
        .await
        .unwrap();
    assert_eq!(texts(&events), [["1"]]);
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// Only a query or an INSERT, UPDATE or DELETE is analyzed: a COPY or a SET does what it
/// does outside the transaction rolled back after it.
#[tokio::test(flavor = "multi_thread")]
async fn analyze_runs_only_queries_and_row_changes() {
    let (dir, path) = seeded().await;
    let session = open(&path, false).await;
    let explain = session.explain().unwrap();
    let leak = dir.path().join("leak.csv");
    for sql in [
        format!("copy notes to '{}'", leak.display()),
        "set threads = 1".to_string(),
        "checkpoint".to_string(),
    ] {
        let refused = explain
            .explain(ExplainRequest::analyzed(sql.clone()))
            .await
            .unwrap_err();
        assert_eq!(refused.category(), DriverErrorCategory::Capability, "{sql}");
    }
    assert!(!leak.exists());
    explain
        .explain(ExplainRequest::analyzed("update notes set body = 'x'"))
        .await
        .unwrap();
}

/// DuckDB reads `[`, `*` and `?` in a file name as a glob: `s[1].csv` opened `s1.csv`.
#[tokio::test(flavor = "multi_thread")]
async fn a_data_file_is_the_file_its_path_names() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("s1.csv"), "n\n99\n").unwrap();
    for name in ["s[1].csv", "s?.csv", "s*.csv"] {
        let path = dir.path().join(name);
        std::fs::write(&path, "n\n1\n").unwrap();
        let session = open(&path, false).await;
        let view = name.split('.').next().unwrap();
        let events = run(
            &*session,
            QueryRequest::read(format!("select n from \"{view}\""), 0),
        )
        .await
        .unwrap();
        assert_eq!(texts(&events), [["1"]], "{name}");
    }
}

/// A COMMIT DuckDB refuses rolls the transaction back: the session is idle after it, not
/// failed with nothing open.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_commit_leaves_no_transaction() {
    let (_dir, path) = seeded().await;
    let first = open(&path, false).await;
    let second = open(&path, false).await;
    let (one, two) = (
        first.transactions().unwrap(),
        second.transactions().unwrap(),
    );
    one.begin(TransactionMode::ReadWrite).await.unwrap();
    two.begin(TransactionMode::ReadWrite).await.unwrap();
    run(
        &*first,
        QueryRequest::write("update customers set score = 1 where id = 1"),
    )
    .await
    .unwrap();
    let conflict = run(
        &*second,
        QueryRequest::write("update customers set score = 2 where id = 1"),
    )
    .await;
    one.commit().await.unwrap();
    let committed = two.commit().await;
    assert!(conflict.is_err() || committed.is_err());
    assert_eq!(two.state(), dexo_driver_api::TransactionState::Idle);
    two.begin(TransactionMode::ReadWrite).await.unwrap();
    two.rollback().await.unwrap();
}

/// A key keeps the case it was written in, `REFERENCES PARENT`, and DuckDB's names ignore
/// case: the table it points at is found by its own name, both ways, from any case.
#[tokio::test(flavor = "multi_thread")]
async fn keys_point_at_tables_whatever_case_they_were_written_in() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys.duckdb");
    let session = open(&path, false).await;
    run(
        &*session,
        QueryRequest::write(
            "create table parent (id int primary key); create table child (p int references PARENT (id))",
        ),
    )
    .await
    .unwrap();
    let catalog = session.catalog().unwrap();
    let id = |kind: &str, name: &str| {
        dexo_driver_api::ObjectId::new(format!("dk:{kind}:keys/main/{name}"))
    };
    assert_eq!(
        catalog.dependencies(&id("table", "child")).await.unwrap(),
        [id("table", "parent")]
    );
    assert_eq!(
        catalog.dependents(&id("table", "parent")).await.unwrap(),
        [id("table", "child")]
    );
    let keys = catalog
        .foreign_keys(&QualifiedName::new(Some("KEYS"), Some("MAIN"), "Parent"))
        .await
        .unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].to.object(), "parent");
    let children = catalog
        .list_children(Some(&id("table", "child")), &CatalogListOptions::default())
        .await
        .unwrap()
        .objects;
    let key = children
        .iter()
        .find(|object| object.attributes.contains_key("fk_table"))
        .unwrap();
    assert_eq!(key.attributes["fk_table"], "parent");
}

/// An import goes through DuckDB's appender, every value cast to its column, and lands
/// whole or not at all.
#[tokio::test(flavor = "multi_thread")]
async fn an_import_appends_every_row_or_none() {
    let session = DuckdbFactory
        .connect(request(":memory:", false))
        .await
        .unwrap();
    run(
        &*session,
        QueryRequest::write("create table imported (id int, price double, day date)"),
    )
    .await
    .unwrap();
    let table = QualifiedName::new(None::<String>, None::<String>, "imported");
    let columns = ["id", "price", "day"].map(String::from);
    let row = |id: usize, day: &str| {
        vec![
            DbValue::Text(id.to_string()),
            DbValue::Text("2.5".into()),
            DbValue::Text(day.into()),
        ]
    };
    let rows: Vec<_> = (0..1000).map(|id| row(id, "2020-01-02")).collect();
    let bulk = session.bulk().unwrap();
    assert_eq!(
        bulk.insert_batch(&table, &columns, &rows).await.unwrap(),
        1000
    );
    let mut broken = rows.clone();
    broken[500] = row(500, "not a date");
    bulk.insert_batch(&table, &columns, &broken)
        .await
        .unwrap_err();
    let events = run(
        &*session,
        QueryRequest::read("select count(*), sum(price), min(day) from imported", 0),
    )
    .await
    .unwrap();
    assert_eq!(texts(&events), [["1000", "2500.0", "2020-01-02"]]);
}
