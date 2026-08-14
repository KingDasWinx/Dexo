use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriverErrorCategory {
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
    Internal,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct DriverError {
    category: DriverErrorCategory,
    message: String,
    native_code: Option<String>,
    position: Option<u32>,
    retryable: bool,
}

impl DriverError {
    pub fn new(category: DriverErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            native_code: None,
            position: None,
            retryable: false,
        }
    }

    pub fn unsupported(reason: impl Into<String>) -> Self {
        Self::new(DriverErrorCategory::Capability, reason)
    }

    pub fn with_native_code(mut self, code: impl Into<String>) -> Self {
        self.native_code = Some(code.into());
        self
    }

    pub fn with_position(mut self, position: u32) -> Self {
        self.position = Some(position);
        self
    }

    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    pub fn category(&self) -> DriverErrorCategory {
        self.category
    }

    pub fn native_code(&self) -> Option<&str> {
        self.native_code.as_deref()
    }

    pub fn position(&self) -> Option<u32> {
        self.position
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }
}
