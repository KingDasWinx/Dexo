use std::collections::HashMap;
use std::sync::Mutex;

use secrecy::{ExposeSecret, SecretString};

use crate::store::{SecretError, SecretStore};

#[derive(Default)]
pub struct MemorySecretStore {
    inner: Mutex<HashMap<String, SecretString>>,
}

impl SecretStore for MemorySecretStore {
    fn put(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string(), SecretString::from(value));
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<SecretString>, SecretError> {
        Ok(self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .map(|s| SecretString::from(s.expose_secret())))
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
        Ok(())
    }
}
