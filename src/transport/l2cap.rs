use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

use crate::protocols::aap::{self, Frame, ListeningMode, MicSelection, SettingId};
use crate::transport::bluetooth_addr::{self, BdAddr, SockAddrL2};

#[derive(Debug, thiserror::Error)]
pub enum L2capError {
    #[error("Failed to parse Bluetooth MAC address: {0}")]
    InvalidAddress(String),
    #[error("Socket creation failed: {0}")]
    SocketFailed(std::io::Error),
    #[error("Connect failed: {0}")]
    ConnectFailed(std::io::Error),
    #[error("Connection timed out")]
    Timeout,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct L2capSocket {
    async_fd: AsyncFd<OwnedFd>,
}

impl L2capSocket {
    pub async fn connect(address: &str, psm: u16, timeout: Duration) -> Result<Self, L2capError> {
        let bd_addr = BdAddr::parse(address).ok_or_else(|| L2capError::InvalidAddress(address.to_string()))?;

        tokio::time::timeout(timeout, async move {
            let fd = unsafe {
                libc::socket(
                    bluetooth_addr::AF_BLUETOOTH as i32,
                    libc::SOCK_SEQPACKET,
                    bluetooth_addr::BTPROTO_L2CAP,
                )
            };
            if fd < 0 {
                return Err(L2capError::SocketFailed(std::io::Error::last_os_error()));
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
            let async_fd = unsafe {
                AsyncFd::register_with_interest(
                    owned,
                    tokio::io::Interest::READABLE | tokio::io::Interest::WRITABLE,
                )
            }.map_err(|e| L2capError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINPROGRESS) {
                    // Wait for writable readiness
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
                        return Err(L2capError::ConnectFailed(e));
                    }
                    guard.retain_ready();
                } else {
                    return Err(L2capError::ConnectFailed(err));
                }
            }

            Ok(Self { async_fd })
        })
        .await
        .map_err(|_| L2capError::Timeout)?
    }

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
            guard.retain_ready();
            return Ok(res as usize);
        }
    }
}

pub struct AppleL2capSession {
    socket: Arc<L2capSocket>,
    _reader_task: tokio::task::JoinHandle<()>,
}

impl AppleL2capSession {
    pub async fn start(
        address: &str,
        tx_incoming: mpsc::Sender<Frame>,
    ) -> Result<Self, L2capError> {
        let socket = Arc::new(L2capSocket::connect(address, aap::AAP_PSM, Duration::from_millis(3500)).await?);

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
                        if let Some(frame) = Frame::parse(&buf[..n]) {
                            if tx_incoming.send(frame).await.is_err() {
                                break;
                            }
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
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 2. Set Pro 2 Feature Flags (0xD7 mask)
        socket.send(&aap::SET_FEATURES_PRO2_BYTES).await?;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 3. Subscribe to notifications
        socket.send(&aap::subscribe_notifications_frame().encode()).await?;

        Ok(Self {
            socket,
            _reader_task: reader_task,
        })
    }

    pub async fn send_frame(&self, frame: &Frame) -> Result<(), L2capError> {
        self.socket.send(&frame.encode()).await
    }

    pub async fn set_noise_mode(&self, mode: ListeningMode) -> Result<(), L2capError> {
        self.send_frame(&aap::set_noise_control(mode)).await
    }

    pub async fn set_conversation_awareness(&self, enabled: bool) -> Result<(), L2capError> {
        self.send_frame(&aap::set_conversation_awareness(enabled)).await
    }

    pub async fn set_ear_detection(&self, enabled: bool) -> Result<(), L2capError> {
        self.send_frame(&aap::set_ear_detection(enabled)).await
    }

    pub async fn set_mic_mode(&self, mode: MicSelection) -> Result<(), L2capError> {
        self.send_frame(&aap::set_mic_mode(mode)).await
    }

    pub async fn set_one_bud_anc(&self, enabled: bool) -> Result<(), L2capError> {
        self.send_frame(&aap::write_setting(SettingId::OneBudAnc as u8, if enabled { 1 } else { 2 })).await
    }

    pub async fn set_chime_volume(&self, vol: u8) -> Result<(), L2capError> {
        self.send_frame(&aap::write_setting(SettingId::ChimeVolume as u8, vol.min(100))).await
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
        if let Ok(session) = AppleL2capSession::start(mac, tx).await {
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

