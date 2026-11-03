use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Duration;
use tokio::io::unix::AsyncFd;

use crate::protocols::sony::{SonyMDRFrameParser, SonyMDRFraming, SonyMDRPacket};
use crate::transport::bluetooth_addr::{self, BdAddr, SockAddrRc};

#[derive(Debug, thiserror::Error)]
pub enum RfcommError {
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

pub struct RfcommSocket {
    async_fd: AsyncFd<OwnedFd>,
    parser: SonyMDRFrameParser,
}

impl RfcommSocket {
    pub async fn connect(address: &str, channel: u8, timeout: Duration) -> Result<Self, RfcommError> {
        let bd_addr = BdAddr::parse(address).ok_or_else(|| RfcommError::InvalidAddress(address.to_string()))?;

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
            }.map_err(|e| RfcommError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))?;

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

            Ok(Self {
                async_fd,
                parser: SonyMDRFrameParser::new(),
            })
        })
        .await
        .map_err(|_| RfcommError::Timeout)?
    }

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

    pub async fn read_packets(&mut self) -> Result<Vec<SonyMDRPacket>, RfcommError> {
        let mut buf = [0u8; 1024];
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
                return Ok(Vec::new());
            }
            return Err(RfcommError::Io(err));
        }
        guard.retain_ready();
        let bytes_read = res as usize;
        if bytes_read == 0 {
            return Err(RfcommError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "RFCOMM peer closed connection",
            )));
        }

        Ok(self.parser.append(&buf[..bytes_read]))
    }
}
