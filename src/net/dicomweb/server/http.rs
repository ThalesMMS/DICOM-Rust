use super::super::mimeheader::{Header, canonical_header_key, read_mime_header};
use super::super::types::status_text;
use super::super::urlrules::{self, Url};
use super::ServerError;
use crate::net::ul::{CancelToken, Ctx};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
pub type Body = Box<dyn Read + Send>;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerTls {
    pub peer_certificates: Vec<Vec<u8>>,
    pub verified_chains: Vec<Vec<Vec<u8>>>,
}
pub struct Request {
    pub method: String,
    pub url: Url,
    pub request_uri: String,
    pub proto: String,
    pub host: String,
    pub header: Header,
    pub remote_addr: String,
    pub tls: bool,
    pub content_length: i64,
    pub body: Body,
    ctx: Ctx,
}
impl Request {
    pub fn new(method: &str, target: &str) -> Result<Request, crate::MessageError> {
        Ok(Request {
            method: method.to_string(),
            url: urlrules::parse_request_uri(target)?,
            request_uri: target.to_string(),
            proto: "HTTP/1.1".into(),
            host: String::new(),
            header: Header::new(),
            remote_addr: String::new(),
            tls: false,
            content_length: 0,
            body: Box::new(io::empty()),
            ctx: Ctx::background(),
        })
    }
    pub fn context(&self) -> &Ctx {
        &self.ctx
    }
    pub fn set_context(&mut self, ctx: Ctx) {
        self.ctx = ctx;
    }
}
pub trait ResponseWriter {
    fn header(&mut self) -> &mut Header;
    fn write_header(&mut self, status: u16);
    fn write(&mut self, data: &[u8]) -> Result<usize, ServerError>;
    fn flush(&mut self) {}
    fn abort(&mut self) {}
    fn hijack(&mut self) -> Option<Hijacked> {
        None
    }
}
pub struct Hijacked {
    pub stream: TcpStream,
    pub buffered: Vec<u8>,
    #[cfg(feature = "tls")]
    pub tls: Option<Box<rustls::ServerConnection>>,
}
#[cfg(feature = "tls")]
type TlsSession = Arc<Mutex<Option<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>>>;
enum Stream {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(TlsSession, TcpStream),
}
impl Stream {
    fn socket(&self) -> &TcpStream {
        match self {
            Stream::Plain(sock) => sock,
            #[cfg(feature = "tls")]
            Stream::Tls(_, sock) => sock,
        }
    }
    fn try_clone(&self) -> io::Result<Stream> {
        Ok(match self {
            Stream::Plain(sock) => Stream::Plain(sock.try_clone()?),
            #[cfg(feature = "tls")]
            Stream::Tls(session, sock) => Stream::Tls(session.clone(), sock.try_clone()?),
        })
    }
    fn is_tls(&self) -> bool {
        !matches!(self, Stream::Plain(_))
    }
    fn into_hijacked(self) -> Option<Hijacked> {
        match self {
            Stream::Plain(stream) => Some(Hijacked {
                stream,
                buffered: Vec::new(),
                #[cfg(feature = "tls")]
                tls: None,
            }),
            #[cfg(feature = "tls")]
            Stream::Tls(session, stream) => {
                let owned = session.lock().ok()?.take()?;
                Some(Hijacked {
                    stream,
                    buffered: Vec::new(),
                    tls: Some(Box::new(owned.conn)),
                })
            }
        }
    }
}
impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(sock) => sock.read(buf),
            #[cfg(feature = "tls")]
            Stream::Tls(session, _) => {
                let mut guard = session
                    .lock()
                    .map_err(|_| io::Error::other("tls: session lock poisoned"))?;
                match guard.as_mut() {
                    None => Ok(0),
                    Some(stream) => match stream.read(buf) {
                        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(0),
                        other => other,
                    },
                }
            }
        }
    }
}
impl Write for Stream {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(sock) => sock.write(data),
            #[cfg(feature = "tls")]
            Stream::Tls(session, _) => {
                let mut guard = session
                    .lock()
                    .map_err(|_| io::Error::other("tls: session lock poisoned"))?;
                match guard.as_mut() {
                    None => Err(io::ErrorKind::NotConnected.into()),
                    Some(stream) => {
                        let n = stream.write(data)?;
                        stream.flush()?;
                        Ok(n)
                    }
                }
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Plain(sock) => sock.flush(),
            #[cfg(feature = "tls")]
            Stream::Tls(session, _) => {
                let mut guard = session
                    .lock()
                    .map_err(|_| io::Error::other("tls: session lock poisoned"))?;
                match guard.as_mut() {
                    None => Ok(()),
                    Some(stream) => stream.flush(),
                }
            }
        }
    }
}
pub type Handler = Arc<dyn Fn(&mut dyn ResponseWriter, &mut Request) + Send + Sync>;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HttpConfig {
    pub read_header_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_header_bytes: usize,
}
const DEFAULT_MAX_HEADER_BYTES: usize = 1 << 20;
const BUFFER_BEFORE_CHUNKING: usize = 2048;
pub fn http_error(w: &mut dyn ResponseWriter, message: &str, code: u16) {
    let h = w.header();
    h.del("Content-Length");
    h.set("Content-Type", "text/plain; charset=utf-8");
    h.set("X-Content-Type-Options", "nosniff");
    w.write_header(code);
    let _ = w.write(format!("{message}\n").as_bytes());
}
pub struct ServeHandle {
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    conns: Arc<Mutex<Vec<(u64, TcpStream)>>>,
    next_id: Arc<AtomicUsize>,
}
impl ServeHandle {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    pub fn active(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }
    pub fn close_connections(&self) {
        if let Ok(conns) = self.conns.lock() {
            for (_, c) in conns.iter() {
                let _ = c.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}
pub fn serve(
    listener: TcpListener,
    handler: Handler,
    config: HttpConfig,
    handle: &ServeHandle,
) -> io::Result<()> {
    serve_with(listener, handler, config, handle, Security::Plain)
}
#[cfg(feature = "tls")]
#[doc(hidden)]
pub fn serve_tls(
    listener: TcpListener,
    tls: Arc<rustls::ServerConfig>,
    handler: Handler,
    config: HttpConfig,
    handle: &ServeHandle,
) -> io::Result<()> {
    serve_with(listener, handler, config, handle, Security::Tls(tls, None))
}
#[cfg(feature = "tls")]
pub fn serve_tls_profile(
    listener: TcpListener,
    tls: &crate::net::ul::TlsServerConfig,
    handler: Handler,
    config: HttpConfig,
    handle: &ServeHandle,
) -> io::Result<()> {
    let security = Security::Tls(tls.rustls_config(), Some(Arc::new(tls.clone())));
    serve_with(listener, handler, config, handle, security)
}
#[derive(Clone)]
enum Security {
    Plain,
    #[cfg(feature = "tls")]
    Tls(
        Arc<rustls::ServerConfig>,
        Option<Arc<crate::net::ul::TlsServerConfig>>,
    ),
}
fn serve_with(
    listener: TcpListener,
    handler: Handler,
    config: HttpConfig,
    handle: &ServeHandle,
    security: Security,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let mut workers = Vec::new();
    loop {
        if handle.stop.load(Ordering::SeqCst) {
            break;
        }
        match listener.accept() {
            Ok((stream, addr)) => {
                let _ = stream.set_nonblocking(false);
                let handler = handler.clone();
                let active = handle.active.clone();
                let conns = handle.conns.clone();
                let id = handle.next_id.fetch_add(1, Ordering::SeqCst) as u64;
                if let (Ok(clone), Ok(mut list)) = (stream.try_clone(), conns.lock()) {
                    list.push((id, clone));
                }
                active.fetch_add(1, Ordering::SeqCst);
                let security = security.clone();
                workers.push(std::thread::spawn(move || {
                    match security {
                        Security::Plain => serve_connection(stream, addr, &handler, &config),
                        #[cfg(feature = "tls")]
                        Security::Tls(tls, profile) => {
                            serve_connection_tls_with(
                                stream,
                                addr,
                                &handler,
                                &config,
                                &tls,
                                profile.as_deref(),
                            );
                        }
                    }
                    if let Ok(mut list) = conns.lock() {
                        list.retain(|(c, _)| *c != id);
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                }));
                workers.retain(|w: &std::thread::JoinHandle<()>| !w.is_finished());
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
impl ServeHandle {
    pub fn new() -> ServeHandle {
        ServeHandle {
            stop: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicUsize::new(0)),
            conns: Arc::new(Mutex::new(Vec::new())),
            next_id: Arc::new(AtomicUsize::new(0)),
        }
    }
}
impl Default for ServeHandle {
    fn default() -> Self {
        ServeHandle::new()
    }
}
fn write_simple(stream: &mut Stream, status: u16, detail: &str) {
    let mut reason = status_text(status as i64).to_string();
    if !detail.is_empty() {
        reason = format!("{reason}: {detail}");
    }
    let text = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\n{status} {reason}"
    );
    let _ = stream.write_all(text.as_bytes());
}
fn read_head(reader: &mut BufReader<Stream>, max: usize) -> Result<Vec<u8>, u16> {
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => return Err(0),
            Ok(_) => {}
            Err(_) => return Err(0),
        }
        if head.is_empty() && (line == b"\r\n" || line == b"\n") {
            continue;
        }
        head.extend_from_slice(&line);
        if head.len() > max {
            return Err(431);
        }
        if line == b"\r\n" || line == b"\n" {
            return Ok(head);
        }
        if !line.ends_with(b"\n") {
            return Err(0);
        }
    }
}
fn header_timeout(config: &HttpConfig) -> Duration {
    if config.read_header_timeout.is_zero() {
        config.read_timeout
    } else {
        config.read_header_timeout
    }
}
pub(crate) fn serve_connection(
    stream: TcpStream,
    addr: SocketAddr,
    handler: &Handler,
    config: &HttpConfig,
) {
    let timeout = header_timeout(config);
    let _ = stream.set_read_timeout((!timeout.is_zero()).then_some(timeout));
    serve_stream(Stream::Plain(stream), addr, handler, config, None);
}
#[cfg(feature = "tls")]
fn looks_like_http(first: &[u8]) -> bool {
    [b"GET /", b"HEAD ", b"POST ", b"PUT /", b"OPTIO"]
        .iter()
        .any(|method| first == *method)
}
#[cfg(feature = "tls")]
fn peek_record_header(stream: &TcpStream, timeout: Duration) -> Vec<u8> {
    let deadline = (!timeout.is_zero()).then(|| Instant::now() + timeout);
    let mut first = [0u8; 5];
    let mut last = 0;
    loop {
        match stream.peek(&mut first) {
            Ok(5) => return first.to_vec(),
            Ok(0) => return Vec::new(),
            Ok(n) => {
                if n == last && deadline.is_some_and(|d| Instant::now() >= d) {
                    return first[..n].to_vec();
                }
                last = n;
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return first[..last].to_vec(),
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return first[..last].to_vec();
        }
    }
}
#[cfg(feature = "tls")]
#[doc(hidden)]
pub fn serve_connection_tls(
    stream: TcpStream,
    addr: SocketAddr,
    handler: &Handler,
    config: &HttpConfig,
    tls: &Arc<rustls::ServerConfig>,
) {
    serve_connection_tls_with(stream, addr, handler, config, tls, None);
}
#[cfg(feature = "tls")]
fn serve_connection_tls_with(
    mut stream: TcpStream,
    addr: SocketAddr,
    handler: &Handler,
    config: &HttpConfig,
    tls: &Arc<rustls::ServerConfig>,
    profile: Option<&crate::net::ul::TlsServerConfig>,
) {
    let timeout = header_timeout(config);
    let _ = stream.set_read_timeout((!timeout.is_zero()).then_some(timeout));
    let _ = stream.set_write_timeout((!timeout.is_zero()).then_some(timeout));
    if looks_like_http(&peek_record_header(&stream, timeout)) {
        let mut first = [0u8; 512];
        let _ = stream.read(&mut first);
        let _ = stream.write_all(
            b"HTTP/1.0 400 Bad Request\r\n\r\nClient sent an HTTP request to an HTTPS server.\n",
        );
        drop(stream);
        return;
    }
    let Ok(mut session) = rustls::ServerConnection::new(tls.clone()) else {
        return;
    };
    while session.is_handshaking() {
        if session.complete_io(&mut stream).is_err() {
            let _ = session.write_tls(&mut stream);
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return;
        }
    }
    let _ = stream.set_write_timeout(None);
    let Ok(sock) = stream.try_clone() else {
        return;
    };
    let presented = session.peer_certificates().unwrap_or_default();
    let peer = PeerTls {
        peer_certificates: presented.iter().map(|c| c.as_ref().to_vec()).collect(),
        verified_chains: profile.map_or_else(Vec::new, |p| p.verified_chains(presented)),
    };
    let session = Arc::new(Mutex::new(Some(rustls::StreamOwned::new(session, stream))));
    serve_stream(
        Stream::Tls(session, sock),
        addr,
        handler,
        config,
        Some(peer),
    );
}
fn serve_stream(
    stream: Stream,
    addr: SocketAddr,
    handler: &Handler,
    config: &HttpConfig,
    peer: Option<PeerTls>,
) {
    let tls = stream.is_tls();
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let max = if config.max_header_bytes == 0 {
        DEFAULT_MAX_HEADER_BYTES
    } else {
        config.max_header_bytes
    } + 4096;
    let head = match read_head(&mut reader, max) {
        Ok(h) => h,
        Err(431) => {
            write_simple(&mut out, 431, "");
            return;
        }
        Err(_) => return,
    };
    let mut request = match parse_head(&head, addr) {
        Ok(r) => r,
        Err(message) => {
            write_simple(&mut out, 400, &message);
            return;
        }
    };
    request.tls = tls;
    if let Some(peer) = peer {
        request.ctx = request.ctx.with_value(peer);
    }
    let _ = reader
        .get_ref()
        .socket()
        .set_read_timeout((!config.read_timeout.is_zero()).then_some(config.read_timeout));
    let _ = out
        .socket()
        .set_write_timeout((!config.write_timeout.is_zero()).then_some(config.write_timeout));
    let chunked = request
        .header
        .values("Transfer-Encoding")
        .iter()
        .any(|v| v.eq_ignore_ascii_case("chunked"));
    if chunked {
        request.header.del("Transfer-Encoding");
        request.content_length = -1;
        request.body = Box::new(ChunkedReader {
            inner: reader,
            remaining: 0,
            done: false,
        });
    } else {
        let length = request.header.get("Content-Length").trim().to_string();
        let n = if length.is_empty() {
            0
        } else {
            match length.parse::<u64>() {
                Ok(n) => n,
                Err(_) => {
                    write_simple(&mut out, 400, "");
                    return;
                }
            }
        };
        request.content_length = n as i64;
        request.body = Box::new(reader.take(n));
    }
    let cancel = CancelToken::new();
    request.set_context(request.context().with_cancel(&cancel));
    let mut writer = ConnWriter {
        stream: Some(out),
        header: Header::new(),
        snapshot: None,
        status: 0,
        committed: false,
        chunked: false,
        buffer: Vec::new(),
        aborted: false,
        broken: false,
        head_request: request.method == "HEAD",
        http10: request.proto == "HTTP/1.0",
        total: 0,
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handler(&mut writer, &mut request);
    }));
    cancel.cancel();
    if result.is_err() || writer.aborted {
        writer.abort_now();
        return;
    }
    writer.finish();
}
fn parse_head(head: &[u8], addr: SocketAddr) -> Result<Request, String> {
    let line_end = head.iter().position(|&c| c == b'\n').unwrap_or(head.len());
    let line = &head[..line_end];
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let line = std::str::from_utf8(line).map_err(|_| "".to_string())?;
    let mut parts = line.splitn(3, ' ');
    let (Some(method), Some(target), Some(proto)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err("".into());
    };
    if method.is_empty() || !method.bytes().all(is_token) {
        return Err("".into());
    }
    if proto != "HTTP/1.1" && proto != "HTTP/1.0" {
        return Err("".into());
    }
    let url = urlrules::parse_request_uri(target).map_err(|_| "".to_string())?;
    let mut header = read_mime_header(&head[line_end + 1..]).map_err(|_| "".to_string())?;
    let hosts = header.values("Host").to_vec();
    if hosts.len() > 1 {
        return Err(String::new());
    }
    if proto == "HTTP/1.1" && hosts.is_empty() {
        return Err("missing required Host header".into());
    }
    header.del("Host");
    let host = if url.host.is_empty() {
        hosts.first().cloned().unwrap_or_default()
    } else {
        url.host.clone()
    };
    Ok(Request {
        method: method.to_string(),
        url,
        request_uri: target.to_string(),
        proto: proto.to_string(),
        host,
        header,
        remote_addr: addr.to_string(),
        tls: false,
        content_length: 0,
        body: Box::new(io::empty()),
        ctx: Ctx::background(),
    })
}
fn is_token(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c)
}
struct ChunkedReader {
    inner: BufReader<Stream>,
    remaining: u64,
    done: bool,
}
impl Read for ChunkedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut line = String::new();
            self.inner.read_line(&mut line)?;
            let size = line.trim_end().split(';').next().unwrap_or("").trim();
            let n = u64::from_str_radix(size, 16).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "malformed chunked encoding")
            })?;
            if n == 0 {
                loop {
                    let mut trailer = String::new();
                    if self.inner.read_line(&mut trailer)? == 0 || trailer.trim_end().is_empty() {
                        break;
                    }
                }
                self.done = true;
                return Ok(0);
            }
            self.remaining = n;
        }
        let want = buf.len().min(self.remaining as usize);
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.remaining -= n as u64;
        if self.remaining == 0 {
            let mut crlf = [0u8; 2];
            self.inner.read_exact(&mut crlf)?;
        }
        Ok(n)
    }
}
struct ConnWriter {
    stream: Option<Stream>,
    header: Header,
    snapshot: Option<Header>,
    status: u16,
    committed: bool,
    chunked: bool,
    buffer: Vec<u8>,
    aborted: bool,
    broken: bool,
    head_request: bool,
    http10: bool,
    total: usize,
}
fn body_allowed(status: u16) -> bool {
    !((100..200).contains(&status) || status == 204 || status == 304)
}
impl ConnWriter {
    fn commit(&mut self, final_length: Option<usize>) {
        if self.committed {
            return;
        }
        self.committed = true;
        let status = if self.status == 0 { 200 } else { self.status };
        let mut header = self.snapshot.take().unwrap_or_else(|| self.header.clone());
        if header.get("Content-Type").is_empty()
            && header.values("Transfer-Encoding").is_empty()
            && body_allowed(status)
            && !self.buffer.is_empty()
        {
            let n = self.buffer.len().min(512);
            header.set("Content-Type", detect_content_type(&self.buffer[..n]));
        }
        header.del("Connection");
        header.del("Transfer-Encoding");
        match final_length {
            Some(n) if body_allowed(status) && header.get("Content-Length").is_empty() => {
                header.set("Content-Length", &n.to_string());
            }
            Some(_) => {}
            None => {
                if body_allowed(status) && header.get("Content-Length").parse::<u64>().is_err() {
                    header.del("Content-Length");
                    self.chunked = true;
                }
            }
        }
        let version = if self.http10 { "HTTP/1.0" } else { "HTTP/1.1" };
        let mut text = format!("{version} {status} {}\r\n", status_text(status as i64));
        for (key, values) in &header.0 {
            for v in values {
                text.push_str(&format!("{key}: {v}\r\n"));
            }
        }
        if self.chunked {
            text.push_str("Transfer-Encoding: chunked\r\n");
        }
        text.push_str("Connection: close\r\n\r\n");
        self.raw(text.as_bytes());
    }
    fn raw(&mut self, data: &[u8]) {
        if self.broken {
            return;
        }
        if let Some(s) = self.stream.as_mut()
            && s.write_all(data).is_err()
        {
            self.broken = true;
        }
    }
    fn emit(&mut self, data: &[u8]) {
        if data.is_empty() || self.head_request {
            return;
        }
        if self.chunked {
            let mut chunk = format!("{:x}\r\n", data.len()).into_bytes();
            chunk.extend_from_slice(data);
            chunk.extend_from_slice(b"\r\n");
            self.raw(&chunk);
        } else {
            self.raw(data);
        }
    }
    fn declared(&self) -> Option<usize> {
        let h = self.snapshot.as_ref().unwrap_or(&self.header);
        h.get("Content-Length").parse().ok()
    }
    fn finish(&mut self) {
        if let Some(declared) = self.declared()
            && self.total < declared
            && body_allowed(if self.status == 0 { 200 } else { self.status })
            && !self.head_request
        {
            if !self.committed {
                self.commit(None);
            }
            self.abort_now();
            return;
        }
        let status = if self.status == 0 { 200 } else { self.status };
        if !self.committed {
            let n = self.buffer.len();
            self.commit(Some(n));
            if body_allowed(status) {
                let data = std::mem::take(&mut self.buffer);
                self.emit(&data);
            }
        } else {
            let data = std::mem::take(&mut self.buffer);
            self.emit(&data);
            if self.chunked && !self.head_request {
                self.raw(b"0\r\n\r\n");
            }
        }
        if let Some(s) = self.stream.take() {
            let _ = s.flush_and_close();
        }
    }
    fn abort_now(&mut self) {
        if self.committed {
            let data = std::mem::take(&mut self.buffer);
            self.emit(&data);
        }
        if let Some(s) = self.stream.take() {
            let _ = s.flush_and_close();
        }
    }
}
trait CloseExt {
    fn flush_and_close(self) -> io::Result<()>;
}
impl CloseExt for Stream {
    fn flush_and_close(self) -> io::Result<()> {
        match self {
            Stream::Plain(sock) => sock.flush_and_close(),
            #[cfg(feature = "tls")]
            Stream::Tls(session, sock) => {
                if let Ok(mut guard) = session.lock()
                    && let Some(stream) = guard.as_mut()
                {
                    stream.conn.send_close_notify();
                    let _ = stream.flush();
                    while stream.conn.wants_write() {
                        if stream.conn.write_tls(&mut stream.sock).is_err() {
                            break;
                        }
                    }
                }
                sock.flush_and_close()
            }
        }
    }
}
impl CloseExt for TcpStream {
    fn flush_and_close(mut self) -> io::Result<()> {
        self.flush()?;
        let _ = self.shutdown(std::net::Shutdown::Write);
        let _ = self.set_read_timeout(Some(Duration::from_millis(200)));
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut sink = [0u8; 4096];
        while Instant::now() < deadline {
            match self.read(&mut sink) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        Ok(())
    }
}
impl ResponseWriter for ConnWriter {
    fn header(&mut self) -> &mut Header {
        &mut self.header
    }
    fn write_header(&mut self, status: u16) {
        if self.status != 0 {
            return;
        }
        self.status = status;
        self.snapshot = Some(self.header.clone());
    }
    fn write(&mut self, data: &[u8]) -> Result<usize, ServerError> {
        if self.broken {
            return Err(ServerError::Other("http: connection broken".into()));
        }
        if self.status == 0 {
            self.write_header(200);
        }
        if !body_allowed(self.status) {
            return Err(ServerError::Other(
                "http: request method or response status code does not allow body".into(),
            ));
        }
        if let Some(declared) = self.declared()
            && self.total + data.len() > declared
        {
            return Err(ServerError::Other(
                "http: wrote more than the declared Content-Length".into(),
            ));
        }
        self.total += data.len();
        self.buffer.extend_from_slice(data);
        if self.buffer.len() > BUFFER_BEFORE_CHUNKING {
            if !self.committed {
                self.commit(None);
            }
            let data = std::mem::take(&mut self.buffer);
            self.emit(&data);
        }
        if self.broken {
            return Err(ServerError::Other("http: connection broken".into()));
        }
        Ok(data.len())
    }
    fn flush(&mut self) {
        if self.status == 0 {
            self.write_header(200);
        }
        if !self.committed {
            self.commit(None);
        }
        let data = std::mem::take(&mut self.buffer);
        self.emit(&data);
    }
    fn abort(&mut self) {
        self.aborted = true;
    }
    fn hijack(&mut self) -> Option<Hijacked> {
        if self.committed {
            let data = std::mem::take(&mut self.buffer);
            self.emit(&data);
        }
        self.aborted = true;
        self.stream.take().and_then(Stream::into_hijacked)
    }
}
pub(crate) fn detect_content_type(data: &[u8]) -> &'static str {
    let ws = data
        .iter()
        .position(|c| !matches!(c, b'\t' | b'\n' | 0x0c | b'\r' | b' '))
        .unwrap_or(data.len());
    let trimmed = &data[ws..];
    let lower: Vec<u8> = trimmed
        .iter()
        .take(16)
        .map(u8::to_ascii_lowercase)
        .collect();
    for tag in [
        &b"<!doctype html"[..],
        b"<html",
        b"<head",
        b"<script",
        b"<iframe",
        b"<h1",
        b"<div",
        b"<font",
        b"<table",
        b"<a",
        b"<style",
        b"<title",
        b"<b",
        b"<body",
        b"<br",
        b"<p",
        b"<!--",
    ] {
        if lower.starts_with(tag) && matches!(trimmed.get(tag.len()), Some(b' ' | b'>')) {
            return "text/html; charset=utf-8";
        }
    }
    if trimmed.starts_with(b"<?xml") {
        return "text/xml; charset=utf-8";
    }
    if data.starts_with(b"%PDF-") {
        return "application/pdf";
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if data.starts_with(b"\xff\xd8\xff") {
        return "image/jpeg";
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return "image/gif";
    }
    if data.starts_with(b"\xfe\xff") {
        return "text/plain; charset=utf-16be";
    }
    if data.starts_with(b"\xff\xfe") {
        return "text/plain; charset=utf-16le";
    }
    if data.starts_with(b"\xef\xbb\xbf") {
        return "text/plain; charset=utf-8";
    }
    if data.iter().any(|&c| {
        c <= 0x08 || c == 0x0b || (0x0e..=0x1a).contains(&c) || (0x1c..=0x1f).contains(&c)
    }) {
        return "application/octet-stream";
    }
    "text/plain; charset=utf-8"
}
#[doc(hidden)]
pub fn canonical_key(key: &str) -> String {
    canonical_header_key(key)
}
