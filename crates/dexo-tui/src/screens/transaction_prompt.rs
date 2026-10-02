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
    /// Named for what it does: the three share a form, not a title.
    pub fn title(&self) -> &'static str {
        match self.intent {
            Some(SavepointIntent::Create) => "Create savepoint",
            Some(SavepointIntent::Rollback) => "Roll back to savepoint",
            Some(SavepointIntent::Release) => "Release savepoint",
            None => "Savepoint",
        }
    }

    pub fn submit_label(&self) -> &'static str {
        match self.intent {
            Some(SavepointIntent::Create) => "Create",
            Some(SavepointIntent::Rollback) => "Roll back",
            Some(SavepointIntent::Release) => "Release",
            None => "Submit",
        }
    }

    /// What the savepoint is for, in a line: the three share a form and say different things.
    fn hint(&self) -> &'static str {
        match self.intent {
            Some(SavepointIntent::Create) => "A point in this transaction to come back to.",
            Some(SavepointIntent::Rollback) => "Undoes what was done after that savepoint.",
            Some(SavepointIntent::Release) => "Keeps the work and forgets the savepoint.",
            None => "",
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            self.hint().to_string(),
            format!("name: {}", self.name.as_str()),
        ];
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line(self.submit_label(), self.footer));
        lines
    }
}
