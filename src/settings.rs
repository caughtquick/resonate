//! Persistent user settings and state caching.
//!
//! Saves and restores configuration preferences (smart auto-pause toggle, desktop notifications,
//! preferred noise cancellation modes per MAC, and last known charging case battery levels)
//! to `$XDG_CONFIG_HOME/resonate/settings.json` (falling back to `~/.config/resonate/settings.json`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Serializable configuration data model stored in `settings.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettingsData {
    /// Whether desktop notifications are enabled for battery warnings and connection alerts.
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    /// Whether smart auto-pause / auto-resume is active when removing earbuds or headphones.
    #[serde(default = "default_true")]
    pub auto_pause_enabled: bool,
    /// Last known battery level of charging cases indexed by uppercase device MAC address.
    #[serde(default)]
    pub cached_case_batteries: HashMap<String, u8>,
    /// Last selected listening mode per device (e.g. `"noise_cancellation"`, `"transparency"`).
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
            auto_pause_enabled: true,
            cached_case_batteries: HashMap::new(),
            preferred_noise_modes: HashMap::new(),
        }
    }
}

/// Thread-safe settings store managing disk read and write operations.
pub struct SettingsStore {
    file_path: PathBuf,
    /// In-memory copy of the settings data.
    pub data: AppSettingsData,
}

impl SettingsStore {
    /// Loads user settings from `settings.json` if it exists, or initializes default settings.
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

    /// Persists the current settings to disk in JSON format.
    pub fn save(&self) {
        if let Some(parent) = self.file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.data) {
            let _ = fs::write(&self.file_path, json);
        }
    }

    /// Returns the cached case battery percentage for the given device address if known.
    pub fn get_cached_case_battery(&self, address: &str) -> Option<u8> {
        self.data
            .cached_case_batteries
            .get(&address.to_uppercase())
            .copied()
    }

    /// Updates and persists the cached case battery percentage for a device address.
    pub fn set_cached_case_battery(&mut self, address: &str, level: u8) {
        self.data
            .cached_case_batteries
            .insert(address.to_uppercase(), level);
        self.save();
    }

    /// Returns the user's preferred noise control mode for a device address.
    pub fn get_preferred_noise_mode(&self, address: &str) -> Option<String> {
        self.data
            .preferred_noise_modes
            .get(&address.to_uppercase())
            .cloned()
    }

    /// Updates and persists the user's preferred noise control mode for a device address.
    pub fn set_preferred_noise_mode(&mut self, address: &str, mode: &str) {
        self.data
            .preferred_noise_modes
            .insert(address.to_uppercase(), mode.to_string());
        self.save();
    }

    /// Returns true if smart auto-pause is enabled.
    pub fn get_auto_pause(&self) -> bool {
        self.data.auto_pause_enabled
    }

    /// Updates and persists the smart auto-pause toggle state.
    pub fn set_auto_pause(&mut self, enabled: bool) {
        self.data.auto_pause_enabled = enabled;
        self.save();
    }
}

/// Resolves the user's configuration base directory according to XDG standards.
///
/// Priority: `$XDG_CONFIG_HOME` > `$HOME/.config` > `/tmp`.
pub fn dirs_config_dir() -> PathBuf {
    if let Ok(config_home) = std::env::var("XDG_CONFIG_HOME")
        && !config_home.is_empty()
    {
        return PathBuf::from(config_home);
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
        let temp_dir = std::env::temp_dir().join("resonate_test_settings");
        let file_path = temp_dir.join("settings.json");
        let _ = fs::remove_file(&file_path);

        let mut store = SettingsStore {
            file_path: file_path.clone(),
            data: AppSettingsData::default(),
        };

        store.set_cached_case_battery("AA:BB:CC:DD:EE:FF", 85);
        store.set_preferred_noise_mode("AA:BB:CC:DD:EE:FF", "noise_cancellation");
        store.set_auto_pause(false);

        let loaded_content = fs::read_to_string(&file_path).expect("read failed");
        let loaded: AppSettingsData = serde_json::from_str(&loaded_content).expect("parse failed");

        assert_eq!(
            loaded.cached_case_batteries.get("AA:BB:CC:DD:EE:FF"),
            Some(&85)
        );
        assert_eq!(
            loaded
                .preferred_noise_modes
                .get("AA:BB:CC:DD:EE:FF")
                .map(|s| s.as_str()),
            Some("noise_cancellation")
        );
        assert!(!loaded.auto_pause_enabled);

        let _ = fs::remove_file(&file_path);
        let _ = fs::remove_dir(&temp_dir);
    }
}
