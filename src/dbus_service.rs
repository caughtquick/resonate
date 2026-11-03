//! D-Bus service exposing the `org.headbridge.Manager` interface.
//!
//! Provides the primary IPC boundary between the background daemon (`resonate daemon`)
//! and CLI invocations (`resonate <command>`), status bars, and desktop panels.
//!
//! ## Interface Definition
//!
//! - **Bus Names**: `org.headbridge`, `org.resonate`
//! - **Object Path**: `/org/headbridge`
//! - **Interface**: `org.headbridge.Manager`
//! - **Signals**:
//!   - `StateChanged(s)`: Emitted with serialized JSON whenever headphone telemetry or settings change.
//! - **Properties**:
//!   - `Connected` (b), `ConnectionState` (s), `DeviceName` (s), `Vendor` (s),
//!     `BatteryPercent` (i), `BatteryLeft` (i), `BatteryRight` (i), `BatteryCase` (i),
//!     `IsCharging` (b), `NoiseMode` (s), `EarDetection` (b), `AutoPause` (b),
//!     `InEar` (b), `InEarPrimary` (s), `InEarSecondary` (s), `ConversationAwareness` (b),
//!     `MicrophoneMode` (s), `OneBudAnc` (b), `ChimeVolume` (i), `StandbyMinutes` (i),
//!     `TouchSensor` (b), `Dsee` (b), `EqualizerPreset` (s), `AudioSink` (s).

use std::sync::Arc;
use zbus::interface;

use crate::manager::DeviceManager;

/// In-memory snapshot of the active headphone telemetry and feature configurations.
#[derive(Debug, Clone, Default)]
pub struct ResonateState {
    /// True if a supported headphone session is currently connected and communicating.
    pub connected: bool,
    /// Detailed connection status description (e.g. `"connected"`, `"connecting"`, `"disconnected"`).
    pub connection_state: String,
    /// Friendly product or alias name of the active device.
    pub device_name: String,
    /// Hardware manufacturer name (`"Apple"`, `"Sony"`, `"Bowers & Wilkins"`, or `"Unknown"`).
    pub vendor: String,
    /// Overall or single battery percentage (-1 if unavailable).
    pub battery_percent: i32,
    /// Left earbud battery percentage (-1 if unavailable).
    pub battery_left: i32,
    /// Right earbud battery percentage (-1 if unavailable).
    pub battery_right: i32,
    /// Charging case battery percentage (-1 if unavailable).
    pub battery_case: i32,
    /// Whether any connected component is currently charging.
    pub is_charging: bool,
    /// Current noise control mode (`"off"`, `"noise_cancellation"`, `"transparency"`, `"adaptive"`, `"wind"`).
    pub noise_mode: String,
    /// Whether hardware ear detection is enabled in settings.
    pub ear_detection: bool,
    /// Whether smart auto-pause / resume is enabled in Resonate.
    pub auto_pause: bool,
    /// True if at least one bud or over-ear sensor detects the headphone is worn.
    pub in_ear: bool,
    /// Primary bud wear placement (`"in_ear"`, `"out_of_ear"`, `"in_case"`, `"unknown"`).
    pub in_ear_primary: String,
    /// Secondary bud wear placement (`"in_ear"`, `"out_of_ear"`, `"in_case"`, `"unknown"`).
    pub in_ear_secondary: String,
    /// Whether Conversation Awareness is active (AirPods Pro 2).
    pub conv_aware: bool,
    /// Active microphone routing (`"auto"`, `"left"`, `"right"`).
    pub mic_mode: String,
    /// Whether single-bud ANC mode is enabled.
    pub one_bud_anc: bool,
    /// Chime tone volume on buds/case (0-100).
    pub chime_volume: i32,
    /// Auto-power off timeout in minutes (0 = disabled).
    pub standby_minutes: i32,
    /// Whether touch sensor controls are active (Sony).
    pub touch_sensor: bool,
    /// Whether DSEE audio upscaling is active (Sony).
    pub dsee: bool,
    /// Current equalizer preset name or settings string.
    pub equalizer_preset: String,
    /// Current Linux default audio sink name (e.g. `"bluez_output.6C_12_70_3F_8E_AE.a2dp-sink"`).
    pub audio_sink: String,
}

/// D-Bus interface implementation for `org.headbridge.Manager`.
pub struct ResonateDbusManager {
    manager: Arc<DeviceManager>,
}

impl ResonateDbusManager {
    /// Creates a new `ResonateDbusManager` wrapping the shared [`DeviceManager`].
    pub fn new(manager: Arc<DeviceManager>) -> Self {
        Self { manager }
    }
}

#[interface(name = "org.headbridge.Manager")]
impl ResonateDbusManager {
    /// True if a supported headphone session is active.
    #[zbus(property)]
    async fn connected(&self) -> bool {
        self.manager.state.read().await.connected
    }

    /// Detailed connection status description string.
    #[zbus(property)]
    async fn connection_state(&self) -> String {
        self.manager.state.read().await.connection_state.clone()
    }

    /// Device friendly name.
    #[zbus(property)]
    async fn device_name(&self) -> String {
        self.manager.state.read().await.device_name.clone()
    }

    /// Identified manufacturer vendor name.
    #[zbus(property)]
    async fn vendor(&self) -> String {
        self.manager.state.read().await.vendor.clone()
    }

    /// Combined or single battery percentage (-1 if unknown).
    #[zbus(property)]
    async fn battery_percent(&self) -> i32 {
        self.manager.state.read().await.battery_percent
    }

    /// Left earbud battery percentage.
    #[zbus(property)]
    async fn battery_left(&self) -> i32 {
        self.manager.state.read().await.battery_left
    }

    /// Right earbud battery percentage.
    #[zbus(property)]
    async fn battery_right(&self) -> i32 {
        self.manager.state.read().await.battery_right
    }

    /// Charging case battery percentage.
    #[zbus(property)]
    async fn battery_case(&self) -> i32 {
        self.manager.state.read().await.battery_case
    }

    /// True if battery is actively charging.
    #[zbus(property)]
    async fn is_charging(&self) -> bool {
        self.manager.state.read().await.is_charging
    }

    /// Active noise control mode.
    #[zbus(property)]
    async fn noise_mode(&self) -> String {
        self.manager.state.read().await.noise_mode.clone()
    }

    /// Hardware in-ear detection setting.
    #[zbus(property)]
    async fn ear_detection(&self) -> bool {
        self.manager.state.read().await.ear_detection
    }

    /// Smart auto-pause software setting.
    #[zbus(property)]
    async fn auto_pause(&self) -> bool {
        self.manager.state.read().await.auto_pause
    }

    /// Whether headphone is currently placed in ears.
    #[zbus(property)]
    async fn in_ear(&self) -> bool {
        self.manager.state.read().await.in_ear
    }

    /// Primary earbud placement status (`"in_ear"`, `"out_of_ear"`, `"in_case"`, `"unknown"`).
    #[zbus(property)]
    async fn in_ear_primary(&self) -> String {
        self.manager.state.read().await.in_ear_primary.clone()
    }

    /// Secondary earbud placement status (`"in_ear"`, `"out_of_ear"`, `"in_case"`, `"unknown"`).
    #[zbus(property)]
    async fn in_ear_secondary(&self) -> String {
        self.manager.state.read().await.in_ear_secondary.clone()
    }

    /// Conversation Awareness toggle.
    #[zbus(property)]
    async fn conversation_awareness(&self) -> bool {
        self.manager.state.read().await.conv_aware
    }

    /// Active microphone routing selection.
    #[zbus(property)]
    async fn microphone_mode(&self) -> String {
        self.manager.state.read().await.mic_mode.clone()
    }

    /// One bud ANC permission setting.
    #[zbus(property)]
    async fn one_bud_anc(&self) -> bool {
        self.manager.state.read().await.one_bud_anc
    }

    /// Case and bud chime tone volume.
    #[zbus(property)]
    async fn chime_volume(&self) -> i32 {
        self.manager.state.read().await.chime_volume
    }

    /// Auto-standby timeout in minutes.
    #[zbus(property)]
    async fn standby_minutes(&self) -> i32 {
        self.manager.state.read().await.standby_minutes
    }

    /// Touch sensor panel toggle (Sony).
    #[zbus(property)]
    async fn touch_sensor(&self) -> bool {
        self.manager.state.read().await.touch_sensor
    }

    /// DSEE audio enhancement toggle (Sony).
    #[zbus(property)]
    async fn dsee(&self) -> bool {
        self.manager.state.read().await.dsee
    }

    /// Current equalizer preset name (Sony).
    #[zbus(property)]
    async fn equalizer_preset(&self) -> String {
        self.manager.state.read().await.equalizer_preset.clone()
    }

    /// PulseAudio / PipeWire default sink name.
    #[zbus(property)]
    async fn audio_sink(&self) -> String {
        self.manager.state.read().await.audio_sink.clone()
    }

    /// Returns the full runtime status serialized as a JSON string.
    async fn get_status(&self) -> String {
        self.manager.get_status_json().await
    }

    /// Returns the list of all paired devices from BlueZ as a JSON array string.
    async fn list_devices(&self) -> String {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.list_devices_json().await })
            .await
            .unwrap_or_else(|_| "[]".to_string())
    }

    /// Connects to a headphone by name or Bluetooth MAC address.
    async fn connect(&self, target: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.connect_device(&target).await })
            .await
            .unwrap_or_else(|e| (false, format!("Tokio runtime spawn error: {}", e)))
    }

    /// Disconnects the currently active headphone session.
    async fn disconnect(&self) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.disconnect_device().await })
            .await
            .unwrap_or(false)
    }

    /// Sets the active noise control mode (`"off"`, `"anc"`, `"ambient"`, `"adaptive"`, `"wind"`).
    async fn set_anc(&self, mode: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_anc(&mode).await })
            .await
            .unwrap_or_else(|e| (false, format!("Tokio runtime spawn error: {}", e)))
    }

    /// Sets the ambient sound level on Sony headphones (0-20).
    async fn set_ambient_level(&self, level: i32) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_ambient_level(level).await })
            .await
            .unwrap_or(false)
    }

    /// Sets equalizer preset or custom gains.
    async fn set_equalizer(&self, args: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_equalizer(&args).await })
            .await
            .unwrap_or_else(|e| (false, format!("Error: {}", e)))
    }

    /// Enables or disables in-ear / wear detection.
    async fn set_wear_sensor(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_ear_detection(enable).await })
            .await
            .unwrap_or(false)
    }

    /// Enables or disables smart auto-pause.
    async fn set_auto_pause(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_auto_pause(enable).await })
            .await
            .unwrap_or(false)
    }

    /// Queries the current smart auto-pause toggle state.
    async fn get_auto_pause(&self) -> bool {
        self.manager.state.read().await.auto_pause
    }

    /// Enables or disables Conversation Awareness (AirPods Pro 2).
    async fn set_conversation_awareness(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_conversation_awareness(enable).await })
            .await
            .unwrap_or(false)
    }

    /// Configures microphone routing (`"auto"`, `"left"`, `"right"`).
    async fn set_microphone_mode(&self, mode: String) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_mic_mode(&mode).await })
            .await
            .unwrap_or(false)
    }

    /// Enables or disables single bud Active Noise Cancellation.
    async fn set_one_bud_anc(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_one_bud_anc(enable).await })
            .await
            .unwrap_or(false)
    }

    /// Sets chime tone alert volume (0-100).
    async fn set_chime_volume(&self, vol: u8) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_chime_volume(vol).await })
            .await
            .unwrap_or(false)
    }

    /// Enables or disables the touch sensor control panel (Sony).
    async fn set_touch_sensor(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_touch_sensor(enable).await })
            .await
            .unwrap_or(false)
    }

    /// Enables or disables DSEE audio upscaling (Sony).
    async fn set_dsee(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_dsee(enable).await })
            .await
            .unwrap_or(false)
    }

    /// Configures auto-standby timer (Sony, B&W).
    async fn set_standby(&self, val: String) -> bool {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_standby(&val).await })
            .await
            .unwrap_or(false)
    }

    /// Remaps the hardware custom action button (B&W).
    async fn set_action_button(&self, action: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.set_action_button(&action).await })
            .await
            .unwrap_or_else(|e| (false, format!("Error: {}", e)))
    }

    /// Starts or checks NC and atmospheric pressure calibration (Sony).
    async fn optimize(&self, action: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager
            .handle
            .spawn(async move { mgr.optimize(&action).await })
            .await
            .unwrap_or_else(|e| (false, format!("Error: {}", e)))
    }
}
