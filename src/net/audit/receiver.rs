#![forbid(unsafe_code)]
use super::syslog::MAX_SYSLOG_UDP_MESSAGE;
use super::timetext;
use super::{CallbackError, Ctx, Error, ErrorKind, Nanos, Result, ctx_err};
use std::collections::HashSet;
use std::io::Read;
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyslogRecord {
    pub priority: i64,
    pub timestamp: String,
    pub hostname: String,
    pub app_name: String,
    pub process_id: String,
    pub message_id: String,
    pub structured_data: String,
    pub raw: Vec<u8>,
    pub message: Vec<u8>,
}
fn bad_record() -> Error {
    Error::sentinel(ErrorKind::SyslogRecord)
}
fn parse_record(raw: Vec<u8>) -> Result<SyslogRecord> {
    if raw.len() < 4 || raw[0] != b'<' {
        return Err(bad_record());
    }
    let end = match raw.iter().position(|c| *c == b'>') {
        Some(end) if (2..=4).contains(&end) => end,
        _ => return Err(bad_record()),
    };
    let priority = &raw[1..end];
    if priority.len() > 1 && priority[0] == b'0' {
        return Err(bad_record());
    }
    if !priority.iter().all(u8::is_ascii_digit) {
        return Err(bad_record());
    }
    let p = priority
        .iter()
        .fold(0i64, |v, c| v * 10 + i64::from(c - b'0'));
    if p > 191 {
        return Err(bad_record());
    }
    let mut rest = &raw[end + 1..];
    let mut fields: Vec<String> = Vec::with_capacity(6);
    for _ in 0..6 {
        let sep = match rest.iter().position(|c| *c == b' ') {
            Some(sep) if sep >= 1 => sep,
            _ => return Err(bad_record()),
        };
        let field = &rest[..sep];
        if !field.iter().all(|c| (33..=126).contains(c)) {
            return Err(bad_record());
        }
        fields.push(field.iter().map(|c| char::from(*c)).collect());
        rest = &rest[sep + 1..];
    }
    if fields[0] != "1"
        || fields[1].len() > 64
        || fields[2].len() > 255
        || fields[3].len() > 48
        || fields[4].len() > 128
        || fields[5].len() > 32
    {
        return Err(bad_record());
    }
    if fields[1] != "-" && !valid_syslog_timestamp(&fields[1]) {
        return Err(bad_record());
    }
    if rest.is_empty() {
        return Err(bad_record());
    }
    let offset = structured_data_end(rest).ok_or_else(bad_record)?;
    if offset < rest.len() && rest[offset] != b' ' {
        return Err(bad_record());
    }
    let structured_data = String::from_utf8_lossy(&rest[..offset]).into_owned();
    rest = if offset == rest.len() {
        &[]
    } else {
        &rest[offset + 1..]
    };
    if rest.starts_with(&[0xef, 0xbb, 0xbf]) {
        rest = &rest[3..];
        if std::str::from_utf8(rest).is_err() {
            return Err(bad_record());
        }
    }
    let message = rest.to_vec();
    let mut fields = fields.into_iter().skip(1);
    let mut next = || fields.next().unwrap_or_default();
    Ok(SyslogRecord {
        priority: p,
        timestamp: next(),
        hostname: next(),
        app_name: next(),
        process_id: next(),
        message_id: next(),
        structured_data,
        message,
        raw,
    })
}
fn valid_syslog_timestamp(value: &str) -> bool {
    let b = value.as_bytes();
    if b.len() < 20 || b.len() > 32 {
        return false;
    }
    for (i, c) in b.iter().enumerate().take(19) {
        let separator = match i {
            4 | 7 => b'-',
            10 => b'T',
            13 | 16 => b':',
            _ => 0,
        };
        if separator != 0 {
            if *c != separator {
                return false;
            }
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    let mut zone = 19;
    if b[zone] == b'.' {
        zone += 1;
        let start = zone;
        while zone < b.len() && b[zone].is_ascii_digit() {
            zone += 1;
        }
        if zone - start < 1 || zone - start > 6 {
            return false;
        }
    }
    let offset = &b[zone..];
    if offset != b"Z" {
        if offset.len() != 6 || (offset[0] != b'+' && offset[0] != b'-') || offset[3] != b':' {
            return false;
        }
        if ![1, 2, 4, 5].iter().all(|i| offset[*i].is_ascii_digit()) {
            return false;
        }
        if (offset[1] - b'0') * 10 + offset[2] - b'0' > 23
            || (offset[4] - b'0') * 10 + offset[5] - b'0' > 59
        {
            return false;
        }
    }
    timetext::parse_rfc3339(value).is_some()
}
pub fn parse_syslog_record(raw: &[u8], maximum: i64) -> Result<SyslogRecord> {
    if maximum < 1 || raw.len() as i64 > maximum {
        return Err(bad_record());
    }
    parse_record(raw.to_vec())
}
fn structured_data_end(p: &[u8]) -> Option<usize> {
    if p.is_empty() {
        return None;
    }
    if p[0] == b'-' {
        return Some(1);
    }
    let mut offset = 0;
    let mut ids: HashSet<&[u8]> = HashSet::new();
    fn name(p: &[u8], offset: &mut usize) -> bool {
        let start = *offset;
        while *offset < p.len() {
            let c = p[*offset];
            if c == b' ' || c == b']' || c == b'=' {
                break;
            }
            if !(33..=126).contains(&c) || c == b'"' {
                return false;
            }
            *offset += 1;
        }
        *offset > start && *offset - start <= 32
    }
    while offset < p.len() && p[offset] == b'[' {
        offset += 1;
        let id_start = offset;
        if !name(p, &mut offset) {
            return None;
        }
        if !ids.insert(&p[id_start..offset]) {
            return None;
        }
        while offset < p.len() && p[offset] == b' ' {
            offset += 1;
            if !name(p, &mut offset) || offset >= p.len() || p[offset] != b'=' {
                return None;
            }
            offset += 1;
            if offset >= p.len() || p[offset] != b'"' {
                return None;
            }
            offset += 1;
            let mut closed = false;
            while offset < p.len() {
                let c = p[offset];
                offset += 1;
                if c == b'"' {
                    closed = true;
                    break;
                }
                if c == b'\\' {
                    if offset >= p.len() {
                        return None;
                    }
                    if matches!(p[offset], b'"' | b'\\' | b']') {
                        offset += 1;
                    }
                } else if c == b']' {
                    return None;
                }
            }
            if !closed {
                return None;
            }
        }
        if offset >= p.len() || p[offset] != b']' {
            return None;
        }
        offset += 1;
    }
    (offset > 0 && std::str::from_utf8(&p[..offset]).is_ok()).then_some(offset)
}
pub type SyslogHandler =
    Arc<dyn Fn(&Ctx, SyslogRecord) -> std::result::Result<(), CallbackError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct ReceiverOptions {
    pub max_record_bytes: i64,
    pub max_connections: i64,
    pub read_timeout: Nanos,
    pub handle: Option<SyslogHandler>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReceiverStats {
    pub received: u64,
    pub failed: u64,
    pub rejected: u64,
}
pub struct Receiver {
    max_record_bytes: usize,
    max_connections: usize,
    read_timeout: Duration,
    handle: SyslogHandler,
    received: AtomicU64,
    failed: AtomicU64,
    rejected: AtomicU64,
}
enum Framing {
    Eof,
    UnexpectedEof,
    Io,
    Malformed,
    Ended,
}
impl Receiver {
    pub fn new(options: ReceiverOptions) -> Result<Receiver> {
        let mut o = options;
        if o.max_record_bytes == 0 {
            o.max_record_bytes = 4 << 20;
        }
        if o.max_connections == 0 {
            o.max_connections = 32;
        }
        if o.read_timeout.is_zero() {
            o.read_timeout = Nanos::from_secs(30);
        }
        let handle = match o.handle {
            Some(handle)
                if (32768..=64 << 20).contains(&o.max_record_bytes)
                    && (1..=1024).contains(&o.max_connections)
                    && o.read_timeout.0 >= 1_000_000
                    && o.read_timeout.0 <= 3_600_000_000_000 =>
            {
                handle
            }
            _ => {
                return Err(Error::spool(
                    "receiver_configuration",
                    ErrorKind::SyslogConfig,
                ));
            }
        };
        Ok(Receiver {
            max_record_bytes: o.max_record_bytes as usize,
            max_connections: o.max_connections as usize,
            read_timeout: Duration::from_nanos(o.read_timeout.0 as u64),
            handle,
            received: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
        })
    }
    pub fn stats(&self) -> ReceiverStats {
        ReceiverStats {
            received: self.received.load(Ordering::SeqCst),
            failed: self.failed.load(Ordering::SeqCst),
            rejected: self.rejected.load(Ordering::SeqCst),
        }
    }
    fn handle(&self, ctx: &Ctx, raw: Vec<u8>) -> Result<()> {
        let record = match parse_record(raw) {
            Ok(record) => record,
            Err(e) => {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                return Err(e);
            }
        };
        let handle = self.handle.clone();
        let ctx = ctx.clone();
        let handled =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || handle(&ctx, record)));
        if !matches!(handled, Ok(Ok(()))) {
            self.failed.fetch_add(1, Ordering::SeqCst);
            return Err(Error::spool("receiver_handler", ErrorKind::DurableSpool));
        }
        self.received.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn read_full(
        &self,
        ctx: &Ctx,
        stream: &mut TcpStream,
        buf: &mut [u8],
        deadline: Instant,
    ) -> std::result::Result<(), Framing> {
        let mut filled = 0;
        while filled < buf.len() {
            if ctx.err().is_some() {
                return Err(Framing::Ended);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Framing::Io);
            }
            let _ = stream.set_nonblocking(true);
            match stream.read(&mut buf[filled..]) {
                Ok(0) if filled == 0 => return Err(Framing::Eof),
                Ok(0) => return Err(Framing::UnexpectedEof),
                Ok(n) => filled += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(_) => return Err(Framing::Io),
            }
        }
        Ok(())
    }
    fn read_octet_record(
        &self,
        ctx: &Ctx,
        stream: &mut TcpStream,
    ) -> std::result::Result<Vec<u8>, Framing> {
        let maximum = self.max_record_bytes;
        let deadline = Instant::now() + self.read_timeout;
        let mut size = 0usize;
        let mut digits = 0;
        loop {
            let mut c = [0u8; 1];
            self.read_full(ctx, stream, &mut c, deadline)?;
            let c = c[0];
            if c == b' ' {
                if digits == 0 || size == 0 {
                    return Err(Framing::Malformed);
                }
                break;
            }
            if !c.is_ascii_digit() || digits >= 10 || (digits == 0 && c == b'0') {
                return Err(Framing::Malformed);
            }
            let digit = usize::from(c - b'0');
            if size > (maximum - digit) / 10 {
                return Err(Framing::Malformed);
            }
            size = size * 10 + digit;
            digits += 1;
        }
        if size > maximum {
            return Err(Framing::Malformed);
        }
        let mut raw = vec![0u8; size];
        self.read_full(ctx, stream, &mut raw, deadline)?;
        Ok(raw)
    }
    fn connection(&self, ctx: &Ctx, mut stream: TcpStream) {
        loop {
            if ctx.err().is_some() {
                return;
            }
            let raw = match self.read_octet_record(ctx, &mut stream) {
                Ok(raw) => raw,
                Err(framing) => {
                    let eof = matches!(framing, Framing::Eof);
                    if !eof && !matches!(framing, Framing::Ended) && ctx.err().is_none() {
                        self.rejected.fetch_add(1, Ordering::SeqCst);
                    }
                    return;
                }
            };
            if self.handle(ctx, raw).is_err() {
                return;
            }
        }
    }
    pub fn serve(self: &Arc<Self>, ctx: &Ctx, listener: TcpListener) -> Error {
        if listener.set_nonblocking(true).is_err() {
            return Error::spool("receiver_accept", ErrorKind::DurableSpool);
        }
        let active = Arc::new(Mutex::new(0usize));
        let mut workers = Vec::new();
        let mut accept_failed = false;
        loop {
            if ctx.err().is_some() {
                break;
            }
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                    workers.retain(|w: &std::thread::JoinHandle<()>| !w.is_finished());
                    continue;
                }
                Err(_) => {
                    accept_failed = true;
                    break;
                }
            };
            let _ = stream.set_nonblocking(false);
            {
                let mut n = active.lock().unwrap_or_else(|e| e.into_inner());
                if *n >= self.max_connections {
                    self.rejected.fetch_add(1, Ordering::SeqCst);
                    continue;
                }
                *n += 1;
            }
            let receiver = self.clone();
            let ctx = ctx.clone();
            let active = active.clone();
            workers.push(std::thread::spawn(move || {
                receiver.connection(&ctx, stream);
                *active.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
            }));
        }
        drop(listener);
        for worker in workers {
            let _ = worker.join();
        }
        if let Some(e) = ctx_err(ctx) {
            return e;
        }
        debug_assert!(accept_failed);
        Error::spool("receiver_accept", ErrorKind::DurableSpool)
    }
    pub fn serve_udp(&self, ctx: &Ctx, socket: UdpSocket) -> Error {
        let maximum = self.max_record_bytes.min(MAX_SYSLOG_UDP_MESSAGE);
        let mut buffer = vec![0u8; maximum + 1];
        let _ = socket.set_read_timeout(None);
        let _ = socket.set_nonblocking(true);
        loop {
            if let Some(e) = ctx_err(ctx) {
                return e;
            }
            let n = match socket.recv_from(&mut buffer) {
                Ok((n, _)) => n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(e) if e.raw_os_error() == Some(10040) => maximum + 1,
                Err(_) => {
                    return ctx_err(ctx)
                        .unwrap_or_else(|| Error::spool("receiver_read", ErrorKind::DurableSpool));
                }
            };
            if n > maximum {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                continue;
            }
            let _ = self.handle(ctx, buffer[..n].to_vec());
        }
    }
}
