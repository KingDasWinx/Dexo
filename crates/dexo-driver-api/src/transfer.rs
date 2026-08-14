use crate::{DbValue, DriverError, QualifiedName};

#[async_trait::async_trait]
pub trait BulkWriter: Send + Sync {
    async fn insert_batch(
        &self,
        table: &QualifiedName,
        columns: &[String],
        rows: &[Vec<DbValue>],
    ) -> Result<u64, DriverError>;
}
