use dexo_driver_api::{DriverError, DriverErrorCategory};
use rusqlite::ErrorCode;

/// Keeps SQLite's own message and its extended result code (`2067` is a UNIQUE
/// constraint), and where in the statement a syntax error sits.
pub fn map_error(error: rusqlite::Error) -> DriverError {
    let (code, message, position) = match &error {
        rusqlite::Error::SqliteFailure(code, message) => (
            *code,
            message.clone().unwrap_or_else(|| code.to_string()),
            None,
        ),
        rusqlite::Error::SqlInputError {
            error,
            msg,
            sql,
            offset,
        } => (*error, msg.clone(), position(sql, *offset)),
        _ => return DriverError::new(DriverErrorCategory::Internal, error.to_string()),
    };
    let category = match code.code {
        ErrorCode::OperationInterrupted => {
            return DriverError::new(DriverErrorCategory::Cancelled, "query cancelled");
        }
        ErrorCode::ReadOnly
        | ErrorCode::PermissionDenied
        | ErrorCode::AuthorizationForStatementDenied => DriverErrorCategory::Permission,
        ErrorCode::CannotOpen | ErrorCode::NotADatabase | ErrorCode::NotFound => {
            DriverErrorCategory::Configuration
        }
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => DriverErrorCategory::Timeout,
        ErrorCode::SystemIoFailure | ErrorCode::DiskFull | ErrorCode::DatabaseCorrupt => {
            DriverErrorCategory::Internal
        }
        _ => DriverErrorCategory::Syntax,
    };
    let mut mapped =
        DriverError::new(category, message).with_native_code(code.extended_code.to_string());
    if let Some(position) = position {
        mapped = mapped.with_position(position);
    }
    if matches!(
        code.code,
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
    ) {
        mapped = mapped.retryable();
    }
    mapped
}

/// SQLite reports a byte offset from 0; the contract counts characters from 1.
fn position(sql: &str, offset: i32) -> Option<u32> {
    let offset = usize::try_from(offset).ok()?;
    let chars = sql.get(..offset)?.chars().count();
    u32::try_from(chars + 1).ok()
}

pub fn internal(error: impl std::fmt::Display) -> DriverError {
    DriverError::new(DriverErrorCategory::Internal, error.to_string())
}
