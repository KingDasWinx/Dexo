use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("{0}")]
    InvalidConfig(String),
    #[error("{0}")]
    UnsafeConfiguration(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Proxy(String),
    #[error("{0}")]
    Tls(String),
    #[error("{0}")]
    Ssh(String),
    #[error("new SSH host key requires confirmation: {fingerprint}")]
    HostKeyNew { fingerprint: String },
    #[error("SSH host key changed")]
    HostKeyChanged,
}

#[derive(Clone, Debug)]
pub enum ProxyConfig {
    Socks5 { host: String, port: u16 },
    HttpConnect { host: String, port: u16 },
}

impl ProxyConfig {
    pub fn http_connect(host: impl Into<String>, port: u16) -> Self {
        Self::HttpConnect {
            host: host.into(),
            port,
        }
    }

    pub fn socks5(host: impl Into<String>, port: u16) -> Self {
        Self::Socks5 {
            host: host.into(),
            port,
        }
    }

    pub fn validate(&self) -> Result<(), TransportError> {
        let (host, port) = match self {
            Self::Socks5 { host, port } | Self::HttpConnect { host, port } => (host, *port),
        };
        validate_host(host)?;
        validate_port(port, "proxy port must be between 1 and 65535")?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TlsMode {
    Preferred,
    Required,
    VerifyCa,
    VerifyFull,
    #[cfg(feature = "dangerous-tls")]
    DisableVerification,
}

#[derive(Clone, Debug)]
pub struct TlsConfig {
    pub mode: TlsMode,
    pub explicit_insecure: bool,
    pub server_name: String,
    pub ca_file: Option<PathBuf>,
}

impl TlsConfig {
    pub fn validate(&self) -> Result<(), TransportError> {
        validate_host(&self.server_name)?;
        #[cfg(feature = "dangerous-tls")]
        if self.mode == TlsMode::DisableVerification && !self.explicit_insecure {
            return Err(TransportError::UnsafeConfiguration(
                "insecure TLS requires explicit_insecure=true".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ClientCertificate {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
}

pub(crate) fn validate_host(host: &str) -> Result<(), TransportError> {
    if host.is_empty()
        || host
            .bytes()
            .any(|b| b == b'\r' || b == b'\n' || b.is_ascii_whitespace())
    {
        return Err(TransportError::InvalidConfig("invalid host".into()));
    }
    Ok(())
}

pub(crate) fn validate_port(port: u16, message: &str) -> Result<(), TransportError> {
    if port == 0 {
        return Err(TransportError::InvalidConfig(message.into()));
    }
    Ok(())
}
