use dexo_app::transfer::{Detection, ErrorStrategy, ExportProgress, RejectedRow};

use crate::widgets::form::{FooterFocus, footer_line};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TransferMode {
    #[default]
    Export,
    Import,
    Backup,
    Restore,
}

impl TransferMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Import => "import",
            Self::Backup => "backup",
            Self::Restore => "restore",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransferScreen {
    pub open: bool,
    pub mode: TransferMode,
    pub path: String,
    pub format: String,
    pub preview: Vec<String>,
    pub progress: ExportProgress,
    pub rejects: Vec<RejectedRow>,
    pub strategy: ErrorStrategy,
    pub running: bool,
    pub operation: Option<crate::runtime::OperationId>,
    pub error: Option<String>,
    pub message: Option<String>,
    pub confirm_restore: bool,
    pub footer: FooterFocus,
    pub scroll: usize,
}

impl Default for TransferScreen {
    fn default() -> Self {
        Self {
            open: false,
            mode: TransferMode::Export,
            path: String::new(),
            format: "csv".into(),
            preview: Vec::new(),
            progress: ExportProgress { rows: 0, bytes: 0 },
            rejects: Vec::new(),
            strategy: ErrorStrategy::Stop,
            running: false,
            operation: None,
            error: None,
            message: None,
            confirm_restore: false,
            footer: FooterFocus::Input,
            scroll: 0,
        }
    }
}

impl TransferScreen {
    pub fn sample_preview() -> Self {
        Self {
            open: true,
            mode: TransferMode::Import,
            path: "orders.csv".into(),
            format: "csv".into(),
            preview: vec!["id,name".into(), "1,ok".into(), "2,BAD".into()],
            progress: ExportProgress { rows: 0, bytes: 0 },
            rejects: Vec::new(),
            strategy: ErrorStrategy::RejectFile,
            running: false,
            operation: None,
            error: None,
            message: None,
            confirm_restore: false,
            footer: FooterFocus::Input,
            scroll: 0,
        }
    }

    pub fn sample_progress() -> Self {
        Self {
            open: true,
            mode: TransferMode::Export,
            path: "out.csv".into(),
            format: "csv".into(),
            preview: Vec::new(),
            progress: ExportProgress {
                rows: 10_000,
                bytes: 80_000,
            },
            rejects: Vec::new(),
            strategy: ErrorStrategy::Stop,
            running: true,
            operation: None,
            error: None,
            message: None,
            confirm_restore: false,
            footer: FooterFocus::Input,
            scroll: 0,
        }
    }

    pub fn sample_rejects() -> Self {
        Self {
            open: true,
            mode: TransferMode::Import,
            path: "orders.csv".into(),
            format: "csv".into(),
            preview: vec!["preview ready".into()],
            progress: ExportProgress { rows: 2, bytes: 0 },
            rejects: vec![RejectedRow {
                line: 3,
                safe_error: "invalid value".into(),
                original_fields: vec!["BAD".into()],
            }],
            strategy: ErrorStrategy::RejectFile,
            running: false,
            operation: None,
            error: None,
            message: None,
            confirm_restore: false,
            footer: FooterFocus::Input,
            scroll: 0,
        }
    }

    pub fn from_detection(path: &str, detection: &Detection) -> Self {
        Self {
            open: true,
            mode: TransferMode::Import,
            path: path.into(),
            format: if detection.delimiter == b'\t' {
                "tsv"
            } else {
                "csv"
            }
            .into(),
            preview: detection.sample.iter().map(|row| row.join(",")).collect(),
            progress: ExportProgress { rows: 0, bytes: 0 },
            rejects: Vec::new(),
            strategy: ErrorStrategy::Stop,
            running: false,
            operation: None,
            error: None,
            message: None,
            confirm_restore: false,
            footer: FooterFocus::Input,
            scroll: 0,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("{} {}", self.mode.as_str(), self.path),
            format!("format={} strategy={:?}", self.format, self.strategy),
            format!(
                "progress rows={} bytes={} running={}",
                self.progress.rows, self.progress.bytes, self.running
            ),
        ];
        if self.mode == TransferMode::Restore {
            if self.confirm_restore {
                lines.push("restore confirmed".into());
            } else {
                lines.push("confirm restore into current session".into());
            }
        }
        if let Some(error) = &self.error {
            lines.push(format!("error: {error}"));
        }
        if let Some(message) = &self.message {
            lines.push(message.clone());
        }
        if !self.preview.is_empty() {
            lines.push("preview:".into());
            lines.extend(self.preview.iter().cloned());
        }
        for reject in &self.rejects {
            lines.push(format!("reject line={} {}", reject.line, reject.safe_error));
        }
        lines.push(footer_line("Submit", self.footer));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::TransferScreen;

    #[test]
    fn preview_progress_and_rejects_render() {
        assert!(
            TransferScreen::sample_preview()
                .lines()
                .join("\n")
                .contains("preview")
        );
        assert!(
            TransferScreen::sample_progress()
                .lines()
                .join("\n")
                .contains("10000")
        );
        assert!(
            TransferScreen::sample_rejects()
                .lines()
                .join("\n")
                .contains("reject line=3")
        );
    }
}
