use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCategory {
    Configuration,
    Authentication,
    Network,
    Transport,
    Permission,
    Syntax,
    Conflict,
    Timeout,
    Cancelled,
    Capability,
    Storage,
    ExternalTool,
    McpPolicy,
    Internal,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct AppError {
    category: ErrorCategory,
    message: String,
    technical: Option<String>,
}

impl AppError {
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            technical: None,
        }
    }

    pub fn with_technical(mut self, technical: impl Into<String>) -> Self {
        self.technical = Some(technical.into());
        self
    }

    pub fn category(&self) -> ErrorCategory {
        self.category
    }

    pub fn technical(&self) -> Option<&str> {
        self.technical.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppError, ErrorCategory};

    #[test]
    fn public_error_never_exposes_technical_source() {
        let error = AppError::new(ErrorCategory::Network, "connection failed")
            .with_technical("password=hunter2");
        assert_eq!(error.to_string(), "connection failed");
        assert_eq!(error.category(), ErrorCategory::Network);
    }
}
