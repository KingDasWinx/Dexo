use dexo_secrets::{SecretError, SecretStore};
use uuid::Uuid;

use crate::connection_profile::{ConnectionId, ConnectionProfile, SecretRef};
use crate::error::{AppError, ErrorCategory};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewConnection {
    pub name: String,
    pub driver: String,
    pub host: String,
    pub port: Option<u16>,
    pub database: String,
    pub username: String,
    pub environment: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretPersist {
    Stored,
    SessionOnly,
}

pub trait ConnectionProfiles {
    fn get_by_name(&self, name: &str) -> Result<Option<ConnectionProfile>, AppError>;
    fn save(&self, profile: &ConnectionProfile) -> Result<(), AppError>;
}

pub fn create(
    input: NewConnection,
    password: &str,
    secrets: &dyn SecretStore,
    repo: &impl ConnectionProfiles,
) -> Result<(ConnectionProfile, SecretPersist), AppError> {
    let profile = build_profile(input)?;
    if password.is_empty() {
        return Err(AppError::new(
            ErrorCategory::Authentication,
            "password is required",
        ));
    }
    if repo.get_by_name(&profile.name)?.is_some() {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            format!("connection '{}' already exists", profile.name),
        ));
    }
    repo.save(&profile)?;
    let persist = put_secret(secrets, profile.secret_ref.as_str(), password)?;
    Ok((profile, persist))
}

pub fn set_secret(
    name: &str,
    password: &str,
    secrets: &dyn SecretStore,
    repo: &impl ConnectionProfiles,
) -> Result<(ConnectionProfile, SecretPersist), AppError> {
    if password.is_empty() {
        return Err(AppError::new(
            ErrorCategory::Authentication,
            "password is required",
        ));
    }
    let profile = repo.get_by_name(name)?.ok_or_else(|| {
        AppError::new(
            ErrorCategory::Configuration,
            format!("unknown connection '{name}'"),
        )
    })?;
    let persist = put_secret(secrets, profile.secret_ref.as_str(), password)?;
    Ok((profile, persist))
}

fn build_profile(input: NewConnection) -> Result<ConnectionProfile, AppError> {
    let name = require_field("name", input.name)?;
    let driver = normalize_driver(&input.driver)?;
    let host = require_field("host", input.host)?;
    let database = require_field("database", input.database)?;
    let username = require_field("username", input.username)?;
    let port = input.port.unwrap_or(default_port(&driver));
    if port == 0 {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            "connection port is invalid",
        ));
    }
    let environment = if input.environment.trim().is_empty() {
        "local".into()
    } else {
        input.environment.trim().to_ascii_lowercase()
    };
    Ok(ConnectionProfile {
        id: ConnectionId(Uuid::new_v4()),
        project_id: None,
        name,
        driver,
        environment,
        config: serde_json::json!({
            "host": host,
            "port": port,
            "database": database,
            "username": username,
        }),
        secret_ref: SecretRef::new(Uuid::new_v4().to_string()),
    })
}

fn put_secret(
    secrets: &dyn SecretStore,
    key: &str,
    password: &str,
) -> Result<SecretPersist, AppError> {
    match secrets.put(key, password) {
        Ok(()) => Ok(SecretPersist::Stored),
        Err(SecretError::Unavailable) => Ok(SecretPersist::SessionOnly),
        Err(SecretError::Internal) => Err(AppError::new(
            ErrorCategory::Internal,
            "secret store failed",
        )),
    }
}

fn require_field(field: &str, value: String) -> Result<String, AppError> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            format!("{field} is required"),
        ));
    }
    Ok(value)
}

fn normalize_driver(driver: &str) -> Result<String, AppError> {
    match driver.trim().to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => Ok("postgres".into()),
        "mysql" => Ok("mysql".into()),
        "" => Err(AppError::new(
            ErrorCategory::Configuration,
            "driver is required",
        )),
        other => Err(AppError::new(
            ErrorCategory::Configuration,
            format!("unknown driver '{other}'"),
        )),
    }
}

fn default_port(driver: &str) -> u16 {
    match driver {
        "mysql" => 3306,
        _ => 5432,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dexo_secrets::{MemorySecretStore, SecretStore};
    use secrecy::ExposeSecret;

    use super::{ConnectionProfiles, NewConnection, SecretPersist, create, set_secret};
    use crate::connection_profile::ConnectionProfile;
    use crate::error::{AppError, ErrorCategory};

    #[derive(Default)]
    struct MemoryRepo(Mutex<Vec<ConnectionProfile>>);

    impl ConnectionProfiles for MemoryRepo {
        fn get_by_name(&self, name: &str) -> Result<Option<ConnectionProfile>, AppError> {
            Ok(self
                .0
                .lock()
                .expect("repo lock")
                .iter()
                .find(|profile| profile.name == name)
                .cloned())
        }

        fn save(&self, profile: &ConnectionProfile) -> Result<(), AppError> {
            let mut rows = self.0.lock().expect("repo lock");
            if let Some(existing) = rows.iter_mut().find(|row| row.id == profile.id) {
                *existing = profile.clone();
            } else {
                rows.push(profile.clone());
            }
            Ok(())
        }
    }

    fn input() -> NewConnection {
        NewConnection {
            name: "local-pg".into(),
            driver: "postgres".into(),
            host: "127.0.0.1".into(),
            port: None,
            database: "dexo".into(),
            username: "dexo".into(),
            environment: "local".into(),
        }
    }

    #[test]
    fn create_keeps_password_out_of_profile() {
        const SENTINEL: &str = "SUPER_SECRET_SENTINEL";
        let repo = MemoryRepo::default();
        let store = MemorySecretStore::default();
        let (profile, persist) = create(input(), SENTINEL, &store, &repo).unwrap();
        assert_eq!(persist, SecretPersist::Stored);
        assert_eq!(profile.config["port"], 5432);
        let dumped = format!("{profile:?}");
        assert!(!dumped.contains(SENTINEL));
        assert!(!profile.config.to_string().contains(SENTINEL));
        let loaded = ConnectionProfiles::get_by_name(&repo, "local-pg")
            .unwrap()
            .unwrap();
        assert!(!loaded.config.to_string().contains(SENTINEL));
        assert_eq!(
            store
                .get(profile.secret_ref.as_str())
                .unwrap()
                .unwrap()
                .expose_secret(),
            SENTINEL
        );
    }

    #[test]
    fn create_rejects_duplicate_name() {
        let repo = MemoryRepo::default();
        let store = MemorySecretStore::default();
        create(input(), "pw", &store, &repo).unwrap();
        let error = create(input(), "pw", &store, &repo).unwrap_err();
        assert_eq!(error.category(), ErrorCategory::Configuration);
        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn set_secret_replaces_keychain_value() {
        let repo = MemoryRepo::default();
        let store = MemorySecretStore::default();
        let (profile, _) = create(input(), "old", &store, &repo).unwrap();
        set_secret("local-pg", "new-secret", &store, &repo).unwrap();
        assert_eq!(
            store
                .get(profile.secret_ref.as_str())
                .unwrap()
                .unwrap()
                .expose_secret(),
            "new-secret"
        );
        assert!(!format!("{profile:?}").contains("new-secret"));
    }
}
