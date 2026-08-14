use std::sync::Arc;

use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use dexo_transport::{TlsConfig, TlsMode, connect_direct, connect_tls};

#[cfg(feature = "dangerous-tls")]
#[test]
fn insecure_tls_requires_explicit_flag() {
    let config = TlsConfig {
        mode: TlsMode::DisableVerification,
        explicit_insecure: false,
        server_name: "db.local".into(),
        ca_file: None,
    };
    assert!(matches!(
        config.validate(),
        Err(dexo_transport::TransportError::UnsafeConfiguration(_))
    ));
}

#[tokio::test]
async fn trusted_test_ca_connects() {
    let fixture = TlsFixture::start("db.local", CertValidity::Valid).await;
    let config = TlsConfig {
        mode: TlsMode::VerifyFull,
        explicit_insecure: false,
        server_name: "db.local".into(),
        ca_file: Some(fixture.ca_path()),
    };
    let stream = connect_direct("127.0.0.1", fixture.port).await.unwrap();
    let mut tls = connect_tls(stream, &config, None).await.unwrap();
    let mut buf = [0u8; 4];
    tls.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"pong");
}

#[tokio::test]
async fn hostname_mismatch_is_rejected() {
    let fixture = TlsFixture::start("db.local", CertValidity::Valid).await;
    let config = TlsConfig {
        mode: TlsMode::VerifyFull,
        explicit_insecure: false,
        server_name: "other.local".into(),
        ca_file: Some(fixture.ca_path()),
    };
    let stream = connect_direct("127.0.0.1", fixture.port).await.unwrap();
    let error = connect_tls(stream, &config, None)
        .await
        .err()
        .expect("hostname mismatch should fail");
    assert!(
        error.to_string().to_lowercase().contains("name")
            || error.to_string().to_lowercase().contains("cert")
            || error.to_string().to_lowercase().contains("tls")
    );
}

#[tokio::test]
async fn expired_cert_is_rejected() {
    let fixture = TlsFixture::start("db.local", CertValidity::Expired).await;
    let config = TlsConfig {
        mode: TlsMode::VerifyFull,
        explicit_insecure: false,
        server_name: "db.local".into(),
        ca_file: Some(fixture.ca_path()),
    };
    let stream = connect_direct("127.0.0.1", fixture.port).await.unwrap();
    assert!(connect_tls(stream, &config, None).await.is_err());
}

#[cfg(feature = "dangerous-tls")]
#[tokio::test]
async fn explicit_insecure_mode_connects() {
    let fixture = TlsFixture::start("db.local", CertValidity::Expired).await;
    let config = TlsConfig {
        mode: TlsMode::DisableVerification,
        explicit_insecure: true,
        server_name: "db.local".into(),
        ca_file: None,
    };
    let stream = connect_direct("127.0.0.1", fixture.port).await.unwrap();
    let mut tls = connect_tls(stream, &config, None).await.unwrap();
    let mut buf = [0u8; 4];
    tls.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"pong");
}

enum CertValidity {
    Valid,
    Expired,
}

struct TlsFixture {
    port: u16,
    dir: TempDir,
}

impl TlsFixture {
    async fn start(dns_name: &str, validity: CertValidity) -> Self {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let dir = TempDir::new().unwrap();
        let (ca_pem, cert_der, key_der) = mint_certs(dns_name, validity);
        std::fs::write(dir.path().join("ca.pem"), ca_pem).unwrap();

        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert_der)],
                PrivateKeyDer::try_from(key_der).unwrap(),
            )
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(server_config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut tls = acceptor.accept(stream).await.unwrap();
            tls.write_all(b"pong").await.unwrap();
        });
        Self { port, dir }
    }

    fn ca_path(&self) -> std::path::PathBuf {
        self.dir.path().join("ca.pem")
    }
}

fn mint_certs(dns_name: &str, validity: CertValidity) -> (String, Vec<u8>, Vec<u8>) {
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::CrlSign,
    ];
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);

    let mut server_params = CertificateParams::new(vec![dns_name.to_string()]).unwrap();
    server_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ServerAuth);
    if matches!(validity, CertValidity::Expired) {
        server_params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        server_params.not_after = rcgen::date_time_ymd(2020, 1, 2);
    }
    let server_key = KeyPair::generate().unwrap();
    let server_cert = server_params.signed_by(&server_key, &issuer).unwrap();
    (
        ca_cert.pem(),
        server_cert.der().to_vec(),
        server_key.serialize_der(),
    )
}
