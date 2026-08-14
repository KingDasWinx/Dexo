use dexo_driver_api::{DriverError, DriverErrorCategory};

pub fn map_error(error: tokio_postgres::Error) -> DriverError {
    if error.code().is_some_and(|code| code.code() == "57014") {
        return DriverError::new(DriverErrorCategory::Cancelled, "query cancelled");
    }
    let mut mapped = DriverError::new(category(&error), safe_message(&error));
    if let Some(db) = error.as_db_error() {
        mapped = mapped.with_native_code(db.code().code());
        if let Some(position) = db.hint() {
            let _ = position;
        }
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

fn safe_message(error: &tokio_postgres::Error) -> String {
    error
        .as_db_error()
        .map(|error| error.message().to_string())
        .unwrap_or_else(|| "postgres query failed".into())
}
