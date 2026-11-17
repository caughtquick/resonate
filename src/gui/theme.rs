//! Configuration and runtime theming system for Resonate Studio.
//!
//! Reads theme preferences from `~/.config/resonate-studio/config.toml`.
//! Provides preset support (Gruvbox Dark Soft, Catppuccin Mocha, Tokyo Night, Nord, Dracula, Generic Dark),
//! custom palette overrides, and a live file watcher that dynamically hot-reloads the UI without restarting.

use serde::Deserialize;
use slint::ComponentHandle;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{MainWindow, Theme};

/// Top-level configuration file schema (`~/.config/resonate-studio/config.toml`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct StudioConfig {
    #[serde(default)]
    pub appearance: AppearanceConfig,
}

/// Appearance settings including preset name, font, and color overrides.
#[derive(Debug, Clone, Deserialize)]
pub struct AppearanceConfig {
    #[serde(default = "default_preset")]
    pub theme_preset: String,

    #[serde(default = "default_font")]
    pub font_family: String,

    #[serde(default)]
    pub colors: ColorOverrides,

    #[serde(default)]
    pub accents: AccentOverrides,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme_preset: default_preset(),
            font_family: default_font(),
            colors: ColorOverrides::default(),
            accents: AccentOverrides::default(),
        }
    }
}

fn default_preset() -> String {
    "gruvbox_dark_soft".to_string()
}

fn default_font() -> String {
    "JetBrainsMono Nerd Font".to_string()
}

/// Custom surface color overrides.
/// Also accepts cqicshell naming conventions (e.g. `bar_bg`, `applet_bg`, `card_bg`, `subtext_color`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ColorOverrides {
    pub bg_darkest: Option<String>,
    pub bg_sidebar: Option<String>,
    pub bg_main: Option<String>,
    pub bg_card: Option<String>,
    pub bg_tile: Option<String>,
    pub bg_card_hover: Option<String>,
    pub bg_input: Option<String>,
    pub bg_pill: Option<String>,

    pub border: Option<String>,
    pub border_light: Option<String>,

    pub text_primary: Option<String>,
    pub text_secondary: Option<String>,
    pub text_muted: Option<String>,
    pub text_disabled: Option<String>,

    // cqicshell compatibility aliases
    pub bar_bg: Option<String>,
    pub applet_bg: Option<String>,
    pub card_bg: Option<String>,
    pub subtext_color: Option<String>,
    pub bar_fg: Option<String>,
    pub pill_bg: Option<String>,
    pub pill_border_color: Option<String>,
}

/// Custom accent color overrides.
/// Also accepts cqicshell naming conventions (e.g. `workspaces`, `bluetooth`, `battery`, `audio`, `media`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AccentOverrides {
    pub primary: Option<String>,
    pub accent: Option<String>,
    pub bluetooth: Option<String>,
    pub battery: Option<String>,
    pub audio: Option<String>,
    pub media: Option<String>,
    pub orange: Option<String>,
    pub purple: Option<String>,
    pub aqua: Option<String>,
    pub blue: Option<String>,

    pub success: Option<String>,
    pub warning: Option<String>,
    pub danger: Option<String>,
    pub info: Option<String>,

    // cqicshell compatibility aliases
    pub launcher: Option<String>,
    pub workspaces: Option<String>,
}

/// Internal resolved color palette with all standard colors.
#[derive(Debug, Clone)]
pub struct Palette {
    pub font_family: String,

    pub bg_darkest: slint::Color,
    pub bg_sidebar: slint::Color,
    pub bg_main: slint::Color,
    pub bg_card: slint::Color,
    pub bg_tile: slint::Color,
    pub bg_card_hover: slint::Color,
    pub bg_card_active: slint::Color,
    pub bg_input: slint::Color,
    pub bg_pill: slint::Color,

    pub accent: slint::Color,
    pub accent_hover: slint::Color,
    pub accent_subtle: slint::Color,
    pub accent_orange: slint::Color,
    pub accent_purple: slint::Color,
    pub accent_aqua: slint::Color,
    pub accent_blue: slint::Color,

    pub icon_bg_amber: slint::Color,
    pub icon_bg_green: slint::Color,
    pub icon_bg_purple: slint::Color,
    pub icon_bg_aqua: slint::Color,
    pub icon_bg_orange: slint::Color,
    pub icon_bg_neutral: slint::Color,

    pub success: slint::Color,
    pub warning: slint::Color,
    pub danger: slint::Color,
    pub info: slint::Color,

    pub text_primary: slint::Color,
    pub text_secondary: slint::Color,
    pub text_muted: slint::Color,
    pub text_disabled: slint::Color,

    pub border: slint::Color,
    pub border_light: slint::Color,
}

/// Parses a hex color string in `#RRGGBB` or `#RRGGBBAA` format.
pub fn parse_color(hex: &str) -> Option<slint::Color> {
    let s = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(slint::Color::from_argb_u8(255, r, g, b))
    } else if s.len() == 8 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        let a = u8::from_str_radix(&s[6..8], 16).ok()?;
        Some(slint::Color::from_argb_u8(a, r, g, b))
    } else {
        None
    }
}

/// Helper to parse color and override alpha channel (0..255).
fn parse_color_with_alpha(hex: &str, alpha: u8) -> Option<slint::Color> {
    let s = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(slint::Color::from_argb_u8(alpha, r, g, b))
    } else {
        None
    }
}

fn c(hex: &str) -> slint::Color {
    parse_color(hex).unwrap_or_else(|| slint::Color::from_argb_u8(255, 0, 0, 0))
}

fn ca(hex: &str, a: u8) -> slint::Color {
    parse_color_with_alpha(hex, a).unwrap_or_else(|| slint::Color::from_argb_u8(a, 0, 0, 0))
}

impl Palette {
    /// Builds a base palette for the given preset name.
    pub fn from_preset(name: &str) -> Self {
        match name.to_lowercase().replace('-', "_").as_str() {
            "catppuccin_mocha" | "mocha" => Self {
                font_family: "JetBrainsMono Nerd Font".into(),
                bg_darkest: c("#11111b"),
                bg_sidebar: c("#181825"),
                bg_main: c("#1e1e2e"),
                bg_card: c("#252538"),
                bg_tile: c("#313244"),
                bg_card_hover: c("#363a4f"),
                bg_card_active: c("#45475a"),
                bg_input: c("#181825"),
                bg_pill: c("#313244"),
                accent: c("#cba6f7"), // Mauve
                accent_hover: c("#f5e0dc"),
                accent_subtle: ca("#cba6f7", 0x22),
                accent_orange: c("#fab387"),
                accent_purple: c("#b4befe"),
                accent_aqua: c("#94e2d5"),
                accent_blue: c("#89b4fa"),
                icon_bg_amber: ca("#f9e2af", 0x26),
                icon_bg_green: ca("#a6e3a1", 0x26),
                icon_bg_purple: ca("#cba6f7", 0x26),
                icon_bg_aqua: ca("#94e2d5", 0x26),
                icon_bg_orange: ca("#fab387", 0x26),
                icon_bg_neutral: ca("#45475a", 0x40),
                success: c("#a6e3a1"),
                warning: c("#f9e2af"),
                danger: c("#f38ba8"),
                info: c("#89dceb"),
                text_primary: c("#cdd6f4"),
                text_secondary: c("#bac2de"),
                text_muted: c("#6c7086"),
                text_disabled: c("#45475a"),
                border: c("#313244"),
                border_light: c("#45475a"),
            },
            "tokyo_night" => Self {
                font_family: "JetBrainsMono Nerd Font".into(),
                bg_darkest: c("#13141c"),
                bg_sidebar: c("#16161e"),
                bg_main: c("#1a1b26"),
                bg_card: c("#202334"),
                bg_tile: c("#24283b"),
                bg_card_hover: c("#292e42"),
                bg_card_active: c("#343b58"),
                bg_input: c("#16161e"),
                bg_pill: c("#24283b"),
                accent: c("#7aa2f7"), // Tokyo Blue
                accent_hover: c("#c0caf5"),
                accent_subtle: ca("#7aa2f7", 0x22),
                accent_orange: c("#ff9e64"),
                accent_purple: c("#bb9af7"),
                accent_aqua: c("#7dcfff"),
                accent_blue: c("#7aa2f7"),
                icon_bg_amber: ca("#e0af68", 0x26),
                icon_bg_green: ca("#9ece6a", 0x26),
                icon_bg_purple: ca("#bb9af7", 0x26),
                icon_bg_aqua: ca("#7dcfff", 0x26),
                icon_bg_orange: ca("#ff9e64", 0x26),
                icon_bg_neutral: ca("#414868", 0x40),
                success: c("#9ece6a"),
                warning: c("#e0af68"),
                danger: c("#f7768e"),
                info: c("#7dcfff"),
                text_primary: c("#c0caf5"),
                text_secondary: c("#a9b1d6"),
                text_muted: c("#565f89"),
                text_disabled: c("#3b4261"),
                border: c("#292e42"),
                border_light: c("#414868"),
            },
            "nord" => Self {
                font_family: "JetBrainsMono Nerd Font".into(),
                bg_darkest: c("#1e222a"),
                bg_sidebar: c("#242933"),
                bg_main: c("#2e3440"),
                bg_card: c("#353b49"),
                bg_tile: c("#3b4252"),
                bg_card_hover: c("#434c5e"),
                bg_card_active: c("#4c566a"),
                bg_input: c("#242933"),
                bg_pill: c("#3b4252"),
                accent: c("#88c0d0"), // Frost Cyan
                accent_hover: c("#eceff4"),
                accent_subtle: ca("#88c0d0", 0x22),
                accent_orange: c("#d08770"),
                accent_purple: c("#b48ead"),
                accent_aqua: c("#8fbcbb"),
                accent_blue: c("#81a1c1"),
                icon_bg_amber: ca("#ebcb8b", 0x26),
                icon_bg_green: ca("#a3be8c", 0x26),
                icon_bg_purple: ca("#b48ead", 0x26),
                icon_bg_aqua: ca("#8fbcbb", 0x26),
                icon_bg_orange: ca("#d08770", 0x26),
                icon_bg_neutral: ca("#4c566a", 0x40),
                success: c("#a3be8c"),
                warning: c("#ebcb8b"),
                danger: c("#bf616a"),
                info: c("#88c0d0"),
                text_primary: c("#eceff4"),
                text_secondary: c("#d8dee9"),
                text_muted: c("#7b88a1"),
                text_disabled: c("#4c566a"),
                border: c("#3b4252"),
                border_light: c("#4c566a"),
            },
            "dracula" => Self {
                font_family: "JetBrainsMono Nerd Font".into(),
                bg_darkest: c("#191a21"),
                bg_sidebar: c("#21222c"),
                bg_main: c("#282a36"),
                bg_card: c("#343746"),
                bg_tile: c("#44475a"),
                bg_card_hover: c("#4d5166"),
                bg_card_active: c("#6272a4"),
                bg_input: c("#21222c"),
                bg_pill: c("#44475a"),
                accent: c("#bd93f9"), // Purple
                accent_hover: c("#f8f8f2"),
                accent_subtle: ca("#bd93f9", 0x22),
                accent_orange: c("#ffb86c"),
                accent_purple: c("#bd93f9"),
                accent_aqua: c("#8be9fd"),
                accent_blue: c("#6272a4"),
                icon_bg_amber: ca("#f1fa8c", 0x26),
                icon_bg_green: ca("#50fa7b", 0x26),
                icon_bg_purple: ca("#bd93f9", 0x26),
                icon_bg_aqua: ca("#8be9fd", 0x26),
                icon_bg_orange: ca("#ffb86c", 0x26),
                icon_bg_neutral: ca("#6272a4", 0x40),
                success: c("#50fa7b"),
                warning: c("#f1fa8c"),
                danger: c("#ff5555"),
                info: c("#8be9fd"),
                text_primary: c("#f8f8f2"),
                text_secondary: c("#bfbfbf"),
                text_muted: c("#6272a4"),
                text_disabled: c("#44475a"),
                border: c("#44475a"),
                border_light: c("#6272a4"),
            },
            "generic_dark" => Self {
                font_family: "JetBrainsMono Nerd Font".into(),
                bg_darkest: c("#121212"),
                bg_sidebar: c("#18181b"),
                bg_main: c("#1f1f23"),
                bg_card: c("#27272a"),
                bg_tile: c("#323236"),
                bg_card_hover: c("#38383c"),
                bg_card_active: c("#3f3f46"),
                bg_input: c("#18181b"),
                bg_pill: c("#323236"),
                accent: c("#38bdf8"), // Sky Blue
                accent_hover: c("#f4f4f5"),
                accent_subtle: ca("#38bdf8", 0x22),
                accent_orange: c("#fb923c"),
                accent_purple: c("#c084fc"),
                accent_aqua: c("#2dd4bf"),
                accent_blue: c("#60a5fa"),
                icon_bg_amber: ca("#facc15", 0x26),
                icon_bg_green: ca("#4ade80", 0x26),
                icon_bg_purple: ca("#c084fc", 0x26),
                icon_bg_aqua: ca("#2dd4bf", 0x26),
                icon_bg_orange: ca("#fb923c", 0x26),
                icon_bg_neutral: ca("#52525b", 0x40),
                success: c("#4ade80"),
                warning: c("#facc15"),
                danger: c("#f87171"),
                info: c("#38bdf8"),
                text_primary: c("#f4f4f5"),
                text_secondary: c("#d4d4d8"),
                text_muted: c("#71717a"),
                text_disabled: c("#52525b"),
                border: c("#3f3f46"),
                border_light: c("#52525b"),
            },
            // Default: gruvbox_dark_soft
            _ => Self {
                font_family: "JetBrainsMono Nerd Font".into(),
                bg_darkest: c("#161616"),
                bg_sidebar: c("#1d1c1a"),
                bg_main: c("#242424"),
                bg_card: c("#282828"),
                bg_tile: c("#302f2d"),
                bg_card_hover: c("#32302f"),
                bg_card_active: c("#3c3836"),
                bg_input: c("#1c1c1c"),
                bg_pill: c("#3c3836"),
                accent: c("#fabd2f"), // Warm Gold / Amber
                accent_hover: c("#fbf1c7"),
                accent_subtle: ca("#fabd2f", 0x22),
                accent_orange: c("#fe8019"),
                accent_purple: c("#d3869b"),
                accent_aqua: c("#8ec07c"),
                accent_blue: c("#83a598"),
                icon_bg_amber: ca("#fabd2f", 0x26),
                icon_bg_green: ca("#b8bb26", 0x26),
                icon_bg_purple: ca("#d3869b", 0x26),
                icon_bg_aqua: ca("#8ec07c", 0x26),
                icon_bg_orange: ca("#fe8019", 0x26),
                icon_bg_neutral: ca("#504945", 0x40),
                success: c("#b8bb26"),
                warning: c("#fabd2f"),
                danger: c("#fb4934"),
                info: c("#8ec07c"),
                text_primary: c("#ebdbb2"),
                text_secondary: c("#bdae93"),
                text_muted: c("#7c6f64"),
                text_disabled: c("#504945"),
                border: c("#3a3836"),
                border_light: c("#504945"),
            },
        }
    }

    /// Overrides palette fields with configuration overrides.
    pub fn apply_overrides(&mut self, cfg: &AppearanceConfig) {
        if !cfg.font_family.is_empty() {
            self.font_family = cfg.font_family.clone();
        }

        // Color overrides (handling both native and cqicshell aliases)
        if let Some(ref hex) = cfg.colors.bg_darkest.as_ref().or(cfg.colors.applet_bg.as_ref()) {
            if let Some(col) = parse_color(hex) { self.bg_darkest = col; }
        }
        if let Some(ref hex) = cfg.colors.bg_sidebar.as_ref().or(cfg.colors.bar_bg.as_ref()) {
            if let Some(col) = parse_color(hex) { self.bg_sidebar = col; }
        }
        if let Some(ref hex) = cfg.colors.bg_main.as_ref().or(cfg.colors.applet_bg.as_ref()) {
            if let Some(col) = parse_color(hex) { self.bg_main = col; }
        }
        if let Some(ref hex) = cfg.colors.bg_card.as_ref().or(cfg.colors.card_bg.as_ref()) {
            if let Some(col) = parse_color(hex) { self.bg_card = col; }
        }
        if let Some(ref hex) = cfg.colors.bg_tile.as_ref().or(cfg.colors.pill_bg.as_ref()) {
            if let Some(col) = parse_color(hex) { self.bg_tile = col; }
        }
        if let Some(ref hex) = cfg.colors.bg_card_hover {
            if let Some(col) = parse_color(hex) { self.bg_card_hover = col; }
        }
        if let Some(ref hex) = cfg.colors.bg_input {
            if let Some(col) = parse_color(hex) { self.bg_input = col; }
        }
        if let Some(ref hex) = cfg.colors.bg_pill.as_ref().or(cfg.colors.pill_bg.as_ref()) {
            if let Some(col) = parse_color(hex) { self.bg_pill = col; }
        }

        if let Some(ref hex) = cfg.colors.border.as_ref().or(cfg.colors.pill_border_color.as_ref()) {
            if let Some(col) = parse_color(hex) { self.border = col; }
        }
        if let Some(ref hex) = cfg.colors.border_light {
            if let Some(col) = parse_color(hex) { self.border_light = col; }
        }

        if let Some(ref hex) = cfg.colors.text_primary.as_ref().or(cfg.colors.bar_fg.as_ref()) {
            if let Some(col) = parse_color(hex) { self.text_primary = col; }
        }
        if let Some(ref hex) = cfg.colors.text_secondary.as_ref().or(cfg.colors.subtext_color.as_ref()) {
            if let Some(col) = parse_color(hex) { self.text_secondary = col; }
        }
        if let Some(ref hex) = cfg.colors.text_muted {
            if let Some(col) = parse_color(hex) { self.text_muted = col; }
        }
        if let Some(ref hex) = cfg.colors.text_disabled {
            if let Some(col) = parse_color(hex) { self.text_disabled = col; }
        }

        // Accents
        let primary_override = cfg.accents.primary.as_ref()
            .or(cfg.accents.accent.as_ref())
            .or(cfg.accents.launcher.as_ref());
        if let Some(hex) = primary_override {
            if let Some(col) = parse_color(hex) {
                self.accent = col;
                self.icon_bg_amber = ca(hex, 0x26);
                self.accent_subtle = ca(hex, 0x22);
            }
        }

        let purple_override = cfg.accents.purple.as_ref()
            .or(cfg.accents.bluetooth.as_ref())
            .or(cfg.accents.workspaces.as_ref());
        if let Some(hex) = purple_override {
            if let Some(col) = parse_color(hex) {
                self.accent_purple = col;
                self.icon_bg_purple = ca(hex, 0x26);
            }
        }

        let battery_override = cfg.accents.battery.as_ref();
        if let Some(hex) = battery_override {
            if let Some(col) = parse_color(hex) {
                self.success = col;
                self.icon_bg_green = ca(hex, 0x26);
            }
        }

        let audio_override = cfg.accents.audio.as_ref();
        if let Some(hex) = audio_override {
            if let Some(col) = parse_color(hex) {
                self.accent_blue = col;
            }
        }

        let media_override = cfg.accents.media.as_ref().or(cfg.accents.orange.as_ref());
        if let Some(hex) = media_override {
            if let Some(col) = parse_color(hex) {
                self.accent_orange = col;
                self.icon_bg_orange = ca(hex, 0x26);
            }
        }

        if let Some(ref hex) = cfg.accents.aqua {
            if let Some(col) = parse_color(hex) {
                self.accent_aqua = col;
                self.icon_bg_aqua = ca(hex, 0x26);
            }
        }
        if let Some(ref hex) = cfg.accents.blue {
            if let Some(col) = parse_color(hex) { self.accent_blue = col; }
        }
        if let Some(ref hex) = cfg.accents.success {
            if let Some(col) = parse_color(hex) { self.success = col; }
        }
        if let Some(ref hex) = cfg.accents.warning {
            if let Some(col) = parse_color(hex) { self.warning = col; }
        }
        if let Some(ref hex) = cfg.accents.danger {
            if let Some(col) = parse_color(hex) { self.danger = col; }
        }
        if let Some(ref hex) = cfg.accents.info {
            if let Some(col) = parse_color(hex) { self.info = col; }
        }
    }
}

/// Applies the given configuration palette to the Slint Theme global.
pub fn apply_theme_to_ui(window: &MainWindow, config: &StudioConfig) {
    let mut palette = Palette::from_preset(&config.appearance.theme_preset);
    palette.apply_overrides(&config.appearance);

    let theme = window.global::<Theme>();

    theme.set_font_family(palette.font_family.into());

    theme.set_bg_darkest(palette.bg_darkest);
    theme.set_bg_sidebar(palette.bg_sidebar);
    theme.set_bg_main(palette.bg_main);
    theme.set_bg_card(palette.bg_card);
    theme.set_bg_tile(palette.bg_tile);
    theme.set_bg_card_hover(palette.bg_card_hover);
    theme.set_bg_card_active(palette.bg_card_active);
    theme.set_bg_input(palette.bg_input);
    theme.set_bg_pill(palette.bg_pill);

    theme.set_accent(palette.accent);
    theme.set_accent_hover(palette.accent_hover);
    theme.set_accent_subtle(palette.accent_subtle);
    theme.set_accent_orange(palette.accent_orange);
    theme.set_accent_purple(palette.accent_purple);
    theme.set_accent_aqua(palette.accent_aqua);
    theme.set_accent_blue(palette.accent_blue);

    theme.set_icon_bg_amber(palette.icon_bg_amber);
    theme.set_icon_bg_green(palette.icon_bg_green);
    theme.set_icon_bg_purple(palette.icon_bg_purple);
    theme.set_icon_bg_aqua(palette.icon_bg_aqua);
    theme.set_icon_bg_orange(palette.icon_bg_orange);
    theme.set_icon_bg_neutral(palette.icon_bg_neutral);

    theme.set_success(palette.success);
    theme.set_warning(palette.warning);
    theme.set_danger(palette.danger);
    theme.set_info(palette.info);

    theme.set_text_primary(palette.text_primary);
    theme.set_text_secondary(palette.text_secondary);
    theme.set_text_muted(palette.text_muted);
    theme.set_text_disabled(palette.text_disabled);

    theme.set_border(palette.border);
    theme.set_border_light(palette.border_light);
}

/// Resolves the absolute path to `~/.config/resonate-studio/config.toml`.
pub fn config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("resonate-studio").join("config.toml");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".config")
        .join("resonate-studio")
        .join("config.toml")
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

/// Ensures that `~/.config/resonate-studio/config.toml` exists, writing defaults if not.
/// Returns the parsed configuration.
pub fn ensure_config_file() -> StudioConfig {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if !path.exists() {
        if let Err(e) = fs::write(&path, DEFAULT_CONFIG_TOML) {
            eprintln!("Warning: Failed to create default config at {}: {}", path.display(), e);
        } else {
            println!("Created default Resonate Studio config at {}", path.display());
        }
    }

    load_config(&path).unwrap_or_default()
}

/// Loads and parses the configuration file at `path`.
pub fn load_config(path: &Path) -> Result<StudioConfig, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let config: StudioConfig = toml::from_str(&content)?;
    Ok(config)
}

/// Spawns a background thread that monitors `config.toml` modification time every 600ms.
/// When changes occur, it invokes `apply_theme_to_ui` on the main thread via Slint's event loop.
pub fn start_theme_watcher(weak_ui: slint::Weak<MainWindow>) {
    let path = config_path();

    std::thread::spawn(move || {
        let mut last_modified = fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);

        loop {
            std::thread::sleep(Duration::from_millis(600));

            if let Ok(meta) = fs::metadata(&path) {
                if let Ok(mod_time) = meta.modified() {
                    if mod_time > last_modified {
                        last_modified = mod_time;
                        if let Ok(new_cfg) = load_config(&path) {
                            let weak_clone = weak_ui.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = weak_clone.upgrade() {
                                    apply_theme_to_ui(&ui, &new_cfg);
                                }
                            });
                        }
                    }
                }
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
        let generic = Palette::from_preset("generic_dark");

        assert_ne!(gruvbox.bg_main, mocha.bg_main);
        assert_ne!(mocha.accent, tokyo.accent);
        assert_ne!(nord.bg_sidebar, dracula.bg_sidebar);
        assert_ne!(dracula.accent, generic.accent);
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
        assert_eq!(palette.accent_purple, parse_color("#55ffff").unwrap());
    }
}
