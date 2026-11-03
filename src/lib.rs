//! # Resonate
//!
//! `resonate` is a native Linux Bluetooth headphone daemon and CLI providing reverse-engineered,
//! vendor-specific controls for popular premium wireless headphones and earbuds.
//!
//! ## Supported Hardware
//!
//! - **Apple / Beats**: AirPods (1st, 2nd, 3rd, 4th gen, 4th gen ANC), AirPods Pro (1st, 2nd gen, USB-C, 3rd gen),
//!   AirPods Max, Beats Fit Pro, Beats Studio Pro, Beats Studio Buds+, Beats Solo 4, Beats Solo Buds.
//! - **Sony**: WH-1000XM3, WH-1000XM4, WH-1000XM5, WF-1000XM3, WF-1000XM4, WF-1000XM5, LinkBuds, and MDR series.
//! - **Bowers & Wilkins**: Px7 S2, Px7 S2e, Px8, PI7, PI5 series.
//!
//! ## Architecture Overview
//!
//! Resonate is structured into several modular layers:
//!
//! 1. **Transport Layer** ([`transport`]): Low-level communications including raw Linux Bluetooth
//!    sockets (`L2CAP` via [`transport::l2cap`], `RFCOMM` via [`transport::rfcomm`]), and BlueZ GATT
//!    over D-Bus ([`transport::bluez`]).
//! 2. **Protocols Layer** ([`protocols`]): Binary protocol parsing and frame encoding:
//!    - [`protocols::aap`]: Apple Accessory Protocol over L2CAP (PSM `0x1001`).
//!    - [`protocols::apple_models`]: Model identification, modalias parsing, and device capability maps.
//!    - [`protocols::sony`]: Sony MDR framing, escape sequencing, packet checksums, and payload commands.
//!    - [`protocols::bw`]: Bowers & Wilkins RPC protocol using MessagePack over BLE GATT.
//! 3. **Device Management** ([`manager`]): Runtime state tracking ([`manager::DeviceManager`]), device
//!    discovery, automatic session connection, reconnection loops, and state synchronization.
//! 4. **Audio & Media** ([`audio`]): MPRIS D-Bus player discovery, smart auto-pause and resume when
//!    earbuds are removed or inserted, and PulseAudio/PipeWire sink monitoring.
//! 5. **D-Bus Daemon Interface** ([`dbus_service`]): Exposes `org.headbridge.Manager` on `/org/headbridge`
//!    over session D-Bus for CLI control, desktop widgets (Waybar/Polybar), and GUI apps.
//! 6. **Inventory & Settings** ([`inventory`], [`settings`], [`notifications`]): BlueZ device querying,
//!    persistent configuration (`~/.config/resonate/settings.json`), and desktop notifications.
//!
//! ## License & Attribution
//!
//! Resonate is licensed under the MIT License.
//! Parts of protocol reverse engineering and interface compatibility are derived from or inspired by
//! [HeadBridge](https://github.com/herenickname/HeadBridge) (Copyright (c) 2026 HeadBridge contributors).

pub mod audio;
pub mod cli;
pub mod dbus_service;
pub mod inventory;
pub mod manager;
pub mod notifications;
pub mod protocols;
pub mod settings;
pub mod transport;
