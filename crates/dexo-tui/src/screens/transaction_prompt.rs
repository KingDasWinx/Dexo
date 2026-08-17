use crate::widgets::form::{FooterFocus, footer_line};

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
    pub name: String,
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
        let mut lines = vec![action.into(), format!("name: {}", self.name)];
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line("Submit", self.footer));
        lines
    }
}
