#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dialect {
    Postgres,
    Mysql,
}

impl Dialect {
    pub fn name(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
        }
    }

    pub fn quote(self) -> char {
        match self {
            Self::Postgres => '"',
            Self::Mysql => '`',
        }
    }

    /// Whether an identifier survives being written bare. Postgres folds unquoted names
    /// to lower case, so anything with an upper-case letter needs quoting there and does
    /// not in MySQL.
    pub fn needs_quotes(self, ident: &str) -> bool {
        if ident.is_empty() {
            return true;
        }
        let first = ident.chars().next().unwrap_or_default();
        if !(first.is_ascii_alphabetic() || first == '_') {
            return true;
        }
        ident.chars().any(|ch| {
            !(ch.is_ascii_alphanumeric() || ch == '_')
                || (self == Self::Postgres && ch.is_ascii_uppercase())
        })
    }

    /// The identifier as it has to be written to mean itself. The drivers quote
    /// unconditionally, which is right for DDL and wrong here -- nobody wants every
    /// accepted completion to arrive as `"users"`.
    pub fn quote_if_needed(self, ident: &str) -> String {
        if !self.needs_quotes(ident) {
            return ident.to_string();
        }
        let quote = self.quote();
        let escaped = ident.replace(quote, &format!("{quote}{quote}"));
        format!("{quote}{escaped}{quote}")
    }
}
