//! Transport abstraction layer for Bluetooth communication.
//!
//! Provides socket and GATT interfaces for interacting with Bluetooth devices:
//! - [`bluetooth_addr`]: Bluetooth MAC address representation (`BdAddr`) and socket address structures.
//! - [`l2cap`]: Asynchronous L2CAP seqpacket socket wrapper for Apple Accessory Protocol.
//! - [`rfcomm`]: Asynchronous RFCOMM stream socket wrapper for Sony MDR protocol.
//! - [`bluez`]: BlueZ GATT characteristic proxy client for Bowers & Wilkins devices.

pub mod bluetooth_addr;
pub mod bluez;
pub mod l2cap;
pub mod rfcomm;
