use crate::dialect::Dialect;
use crate::document::SqlError;
use crate::lex::{Token, TokenKind, is_reserved, tokenize};
use crate::statement::{segments, split_statements};

/// Lays `sql` out one clause per line, with lists, subqueries and CASE blocks indented
/// and keywords in capitals. Each statement is formatted on its own and kept exactly as
/// written if the result would not mean the same thing, so one the formatter cannot
/// handle never costs the others. `FormatUnsafe` only when none could be formatted.
pub fn format_sql(sql: &str, dialect: Dialect) -> Result<String, SqlError> {
    let mut pieces = Vec::new();
    let mut formatted_any = false;
    let mut kept_any = false;
    for range in segments(sql, &split_statements(sql)) {
        let text = sql[range].trim();
        if text.is_empty() {
            continue;
        }
        match format_statement(text, dialect) {
            Some(formatted) => {
                formatted_any = true;
                pieces.push(formatted);
            }
            None => {
                kept_any = true;
                pieces.push(text.to_string());
            }
        }
    }
    if kept_any && !formatted_any {
        return Err(SqlError::FormatUnsafe);
    }
    let mut out = pieces.join("\n\n");
    if sql.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

fn format_statement(text: &str, dialect: Dialect) -> Option<String> {
    // sqlformat misreads string bodies it does not know -- `$$it's$$` derails it for the
    // rest of the statement -- so it never sees one: each literal goes in as a plain
    // word and comes back out afterwards.
    if text.contains(MARK) {
        return None;
    }
    let tokens = tokenize(text, dialect);
    let mut literals = Vec::new();
    let mut masked = String::with_capacity(text.len());
    let mut at = 0;
    for token in tokens
        .iter()
        .filter(|token| token.kind == TokenKind::String)
    {
        masked.push_str(&text[at..token.span.start]);
        masked.push_str(&placeholder(literals.len()));
        literals.push(token.text(text));
        at = token.span.end;
    }
    masked.push_str(&text[at..]);

    let options = sqlformat::FormatOptions {
        indent: sqlformat::Indent::Spaces(2),
        // Capitals are applied below, to reserved words only: sqlformat's own list
        // takes in names like `level`, and a MySQL table name is case-sensitive.
        uppercase: None,
        lines_between_queries: 1,
        dialect: match dialect {
            Dialect::Postgres => sqlformat::Dialect::PostgreSql,
            Dialect::Mysql => sqlformat::Dialect::Generic,
        },
        ..Default::default()
    };
    let formatted = sqlformat::format(&masked, &sqlformat::QueryParams::None, &options);
    let mut out = capitalize(&formatted, dialect);
    for (index, literal) in literals.iter().enumerate().rev() {
        out = out.replacen(&placeholder(index), literal, 1);
    }
    same_meaning(text, &out, dialect).then_some(out)
}

const MARK: &str = "__dexo_literal_";

fn placeholder(index: usize) -> String {
    format!("{MARK}{index}__")
}

fn capitalize(sql: &str, dialect: Dialect) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut at = 0;
    for token in tokenize(sql, dialect) {
        if token.kind == TokenKind::Word && is_reserved(token.text(sql)) {
            out.push_str(&sql[at..token.span.start]);
            out.push_str(&token.text(sql).to_ascii_uppercase());
            at = token.span.end;
        }
    }
    out.push_str(&sql[at..]);
    out
}

/// The same tokens in the same order, whatever the whitespace between them. Reserved
/// words may change case; nothing else may change at all.
fn same_meaning(before: &str, after: &str, dialect: Dialect) -> bool {
    let significant = |sql: &str| -> Vec<(TokenKind, String)> {
        tokenize(sql, dialect)
            .iter()
            .map(|token: &Token| {
                let text = token.text(sql);
                let text = match token.kind {
                    TokenKind::Word if is_reserved(text) => text.to_ascii_uppercase(),
                    TokenKind::Comment => text.trim_end().to_string(),
                    _ => text.to_string(),
                };
                (token.kind, text)
            })
            .collect()
    };
    significant(before) == significant(after)
}

#[cfg(test)]
mod tests {
    use super::format_sql;
    use crate::dialect::Dialect;

    fn format(sql: &str) -> String {
        format_sql(sql, Dialect::Postgres).unwrap()
    }

    #[test]
    fn format_is_idempotent_and_preserves_literals() {
        let sql = "select 1 from t where name='a  b' -- keep";
        let once = format(sql);
        let twice = format(&once);
        assert_eq!(once, twice);
        assert!(once.contains("'a  b'"));
        assert!(once.contains("-- keep"));
    }

    #[test]
    fn clauses_go_on_their_own_lines_and_their_contents_are_indented() {
        let formatted = format(
            "select u.id, count(o.id) as total from users u left join orders o on o.user_id = u.id \
             where u.id in (select user_id from vip) group by u.id order by total desc",
        );
        assert_eq!(
            formatted,
            "SELECT\n  u.id,\n  count(o.id) AS total\nFROM\n  users u\n  LEFT JOIN orders o ON \
             o.user_id = u.id\nWHERE\n  u.id IN (\n    SELECT\n      user_id\n    FROM\n      \
             vip\n  )\nGROUP BY\n  u.id\nORDER BY\n  total DESC"
        );
    }

    /// sqlformat alone turned `$$it's$$` into `$$it ' s` and scrambled everything after.
    #[test]
    fn dollar_quoted_and_escaped_strings_come_through_untouched() {
        let sql = "update t set b = $$it's$$, c = $tag$x;y$tag$, d = e'a\\'b' where id = :id";
        let formatted = format(sql);
        assert!(formatted.contains("$$it's$$"), "{formatted}");
        assert!(formatted.contains("$tag$x;y$tag$"), "{formatted}");
        assert!(formatted.contains("e'a\\'b'"), "{formatted}");
        assert!(formatted.contains(":id"), "{formatted}");
    }

    #[test]
    fn postgres_operators_are_not_split() {
        let formatted = format(
            "select \"Mixed Col\" from t where data->>'k' = 'v' and x::int > 1 and tags @> array['a']",
        );
        for piece in [
            "\"Mixed Col\"",
            "data ->> 'k'",
            "x::int",
            "tags @> array['a']",
        ] {
            assert!(formatted.contains(piece), "{piece} in\n{formatted}");
        }
    }

    /// `level` and `status` are words sqlformat capitalizes; as names they are left be.
    #[test]
    fn only_reserved_words_change_case() {
        let formatted = format("select level, status, Name from Users where level > 1");
        assert!(formatted.contains("level,"), "{formatted}");
        assert!(formatted.contains("Users"), "{formatted}");
        assert!(formatted.contains("Name"), "{formatted}");
        assert!(formatted.starts_with("SELECT"), "{formatted}");
    }

    #[test]
    fn each_statement_is_formatted_on_its_own() {
        let formatted = format("select 1; -- one\nselect 2;\n");
        assert_eq!(formatted, "SELECT\n  1;\n-- one\n\nSELECT\n  2;\n");
        assert_eq!(format(&formatted), formatted);
    }

    #[test]
    fn mysql_comments_and_quoting_survive() {
        let formatted = format_sql(
            "select `a`, b from `t` where c = 'it''s' # note\nand d <=> null",
            Dialect::Mysql,
        )
        .unwrap();
        assert!(formatted.contains("`a`"), "{formatted}");
        assert!(formatted.contains("'it''s'"), "{formatted}");
        assert!(formatted.contains("# note"), "{formatted}");
    }
}
