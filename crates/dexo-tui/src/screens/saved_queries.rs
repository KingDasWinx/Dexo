//! Named SQL kept per project and connection: Save Query As, and the Open Saved Query
//! picker that searches them, shows the SQL, renames and deletes.

use dexo_storage::SavedQuery;

use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

/// Save Query As: a name for what the selection or the document holds.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveQueryPrompt {
    pub name: TextInput,
    pub footer: FooterFocus,
    pub error: Option<String>,
    pub sql: String,
    pub connection_id: String,
    /// Where the SQL came from, for the line under the name.
    pub source: &'static str,
}

impl SaveQueryPrompt {
    pub fn lines(&self) -> Vec<String> {
        let focused = self.footer == FooterFocus::Input;
        let mut lines = vec![
            self.name.inline_line("name: ", focused),
            format!(
                "{}, {} line{}; a query of the same name is replaced",
                self.source,
                self.sql.lines().count(),
                if self.sql.lines().count() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
        ];
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line("Save", self.footer));
        lines
    }
}

/// Open Saved Query.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedQueriesPicker {
    pub open: bool,
    /// `None` until the list has been read.
    pub items: Option<Vec<SavedQuery>>,
    pub search: TextInput,
    /// Into what the search lets through.
    pub selected: usize,
    pub renaming: Option<TextInput>,
    /// Delete asked; the footer's focus while it waits for an answer.
    pub deleting: Option<FooterFocus>,
    pub error: Option<String>,
}

impl SavedQueriesPicker {
    /// The queries whose name or SQL holds the search text, without regard to case.
    pub fn filtered(&self) -> Vec<&SavedQuery> {
        let needle = self.search.as_str().trim().to_lowercase();
        self.items
            .iter()
            .flatten()
            .filter(|query| {
                needle.is_empty()
                    || query.name.to_lowercase().contains(&needle)
                    || query.sql.to_lowercase().contains(&needle)
            })
            .collect()
    }

    pub fn current(&self) -> Option<&SavedQuery> {
        self.filtered().get(self.selected).copied()
    }

    /// Keeps the highlight on a row that is there after the list or the search changed.
    pub fn clamp(&mut self) {
        let count = self.filtered().len();
        self.selected = self.selected.min(count.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::SavedQueriesPicker;

    fn query(name: &str, sql: &str) -> dexo_storage::SavedQuery {
        dexo_storage::SavedQuery {
            id: name.into(),
            project_id: "p".into(),
            connection_id: "c".into(),
            name: name.into(),
            sql: sql.into(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn the_search_matches_names_and_sql() {
        let mut picker = SavedQueriesPicker {
            items: Some(vec![
                query("Top customers", "select * from customers"),
                query("Late orders", "select * from orders where late"),
            ]),
            ..SavedQueriesPicker::default()
        };
        picker.search.set_text("ORDERS");
        assert_eq!(picker.filtered().len(), 1);
        picker.search.set_text("custom");
        assert_eq!(
            picker.current().map(|query| query.name.as_str()),
            Some("Top customers")
        );
        picker.selected = 5;
        picker.clamp();
        assert_eq!(picker.selected, 0);
    }
}
