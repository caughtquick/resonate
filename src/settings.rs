use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsData {
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub cached_case_batteries: HashMap<String, u8>,
    #[serde(default)]
    pub preferred_noise_modes: HashMap<String, String>,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettingsData {
    fn default() -> Self {
        Self {
            notifications_enabled: true,
            cached_case_batteries: HashMap::new(),
            preferred_noise_modes: HashMap::new(),
        }
    }
}

pub struct SettingsStore {
    file_path: PathBuf,
    pub data: AppSettingsData,
}

impl SettingsStore {
    pub fn load() -> Self {
        let config_dir = dirs_config_dir().join("resonate");
        let file_path = config_dir.join("settings.json");

        let data = if file_path.exists() {
            fs::read_to_string(&file_path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
                .unwrap_or_default()
        } else {
            AppSettingsData::default()
        };

        Self { file_path, data }
    }

    pub fn save(&self) {
        if let Some(parent) = self.file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.data) {
            let _ = fs::write(&self.file_path, json);
        }
    }

    pub fn get_cached_case_battery(&self, address: &str) -> Option<u8> {
        self.data.cached_case_batteries.get(&address.to_uppercase()).copied()
    }

    pub fn set_cached_case_battery(&mut self, address: &str, level: u8) {
        self.data.cached_case_batteries.insert(address.to_uppercase(), level);
        self.save();
    }

    pub fn get_preferred_noise_mode(&self, address: &str) -> Option<String> {
        self.data.preferred_noise_modes.get(&address.to_uppercase()).cloned()
    }

    pub fn set_preferred_noise_mode(&mut self, address: &str, mode: &str) {
        self.data.preferred_noise_modes.insert(address.to_uppercase(), mode.to_string());
        self.save();
    }
}

pub fn dirs_config_dir() -> PathBuf {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME") {
        if !config_home.is_empty() {
            return PathBuf::from(config_home);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config");
    }
    PathBuf::from("/tmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_store_save_and_load() {
        let mut store = SettingsStore::load();
        store.set_cached_case_battery("6C:12:70:3F:8E:AE", 85);
        assert_eq!(store.get_cached_case_battery("6C:12:70:3F:8E:AE"), Some(85));
    }
}
