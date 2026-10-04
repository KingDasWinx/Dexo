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
        "INTERNAL" | "FATAL" | "Out of Memory" => DriverErrorCategory::Internal,
        "HTTP" => DriverErrorCategory::Network,
        // The rest is the statement's: a file it names that is not there (`IO`), a
        // constraint it breaks -- filed as the other drivers file a server's refusal.
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

#[cfg(test)]
mod tests {
    use dexo_driver_api::DriverErrorCategory;

    use super::map_error;

    fn failure(message: &str) -> duckdb::Error {
        duckdb::Error::DuckDBFailure(
            duckdb::ffi::Error::new(duckdb::ffi::DuckDBError),
            Some(message.into()),
        )
    }

    #[test]
    fn errors_are_filed_by_their_kind() {
        let category = |message: &str| map_error(failure(message)).category();
        assert_eq!(
            category("IO Error: No files found that match the pattern \"nope.csv\""),
            DriverErrorCategory::Syntax
        );
        assert_eq!(category("HTTP Error: 404"), DriverErrorCategory::Network);
        assert_eq!(
            category("INTERRUPT Error: Interrupted!"),
            DriverErrorCategory::Cancelled
        );
        assert_eq!(
            category("Invalid Input Error: Cannot execute statement in read-only mode!"),
            DriverErrorCategory::Permission
        );
        assert_eq!(
            category("Out of Memory Error: failed"),
            DriverErrorCategory::Internal
        );
    }
}
