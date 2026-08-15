use assert_cmd::Command;
use dexo_app::{ConnectionId, ConnectionProfile, SecretRef};
use dexo_secrets::{KeyringSecretStore, SecretStore};
use dexo_storage::{AppPaths, ConnectionRepository, Database};
use dexo_test_support::DatabasePair;
use predicates::prelude::*;

#[tokio::test]
#[ignore = "requires Docker"]
async fn jsonl_query_keeps_diagnostics_off_stdout() {
    let pair = DatabasePair::start().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::from_data_home(dir.path().to_path_buf());
    let db = Database::open(&paths.database).unwrap();
    let (host, port) = pair.postgres_endpoint().rsplit_once(':').unwrap();
    let secret_ref = uuid::Uuid::new_v4().to_string();
    let profile = ConnectionProfile::new(
        ConnectionId(uuid::Uuid::new_v4()),
        None,
        "fixture",
        "postgres",
        "local",
        serde_json::json!({
            "host": host,
            "port": port.parse::<u16>().unwrap(),
            "database": "dexo",
            "username": "dexo"
        }),
        SecretRef::new(secret_ref.clone()),
    );
    ConnectionRepository::new(db.connection())
        .save(&profile)
        .unwrap();
    KeyringSecretStore
        .put(&secret_ref, "dexo_test_only")
        .unwrap();
    let result = Command::cargo_bin("dexo")
        .unwrap()
        .env("DEXO_DATA_HOME", dir.path())
        .args([
            "query",
            "--connection",
            "fixture",
            "--sql",
            "select 1 as n",
            "--format",
            "jsonl",
            "--non-interactive",
        ])
        .assert()
        .try_success();
    let _ = KeyringSecretStore.delete(&secret_ref);
    result
        .unwrap()
        .stdout("{\"n\":1}\n")
        .stderr(predicate::str::is_empty());
}
