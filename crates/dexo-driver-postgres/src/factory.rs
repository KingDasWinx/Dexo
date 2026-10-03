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
        // pg_stat_activity, and every tool reading it, tells Dexo's sessions apart by this.
        config.application_name("dexo");
        if request.read_only {
            config.options("-c default_transaction_read_only=on");
        }
        let use_tls = transport
            .tls
            .as_ref()
            .is_some_and(|tls| tls.mode != TlsMode::Disable);
        config.ssl_mode(ssl_mode(transport.tls.as_ref().map(|tls| tls.mode)));
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
            let (client, mut connection) = config
                .connect(tls)
                .await
                .map_err(|error| connect_error(error, &transport, lease.as_ref()))?;
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
            let session = PostgresSession::new(client, notice_rx, cancel, lease);
            session.read_backend_pid().await;
            return Ok(Box::new(session));
        }
        let (client, mut connection) = config
            .connect(tokio_postgres::NoTls)
            .await
            .map_err(|error| connect_error(error, &transport, lease.as_ref()))?;
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
        let session = PostgresSession::new(client, notice_rx, cancel, lease);
        session.read_backend_pid().await;
        Ok(Box::new(session))
    }
}

/// A failed connect, said in terms of the address it tried. What the server answered --
/// a rejected password, a missing database -- keeps the server's own words; a failure
/// before any answer (refused, no route, a name that does not resolve, a server that
/// will not speak TLS) names the host and the reason, where the driver's own text is
/// just "error connecting to server".
fn connect_error(
    error: tokio_postgres::Error,
    transport: &TransportRequest,
    lease: Option<&TransportLease>,
) -> DriverError {
    // A proxy or tunnel that would not carry the connection shows to the driver only as
    // a socket that closed; the lease knows why.
    if let Some(message) = lease
        .and_then(TransportLease::failure)
        .and_then(|cause| transport.route.failure_message(&cause))
    {
        return DriverError::new(DriverErrorCategory::Transport, message);
    }
    if error.as_db_error().is_some() || error.is_closed() {
        return map_error(error);
    }
    let (host, port) = (transport.target_host.as_str(), transport.target_port);
    let cause = dexo_driver_api::root_cause(&error);
    if cause.contains("does not support TLS") {
        return DriverError::new(
            DriverErrorCategory::Configuration,
            format!(
                "{host}:{port} does not support TLS, which this connection's tls_mode requires: \
                 turn TLS on in the server, or lower tls_mode"
            ),
        );
    }
    DriverError::unreachable(host, port, &cause)
}

/// What the driver does when the server will not speak TLS. Only `preferred` may carry on
/// in plaintext: `required`, `verify_ca` and `verify_full` promise that nothing travels
/// unencrypted, and the driver's own default (`prefer`) quietly breaks that promise.
fn ssl_mode(mode: Option<TlsMode>) -> tokio_postgres::config::SslMode {
    use tokio_postgres::config::SslMode;
    match mode {
        None | Some(TlsMode::Disable) => SslMode::Disable,
        Some(TlsMode::Preferred) => SslMode::Prefer,
        Some(TlsMode::Required | TlsMode::VerifyCa | TlsMode::VerifyFull) => SslMode::Require,
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
            // A key file is the key to use, with its passphrase when it has one; without
            // one, the password, and failing that the agent.
            let auth = match (&ssh.key_file, request.secrets.get("ssh_password")) {
                (Some(path), _) => {
                    SshAuth::from_key_file(path, request.secrets.get("ssh_passphrase").cloned())
                        .map_err(map_transport)?
                }
                (None, Some(password)) => SshAuth::Password(password.clone()),
                (None, None) => SshAuth::Agent,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_postgres::config::SslMode;

    #[test]
    fn only_preferred_tls_may_fall_back_to_plaintext() {
        assert_eq!(ssl_mode(None), SslMode::Disable);
        assert_eq!(ssl_mode(Some(TlsMode::Disable)), SslMode::Disable);
        assert_eq!(ssl_mode(Some(TlsMode::Preferred)), SslMode::Prefer);
        for mode in [TlsMode::Required, TlsMode::VerifyCa, TlsMode::VerifyFull] {
            assert_eq!(ssl_mode(mode.into()), SslMode::Require, "{mode:?}");
        }
    }
}
