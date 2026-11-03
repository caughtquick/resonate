use serde::{Deserialize, Serialize};

pub const AAP_PSM: u16 = 0x1001;
pub const FRAME_MAGIC: [u8; 4] = [0x04, 0x00, 0x04, 0x00];
pub const HANDSHAKE_REPLY_MAGIC: [u8; 4] = [0x01, 0x00, 0x04, 0x00];

pub const HANDSHAKE_RAW_BYTES: [u8; 16] = [
    0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

pub const SET_FEATURES_PRO2_BYTES: [u8; 14] = [
    0x04, 0x00, 0x04, 0x00, 0x4D, 0x00, 0xD7, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Opcode {
    Handshake = 0x00,
    Battery = 0x04,
    EarDetection = 0x06,
    Settings = 0x09,
    SubscribeNotif = 0x0F,
    Rename = 0x1A,
    Metadata = 0x1D,
    RequestInfo = 0x30,
    ProximityKeys = 0x31,
    ConvAwarenessLvl = 0x4B,
    FeatureFlags = 0x4D,
}

impl Opcode {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum SettingId {
    MicMode = 0x01,       // 0=auto, 1=right, 2=left
    EarDetection = 0x0A,  // 1=enabled, 2=disabled
    NoiseControl = 0x0D,  // 1=Off, 2=NC, 3=Transparency, 4=Adaptive
    OneBudAnc = 0x1B,     // 1=enabled, 2=disabled
    ChimeVolume = 0x1F,   // 0..100
    ConvAware = 0x28,     // 1=enabled, 2=disabled
    AutoAncLevel = 0x2E,  // 0..100
}

impl SettingId {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum BatteryKind {
    Single = 0x01,
    Right = 0x02,
    Left = 0x04,
    Case = 0x08,
}

impl BatteryKind {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum BatteryStatus {
    Unknown = 0x00,
    Charging = 0x01,
    Discharging = 0x02,
    Disconnected = 0x04,
}

impl BatteryStatus {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0x01 => Self::Charging,
            0x02 => Self::Discharging,
            0x04 => Self::Disconnected,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum EarStatus {
    InEar = 0x00,
    OutOfEar = 0x01,
    InCase = 0x02,
    Unknown = 0xFF,
}

impl EarStatus {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0x00 => Self::InEar,
            0x01 => Self::OutOfEar,
            0x02 => Self::InCase,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ListeningMode {
    Off = 0x01,
    NoiseCancellation = 0x02,
    Transparency = 0x03,
    Adaptive = 0x04,
}

impl ListeningMode {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MicSelection {
    Auto = 0x00,
    AlwaysRight = 0x01,
    AlwaysLeft = 0x02,
}

impl MicSelection {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0x00 => Some(Self::Auto),
            0x01 => Some(Self::AlwaysRight),
            0x02 => Some(Self::AlwaysLeft),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryComponent {
    pub kind: BatteryKind,
    pub level: u8,
    pub status: BatteryStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InEarStatus {
    pub primary: EarStatus,
    pub secondary: EarStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProximityKeys {
    pub irk: [u8; 16],
    pub enc: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub opcode: u8,
    pub subop: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(opcode: u8, subop: u8, payload: Vec<u8>) -> Self {
        Self { opcode, subop, payload }
    }

    pub fn with_opcode(opcode: Opcode, subop: u8, payload: Vec<u8>) -> Self {
        Self {
            opcode: opcode as u8,
            subop,
            payload,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.payload.len());
        out.extend_from_slice(&FRAME_MAGIC);
        out.push(self.opcode);
        out.push(self.subop);
        out.extend_from_slice(&self.payload);
        out
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingNotice {
    pub id: u8,
    pub data: Vec<u8>,
}

impl SettingNotice {
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

pub fn handshake_frame() -> Frame {
    Frame::with_opcode(Opcode::Handshake, 0x01, vec![0x01, 0x00, 0x02, 0x00])
}

pub fn subscribe_notifications_frame() -> Frame {
    Frame::with_opcode(Opcode::SubscribeNotif, 0x00, vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF])
}

pub fn write_setting(id: u8, value: u8) -> Frame {
    Frame::with_opcode(Opcode::Settings, 0x00, vec![id, value, 0x00, 0x00, 0x00])
}

pub fn set_noise_control(mode: ListeningMode) -> Frame {
    write_setting(SettingId::NoiseControl as u8, mode as u8)
}

pub fn set_conversation_awareness(on: bool) -> Frame {
    write_setting(SettingId::ConvAware as u8, if on { 1 } else { 2 })
}

pub fn set_ear_detection(on: bool) -> Frame {
    write_setting(SettingId::EarDetection as u8, if on { 1 } else { 2 })
}

pub fn set_mic_mode(mic: MicSelection) -> Frame {
    write_setting(SettingId::MicMode as u8, mic as u8)
}

pub fn request_keys() -> Frame {
    Frame::with_opcode(Opcode::RequestInfo, 0x00, vec![0x05, 0x00])
}

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
            out.push(BatteryComponent { kind, level, status });
        }
        idx += 5;
    }
    out
}

pub fn parse_in_ear(payload: &[u8]) -> Option<InEarStatus> {
    if payload.len() < 2 {
        return None;
    }
    Some(InEarStatus {
        primary: EarStatus::from_u8(payload[0]),
        secondary: EarStatus::from_u8(payload[1]),
    })
}

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
        let is_name_valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_graphic() || c == ' ');
        let is_model_valid = candidate.len() == 5
            && candidate.starts_with('A')
            && candidate[1..].chars().all(|c| c.is_ascii_digit());
        if is_name_valid && is_model_valid {
            return Some(candidate.to_string());
        }
    }
    None
}

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
            0x04, 0x00, 0x04, 0x00,
            0x09, 0x00,
            SettingId::NoiseControl as u8, 0x02, 0, 0, 0,
        ];
        assert_eq!(encoded, expected);

        let parsed = Frame::parse(&encoded).expect("frame parse failed");
        assert_eq!(parsed.opcode, 0x09);
        assert_eq!(parsed.subop, 0x00);
        assert_eq!(parsed.payload, vec![SettingId::NoiseControl as u8, 0x02, 0, 0, 0]);
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
        assert!(Frame::parse(&[0xFF, 0xFF, 0xFF]).is_none());
        assert!(Frame::parse(&[0x04, 0x00, 0x05, 0x00, 0x00, 0x00]).is_none());
    }

    #[test]
    fn test_battery_decodes_example_from_doc() {
        // Right 100% discharging, Left 99% charging, Case 17% discharging.
        let payload = [
            0x03, 0x02, 0x01, 0x64, 0x02, 0x01, 0x04, 0x01, 0x63, 0x01, 0x01, 0x08, 0x01, 0x11, 0x02, 0x01,
        ];
        let comps = parse_battery(&payload);
        assert_eq!(comps.len(), 3);

        assert_eq!(comps[0].kind, BatteryKind::Right);
        assert_eq!(comps[0].level, 100);
        assert_eq!(comps[0].status, BatteryStatus::Discharging);

        assert_eq!(comps[1].kind, BatteryKind::Left);
        assert_eq!(comps[1].level, 99);
        assert_eq!(comps[1].status, BatteryStatus::Charging);

        assert_eq!(comps[2].kind, BatteryKind::Case);
        assert_eq!(comps[2].level, 17);
        assert_eq!(comps[2].status, BatteryStatus::Discharging);
    }

    #[test]
    fn test_battery_handles_short_payloads() {
        assert!(parse_battery(&[]).is_empty());
        let short_payload = [3, 2, 1, 50, 2, 1];
        let comps = parse_battery(&short_payload);
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].kind, BatteryKind::Right);
        assert_eq!(comps[0].level, 50);
    }

    #[test]
    fn test_battery_kind_single_handling() {
        let payload = [0x01, 0x01, 0x01, 0x55, 0x02, 0x01];
        let comps = parse_battery(&payload);
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].kind, BatteryKind::Single);
        assert_eq!(comps[0].level, 85);
        assert_eq!(comps[0].status, BatteryStatus::Discharging);
    }

    #[test]
    fn test_in_ear_maps_statuses() {
        let status1 = parse_in_ear(&[0x00, 0x02]).unwrap();
        assert_eq!(status1.primary, EarStatus::InEar);
        assert_eq!(status1.secondary, EarStatus::InCase);

        let status2 = parse_in_ear(&[0x01, 0x00]).unwrap();
        assert_eq!(status2.primary, EarStatus::OutOfEar);
        assert_eq!(status2.secondary, EarStatus::InEar);

        let status3 = parse_in_ear(&[0x99, 0x00]).unwrap();
        assert_eq!(status3.primary, EarStatus::Unknown);
        assert_eq!(status3.secondary, EarStatus::InEar);

        assert!(parse_in_ear(&[0x00]).is_none());
    }

    #[test]
    fn test_model_number_extraction() {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"AirPods Pro\0A2084\0");
        assert_eq!(parse_model_number(&payload).as_deref(), Some("A2084"));

        assert_eq!(parse_model_number(b"\0\0").as_deref(), None);
        assert_eq!(parse_model_number(b"AirPods Pro\0B2084\0").as_deref(), None);
        assert_eq!(parse_model_number(b"AirPods Pro\0A208\0").as_deref(), None);
    }

    #[test]
    fn test_proximity_pair_extracts_model_code() {
        let packet = [0x07, 0x19, 0x01, 0x24, 0x20, 0x00, 0x00, 0x00];
        assert_eq!(parse_proximity_pair(&packet), Some(0x2024));

        let bad_packet = [0x06, 0x19, 0x01, 0x24, 0x20, 0x00, 0x00, 0x00];
        assert_eq!(parse_proximity_pair(&bad_packet), None);
    }

    #[test]
    fn test_settings_frame_encoders_and_decoders() {
        let anc_frame = set_noise_control(ListeningMode::NoiseCancellation);
        let anc_parsed = Frame::parse(&anc_frame.encode()).unwrap();
        let anc_notice = SettingNotice::parse(&anc_parsed).unwrap();
        assert_eq!(anc_notice.id, SettingId::NoiseControl as u8);
        assert_eq!(anc_notice.data[0], ListeningMode::NoiseCancellation as u8);

        let conv_on_frame = set_conversation_awareness(true);
        let conv_on_parsed = Frame::parse(&conv_on_frame.encode()).unwrap();
        let conv_on_notice = SettingNotice::parse(&conv_on_parsed).unwrap();
        assert_eq!(conv_on_notice.id, SettingId::ConvAware as u8);
        assert_eq!(conv_on_notice.data[0], 1);

        let conv_off_frame = set_conversation_awareness(false);
        let conv_off_parsed = Frame::parse(&conv_off_frame.encode()).unwrap();
        let conv_off_notice = SettingNotice::parse(&conv_off_parsed).unwrap();
        assert_eq!(conv_off_notice.data[0], 2);
    }

    #[test]
    fn test_handshake_and_feature_raw_bytes() {
        assert_eq!(HANDSHAKE_RAW_BYTES.len(), 16);
        assert_eq!(&HANDSHAKE_RAW_BYTES[0..4], &[0x00, 0x00, 0x04, 0x00]);

        assert_eq!(SET_FEATURES_PRO2_BYTES.len(), 14);
        assert_eq!(SET_FEATURES_PRO2_BYTES[4], Opcode::FeatureFlags as u8);
        assert_eq!(SET_FEATURES_PRO2_BYTES[6], 0xD7);
    }

    #[test]
    fn test_subscribe_notifications_frame() {
        let frame = subscribe_notifications_frame();
        let encoded = frame.encode();
        let expected = [
            0x04, 0x00, 0x04, 0x00,
            0x0F, 0x00,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ];
        assert_eq!(encoded, expected);
    }
}
