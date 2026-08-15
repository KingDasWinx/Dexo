use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{AppError, ErrorCategory};

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ThemeId {
    #[default]
    Dark,
    HighContrast,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeymapConfig {
    pub run_statement: String,
}

impl Default for KeymapConfig {
    fn default() -> Self {
        Self {
            run_statement: "Ctrl+Enter".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum UnicodeMode {
    #[default]
    Unicode,
    Ascii,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SettingsFile {
    pub version: u32,
    pub theme: ThemeId,
    pub keymap: KeymapConfig,
    pub mouse: bool,
    pub animation: bool,
    pub unicode: UnicodeMode,
    pub recovery_interval_secs: u64,
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: ThemeId::Dark,
            keymap: KeymapConfig::default(),
            mouse: true,
            animation: true,
            unicode: UnicodeMode::Unicode,
            recovery_interval_secs: 5,
        }
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.toml")
}

pub fn load_settings(config_dir: &Path) -> SettingsFile {
    let path = settings_path(config_dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return SettingsFile::default();
    };
    match toml::from_str::<SettingsFile>(&text) {
        Ok(settings) if settings.version == SETTINGS_VERSION => settings,
        _ => SettingsFile::default(),
    }
}

pub fn save_settings(config_dir: &Path, settings: &SettingsFile) -> Result<(), AppError> {
    if settings.version != SETTINGS_VERSION {
        return Err(AppError::new(
            ErrorCategory::Configuration,
            "unsupported settings version",
        ));
    }
    std::fs::create_dir_all(config_dir)
        .map_err(|error| AppError::new(ErrorCategory::Storage, error.to_string()))?;
    let path = settings_path(config_dir);
    if path.exists() {
        let _ = std::fs::copy(&path, path.with_extension("toml.bak"));
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(
        &tmp,
        toml::to_string(settings)
            .map_err(|error| AppError::new(ErrorCategory::Internal, error.to_string()))?,
    )
    .map_err(|error| AppError::new(ErrorCategory::Storage, error.to_string()))?;
    std::fs::rename(&tmp, path)
        .map_err(|error| AppError::new(ErrorCategory::Storage, error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{ThemeId, load_settings, save_settings};
    use crate::settings::SettingsFile;

    #[test]
    fn saved_theme_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let settings = SettingsFile {
            theme: ThemeId::HighContrast,
            mouse: false,
            ..SettingsFile::default()
        };
        save_settings(dir.path(), &settings).unwrap();
        let loaded = load_settings(dir.path());
        assert_eq!(loaded.theme, ThemeId::HighContrast);
        assert!(!loaded.mouse);
    }
}
