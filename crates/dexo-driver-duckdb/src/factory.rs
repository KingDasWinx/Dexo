use std::path::Path;

use dexo_driver_api::{
    Capability, CapabilityState, ConnectRequest, ConnectionFactory, DriverDescriptor, DriverError,
    DriverErrorCategory, Session,
};
use duckdb::{AccessMode, Config, Connection};

use crate::decode::quote;
use crate::error::internal;
use crate::session::DuckdbSession;

pub struct DuckdbFactory;

#[async_trait::async_trait]
impl ConnectionFactory for DuckdbFactory {
    fn descriptor(&self) -> DriverDescriptor {
        DriverDescriptor::duckdb()
    }

    /// The endpoint is a DuckDB file, `:memory:`, or a CSV, Parquet or JSON file, which
    /// opens an in-memory database with the file as a view named after it, and only
    /// reads: a `COPY ... TO` its own path would replace it. A read-only connection opens
    /// a DuckDB file with `AccessMode::ReadOnly`, so DuckDB itself refuses every write.
    async fn connect(&self, request: ConnectRequest) -> Result<Box<dyn Session>, DriverError> {
        let endpoint = request.endpoint.trim().to_string();
        if endpoint.is_empty() {
            return Err(DriverError::new(
                DriverErrorCategory::Configuration,
                "a DuckDB connection needs the path of its file, or :memory:",
            ));
        }
        let read_only = request.read_only;
        let data_file = reader(Path::new(&endpoint)).is_some();
        let conn = tokio::task::spawn_blocking(move || open(&endpoint, read_only))
            .await
            .map_err(internal)??;
        Ok(Box::new(DuckdbSession::new(conn, read_only || data_file)))
    }
}

/// How DuckDB reads a data file, by its extension.
fn reader(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    let name = name.strip_suffix(".gz").unwrap_or(&name);
    let extension = name.rsplit_once('.')?.1;
    Some(match extension {
        "csv" | "tsv" => "read_csv_auto",
        "parquet" => "read_parquet",
        "json" | "jsonl" | "ndjson" => "read_json_auto",
        _ => return None,
    })
}

fn open(endpoint: &str, read_only: bool) -> Result<Connection, DriverError> {
    let cannot_open = |error: duckdb::Error| {
        DriverError::new(
            DriverErrorCategory::Configuration,
            format!("cannot open {endpoint}: {error}"),
        )
    };
    if endpoint == ":memory:" {
        return Connection::open_in_memory().map_err(cannot_open);
    }
    let path = Path::new(endpoint);
    if let Some(reader) = reader(path) {
        return open_data_file(path, reader).map_err(cannot_open);
    }
    if read_only && !path.exists() {
        return Err(DriverError::new(
            DriverErrorCategory::Configuration,
            format!("{endpoint} does not exist, and a read-only connection cannot create it"),
        ));
    }
    let config = if read_only {
        Config::default()
            .access_mode(AccessMode::ReadOnly)
            .map_err(cannot_open)?
    } else {
        Config::default()
    };
    Connection::open_with_flags(path, config).map_err(cannot_open)
}

/// An in-memory database with the file as a view named after it, `sales.parquet` as
/// `sales`: the file browses like a table and is only ever read. The view reads the
/// file as it is when queried; creating it reads its header, so a file that is missing
/// or not what its name says fails here and not on the first query.
fn open_data_file(path: &Path, reader: &str) -> Result<Connection, duckdb::Error> {
    let conn = Connection::open_in_memory()?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split('.').next())
        .filter(|name| !name.is_empty())
        .unwrap_or("data");
    let literal = format!("'{}'", path.display().to_string().replace('\'', "''"));
    conn.execute_batch(&format!(
        "CREATE VIEW {} AS SELECT * FROM {reader}({literal})",
        quote(name)
    ))?;
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
            "DuckDB schema changes are written in the editor; the schema editor does not plan them yet",
        ),
        CapabilityState::available(Capability::Explain),
        CapabilityState::available(Capability::ExplainAnalyze),
        CapabilityState::unavailable(
            Capability::Admin,
            "DuckDB has no users, grants, server sessions or locks to administer",
        ),
        CapabilityState::available(Capability::Import),
        CapabilityState::available(Capability::Export),
        CapabilityState::unavailable(
            Capability::Backup,
            "a DuckDB database is its file: copy the file, or EXPORT DATABASE, to back it up",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::reader;

    #[test]
    fn data_files_are_read_by_their_extension() {
        assert_eq!(reader(Path::new("/d/sales.CSV")), Some("read_csv_auto"));
        assert_eq!(reader(Path::new("/d/sales.csv.gz")), Some("read_csv_auto"));
        assert_eq!(reader(Path::new("s.parquet")), Some("read_parquet"));
        assert_eq!(reader(Path::new("s.jsonl")), Some("read_json_auto"));
        assert_eq!(reader(Path::new("shop.duckdb")), None);
        assert_eq!(reader(Path::new("shop")), None);
    }
}
