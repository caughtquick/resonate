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

#[cfg(test)]
mod tests {
    use super::*;

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
}
