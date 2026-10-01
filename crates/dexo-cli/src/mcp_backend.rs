use std::path::PathBuf;

use dexo_app::schema_diff::SchemaSnapshot;
use dexo_app::{AppError, ConnectionProfile, DriverRegistry, ErrorCategory};
use dexo_driver_api::{CatalogObject, Session};
use dexo_mcp::McpBackend;
use dexo_storage::{CatalogCache, ConnectionRepository, Database, SchemaSnapshotStore};

use crate::run::{catalog_database_name, connect_session, refresh_catalog};

/// Hands the MCP adapter the keychain, the drivers and SQLite. The database file is
/// reopened per call because a SQLite connection cannot be held across an await.
pub struct CliMcpBackend {
    registry: DriverRegistry,
    database: PathBuf,
}

impl CliMcpBackend {
    pub fn new(registry: DriverRegistry, database: PathBuf) -> Self {
        Self { registry, database }
    }

    fn saved(&self, name: &str) -> Result<ConnectionProfile, AppError> {
        let db = Database::open(&self.database).map_err(storage)?;
        ConnectionRepository::new(db.connection())
            .get_by_name(name)
            .map_err(storage)?
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::Configuration,
                    format!("unknown connection '{name}'"),
                )
            })
    }
}

#[async_trait::async_trait]
impl McpBackend for CliMcpBackend {
    async fn connect(&self, connection: &str) -> Result<Box<dyn Session>, AppError> {
        let saved = self.saved(connection)?;
        connect_session(&self.registry, &saved)
            .await
            .map_err(from_anyhow)
    }

    async fn catalog_snapshot(&self, connection: &str) -> Result<Vec<CatalogObject>, AppError> {
        let saved = self.saved(connection)?;
        let key = saved.id.0.to_string();
        let database = catalog_database_name(&saved);
        let cached = {
            let db = Database::open(&self.database).map_err(storage)?;
            CatalogCache::new(db.connection())
                .load_latest(&key, &database)
                .map_err(storage)?
        };
        if !cached.is_empty() {
            return Ok(cached);
        }
        let objects = refresh_catalog(&self.registry, &saved)
            .await
            .map_err(from_anyhow)?;
        let db = Database::open(&self.database).map_err(storage)?;
        CatalogCache::new(db.connection())
            .replace_snapshot(&key, &database, &objects)
            .map_err(storage)?;
        Ok(objects)
    }

    fn schema_snapshot(&self, name: &str) -> Result<Option<SchemaSnapshot>, AppError> {
        let db = Database::open(&self.database).map_err(storage)?;
        SchemaSnapshotStore::new(db.connection())
            .load_by_name(name)
            .map_err(storage)
    }
}

fn storage(error: anyhow::Error) -> AppError {
    AppError::new(ErrorCategory::Storage, error.to_string())
}

fn from_anyhow(error: anyhow::Error) -> AppError {
    match error.downcast::<AppError>() {
        Ok(error) => error,
        Err(error) => AppError::new(ErrorCategory::Internal, error.to_string()),
    }
}
