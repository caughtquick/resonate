//! Sony MDR Bluetooth protocol encoding, framing, and command payload builders.
//!
//! Sony WH-1000XM / WF-1000XM / MDR series headphones communicate over RFCOMM using
//! the Sony MDR packet protocol. Packets are framed with start (`0x3E`), end (`0x3C`),
//! and escape (`0x3D`) markers with an additive wrapping checksum.
//!
//! This module provides:
//! - [`SonyMDRPacket`]: packet representation with data type, sequence number, and payload.
//! - [`SonyMDRFraming`]: frame serialization with escape byte handling and checksum calculation.
//! - [`SonyMDRFrameParser`]: resilient streaming frame parser with desync protection.
//! - [`SonyV1Opcode`]: Sony V1 command constants (EQ, NC, touch, optimizer, power).
//! - [`SonyV1EqualizerPreset`]: factory EQ presets (Bright, Excited, Vocal, Bass Boost, etc.).
//! - [`SonyV1Payloads`]: command payload builders for setting EQ bands, NC modes, and power timeouts.
//!
//! Protocol framing and command structure insights are derived from or inspired by
//! [HeadBridge](https://github.com/herenickname/HeadBridge).

use serde::{Deserialize, Serialize};

/// A decoded Sony MDR protocol packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SonyMDRPacket {
    /// Packet type (e.g. [`SonyMDRPacket::COMMAND_TYPE`] or [`SonyMDRPacket::ACK_TYPE`]).
    pub data_type: u8,
    /// Sequence number / alternating sequence bit.
    pub sequence: u8,
    /// Command or response payload bytes.
    pub payload: Vec<u8>,
}

impl SonyMDRPacket {
    /// Packet type byte for Acknowledgment packets (`0x01`).
    pub const ACK_TYPE: u8 = 0x01;
    /// Packet type byte for standard Command packets (`0x0C`).
    pub const COMMAND_TYPE: u8 = 0x0C;
    /// Secondary command packet type (`0x0E`).
    pub const COMMAND_TYPE2: u8 = 0x0E;

    /// Constructs a new [`SonyMDRPacket`].
    pub fn new(data_type: u8, sequence: u8, payload: Vec<u8>) -> Self {
        Self {
            data_type,
            sequence,
            payload,
        }
    }
}

/// Framing encoder for Sony MDR packets.
///
/// Encodes packets into wire format: `[0x3E, data_type, sequence, length (4 bytes BE), payload, checksum, 0x3C]`
/// with byte-escaping for `0x3E`, `0x3C`, and `0x3D` bytes.
pub struct SonyMDRFraming;

impl SonyMDRFraming {
    /// Start of frame delimiter (`0x3E`).
    pub const START_MARKER: u8 = 0x3E;
    /// End of frame delimiter (`0x3C`).
    pub const END_MARKER: u8 = 0x3C;
    /// Escape marker byte (`0x3D`).
    pub const ESCAPE_MARKER: u8 = 0x3D;
    /// Bitwise mask applied to escaped bytes (`0xEF`).
    pub const ESCAPE_MASK: u8 = 0xEF;

    /// Serializes a [`SonyMDRPacket`] into a framed, escaped byte sequence.
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
            if byte == Self::START_MARKER || byte == Self::END_MARKER || byte == Self::ESCAPE_MARKER
            {
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

/// Streaming parser for Sony MDR frames.
///
/// Handles chunked socket reads, unescaping bytes, validating wrapping 8-bit checksums,
/// and re-synchronizing upon malformed or oversized frames.
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
    /// Default maximum acceptable payload length (1 MiB) to guard against unbounded allocation on corrupted headers.
    pub const DEFAULT_MAX_PAYLOAD_LENGTH: usize = 1_048_576;

    /// Creates a new frame parser with default buffer limits.
    pub fn new() -> Self {
        Self::with_max_payload(Self::DEFAULT_MAX_PAYLOAD_LENGTH)
    }

    /// Creates a new frame parser with a custom maximum payload length.
    pub fn with_max_payload(max_payload_len: usize) -> Self {
        Self {
            state: ParserState::WaitingForStart,
            body: Vec::new(),
            is_escaped: false,
            max_payload_len,
        }
    }

    /// Resets the internal parser state.
    pub fn reset(&mut self) {
        self.state = ParserState::WaitingForStart;
        self.body.clear();
        self.is_escaped = false;
    }

    /// Appends incoming socket data and returns all complete, valid packets parsed.
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
                        self.body.push(byte | (!SonyMDRFraming::ESCAPE_MASK));
                        self.is_escaped = false;
                    } else if byte == SonyMDRFraming::ESCAPE_MARKER {
                        self.is_escaped = true;
                    } else if byte == SonyMDRFraming::START_MARKER {
                        // Re-sync: restart packet reading
                        self.body.clear();
                        self.is_escaped = false;
                    } else if byte == SonyMDRFraming::END_MARKER {
                        // Finished reading body: parse packet
                        if let Some(pkt) = self.parse_body() {
                            packets.push(pkt);
                        }
                        self.state = ParserState::WaitingForStart;
                        self.body.clear();
                        self.is_escaped = false;
                    } else {
                        self.body.push(byte);
                        // Prevent unbounded buffer allocation on corrupted stream without end markers
                        let max_body_len = self.max_payload_len.saturating_add(7);
                        if self.body.len() > max_body_len {
                            self.reset();
                        }
                    }
                }
            }
        }

        packets
    }

    fn parse_body(&self) -> Option<SonyMDRPacket> {
        // Body format: [data_type, sequence, len_b3, len_b2, len_b1, len_b0, ...payload..., checksum]
        if self.body.len() < 7 {
            return None;
        }

        let checksum_idx = self.body.len() - 1;
        let expected_checksum = self.body[checksum_idx];
        let computed_checksum = self.body[..checksum_idx]
            .iter()
            .fold(0u8, |acc, &b| acc.wrapping_add(b));

        if expected_checksum != computed_checksum {
            return None;
        }

        let data_type = self.body[0];
        let sequence = self.body[1];
        let payload_len = ((self.body[2] as usize) << 24)
            | ((self.body[3] as usize) << 16)
            | ((self.body[4] as usize) << 8)
            | (self.body[5] as usize);

        if payload_len > self.max_payload_len {
            return None;
        }

        let actual_payload_len = self.body.len() - 7;
        if payload_len != actual_payload_len {
            return None;
        }

        let payload = self.body[6..6 + payload_len].to_vec();
        Some(SonyMDRPacket::new(data_type, sequence, payload))
    }
}

impl Default for SonyMDRFrameParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Sony MDR V1 operation codes for audio processing, ANC, EQ, and device status.
pub struct SonyV1Opcode;

impl SonyV1Opcode {
    /// Noise cancellation / Ambient sound control set.
    pub const NC_ASM_SET: u8 = 0x22;
    /// NC/ASM state notification.
    pub const NC_ASM_NOTIFY: u8 = 0x23;
    /// Equalizer setting write.
    pub const EQUALIZER_SET: u8 = 0x68;
    /// Equalizer setting notification.
    pub const EQUALIZER_NOTIFY: u8 = 0x69;
    /// Battery and power status query.
    pub const BATTERY_GET: u8 = 0x16;
    /// Battery status reply.
    pub const BATTERY_RETURN: u8 = 0x17;
    /// Optimizer calibration start/stop.
    pub const OPTIMIZER_START: u8 = 0x88;
    /// Optimizer calibration status notification.
    pub const OPTIMIZER_STATE_NOTIFY: u8 = 0x89;
    /// Playback status query.
    pub const PLAYBACK_GET: u8 = 0xA6;
    /// Playback status reply.
    pub const PLAYBACK_RETURN: u8 = 0xA7;
    /// Playback control set.
    pub const PLAYBACK_SET: u8 = 0xA8;
    /// Playback status notification.
    pub const PLAYBACK_NOTIFY: u8 = 0xA9;
    /// Touch sensor panel query.
    pub const TOUCH_SENSOR_GET: u8 = 0xD6;
    /// Touch sensor panel reply.
    pub const TOUCH_SENSOR_RETURN: u8 = 0xD7;
    /// Touch sensor panel configuration set.
    pub const TOUCH_SENSOR_SET: u8 = 0xD8;
    /// Touch sensor panel notification.
    pub const TOUCH_SENSOR_NOTIFY: u8 = 0xD9;
    /// Audio setting query (e.g. DSEE).
    pub const AUDIO_SETTING_GET: u8 = 0xE6;
    /// Audio setting reply.
    pub const AUDIO_SETTING_RETURN: u8 = 0xE7;
    /// Audio setting configuration set.
    pub const AUDIO_SETTING_SET: u8 = 0xE8;
    /// Audio setting notification.
    pub const AUDIO_SETTING_NOTIFY: u8 = 0xE9;
    /// System setting query (e.g. Auto-Power Off).
    pub const SYSTEM_SETTING_GET: u8 = 0xF6;
    /// System setting reply.
    pub const SYSTEM_SETTING_RETURN: u8 = 0xF7;
    /// System setting configuration set.
    pub const SYSTEM_SETTING_SET: u8 = 0xF8;
    /// System setting notification.
    pub const SYSTEM_SETTING_NOTIFY: u8 = 0xF9;
}

/// Decoded noise cancellation and ambient sound modes for Sony headphones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SonyV1NoiseMode {
    /// Noise cancellation and ambient sound processing disabled.
    Off,
    /// Active Noise Cancellation enabled.
    NoiseCancellation,
    /// Ambient Sound / Transparency mode enabled.
    Ambient,
    /// Automatic wind noise reduction mode.
    WindReduction,
}

impl SonyV1NoiseMode {
    /// Decodes raw Sony telemetry parameters into a [`SonyV1NoiseMode`].
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

/// Built-in and user equalizer presets for Sony headphones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum SonyV1EqualizerPreset {
    /// Equalizer turned off (flat response).
    Off = 0x00,
    /// Bright preset emphasizing highs.
    Bright = 0x10,
    /// Excited preset with V-shaped curve.
    Excited = 0x11,
    /// Mellow preset with rolled-off highs.
    Mellow = 0x12,
    /// Relaxed preset with gentle mids.
    Relaxed = 0x13,
    /// Vocal preset emphasizing vocal clarity.
    Vocal = 0x14,
    /// Treble boost preset.
    TrebleBoost = 0x15,
    /// Bass boost preset.
    BassBoost = 0x16,
    /// Speech preset tailored for podcasts and calls.
    Speech = 0x17,
    /// Manual custom EQ curve.
    Manual = 0xA0,
    /// User Custom Preset 1.
    Custom1 = 0xA1,
    /// User Custom Preset 2.
    Custom2 = 0xA2,
}

impl SonyV1EqualizerPreset {
    /// Converts a raw byte to a [`SonyV1EqualizerPreset`] variant if valid.
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

    /// Returns the human-readable display title for this preset.
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

/// Standby and automatic power-off timeout modes for Sony headphones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SonyV1AutomaticPowerOff {
    /// Never turn off automatically.
    Off,
    /// Power off after 5 minutes of inactivity.
    After5Minutes,
    /// Power off after 30 minutes of inactivity.
    After30Minutes,
    /// Power off after 1 hour of inactivity.
    After1Hour,
    /// Power off after 3 hours of inactivity.
    After3Hours,
}

impl SonyV1AutomaticPowerOff {
    /// Returns the two-byte code representing this mode.
    pub fn code(&self) -> (u8, u8) {
        match self {
            Self::Off => (0x11, 0x00),
            Self::After5Minutes => (0x00, 0x00),
            Self::After30Minutes => (0x01, 0x01),
            Self::After1Hour => (0x02, 0x02),
            Self::After3Hours => (0x03, 0x03),
        }
    }

    /// Converts raw byte pairs into a [`SonyV1AutomaticPowerOff`] variant.
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

    /// Returns the human-readable display title for this power-off mode.
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

/// Payload builders for generating Sony V1 command byte sequences.
pub struct SonyV1Payloads;

impl SonyV1Payloads {
    /// Builds payload to set an equalizer preset.
    pub fn equalizer_preset(preset: SonyV1EqualizerPreset) -> Vec<u8> {
        vec![SonyV1Opcode::EQUALIZER_SET, 0x01, preset as u8, 0x00]
    }

    /// Builds payload to configure individual equalizer frequency bands.
    ///
    /// Frequency bands are: 400 Hz, 1 kHz, 2.5 kHz, 6.3 kHz, 16 kHz, and Clear Bass (-10 to +10 dB).
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

    /// Builds payload to enable or disable DSEE Extreme audio upscaling.
    pub fn dsee(enabled: bool) -> Vec<u8> {
        vec![
            SonyV1Opcode::AUDIO_SETTING_SET,
            0x02,
            0x00,
            if enabled { 0x01 } else { 0x00 },
        ]
    }

    /// Builds payload to enable or disable the touch sensor control panel.
    pub fn touch_sensor(enabled: bool, slot: u8) -> Vec<u8> {
        vec![
            SonyV1Opcode::TOUCH_SENSOR_SET,
            slot,
            0x01,
            if enabled { 0x01 } else { 0x00 },
        ]
    }

    /// Builds payload to start or cancel NC optimizer / atmospheric pressure calibration.
    pub fn optimizer(start: bool) -> Vec<u8> {
        vec![
            SonyV1Opcode::OPTIMIZER_START,
            0x01,
            0x00,
            if start { 0x01 } else { 0x00 },
        ]
    }

    /// Builds payload to configure automatic power-off timeout.
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
        assert_eq!(
            SonyV1NoiseMode::decode(0x00, 0x01, 0x00, 0),
            SonyV1NoiseMode::Off
        );
        assert_eq!(
            SonyV1NoiseMode::decode(0x01, 0x02, 0x01, 0),
            SonyV1NoiseMode::WindReduction
        );
        assert_eq!(
            SonyV1NoiseMode::decode(0x01, 0x01, 0x02, 0),
            SonyV1NoiseMode::NoiseCancellation
        );
        assert_eq!(
            SonyV1NoiseMode::decode(0x01, 0x01, 0x00, 10),
            SonyV1NoiseMode::Ambient
        );
    }

    #[test]
    fn test_sony_equalizer_bands_encoder() {
        let bands = vec![0, 2, -5, 10, -10, 3];
        let payload = SonyV1Payloads::equalizer_bands(&bands);
        assert_eq!(payload[0], SonyV1Opcode::EQUALIZER_SET);
        assert_eq!(payload[1], 0x01);
        assert_eq!(payload[2], 0xFF);
        assert_eq!(payload[3], 0x06);
        // Clear Bass is index 5 (+3 normalized to 13)
        assert_eq!(payload[4], 13);
        // 400Hz (0 normalized to 10)
        assert_eq!(payload[5], 10);
        // 1kHz (+2 normalized to 12)
        assert_eq!(payload[6], 12);
        // 2.5kHz (-5 normalized to 5)
        assert_eq!(payload[7], 5);
        // 6.3kHz (+10 normalized to 20)
        assert_eq!(payload[8], 20);
        // 16kHz (-10 normalized to 0)
        assert_eq!(payload[9], 0);
    }
}
