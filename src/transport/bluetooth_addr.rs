#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BdAddr {
    pub b: [u8; 6],
}

impl BdAddr {
    pub const ANY: Self = Self { b: [0; 6] };

    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 6 {
            return None;
        }
        let mut b = [0u8; 6];
        for (i, part) in parts.iter().enumerate() {
            let byte = u8::from_str_radix(part, 16).ok()?;
            // Linux kernel Bluetooth addresses are stored in reverse byte order
            b[5 - i] = byte;
        }
        Some(Self { b })
    }
}

impl std::fmt::Display for BdAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.b[5], self.b[4], self.b[3], self.b[2], self.b[1], self.b[0]
        )
    }
}

pub const AF_BLUETOOTH: libc::sa_family_t = 31;
pub const BTPROTO_L2CAP: i32 = 0;
pub const BTPROTO_RFCOMM: i32 = 3;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SockAddrL2 {
    pub l2_family: libc::sa_family_t,
    pub l2_psm: u16,
    pub l2_bdaddr: BdAddr,
    pub l2_cid: u16,
    pub l2_bdaddr_type: u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SockAddrRc {
    pub rc_family: libc::sa_family_t,
    pub rc_bdaddr: BdAddr,
    pub rc_channel: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bdaddr_parse_and_format() {
        let mac = "6C:12:70:3F:8E:AE";
        let addr = BdAddr::parse(mac).expect("parse failed");
        assert_eq!(addr.b, [0xAE, 0x8E, 0x3F, 0x70, 0x12, 0x6C]);
        assert_eq!(addr.to_string(), "6C:12:70:3F:8E:AE");
    }

    #[test]
    fn test_bdaddr_invalid() {
        assert!(BdAddr::parse("invalid").is_none());
        assert!(BdAddr::parse("00:11:22:33:44").is_none());
        assert!(BdAddr::parse("00:11:22:33:44:GG").is_none());
    }
}
