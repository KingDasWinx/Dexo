use dexo_secrets::{MemorySecretStore, SecretStore};
use dexo_storage::{ConnectionRepository, Database, export_portable, import_portable};
use secrecy::ExposeSecret;

fn export_fixture_with_secret(secret: &str) -> String {
    let store = MemorySecretStore::default();
    store.put("secret-123", secret).unwrap();
    let db = Database::open_in_memory().unwrap();
    let repo = ConnectionRepository::new(db.connection());
    repo.save(&dexo_app::ConnectionProfile::new(
        dexo_app::ConnectionId(uuid::Uuid::new_v4()),
        None,
        "local-pg",
        "postgres",
        "local",
        serde_json::json!({"host":"localhost","port":5432}),
        dexo_app::SecretRef::new("secret-123".into()),
    ))
    .unwrap();
    let output = export_portable(db.connection()).unwrap();
    assert_eq!(
        store.get("secret-123").unwrap().unwrap().expose_secret(),
        secret
    );
    output
}

#[test]
fn exported_config_contains_reference_not_secret() {
    let output = export_fixture_with_secret("SUPER_SECRET_SENTINEL");
    assert!(output.contains("secret_ref"));
    assert!(!output.contains("SUPER_SECRET_SENTINEL"));
}

#[test]
fn import_generates_fresh_secret_refs_and_asks_for_secrets() {
    let db = Database::open_in_memory().unwrap();
    let exported = export_fixture_with_secret("SUPER_SECRET_SENTINEL");
    let report = import_portable(db.connection(), &exported).unwrap();
    assert_eq!(report.connections_needing_secret, vec!["local-pg"]);
    let loaded = ConnectionRepository::new(db.connection()).list().unwrap();
    assert_eq!(loaded.len(), 1);
    assert_ne!(loaded[0].secret_ref.as_str(), "secret-123");
    assert!(!loaded[0].secret_ref.as_str().is_empty());
    let dumped = format!("{loaded:?}");
    assert!(!dumped.contains("SUPER_SECRET_SENTINEL"));
}
