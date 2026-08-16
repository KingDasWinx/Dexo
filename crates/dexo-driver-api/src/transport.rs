use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use crate::error::{DriverError, DriverErrorCategory};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    Disable,
    Preferred,
    Required,
    VerifyCa,
    VerifyFull,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TlsRequest {
    pub mode: TlsMode,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub ca_file: Option<PathBuf>,
    #[serde(default)]
    pub client_cert: Option<PathBuf>,
    #[serde(default)]
    pub client_key: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SshRequest {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub key_file: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RouteRequest {
    Direct,
    Socks5 { host: String, port: u16 },
    HttpConnect { host: String, port: u16 },
    Ssh(SshRequest),
}

pub type ProxyMode = RouteRequest;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TransportRequest {
    pub target_host: String,
    pub target_port: u16,
    pub tls: Option<TlsRequest>,
    pub route: RouteRequest,
}

#[derive(Clone, Default)]
pub struct ConnectionSecrets {
    inner: BTreeMap<String, SecretString>,
}

impl ConnectionSecrets {
    pub fn database_password(secret: SecretString) -> Self {
        let mut secrets = Self::default();
        secrets.insert("database_password", secret);
        secrets
    }

    pub fn insert(&mut self, purpose: impl Into<String>, secret: SecretString) {
        self.inner.insert(purpose.into(), secret);
    }

    pub fn get(&self, purpose: &str) -> Option<&SecretString> {
        self.inner.get(purpose)
    }

    pub fn contains(&self, purpose: &str) -> bool {
        self.inner.contains_key(purpose)
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl From<SecretString> for ConnectionSecrets {
    fn from(secret: SecretString) -> Self {
        Self::database_password(secret)
    }
}

impl fmt::Debug for ConnectionSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.inner.keys().map(|key| (key, "[REDACTED]")))
            .finish()
    }
}

impl TransportRequest {
    pub fn direct(host: impl Into<String>, port: u16) -> Self {
        Self {
            target_host: host.into(),
            target_port: port,
            tls: None,
            route: RouteRequest::Direct,
        }
    }

    pub fn from_endpoint(endpoint: &str) -> Result<Self, DriverError> {
        let (host, port) = split_endpoint(endpoint)?;
        Ok(Self::direct(host, port))
    }

    pub fn validate(&self) -> Result<(), DriverError> {
        validate_host(&self.target_host, "target host")?;
        validate_port(self.target_port, "target port")?;
        if let Some(tls) = &self.tls {
            tls.validate()?;
        }
        match &self.route {
            RouteRequest::Direct => Ok(()),
            RouteRequest::Socks5 { host, port } | RouteRequest::HttpConnect { host, port } => {
                validate_host(host, "proxy host")?;
                validate_port(*port, "proxy port")
            }
            RouteRequest::Ssh(ssh) => {
                validate_host(&ssh.host, "ssh host")?;
                validate_port(ssh.port, "ssh port")?;
                if ssh.username.trim().is_empty() {
                    return Err(config_error("ssh username is required"));
                }
                if let Some(path) = &ssh.key_file {
                    validate_path(path, "ssh key")?;
                }
                Ok(())
            }
        }
    }

    pub fn validate_for_policy(&self, require_verified_tls: bool) -> Result<(), DriverError> {
        self.validate()?;
        if require_verified_tls {
            match self.tls.as_ref().map(|tls| tls.mode) {
                Some(TlsMode::VerifyCa | TlsMode::VerifyFull) => {}
                _ => {
                    return Err(config_error(
                        "verified TLS is required for this environment",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn required_secret_purposes(&self) -> Vec<&'static str> {
        let mut purposes = vec!["database_password"];
        if let RouteRequest::Ssh(ssh) = &self.route
            && ssh.key_file.is_none()
        {
            purposes.push("ssh_password");
        }
        purposes
    }
}

impl TlsRequest {
    fn validate(&self) -> Result<(), DriverError> {
        if let Some(name) = &self.server_name {
            validate_host(name, "tls server name")?;
        }
        if let Some(path) = &self.ca_file {
            validate_path(path, "ca file")?;
        }
        if let Some(path) = &self.client_cert {
            validate_path(path, "client certificate")?;
        }
        if let Some(path) = &self.client_key {
            validate_path(path, "client key")?;
        }
        if self.client_cert.is_some() != self.client_key.is_some() {
            return Err(config_error(
                "client certificate and key must be provided together",
            ));
        }
        Ok(())
    }
}

pub fn split_endpoint(endpoint: &str) -> Result<(String, u16), DriverError> {
    let (host, port) = endpoint
        .rsplit_once(':')
        .ok_or_else(|| config_error("endpoint must be host:port"))?;
    if host.is_empty() {
        return Err(config_error("endpoint host is empty"));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_| config_error("endpoint port is invalid"))?;
    Ok((host.trim_matches(['[', ']']).to_string(), port))
}

fn validate_host(host: &str, label: &str) -> Result<(), DriverError> {
    if host.is_empty()
        || host
            .bytes()
            .any(|b| b == b'\r' || b == b'\n' || b.is_ascii_whitespace())
    {
        return Err(config_error(format!("invalid {label}")));
    }
    Ok(())
}

fn validate_port(port: u16, label: &str) -> Result<(), DriverError> {
    if port == 0 {
        return Err(config_error(format!("{label} is invalid")));
    }
    Ok(())
}

fn validate_path(path: &Path, label: &str) -> Result<(), DriverError> {
    // ponytail: reject empty/NUL paths here; existence is checked when TLS/SSH opens the file.
    let text = path.to_string_lossy();
    if text.is_empty() || text.contains('\0') {
        return Err(config_error(format!("invalid {label} path")));
    }
    Ok(())
}

fn config_error(message: impl Into<String>) -> DriverError {
    DriverError::new(DriverErrorCategory::Configuration, message)
}
