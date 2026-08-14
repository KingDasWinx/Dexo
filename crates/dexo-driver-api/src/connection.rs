use secrecy::SecretString;

use crate::{
    BulkWriter, CapabilityState, CatalogReader, DataMutator, DdlExecutor, DriverError, QueryId,
    QueryRequest, QueryStream, SecurityAdmin, TransactionControl,
};

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
    fn driver_name(&self) -> &'static str;
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
}
