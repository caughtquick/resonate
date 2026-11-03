use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;
use tokio::io::unix::AsyncFd;
use tokio::sync::mpsc;

use crate::protocols::aap::{self, Frame};
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

            // Bind to BDADDR_ANY
            let mut local_addr: SockAddrL2 = unsafe { std::mem::zeroed() };
            local_addr.l2_family = bluetooth_addr::AF_BLUETOOTH;
            local_addr.l2_bdaddr = BdAddr::ANY;

            let bind_res = unsafe {
                libc::bind(
                    fd,
                    &local_addr as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<SockAddrL2>() as libc::socklen_t,
                )
            };
            if bind_res < 0 {
                let err = std::io::Error::last_os_error();
                unsafe { libc::close(fd) };
                return Err(L2capError::SocketFailed(err));
            }

            // Connect to remote target
            let mut remote_addr: SockAddrL2 = unsafe { std::mem::zeroed() };
            remote_addr.l2_family = bluetooth_addr::AF_BLUETOOTH;
            remote_addr.l2_psm = psm.to_le();
            remote_addr.l2_bdaddr = bd_addr;

            let ret = unsafe {
                libc::connect(
                    fd,
                    &remote_addr as *const _ as *const libc::sockaddr,
                    std::mem::size_of::<SockAddrL2>() as libc::socklen_t,
                )
            };

            let owned = unsafe { OwnedFd::from_raw_fd(fd) };
            let async_fd = AsyncFd::new(owned)?;

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
            return Err(L2capError::Io(std::io::Error::last_os_error()));
        }
        guard.retain_ready();
        Ok(())
    }

    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize, L2capError> {
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
                return Ok(0);
            }
            return Err(L2capError::Io(err));
        }
        guard.retain_ready();
        Ok(res as usize)
    }
}

pub struct AppleL2capSession {
    socket: L2capSocket,
}

impl AppleL2capSession {
    pub async fn start(address: &str, _tx_incoming: mpsc::Sender<Frame>) -> Result<Self, L2capError> {
        let socket = L2capSocket::connect(address, aap::AAP_PSM, Duration::from_millis(3000)).await?;

        // 1. Handshake
        socket.send(&aap::HANDSHAKE_RAW_BYTES).await?;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 2. Set Pro 2 Feature Flags (0xD7 mask)
        socket.send(&aap::SET_FEATURES_PRO2_BYTES).await?;
        tokio::time::sleep(Duration::from_millis(150)).await;

        // 3. Subscribe to notifications
        socket.send(&aap::subscribe_notifications_frame().encode()).await?;

        Ok(Self { socket })
    }

    pub async fn send_frame(&self, frame: &Frame) -> Result<(), L2capError> {
        self.socket.send(&frame.encode()).await
    }
}
