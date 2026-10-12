use dexo_driver_api::{DriverError, DriverErrorCategory};
use tokio_postgres::error::ErrorPosition;

/// Keeps what the server said: the SQLSTATE, the DETAIL and HINT lines, and where in the
/// statement it failed. A failure that never reached the server (a closed socket, a
/// refused connection) keeps its own text instead of a generic "query failed".
pub fn map_error(error: tokio_postgres::Error) -> DriverError {
    if error.code().is_some_and(|code| code.code() == "57014") {
        // The server's own statement_timeout cancels the statement too: that is a
        // timeout, not a cancel the person asked for.
        if error
            .as_db_error()
            .is_some_and(|db| db.message().contains("statement timeout"))
        {
            return DriverError::new(DriverErrorCategory::Timeout, "query timed out");
        }
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
    match error.as_db_error() {
        Some(db) => server_category(db.code().code()),
        None => DriverErrorCategory::Internal,
    }
}

/// What a SQLSTATE the server answered with is.
fn server_category(code: &str) -> DriverErrorCategory {
    if code == "42501" {
        return DriverErrorCategory::Permission;
    }
    // Class 28 is a rejected login (a wrong password is 28P01); class 08 a connection
    // that failed. Neither is the statement's fault.
    if code.starts_with("28") {
        return DriverErrorCategory::Authentication;
    }
    // 57P01 to 57P03: the server ended this session, shut down or is not taking
    // connections -- the statement is not what failed.
    if code.starts_with("08") || matches!(code, "57P01" | "57P02" | "57P03") {
        return DriverErrorCategory::Network;
    }
    // Class 23 is a constraint the data broke -- a duplicate key, a foreign key -- and
    // class 40 a deadlock or a serialization failure: the statement was well formed.
    if code.starts_with("23") || code.starts_with("40") {
        return DriverErrorCategory::Conflict;
    }
    DriverErrorCategory::Syntax
}

pub fn is_permission(error: &tokio_postgres::Error) -> bool {
    error
        .as_db_error()
        .is_some_and(|db| db.code().code() == "42501")
}

#[cfg(test)]
mod tests {
    use dexo_driver_api::DriverErrorCategory;

    use super::server_category;

    /// A broken constraint or a deadlock is a conflict to retry or resolve, not SQL to fix.
    #[test]
    fn a_broken_constraint_is_a_conflict_not_bad_sql() {
        for code in ["23505", "23503", "40001", "40P01"] {
            assert_eq!(
                server_category(code),
                DriverErrorCategory::Conflict,
                "{code}"
            );
        }
        assert_eq!(server_category("42601"), DriverErrorCategory::Syntax);
        assert_eq!(
            server_category("28P01"),
            DriverErrorCategory::Authentication
        );
    }
}
