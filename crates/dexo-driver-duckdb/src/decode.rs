use dexo_driver_api::{ColumnMeta, DbValue};
use duckdb::arrow::array::{Array, ArrayRef, AsArray};
use duckdb::arrow::datatypes::{DataType, Field, UInt64Type};
use duckdb::core::{LogicalTypeHandle, LogicalTypeId};
use duckdb::types::Value;

use crate::render;

pub fn column_meta(name: &str, logical: &LogicalTypeHandle) -> ColumnMeta {
    ColumnMeta {
        name: name.to_string(),
        type_name: type_name(logical),
        nullable: true,
    }
}

/// DuckDB's name for a result column's type: `DECIMAL(10,2)`, `INTEGER[]`,
/// `STRUCT(a INTEGER)`, and an alias such as `JSON` by its alias. The C API gives neither
/// an ARRAY's size nor an ENUM's values, so those read `INTEGER[]` and `ENUM`; a table's
/// columns take their names from `duckdb_columns()`, which has both.
pub fn type_name(logical: &LogicalTypeHandle) -> String {
    if let Some(alias) = logical.get_alias() {
        return alias;
    }
    let fields = |kind: &str| {
        let fields = (0..logical.num_children())
            .map(|index| {
                format!(
                    "{} {}",
                    logical.child_name(index),
                    type_name(&logical.child(index))
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{kind}({fields})")
    };
    match logical.id() {
        LogicalTypeId::Decimal => format!(
            "DECIMAL({},{})",
            logical.decimal_width(),
            logical.decimal_scale()
        ),
        LogicalTypeId::List | LogicalTypeId::Array => {
            format!("{}[]", type_name(&logical.child(0)))
        }
        LogicalTypeId::Map => format!(
            "MAP({}, {})",
            type_name(&logical.child(0)),
            type_name(&logical.child(1))
        ),
        LogicalTypeId::Struct => fields("STRUCT"),
        LogicalTypeId::Union => fields("UNION"),
        id => simple_type_name(id).to_string(),
    }
}

fn simple_type_name(id: LogicalTypeId) -> &'static str {
    match id {
        LogicalTypeId::Boolean => "BOOLEAN",
        LogicalTypeId::Tinyint => "TINYINT",
        LogicalTypeId::Smallint => "SMALLINT",
        LogicalTypeId::Integer => "INTEGER",
        LogicalTypeId::Bigint => "BIGINT",
        LogicalTypeId::Hugeint => "HUGEINT",
        LogicalTypeId::UTinyint => "UTINYINT",
        LogicalTypeId::USmallint => "USMALLINT",
        LogicalTypeId::UInteger => "UINTEGER",
        LogicalTypeId::UBigint => "UBIGINT",
        LogicalTypeId::UHugeint => "UHUGEINT",
        LogicalTypeId::Float => "FLOAT",
        LogicalTypeId::Double => "DOUBLE",
        LogicalTypeId::Varchar => "VARCHAR",
        LogicalTypeId::Blob => "BLOB",
        LogicalTypeId::Date => "DATE",
        LogicalTypeId::Time => "TIME",
        LogicalTypeId::TimeNs => "TIME_NS",
        LogicalTypeId::TimeTZ => "TIME WITH TIME ZONE",
        LogicalTypeId::Timestamp => "TIMESTAMP",
        LogicalTypeId::TimestampS => "TIMESTAMP_S",
        LogicalTypeId::TimestampMs => "TIMESTAMP_MS",
        LogicalTypeId::TimestampNs => "TIMESTAMP_NS",
        LogicalTypeId::TimestampTZ => "TIMESTAMP WITH TIME ZONE",
        LogicalTypeId::Interval => "INTERVAL",
        LogicalTypeId::Uuid => "UUID",
        LogicalTypeId::Enum => "ENUM",
        LogicalTypeId::Bit => "BIT",
        LogicalTypeId::Bignum => "BIGNUM",
        LogicalTypeId::Geometry => "GEOMETRY",
        LogicalTypeId::Variant => "VARIANT",
        LogicalTypeId::SqlNull => "NULL",
        _ => "UNKNOWN",
    }
}

/// The cells of one column of a chunk. Integers, booleans, text, JSON and blobs arrive
/// as themselves; a DECIMAL, HUGEINT or BIGNUM as its digits; anything else -- floats,
/// dates, intervals, lists, structs, maps -- as DuckDB writes it cast to VARCHAR, under
/// `type_name`, which DuckDB reads back as the same value.
pub fn decode_column(array: &ArrayRef, field: &Field, type_name: &str) -> Vec<DbValue> {
    let opaque = render::opaque_type(field);
    (0..array.len())
        .map(|row| {
            if array.is_null(row) {
                return DbValue::Null;
            }
            match (array.data_type(), opaque) {
                (DataType::Boolean, _) => DbValue::Bool(array.as_boolean().value(row)),
                (_, Some("bool8")) => DbValue::Bool(
                    render::render(array.as_ref(), field, row).as_deref() == Some("true"),
                ),
                (DataType::UInt64, _) => {
                    DbValue::U64(array.as_primitive::<UInt64Type>().value(row))
                }
                (
                    DataType::Int8
                    | DataType::Int16
                    | DataType::Int32
                    | DataType::Int64
                    | DataType::UInt8
                    | DataType::UInt16
                    | DataType::UInt32,
                    _,
                ) => render::render(array.as_ref(), field, row)
                    .and_then(|text| text.parse().ok())
                    .map_or(DbValue::Null, DbValue::I64),
                (
                    DataType::Utf8
                    | DataType::LargeUtf8
                    | DataType::Utf8View
                    | DataType::Dictionary(..),
                    _,
                ) => {
                    let text = render::render(array.as_ref(), field, row).unwrap_or_default();
                    if type_name == "JSON" {
                        DbValue::Json(text)
                    } else {
                        DbValue::Text(text)
                    }
                }
                (_, Some("uuid")) => {
                    DbValue::Text(render::render(array.as_ref(), field, row).unwrap_or_default())
                }
                (_, Some("hugeint" | "uhugeint" | "bignum")) | (DataType::Decimal128(..), _) => {
                    DbValue::Decimal(render::render(array.as_ref(), field, row).unwrap_or_default())
                }
                (DataType::Binary | DataType::LargeBinary | DataType::BinaryView, None) => {
                    let bytes = blob(array, row);
                    if type_name == "BIT" {
                        native(type_name, render::bit_text(&bytes))
                    } else {
                        DbValue::Bytes(bytes)
                    }
                }
                _ => match render::render(array.as_ref(), field, row) {
                    Some(text) => native(type_name, text),
                    // A union holding a NULL member.
                    None => DbValue::Null,
                },
            }
        })
        .collect()
}

fn native(type_name: &str, text: String) -> DbValue {
    DbValue::Native {
        type_name: type_name.to_string(),
        bytes: Vec::new(),
        text,
    }
}

fn blob(array: &ArrayRef, row: usize) -> Vec<u8> {
    match array.data_type() {
        DataType::Binary => array.as_binary::<i32>().value(row).to_vec(),
        DataType::LargeBinary => array.as_binary::<i64>().value(row).to_vec(),
        _ => array.as_binary_view().value(row).to_vec(),
    }
}

/// A type whose value goes back as DuckDB's text of it, compared as text: a list, an
/// array, a struct, a map or a union, which a cast from text reads less surely than
/// their text compares.
pub fn compared_as_text(type_name: &str) -> bool {
    type_name.ends_with(']')
        || ["STRUCT(", "MAP(", "UNION("]
            .iter()
            .any(|prefix| type_name.starts_with(prefix))
}

/// What a value binds as. A float read from the file goes back as the same float, so a
/// row's original values still match it when an edit is keyed by them; anything else
/// DuckDB writes as text goes back as that text, which DuckDB casts to the column's type.
pub fn to_sql(value: &DbValue) -> Value {
    match value {
        DbValue::Null => Value::Null,
        DbValue::Bool(value) => Value::Boolean(*value),
        DbValue::I64(value) => Value::BigInt(*value),
        DbValue::U64(value) => Value::UBigInt(*value),
        DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Json(text) => {
            Value::Text(text.clone())
        }
        DbValue::Bytes(bytes) => Value::Blob(bytes.clone()),
        DbValue::Native {
            type_name, text, ..
        } if type_name == "DOUBLE" => text
            .parse()
            .map(Value::Double)
            .unwrap_or_else(|_| Value::Text(text.clone())),
        DbValue::Native {
            type_name, text, ..
        } if type_name == "FLOAT" => text
            .parse()
            .map(Value::Float)
            .unwrap_or_else(|_| Value::Text(text.clone())),
        DbValue::Native { text, .. } => Value::Text(text.clone()),
    }
}

pub fn quote(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// `"db"."schema"."table"`, with only the parts the name has.
pub fn qualify(name: &dexo_driver_api::QualifiedName) -> String {
    name.catalog()
        .into_iter()
        .chain(name.schema())
        .chain([name.object()])
        .map(quote)
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::{compared_as_text, quote};

    #[test]
    fn nested_types_compare_as_text() {
        for name in [
            "INTEGER[]",
            "INTEGER[2]",
            "STRUCT(a INTEGER)",
            "MAP(VARCHAR, INTEGER)",
            "UNION(n INTEGER)",
        ] {
            assert!(compared_as_text(name), "{name}");
        }
        assert!(!compared_as_text("VARCHAR") && !compared_as_text("DATE"));
        assert_eq!(quote("a\"b"), "\"a\"\"b\"");
    }
}
