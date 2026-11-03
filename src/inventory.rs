use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use zbus::Connection;
use zbus::fdo::ObjectManagerProxy;
use zbus::zvariant::OwnedValue;

use crate::protocols::apple_models::AppleModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceVendor {
    Apple,
    BowersWilkins,
    Sony,
    Unknown,
}

impl std::fmt::Display for DeviceVendor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceVendor::Apple => write!(f, "Apple"),
            DeviceVendor::BowersWilkins => write!(f, "Bowers & Wilkins"),
            DeviceVendor::Sony => write!(f, "Sony"),
            DeviceVendor::Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedDevice {
    pub address: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    pub modalias: Option<String>,
    pub uuids: Vec<String>,
    pub vendor: DeviceVendor,
    pub model_name: Option<String>,
    pub battery_percent: Option<u8>,
}

impl PairedDevice {
    pub fn is_supported(&self) -> bool {
        self.vendor != DeviceVendor::Unknown
    }
}

pub struct BluetoothInventory;

impl BluetoothInventory {
    pub async fn fetch_paired_devices() -> Result<Vec<PairedDevice>, Box<dyn std::error::Error + Send + Sync>> {
        let conn = Connection::system().await?;
        let proxy = ObjectManagerProxy::builder(&conn)
            .destination("org.bluez")?
            .path("/")?
            .build()
            .await?;

        let managed_objects = proxy.get_managed_objects().await?;
        let mut devices = Vec::new();

        for (_path, interfaces) in managed_objects {
            if let Some(dev_props) = interfaces.get("org.bluez.Device1") {
                let paired = get_bool_prop(dev_props, "Paired").unwrap_or(false);
                if !paired {
                    continue;
                }

                let address = match get_string_prop(dev_props, "Address") {
                    Some(a) => a,
                    None => continue,
                };

                let name = get_string_prop(dev_props, "Name")
                    .or_else(|| get_string_prop(dev_props, "Alias"))
                    .unwrap_or_else(|| address.clone());

                let connected = get_bool_prop(dev_props, "Connected").unwrap_or(false);
                let modalias = get_string_prop(dev_props, "Modalias");
                let uuids = get_string_vec_prop(dev_props, "UUIDs").unwrap_or_default();

                let mut battery_percent = None;
                if let Some(batt_props) = interfaces.get("org.bluez.Battery1")
                    && let Some(p) = get_u8_prop(batt_props, "Percentage") {
                        battery_percent = Some(p);
                    }

                let vendor = identify_vendor(&name, modalias.as_deref(), &uuids);
                let mut model_name = None;

                if vendor == DeviceVendor::Apple {
                    let apple_model = if let Some(m) = modalias.as_deref() {
                        let model = AppleModel::from_modalias(m);
                        if model != AppleModel::Unknown {
                            model
                        } else {
                            AppleModel::from_name(&name)
                        }
                    } else {
                        AppleModel::from_name(&name)
                    };
                    model_name = Some(apple_model.display_name().to_string());
                } else if vendor == DeviceVendor::BowersWilkins || vendor == DeviceVendor::Sony {
                    model_name = Some(name.clone());
                }

                devices.push(PairedDevice {
                    address: address.to_uppercase(),
                    name,
                    paired,
                    connected,
                    modalias,
                    uuids,
                    vendor,
                    model_name,
                    battery_percent,
                });
            }
        }

        // Sort: connected first, then supported first, then alphabetical by name
        devices.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then_with(|| b.is_supported().cmp(&a.is_supported()))
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(devices)
    }
}

pub fn identify_vendor(name: &str, modalias: Option<&str>, uuids: &[String]) -> DeviceVendor {
    let lower_name = name.to_lowercase();
    let lower_modalias = modalias.map(|m| m.to_lowercase()).unwrap_or_default();

    // 1. Apple
    // Vendor ID 0x004C
    if lower_modalias.contains("v004c") {
        return DeviceVendor::Apple;
    }
    // Apple Accessory UUID
    for u in uuids {
        if u.to_lowercase().contains("74ec2172-0bad-4d01-8f77-99e2b2e07755") {
            return DeviceVendor::Apple;
        }
    }
    if lower_name.contains("airpods") || lower_name.contains("beats") {
        return DeviceVendor::Apple;
    }

    // 2. Bowers & Wilkins
    // Vendor ID 0x19B5 or known B&W GATT UUID
    if lower_modalias.contains("v19b5") {
        return DeviceVendor::BowersWilkins;
    }
    for u in uuids {
        let u_low = u.to_lowercase();
        if u_low.contains("ada50ce9-67b8-4a97-9d8e-37e1d083156c")
            || u_low.contains("00000001-0002-")
        {
            return DeviceVendor::BowersWilkins;
        }
    }
    if lower_name.contains("px7")
        || lower_name.contains("px8")
        || lower_name.contains("pi7")
        || lower_name.contains("pi8")
        || lower_name.contains("bowers & wilkins")
    {
        return DeviceVendor::BowersWilkins;
    }

    // 3. Sony
    // Vendor ID 0x054C
    if lower_modalias.contains("v054c") {
        return DeviceVendor::Sony;
    }
    for u in uuids {
        if u.to_lowercase().contains("96e42634-4390-4e9b-b09d-00c7716a3b37") {
            return DeviceVendor::Sony;
        }
    }
    if lower_name.contains("wh-1000")
        || lower_name.contains("wf-1000")
        || lower_name.contains("linkbuds")
        || lower_name.contains("mdr-")
    {
        return DeviceVendor::Sony;
    }

    DeviceVendor::Unknown
}

fn get_string_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| {
        let str_ref: Result<&str, _> = v.try_into();
        str_ref.ok().map(|s| s.to_string())
    })
}

fn get_bool_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    props.get(key).and_then(|v| {
        let b_val: Result<bool, _> = v.try_into();
        b_val.ok()
    })
}

fn get_u8_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<u8> {
    props.get(key).and_then(|v| {
        let u_val: Result<u8, _> = v.try_into();
        u_val.ok()
    })
}

fn get_string_vec_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<Vec<String>> {
    props.get(key).and_then(|v| {
        <Vec<String>>::try_from(v.clone()).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_paired_devices_on_system() {
        if let Ok(devs) = BluetoothInventory::fetch_paired_devices().await {
            println!("Discovered {} paired devices:", devs.len());
            for d in devs {
                println!(
                    " - [{}] {} ({:?}) - Connected: {}",
                    d.address, d.name, d.vendor, d.connected
                );
            }
        }
    }
}

