use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock};
use zbus::Connection;

use crate::dbus_service::ResonateState;
use crate::inventory::{BluetoothInventory, DeviceVendor, PairedDevice};
use crate::protocols::aap::{self, BatteryKind, BatteryStatus, EarStatus, Frame, ListeningMode, MicSelection, Opcode, SettingId, SettingNotice};
use crate::transport::l2cap::AppleL2capSession;

pub struct DeviceManager {
    pub state: Arc<RwLock<ResonateState>>,
    active_apple_session: Arc<Mutex<Option<AppleL2capSession>>>,
    active_address: Arc<RwLock<Option<String>>>,
    pub handle: tokio::runtime::Handle,
}

impl DeviceManager {
    pub fn new(state: Arc<RwLock<ResonateState>>, handle: tokio::runtime::Handle) -> Self {
        Self {
            state,
            active_apple_session: Arc::new(Mutex::new(None)),
            active_address: Arc::new(RwLock::new(None)),
            handle,
        }
    }

    pub async fn get_status_json(&self) -> String {
        let s = self.state.read().await;
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
            "conversationAwareness": s.conv_aware,
            "micMode": s.mic_mode,
            "oneBudAnc": s.one_bud_anc,
            "chimeVolume": s.chime_volume,
        })).unwrap_or_else(|_| "{}".to_string())
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

        if let Some(current) = self.active_address.read().await.as_deref() {
            if current == device.address && self.state.read().await.connected {
                return (true, format!("Already connected to {} ({})", device.name, device.address));
            }
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
        let mut session = self.active_apple_session.lock().await;
        *session = None;
        *self.active_address.write().await = None;

        let mut s = self.state.write().await;
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

        true
    }

    pub async fn set_anc(&self, mode_str: &str) -> (bool, String) {
        let mode_norm = mode_str.to_lowercase();
        let session_guard = self.active_apple_session.lock().await;
        if let Some(session) = session_guard.as_ref() {
            let mode = match mode_norm.as_str() {
                "off" => ListeningMode::Off,
                "anc" | "nc" | "on" => ListeningMode::NoiseCancellation,
                "ambient" | "transparency" => ListeningMode::Transparency,
                "adaptive" => ListeningMode::Adaptive,
                other => return (false, format!("Unsupported ANC mode: '{}'. Use off, anc, ambient, or adaptive.", other)),
            };

            if let Err(e) = session.set_noise_mode(mode).await {
                return (false, format!("Failed to send ANC command: {}", e));
            }

            let mut s = self.state.write().await;
            s.noise_mode = mode_norm.clone();
            return (true, format!("ANC mode set to {}", mode_norm));
        }

        (false, "No active headphone session connected".to_string())
    }

    pub async fn set_conversation_awareness(&self, enabled: bool) -> bool {
        let session_guard = self.active_apple_session.lock().await;
        if let Some(session) = session_guard.as_ref() {
            if session.set_conversation_awareness(enabled).await.is_ok() {
                self.state.write().await.conv_aware = enabled;
                return true;
            }
        }
        false
    }

    pub async fn set_ear_detection(&self, enabled: bool) -> bool {
        let session_guard = self.active_apple_session.lock().await;
        if let Some(session) = session_guard.as_ref() {
            if session.set_ear_detection(enabled).await.is_ok() {
                self.state.write().await.ear_detection = enabled;
                return true;
            }
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
        if let Some(session) = session_guard.as_ref() {
            if session.set_mic_mode(mode).await.is_ok() {
                self.state.write().await.mic_mode = mode_str.to_string();
                return true;
            }
        }
        false
    }

    pub async fn set_one_bud_anc(&self, enabled: bool) -> bool {
        let session_guard = self.active_apple_session.lock().await;
        if let Some(session) = session_guard.as_ref() {
            if session.set_one_bud_anc(enabled).await.is_ok() {
                self.state.write().await.one_bud_anc = enabled;
                return true;
            }
        }
        false
    }

    pub async fn set_chime_volume(&self, vol: u8) -> bool {
        let session_guard = self.active_apple_session.lock().await;
        if let Some(session) = session_guard.as_ref() {
            if session.set_chime_volume(vol).await.is_ok() {
                self.state.write().await.chime_volume = vol as i32;
                return true;
            }
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
                }

                match AppleL2capSession::start(&address, tx).await {
                    Ok(session) => {
                        *self.active_apple_session.lock().await = Some(session);
                        *self.active_address.write().await = Some(address.clone());

                        {
                            let mut s = self.state.write().await;
                            s.connected = true;
                            s.connection_state = "connected".to_string();
                            s.device_name = name.clone();
                            s.vendor = "Apple".to_string();
                            if let Some(p) = device.battery_percent {
                                s.battery_percent = p as i32;
                            }
                        }

                        // Spawn frame handling loop
                        let state_clone = self.state.clone();
                        let active_addr_clone = self.active_address.clone();
                        let expected_addr = address.clone();

                        tokio::spawn(async move {
                            while let Some(frame) = rx.recv().await {
                                // Check if we are still connected to this device
                                if active_addr_clone.read().await.as_deref() != Some(&expected_addr) {
                                    break;
                                }

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
                                                    if c.status != BatteryStatus::Disconnected {
                                                        s.battery_case = c.level as i32;
                                                    }
                                                }
                                                BatteryKind::Single => {
                                                    s.battery_percent = c.level as i32;
                                                }
                                            }
                                        }

                                        // Compute overall battery percent
                                        if s.battery_percent < 0 {
                                            if s.battery_right >= 0 && s.battery_left >= 0 {
                                                s.battery_percent = s.battery_right.min(s.battery_left);
                                            } else if s.battery_right >= 0 {
                                                s.battery_percent = s.battery_right;
                                            } else if s.battery_left >= 0 {
                                                s.battery_percent = s.battery_left;
                                            }
                                        }
                                    }

                                    Some(Opcode::Settings) => {
                                        if let Some(notice) = SettingNotice::parse(&frame) {
                                            if let (Some(setting_id), Some(&val)) = (SettingId::from_u8(notice.id), notice.data.first()) {
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
                                                    }
                                                    SettingId::ConvAware => {
                                                        s.conv_aware = val == 1;
                                                    }
                                                    SettingId::OneBudAnc => {
                                                        s.one_bud_anc = val == 1;
                                                    }
                                                    SettingId::ChimeVolume => {
                                                        s.chime_volume = val as i32;
                                                    }
                                                    SettingId::MicMode => {
                                                        s.mic_mode = match val {
                                                            0 => "auto".to_string(),
                                                            1 => "right".to_string(),
                                                            2 => "left".to_string(),
                                                            _ => "auto".to_string(),
                                                        };
                                                    }
                                                    SettingId::EarDetection => {
                                                        s.ear_detection = val == 1;
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }

                                    Some(Opcode::EarDetection) => {
                                        if let Some(in_ear) = aap::parse_in_ear(&frame.payload) {
                                            let mut s = state_clone.write().await;
                                            s.ear_detection = in_ear.primary == EarStatus::InEar || in_ear.secondary == EarStatus::InEar;
                                        }
                                    }

                                    _ => {}
                                }
                            }
                        });

                        (true, format!("Successfully connected to {} ({})", name, address))
                    }
                    Err(e) => {
                        let mut s = self.state.write().await;
                        s.connected = false;
                        s.connection_state = "failed".to_string();
                        (false, format!("L2CAP connect to {} failed: {}", address, e))
                    }
                }
            }

            DeviceVendor::BowersWilkins => {
                let mut s = self.state.write().await;
                s.connected = true;
                s.connection_state = "connected".to_string();
                s.device_name = device.name.clone();
                s.vendor = "Bowers & Wilkins".to_string();
                if let Some(p) = device.battery_percent {
                    s.battery_percent = p as i32;
                }
                *self.active_address.write().await = Some(device.address.clone());
                (true, format!("Connected to Bowers & Wilkins {} ({})", device.name, device.address))
            }

            DeviceVendor::Sony => {
                let mut s = self.state.write().await;
                s.connected = true;
                s.connection_state = "connected".to_string();
                s.device_name = device.name.clone();
                s.vendor = "Sony".to_string();
                if let Some(p) = device.battery_percent {
                    s.battery_percent = p as i32;
                }
                *self.active_address.write().await = Some(device.address.clone());
                (true, format!("Connected to Sony {} ({})", device.name, device.address))
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

            let devices = match BluetoothInventory::fetch_paired_devices().await {
                Ok(d) => d,
                Err(_) => continue,
            };

            let current_addr = self.active_address.read().await.clone();

            // Find if any supported device is connected in BlueZ
            let connected_device = devices.iter().find(|d| d.connected && d.is_supported());

            match (current_addr, connected_device) {
                (None, Some(dev)) => {
                    // Auto-connect to the active Bluetooth headphone!
                    println!("Detected connected supported headphone: {} ({}) - Initiating session...", dev.name, dev.address);
                    self.connect_vendor_session(dev).await;
                }
                (Some(addr), Some(dev)) if dev.address != addr => {
                    // Headphone switched! Disconnect old, connect new
                    println!("Headphone switched to: {} ({})", dev.name, dev.address);
                    self.disconnect_device().await;
                    self.connect_vendor_session(dev).await;
                }
                (Some(addr), None) => {
                    // Headphone disconnected in BlueZ
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
