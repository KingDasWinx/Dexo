//! Asked before the SQL editor runs a write on production, or a destructive statement
//! anywhere the connection's policy confirms them.

use dexo_app::run_guard::Flagged;

use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

/// Flagged statements listed before the dialog says how many more there are.
const LISTED: usize = 5;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunPrompt {
    /// Exactly what Run sends: the statements that were judged, not a fresh read of
    /// the document.
    pub statements: Vec<String>,
    pub flagged: Vec<Flagged>,
    /// On production, the connection name that has to be typed before Run does anything.
    pub expected: Option<String>,
    pub typed: TextInput,
    pub footer: FooterFocus,
    pub error: Option<String>,
    /// The connection and session the statements were judged for. Run refuses when
    /// either moved, whatever moved it.
    pub connection: String,
    pub session: Option<crate::runtime::SessionId>,
}

impl RunPrompt {
    pub fn new(statements: Vec<String>, flagged: Vec<Flagged>, expected: Option<String>) -> Self {
        // With nothing to type, Cancel has the focus, so an Enter pressed out of habit
        // runs nothing.
        let footer = if expected.is_some() {
            FooterFocus::Input
        } else {
            FooterFocus::Cancel
        };
        Self {
            statements,
            flagged,
            expected,
            typed: TextInput::default(),
            footer,
            error: None,
            connection: String::new(),
            session: None,
        }
    }

    pub fn title(&self) -> &'static str {
        // A statement Dexo could not read is not called destructive: nothing says it is.
        let unread = dexo_sql::Destructive::Unrecognized.describe();
        if self.expected.is_some() {
            "Run on production"
        } else if !self.flagged.is_empty() && self.flagged.iter().all(|f| f.reason == unread) {
            "Run statements Dexo cannot read"
        } else {
            "Run destructive statements"
        }
    }

    /// Nothing to type, or the connection name typed exactly.
    pub fn accepted(&self) -> bool {
        self.expected
            .as_deref()
            .is_none_or(|name| self.typed.as_str() == name)
    }

    pub fn lines(&self, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for flagged in self.flagged.iter().take(LISTED) {
            lines.push(format!(
                "{}. {}",
                flagged.index + 1,
                preview(&flagged.sql, width.saturating_sub(4))
            ));
            lines.push(format!("   {}", flagged.reason));
        }
        if self.flagged.len() > LISTED {
            lines.push(format!("... and {} more", self.flagged.len() - LISTED));
        }
        lines.push(String::new());
        if let Some(name) = &self.expected {
            lines.push(format!("Type {name} to run this on production."));
            lines.push(
                self.typed
                    .inline_line("name: ", self.footer == FooterFocus::Input),
            );
        }
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line("Run", self.footer));
        lines
    }
}

/// The statement's first line, cut to `width` characters with an ellipsis when there
/// is more of it.
fn preview(sql: &str, width: usize) -> String {
    let sql = sql.trim();
    let first = sql.lines().next().unwrap_or_default();
    if first.chars().count() <= width && sql.lines().nth(1).is_none() {
        return first.to_string();
    }
    let mut cut: String = first.chars().take(width.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::RunPrompt;
    use dexo_app::run_guard::Flagged;
    use dexo_sql::Destructive;

    fn flagged(sql: &str, why: Destructive) -> Flagged {
        Flagged {
            index: 0,
            sql: sql.into(),
            reason: why.describe().to_string(),
        }
    }

    /// A statement Dexo could not read was titled "Run destructive statements" over a
    /// body that said it could not read it.
    #[test]
    fn an_unreadable_statement_is_not_called_destructive() {
        let unread = RunPrompt::new(
            vec!["4".into()],
            vec![flagged("4", Destructive::Unrecognized)],
            None,
        );
        assert_eq!(unread.title(), "Run statements Dexo cannot read");
        let drop = RunPrompt::new(
            vec!["drop table t".into()],
            vec![flagged("drop table t", Destructive::Drop)],
            None,
        );
        assert_eq!(drop.title(), "Run destructive statements");
        let mixed = RunPrompt::new(
            Vec::new(),
            vec![
                flagged("4", Destructive::Unrecognized),
                flagged("drop table t", Destructive::Drop),
            ],
            None,
        );
        assert_eq!(mixed.title(), "Run destructive statements");
    }
}
