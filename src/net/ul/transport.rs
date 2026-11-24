#![forbid(unsafe_code)]
use super::conn::{Conn, Ctx, Nanos, association_context_error, ended_context_error};
use super::error::{Detail, Error, Kind, Result};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;
pub type DialContextFn = Arc<dyn Fn(&Ctx, &str, &str) -> Result<TcpStream> + Send + Sync>;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpConnectOptions {
    pub address: String,
    pub username: String,
    pub password: String,
    pub timeout: Nanos,
    pub max_response_header_bytes: i64,
}
#[derive(Clone, Default)]
pub struct TransportOptions {
    pub dial_context: Option<DialContextFn>,
    pub local_address: Option<SocketAddr>,
    pub http_connect: Option<HttpConnectOptions>,
}
impl std::fmt::Debug for TransportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportOptions")
            .field("dial_context", &self.dial_context.is_some())
            .field("local_address", &self.local_address)
            .field(
                "http_connect",
                &self.http_connect.as_ref().map(|p| &p.address),
            )
            .finish()
    }
}
fn configuration() -> Error {
    Error::sentinel(Kind::TransportConfiguration)
}
fn split_host_port(address: &str) -> Option<(&str, &str)> {
    let i = address.rfind(':')?;
    let (host, port) = (&address[..i], &address[i + 1..]);
    if let Some(rest) = address.strip_prefix('[') {
        let end = rest.find(']')?;
        if end + 2 != i {
            return None;
        }
        let host = &rest[..end];
        if host.contains('[') || host.contains(']') {
            return None;
        }
        return Some((host, port));
    }
    if host.contains(':') || host.contains('[') || host.contains(']') {
        return None;
    }
    Some((host, port))
}
fn valid_transport_authority(address: &str) -> bool {
    if address.len() > 1024 || address.contains([' ', '\t', '\r', '\n', '/', '?', '#', '@']) {
        return false;
    }
    let Some((host, port)) = split_host_port(address) else {
        return false;
    };
    if host.is_empty() {
        return false;
    }
    let digits = port.strip_prefix(['+', '-']).unwrap_or(port);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    matches!(port.parse::<i64>(), Ok(n) if n > 0 && n <= 65535)
}
impl TransportOptions {
    pub fn validate(&self) -> Result<()> {
        self.validate_with(&super::socket_buffers::SocketBuffers::default())
    }
    pub(crate) fn validate_with(
        &self,
        buffers: &super::socket_buffers::SocketBuffers,
    ) -> Result<()> {
        if self.dial_context.is_some() && (self.local_address.is_some() || !buffers.is_zero()) {
            return Err(Error::wrap(
                Kind::TransportConfiguration,
                "custom dialer owns local binding and socket buffers",
            ));
        }
        if let Some(p) = &self.http_connect {
            let max = p.max_response_header_bytes;
            if !valid_transport_authority(&p.address)
                || p.timeout.is_negative()
                || (max != 0 && !(256..=1 << 20).contains(&max))
                || p.username.len() + p.password.len() > 8192
                || p.username.contains([':', '\r', '\n'])
                || p.password.contains(['\r', '\n'])
            {
                return Err(configuration());
            }
        }
        Ok(())
    }
    pub(crate) fn dial(
        &self,
        ctx: &Ctx,
        address: &str,
        connect: impl Fn(&str, &Ctx) -> Result<Conn>,
    ) -> Result<Conn> {
        self.validate()?;
        let mut ctx = ctx.clone();
        let mut destination = address;
        if let Some(p) = &self.http_connect {
            if !valid_transport_authority(address) {
                return Err(configuration());
            }
            destination = &p.address;
            let timeout = p.timeout.positive().unwrap_or(Duration::from_secs(30));
            ctx = ctx.with_timeout(timeout);
        }
        let conn = match &self.dial_context {
            Some(dialer) => dialer(&ctx, "tcp", destination).and_then(|stream| {
                Conn::new(stream).map_err(|e| Error::other(format!("dial tcp {destination}: {e}")))
            }),
            None => connect(destination, &ctx),
        }
        .map_err(|e| association_context_error(&ctx, e))?;
        if let Some(kind) = ctx.err() {
            conn.close();
            return Err(Error::sentinel(kind));
        }
        let Some(p) = &self.http_connect else {
            return Ok(conn);
        };
        match establish_http_connect(&ctx, &conn, address, p) {
            Ok(()) => Ok(conn),
            Err(e) => {
                conn.close();
                Err(e)
            }
        }
    }
}
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
fn malformed() -> Error {
    Error::wrap(Kind::HttpConnect, "malformed response")
}
fn stream_error(ctx: &Ctx, err: Error, detail: &str) -> Error {
    if let Some(ended) = ended_context_error(ctx) {
        return ended;
    }
    if err.is(Kind::NetTimeout) || err.is(Kind::AssociationTimeout) {
        return Error::sentinel(Kind::AssociationTimeout);
    }
    Error::wrap(Kind::HttpConnect, detail)
}
fn establish_http_connect(
    ctx: &Ctx,
    conn: &Conn,
    address: &str,
    p: &HttpConnectOptions,
) -> Result<()> {
    let max = if p.max_response_header_bytes == 0 {
        32 << 10
    } else {
        p.max_response_header_bytes as usize
    };
    let mut request = format!("CONNECT {address} HTTP/1.1\r\nHost: {address}\r\n");
    if !p.username.is_empty() || !p.password.is_empty() {
        let credentials = format!("{}:{}", p.username, p.password);
        request.push_str(&format!(
            "Proxy-Authorization: Basic {}\r\n",
            base64(credentials.as_bytes())
        ));
    }
    request.push_str("\r\n");
    conn.raw_write(request.as_bytes(), ctx)
        .1
        .map_err(|e| stream_error(ctx, e, "write request"))?;
    let mut header: Vec<u8> = Vec::new();
    let mut buffered: Vec<u8> = Vec::new();
    let mut line_start = 0usize;
    let mut chunk = [0u8; 4096];
    loop {
        let mut failure = None;
        let piece_len = loop {
            if let Some(i) = buffered.iter().position(|b| *b == b'\n') {
                break i + 1;
            }
            if buffered.len() >= 4096 {
                break 4096;
            }
            match conn.raw_read(&mut chunk, ctx) {
                Ok(0) => {
                    failure = Some(Error::eof());
                    break buffered.len();
                }
                Ok(n) => buffered.extend_from_slice(&chunk[..n]),
                Err(e) => {
                    failure = Some(e);
                    break buffered.len();
                }
            }
        };
        let complete = buffered[..piece_len].last() == Some(&b'\n');
        if header.len() + piece_len > max {
            return Err(Error::sentinel(Kind::HttpConnectHeaderLimit));
        }
        header.extend(buffered.drain(..piece_len));
        if let Some(e) = failure {
            return Err(stream_error(ctx, e, "incomplete response"));
        }
        if !complete {
            continue;
        }
        if header.len() < 2 || header[header.len() - 2] != b'\r' {
            return Err(malformed());
        }
        if header.len() - line_start == 2 {
            break;
        }
        line_start = header.len();
    }
    let status = parse_response(&header).ok_or_else(malformed)?;
    if !(200..=299).contains(&status) {
        return Err(Error::with_kinds(
            format!("{} (status {status})", Kind::HttpConnect.text()),
            vec![Kind::HttpConnect],
        )
        .with_detail(Detail::HttpConnect(status)));
    }
    if let Some(kind) = ctx.err() {
        return Err(Error::sentinel(kind));
    }
    conn.set_prefetched(buffered);
    Ok(())
}
fn token_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c)
}
fn trim_http(s: &str) -> &str {
    s.trim_matches([' ', '\t'])
}
fn parse_response(header: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(header).ok()?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next()?;
    let (proto, status) = status_line.split_once(' ')?;
    let status = status.trim_start_matches(' ');
    let code = status.split(' ').next().unwrap_or("");
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let code: i64 = code.parse().ok()?;
    let version = proto.strip_prefix("HTTP/")?.as_bytes();
    if version.len() != 3 || version[1] != b'.' {
        return None;
    }
    if !version[0].is_ascii_digit() || !version[2].is_ascii_digit() {
        return None;
    }
    let (major, minor) = (version[0] - b'0', version[2] - b'0');
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut first = true;
    for line in lines {
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if first {
                return None;
            }
            if let Some(last) = fields.last_mut() {
                last.1.push(' ');
                last.1.push_str(trim_http(line));
            }
            continue;
        }
        first = false;
        let (key, value) = line.split_once(':')?;
        if key.bytes().any(|c| !token_byte(c) && c != b' ') {
            return None;
        }
        if key.is_empty() || value.bytes().any(|c| c < 0x20 && c != 9) {
            return None;
        }
        fields.push((key.to_ascii_lowercase(), trim_http(value).to_string()));
    }
    let values = |name: &str| -> Vec<&str> {
        fields
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect()
    };
    let encodings = values("transfer-encoding");
    let mut chunked = false;
    if !encodings.is_empty() && (major, minor) >= (1, 1) {
        if encodings.len() != 1 || !encodings[0].eq_ignore_ascii_case("chunked") {
            return None;
        }
        chunked = true;
    }
    let lengths = values("content-length");
    if lengths.len() > 1
        && lengths
            .iter()
            .any(|l| trim_http(l) != trim_http(lengths[0]))
    {
        return None;
    }
    let _ = chunked;
    {
        if let Some(length) = lengths.first() {
            let length = trim_http(length);
            if length.is_empty()
                || !length.bytes().all(|b| b.is_ascii_digit())
                || length.parse::<i64>().is_err()
            {
                return None;
            }
        }
    }
    if major != 1 || minor > 1 {
        return None;
    }
    Some(code)
}
