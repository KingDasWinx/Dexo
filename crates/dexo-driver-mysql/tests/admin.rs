use dexo_driver_api::{
    AdminAction, AdminConfirmKind, ConnectRequest, ConnectionFactory, DriverErrorCategory,
    LockLevel, Page, QualifiedName,
};
use dexo_driver_mysql::{MysqlFactory, preview_mysql};
use secrecy::SecretString;

fn table() -> QualifiedName {
    QualifiedName::new(None::<String>, Some("dexo"), "items")
}

#[test]
fn preview_exact_commands_and_lock_risk() {
    let analyze = preview_mysql(&AdminAction::Analyze { target: table() }).unwrap();
    assert_eq!(analyze.command, "ANALYZE TABLE `dexo`.`items`");
    assert_eq!(analyze.lock_risk, LockLevel::Share);
    let optimize = preview_mysql(&AdminAction::Optimize { target: table() }).unwrap();
    assert!(optimize.command.contains("OPTIMIZE TABLE"));
    assert_eq!(optimize.lock_risk, LockLevel::Exclusive);
    let cancel = preview_mysql(&AdminAction::CancelQuery {
        session_id: "12".into(),
    })
    .unwrap();
    assert_eq!(cancel.command, "KILL QUERY 12");
    assert_eq!(cancel.confirmation, AdminConfirmKind::Once);
    let terminate = preview_mysql(&AdminAction::TerminateSession {
        session_id: "12".into(),
    })
    .unwrap();
    assert_eq!(terminate.confirmation, AdminConfirmKind::TypeTarget);
    assert!(preview_mysql(&AdminAction::Vacuum { target: table() }).is_err());
}

#[test]
fn admin_errors_are_not_retryable() {
    let error = preview_mysql(&AdminAction::Vacuum { target: table() }).unwrap_err();
    assert!(!error.is_retryable());
    assert_eq!(error.category(), DriverErrorCategory::Capability);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn sessions_sizes_stats_variables_and_restricted_role() {
    let pair = dexo_test_support::DatabasePair::start().await.unwrap();
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
    // The list leaves out the session that reads it: another one is there to show.
    let _other = MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    let admin = session.admin().unwrap();
    let sessions = admin.list_sessions().await.unwrap();
    assert!(
        sessions.items.iter().any(|info| info.client.is_some()),
        "a session says where it comes from: {:?}",
        sessions.items
    );
    assert!(!sessions.captured_at.is_empty());
    let sizes = admin.sizes(Page::new(0, 20).unwrap()).await.unwrap();
    assert!(
        sizes
            .items
            .iter()
            .all(|item| item.bytes.is_none() || item.native_size.is_some())
    );
    let stats = admin.statistics().await.unwrap();
    assert!(stats.items.iter().all(|item| !item.captured_at.is_empty()));
    let vars = admin.variables().await.unwrap();
    assert!(
        vars.items
            .iter()
            .any(|item| item.scope == dexo_driver_api::VariableScope::Session)
    );
    assert!(
        vars.items
            .iter()
            .any(|item| item.scope == dexo_driver_api::VariableScope::Server)
    );
    let missing = admin
        .execute_action(AdminAction::CancelQuery {
            session_id: "1".into(),
        })
        .await
        .unwrap();
    assert!(missing.ok);
    assert!(missing.idempotent_noop);

    let root = MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "root".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
        .await
        .unwrap();
    for sql in [
        "create user if not exists 'dexo_limited'@'%' identified by 'limited_test_only'",
        "alter user 'dexo_limited'@'%' identified by 'limited_test_only'",
        "grant select on dexo.* to 'dexo_limited'@'%'",
        "flush privileges",
    ] {
        drain(
            root.execute(dexo_driver_api::QueryRequest::write(sql))
                .await
                .unwrap(),
        )
        .await;
    }
    let limited = MysqlFactory
        .connect(ConnectRequest::new(
            pair.mysql_endpoint().to_string(),
            Some("dexo".into()),
            "dexo_limited".into(),
            SecretString::from("limited_test_only"),
            false,
        ))
        .await
        .unwrap();
    let limited_sessions = limited.admin().unwrap().list_sessions().await.unwrap();
    assert!(
        limited_sessions.restriction.is_some(),
        "restricted role must keep a safe reason"
    );
}

fn connect_as(
    pair: &dexo_test_support::DatabasePair,
    user: &str,
) -> impl std::future::Future<
    Output = Result<Box<dyn dexo_driver_api::Session>, dexo_driver_api::DriverError>,
> {
    MysqlFactory.connect(ConnectRequest::new(
        pair.mysql_endpoint().to_string(),
        Some("dexo".into()),
        user.into(),
        SecretString::from("dexo_test_only"),
        false,
    ))
}

async fn first_value(
    session: &dyn dexo_driver_api::Session,
    sql: &str,
) -> Option<dexo_driver_api::DbValue> {
    use futures_util::StreamExt;
    let mut stream = session
        .execute(dexo_driver_api::QueryRequest::read(sql, 1))
        .await
        .unwrap();
    while let Some(event) = stream.next().await {
        if let dexo_driver_api::QueryEvent::Rows(batch) = event.unwrap()
            && let Some(row) = batch.rows.into_iter().next()
        {
            return row.into_iter().next();
        }
    }
    None
}

async fn connection_id(session: &dyn dexo_driver_api::Session) -> String {
    match first_value(session, "select connection_id()").await {
        Some(dexo_driver_api::DbValue::U64(id)) => id.to_string(),
        Some(dexo_driver_api::DbValue::I64(id)) => id.to_string(),
        other => panic!("connection id read as {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_server_sees_dexo_as_the_program_behind_its_sessions() {
    use dexo_driver_api::DbValue;
    let pair = dexo_test_support::DatabasePair::start().await.unwrap();
    let root = connect_as(&pair, "root").await.unwrap();
    let program = first_value(
        root.as_ref(),
        "select attr_value from performance_schema.session_connect_attrs
         where processlist_id = connection_id() and attr_name = 'program_name'",
    )
    .await;
    let instrumented = first_value(root.as_ref(), "select @@performance_schema").await;
    if matches!(instrumented, Some(DbValue::I64(0) | DbValue::U64(0))) {
        // MariaDB ships with performance_schema off, and records no attributes then.
        assert_eq!(program, None);
    } else {
        assert_eq!(program, Some(DbValue::Text("dexo".into())));
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_lock_wait_names_the_processlist_ids_of_blocker_and_blocked() {
    let pair = dexo_test_support::DatabasePair::start().await.unwrap();
    let root = connect_as(&pair, "root").await.unwrap();
    let blocker = connect_as(&pair, "dexo").await.unwrap();
    let blocked = connect_as(&pair, "dexo").await.unwrap();
    for sql in [
        "create table if not exists lock_wait (id int primary key) engine = innodb",
        "insert ignore into lock_wait values (1)",
    ] {
        drain(
            blocker
                .execute(dexo_driver_api::QueryRequest::write(sql))
                .await
                .unwrap(),
        )
        .await;
    }
    let blocker_id = connection_id(blocker.as_ref()).await;
    let blocked_id = connection_id(blocked.as_ref()).await;
    let lock_row = "select id from lock_wait where id = 1 for update";
    blocker
        .transactions()
        .unwrap()
        .begin(dexo_driver_api::TransactionMode::ReadWrite)
        .await
        .unwrap();
    drain(
        blocker
            .execute(dexo_driver_api::QueryRequest::write(lock_row))
            .await
            .unwrap(),
    )
    .await;
    let waiter = tokio::spawn(async move {
        blocked
            .transactions()
            .unwrap()
            .begin(dexo_driver_api::TransactionMode::ReadWrite)
            .await
            .unwrap();
        drain(
            blocked
                .execute(dexo_driver_api::QueryRequest::write(lock_row))
                .await
                .unwrap(),
        )
        .await;
        blocked.transactions().unwrap().rollback().await.unwrap();
    });

    let admin = root.admin().unwrap();
    let mut graph = admin.blocking_graph().await.unwrap();
    for _ in 0..50 {
        if !graph.items.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        graph = admin.blocking_graph().await.unwrap();
    }
    assert!(
        graph
            .items
            .iter()
            .any(|edge| edge.blocker == blocker_id && edge.blocked == blocked_id),
        "expected {blocker_id} blocking {blocked_id}, got {:?}",
        graph.items
    );
    let locks = admin.list_locks().await.unwrap();
    assert!(locks.restriction.is_none(), "{:?}", locks.restriction);
    let held = |id: &str, granted: bool| {
        locks.items.iter().any(|lock| {
            lock.session_id == id
                && lock.granted == granted
                && lock
                    .relation
                    .as_deref()
                    .is_some_and(|relation| relation.contains("lock_wait"))
        })
    };
    assert!(
        held(&blocker_id, true) && held(&blocked_id, false),
        "expected {blocker_id} holding and {blocked_id} waiting, got {:?}",
        locks.items
    );

    // An account that may not read the lock tables gets a reason, not an error.
    let limited = connect_as(&pair, "dexo").await.unwrap();
    let limited = limited.admin().unwrap();
    assert!(limited.list_locks().await.unwrap().restriction.is_some());
    assert!(
        limited
            .blocking_graph()
            .await
            .unwrap()
            .restriction
            .is_some()
    );

    blocker.transactions().unwrap().rollback().await.unwrap();
    waiter.await.unwrap();
}

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    use futures_util::StreamExt;
    while stream.next().await.is_some() {}
}
