//! The SSH key file of a connection is the key the tunnel authenticates with; the agent was
//! used whatever the field said.
use std::path::Path;

use dexo_transport::{SshAuth, key_needs_passphrase};
use secrecy::SecretString;

const ENCRYPTED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/encrypted_ed25519"
);
const PLAIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plain_ed25519");

#[test]
fn only_an_encrypted_key_asks_for_a_passphrase() {
    assert!(key_needs_passphrase(Path::new(ENCRYPTED)));
    assert!(!key_needs_passphrase(Path::new(PLAIN)));
    assert!(!key_needs_passphrase(Path::new("/no/such/key")));
}

#[test]
fn a_key_file_becomes_the_authentication() {
    assert!(matches!(
        SshAuth::from_key_file(Path::new(PLAIN), None),
        Ok(SshAuth::PrivateKey { .. })
    ));
    let right = SecretString::from("testpass");
    assert!(SshAuth::from_key_file(Path::new(ENCRYPTED), Some(right)).is_ok());
}

#[test]
fn a_missing_passphrase_a_wrong_one_and_a_missing_file_each_say_so() {
    let error = SshAuth::from_key_file(Path::new(ENCRYPTED), None).unwrap_err();
    assert!(error.to_string().contains("passphrase"), "{error}");
    let wrong = SecretString::from("nope");
    assert!(SshAuth::from_key_file(Path::new(ENCRYPTED), Some(wrong)).is_err());
    let error = SshAuth::from_key_file(Path::new("/no/such/key"), None).unwrap_err();
    assert!(error.to_string().contains("/no/such/key"), "{error}");
}
