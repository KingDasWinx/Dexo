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

pub fn is_permission(error: &mysql_async::Error) -> bool {
    match error {
        mysql_async::Error::Server(err) => matches!(err.code, 1044 | 1142 | 1143 | 1227 | 1370),
        _ => {
            let message = error.to_string().to_ascii_lowercase();
            message.contains("denied") || message.contains("access")
        }
    }
}
