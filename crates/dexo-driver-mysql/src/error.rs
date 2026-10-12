use dexo_driver_api::{DriverError, DriverErrorCategory};

/// Server errors keep the server's own message, its error number and its SQLSTATE, as
/// `1146 (42S02)`. Every failure used to read "mysql query failed", with the real text
/// cut to 32 characters and filed as the code.
pub fn map_error(error: mysql_async::Error) -> DriverError {
    if let mysql_async::Error::Server(server) = &error {
        if server.code == 1317 {
            return DriverError::new(DriverErrorCategory::Cancelled, "query cancelled");
        }
        // 1045 is a rejected login: a wrong password, or a user that does not exist.
        let category = if server.code == 1045 {
            DriverErrorCategory::Authentication
        } else if is_permission(&error) {
            DriverErrorCategory::Permission
        } else if is_conflict(&server.state) {
            DriverErrorCategory::Conflict
        } else {
            DriverErrorCategory::Syntax
        };
        return DriverError::new(category, server.message.clone())
            .with_native_code(format!("{} ({})", server.code, server.state));
    }
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    if lower.contains("kill") || lower.contains("interrupted") {
        return DriverError::new(DriverErrorCategory::Cancelled, "query cancelled");
    }
    let category = match &error {
        mysql_async::Error::Io(_) => DriverErrorCategory::Network,
        _ if is_permission(&error) => DriverErrorCategory::Permission,
        _ => DriverErrorCategory::Internal,
    };
    DriverError::new(category, message)
}

/// SQLSTATE class 23 is a constraint the data broke -- a duplicate key, a foreign key --
/// and class 40 a deadlock: the statement was well formed.
fn is_conflict(state: &str) -> bool {
    state.starts_with("23") || state.starts_with("40")
}

pub fn is_permission(error: &mysql_async::Error) -> bool {
    match error {
        mysql_async::Error::Server(err) => matches!(err.code, 1044 | 1142 | 1143 | 1227 | 1370),
        _ => {
            let message = error.to_string().to_ascii_lowercase();
            message.contains("denied") || message.contains("access")
        }
    }
}

#[cfg(test)]
mod tests {
    /// A duplicate key or a deadlock is a conflict to retry or resolve, not SQL to fix.
    #[test]
    fn a_broken_constraint_is_a_conflict_not_bad_sql() {
        assert!(super::is_conflict("23000"));
        assert!(super::is_conflict("40001"));
        assert!(!super::is_conflict("42S02"));
    }
}
