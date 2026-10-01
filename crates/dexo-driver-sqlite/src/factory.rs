use std::path::{Path, PathBuf};

use dexo_driver_api::{
    Capability, CapabilityState, ConnectRequest, ConnectionFactory, DriverDescriptor, DriverError,
    DriverErrorCategory, Session,
};
use rusqlite::{Connection, OpenFlags};

use crate::error::internal;
use crate::session::SqliteSession;

pub struct SqliteFactory;

#[async_trait::async_trait]
impl ConnectionFactory for SqliteFactory {
    fn descriptor(&self) -> DriverDescriptor {
        DriverDescriptor::sqlite()
    }

    /// The endpoint is the file's path. A read-only connection opens it with
    /// `SQLITE_OPEN_READ_ONLY`, so SQLite itself refuses every write. An ATTACH opens its
    /// file with the same flags, and SQLite rejects a `file:` URI whose `mode=` asks for
    /// more than they allow.
    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        if request.endpoint.trim().is_empty() {
            return Err(DriverError::new(
                DriverErrorCategory::Configuration,
                "a SQLite connection needs the path of its file",
            ));
        }
        let path = PathBuf::from(request.endpoint);
        let read_only = request.read_only;
        let conn = tokio::task::spawn_blocking(move || open(&path, read_only))
            .await
            .map_err(internal)??;
        Ok(Box::new(SqliteSession::new(conn, read_only)))
    }
}

fn open(path: &Path, read_only: bool) -> Result<Connection, DriverError> {
    if read_only && !path.exists() {
        return Err(DriverError::new(
            DriverErrorCategory::Configuration,
            format!(
                "{} does not exist, and a read-only connection cannot create it",
                path.display()
            ),
        ));
    }
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let cannot_open = |error: rusqlite::Error| {
        DriverError::new(
            DriverErrorCategory::Configuration,
            format!("cannot open {}: {error}", path.display()),
        )
    };
    let conn = Connection::open_with_flags(path, flags).map_err(cannot_open)?;
    // SQLite opens lazily: a file that is not a database only fails on its first read,
    // which would otherwise be the user's first query.
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
        .map_err(cannot_open)?;
    Ok(conn)
}

pub(crate) fn capabilities() -> Vec<CapabilityState> {
    vec![
        CapabilityState::available(Capability::Catalog),
        CapabilityState::available(Capability::Query),
        CapabilityState::available(Capability::Cancel),
        CapabilityState::available(Capability::Transactions),
        CapabilityState::available(Capability::DataWrite),
        CapabilityState::unavailable(
            Capability::Ddl,
            "SQLite schema changes are written in the editor; the schema editor does not plan them yet",
        ),
        CapabilityState::available(Capability::Explain),
        CapabilityState::unavailable(Capability::ExplainAnalyze, "SQLite has no EXPLAIN ANALYZE"),
        CapabilityState::unavailable(
            Capability::Admin,
            "SQLite has no server sessions, locks or variables to administer",
        ),
        CapabilityState::available(Capability::Import),
        CapabilityState::available(Capability::Export),
    ]
}
