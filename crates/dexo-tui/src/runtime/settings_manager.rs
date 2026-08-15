use dexo_app::settings::{SettingsFile, load_settings, save_settings};
use std::path::Path;

pub struct SettingsManager {
    pub active: SettingsFile,
}

impl SettingsManager {
    pub fn load(config_dir: &Path) -> Self {
        Self {
            active: load_settings(config_dir),
        }
    }

    pub fn save(&mut self, config_dir: &Path, next: SettingsFile) -> Result<(), String> {
        save_settings(config_dir, &next).map_err(|error| error.to_string())?;
        self.active = next;
        Ok(())
    }
}
