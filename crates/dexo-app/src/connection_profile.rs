use std::collections::BTreeMap;

use dexo_driver_api::{
    ConnectRequest, ConnectionSecrets, DriverDescriptor, RouteRequest, SshRequest, TlsRequest,
    TransportRequest, split_endpoint,
};
use dexo_secrets::{SecretError, SecretStore};
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

    /// The command that prints this connection's password, when it takes the password
    /// from a password manager instead of the keychain.
    pub fn password_command(&self) -> Option<&str> {
        self.config
            .get("password_command")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|command| !command.is_empty())
    }

    /// Whether the driver opens a file (SQLite) rather than dialling a host.
    pub fn is_file(&self) -> bool {
        DriverDescriptor::for_id(&self.driver).is_some_and(|descriptor| descriptor.file)
    }

    /// The database password `store` holds for this connection. A file has none, so it
    /// is never looked up and nothing ever asks for it.
    pub fn password(&self, store: &dyn SecretStore) -> Result<Option<SecretString>, SecretError> {
        if self.is_file() {
            return Ok(Some(SecretString::from(String::new())));
        }
        store.get(self.secret_ref.as_str())
    }

    /// The user the connection logs in as; a file has none.
    pub fn username(&self) -> Option<String> {
        if self.is_file() {
            return None;
        }
        config_str(&self.config, &["username", "user"])
    }

    /// Where the connection dials, as `host:port` -- or the file it opens -- for a message
    /// that says what did not answer.
    pub fn target(&self) -> String {
        if self.is_file() {
            return config_str(&self.config, &["path"]).unwrap_or_default();
        }
        dial_target(&self.config, &self.driver)
            .map(|(host, port, _)| format!("{host}:{port}"))
            .unwrap_or_default()
    }

    /// Where the SSH password is kept, when the connection tunnels through SSH without a
    /// key file and so needs one: next to the database password's entry, under a key
    /// derived from it, so deleting the connection can find both.
    pub fn ssh_password_key(&self) -> Option<String> {
        match parse_route(&self.config) {
            Ok(RouteRequest::Ssh(ssh)) if ssh.key_file.is_none() => {
                Some(format!("{}:ssh_password", self.secret_ref.as_str()))
            }
            _ => None,
        }
    }

    /// A file's request is its `config.path` and nothing else: no host, user or secret,
    /// and no transport to validate. A policy's verified-TLS requirement guards a network
    /// path, so production and staging do not refuse a file for having none.
    pub fn connect_request(
        &self,
        secrets: impl Into<ConnectionSecrets>,
    ) -> Result<(ConnectRequest, ConnectionPolicy), AppError> {
        let secrets = secrets.into();
        let policy = ConnectionPolicy::resolve(&self.environment, &self.policy)?;
        if self.is_file() {
            let path = config_str(&self.config, &["path"]).ok_or_else(|| {
                AppError::new(
                    ErrorCategory::Configuration,
                    "the path of the database file is required",
                )
            })?;
            let request = ConnectRequest::new(
                path,
                None,
                String::new(),
                SecretString::from(String::new()),
                policy.read_only,
            );
            return Ok((request, policy));
        }
        let transport = transport_from_config(&self.config, &self.driver)?;
        transport
            .validate_for_policy(policy.require_verified_tls)
            .map_err(map_driver_config)?;
        for purpose in transport.required_secret_purposes() {
            if secrets.get(purpose).is_none() {
                return Err(AppError::new(
                    ErrorCategory::Authentication,
                    format!("missing secret for {purpose}"),
                ));
            }
        }
        let secret = secrets
            .get(PURPOSE_DATABASE_PASSWORD)
            .cloned()
            .ok_or_else(|| {
                AppError::new(
                    ErrorCategory::Authentication,
                    "missing secret for database_password",
                )
            })?;
        Ok((
            ConnectRequest {
                endpoint: format!("{}:{}", transport.target_host, transport.target_port),
                database: config_str(&self.config, &["database", "dbname"]),
                username: config_str(&self.config, &["username", "user"]).ok_or_else(|| {
                    AppError::new(
                        ErrorCategory::Configuration,
                        "connection username is required",
                    )
                })?,
                secret,
                read_only: policy.read_only,
                transport,
                secrets,
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

/// The host and port the profile dials -- an `endpoint`, or `host` and `port` with the
/// driver's default port -- and whether it goes through SSH or a proxy to get there.
pub(crate) fn dial_target(
    config: &serde_json::Value,
    driver: &str,
) -> Result<(String, u16, bool), AppError> {
    let endpoint = endpoint_from_config(config, driver)?;
    let (host, port) = split_endpoint(&endpoint).map_err(map_driver_config)?;
    let routed = !matches!(parse_route(config)?, RouteRequest::Direct);
    Ok((host, port, routed))
}

fn transport_from_config(
    config: &serde_json::Value,
    driver: &str,
) -> Result<TransportRequest, AppError> {
    let endpoint = endpoint_from_config(config, driver)?;
    let (target_host, target_port) = split_endpoint(&endpoint).map_err(map_driver_config)?;
    Ok(TransportRequest {
        target_host,
        target_port,
        tls: config.get("tls").map(parse_tls).transpose()?,
        route: parse_route(config)?,
    })
}

fn parse_tls(value: &serde_json::Value) -> Result<TlsRequest, AppError> {
    serde_json::from_value(value.clone()).map_err(|error| {
        AppError::new(
            ErrorCategory::Configuration,
            format!("invalid tls config: {error}"),
        )
    })
}

fn parse_route(config: &serde_json::Value) -> Result<RouteRequest, AppError> {
    if let Some(ssh) = config.get("ssh") {
        let request: SshRequest = serde_json::from_value(ssh.clone()).map_err(|error| {
            AppError::new(
                ErrorCategory::Configuration,
                format!("invalid ssh config: {error}"),
            )
        })?;
        return Ok(RouteRequest::Ssh(request));
    }
    if let Some(proxy) = config.get("proxy") {
        let host = proxy
            .get("host")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let port = proxy
            .get("port")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u16;
        return Ok(match proxy.get("kind").and_then(|value| value.as_str()) {
            Some("socks5") => RouteRequest::Socks5 { host, port },
            _ => RouteRequest::HttpConnect { host, port },
        });
    }
    Ok(RouteRequest::Direct)
}

fn map_driver_config(error: dexo_driver_api::DriverError) -> AppError {
    AppError::new(ErrorCategory::Configuration, error.to_string())
}

#[cfg(test)]
mod tests {
    use dexo_driver_api::{ConnectionSecrets, ProxyMode, TlsMode};
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

    fn secret_map() -> ConnectionSecrets {
        ConnectionSecrets::database_password(SecretString::from("s"))
    }

    fn production_profile_with_transport(tls: TlsMode, proxy: ProxyMode) -> ConnectionProfile {
        let (kind, host, port) = match proxy {
            ProxyMode::HttpConnect { host, port } => ("http", host, port),
            ProxyMode::Socks5 { host, port } => ("socks5", host, port),
            ProxyMode::Direct => ("direct", String::new(), 0),
            ProxyMode::Ssh(_) => ("ssh", String::new(), 0),
        };
        let mut config = serde_json::json!({
            "host": "db.example.com",
            "port": 5432,
            "username": "u",
            "database": "d",
            "tls": { "mode": tls }
        });
        if kind != "direct" && kind != "ssh" {
            config["proxy"] = serde_json::json!({ "kind": kind, "host": host, "port": port });
        }
        ConnectionProfile::new(
            ConnectionId(uuid::Uuid::nil()),
            None,
            "prod",
            "postgres",
            "production",
            config,
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
        assert!(matches!(
            request.transport.route,
            dexo_driver_api::RouteRequest::Direct
        ));
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
        profile
            .config
            .as_object_mut()
            .expect("config object")
            .insert("tls".into(), serde_json::json!({ "mode": "verify_full" }));
        let (_, policy) = profile.connect_request(SecretString::from("s")).unwrap();
        assert!(policy.read_only);
        assert_eq!(policy.max_rows, 10);
    }

    #[test]
    fn production_profile_rejects_unverified_tls_and_invalid_proxy() {
        let profile = production_profile_with_transport(
            TlsMode::Disable,
            ProxyMode::HttpConnect {
                host: "".into(),
                port: 0,
            },
        );
        let error = profile.connect_request(secret_map()).unwrap_err();
        assert_eq!(error.category(), ErrorCategory::Configuration);
    }

    #[test]
    fn a_file_needs_no_host_user_secret_or_tls() {
        let profile = ConnectionProfile::new(
            ConnectionId(uuid::Uuid::nil()),
            None,
            "shop",
            "sqlite",
            "production",
            serde_json::json!({ "path": "/data/shop.db" }),
            SecretRef::new("r".into()),
        );
        let (request, policy) = profile
            .connect_request(dexo_driver_api::ConnectionSecrets::default())
            .unwrap();
        assert_eq!(request.endpoint, "/data/shop.db");
        assert!(request.database.is_none() && request.username.is_empty());
        assert!(policy.require_verified_tls);
        let store = dexo_secrets::MemorySecretStore::default();
        assert!(profile.password(&store).unwrap().is_some());
    }

    #[test]
    fn production_profile_accepts_verified_tls_direct() {
        let profile = production_profile_with_transport(TlsMode::VerifyFull, ProxyMode::Direct);
        let (request, policy) = profile.connect_request(secret_map()).unwrap();
        assert!(policy.require_verified_tls);
        assert_eq!(request.transport.tls.unwrap().mode, TlsMode::VerifyFull);
    }
}
