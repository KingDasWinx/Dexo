use crate::dialect::Dialect;
use crate::document::SqlError;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Token<'a> {
    Ws(&'a str),
    Comment(&'a str),
    Literal(&'a str),
    Word(&'a str),
    Punct(&'a str),
}

pub fn format_sql(sql: &str, _dialect: Dialect) -> Result<String, SqlError> {
    let tokens = tokenize(sql);
    let formatted = render(&tokens);
    let original_sig = significant(&tokens);
    let roundtrip_sig = significant(&tokenize(&formatted));
    if original_sig != roundtrip_sig {
        return Err(SqlError::FormatUnsafe);
    }
    Ok(formatted)
}

pub fn format_preview(original: &str, formatted: &str) -> String {
    format!("- {original}\n+ {formatted}")
}

fn significant(tokens: &[Token<'_>]) -> Vec<String> {
    tokens
        .iter()
        .filter_map(|token| match token {
            Token::Ws(_) => None,
            Token::Comment(text) | Token::Literal(text) | Token::Punct(text) => {
                Some((*text).to_string())
            }
            Token::Word(word) if is_keyword(word) => Some(word.to_ascii_uppercase()),
            Token::Word(word) => Some((*word).to_string()),
        })
        .collect()
}

fn render(tokens: &[Token<'_>]) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for token in tokens {
        match token {
            Token::Ws(_) => pending_space = true,
            Token::Comment(text) | Token::Literal(text) => {
                if pending_space && !out.is_empty() && !out.ends_with('\n') {
                    out.push(' ');
                }
                out.push_str(text);
                pending_space = false;
            }
            Token::Word(word) => {
                if is_break_keyword(word) && !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                } else if pending_space && !out.is_empty() && !out.ends_with('\n') {
                    out.push(' ');
                }
                if is_keyword(word) {
                    out.push_str(&word.to_ascii_uppercase());
                } else {
                    out.push_str(word);
                }
                pending_space = false;
            }
            Token::Punct(p) => {
                if pending_space
                    && *p != ","
                    && *p != ";"
                    && !out.ends_with('\n')
                    && !out.is_empty()
                {
                    out.push(' ');
                }
                out.push_str(p);
                pending_space = *p == "," || *p == ";";
                if *p == ";" {
                    out.push('\n');
                    pending_space = false;
                }
            }
        }
    }
    out.trim().to_string()
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word.to_ascii_uppercase().as_str(),
        "SELECT"
            | "FROM"
            | "WHERE"
            | "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "ON"
            | "GROUP"
            | "BY"
            | "ORDER"
            | "LIMIT"
            | "INSERT"
            | "INTO"
            | "UPDATE"
            | "DELETE"
            | "WITH"
            | "AS"
            | "AND"
            | "OR"
            | "NOT"
            | "VALUES"
            | "SET"
    )
}

fn is_break_keyword(word: &str) -> bool {
    matches!(
        word.to_ascii_uppercase().as_str(),
        "SELECT" | "FROM" | "WHERE" | "JOIN" | "GROUP" | "ORDER" | "LIMIT" | "VALUES" | "SET"
    )
}

fn tokenize(sql: &str) -> Vec<Token<'_>> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => {
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                tokens.push(Token::Ws(&sql[start..i]));
            }
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                tokens.push(Token::Comment(&sql[start..i]));
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                tokens.push(Token::Comment(&sql[start..i]));
            }
            b'\'' | b'"' | b'`' => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == quote {
                        if bytes.get(i + 1) == Some(&quote) {
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                tokens.push(Token::Literal(&sql[start..i]));
            }
            b'$' => {
                if let Some(end) = dollar_end(sql, i) {
                    tokens.push(Token::Literal(&sql[i..end]));
                    i = end;
                } else {
                    i += 1;
                    tokens.push(Token::Punct(&sql[start..i]));
                }
            }
            b if b.is_ascii_alphanumeric() || b == b'_' => {
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                tokens.push(Token::Word(&sql[start..i]));
            }
            _ => {
                i += 1;
                tokens.push(Token::Punct(&sql[start..i]));
            }
        }
    }
    tokens
}

fn dollar_end(sql: &str, start: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    if bytes.get(i) != Some(&b'$') {
        return None;
    }
    let tag = &sql[start..=i];
    i += 1;
    sql[i..].find(tag).map(|rel| i + rel + tag.len())
}

#[cfg(test)]
mod tests {
    use super::format_sql;
    use crate::dialect::Dialect;

    #[test]
    fn format_is_idempotent_and_preserves_literals() {
        let sql = "select 1 from t where name='a  b' -- keep";
        let once = format_sql(sql, Dialect::Postgres).unwrap();
        let twice = format_sql(&once, Dialect::Postgres).unwrap();
        assert_eq!(once, twice);
        assert!(once.contains("'a  b'"));
        assert!(once.contains("-- keep"));
    }
}
