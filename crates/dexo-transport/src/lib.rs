mod config;
mod host_key;
mod proxy;
mod ssh;
mod tcp;
mod tls;

pub use config::{ClientCertificate, ProxyConfig, TlsConfig, TlsMode, TransportError};
pub use host_key::{HostKeyDecision, KnownHost, ssh_fingerprint, verify_host_key};
pub use proxy::{ProxyCredentials, connect_proxy};
pub use ssh::{SshAuth, SshTunnelRequest, open_ssh_tunnel};
pub use tcp::connect_direct;
pub use tls::connect_tls;

pub trait AsyncStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T> AsyncStream for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
pub type BoxStream = Box<dyn AsyncStream>;
