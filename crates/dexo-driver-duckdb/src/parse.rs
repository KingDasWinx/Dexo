//! DuckDB's own reading of SQL text. Dexo splits a script and judges each statement with
//! its own parser; where the two read a text differently -- a `--` comment ended by a
//! bare carriage return, say -- what DuckDB runs is not what Dexo checked. These ask
//! DuckDB itself.

use std::ffi::CString;
use std::sync::{Mutex, OnceLock, PoisonError};

use dexo_driver_api::{DriverError, DriverErrorCategory};
use duckdb::{Connection, ffi};

use crate::error::map_error;

/// A connection to an empty in-memory database, kept for the life of the process: it
/// only ever parses, which needs no catalog.
struct Parser {
    connection: ffi::duckdb_connection,
}

// SAFETY: a DuckDB connection may be used from any thread, one call at a time; the
// mutex around it sees to that.
unsafe impl Send for Parser {}

fn parser() -> Option<&'static Mutex<Parser>> {
    static PARSER: OnceLock<Option<Mutex<Parser>>> = OnceLock::new();
    PARSER
        .get_or_init(|| {
            // SAFETY: both out-pointers are valid; on failure nothing is kept.
            unsafe {
                let mut database = std::ptr::null_mut();
                if ffi::duckdb_open(std::ptr::null(), &mut database) != ffi::DuckDBSuccess {
                    return None;
                }
                let mut connection = std::ptr::null_mut();
                if ffi::duckdb_connect(database, &mut connection) != ffi::DuckDBSuccess {
                    ffi::duckdb_close(&mut database);
                    return None;
                }
                Some(Mutex::new(Parser { connection }))
            }
        })
        .as_ref()
}

/// How many statements DuckDB reads in `sql`, comments and empty ones not counted;
/// `None` when it cannot parse it, which running it would report anyway.
pub fn statement_count(sql: &str) -> Option<usize> {
    parse(sql).ok()
}

/// [`statement_count`], or DuckDB's own words on why it cannot parse `sql`.
pub fn parse(sql: &str) -> Result<usize, DriverError> {
    let syntax = |message: String| DriverError::new(DriverErrorCategory::Syntax, message);
    let text = CString::new(sql).map_err(|_| syntax("the SQL holds a NUL character".into()))?;
    let Some(parser) = parser() else {
        return Err(DriverError::new(
            DriverErrorCategory::Internal,
            "DuckDB's parser did not start",
        ));
    };
    let parser = parser.lock().unwrap_or_else(PoisonError::into_inner);
    // SAFETY: the connection lives for the process and is used under the lock; the
    // error text is copied before the extracted statements are destroyed.
    unsafe {
        let mut extracted = std::ptr::null_mut();
        let count =
            ffi::duckdb_extract_statements(parser.connection, text.as_ptr(), &mut extracted);
        let error = ffi::duckdb_extract_statements_error(extracted);
        let error = (!error.is_null()).then(|| {
            std::ffi::CStr::from_ptr(error)
                .to_string_lossy()
                .into_owned()
        });
        ffi::duckdb_destroy_extracted(&mut extracted);
        match error {
            Some(message) => Err(syntax(message)),
            None => Ok(usize::try_from(count).unwrap_or(usize::MAX)),
        }
    }
}

/// Whether DuckDB reads every statement of `sql` as a query: SELECT, FROM-first, VALUES,
/// WITH, and the SUMMARIZE, DESCRIBE and SHOW it turns into queries. Its JSON
/// serialisation of a statement takes only queries, so it answers for DuckDB's grammar,
/// not Dexo's. A query can still call a function that writes; that is Dexo's check.
pub fn only_queries(conn: &Connection, sql: &str) -> Result<bool, DriverError> {
    let json: String = conn
        .query_row(
            "SELECT json_serialize_sql(?::VARCHAR)::VARCHAR",
            [sql],
            |row| row.get(0),
        )
        .map_err(map_error)?;
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
    Ok(parsed.get("error") == Some(&serde_json::Value::Bool(false)))
}

#[cfg(test)]
mod tests {
    use super::{only_queries, statement_count};

    #[test]
    fn duckdb_counts_the_statements_it_would_run() {
        assert_eq!(statement_count("select 1"), Some(1));
        assert_eq!(statement_count("select 1; select 2;"), Some(2));
        assert_eq!(statement_count("select 1 --\r; drop table t --\n"), Some(2));
        assert_eq!(statement_count("-- only a comment"), Some(0));
        assert_eq!(statement_count("selec 1"), None);
    }

    #[test]
    fn only_queries_are_queries() {
        let conn = duckdb::Connection::open_in_memory().unwrap();
        for sql in [
            "select 1",
            "from t",
            "summarize t",
            "show tables",
            "select 1; select 2",
        ] {
            assert!(only_queries(&conn, sql).unwrap(), "{sql}");
        }
        for sql in [
            "select 1 --\r; copy t to 'x.csv' --\n",
            "copy t to 'x.csv'",
            "attach 'x.duckdb'",
            "pivot t on a using count(*)",
            "explain select 1",
            "selec 1",
        ] {
            assert!(!only_queries(&conn, sql).unwrap(), "{sql}");
        }
    }
}
