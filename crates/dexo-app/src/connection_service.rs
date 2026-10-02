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
    pub extra_config: serde_json::Value,
    pub policy: crate::connection_policy::ConnectionPolicyOverrides,
    pub group_path: Option<String>,
    /// The database lets the user in without a password (a Docker container made so):
    /// an empty one is kept rather than refused.
    pub allow_empty_password: bool,
}

impl Default for NewConnection {
    fn default() -> Self {
        Self {
            name: String::new(),
            driver: String::new(),
            host: String::new(),
            port: None,
            database: String::new(),
            username: String::new(),
            environment: "local".into(),
            extra_config: serde_json::json!({}),
            policy: crate::connection_policy::ConnectionPolicyOverrides::default(),
            group_path: None,
            allow_empty_password: false,
        }
    }
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
    let allow_empty_password = input.allow_empty_password;
    let profile = build_profile(input)?;
    if profile.password_command().is_some() {
        // The password manager answers every connect; nothing goes to the keychain.
        if repo.get_by_name(&profile.name)?.is_some() {
            return Err(AppError::new(
                ErrorCategory::Configuration,
                format!("connection '{}' already exists", profile.name),
            ));
        }
        repo.save(&profile)?;
        return Ok((profile, SecretPersist::Stored));
    }
    if password.is_empty() && !profile.is_file() && !allow_empty_password {
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
    if profile.is_file() {
        return Ok((profile, SecretPersist::Stored));
    }
    let persist = put_secret(secrets, profile.secret_ref.as_str(), password)?;
    Ok((profile, persist))
}

pub fn test_input(input: NewConnection) -> Result<ConnectionProfile, AppError> {
    build_profile(input)
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
    if profile.is_file() {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            format!("'{name}' opens a file and has no password"),
        ));
    }
    // Its password command answers every connect; a keychain entry would never be read.
    if let Some(command) = profile.password_command() {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            format!("'{name}' takes its password from `{command}`; nothing to set"),
        ));
    }
    let persist = put_secret(secrets, profile.secret_ref.as_str(), password)?;
    Ok((profile, persist))
}

fn build_profile(input: NewConnection) -> Result<ConnectionProfile, AppError> {
    let name = require_field("name", input.name.clone())?;
    let driver = normalize_driver(&input.driver)?;
    let environment = if input.environment.trim().is_empty() {
        "local".into()
    } else {
        input.environment.trim().to_ascii_lowercase()
    };
    let config = if dexo_driver_api::DriverDescriptor::for_id(&driver).is_some_and(|d| d.file) {
        file_config(&input.extra_config)?
    } else {
        host_config(&input, &driver)?
    };
    let mut profile = ConnectionProfile::new(
        ConnectionId(Uuid::new_v4()),
        None,
        name,
        driver,
        environment,
        config,
        SecretRef::new(Uuid::new_v4().to_string()),
    );
    profile.policy = input.policy;
    profile.group_path = input.group_path;
    Ok(profile)
}

/// A file connection keeps only its path, made absolute: the profile is opened later
/// from wherever Dexo runs, and a relative path would name a different file there.
fn file_config(extra: &serde_json::Value) -> Result<serde_json::Value, AppError> {
    let path = extra
        .get("path")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let path = require_field("path", path.to_string())?;
    let path = std::path::absolute(&path)
        .ok()
        .and_then(|path| path.into_os_string().into_string().ok())
        .ok_or_else(|| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("the path {path} cannot be made absolute"),
            )
        })?;
    Ok(serde_json::json!({ "path": path }))
}

fn host_config(input: &NewConnection, driver: &str) -> Result<serde_json::Value, AppError> {
    let host = require_field("host", input.host.clone())?;
    let database = require_field("database", input.database.clone())?;
    let username = require_field("username", input.username.clone())?;
    let port = input.port.unwrap_or(default_port(driver));
    if port == 0 {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            "connection port is invalid",
        ));
    }
    let mut config = serde_json::json!({
        "host": host,
        "port": port,
        "database": database,
        "username": username,
    });
    if let (Some(target), Some(extra)) = (config.as_object_mut(), input.extra_config.as_object()) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
    Ok(config)
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
        "mariadb" => Ok("mariadb".into()),
        "sqlite" | "sqlite3" => Ok("sqlite".into()),
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
    dexo_driver_api::DriverDescriptor::for_id(driver)
        .map(|descriptor| descriptor.default_port)
        .unwrap_or(5432)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dexo_secrets::{MemorySecretStore, SecretStore};
    use secrecy::ExposeSecret;

    use super::{ConnectionProfiles, NewConnection, SecretPersist, create, set_secret, test_input};
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
            ..NewConnection::default()
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

    /// An empty password is refused unless the database lets its user in without one.
    #[test]
    fn create_keeps_an_empty_password_only_when_allowed() {
        let repo = MemoryRepo::default();
        let store = MemorySecretStore::default();
        let error = create(input(), "", &store, &repo).unwrap_err();
        assert!(error.to_string().contains("password is required"));
        let open = NewConnection {
            allow_empty_password: true,
            ..input()
        };
        let (profile, _) = create(open, "", &store, &repo).unwrap();
        assert_eq!(
            store
                .get(profile.secret_ref.as_str())
                .unwrap()
                .unwrap()
                .expose_secret(),
            ""
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

    /// A connection whose password comes from a command has no keychain secret to set.
    #[test]
    fn set_secret_refuses_a_password_command_connection() {
        let repo = MemoryRepo::default();
        let store = MemorySecretStore::default();
        let mut with_command = input();
        with_command.extra_config = serde_json::json!({ "password_command": "pass show db" });
        let (profile, _) = create(with_command, "", &store, &repo).unwrap();
        let error = set_secret("local-pg", "pw", &store, &repo).unwrap_err();
        assert!(error.to_string().contains("pass show db"), "{error}");
        assert!(store.get(profile.secret_ref.as_str()).unwrap().is_none());
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

    #[test]
    fn test_input_does_not_save() {
        let repo = MemoryRepo::default();
        let profile = test_input(input()).unwrap();
        assert_eq!(profile.name, "local-pg");
        assert!(
            ConnectionProfiles::get_by_name(&repo, "local-pg")
                .unwrap()
                .is_none()
        );
    }
}
