use std::sync::Arc;
use tokio::sync::RwLock;
use zbus::interface;

#[derive(Debug, Clone, Default)]
pub struct ResonateState {
    pub connected: bool,
    pub connection_state: String,
    pub device_name: String,
    pub vendor: String,
    pub battery_percent: i32,
    pub battery_left: i32,
    pub battery_right: i32,
    pub battery_case: i32,
    pub is_charging: bool,
    pub noise_mode: String,
    pub ear_detection: bool,
    pub conv_aware: bool,
    pub mic_mode: String,
    pub one_bud_anc: bool,
    pub chime_volume: i32,
    pub standby_minutes: i32,
    pub touch_sensor: bool,
    pub dsee: bool,
    pub equalizer_preset: String,
    pub audio_sink: String,
}

pub struct ResonateDbusManager {
    state: Arc<RwLock<ResonateState>>,
}

impl ResonateDbusManager {
    pub fn new(state: Arc<RwLock<ResonateState>>) -> Self {
        Self { state }
    }
}

#[interface(name = "org.headbridge.Manager")]
impl ResonateDbusManager {
    #[zbus(property)]
    async fn connected(&self) -> bool {
        self.state.read().await.connected
    }

    #[zbus(property)]
    async fn connection_state(&self) -> String {
        self.state.read().await.connection_state.clone()
    }

    #[zbus(property)]
    async fn device_name(&self) -> String {
        self.state.read().await.device_name.clone()
    }

    #[zbus(property)]
    async fn vendor(&self) -> String {
        self.state.read().await.vendor.clone()
    }

    #[zbus(property)]
    async fn battery_percent(&self) -> i32 {
        self.state.read().await.battery_percent
    }

    #[zbus(property)]
    async fn battery_left(&self) -> i32 {
        self.state.read().await.battery_left
    }

    #[zbus(property)]
    async fn battery_right(&self) -> i32 {
        self.state.read().await.battery_right
    }

    #[zbus(property)]
    async fn battery_case(&self) -> i32 {
        self.state.read().await.battery_case
    }

    #[zbus(property)]
    async fn is_charging(&self) -> bool {
        self.state.read().await.is_charging
    }

    #[zbus(property)]
    async fn noise_mode(&self) -> String {
        self.state.read().await.noise_mode.clone()
    }

    #[zbus(property)]
    async fn ear_detection(&self) -> bool {
        self.state.read().await.ear_detection
    }

    #[zbus(property)]
    async fn conversation_awareness(&self) -> bool {
        self.state.read().await.conv_aware
    }

    #[zbus(property)]
    async fn microphone_mode(&self) -> String {
        self.state.read().await.mic_mode.clone()
    }

    #[zbus(property)]
    async fn one_bud_anc(&self) -> bool {
        self.state.read().await.one_bud_anc
    }

    #[zbus(property)]
    async fn chime_volume(&self) -> i32 {
        self.state.read().await.chime_volume
    }

    #[zbus(property)]
    async fn standby_minutes(&self) -> i32 {
        self.state.read().await.standby_minutes
    }

    #[zbus(property)]
    async fn touch_sensor(&self) -> bool {
        self.state.read().await.touch_sensor
    }

    #[zbus(property)]
    async fn dsee(&self) -> bool {
        self.state.read().await.dsee
    }

    #[zbus(property)]
    async fn equalizer_preset(&self) -> String {
        self.state.read().await.equalizer_preset.clone()
    }

    #[zbus(property)]
    async fn audio_sink(&self) -> String {
        self.state.read().await.audio_sink.clone()
    }

    async fn get_status(&self) -> String {
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
        })).unwrap_or_else(|_| "{}".to_string())
    }

    async fn list_devices(&self) -> String {
        "[]".to_string()
    }

    async fn connect(&self, _target: String) -> (bool, String) {
        (true, "Connecting...".to_string())
    }

    async fn disconnect(&self) -> bool {
        let mut s = self.state.write().await;
        s.connected = false;
        s.connection_state = "idle".to_string();
        true
    }

    async fn set_anc(&self, mode: String) -> (bool, String) {
        let mut s = self.state.write().await;
        s.noise_mode = mode.clone();
        (true, format!("ANC mode set to: {}", mode))
    }

    async fn set_ambient_level(&self, _level: i32) -> bool {
        true
    }

    async fn set_equalizer(&self, _args: String) -> (bool, String) {
        (true, "Equalizer set".to_string())
    }

    async fn set_wear_sensor(&self, enable: bool) -> bool {
        let mut s = self.state.write().await;
        s.ear_detection = enable;
        true
    }

    async fn set_conversation_awareness(&self, enable: bool) -> bool {
        let mut s = self.state.write().await;
        s.conv_aware = enable;
        true
    }

    async fn set_microphone_mode(&self, mode: String) -> bool {
        let mut s = self.state.write().await;
        s.mic_mode = mode;
        true
    }

    async fn set_one_bud_anc(&self, enable: bool) -> bool {
        let mut s = self.state.write().await;
        s.one_bud_anc = enable;
        true
    }

    async fn set_touch_sensor(&self, enable: bool) -> bool {
        let mut s = self.state.write().await;
        s.touch_sensor = enable;
        true
    }

    async fn set_dsee(&self, enable: bool) -> bool {
        let mut s = self.state.write().await;
        s.dsee = enable;
        true
    }

    async fn set_standby(&self, _val: String) -> bool {
        true
    }

    async fn set_action_button(&self, _action: String) -> (bool, String) {
        (true, "Action button configured".to_string())
    }

    async fn optimize(&self, _action: String) -> (bool, String) {
        (true, "Optimizer calibration finished".to_string())
    }
}
