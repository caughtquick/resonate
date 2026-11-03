use std::sync::Arc;
use zbus::interface;

use crate::manager::DeviceManager;

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
    pub auto_pause: bool,
    pub in_ear: bool,
    pub in_ear_primary: String,
    pub in_ear_secondary: String,
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
    manager: Arc<DeviceManager>,
}

impl ResonateDbusManager {
    pub fn new(manager: Arc<DeviceManager>) -> Self {
        Self { manager }
    }
}

#[interface(name = "org.headbridge.Manager")]
impl ResonateDbusManager {
    #[zbus(property)]
    async fn connected(&self) -> bool {
        self.manager.state.read().await.connected
    }

    #[zbus(property)]
    async fn connection_state(&self) -> String {
        self.manager.state.read().await.connection_state.clone()
    }

    #[zbus(property)]
    async fn device_name(&self) -> String {
        self.manager.state.read().await.device_name.clone()
    }

    #[zbus(property)]
    async fn vendor(&self) -> String {
        self.manager.state.read().await.vendor.clone()
    }

    #[zbus(property)]
    async fn battery_percent(&self) -> i32 {
        self.manager.state.read().await.battery_percent
    }

    #[zbus(property)]
    async fn battery_left(&self) -> i32 {
        self.manager.state.read().await.battery_left
    }

    #[zbus(property)]
    async fn battery_right(&self) -> i32 {
        self.manager.state.read().await.battery_right
    }

    #[zbus(property)]
    async fn battery_case(&self) -> i32 {
        self.manager.state.read().await.battery_case
    }

    #[zbus(property)]
    async fn is_charging(&self) -> bool {
        self.manager.state.read().await.is_charging
    }

    #[zbus(property)]
    async fn noise_mode(&self) -> String {
        self.manager.state.read().await.noise_mode.clone()
    }

    #[zbus(property)]
    async fn ear_detection(&self) -> bool {
        self.manager.state.read().await.ear_detection
    }

    #[zbus(property)]
    async fn auto_pause(&self) -> bool {
        self.manager.state.read().await.auto_pause
    }

    #[zbus(property)]
    async fn in_ear(&self) -> bool {
        self.manager.state.read().await.in_ear
    }

    #[zbus(property)]
    async fn in_ear_primary(&self) -> String {
        self.manager.state.read().await.in_ear_primary.clone()
    }

    #[zbus(property)]
    async fn in_ear_secondary(&self) -> String {
        self.manager.state.read().await.in_ear_secondary.clone()
    }

    #[zbus(property)]
    async fn conversation_awareness(&self) -> bool {
        self.manager.state.read().await.conv_aware
    }

    #[zbus(property)]
    async fn microphone_mode(&self) -> String {
        self.manager.state.read().await.mic_mode.clone()
    }

    #[zbus(property)]
    async fn one_bud_anc(&self) -> bool {
        self.manager.state.read().await.one_bud_anc
    }

    #[zbus(property)]
    async fn chime_volume(&self) -> i32 {
        self.manager.state.read().await.chime_volume
    }

    #[zbus(property)]
    async fn standby_minutes(&self) -> i32 {
        self.manager.state.read().await.standby_minutes
    }

    #[zbus(property)]
    async fn touch_sensor(&self) -> bool {
        self.manager.state.read().await.touch_sensor
    }

    #[zbus(property)]
    async fn dsee(&self) -> bool {
        self.manager.state.read().await.dsee
    }

    #[zbus(property)]
    async fn equalizer_preset(&self) -> String {
        self.manager.state.read().await.equalizer_preset.clone()
    }

    #[zbus(property)]
    async fn audio_sink(&self) -> String {
        self.manager.state.read().await.audio_sink.clone()
    }

    async fn get_status(&self) -> String {
        self.manager.get_status_json().await
    }

    async fn list_devices(&self) -> String {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.list_devices_json().await
        }).await.unwrap_or_else(|_| "[]".to_string())
    }

    async fn connect(&self, target: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.connect_device(&target).await
        }).await.unwrap_or_else(|e| (false, format!("Tokio runtime spawn error: {}", e)))
    }

    async fn disconnect(&self) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.disconnect_device().await
        }).await.unwrap_or(false)
    }

    async fn set_anc(&self, mode: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_anc(&mode).await
        }).await.unwrap_or_else(|e| (false, format!("Tokio runtime spawn error: {}", e)))
    }

    async fn set_ambient_level(&self, level: i32) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_ambient_level(level).await
        }).await.unwrap_or(false)
    }

    async fn set_equalizer(&self, args: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_equalizer(&args).await
        }).await.unwrap_or_else(|e| (false, format!("Error: {}", e)))
    }

    async fn set_wear_sensor(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_ear_detection(enable).await
        }).await.unwrap_or(false)
    }

    async fn set_auto_pause(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_auto_pause(enable).await
        }).await.unwrap_or(false)
    }

    async fn get_auto_pause(&self) -> bool {
        self.manager.state.read().await.auto_pause
    }

    async fn set_conversation_awareness(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_conversation_awareness(enable).await
        }).await.unwrap_or(false)
    }

    async fn set_microphone_mode(&self, mode: String) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_mic_mode(&mode).await
        }).await.unwrap_or(false)
    }

    async fn set_one_bud_anc(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_one_bud_anc(enable).await
        }).await.unwrap_or(false)
    }

    async fn set_chime_volume(&self, vol: u8) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_chime_volume(vol).await
        }).await.unwrap_or(false)
    }

    async fn set_touch_sensor(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_touch_sensor(enable).await
        }).await.unwrap_or(false)
    }

    async fn set_dsee(&self, enable: bool) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_dsee(enable).await
        }).await.unwrap_or(false)
    }

    async fn set_standby(&self, val: String) -> bool {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_standby(&val).await
        }).await.unwrap_or(false)
    }

    async fn set_action_button(&self, action: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.set_action_button(&action).await
        }).await.unwrap_or_else(|e| (false, format!("Error: {}", e)))
    }

    async fn optimize(&self, action: String) -> (bool, String) {
        let mgr = self.manager.clone();
        self.manager.handle.spawn(async move {
            mgr.optimize(&action).await
        }).await.unwrap_or_else(|e| (false, format!("Error: {}", e)))
    }
}
