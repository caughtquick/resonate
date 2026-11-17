//! Configuration and runtime theming system for Resonate Studio.
//! Powered by cquifw (CaughtQuick UI Framework).

pub use cquifw::theme::{
    parse_color, with_alpha, AccentOverrides, AppearanceConfig, ColorOverrides, Palette,
};
use serde::Deserialize;
use slint::ComponentHandle;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use super::{MainWindow, Theme};

/// Top-level configuration file schema (`~/.config/resonate-studio/config.toml`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct StudioConfig {
    #[serde(default)]
    pub appearance: AppearanceConfig,
}

/// Applies the given configuration palette to the Slint Theme global.
pub fn apply_theme_to_ui(window: &MainWindow, config: &StudioConfig) {
    let mut palette = Palette::from_preset(&config.appearance.theme_preset);
    palette.apply_overrides(&config.appearance);

    let theme = window.global::<Theme>();
    cquifw::apply_theme_fields!(&theme, &palette);
}

/// Resolves the absolute path to `~/.config/resonate-studio/config.toml`.
pub fn config_path() -> PathBuf {
    cquifw::settings::resolve_config_path("resonate-studio")
}

const DEFAULT_CONFIG_TOML: &str = r##"# ==============================================================================
# Resonate Studio Theme Configuration
# File: ~/.config/resonate-studio/config.toml
# ==============================================================================
# Live reloaded: Edits to this file take effect immediately without restarting!

[appearance]
# Available theme presets:
#   "gruvbox_dark_soft" (Default - warm charcoal & amber)
#   "catppuccin_mocha"   (Modern mauve, teal & deep navy)
#   "tokyo_night"        (Vibrant blue, cyan & storm dark)
#   "nord"               (Crisp arctic frost & slate)
#   "dracula"            (Gothic purple, pink & dark graphite)
#   "generic_dark"       (Clean zinc dark mode & sky blue)
theme_preset = "gruvbox_dark_soft"

# Font family applied across all studio labels, telemetry, and cards
font_family = "JetBrainsMono Nerd Font"

# Surface & background color overrides (Hex: #RRGGBB or #RRGGBBAA)
# Supports native names as well as cqicshell bar/card keys!
[appearance.colors]
# bg_sidebar = "#1d1c1a"
# bg_main = "#242424"
# bg_card = "#282828"
# bg_tile = "#302f2d"
# bg_card_hover = "#32302f"
# bg_input = "#1c1c1c"
# border = "#3a3836"
# text_primary = "#ebdbb2"
# text_secondary = "#bdae93"
# text_muted = "#7c6f64"

# Accent & status color overrides
# Supports native names as well as cqicshell cluster keys!
[appearance.accents]
# primary = "#fabd2f"       # Main studio accent & active rings
# bluetooth = "#d3869b"     # Multipoint & device icons
# battery = "#b8bb26"       # Battery levels & charging state
# audio = "#83a598"         # Equalizer bars & headphone details
# media = "#fe8019"         # Smart auto-pause tile
# danger = "#fb4934"        # Disconnected state & errors
# success = "#b8bb26"       # Connected state & confirmations
"##;

/// Ensures `~/.config/resonate-studio/config.toml` exists with default contents.
/// If it exists, parses and returns the configuration; otherwise creates it and returns default.
pub fn ensure_config_file() -> StudioConfig {
    let path = config_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, DEFAULT_CONFIG_TOML);
        return toml::from_str(DEFAULT_CONFIG_TOML).unwrap_or_default();
    }

    match fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("[resonate-studio] Warning: failed to parse config.toml: {e}");
            toml::from_str(DEFAULT_CONFIG_TOML).unwrap_or_default()
        }),
        Err(_) => toml::from_str(DEFAULT_CONFIG_TOML).unwrap_or_default(),
    }
}

/// Spawns a background thread that monitors `config.toml` modification time every 600ms.
/// When changes occur, it invokes `apply_theme_to_ui` on the main thread via Slint's event loop.
pub fn start_theme_watcher(weak_ui: slint::Weak<MainWindow>) {
    let path = config_path();

    cquifw::theme::ConfigWatcher::watch(path.clone(), Duration::from_millis(600), move || {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(config) = toml::from_str::<StudioConfig>(&content) {
                let _ = weak_ui.upgrade_in_event_loop(move |ui| {
                    apply_theme_to_ui(&ui, &config);
                });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color_rgb_and_rgba() {
        let col = parse_color("#1d1c1a").expect("Valid 6-digit hex");
        assert_eq!(col.red(), 0x1d);
        assert_eq!(col.green(), 0x1c);
        assert_eq!(col.blue(), 0x1a);
        assert_eq!(col.alpha(), 255);

        let col_alpha = parse_color("#fabd2f80").expect("Valid 8-digit hex");
        assert_eq!(col_alpha.red(), 0xfa);
        assert_eq!(col_alpha.green(), 0xbd);
        assert_eq!(col_alpha.blue(), 0x2f);
        assert_eq!(col_alpha.alpha(), 0x80);

        assert!(parse_color("invalid").is_none());
    }

    #[test]
    fn test_theme_presets() {
        let gruvbox = Palette::from_preset("gruvbox_dark_soft");
        let mocha = Palette::from_preset("catppuccin_mocha");
        let tokyo = Palette::from_preset("tokyo_night");
        let nord = Palette::from_preset("nord");
        let dracula = Palette::from_preset("dracula");

        assert_ne!(gruvbox.bg_main, mocha.bg_main);
        assert_ne!(mocha.accent, tokyo.accent);
        assert_ne!(nord.bg_sidebar, dracula.bg_sidebar);
    }

    #[test]
    fn test_toml_deserialization_and_cqicshell_aliases() {
        let toml_str = r##"
        [appearance]
        theme_preset = "tokyo_night"
        font_family = "FiraCode Nerd Font"

        [appearance.colors]
        bar_bg = "#111111"
        card_bg = "#222222"
        bar_fg = "#eeeeee"
        subtext_color = "#999999"

        [appearance.accents]
        launcher = "#ff5555"
        bluetooth = "#55ffff"
        "##;

        let config: StudioConfig = toml::from_str(toml_str).expect("Valid toml");
        assert_eq!(config.appearance.theme_preset, "tokyo_night");
        assert_eq!(config.appearance.font_family, "FiraCode Nerd Font");

        let mut palette = Palette::from_preset(&config.appearance.theme_preset);
        palette.apply_overrides(&config.appearance);

        assert_eq!(palette.font_family, "FiraCode Nerd Font");
        assert_eq!(palette.bg_sidebar, parse_color("#111111").unwrap());
        assert_eq!(palette.bg_card, parse_color("#222222").unwrap());
        assert_eq!(palette.text_primary, parse_color("#eeeeee").unwrap());
        assert_eq!(palette.text_secondary, parse_color("#999999").unwrap());
        assert_eq!(palette.accent, parse_color("#ff5555").unwrap());
        assert_eq!(palette.accent_bluetooth, parse_color("#55ffff").unwrap());
    }
}
