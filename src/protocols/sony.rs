use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SonyMDRPacket {
    pub data_type: u8,
    pub sequence: u8,
    pub payload: Vec<u8>,
}

impl SonyMDRPacket {
    pub const ACK_TYPE: u8 = 0x01;
    pub const COMMAND_TYPE: u8 = 0x0C;
    pub const COMMAND_TYPE2: u8 = 0x0E;

    pub fn new(data_type: u8, sequence: u8, payload: Vec<u8>) -> Self {
        Self {
            data_type,
            sequence,
            payload,
        }
    }
}

pub struct SonyMDRFraming;

impl SonyMDRFraming {
    pub const START_MARKER: u8 = 0x3E;
    pub const END_MARKER: u8 = 0x3C;
    pub const ESCAPE_MARKER: u8 = 0x3D;
    pub const ESCAPE_MASK: u8 = 0xEF;

    pub fn encode(packet: &SonyMDRPacket) -> Vec<u8> {
        let length = packet.payload.len() as u32;
        let mut body = Vec::with_capacity(7 + packet.payload.len());
        body.push(packet.data_type);
        body.push(packet.sequence);
        body.push((length >> 24) as u8);
        body.push((length >> 16) as u8);
        body.push((length >> 8) as u8);
        body.push(length as u8);
        body.extend_from_slice(&packet.payload);

        let checksum = body.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        body.push(checksum);

        let mut encoded = Vec::with_capacity(body.len() + 2);
        encoded.push(Self::START_MARKER);
        for byte in body {
            if byte == Self::START_MARKER || byte == Self::END_MARKER || byte == Self::ESCAPE_MARKER {
                encoded.push(Self::ESCAPE_MARKER);
                encoded.push(byte & Self::ESCAPE_MASK);
            } else {
                encoded.push(byte);
            }
        }
        encoded.push(Self::END_MARKER);
        encoded
    }
}

pub struct SonyMDRFrameParser {
    state: ParserState,
    body: Vec<u8>,
    is_escaped: bool,
    max_payload_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    WaitingForStart,
    ReadingBody,
}

impl SonyMDRFrameParser {
    pub const DEFAULT_MAX_PAYLOAD_LENGTH: usize = 1_048_576;

    pub fn new() -> Self {
        Self::with_max_payload(Self::DEFAULT_MAX_PAYLOAD_LENGTH)
    }

    pub fn with_max_payload(max_payload_len: usize) -> Self {
        Self {
            state: ParserState::WaitingForStart,
            body: Vec::new(),
            is_escaped: false,
            max_payload_len,
        }
    }

    pub fn reset(&mut self) {
        self.state = ParserState::WaitingForStart;
        self.body.clear();
        self.is_escaped = false;
    }

    pub fn append(&mut self, data: &[u8]) -> Vec<SonyMDRPacket> {
        let mut packets = Vec::new();

        for &byte in data {
            match self.state {
                ParserState::WaitingForStart => {
                    if byte == SonyMDRFraming::START_MARKER {
                        self.state = ParserState::ReadingBody;
                        self.body.clear();
                        self.is_escaped = false;
                    }
                }
                ParserState::ReadingBody => {
                    if self.is_escaped {
                        self.body.push(byte | 0x10);
                        self.is_escaped = false;
                    } else if byte == SonyMDRFraming::ESCAPE_MARKER {
                        self.is_escaped = true;
                    } else if byte == SonyMDRFraming::END_MARKER {
                        if let Some(packet) = Self::decode_body(&self.body, self.max_payload_len) {
                            packets.push(packet);
                        }
                        self.state = ParserState::WaitingForStart;
                        self.body.clear();
                    } else if byte == SonyMDRFraming::START_MARKER {
                        // Resynchronization point after corrupt stream
                        self.body.clear();
                        self.is_escaped = false;
                    } else {
                        self.body.push(byte);
                    }

                    if self.body.len() > self.max_payload_len + 7 {
                        self.reset();
                    }
                }
            }
        }

        packets
    }

    fn decode_body(body: &[u8], max_len: usize) -> Option<SonyMDRPacket> {
        if body.len() < 7 {
            return None;
        }
        let length = ((body[2] as u32) << 24)
            | ((body[3] as u32) << 16)
            | ((body[4] as u32) << 8)
            | (body[5] as u32);
        let len_usize = length as usize;
        if len_usize > max_len || body.len() != len_usize + 7 {
            return None;
        }

        let checksum_idx = 6 + len_usize;
        let expected_checksum = body[..checksum_idx]
            .iter()
            .fold(0u8, |acc, &b| acc.wrapping_add(b));
        if body[checksum_idx] != expected_checksum {
            return None;
        }

        Some(SonyMDRPacket {
            data_type: body[0],
            sequence: body[1],
            payload: body[6..checksum_idx].to_vec(),
        })
    }
}

impl Default for SonyMDRFrameParser {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SonyV1Opcode;

impl SonyV1Opcode {
    pub const INIT_GET: u8 = 0x00;
    pub const INIT_RETURN: u8 = 0x01;
    pub const MODEL_GET: u8 = 0x04;
    pub const MODEL_RETURN: u8 = 0x05;
    pub const BATTERY_GET: u8 = 0x10;
    pub const BATTERY_RETURN: u8 = 0x11;
    pub const BATTERY_NOTIFY: u8 = 0x13;
    pub const CODEC_GET: u8 = 0x18;
    pub const CODEC_RETURN: u8 = 0x19;
    pub const CODEC_NOTIFY: u8 = 0x1B;
    pub const EQUALIZER_GET: u8 = 0x56;
    pub const EQUALIZER_RETURN: u8 = 0x57;
    pub const EQUALIZER_SET: u8 = 0x58;
    pub const EQUALIZER_NOTIFY: u8 = 0x59;
    pub const NOISE_CAPABILITY_GET: u8 = 0x60;
    pub const NOISE_CAPABILITY_RETURN: u8 = 0x61;
    pub const NOISE_GET: u8 = 0x66;
    pub const NOISE_RETURN: u8 = 0x67;
    pub const NOISE_SET: u8 = 0x68;
    pub const NOISE_NOTIFY: u8 = 0x69;
    pub const OPTIMIZER_START: u8 = 0x84;
    pub const OPTIMIZER_STATUS: u8 = 0x85;
    pub const OPTIMIZER_STATE_GET: u8 = 0x86;
    pub const OPTIMIZER_STATE_RETURN: u8 = 0x87;
    pub const OPTIMIZER_STATE_NOTIFY: u8 = 0x89;
    pub const PLAYBACK_GET: u8 = 0xA6;
    pub const PLAYBACK_RETURN: u8 = 0xA7;
    pub const PLAYBACK_SET: u8 = 0xA8;
    pub const PLAYBACK_NOTIFY: u8 = 0xA9;
    pub const TOUCH_SENSOR_GET: u8 = 0xD6;
    pub const TOUCH_SENSOR_RETURN: u8 = 0xD7;
    pub const TOUCH_SENSOR_SET: u8 = 0xD8;
    pub const TOUCH_SENSOR_NOTIFY: u8 = 0xD9;
    pub const AUDIO_SETTING_GET: u8 = 0xE6;
    pub const AUDIO_SETTING_RETURN: u8 = 0xE7;
    pub const AUDIO_SETTING_SET: u8 = 0xE8;
    pub const AUDIO_SETTING_NOTIFY: u8 = 0xE9;
    pub const SYSTEM_SETTING_GET: u8 = 0xF6;
    pub const SYSTEM_SETTING_RETURN: u8 = 0xF7;
    pub const SYSTEM_SETTING_SET: u8 = 0xF8;
    pub const SYSTEM_SETTING_NOTIFY: u8 = 0xF9;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SonyV1NoiseMode {
    Off,
    NoiseCancellation,
    Ambient,
    WindReduction,
}

impl SonyV1NoiseMode {
    pub fn decode(effect: u8, nc_setting_type: u8, nc_val: u8, ambient_level: i32) -> Self {
        if effect == 0x00 {
            return Self::Off;
        }
        if nc_setting_type == 0x02 && nc_val == 0x01 {
            return Self::WindReduction;
        }
        if nc_val != 0x00 {
            return Self::NoiseCancellation;
        }
        if (1..=20).contains(&ambient_level) {
            return Self::Ambient;
        }
        Self::Off
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum SonyV1EqualizerPreset {
    Off = 0x00,
    Bright = 0x10,
    Excited = 0x11,
    Mellow = 0x12,
    Relaxed = 0x13,
    Vocal = 0x14,
    TrebleBoost = 0x15,
    BassBoost = 0x16,
    Speech = 0x17,
    Manual = 0xA0,
    Custom1 = 0xA1,
    Custom2 = 0xA2,
}

impl SonyV1EqualizerPreset {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x00 => Some(Self::Off),
            0x10 => Some(Self::Bright),
            0x11 => Some(Self::Excited),
            0x12 => Some(Self::Mellow),
            0x13 => Some(Self::Relaxed),
            0x14 => Some(Self::Vocal),
            0x15 => Some(Self::TrebleBoost),
            0x16 => Some(Self::BassBoost),
            0x17 => Some(Self::Speech),
            0xA0 => Some(Self::Manual),
            0xA1 => Some(Self::Custom1),
            0xA2 => Some(Self::Custom2),
            _ => None,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Bright => "Bright",
            Self::Excited => "Excited",
            Self::Mellow => "Mellow",
            Self::Relaxed => "Relaxed",
            Self::Vocal => "Vocal",
            Self::TrebleBoost => "Treble Boost",
            Self::BassBoost => "Bass Boost",
            Self::Speech => "Speech",
            Self::Manual => "Manual",
            Self::Custom1 => "Custom 1",
            Self::Custom2 => "Custom 2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SonyV1AutomaticPowerOff {
    Off,
    After5Minutes,
    After30Minutes,
    After1Hour,
    After3Hours,
}

impl SonyV1AutomaticPowerOff {
    pub fn code(&self) -> (u8, u8) {
        match self {
            Self::Off => (0x11, 0x00),
            Self::After5Minutes => (0x00, 0x00),
            Self::After30Minutes => (0x01, 0x01),
            Self::After1Hour => (0x02, 0x02),
            Self::After3Hours => (0x03, 0x03),
        }
    }

    pub fn from_codes(first: u8, second: u8) -> Option<Self> {
        match (first, second) {
            (0x11, 0x00) => Some(Self::Off),
            (0x00, 0x00) => Some(Self::After5Minutes),
            (0x01, 0x01) => Some(Self::After30Minutes),
            (0x02, 0x02) => Some(Self::After1Hour),
            (0x03, 0x03) => Some(Self::After3Hours),
            _ => None,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::After5Minutes => "5 minutes",
            Self::After30Minutes => "30 minutes",
            Self::After1Hour => "1 hour",
            Self::After3Hours => "3 hours",
        }
    }
}

pub struct SonyV1Payloads;

impl SonyV1Payloads {
    pub fn equalizer_preset(preset: SonyV1EqualizerPreset) -> Vec<u8> {
        vec![SonyV1Opcode::EQUALIZER_SET, 0x01, preset as u8, 0x00]
    }

    /// UI order is 400 Hz, 1 kHz, 2.5 kHz, 6.3 kHz, 16 kHz, Clear Bass (-10 to +10).
    pub fn equalizer_bands(values: &[i32]) -> Vec<u8> {
        let mut bands = [0i32; 6];
        for (i, &v) in values.iter().take(6).enumerate() {
            bands[i] = v;
        }
        let normalized: Vec<u8> = bands
            .iter()
            .map(|&v| (v.clamp(-10, 10) + 10) as u8)
            .collect();
        vec![
            SonyV1Opcode::EQUALIZER_SET,
            0x01,
            0xFF,
            0x06,
            normalized[5], // Clear Bass first in Sony wire order
            normalized[0],
            normalized[1],
            normalized[2],
            normalized[3],
            normalized[4],
        ]
    }

    pub fn dsee(enabled: bool) -> Vec<u8> {
        vec![
            SonyV1Opcode::AUDIO_SETTING_SET,
            0x02,
            0x00,
            if enabled { 0x01 } else { 0x00 },
        ]
    }

    pub fn touch_sensor(enabled: bool, slot: u8) -> Vec<u8> {
        vec![
            SonyV1Opcode::TOUCH_SENSOR_SET,
            slot,
            0x01,
            if enabled { 0x01 } else { 0x00 },
        ]
    }

    pub fn optimizer(start: bool) -> Vec<u8> {
        vec![
            SonyV1Opcode::OPTIMIZER_START,
            0x01,
            0x00,
            if start { 0x01 } else { 0x00 },
        ]
    }

    pub fn automatic_power_off(mode: SonyV1AutomaticPowerOff) -> Vec<u8> {
        let (c0, c1) = mode.code();
        vec![SonyV1Opcode::SYSTEM_SETTING_SET, 0x04, 0x01, c0, c1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sony_mdr_framing_roundtrip_with_escaped_markers() {
        // Payload with byte markers 0x3E, 0x3C, 0x3D
        let payload = vec![0x3E, 0x01, 0x3C, 0x02, 0x3D, 0x03];
        let packet = SonyMDRPacket::new(SonyMDRPacket::COMMAND_TYPE, 0x05, payload.clone());
        let encoded = SonyMDRFraming::encode(&packet);

        let mut parser = SonyMDRFrameParser::new();
        let decoded = parser.append(&encoded);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].data_type, SonyMDRPacket::COMMAND_TYPE);
        assert_eq!(decoded[0].sequence, 0x05);
        assert_eq!(decoded[0].payload, payload);
    }

    #[test]
    fn test_sony_mdr_parser_rejects_bad_checksum() {
        let packet = SonyMDRPacket::new(SonyMDRPacket::COMMAND_TYPE, 0x01, vec![0x10, 0x20]);
        let mut encoded = SonyMDRFraming::encode(&packet);

        // Corrupt checksum byte (second to last byte before 0x3C)
        let len = encoded.len();
        encoded[len - 2] ^= 0xFF;

        let mut parser = SonyMDRFrameParser::new();
        let decoded = parser.append(&encoded);
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_sony_mdr_parser_resynchronizes_after_oversized_frame() {
        let mut parser = SonyMDRFrameParser::with_max_payload(10);
        let mut corrupt = vec![SonyMDRFraming::START_MARKER];
        corrupt.extend(vec![0xAA; 30]); // Exceeds max payload + envelope

        let valid = SonyMDRFraming::encode(&SonyMDRPacket::new(
            SonyMDRPacket::COMMAND_TYPE,
            0x01,
            vec![0x05],
        ));
        corrupt.extend_from_slice(&valid);

        let decoded = parser.append(&corrupt);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].payload, vec![0x05]);
    }

    #[test]
    fn test_sony_noise_mode_decode() {
        // Off
        assert_eq!(
            SonyV1NoiseMode::decode(0x00, 0x00, 0x00, 0),
            SonyV1NoiseMode::Off
        );
        // ANC
        assert_eq!(
            SonyV1NoiseMode::decode(0x01, 0x01, 0x02, 0),
            SonyV1NoiseMode::NoiseCancellation
        );
        // Wind
        assert_eq!(
            SonyV1NoiseMode::decode(0x01, 0x02, 0x01, 0),
            SonyV1NoiseMode::WindReduction
        );
        // Ambient
        assert_eq!(
            SonyV1NoiseMode::decode(0x01, 0x01, 0x00, 10),
            SonyV1NoiseMode::Ambient
        );
    }

    #[test]
    fn test_sony_equalizer_bands_encoder() {
        // Bands: 400Hz=2, 1kHz=-3, 2.5kHz=0, 6.3kHz=5, 16kHz=10, ClearBass=8
        let bands = vec![2, -3, 0, 5, 10, 8];
        let payload = SonyV1Payloads::equalizer_bands(&bands);
        assert_eq!(payload.len(), 10);
        assert_eq!(payload[0], SonyV1Opcode::EQUALIZER_SET);
        assert_eq!(payload[1], 0x01);
        assert_eq!(payload[2], 0xFF);
        assert_eq!(payload[3], 0x06);
        assert_eq!(payload[4], 18); // Clear Bass = 8 + 10 = 18
        assert_eq!(payload[5], 12); // 400Hz = 2 + 10 = 12
        assert_eq!(payload[6], 7);  // 1kHz = -3 + 10 = 7
    }
}
