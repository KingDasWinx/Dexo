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

/// What an I/O failure says, in words: the operating system's `Connection refused (os
/// error 111)` is a connection that was refused.
pub fn plain_cause(text: &str) -> String {
    let text = text.trim();
    let text = match text.rfind(" (os error ") {
        Some(at) if text.ends_with(')') => &text[..at],
        _ => text,
    };
    if text.contains("failed to lookup address") || text.contains("Name or service not known") {
        return "the host name was not found".into();
    }
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The innermost cause of `error`: the one link of a chain a person can act on, where
/// the outer ones only say which step was running.
pub fn root_cause(error: &(dyn std::error::Error + 'static)) -> String {
    let mut innermost = error;
    while let Some(next) = innermost.source() {
        innermost = next;
    }
    plain_cause(&innermost.to_string())
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct DriverError {
    category: DriverErrorCategory,
    message: String,
    native_code: Option<String>,
    /// 1-based character offset into the statement, where the server says it failed.
    position: Option<u32>,
    detail: Option<String>,
    hint: Option<String>,
    retryable: bool,
}

impl DriverError {
    pub fn new(category: DriverErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
            native_code: None,
            position: None,
            detail: None,
            hint: None,
            retryable: false,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// A connect that never reached a server: names the address it tried and why it
    /// failed, so the message says what happened and to what.
    pub fn unreachable(host: &str, port: u16, cause: &str) -> Self {
        Self::new(
            DriverErrorCategory::Network,
            format!("could not connect to {host}:{port}: {cause}"),
        )
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

#[cfg(test)]
mod tests {
    use super::{DriverError, DriverErrorCategory, plain_cause, root_cause};

    #[test]
    fn an_io_failure_reads_in_words() {
        assert_eq!(
            plain_cause("Connection refused (os error 111)"),
            "connection refused"
        );
        assert_eq!(
            plain_cause("failed to lookup address information: Name or service not known"),
            "the host name was not found"
        );
        assert_eq!(plain_cause("timed out"), "timed out");
    }

    #[test]
    fn the_root_of_a_chain_is_the_part_to_act_on() {
        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("error connecting to server")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let refused = Outer(std::io::Error::from(std::io::ErrorKind::ConnectionRefused));
        assert_eq!(root_cause(&refused), "connection refused");
        let error = DriverError::unreachable("127.0.0.1", 1, &root_cause(&refused));
        assert_eq!(
            error.to_string(),
            "could not connect to 127.0.0.1:1: connection refused"
        );
        assert_eq!(error.category(), DriverErrorCategory::Network);
    }
}
