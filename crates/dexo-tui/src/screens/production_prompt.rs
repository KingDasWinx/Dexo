//! Asked before a write that does not pass through the SQL editor -- applying grid
//! edits, applying DDL from the schema form, an import, a restore, an EXPLAIN ANALYZE of
//! a write -- reaches a production connection. The editor's own guard asks the same
//! question; every way of writing to production asks it, the connection's name typed.

use crate::action::Action;
use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

#[derive(Clone, Debug, PartialEq)]
pub struct ProductionPrompt {
    /// The connection's name: what has to be typed.
    pub connection: String,
    /// The session the write was asked for; the prompt refuses once it changed.
    pub session: Option<crate::runtime::SessionId>,
    /// What the write will do, one line each.
    pub what: Vec<String>,
    /// Dispatched again once the name is typed.
    pub then: Box<Action>,
    pub typed: TextInput,
    pub footer: FooterFocus,
    pub error: Option<String>,
}

impl ProductionPrompt {
    pub fn new(
        connection: String,
        session: Option<crate::runtime::SessionId>,
        what: Vec<String>,
        then: Action,
    ) -> Self {
        Self {
            connection,
            session,
            what,
            then: Box::new(then),
            typed: TextInput::default(),
            footer: FooterFocus::Input,
            error: None,
        }
    }

    pub fn accepted(&self) -> bool {
        self.typed.as_str() == self.connection
    }

    pub fn lines(&self, width: usize) -> Vec<String> {
        let mut lines: Vec<String> = self.what.iter().map(|line| cut(line, width)).collect();
        lines.push(String::new());
        lines.push(format!(
            "Type {} to do this on production.",
            self.connection
        ));
        lines.push(
            self.typed
                .inline_line("name: ", self.footer == FooterFocus::Input),
        );
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line("Confirm", self.footer));
        lines
    }
}

/// `text` in at most `width` characters, an ellipsis marking what was cut.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}
