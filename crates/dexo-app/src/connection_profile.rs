use dexo_driver_api::ConnectRequest;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::connection_policy::{ConnectionPolicy, Environment};
use crate::error::{AppError, ErrorCategory};

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
    pub secret_ref: SecretRef,
}

impl ConnectionProfile {
    pub fn connect_request(
        &self,
        secret: SecretString,
    ) -> Result<(ConnectRequest, ConnectionPolicy), AppError> {
        let policy = ConnectionPolicy::for_environment(Environment::parse(&self.environment));
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
        .unwrap_or(match driver {
            "mysql" => 3306,
            _ => 5432,
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

    #[test]
    fn connect_request_builds_host_port_endpoint() {
        let profile = ConnectionProfile {
            id: ConnectionId(uuid::Uuid::nil()),
            project_id: None,
            name: "x".into(),
            driver: "postgres".into(),
            environment: "local".into(),
            config: serde_json::json!({
                "host": "localhost",
                "port": 5432,
                "username": "u",
                "database": "d"
            }),
            secret_ref: SecretRef::new("r".into()),
        };
        let (request, policy) = profile.connect_request(SecretString::from("s")).unwrap();
        assert_eq!(request.endpoint, "localhost:5432");
        assert_eq!(request.username, "u");
        assert_eq!(request.database.as_deref(), Some("d"));
        assert_eq!(policy.max_rows, 100_000);
    }
}
