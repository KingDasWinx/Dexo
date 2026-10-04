use std::path::PathBuf;

use dexo_app::schema_diff::SchemaSnapshot;
use dexo_app::{AppError, ConnectionProfile, DriverRegistry, ErrorCategory};
use dexo_driver_api::{CatalogObject, Session};
use dexo_mcp::McpBackend;
use dexo_storage::{CatalogCache, ConnectionRepository, Database, SchemaSnapshotStore};

use crate::run::{catalog_database_name, profile_secret, refresh_catalog};

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
    /// The agent is told which command failed, never the command line or what it
    /// printed: those carry hosts and paths MCP keeps to itself.
    async fn connect(
        &self,
        connection: &str,
        timeout: std::time::Duration,
    ) -> Result<Box<dyn Session>, AppError> {
        let saved = self.saved(connection)?;
        let commanded = |error: AppError, what: &str| {
            AppError::new(
                error.category(),
                format!(
                    "the {what} of {connection} failed; run `dexo connections test {connection}` to see why"
                ),
            )
        };
        let secret = profile_secret(&saved).await.map_err(|error| {
            let error = from_anyhow(error);
            if saved.password_command().is_some() {
                commanded(error, "password command")
            } else {
                error
            }
        })?;
        let factory = self.registry.get(&saved.driver)?;
        dexo_app::connect::open(factory.as_ref(), &saved, secret, Some(timeout))
            .await
            .map(|opened| opened.session)
            .map_err(|error| match error {
                dexo_app::connect::ConnectError::PreConnect(error) => {
                    commanded(error, "pre-connect command")
                }
                error => error.into(),
            })
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

    fn notes(
        &self,
        connection: &str,
    ) -> Result<std::collections::HashMap<String, String>, AppError> {
        let saved = self.saved(connection)?;
        let db = Database::open(&self.database).map_err(storage)?;
        dexo_storage::ObjectNoteRepository::new(db.connection())
            .for_connection(&saved.id.0.to_string())
            .map_err(storage)
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

#[cfg(all(test, unix))]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use dexo_app::connection_profile::{ConnectionId, SecretRef};
    use dexo_app::{ConnectionProfile, DriverRegistry};
    use dexo_driver_api::{
        ConnectRequest, ConnectionFactory, DriverDescriptor, DriverError, DriverErrorCategory,
        Session,
    };
    use dexo_mcp::McpBackend;
    use dexo_storage::{ConnectionRepository, Database};

    use super::CliMcpBackend;

    struct Refusing;

    #[async_trait::async_trait]
    impl ConnectionFactory for Refusing {
        fn descriptor(&self) -> DriverDescriptor {
            DriverDescriptor::postgres()
        }

        async fn connect(&self, _request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
            Err(DriverError::new(DriverErrorCategory::Transport, "refused"))
        }
    }

    /// A failed pre-connect or password command tells the agent which one failed, not
    /// the command line or what it printed.
    #[tokio::test]
    async fn an_agent_is_not_told_the_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dexo.db");
        let saved = |name: &str, config: serde_json::Value| {
            let profile = ConnectionProfile::new(
                ConnectionId(uuid::Uuid::new_v4()),
                None,
                name,
                "postgres",
                "local",
                config,
                SecretRef::new(format!("ref-{name}")),
            );
            let db = Database::open(&path).unwrap();
            ConnectionRepository::new(db.connection())
                .save(&profile)
                .unwrap();
        };
        saved(
            "tunnelled",
            serde_json::json!({
                "host": "secret-host.internal", "port": 5432, "username": "u",
                "password_command": "echo pw",
                "pre_connect": "echo 'cannot reach secret-host.internal' >&2; exit 2 # ${port}"
            }),
        );
        saved(
            "vaulted",
            serde_json::json!({
                "host": "h", "port": 5432, "username": "u",
                "password_command": "echo 'vault at secret-vault.internal is sealed' >&2; exit 1"
            }),
        );
        let mut registry = DriverRegistry::new();
        registry.register(Arc::new(Refusing));
        let backend = CliMcpBackend::new(registry, path.clone());
        for (name, what) in [
            ("tunnelled", "pre-connect command"),
            ("vaulted", "password command"),
        ] {
            let error = backend
                .connect(name, Duration::from_secs(5))
                .await
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains(what), "{error}");
            assert!(!error.contains("secret-"), "{error}");
            assert!(!error.contains("echo"), "{error}");
        }
    }
}
