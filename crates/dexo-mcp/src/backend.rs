use dexo_app::AppError;
use dexo_app::schema_diff::SchemaSnapshot;
use dexo_driver_api::{CatalogObject, Session};

/// What the server needs from the host binary. The keychain, the drivers and the local
/// SQLite belong to `dexo`; the adapter crate reaches them only through this.
#[async_trait::async_trait]
pub trait McpBackend: Send + Sync {
    async fn connect(&self, connection: &str) -> Result<Box<dyn Session>, AppError>;
    /// The connection's indexed catalog; the first call on a connection may build it.
    async fn catalog_snapshot(&self, connection: &str) -> Result<Vec<CatalogObject>, AppError>;
    fn schema_snapshot(&self, name: &str) -> Result<Option<SchemaSnapshot>, AppError>;
}
