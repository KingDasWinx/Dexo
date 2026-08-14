use dexo_secrets::{KeyringSecretStore, MemorySecretStore, SecretStore};
use secrecy::ExposeSecret;

#[test]
fn secret_round_trip_and_delete() {
    let store = MemorySecretStore::default();
    store.put("conn-1", "hunter2").unwrap();
    assert_eq!(
        store.get("conn-1").unwrap().unwrap().expose_secret(),
        "hunter2"
    );
    store.delete("conn-1").unwrap();
    assert!(store.get("conn-1").unwrap().is_none());
}

#[test]
fn keyring_store_implements_secret_store() {
    fn assert_store<T: SecretStore>() {}
    assert_store::<KeyringSecretStore>();
    assert_store::<MemorySecretStore>();
}
