use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{AppError, ErrorCategory};

pub const SETTINGS_VERSION: u32 = 1;

/// The light/dark surface. Orthogonal to [`SettingsFile::accent`], which carries
/// the system's primary color.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ModeId {
    #[default]
    Dark,
    Light,
    HighContrast,
}

pub const DEFAULT_ACCENT: &str = "cyan";

fn default_accent() -> String {
    DEFAULT_ACCENT.into()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeymapConfig {
    pub run_statement: String,
    /// Settings written before profiles were persisted carry no value here.
    #[serde(default = "default_keymap_profile")]
    pub profile: String,
}

fn default_keymap_profile() -> String {
    "default".into()
}

impl Default for KeymapConfig {
    fn default() -> Self {
        Self {
            run_statement: "Ctrl+Enter".into(),
            profile: default_keymap_profile(),
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
    /// Named `theme` in settings written before the mode and the accent split.
    #[serde(alias = "theme")]
    pub mode: ModeId,
    #[serde(default = "default_accent")]
    pub accent: String,
    pub keymap: KeymapConfig,
    pub mouse: bool,
    pub animation: bool,
    pub unicode: UnicodeMode,
    pub recovery_interval_secs: u64,
    /// Additive, so it must have a serde default: `load_settings` falls back to the
    /// whole default file on any deserialize failure, and a missing field would take
    /// the user's theme and keymap down with it.
    #[serde(default)]
    pub completion_trigger: dexo_sql::TriggerMode,
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            mode: ModeId::Dark,
            accent: default_accent(),
            keymap: KeymapConfig::default(),
            completion_trigger: dexo_sql::TriggerMode::default(),
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
    use super::{ModeId, load_settings, save_settings};
    use crate::settings::SettingsFile;

    #[test]
    fn saved_mode_and_accent_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let settings = SettingsFile {
            mode: ModeId::HighContrast,
            accent: "rose".into(),
            mouse: false,
            ..SettingsFile::default()
        };
        save_settings(dir.path(), &settings).unwrap();
        let loaded = load_settings(dir.path());
        assert_eq!(loaded.mode, ModeId::HighContrast);
        assert_eq!(loaded.accent, "rose");
        assert!(!loaded.mouse);
    }

    #[test]
    fn settings_written_before_the_split_still_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            super::settings_path(dir.path()),
            "version = 1\ntheme = \"Light\"\nmouse = true\nanimation = true\nunicode = \"Unicode\"\nrecovery_interval_secs = 5\n[keymap]\nrun_statement = \"Ctrl+Enter\"\nprofile = \"default\"\n",
        )
        .unwrap();
        let loaded = load_settings(dir.path());
        assert_eq!(loaded.mode, ModeId::Light);
        assert_eq!(loaded.accent, super::DEFAULT_ACCENT);
    }
}
