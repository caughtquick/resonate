use std::collections::{HashMap, HashSet};
use zbus::Connection;
use zbus::zvariant::Value;

pub struct NotificationService {
    low_battery_notified: HashMap<String, HashSet<i32>>,
    last_connected_device: Option<String>,
}

impl NotificationService {
    pub fn new() -> Self {
        Self {
            low_battery_notified: HashMap::new(),
            last_connected_device: None,
        }
    }

    pub async fn send(title: &str, body: &str, icon: &str, timeout_ms: i32) {
        if let Ok(conn) = Connection::session().await {
            let proxy_res = zbus::Proxy::new(
                &conn,
                "org.freedesktop.Notifications",
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
            ).await;

            if let Ok(proxy) = proxy_res {
                let empty_actions: Vec<&str> = Vec::new();
                let empty_hints: HashMap<&str, Value<'_>> = HashMap::new();
                let _: Result<u32, _> = proxy.call(
                    "Notify",
                    &(
                        "Resonate",
                        0u32,
                        icon,
                        title,
                        body,
                        empty_actions,
                        empty_hints,
                        timeout_ms,
                    ),
                ).await;
            }
        }
    }

    pub async fn notify_connected(&mut self, device: &str, vendor: &str, battery: Option<i32>) {
        if self.last_connected_device.as_deref() == Some(device) {
            return;
        }
        self.last_connected_device = Some(device.to_string());
        self.low_battery_notified.insert(device.to_string(), HashSet::new());

        let mut body = format!("Connected ({})", vendor);
        if let Some(pct) = battery {
            if pct >= 0 {
                body.push_str(&format!(" • {}% battery", pct));
            }
        }

        Self::send(device, &body, "audio-headphones", 4000).await;
    }

    pub async fn notify_disconnected(&mut self, device: &str) {
        if self.last_connected_device.as_deref() == Some(device) {
            self.last_connected_device = None;
            self.low_battery_notified.remove(device);
            Self::send(device, "Headphones disconnected", "audio-headphones", 3000).await;
        }
    }

    pub async fn check_battery(&mut self, device: &str, percent: i32) {
        if percent < 0 || percent > 100 {
            return;
        }
        let notified = self.low_battery_notified.entry(device.to_string()).or_default();

        if percent <= 10 && !notified.contains(&10) {
            notified.insert(10);
            Self::send(
                &format!("Low Battery: {}", device),
                &format!("Battery is down to {}%. Please recharge soon.", percent),
                "battery-caution",
                6000,
            ).await;
        } else if percent <= 20 && !notified.contains(&20) {
            notified.insert(20);
            Self::send(
                &format!("Low Battery: {}", device),
                &format!("Battery is down to {}%.", percent),
                "battery-caution",
                5000,
            ).await;
        } else if percent > 25 {
            notified.clear();
        }
    }
}
