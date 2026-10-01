use dexo_driver_api::{ColumnMeta, DbValue};
use rusqlite::types::{Value, ValueRef};

pub fn column_meta(column: &rusqlite::Column<'_>) -> ColumnMeta {
    ColumnMeta {
        name: column.name().to_string(),
        type_name: column.decl_type().unwrap_or_default().to_string(),
        nullable: true,
    }
}

/// A cell by its storage class, which is what SQLite keeps, whatever the column was
/// declared as. A REAL keeps the shortest text that reads back to the same number.
pub fn decode(value: ValueRef<'_>) -> DbValue {
    match value {
        ValueRef::Null => DbValue::Null,
        ValueRef::Integer(value) => DbValue::I64(value),
        ValueRef::Real(value) => DbValue::Native {
            type_name: "real".into(),
            bytes: Vec::new(),
            text: format!("{value:?}"),
        },
        ValueRef::Text(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => DbValue::Text(text.to_string()),
            Err(_) => DbValue::Bytes(bytes.to_vec()),
        },
        ValueRef::Blob(bytes) => DbValue::Bytes(bytes.to_vec()),
    }
}

/// What a value binds as. A REAL read from the file goes back as a REAL, so a row's
/// original values still match it when an edit is keyed by them.
pub fn to_sql(value: &DbValue) -> Value {
    match value {
        DbValue::Null => Value::Null,
        DbValue::Bool(value) => Value::Integer(i64::from(*value)),
        DbValue::I64(value) => Value::Integer(*value),
        DbValue::U64(value) => i64::try_from(*value)
            .map(Value::Integer)
            .unwrap_or_else(|_| Value::Text(value.to_string())),
        DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Json(text) => {
            Value::Text(text.clone())
        }
        DbValue::Bytes(bytes) => Value::Blob(bytes.clone()),
        DbValue::Native {
            type_name, text, ..
        } if type_name == "real" => text
            .parse()
            .map(Value::Real)
            .unwrap_or_else(|_| Value::Text(text.clone())),
        DbValue::Native { text, .. } => Value::Text(text.clone()),
    }
}

pub fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::{decode, quote, to_sql};
    use rusqlite::types::{Value, ValueRef};

    #[test]
    fn reals_read_back_as_the_same_number() {
        for number in [0.1, 1.0, 1e300, -2.5e-12] {
            let value = decode(ValueRef::Real(number));
            assert_eq!(to_sql(&value), Value::Real(number));
        }
        assert_eq!(quote("a\"b"), "\"a\"\"b\"");
    }
}
