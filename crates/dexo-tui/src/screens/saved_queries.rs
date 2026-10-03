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
    /// The dialog's lines, `width` columns wide.
    pub fn lines(&self, width: usize) -> Vec<String> {
        let focused = self.footer == FooterFocus::Input;
        let count = self.sql.lines().count();
        let mut lines = vec![
            self.name.inline_line_within("name: ", focused, width),
            format!(
                "{}, {count} line{}",
                self.source,
                if count == 1 { "" } else { "s" }
            ),
            "The same name replaces that query.".into(),
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
    /// `None` until the list has been read.
    pub items: Option<Vec<SavedQuery>>,
    pub search: TextInput,
    /// Into what the search lets through.
    pub selected: usize,
    pub renaming: Option<TextInput>,
    /// Delete asked; the footer's focus while it waits for an answer.
    pub deleting: Option<FooterFocus>,
    pub error: Option<String>,
    /// Each item's name and SQL, lowered, for the search; see [`Self::set_items`].
    pub lowered: Vec<String>,
}

impl SavedQueriesPicker {
    /// The queries whose name or SQL holds the search text, without regard to case.
    /// Takes the list as read, with each query's text lowered once for the search.
    pub fn set_items(&mut self, items: Vec<SavedQuery>) {
        self.lowered = items
            .iter()
            .map(|query| format!("{}\n{}", query.name, query.sql).to_lowercase())
            .collect();
        self.items = Some(items);
    }

    pub fn filtered(&self) -> Vec<&SavedQuery> {
        let needle = self.search.as_str().trim().to_lowercase();
        self.items
            .iter()
            .flatten()
            .zip(&self.lowered)
            .filter(|(_, lowered)| needle.is_empty() || lowered.contains(&needle))
            .map(|(query, _)| query)
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
            connection_id: "c".into(),
            name: name.into(),
            sql: sql.into(),
        }
    }

    #[test]
    fn the_search_matches_names_and_sql() {
        let mut picker = SavedQueriesPicker::default();
        picker.set_items(vec![
            query("Top customers", "select * from customers"),
            query("Late orders", "select * from orders where late"),
        ]);
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
