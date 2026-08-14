use dexo_driver_api::{ColumnMeta, DbValue};
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::{Column, Row, Value};

pub fn column_meta(column: &Column) -> ColumnMeta {
    ColumnMeta {
        name: String::from_utf8_lossy(column.name_ref()).into_owned(),
        type_name: format!("{:?}", column.column_type()),
        nullable: true,
    }
}

pub fn decode_row(row: &Row) -> Vec<DbValue> {
    (0..row.len())
        .map(|idx| decode_value(&row[idx], row.columns_ref().get(idx)))
        .collect()
}

fn decode_value(value: &Value, column: Option<&Column>) -> DbValue {
    match value {
        Value::NULL => DbValue::Null,
        Value::Int(value) => DbValue::I64(*value),
        Value::UInt(value) => DbValue::U64(*value),
        Value::Bytes(bytes) => decode_bytes(bytes, column),
        Value::Float(value) => DbValue::Native {
            type_name: "float".into(),
            bytes: value.to_le_bytes().to_vec(),
            text: value.to_string(),
        },
        Value::Double(value) => DbValue::Native {
            type_name: "double".into(),
            bytes: value.to_le_bytes().to_vec(),
            text: value.to_string(),
        },
        Value::Date(year, month, day, hour, minute, second, micros) => {
            if *year == 0 && *month == 0 && *day == 0 {
                DbValue::Native {
                    type_name: "date".into(),
                    bytes: vec![],
                    text: "0000-00-00".into(),
                }
            } else {
                DbValue::Native {
                    type_name: "datetime".into(),
                    bytes: vec![],
                    text: format!(
                        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}"
                    ),
                }
            }
        }
        Value::Time(neg, days, hours, minutes, seconds, micros) => DbValue::Native {
            type_name: "time".into(),
            bytes: vec![],
            text: format!(
                "{}{days} {hours:02}:{minutes:02}:{seconds:02}.{micros:06}",
                if *neg { "-" } else { "" }
            ),
        },
    }
}

fn decode_bytes(bytes: &[u8], column: Option<&Column>) -> DbValue {
    let text = String::from_utf8_lossy(bytes).into_owned();
    if let Some(column) = column {
        match column.column_type() {
            ColumnType::MYSQL_TYPE_TINY
            | ColumnType::MYSQL_TYPE_SHORT
            | ColumnType::MYSQL_TYPE_LONG
            | ColumnType::MYSQL_TYPE_INT24
            | ColumnType::MYSQL_TYPE_LONGLONG
            | ColumnType::MYSQL_TYPE_YEAR => {
                if column.flags().contains(ColumnFlags::UNSIGNED_FLAG) {
                    return text
                        .parse::<u64>()
                        .map(DbValue::U64)
                        .unwrap_or(DbValue::Text(text));
                }
                return text
                    .parse::<i64>()
                    .map(DbValue::I64)
                    .or_else(|_| text.parse::<u64>().map(DbValue::U64))
                    .unwrap_or(DbValue::Text(text));
            }
            ColumnType::MYSQL_TYPE_JSON => return DbValue::Json(text),
            ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => {
                return DbValue::Decimal(text);
            }
            ColumnType::MYSQL_TYPE_ENUM | ColumnType::MYSQL_TYPE_SET => return DbValue::Text(text),
            ColumnType::MYSQL_TYPE_TINY_BLOB
            | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
            | ColumnType::MYSQL_TYPE_LONG_BLOB
            | ColumnType::MYSQL_TYPE_BLOB => {
                if let Ok(text) = std::str::from_utf8(bytes) {
                    return DbValue::Text(text.to_string());
                }
                return DbValue::Bytes(bytes.to_vec());
            }
            _ => {}
        }
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        DbValue::Text(text.to_string())
    } else {
        DbValue::Bytes(bytes.to_vec())
    }
}
