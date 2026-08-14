use crate::{DriverError, DriverErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionMode {
    ReadWrite,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionState {
    Idle,
    Active,
    Failed,
    Unknown,
}

#[async_trait::async_trait]
pub trait TransactionControl: Send + Sync {
    async fn begin(&self, mode: TransactionMode) -> Result<(), DriverError>;
    async fn commit(&self) -> Result<(), DriverError>;
    async fn rollback(&self) -> Result<(), DriverError>;
    async fn savepoint(&self, name: &str) -> Result<(), DriverError>;
    async fn rollback_to(&self, name: &str) -> Result<(), DriverError>;
    async fn release_savepoint(&self, name: &str) -> Result<(), DriverError>;
    fn state(&self) -> TransactionState;
}

pub fn validate_savepoint(name: &str) -> Result<(), DriverError> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(DriverError::new(
            DriverErrorCategory::Configuration,
            "invalid savepoint identifier",
        ));
    }
    Ok(())
}
