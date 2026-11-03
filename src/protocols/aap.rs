//! Apple Accessory Protocol (AAP) framing and packet parsing over L2CAP.
//!
//! Apple AirPods and Beats accessories communicate using a proprietary packet protocol
//! over L2CAP PSM `0x1001`. Frames begin with a 4-byte magic sequence `[0x04, 0x00, 0x04, 0x00]`
//! (or `[0x01, 0x00, 0x04, 0x00]` for initial handshake responses), followed by a 1-byte
//! opcode, 1-byte sub-opcode, and variable-length payload.
//!
//! This module implements encoding and decoding for:
//! - Connection handshakes and feature flags ([`HANDSHAKE_RAW_BYTES`], [`SET_FEATURES_PRO2_BYTES`])
//! - Telemetry opcodes ([`Opcode`]) including battery, in-ear detection, and metadata
//! - Hardware setting writes and notifications ([`SettingId`], [`write_setting`])
//! - Multi-component battery decoding ([`BatteryComponent`], [`parse_battery`])
//! - Dual earbud wear state decoding ([`InEarStatus`], [`parse_in_ear`])

use serde::{Deserialize, Serialize};

/// L2CAP Protocol/Service Multiplexer (PSM) for the Apple Accessory Protocol (`0x1001`).
pub const AAP_PSM: u16 = 0x1001;

/// 4-byte magic preamble for standard AAP request and notification frames (`0x04 0x00 0x04 0x00`).
pub const FRAME_MAGIC: [u8; 4] = [0x04, 0x00, 0x04, 0x00];

/// 4-byte magic preamble sent by AirPods in response to the initial handshake (`0x01 0x00 0x04 0x00`).
pub const HANDSHAKE_REPLY_MAGIC: [u8; 4] = [0x01, 0x00, 0x04, 0x00];

/// Raw 16-byte initial handshake packet sent after opening the L2CAP connection.
pub const HANDSHAKE_RAW_BYTES: [u8; 16] = [
    0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// 14-byte feature negotiation packet setting bitmask `0xD7` enabling AirPods Pro 2 capabilities.
pub const SET_FEATURES_PRO2_BYTES: [u8; 14] = [
    0x04, 0x00, 0x04, 0x00, 0x4D, 0x00, 0xD7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// Apple Accessory Protocol operation codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Opcode {
    /// Initial session handshake.
    Handshake = 0x00,
    /// Multi-component battery telemetry (left, right, case).
    Battery = 0x04,
    /// In-ear wear status update for each earbud.
    EarDetection = 0x06,
    /// Setting read, write, or notification update.
    Settings = 0x09,
    /// Subscribe to accessory event notifications.
    SubscribeNotif = 0x0F,
    /// Accessory name change.
    Rename = 0x1A,
    /// Device metadata (model number, hardware revision, serial numbers).
    Metadata = 0x1D,
    /// Request cryptographic proximity keys or device info.
    RequestInfo = 0x30,
    /// Proximity pairing keys (IRK and ENC).
    ProximityKeys = 0x31,
    /// Conversation awareness volume attenuation level.
    ConvAwarenessLvl = 0x4B,
    /// Feature flag negotiation.
    FeatureFlags = 0x4D,
}

impl Opcode {
    /// Converts a raw byte to an [`Opcode`] variant if known.
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x00 => Some(Self::Handshake),
            0x04 => Some(Self::Battery),
            0x06 => Some(Self::EarDetection),
            0x09 => Some(Self::Settings),
            0x0F => Some(Self::SubscribeNotif),
            0x1A => Some(Self::Rename),
            0x1D => Some(Self::Metadata),
            0x30 => Some(Self::RequestInfo),
            0x31 => Some(Self::ProximityKeys),
            0x4B => Some(Self::ConvAwarenessLvl),
            0x4D => Some(Self::FeatureFlags),
            _ => None,
        }
    }
}

/// Identifiers for configurable settings within AAP opcode `0x09`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum SettingId {
    /// Active microphone selection (`0 = Auto, 1 = Right, 2 = Left`).
    MicMode = 0x01,
    /// Automatic ear detection setting (`1 = Enabled, 2 = Disabled`).
    EarDetection = 0x0A,
    /// Active noise control mode (`1 = Off, 2 = ANC, 3 = Transparency, 4 = Adaptive`).
    NoiseControl = 0x0D,
    /// Noise cancellation with only one earbud inserted (`1 = Enabled, 2 = Disabled`).
    OneBudAnc = 0x1B,
    /// Chime tone volume on case and buds (`0..=100`).
    ChimeVolume = 0x1F,
    /// Conversation Awareness toggle (`1 = Enabled, 2 = Disabled`).
    ConvAware = 0x28,
    /// Adaptive ANC level adjustment (`0..=100`).
    AutoAncLevel = 0x2E,
}

impl SettingId {
    /// Converts a raw byte to a [`SettingId`] variant if known.
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x01 => Some(Self::MicMode),
            0x0A => Some(Self::EarDetection),
            0x0D => Some(Self::NoiseControl),
            0x1B => Some(Self::OneBudAnc),
            0x1F => Some(Self::ChimeVolume),
            0x28 => Some(Self::ConvAware),
            0x2E => Some(Self::AutoAncLevel),
            _ => None,
        }
    }
}

/// Identifies which physical component a battery telemetry entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum BatteryKind {
    /// Single battery unit (for over-ear models like AirPods Max).
    Single = 0x01,
    /// Right earbud.
    Right = 0x02,
    /// Left earbud.
    Left = 0x04,
    /// Charging case.
    Case = 0x08,
}

impl BatteryKind {
    /// Converts a raw byte to a [`BatteryKind`] variant if recognized.
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x01 => Some(Self::Single),
            0x02 => Some(Self::Right),
            0x04 => Some(Self::Left),
            0x08 => Some(Self::Case),
            _ => None,
        }
    }
}

/// Charging state of an individual battery component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum BatteryStatus {
    /// State is unspecified or pending.
    Unknown = 0x00,
    /// Component is actively charging.
    Charging = 0x01,
    /// Component is discharging on battery power.
    Discharging = 0x02,
    /// Component is disconnected or inactive.
    Disconnected = 0x04,
}

impl BatteryStatus {
    /// Converts a raw byte into a [`BatteryStatus`].
    pub fn from_u8(val: u8) -> Self {
        match val {
            0x01 => Self::Charging,
            0x02 => Self::Discharging,
            0x04 => Self::Disconnected,
            _ => Self::Unknown,
        }
    }
}

/// In-ear wear placement status of an individual earbud.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EarStatus {
    /// Earbud is placed securely in the ear canal.
    InEar = 0x00,
    /// Earbud is outside the ear but out of the case.
    OutOfEar = 0x01,
    /// Earbud is docked inside the charging case.
    InCase = 0x02,
    /// Earbud status is unknown or disconnected.
    Unknown = 0xFF,
}

impl EarStatus {
    /// Converts a raw byte into an [`EarStatus`].
    pub fn from_u8(val: u8) -> Self {
        match val {
            0x00 => Self::InEar,
            0x01 => Self::OutOfEar,
            0x02 => Self::InCase,
            _ => Self::Unknown,
        }
    }

    /// Returns a string representation (`"in_ear"`, `"out_of_ear"`, `"in_case"`, `"unknown"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InEar => "in_ear",
            Self::OutOfEar => "out_of_ear",
            Self::InCase => "in_case",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for EarStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Active listening / noise control modes for Apple AirPods and Beats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ListeningMode {
    /// Passive isolation / all active noise processing turned off.
    Off = 0x01,
    /// Active Noise Cancellation (ANC).
    NoiseCancellation = 0x02,
    /// Transparency / Ambient sound mode.
    Transparency = 0x03,
    /// Adaptive audio mode dynamically balancing ANC and Transparency.
    Adaptive = 0x04,
}

impl ListeningMode {
    /// Converts a raw byte to a [`ListeningMode`] variant if valid.
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x01 => Some(Self::Off),
            0x02 => Some(Self::NoiseCancellation),
            0x03 => Some(Self::Transparency),
            0x04 => Some(Self::Adaptive),
            _ => None,
        }
    }
}

/// Microphone routing selection for dual-bud Apple accessories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MicSelection {
    /// Automatically switch microphone based on wear position.
    Auto = 0x00,
    /// Always use right earbud microphone.
    AlwaysRight = 0x01,
    /// Always use left earbud microphone.
    AlwaysLeft = 0x02,
}

impl MicSelection {
    /// Converts a raw byte to a [`MicSelection`] variant if valid.
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x00 => Some(Self::Auto),
            0x01 => Some(Self::AlwaysRight),
            0x02 => Some(Self::AlwaysLeft),
            _ => None,
        }
    }
}

/// Battery telemetry report for an individual hardware component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryComponent {
    /// Which physical unit (Single, Right, Left, Case).
    pub kind: BatteryKind,
    /// Percentage level from 0 to 100.
    pub level: u8,
    /// Charging status (Charging, Discharging, Disconnected).
    pub status: BatteryStatus,
}

/// In-ear wear placement status of both primary and secondary earbuds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InEarStatus {
    /// Wear status of primary earbud.
    pub primary: EarStatus,
    /// Wear status of secondary earbud.
    pub secondary: EarStatus,
}

impl InEarStatus {
    /// Returns true if at least one earbud is actively in an ear.
    pub fn is_worn(&self) -> bool {
        self.primary == EarStatus::InEar || self.secondary == EarStatus::InEar
    }

    /// Returns the number of earbuds currently in the ear (0, 1, or 2).
    pub fn in_ear_count(&self) -> usize {
        (if self.primary == EarStatus::InEar {
            1
        } else {
            0
        }) + (if self.secondary == EarStatus::InEar {
            1
        } else {
            0
        })
    }

    /// Returns true if both earbuds are currently docked inside the charging case.
    pub fn are_both_in_case(&self) -> bool {
        self.primary == EarStatus::InCase && self.secondary == EarStatus::InCase
    }
}

/// Identity Resolving Key (IRK) and encryption key extracted from proximity pairing frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProximityKeys {
    /// 16-byte Identity Resolving Key.
    pub irk: [u8; 16],
    /// 16-byte Encryption Key.
    pub enc: [u8; 16],
}

/// A parsed Apple Accessory Protocol (AAP) frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Operation code.
    pub opcode: u8,
    /// Sub-operation code or sequence number.
    pub subop: u8,
    /// Variable length frame payload.
    pub payload: Vec<u8>,
}

impl Frame {
    /// Constructs a new raw frame with explicit numeric opcode and sub-opcode.
    pub fn new(opcode: u8, subop: u8, payload: Vec<u8>) -> Self {
        Self {
            opcode,
            subop,
            payload,
        }
    }

    /// Constructs a frame using the typed [`Opcode`] enum.
    pub fn with_opcode(opcode: Opcode, subop: u8, payload: Vec<u8>) -> Self {
        Self {
            opcode: opcode as u8,
            subop,
            payload,
        }
    }

    /// Encodes this frame into a byte vector prefixed by [`FRAME_MAGIC`].
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.payload.len());
        out.extend_from_slice(&FRAME_MAGIC);
        out.push(self.opcode);
        out.push(self.subop);
        out.extend_from_slice(&self.payload);
        out
    }

    /// Parses a raw byte slice into a [`Frame`], validating the 4-byte header magic.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 6 {
            return None;
        }
        // Lenient on byte 0 to handle steady state 04 00 04 00 and handshake reply 01 00 04 00
        if data[1] != 0x00 || data[2] != 0x04 || data[3] != 0x00 {
            return None;
        }
        Some(Self {
            opcode: data[4],
            subop: data[5],
            payload: data[6..].to_vec(),
        })
    }
}

/// A parsed setting notification frame carrying a setting identifier and payload data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingNotice {
    /// The setting identifier (corresponds to [`SettingId`]).
    pub id: u8,
    /// Raw payload bytes associated with this setting update.
    pub data: Vec<u8>,
}

impl SettingNotice {
    /// Extracts a [`SettingNotice`] from a received [`Frame`] if opcode is [`Opcode::Settings`].
    pub fn parse(frame: &Frame) -> Option<Self> {
        if frame.opcode != Opcode::Settings as u8 || frame.payload.is_empty() {
            return None;
        }
        Some(Self {
            id: frame.payload[0],
            data: frame.payload[1..].to_vec(),
        })
    }
}

/// Builds an initial handshake frame.
pub fn handshake_frame() -> Frame {
    Frame::with_opcode(Opcode::Handshake, 0x01, vec![0x01, 0x00, 0x02, 0x00])
}

/// Builds a notification subscription frame requesting all accessory telemetry broadcasts.
pub fn subscribe_notifications_frame() -> Frame {
    Frame::with_opcode(
        Opcode::SubscribeNotif,
        0x00,
        vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
    )
}

/// Builds an AAP setting write command frame.
pub fn write_setting(id: u8, value: u8) -> Frame {
    Frame::with_opcode(Opcode::Settings, 0x00, vec![id, value, 0x00, 0x00, 0x00])
}

/// Builds a frame to change the noise control mode.
pub fn set_noise_control(mode: ListeningMode) -> Frame {
    write_setting(SettingId::NoiseControl as u8, mode as u8)
}

/// Builds a frame to toggle Conversation Awareness on or off.
pub fn set_conversation_awareness(on: bool) -> Frame {
    write_setting(SettingId::ConvAware as u8, if on { 1 } else { 2 })
}

/// Builds a frame to toggle in-ear detection on or off.
pub fn set_ear_detection(on: bool) -> Frame {
    write_setting(SettingId::EarDetection as u8, if on { 1 } else { 2 })
}

/// Builds a frame to configure the microphone selection mode.
pub fn set_mic_mode(mic: MicSelection) -> Frame {
    write_setting(SettingId::MicMode as u8, mic as u8)
}

/// Builds a frame to request proximity pairing keys from the accessory.
pub fn request_keys() -> Frame {
    Frame::with_opcode(Opcode::RequestInfo, 0x00, vec![0x05, 0x00])
}

/// Parses an AAP battery telemetry payload into a list of [`BatteryComponent`] entries.
pub fn parse_battery(payload: &[u8]) -> Vec<BatteryComponent> {
    if payload.is_empty() {
        return Vec::new();
    }
    let declared = payload[0] as usize;
    let max_fit = (payload.len() - 1) / 5;
    let count = declared.min(max_fit);
    let mut out = Vec::with_capacity(count);

    let mut idx = 1;
    for _ in 0..count {
        if idx + 5 > payload.len() {
            break;
        }
        let kind_raw = payload[idx];
        let level = payload[idx + 2];
        let status_raw = payload[idx + 3];

        if let Some(kind) = BatteryKind::from_u8(kind_raw) {
            let status = BatteryStatus::from_u8(status_raw);
            out.push(BatteryComponent {
                kind,
                level,
                status,
            });
        }
        idx += 5;
    }
    out
}

/// Parses an AAP in-ear detection payload into primary and secondary ear wear statuses.
pub fn parse_in_ear(payload: &[u8]) -> Option<InEarStatus> {
    if payload.len() < 2 {
        return None;
    }
    Some(InEarStatus {
        primary: EarStatus::from_u8(payload[0]),
        secondary: EarStatus::from_u8(payload[1]),
    })
}

/// Extracts an Apple model number (e.g. `"A2698"`) from metadata descriptor payload bytes.
pub fn parse_model_number(payload: &[u8]) -> Option<String> {
    let slices: Vec<&[u8]> = payload.split(|&b| b == 0).collect();
    let strings: Vec<&str> = slices
        .iter()
        .filter_map(|s| std::str::from_utf8(s).ok())
        .collect();

    if strings.len() < 2 {
        return None;
    }

    for i in 0..(strings.len() - 1) {
        let name = strings[i];
        let candidate = strings[i + 1];
        let is_name_valid =
            !name.is_empty() && name.chars().all(|c| c.is_ascii_graphic() || c == ' ');
        let is_model_valid = candidate.len() == 5
            && candidate.starts_with('A')
            && candidate[1..].chars().all(|c| c.is_ascii_digit());
        if is_name_valid && is_model_valid {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Parses cryptographic IRK and encryption keys from proximity key reply payloads.
pub fn parse_keys(payload: &[u8]) -> Option<ProximityKeys> {
    if payload.len() <= 1 {
        return None;
    }
    let mut irk: Option<[u8; 16]> = None;
    let mut enc: Option<[u8; 16]> = None;
    let mut rest = &payload[1..];

    while rest.len() >= 4 {
        let kind = rest[0];
        let len = ((rest[1] as usize) << 8) | (rest[2] as usize);
        if rest.len() < 4 + len {
            break;
        }
        let body = &rest[4..4 + len];
        if len == 16 {
            let mut key_bytes = [0u8; 16];
            key_bytes.copy_from_slice(body);
            key_bytes.reverse(); // Reverse LSB to MSB spec order
            if kind == 1 {
                irk = Some(key_bytes);
            } else if kind == 4 {
                enc = Some(key_bytes);
            }
        }
        rest = &rest[4 + len..];
    }

    match (irk, enc) {
        (Some(i), Some(e)) => Some(ProximityKeys { irk: i, enc: e }),
        _ => None,
    }
}

/// Extracts a 16-bit Apple product code from proximity pair advertisement payload bytes.
pub fn parse_proximity_pair(data: &[u8]) -> Option<u16> {
    if data.len() < 8 || data[0] != 0x07 || data[1] != 0x19 {
        return None;
    }
    Some((data[3] as u16) | ((data[4] as u16) << 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_roundtrip() {
        let frame = Frame::with_opcode(
            Opcode::Settings,
            0x00,
            vec![SettingId::NoiseControl as u8, 0x02, 0, 0, 0],
        );
        let encoded = frame.encode();
        let expected = [
            0x04,
            0x00,
            0x04,
            0x00,
            0x09,
            0x00,
            SettingId::NoiseControl as u8,
            0x02,
            0,
            0,
            0,
        ];
        assert_eq!(encoded, expected);

        let parsed = Frame::parse(&encoded).expect("frame parse failed");
        assert_eq!(parsed.opcode, 0x09);
        assert_eq!(parsed.subop, 0x00);
        assert_eq!(
            parsed.payload,
            vec![SettingId::NoiseControl as u8, 0x02, 0, 0, 0]
        );
    }

    #[test]
    fn test_frame_parse_accepts_handshake_reply_magic() {
        let bytes = [0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x03, 0x00];
        let parsed = Frame::parse(&bytes).expect("handshake reply parse failed");
        assert_eq!(parsed.opcode, 0x00);
    }

    #[test]
    fn test_frame_parse_rejects_malformed() {
        assert!(Frame::parse(&[]).is_none());
        assert!(Frame::parse(&[0x04, 0x00]).is_none());
        assert!(Frame::parse(&[0x04, 0x01, 0x04, 0x00, 0x00, 0x00]).is_none());
    }

    #[test]
    fn test_battery_decodes_example_from_doc() {
        let payload = [
            0x03, 0x04, 0x00, 0x48, 0x02, 0x00, 0x02, 0x00, 0x58, 0x02, 0x00, 0x08, 0x00, 0x50,
            0x01, 0x00,
        ];
        let comps = parse_battery(&payload);
        assert_eq!(comps.len(), 3);
        assert_eq!(comps[0].kind, BatteryKind::Left);
        assert_eq!(comps[0].level, 72);
        assert_eq!(comps[0].status, BatteryStatus::Discharging);

        assert_eq!(comps[1].kind, BatteryKind::Right);
        assert_eq!(comps[1].level, 88);
        assert_eq!(comps[1].status, BatteryStatus::Discharging);

        assert_eq!(comps[2].kind, BatteryKind::Case);
        assert_eq!(comps[2].level, 80);
        assert_eq!(comps[2].status, BatteryStatus::Charging);
    }

    #[test]
    fn test_battery_kind_single_handling() {
        let payload = [0x01, 0x01, 0x00, 0x64, 0x02, 0x00];
        let comps = parse_battery(&payload);
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].kind, BatteryKind::Single);
        assert_eq!(comps[0].level, 100);
        assert_eq!(comps[0].status, BatteryStatus::Discharging);
    }

    #[test]
    fn test_battery_handles_short_payloads() {
        assert!(parse_battery(&[]).is_empty());
        let payload = [0x02, 0x04, 0x00];
        assert!(parse_battery(&payload).is_empty());
    }

    #[test]
    fn test_in_ear_maps_statuses() {
        let payload = [0x00, 0x01];
        let status = parse_in_ear(&payload).expect("in ear parse failed");
        assert_eq!(status.primary, EarStatus::InEar);
        assert_eq!(status.secondary, EarStatus::OutOfEar);
        assert!(status.is_worn());
        assert_eq!(status.in_ear_count(), 1);
        assert!(!status.are_both_in_case());

        let case_payload = [0x02, 0x02];
        let case_status = parse_in_ear(&case_payload).expect("in ear parse failed");
        assert_eq!(case_status.primary, EarStatus::InCase);
        assert_eq!(case_status.secondary, EarStatus::InCase);
        assert!(!case_status.is_worn());
        assert_eq!(case_status.in_ear_count(), 0);
        assert!(case_status.are_both_in_case());

        let both_in_payload = [0x00, 0x00];
        let both_in = parse_in_ear(&both_in_payload).unwrap();
        assert_eq!(both_in.in_ear_count(), 2);
    }

    #[test]
    fn test_settings_frame_encoders_and_decoders() {
        let frame = set_noise_control(ListeningMode::NoiseCancellation);
        assert_eq!(frame.opcode, Opcode::Settings as u8);
        assert_eq!(frame.payload[0], SettingId::NoiseControl as u8);
        assert_eq!(frame.payload[1], ListeningMode::NoiseCancellation as u8);

        let notice = SettingNotice::parse(&frame).expect("notice parse failed");
        assert_eq!(notice.id, SettingId::NoiseControl as u8);
        assert_eq!(notice.data, vec![0x02, 0, 0, 0]);

        let conv_frame = set_conversation_awareness(true);
        assert_eq!(conv_frame.payload[0], SettingId::ConvAware as u8);
        assert_eq!(conv_frame.payload[1], 1);

        let ear_frame = set_ear_detection(false);
        assert_eq!(ear_frame.payload[0], SettingId::EarDetection as u8);
        assert_eq!(ear_frame.payload[1], 2);

        let mic_frame = set_mic_mode(MicSelection::AlwaysRight);
        assert_eq!(mic_frame.payload[0], SettingId::MicMode as u8);
        assert_eq!(mic_frame.payload[1], MicSelection::AlwaysRight as u8);
    }

    #[test]
    fn test_subscribe_notifications_frame() {
        let frame = subscribe_notifications_frame();
        assert_eq!(frame.opcode, Opcode::SubscribeNotif as u8);
        assert_eq!(frame.payload, vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn test_model_number_extraction() {
        let mut sample = Vec::new();
        sample.extend_from_slice(b"AirPods Pro\0A2698\x003A283\0");
        let model = parse_model_number(&sample);
        assert_eq!(model.as_deref(), Some("A2698"));

        let invalid = b"AirPods Pro\0XYZ12\0";
        assert_eq!(parse_model_number(invalid), None);
    }

    #[test]
    fn test_proximity_pair_extracts_model_code() {
        let mut raw = vec![0x07, 0x19, 0x01, 0x24, 0x20, 0x00, 0x00, 0x00];
        let code = parse_proximity_pair(&raw);
        assert_eq!(code, Some(0x2024));

        raw[0] = 0x00;
        assert_eq!(parse_proximity_pair(&raw), None);
    }

    #[test]
    fn test_handshake_and_feature_raw_bytes() {
        assert_eq!(HANDSHAKE_RAW_BYTES.len(), 16);
        assert_eq!(SET_FEATURES_PRO2_BYTES.len(), 14);
        assert_eq!(SET_FEATURES_PRO2_BYTES[4], Opcode::FeatureFlags as u8);
        assert_eq!(SET_FEATURES_PRO2_BYTES[6], 0xD7);
    }
}
