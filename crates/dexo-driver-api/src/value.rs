#[derive(Clone, Debug, PartialEq)]
pub enum DbValue {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    Decimal(String),
    Text(String),
    Bytes(Vec<u8>),
    Json(String),
    Native {
        type_name: String,
        bytes: Vec<u8>,
        text: String,
    },
}

impl DbValue {
    pub fn type_name(&self) -> Option<&str> {
        match self {
            Self::Native { type_name, .. } => Some(type_name),
            _ => None,
        }
    }
}
