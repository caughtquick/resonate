//! Asynchronous D-Bus client talking to `org.headbridge.Manager`.

use serde::Deserialize;
use std::sync::Arc;
use zbus::{Connection, Proxy};

/// Strongly-typed deserialization of the status snapshot from `GetStatus` or `StateChanged`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResonateStatusDto {
    #[serde(default)]
    pub connected: bool,
    #[serde(rename = "connectionState", default)]
    pub connection_state: String,
    #[serde(rename = "deviceName", default)]
    pub device_name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(rename = "batteryPercent")]
    pub battery_percent: Option<i32>,
    #[serde(rename = "batteryLeft")]
    pub battery_left: Option<i32>,
    #[serde(rename = "batteryRight")]
    pub battery_right: Option<i32>,
    #[serde(rename = "batteryCase")]
    pub battery_case: Option<i32>,
    #[serde(rename = "isCharging", default)]
    pub is_charging: bool,
    #[serde(rename = "noiseMode", default)]
    pub noise_mode: String,
    #[serde(rename = "audioSink", default)]
    pub audio_sink: String,
    #[serde(rename = "earDetection", default)]
    pub ear_detection: bool,
    #[serde(rename = "autoPause", default)]
    pub auto_pause: bool,
    #[serde(rename = "inEar", default)]
    pub in_ear: bool,
    #[serde(rename = "inEarPrimary", default)]
    pub in_ear_primary: String,
    #[serde(rename = "inEarSecondary", default)]
    pub in_ear_secondary: String,
    #[serde(rename = "wearSensitivity")]
    pub wear_sensitivity: Option<i32>,
    #[serde(rename = "conversationAwareness", default)]
    pub conversation_awareness: bool,
    #[serde(rename = "micMode", default)]
    pub mic_mode: String,
    #[serde(rename = "oneBudAnc", default)]
    pub one_bud_anc: bool,
    #[serde(rename = "chimeVolume", default)]
    pub chime_volume: i32,
    #[serde(rename = "standbyMinutes", default)]
    pub standby_minutes: i32,
    #[serde(rename = "actionButton", default = "default_action_button")]
    pub action_button: String,
    #[serde(rename = "equalizerPreset", default)]
    pub equalizer_preset: String,
}

fn default_action_button() -> String {
    "anc".to_string()
}

/// A paired multipoint slot reported by Bowers & Wilkins headphones over BWRPC.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BwMultipointDeviceDto {
    pub id: usize,
    pub name: String,
    pub address: String,
    pub connected: bool,
}

/// A paired device reported by BlueZ through Resonate.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PairedDeviceDto {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub vendor: Option<serde_json::Value>,
    #[serde(default)]
    pub connected: bool,
}

impl PairedDeviceDto {
    /// Formats the vendor name for UI display.
    pub fn vendor_display(&self) -> String {
        match &self.vendor {
            Some(serde_json::Value::String(s)) => match s.as_str() {
                "Apple" => "Apple".to_string(),
                "BowersWilkins" | "Bowers & Wilkins" => "Bowers & Wilkins".to_string(),
                other => other.to_string(),
            },
            _ => "Bluetooth".to_string(),
        }
    }

    /// Returns true if this device is recognized as a supported headphone (Apple or Bowers & Wilkins).
    pub fn is_supported(&self) -> bool {
        match &self.vendor {
            Some(serde_json::Value::String(s)) => {
                s == "Apple" || s == "BowersWilkins" || s == "Bowers & Wilkins"
            }
            _ => false,
        }
    }
}

/// Helper that parses bass and treble dB values from an equalizer string like `"Bass: +2.0 dB, Treble: -1.0 dB"`.
pub fn parse_eq_gains(preset_str: &str) -> (f32, f32) {
    let mut bass = 0.0f32;
    let mut treble = 0.0f32;

    for part in preset_str.split(',') {
        let trimmed = part.trim();
        if trimmed.to_lowercase().starts_with("bass:") {
            if let Some(val_str) = trimmed.strip_prefix("Bass:") {
                let clean = val_str.replace("dB", "").replace('+', "").trim().to_string();
                if let Ok(val) = clean.parse::<f32>() {
                    bass = val;
                }
            }
        } else if trimmed.to_lowercase().starts_with("treble:") {
            if let Some(val_str) = trimmed.strip_prefix("Treble:") {
                let clean = val_str.replace("dB", "").replace('+', "").trim().to_string();
                if let Ok(val) = clean.parse::<f32>() {
                    treble = val;
                }
            }
        }
    }

    (bass, treble)
}

/// D-Bus client proxy wrapper.
#[derive(Clone)]
pub struct ResonateDbusClient {
    pub conn: Arc<Connection>,
    proxy: Proxy<'static>,
}

impl ResonateDbusClient {
    /// Connects to the session D-Bus and attaches to `org.headbridge.Manager`.
    pub async fn connect() -> Result<Self, zbus::Error> {
        let conn = Connection::session().await?;
        let proxy = Proxy::new(
            &conn,
            "org.headbridge",
            "/org/headbridge",
            "org.headbridge.Manager",
        )
        .await?;

        Ok(Self {
            conn: Arc::new(conn),
            proxy,
        })
    }

    /// Queries the full status snapshot as a deserialized DTO.
    pub async fn get_status(&self) -> Result<ResonateStatusDto, zbus::Error> {
        let json_str: String = self.proxy.call("GetStatus", &()).await?;
        serde_json::from_str(&json_str).map_err(|e| {
            zbus::Error::Failure(format!("Failed to parse status JSON: {}", e))
        })
    }

    /// Queries the list of paired Bluetooth headphones.
    pub async fn list_devices(&self) -> Result<Vec<PairedDeviceDto>, zbus::Error> {
        let json_str: String = self.proxy.call("ListDevices", &()).await?;
        serde_json::from_str(&json_str).map_err(|e| {
            zbus::Error::Failure(format!("Failed to parse device list JSON: {}", e))
        })
    }

    /// Sets the active noise control mode.
    pub async fn set_anc(&self, mode: &str) -> Result<(bool, String), zbus::Error> {
        self.proxy.call("SetAnc", &(mode,)).await
    }

    /// Sets the smart auto-pause toggle.
    pub async fn set_auto_pause(&self, enable: bool) -> Result<bool, zbus::Error> {
        self.proxy.call("SetAutoPause", &(enable,)).await
    }

    /// Sets Conversation Awareness.
    pub async fn set_conversation_awareness(&self, enable: bool) -> Result<bool, zbus::Error> {
        self.proxy.call("SetConversationAwareness", &(enable,)).await
    }

    /// Sets single-bud ANC permission.
    pub async fn set_one_bud_anc(&self, enable: bool) -> Result<bool, zbus::Error> {
        self.proxy.call("SetOneBudAnc", &(enable,)).await
    }

    /// Enables or disables in-ear wear sensors.
    pub async fn set_wear_sensor(&self, enable: bool) -> Result<bool, zbus::Error> {
        self.proxy.call("SetWearSensor", &(enable,)).await
    }

    /// Sets equalizer parameters or preset string.
    pub async fn set_equalizer(&self, args: &str) -> Result<(bool, String), zbus::Error> {
        self.proxy.call("SetEqualizer", &(args,)).await
    }

    /// Configures active microphone earbud selection.
    pub async fn set_microphone_mode(&self, mode: &str) -> Result<bool, zbus::Error> {
        self.proxy.call("SetMicrophoneMode", &(mode,)).await
    }

    /// Sets case speaker and bud alert chime volume (`0..=100`).
    pub async fn set_chime_volume(&self, vol: u8) -> Result<bool, zbus::Error> {
        self.proxy.call("SetChimeVolume", &(vol,)).await
    }

    /// Configures auto-standby / sleep timer.
    pub async fn set_standby(&self, val: &str) -> Result<bool, zbus::Error> {
        self.proxy.call("SetStandby", &(val,)).await
    }

    /// Remaps the physical custom action button.
    pub async fn set_action_button(&self, action: &str) -> Result<(bool, String), zbus::Error> {
        self.proxy.call("SetActionButton", &(action,)).await
    }

    /// Connects to a device by MAC address or name substring.
    pub async fn connect_device(&self, target: &str) -> Result<(bool, String), zbus::Error> {
        self.proxy.call("Connect", &(target,)).await
    }

    /// Disconnects the active headphone session.
    pub async fn disconnect_device(&self) -> Result<bool, zbus::Error> {
        self.proxy.call("Disconnect", &()).await
    }

    /// Connects a paired multipoint device by index, MAC address, or name.
    pub async fn connect_multipoint(&self, target: &str) -> Result<(bool, String), zbus::Error> {
        self.proxy.call("ConnectMultipoint", &(target,)).await
    }

    /// Disconnects a paired multipoint device by index, MAC address, or name.
    pub async fn disconnect_multipoint(&self, target: &str) -> Result<(bool, String), zbus::Error> {
        self.proxy.call("DisconnectMultipoint", &(target,)).await
    }

    /// Queries paired multipoint slots from Bowers & Wilkins headphones over BWRPC.
    pub async fn get_multipoint(&self) -> Result<(bool, String), zbus::Error> {
        self.proxy.call("GetMultipoint", &()).await
    }

    /// Configures wear sensor state or sensitivity ("on", "off", "low", "medium", "high").
    pub async fn set_wear_sensor_string(&self, action: &str) -> Result<(bool, String), zbus::Error> {
        self.proxy.call("SetWearSensorString", &(action,)).await
    }

    /// Subscribes to the `StateChanged(s)` signal stream.
    pub async fn subscribe_state_changed(&self) -> Result<zbus::proxy::SignalStream<'static>, zbus::Error> {
        self.proxy.receive_signal("StateChanged").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_eq_gains() {
        let (b, t) = parse_eq_gains("Bass: +3.0 dB, Treble: -1.5 dB");
        assert_eq!(b, 3.0);
        assert_eq!(t, -1.5);

        let (b2, t2) = parse_eq_gains("Flat");
        assert_eq!(b2, 0.0);
        assert_eq!(t2, 0.0);
    }

    #[tokio::test]
    async fn test_dbus_client_live_query() {
        if let Ok(client) = ResonateDbusClient::connect().await {
            if let Ok(status) = client.get_status().await {
                assert!(!status.device_name.is_empty() || !status.connected);
            }
        }
    }
}
