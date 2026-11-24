#![forbid(unsafe_code)]
use super::error::{Error, Result};
use socket2::{SockRef, Socket};
use std::net::TcpStream;
pub const MAX_SOCKET_BUFFER_BYTES: i64 = 64 << 20;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SocketBuffers {
    pub send: i64,
    pub receive: i64,
}
fn os_text(err: &std::io::Error) -> String {
    let text = err.to_string();
    match text.find(" (os error") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}
impl SocketBuffers {
    pub fn is_zero(&self) -> bool {
        self.send == 0 && self.receive == 0
    }
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [("send", self.send), ("receive", self.receive)] {
            if !(0..=MAX_SOCKET_BUFFER_BYTES).contains(&value) {
                return Err(Error::other(format!(
                    "dicom ul: {name} socket buffer {value} outside 0..{MAX_SOCKET_BUFFER_BYTES} bytes"
                )));
            }
        }
        Ok(())
    }
    pub(crate) fn control(&self, socket: &Socket) -> std::io::Result<()> {
        if self.send > 0 {
            socket.set_send_buffer_size(self.send as usize)?;
        }
        if self.receive > 0 {
            socket.set_recv_buffer_size(self.receive as usize)?;
        }
        Ok(())
    }
    pub fn apply(&self, stream: &TcpStream) -> Result<()> {
        if self.is_zero() {
            return Ok(());
        }
        let socket = SockRef::from(stream);
        if self.send > 0 {
            socket
                .set_send_buffer_size(self.send as usize)
                .map_err(|e| Error::other(format!("dicom ul: set send buffer: {}", os_text(&e))))?;
        }
        if self.receive > 0 {
            socket
                .set_recv_buffer_size(self.receive as usize)
                .map_err(|e| {
                    Error::other(format!("dicom ul: set receive buffer: {}", os_text(&e)))
                })?;
        }
        Ok(())
    }
}
pub fn effective_socket_buffers(stream: &TcpStream) -> Result<SocketBuffers> {
    let socket = SockRef::from(stream);
    let read = |size: std::io::Result<usize>| {
        size.map(|n| n as i64)
            .map_err(|e| Error::other(os_text(&e)))
    };
    Ok(SocketBuffers {
        send: read(socket.send_buffer_size())?,
        receive: read(socket.recv_buffer_size())?,
    })
}
#[cfg(feature = "tls")]
pub(crate) fn unsupported() -> Error {
    Error::sentinel(super::error::Kind::SocketBuffersUnsupported)
}
