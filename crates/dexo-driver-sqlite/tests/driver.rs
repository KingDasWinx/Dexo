//! The SQLite driver's contract, in process.
mod suite;

use dexo_driver_api::{ConnectRequest, ConnectionFactory, DriverError, Session};

async fn connect(request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
    dexo_driver_sqlite::SqliteFactory.connect(request).await
}
