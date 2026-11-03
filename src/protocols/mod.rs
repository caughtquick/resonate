//! Reverse-engineered vendor protocols and model catalogs.
//!
//! Implements protocol encoding and decoding for each supported headphone manufacturer:
//! - [`aap`]: Apple Accessory Protocol frames over L2CAP (AirPods and Beats).
//! - [`apple_models`]: Apple model catalog, modalias parsing, and device capabilities.
//! - [`sony`]: Sony MDR binary protocol framing, escape codes, checksums, and commands.
//! - [`bw`]: Bowers & Wilkins RPC protocol over BLE GATT using MessagePack.

pub mod aap;
pub mod apple_models;
pub mod bw;
pub mod sony;
