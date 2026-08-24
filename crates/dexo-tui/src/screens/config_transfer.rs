use std::collections::HashMap;
use std::path::PathBuf;

use dexo_storage::{ImportPreview, ImportResolution};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConfigTransferMode {
    #[default]
    Closed,
    Export,
    Import,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigTransferScreen {
    pub open: bool,
    pub mode: ConfigTransferMode,
    pub path: PathBuf,
    pub preview: Option<ImportPreview>,
    pub resolutions: HashMap<String, ImportResolution>,
    pub needing_secret: Vec<String>,
    pub message: Option<String>,
}

impl ConfigTransferScreen {
    pub fn with_resolution(mut self, name: &str, resolution: ImportResolution) -> Self {
        self.resolutions.insert(name.to_string(), resolution);
        self
    }

    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.path.as_os_str().is_empty() {
            lines.push(format!("path: {}", self.path.display()));
        }
        if let Some(preview) = &self.preview {
            if preview.conflicts.is_empty() {
                lines.push("conflicts: none".into());
            } else {
                lines.push(format!("conflicts: {}", preview.conflicts.join(", ")));
            }
            for name in &preview.conflicts {
                let resolution = self
                    .resolutions
                    .get(name)
                    .cloned()
                    .unwrap_or(ImportResolution::Skip);
                lines.push(format!("  {name}: {resolution:?}"));
            }
            if !preview.connections_needing_secret.is_empty() {
                lines.push(format!(
                    "need secret: {}",
                    preview.connections_needing_secret.join(", ")
                ));
            }
        }
        if !self.needing_secret.is_empty() {
            lines.push(format!(
                "imported, need secret: {}",
                self.needing_secret.join(", ")
            ));
        }
        if let Some(message) = &self.message {
            lines.push(message.clone());
        }
        lines
    }
}
