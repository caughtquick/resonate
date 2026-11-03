use rmp_serde::Deserializer;
use serde::{Deserialize, Serialize};

pub const BW_REQUEST_UUID: &str = "ada50ce9-67b8-4a97-9d8e-37e1d083156c";
pub const BW_RESPONSE_UUID: &str = "cb909093-3559-4b0c-9a7f-3f1773122fdc";
pub const BW_NOTIFICATION_UUID: &str = "df55d475-9a32-457a-9e20-38cf14e853fb";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BWRPCCommand {
    pub namespace: u8,
    pub id: u8,
    pub name: &'static str,
}

impl BWRPCCommand {
    pub const fn new(namespace: u8, id: u8, name: &'static str) -> Self {
        Self { namespace, id, name }
    }

    pub fn key(&self) -> String {
        format!("{:02X}:{:02X}", self.namespace, self.id)
    }
}

pub struct BWRPCatalog;

impl BWRPCatalog {
    pub const ANC_GET: BWRPCCommand = BWRPCCommand::new(0x03, 0x01, "ANC mode");
    pub const ANC_SET: BWRPCCommand = BWRPCCommand::new(0x03, 0x02, "Set ANC mode");
    pub const EQ_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x29, "Set 5-band EQ");
    pub const EQ_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x2A, "5-band EQ");
    pub const EQ_BYPASS_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x2B, "Set EQ bypass");
    pub const EQ_BYPASS_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x2C, "EQ bypass");
    pub const BASS_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x17, "Set bass");
    pub const BASS_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x18, "Bass");
    pub const TREBLE_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x19, "Set treble");
    pub const TREBLE_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x1A, "Treble");
    pub const WEAR_GET: BWRPCCommand = BWRPCCommand::new(0x0A, 0x01, "Wear sensor");
    pub const WEAR_SET: BWRPCCommand = BWRPCCommand::new(0x0A, 0x02, "Set wear sensor");
    pub const WEAR_SENSITIVITY_GET: BWRPCCommand = BWRPCCommand::new(0x0A, 0x03, "Wear sensitivity");
    pub const WEAR_SENSITIVITY_SET: BWRPCCommand = BWRPCCommand::new(0x0A, 0x04, "Set wear sensitivity");
    pub const SLEEP_GET: BWRPCCommand = BWRPCCommand::new(0x02, 0x06, "Sleep timer");
    pub const SLEEP_SET: BWRPCCommand = BWRPCCommand::new(0x02, 0x05, "Set sleep timer");
    pub const BUTTON_GET: BWRPCCommand = BWRPCCommand::new(0x08, 0x2B, "Custom button");
    pub const BUTTON_SET: BWRPCCommand = BWRPCCommand::new(0x08, 0x2A, "Set custom button");
    pub const VOICE_GET: BWRPCCommand = BWRPCCommand::new(0x04, 0x09, "Voice prompts");
    pub const VOICE_SET: BWRPCCommand = BWRPCCommand::new(0x04, 0x08, "Set voice prompts");
    pub const BATTERY_PERCENTAGE_GET: BWRPCCommand = BWRPCCommand::new(0x08, 0x0C, "Battery percentage");
    pub const CHARGING_STATUS_GET: BWRPCCommand = BWRPCCommand::new(0x08, 0x0B, "Charging status");

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BWRPCMessageKind {
    Response,
    Notification,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BWRPCMessage {
    pub kind: BWRPCMessageKind,
    pub command: BWRPCCommand,
    pub error_code: u16,
    pub payload_bytes: Option<Vec<u8>>,
    pub raw: Vec<u8>,
}

impl BWRPCMessage {
    pub fn succeeded(&self) -> bool {
        self.error_code == 0
    }

    pub fn deserialize_payload<'a, T: Deserialize<'a>>(&'a self) -> Option<Result<T, rmp_serde::decode::Error>> {
        self.payload_bytes.as_deref().map(|bytes| {
            let mut de = Deserializer::new(bytes);
            Deserialize::deserialize(&mut de)
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BWRPCError {
    #[error("BWRPC packet is too short")]
    PacketTooShort,
    #[error("Unsupported BWRPC type 0x{0:04X}")]
    UnsupportedType(u16),
    #[error("BWRPC payload length exceeds packet bounds")]
    InvalidPayloadLength,
    #[error("MessagePack serialization error: {0}")]
    MsgPackError(#[from] rmp_serde::encode::Error),
}

pub struct BWRPC;

impl BWRPC {
    pub fn build_request<T: Serialize>(command: BWRPCCommand, payload: Option<&T>) -> Result<Vec<u8>, BWRPCError> {
        let payload_bytes = match payload {
            Some(p) => rmp_serde::to_vec(p)?,
            None => Vec::new(),
        };

        let has_payload = !payload_bytes.is_empty();
        let packet_size = if has_payload { payload_bytes.len() + 6 } else { 4 };

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

    pub fn decode(data: &[u8]) -> Result<BWRPCMessage, BWRPCError> {
        if data.len() < 4 {
            return Err(BWRPCError::PacketTooShort);
        }
        let rpc_type = (data[0] as u16) | ((data[1] as u16) << 8);
        let cmd_id = data[2];
        let cmd_namespace = data[3];
        let command = BWRPCCommand::new(cmd_namespace, cmd_id, BWRPCatalog::name(cmd_namespace, cmd_id));

        let (kind, error_code, length_offset, payload_offset) = match rpc_type {
            0x120C | 0x920C => {
                if data.len() < 5 {
                    return Err(BWRPCError::PacketTooShort);
                }
                let err = (data[4] as u16) | (if data.len() > 5 { (data[5] as u16) << 8 } else { 0 });
                (BWRPCMessageKind::Response, err, 6, 8)
            }
            0x120D | 0x920D => {
                (BWRPCMessageKind::Notification, 0, 4, 6)
            }
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
