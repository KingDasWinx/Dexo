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

/// `text` as a MySQL string literal. In MySQL's default sql_mode a backslash starts an
/// escape inside quotes, so doubling the quotes alone let `\'` close the literal early
/// and what followed run as SQL. The backslash is escaped too, and NUL, the line breaks
/// and Ctrl+Z the way MySQL's own quoting writes them.
pub fn mysql_string_literal(text: &str) -> String {
    let mut literal = String::with_capacity(text.len() + 2);
    literal.push('\'');
    for ch in text.chars() {
        match ch {
            '\'' => literal.push_str("''"),
            '\\' => literal.push_str("\\\\"),
            '\0' => literal.push_str("\\0"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\x1a' => literal.push_str("\\Z"),
            _ => literal.push(ch),
        }
    }
    literal.push('\'');
    literal
}

#[cfg(test)]
mod tests {
    use super::mysql_string_literal;

    /// A backslash cannot end the literal early, and comes back as itself.
    #[test]
    fn mysql_literals_escape_backslashes() {
        assert_eq!(
            mysql_string_literal("a\\'); DROP TABLE victim2; -- "),
            "'a\\\\''); DROP TABLE victim2; -- '"
        );
        assert_eq!(
            mysql_string_literal("C:\\new\\table"),
            "'C:\\\\new\\\\table'"
        );
        assert_eq!(
            mysql_string_literal("O'Brien\0\n\r\x1a"),
            "'O''Brien\\0\\n\\r\\Z'"
        );
    }
}
