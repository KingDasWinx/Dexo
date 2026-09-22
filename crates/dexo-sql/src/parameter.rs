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

#[cfg(test)]
mod tests {
    use super::{HistoryEntry, HistoryPolicy};

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
