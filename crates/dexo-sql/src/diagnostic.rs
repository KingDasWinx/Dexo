use std::ops::Range;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSource {
    Local,
    Server,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub source: DiagnosticSource,
    pub message: String,
    pub byte_range: Option<Range<usize>>,
    pub server_code: Option<String>,
}

impl Diagnostic {
    pub fn local(message: impl Into<String>, byte_range: Range<usize>) -> Self {
        Self {
            source: DiagnosticSource::Local,
            message: message.into(),
            byte_range: Some(byte_range),
            server_code: None,
        }
    }

    pub fn server(
        message: impl Into<String>,
        code: impl Into<String>,
        position: Option<u32>,
    ) -> Self {
        Self {
            source: DiagnosticSource::Server,
            message: message.into(),
            byte_range: position.map(|pos| pos as usize..pos as usize),
            server_code: Some(code.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, DiagnosticSource};

    #[test]
    fn local_and_server_diagnostics_are_distinct() {
        let local = Diagnostic::local("bad token", 0..3);
        let server = Diagnostic::server("syntax error", "42601", Some(4));
        assert_eq!(local.source, DiagnosticSource::Local);
        assert_eq!(server.source, DiagnosticSource::Server);
        assert_eq!(server.server_code.as_deref(), Some("42601"));
    }
}
