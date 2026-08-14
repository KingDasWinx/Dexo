use dexo_secrets::{KeyringSecretStore, MemorySecretStore, SecretError, SecretStore};
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

#[test]
fn missing_or_locked_keychain_asks_for_session_secret() {
    let message = SecretError::Unavailable.to_string();
    assert!(message.contains("keychain is unavailable or locked"));
    assert!(message.contains("enter the secret for this session"));
    assert!(!message.to_ascii_lowercase().contains("file vault"));
}
