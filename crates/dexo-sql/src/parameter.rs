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

pub fn named_parameters(sql: &str) -> Vec<Parameter> {
    let mut names = Vec::new();
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
            i += 1;
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = sql[start..i].to_string();
            if !names.iter().any(|item: &Parameter| item.name == name) {
                names.push(Parameter {
                    name,
                    type_name: "text".into(),
                    value: None,
                });
            }
        } else {
            i += 1;
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
