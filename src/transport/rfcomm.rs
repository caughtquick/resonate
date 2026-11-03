//! Asynchronous RFCOMM socket connection and session management for Sony headphones.
//!
//! Sony WH/WF/MDR Bluetooth headphones expose their proprietary MDR protocol over an
//! RFCOMM serial connection (typically channel 1). This module provides [`RfcommSocket`]
//! for packet-framed transport and [`SonyMdrSession`] for orchestrating commands, auto-ACKs,
//! and feature settings.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

use crate::protocols::sony::{
    SonyMDRFrameParser, SonyMDRFraming, SonyMDRPacket, SonyV1AutomaticPowerOff,
    SonyV1EqualizerPreset, SonyV1Payloads,
};
use crate::transport::bluetooth_addr::{self, BdAddr, SockAddrRc};

/// Errors that can occur during RFCOMM socket communication.
#[derive(Debug, thiserror::Error)]
pub enum RfcommError {
    /// The specified Bluetooth MAC address was malformed.
    #[error("Failed to parse Bluetooth MAC address: {0}")]
    InvalidAddress(String),
    /// Creating the raw RFCOMM socket via libc failed.
    #[error("Socket creation failed: {0}")]
    SocketFailed(std::io::Error),
    /// Connection to the RFCOMM channel failed.
    #[error("Connect failed: {0}")]
    ConnectFailed(std::io::Error),
    /// The connection attempt timed out.
    #[error("Connection timed out")]
    Timeout,
    /// An underlying standard I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// An asynchronous non-blocking RFCOMM stream socket.
pub struct RfcommSocket {
    async_fd: AsyncFd<OwnedFd>,
}

impl RfcommSocket {
    /// Connects to an RFCOMM channel on a remote Bluetooth device.
    ///
    /// # Arguments
    ///
    /// * `address` - Colon-separated MAC address (e.g. `"AA:BB:CC:DD:EE:FF"`).
    /// * `channel` - RFCOMM channel number (typically 1 for Sony MDR devices).
    /// * `timeout` - Connection timeout duration.
    pub async fn connect(
        address: &str,
        channel: u8,
        timeout: Duration,
    ) -> Result<Self, RfcommError> {
        let bd_addr = BdAddr::parse(address)
            .ok_or_else(|| RfcommError::InvalidAddress(address.to_string()))?;

        tokio::time::timeout(timeout, async move {
            let fd = unsafe {
                libc::socket(
                    bluetooth_addr::AF_BLUETOOTH as i32,
                    libc::SOCK_STREAM,
                    bluetooth_addr::BTPROTO_RFCOMM,
                )
            };
            if fd < 0 {
                return Err(RfcommError::SocketFailed(std::io::Error::last_os_error()));
            }

            // Set non-blocking
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
            if flags >= 0 {
                unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
            }

            // Connect to remote target without explicit bind
            let mut remote_addr: SockAddrRc = unsafe { std::mem::zeroed() };
            remote_addr.rc_family = bluetooth_addr::AF_BLUETOOTH;
            remote_addr.rc_channel = channel;
            remote_addr.rc_bdaddr = bd_addr;

            let ret = unsafe {
                libc::connect(
                    fd,
                    &remote_addr as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<SockAddrRc>() as libc::socklen_t,
                )
            };

            let owned = unsafe { OwnedFd::from_raw_fd(fd) };
            let async_fd = unsafe {
                AsyncFd::register_with_interest(
                    owned,
                    tokio::io::Interest::READABLE | tokio::io::Interest::WRITABLE,
                )
            }
            .map_err(|e| RfcommError::Io(std::io::Error::other(e.to_string())))?;

            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINPROGRESS) {
                    let mut guard = async_fd.writable().await?;
                    let mut err_code: libc::c_int = 0;
                    let mut err_len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
                    let getsockopt_res = unsafe {
                        libc::getsockopt(
                            async_fd.as_raw_fd(),
                            libc::SOL_SOCKET,
                            libc::SO_ERROR,
                            &mut err_code as *mut _ as *mut libc::c_void,
                            &mut err_len,
                        )
                    };
                    if getsockopt_res < 0 || err_code != 0 {
                        let e = if err_code != 0 {
                            std::io::Error::from_raw_os_error(err_code)
                        } else {
                            std::io::Error::last_os_error()
                        };
                        return Err(RfcommError::ConnectFailed(e));
                    }
                    guard.retain_ready();
                } else {
                    return Err(RfcommError::ConnectFailed(err));
                }
            }

            Ok(Self { async_fd })
        })
        .await
        .map_err(|_| RfcommError::Timeout)?
    }

    /// Encodes and sends a framed [`SonyMDRPacket`] over the RFCOMM socket.
    pub async fn send_packet(&self, packet: &SonyMDRPacket) -> Result<(), RfcommError> {
        let encoded = SonyMDRFraming::encode(packet);
        let mut guard = self.async_fd.writable().await?;
        let res = unsafe {
            libc::send(
                self.async_fd.as_raw_fd(),
                encoded.as_ptr() as *const libc::c_void,
                encoded.len(),
                libc::MSG_NOSIGNAL,
            )
        };
        if res < 0 {
            return Err(RfcommError::Io(std::io::Error::last_os_error()));
        }
        guard.retain_ready();
        Ok(())
    }

    /// Receives raw bytes into `buf` from the non-blocking RFCOMM socket.
    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize, RfcommError> {
        loop {
            let mut guard = self.async_fd.readable().await?;
            let res = unsafe {
                libc::recv(
                    self.async_fd.as_raw_fd(),
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    0,
                )
            };
            if res < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::WouldBlock {
                    guard.clear_ready();
                    continue;
                }
                return Err(RfcommError::Io(err));
            }
            guard.clear_ready();
            return Ok(res as usize);
        }
    }
}

/// An active Sony MDR control session over RFCOMM.
///
/// Dispatches packets, automatically acknowledges incoming non-ACK packets, tracks
/// alternating sequence bits, and sends Sony V1/V2 protocol control messages.
#[derive(Clone)]
pub struct SonyMdrSession {
    socket: Arc<RfcommSocket>,
    sequence: Arc<AtomicU8>,
    _reader_task: Arc<tokio::task::JoinHandle<()>>,
}

impl SonyMdrSession {
    /// Connects to a Sony headphone via RFCOMM, sends initialization packets,
    /// and starts a background packet receiver task that sends decoded packets to `tx_incoming`.
    pub async fn start(
        address: &str,
        tx_incoming: mpsc::Sender<SonyMDRPacket>,
    ) -> Result<Self, RfcommError> {
        let socket =
            Arc::new(RfcommSocket::connect(address, 1, Duration::from_millis(3500)).await?);
        let sequence = Arc::new(AtomicU8::new(0));

        // Send initial V1 Init packet
        let init_pkt = SonyMDRPacket::new(SonyMDRPacket::COMMAND_TYPE, 0, vec![0x00, 0x00]);
        socket.send_packet(&init_pkt).await?;

        let reader_sock = socket.clone();
        let reader_task = tokio::spawn(async move {
            let mut parser = SonyMDRFrameParser::new();
            let mut buf = [0u8; 1024];
            loop {
                match reader_sock.recv(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let packets = parser.append(&buf[..n]);
                        for pkt in packets {
                            // Immediately send ACK for non-ACK packets
                            if pkt.data_type != SonyMDRPacket::ACK_TYPE {
                                let ack = SonyMDRPacket::new(
                                    SonyMDRPacket::ACK_TYPE,
                                    pkt.sequence ^ 1,
                                    vec![],
                                );
                                let _ = reader_sock.send_packet(&ack).await;
                            }

                            if tx_incoming.send(pkt).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            socket,
            sequence,
            _reader_task: Arc::new(reader_task),
        })
    }

    /// Sends a command packet with the next alternating sequence number.
    pub async fn send_command(&self, payload: Vec<u8>) -> Result<(), RfcommError> {
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) % 2;
        let pkt = SonyMDRPacket::new(SonyMDRPacket::COMMAND_TYPE, seq, payload);
        self.socket.send_packet(&pkt).await
    }

    /// Sets the noise control mode and ambient sound level (0 to 20).
    pub async fn set_noise_mode(
        &self,
        mode_str: &str,
        ambient_level: i32,
    ) -> Result<(), RfcommError> {
        let (nc_effect, asm_setting) = match mode_str.to_lowercase().as_str() {
            "anc" | "nc" | "on" => (0x01, 0x00),
            "ambient" | "transparency" => (0x02, 0x01),
            "wind" => (0x01, 0x02),
            _ => (0x00, 0x00), // Off
        };
        let clamped_level = ambient_level.clamp(0, 20) as u8;
        let payload = vec![
            0x22,
            nc_effect,
            0x02,
            0x01,
            asm_setting,
            0x01,
            clamped_level,
        ];
        self.send_command(payload).await
    }

    /// Sets an equalizer preset on the headphone.
    pub async fn set_equalizer_preset(
        &self,
        preset: SonyV1EqualizerPreset,
    ) -> Result<(), RfcommError> {
        let payload = SonyV1Payloads::equalizer_preset(preset);
        self.send_command(payload).await
    }

    /// Sets the 5 equalizer bands and Clear Bass level directly.
    pub async fn set_equalizer_bands(&self, values: &[i32]) -> Result<(), RfcommError> {
        let payload = SonyV1Payloads::equalizer_bands(values);
        self.send_command(payload).await
    }

    /// Enables or disables the side touch sensor control panel.
    pub async fn set_touch_sensor(&self, enabled: bool) -> Result<(), RfcommError> {
        let payload = SonyV1Payloads::touch_sensor(enabled, 0);
        self.send_command(payload).await
    }

    /// Enables or disables DSEE Extreme audio upscaling.
    pub async fn set_dsee(&self, enabled: bool) -> Result<(), RfcommError> {
        let payload = SonyV1Payloads::dsee(enabled);
        self.send_command(payload).await
    }

    /// Starts or cancels NC optimizer / atmospheric pressure calibration.
    pub async fn set_optimizer(&self, start: bool) -> Result<(), RfcommError> {
        let payload = SonyV1Payloads::optimizer(start);
        self.send_command(payload).await
    }

    /// Configures the auto-power off timeout mode.
    pub async fn set_automatic_power_off(
        &self,
        mode: SonyV1AutomaticPowerOff,
    ) -> Result<(), RfcommError> {
        let payload = SonyV1Payloads::automatic_power_off(mode);
        self.send_command(payload).await
    }
}
