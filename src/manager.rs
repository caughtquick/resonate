//! Central device management, background monitoring loop, and vendor session lifecycle.
//!
//! Orchestrates Bluetooth connections, vendor-specific control channels (Apple L2CAP,
//! B&W GATT), smart auto-pause transitions, persistent settings updates,
//! and periodic telemetry health checks.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, mpsc};
use zbus::Connection;

use crate::audio::{AudioMonitor, AutoPauseCoordinator};
use crate::dbus_service::ResonateState;
use crate::inventory::{BluetoothInventory, DeviceVendor, PairedDevice};
use crate::notifications::NotificationService;
use crate::protocols::aap::{
    self, BatteryKind, BatteryStatus, Frame, ListeningMode, MicSelection, Opcode, SettingId,
    SettingNotice,
};
use crate::protocols::bw::{
    BWRPC, BWRPCatalog, BWRPCMessageKind, BwAncWireProfile, BwPairedDevice, decode_paired_device,
    format_bw_5band, format_bw_tone, parse_bw_level, parse_bw_paired_count,
};
use crate::settings::SettingsStore;
use crate::transport::bluez::BluezGattClient;
use crate::transport::l2cap::AppleL2capSession;

/// Formats the current [`ResonateState`] as pretty-printed JSON.
pub fn format_state_json(s: &ResonateState) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "connected": s.connected,
        "connectionState": s.connection_state,
        "deviceName": s.device_name,
        "vendor": s.vendor,
        "batteryPercent": if s.battery_percent >= 0 { serde_json::Value::from(s.battery_percent) } else { serde_json::Value::Null },
        "batteryLeft": if s.battery_left >= 0 { serde_json::Value::from(s.battery_left) } else { serde_json::Value::Null },
        "batteryRight": if s.battery_right >= 0 { serde_json::Value::from(s.battery_right) } else { serde_json::Value::Null },
        "batteryCase": if s.battery_case >= 0 { serde_json::Value::from(s.battery_case) } else { serde_json::Value::Null },
        "isCharging": s.is_charging,
        "noiseMode": s.noise_mode,
        "audioSink": s.audio_sink,
        "earDetection": s.ear_detection,
        "autoPause": s.auto_pause,
        "inEar": s.in_ear,
        "inEarPrimary": s.in_ear_primary,
        "inEarSecondary": s.in_ear_secondary,
        "wearSensitivity": if s.wear_sensitivity >= 0 { serde_json::Value::from(s.wear_sensitivity) } else { serde_json::Value::Null },
        "conversationAwareness": s.conv_aware,
        "micMode": s.mic_mode,
        "oneBudAnc": s.one_bud_anc,
        "chimeVolume": s.chime_volume,
        "standbyMinutes": s.standby_minutes,
        "actionButton": if s.action_button.is_empty() { "anc" } else { &s.action_button },
        "equalizerPreset": s.equalizer_preset,
    })).unwrap_or_else(|_| "{}".to_string())
}

/// Central controller managing runtime state, vendor control sessions, and event dispatch.
pub struct DeviceManager {
    /// Shared runtime state containing current battery, listening mode, and wear status.
    pub state: Arc<RwLock<ResonateState>>,
    active_apple_session: Arc<Mutex<Option<AppleL2capSession>>>,
    active_bw_client: Arc<Mutex<Option<BluezGattClient>>>,
    bw_anc_profile: Arc<RwLock<BwAncWireProfile>>,
    bw_bass: Arc<RwLock<Option<i32>>>,
    bw_treble: Arc<RwLock<Option<i32>>>,
    bw_eq_5band: Arc<RwLock<Option<Vec<i32>>>>,
    bw_supports_5band: Arc<RwLock<bool>>,
    bw_supports_tone: Arc<RwLock<bool>>,
    bw_paired_devices: Arc<RwLock<Vec<BwPairedDevice>>>,
    bw_wear_sensitivity: Arc<RwLock<Option<u8>>>,
    last_reconnect_attempt: Arc<Mutex<Option<(String, std::time::Instant)>>>,
    active_address: Arc<RwLock<Option<String>>>,
    /// Tokio runtime handle for spawning background tasks from synchronous contexts.
    pub handle: tokio::runtime::Handle,
    /// Persistent user settings store.
    pub settings: Arc<Mutex<SettingsStore>>,
    /// Desktop notification service.
    pub notifications: Arc<Mutex<NotificationService>>,
    dbus_conn: Arc<Mutex<Option<Connection>>>,
    /// Media player auto-pause and resume coordinator.
    pub auto_pause: Arc<Mutex<AutoPauseCoordinator>>,
}

impl DeviceManager {
    /// Creates a new `DeviceManager` with initial settings and empty state.
    pub fn new(state: Arc<RwLock<ResonateState>>, handle: tokio::runtime::Handle) -> Self {
        let store = SettingsStore::load();
        let auto_pause_enabled = store.data.auto_pause_enabled;

        Self {
            state,
            active_apple_session: Arc::new(Mutex::new(None)),
            active_bw_client: Arc::new(Mutex::new(None)),
            bw_anc_profile: Arc::new(RwLock::new(BwAncWireProfile::Standard)),
            bw_bass: Arc::new(RwLock::new(None)),
            bw_treble: Arc::new(RwLock::new(None)),
            bw_eq_5band: Arc::new(RwLock::new(None)),
            bw_supports_5band: Arc::new(RwLock::new(false)),
            bw_supports_tone: Arc::new(RwLock::new(false)),
            bw_paired_devices: Arc::new(RwLock::new(Vec::new())),
            bw_wear_sensitivity: Arc::new(RwLock::new(None)),
            last_reconnect_attempt: Arc::new(Mutex::new(None)),
            active_address: Arc::new(RwLock::new(None)),
            handle,
            settings: Arc::new(Mutex::new(store)),
            notifications: Arc::new(Mutex::new(NotificationService::new())),
            dbus_conn: Arc::new(Mutex::new(None)),
            auto_pause: Arc::new(Mutex::new(AutoPauseCoordinator::new(auto_pause_enabled))),
        }
    }

    /// Enables or disables smart auto-pause and auto-resume behavior via MPRIS when earbuds are removed or inserted.
    pub async fn set_auto_pause(&self, enabled: bool) -> bool {
        self.auto_pause.lock().await.set_enabled(enabled);
        self.settings.lock().await.set_auto_pause(enabled);
        self.state.write().await.auto_pause = enabled;
        self.notify_state_changed().await;
        true
    }

    /// Stores the active session D-Bus connection for broadcasting `StateChanged` signals.
    pub async fn set_dbus_connection(&self, conn: Connection) {
        *self.dbus_conn.lock().await = Some(conn);
    }

    /// Emits the `StateChanged(s)` D-Bus signal containing complete JSON status telemetry.
    pub async fn notify_state_changed(&self) {
        let json = self.get_status_json().await;
        if let Some(conn) = self.dbus_conn.lock().await.as_ref() {
            let _ = conn
                .emit_signal(
                    None::<()>,
                    "/org/headbridge",
                    "org.headbridge.Manager",
                    "StateChanged",
                    &(json,),
                )
                .await;
        }
    }

    /// Returns the current runtime state serialized as pretty-printed JSON.
    pub async fn get_status_json(&self) -> String {
        let s = self.state.read().await;
        format_state_json(&s)
    }

    /// Fetches all paired Bluetooth audio devices from BlueZ and serializes the list as pretty-printed JSON.
    pub async fn list_devices_json(&self) -> String {
        match BluetoothInventory::fetch_paired_devices().await {
            Ok(devices) => {
                serde_json::to_string_pretty(&devices).unwrap_or_else(|_| "[]".to_string())
            }
            Err(e) => {
                eprintln!("Error fetching paired devices: {:?}", e);
                "[]".to_string()
            }
        }
    }

    /// Queries the multipoint paired device list from the active Bowers & Wilkins headphone over BLE GATT.
    ///
    /// Returns `(true, json)` with the device list on success, or `(false, error_message)` on failure.
    pub async fn get_multipoint(&self) -> (bool, String) {
        let bw_client = self.active_bw_client.lock().await.clone();
        if let Some(client) = bw_client {
            if self.bw_paired_devices.read().await.is_empty() {
                let _ = client
                    .send_command::<()>(BWRPCatalog::PAIRED_DEVICE_COUNT_GET, None)
                    .await;
                for _ in 0..10 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if !self.bw_paired_devices.read().await.is_empty() {
                        break;
                    }
                }
            } else {
                let _ = client
                    .send_command::<()>(BWRPCatalog::PAIRED_DEVICE_COUNT_GET, None)
                    .await;
            }
            let list = self.bw_paired_devices.read().await.clone();
            match serde_json::to_string(&list) {
                Ok(json) => (true, json),
                Err(e) => (
                    false,
                    format!("Failed to serialize multipoint devices: {}", e),
                ),
            }
        } else {
            (
                false,
                "No active device supporting multipoint is connected".to_string(),
            )
        }
    }

    /// Requests the active Bowers & Wilkins headphone to connect to a paired multipoint device by index, name, or MAC address.
    pub async fn connect_multipoint(&self, target: &str) -> (bool, String) {
        let bw_client = self.active_bw_client.lock().await.clone();
        let Some(client) = bw_client else {
            return (
                false,
                "No Bowers & Wilkins headphone actively connected.".to_string(),
            );
        };

        let target_trim = target.trim();
        if target_trim.is_empty() {
            return (
                false,
                "Target device (index, name, or MAC address) must be specified.".to_string(),
            );
        }

        let devices = self.bw_paired_devices.read().await.clone();
        let matched = devices.iter().find(|d| {
            d.id.to_string() == target_trim
                || d.address.eq_ignore_ascii_case(target_trim)
                || d.name.to_lowercase().contains(&target_trim.to_lowercase())
        });

        let Some(dev) = matched else {
            return (
                false,
                format!(
                    "Device '{}' not found in paired multipoint list. Run 'resonate multipoint' to view devices.",
                    target_trim
                ),
            );
        };

        if let Err(e) = client.connect_paired_device(dev.id as u8).await {
            return (
                false,
                format!("Failed to send connect command for {}: {}", dev.name, e),
            );
        }

        let _ = client
            .send_command::<()>(BWRPCatalog::PAIRED_DEVICE_COUNT_GET, None)
            .await;

        (
            true,
            format!(
                "Requested connection to multipoint device [{}]: {} ({})",
                dev.id, dev.name, dev.address
            ),
        )
    }

    /// Requests the active Bowers & Wilkins headphone to disconnect from a paired multipoint device by index, name, or MAC address.
    pub async fn disconnect_multipoint(&self, target: &str) -> (bool, String) {
        let bw_client = self.active_bw_client.lock().await.clone();
        let Some(client) = bw_client else {
            return (
                false,
                "No Bowers & Wilkins headphone actively connected.".to_string(),
            );
        };

        let target_trim = target.trim();
        if target_trim.is_empty() {
            return (
                false,
                "Target device (index, name, or MAC address) must be specified.".to_string(),
            );
        }

        let devices = self.bw_paired_devices.read().await.clone();
        let matched = devices.iter().find(|d| {
            d.id.to_string() == target_trim
                || d.address.eq_ignore_ascii_case(target_trim)
                || d.name.to_lowercase().contains(&target_trim.to_lowercase())
        });

        let Some(dev) = matched else {
            return (
                false,
                format!(
                    "Device '{}' not found in paired multipoint list. Run 'resonate multipoint' to view devices.",
                    target_trim
                ),
            );
        };

        if let Err(e) = client.disconnect_paired_device(dev.id as u8).await {
            return (
                false,
                format!("Failed to send disconnect command for {}: {}", dev.name, e),
            );
        }

        let _ = client
            .send_command::<()>(BWRPCatalog::PAIRED_DEVICE_COUNT_GET, None)
            .await;

        (
            true,
            format!(
                "Requested disconnection from multipoint device [{}]: {} ({})",
                dev.id, dev.name, dev.address
            ),
        )
    }

    /// Connects to a headphone by name substring or Bluetooth MAC address.
    ///
    /// Ensures connection in BlueZ first if not already connected, then establishes
    /// the vendor-specific control session (L2CAP for Apple, BLE GATT for B&W).
    pub async fn connect_device(&self, target: &str) -> (bool, String) {
        let devices = match BluetoothInventory::fetch_paired_devices().await {
            Ok(d) => d,
            Err(e) => return (false, format!("Failed to read devices: {}", e)),
        };

        let target_upper = target.to_uppercase();
        let target_lower = target.to_lowercase();

        let matched = devices
            .into_iter()
            .find(|d| d.address == target_upper || d.name.to_lowercase().contains(&target_lower));

        let device = match matched {
            Some(d) => d,
            None => {
                return (
                    false,
                    format!("Device '{}' not found in paired devices", target),
                );
            }
        };

        if !device.is_supported() {
            return (
                false,
                format!(
                    "Device '{}' ({}) is not a supported headphone",
                    device.name, device.address
                ),
            );
        }

        if let Some(current) = self.active_address.read().await.as_deref()
            && current == device.address
            && self.state.read().await.connected
        {
            return (
                true,
                format!("Already connected to {} ({})", device.name, device.address),
            );
        }

        // 1. Ensure connected in BlueZ
        if !device.connected {
            if let Err(e) = Self::bluez_connect(&device.address).await {
                return (false, format!("BlueZ connect failed: {}", e));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // 2. Connect vendor session
        self.connect_vendor_session(&device).await
    }

    /// Disconnects the active vendor session, clears cached state, and notifies clients via D-Bus.
    pub async fn disconnect_device(&self) -> bool {
        let old_addr = self.active_address.write().await.take();
        *self.active_apple_session.lock().await = None;
        *self.active_bw_client.lock().await = None;
        *self.bw_bass.write().await = None;
        *self.bw_treble.write().await = None;
        *self.bw_eq_5band.write().await = None;
        *self.bw_supports_5band.write().await = false;
        *self.bw_supports_tone.write().await = false;
        self.bw_paired_devices.write().await.clear();
        *self.bw_wear_sensitivity.write().await = None;
        *self.last_reconnect_attempt.lock().await = None;

        let dbus_conn = self.dbus_conn.lock().await.clone();
        if let Some(conn) = dbus_conn.as_ref() {
            self.auto_pause.lock().await.on_disconnect(conn).await;
        } else {
            self.auto_pause.lock().await.reset();
        }

        let name = {
            let mut s = self.state.write().await;
            let n = s.device_name.clone();
            s.connected = false;
            s.connection_state = "idle".to_string();
            s.device_name = String::new();
            s.vendor = String::new();
            s.battery_percent = -1;
            s.battery_left = -1;
            s.battery_right = -1;
            s.battery_case = -1;
            s.is_charging = false;
            s.noise_mode = "off".to_string();
            s.in_ear = false;
            s.in_ear_primary = String::new();
            s.in_ear_secondary = String::new();
            s.wear_sensitivity = -1;
            n
        };

        if old_addr.is_some() && !name.is_empty() {
            self.notifications
                .lock()
                .await
                .notify_disconnected(&name)
                .await;
        }

        self.notify_state_changed().await;
        true
    }

    /// Sets the Active Noise Cancellation / Listening mode (`"off"`, `"anc"`, `"ambient"` / `"transparency"`, `"adaptive"`, `"wind"`).
    pub async fn set_anc(&self, mode_str: &str) -> (bool, String) {
        let mode_norm = mode_str.to_lowercase();

        // 1. Apple
        let apple_session = self.active_apple_session.lock().await.clone();
        if let Some(session) = apple_session {
            let mode = match mode_norm.as_str() {
                "off" => ListeningMode::Off,
                "anc" | "nc" | "on" => ListeningMode::NoiseCancellation,
                "ambient" | "transparency" => ListeningMode::Transparency,
                "adaptive" => ListeningMode::Adaptive,
                other => {
                    return (
                        false,
                        format!(
                            "Unsupported Apple ANC mode: '{}'. Use off, anc, ambient, or adaptive.",
                            other
                        ),
                    );
                }
            };

            // Check if earbuds are worn in ear
            let is_worn = self.state.read().await.in_ear;
            let ear_detect_enabled = self.state.read().await.ear_detection;
            if mode == ListeningMode::NoiseCancellation && ear_detect_enabled && !is_worn {
                // If buds are out of the ear, record as preferred mode so when they are put on, ANC is enabled automatically.
                if let Some(addr) = self.active_address.read().await.as_deref() {
                    self.settings
                        .lock()
                        .await
                        .set_preferred_noise_mode(addr, &mode_norm);
                }
                return (
                    false,
                    "AirPods are not in your ears. Put them in to use Noise Cancellation."
                        .to_string(),
                );
            }

            if let Err(e) = session.set_noise_mode(mode).await {
                // Mark session as dead if socket broken so monitoring loop reconnects
                *self.active_apple_session.lock().await = None;
                return (false, format!("Failed to send ANC command: {}", e));
            }

            if let Some(addr) = self.active_address.read().await.as_deref() {
                self.settings
                    .lock()
                    .await
                    .set_preferred_noise_mode(addr, &mode_norm);
            }

            let canonical_mode = match mode_norm.as_str() {
                "off" => "off",
                "anc" | "nc" | "on" | "noise_cancellation" => "anc",
                "ambient" | "transparency" => "ambient",
                "adaptive" => "adaptive",
                _ => mode_norm.as_str(),
            };

            // Wait briefly for hardware echo if device reports it back immediately
            let deadline = tokio::time::Instant::now() + Duration::from_millis(250);
            while tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(25)).await;
                if self.state.read().await.noise_mode == canonical_mode {
                    return (true, format!("ANC mode set to {}", canonical_mode));
                }
            }

            // Apply confirmed state and notify D-Bus listeners
            self.state.write().await.noise_mode = canonical_mode.to_string();
            self.notify_state_changed().await;
            return (true, format!("ANC mode set to {}", canonical_mode));
        }

        // 2. Bowers & Wilkins
        let bw_client = self.active_bw_client.lock().await.clone();
        if let Some(client) = bw_client {
            let bw_mode = self.bw_anc_profile.read().await.to_wire_value(&mode_norm);
            if let Err(e) = client.set_anc(bw_mode).await {
                return (false, format!("Failed to send B&W ANC command: {}", e));
            }
            // Request read-back to confirm ANC mode
            let _ = client.send_command::<()>(BWRPCatalog::ANC_GET, None).await;

            self.state.write().await.noise_mode = mode_norm.clone();
            self.notify_state_changed().await;
            return (
                true,
                format!("Bowers & Wilkins ANC mode set to {}", mode_norm),
            );
        }

        (false, "No active headphone session connected".to_string())
    }

    /// Configures tone controls or equalizers, or queries current settings if `args` is empty.
    ///
    /// Supports Bowers & Wilkins 2-band Bass / Treble tone controls (-6 to +6 dB, e.g. `"bass 2 treble -1"`, `"3 -1"`, `"flat"`)
    /// and 5-band parametric equalizers (e.g. `"0 1 2 1 0"`).
    pub async fn set_equalizer(&self, args: &str) -> (bool, String) {
        let args_trim = args.trim();
        if args_trim.is_empty() {
            let preset = self.state.read().await.equalizer_preset.clone();
            if preset.is_empty() {
                return (true, "Equalizer: default / flat".to_string());
            } else {
                return (true, format!("Equalizer: {}", preset));
            }
        }

        // B&W: Equalizer & Tone Controls
        let bw_client = self.active_bw_client.lock().await.clone();
        if let Some(client) = bw_client {
            let lower = args_trim.to_lowercase();
            let is_reset = lower == "flat" || lower == "reset" || lower == "off";

            let tokens: Vec<&str> = args_trim
                .split(|c: char| c.is_whitespace() || c == ',' || c == ':' || c == '=')
                .filter(|s| !s.is_empty())
                .collect();

            let has_labeled = tokens.iter().any(|&t| {
                let l = t.to_lowercase();
                l == "bass" || l == "treble"
            });

            let mut bass: Option<i32> = None;
            let mut treble: Option<i32> = None;
            let mut five_bands: Option<Vec<i32>> = None;

            if is_reset {
                bass = Some(0);
                treble = Some(0);
                five_bands = Some(vec![0, 0, 0, 0, 0]);
            } else if has_labeled {
                let mut i = 0;
                while i < tokens.len() {
                    let tok = tokens[i].to_lowercase();
                    if tok == "bass" && i + 1 < tokens.len() {
                        if let Some(val) = parse_bw_level(tokens[i + 1]) {
                            bass = Some(val);
                        }
                        i += 2;
                    } else if tok == "treble" && i + 1 < tokens.len() {
                        if let Some(val) = parse_bw_level(tokens[i + 1]) {
                            treble = Some(val);
                        }
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            } else {
                let nums: Vec<i32> = tokens.iter().filter_map(|t| parse_bw_level(t)).collect();
                if nums.len() >= 5 {
                    five_bands = Some(nums[..5].to_vec());
                } else if nums.len() == 2 {
                    bass = Some(nums[0]);
                    treble = Some(nums[1]);
                } else if nums.len() == 1 {
                    if nums[0] == 0 {
                        bass = Some(0);
                        treble = Some(0);
                        five_bands = Some(vec![0, 0, 0, 0, 0]);
                    } else {
                        bass = Some(nums[0]);
                    }
                }
            }

            let supports_5band = *self.bw_supports_5band.read().await;
            let supports_tone = *self.bw_supports_tone.read().await;

            if let Some(bands) = five_bands
                && (!supports_tone || supports_5band)
            {
                if let Err(e) = client.send_command(BWRPCatalog::EQ_SET, Some(&bands)).await {
                    return (
                        false,
                        format!("Failed to send B&W 5-band EQ command: {}", e),
                    );
                }
                let _ = client.send_command::<()>(BWRPCatalog::EQ_GET, None).await;
                *self.bw_eq_5band.write().await = Some(bands.clone());
                let status_str = format!("5-band EQ: {}", format_bw_5band(&bands));
                self.state.write().await.equalizer_preset = status_str.clone();
                self.notify_state_changed().await;
                return (true, format!("B&W equalizer updated: {}", status_str));
            }

            if bass.is_some() || treble.is_some() {
                // If device only supports 5-band EQ (Px7 S3)
                if supports_5band && !supports_tone {
                    let mut bands = self
                        .bw_eq_5band
                        .read()
                        .await
                        .clone()
                        .unwrap_or_else(|| vec![0, 0, 0, 0, 0]);
                    if let Some(b) = bass {
                        bands[0] = b;
                    }
                    if let Some(t) = treble {
                        bands[4] = t;
                    }
                    if let Err(e) = client.send_command(BWRPCatalog::EQ_SET, Some(&bands)).await {
                        return (
                            false,
                            format!("Failed to send B&W 5-band EQ command: {}", e),
                        );
                    }
                    let _ = client.send_command::<()>(BWRPCatalog::EQ_GET, None).await;
                    *self.bw_eq_5band.write().await = Some(bands.clone());
                    let status_str = format!("5-band EQ: {}", format_bw_5band(&bands));
                    self.state.write().await.equalizer_preset = status_str.clone();
                    self.notify_state_changed().await;
                    return (true, format!("B&W equalizer updated: {}", status_str));
                }

                // Standard tone controls (Px7 S2, Px7 S2e, Px8)
                if let Some(b) = bass {
                    if let Err(e) = client.send_command(BWRPCatalog::BASS_SET, Some(&b)).await {
                        return (false, format!("Failed to set B&W bass: {}", e));
                    }
                    let _ = client.send_command::<()>(BWRPCatalog::BASS_GET, None).await;
                    *self.bw_bass.write().await = Some(b);
                }
                if let Some(t) = treble {
                    if let Err(e) = client.send_command(BWRPCatalog::TREBLE_SET, Some(&t)).await {
                        return (false, format!("Failed to set B&W treble: {}", e));
                    }
                    let _ = client
                        .send_command::<()>(BWRPCatalog::TREBLE_GET, None)
                        .await;
                    *self.bw_treble.write().await = Some(t);
                }
                let status_str =
                    format_bw_tone(*self.bw_bass.read().await, *self.bw_treble.read().await);
                self.state.write().await.equalizer_preset = status_str.clone();
                self.notify_state_changed().await;
                return (true, format!("B&W tone controls updated: {}", status_str));
            }
        }

        (
            false,
            "Failed to apply equalizer / tone controls".to_string(),
        )
    }

    /// Configures the auto-standby / auto-power-off timer in minutes for Bowers & Wilkins headphones.
    pub async fn set_standby(&self, val: &str) -> bool {
        let minutes: i32 = val.parse().unwrap_or(15);

        // B&W
        let bw_client = self.active_bw_client.lock().await.clone();
        if let Some(client) = bw_client
            && client.set_standby(minutes).await.is_ok()
        {
            self.state.write().await.standby_minutes = minutes;
            self.notify_state_changed().await;
            return true;
        }

        false
    }

    /// Remaps the physical quick action button (`"anc"` or `"voice"`) on Bowers & Wilkins headphones.
    pub async fn set_action_button(&self, action: &str) -> (bool, String) {
        let bw_client = self.active_bw_client.lock().await.clone();
        if let Some(client) = bw_client {
            let mode: u8 = if action.to_lowercase().contains("voice") {
                1
            } else {
                0
            };
            if client.set_action_button(mode).await.is_ok() {
                let chosen = if mode == 1 { "voice" } else { "anc" };
                self.state.write().await.action_button = chosen.to_string();
                self.notify_state_changed().await;
                return (true, format!("Action button configured to {}", chosen));
            }
        }
        (
            false,
            "Action button only supported on Bowers & Wilkins headphones".to_string(),
        )
    }

    /// Enables or disables Conversation Awareness on AirPods Pro 2 / AirPods 4 ANC.
    pub async fn set_conversation_awareness(&self, enabled: bool) -> bool {
        let apple_session = self.active_apple_session.lock().await.clone();
        if let Some(session) = apple_session
            && session.set_conversation_awareness(enabled).await.is_ok()
        {
            self.state.write().await.conv_aware = enabled;
            self.notify_state_changed().await;
            return true;
        }
        false
    }

    /// Enables or disables hardware in-ear wear sensors (Apple, Bowers & Wilkins).
    pub async fn set_ear_detection(&self, enabled: bool) -> bool {
        // Apple
        let apple_session = self.active_apple_session.lock().await.clone();
        if let Some(session) = apple_session
            && session.set_ear_detection(enabled).await.is_ok()
        {
            self.state.write().await.ear_detection = enabled;
            self.notify_state_changed().await;
            return true;
        }

        // B&W
        let bw_client = self.active_bw_client.lock().await.clone();
        if let Some(client) = bw_client
            && client.set_wear_sensor(enabled).await.is_ok()
        {
            self.state.write().await.ear_detection = enabled;
            self.notify_state_changed().await;
            return true;
        }

        false
    }

    /// Sets the wear sensor sensitivity for Bowers & Wilkins headphones (0 = Low, 1 = Normal/Medium, 2 = High).
    pub async fn set_wear_sensor_sensitivity(&self, level: u8) -> bool {
        let bw_client = self.active_bw_client.lock().await.clone();
        if let Some(client) = bw_client
            && client.set_wear_sensor_sensitivity(level).await.is_ok()
        {
            *self.bw_wear_sensitivity.write().await = Some(level);
            self.state.write().await.wear_sensitivity = level as i32;
            self.notify_state_changed().await;
            return true;
        }
        false
    }

    /// Returns the current wear sensor sensitivity for Bowers & Wilkins headphones, if known.
    pub async fn get_wear_sensor_sensitivity(&self) -> Option<u8> {
        *self.bw_wear_sensitivity.read().await
    }

    /// Configures wear sensor state or sensitivity via action string (`"on"`, `"off"`, `"low"`, `"medium"`, `"high"`).
    pub async fn set_wear_sensor_string(&self, action: &str) -> (bool, String) {
        let action_clean = action.trim().to_lowercase();
        match action_clean.as_str() {
            "on" | "enable" | "true" | "1" => {
                let ok = self.set_ear_detection(true).await;
                (
                    ok,
                    if ok {
                        "Wear sensor enabled.".to_string()
                    } else {
                        "Failed to enable wear sensor.".to_string()
                    },
                )
            }
            "off" | "disable" | "false" | "0" => {
                let ok = self.set_ear_detection(false).await;
                (
                    ok,
                    if ok {
                        "Wear sensor disabled.".to_string()
                    } else {
                        "Failed to disable wear sensor.".to_string()
                    },
                )
            }
            "low" => {
                let ok = self.set_wear_sensor_sensitivity(0).await;
                (
                    ok,
                    if ok {
                        "Bowers & Wilkins wear sensor sensitivity set to: Low".to_string()
                    } else {
                        "Failed to set wear sensor sensitivity.".to_string()
                    },
                )
            }
            "medium" | "normal" => {
                let ok = self.set_wear_sensor_sensitivity(1).await;
                (
                    ok,
                    if ok {
                        "Bowers & Wilkins wear sensor sensitivity set to: Normal / Medium".to_string()
                    } else {
                        "Failed to set wear sensor sensitivity.".to_string()
                    },
                )
            }
            "high" => {
                let ok = self.set_wear_sensor_sensitivity(2).await;
                (
                    ok,
                    if ok {
                        "Bowers & Wilkins wear sensor sensitivity set to: High".to_string()
                    } else {
                        "Failed to set wear sensor sensitivity.".to_string()
                    },
                )
            }
            other => (
                false,
                format!(
                    "Unknown wear sensor action '{}'. Use on, off, low, medium, or high.",
                    other
                ),
            ),
        }
    }

    /// Configures active microphone selection (`"auto"`, `"left"`, `"right"`) for Apple accessories.
    pub async fn set_mic_mode(&self, mode_str: &str) -> bool {
        let mode = match mode_str.to_lowercase().as_str() {
            "auto" => MicSelection::Auto,
            "right" => MicSelection::AlwaysRight,
            "left" => MicSelection::AlwaysLeft,
            _ => return false,
        };

        let apple_session = self.active_apple_session.lock().await.clone();
        if let Some(session) = apple_session
            && session.set_mic_mode(mode).await.is_ok()
        {
            self.state.write().await.mic_mode = mode_str.to_string();
            self.notify_state_changed().await;
            return true;
        }
        false
    }

    /// Enables or disables single-bud active noise cancellation permission on Apple accessories.
    pub async fn set_one_bud_anc(&self, enabled: bool) -> bool {
        let apple_session = self.active_apple_session.lock().await.clone();
        if let Some(session) = apple_session
            && session.set_one_bud_anc(enabled).await.is_ok()
        {
            self.state.write().await.one_bud_anc = enabled;
            self.notify_state_changed().await;
            return true;
        }
        false
    }

    /// Configures case speaker and bud alert chime volume (`0..=100`) on AirPods Pro 2.
    pub async fn set_chime_volume(&self, vol: u8) -> bool {
        let apple_session = self.active_apple_session.lock().await.clone();
        if let Some(session) = apple_session
            && session.set_chime_volume(vol).await.is_ok()
        {
            self.state.write().await.chime_volume = vol as i32;
            self.notify_state_changed().await;
            return true;
        }
        false
    }

    async fn connect_vendor_session(&self, device: &PairedDevice) -> (bool, String) {
        let is_same_device = self.active_address.read().await.as_deref() == Some(&device.address);
        if !is_same_device {
            self.disconnect_device().await;
        }

        match device.vendor {
            DeviceVendor::Apple => {
                let (tx, mut rx) = mpsc::channel::<Frame>(64);
                let address = device.address.clone();
                let name = device.name.clone();

                {
                    let mut s = self.state.write().await;
                    s.connection_state = "connecting".to_string();
                    s.device_name = name.clone();
                    s.vendor = "Apple".to_string();
                }

                match AppleL2capSession::start(&address, tx).await {
                    Ok(session) => {
                        *self.active_apple_session.lock().await = Some(session);
                        *self.active_address.write().await = Some(address.clone());

                        // Check cached case battery and preferred noise mode from settings
                        let cached_case =
                            self.settings.lock().await.get_cached_case_battery(&address);
                        let preferred_mode = self
                            .settings
                            .lock()
                            .await
                            .get_preferred_noise_mode(&address);

                        {
                            let mut s = self.state.write().await;
                            s.connected = true;
                            s.connection_state = "connected".to_string();
                            s.device_name = name.clone();
                            s.vendor = "Apple".to_string();
                            s.one_bud_anc = true;
                            if let Some(p) = device.battery_percent {
                                s.battery_percent = p as i32;
                            }
                            if let Some(c) = cached_case {
                                s.battery_case = c as i32;
                            }
                        }

                        // Restore preferred noise mode if configured
                        if let Some(pref) = preferred_mode
                            && let Some(session) = self.active_apple_session.lock().await.as_ref()
                        {
                            let target_mode = match pref.as_str() {
                                "off" => Some(ListeningMode::Off),
                                "anc" | "nc" | "on" => Some(ListeningMode::NoiseCancellation),
                                "ambient" | "transparency" => Some(ListeningMode::Transparency),
                                "adaptive" => Some(ListeningMode::Adaptive),
                                _ => None,
                            };
                            if let Some(m) = target_mode {
                                let _ = session.set_noise_mode(m).await;
                            }
                        }

                        // Send connection desktop notification
                        self.notifications
                            .lock()
                            .await
                            .notify_connected(
                                &name,
                                "Apple",
                                device.battery_percent.map(|b| b as i32),
                            )
                            .await;

                        // Spawn frame handling loop
                        let state_clone = self.state.clone();
                        let active_addr_clone = self.active_address.clone();
                        let settings_clone = self.settings.clone();
                        let notif_clone = self.notifications.clone();
                        let dbus_conn_clone = self.dbus_conn.clone();
                        let auto_pause_clone = self.auto_pause.clone();
                        let active_apple_session_clone = self.active_apple_session.clone();
                        let expected_addr = address.clone();
                        let dev_name = name.clone();

                        self.notify_state_changed().await;

                        tokio::spawn(async move {
                            while let Some(frame) = rx.recv().await {
                                if active_addr_clone.read().await.as_deref() != Some(&expected_addr)
                                {
                                    break;
                                }

                                let mut changed = false;

                                match Opcode::from_u8(frame.opcode) {
                                    Some(Opcode::Battery) => {
                                        let comps = aap::parse_battery(&frame.payload);
                                        let mut s = state_clone.write().await;
                                        s.is_charging = comps
                                            .iter()
                                            .any(|c| c.status == BatteryStatus::Charging);
                                        for c in comps {
                                            match c.kind {
                                                BatteryKind::Right => {
                                                    if c.status != BatteryStatus::Disconnected {
                                                        s.battery_right = c.level as i32;
                                                    }
                                                }
                                                BatteryKind::Left => {
                                                    if c.status != BatteryStatus::Disconnected {
                                                        s.battery_left = c.level as i32;
                                                    }
                                                }
                                                BatteryKind::Case => {
                                                    if c.status != BatteryStatus::Disconnected
                                                        && c.level > 0
                                                    {
                                                        s.battery_case = c.level as i32;
                                                        settings_clone
                                                            .lock()
                                                            .await
                                                            .set_cached_case_battery(
                                                                &expected_addr,
                                                                c.level,
                                                            );
                                                    }
                                                }
                                                BatteryKind::Single => {
                                                    s.battery_percent = c.level as i32;
                                                }
                                            }
                                        }

                                        if s.battery_percent < 0 {
                                            if s.battery_right >= 0 && s.battery_left >= 0 {
                                                s.battery_percent =
                                                    s.battery_right.min(s.battery_left);
                                            } else if s.battery_right >= 0 {
                                                s.battery_percent = s.battery_right;
                                            } else if s.battery_left >= 0 {
                                                s.battery_percent = s.battery_left;
                                            }
                                        }

                                        let current_pct = s.battery_percent;
                                        let json = format_state_json(&s);
                                        drop(s);
                                        notif_clone
                                            .lock()
                                            .await
                                            .check_battery(&dev_name, current_pct)
                                            .await;

                                        if let Some(conn) = dbus_conn_clone.lock().await.as_ref() {
                                            let _ = conn
                                                .emit_signal(
                                                    None::<()>,
                                                    "/org/headbridge",
                                                    "org.headbridge.Manager",
                                                    "StateChanged",
                                                    &(json,),
                                                )
                                                .await;
                                        }
                                    }

                                    Some(Opcode::Settings) => {
                                        if let Some(notice) = SettingNotice::parse(&frame)
                                            && let (Some(setting_id), Some(&val)) =
                                                (SettingId::from_u8(notice.id), notice.data.first())
                                        {
                                            let mut s = state_clone.write().await;
                                            match setting_id {
                                                SettingId::NoiseControl => {
                                                    s.noise_mode = match val {
                                                        1 => "off".to_string(),
                                                        2 => "anc".to_string(),
                                                        3 => "ambient".to_string(),
                                                        4 => "adaptive".to_string(),
                                                        _ => s.noise_mode.clone(),
                                                    };
                                                    changed = true;
                                                }
                                                SettingId::ConvAware => {
                                                    s.conv_aware = val == 1;
                                                    changed = true;
                                                }
                                                SettingId::OneBudAnc => {
                                                    s.one_bud_anc = val == 1;
                                                    changed = true;
                                                }
                                                SettingId::ChimeVolume => {
                                                    s.chime_volume = val as i32;
                                                    changed = true;
                                                }
                                                SettingId::MicMode => {
                                                    s.mic_mode = match val {
                                                        0 => "auto".to_string(),
                                                        1 => "right".to_string(),
                                                        2 => "left".to_string(),
                                                        _ => "auto".to_string(),
                                                    };
                                                    changed = true;
                                                }
                                                SettingId::EarDetection => {
                                                    s.ear_detection = val == 1;
                                                    changed = true;
                                                }
                                                _ => {}
                                            }
                                            let json = format_state_json(&s);
                                            drop(s);

                                            if changed
                                                && let Some(conn) =
                                                    dbus_conn_clone.lock().await.as_ref()
                                            {
                                                let _ = conn
                                                    .emit_signal(
                                                        None::<()>,
                                                        "/org/headbridge",
                                                        "org.headbridge.Manager",
                                                        "StateChanged",
                                                        &(json,),
                                                    )
                                                    .await;
                                            }
                                        }
                                    }

                                    Some(Opcode::EarDetection) => {
                                        if let Some(in_ear) = aap::parse_in_ear(&frame.payload) {
                                            let is_wearing = in_ear.is_worn();
                                            let was_wearing = {
                                                let s = state_clone.read().await;
                                                s.in_ear
                                            };
                                            {
                                                let mut s = state_clone.write().await;
                                                s.in_ear = is_wearing;
                                                s.in_ear_primary =
                                                    in_ear.primary.as_str().to_string();
                                                s.in_ear_secondary =
                                                    in_ear.secondary.as_str().to_string();
                                            }

                                            let dbus_conn = dbus_conn_clone.lock().await.clone();
                                            if let Some(conn) = dbus_conn.as_ref() {
                                                crate::audio::AutoPauseCoordinator::on_ear_status_update(
                                                    auto_pause_clone.clone(),
                                                    in_ear,
                                                    conn,
                                                )
                                                .await;
                                            }

                                            // Auto-restore preferred ANC / listening mode when putting earbuds in!
                                            let current_noise_mode =
                                                state_clone.read().await.noise_mode.clone();
                                            if !was_wearing
                                                && is_wearing
                                                && let Some(pref) = settings_clone
                                                    .lock()
                                                    .await
                                                    .get_preferred_noise_mode(&expected_addr)
                                            {
                                                let target_mode = match pref.as_str() {
                                                    "off" => Some(ListeningMode::Off),
                                                    "anc" | "nc" | "on" => {
                                                        Some(ListeningMode::NoiseCancellation)
                                                    }
                                                    "ambient" | "transparency" => {
                                                        Some(ListeningMode::Transparency)
                                                    }
                                                    "adaptive" => Some(ListeningMode::Adaptive),
                                                    _ => None,
                                                };
                                                let expected_mode = match pref.as_str() {
                                                    "off" => "off",
                                                    "anc" | "nc" | "on" => "anc",
                                                    "ambient" | "transparency" => "ambient",
                                                    "adaptive" => "adaptive",
                                                    _ => "",
                                                };
                                                if let Some(m) = target_mode
                                                    && current_noise_mode != expected_mode
                                                    && let Some(session) =
                                                        active_apple_session_clone
                                                            .lock()
                                                            .await
                                                            .as_ref()
                                                {
                                                    let _ = session.set_noise_mode(m).await;

                                                    // Ensure state reflects restored mode if hardware frame is delayed
                                                    let state_inner = state_clone.clone();
                                                    let dbus_inner = dbus_conn_clone.clone();
                                                    let expected_mode = match m {
                                                        ListeningMode::Off => "off",
                                                        ListeningMode::NoiseCancellation => "anc",
                                                        ListeningMode::Transparency => "ambient",
                                                        ListeningMode::Adaptive => "adaptive",
                                                    }
                                                    .to_string();

                                                    tokio::spawn(async move {
                                                        tokio::time::sleep(Duration::from_millis(
                                                            400,
                                                        ))
                                                        .await;
                                                        let mut updated = false;
                                                        {
                                                            let mut s = state_inner.write().await;
                                                            if s.in_ear
                                                                && s.noise_mode != expected_mode
                                                            {
                                                                s.noise_mode = expected_mode;
                                                                updated = true;
                                                            }
                                                        }
                                                        if updated {
                                                            let s = state_inner.read().await;
                                                            let json = format_state_json(&s);
                                                            drop(s);
                                                            if let Some(conn) =
                                                                dbus_inner.lock().await.as_ref()
                                                            {
                                                                let _ = conn
                                                                    .emit_signal(
                                                                        None::<()>,
                                                                        "/org/headbridge",
                                                                        "org.headbridge.Manager",
                                                                        "StateChanged",
                                                                        &(json,),
                                                                    )
                                                                    .await;
                                                            }
                                                        }
                                                    });
                                                }
                                            }

                                            let s = state_clone.read().await;
                                            let json = format_state_json(&s);
                                            drop(s);
                                            if let Some(conn) =
                                                dbus_conn_clone.lock().await.as_ref()
                                            {
                                                let _ = conn
                                                    .emit_signal(
                                                        None::<()>,
                                                        "/org/headbridge",
                                                        "org.headbridge.Manager",
                                                        "StateChanged",
                                                        &(json,),
                                                    )
                                                    .await;
                                            }
                                        }
                                    }

                                    _ => {}
                                }
                            }

                            println!("Apple AAP L2CAP session ended for {}", expected_addr);
                            *active_apple_session_clone.lock().await = None;
                        });

                        (
                            true,
                            format!("Successfully connected to {} ({})", name, address),
                        )
                    }
                    Err(e) => {
                        let mut s = self.state.write().await;
                        s.connected = false;
                        s.connection_state = "failed".to_string();
                        drop(s);
                        self.notify_state_changed().await;
                        (false, format!("L2CAP connect to {} failed: {}", address, e))
                    }
                }
            }

            DeviceVendor::BowersWilkins => {
                let address = device.address.clone();
                let name = device.name.clone();

                // Select initial ANC wire profile based on device name
                let detected_profile = BwAncWireProfile::for_device_name(&name);
                *self.bw_anc_profile.write().await = detected_profile;

                let conn_res = Connection::system().await;
                let mut gatt_ready = false;

                if let Ok(conn) = conn_res {
                    match BluezGattClient::resolve_paths(&conn, &address).await {
                        Ok(Some(paths)) => {
                            match BluezGattClient::new(
                                conn,
                                &paths.request_path,
                                &paths.response_path,
                                paths.notification_path.as_deref(),
                            )
                            .await
                            {
                                Ok(client) => {
                                    gatt_ready = true;
                                    // Query initial state from headphone
                                    let _ =
                                        client.send_command::<()>(BWRPCatalog::ANC_GET, None).await;
                                    let _ = client
                                        .send_command::<()>(
                                            BWRPCatalog::BATTERY_PERCENTAGE_GET,
                                            None,
                                        )
                                        .await;
                                    let _ = client
                                        .send_command::<()>(BWRPCatalog::CHARGING_STATUS_GET, None)
                                        .await;
                                    let _ = client
                                        .send_command::<()>(BWRPCatalog::WEAR_GET, None)
                                        .await;
                                    let _ = client
                                        .send_command::<()>(
                                            BWRPCatalog::WEAR_SENSITIVITY_GET,
                                            None,
                                        )
                                        .await;
                                    let _ = client
                                        .send_command::<()>(BWRPCatalog::BASS_GET, None)
                                        .await;
                                    let _ = client
                                        .send_command::<()>(BWRPCatalog::TREBLE_GET, None)
                                        .await;
                                    let _ =
                                        client.send_command::<()>(BWRPCatalog::EQ_GET, None).await;
                                    let _ = client
                                        .send_command::<()>(BWRPCatalog::SLEEP_GET, None)
                                        .await;
                                    let _ = client
                                        .send_command::<()>(
                                            BWRPCatalog::PAIRED_DEVICE_COUNT_GET,
                                            None,
                                        )
                                        .await;

                                    let (pkt_tx, mut pkt_rx) = mpsc::channel::<Vec<u8>>(64);

                                    // Forward value changes from resp_char
                                    let tx1 = pkt_tx.clone();
                                    let mut resp_stream =
                                        client.resp_char.receive_value_changed().await;
                                    let active_addr_clone = self.active_address.clone();
                                    let expected_addr = address.clone();
                                    let addr_check1 = active_addr_clone.clone();
                                    let exp_addr1 = expected_addr.clone();
                                    tokio::spawn(async move {
                                        use futures_lite::StreamExt;
                                        while let Some(change) = resp_stream.next().await {
                                            if addr_check1.read().await.as_deref()
                                                != Some(&exp_addr1)
                                            {
                                                break;
                                            }
                                            if let Ok(raw) = change.get().await
                                                && tx1.send(raw).await.is_err()
                                            {
                                                break;
                                            }
                                        }
                                    });

                                    // Forward value changes from notif_char if present
                                    if let Some(ref notif_char) = client.notif_char {
                                        let tx2 = pkt_tx.clone();
                                        let mut notif_stream =
                                            notif_char.receive_value_changed().await;
                                        let addr_check2 = active_addr_clone.clone();
                                        let exp_addr2 = expected_addr.clone();
                                        tokio::spawn(async move {
                                            use futures_lite::StreamExt;
                                            while let Some(change) = notif_stream.next().await {
                                                if addr_check2.read().await.as_deref()
                                                    != Some(&exp_addr2)
                                                {
                                                    break;
                                                }
                                                if let Ok(raw) = change.get().await
                                                    && tx2.send(raw).await.is_err()
                                                {
                                                    break;
                                                }
                                            }
                                        });
                                    }

                                    // Reader task processing all incoming BWRPC packets
                                    let state_clone = self.state.clone();
                                    let notif_clone = self.notifications.clone();
                                    let dbus_conn_clone = self.dbus_conn.clone();
                                    let dev_name = name.clone();
                                    let active_bw_client_clone = self.active_bw_client.clone();
                                    let bw_anc_profile_clone = self.bw_anc_profile.clone();
                                    let bw_bass_clone = self.bw_bass.clone();
                                    let bw_treble_clone = self.bw_treble.clone();
                                    let bw_eq_5band_clone = self.bw_eq_5band.clone();
                                    let bw_supports_5band_clone = self.bw_supports_5band.clone();
                                    let bw_supports_tone_clone = self.bw_supports_tone.clone();
                                    let bw_paired_devices_clone = self.bw_paired_devices.clone();
                                    let bw_wear_sensitivity_clone = self.bw_wear_sensitivity.clone();
                                    let auto_pause_clone = self.auto_pause.clone();
                                    let exp_addr_main = expected_addr.clone();

                                    let reader_task = tokio::spawn(async move {
                                        while let Some(raw) = pkt_rx.recv().await {
                                            if active_addr_clone.read().await.as_deref()
                                                != Some(&exp_addr_main)
                                            {
                                                break;
                                            }
                                            if let Ok(msg) = BWRPC::decode(&raw) {
                                                let mut state_changed = false;

                                                // Dynamic profile fallback on error 6
                                                if msg.command.namespace == 0x03
                                                    && msg.command.id == 0x02
                                                    && msg.error_code == 6
                                                {
                                                    let mut prof =
                                                        bw_anc_profile_clone.write().await;
                                                    if *prof != BwAncWireProfile::OriginalPx8 {
                                                        eprintln!(
                                                            "[B&W] ANC command returned error 6; falling back to OriginalPx8 profile"
                                                        );
                                                        *prof = BwAncWireProfile::OriginalPx8;
                                                    }
                                                }

                                                match (msg.command.namespace, msg.command.id) {
                                                    (0x03, 0x01) | (0x03, 0x02) => {
                                                        let wire_val = msg
                                                            .deserialize_payload::<u8>()
                                                            .and_then(|r| r.ok())
                                                            .or_else(|| {
                                                                msg.deserialize_payload::<i32>()
                                                                    .and_then(|r| r.ok())
                                                                    .map(|v| v as u8)
                                                            })
                                                            .or_else(|| {
                                                                msg.deserialize_payload::<i64>()
                                                                    .and_then(|r| r.ok())
                                                                    .map(|v| v as u8)
                                                            });
                                                        if let Some(val) = wire_val {
                                                            let prof =
                                                                *bw_anc_profile_clone.read().await;
                                                            let mode = prof.from_wire_value(val);
                                                            let mut s = state_clone.write().await;
                                                            if s.noise_mode != mode {
                                                                s.noise_mode = mode.to_string();
                                                                state_changed = true;
                                                            }
                                                        }
                                                    }
                                                    (0x08, 0x0C) => {
                                                        if let Some(Ok(val)) = msg
                                                            .deserialize_payload::<i32>()
                                                            .or_else(|| {
                                                                msg.deserialize_payload::<u8>()
                                                                    .map(|r| r.map(|v| v as i32))
                                                            })
                                                        {
                                                            let mut s = state_clone.write().await;
                                                            if s.battery_percent != val {
                                                                s.battery_percent = val;
                                                                state_changed = true;
                                                            }
                                                            drop(s);
                                                            notif_clone
                                                                .lock()
                                                                .await
                                                                .check_battery(&dev_name, val)
                                                                .await;
                                                        }
                                                    }
                                                    (0x08, 0x0B) => {
                                                        let is_charging = msg
                                                            .deserialize_payload::<bool>()
                                                            .and_then(|r| r.ok())
                                                            .or_else(|| {
                                                                msg.deserialize_payload::<u8>()
                                                                    .and_then(|r| r.ok())
                                                                    .map(|v| v != 0)
                                                            });
                                                        if let Some(charging) = is_charging {
                                                            let mut s = state_clone.write().await;
                                                            if s.is_charging != charging {
                                                                s.is_charging = charging;
                                                                state_changed = true;
                                                            }
                                                        }
                                                    }
                                                    (0x0A, 0x01) | (0x0A, 0x02) => {
                                                        let wear = msg
                                                            .deserialize_payload::<bool>()
                                                            .and_then(|r| r.ok())
                                                            .or_else(|| {
                                                                msg.deserialize_payload::<u8>()
                                                                    .and_then(|r| r.ok())
                                                                    .map(|v| v != 0)
                                                            });
                                                        if let Some(wear_val) = wear {
                                                            let mut s = state_clone.write().await;
                                                            if msg.kind == BWRPCMessageKind::Notification {
                                                                if s.in_ear != wear_val {
                                                                    s.in_ear = wear_val;
                                                                    state_changed = true;
                                                                }
                                                            } else {
                                                                if s.ear_detection != wear_val {
                                                                    s.ear_detection = wear_val;
                                                                    state_changed = true;
                                                                }
                                                            }
                                                            drop(s);

                                                            if msg.kind == BWRPCMessageKind::Notification {
                                                                let dbus_conn = dbus_conn_clone.lock().await.clone();
                                                                if let Some(conn) = dbus_conn.as_ref() {
                                                                    crate::audio::AutoPauseCoordinator::on_wear_sensor_update(
                                                                        auto_pause_clone.clone(),
                                                                        wear_val,
                                                                        conn,
                                                                    )
                                                                    .await;
                                                                }
                                                            }
                                                        }
                                                    }
                                                    (0x0A, 0x03) | (0x0A, 0x04)
                                                        if msg.succeeded() =>
                                                    {
                                                        if let Some(sens) = msg
                                                            .deserialize_payload::<u8>()
                                                            .and_then(|r| r.ok())
                                                            .or_else(|| {
                                                                msg.deserialize_payload::<i32>()
                                                                    .and_then(|r| r.ok())
                                                                    .map(|v| v as u8)
                                                            })
                                                        {
                                                            *bw_wear_sensitivity_clone.write().await = Some(sens);
                                                            let mut s = state_clone.write().await;
                                                            if s.wear_sensitivity != sens as i32 {
                                                                s.wear_sensitivity = sens as i32;
                                                                state_changed = true;
                                                            }
                                                        }
                                                    }
                                                    (0x04, 0x18) | (0x04, 0x17)
                                                        if msg.succeeded() =>
                                                    {
                                                        if let Some(val) = msg
                                                            .deserialize_payload::<i32>()
                                                            .and_then(|r| r.ok())
                                                            .or_else(|| {
                                                                msg.deserialize_payload::<i8>()
                                                                    .and_then(|r| r.ok())
                                                                    .map(|v| v as i32)
                                                            })
                                                        {
                                                            *bw_supports_tone_clone.write().await =
                                                                true;
                                                            *bw_bass_clone.write().await =
                                                                Some(val);
                                                            let mut s = state_clone.write().await;
                                                            s.equalizer_preset = format_bw_tone(
                                                                *bw_bass_clone.read().await,
                                                                *bw_treble_clone.read().await,
                                                            );
                                                            state_changed = true;
                                                        }
                                                    }
                                                    (0x04, 0x1A) | (0x04, 0x19)
                                                        if msg.succeeded() =>
                                                    {
                                                        if let Some(val) = msg
                                                            .deserialize_payload::<i32>()
                                                            .and_then(|r| r.ok())
                                                            .or_else(|| {
                                                                msg.deserialize_payload::<i8>()
                                                                    .and_then(|r| r.ok())
                                                                    .map(|v| v as i32)
                                                            })
                                                        {
                                                            *bw_supports_tone_clone.write().await =
                                                                true;
                                                            *bw_treble_clone.write().await =
                                                                Some(val);
                                                            let mut s = state_clone.write().await;
                                                            s.equalizer_preset = format_bw_tone(
                                                                *bw_bass_clone.read().await,
                                                                *bw_treble_clone.read().await,
                                                            );
                                                            state_changed = true;
                                                        }
                                                    }
                                                    (0x04, 0x2A) | (0x04, 0x29)
                                                        if msg.succeeded() =>
                                                    {
                                                        if let Some(bands) = msg
                                                            .deserialize_payload::<Vec<i32>>()
                                                            .and_then(|r| r.ok())
                                                            && bands.len() >= 5
                                                        {
                                                            *bw_supports_5band_clone
                                                                .write()
                                                                .await = true;
                                                            let bands5 = bands[..5].to_vec();
                                                            *bw_eq_5band_clone.write().await =
                                                                Some(bands5.clone());
                                                            let mut s = state_clone.write().await;
                                                            s.equalizer_preset = format!(
                                                                "5-band EQ: {}",
                                                                format_bw_5band(&bands5)
                                                            );
                                                            state_changed = true;
                                                        }
                                                    }
                                                    (0x05, 0x0B) if msg.succeeded() => {
                                                        if let Some(count) =
                                                            parse_bw_paired_count(&msg)
                                                        {
                                                            let client_opt = active_bw_client_clone
                                                                .lock()
                                                                .await
                                                                .clone();
                                                            if let Some(client) = client_opt {
                                                                tokio::spawn(async move {
                                                                    for i in 0..count {
                                                                        let _ = client
                                                                            .send_command(
                                                                                BWRPCatalog::PAIRED_DEVICE_GET,
                                                                                Some(&(i as u8)),
                                                                            )
                                                                            .await;
                                                                        tokio::time::sleep(
                                                                            Duration::from_millis(
                                                                                30,
                                                                            ),
                                                                        )
                                                                        .await;
                                                                    }
                                                                });
                                                            }
                                                        }
                                                    }
                                                    (0x05, 0x04) if msg.succeeded() => {
                                                        if let Some(ref payload) = msg.payload_bytes
                                                        {
                                                            let current_count =
                                                                bw_paired_devices_clone
                                                                    .read()
                                                                    .await
                                                                    .len();
                                                            if let Some(mut dev) =
                                                                decode_paired_device(
                                                                    current_count,
                                                                    payload,
                                                                )
                                                            {
                                                                let mut list =
                                                                    bw_paired_devices_clone
                                                                        .write()
                                                                        .await;
                                                                if let Some(pos) =
                                                                    list.iter().position(|d| {
                                                                        !d.address.is_empty()
                                                                            && d.address
                                                                                != "00:00:00:00:00:00"
                                                                            && d.address
                                                                                == dev.address
                                                                    })
                                                                {
                                                                    dev.id = list[pos].id;
                                                                    list[pos] = dev;
                                                                } else {
                                                                    dev.id = list.len();
                                                                    list.push(dev);
                                                                }
                                                                list.sort_by_key(|d| d.id);
                                                            }
                                                        }
                                                    }
                                                    (0x05, 0x05) | (0x05, 0x06)
                                                        if msg.succeeded() =>
                                                    {
                                                        let client_opt = active_bw_client_clone
                                                            .lock()
                                                            .await
                                                            .clone();
                                                        if let Some(client) = client_opt {
                                                            tokio::spawn(async move {
                                                                tokio::time::sleep(
                                                                    Duration::from_millis(300),
                                                                )
                                                                .await;
                                                                let _ = client
                                                                    .send_command::<()>(
                                                                        BWRPCatalog::PAIRED_DEVICE_COUNT_GET,
                                                                        None,
                                                                    )
                                                                    .await;
                                                            });
                                                        }
                                                    }
                                                    _ => {}
                                                }

                                                if state_changed {
                                                    let s = state_clone.read().await;
                                                    let json = format_state_json(&s);
                                                    drop(s);
                                                    if let Some(conn) =
                                                        dbus_conn_clone.lock().await.as_ref()
                                                    {
                                                        let _ = conn
                                                            .emit_signal(
                                                                None::<()>,
                                                                "/org/headbridge",
                                                                "org.headbridge.Manager",
                                                                "StateChanged",
                                                                &(json,),
                                                            )
                                                            .await;
                                                    }
                                                }
                                            }
                                        }

                                        println!("B&W GATT session ended for {}", exp_addr_main);
                                        *active_bw_client_clone.lock().await = None;
                                    });

                                    let mut client = client;
                                    client.set_reader_task(reader_task);
                                    *self.active_bw_client.lock().await = Some(client);
                                }
                                Err(e) => {
                                    eprintln!("[B&W] BluezGattClient::new error: {}", e);
                                }
                            }
                        }
                        Ok(None) => {
                            eprintln!("[B&W] GATT paths not yet resolved for {}", address);
                        }
                        Err(e) => {
                            eprintln!("[B&W] Error resolving GATT paths: {}", e);
                        }
                    }
                }

                let prev_addr = self.active_address.read().await.clone();
                {
                    let mut s = self.state.write().await;
                    s.connected = true;
                    s.connection_state = if gatt_ready {
                        "connected".to_string()
                    } else {
                        "connected (audio only)".to_string()
                    };
                    s.device_name = name.clone();
                    s.vendor = "Bowers & Wilkins".to_string();
                    if let Some(p) = device.battery_percent {
                        s.battery_percent = p as i32;
                    }
                }

                *self.active_address.write().await = Some(address.clone());

                // Only send notification if newly connected
                if prev_addr.as_deref() != Some(&address) {
                    self.notifications
                        .lock()
                        .await
                        .notify_connected(
                            &name,
                            "Bowers & Wilkins",
                            device.battery_percent.map(|b| b as i32),
                        )
                        .await;
                }
                self.notify_state_changed().await;

                (
                    gatt_ready,
                    if gatt_ready {
                        format!("Connected to Bowers & Wilkins {} ({})", name, address)
                    } else {
                        format!(
                            "Connected to Bowers & Wilkins {} ({}) [Audio only, GATT resolving]",
                            name, address
                        )
                    },
                )
            }

            DeviceVendor::Unknown => (
                false,
                format!("Device {} is not supported by Resonate", device.name),
            ),
        }
    }

    /// Runs the continuous background device management and monitoring loop.
    ///
    /// Periodically queries PipeWire / PulseAudio default sink, tracks BlueZ connection events,
    /// establishes vendor control sessions for newly connected devices, and cleans up disconnected sessions.
    pub async fn run_monitoring_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_millis(2500));
        loop {
            interval.tick().await;

            // 1. Sync default PipeWire audio sink
            if let Some(sink) = AudioMonitor::get_default_sink().await {
                let mut sink_changed = false;
                {
                    let mut s = self.state.write().await;
                    if s.audio_sink != sink {
                        s.audio_sink = sink;
                        sink_changed = true;
                    }
                }
                if sink_changed {
                    self.notify_state_changed().await;
                }
            }

            // 2. Discover devices
            let devices = match BluetoothInventory::fetch_paired_devices().await {
                Ok(d) => d,
                Err(_) => continue,
            };

            let current_addr = self.active_address.read().await.clone();
            let connected_device = devices.iter().find(|d| d.connected && d.is_supported());

            match (current_addr, connected_device) {
                (None, Some(dev)) => {
                    println!(
                        "Detected connected supported headphone: {} ({}) - Initiating session...",
                        dev.name, dev.address
                    );
                    self.connect_vendor_session(dev).await;
                }
                (Some(addr), Some(dev)) if dev.address != addr => {
                    println!("Headphone switched to: {} ({})", dev.name, dev.address);
                    self.disconnect_device().await;
                    self.connect_vendor_session(dev).await;
                }
                (Some(addr), Some(dev)) if dev.address == addr => {
                    // Check if the vendor control session has dropped while BlueZ link is still connected
                    let session_alive = match dev.vendor {
                        DeviceVendor::Apple => self.active_apple_session.lock().await.is_some(),
                        DeviceVendor::BowersWilkins => self.active_bw_client.lock().await.is_some(),
                        _ => true,
                    };
                    if !session_alive {
                        let mut last_attempt = self.last_reconnect_attempt.lock().await;
                        let now = std::time::Instant::now();
                        let should_attempt = match *last_attempt {
                            Some((ref a, ref t)) if a == &dev.address => {
                                now.duration_since(*t) >= Duration::from_secs(15)
                            }
                            _ => true,
                        };
                        if should_attempt {
                            *last_attempt = Some((dev.address.clone(), now));
                            drop(last_attempt);
                            println!(
                                "Vendor control session for {} ({}) not active while Bluetooth connected — attempting connection...",
                                dev.name, dev.address
                            );
                            self.connect_vendor_session(dev).await;
                        }
                    }
                }
                (Some(addr), None) => {
                    println!("Headphone ({}) disconnected from Bluetooth.", addr);
                    self.disconnect_device().await;
                }
                _ => {}
            }
        }
    }

    async fn bluez_connect(address: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let conn = Connection::system().await?;
        let mac_segment = address.replace(':', "_").to_uppercase();
        let path = format!("/org/bluez/hci0/dev_{}", mac_segment);

        let proxy = zbus::Proxy::new(&conn, "org.bluez", path, "org.bluez.Device1").await?;

        let () = proxy.call("Connect", &()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_set_equalizer_empty_args_queries_current() {
        let state = Arc::new(RwLock::new(ResonateState::default()));
        state.write().await.equalizer_preset = "Bass: +2 dB, Treble: -1 dB".to_string();
        let handle = tokio::runtime::Handle::current();
        let mgr = DeviceManager::new(state, handle);

        let (ok, msg) = mgr.set_equalizer("").await;
        assert!(ok);
        assert_eq!(msg, "Equalizer: Bass: +2 dB, Treble: -1 dB");

        let (ok_spaces, msg_spaces) = mgr.set_equalizer("   ").await;
        assert!(ok_spaces);
        assert_eq!(msg_spaces, "Equalizer: Bass: +2 dB, Treble: -1 dB");
    }
}
