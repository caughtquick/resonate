//! Low-level Bluetooth address representation and libc socket address structures.
//!
//! Provides the [`BdAddr`] type representing a 6-byte Bluetooth MAC address,
//! along with C-compatible `sockaddr` structures for AF_BLUETOOTH L2CAP and RFCOMM sockets.

/// A 6-byte Bluetooth Hardware Device Address (BD_ADDR).
///
/// Note that on Linux kernel Bluetooth APIs, the bytes are stored in reverse (little-endian) order:
/// `b[0]` is the least significant byte and `b[5]` is the most significant byte.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BdAddr {
    /// Raw byte storage for the MAC address (little-endian / reverse order as expected by BlueZ / Linux kernel).
    pub b: [u8; 6],
}

impl BdAddr {
    /// An empty/any address (`00:00:00:00:00:00`), used when binding to any local Bluetooth adapter.
    pub const ANY: Self = Self { b: [0; 6] };

    /// Parses a standard colon-delimited Bluetooth MAC string (e.g. `"AA:BB:CC:DD:EE:FF"`).
    ///
    /// Returns `Some(BdAddr)` if valid, or `None` if the format is invalid or parts cannot be parsed as hex.
    ///
    /// # Examples
    ///
    /// ```
    /// use resonate::transport::bluetooth_addr::BdAddr;
    ///
    /// let addr = BdAddr::parse("6C:12:70:3F:8E:AE").unwrap();
    /// assert_eq!(addr.to_string(), "6C:12:70:3F:8E:AE");
    /// ```
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

/// Address family constant for Bluetooth sockets (`AF_BLUETOOTH = 31`).
pub const AF_BLUETOOTH: libc::sa_family_t = 31;

/// Protocol constant for L2CAP sockets (`BTPROTO_L2CAP = 0`).
pub const BTPROTO_L2CAP: i32 = 0;

/// Protocol constant for RFCOMM sockets (`BTPROTO_RFCOMM = 3`).
pub const BTPROTO_RFCOMM: i32 = 3;

/// C-compatible socket address structure for L2CAP sockets (`sockaddr_l2`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SockAddrL2 {
    /// Socket family (always [`AF_BLUETOOTH`]).
    pub l2_family: libc::sa_family_t,
    /// Protocol/Service Multiplexer (in little-endian).
    pub l2_psm: u16,
    /// Remote or local Bluetooth device address.
    pub l2_bdaddr: BdAddr,
    /// Channel ID (typically 0).
    pub l2_cid: u16,
    /// Address type (0 for standard BR/EDR address).
    pub l2_bdaddr_type: u8,
}

/// C-compatible socket address structure for RFCOMM sockets (`sockaddr_rc`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SockAddrRc {
    /// Socket family (always [`AF_BLUETOOTH`]).
    pub rc_family: libc::sa_family_t,
    /// Remote or local Bluetooth device address.
    pub rc_bdaddr: BdAddr,
    /// RFCOMM channel number (1-30).
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
