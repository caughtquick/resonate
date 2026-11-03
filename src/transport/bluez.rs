use std::collections::HashMap;
use zbus::Connection;
use zbus::fdo::ObjectManagerProxy;
use zbus::proxy;
use zbus::zvariant::Value;

use crate::protocols::bw::{self, BWRPCCommand, BWRPCatalog, BWRPC};

#[proxy(
    default_service = "org.bluez",
    interface = "org.bluez.GattCharacteristic1"
)]
pub trait GattCharacteristic1 {
    fn write_value(&self, value: Vec<u8>, options: HashMap<String, Value<'_>>) -> zbus::Result<()>;
    fn start_notify(&self) -> zbus::Result<()>;
    fn stop_notify(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn value(&self) -> zbus::Result<Vec<u8>>;
}

#[derive(Debug, thiserror::Error)]
pub enum BluezError {
    #[error("D-Bus error: {0}")]
    Dbus(#[from] zbus::Error),
    #[error("Characteristic not found: {0}")]
    CharacteristicNotFound(String),
}

pub struct ResolvedBwPaths {
    pub request_path: String,
    pub response_path: String,
    pub notification_path: Option<String>,
}

pub struct BluezGattClient {
    _conn: Connection,
    req_char: GattCharacteristic1Proxy<'static>,
    pub resp_char: GattCharacteristic1Proxy<'static>,
}

impl BluezGattClient {
    pub async fn resolve_paths(conn: &Connection, address: &str) -> Result<Option<ResolvedBwPaths>, zbus::Error> {
        let proxy = ObjectManagerProxy::builder(conn)
            .destination("org.bluez")?
            .path("/")?
            .build()
            .await?;

        let managed_objects = proxy.get_managed_objects().await?;
        let mac_segment = address.replace(':', "_").to_uppercase();

        let req_uuid = bw::BW_REQUEST_UUID.to_lowercase();
        let resp_uuid = bw::BW_RESPONSE_UUID.to_lowercase();
        let notif_uuid = bw::BW_NOTIFICATION_UUID.to_lowercase();

        let mut req_path = None;
        let mut resp_path = None;
        let mut notif_path = None;

        for (path, ifaces) in managed_objects {
            let path_str = path.to_string();
            // Check if matches device or companion BLE device
            if !path_str.contains(&mac_segment) && !path_str.contains("LE_BWHP") {
                continue;
            }

            if let Some(gatt_props) = ifaces.get("org.bluez.GattCharacteristic1") {
                if let Some(uuid_val) = gatt_props.get("UUID") {
                    let uuid_res: Result<&str, _> = uuid_val.try_into();
                    if let Ok(u) = uuid_res {
                        let u_low = u.to_lowercase();
                        if u_low == req_uuid {
                            req_path = Some(path_str.clone());
                        } else if u_low == resp_uuid {
                            resp_path = Some(path_str.clone());
                        } else if u_low == notif_uuid {
                            notif_path = Some(path_str.clone());
                        }
                    }
                }
            }
        }

        if let (Some(req), Some(resp)) = (req_path, resp_path) {
            Ok(Some(ResolvedBwPaths {
                request_path: req,
                response_path: resp,
                notification_path: notif_path,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn new(
        conn: Connection,
        request_char_path: &str,
        response_char_path: &str,
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

        Ok(Self {
            _conn: conn,
            req_char,
            resp_char,
        })
    }

    pub async fn send_command<T: serde::Serialize>(
        &self,
        command: BWRPCCommand,
        payload: Option<&T>,
    ) -> Result<(), BluezError> {
        let raw = BWRPC::build_request(command, payload).map_err(|e| zbus::Error::Failure(e.to_string()))?;
        let options = HashMap::new();
        self.req_char.write_value(raw, options).await?;
        Ok(())
    }

    pub async fn set_anc(&self, mode: u8) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::ANC_SET, Some(&mode)).await
    }

    pub async fn set_bass_treble(&self, bass: i32, treble: i32) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::BASS_SET, Some(&bass)).await?;
        self.send_command(BWRPCatalog::TREBLE_SET, Some(&treble)).await
    }

    pub async fn set_standby(&self, minutes: i32) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::SLEEP_SET, Some(&minutes)).await
    }

    pub async fn set_wear_sensor(&self, enabled: bool) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::WEAR_SET, Some(&enabled)).await
    }

    pub async fn set_action_button(&self, mode: u8) -> Result<(), BluezError> {
        self.send_command(BWRPCatalog::BUTTON_SET, Some(&mode)).await
    }
}
