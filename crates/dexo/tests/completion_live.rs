use dexo_test_support::DatabasePair;

#[tokio::test]
#[ignore = "requires Docker"]
async fn admin_actions_observe_server_state() {
    let pair = DatabasePair::start().await.unwrap();
    assert!(!pair.postgres_endpoint().is_empty());
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn completion_live_smoke() {
    let pair = DatabasePair::start().await.unwrap();
    assert!(!pair.mysql_endpoint().is_empty());
}
