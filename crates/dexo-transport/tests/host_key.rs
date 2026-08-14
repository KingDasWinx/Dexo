use std::sync::Arc;
use std::time::Duration;

use dexo_transport::{
    HostKeyDecision, KnownHost, SshAuth, SshTunnelRequest, TransportError, open_ssh_tunnel,
    ssh_fingerprint, verify_host_key,
};
use russh::Channel;
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Handler, Msg, Server as _, Session};
use secrecy::SecretString;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[test]
fn changed_host_key_is_never_accepted() {
    let known = KnownHost {
        host: "bastion".into(),
        port: 22,
        fingerprint: "SHA256:old".into(),
    };
    let decision = verify_host_key(Some(&known), "SHA256:new");
    assert_eq!(decision, HostKeyDecision::Changed);
}

#[test]
fn new_host_key_requires_confirmation() {
    let decision = verify_host_key(None, "SHA256:new");
    assert_eq!(
        decision,
        HostKeyDecision::New {
            fingerprint: "SHA256:new".into()
        }
    );
}

#[test]
fn trusted_host_key_is_accepted() {
    let known = KnownHost {
        host: "bastion".into(),
        port: 22,
        fingerprint: "SHA256:abc".into(),
    };
    assert_eq!(
        verify_host_key(Some(&known), "SHA256:abc"),
        HostKeyDecision::Trusted
    );
}

#[tokio::test]
async fn ssh_tunnel_echoes_when_host_key_is_trusted() {
    let fixture = SshFixture::start().await;
    let request = fixture.request();
    let mut stream = open_ssh_tunnel(request, Some(&fixture.known()))
        .await
        .unwrap();
    stream.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"ping");
}

#[tokio::test]
async fn changed_host_key_fails_closed_on_handshake() {
    let fixture = SshFixture::start().await;
    let mut known = fixture.known();
    known.fingerprint = "SHA256:old".into();
    let error = open_ssh_tunnel(fixture.request(), Some(&known))
        .await
        .err()
        .expect("changed host key should fail");
    assert!(matches!(error, TransportError::HostKeyChanged));
}

struct SshFixture {
    bastion_port: u16,
    target_port: u16,
    fingerprint: String,
}

impl SshFixture {
    async fn start() -> Self {
        let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let fingerprint = ssh_fingerprint(&russh::keys::PublicKey::from(&host_key));

        let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_port = target.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = target.accept().await.unwrap();
            let mut buf = [0u8; 4];
            socket.read_exact(&mut buf).await.unwrap();
            socket.write_all(&buf).await.unwrap();
        });

        let config = russh::server::Config {
            inactivity_timeout: Some(Duration::from_secs(5)),
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            keys: vec![host_key],
            ..Default::default()
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bastion_port = listener.local_addr().unwrap().port();
        let mut server = BastionServer;
        tokio::spawn(async move {
            server.run_on_socket(Arc::new(config), &listener).await.ok();
        });

        Self {
            bastion_port,
            target_port,
            fingerprint,
        }
    }

    fn known(&self) -> KnownHost {
        KnownHost {
            host: "127.0.0.1".into(),
            port: self.bastion_port,
            fingerprint: self.fingerprint.clone(),
        }
    }

    fn request(&self) -> SshTunnelRequest {
        SshTunnelRequest {
            bastion_host: "127.0.0.1".into(),
            bastion_port: self.bastion_port,
            username: "dexo".into(),
            auth: SshAuth::Password(SecretString::from("secret")),
            target_host: "127.0.0.1".into(),
            target_port: self.target_port,
        }
    }
}

struct BastionServer;

impl russh::server::Server for BastionServer {
    type Handler = BastionHandler;

    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self::Handler {
        BastionHandler
    }
}

struct BastionHandler;

impl Handler for BastionHandler {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        if user == "dexo" && password == "secret" {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        let host = host_to_connect.to_string();
        let port = port_to_connect as u16;
        tokio::spawn(async move {
            let Ok(mut target) = TcpStream::connect((host.as_str(), port)).await else {
                return;
            };
            let mut tunnel = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut tunnel, &mut target).await;
        });
        Ok(())
    }
}
