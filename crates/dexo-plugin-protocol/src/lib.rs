//! The protocol between Dexo and a driver plugin: JSON-RPC 2.0, one message per line,
//! over the plugin's stdin and stdout. Both sides use these types, so the two ends cannot
//! drift apart the way two copies of a wire format do.

mod messages;

pub use messages::*;

use std::io::{self, BufRead, Read, Write};

use dexo_driver_api::{DriverError, DriverErrorCategory};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol version this build speaks. Version 1 is frozen once released: changes
/// come as new families, which a session negotiates, or as version 2.
pub const PROTOCOL: u32 = 1;
/// A longer line is a protocol error, not a reason to run out of memory.
pub const MAX_LINE: usize = 64 * 1024 * 1024;
/// A page of a result holds at most this many rows...
pub const PAGE_ROWS: usize = 500;
/// ...or about this many bytes of values, whichever comes first.
pub const PAGE_BYTES: usize = 4 * 1024 * 1024;

pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const SESSION_OPEN: &str = "session/open";
    pub const SESSION_CLOSE: &str = "session/close";
    pub const QUERY_EXECUTE: &str = "query/execute";
    pub const QUERY_FETCH: &str = "query/fetch";
    pub const QUERY_CLOSE: &str = "query/close";
    pub const QUERY_CANCEL: &str = "query/cancel";
    pub const CATALOG_LIST_CHILDREN: &str = "catalog/list_children";
    pub const CATALOG_OBJECT: &str = "catalog/object";
    pub const CATALOG_DDL: &str = "catalog/ddl";
    pub const CATALOG_DEPENDENCIES: &str = "catalog/dependencies";
    pub const CATALOG_DEPENDENTS: &str = "catalog/dependents";
    pub const CATALOG_FOREIGN_KEYS: &str = "catalog/foreign_keys";
    pub const CATALOG_RELATIONS_NAMED: &str = "catalog/relations_named";
    pub const CATALOG_DATABASES: &str = "catalog/databases";
    pub const DATA_FETCH: &str = "data/fetch";
    pub const DATA_FETCH_VALUE: &str = "data/fetch_value";
    pub const DATA_APPLY: &str = "data/apply";
    pub const DATA_TABLE_COLUMNS: &str = "data/table_columns";
    pub const DATA_ESTIMATE_ROWS: &str = "data/estimate_rows";
    pub const BULK_INSERT_BATCH: &str = "bulk/insert_batch";
    pub const EXPLAIN: &str = "explain/explain";
    pub const TX_BEGIN: &str = "tx/begin";
    pub const TX_COMMIT: &str = "tx/commit";
    pub const TX_ROLLBACK: &str = "tx/rollback";
    pub const TX_SAVEPOINT: &str = "tx/savepoint";
    pub const TX_ROLLBACK_TO: &str = "tx/rollback_to";
    pub const TX_RELEASE_SAVEPOINT: &str = "tx/release_savepoint";
    /// The host stopped waiting for request `id`: the plugin drops its work, as a driver
    /// drops a future nobody polls. Not answered.
    pub const CANCEL_REQUEST: &str = "$/cancel_request";
}

/// JSON-RPC's error codes, then the protocol's own.
pub mod code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    /// The driver failed; `data` is the `DriverError`, every field of it.
    pub const DRIVER: i64 = 1;
    pub const UNKNOWN_SESSION: i64 = 2;
    pub const UNKNOWN_CURSOR: i64 = 3;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    pub fn new(id: u64, method: &str, params: impl Serialize) -> serde_json::Result<Self> {
        Ok(Self {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params: serde_json::to_value(params)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(id: u64, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: u64, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// A driver's error, every field of it in `data`.
    pub fn driver(error: &DriverError) -> Self {
        Self {
            code: code::DRIVER,
            message: error.to_string(),
            data: serde_json::to_value(error).ok(),
        }
    }

    /// The driver's error this carries, or one made of its message.
    pub fn into_driver_error(self) -> DriverError {
        if self.code == code::DRIVER
            && let Some(error) = self
                .data
                .and_then(|data| serde_json::from_value::<DriverError>(data).ok())
        {
            return error;
        }
        DriverError::new(DriverErrorCategory::Internal, self.message)
    }
}

/// Reads one message line into `line`, its line ending dropped. `Ok(false)` at the end
/// of input; a line over [`MAX_LINE`] is `InvalidData`.
pub fn read_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<bool> {
    read_line_with(reader, line, MAX_LINE)
}

/// [`read_line`] with a cap of `max` bytes.
pub fn read_line_with(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    max: usize,
) -> io::Result<bool> {
    line.clear();
    let read = reader
        .by_ref()
        .take(max as u64 + 1)
        .read_until(b'\n', line)?;
    if read == 0 {
        return Ok(false);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
    } else if line.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a message is longer than the protocol allows",
        ));
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(true)
}

/// Writes `message` as one line and flushes it.
pub fn write_line(writer: &mut impl Write, message: &impl Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()
}
