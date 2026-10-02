//! The tables a statement drops, so the editor stops knowing what a run created.

use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::tokenizer::{Token, Tokenizer};

use crate::Dialect;

/// `DROP [TEMPORARY] TABLE [IF EXISTS] a, s.b`: `a` and `b`, lowered and unqualified as
/// [`crate::created_table`] gives a created one. Views and sequences too.
pub fn dropped_tables(body: &str, dialect: Dialect) -> Vec<String> {
    let tokens = match dialect {
        Dialect::Postgres => Tokenizer::new(&PostgreSqlDialect {}, body).tokenize(),
        Dialect::Mysql => Tokenizer::new(&MySqlDialect {}, body).tokenize(),
        Dialect::Sqlite => Tokenizer::new(&SQLiteDialect {}, body).tokenize(),
    };
    let Ok(tokens) = tokens else {
        return Vec::new();
    };
    let mut tokens = tokens
        .into_iter()
        .filter(|token| !matches!(token, Token::Whitespace(_) | Token::EOF))
        .peekable();
    let keyword = |token: Option<&Token>, words: &[&str]| match token {
        Some(Token::Word(word)) => words
            .iter()
            .any(|expected| word.value.eq_ignore_ascii_case(expected)),
        _ => false,
    };
    if !keyword(tokens.next().as_ref(), &["DROP"]) {
        return Vec::new();
    }
    while keyword(tokens.peek(), &["TEMPORARY", "TEMP", "MATERIALIZED"]) {
        tokens.next();
    }
    if !keyword(tokens.next().as_ref(), &["TABLE", "VIEW", "SEQUENCE"]) {
        return Vec::new();
    }
    if keyword(tokens.peek(), &["IF"]) {
        tokens.next();
        tokens.next();
    }
    let mut dropped = Vec::new();
    let mut last = None;
    for token in tokens {
        match token {
            Token::Word(word)
                if ["CASCADE", "RESTRICT"]
                    .iter()
                    .any(|stop| word.value.eq_ignore_ascii_case(stop)) =>
            {
                break;
            }
            Token::Word(word) => last = Some(word.value.to_lowercase()),
            Token::Period => {}
            Token::Comma => dropped.extend(last.take()),
            _ => break,
        }
    }
    dropped.extend(last);
    dropped
}

#[cfg(test)]
mod tests {
    use super::dropped_tables;
    use crate::Dialect;

    #[test]
    fn the_names_a_drop_names() {
        assert_eq!(
            dropped_tables(
                "DROP TABLE IF EXISTS a, public.\"B\" CASCADE",
                Dialect::Postgres
            ),
            ["a", "b"]
        );
        assert_eq!(
            dropped_tables("drop temporary table t", Dialect::Mysql),
            ["t"]
        );
        assert!(dropped_tables("drop index i", Dialect::Sqlite).is_empty());
        assert!(dropped_tables("select 1", Dialect::Sqlite).is_empty());
    }
}
