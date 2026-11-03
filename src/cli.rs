use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "resonate",
    about = "Native Linux Bluetooth headphone control for AirPods, Sony MDR, and Bowers & Wilkins",
    version
)]
pub struct Cli {
    #[arg(long, global = true, help = "Format output as JSON")]
    pub json: bool,

    #[arg(short, long, global = true, help = "Print verbose logs")]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    #[command(about = "Show status of connected headphone (default)")]
    Status,

    #[command(about = "Stream real-time status as continuous JSON for Waybar, Polybar, or scripts")]
    Watch {
        #[arg(short, long, default_value = "waybar", help = "Output format: waybar, raw, compact")]
        format: String,
    },

    #[command(about = "List paired AirPods, Beats, Bowers & Wilkins, and Sony devices")]
    Devices,

    #[command(about = "Connect to headphone by name or Bluetooth MAC address")]
    Connect { target: Option<String> },

    #[command(about = "Disconnect active headphone session")]
    Disconnect,

    #[command(about = "Get or set Active Noise Cancellation / Listening mode (off, anc, ambient, adaptive, wind)")]
    Anc { mode: Option<String> },

    #[command(about = "Get or set Sony ambient sound level (0-20)")]
    AmbientLevel { level: Option<i32> },

    #[command(about = "Get or set equalizer presets (Sony) or tone controls (B&W)")]
    Eq { args: Vec<String> },

    #[command(about = "Get or set in-ear / wear detection (Apple, B&W)")]
    WearSensor { action: Option<String> },

    #[command(about = "Get or set smart auto-pause on ear removal (on, off)")]
    AutoPause { action: Option<String> },

    #[command(about = "Get or set Conversation Awareness (AirPods Pro 2)")]
    ConvAware { action: Option<String> },

    #[command(about = "Get or set microphone earbud selection (Apple: auto, left, right)")]
    Mic { mode: Option<String> },

    #[command(about = "Get or set single-bud Active Noise Cancellation (Apple)")]
    OneBudAnc { action: Option<String> },

    #[command(about = "Get or set case/bud tone volume (Apple: 0-100)")]
    Chime { volume: Option<u8> },

    #[command(about = "Get or set standby / auto-power off timer (B&W, Sony)")]
    Standby { val: Option<String> },

    #[command(about = "Remap action button function (B&W: anc, voice)")]
    Button { action: Option<String> },

    #[command(about = "Enable or disable touch sensor control panel (Sony: on, off)")]
    Touch { action: Option<String> },

    #[command(about = "Enable or disable DSEE audio enhancement (Sony: on, off)")]
    Dsee { action: Option<String> },

    #[command(about = "Run NC and atmospheric pressure calibration (Sony: start, stop, status)")]
    Optimize { action: Option<String> },

    #[command(about = "Show battery percentage and charging status")]
    Battery,

    #[command(about = "Run or control the background daemon")]
    Daemon { action: Option<String> },

    #[command(about = "Generate shell autocompletions (bash, zsh, fish)")]
    Completion { shell: String },
}
