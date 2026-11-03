use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock};
use zbus::Connection;

use crate::audio::{AudioMonitor, AutoPauseCoordinator};
use crate::dbus_service::ResonateState;
use crate::inventory::{BluetoothInventory, DeviceVendor, PairedDevice};
use crate::notifications::NotificationService;
use crate::protocols::aap::{self, BatteryKind, BatteryStatus, Frame, ListeningMode, MicSelection, Opcode, SettingId, SettingNotice};
use crate::protocols::apple_models::AppleModel;
use crate::protocols::sony::{SonyMDRPacket, SonyV1AutomaticPowerOff, SonyV1EqualizerPreset};
use crate::settings::SettingsStore;
use crate::transport::bluez::BluezGattClient;
use crate::transport::l2cap::AppleL2capSession;
use crate::transport::rfcomm::SonyMdrSession;

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
        "conversationAwareness": s.conv_aware,
        "micMode": s.mic_mode,
        "oneBudAnc": s.one_bud_anc,
        "chimeVolume": s.chime_volume,
        "standbyMinutes": s.standby_minutes,
        "touchSensor": s.touch_sensor,
        "dsee": s.dsee,
        "equalizerPreset": s.equalizer_preset,
        "capabilities": s.capabilities,
    })).unwrap_or_else(|_| "{}".to_string())
}

fn capabilities_for_device(device: &PairedDevice) -> Vec<String> {
    let mut capabilities = vec!["battery".to_string(), "anc".to_string()];
    match device.vendor {
        DeviceVendor::Apple => {
            let model = device.model_name.as_deref()
                .map(AppleModel::from_name)
                .unwrap_or_else(|| AppleModel::from_name(&device.name));
            let apple = model.capabilities();
            capabilities.clear();
            capabilities.push("battery".to_string());
            if apple.has_anc { capabilities.push("anc".to_string()); }
            if apple.has_transparency { capabilities.push("ambient".to_string()); }
            if apple.has_adaptive { capabilities.push("adaptive".to_string()); }
            if apple.has_case_battery { capabilities.push("case_battery".to_string()); }
            if apple.has_in_ear_detection { capabilities.push("in_ear".to_string()); }
            if apple.has_ear_detection_setting { capabilities.push("wear_sensor".to_string()); }
            if apple.has_conv_awareness { capabilities.push("conversation_awareness".to_string()); }
            if apple.has_mic_selection { capabilities.push("microphone".to_string()); }
            if apple.has_press_and_hold { capabilities.push("one_bud_anc".to_string()); }
            capabilities.push("chime".to_string());
        }
        DeviceVendor::Sony => capabilities.extend(["ambient_level", "equalizer", "standby", "touch_sensor", "dsee", "optimize"].into_iter().map(str::to_string)),
        DeviceVendor::BowersWilkins => capabilities.extend(["equalizer", "standby", "wear_sensor", "action_button"].into_iter().map(str::to_string)),
        DeviceVendor::Unknown => capabilities.clear(),
    }
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

pub struct DeviceManager {
    pub state: Arc<RwLock<ResonateState>>,
    active_apple_session: Arc<Mutex<Option<AppleL2capSession>>>,
    active_sony_session: Arc<Mutex<Option<SonyMdrSession>>>,
    active_bw_client: Arc<Mutex<Option<BluezGattClient>>>,
    active_address: Arc<RwLock<Option<String>>>,
    pub handle: tokio::runtime::Handle,
    pub settings: Arc<Mutex<SettingsStore>>,
    pub notifications: Arc<Mutex<NotificationService>>,
    dbus_conn: Arc<Mutex<Option<Connection>>>,
    pub auto_pause: Arc<Mutex<AutoPauseCoordinator>>,
}

impl DeviceManager {
    pub fn new(state: Arc<RwLock<ResonateState>>, handle: tokio::runtime::Handle) -> Self {
        let store = SettingsStore::load();
        let auto_pause_enabled = store.data.auto_pause_enabled;

        Self {
            state,
            active_apple_session: Arc::new(Mutex::new(None)),
            active_sony_session: Arc::new(Mutex::new(None)),
            active_bw_client: Arc::new(Mutex::new(None)),
            active_address: Arc::new(RwLock::new(None)),
            handle,
            settings: Arc::new(Mutex::new(store)),
            notifications: Arc::new(Mutex::new(NotificationService::new())),
            dbus_conn: Arc::new(Mutex::new(None)),
            auto_pause: Arc::new(Mutex::new(AutoPauseCoordinator::new(auto_pause_enabled))),
        }
    }

    pub async fn set_auto_pause(&self, enabled: bool) -> bool {
        self.auto_pause.lock().await.set_enabled(enabled);
        self.settings.lock().await.set_auto_pause(enabled);
        self.state.write().await.auto_pause = enabled;
        self.notify_state_changed().await;
        true
    }

    pub async fn set_dbus_connection(&self, conn: Connection) {
        *self.dbus_conn.lock().await = Some(conn);
    }

    pub async fn notify_state_changed(&self) {
        let json = self.get_status_json().await;
        if let Some(conn) = self.dbus_conn.lock().await.as_ref() {
            let _ = conn.emit_signal(
                None::<()>,
                "/org/headbridge",
                "org.headbridge.Manager",
                "StateChanged",
                &(json,),
            ).await;
        }
    }

    pub async fn get_status_json(&self) -> String {
        let s = self.state.read().await;
        format_state_json(&s)
    }

    pub async fn get_capabilities(&self) -> Vec<String> {
        self.state.read().await.capabilities.clone()
    }

    pub async fn list_devices_json(&self) -> String {
        match BluetoothInventory::fetch_paired_devices().await {
            Ok(devices) => serde_json::to_string_pretty(&devices).unwrap_or_else(|_| "[]".to_string()),
            Err(e) => {
                eprintln!("Error fetching paired devices: {:?}", e);
                "[]".to_string()
            }
        }
    }

    pub async fn connect_device(&self, target: &str) -> (bool, String) {
        let devices = match BluetoothInventory::fetch_paired_devices().await {
            Ok(d) => d,
            Err(e) => return (false, format!("Failed to read devices: {}", e)),
        };

        let target_upper = target.to_uppercase();
        let target_lower = target.to_lowercase();

        let matched = devices.into_iter().find(|d| {
            d.address == target_upper
                || d.name.to_lowercase().contains(&target_lower)
        });

        let device = match matched {
            Some(d) => d,
            None => return (false, format!("Device '{}' not found in paired devices", target)),
        };

        if !device.is_supported() {
            return (false, format!("Device '{}' ({}) is not a supported headphone", device.name, device.address));
        }

        if let Some(current) = self.active_address.read().await.as_deref()
            && current == device.address && self.state.read().await.connected {
                return (true, format!("Already connected to {} ({})", device.name, device.address));
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

    pub async fn disconnect_device(&self) -> bool {
        let old_addr = self.active_address.write().await.take();
        *self.active_apple_session.lock().await = None;
        *self.active_sony_session.lock().await = None;
        *self.active_bw_client.lock().await = None;

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
            s.capabilities.clear();
            s.battery_percent = -1;
            s.battery_left = -1;
            s.battery_right = -1;
            s.battery_case = -1;
            s.is_charging = false;
            s.noise_mode = "off".to_string();
            s.in_ear = false;
            s.in_ear_primary = String::new();
            s.in_ear_secondary = String::new();
            n
        };

        if old_addr.is_some()
            && !name.is_empty() {
                self.notifications.lock().await.notify_disconnected(&name).await;
            }

        self.notify_state_changed().await;
        true
    }

    pub async fn set_anc(&self, mode_str: &str) -> (bool, String) {
        let mode_norm = mode_str.to_lowercase();

        // 1. Apple
        if let Some(session) = self.active_apple_session.lock().await.as_ref() {
            let mode = match mode_norm.as_str() {
                "off" => ListeningMode::Off,
                "anc" | "nc" | "on" => ListeningMode::NoiseCancellation,
                "ambient" | "transparency" => ListeningMode::Transparency,
                "adaptive" => ListeningMode::Adaptive,
                other => return (false, format!("Unsupported Apple ANC mode: '{}'. Use off, anc, ambient, or adaptive.", other)),
            };

            // Check if earbuds are worn in ear
            let is_worn = self.state.read().await.in_ear;
            let ear_detect_enabled = self.state.read().await.ear_detection;
            if mode == ListeningMode::NoiseCancellation && ear_detect_enabled && !is_worn {
                // If buds are out of the ear, record as preferred mode so when they are put on, ANC is enabled automatically.
                if let Some(addr) = self.active_address.read().await.as_deref() {
                    self.settings.lock().await.set_preferred_noise_mode(addr, &mode_norm);
                }
                return (false, "AirPods are not in your ears. Put them in to use Noise Cancellation.".to_string());
            }

            if let Err(e) = session.set_noise_mode(mode).await {
                // Mark session as dead if socket broken so monitoring loop reconnects
                *self.active_apple_session.lock().await = None;
                return (false, format!("Failed to send ANC command: {}", e));
            }

            if let Some(addr) = self.active_address.read().await.as_deref() {
                self.settings.lock().await.set_preferred_noise_mode(addr, &mode_norm);
            }

            // Wait up to 350ms for device confirmation (SettingNotice 0x0D echo from AirPods)
            let deadline = tokio::time::Instant::now() + Duration::from_millis(350);
            while tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(30)).await;
                if self.state.read().await.noise_mode == mode_norm {
                    return (true, format!("ANC mode set to {}", mode_norm));
                }
            }

            // If hardware has not confirmed, do not falsely overwrite internal state. Keep current state.
            let current = self.state.read().await.noise_mode.clone();
            if current != mode_norm {
                if ear_detect_enabled && !is_worn {
                    return (false, "AirPods did not activate ANC (earbuds must be in your ears)".to_string());
                }
                return (false, format!("AirPods did not confirm ANC mode change within timeout (currently {})", current));
            }

            return (true, format!("ANC mode set to {}", mode_norm));
        }

        // 2. Sony
        if let Some(session) = self.active_sony_session.lock().await.as_ref() {
            if let Err(e) = session.set_noise_mode(&mode_norm, 10).await {
                return (false, format!("Failed to send Sony ANC command: {}", e));
            }
            self.state.write().await.noise_mode = mode_norm.clone();
            self.notify_state_changed().await;
            return (true, format!("Sony ANC mode set to {}", mode_norm));
        }

        // 3. Bowers & Wilkins
        if let Some(client) = self.active_bw_client.lock().await.as_ref() {
            let bw_mode: u8 = match mode_norm.as_str() {
                "off" => 0,
                "anc" | "nc" | "on" => 1,
                "ambient" | "transparency" => 2,
                _ => 1,
            };
            if let Err(e) = client.set_anc(bw_mode).await {
                return (false, format!("Failed to send B&W ANC command: {}", e));
            }
            self.state.write().await.noise_mode = mode_norm.clone();
            self.notify_state_changed().await;
            return (true, format!("Bowers & Wilkins ANC mode set to {}", mode_norm));
        }

        (false, "No active headphone session connected".to_string())
    }

    pub async fn set_ambient_level(&self, level: i32) -> bool {
        let session_guard = self.active_sony_session.lock().await;
        if let Some(session) = session_guard.as_ref() {
            let current_mode = self.state.read().await.noise_mode.clone();
            session.set_noise_mode(&current_mode, level).await.is_ok()
        } else {
            false
        }
    }

    pub async fn set_equalizer(&self, args: &str) -> (bool, String) {
        // Sony: parse preset or bands
        if let Some(session) = self.active_sony_session.lock().await.as_ref() {
            if let Some(preset) = match args.to_lowercase().as_str() {
                "off" => Some(SonyV1EqualizerPreset::Off),
                "bright" => Some(SonyV1EqualizerPreset::Bright),
                "excited" => Some(SonyV1EqualizerPreset::Excited),
                "mellow" => Some(SonyV1EqualizerPreset::Mellow),
                "relaxed" => Some(SonyV1EqualizerPreset::Relaxed),
                "vocal" => Some(SonyV1EqualizerPreset::Vocal),
                "treble" => Some(SonyV1EqualizerPreset::TrebleBoost),
                "bass" => Some(SonyV1EqualizerPreset::BassBoost),
                "speech" => Some(SonyV1EqualizerPreset::Speech),
                _ => None,
            }
                && session.set_equalizer_preset(preset).await.is_ok() {
                    self.state.write().await.equalizer_preset = args.to_string();
                    return (true, format!("Sony Equalizer preset set to {}", args));
                }

            // Or parse comma-separated bands: "0,1,2,0,-1,3"
            let values: Vec<i32> = args.split([',', ' ']).filter_map(|s| s.parse().ok()).collect();
            if !values.is_empty()
                && session.set_equalizer_bands(&values).await.is_ok() {
                    return (true, format!("Sony Equalizer custom bands applied: {:?}", values));
                }
        }

        // B&W: parse bass and treble: "bass:2 treble:1" or "2 1"
        if let Some(client) = self.active_bw_client.lock().await.as_ref() {
            let nums: Vec<i32> = args.split_whitespace().filter_map(|s| s.parse().ok()).collect();
            let (bass, treble) = if nums.len() >= 2 {
                (nums[0], nums[1])
            } else {
                (0, 0)
            };
            if client.set_bass_treble(bass, treble).await.is_ok() {
                return (true, format!("B&W tone controls set: Bass={}, Treble={}", bass, treble));
            }
        }

        (false, "Failed to apply equalizer / tone controls".to_string())
    }

    pub async fn set_standby(&self, val: &str) -> bool {
        let minutes: i32 = val.parse().unwrap_or(15);

        // B&W
        if let Some(client) = self.active_bw_client.lock().await.as_ref()
            && client.set_standby(minutes).await.is_ok() {
                self.state.write().await.standby_minutes = minutes;
                return true;
            }

        // Sony
        if let Some(session) = self.active_sony_session.lock().await.as_ref() {
            let mode = if minutes <= 0 {
                SonyV1AutomaticPowerOff::Off
            } else if minutes <= 5 {
                SonyV1AutomaticPowerOff::After5Minutes
            } else if minutes <= 30 {
                SonyV1AutomaticPowerOff::After30Minutes
            } else if minutes <= 60 {
                SonyV1AutomaticPowerOff::After1Hour
            } else {
                SonyV1AutomaticPowerOff::After3Hours
            };
            if session.set_automatic_power_off(mode).await.is_ok() {
                self.state.write().await.standby_minutes = minutes;
                return true;
            }
        }

        false
    }

    pub async fn set_action_button(&self, action: &str) -> (bool, String) {
        if let Some(client) = self.active_bw_client.lock().await.as_ref() {
            let mode: u8 = if action.to_lowercase().contains("voice") { 1 } else { 0 };
            if client.set_action_button(mode).await.is_ok() {
                return (true, format!("Action button configured to {}", action));
            }
        }
        (false, "Action button only supported on Bowers & Wilkins headphones".to_string())
    }

    pub async fn set_touch_sensor(&self, enabled: bool) -> bool {
        if let Some(session) = self.active_sony_session.lock().await.as_ref()
            && session.set_touch_sensor(enabled).await.is_ok() {
                self.state.write().await.touch_sensor = enabled;
                return true;
            }
        false
    }

    pub async fn set_dsee(&self, enabled: bool) -> bool {
        if let Some(session) = self.active_sony_session.lock().await.as_ref()
            && session.set_dsee(enabled).await.is_ok() {
                self.state.write().await.dsee = enabled;
                return true;
            }
        false
    }

    pub async fn optimize(&self, action: &str) -> (bool, String) {
        if let Some(session) = self.active_sony_session.lock().await.as_ref() {
            let start = !action.to_lowercase().contains("stop");
            if session.set_optimizer(start).await.is_ok() {
                return (true, format!("Sony Optimizer calibration {}", if start { "started" } else { "stopped" }));
            }
        }
        (false, "NC Optimizer is only available on supported Sony MDR headphones".to_string())
    }

    pub async fn set_conversation_awareness(&self, enabled: bool) -> bool {
        let session_guard = self.active_apple_session.lock().await;
        if let Some(session) = session_guard.as_ref()
            && session.set_conversation_awareness(enabled).await.is_ok() {
                self.state.write().await.conv_aware = enabled;
                return true;
            }
        false
    }

    pub async fn set_ear_detection(&self, enabled: bool) -> bool {
        // Apple
        if let Some(session) = self.active_apple_session.lock().await.as_ref()
            && session.set_ear_detection(enabled).await.is_ok() {
                self.state.write().await.ear_detection = enabled;
                return true;
            }

        // B&W
        if let Some(client) = self.active_bw_client.lock().await.as_ref()
            && client.set_wear_sensor(enabled).await.is_ok() {
                self.state.write().await.ear_detection = enabled;
                return true;
            }

        false
    }

    pub async fn set_mic_mode(&self, mode_str: &str) -> bool {
        let mode = match mode_str.to_lowercase().as_str() {
            "auto" => MicSelection::Auto,
            "right" => MicSelection::AlwaysRight,
            "left" => MicSelection::AlwaysLeft,
            _ => return false,
        };

        let session_guard = self.active_apple_session.lock().await;
        if let Some(session) = session_guard.as_ref()
            && session.set_mic_mode(mode).await.is_ok() {
                self.state.write().await.mic_mode = mode_str.to_string();
                return true;
            }
        false
    }

    pub async fn set_one_bud_anc(&self, enabled: bool) -> bool {
        let session_guard = self.active_apple_session.lock().await;
        if let Some(session) = session_guard.as_ref()
            && session.set_one_bud_anc(enabled).await.is_ok() {
                self.state.write().await.one_bud_anc = enabled;
                return true;
            }
        false
    }

    pub async fn set_chime_volume(&self, vol: u8) -> bool {
        let session_guard = self.active_apple_session.lock().await;
        if let Some(session) = session_guard.as_ref()
            && session.set_chime_volume(vol).await.is_ok() {
                self.state.write().await.chime_volume = vol as i32;
                return true;
            }
        false
    }

    async fn connect_vendor_session(&self, device: &PairedDevice) -> (bool, String) {
        self.disconnect_device().await;

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
                    s.capabilities = capabilities_for_device(device);
                }

                match AppleL2capSession::start(&address, tx).await {
                    Ok(session) => {
                        *self.active_apple_session.lock().await = Some(session);
                        *self.active_address.write().await = Some(address.clone());

                        // Check cached case battery and preferred noise mode from settings
                        let cached_case = self.settings.lock().await.get_cached_case_battery(&address);
                        let preferred_mode = self.settings.lock().await.get_preferred_noise_mode(&address);

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
                            && let Some(session) = self.active_apple_session.lock().await.as_ref() {
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
                        self.notifications.lock().await.notify_connected(
                            &name,
                            "Apple",
                            device.battery_percent.map(|b| b as i32),
                        ).await;

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
                                if active_addr_clone.read().await.as_deref() != Some(&expected_addr) {
                                    break;
                                }

                                let mut changed = false;

                                match Opcode::from_u8(frame.opcode) {
                                    Some(Opcode::Battery) => {
                                        let comps = aap::parse_battery(&frame.payload);
                                        let mut s = state_clone.write().await;
                                        for c in comps {
                                            match c.kind {
                                                BatteryKind::Right => {
                                                    if c.status != BatteryStatus::Disconnected {
                                                        s.battery_right = c.level as i32;
                                                        if c.status == BatteryStatus::Charging {
                                                            s.is_charging = true;
                                                        }
                                                    }
                                                }
                                                BatteryKind::Left => {
                                                    if c.status != BatteryStatus::Disconnected {
                                                        s.battery_left = c.level as i32;
                                                        if c.status == BatteryStatus::Charging {
                                                            s.is_charging = true;
                                                        }
                                                    }
                                                }
                                                BatteryKind::Case => {
                                                    if c.status != BatteryStatus::Disconnected && c.level > 0 {
                                                        s.battery_case = c.level as i32;
                                                        settings_clone.lock().await.set_cached_case_battery(&expected_addr, c.level);
                                                    }
                                                }
                                                BatteryKind::Single => {
                                                    s.battery_percent = c.level as i32;
                                                }
                                            }
                                        }

                                        if s.battery_percent < 0 {
                                            if s.battery_right >= 0 && s.battery_left >= 0 {
                                                s.battery_percent = s.battery_right.min(s.battery_left);
                                            } else if s.battery_right >= 0 {
                                                s.battery_percent = s.battery_right;
                                            } else if s.battery_left >= 0 {
                                                s.battery_percent = s.battery_left;
                                            }
                                        }

                                        let current_pct = s.battery_percent;
                                        let json = format_state_json(&s);
                                        drop(s);
                                        notif_clone.lock().await.check_battery(&dev_name, current_pct).await;

                                        if let Some(conn) = dbus_conn_clone.lock().await.as_ref() {
                                            let _ = conn.emit_signal(
                                                None::<()>,
                                                "/org/headbridge",
                                                "org.headbridge.Manager",
                                                "StateChanged",
                                                &(json,),
                                            ).await;
                                        }
                                    }

                                    Some(Opcode::Settings) => {
                                        if let Some(notice) = SettingNotice::parse(&frame)
                                            && let (Some(setting_id), Some(&val)) = (SettingId::from_u8(notice.id), notice.data.first()) {
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
                                                    && let Some(conn) = dbus_conn_clone.lock().await.as_ref() {
                                                        let _ = conn.emit_signal(
                                                            None::<()>,
                                                            "/org/headbridge",
                                                            "org.headbridge.Manager",
                                                            "StateChanged",
                                                            &(json,),
                                                        ).await;
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
                                                s.in_ear_primary = in_ear.primary.as_str().to_string();
                                                s.in_ear_secondary = in_ear.secondary.as_str().to_string();
                                            }

                                            let dbus_conn = dbus_conn_clone.lock().await.clone();
                                            if let Some(conn) = dbus_conn.as_ref() {
                                                auto_pause_clone.lock().await.on_ear_status_update(in_ear, conn).await;
                                            }

                                            // Auto-restore preferred ANC / listening mode when putting earbuds in!
                                            if !was_wearing && is_wearing
                                                && let Some(pref) = settings_clone.lock().await.get_preferred_noise_mode(&expected_addr) {
                                                    let target_mode = match pref.as_str() {
                                                        "off" => Some(ListeningMode::Off),
                                                        "anc" | "nc" | "on" => Some(ListeningMode::NoiseCancellation),
                                                        "ambient" | "transparency" => Some(ListeningMode::Transparency),
                                                        "adaptive" => Some(ListeningMode::Adaptive),
                                                        _ => None,
                                                    };
                                                    if let Some(m) = target_mode
                                                        && let Some(session) = active_apple_session_clone.lock().await.as_ref() {
                                                            let _ = session.set_noise_mode(m).await;

                                                            // Ensure state reflects restored mode if hardware frame is delayed
                                                            let state_inner = state_clone.clone();
                                                            let dbus_inner = dbus_conn_clone.clone();
                                                            let expected_mode = match m {
                                                                ListeningMode::Off => "off",
                                                                ListeningMode::NoiseCancellation => "anc",
                                                                ListeningMode::Transparency => "ambient",
                                                                ListeningMode::Adaptive => "adaptive",
                                                            }.to_string();

                                                            tokio::spawn(async move {
                                                                tokio::time::sleep(Duration::from_millis(400)).await;
                                                                let mut updated = false;
                                                                {
                                                                    let mut s = state_inner.write().await;
                                                                    if s.in_ear && s.noise_mode != expected_mode {
                                                                        s.noise_mode = expected_mode;
                                                                        updated = true;
                                                                    }
                                                                }
                                                                if updated {
                                                                    let s = state_inner.read().await;
                                                                    let json = format_state_json(&s);
                                                                    drop(s);
                                                                    if let Some(conn) = dbus_inner.lock().await.as_ref() {
                                                                        let _ = conn.emit_signal(
                                                                            None::<()>,
                                                                            "/org/headbridge",
                                                                            "org.headbridge.Manager",
                                                                            "StateChanged",
                                                                            &(json,),
                                                                        ).await;
                                                                    }
                                                                }
                                                            });
                                                        }
                                                }

                                            let s = state_clone.read().await;
                                            let json = format_state_json(&s);
                                            drop(s);
                                            if let Some(conn) = dbus_conn_clone.lock().await.as_ref() {
                                                let _ = conn.emit_signal(
                                                    None::<()>,
                                                    "/org/headbridge",
                                                    "org.headbridge.Manager",
                                                    "StateChanged",
                                                    &(json,),
                                                ).await;
                                            }
                                        }
                                    }

                                    _ => {}
                                }
                            }

                            println!("Apple AAP L2CAP session ended for {}", expected_addr);
                            *active_apple_session_clone.lock().await = None;
                        });

                        (true, format!("Successfully connected to {} ({})", name, address))
                    }
                    Err(e) => {
                        let mut s = self.state.write().await;
                        s.connected = false;
                        s.connection_state = "failed".to_string();
                        s.capabilities.clear();
                        (false, format!("L2CAP connect to {} failed: {}", address, e))
                    }
                }
            }

            DeviceVendor::BowersWilkins => {
                let address = device.address.clone();
                let name = device.name.clone();

                let conn_res = Connection::system().await;
                let bw_client = if let Ok(conn) = conn_res
                    && let Ok(Some(paths)) = BluezGattClient::resolve_paths(&conn, &address).await
                        && let Ok(client) = BluezGattClient::new(conn, &paths.request_path, &paths.response_path).await {
                            Some(client)
                        } else {
                            None
                        };
                let Some(bw_client) = bw_client else {
                    let mut s = self.state.write().await;
                    s.connected = false;
                    s.connection_state = "failed".to_string();
                    return (false, format!("Unable to establish Bowers & Wilkins control session for {} ({})", name, address));
                };
                *self.active_bw_client.lock().await = Some(bw_client);

                {
                    let mut s = self.state.write().await;
                    s.connected = true;
                    s.connection_state = "connected".to_string();
                    s.device_name = name.clone();
                    s.vendor = "Bowers & Wilkins".to_string();
                    s.capabilities = capabilities_for_device(device);
                    if let Some(p) = device.battery_percent {
                        s.battery_percent = p as i32;
                    }
                }

                *self.active_address.write().await = Some(address.clone());
                self.notifications.lock().await.notify_connected(
                    &name,
                    "Bowers & Wilkins",
                    device.battery_percent.map(|b| b as i32),
                ).await;

                (true, format!("Connected to Bowers & Wilkins {} ({})", name, address))
            }

            DeviceVendor::Sony => {
                let address = device.address.clone();
                let name = device.name.clone();
                let (tx, mut rx) = mpsc::channel::<SonyMDRPacket>(64);

                match SonyMdrSession::start(&address, tx).await {
                    Ok(session) => {
                        *self.active_sony_session.lock().await = Some(session);
                        *self.active_address.write().await = Some(address.clone());

                        {
                            let mut s = self.state.write().await;
                            s.connected = true;
                            s.connection_state = "connected".to_string();
                            s.device_name = name.clone();
                            s.vendor = "Sony".to_string();
                            s.capabilities = capabilities_for_device(device);
                            if let Some(p) = device.battery_percent {
                                s.battery_percent = p as i32;
                            }
                        }

                        self.notifications.lock().await.notify_connected(
                            &name,
                            "Sony",
                            device.battery_percent.map(|b| b as i32),
                        ).await;

                        let state_clone = self.state.clone();
                        let active_addr_clone = self.active_address.clone();
                        let notif_clone = self.notifications.clone();
                        let expected_addr = address.clone();
                        let dev_name = name.clone();

                        let active_sony_session_clone = self.active_sony_session.clone();
                        tokio::spawn(async move {
                            while let Some(pkt) = rx.recv().await {
                                if active_addr_clone.read().await.as_deref() != Some(&expected_addr) {
                                    break;
                                }

                                if let Some(&opcode) = pkt.payload.first() {
                                    // Battery report (0x20 or 0x21)
                                    if opcode == 0x20 || opcode == 0x21 {
                                        if pkt.payload.len() >= 3 {
                                            let batt = pkt.payload[2] as i32;
                                            let mut s = state_clone.write().await;
                                            s.battery_percent = batt;
                                            drop(s);
                                            notif_clone.lock().await.check_battery(&dev_name, batt).await;
                                        }
                                    }
                                    // NC / Ambient report (0x22 or 0x23)
                                    else if (opcode == 0x22 || opcode == 0x23)
                                        && pkt.payload.len() >= 2 {
                                            let mut s = state_clone.write().await;
                                            s.noise_mode = match pkt.payload[1] {
                                                0x01 => "anc".to_string(),
                                                0x02 => "ambient".to_string(),
                                                _ => "off".to_string(),
                                            };
                                        }
                                }
                            }

                            println!("Sony MDR session ended for {}", expected_addr);
                            *active_sony_session_clone.lock().await = None;
                        });

                        (true, format!("Connected to Sony {} ({})", name, address))
                    }
                    Err(_) => {
                        // Fallback to basic BlueZ telemetry if RFCOMM channel connection fails
                        let mut s = self.state.write().await;
                        s.connected = true;
                        s.connection_state = "connected".to_string();
                        s.device_name = name.clone();
                        s.vendor = "Sony".to_string();
                        s.capabilities = capabilities_for_device(device);
                        if let Some(p) = device.battery_percent {
                            s.battery_percent = p as i32;
                        }
                        *self.active_address.write().await = Some(address.clone());
                        (true, format!("Connected to Sony {} ({})", name, address))
                    }
                }
            }

            DeviceVendor::Unknown => {
                (false, format!("Device {} is not supported by Resonate", device.name))
            }
        }
    }

    pub async fn run_monitoring_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_millis(2500));
        loop {
            interval.tick().await;

            // 1. Sync default PipeWire audio sink
            if let Some(sink) = AudioMonitor::get_default_sink().await {
                let mut s = self.state.write().await;
                if s.audio_sink != sink {
                    s.audio_sink = sink;
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
                    println!("Detected connected supported headphone: {} ({}) - Initiating session...", dev.name, dev.address);
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
                        DeviceVendor::Sony => self.active_sony_session.lock().await.is_some(),
                        DeviceVendor::BowersWilkins => self.active_bw_client.lock().await.is_some(),
                        _ => true,
                    };
                    if !session_alive {
                        println!("Vendor control session for {} ({}) dropped while Bluetooth connected — reconnecting...", dev.name, dev.address);
                        self.connect_vendor_session(dev).await;
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

        let proxy = zbus::Proxy::new(
            &conn,
            "org.bluez",
            path,
            "org.bluez.Device1",
        ).await?;

        let () = proxy.call("Connect", &()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(vendor: DeviceVendor, name: &str) -> PairedDevice {
        PairedDevice {
            address: "00:11:22:33:44:55".to_string(),
            name: name.to_string(),
            paired: true,
            connected: true,
            modalias: None,
            uuids: Vec::new(),
            vendor,
            model_name: None,
            battery_percent: Some(80),
        }
    }

    #[test]
    fn capabilities_are_model_aware_for_apple() {
        let caps = capabilities_for_device(&device(DeviceVendor::Apple, "AirPods Pro 2"));
        assert!(caps.iter().any(|cap| cap == "anc"));
        assert!(caps.iter().any(|cap| cap == "adaptive"));
        assert!(caps.iter().any(|cap| cap == "conversation_awareness"));
    }

    #[test]
    fn capabilities_are_vendor_specific() {
        let sony = capabilities_for_device(&device(DeviceVendor::Sony, "WH-1000XM5"));
        assert!(sony.iter().any(|cap| cap == "equalizer"));
        assert!(sony.iter().any(|cap| cap == "dsee"));
        assert!(!sony.iter().any(|cap| cap == "conversation_awareness"));
    }
}
