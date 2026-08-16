use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::{
    BoxStream, KnownHost, ProxyConfig, ProxyCredentials, SshTunnelRequest, TransportError,
    connect_direct, connect_proxy, open_ssh_tunnel,
};

const MAX_FORWARDS: usize = 32;

enum ForwardTarget {
    Direct {
        host: String,
        port: u16,
    },
    Proxy {
        proxy: ProxyConfig,
        target_host: String,
        target_port: u16,
        auth: Option<ProxyCredentials>,
    },
    Ssh {
        request: SshTunnelRequest,
        known: Option<KnownHost>,
    },
}

pub struct TransportLease {
    endpoint: SocketAddr,
    cancel: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl TransportLease {
    pub async fn direct(target: SocketAddr) -> Result<Self, TransportError> {
        Self::listen(ForwardTarget::Direct {
            host: target.ip().to_string(),
            port: target.port(),
        })
        .await
    }

    pub async fn proxy(
        proxy: ProxyConfig,
        target_host: impl Into<String>,
        target_port: u16,
        auth: Option<ProxyCredentials>,
    ) -> Result<Self, TransportError> {
        Self::listen(ForwardTarget::Proxy {
            proxy,
            target_host: target_host.into(),
            target_port,
            auth,
        })
        .await
    }

    pub async fn ssh(
        request: SshTunnelRequest,
        known: Option<KnownHost>,
    ) -> Result<Self, TransportError> {
        Self::listen(ForwardTarget::Ssh { request, known }).await
    }

    pub fn endpoint(&self) -> SocketAddr {
        self.endpoint
    }

    pub async fn close(mut self) {
        self.cancel.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    async fn listen(target: ForwardTarget) -> Result<Self, TransportError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| TransportError::Io(error.to_string()))?;
        let endpoint = listener
            .local_addr()
            .map_err(|error| TransportError::Io(error.to_string()))?;
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let target = Arc::new(target);
        let cap = Arc::new(Semaphore::new(MAX_FORWARDS));
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = task_cancel.cancelled() => break,
                    accepted = listener.accept() => {
                        let Ok((local, _)) = accepted else { break; };
                        let Ok(permit) = cap.clone().try_acquire_owned() else {
                            drop(local);
                            continue;
                        };
                        let target = Arc::clone(&target);
                        let child = task_cancel.child_token();
                        tokio::spawn(async move {
                            let _permit = permit;
                            tokio::select! {
                                _ = child.cancelled() => {}
                                _ = forward_one(local, &target) => {}
                            }
                        });
                    }
                }
            }
        });
        Ok(Self {
            endpoint,
            cancel,
            task: Some(task),
        })
    }
}

impl Drop for TransportLease {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn forward_one(mut local: TcpStream, target: &ForwardTarget) -> Result<(), TransportError> {
    let mut remote = open_remote(target).await?;
    tokio::io::copy_bidirectional(&mut local, &mut remote)
        .await
        .map(|_| ())
        .map_err(|error| TransportError::Io(error.to_string()))
}

async fn open_remote(target: &ForwardTarget) -> Result<BoxStream, TransportError> {
    match target {
        ForwardTarget::Direct { host, port } => connect_direct(host, *port).await,
        ForwardTarget::Proxy {
            proxy,
            target_host,
            target_port,
            auth,
        } => connect_proxy(proxy, target_host, *target_port, auth.as_ref()).await,
        ForwardTarget::Ssh { request, known } => {
            open_ssh_tunnel(clone_ssh_request(request), known.as_ref()).await
        }
    }
}

fn clone_ssh_request(request: &SshTunnelRequest) -> SshTunnelRequest {
    SshTunnelRequest {
        bastion_host: request.bastion_host.clone(),
        bastion_port: request.bastion_port,
        username: request.username.clone(),
        auth: clone_ssh_auth(&request.auth),
        target_host: request.target_host.clone(),
        target_port: request.target_port,
    }
}

fn clone_ssh_auth(auth: &crate::SshAuth) -> crate::SshAuth {
    match auth {
        crate::SshAuth::Password(secret) => crate::SshAuth::Password(secret.clone()),
        crate::SshAuth::PrivateKey { pem, passphrase } => crate::SshAuth::PrivateKey {
            pem: pem.clone(),
            passphrase: passphrase.clone(),
        },
        crate::SshAuth::Agent => crate::SshAuth::Agent,
    }
}
