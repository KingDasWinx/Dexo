use dexo_driver_api::{DriverError, DriverErrorCategory};
use tokio_postgres::error::ErrorPosition;

/// Keeps what the server said: the SQLSTATE, the DETAIL and HINT lines, and where in the
/// statement it failed. A failure that never reached the server (a closed socket, a
/// refused connection) keeps its own text instead of a generic "query failed".
pub fn map_error(error: tokio_postgres::Error) -> DriverError {
    if error.code().is_some_and(|code| code.code() == "57014") {
        return DriverError::new(DriverErrorCategory::Cancelled, "query cancelled");
    }
    let Some(db) = error.as_db_error() else {
        return DriverError::new(category(&error), error.to_string());
    };
    let mut mapped =
        DriverError::new(category(&error), db.message()).with_native_code(db.code().code());
    if let Some(detail) = db.detail() {
        mapped = mapped.with_detail(detail);
    }
    if let Some(hint) = db.hint() {
        mapped = mapped.with_hint(hint);
    }
    if let Some(ErrorPosition::Original(position)) = db.position() {
        mapped = mapped.with_position(*position);
    }
    mapped
}

fn category(error: &tokio_postgres::Error) -> DriverErrorCategory {
    if error.is_closed() {
        return DriverErrorCategory::Network;
    }
    if let Some(db) = error.as_db_error() {
        if db.code().code() == "42501" {
            return DriverErrorCategory::Permission;
        }
        return DriverErrorCategory::Syntax;
    }
    DriverErrorCategory::Internal
}

pub fn is_permission(error: &tokio_postgres::Error) -> bool {
    error
        .as_db_error()
        .is_some_and(|db| db.code().code() == "42501")
}
