use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SavepointIntent {
    Create,
    Rollback,
    Release,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransactionPrompt {
    pub open: bool,
    pub intent: Option<SavepointIntent>,
    pub name: TextInput,
    pub error: Option<String>,
    pub footer: FooterFocus,
}

impl TransactionPrompt {
    pub fn lines(&self) -> Vec<String> {
        let action = match self.intent {
            Some(SavepointIntent::Create) => "create savepoint",
            Some(SavepointIntent::Rollback) => "rollback savepoint",
            Some(SavepointIntent::Release) => "release savepoint",
            None => "savepoint",
        };
        let mut lines = Vec::new();
        if action != "savepoint" {
            lines.push(action.into());
        }
        lines.push(format!("name: {}", self.name.as_str()));
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line("Submit", self.footer));
        lines
    }
}
