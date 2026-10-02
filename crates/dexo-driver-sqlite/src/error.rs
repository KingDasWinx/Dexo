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
    let message = friendly(&message);
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

/// What SQLite says, with a sentence in front of the three a person meets most: its own
/// text stays in brackets, for whoever searches for it.
fn friendly(message: &str) -> String {
    let columns = |list: &str| -> (String, Vec<String>) {
        let mut table = String::new();
        let names = list
            .split(", ")
            .map(|qualified| match qualified.rsplit_once('.') {
                Some((owner, column)) => {
                    table = owner.to_string();
                    column.to_string()
                }
                None => qualified.to_string(),
            })
            .collect();
        (table, names)
    };
    if let Some(list) = message.strip_prefix("UNIQUE constraint failed: ") {
        let (table, names) = columns(list);
        return format!(
            "A row with the same {} already exists in {table} ({message})",
            names.join(", ")
        );
    }
    if let Some(list) = message.strip_prefix("NOT NULL constraint failed: ") {
        let (table, names) = columns(list);
        return format!(
            "{} in {table} cannot be empty ({message})",
            names.join(", ")
        );
    }
    if message == "datatype mismatch" {
        return "A value does not fit the type of its column (datatype mismatch)".into();
    }
    message.to_string()
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

#[cfg(test)]
mod tests {
    use super::friendly;

    #[test]
    fn the_common_constraint_errors_say_what_happened() {
        assert_eq!(
            friendly("UNIQUE constraint failed: tbl.id"),
            "A row with the same id already exists in tbl (UNIQUE constraint failed: tbl.id)"
        );
        assert_eq!(
            friendly("NOT NULL constraint failed: tbl.name"),
            "name in tbl cannot be empty (NOT NULL constraint failed: tbl.name)"
        );
        assert_eq!(
            friendly("datatype mismatch"),
            "A value does not fit the type of its column (datatype mismatch)"
        );
        assert_eq!(friendly("no such table: x"), "no such table: x");
    }
}
