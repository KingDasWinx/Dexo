use std::error::Error;

use bytes::BytesMut;
use dexo_driver_api::{DbValue, DriverError, DriverErrorCategory};
use tokio_postgres::Statement;
use tokio_postgres::types::{Format, IsNull, ToSql, Type, to_sql_checked};

#[derive(Debug)]
pub enum PgParam {
    Null,
    Bool(bool),
    I64(i64),
    Text(String),
    Bytes(Vec<u8>),
}

impl PgParam {
    pub fn from_value(value: &DbValue) -> Self {
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
            // Sent in text format (see `encode_format`), so the server parses it into
            // whatever the placeholder's type is. Writing the characters as a binary
            // payload is only correct when that type is textual, and for everything else
            // the server rejected the statement: "incorrect binary data format".
            Self::Text(value) => {
                out.extend_from_slice(value.as_bytes());
                Ok(IsNull::No)
            }
            Self::Bytes(value) => value.as_slice().to_sql(ty, out),
        }
    }

    /// Only the text variant asks for text format: the others carry a payload that is
    /// already in the binary form the placeholder's type expects -- `Bytes` holds the
    /// wire bytes a decoded value came in as.
    fn encode_format(&self, _ty: &Type) -> Format {
        match self {
            Self::Text(_) => Format::Text,
            _ => Format::Binary,
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

#[cfg(test)]
mod tests {
    use super::PgParam;
    use dexo_driver_api::DbValue;
    use tokio_postgres::types::{Format, ToSql, Type};

    /// A typed cell arrives as text -- the insert form sends every field that way -- and
    /// the placeholder it fills is rarely a text column. Sent as a binary payload, the
    /// server rejects it: "incorrect binary data format in bind parameter 1". Sent as
    /// text, the server parses it into whatever the placeholder's type is.
    #[test]
    fn text_values_are_sent_for_the_server_to_parse() {
        for ty in [Type::INT8, Type::TIMESTAMPTZ, Type::NUMERIC, Type::TEXT] {
            assert!(matches!(
                PgParam::from_value(&DbValue::Text("42".into())).encode_format(&ty),
                Format::Text
            ));
        }
        // A decoded value carries the wire bytes it arrived as, which are already in the
        // form its own type expects.
        assert!(matches!(
            PgParam::from_value(&DbValue::Native {
                type_name: "timestamptz".into(),
                bytes: vec![0, 2, 66, 200, 148, 40, 144, 0],
                text: "2020-03-01 12:00:00+00".into(),
            })
            .encode_format(&Type::TIMESTAMPTZ),
            Format::Binary
        ));
    }
}
