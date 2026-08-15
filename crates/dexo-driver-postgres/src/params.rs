use std::error::Error;

use bytes::BytesMut;
use dexo_driver_api::{DbValue, DriverError, DriverErrorCategory};
use tokio_postgres::Statement;
use tokio_postgres::types::{IsNull, ToSql, Type, to_sql_checked};

#[derive(Debug)]
pub enum PgParam {
    Null,
    Bool(bool),
    I64(i64),
    Text(String),
    Bytes(Vec<u8>),
}

impl PgParam {
    fn from_value(value: &DbValue) -> Self {
        match value {
            DbValue::Null => Self::Null,
            DbValue::Bool(value) => Self::Bool(*value),
            DbValue::I64(value) => Self::I64(*value),
            DbValue::U64(value) if *value <= i64::MAX as u64 => Self::I64(*value as i64),
            DbValue::U64(value) => Self::Text(value.to_string()),
            DbValue::Decimal(text) | DbValue::Text(text) | DbValue::Json(text) => {
                Self::Text(text.clone())
            }
            DbValue::Bytes(bytes) => Self::Bytes(bytes.clone()),
            DbValue::Native { text, bytes, .. } => {
                if bytes.is_empty() {
                    Self::Text(text.clone())
                } else {
                    Self::Bytes(bytes.clone())
                }
            }
        }
    }
}

impl ToSql for PgParam {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match self {
            // ponytail: NULL accepts every OID; typed variants still go through ToSql.
            Self::Null => Ok(IsNull::Yes),
            Self::Bool(value) => value.to_sql(ty, out),
            Self::I64(value) => match *ty {
                Type::INT2 => i16::try_from(*value)?.to_sql(ty, out),
                Type::INT4 => i32::try_from(*value)?.to_sql(ty, out),
                Type::INT8 => value.to_sql(ty, out),
                _ => value.to_sql(ty, out),
            },
            Self::Text(value) => value.to_sql(ty, out),
            Self::Bytes(value) => value.as_slice().to_sql(ty, out),
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

pub fn bind(statement: &Statement, values: &[DbValue]) -> Result<Vec<PgParam>, DriverError> {
    let expected = statement.params().len();
    if !values.is_empty() && values.len() != expected {
        return Err(DriverError::new(
            DriverErrorCategory::Syntax,
            format!(
                "expected {expected} query parameters, received {}",
                values.len()
            ),
        ));
    }
    Ok(values.iter().map(PgParam::from_value).collect())
}
