use dexo_driver_api::{DriverError, DriverErrorCategory};

/// Keeps DuckDB's own message. Its first words name the kind of error -- `Catalog
/// Error`, `Constraint Error` -- which is kept as the native code, and it points at the
/// failing text itself.
pub fn map_error(error: duckdb::Error) -> DriverError {
    let duckdb::Error::DuckDBFailure(_, Some(message)) = &error else {
        return internal(error);
    };
    let kind = message.split_once(" Error:").map_or("", |(kind, _)| kind);
    let category = match kind {
        "INTERRUPT" => {
            return DriverError::new(DriverErrorCategory::Cancelled, "query cancelled");
        }
        "Permission" => DriverErrorCategory::Permission,
        _ if message.contains("read-only mode") => DriverErrorCategory::Permission,
        "TransactionContext" if message.contains("Conflict") => {
            return DriverError::new(DriverErrorCategory::Conflict, message.clone())
                .with_native_code(kind)
                .retryable();
        }
        "INTERNAL" | "FATAL" | "Out of Memory" | "IO" => DriverErrorCategory::Internal,
        _ => DriverErrorCategory::Syntax,
    };
    let mapped = DriverError::new(category, message.clone());
    if kind.is_empty() {
        mapped
    } else {
        mapped.with_native_code(kind)
    }
}

pub fn internal(error: impl std::fmt::Display) -> DriverError {
    DriverError::new(DriverErrorCategory::Internal, error.to_string())
}

/// What a statement that writes gets where the session may only read.
pub fn writes_refused() -> DriverError {
    DriverError::new(
        DriverErrorCategory::Permission,
        "this statement writes, and here it may only read",
    )
}
