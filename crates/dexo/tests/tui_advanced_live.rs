use dexo_test_support::DatabasePair;

#[tokio::test]
#[ignore = "requires Docker"]
async fn advanced_postgres_and_mysql_operations() {
    let pair = DatabasePair::start().await.unwrap();
    assert!(!pair.postgres_endpoint().is_empty());
    assert!(!pair.mysql_endpoint().is_empty());
}
