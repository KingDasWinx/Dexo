use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryPolicy {
    SqlOnly,
    WithValues,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub sql: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<(String, String)>>,
}

impl HistoryEntry {
    pub fn new(
        sql: impl Into<String>,
        parameters: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self {
            sql: sql.into(),
            parameters: Some(
                parameters
                    .into_iter()
                    .map(|(name, value)| (name.into(), value.into()))
                    .collect(),
            ),
        }
    }

    pub fn for_storage(&self, policy: HistoryPolicy) -> Self {
        match policy {
            HistoryPolicy::SqlOnly => Self {
                sql: self.sql.clone(),
                parameters: None,
            },
            HistoryPolicy::WithValues => self.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub type_name: String,
    pub value: Option<String>,
}

/// The `:name` placeholders the statement will ask a value for, in first-seen order.
///
/// Read off the lexer's tokens rather than the raw bytes. A byte scan took every colon
/// followed by a letter: `N:N` in a comment or a string, `"weird:col"`, a `$$` function
/// body, and every Postgres cast -- `id::text` asked for a parameter called `text`. The
/// lexer already knows where strings, comments and quoted names end.
pub fn named_parameters(sql: &str, dialect: crate::Dialect) -> Vec<Parameter> {
    let mut names: Vec<Parameter> = Vec::new();
    for token in crate::lex::tokenize(sql, dialect) {
        if token.kind != crate::lex::TokenKind::Param {
            continue;
        }
        let Some(name) = sql[token.span.clone()].strip_prefix(':') else {
            continue;
        };
        if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
            continue;
        }
        if !names.iter().any(|item| item.name == name) {
            names.push(Parameter {
                name: name.to_string(),
                type_name: "text".into(),
                value: None,
            });
        }
    }
    names
}

/// `sql` with its `:name` placeholders written the way `dialect` binds by position, and
/// the values in the order those placeholders take them: `$1`, `$2` on Postgres and
/// DuckDB and `?1` on SQLite, a name used twice keeping its number; `?` for each use on
/// MySQL. No server reads `:name` itself (SQLite aside), so sending it as typed was a
/// syntax error. A name with no value is left as it is.
///
/// A statement without named placeholders keeps its text: it takes every value, in
/// order, when it has positional ones of its own (`$1`, `?`), and none otherwise -- a
/// script's other statements no longer get values they never asked for.
pub fn bind_named<T: Clone>(
    sql: &str,
    dialect: crate::Dialect,
    values: &[(String, T)],
) -> (String, Vec<T>) {
    let placeholders: Vec<_> = crate::lex::tokenize(sql, dialect)
        .into_iter()
        .filter(|token| token.kind == crate::lex::TokenKind::Param)
        .collect();
    let named: Vec<_> = placeholders
        .iter()
        .filter_map(|token| {
            let name = sql[token.span.clone()].strip_prefix(':')?;
            name.starts_with(|c: char| c.is_ascii_alphabetic())
                .then_some((token.span.clone(), name))
        })
        .collect();
    if named.is_empty() {
        let positional = placeholders.iter().any(|token| {
            let text = &sql[token.span.clone()];
            text.starts_with('?')
                || text
                    .strip_prefix('$')
                    .is_some_and(|digits| !digits.is_empty())
        });
        let values = if positional {
            values.iter().map(|(_, value)| value.clone()).collect()
        } else {
            Vec::new()
        };
        return (sql.to_string(), values);
    }
    let mut out = String::with_capacity(sql.len());
    let mut last = 0;
    let mut numbered: Vec<&str> = Vec::new();
    let mut bound = Vec::new();
    for (span, name) in named {
        let Some(value) = values
            .iter()
            .find(|(given, _)| given == name)
            .map(|(_, value)| value.clone())
        else {
            continue;
        };
        out.push_str(&sql[last..span.start]);
        if dialect == crate::Dialect::Mysql {
            bound.push(value);
            out.push('?');
        } else {
            let number = match numbered.iter().position(|seen| *seen == name) {
                Some(index) => index + 1,
                None => {
                    numbered.push(name);
                    bound.push(value);
                    numbered.len()
                }
            };
            let sigil = if dialect == crate::Dialect::Sqlite {
                '?'
            } else {
                '$'
            };
            out.push(sigil);
            out.push_str(&number.to_string());
        }
        last = span.end;
    }
    out.push_str(&sql[last..]);
    (out, bound)
}

#[cfg(test)]
mod tests {
    use super::{HistoryEntry, HistoryPolicy, bind_named};
    use crate::Dialect;

    fn values() -> Vec<(String, i32)> {
        vec![("id".into(), 7), ("name".into(), 9)]
    }

    #[test]
    fn named_placeholders_bind_by_name_in_each_dialect() {
        let sql = "select * from t where id = :id and (name = :name or alt = :id)";
        assert_eq!(
            bind_named(sql, Dialect::Postgres, &values()),
            (
                "select * from t where id = $1 and (name = $2 or alt = $1)".into(),
                vec![7, 9]
            )
        );
        assert_eq!(
            bind_named(sql, Dialect::Mysql, &values()),
            (
                "select * from t where id = ? and (name = ? or alt = ?)".into(),
                vec![7, 9, 7]
            )
        );
        assert_eq!(
            bind_named(sql, Dialect::Sqlite, &values()).0,
            "select * from t where id = ?1 and (name = ?2 or alt = ?1)"
        );
        // A cast, a string and a comment are not placeholders.
        let (text, bound) = bind_named(
            "select :name::text, ':id' -- :id",
            Dialect::Postgres,
            &values(),
        );
        assert_eq!(text, "select $1::text, ':id' -- :id");
        assert_eq!(bound, vec![9]);
    }

    #[test]
    fn a_statement_takes_only_the_values_it_asks_for() {
        assert_eq!(
            bind_named("select 1", Dialect::Postgres, &values()),
            ("select 1".into(), Vec::new())
        );
        assert_eq!(
            bind_named("select $1, $2", Dialect::Postgres, &values()).1,
            vec![7, 9]
        );
        assert_eq!(
            bind_named("select ?", Dialect::Mysql, &values()).1,
            vec![7, 9]
        );
    }

    #[test]
    fn history_excludes_parameter_values_by_default() {
        let entry = HistoryEntry::new(
            "select * from users where email=:email",
            [("email", "secret@example.com")],
        );
        let stored = entry.for_storage(HistoryPolicy::SqlOnly);
        assert!(stored.sql.contains(":email"));
        assert!(
            !serde_json::to_string(&stored)
                .unwrap()
                .contains("secret@example.com")
        );
    }
}
