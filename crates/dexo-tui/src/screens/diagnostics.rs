use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiagnosticsScreen {
    pub open: bool,
    pub preview: String,
    pub path: Option<PathBuf>,
    pub writing: bool,
    pub error: Option<String>,
}

impl DiagnosticsScreen {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![self.preview.clone()];
        if let Some(path) = &self.path {
            lines.push(path.display().to_string());
        }
        if self.writing {
            lines.push("writing...".into());
        }
        if let Some(error) = &self.error {
            lines.push(format!("error: {error}"));
        }
        lines
    }
}
