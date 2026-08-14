use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::config::{validate_host, validate_port};
use crate::host_key::{HostKeyDecision, KnownHost, ssh_fingerprint, verify_host_key};
use crate::{BoxStream, TransportError};

pub enum SshAuth {
    Password(SecretString),
    PrivateKey {
        pem: SecretString,
        passphrase: Option<SecretString>,
    },
    Agent,
}

impl std::fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Password(_) => f.debug_tuple("Password").field(&"[redacted]").finish(),
            Self::PrivateKey { .. } => f
                .debug_struct("PrivateKey")
                .field("pem", &"[redacted]")
                .field("passphrase", &"[redacted]")
                .finish(),
            Self::Agent => write!(f, "Agent"),
        }
    }
}

pub struct SshTunnelRequest {
    pub bastion_host: String,
    pub bastion_port: u16,
    pub username: String,
    pub auth: SshAuth,
    pub target_host: String,
    pub target_port: u16,
}

struct HostKeyHandler {
    known: Option<KnownHost>,
}

impl russh::client::Handler for HostKeyHandler {
    type Error = TransportError;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        match verify_host_key(self.known.as_ref(), &ssh_fingerprint(server_public_key)) {
            HostKeyDecision::Trusted => Ok(true),
            HostKeyDecision::New { fingerprint } => Err(TransportError::HostKeyNew { fingerprint }),
            HostKeyDecision::Changed => Err(TransportError::HostKeyChanged),
        }
    }
}

struct SshTunnel {
    _session: russh::client::Handle<HostKeyHandler>,
    stream: russh::ChannelStream<russh::client::Msg>,
}

impl AsyncRead for SshTunnel {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for SshTunnel {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

pub async fn open_ssh_tunnel(
    request: SshTunnelRequest,
    known: Option<&KnownHost>,
) -> Result<BoxStream, TransportError> {
    validate_host(&request.bastion_host)?;
    validate_port(
        request.bastion_port,
        "bastion port must be between 1 and 65535",
    )?;
    validate_host(&request.target_host)?;
    validate_port(
        request.target_port,
        "target port must be between 1 and 65535",
    )?;

    let tcp = TcpStream::connect((request.bastion_host.as_str(), request.bastion_port))
        .await
        .map_err(|error| TransportError::Io(error.to_string()))?;
    let config = Arc::new(russh::client::Config::default());
    let handler = HostKeyHandler {
        known: known.cloned(),
    };
    let mut session = russh::client::connect_stream(config, tcp, handler).await?;
    authenticate(&mut session, &request.username, &request.auth).await?;
    let channel = session
        .channel_open_direct_tcpip(
            request.target_host,
            u32::from(request.target_port),
            "127.0.0.1",
            0,
        )
        .await?;
    Ok(Box::new(SshTunnel {
        _session: session,
        stream: channel.into_stream(),
    }))
}

async fn authenticate(
    session: &mut russh::client::Handle<HostKeyHandler>,
    username: &str,
    auth: &SshAuth,
) -> Result<(), TransportError> {
    let result = match auth {
        SshAuth::Password(password) => {
            session
                .authenticate_password(username, password.expose_secret())
                .await?
        }
        SshAuth::PrivateKey { pem, passphrase } => {
            let key = load_private_key(pem.expose_secret(), passphrase.as_ref())?;
            let hash = session.best_supported_rsa_hash().await?.flatten();
            session
                .authenticate_publickey(
                    username,
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await?
        }
        SshAuth::Agent => return authenticate_agent(session, username).await,
    };
    if !auth_succeeded(&result) {
        return Err(TransportError::Ssh("SSH authentication failed".into()));
    }
    Ok(())
}

fn load_private_key(
    pem: &str,
    passphrase: Option<&SecretString>,
) -> Result<russh::keys::PrivateKey, TransportError> {
    let key = russh::keys::PrivateKey::from_openssh(pem)
        .map_err(|error| TransportError::Ssh(error.to_string()))?;
    if key.is_encrypted() {
        let Some(passphrase) = passphrase else {
            return Err(TransportError::Ssh(
                "encrypted private key requires a passphrase".into(),
            ));
        };
        return key
            .decrypt(passphrase.expose_secret())
            .map_err(|error| TransportError::Ssh(error.to_string()));
    }
    Ok(key)
}

async fn authenticate_agent(
    session: &mut russh::client::Handle<HostKeyHandler>,
    username: &str,
) -> Result<(), TransportError> {
    let mut agent = connect_agent().await?;
    let identities = agent
        .request_identities()
        .await
        .map_err(|error| TransportError::Ssh(error.to_string()))?;
    for identity in identities {
        let hash = session.best_supported_rsa_hash().await?.flatten();
        match session
            .authenticate_publickey_with(
                username,
                identity.public_key().into_owned(),
                hash,
                &mut agent,
            )
            .await
        {
            Ok(result) if auth_succeeded(&result) => return Ok(()),
            Ok(_) => continue,
            Err(error) => return Err(TransportError::Ssh(error.to_string())),
        }
    }
    Err(TransportError::Ssh(
        "SSH agent authentication failed".into(),
    ))
}

fn auth_succeeded(result: &russh::client::AuthResult) -> bool {
    matches!(result, russh::client::AuthResult::Success)
}

async fn connect_agent() -> Result<
    russh::keys::agent::client::AgentClient<
        impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    >,
    TransportError,
> {
    #[cfg(windows)]
    {
        let pipe = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(r"\\.\pipe\openssh-ssh-agent")
            .map_err(|error| TransportError::Ssh(error.to_string()))?;
        Ok(russh::keys::agent::client::AgentClient::connect(pipe))
    }
    #[cfg(unix)]
    {
        let path = std::env::var("SSH_AUTH_SOCK")
            .map_err(|_| TransportError::Ssh("SSH_AUTH_SOCK is not set".into()))?;
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .map_err(|error| TransportError::Ssh(error.to_string()))?;
        Ok(russh::keys::agent::client::AgentClient::connect(stream))
    }
}

impl From<russh::Error> for TransportError {
    fn from(error: russh::Error) -> Self {
        Self::Ssh(error.to_string())
    }
}
