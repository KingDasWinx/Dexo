use std::sync::Arc;

use dexo_driver_api::{
    Capability, CapabilityState, ConnectRequest, ConnectionFactory, DriverError,
    DriverErrorCategory, Session,
};
use mysql_async::{Conn, OptsBuilder};
use secrecy::ExposeSecret;
use tokio::sync::Mutex;

use crate::error::map_error;
use crate::session::MysqlSession;

pub struct MysqlFactory;

#[async_trait::async_trait]
impl ConnectionFactory for MysqlFactory {
    fn driver_name(&self) -> &'static str {
        "mysql"
    }

    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        let (host, port) = parse_endpoint(&request.endpoint)?;
        let opts = mysql_async::Opts::from(
            OptsBuilder::default()
                .ip_or_hostname(host)
                .tcp_port(port)
                .user(Some(request.username))
                .pass(Some(request.secret.expose_secret().to_string()))
                .db_name(request.database),
        );
        let conn = Conn::new(opts.clone()).await.map_err(map_error)?;
        let conn_id = conn.id();
        Ok(Box::new(MysqlSession::new(
            Arc::new(Mutex::new(conn)),
            opts,
            conn_id,
        )))
    }
}

pub(crate) fn parse_endpoint(endpoint: &str) -> Result<(String, u16), DriverError> {
    let (host, port) = endpoint.rsplit_once(':').ok_or_else(|| {
        DriverError::new(
            DriverErrorCategory::Configuration,
            "endpoint must be host:port",
        )
    })?;
    let port = port.parse::<u16>().map_err(|_| {
        DriverError::new(
            DriverErrorCategory::Configuration,
            "endpoint port is invalid",
        )
    })?;
    if host.is_empty() || port == 0 {
        return Err(DriverError::new(
            DriverErrorCategory::Configuration,
            "endpoint is invalid",
        ));
    }
    Ok((host.trim_matches(['[', ']']).to_string(), port))
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
