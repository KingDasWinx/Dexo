use std::sync::Arc;

use dexo_driver_api::{DriverError, DriverErrorCategory, TlsMode, TlsRequest, TransportRequest};
use dexo_transport::{ClientCertificate, TlsConfig};
use rustls::ClientConfig;
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres_rustls::MakeRustlsConnect;

pub struct PostgresCancelContext {
    pub config: tokio_postgres::Config,
    pub transport: TransportRequest,
    pub tls: Option<NamedRustls>,
}

#[derive(Clone)]
pub struct NamedRustls {
    inner: MakeRustlsConnect,
    server_name: String,
}

impl NamedRustls {
    pub fn new(config: ClientConfig, server_name: String) -> Self {
        Self {
            inner: MakeRustlsConnect::new(config),
            server_name,
        }
    }
}

impl<S> MakeTlsConnect<S> for NamedRustls
where
    MakeRustlsConnect: MakeTlsConnect<S>,
{
    type Stream = <MakeRustlsConnect as MakeTlsConnect<S>>::Stream;
    type TlsConnect = <MakeRustlsConnect as MakeTlsConnect<S>>::TlsConnect;
    type Error = <MakeRustlsConnect as MakeTlsConnect<S>>::Error;

    fn make_tls_connect(&mut self, _domain: &str) -> Result<Self::TlsConnect, Self::Error> {
        self.inner.make_tls_connect(&self.server_name)
    }
}

pub fn rustls_from_request(
    tls: &TlsRequest,
    server_name: &str,
) -> Result<NamedRustls, DriverError> {
    let mode = match tls.mode {
        TlsMode::Disable => {
            return Err(DriverError::new(
                DriverErrorCategory::Configuration,
                "disabled TLS does not use rustls",
            ));
        }
        TlsMode::Preferred | TlsMode::Required => dexo_transport::TlsMode::Preferred,
        TlsMode::VerifyCa => dexo_transport::TlsMode::VerifyCa,
        TlsMode::VerifyFull => dexo_transport::TlsMode::VerifyFull,
    };
    let client_cert = match (&tls.client_cert, &tls.client_key) {
        (Some(cert), Some(key)) => Some(ClientCertificate {
            cert_file: cert.clone(),
            key_file: key.clone(),
        }),
        _ => None,
    };
    let config = TlsConfig {
        mode,
        explicit_insecure: false,
        server_name: tls
            .server_name
            .clone()
            .unwrap_or_else(|| server_name.to_string()),
        ca_file: tls.ca_file.clone(),
    };
    let client = dexo_transport::rustls_client_config(&config, client_cert.as_ref())
        .map_err(|error| DriverError::new(DriverErrorCategory::Transport, error.to_string()))?;
    Ok(NamedRustls::new(
        Arc::unwrap_or_clone(client),
        config.server_name,
    ))
}
