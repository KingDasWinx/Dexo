use std::sync::Arc;

use dexo_driver_api::{ColumnMeta, DbValue, DriverError};
use duckdb::arrow::array::{Array, ArrayData, ArrayRef, AsArray, make_array};
use duckdb::arrow::compute::cast;
use duckdb::arrow::datatypes::{
    DataType, Decimal128Type, Field, Fields, Float32Type, Float64Type, Int64Type, UInt64Type,
};
use duckdb::arrow::util::display::{ArrayFormatter, FormatOptions};
use duckdb::core::{LogicalTypeHandle, LogicalTypeId};
use duckdb::types::Value;

use crate::error::internal;

pub fn column_meta(name: &str, logical: &LogicalTypeHandle) -> ColumnMeta {
    ColumnMeta {
        name: name.to_string(),
        type_name: type_name(logical),
        nullable: true,
    }
}

/// DuckDB's own name for a type, as `duckdb_columns()` writes it: `DECIMAL(10,2)`,
/// `INTEGER[]`, `STRUCT(a INTEGER)`, and an alias such as `JSON` by its alias.
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
/// as themselves; a DECIMAL or HUGEINT as its digits; a float as the shortest text that
/// reads back as the same number; anything else -- dates, intervals, lists, structs,
/// maps -- as DuckDB writes it, under `type_name`.
pub fn decode_column(array: &ArrayRef, type_name: &str) -> Result<Vec<DbValue>, DriverError> {
    let len = array.len();
    let cells = |value: &dyn Fn(usize) -> DbValue| -> Vec<DbValue> {
        (0..len)
            .map(|row| {
                if array.is_null(row) {
                    DbValue::Null
                } else {
                    value(row)
                }
            })
            .collect()
    };
    let native = |text: String| DbValue::Native {
        type_name: type_name.to_string(),
        bytes: Vec::new(),
        text,
    };
    Ok(match array.data_type() {
        DataType::Boolean => {
            let values = array.as_boolean();
            cells(&|row| DbValue::Bool(values.value(row)))
        }
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32 => {
            let wide = cast(array, &DataType::Int64).map_err(internal)?;
            let values = wide.as_primitive::<Int64Type>();
            cells(&|row| DbValue::I64(values.value(row)))
        }
        DataType::UInt64 => {
            let values = array.as_primitive::<UInt64Type>();
            cells(&|row| DbValue::U64(values.value(row)))
        }
        DataType::Float32 => {
            let values = array.as_primitive::<Float32Type>();
            cells(&|row| native(format!("{:?}", values.value(row))))
        }
        DataType::Float64 => {
            let values = array.as_primitive::<Float64Type>();
            cells(&|row| native(format!("{:?}", values.value(row))))
        }
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View | DataType::Dictionary(..) => {
            let text = cast(array, &DataType::LargeUtf8).map_err(internal)?;
            let values = text.as_string::<i64>();
            let json = type_name == "JSON";
            cells(&|row| {
                let text = values.value(row).to_string();
                if json {
                    DbValue::Json(text)
                } else {
                    DbValue::Text(text)
                }
            })
        }
        DataType::Binary | DataType::LargeBinary | DataType::BinaryView => {
            let bytes = cast(array, &DataType::LargeBinary).map_err(internal)?;
            let values = bytes.as_binary::<i64>();
            if type_name == "BIT" {
                cells(&|row| native(bit_text(values.value(row))))
            } else {
                cells(&|row| DbValue::Bytes(values.value(row).to_vec()))
            }
        }
        // Arrow's formatter cuts a value to the type's precision, and DuckDB hands over a
        // HUGEINT as a DECIMAL(38,0) whose largest values have 39 digits.
        DataType::Decimal128(_, scale) => {
            let values = array.as_primitive::<Decimal128Type>();
            cells(&|row| DbValue::Decimal(decimal_text(values.value(row), *scale)))
        }
        DataType::Decimal256(..) => {
            let texts = formatted(array)?;
            cells(&|row| DbValue::Decimal(texts[row].clone()))
        }
        _ => {
            let texts = formatted(array)?;
            cells(&|row| native(texts[row].clone()))
        }
    })
}

/// Each cell as DuckDB's shell writes it, a NULL inside a list or struct as `NULL`.
fn formatted(array: &ArrayRef) -> Result<Vec<String>, DriverError> {
    let array = with_offsets(array)?;
    let options = FormatOptions::new()
        .with_null("NULL")
        .with_timestamp_format(Some("%Y-%m-%d %H:%M:%S%.f"))
        .with_timestamp_tz_format(Some("%Y-%m-%d %H:%M:%S%.f%:z"));
    let formatter = ArrayFormatter::try_new(array.as_ref(), &options).map_err(internal)?;
    Ok((0..array.len())
        .map(|row| formatter.value(row).to_string())
        .collect())
}

/// Arrow names the zone of a TIMESTAMPTZ, and formats a named zone only with chrono-tz.
/// The values are UTC instants whatever the name, so the zone is written as its offset,
/// in lists and structs too.
fn with_offsets(array: &ArrayRef) -> Result<ArrayRef, DriverError> {
    if offsets(array.data_type()) == *array.data_type() {
        return Ok(Arc::clone(array));
    }
    Ok(make_array(retyped(array.to_data())?))
}

fn retyped(data: ArrayData) -> Result<ArrayData, DriverError> {
    let data_type = offsets(data.data_type());
    let children = data
        .child_data()
        .iter()
        .cloned()
        .map(retyped)
        .collect::<Result<Vec<_>, _>>()?;
    data.into_builder()
        .data_type(data_type)
        .child_data(children)
        .build()
        .map_err(internal)
}

fn offsets(data_type: &DataType) -> DataType {
    let field = |field: &Field| Arc::new(field.clone().with_data_type(offsets(field.data_type())));
    match data_type {
        DataType::Timestamp(unit, Some(_)) => DataType::Timestamp(*unit, Some("+00:00".into())),
        DataType::List(inner) => DataType::List(field(inner)),
        DataType::LargeList(inner) => DataType::LargeList(field(inner)),
        DataType::FixedSizeList(inner, size) => DataType::FixedSizeList(field(inner), *size),
        DataType::Map(inner, sorted) => DataType::Map(field(inner), *sorted),
        DataType::Struct(fields) => {
            DataType::Struct(fields.iter().map(|inner| field(inner)).collect::<Fields>())
        }
        other => other.clone(),
    }
}

/// `value` with `scale` digits after the point.
fn decimal_text(value: i128, scale: i8) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let digits = value.unsigned_abs().to_string();
    let Ok(scale) = usize::try_from(scale) else {
        return format!(
            "{sign}{digits}{}",
            "0".repeat(usize::from(scale.unsigned_abs()))
        );
    };
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (whole, fraction) = padded.split_at(padded.len() - scale);
    format!("{sign}{whole}.{fraction}")
}

/// DuckDB keeps a BIT as one byte counting the unused leading bits, then the bits.
fn bit_text(raw: &[u8]) -> String {
    let Some((&padding, bytes)) = raw.split_first() else {
        return String::new();
    };
    bytes
        .iter()
        .flat_map(|byte| (0..8).rev().map(move |bit| (byte >> bit) & 1))
        .skip(usize::from(padding))
        .map(|bit| if bit == 1 { '1' } else { '0' })
        .collect()
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
    use super::{bit_text, decimal_text, quote};

    #[test]
    fn decimals_keep_every_digit() {
        assert_eq!(decimal_text(150, 2), "1.50");
        assert_eq!(decimal_text(-5, 2), "-0.05");
        assert_eq!(decimal_text(i128::MAX, 0), i128::MAX.to_string());
        assert_eq!(decimal_text(12, -2), "1200");
    }

    #[test]
    fn bits_skip_their_padding() {
        // `0101`: four unused bits, then 0101.
        assert_eq!(bit_text(&[4, 0b0000_0101]), "0101");
        assert_eq!(bit_text(&[]), "");
        assert_eq!(quote("a\"b"), "\"a\"\"b\"");
    }
}
