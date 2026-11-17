//! BlueZ D-Bus GATT client integration for Bowers & Wilkins headphones.
//!
//! Bowers & Wilkins headphones (Px7 S2, Px8, etc.) communicate via custom BLE GATT services.
//! This module resolves the GATT object paths registered in BlueZ for the device and exposes
//! [`BluezGattClient`] for writing RPC requests and subscribing to notification characteristics.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use zbus::Connection;
use zbus::fdo::ObjectManagerProxy;
use zbus::proxy;
use zbus::zvariant::Value;

use crate::protocols::bw::{self, BWRPC, BWRPCCommand, BWRPCatalog};

/// RAII wrapper that aborts a spawned Tokio background task when dropped.
#[derive(Debug)]
pub struct AbortOnDrop(pub tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// D-Bus proxy trait for the standard BlueZ `org.bluez.GattCharacteristic1` interface.
#[proxy(
    default_service = "org.bluez",
    interface = "org.bluez.GattCharacteristic1"
)]
pub trait GattCharacteristic1 {
    /// Writes a value to the characteristic.
    fn write_value(&self, value: Vec<u8>, options: HashMap<String, Value<'_>>) -> zbus::Result<()>;
    /// Starts notification or indication delivery for this characteristic.
    fn start_notify(&self) -> zbus::Result<()>;
    /// Stops notification or indication delivery for this characteristic.
    fn stop_notify(&self) -> zbus::Result<()>;

    /// Reads the current value of the characteristic.
    #[zbus(property)]
    fn value(&self) -> zbus::Result<Vec<u8>>;
}

/// D-Bus proxy trait for the standard BlueZ `org.bluez.Adapter1` interface.
#[proxy(default_service = "org.bluez", interface = "org.bluez.Adapter1")]
pub trait Adapter1 {
    /// Starts Bluetooth device discovery.
    fn start_discovery(&self) -> zbus::Result<()>;
    /// Stops Bluetooth device discovery.
    fn stop_discovery(&self) -> zbus::Result<()>;
    /// Sets Bluetooth device discovery filter.
    fn set_discovery_filter(&self, properties: HashMap<String, Value<'_>>) -> zbus::Result<()>;
}

/// D-Bus proxy trait for the standard BlueZ `org.bluez.Device1` interface.
#[proxy(default_service = "org.bluez", interface = "org.bluez.Device1")]
pub trait Device1 {
    /// Connects to the remote Bluetooth device.
    fn connect(&self) -> zbus::Result<()>;
    /// Disconnects from the remote device.
    fn disconnect(&self) -> zbus::Result<()>;
    /// Whether GATT services have finished discovery on this device.
    #[zbus(property)]
    fn services_resolved(&self) -> zbus::Result<bool>;
    /// Whether the device is currently connected.
    #[zbus(property)]
    fn connected(&self) -> zbus::Result<bool>;
    /// Sets the trusted state of the device.
    #[zbus(property)]
    fn set_trusted(&self, value: bool) -> zbus::Result<()>;
}

/// Errors originating from BlueZ D-Bus GATT operations.
#[derive(Debug, thiserror::Error)]
pub enum BluezError {
    /// A zbus D-Bus communication error occurred.
    #[error("D-Bus error: {0}")]
    Dbus(#[from] zbus::Error),
    /// The required GATT characteristic path was not found in BlueZ's object tree.
    #[error("Characteristic not found: {0}")]
    CharacteristicNotFound(String),
}

/// D-Bus object paths for Bowers & Wilkins GATT characteristics.
#[derive(Debug, Clone)]
pub struct ResolvedBwPaths {
    /// Object path of the request/write characteristic (`BW_REQUEST_UUID`).
    pub request_path: String,
    /// Object path of the response/read characteristic (`BW_RESPONSE_UUID`).
    pub response_path: String,
    /// Optional object path of the asynchronous notification characteristic (`BW_NOTIFICATION_UUID`).
    pub notification_path: Option<String>,
}

/// A GATT client for Bowers & Wilkins wireless headphones interacting with BlueZ over D-Bus.
#[derive(Clone)]
pub struct BluezGattClient {
    _conn: Connection,
    req_char: GattCharacteristic1Proxy<'static>,
    /// Proxy to the GATT characteristic receiving command responses and telemetry notifications.
    pub resp_char: GattCharacteristic1Proxy<'static>,
    /// Optional proxy to the GATT characteristic receiving unsolicited device notifications.
    pub notif_char: Option<GattCharacteristic1Proxy<'static>>,
    _reader_task: Option<Arc<AbortOnDrop>>,
}

impl BluezGattClient {
    /// Discovers and resolves the D-Bus GATT characteristic object paths for a Bowers & Wilkins device.
    ///
    /// Searches `org.bluez` managed objects for GATT characteristics matching the known B&W UUIDs.
    /// If not immediately present, attempts to discover and connect the companion BLE peripheral
    /// (e.g. `LE_BWHP`) following HeadBridge's hardware-verified resolution flow.
    pub async fn resolve_paths(
        conn: &Connection,
        address: &str,
    ) -> Result<Option<ResolvedBwPaths>, zbus::Error> {
        let proxy = ObjectManagerProxy::builder(conn)
            .destination("org.bluez")?
            .path("/")?
            .build()
            .await?;

        let mut managed_objects = proxy.get_managed_objects().await?;
        let mac_segment = address.replace(':', "_").to_uppercase();

        let req_uuid = bw::BW_REQUEST_UUID.to_lowercase();
        let resp_uuid = bw::BW_RESPONSE_UUID.to_lowercase();
        let notif_uuid = bw::BW_NOTIFICATION_UUID.to_lowercase();

        // Helper to locate GATT characteristics from managed objects
        let find_chars = |objs: &std::collections::HashMap<
            zbus::zvariant::OwnedObjectPath,
            std::collections::HashMap<
                zbus::names::OwnedInterfaceName,
                std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
            >,
        >| {
            let mut req = None;
            let mut resp = None;
            let mut notif = None;

            // Pass 1: device matching mac_segment
            for (path, ifaces) in objs {
                let path_str = path.to_string();
                if !path_str.contains(&format!("dev_{}", mac_segment)) {
                    continue;
                }
                if let Some((_, gatt_props)) = ifaces
                    .iter()
                    .find(|(k, _)| k.as_str() == "org.bluez.GattCharacteristic1")
                    && let Some(uuid_val) = gatt_props.get("UUID")
                    && let Ok(u) = <&str>::try_from(uuid_val)
                {
                    let u_low = u.to_lowercase();
                    if u_low == req_uuid {
                        req = Some(path_str.clone());
                    } else if u_low == resp_uuid {
                        resp = Some(path_str.clone());
                    } else if u_low == notif_uuid {
                        notif = Some(path_str.clone());
                    }
                }
            }

            // Pass 2: any device exposing BWRPC characteristics (companion BLE device)
            if req.is_none() || resp.is_none() {
                for (path, ifaces) in objs {
                    let path_str = path.to_string();
                    if let Some((_, gatt_props)) = ifaces
                        .iter()
                        .find(|(k, _)| k.as_str() == "org.bluez.GattCharacteristic1")
                        && let Some(uuid_val) = gatt_props.get("UUID")
                        && let Ok(u) = <&str>::try_from(uuid_val)
                    {
                        let u_low = u.to_lowercase();
                        if u_low == req_uuid {
                            req = Some(path_str.clone());
                        } else if u_low == resp_uuid {
                            resp = Some(path_str.clone());
                        } else if u_low == notif_uuid {
                            notif = Some(path_str.clone());
                        }
                    }
                }
            }

            if let (Some(r), Some(s)) = (req, resp) {
                Some(ResolvedBwPaths {
                    request_path: r,
                    response_path: s,
                    notification_path: notif,
                })
            } else {
                None
            }
        };

        if let Some(paths) = find_chars(&managed_objects) {
            return Ok(Some(paths));
        }

        // Pass 3: Companion LE device resolution
        // Find default adapter
        let mut adapter_path = "/org/bluez/hci0".to_string();
        for (path, ifaces) in &managed_objects {
            if ifaces.keys().any(|k| k.as_str() == "org.bluez.Adapter1") {
                adapter_path = path.to_string();
                break;
            }
        }

        // Helper to locate a companion LE device from managed objects
        let is_bw_le_candidate = |ifaces: &std::collections::HashMap<
            zbus::names::OwnedInterfaceName,
            std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        >|
         -> bool {
            if let Some((_, dev)) = ifaces
                .iter()
                .find(|(k, _)| k.as_str() == "org.bluez.Device1")
            {
                let name = dev
                    .get("Name")
                    .and_then(|v| <&str>::try_from(v).ok())
                    .unwrap_or("")
                    .to_lowercase();
                let alias = dev
                    .get("Alias")
                    .and_then(|v| <&str>::try_from(v).ok())
                    .unwrap_or("")
                    .to_lowercase();
                let modalias = dev
                    .get("Modalias")
                    .and_then(|v| <&str>::try_from(v).ok())
                    .unwrap_or("")
                    .to_lowercase();

                if name == "le_bwhp"
                    || alias == "le_bwhp"
                    || name.contains("bwhp")
                    || alias.contains("bwhp")
                    || name.contains("bw")
                    || alias.contains("bw")
                {
                    return true;
                }
                if modalias.contains("v19b5") {
                    return true;
                }
                if name.contains("px7")
                    || alias.contains("px7")
                    || name.contains("px8")
                    || alias.contains("px8")
                    || name.contains("bowers")
                    || alias.contains("bowers")
                {
                    return true;
                }
            }
            false
        };

        let mut le_device_path: Option<String> = None;
        for (path, ifaces) in &managed_objects {
            let path_str = path.to_string();
            // Don't pick the classic audio device itself unless no other option
            if path_str.contains(&format!("dev_{}", mac_segment)) {
                continue;
            }
            if is_bw_le_candidate(ifaces) {
                le_device_path = Some(path_str);
                break;
            }
        }

        // If not found in current inventory, run a brief LE discovery scan directly via Adapter1Proxy
        if le_device_path.is_none()
            && let Ok(adapter_proxy) = Adapter1Proxy::builder(conn)
                .path(adapter_path.clone())?
                .build()
                .await
        {
            let mut filter = HashMap::new();
            filter.insert("Transport".to_string(), Value::from("le"));
            let _ = adapter_proxy.set_discovery_filter(filter).await;
            let _ = adapter_proxy.start_discovery().await;
            tokio::time::sleep(Duration::from_millis(2000)).await;
            let _ = adapter_proxy.stop_discovery().await;

            if let Ok(fresh_objects) = proxy.get_managed_objects().await {
                managed_objects = fresh_objects;
                for (path, ifaces) in &managed_objects {
                    let path_str = path.to_string();
                    if path_str.contains(&format!("dev_{}", mac_segment)) {
                        continue;
                    }
                    if is_bw_le_candidate(ifaces) {
                        le_device_path = Some(path_str);
                        break;
                    }
                }
            }
        }

        // If an LE device path was found, ensure it is trusted and connected
        if let Some(ref le_path) = le_device_path
            && let Ok(dev_proxy) = Device1Proxy::builder(conn)
                .path(le_path.clone())?
                .build()
                .await
        {
            let _ = dev_proxy.set_trusted(true).await;
            let is_connected = dev_proxy.connected().await.unwrap_or(false);
            if !is_connected {
                let _ = dev_proxy.connect().await;
            }

            // Wait up to 2 seconds for GATT services to resolve
            for _ in 0..12 {
                tokio::time::sleep(Duration::from_millis(150)).await;
                if dev_proxy.services_resolved().await.unwrap_or(false) {
                    break;
                }
            }

            if let Ok(updated_objects) = proxy.get_managed_objects().await {
                managed_objects = updated_objects;
            }
        }

        // Final attempt to find characteristics
        Ok(find_chars(&managed_objects))
    }

    /// Creates a new `BluezGattClient` by binding to the request, response, and optional notification GATT characteristics.
    pub async fn new(
        conn: Connection,
        request_char_path: &str,
        response_char_path: &str,
        notification_char_path: Option<&str>,
    ) -> Result<Self, BluezError> {
        let req_char = GattCharacteristic1Proxy::builder(&conn)
            .path(request_char_path.to_string())?
            .build()
            .await?;

        let resp_char = GattCharacteristic1Proxy::builder(&conn)
            .path(response_char_path.to_string())?
            .build()
            .await?;

        // Enable notifications on the response characteristic
        let _ = resp_char.start_notify().await;

        let notif_char = if let Some(notif_path) = notification_char_path {
            if let Ok(proxy) = GattCharacteristic1Proxy::builder(&conn)
                .path(notif_path.to_string())?
                .build()
                .await
            {
                let _ = proxy.start_notify().await;
                Some(proxy)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            _conn: conn,
            req_char,
            resp_char,
            notif_char,
            _reader_task: None,
        })
    }

    /// Attaches a reader task handle to this client, ensuring it aborts when the client is dropped.
    pub fn set_reader_task(&mut self, handle: tokio::task::JoinHandle<()>) {
        self._reader_task = Some(Arc::new(AbortOnDrop(handle)));
    }

    /// Serializes and writes a MessagePack RPC command to the request characteristic.
    pub async fn send_command<T: serde::Serialize>(
        &self,
        command: BWRPCCommand,
        payload: Option<&T>,
    ) -> Result<(), BluezError> {
        let raw = BWRPC::build_request(command, payload)
            .map_err(|e| zbus::Error::Failure(e.to_string()))?;
        let options = HashMap::new();
        self.req_char.write_value(raw, options).await?;
        Ok(())
    }

    /// Sets the ANC mode (0 = Off, 1 = Auto/Adaptive, 2 = Low, 3 = High).
    pub async fn set_anc(&self, mode: u8) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::ANC_SET, Some(&mode)).await
    }

    /// Sets the hardware bass and treble tone controls (-6 to +6 dB).
    pub async fn set_bass_treble(&self, bass: i32, treble: i32) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::BASS_SET, Some(&bass))
            .await?;
        self.send_command(BWRPCatalog::TREBLE_SET, Some(&treble))
            .await
    }

    /// Sets 5-band equalizer gains.
    pub async fn set_5band_eq(&self, bands: &[i32]) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::EQ_SET, Some(&bands.to_vec()))
            .await
    }

    /// Sets the auto-standby timer duration in minutes (0 = disabled, 15, 30, etc.).
    pub async fn set_standby(&self, minutes: i32) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::SLEEP_SET, Some(&minutes))
            .await
    }

    /// Enables or disables the wear sensor for auto-pause / auto-standby.
    pub async fn set_wear_sensor(&self, enabled: bool) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::WEAR_SET, Some(&enabled))
            .await
    }

    /// Sets the wear sensor sensitivity (0 = Low, 1 = Normal/Medium, 2 = High).
    pub async fn set_wear_sensor_sensitivity(&self, level: u8) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::WEAR_SENSITIVITY_SET, Some(&level))
            .await
    }

    /// Requests the headphone to connect to a paired multipoint device by index.
    pub async fn connect_paired_device(&self, index: u8) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::PAIRED_DEVICE_CONNECT, Some(&index))
            .await
    }

    /// Requests the headphone to disconnect from a paired multipoint device by index.
    pub async fn disconnect_paired_device(&self, index: u8) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::PAIRED_DEVICE_DISCONNECT, Some(&index))
            .await
    }

    /// Configures the function of the customizable hardware action button.
    pub async fn set_action_button(&self, mode: u8) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::BUTTON_SET, Some(&mode))
            .await
    }
}
