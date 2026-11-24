use super::mimeheader::{Header, read_mime_header};
use super::types::{Error, ResponseDecodeError, ResponseDecodeErrorKind};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadErr {
    Eof,
    UnexpectedEof,
    BufferFull,
    Decode(ResponseDecodeError),
    Web(Box<Error>),
    Net { timeout: bool, text: String },
    Canceled,
    Deadline,
    Other(String),
}
impl std::fmt::Display for ReadErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadErr::Eof => f.write_str("EOF"),
            ReadErr::UnexpectedEof => f.write_str("unexpected EOF"),
            ReadErr::BufferFull => f.write_str("bufio: buffer full"),
            ReadErr::Decode(e) => write!(f, "{e}"),
            ReadErr::Web(e) => write!(f, "{e}"),
            ReadErr::Net { text, .. } | ReadErr::Other(text) => f.write_str(text),
            ReadErr::Canceled => f.write_str("context canceled"),
            ReadErr::Deadline => f.write_str("context deadline exceeded"),
        }
    }
}
impl std::error::Error for ReadErr {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ReadErr::Decode(e) => std::error::Error::source(e),
            ReadErr::Web(e) => std::error::Error::source(e.as_ref()),
            _ => None,
        }
    }
}
pub trait ByteRead {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>);
}
pub struct SliceReader<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> SliceReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        SliceReader { data, pos: 0 }
    }
}
impl ByteRead for SliceReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        if self.pos >= self.data.len() {
            return (0, Some(ReadErr::Eof));
        }
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        (n, None)
    }
}
pub fn read_all(reader: &mut dyn ByteRead) -> (Vec<u8>, Option<ReadErr>) {
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 32 << 10];
    loop {
        let (n, err) = reader.read(&mut chunk);
        out.extend_from_slice(&chunk[..n]);
        match err {
            Some(ReadErr::Eof) => return (out, None),
            Some(e) => return (out, Some(e)),
            None => {}
        }
    }
}
struct BufReader<'a> {
    rd: &'a mut dyn ByteRead,
    buf: Vec<u8>,
    r: usize,
    w: usize,
    err: Option<ReadErr>,
    sticky: Option<ReadErr>,
}
impl<'a> BufReader<'a> {
    fn new(rd: &'a mut dyn ByteRead, size: usize) -> Self {
        BufReader {
            rd,
            buf: vec![0u8; size.max(16)],
            r: 0,
            w: 0,
            err: None,
            sticky: None,
        }
    }
    fn source_read(&mut self, from: usize) -> (usize, Option<ReadErr>) {
        if let Some(e) = &self.sticky {
            return (0, Some(e.clone()));
        }
        let (n, err) = self.rd.read(&mut self.buf[from..]);
        self.sticky = err.clone();
        (n, err)
    }
    fn fill(&mut self) {
        if self.r > 0 {
            self.buf.copy_within(self.r..self.w, 0);
            self.w -= self.r;
            self.r = 0;
        }
        for _ in 0..100 {
            let (n, err) = self.source_read(self.w);
            self.w += n;
            if err.is_some() {
                self.err = err;
                return;
            }
            if n > 0 {
                return;
            }
        }
        self.err = Some(ReadErr::Other(
            "multiple Read calls return no data or error".into(),
        ));
    }
    fn buffered(&self) -> usize {
        self.w - self.r
    }
    fn peek(&mut self, n: usize) -> (usize, Option<ReadErr>) {
        while self.buffered() < n && self.buffered() < self.buf.len() && self.err.is_none() {
            self.fill();
        }
        if n > self.buf.len() {
            return (self.buffered(), Some(ReadErr::BufferFull));
        }
        let avail = self.buffered();
        if avail < n {
            let err = self.err.take().unwrap_or(ReadErr::BufferFull);
            return (avail, Some(err));
        }
        (n, None)
    }
    fn data(&self, n: usize) -> &[u8] {
        &self.buf[self.r..self.r + n.min(self.buffered())]
    }
    fn discard(&mut self, n: usize) {
        self.r += n.min(self.buffered());
    }
    fn read(&mut self, out: &mut [u8]) -> (usize, Option<ReadErr>) {
        if out.is_empty() {
            return (0, None);
        }
        if self.r == self.w {
            if let Some(e) = self.err.take() {
                return (0, Some(e));
            }
            self.r = 0;
            self.w = 0;
            let (n, err) = self.source_read(0);
            if n == 0 {
                return (0, err);
            }
            self.err = err;
            self.w = n;
        }
        let n = out.len().min(self.buffered());
        out[..n].copy_from_slice(&self.buf[self.r..self.r + n]);
        self.r += n;
        (n, None)
    }
    fn read_slice(&mut self) -> (Vec<u8>, Option<ReadErr>) {
        let mut scanned = 0;
        loop {
            if let Some(i) = self.buf[self.r + scanned..self.w]
                .iter()
                .position(|&c| c == b'\n')
            {
                let end = self.r + scanned + i + 1;
                let line = self.buf[self.r..end].to_vec();
                self.r = end;
                return (line, None);
            }
            if let Some(e) = self.err.take() {
                let line = self.buf[self.r..self.w].to_vec();
                self.r = self.w;
                return (line, Some(e));
            }
            if self.buffered() >= self.buf.len() {
                let line = self.buf[self.r..self.w].to_vec();
                self.r = self.w;
                return (line, Some(ReadErr::BufferFull));
            }
            scanned = self.buffered();
            self.fill();
        }
    }
}
const LINE_BYTES: usize = 4096;
const HEADER_FIELDS: usize = 10_000;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Preamble,
    Headers,
    Body,
    End,
}
#[derive(Default)]
struct PartState {
    first: bool,
    safe: usize,
    err: Option<ReadErr>,
}
pub struct MultipartReader<'a> {
    wire: BufReader<'a>,
    marker: Vec<u8>,
    separator: Vec<u8>,
    line_marker: Vec<u8>,
    max_header_bytes: usize,
    phase: Phase,
    part: PartState,
}
fn incomplete(err: ReadErr) -> ReadErr {
    if err == ReadErr::Eof {
        ReadErr::UnexpectedEof
    } else {
        err
    }
}
fn trim_left_lws(s: &[u8]) -> &[u8] {
    let n = s
        .iter()
        .position(|c| !matches!(c, b' ' | b'\t'))
        .unwrap_or(s.len());
    &s[n..]
}
impl<'a> MultipartReader<'a> {
    pub fn new(input: &'a mut dyn ByteRead, boundary: &str, max_header_bytes: i64) -> Self {
        MultipartReader {
            wire: BufReader::new(input, LINE_BYTES + 2),
            marker: format!("--{boundary}").into_bytes(),
            separator: Vec::new(),
            line_marker: Vec::new(),
            max_header_bytes: usize::try_from(max_header_bytes).unwrap_or(0),
            phase: Phase::Preamble,
            part: PartState::default(),
        }
    }
    pub(crate) fn next_part(&mut self) -> Result<Header, ReadErr> {
        if self.phase == Phase::Preamble {
            self.read_first_delimiter()?;
        }
        if self.phase == Phase::End {
            return Err(ReadErr::Eof);
        }
        if self.phase != Phase::Headers {
            return Err(ReadErr::UnexpectedEof);
        }
        let header = self.read_headers()?;
        self.phase = Phase::Body;
        self.part = PartState {
            first: true,
            safe: 0,
            err: None,
        };
        Ok(header)
    }
    fn read_first_delimiter(&mut self) -> Result<(), ReadErr> {
        loop {
            let (line, err) = self.wire.read_slice();
            if line.len() > LINE_BYTES {
                return Err(ReadErr::UnexpectedEof);
            }
            let eof = match err {
                None => false,
                Some(ReadErr::Eof) => true,
                Some(e) => return Err(e),
            };
            if let Some(suffix) = line.strip_prefix(self.marker.as_slice()) {
                let (suffix, closing) = match suffix.strip_prefix(b"--") {
                    Some(rest) => (rest, true),
                    None => (suffix, false),
                };
                let rest = trim_left_lws(suffix);
                if rest == b"\r\n" || rest == b"\n" {
                    let ending: &[u8] = if line.ends_with(b"\r\n") {
                        b"\r\n"
                    } else {
                        b"\n"
                    };
                    self.separator = ending.to_vec();
                    self.line_marker = [ending, self.marker.as_slice()].concat();
                    self.finish_delimiter(closing);
                    return Ok(());
                }
                if rest.is_empty() && eof && closing {
                    self.phase = Phase::End;
                    return Ok(());
                }
            }
            if eof {
                return Err(ReadErr::UnexpectedEof);
            }
        }
    }
    fn finish_delimiter(&mut self, closing: bool) {
        self.phase = if closing { Phase::End } else { Phase::Headers };
    }
    fn read_headers(&mut self) -> Result<Header, ReadErr> {
        let mut raw: Vec<u8> = Vec::new();
        let mut fields = 0;
        let mut continued = false;
        loop {
            let (line, err) = self.wire.read_slice();
            if line.len() > self.max_header_bytes.saturating_sub(raw.len()) {
                let limit = self.max_header_bytes as i64;
                return Err(ReadErr::Decode(ResponseDecodeError::with(
                    ResponseDecodeErrorKind::HeaderBytes,
                    limit,
                    limit + 1,
                )));
            }
            raw.extend_from_slice(&line);
            if !continued && err.is_none() && (line == b"\r\n" || line == b"\n") {
                return read_mime_header(&raw).map_err(ReadErr::Other);
            }
            if !continued && !line.is_empty() && line[0] != b' ' && line[0] != b'\t' {
                fields += 1;
                if fields > HEADER_FIELDS {
                    return Err(ReadErr::Decode(ResponseDecodeError::new(
                        ResponseDecodeErrorKind::MalformedMultipart,
                    )));
                }
            }
            match err {
                None => continued = false,
                Some(ReadErr::BufferFull) => continued = true,
                Some(e) => return Err(incomplete(e)),
            }
        }
    }
    fn complete_prefix(&mut self, prefix: &[u8]) -> bool {
        let mut checked = 0;
        loop {
            let count = self.wire.buffered().min(prefix.len());
            if self.wire.data(count)[checked..] != prefix[checked..count] {
                return false;
            }
            if count == prefix.len() {
                return true;
            }
            checked = count;
            if self.wire.peek(count + 1).1.is_some() {
                return false;
            }
        }
    }
    fn delimiter_at(
        &mut self,
        prefix_bytes: usize,
        preceding_bytes: usize,
    ) -> Option<(usize, bool)> {
        let mut cursor = prefix_bytes;
        let limit = LINE_BYTES + preceding_bytes;
        let mut closing = false;
        if self.wire.peek(cursor + 1).1.is_some() {
            return None;
        }
        if self.wire.data(cursor + 1)[cursor] == b'-' {
            if self.wire.peek(cursor + 2).1.is_some()
                || self.wire.data(cursor + 2)[cursor + 1] != b'-'
            {
                return None;
            }
            closing = true;
            cursor += 2;
        }
        while cursor < limit {
            match self.wire.peek(cursor + 1).1 {
                Some(ReadErr::Eof) if closing => return Some((cursor, true)),
                Some(_) => break,
                None => {}
            }
            let c = self.wire.data(cursor + 1)[cursor];
            if c == b' ' || c == b'\t' {
                cursor += 1;
                continue;
            }
            if c != self.separator[0] {
                return None;
            }
            let size = cursor + self.separator.len();
            if size <= limit
                && self.wire.peek(size).1.is_none()
                && self.wire.data(size)[cursor..] == self.separator[..]
            {
                return Some((size, closing));
            }
            return None;
        }
        None
    }
    pub(crate) fn read_part(&mut self, out: &mut [u8]) -> (usize, Option<ReadErr>) {
        if out.is_empty() {
            return (0, None);
        }
        if self.part.safe == 0 && self.part.err.is_none() {
            self.scan();
        }
        if self.part.safe == 0 {
            return (0, self.part.err.clone());
        }
        let limit = out.len().min(self.part.safe);
        let (n, err) = self.wire.read(&mut out[..limit]);
        self.part.safe -= n;
        (n, err)
    }
    fn scan(&mut self) {
        if let (_, Some(err)) = self.wire.peek(1) {
            self.part.err = Some(incomplete(err));
            return;
        }
        if self.part.first {
            self.part.first = false;
            let marker = self.marker.clone();
            if self.complete_prefix(&marker) {
                self.use_candidate(marker.len(), 0);
                return;
            }
        }
        let line_marker = self.line_marker.clone();
        let buffered = self.wire.buffered();
        let (position, tail) = {
            let data = self.wire.data(buffered);
            let position = data
                .windows(line_marker.len())
                .position(|w| w == line_marker.as_slice());
            let tail = data
                .iter()
                .rposition(|&c| c == line_marker[0])
                .filter(|&t| line_marker.starts_with(&data[t..]));
            (position, tail)
        };
        if let Some(position) = position {
            if position == 0 {
                self.use_candidate(line_marker.len(), self.separator.len());
            } else {
                self.part.safe = position;
            }
            return;
        }
        let Some(tail) = tail else {
            self.part.safe = buffered;
            return;
        };
        if tail != 0 {
            self.part.safe = tail;
            return;
        }
        if self.complete_prefix(&line_marker) {
            self.use_candidate(line_marker.len(), self.separator.len());
        } else {
            self.part.safe = 1;
        }
    }
    fn use_candidate(&mut self, prefix_bytes: usize, separator_bytes: usize) {
        match self.delimiter_at(prefix_bytes, separator_bytes) {
            Some((size, closing)) if size > 0 => {
                self.wire.discard(size);
                self.finish_delimiter(closing);
                self.part.err = Some(ReadErr::Eof);
            }
            _ => self.part.safe = prefix_bytes,
        }
    }
}
pub struct PartReader<'m, 'a> {
    pub multipart: &'m mut MultipartReader<'a>,
}
impl ByteRead for PartReader<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        self.multipart.read_part(buf)
    }
}
