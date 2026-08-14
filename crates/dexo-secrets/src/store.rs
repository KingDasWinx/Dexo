use secrecy::SecretString;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("keychain is unavailable or locked; enter the secret for this session")]
    Unavailable,
    #[error("secret store failed")]
    Internal,
}

pub trait SecretStore: Send + Sync {
    fn put(&self, key: &str, value: &str) -> Result<(), SecretError>;
    fn get(&self, key: &str) -> Result<Option<SecretString>, SecretError>;
    fn delete(&self, key: &str) -> Result<(), SecretError>;
}
