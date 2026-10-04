use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

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
        let (conn, database) = tokio::task::spawn_blocking(move || open(&endpoint, read_only))
            .await
            .map_err(internal)??;
        Ok(Box::new(DuckdbSession::new(
            conn,
            read_only || data_file,
            database,
        )))
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

/// One database per file in the process, which every session on the file shares through
/// a connection of its own. A second engine on a file the first has open checkpoints, as
/// it closes, and deletes the write-ahead log the first is still writing to: a crash
/// after that lost committed work. The database closes with its last session.
pub(crate) struct SharedDatabase {
    root: Mutex<Connection>,
    read_only: bool,
}

fn databases() -> &'static Mutex<HashMap<PathBuf, Weak<SharedDatabase>>> {
    static OPEN: OnceLock<Mutex<HashMap<PathBuf, Weak<SharedDatabase>>>> = OnceLock::new();
    OPEN.get_or_init(Mutex::default)
}

/// Extensions DuckDB does not have are never fetched on their own: the one request Dexo
/// makes by itself is its update check. One installed already still loads when a query
/// needs it. Settings stay unlocked -- EXPLAIN ANALYZE turns the profiler on and off --
/// and a read-only session refuses the SET, PRAGMA, INSTALL and LOAD that would change
/// them, which are not queries.
fn config(read_only: bool) -> Result<Config, duckdb::Error> {
    // Types Arrow has none for -- TIMETZ, UHUGEINT, BIGNUM -- arrive as their own bytes,
    // not as a nearest Arrow type that loses the offset or wraps the number.
    let config = Config::default()
        .with("autoinstall_known_extensions", "false")?
        .with("arrow_lossless_conversion", "true")?;
    if read_only {
        config.access_mode(AccessMode::ReadOnly)
    } else {
        Ok(config)
    }
}

type Opened = (Connection, Option<Arc<SharedDatabase>>);

fn open(endpoint: &str, read_only: bool) -> Result<Opened, DriverError> {
    let cannot_open = |error: duckdb::Error| {
        DriverError::new(
            DriverErrorCategory::Configuration,
            format!("cannot open {endpoint}: {error}"),
        )
    };
    if endpoint == ":memory:" {
        let conn = Connection::open_in_memory_with_flags(config(false).map_err(cannot_open)?)
            .map_err(cannot_open)?;
        return Ok((conn, None));
    }
    let path = Path::new(endpoint);
    if let Some(reader) = reader(path) {
        return Ok((open_data_file(path, reader).map_err(cannot_open)?, None));
    }
    let key = std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .map_err(|error| {
            DriverError::new(
                DriverErrorCategory::Configuration,
                format!("cannot open {endpoint}: {error}"),
            )
        })?;
    let mut open = databases().lock().unwrap_or_else(PoisonError::into_inner);
    open.retain(|_, database| database.strong_count() > 0);
    if let Some(database) = open.get(&key).and_then(Weak::upgrade) {
        // A read-only session on a file open for writing shares that database, and
        // Dexo's own checks keep it to reads.
        if database.read_only && !read_only {
            return Err(DriverError::new(
                DriverErrorCategory::Configuration,
                format!(
                    "{endpoint} is open read-only in this Dexo: close its read-only connections to open it for writing"
                ),
            ));
        }
        let conn = database
            .root
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .try_clone()
            .map_err(cannot_open)?;
        return Ok((conn, Some(database)));
    }
    if read_only && !path.exists() {
        return Err(DriverError::new(
            DriverErrorCategory::Configuration,
            format!("{endpoint} does not exist, and a read-only connection cannot create it"),
        ));
    }
    let root = Connection::open_with_flags(path, config(read_only).map_err(cannot_open)?)
        .map_err(cannot_open)?;
    let conn = root.try_clone().map_err(cannot_open)?;
    let database = Arc::new(SharedDatabase {
        root: Mutex::new(root),
        read_only,
    });
    open.insert(key, Arc::downgrade(&database));
    Ok((conn, Some(database)))
}

/// An in-memory database with the file as a view named after it, `sales.parquet` as
/// `sales`: the file browses like a table and is only ever read. The view reads the
/// file as it is when queried; creating it reads its header, so a file that is missing
/// or not what its name says fails here and not on the first query.
fn open_data_file(path: &Path, reader: &str) -> Result<Connection, duckdb::Error> {
    let conn = Connection::open_in_memory_with_flags(config(false)?)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split('.').next())
        .filter(|name| !name.is_empty())
        .unwrap_or("data");
    let Some(text) = path.to_str() else {
        return Err(duckdb::Error::InvalidPath(path.to_path_buf()));
    };
    // DuckDB reads `[`, `*` and `?` in a file name as a glob: `s[1].csv` read `s1.csv`.
    // Each stands for itself inside brackets.
    let literal = text
        .replace('[', "[[]")
        .replace('*', "[*]")
        .replace('?', "[?]")
        .replace('\'', "''");
    let literal = format!("'{literal}'");
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
            "a DuckDB database is its file: copy it to back it up, replace it to restore",
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
