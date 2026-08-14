use keyring::Entry;
use secrecy::SecretString;

use crate::store::{SecretError, SecretStore};

const SERVICE: &str = "dev.dexo.connection";

#[derive(Clone, Debug, Default)]
pub struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn put(&self, key: &str, value: &str) -> Result<(), SecretError> {
        let entry = Entry::new(SERVICE, key).map_err(map_keyring_error)?;
        entry.set_password(value).map_err(map_keyring_error)
    }

    fn get(&self, key: &str) -> Result<Option<SecretString>, SecretError> {
        let entry = Entry::new(SERVICE, key).map_err(map_keyring_error)?;
        match entry.get_password() {
            Ok(password) => Ok(Some(SecretString::from(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(map_keyring_error(err)),
        }
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        let entry = Entry::new(SERVICE, key).map_err(map_keyring_error)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(map_keyring_error(err)),
        }
    }
}

fn map_keyring_error(err: keyring::Error) -> SecretError {
    match err {
        keyring::Error::NoDefaultStore
        | keyring::Error::NoStorageAccess(_)
        | keyring::Error::NotSupportedByStore(_) => SecretError::Unavailable,
        other => {
            let msg = other.to_string().to_ascii_lowercase();
            if msg.contains("lock") || msg.contains("unavailable") || msg.contains("denied") {
                SecretError::Unavailable
            } else {
                SecretError::Internal
            }
        }
    }
}
