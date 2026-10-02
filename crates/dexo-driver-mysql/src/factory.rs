use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use dexo_driver_api::{
    Capability, CapabilityState, ConnectRequest, ConnectionFactory, DriverError,
    DriverErrorCategory, RouteRequest, Session, TlsMode, TlsRequest, TransportRequest,
};
use dexo_transport::{ProxyConfig, SshAuth, SshTunnelRequest, TransportLease};
use mysql_async::prelude::Queryable;
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
            .db_name(request.database)
            // The name performance_schema.session_connect_attrs, and the tools reading
            // it, show for Dexo's sessions; the mysql client sends its own the same way.
            .connect_attribute("program_name", "dexo");
        if request.read_only {
            // `init` runs on every connection these options open, the cancel connection
            // and reconnects included, so the server refuses the write, not only Dexo.
            builder = builder.init(vec!["SET SESSION TRANSACTION READ ONLY"]);
        }
        let routed = !matches!(transport.route, RouteRequest::Direct);
        if let Some(tls) = &transport.tls
            && tls.mode != TlsMode::Disable
        {
            builder = builder.ssl_opts(Some(ssl_opts(tls, &original_host, routed)?));
        }
        let opts = mysql_async::Opts::from(builder);
        let mut conn = Conn::new(opts.clone())
            .await
            .map_err(|error| connect_error(error, &transport, lease.as_ref()))?;
        let conn_id = conn.id();
        // MariaDB answers the MySQL handshake; its version string is how it says so.
        let version: Option<String> = conn
            .query_first("SELECT VERSION()")
            .await
            .map_err(map_error)?;
        let mariadb =
            version.is_some_and(|version| version.to_ascii_lowercase().contains("mariadb"));
        Ok(Box::new(
            MysqlSession::new(
                Arc::new(Mutex::new(conn)),
                opts,
                conn_id,
                Arc::new(AtomicU64::new(1)),
                lease,
            )
            .with_mariadb(mariadb),
        ))
    }
}

/// MariaDB through the MySQL driver: the same protocol and catalog, its own name in the
/// connection form. The session finds out which server it reached on its own, so a
/// MariaDB profile saved as `mysql` behaves the same.
pub struct MariadbFactory;

#[async_trait::async_trait]
impl ConnectionFactory for MariadbFactory {
    fn descriptor(&self) -> dexo_driver_api::DriverDescriptor {
        dexo_driver_api::DriverDescriptor::mariadb()
    }

    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        MysqlFactory.connect(request).await
    }
}

/// A failed connect, said in terms of the address it tried. A server's own answer keeps
/// its words; a failure before one names the host and the reason, where the driver's
/// text read `Input/output error: Input/output error: ...`.
fn connect_error(
    error: mysql_async::Error,
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
    if !matches!(error, mysql_async::Error::Io(_)) {
        return map_error(error);
    }
    let (host, port) = (transport.target_host.as_str(), transport.target_port);
    let cause = dexo_driver_api::root_cause(&error);
    if cause.contains("invalid peer certificate") || cause.contains("certificate") {
        return DriverError::new(
            DriverErrorCategory::Configuration,
            format!(
                "{host}:{port} sent a TLS certificate this connection does not trust ({cause}): \
                 set ca_file to the certificate that signed it, or lower tls_mode"
            ),
        );
    }
    DriverError::unreachable(host, port, &cause)
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
