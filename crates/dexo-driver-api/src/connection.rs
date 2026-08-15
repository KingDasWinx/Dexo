use secrecy::SecretString;

use crate::query::SessionEventStream;
use crate::{
    AdministrationProvider, BulkWriter, CapabilityState, CatalogReader, DataMutator, DdlExecutor,
    DriverError, ExplainProvider, QueryId, QueryRequest, QueryStream, SecurityAdmin,
    TransactionControl,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionOptions {
    pub tls: bool,
    pub client_certificate: bool,
    pub ssh: bool,
    pub proxy: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
    pub default_port: u16,
    pub options: ConnectionOptions,
}

impl DriverDescriptor {
    pub fn postgres() -> Self {
        Self {
            id: "postgres",
            display_name: "PostgreSQL",
            default_port: 5432,
            options: ConnectionOptions {
                tls: true,
                client_certificate: true,
                ssh: true,
                proxy: true,
            },
        }
    }

    pub fn mysql() -> Self {
        Self {
            id: "mysql",
            display_name: "MySQL",
            default_port: 3306,
            options: ConnectionOptions {
                tls: true,
                client_certificate: true,
                ssh: true,
                proxy: true,
            },
        }
    }

    pub fn for_id(id: &str) -> Option<Self> {
        [Self::postgres(), Self::mysql()]
            .into_iter()
            .find(|descriptor| descriptor.id == id)
    }
}

#[derive(Clone, Debug)]
pub struct ConnectRequest {
    pub endpoint: String,
    pub database: Option<String>,
    pub username: String,
    pub secret: SecretString,
    pub read_only: bool,
}

#[async_trait::async_trait]
pub trait ConnectionFactory: Send + Sync {
    fn descriptor(&self) -> DriverDescriptor;
    fn driver_name(&self) -> &'static str {
        self.descriptor().id
    }
    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError>;
}

#[async_trait::async_trait]
pub trait Session: Send + Sync {
    fn capabilities(&self) -> &[CapabilityState];
    async fn execute(&self, request: QueryRequest) -> Result<QueryStream, DriverError>;
    async fn cancel(&self, query: QueryId) -> Result<(), DriverError>;
    async fn close(self: Box<Self>) -> Result<(), DriverError>;
    fn transactions(&self) -> Option<&dyn TransactionControl> {
        None
    }

    fn catalog(&self) -> Option<&dyn CatalogReader> {
        None
    }

    fn data(&self) -> Option<&dyn DataMutator> {
        None
    }

    fn ddl(&self) -> Option<&dyn DdlExecutor> {
        None
    }

    fn security(&self) -> Option<&dyn SecurityAdmin> {
        None
    }

    fn bulk(&self) -> Option<&dyn BulkWriter> {
        None
    }

    fn explain(&self) -> Option<&dyn ExplainProvider> {
        None
    }

    fn admin(&self) -> Option<&dyn AdministrationProvider> {
        None
    }

    fn events(&self) -> Option<SessionEventStream> {
        None
    }
}
