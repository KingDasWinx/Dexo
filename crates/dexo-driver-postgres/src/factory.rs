use dexo_driver_api::{
    Capability, CapabilityState, ConnectRequest, ConnectionFactory, DriverError,
    DriverErrorCategory, Session,
};
use secrecy::ExposeSecret;

use crate::error::map_error;
use crate::session::PostgresSession;

pub struct PostgresFactory;

#[async_trait::async_trait]
impl ConnectionFactory for PostgresFactory {
    fn driver_name(&self) -> &'static str {
        "postgres"
    }

    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        let (host, port) = parse_endpoint(&request.endpoint)?;
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
        config.ssl_mode(tokio_postgres::config::SslMode::Disable);
        let (client, connection) = config
            .connect(tokio_postgres::NoTls)
            .await
            .map_err(map_error)?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(Box::new(PostgresSession::new(client)))
    }
}

pub(crate) fn parse_endpoint(endpoint: &str) -> Result<(String, u16), DriverError> {
    let (host, port) = endpoint.rsplit_once(':').ok_or_else(|| {
        DriverError::new(
            DriverErrorCategory::Configuration,
            "endpoint must be host:port",
        )
    })?;
    if host.is_empty() {
        return Err(DriverError::new(
            DriverErrorCategory::Configuration,
            "endpoint host is empty",
        ));
    }
    let port = port.parse::<u16>().map_err(|_| {
        DriverError::new(
            DriverErrorCategory::Configuration,
            "endpoint port is invalid",
        )
    })?;
    if port == 0 {
        return Err(DriverError::new(
            DriverErrorCategory::Configuration,
            "endpoint port is invalid",
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
        CapabilityState::available(Capability::Admin),
        CapabilityState::available(Capability::Import),
        CapabilityState::available(Capability::Export),
    ]
}
