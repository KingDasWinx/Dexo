use crate::widgets::form::{FooterFocus, footer_line};
use crate::widgets::text_input::TextInput;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentNameIntent {
    Create,
    Rename,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocumentNamePrompt {
    pub open: bool,
    pub intent: Option<DocumentNameIntent>,
    pub name: TextInput,
    pub default_name: String,
    pub document_index: usize,
    pub error: Option<String>,
    pub footer: FooterFocus,
}

impl DocumentNamePrompt {
    pub fn open_create(default_name: String) -> Self {
        Self {
            open: true,
            intent: Some(DocumentNameIntent::Create),
            name: TextInput::new(default_name.clone()),
            default_name,
            document_index: 0,
            error: None,
            footer: FooterFocus::Input,
        }
    }

    pub fn open_rename(document_index: usize, current: String) -> Self {
        Self {
            open: true,
            intent: Some(DocumentNameIntent::Rename),
            name: TextInput::new(current.clone()),
            default_name: current,
            document_index,
            error: None,
            footer: FooterFocus::Input,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.intent {
            Some(DocumentNameIntent::Create) => "New document",
            Some(DocumentNameIntent::Rename) => "Rename document",
            None => "Document",
        }
    }

    pub fn submit_label(&self) -> &'static str {
        match self.intent {
            Some(DocumentNameIntent::Create) => "Create",
            Some(DocumentNameIntent::Rename) => "Rename",
            None => "Submit",
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let focused = self.footer == FooterFocus::Input;
        let mut lines = vec![self.name.inline_line("name: ", focused)];
        if let Some(error) = &self.error {
            lines.push(error.clone());
        }
        lines.push(footer_line(self.submit_label(), self.footer));
        lines
    }
}

pub fn normalize_document_name(input: &str, fallback: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let name = if trimmed.is_empty() { fallback.trim() } else { trimmed };
    if name.is_empty() {
        return Err("name is required".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("name cannot contain path separators".into());
    }
    Ok(if name.contains('.') {
        name.to_string()
    } else {
        format!("{name}.sql")
    })
}

#[cfg(test)]
mod tests {
    use super::{DocumentNamePrompt, normalize_document_name};

    #[test]
    fn empty_input_falls_back_to_default_name() {
        assert_eq!(
            normalize_document_name("  ", "query-1.sql").unwrap(),
            "query-1.sql"
        );
    }

    #[test]
    fn bare_name_gets_sql_extension() {
        assert_eq!(normalize_document_name("reports", "query-1.sql").unwrap(), "reports.sql");
    }

    #[test]
    fn create_prompt_prefills_the_default_name() {
        let prompt = DocumentNamePrompt::open_create("query-2.sql".into());
        assert!(prompt.open);
        assert_eq!(prompt.name.as_str(), "query-2.sql");
        assert!(prompt.lines()[0].contains("query-2.sql"));
    }
}
