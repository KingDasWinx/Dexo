use std::sync::{Arc, Once};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject};
use rustls::{ClientConfig, RootCertStore};
use rustls_platform_verifier::BuilderVerifierExt;
use tokio_rustls::TlsConnector;

#[cfg(feature = "dangerous-tls")]
use crate::TlsMode;
use crate::{BoxStream, ClientCertificate, TlsConfig, TransportError};

pub fn rustls_client_config(
    config: &TlsConfig,
    client_cert: Option<&ClientCertificate>,
) -> Result<Arc<ClientConfig>, TransportError> {
    ensure_crypto_provider();
    Ok(Arc::new(build_client_config(config, client_cert)?))
}

pub async fn connect_tls(
    stream: BoxStream,
    config: &TlsConfig,
    client_cert: Option<&ClientCertificate>,
) -> Result<BoxStream, TransportError> {
    config.validate()?;
    let client_config = rustls_client_config(config, client_cert)?;
    let connector = TlsConnector::from(client_config);
    let server_name = ServerName::try_from(config.server_name.clone())
        .map_err(|_| TransportError::InvalidConfig("invalid TLS server name".into()))?;
    let tls = connector
        .connect(server_name, stream)
        .await
        .map_err(|error| TransportError::Tls(error.to_string()))?;
    Ok(Box::new(tls))
}

fn build_client_config(
    config: &TlsConfig,
    client_cert: Option<&ClientCertificate>,
) -> Result<ClientConfig, TransportError> {
    #[cfg(feature = "dangerous-tls")]
    if config.mode == TlsMode::DisableVerification {
        return insecure_config(client_cert);
    }

    // ponytail: VerifyCa currently uses the same hostname check as VerifyFull; split if CA-only is required
    let _mode = config.mode;
    if let Some(ca_file) = &config.ca_file {
        let roots = load_ca_store(ca_file)?;
        let builder = ClientConfig::builder().with_root_certificates(roots);
        return with_client_auth(builder, client_cert);
    }

    let builder = ClientConfig::builder()
        .with_platform_verifier()
        .map_err(|error| TransportError::Tls(error.to_string()))?;
    with_client_auth(builder, client_cert)
}

fn with_client_auth(
    builder: rustls::ConfigBuilder<ClientConfig, rustls::client::WantsClientCert>,
    client_cert: Option<&ClientCertificate>,
) -> Result<ClientConfig, TransportError> {
    match client_cert {
        None => Ok(builder.with_no_client_auth()),
        Some(identity) => {
            let certs = load_certs(&identity.cert_file)?;
            let key = load_key(&identity.key_file)?;
            builder
                .with_client_auth_cert(certs, key)
                .map_err(|error| TransportError::Tls(error.to_string()))
        }
    }
}

fn load_ca_store(path: &std::path::Path) -> Result<RootCertStore, TransportError> {
    let certs = load_certs(path)?;
    let mut roots = RootCertStore::empty();
    let (added, _) = roots.add_parsable_certificates(certs);
    if added == 0 {
        return Err(TransportError::InvalidConfig(
            "CA file contained no certificates".into(),
        ));
    }
    Ok(roots)
}

fn load_certs(path: &std::path::Path) -> Result<Vec<CertificateDer<'static>>, TransportError> {
    CertificateDer::pem_file_iter(path)
        .map_err(|error| TransportError::InvalidConfig(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| TransportError::InvalidConfig(error.to_string()))
}

fn load_key(path: &std::path::Path) -> Result<PrivateKeyDer<'static>, TransportError> {
    PrivateKeyDer::from_pem_file(path)
        .map_err(|error| TransportError::InvalidConfig(error.to_string()))
}

fn ensure_crypto_provider() {
    static START: Once = Once::new();
    START.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

#[cfg(feature = "dangerous-tls")]
fn insecure_config(
    client_cert: Option<&ClientCertificate>,
) -> Result<ClientConfig, TransportError> {
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()));
    let builder = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertificateVerification(Arc::clone(
            &provider,
        ))));
    with_client_auth(builder, client_cert)
}

#[cfg(feature = "dangerous-tls")]
#[derive(Debug)]
struct NoCertificateVerification(Arc<rustls::crypto::CryptoProvider>);

#[cfg(feature = "dangerous-tls")]
impl rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
