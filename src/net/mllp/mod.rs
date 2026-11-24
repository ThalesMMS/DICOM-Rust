#![forbid(unsafe_code)]
mod client;
mod server;
mod stream;
use crate::hl7;
pub use client::{Client, ClientOptions};
#[cfg(feature = "tls")]
pub use server::serve_tls;
pub use server::{Handler, OriginalAckHandler, Request, ServerOptions, Shutdown, serve};
use std::fmt;
use std::io::Read;
#[cfg(feature = "tls")]
pub use stream::ClientTls;
pub const START_BLOCK: u8 = 0x0B;
pub const END_BLOCK: u8 = 0x1C;
pub const CARRIAGE_RETURN: u8 = 0x0D;
pub const DEFAULT_MAX_FRAME_BYTES: i64 = 1 << 20;
const TRANSPORT_ACK: u8 = 0x06;
const TRANSPORT_NAK: u8 = 0x15;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentinel {
    Framing,
    FrameTooLarge,
    Eof,
    UnexpectedEof,
    ShortWrite,
    UnsupportedRelease,
    TransportNak,
    TransportConfirmation,
    ResponseMismatch,
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        match self {
            Sentinel::Framing => "mllp: framing error",
            Sentinel::FrameTooLarge => "mllp: frame exceeds the size limit",
            Sentinel::Eof => "EOF",
            Sentinel::UnexpectedEof => "unexpected EOF",
            Sentinel::ShortWrite => "short write",
            Sentinel::UnsupportedRelease => "mllp: unsupported release",
            Sentinel::TransportNak => "mllp: transport frame rejected",
            Sentinel::TransportConfirmation => "mllp: malformed transport confirmation",
            Sentinel::ResponseMismatch => "mllp: response does not acknowledge the message sent",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangePhase {
    WriteRequest,
    RequestConfirmation,
    ReadResponse,
    ResponseConfirmation,
    ParseResponse,
    CorrelateResponse,
}
impl ExchangePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            ExchangePhase::WriteRequest => "write request",
            ExchangePhase::RequestConfirmation => "request transport confirmation",
            ExchangePhase::ReadResponse => "read application response",
            ExchangePhase::ResponseConfirmation => "response transport confirmation",
            ExchangePhase::ParseResponse => "parse application response",
            ExchangePhase::CorrelateResponse => "correlate application response",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTransportState {
    NotSent,
    Unknown,
    Acknowledged,
    Rejected,
}
impl RequestTransportState {
    pub fn as_str(self) -> &'static str {
        match self {
            RequestTransportState::NotSent => "not sent",
            RequestTransportState::Unknown => "unknown",
            RequestTransportState::Acknowledged => "acknowledged",
            RequestTransportState::Rejected => "rejected",
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ExchangeError {
    pub phase: ExchangePhase,
    pub request_state: RequestTransportState,
    pub err: Error,
}
impl std::fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "mllp: MLLP2 exchange failed (phase={}, request transport={})",
            self.phase.as_str(),
            self.request_state.as_str()
        )
    }
}
impl std::error::Error for ExchangeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.err)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Sentinel(Sentinel),
    Wrapped { text: String, wrapped: Vec<Error> },
    Text(String),
    Hl7(hl7::Error),
    Exchange(Box<ExchangeError>),
    Io(String),
}
impl Error {
    fn wrap(s: Sentinel, detail: &str) -> Error {
        Error::Wrapped {
            text: format!("{}: {detail}", s.text()),
            wrapped: vec![Error::Sentinel(s)],
        }
    }
    fn join(a: Error, b: Error) -> Error {
        Error::Wrapped {
            text: format!("{a}\n{b}"),
            wrapped: vec![a, b],
        }
    }
    fn invalid_write_count() -> Error {
        Error::wrap(Sentinel::ShortWrite, "invalid writer byte count")
    }
    fn is_invalid_write_count(&self) -> bool {
        *self == Error::invalid_write_count()
    }
    pub fn is(&self, s: Sentinel) -> bool {
        match self {
            Error::Sentinel(x) => *x == s,
            Error::Wrapped { wrapped, .. } => wrapped.iter().any(|e| e.is(s)),
            Error::Exchange(e) => e.err.is(s),
            _ => false,
        }
    }
    pub fn is_hl7(&self, s: hl7::Sentinel) -> bool {
        match self {
            Error::Hl7(e) => e.is(s),
            Error::Wrapped { wrapped, .. } => wrapped.iter().any(|e| e.is_hl7(s)),
            Error::Exchange(e) => e.err.is_hl7(s),
            _ => false,
        }
    }
    pub fn as_exchange(&self) -> Option<&ExchangeError> {
        match self {
            Error::Exchange(e) => Some(e),
            Error::Wrapped { wrapped, .. } => wrapped.iter().find_map(Error::as_exchange),
            _ => None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Sentinel(s) => f.write_str(s.text()),
            Error::Wrapped { text, .. } | Error::Text(text) | Error::Io(text) => f.write_str(text),
            Error::Hl7(e) => e.fmt(f),
            Error::Exchange(e) => write!(
                f,
                "mllp: MLLP2 exchange failed (phase={}, request transport={})",
                e.phase.as_str(),
                e.request_state.as_str()
            ),
        }
    }
}
impl std::error::Error for Error {}
impl From<hl7::Error> for Error {
    fn from(e: hl7::Error) -> Error {
        Error::Hl7(e)
    }
}
fn io_error(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}
pub type Release = u8;
pub const MLLP1: Release = 0;
pub const MLLP2: Release = 1;
fn release_valid(r: Release) -> bool {
    r == MLLP1 || r == MLLP2
}
pub struct Reader<R: Read> {
    r: R,
    buf: Vec<u8>,
    start: usize,
    end: usize,
    err: Option<Error>,
    max: i64,
    on_start: Option<Box<dyn FnMut() -> Result<(), Error> + Send>>,
}
const READER_SIZE: usize = 32 << 10;
enum Slice {
    Delim(Vec<u8>),
    Full(Vec<u8>),
    Err(Vec<u8>, Error),
}
impl<R: Read> Reader<R> {
    pub fn new(r: R, max_frame_bytes: i64) -> Reader<R> {
        Reader {
            r,
            buf: vec![0; READER_SIZE],
            start: 0,
            end: 0,
            err: None,
            max: if max_frame_bytes <= 0 {
                DEFAULT_MAX_FRAME_BYTES
            } else {
                max_frame_bytes
            },
            on_start: None,
        }
    }
    fn fill(&mut self) {
        if self.start > 0 {
            self.buf.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
        for _ in 0..100 {
            match self.r.read(&mut self.buf[self.end..]) {
                Ok(0) => {
                    self.err = Some(Error::Sentinel(Sentinel::Eof));
                    return;
                }
                Ok(n) => {
                    self.end += n;
                    return;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.err = Some(io_error(e));
                    return;
                }
            }
        }
        self.err = Some(Error::Text(
            "multiple Read calls return no data or error".into(),
        ));
    }
    fn read_byte(&mut self) -> Result<u8, Error> {
        while self.start == self.end {
            if let Some(e) = self.err.take() {
                return Err(e);
            }
            self.fill();
        }
        let b = self.buf[self.start];
        self.start += 1;
        Ok(b)
    }
    fn read_slice(&mut self, delim: u8) -> Slice {
        let mut s = 0;
        loop {
            if let Some(i) = self.buf[self.start + s..self.end]
                .iter()
                .position(|&c| c == delim)
            {
                let i = i + s;
                let line = self.buf[self.start..self.start + i + 1].to_vec();
                self.start += i + 1;
                return Slice::Delim(line);
            }
            if let Some(e) = self.err.take() {
                let line = self.buf[self.start..self.end].to_vec();
                self.start = self.end;
                return Slice::Err(line, e);
            }
            if self.end - self.start >= self.buf.len() {
                let line = self.buf[self.start..self.end].to_vec();
                self.start = self.end;
                return Slice::Full(line);
            }
            s = self.end - self.start;
            self.fill();
        }
    }
    pub fn read_frame(&mut self) -> Result<Vec<u8>, Error> {
        let max = self.max;
        self.read_frame_max(max)
    }
    fn read_frame_max(&mut self, max: i64) -> Result<Vec<u8>, Error> {
        let first = self.read_byte()?;
        if first != START_BLOCK {
            return Err(Error::wrap(
                Sentinel::Framing,
                &format!("byte 0x{first:02X} before the start block"),
            ));
        }
        if let Some(on_start) = self.on_start.as_mut() {
            on_start()?;
        }
        let mut payload: Vec<u8> = Vec::new();
        loop {
            let (chunk, err, full) = match self.read_slice(END_BLOCK) {
                Slice::Delim(c) => (c, None, false),
                Slice::Full(c) => (c, None, true),
                Slice::Err(c, e) => (c, Some(e), false),
            };
            let room = max - payload.len() as i64;
            if chunk.len() as i64 > room && (err.is_some() || full || chunk.len() as i64 - 1 > room)
            {
                return Err(Error::wrap(
                    Sentinel::FrameTooLarge,
                    &format!("more than {max} bytes"),
                ));
            }
            payload.extend_from_slice(&chunk);
            if full {
                continue;
            }
            if let Some(e) = err {
                if e.is(Sentinel::Eof) {
                    return Err(Error::wrap(
                        Sentinel::UnexpectedEof,
                        &format!("stream ended inside a frame of {} bytes", payload.len()),
                    ));
                }
                return Err(e);
            }
            break;
        }
        payload.pop();
        let trailer = match self.read_byte() {
            Ok(b) => b,
            Err(e) if e.is(Sentinel::Eof) => {
                return Err(Error::wrap(
                    Sentinel::UnexpectedEof,
                    "stream ended after the end block",
                ));
            }
            Err(e) => return Err(e),
        };
        if trailer != CARRIAGE_RETURN {
            return Err(Error::wrap(
                Sentinel::Framing,
                &format!("byte 0x{trailer:02X} after the end block, want CR"),
            ));
        }
        if payload.is_empty() {
            return Err(Error::wrap(Sentinel::Framing, "empty frame"));
        }
        if payload.len() as i64 > max {
            return Err(Error::wrap(
                Sentinel::FrameTooLarge,
                &format!("more than {max} bytes"),
            ));
        }
        if let Some(i) = payload.iter().position(|&c| c == START_BLOCK) {
            return Err(Error::wrap(
                Sentinel::Framing,
                &format!("start block inside the frame at byte {i}"),
            ));
        }
        Ok(payload)
    }
    fn read_confirmation(&mut self) -> Result<(), Error> {
        let on_start = self.on_start.take();
        let result = self.read_frame_max(1);
        self.on_start = on_start;
        let payload = match result {
            Ok(p) => p,
            Err(e) if e.is(Sentinel::Framing) || e.is(Sentinel::FrameTooLarge) => {
                return Err(Error::join(
                    Error::Sentinel(Sentinel::TransportConfirmation),
                    e,
                ));
            }
            Err(e) => return Err(e),
        };
        match payload[0] {
            TRANSPORT_ACK => Ok(()),
            TRANSPORT_NAK => Err(Error::Sentinel(Sentinel::TransportNak)),
            _ => Err(Error::Sentinel(Sentinel::TransportConfirmation)),
        }
    }
}
pub fn append_frame(mut dst: Vec<u8>, payload: &[u8]) -> (Vec<u8>, Option<Error>) {
    if let Err(e) = validate_frame_payload(payload) {
        return (dst, Some(e));
    }
    dst.push(START_BLOCK);
    dst.extend_from_slice(payload);
    dst.push(END_BLOCK);
    dst.push(CARRIAGE_RETURN);
    (dst, None)
}
pub(crate) fn validate_frame_payload(payload: &[u8]) -> Result<(), Error> {
    if payload.is_empty() {
        return Err(Error::wrap(Sentinel::Framing, "empty payload"));
    }
    if let Some(i) = payload
        .iter()
        .position(|&c| c == START_BLOCK || c == END_BLOCK)
    {
        return Err(Error::wrap(
            Sentinel::Framing,
            &format!("frame byte inside the payload at byte {i}"),
        ));
    }
    Ok(())
}
pub trait ByteWrite {
    fn write_avail(&mut self, p: &[u8]) -> (i64, Option<Error>);
}
pub(crate) struct IoWriter<'a, W: std::io::Write>(pub &'a mut W);
impl<W: std::io::Write> ByteWrite for IoWriter<'_, W> {
    fn write_avail(&mut self, p: &[u8]) -> (i64, Option<Error>) {
        loop {
            match self.0.write(p) {
                Ok(n) => return (n as i64, None),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return (0, Some(io_error(e))),
            }
        }
    }
}
pub fn write_frame(w: &mut dyn ByteWrite, payload: &[u8]) -> Result<(), Error> {
    write_frame_count(w, payload).1
}
fn write_frame_count(w: &mut dyn ByteWrite, payload: &[u8]) -> (usize, Result<(), Error>) {
    if let Err(e) = validate_frame_payload(payload) {
        return (0, Err(e));
    }
    let (frame, _) = append_frame(Vec::with_capacity(payload.len() + 3), payload);
    let mut written = 0usize;
    while written < frame.len() {
        let (n, err) = w.write_avail(&frame[written..]);
        if n < 0 || n as usize > frame.len() - written {
            return (written, Err(Error::invalid_write_count()));
        }
        written += n as usize;
        if let Some(e) = err {
            return (written, Err(e));
        }
        if n == 0 {
            return (written, Err(Error::Sentinel(Sentinel::ShortWrite)));
        }
    }
    (written, Ok(()))
}
fn write_confirmation(w: &mut dyn ByteWrite, accepted: bool) -> Result<(), Error> {
    write_frame(
        w,
        &[if accepted {
            TRANSPORT_ACK
        } else {
            TRANSPORT_NAK
        }],
    )
}
fn transport_phase_error(phase: &str, err: Error) -> Error {
    Error::Wrapped {
        text: format!("mllp: {phase}: {err}"),
        wrapped: vec![err],
    }
}
fn exchange_error(phase: ExchangePhase, state: RequestTransportState, err: Error) -> Error {
    Error::Exchange(Box::new(ExchangeError {
        phase,
        request_state: state,
        err,
    }))
}
