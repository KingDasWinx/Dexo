use dexo_driver_mysql::{parse_explain_json, parse_explain_tree};

#[test]
fn json_and_tree_goldens_and_unavailable_metrics() {
    let scan = parse_explain_json(include_str!("fixtures/explain/scan.json")).unwrap();
    assert_eq!(scan.root.kind, "Table scan");
    assert_eq!(scan.root.relation.as_deref(), Some("items"));
    assert_eq!(scan.root.estimates.rows, Some(1000.0));
    assert!(
        scan.root.actual.rows.is_none(),
        "json actual must not be zeroed"
    );
    assert!(scan.root.loops.is_none());

    let join = parse_explain_json(include_str!("fixtures/explain/join.json")).unwrap();
    assert_eq!(join.root.kind, "Nested loop");
    assert_eq!(join.root.children.len(), 2);
    assert_eq!(join.root.children[0].relation.as_deref(), Some("orders"));
    assert_eq!(join.root.children[1].kind, "Index lookup");

    let tree = parse_explain_tree(include_str!("fixtures/explain/tree.txt")).unwrap();
    assert_eq!(tree.root.kind, "Sort");
    assert_eq!(tree.root.detail.as_deref(), Some("items.name"));
    assert_eq!(tree.root.children[0].kind, "Table scan");
    assert_eq!(tree.root.children[0].relation.as_deref(), Some("items"));
    assert!(tree.root.actual.time_ms.is_none());

    let analyzed = parse_explain_tree(include_str!("fixtures/explain/tree_analyze.txt")).unwrap();
    assert_eq!(analyzed.root.kind, "Aggregate");
    assert_eq!(analyzed.root.detail.as_deref(), Some("count(0)"));
    assert_eq!(analyzed.root.actual.rows, Some(10.0));
    assert_eq!(analyzed.root.loops, Some(1));
    assert!(analyzed.root.actual.time_ms.is_some());
    assert!(analyzed.root.children[0].actual.rows.is_some());
}

#[test]
fn capability_fallback_prefers_json_then_tree() {
    use dexo_driver_mysql::{MysqlExplainCaps, NativeExplainFormat, select_format};
    let full = MysqlExplainCaps::mysql();
    assert_eq!(
        select_format(false, full).unwrap(),
        NativeExplainFormat::Json
    );
    assert_eq!(
        select_format(true, full).unwrap(),
        NativeExplainFormat::Tree
    );
    // MariaDB has no FORMAT=TREE; its EXPLAIN ANALYZE is ANALYZE FORMAT=JSON.
    let mariadb = MysqlExplainCaps::mariadb();
    assert_eq!(
        select_format(true, mariadb).unwrap(),
        NativeExplainFormat::Json
    );
    assert!(
        dexo_driver_mysql::wrap_explain("select 1", NativeExplainFormat::Json, true)
            .starts_with("ANALYZE FORMAT=JSON")
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn container_explain_json_and_analyze_tree() {
    use dexo_driver_api::{ConnectRequest, ConnectionFactory, ExplainRequest};
    use dexo_driver_mysql::MysqlFactory;
    use dexo_test_support::DatabasePair;
    use secrecy::SecretString;

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
    let estimated = session
        .explain()
        .unwrap()
        .explain(ExplainRequest::estimated("select 1"))
        .await
        .unwrap();
    assert!(!estimated.raw.is_empty());
    let analyzed = session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("select 1"))
        .await
        .unwrap();
    assert!(
        analyzed.raw.contains("->")
            || analyzed.root.loops.is_some()
            || !analyzed.root.kind.is_empty()
    );
    // A `?` is a syntax error inside an EXPLAIN; it is refused as what it is.
    for request in [
        ExplainRequest::estimated("select ? + 1"),
        ExplainRequest::analyzed("select ? + 1"),
    ] {
        let refused = session
            .explain()
            .unwrap()
            .explain(request)
            .await
            .unwrap_err();
        assert_eq!(
            refused.category(),
            dexo_driver_api::DriverErrorCategory::Capability
        );
        assert!(refused.to_string().contains("parameters"), "{refused}");
    }
}

/// Every table read shows in the plan, the ones a subquery, a derived table or a CTE
/// reads too.
#[tokio::test]
#[ignore = "requires Docker"]
async fn subqueries_derived_tables_and_ctes_keep_their_tables() {
    use dexo_driver_api::{ConnectRequest, ConnectionFactory, ExplainRequest, PlanNode};
    use dexo_driver_mysql::MysqlFactory;
    use dexo_test_support::DatabasePair;
    use futures_util::StreamExt;
    use secrecy::SecretString;

    fn relations(node: &PlanNode, out: &mut Vec<String>) {
        out.extend(node.relation.clone());
        for child in &node.children {
            relations(child, out);
        }
    }
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
    for sql in [
        "create table c (id int primary key, name varchar(20))",
        "create table o (id int primary key, customer_id int, total int, key (customer_id))",
        "insert into c values (1, 'a'), (2, 'b'), (3, 'c')",
        "insert into o values (1, 1, 10), (2, 1, 20), (3, 2, 30), (4, 3, 40)",
        "analyze table c, o",
    ] {
        let mut stream = session
            .execute(dexo_driver_api::QueryRequest::write(sql))
            .await
            .unwrap();
        while let Some(event) = stream.next().await {
            event.unwrap();
        }
    }
    let queries = [
        "select c.id, (select count(*) from o where o.customer_id = c.id) n from c",
        "select * from (select customer_id, sum(total) s from o group by customer_id) t \
         join c on c.id = t.customer_id",
        "with t as (select customer_id, sum(total) s from o group by customer_id) \
         select * from t join c on c.id = t.customer_id",
    ];
    let check = |plan: dexo_driver_api::ExplainPlan, sql: &str| {
        let mut found = Vec::new();
        relations(&plan.root, &mut found);
        assert!(
            found.iter().any(|name| name == "o") && found.iter().any(|name| name == "c"),
            "{sql}: {found:?}"
        );
    };
    for sql in queries {
        let plan = session
            .explain()
            .unwrap()
            .explain(ExplainRequest::estimated(sql))
            .await
            .unwrap();
        check(plan, sql);
    }
    // MySQL 8.3 on can answer in its second JSON format, which came out empty.
    // MariaDB has no such setting.
    let mut stream = session
        .execute(dexo_driver_api::QueryRequest::write(
            "set explain_json_format_version = 2",
        ))
        .await
        .unwrap();
    let mut second_format = true;
    while let Some(event) = stream.next().await {
        second_format &= event.is_ok();
    }
    if second_format {
        for sql in queries {
            let plan = session
                .explain()
                .unwrap()
                .explain(ExplainRequest::estimated(sql))
                .await
                .unwrap();
            assert!(plan.raw.contains("\"operation\""), "{}", plan.raw);
            check(plan, sql);
        }
    }
}

async fn connect(
    pair: &dexo_test_support::DatabasePair,
    user: &str,
) -> Box<dyn dexo_driver_api::Session> {
    use dexo_driver_api::{ConnectRequest, ConnectionFactory};
    dexo_driver_mysql::MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            user.into(),
            secrecy::SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap()
}

async fn run(session: &dyn dexo_driver_api::Session, sql: &str) -> Vec<String> {
    use futures_util::StreamExt;
    let mut stream = session
        .execute(dexo_driver_api::QueryRequest::write(sql))
        .await
        .unwrap();
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

/// The rollback after EXPLAIN ANALYZE undoes nothing in a table without transactions:
/// a DELETE, an UPDATE or an INSERT ... SELECT explained that way stayed for good. Such
/// a write is refused before it runs, and one a trigger may carry there too.
#[tokio::test]
#[ignore = "requires Docker"]
async fn analyze_refuses_a_write_no_rollback_undoes() {
    use dexo_driver_api::{DriverErrorCategory, ExplainRequest};

    let pair = dexo_test_support::DatabasePair::start().await.unwrap();
    let session = connect(&pair, "dexo").await;
    // Binary logging keeps a trigger to an account with SUPER.
    let root = connect(&pair, "root").await;
    let mariadb = run(&*session, "select version()").await[0].contains("MariaDB");
    let mut engines = vec!["MyISAM", "MEMORY"];
    if mariadb {
        engines.push("Aria");
    }
    for engine in engines {
        run(&*session, "drop table if exists mi").await;
        run(
            &*session,
            &format!("create table mi (id int primary key, v int) engine = {engine}"),
        )
        .await;
        run(&*session, "insert into mi values (1, 1), (2, 2)").await;
        for sql in [
            "delete from mi where id = 1",
            "delete mi from mi where id = 1",
            "update mi set v = 99",
            "insert into mi select id + 10, v from mi",
            "insert into dexo.mi values (3, 3)",
        ] {
            let refused = session
                .explain()
                .unwrap()
                .explain(ExplainRequest::analyzed(sql))
                .await
                .unwrap_err();
            assert_eq!(refused.category(), DriverErrorCategory::Capability, "{sql}");
            assert!(refused.to_string().contains("`mi`"), "{sql}: {refused}");
        }
        assert_eq!(
            run(&*session, "select count(*), sum(v) from mi").await,
            ["I64(2)", "Decimal(\"3\")"],
            "{engine}"
        );
    }

    // A transactional table still runs, and is rolled back.
    run(
        &*session,
        "create table ii (id int primary key, v int) engine = InnoDB",
    )
    .await;
    run(&*session, "insert into ii values (1, 1), (2, 2)").await;
    let analyzed = session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("delete ii from ii where id = 1"))
        .await;
    assert!(analyzed.is_ok(), "{analyzed:?}");
    assert_eq!(run(&*session, "select count(*) from ii").await, ["I64(2)"]);

    // A trigger may write where no rollback reaches, while such a table exists.
    let trigger = "create trigger ii_log after delete on ii for each row insert into mi values (old.id + 100, 0)";
    run(&*root, trigger).await;
    let refused = session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("delete ii from ii where id = 1"))
        .await
        .unwrap_err();
    assert!(refused.to_string().contains("triggers"), "{refused}");
    assert_eq!(run(&*session, "select count(*) from mi").await, ["I64(2)"]);
    run(&*root, "drop trigger ii_log").await;
    run(&*session, "drop table mi").await;
    run(
        &*session,
        "create table mi (id int primary key, v int) engine = InnoDB",
    )
    .await;
    run(&*root, trigger).await;
    session
        .explain()
        .unwrap()
        .explain(ExplainRequest::analyzed("delete ii from ii where id = 1"))
        .await
        .unwrap();
    assert_eq!(run(&*session, "select count(*) from mi").await, ["I64(0)"]);
    assert_eq!(run(&*session, "select count(*) from ii").await, ["I64(2)"]);
}
