use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use dexo_driver_api::{
    Capability, CapabilityState, ConnectRequest, ConnectionFactory, DriverError,
    DriverErrorCategory, RouteRequest, Session, TlsMode, TlsRequest, TransportRequest,
};
use dexo_transport::{ProxyConfig, SshAuth, SshTunnelRequest, TransportLease};
use mysql_async::{Conn, OptsBuilder, SslOpts};
use secrecy::ExposeSecret;
use tokio::sync::Mutex;

use crate::error::map_error;
use crate::session::MysqlSession;

pub struct MysqlFactory;

#[async_trait::async_trait]
impl ConnectionFactory for MysqlFactory {
    fn descriptor(&self) -> dexo_driver_api::DriverDescriptor {
        dexo_driver_api::DriverDescriptor::mysql()
    }

    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        let transport = effective_transport(&request)?;
        transport.validate().map_err(|error| {
            DriverError::new(DriverErrorCategory::Configuration, error.to_string())
        })?;
        let original_host = transport.target_host.clone();
        let (host, port, lease) = bind_route(&transport, &request).await?;
        let mut builder = OptsBuilder::default()
            .ip_or_hostname(host)
            .tcp_port(port)
            .user(Some(request.username))
            .pass(Some(request.secret.expose_secret().to_string()))
            .db_name(request.database);
        let routed = !matches!(transport.route, RouteRequest::Direct);
        if let Some(tls) = &transport.tls
            && tls.mode != TlsMode::Disable
        {
            builder = builder.ssl_opts(Some(ssl_opts(tls, &original_host, routed)?));
        }
        let opts = mysql_async::Opts::from(builder);
        let conn = Conn::new(opts.clone()).await.map_err(map_error)?;
        let conn_id = conn.id();
        Ok(Box::new(MysqlSession::new(
            Arc::new(Mutex::new(conn)),
            opts,
            conn_id,
            Arc::new(AtomicU64::new(1)),
            lease,
        )))
    }
}

fn ssl_opts(tls: &TlsRequest, original_host: &str, routed: bool) -> Result<SslOpts, DriverError> {
    let mut opts = SslOpts::default();
    if let Some(ca) = &tls.ca_file {
        opts = opts.with_root_certs(vec![ca.clone().into()]);
        opts = opts.with_disable_built_in_roots(true);
    }
    if let (Some(cert), Some(key)) = (&tls.client_cert, &tls.client_key) {
        opts = opts.with_client_identity(Some(mysql_async::ClientIdentity::new(
            cert.clone().into(),
            key.clone().into(),
        )));
    }
    if tls.mode == TlsMode::VerifyCa {
        opts = opts.with_danger_skip_domain_validation(true);
    }
    if routed {
        opts = opts.with_danger_tls_hostname_override(Some(original_host.to_string()));
    } else if let Some(name) = &tls.server_name {
        opts = opts.with_danger_tls_hostname_override(Some(name.clone()));
    }
    Ok(opts)
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
            lease_proxy(ProxyConfig::socks5(host.clone(), *port), transport).await
        }
        RouteRequest::HttpConnect { host, port } => {
            lease_proxy(ProxyConfig::http_connect(host.clone(), *port), transport).await
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

async fn lease_proxy(
    proxy: ProxyConfig,
    transport: &TransportRequest,
) -> Result<(String, u16, Option<TransportLease>), DriverError> {
    let lease = TransportLease::proxy(
        proxy,
        transport.target_host.clone(),
        transport.target_port,
        None,
    )
    .await
    .map_err(map_transport)?;
    let endpoint = lease.endpoint();
    Ok((endpoint.ip().to_string(), endpoint.port(), Some(lease)))
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
