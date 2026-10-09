use crate::{DbValue, DriverError, DriverErrorCategory, QualifiedName};

#[async_trait::async_trait]
pub trait BulkWriter: Send + Sync {
    /// `rows` in a transaction of their own: all of the batch or none.
    async fn insert_batch(
        &self,
        table: &QualifiedName,
        columns: &[String],
        rows: &[Vec<DbValue>],
    ) -> Result<u64, DriverError>;

    /// `rows` inside the transaction the caller holds open, with no BEGIN, COMMIT or
    /// ROLLBACK of their own, so the caller's rollback undoes every batch it wrote. A
    /// driver that cannot says so, and the caller does not import half a file.
    async fn insert_in_transaction(
        &self,
        _table: &QualifiedName,
        _columns: &[String],
        _rows: &[Vec<DbValue>],
    ) -> Result<u64, DriverError> {
        Err(DriverError::new(
            DriverErrorCategory::Capability,
            "this connection cannot import a file inside one transaction",
        ))
    }
}
