use dexo_driver_api::{
    Capability, CapabilityState, ConnectRequest, ConnectionFactory, DriverError,
    DriverErrorCategory, RouteRequest, Session, TlsMode, TransportRequest,
};
use dexo_transport::{ProxyConfig, SshAuth, SshTunnelRequest, TransportLease};
use secrecy::ExposeSecret;

use crate::error::map_error;
use crate::session::PostgresSession;
use crate::tls::{PostgresCancelContext, rustls_from_request};

pub struct PostgresFactory;

#[async_trait::async_trait]
impl ConnectionFactory for PostgresFactory {
    fn descriptor(&self) -> dexo_driver_api::DriverDescriptor {
        dexo_driver_api::DriverDescriptor::postgres()
    }

    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        let transport = effective_transport(&request)?;
        transport.validate().map_err(|error| {
            DriverError::new(DriverErrorCategory::Configuration, error.to_string())
        })?;
        let (host, port, lease) = bind_route(&transport, &request).await?;
        let original_host = transport.target_host.clone();
        let mut config = tokio_postgres::Config::new();
        config.host(&host);
        config.port(port);
        config.user(&request.username);
        config.password(request.secret.expose_secret());
        if let Some(database) = &request.database {
            config.dbname(database);
        }
        if request.read_only {
            config.options("-c default_transaction_read_only=on");
        }
        let use_tls = transport
            .tls
            .as_ref()
            .is_some_and(|tls| tls.mode != TlsMode::Disable);
        if !use_tls {
            config.ssl_mode(tokio_postgres::config::SslMode::Disable);
        }
        let cancel = PostgresCancelContext {
            config: config.clone(),
            transport: transport.clone(),
            tls: None,
        };
        if use_tls {
            let tls_req = transport.tls.as_ref().expect("tls checked");
            let tls = rustls_from_request(tls_req, &original_host)?;
            let mut cancel = cancel;
            cancel.tls = Some(tls.clone());
            let (client, mut connection) = config.connect(tls).await.map_err(map_error)?;
            let notice_rx = {
                let (notice_tx, notice_rx) = tokio::sync::mpsc::unbounded_channel();
                tokio::spawn(async move {
                    loop {
                        match std::future::poll_fn(|cx| connection.poll_message(cx)).await {
                            Some(Ok(tokio_postgres::AsyncMessage::Notice(notice))) => {
                                let _ = notice_tx.send(dexo_driver_api::SessionEvent::Notice {
                                    severity: Some(notice.severity().to_string()),
                                    message: notice.message().to_string(),
                                });
                            }
                            Some(Ok(tokio_postgres::AsyncMessage::Notification(_)))
                            | Some(Ok(_)) => {}
                            Some(Err(_)) | None => break,
                        }
                    }
                });
                notice_rx
            };
            return Ok(Box::new(PostgresSession::new(
                client, notice_rx, cancel, lease,
            )));
        }
        let (client, mut connection) = config
            .connect(tokio_postgres::NoTls)
            .await
            .map_err(map_error)?;
        let notice_rx = {
            let (notice_tx, notice_rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(async move {
                loop {
                    match std::future::poll_fn(|cx| connection.poll_message(cx)).await {
                        Some(Ok(tokio_postgres::AsyncMessage::Notice(notice))) => {
                            let _ = notice_tx.send(dexo_driver_api::SessionEvent::Notice {
                                severity: Some(notice.severity().to_string()),
                                message: notice.message().to_string(),
                            });
                        }
                        Some(Ok(tokio_postgres::AsyncMessage::Notification(_))) | Some(Ok(_)) => {}
                        Some(Err(_)) | None => break,
                    }
                }
            });
            notice_rx
        };
        Ok(Box::new(PostgresSession::new(
            client, notice_rx, cancel, lease,
        )))
    }
}

fn effective_transport(request: &ConnectRequest) -> Result<TransportRequest, DriverError> {
    if request.transport.target_port != 0 && !request.transport.target_host.is_empty() {
        return Ok(request.transport.clone());
    }
    TransportRequest::from_endpoint(&request.endpoint)
        .map_err(|error| DriverError::new(DriverErrorCategory::Configuration, error.to_string()))
}

async fn bind_route(
    transport: &TransportRequest,
    request: &ConnectRequest,
) -> Result<(String, u16, Option<TransportLease>), DriverError> {
    match &transport.route {
        RouteRequest::Direct => Ok((transport.target_host.clone(), transport.target_port, None)),
        RouteRequest::Socks5 { host, port } => {
            let lease = TransportLease::proxy(
                ProxyConfig::socks5(host.clone(), *port),
                transport.target_host.clone(),
                transport.target_port,
                None,
            )
            .await
            .map_err(map_transport)?;
            let endpoint = lease.endpoint();
            Ok((endpoint.ip().to_string(), endpoint.port(), Some(lease)))
        }
        RouteRequest::HttpConnect { host, port } => {
            let lease = TransportLease::proxy(
                ProxyConfig::http_connect(host.clone(), *port),
                transport.target_host.clone(),
                transport.target_port,
                None,
            )
            .await
            .map_err(map_transport)?;
            let endpoint = lease.endpoint();
            Ok((endpoint.ip().to_string(), endpoint.port(), Some(lease)))
        }
        RouteRequest::Ssh(ssh) => {
            let auth = match request.secrets.get("ssh_password") {
                Some(password) => SshAuth::Password(password.clone()),
                None => SshAuth::Agent,
            };
            let lease = TransportLease::ssh(
                SshTunnelRequest {
                    bastion_host: ssh.host.clone(),
                    bastion_port: ssh.port,
                    username: ssh.username.clone(),
                    auth,
                    target_host: transport.target_host.clone(),
                    target_port: transport.target_port,
                },
                None,
            )
            .await
            .map_err(map_transport)?;
            let endpoint = lease.endpoint();
            Ok((endpoint.ip().to_string(), endpoint.port(), Some(lease)))
        }
    }
}

fn map_transport(error: dexo_transport::TransportError) -> DriverError {
    DriverError::new(DriverErrorCategory::Transport, error.to_string())
}

pub(crate) fn capabilities() -> Vec<CapabilityState> {
    vec![
        CapabilityState::available(Capability::Catalog),
        CapabilityState::available(Capability::Query),
        CapabilityState::available(Capability::Cancel),
        CapabilityState::available(Capability::Transactions),
        CapabilityState::available(Capability::DataWrite),
        CapabilityState::available(Capability::Ddl),
        CapabilityState::available(Capability::Explain),
        CapabilityState::available(Capability::ExplainAnalyze),
        CapabilityState::available(Capability::Admin),
        CapabilityState::available(Capability::Import),
        CapabilityState::available(Capability::Export),
    ]
}
