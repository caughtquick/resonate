use std::collections::HashMap;
use zbus::proxy;
use zbus::zvariant::Value;

use crate::protocols::bw::{BWRPCCommand, BWRPC};

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

pub struct BluezGattClient<'a> {
    req_char: GattCharacteristic1Proxy<'a>,
    resp_char: GattCharacteristic1Proxy<'a>,
}

impl<'a> BluezGattClient<'a> {
    pub async fn new(
        conn: &'a zbus::Connection,
        request_char_path: &'a str,
        response_char_path: &'a str,
    ) -> Result<Self, BluezError> {
        let req_char = GattCharacteristic1Proxy::builder(conn)
            .path(request_char_path)?
            .build()
            .await?;

        let resp_char = GattCharacteristic1Proxy::builder(conn)
            .path(response_char_path)?
            .build()
            .await?;

        // Start listening to notifications
        resp_char.start_notify().await?;

        Ok(Self {
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
}
