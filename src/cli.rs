//! Command-line argument definitions and subcommand parser.
//!
//! Defines the [`Cli`] top-level parser and the [`Commands`] enumeration covering
//! daemon lifecycle management, connection handling, device introspection, telemetry streaming,
//! and vendor-specific audio/ANC controls.

use clap::{Parser, Subcommand};

/// Command-line interface definition parsed by `clap`.
#[derive(Parser, Debug)]
#[command(
    name = "resonate",
    about = "Native Linux Bluetooth headphone control for AirPods, Sony MDR, and Bowers & Wilkins",
    version
)]
pub struct Cli {
    /// Format output as JSON instead of human-readable text.
    #[arg(long, global = true, help = "Format output as JSON")]
    pub json: bool,

    /// Print verbose debugging and protocol logs.
    #[arg(short, long, global = true, help = "Print verbose logs")]
    pub verbose: bool,

    /// Subcommand to execute (defaults to `Status` if omitted).
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available CLI subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Show status of connected headphone (default command).
    #[command(about = "Show status of connected headphone (default)")]
    Status,

    /// Stream real-time status as continuous JSON for Waybar, Polybar, or scripts.
    #[command(about = "Stream real-time status as continuous JSON for Waybar, Polybar, or scripts")]
    Watch {
        /// Output format (`"waybar"`, `"raw"`, `"compact"`).
        #[arg(
            short,
            long,
            default_value = "waybar",
            help = "Output format: waybar, raw, compact"
        )]
        format: String,
    },

    /// List paired AirPods, Beats, Bowers & Wilkins, and Sony devices.
    #[command(about = "List paired AirPods, Beats, Bowers & Wilkins, and Sony devices")]
    Devices,

    /// Connect to headphone by name or Bluetooth MAC address.
    #[command(about = "Connect to headphone by name or Bluetooth MAC address")]
    Connect {
        /// Device name substring or MAC address.
        target: Option<String>,
    },

    /// Disconnect active headphone session.
    #[command(about = "Disconnect active headphone session")]
    Disconnect,

    /// Get or set Active Noise Cancellation / Listening mode (off, anc, ambient, adaptive, wind).
    #[command(
        about = "Get or set Active Noise Cancellation / Listening mode (off, anc, ambient, adaptive, wind)"
    )]
    Anc {
        /// Desired listening mode.
        mode: Option<String>,
    },

    /// Get or set Sony ambient sound level (0-20).
    #[command(about = "Get or set Sony ambient sound level (0-20)")]
    AmbientLevel {
        /// Ambient transparency level from 0 to 20.
        level: Option<i32>,
    },

    /// Get or set equalizer presets (Sony) or tone controls (B&W).
    #[command(about = "Get or set equalizer presets (Sony) or tone controls (B&W)")]
    Eq {
        /// Equalizer arguments (preset name or bass/treble values).
        args: Vec<String>,
    },

    /// Get or set in-ear / wear detection (Apple, B&W).
    #[command(about = "Get or set in-ear / wear detection (Apple, B&W)")]
    WearSensor {
        /// Action (`"on"`, `"off"`, `"enable"`, `"disable"`).
        action: Option<String>,
    },

    /// Get or set smart auto-pause on ear removal (on, off).
    #[command(about = "Get or set smart auto-pause on ear removal (on, off)")]
    AutoPause {
        /// Action (`"on"`, `"off"`, `"enable"`, `"disable"`).
        action: Option<String>,
    },

    /// Get or set Conversation Awareness (AirPods Pro 2).
    #[command(about = "Get or set Conversation Awareness (AirPods Pro 2)")]
    ConvAware {
        /// Action (`"on"`, `"off"`, `"enable"`, `"disable"`).
        action: Option<String>,
    },

    /// Get or set microphone earbud selection (Apple: auto, left, right).
    #[command(about = "Get or set microphone earbud selection (Apple: auto, left, right)")]
    Mic {
        /// Microphone routing mode (`"auto"`, `"left"`, `"right"`).
        mode: Option<String>,
    },

    /// Get or set single-bud Active Noise Cancellation (Apple).
    #[command(about = "Get or set single-bud Active Noise Cancellation (Apple)")]
    OneBudAnc {
        /// Action (`"on"`, `"off"`, `"enable"`, `"disable"`).
        action: Option<String>,
    },

    /// Get or set case/bud tone volume (Apple: 0-100).
    #[command(about = "Get or set case/bud tone volume (Apple: 0-100)")]
    Chime {
        /// Tone volume percentage (0-100).
        volume: Option<u8>,
    },

    /// Get or set standby / auto-power off timer (B&W, Sony).
    #[command(about = "Get or set standby / auto-power off timer (B&W, Sony)")]
    Standby {
        /// Standby timeout value in minutes or preset name.
        val: Option<String>,
    },

    /// Remap action button function (B&W: anc, voice).
    #[command(about = "Remap action button function (B&W: anc, voice)")]
    Button {
        /// Action button mapping (`"anc"`, `"voice"`).
        action: Option<String>,
    },

    /// Enable or disable touch sensor control panel (Sony: on, off).
    #[command(about = "Enable or disable touch sensor control panel (Sony: on, off)")]
    Touch {
        /// Action (`"on"`, `"off"`).
        action: Option<String>,
    },

    /// Enable or disable DSEE audio enhancement (Sony: on, off).
    #[command(about = "Enable or disable DSEE audio enhancement (Sony: on, off)")]
    Dsee {
        /// Action (`"on"`, `"off"`).
        action: Option<String>,
    },

    /// Run NC and atmospheric pressure calibration (Sony: start, stop, status).
    #[command(about = "Run NC and atmospheric pressure calibration (Sony: start, stop, status)")]
    Optimize {
        /// Optimizer command (`"start"`, `"stop"`, `"status"`).
        action: Option<String>,
    },

    /// Show battery percentage and charging status.
    #[command(about = "Show battery percentage and charging status")]
    Battery,

    /// Run or control the background daemon.
    #[command(about = "Run or control the background daemon")]
    Daemon {
        /// Action (`"install"`, `"start"`, `"stop"`, `"restart"`, `"status"`, `"uninstall"`, or omit to run in foreground).
        action: Option<String>,
    },

    /// Generate shell autocompletions (bash, zsh, fish).
    #[command(about = "Generate shell autocompletions (bash, zsh, fish)")]
    Completion {
        /// Target shell name (`"bash"`, `"zsh"`, `"fish"`).
        shell: String,
    },
}
