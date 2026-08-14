use dexo_driver_api::{Session, TransactionControl, TransactionMode};

use crate::error::{AppError, ErrorCategory};
use crate::query_service::map_driver_error;

pub struct TransactionService;

impl TransactionService {
    fn control(session: &dyn Session) -> Result<&dyn TransactionControl, AppError> {
        session
            .transactions()
            .ok_or_else(|| AppError::new(ErrorCategory::Capability, "transactions unavailable"))
    }

    pub async fn begin(session: &dyn Session, mode: TransactionMode) -> Result<(), AppError> {
        Self::control(session)?
            .begin(mode)
            .await
            .map_err(map_driver_error)
    }

    pub async fn commit(session: &dyn Session) -> Result<(), AppError> {
        Self::control(session)?
            .commit()
            .await
            .map_err(map_driver_error)
    }

    pub async fn rollback(session: &dyn Session) -> Result<(), AppError> {
        Self::control(session)?
            .rollback()
            .await
            .map_err(map_driver_error)
    }

    pub async fn savepoint(session: &dyn Session, name: &str) -> Result<(), AppError> {
        Self::control(session)?
            .savepoint(name)
            .await
            .map_err(map_driver_error)
    }

    pub async fn rollback_to(session: &dyn Session, name: &str) -> Result<(), AppError> {
        Self::control(session)?
            .rollback_to(name)
            .await
            .map_err(map_driver_error)
    }

    pub async fn release_savepoint(session: &dyn Session, name: &str) -> Result<(), AppError> {
        Self::control(session)?
            .release_savepoint(name)
            .await
            .map_err(map_driver_error)
    }
}
