use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppleModel {
    Gen1,
    Gen2,
    Gen3,
    Gen4,
    Gen4Anc,
    Pro,
    Pro2,
    Pro2UsbC,
    Pro3,
    Max,
    MaxUsbC,
    Max2,
    BeatsFitPro,
    BeatsStudioPro,
    BeatsStudioBudsPlus,
    BeatsSolo4,
    BeatsSoloBuds,
    Unknown,
}

impl AppleModel {
    pub fn from_product_code(product_code: u16) -> Self {
        match product_code {
            0x2002 => Self::Gen1,
            0x200F => Self::Gen2,
            0x2013 => Self::Gen3,
            0x201B => Self::Gen4Anc,
            0x200E => Self::Pro,
            0x2014 => Self::Pro2,
            0x2024 => Self::Pro2UsbC,
            0x2027 => Self::Pro3,
            0x200A => Self::Max,
            0x201F => Self::MaxUsbC,
            0x202D => Self::Max2,
            0x2012 => Self::BeatsFitPro,
            0x2017 => Self::BeatsStudioPro,
            0x2019 => Self::BeatsStudioBudsPlus,
            0x2025 => Self::BeatsSolo4,
            0x2026 => Self::BeatsSoloBuds,
            _ => Self::Unknown,
        }
    }

    pub fn from_modalias(modalias: &str) -> Self {
        let lower = modalias.to_lowercase();
        let Some(v_idx) = lower.find("v004c") else {
            return Self::Unknown;
        };
        let after_v = &lower[v_idx + 5..];
        let Some(p_idx) = after_v.find('p') else {
            return Self::Unknown;
        };
        let after_p = &after_v[p_idx + 1..];
        if after_p.len() < 4 {
            return Self::Unknown;
        }
        let hex_str = &after_p[..4];
        if let Ok(code) = u16::from_str_radix(hex_str, 16) {
            Self::from_product_code(code)
        } else {
            Self::Unknown
        }
    }

    pub fn from_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("beats fit pro") {
            Self::BeatsFitPro
        } else if lower.contains("beats studio pro") {
            Self::BeatsStudioPro
        } else if lower.contains("beats studio buds") {
            Self::BeatsStudioBudsPlus
        } else if lower.contains("beats solo 4") {
            Self::BeatsSolo4
        } else if lower.contains("beats solo buds") {
            Self::BeatsSoloBuds
        } else if lower.contains("pro 2") || lower.contains("pro (2") || lower.contains("pro 2nd") {
            Self::Pro2
        } else if lower.contains("pro 3") {
            Self::Pro3
        } else if lower.contains("max") {
            Self::Max
        } else if lower.contains("pro") {
            Self::Pro
        } else if lower.contains("4") && lower.contains("anc") {
            Self::Gen4Anc
        } else if lower.contains("4") {
            Self::Gen4
        } else if lower.contains("3") {
            Self::Gen3
        } else if lower.contains("2") {
            Self::Gen2
        } else if lower.contains("1") {
            Self::Gen1
        } else {
            Self::Unknown
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Gen1 => "AirPods (1st gen)",
            Self::Gen2 => "AirPods (2nd gen)",
            Self::Gen3 => "AirPods (3rd gen)",
            Self::Gen4 => "AirPods 4",
            Self::Gen4Anc => "AirPods 4 (ANC)",
            Self::Pro => "AirPods Pro",
            Self::Pro2 => "AirPods Pro (2nd gen)",
            Self::Pro2UsbC => "AirPods Pro (2nd gen, USB-C)",
            Self::Pro3 => "AirPods Pro (3rd gen)",
            Self::Max => "AirPods Max",
            Self::MaxUsbC => "AirPods Max (USB-C)",
            Self::Max2 => "AirPods Max (2nd gen)",
            Self::BeatsFitPro => "Beats Fit Pro",
            Self::BeatsStudioPro => "Beats Studio Pro",
            Self::BeatsStudioBudsPlus => "Beats Studio Buds+",
            Self::BeatsSolo4 => "Beats Solo 4",
            Self::BeatsSoloBuds => "Beats Solo Buds",
            Self::Unknown => "AirPods / Apple Accessory",
        }
    }

    pub fn capabilities(&self) -> AppleCapabilities {
        match self {
            Self::Gen1 | Self::Gen2 | Self::Gen3 | Self::Gen4 | Self::BeatsSolo4 | Self::BeatsSoloBuds => {
                AppleCapabilities {
                    model: *self,
                    has_anc: false,
                    has_transparency: false,
                    has_adaptive: false,
                    has_conv_awareness: false,
                    has_case_battery: true,
                    has_in_ear_detection: true,
                    has_ear_detection_setting: true,
                    has_loud_sound_reduction: false,
                    has_spatial_audio: *self == Self::Gen3 || *self == Self::Gen4 || *self == Self::BeatsSolo4,
                    has_press_volume: false,
                    has_press_and_hold: false,
                    has_mic_selection: true,
                    has_rename: true,
                    has_tone_on_press: false,
                }
            }
            Self::Gen4Anc => AppleCapabilities {
                model: *self,
                has_anc: true,
                has_transparency: true,
                has_adaptive: true,
                has_conv_awareness: true,
                has_case_battery: true,
                has_in_ear_detection: true,
                has_ear_detection_setting: true,
                has_loud_sound_reduction: true,
                has_spatial_audio: true,
                has_press_volume: false,
                has_press_and_hold: true,
                has_mic_selection: true,
                has_rename: true,
                has_tone_on_press: true,
            },
            Self::Pro | Self::BeatsFitPro | Self::BeatsStudioBudsPlus | Self::BeatsStudioPro => {
                AppleCapabilities {
                    model: *self,
                    has_anc: true,
                    has_transparency: true,
                    has_adaptive: false,
                    has_conv_awareness: false,
                    has_case_battery: *self != Self::BeatsStudioPro,
                    has_in_ear_detection: true,
                    has_ear_detection_setting: true,
                    has_loud_sound_reduction: false,
                    has_spatial_audio: true,
                    has_press_volume: false,
                    has_press_and_hold: true,
                    has_mic_selection: true,
                    has_rename: true,
                    has_tone_on_press: true,
                }
            }
            Self::Pro2 | Self::Pro2UsbC | Self::Pro3 => AppleCapabilities {
                model: *self,
                has_anc: true,
                has_transparency: true,
                has_adaptive: true,
                has_conv_awareness: true,
                has_case_battery: true,
                has_in_ear_detection: true,
                has_ear_detection_setting: true,
                has_loud_sound_reduction: true,
                has_spatial_audio: true,
                has_press_volume: true,
                has_press_and_hold: true,
                has_mic_selection: true,
                has_rename: true,
                has_tone_on_press: true,
            },
            Self::Max | Self::MaxUsbC | Self::Max2 => AppleCapabilities {
                model: *self,
                has_anc: true,
                has_transparency: true,
                has_adaptive: false,
                has_conv_awareness: false,
                has_case_battery: false,
                has_in_ear_detection: false,
                has_ear_detection_setting: false,
                has_loud_sound_reduction: true,
                has_spatial_audio: true,
                has_press_volume: false,
                has_press_and_hold: true,
                has_mic_selection: false,
                has_rename: true,
                has_tone_on_press: false,
            },
            Self::Unknown => AppleCapabilities::conservative(*self),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppleCapabilities {
    pub model: AppleModel,
    pub has_anc: bool,
    pub has_transparency: bool,
    pub has_adaptive: bool,
    pub has_conv_awareness: bool,
    pub has_case_battery: bool,
    pub has_in_ear_detection: bool,
    pub has_ear_detection_setting: bool,
    pub has_loud_sound_reduction: bool,
    pub has_spatial_audio: bool,
    pub has_press_volume: bool,
    pub has_press_and_hold: bool,
    pub has_mic_selection: bool,
    pub has_rename: bool,
    pub has_tone_on_press: bool,
}

impl AppleCapabilities {
    pub fn conservative(model: AppleModel) -> Self {
        Self {
            model,
            has_anc: false,
            has_transparency: false,
            has_adaptive: false,
            has_conv_awareness: false,
            has_case_battery: true,
            has_in_ear_detection: false,
            has_ear_detection_setting: false,
            has_loud_sound_reduction: false,
            has_spatial_audio: false,
            has_press_volume: false,
            has_press_and_hold: false,
            has_mic_selection: false,
            has_rename: true,
            has_tone_on_press: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_product_codes_resolve_to_their_model() {
        assert_eq!(AppleModel::from_product_code(0x2024), AppleModel::Pro2UsbC);
        assert_eq!(AppleModel::from_product_code(0x2014), AppleModel::Pro2);
        assert_eq!(AppleModel::from_product_code(0x200E), AppleModel::Pro);
        assert_eq!(AppleModel::from_product_code(0x200A), AppleModel::Max);
        assert_eq!(AppleModel::from_product_code(0x201F), AppleModel::MaxUsbC);
        assert_eq!(AppleModel::from_product_code(0x2013), AppleModel::Gen3);
        assert_eq!(AppleModel::from_product_code(0x200F), AppleModel::Gen2);
        assert_eq!(AppleModel::from_product_code(0x2002), AppleModel::Gen1);
        assert_eq!(AppleModel::from_product_code(0x2012), AppleModel::BeatsFitPro);
        assert_eq!(AppleModel::from_product_code(0x2017), AppleModel::BeatsStudioPro);
        assert_eq!(AppleModel::from_product_code(0x2019), AppleModel::BeatsStudioBudsPlus);
        assert_eq!(AppleModel::from_product_code(0x2025), AppleModel::BeatsSolo4);
        assert_eq!(AppleModel::from_product_code(0x2026), AppleModel::BeatsSoloBuds);
        assert_eq!(AppleModel::from_product_code(0x9999), AppleModel::Unknown);
    }

    #[test]
    fn test_modalias_parsing() {
        assert_eq!(
            AppleModel::from_modalias("bluetooth:v004Cp2024d0600"),
            AppleModel::Pro2UsbC
        );
        assert_eq!(
            AppleModel::from_modalias("BLUETOOTH:V004CP2014D0600"),
            AppleModel::Pro2
        );
        assert_eq!(
            AppleModel::from_modalias("bluetooth:v004Cp200Ad0600"),
            AppleModel::Max
        );
        assert_eq!(AppleModel::from_modalias("usb:v05ACp0223"), AppleModel::Unknown);
        assert_eq!(AppleModel::from_modalias(""), AppleModel::Unknown);
    }

    #[test]
    fn test_name_fallback() {
        assert_eq!(AppleModel::from_name("Abhijit's AirPods Pro"), AppleModel::Pro);
        assert_eq!(AppleModel::from_name("AirPods Pro (2nd generation)"), AppleModel::Pro2);
        assert_eq!(AppleModel::from_name("AirPods Max - Silver"), AppleModel::Max);
        assert_eq!(AppleModel::from_name("Beats Fit Pro"), AppleModel::BeatsFitPro);
        assert_eq!(AppleModel::from_name("Beats Studio Pro"), AppleModel::BeatsStudioPro);
        assert_eq!(AppleModel::from_name("Some Random Headphone"), AppleModel::Unknown);
    }

    #[test]
    fn test_pro2_capabilities() {
        let caps = AppleModel::Pro2.capabilities();
        assert!(caps.has_anc);
        assert!(caps.has_transparency);
        assert!(caps.has_adaptive);
        assert!(caps.has_conv_awareness);
        assert!(caps.has_press_volume);
        assert!(caps.has_in_ear_detection);
    }

    #[test]
    fn test_airpods_max_capabilities() {
        let caps = AppleModel::Max.capabilities();
        assert!(caps.has_anc);
        assert!(caps.has_transparency);
        assert!(!caps.has_adaptive);
        assert!(!caps.has_conv_awareness);
        assert!(!caps.has_press_volume);
        assert!(!caps.has_case_battery);
        assert!(!caps.has_in_ear_detection);
    }

    #[test]
    fn test_airpods_gen3_capabilities() {
        let caps = AppleModel::Gen3.capabilities();
        assert!(!caps.has_anc);
        assert!(!caps.has_transparency);
        assert!(!caps.has_adaptive);
        assert!(caps.has_case_battery);
        assert!(caps.has_in_ear_detection);
        assert!(caps.has_spatial_audio);
    }
}
