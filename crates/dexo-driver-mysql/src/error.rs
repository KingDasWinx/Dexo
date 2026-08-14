use dexo_driver_api::{DriverError, DriverErrorCategory};

pub fn map_error(error: mysql_async::Error) -> DriverError {
    let message = error.to_string();
    if message.to_ascii_lowercase().contains("kill")
        || message.to_ascii_lowercase().contains("interrupted")
        || message.to_ascii_lowercase().contains("1317")
    {
        return DriverError::new(DriverErrorCategory::Cancelled, "query cancelled");
    }
    if is_permission(&error) {
        return DriverError::new(DriverErrorCategory::Permission, "mysql permission denied");
    }
    DriverError::new(DriverErrorCategory::Internal, "mysql query failed")
        .with_native_code(message.chars().take(32).collect::<String>())
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
