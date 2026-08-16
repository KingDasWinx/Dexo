use dexo_driver_api::{
    AdminAction, AdminConfirmKind, ConnectRequest, ConnectionFactory, DriverErrorCategory,
    LockLevel, Page, QualifiedName,
};
use dexo_driver_postgres::{PostgresFactory, preview_postgres};
use secrecy::SecretString;

fn table() -> QualifiedName {
    QualifiedName::new(None::<String>, Some("public"), "items")
}

#[test]
fn preview_exact_commands_and_lock_risk() {
    let vacuum = preview_postgres(&AdminAction::Vacuum { target: table() }).unwrap();
    assert_eq!(vacuum.command, r#"VACUUM "public"."items""#);
    assert_eq!(vacuum.lock_risk, LockLevel::Share);
    let analyze = preview_postgres(&AdminAction::Analyze { target: table() }).unwrap();
    assert_eq!(analyze.command, r#"ANALYZE "public"."items""#);
    let reindex = preview_postgres(&AdminAction::Reindex { target: table() }).unwrap();
    assert_eq!(reindex.command, r#"REINDEX TABLE "public"."items""#);
    assert_eq!(reindex.lock_risk, LockLevel::AccessExclusive);
    let cancel = preview_postgres(&AdminAction::CancelQuery {
        session_id: "42".into(),
    })
    .unwrap();
    assert_eq!(cancel.command, "SELECT pg_cancel_backend(42)");
    assert_eq!(cancel.confirmation, AdminConfirmKind::Once);
    let terminate = preview_postgres(&AdminAction::TerminateSession {
        session_id: "99".into(),
    })
    .unwrap();
    assert_eq!(terminate.confirmation, AdminConfirmKind::TypeTarget);
    assert!(preview_postgres(&AdminAction::Optimize { target: table() }).is_err());
}

#[test]
fn admin_errors_are_not_retryable() {
    let error = preview_postgres(&AdminAction::Optimize { target: table() }).unwrap_err();
    assert!(!error.is_retryable());
    assert_eq!(error.category(), DriverErrorCategory::Capability);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn sessions_locks_sizes_stats_variables_and_blocker() {
    let pair = dexo_test_support::DatabasePair::start().await.unwrap();
    let connect = || {
        PostgresFactory.connect(ConnectRequest::new(
            pair.postgres_endpoint().to_string(),
            Some("dexo".into()),
            "dexo".into(),
            SecretString::from("dexo_test_only"),
            false,
        ))
    };
    let admin = connect().await.unwrap();
    let provider = admin.admin().unwrap();
    let created = admin
        .execute(dexo_driver_api::QueryRequest::write(
            "create table if not exists admin_lock (id int primary key)",
        ))
        .await
        .unwrap();
    drain(created).await;

    let sessions = provider.list_sessions().await.unwrap();
    assert!(!sessions.captured_at.is_empty());
    assert!(
        sessions
            .items
            .iter()
            .any(|session| session.id.parse::<i32>().is_ok())
    );

    let sizes = provider.sizes(Page::new(0, 10).unwrap()).await.unwrap();
    assert!(
        sizes.restriction.is_none()
            || sizes
                .items
                .iter()
                .all(|item| item.bytes.is_some() || item.native_size.is_some())
    );
    let stats = provider.statistics().await.unwrap();
    assert!(stats.items.iter().all(|item| !item.captured_at.is_empty()));
    let vars = provider.variables().await.unwrap();
    assert!(
        vars.items
            .iter()
            .any(|item| item.scope == dexo_driver_api::VariableScope::Server)
    );
    assert!(
        vars.items
            .iter()
            .any(|item| item.scope == dexo_driver_api::VariableScope::Session)
    );

    let missing = provider
        .execute_action(AdminAction::CancelQuery {
            session_id: "1".into(),
        })
        .await
        .unwrap();
    assert!(missing.idempotent_noop || missing.ok);

    let blocker = connect().await.unwrap();
    let blocked = connect().await.unwrap();
    drain(
        blocker
            .execute(dexo_driver_api::QueryRequest::write("begin"))
            .await
            .unwrap(),
    )
    .await;
    drain(
        blocker
            .execute(dexo_driver_api::QueryRequest::write(
                "lock table admin_lock in access exclusive mode",
            ))
            .await
            .unwrap(),
    )
    .await;
    let blocked_query = tokio::spawn(async move {
        let stream = blocked
            .execute(dexo_driver_api::QueryRequest::read(
                "select * from admin_lock",
                10,
            ))
            .await
            .unwrap();
        drain(stream).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let graph = provider.blocking_graph().await.unwrap();
    assert!(
        graph
            .items
            .iter()
            .any(|edge| !edge.blocker.is_empty() && !edge.blocked.is_empty()),
        "expected blocker/blocked edge, got {:?}",
        graph.items
    );
    drain(
        blocker
            .execute(dexo_driver_api::QueryRequest::write("rollback"))
            .await
            .unwrap(),
    )
    .await;
    let _ = blocked_query.await;

    let _ = admin
        .execute(dexo_driver_api::QueryRequest::write(
            "do $$ begin
               if not exists (select 1 from pg_roles where rolname = 'dexo_limited') then
                 create role dexo_limited login password 'limited_test_only';
               end if;
             end $$;",
        ))
        .await;
    let limited = PostgresFactory
        .connect(ConnectRequest::new(
            pair.postgres_endpoint().to_string(),
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

async fn drain(mut stream: dexo_driver_api::QueryStream) {
    use futures_util::StreamExt;
    while stream.next().await.is_some() {}
}
