//! Asynchronous L2CAP socket connection and session management for Apple devices.
//!
//! Apple AirPods and Beats devices expose the Apple Accessory Protocol (AAP) over an
//! L2CAP seqpacket socket on PSM `0x1001`. This module provides [`L2capSocket`] for raw
//! frame exchange and [`AppleL2capSession`] for high-level session initialization, background
//! frame dispatch, and device setting controls.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

use crate::protocols::aap::{self, Frame, ListeningMode, MicSelection, SettingId};
use crate::transport::bluetooth_addr::{self, BdAddr, SockAddrL2};

/// Errors encountered during L2CAP socket operations and connection setup.
#[derive(Debug, thiserror::Error)]
pub enum L2capError {
    /// The specified Bluetooth MAC address was malformed.
    #[error("Failed to parse Bluetooth MAC address: {0}")]
    InvalidAddress(String),
    /// Creating the raw L2CAP socket via libc failed.
    #[error("Socket creation failed: {0}")]
    SocketFailed(std::io::Error),
    /// Socket connection failed or was refused by peer.
    #[error("Connect failed: {0}")]
    ConnectFailed(std::io::Error),
    /// Connection attempt timed out.
    #[error("Connection timed out")]
    Timeout,
    /// An underlying standard I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// An asynchronous non-blocking L2CAP seqpacket socket.
///
/// Uses Linux `AF_BLUETOOTH` with `SOCK_SEQPACKET` and `BTPROTO_L2CAP`, integrated
/// with the Tokio asynchronous runtime via [`AsyncFd`].
pub struct L2capSocket {
    async_fd: AsyncFd<OwnedFd>,
}

impl L2capSocket {
    /// Connects to a remote Bluetooth device on the given PSM with retry delays.
    ///
    /// # Arguments
    ///
    /// * `address` - Colon-separated Bluetooth MAC address (e.g. `"AA:BB:CC:DD:EE:FF"`).
    /// * `psm` - Protocol/Service Multiplexer (e.g. `0x1001` for Apple AAP).
    /// * `timeout` - Total deadline for connection attempts.
    pub async fn connect(address: &str, psm: u16, timeout: Duration) -> Result<Self, L2capError> {
        let bd_addr = BdAddr::parse(address)
            .ok_or_else(|| L2capError::InvalidAddress(address.to_string()))?;

        tokio::time::timeout(timeout, async move {
            const RETRY_DELAYS_MS: &[u64] = &[0, 300, 700, 1200, 1800];
            let mut last_error = None;

            for &delay in RETRY_DELAYS_MS.iter() {
                if delay > 0 {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }

                let fd = unsafe {
                    libc::socket(
                        bluetooth_addr::AF_BLUETOOTH as i32,
                        libc::SOCK_SEQPACKET,
                        bluetooth_addr::BTPROTO_L2CAP,
                    )
                };
                if fd < 0 {
                    last_error = Some(L2capError::SocketFailed(std::io::Error::last_os_error()));
                    continue;
                }

                // Set non-blocking
                let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
                if flags >= 0 {
                    unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
                }

                // Connect to remote target without explicit bind
                let mut remote_addr: SockAddrL2 = unsafe { std::mem::zeroed() };
                remote_addr.l2_family = bluetooth_addr::AF_BLUETOOTH;
                remote_addr.l2_psm = psm.to_le();
                remote_addr.l2_bdaddr = bd_addr;
                remote_addr.l2_bdaddr_type = 0; // BDADDR_BREDR

                let ret = unsafe {
                    libc::connect(
                        fd,
                        &remote_addr as *const _ as *const libc::sockaddr,
                        std::mem::size_of::<SockAddrL2>() as libc::socklen_t,
                    )
                };

                let owned = unsafe { OwnedFd::from_raw_fd(fd) };
                let async_fd = match unsafe {
                    AsyncFd::register_with_interest(
                        owned,
                        tokio::io::Interest::READABLE | tokio::io::Interest::WRITABLE,
                    )
                } {
                    Ok(af) => af,
                    Err(e) => {
                        last_error = Some(L2capError::Io(std::io::Error::other(e.to_string())));
                        continue;
                    }
                };

                if ret < 0 {
                    let err = std::io::Error::last_os_error();
                    if err.raw_os_error() == Some(libc::EINPROGRESS) {
                        // Wait for writable readiness with bounded timeout
                        let wait_res =
                            tokio::time::timeout(Duration::from_millis(1500), async_fd.writable())
                                .await;
                        match wait_res {
                            Ok(Ok(mut guard)) => {
                                let mut err_code: libc::c_int = 0;
                                let mut err_len =
                                    std::mem::size_of::<libc::c_int>() as libc::socklen_t;
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
                                    last_error = Some(L2capError::ConnectFailed(e));
                                    continue;
                                }
                                guard.retain_ready();
                                return Ok(Self { async_fd });
                            }
                            _ => {
                                last_error = Some(L2capError::Timeout);
                                continue;
                            }
                        }
                    } else {
                        last_error = Some(L2capError::ConnectFailed(err));
                        continue;
                    }
                } else {
                    return Ok(Self { async_fd });
                }
            }

            Err(last_error.unwrap_or(L2capError::Timeout))
        })
        .await
        .map_err(|_| L2capError::Timeout)?
    }

    /// Sends raw bytes over the non-blocking L2CAP socket.
    pub async fn send(&self, data: &[u8]) -> Result<(), L2capError> {
        loop {
            let mut guard = self.async_fd.writable().await?;
            let res = unsafe {
                libc::send(
                    self.async_fd.as_raw_fd(),
                    data.as_ptr() as *const libc::c_void,
                    data.len(),
                    libc::MSG_NOSIGNAL,
                )
            };
            if res < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::WouldBlock {
                    guard.clear_ready();
                    continue;
                }
                return Err(L2capError::Io(err));
            }
            guard.retain_ready();
            return Ok(());
        }
    }

    /// Receives raw bytes into `buf` from the non-blocking L2CAP socket.
    ///
    /// Returns the number of bytes read.
    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize, L2capError> {
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
                return Err(L2capError::Io(err));
            }
            guard.clear_ready();
            return Ok(res as usize);
        }
    }
}

/// RAII wrapper that aborts a spawned Tokio background task when dropped.
#[derive(Debug)]
pub struct AbortOnDrop(pub tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// An active control session with an Apple AirPods or Beats accessory over L2CAP.
///
/// Handles session initialization (handshake, feature bitmask, notification subscription),
/// spawns a background frame ingestion loop sending incoming frames via an `mpsc` channel,
/// and provides high-level commands for changing noise modes and device settings.
#[derive(Clone)]
pub struct AppleL2capSession {
    socket: Arc<L2capSocket>,
    _reader_task: Arc<AbortOnDrop>,
}

impl AppleL2capSession {
    /// Initiates an L2CAP session with an Apple device at the specified MAC address.
    ///
    /// Performs protocol handshake, enables Pro 2 feature flags, subscribes to notifications,
    /// and ensures single-bud ANC mode is enabled by default.
    pub async fn start(
        address: &str,
        tx_incoming: mpsc::Sender<Frame>,
    ) -> Result<Self, L2capError> {
        let socket = Arc::new(
            L2capSocket::connect(address, aap::AAP_PSM, Duration::from_millis(6000)).await?,
        );

        // Spawn background reader
        let reader_sock = socket.clone();
        let reader_task = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            loop {
                match reader_sock.recv(&mut buf).await {
                    Ok(0) => {
                        // Socket closed by peer
                        break;
                    }
                    Ok(n) => {
                        if let Some(frame) = Frame::parse(&buf[..n])
                            && tx_incoming.send(frame).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        });

        // 1. Handshake
        socket.send(&aap::HANDSHAKE_RAW_BYTES).await?;
        tokio::time::sleep(Duration::from_millis(200)).await;

        // 2. Set Pro 2 Feature Flags (0xD7 mask)
        socket.send(&aap::SET_FEATURES_PRO2_BYTES).await?;
        tokio::time::sleep(Duration::from_millis(200)).await;

        // 3. Subscribe to notifications
        socket
            .send(&aap::subscribe_notifications_frame().encode())
            .await?;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 4. Auto-enable Single-Bud ANC so Noise Cancellation works reliably
        // regardless of whether user wears one bud, both buds, or adjusts a bud.
        let _ = socket
            .send(&aap::write_setting(SettingId::OneBudAnc as u8, 1).encode())
            .await;

        Ok(Self {
            socket,
            _reader_task: Arc::new(AbortOnDrop(reader_task)),
        })
    }

    /// Encodes and transmits a raw Apple Accessory Protocol frame.
    pub async fn send_frame(&self, frame: &Frame) -> Result<(), L2capError> {
        self.socket.send(&frame.encode()).await
    }

    /// Sets the active listening / noise control mode (Off, Noise Cancellation, Transparency, Adaptive).
    pub async fn set_noise_mode(&self, mode: ListeningMode) -> Result<(), L2capError> {
        if mode == ListeningMode::NoiseCancellation {
            // Ensure OneBudAnc is enabled so hardware will not reject ANC if single bud is worn
            let _ = self.set_one_bud_anc(true).await;
        }
        self.send_frame(&aap::set_noise_control(mode)).await
    }

    /// Enables or disables Conversation Awareness (AirPods Pro 2).
    pub async fn set_conversation_awareness(&self, enabled: bool) -> Result<(), L2capError> {
        self.send_frame(&aap::set_conversation_awareness(enabled))
            .await
    }

    /// Enables or disables automatic in-ear detection.
    pub async fn set_ear_detection(&self, enabled: bool) -> Result<(), L2capError> {
        self.send_frame(&aap::set_ear_detection(enabled)).await
    }

    /// Configures the active microphone selection (automatic, left bud, right bud).
    pub async fn set_mic_mode(&self, mode: MicSelection) -> Result<(), L2capError> {
        self.send_frame(&aap::set_mic_mode(mode)).await
    }

    /// Configures single-bud ANC permission (allows ANC when only one earbud is worn).
    pub async fn set_one_bud_anc(&self, enabled: bool) -> Result<(), L2capError> {
        self.send_frame(&aap::write_setting(
            SettingId::OneBudAnc as u8,
            if enabled { 1 } else { 2 },
        ))
        .await
    }

    /// Adjusts the volume of charging case and bud alert tones (0 to 100).
    pub async fn set_chime_volume(&self, vol: u8) -> Result<(), L2capError> {
        self.send_frame(&aap::write_setting(
            SettingId::ChimeVolume as u8,
            vol.min(100),
        ))
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_live_airpods_connection() {
        let (tx, mut rx) = mpsc::channel(32);
        let mac = "6C:12:70:3F:8E:AE";
        println!("Attempting L2CAP connection to {}", mac);
        if let Ok(_session) = AppleL2capSession::start(mac, tx).await {
            println!("Connected successfully to AirPods via L2CAP!");
            let timer = tokio::time::sleep(Duration::from_millis(1500));
            tokio::pin!(timer);
            loop {
                tokio::select! {
                    _ = &mut timer => {
                        println!("Finished listen period.");
                        break;
                    }
                    Some(frame) = rx.recv() => {
                        println!("Received AAP Frame: opcode=0x{:02X}, payload_len={}", frame.opcode, frame.payload.len());
                        if frame.opcode == aap::Opcode::Battery as u8 {
                            let comps = aap::parse_battery(&frame.payload);
                            println!("Parsed Battery Components: {:?}", comps);
                        }
                    }
                }
            }
        } else {
            println!("AirPods not currently reachable via L2CAP (may be in case or disconnected).");
        }
    }
}
