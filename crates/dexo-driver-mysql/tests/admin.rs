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
        .connect(ConnectRequest {
            endpoint: pair.mysql_endpoint().to_string(),
            database: Some("dexo".into()),
            username: "dexo".into(),
            secret: SecretString::from("dexo_test_only"),
            read_only: false,
        })
        .await
        .unwrap();
    let admin = session.admin().unwrap();
    let sessions = admin.list_sessions().await.unwrap();
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
        .connect(ConnectRequest {
            endpoint: pair.mysql_endpoint().to_string(),
            database: Some("dexo".into()),
            username: "root".into(),
            secret: SecretString::from("dexo_test_only"),
            read_only: false,
        })
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
        .connect(ConnectRequest {
            endpoint: pair.mysql_endpoint().to_string(),
            database: Some("dexo".into()),
            username: "dexo_limited".into(),
            secret: SecretString::from("limited_test_only"),
            read_only: false,
        })
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
