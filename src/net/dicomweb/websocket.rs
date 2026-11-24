use super::mimeheader::Header;
use std::io::{self, BufReader, Cursor, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
pub(crate) const OP_CONTINUATION: u8 = 0x0;
pub(crate) const OP_TEXT: u8 = 0x1;
pub(crate) const OP_BINARY: u8 = 0x2;
pub(crate) const OP_CLOSE: u8 = 0x8;
pub(crate) const OP_PING: u8 = 0x9;
pub(crate) const OP_PONG: u8 = 0xA;
pub(crate) const CLOSE_NORMAL: u16 = 1000;
pub(crate) const CLOSE_GOING_AWAY: u16 = 1001;
pub(crate) const CLOSE_PROTOCOL_ERROR: u16 = 1002;
pub(crate) const CLOSE_NO_STATUS: u16 = 1005;
pub(crate) const CLOSE_INVALID_PAYLOAD: u16 = 1007;
pub(crate) const CLOSE_MESSAGE_TOO_BIG: u16 = 1009;
const MAX_CONTROL_PAYLOAD: usize = 125;
const DEFAULT_MAX_MESSAGE_SIZE: i64 = 1 << 20;
const ACCEPT_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const CLOSE_WRITE_TIMEOUT: Duration = Duration::from_secs(1);
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WsError {
    Handshake,
    Protocol,
    MessageTooLarge,
    Closed,
    CloseFrame { code: u16, reason: String },
    Io { text: String, timeout: bool },
}
impl WsError {
    pub fn is_closed(&self) -> bool {
        matches!(self, WsError::Closed | WsError::CloseFrame { .. })
    }
    fn io(e: &io::Error) -> WsError {
        WsError::Io {
            text: e.to_string(),
            timeout: matches!(
                e.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ),
        }
    }
}
impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsError::Handshake => f.write_str("websocket: invalid handshake"),
            WsError::Protocol => f.write_str("websocket: protocol error"),
            WsError::MessageTooLarge => f.write_str("websocket: message too large"),
            WsError::Closed => f.write_str("websocket: connection closed"),
            WsError::CloseFrame { code, .. } => write!(f, "websocket: closed by peer ({code})"),
            WsError::Io { text, .. } => f.write_str(text),
        }
    }
}
impl std::error::Error for WsError {}
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut message = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}
#[doc(hidden)]
pub fn accept_key(key: &str) -> String {
    crate::dicomjson::sizedparse::base64_encode(&sha1(format!("{key}{ACCEPT_GUID}").as_bytes()))
}
fn random_bytes<const N: usize>() -> [u8; N] {
    use std::hash::{BuildHasher, Hasher};
    let mut out = [0u8; N];
    for chunk in out.chunks_mut(8) {
        let bits = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish()
            .to_le_bytes();
        chunk.copy_from_slice(&bits[..chunk.len()]);
    }
    out
}
pub(crate) fn new_client_key() -> String {
    crate::dicomjson::sizedparse::base64_encode(&random_bytes::<16>())
}
pub(crate) fn header_contains_token(header: &Header, name: &str, token: &str) -> bool {
    header.values(name).iter().any(|value| {
        value
            .split(',')
            .any(|field| field.trim().eq_ignore_ascii_case(token))
    })
}
struct WriteState {
    stream: Box<dyn Write + Send>,
    sent_close: bool,
}
pub(crate) type CloseHook = Box<dyn Fn() + Send + Sync>;
type ReadSide = BufReader<io::Chain<Cursor<Vec<u8>>, Box<dyn Read + Send>>>;
pub struct Conn {
    reader: Mutex<ReadSide>,
    writer: Mutex<WriteState>,
    socket: TcpStream,
    on_close: Option<CloseHook>,
    client: bool,
    max_message: i64,
    closed: AtomicBool,
    done: (Mutex<bool>, Condvar),
    read_deadline: Mutex<Option<Instant>>,
}
const READ_POLL: Duration = Duration::from_millis(25);
impl Conn {
    pub fn new(
        stream: TcpStream,
        buffered: Vec<u8>,
        client: bool,
        max_message_bytes: i64,
    ) -> io::Result<Arc<Conn>> {
        let read_half = stream.try_clone()?;
        let write_half = stream.try_clone()?;
        Conn::with_transport(
            stream,
            Box::new(read_half),
            Box::new(write_half),
            None,
            buffered,
            client,
            max_message_bytes,
        )
    }
    pub fn with_transport(
        socket: TcpStream,
        read_half: Box<dyn Read + Send>,
        write_half: Box<dyn Write + Send>,
        on_close: Option<CloseHook>,
        buffered: Vec<u8>,
        client: bool,
        max_message_bytes: i64,
    ) -> io::Result<Arc<Conn>> {
        socket.set_read_timeout(Some(READ_POLL))?;
        Ok(Arc::new(Conn {
            reader: Mutex::new(BufReader::new(Cursor::new(buffered).chain(read_half))),
            writer: Mutex::new(WriteState {
                stream: write_half,
                sent_close: false,
            }),
            socket,
            on_close,
            client,
            max_message: if max_message_bytes <= 0 {
                DEFAULT_MAX_MESSAGE_SIZE
            } else {
                max_message_bytes
            },
            closed: AtomicBool::new(false),
            done: (Mutex::new(false), Condvar::new()),
            read_deadline: Mutex::new(None),
        }))
    }
    #[doc(hidden)]
    pub fn set_read_deadline(&self, deadline: Option<Instant>) {
        if let Ok(mut slot) = self.read_deadline.lock() {
            *slot = deadline;
        }
    }
    pub(crate) fn set_write_deadline(&self, deadline: Option<Instant>) {
        let _ = self.socket.set_write_timeout(timeout_until(deadline));
    }
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    pub fn wait_closed(&self, timeout: Duration) -> bool {
        let Ok(guard) = self.done.0.lock() else {
            return true;
        };
        match self
            .done
            .1
            .wait_timeout_while(guard, timeout, |closed| !*closed)
        {
            Ok((guard, _)) => *guard,
            Err(_) => true,
        }
    }
    fn read_exact(&self, buf: &mut [u8]) -> Result<(), WsError> {
        let Ok(mut reader) = self.reader.lock() else {
            return Err(WsError::Closed);
        };
        let mut got = 0;
        while got < buf.len() {
            match reader.read(&mut buf[got..]) {
                Ok(0) => {
                    let text = if got == 0 { "EOF" } else { "unexpected EOF" };
                    return Err(WsError::Io {
                        text: text.into(),
                        timeout: false,
                    });
                }
                Ok(n) => got += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    if self.closed.load(Ordering::SeqCst) {
                        return Err(WsError::Io {
                            text: "use of closed network connection".into(),
                            timeout: false,
                        });
                    }
                    let deadline = self.read_deadline.lock().ok().and_then(|d| *d);
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        return Err(WsError::Io {
                            text: "i/o timeout".into(),
                            timeout: true,
                        });
                    }
                }
                Err(e) => return Err(WsError::io(&e)),
            }
        }
        Ok(())
    }
    fn read_frame_header(&self) -> Result<FrameHeader, WsError> {
        let mut head = [0u8; 2];
        self.read_exact(&mut head)?;
        let mut h = FrameHeader {
            fin: head[0] & 0x80 != 0,
            rsv: head[0] & 0x70,
            opcode: head[0] & 0x0F,
            masked: head[1] & 0x80 != 0,
            mask: [0; 4],
            length: u64::from(head[1] & 0x7F),
        };
        match h.length {
            126 => {
                let mut ext = [0u8; 2];
                self.read_exact(&mut ext)?;
                h.length = u64::from(u16::from_be_bytes(ext));
                if h.length < 126 {
                    return Err(WsError::Protocol);
                }
            }
            127 => {
                let mut ext = [0u8; 8];
                self.read_exact(&mut ext)?;
                h.length = u64::from_be_bytes(ext);
                if h.length >> 63 != 0 || h.length <= 0xFFFF {
                    return Err(WsError::Protocol);
                }
            }
            _ => {}
        }
        if h.masked {
            self.read_exact(&mut h.mask)?;
        }
        Ok(h)
    }
    pub fn read_message(&self) -> Result<(u8, Vec<u8>), WsError> {
        let mut message: Vec<u8> = Vec::new();
        let mut message_opcode: Option<u8> = None;
        loop {
            let h = match self.read_frame_header() {
                Ok(h) => h,
                Err(e) => {
                    if e == WsError::Protocol {
                        self.fail(CLOSE_PROTOCOL_ERROR);
                    } else {
                        self.close_conn();
                    }
                    return Err(e);
                }
            };
            if h.rsv != 0 || h.masked == self.client {
                self.fail(CLOSE_PROTOCOL_ERROR);
                return Err(WsError::Protocol);
            }
            let control = h.opcode >= 0x8;
            if control {
                if !h.fin
                    || h.length > MAX_CONTROL_PAYLOAD as u64
                    || !matches!(h.opcode, OP_CLOSE | OP_PING | OP_PONG)
                {
                    self.fail(CLOSE_PROTOCOL_ERROR);
                    return Err(WsError::Protocol);
                }
            } else if !matches!(h.opcode, OP_CONTINUATION | OP_TEXT | OP_BINARY) {
                self.fail(CLOSE_PROTOCOL_ERROR);
                return Err(WsError::Protocol);
            }
            if !control && (message.len() as u64).saturating_add(h.length) > self.max_message as u64
            {
                self.fail(CLOSE_MESSAGE_TOO_BIG);
                return Err(WsError::MessageTooLarge);
            }
            let mut payload = vec![0u8; h.length as usize];
            if let Err(e) = self.read_exact(&mut payload) {
                self.close_conn();
                return Err(e);
            }
            if h.masked {
                for (i, b) in payload.iter_mut().enumerate() {
                    *b ^= h.mask[i % 4];
                }
            }
            match h.opcode {
                OP_PING => {
                    self.write_frame(OP_PONG, &payload)?;
                    continue;
                }
                OP_PONG => continue,
                OP_CLOSE => return Err(self.handle_close(&payload)),
                OP_CONTINUATION => {
                    if message_opcode.is_none() {
                        self.fail(CLOSE_PROTOCOL_ERROR);
                        return Err(WsError::Protocol);
                    }
                }
                _ => {
                    if message_opcode.is_some() {
                        self.fail(CLOSE_PROTOCOL_ERROR);
                        return Err(WsError::Protocol);
                    }
                    message_opcode = Some(h.opcode);
                }
            }
            message.extend_from_slice(&payload);
            if h.fin {
                let opcode = message_opcode.unwrap_or(OP_BINARY);
                if opcode == OP_TEXT && std::str::from_utf8(&message).is_err() {
                    self.fail(CLOSE_INVALID_PAYLOAD);
                    return Err(WsError::Protocol);
                }
                return Ok((opcode, message));
            }
        }
    }
    fn handle_close(&self, payload: &[u8]) -> WsError {
        let mut code = CLOSE_NO_STATUS;
        let mut reason = String::new();
        if payload.len() == 1 {
            self.fail(CLOSE_PROTOCOL_ERROR);
            return WsError::Protocol;
        }
        if payload.len() >= 2 {
            code = u16::from_be_bytes([payload[0], payload[1]]);
            match std::str::from_utf8(&payload[2..]) {
                Ok(text) if valid_close_code(code) => reason = text.to_string(),
                _ => {
                    self.fail(CLOSE_PROTOCOL_ERROR);
                    return WsError::Protocol;
                }
            }
        }
        let reply = if code != CLOSE_NO_STATUS {
            code
        } else {
            CLOSE_NORMAL
        };
        self.send_close_and_close(reply, "");
        WsError::CloseFrame { code, reason }
    }
    pub fn write_message(&self, opcode: u8, data: &[u8]) -> Result<(), WsError> {
        if opcode != OP_TEXT && opcode != OP_BINARY {
            return Err(WsError::Protocol);
        }
        if opcode == OP_TEXT && std::str::from_utf8(data).is_err() {
            return Err(WsError::Protocol);
        }
        self.write_frame(opcode, data)
    }
    pub(crate) fn write_ping(&self, data: &[u8]) -> Result<(), WsError> {
        if data.len() > MAX_CONTROL_PAYLOAD {
            return Err(WsError::Protocol);
        }
        self.write_frame(OP_PING, data)
    }
    fn write_frame(&self, opcode: u8, payload: &[u8]) -> Result<(), WsError> {
        let Ok(mut state) = self.writer.lock() else {
            return Err(WsError::Closed);
        };
        self.write_frame_locked(&mut state, opcode, payload)
    }
    fn write_frame_locked(
        &self,
        state: &mut WriteState,
        opcode: u8,
        payload: &[u8],
    ) -> Result<(), WsError> {
        if self.is_closed() || state.sent_close {
            return Err(WsError::Closed);
        }
        let mut frame = Vec::with_capacity(payload.len() + 14);
        frame.push(0x80 | opcode);
        let mask_bit = if self.client { 0x80 } else { 0 };
        let length = payload.len();
        if length <= 125 {
            frame.push(mask_bit | length as u8);
        } else if length <= 0xFFFF {
            frame.push(mask_bit | 126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        } else {
            frame.push(mask_bit | 127);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }
        if self.client {
            let mask = random_bytes::<4>();
            frame.extend_from_slice(&mask);
            frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        } else {
            frame.extend_from_slice(payload);
        }
        if let Err(e) = state.stream.write_all(&frame) {
            self.close_conn();
            return Err(WsError::io(&e));
        }
        if opcode == OP_CLOSE {
            state.sent_close = true;
        }
        Ok(())
    }
    pub fn close(&self, code: u16, reason: &str) {
        self.send_close_and_close(code, reason);
    }
    fn fail(&self, code: u16) {
        self.send_close_and_close(code, "");
    }
    fn send_close_and_close(&self, code: u16, reason: &str) {
        let mut reason = reason.as_bytes();
        if reason.len() > MAX_CONTROL_PAYLOAD - 2 {
            reason = &reason[..MAX_CONTROL_PAYLOAD - 2];
        }
        let mut payload = code.to_be_bytes().to_vec();
        payload.extend_from_slice(reason);
        if let Ok(mut state) = self.writer.lock()
            && !state.sent_close
        {
            let _ = self.socket.set_write_timeout(Some(CLOSE_WRITE_TIMEOUT));
            let _ = self.write_frame_locked(&mut state, OP_CLOSE, &payload);
        }
        self.close_conn();
    }
    fn close_conn(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(hook) = &self.on_close {
            hook();
        }
        let _ = self.socket.shutdown(Shutdown::Both);
        if let Ok(mut done) = self.done.0.lock() {
            *done = true;
            self.done.1.notify_all();
        }
    }
}
struct FrameHeader {
    fin: bool,
    rsv: u8,
    opcode: u8,
    masked: bool,
    mask: [u8; 4],
    length: u64,
}
fn valid_close_code(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1011 | 3000..=4999)
}
fn timeout_until(deadline: Option<Instant>) -> Option<Duration> {
    deadline.map(|d| {
        d.saturating_duration_since(Instant::now())
            .max(Duration::from_millis(1))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc6455_accept_key() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }
}
