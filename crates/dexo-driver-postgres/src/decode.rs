use dexo_driver_api::{ColumnMeta, DbValue};
use tokio_postgres::Row;
use tokio_postgres::types::Type;

pub fn column_meta(column: &tokio_postgres::Column) -> ColumnMeta {
    ColumnMeta {
        name: column.name().to_string(),
        type_name: column.type_().name().to_string(),
        nullable: true,
    }
}

pub fn decode_row(row: &Row) -> Vec<DbValue> {
    (0..row.len()).map(|idx| decode_value(row, idx)).collect()
}

fn decode_value(row: &Row, idx: usize) -> DbValue {
    match row.columns()[idx].type_() {
        &Type::BOOL => map_opt(row.try_get(idx).ok().flatten(), DbValue::Bool),
        &Type::INT2 => map_opt(
            row.try_get::<_, Option<i16>>(idx)
                .ok()
                .flatten()
                .map(i64::from),
            DbValue::I64,
        ),
        &Type::INT4 => map_opt(
            row.try_get::<_, Option<i32>>(idx)
                .ok()
                .flatten()
                .map(i64::from),
            DbValue::I64,
        ),
        &Type::INT8 => map_opt(row.try_get(idx).ok().flatten(), DbValue::I64),
        &Type::TEXT | &Type::VARCHAR | &Type::NAME => {
            map_opt(row.try_get(idx).ok().flatten(), DbValue::Text)
        }
        &Type::BYTEA => map_opt(row.try_get(idx).ok().flatten(), DbValue::Bytes),
        &Type::JSON | &Type::JSONB => map_opt(
            row.try_get::<_, Option<serde_json::Value>>(idx)
                .ok()
                .flatten()
                .map(|value| value.to_string()),
            DbValue::Json,
        ),
        &Type::UUID => map_opt(
            row.try_get::<_, Option<uuid::Uuid>>(idx)
                .ok()
                .flatten()
                .map(|value| value.to_string()),
            DbValue::Text,
        ),
        &Type::NUMERIC => map_opt(row.try_get(idx).ok().flatten(), DbValue::Decimal),
        &Type::FLOAT4 => native(
            "float4",
            row.try_get::<_, Option<f32>>(idx)
                .ok()
                .flatten()
                .map(|value| value.to_string()),
        ),
        &Type::FLOAT8 => native(
            "float8",
            row.try_get::<_, Option<f64>>(idx)
                .ok()
                .flatten()
                .map(|value| value.to_string()),
        ),
        ty => native(ty.name(), row.try_get(idx).ok().flatten()),
    }
}

fn map_opt<T>(value: Option<T>, map: impl FnOnce(T) -> DbValue) -> DbValue {
    value.map(map).unwrap_or(DbValue::Null)
}

fn native(type_name: &str, text: Option<String>) -> DbValue {
    match text {
        None => DbValue::Null,
        Some(text) => DbValue::Native {
            type_name: type_name.to_string(),
            bytes: text.as_bytes().to_vec(),
            text,
        },
    }
}
