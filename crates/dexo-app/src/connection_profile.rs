use std::collections::BTreeMap;

use dexo_driver_api::ConnectRequest;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::connection_policy::{ConnectionPolicy, ConnectionPolicyOverrides};
use crate::error::{AppError, ErrorCategory};

pub const PURPOSE_DATABASE_PASSWORD: &str = "database_password";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ConnectionId(pub Uuid);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SecretRef(String);

impl SecretRef {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: ConnectionId,
    pub project_id: Option<Uuid>,
    pub name: String,
    pub driver: String,
    pub environment: String,
    pub config: serde_json::Value,
    #[serde(default)]
    pub group_path: Option<String>,
    #[serde(default)]
    pub policy: ConnectionPolicyOverrides,
    pub secret_ref: SecretRef,
    #[serde(default)]
    pub secret_refs: BTreeMap<String, SecretRef>,
}

impl ConnectionProfile {
    pub fn new(
        id: ConnectionId,
        project_id: Option<Uuid>,
        name: impl Into<String>,
        driver: impl Into<String>,
        environment: impl Into<String>,
        config: serde_json::Value,
        secret_ref: SecretRef,
    ) -> Self {
        let mut secret_refs = BTreeMap::new();
        if !secret_ref.as_str().is_empty() {
            secret_refs.insert(PURPOSE_DATABASE_PASSWORD.to_string(), secret_ref.clone());
        }
        Self {
            id,
            project_id,
            name: name.into(),
            driver: driver.into(),
            environment: environment.into(),
            config,
            group_path: None,
            policy: ConnectionPolicyOverrides::default(),
            secret_ref,
            secret_refs,
        }
    }

    pub fn connect_request(
        &self,
        secret: SecretString,
    ) -> Result<(ConnectRequest, ConnectionPolicy), AppError> {
        let policy = ConnectionPolicy::resolve(&self.environment, &self.policy)?;
        Ok((
            ConnectRequest {
                endpoint: endpoint_from_config(&self.config, &self.driver)?,
                database: config_str(&self.config, &["database", "dbname"]),
                username: config_str(&self.config, &["username", "user"]).ok_or_else(|| {
                    AppError::new(
                        ErrorCategory::Configuration,
                        "connection username is required",
                    )
                })?,
                secret,
                read_only: policy.read_only,
            },
            policy,
        ))
    }
}

fn config_str(config: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        config
            .get(*key)
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn endpoint_from_config(config: &serde_json::Value, driver: &str) -> Result<String, AppError> {
    if let Some(endpoint) = config_str(config, &["endpoint"]) {
        return Ok(endpoint);
    }
    let host = config_str(config, &["host"]).ok_or_else(|| {
        AppError::new(ErrorCategory::Configuration, "connection host is required")
    })?;
    let port = config
        .get("port")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .unwrap_or_else(|| {
            dexo_driver_api::DriverDescriptor::for_id(driver)
                .map(|descriptor| u64::from(descriptor.default_port))
                .unwrap_or(5432)
        });
    if port == 0 || port > u64::from(u16::MAX) {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            "connection port is invalid",
        ));
    }
    Ok(format!("{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;

    use super::{ConnectionId, ConnectionProfile, SecretRef};
    use crate::connection_policy::ConnectionPolicyOverrides;
    use crate::error::ErrorCategory;

    fn sample(environment: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            ConnectionId(uuid::Uuid::nil()),
            None,
            "x",
            "postgres",
            environment,
            serde_json::json!({
                "host": "localhost",
                "port": 5432,
                "username": "u",
                "database": "d"
            }),
            SecretRef::new("r".into()),
        )
    }

    #[test]
    fn connect_request_builds_host_port_endpoint() {
        let (request, policy) = sample("local")
            .connect_request(SecretString::from("s"))
            .unwrap();
        assert_eq!(request.endpoint, "localhost:5432");
        assert_eq!(request.username, "u");
        assert_eq!(request.database.as_deref(), Some("d"));
        assert_eq!(policy.max_rows, 100_000);
    }

    #[test]
    fn custom_environment_rejects_missing_persisted_policy() {
        let error = sample("pci-lab")
            .connect_request(SecretString::from("s"))
            .unwrap_err();
        assert_eq!(error.category(), ErrorCategory::Configuration);
    }

    #[test]
    fn custom_environment_honors_persisted_policy() {
        let mut profile = sample("pci-lab");
        profile.policy = ConnectionPolicyOverrides {
            read_only: Some(true),
            confirm_destructive: Some(true),
            require_verified_tls: Some(true),
            max_rows: Some(10),
            timeout_secs: Some(3),
        };
        let (_, policy) = profile.connect_request(SecretString::from("s")).unwrap();
        assert!(policy.read_only);
        assert_eq!(policy.max_rows, 10);
    }
}
