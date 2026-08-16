use dexo_app::transfer::{Detection, ErrorStrategy, ExportProgress, RejectedRow};

#[derive(Clone, Debug, PartialEq)]
pub struct TransferScreen {
    pub open: bool,
    pub mode: &'static str,
    pub path: String,
    pub format: String,
    pub preview: Vec<String>,
    pub progress: ExportProgress,
    pub rejects: Vec<RejectedRow>,
    pub strategy: ErrorStrategy,
    pub running: bool,
}

impl Default for TransferScreen {
    fn default() -> Self {
        Self {
            open: false,
            mode: "export",
            path: String::new(),
            format: "csv".into(),
            preview: Vec::new(),
            progress: ExportProgress { rows: 0, bytes: 0 },
            rejects: Vec::new(),
            strategy: ErrorStrategy::Stop,
            running: false,
        }
    }
}

impl TransferScreen {
    pub fn sample_preview() -> Self {
        Self {
            open: true,
            mode: "import",
            path: "orders.csv".into(),
            format: "csv".into(),
            preview: vec!["id,name".into(), "1,ok".into(), "2,BAD".into()],
            progress: ExportProgress { rows: 0, bytes: 0 },
            rejects: Vec::new(),
            strategy: ErrorStrategy::RejectFile,
            running: false,
        }
    }

    pub fn sample_progress() -> Self {
        Self {
            open: true,
            mode: "export",
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
        }
    }

    pub fn sample_rejects() -> Self {
        Self {
            open: true,
            mode: "import",
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
        }
    }

    pub fn from_detection(path: &str, detection: &Detection) -> Self {
        Self {
            open: true,
            mode: "import",
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
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!("{} {}", self.mode, self.path),
            format!("format={} strategy={:?}", self.format, self.strategy),
            format!(
                "progress rows={} bytes={} running={}",
                self.progress.rows, self.progress.bytes, self.running
            ),
        ];
        if !self.preview.is_empty() {
            lines.push("preview:".into());
            lines.extend(self.preview.iter().cloned());
        }
        for reject in &self.rejects {
            lines.push(format!("reject line={} {}", reject.line, reject.safe_error));
        }
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
