//! Bowers & Wilkins Bluetooth Low Energy RPC protocol encoding and decoding.
//!
//! Bowers & Wilkins wireless headphones (Px7 S2, Px8, etc.) communicate using a binary RPC
//! format encoded with MessagePack over custom BLE GATT characteristics.
//!
//! This module provides:
//! - Characteristic UUID constants ([`BW_REQUEST_UUID`], [`BW_RESPONSE_UUID`], [`BW_NOTIFICATION_UUID`])
//! - Command identifiers and catalog ([`BWRPCCommand`], [`BWRPCatalog`])
//! - MessagePack request frame generator ([`BWRPC::build_request`])
//! - Response and asynchronous notification decoding ([`BWRPC::decode`], [`BWRPCMessage`])
//!
//! Protocol reverse engineering and command structures are derived from or inspired by
//! [HeadBridge](https://github.com/herenickname/HeadBridge).

use rmp_serde::Deserializer;
use serde::{Deserialize, Serialize};

/// 128-bit BLE GATT UUID of the Bowers & Wilkins RPC write/request characteristic.
pub const BW_REQUEST_UUID: &str = "ada50ce9-67b8-4a97-9d8e-37e1d083156c";

/// 128-bit BLE GATT UUID of the Bowers & Wilkins RPC response/read characteristic.
pub const BW_RESPONSE_UUID: &str = "cb909093-3559-4b0c-9a7f-3f1773122fdc";

/// 128-bit BLE GATT UUID of the Bowers & Wilkins RPC event notification characteristic.
pub const BW_NOTIFICATION_UUID: &str = "df55d475-9a32-457a-9e20-38cf14e853fb";

/// A command in the Bowers & Wilkins RPC protocol identified by namespace and command ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BWRPCCommand {
    /// Namespace byte group (e.g. `0x03` for ANC, `0x04` for Audio/EQ, `0x0A` for Wear Sensor).
    pub namespace: u8,
    /// Function/command ID within the namespace.
    pub id: u8,
    /// Human-readable label for debugging and logging.
    pub name: &'static str,
}

impl BWRPCCommand {
    /// Constructs a new [`BWRPCCommand`].
    pub const fn new(namespace: u8, id: u8, name: &'static str) -> Self {
        Self {
            namespace,
            id,
            name,
        }
    }

    /// Formats the namespace and ID as a hexadecimal string key (`"NN:II"`).
    pub fn key(&self) -> String {
        format!("{:02X}:{:02X}", self.namespace, self.id)
    }
}

/// Known Bowers & Wilkins RPC command catalog.
pub struct BWRPCatalog;

impl BWRPCatalog {
    /// Query active ANC mode.
    pub const ANC_GET: BWRPCCommand = BWRPCCommand::new(0x03, 0x01, "ANC mode");
    /// Set active ANC mode.
    pub const ANC_SET: BWRPCCommand = BWRPCCommand::new(0x03, 0x02, "Set ANC mode");
    /// Set 5-band equalizer gains.
    pub const EQ_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x29, "Set 5-band EQ");
    /// Query 5-band equalizer gains.
    pub const EQ_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x2A, "5-band EQ");
    /// Enable or disable equalizer bypass.
    pub const EQ_BYPASS_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x2B, "Set EQ bypass");
    /// Query equalizer bypass state.
    pub const EQ_BYPASS_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x2C, "EQ bypass");
    /// Set hardware bass tone control (-6 to +6 dB).
    pub const BASS_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x17, "Set bass");
    /// Query hardware bass tone control.
    pub const BASS_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x18, "Bass");
    /// Set hardware treble tone control (-6 to +6 dB).
    pub const TREBLE_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x19, "Set treble");
    /// Query hardware treble tone control.
    pub const TREBLE_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x1A, "Treble");
    /// Query wear sensor state.
    pub const WEAR_GET: BWRPCCommand = BWRPCCommand::new(0x0A, 0x01, "Wear sensor");
    /// Enable or disable wear sensor.
    pub const WEAR_SET: BWRPCCommand = BWRPCCommand::new(0x0A, 0x02, "Set wear sensor");
    /// Query wear sensor sensitivity.
    pub const WEAR_SENSITIVITY_GET: BWRPCCommand =
        BWRPCCommand::new(0x0A, 0x03, "Wear sensitivity");
    /// Set wear sensor sensitivity.
    pub const WEAR_SENSITIVITY_SET: BWRPCCommand =
        BWRPCCommand::new(0x0A, 0x04, "Set wear sensitivity");
    /// Query auto-standby / sleep timer.
    pub const SLEEP_GET: BWRPCCommand = BWRPCCommand::new(0x02, 0x06, "Sleep timer");
    /// Configure auto-standby / sleep timer.
    pub const SLEEP_SET: BWRPCCommand = BWRPCCommand::new(0x02, 0x05, "Set sleep timer");
    /// Query physical quick action button mapping.
    pub const BUTTON_GET: BWRPCCommand = BWRPCCommand::new(0x08, 0x2B, "Custom button");
    /// Remap physical quick action button mapping.
    pub const BUTTON_SET: BWRPCCommand = BWRPCCommand::new(0x08, 0x2A, "Set custom button");
    /// Query voice prompt announcements.
    pub const VOICE_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x09, "Voice prompts");
    /// Enable or disable voice prompt announcements.
    pub const VOICE_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x08, "Set voice prompts");
    /// Query battery percentage level.
    pub const BATTERY_PERCENTAGE_GET: BWRPCCommand =
        BWRPCCommand::new(0x08, 0x0C, "Battery percentage");
    /// Query charging status.
    pub const CHARGING_STATUS_GET: BWRPCCommand = BWRPCCommand::new(0x08, 0x0B, "Charging status");
    /// Query paired device list size.
    pub const PAIRED_DEVICE_COUNT_GET: BWRPCCommand =
        BWRPCCommand::new(0x05, 0x0B, "Paired device count");
    /// Query paired device at index.
    pub const PAIRED_DEVICE_GET: BWRPCCommand = BWRPCCommand::new(0x05, 0x04, "Paired device");
    /// Request to connect a paired device at index.
    pub const PAIRED_DEVICE_CONNECT: BWRPCCommand =
        BWRPCCommand::new(0x05, 0x05, "Connect paired device");
    /// Request to disconnect a paired device at index.
    pub const PAIRED_DEVICE_DISCONNECT: BWRPCCommand =
        BWRPCCommand::new(0x05, 0x06, "Disconnect paired device");
    /// Remove / unpair a device at index.
    pub const PAIRED_DEVICE_REMOVE: BWRPCCommand =
        BWRPCCommand::new(0x05, 0x07, "Remove paired device");

    /// Resolves a namespace and command ID to its human-readable title.
    pub fn name(namespace: u8, id: u8) -> &'static str {
        match (namespace, id) {
            (0x03, 0x01) => "ANC mode",
            (0x03, 0x02) => "Set ANC mode",
            (0x04, 0x29) => "Set 5-band EQ",
            (0x04, 0x2A) => "5-band EQ",
            (0x04, 0x17) => "Set bass",
            (0x04, 0x18) => "Bass",
            (0x04, 0x19) => "Set treble",
            (0x04, 0x1A) => "Treble",
            (0x05, 0x0B) => "Paired device count",
            (0x05, 0x04) => "Paired device",
            (0x05, 0x05) => "Connect paired device",
            (0x05, 0x06) => "Disconnect paired device",
            (0x05, 0x07) => "Remove paired device",
            (0x0A, 0x01) => "Wear sensor",
            (0x0A, 0x02) => "Set wear sensor",
            (0x0A, 0x03) => "Wear sensitivity",
            (0x0A, 0x04) => "Set wear sensitivity",
            (0x02, 0x06) => "Sleep timer",
            (0x02, 0x05) => "Set sleep timer",
            (0x08, 0x2B) => "Custom button",
            (0x08, 0x2A) => "Set custom button",
            (0x08, 0x0C) => "Battery percentage",
            (0x08, 0x0B) => "Charging status",
            _ => "Unknown BWRPC command",
        }
    }
}

/// Classification of decoded BWRPC message packets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BWRPCMessageKind {
    /// Direct synchronous response to a prior client request.
    Response,
    /// Unsolicited telemetry notification pushed from the headphone.
    Notification,
}

/// A parsed Bowers & Wilkins RPC packet.
#[derive(Debug, Clone, PartialEq)]
pub struct BWRPCMessage {
    /// Message category (Response or Notification).
    pub kind: BWRPCMessageKind,
    /// Associated RPC command identifier.
    pub command: BWRPCCommand,
    /// Result error code (0 indicates success).
    pub error_code: u16,
    /// MessagePack payload bytes if present.
    pub payload_bytes: Option<Vec<u8>>,
    /// Full unparsed raw frame.
    pub raw: Vec<u8>,
}

impl BWRPCMessage {
    /// Returns true if error code is 0.
    pub fn succeeded(&self) -> bool {
        self.error_code == 0
    }

    /// Deserializes the MessagePack payload into a specified target type.
    pub fn deserialize_payload<'a, T: Deserialize<'a>>(
        &'a self,
    ) -> Option<Result<T, rmp_serde::decode::Error>> {
        self.payload_bytes.as_deref().map(|bytes| {
            let mut de = Deserializer::new(bytes);
            Deserialize::deserialize(&mut de)
        })
    }
}

/// Errors occurring during Bowers & Wilkins RPC serialization or decoding.
#[derive(Debug, thiserror::Error)]
pub enum BWRPCError {
    /// Packet byte count is less than the minimum required header size.
    #[error("BWRPC packet is too short")]
    PacketTooShort,
    /// Unrecognized RPC frame header type.
    #[error("Unsupported BWRPC type 0x{0:04X}")]
    UnsupportedType(u16),
    /// Frame payload length field exceeds available packet bytes.
    #[error("BWRPC payload length exceeds packet bounds")]
    InvalidPayloadLength,
    /// MessagePack serialization failed.
    #[error("MessagePack serialization error: {0}")]
    MsgPackError(#[from] rmp_serde::encode::Error),
}

/// Encoder and decoder for Bowers & Wilkins RPC frames.
pub struct BWRPC;

impl BWRPC {
    /// Builds a serialized BWRPC request packet ready to write to the request characteristic.
    pub fn build_request<T: Serialize>(
        command: BWRPCCommand,
        payload: Option<&T>,
    ) -> Result<Vec<u8>, BWRPCError> {
        let payload_bytes = match payload {
            Some(p) => rmp_serde::to_vec(p)?,
            None => Vec::new(),
        };

        let has_payload = !payload_bytes.is_empty();
        let packet_size = if has_payload {
            payload_bytes.len() + 6
        } else {
            4
        };

        let mut out = Vec::with_capacity(packet_size + 1);
        out.push(packet_size as u8);
        if has_payload {
            out.extend_from_slice(&[0x0B, 0x92]);
        } else {
            out.extend_from_slice(&[0x0B, 0x12]);
        }
        out.push(command.id);
        out.push(command.namespace);

        if has_payload {
            let len = payload_bytes.len() as u16;
            out.push((len & 0xFF) as u8);
            out.push(((len >> 8) & 0xFF) as u8);
            out.extend_from_slice(&payload_bytes);
        }

        Ok(out)
    }

    /// Decodes a raw byte buffer received from the response or notification characteristic into a [`BWRPCMessage`].
    pub fn decode(data: &[u8]) -> Result<BWRPCMessage, BWRPCError> {
        if data.len() < 4 {
            return Err(BWRPCError::PacketTooShort);
        }
        let rpc_type = (data[0] as u16) | ((data[1] as u16) << 8);
        let cmd_id = data[2];
        let cmd_namespace = data[3];
        let command = BWRPCCommand::new(
            cmd_namespace,
            cmd_id,
            BWRPCatalog::name(cmd_namespace, cmd_id),
        );

        let (kind, error_code, length_offset, payload_offset) = match rpc_type {
            0x120C | 0x920C => {
                if data.len() < 5 {
                    return Err(BWRPCError::PacketTooShort);
                }
                let err = (data[4] as u16)
                    | (if data.len() > 5 {
                        (data[5] as u16) << 8
                    } else {
                        0
                    });
                (BWRPCMessageKind::Response, err, 6, 8)
            }
            0x120D | 0x920D => (BWRPCMessageKind::Notification, 0, 4, 6),
            _ => return Err(BWRPCError::UnsupportedType(rpc_type)),
        };

        let has_payload = rpc_type == 0x920C || rpc_type == 0x920D;
        let payload_bytes = if has_payload {
            if data.len() < payload_offset {
                return Err(BWRPCError::PacketTooShort);
            }
            let length = (data[length_offset] as usize) | ((data[length_offset + 1] as usize) << 8);
            if payload_offset + length > data.len() {
                return Err(BWRPCError::InvalidPayloadLength);
            }
            Some(data[payload_offset..payload_offset + length].to_vec())
        } else {
            None
        };

        Ok(BWRPCMessage {
            kind,
            command,
            error_code,
            payload_bytes,
            raw: data.to_vec(),
        })
    }
}

/// Wire values of the ANC mode differ between Bowers & Wilkins hardware generations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BwAncWireProfile {
    /// Hardware-verified on Px7 S3: off 0, noise cancellation 1, pass-through 2.
    Standard,
    /// Hardware-verified on original Px8 (RPC software `0(20.0.2.0)`) and
    /// Px7 S2 / Px7 S2e: off 1, noise cancellation 2, pass-through 3.
    /// The devices reject 0 with device error 6.
    OriginalPx8,
}

impl BwAncWireProfile {
    /// Resolves the wire profile from a device name or model name.
    pub fn for_device_name(name: &str) -> Self {
        let norm: String = name
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();
        if norm.contains("px7s2")
            || norm.contains("px7s2e")
            || (norm.contains("px8") && !norm.contains("px8s2"))
        {
            Self::OriginalPx8
        } else {
            Self::Standard
        }
    }

    /// Translates a normalized mode string ("off", "anc", "transparency") into the wire integer.
    pub fn to_wire_value(&self, mode: &str) -> u8 {
        match self {
            Self::Standard => match mode {
                "off" => 0,
                "anc" | "nc" | "on" => 1,
                "transparency" | "ambient" | "passthrough" => 2,
                _ => 1,
            },
            Self::OriginalPx8 => match mode {
                "off" => 1,
                "anc" | "nc" | "on" => 2,
                "transparency" | "ambient" | "passthrough" => 3,
                _ => 2,
            },
        }
    }

    /// Translates a wire integer into a normalized mode string.
    pub fn from_wire_value(&self, val: u8) -> &'static str {
        match self {
            Self::Standard => match val {
                0 => "off",
                1 => "anc",
                2 => "transparency",
                _ => "anc",
            },
            Self::OriginalPx8 => match val {
                1 => "off",
                2 => "anc",
                3 => "transparency",
                _ => "anc",
            },
        }
    }
}

/// Converts decibel or tenth-decibel user input into B&W hardware wire values (-60 to +60).
pub fn parse_bw_level(s: &str) -> Option<i32> {
    let clean = s
        .trim()
        .trim_end_matches(|c: char| c.eq_ignore_ascii_case(&'d') || c.eq_ignore_ascii_case(&'b'))
        .trim();
    if let Ok(f) = clean.parse::<f64>() {
        // Values within [-6.0, 6.0] are interpreted as decibels (scaled to tenths).
        // Values outside [-6.0, 6.0] are interpreted as already in tenths of a dB.
        let wire = if f.abs() <= 6.0 && f != 0.0 {
            (f * 10.0).round() as i32
        } else {
            f.round() as i32
        };
        Some(wire.clamp(-60, 60))
    } else {
        None
    }
}

/// Formats a wire level (-60 to +60) into a human-readable dB string (e.g. "+2 dB", "-1.5 dB", "0 dB").
pub fn format_bw_db(val: i32) -> String {
    if val == 0 {
        "0 dB".to_string()
    } else if val % 10 == 0 {
        format!("{:+}", val / 10) + " dB"
    } else {
        format!("{:+.1} dB", val as f64 / 10.0)
    }
}

/// Formats combined bass and treble tone controls into a human-readable string.
pub fn format_bw_tone(bass: Option<i32>, treble: Option<i32>) -> String {
    match (bass, treble) {
        (Some(b), Some(t)) => format!("Bass: {}, Treble: {}", format_bw_db(b), format_bw_db(t)),
        (Some(b), None) => format!("Bass: {}", format_bw_db(b)),
        (None, Some(t)) => format!("Treble: {}", format_bw_db(t)),
        (None, None) => "Flat (0 dB)".to_string(),
    }
}

/// Formats 5-band equalizer gains into a human-readable string.
pub fn format_bw_5band(bands: &[i32]) -> String {
    let band_strs: Vec<String> = bands.iter().map(|&v| format_bw_db(v)).collect();
    format!("[{}]", band_strs.join(", "))
}

/// A paired device reported by Bowers & Wilkins headphones over BWRPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BwPairedDevice {
    /// Slot index in the headphone's paired device list.
    pub id: usize,
    /// Advertised or stored device name.
    pub name: String,
    /// Bluetooth MAC address formatted as `"XX:XX:XX:XX:XX:XX"`.
    pub address: String,
    /// Whether this paired device is currently connected to the headphone.
    pub connected: bool,
}

/// Decodes a MessagePack payload from `PAIRED_DEVICE_GET` (0x05, 0x04) into a [`BwPairedDevice`].
///
/// B&W returns a MessagePack map containing:
/// - Key 1 / "1": MAC address (array of integers or binary slice)
/// - Key 2 / "2": Friendly device name (string)
/// - Key 3 / "3": Multipoint connection status (boolean)
pub fn decode_paired_device(index: usize, payload: &[u8]) -> Option<BwPairedDevice> {
    use std::io::Cursor;
    let mut cur = Cursor::new(payload);
    let val = rmpv::decode::read_value(&mut cur).ok()?;
    let map = val.as_map()?;

    let mut mac_bytes: Option<Vec<u8>> = None;
    let mut name: Option<String> = None;
    let mut connected: Option<bool> = None;
    let mut extracted_id: Option<usize> = None;

    for (k, v) in map {
        let key_id: Option<u64> = if let Some(i) = k.as_u64() {
            Some(i)
        } else if let Some(s) = k.as_str() {
            s.parse::<u64>().ok()
        } else {
            None
        };

        match key_id {
            Some(0) => {
                if let Some(i) = v.as_u64() {
                    extracted_id = Some(i as usize);
                }
            }
            Some(1) => {
                if let Some(arr) = v.as_array() {
                    let bytes: Vec<u8> = arr
                        .iter()
                        .filter_map(|x| x.as_u64().map(|b| b as u8))
                        .collect();
                    mac_bytes = Some(bytes);
                } else if let Some(bin) = v.as_slice() {
                    mac_bytes = Some(bin.to_vec());
                }
            }
            Some(2) => {
                if let Some(s) = v.as_str() {
                    let clean = s.trim_matches('\0').trim().to_string();
                    name = Some(clean);
                }
            }
            Some(3) => {
                if let Some(b) = v.as_bool() {
                    connected = Some(b);
                } else if let Some(i) = v.as_u64() {
                    connected = Some(i != 0);
                }
            }
            _ => {}
        }
    }

    let addr_str = if let Some(bytes) = mac_bytes
        && bytes.len() >= 6
    {
        format!(
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
        )
    } else {
        "00:00:00:00:00:00".to_string()
    };

    let final_id = extracted_id.unwrap_or(index);

    Some(BwPairedDevice {
        id: final_id,
        name: name.unwrap_or_else(|| format!("Device {}", final_id)),
        address: addr_str,
        connected: connected.unwrap_or(false),
    })
}

/// Extracts the paired device count from a `PAIRED_DEVICE_COUNT_GET` response message.
pub fn parse_bw_paired_count(msg: &BWRPCMessage) -> Option<usize> {
    msg.deserialize_payload::<usize>()
        .and_then(|r| r.ok())
        .or_else(|| {
            msg.deserialize_payload::<u64>()
                .and_then(|r| r.ok())
                .map(|v| v as usize)
        })
        .or_else(|| {
            msg.deserialize_payload::<u32>()
                .and_then(|r| r.ok())
                .map(|v| v as usize)
        })
        .or_else(|| {
            msg.deserialize_payload::<u8>()
                .and_then(|r| r.ok())
                .map(|v| v as usize)
        })
        .or_else(|| {
            if let Some(ref bytes) = msg.payload_bytes {
                let mut cur = std::io::Cursor::new(bytes);
                rmpv::decode::read_value(&mut cur)
                    .ok()
                    .and_then(|v| v.as_u64().map(|n| n as usize))
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bw_anc_wire_profile_selection() {
        assert_eq!(
            BwAncWireProfile::for_device_name("Abhijit’s Px7 S2e"),
            BwAncWireProfile::OriginalPx8
        );
        assert_eq!(
            BwAncWireProfile::for_device_name("Px7 S2"),
            BwAncWireProfile::OriginalPx8
        );
        assert_eq!(
            BwAncWireProfile::for_device_name("Bowers & Wilkins Px8"),
            BwAncWireProfile::OriginalPx8
        );
        assert_eq!(
            BwAncWireProfile::for_device_name("Px7 S3"),
            BwAncWireProfile::Standard
        );
    }

    #[test]
    fn test_bw_anc_wire_values() {
        let p = BwAncWireProfile::OriginalPx8;
        assert_eq!(p.to_wire_value("off"), 1);
        assert_eq!(p.to_wire_value("anc"), 2);
        assert_eq!(p.to_wire_value("transparency"), 3);
        assert_eq!(p.from_wire_value(1), "off");
        assert_eq!(p.from_wire_value(2), "anc");
        assert_eq!(p.from_wire_value(3), "transparency");

        let s = BwAncWireProfile::Standard;
        assert_eq!(s.to_wire_value("off"), 0);
        assert_eq!(s.to_wire_value("anc"), 1);
        assert_eq!(s.to_wire_value("transparency"), 2);
        assert_eq!(s.from_wire_value(0), "off");
        assert_eq!(s.from_wire_value(1), "anc");
        assert_eq!(s.from_wire_value(2), "transparency");
    }

    #[test]
    fn test_bwrpc_request_without_payload() {
        let req = BWRPC::build_request::<()>(BWRPCatalog::ANC_GET, None).unwrap();
        // size=4, 0x0B, 0x12, id=0x01, namespace=0x03
        assert_eq!(req, vec![0x04, 0x0B, 0x12, 0x01, 0x03]);
    }

    #[test]
    fn test_bwrpc_request_with_payload() {
        let anc_mode = 1u8; // Noise cancellation
        let req = BWRPC::build_request(BWRPCatalog::ANC_SET, Some(&anc_mode)).unwrap();
        // size = 1 + 6 = 7
        assert_eq!(req[0], 0x07);
        assert_eq!(&req[1..5], &[0x0B, 0x92, 0x02, 0x03]);
        assert_eq!(req[5], 0x01); // length lo
        assert_eq!(req[6], 0x00); // length hi
        assert_eq!(req[7], 0x01); // msgpack 1u8
    }

    #[test]
    fn test_bwrpc_response_decode() {
        // Response with payload: type 0x920C (0x0C, 0x92), id 0x01, ns 0x03, err 0x00 0x00, len 1 (0x01, 0x00), payload 0x02
        let raw = vec![0x0C, 0x92, 0x01, 0x03, 0x00, 0x00, 0x01, 0x00, 0x02];
        let msg = BWRPC::decode(&raw).expect("decode failed");
        assert_eq!(msg.kind, BWRPCMessageKind::Response);
        assert_eq!(msg.command.namespace, 0x03);
        assert_eq!(msg.command.id, 0x01);
        assert_eq!(msg.error_code, 0);
        assert!(msg.succeeded());

        let val: u8 = msg.deserialize_payload().unwrap().unwrap();
        assert_eq!(val, 2);
    }

    #[test]
    fn test_bw_level_parsing_and_formatting() {
        assert_eq!(parse_bw_level("2"), Some(20));
        assert_eq!(parse_bw_level("+2"), Some(20));
        assert_eq!(parse_bw_level("-1"), Some(-10));
        assert_eq!(parse_bw_level("2.5"), Some(25));
        assert_eq!(parse_bw_level("-1.5dB"), Some(-15));
        assert_eq!(parse_bw_level("0"), Some(0));
        assert_eq!(parse_bw_level("20"), Some(20));
        assert_eq!(parse_bw_level("-10"), Some(-10));
        assert_eq!(parse_bw_level("60"), Some(60));
        assert_eq!(parse_bw_level("-60"), Some(-60));

        assert_eq!(format_bw_db(20), "+2 dB");
        assert_eq!(format_bw_db(-10), "-1 dB");
        assert_eq!(format_bw_db(25), "+2.5 dB");
        assert_eq!(format_bw_db(0), "0 dB");

        assert_eq!(
            format_bw_tone(Some(20), Some(-10)),
            "Bass: +2 dB, Treble: -1 dB"
        );
        assert_eq!(
            format_bw_5band(&[0, 10, 20, -10, 0]),
            "[0 dB, +1 dB, +2 dB, -1 dB, 0 dB]"
        );
    }

    #[test]
    fn test_bw_5band_eq_request_and_response() {
        let bands = vec![0i32, 10, 20, -10, 0];
        let req = BWRPC::build_request(BWRPCatalog::EQ_SET, Some(&bands)).unwrap();
        assert_eq!(req[1], 0x0B);
        assert_eq!(req[2], 0x92);
        assert_eq!(req[3], 0x29); // EQ_SET id
        assert_eq!(req[4], 0x04); // namespace 0x04

        // Simulated EQ_GET response with 5 bands
        let payload = rmp_serde::to_vec(&bands).unwrap();
        let mut raw = vec![0x0C, 0x92, 0x2A, 0x04, 0x00, 0x00];
        let len = payload.len() as u16;
        raw.push((len & 0xFF) as u8);
        raw.push(((len >> 8) & 0xFF) as u8);
        raw.extend_from_slice(&payload);

        let msg = BWRPC::decode(&raw).unwrap();
        assert!(msg.succeeded());
        assert_eq!(msg.command.id, 0x2A);
        assert_eq!(msg.command.namespace, 0x04);
        let decoded_bands: Vec<i32> = msg.deserialize_payload().unwrap().unwrap();
        assert_eq!(decoded_bands, bands);
    }

    #[test]
    fn test_decode_paired_device_string_keys() {
        use std::collections::BTreeMap;
        // Test map with string keys
        let mut map: BTreeMap<String, rmpv::Value> = BTreeMap::new();
        map.insert(
            "1".to_string(),
            rmpv::Value::Array(vec![
                rmpv::Value::Integer(0xEC.into()),
                rmpv::Value::Integer(0x66.into()),
                rmpv::Value::Integer(0xD1.into()),
                rmpv::Value::Integer(0xBE.into()),
                rmpv::Value::Integer(0x1D.into()),
                rmpv::Value::Integer(0x80.into()),
            ]),
        );
        map.insert("2".to_string(), rmpv::Value::String("MacBook Pro".into()));
        map.insert("3".to_string(), rmpv::Value::Boolean(true));

        let payload = rmp_serde::to_vec(&map).unwrap();
        let dev = decode_paired_device(0, &payload).expect("should decode paired device");
        assert_eq!(dev.id, 0);
        assert_eq!(dev.name, "MacBook Pro");
        assert_eq!(dev.address, "EC:66:D1:BE:1D:80");
        assert!(dev.connected);
    }

    #[test]
    fn test_decode_paired_device_int_keys() {
        use std::collections::BTreeMap;
        // Test map with integer keys and binary MAC address
        let mut map: BTreeMap<u8, rmpv::Value> = BTreeMap::new();
        map.insert(
            1,
            rmpv::Value::Binary(vec![0x30, 0xE3, 0xA4, 0x86, 0x67, 0xF2]),
        );
        map.insert(2, rmpv::Value::String("papaya".into()));
        map.insert(3, rmpv::Value::Boolean(false));

        let payload = rmp_serde::to_vec(&map).unwrap();
        let dev = decode_paired_device(1, &payload).expect("should decode paired device");
        assert_eq!(dev.id, 1);
        assert_eq!(dev.name, "papaya");
        assert_eq!(dev.address, "30:E3:A4:86:67:F2");
        assert!(!dev.connected);
    }

    #[test]
    fn test_bwrpc_paired_device_connect_disconnect_and_wear_sensitivity() {
        assert_eq!(BWRPCatalog::PAIRED_DEVICE_CONNECT.namespace, 0x05);
        assert_eq!(BWRPCatalog::PAIRED_DEVICE_CONNECT.id, 0x05);
        assert_eq!(BWRPCatalog::PAIRED_DEVICE_DISCONNECT.namespace, 0x05);
        assert_eq!(BWRPCatalog::PAIRED_DEVICE_DISCONNECT.id, 0x06);
        assert_eq!(BWRPCatalog::WEAR_SENSITIVITY_GET.namespace, 0x0A);
        assert_eq!(BWRPCatalog::WEAR_SENSITIVITY_GET.id, 0x03);
        assert_eq!(BWRPCatalog::WEAR_SENSITIVITY_SET.namespace, 0x0A);
        assert_eq!(BWRPCatalog::WEAR_SENSITIVITY_SET.id, 0x04);

        let req_connect =
            BWRPC::build_request(BWRPCatalog::PAIRED_DEVICE_CONNECT, Some(&1u8)).unwrap();
        assert_eq!(req_connect[1], 0x0B);
        assert_eq!(req_connect[2], 0x92);
        assert_eq!(req_connect[3], 0x05);
        assert_eq!(req_connect[4], 0x05);

        let req_disconnect =
            BWRPC::build_request(BWRPCatalog::PAIRED_DEVICE_DISCONNECT, Some(&2u8)).unwrap();
        assert_eq!(req_disconnect[1], 0x0B);
        assert_eq!(req_disconnect[2], 0x92);
        assert_eq!(req_disconnect[3], 0x06);
        assert_eq!(req_disconnect[4], 0x05);

        let req_wear_sens =
            BWRPC::build_request(BWRPCatalog::WEAR_SENSITIVITY_SET, Some(&2u8)).unwrap();
        assert_eq!(req_wear_sens[1], 0x0B);
        assert_eq!(req_wear_sens[2], 0x92);
        assert_eq!(req_wear_sens[3], 0x04);
        assert_eq!(req_wear_sens[4], 0x0A);
    }
}
