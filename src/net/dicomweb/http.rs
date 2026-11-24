use super::mimeheader::{Header, read_mime_header};
use super::multipart::{ByteRead, ReadErr, SliceReader};
use super::types::TransportCause;
use super::urlrules::{self, Url};
use crate::net::ul::Ctx;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, Instant};
const MAX_RESPONSE_HEADER_BYTES: usize = 10 << 20;
const POLL: Duration = Duration::from_millis(50);
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TransportError {
    Timeout,
    Canceled,
    ConnectionBroken,
    Redirect {
        location: String,
        limit: bool,
    },
    After {
        method: String,
        url: String,
        inner: Box<TransportError>,
    },
    Other,
    Failed(Box<TransportCause>),
}
fn failed(cause: TransportCause) -> TransportError {
    TransportError::Failed(Box::new(cause))
}
fn io_cause(e: &std::io::Error) -> TransportCause {
    #[cfg(feature = "tls")]
    if let Some(tls) = e
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        let (class, early) = crate::net::ul::tls_failure_of(tls.clone());
        return TransportCause {
            tls: Some(class),
            tls_certificate_not_yet_valid: early,
            ..TransportCause::default()
        };
    }
    TransportCause {
        os_error: e.raw_os_error(),
        ..TransportCause::default()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RedirectCheck {
    Default,
    SameOrigin,
    NoFollow,
    SameOriginNoFollow,
}
fn ctx_error(ctx: &Ctx) -> Option<ReadErr> {
    use crate::net::ul::Kind;
    match ctx.err() {
        Some(Kind::Canceled) => Some(ReadErr::Canceled),
        Some(_) => Some(ReadErr::Deadline),
        None => None,
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpsOptions {
    pub root_certificates_der: Vec<Vec<u8>>,
    pub system_roots: bool,
    pub insecure_skip_verify: bool,
}
#[derive(Clone, Default, PartialEq, Eq)]
pub struct HttpsIdentity {
    pub server_name: String,
    pub client_certificate_chain_der: Vec<Vec<u8>>,
    pub client_private_key_der: Vec<u8>,
}
impl std::fmt::Debug for HttpsIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpsIdentity")
            .field("server_name", &self.server_name)
            .field(
                "client_certificates",
                &self.client_certificate_chain_der.len(),
            )
            .field(
                "client_private_key",
                &!self.client_private_key_der.is_empty(),
            )
            .finish()
    }
}
enum Stream {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}
impl Stream {
    fn socket(&self) -> &TcpStream {
        match self {
            Stream::Plain(stream) => stream,
            #[cfg(feature = "tls")]
            Stream::Tls(stream) => &stream.sock,
        }
    }
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(stream) => stream.read(buf),
            #[cfg(feature = "tls")]
            Stream::Tls(stream) => match stream.read(buf) {
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(0),
                other => other,
            },
        }
    }
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(stream) => stream.write(data),
            #[cfg(feature = "tls")]
            Stream::Tls(stream) => stream.write(data),
        }
    }
}
#[cfg(feature = "tls")]
mod tls {
    use super::{HttpsIdentity, HttpsOptions};
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::CryptoProvider;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use std::sync::Arc;
    #[derive(Debug)]
    struct NoVerification(Arc<CryptoProvider>);
    impl ServerCertVerifier for NoVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }
        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &rustls::DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
    fn provider_of() -> CryptoProvider {
        rustls::crypto::ring::default_provider()
    }
    pub(super) fn session(
        options: Option<&HttpsOptions>,
        identity: Option<&HttpsIdentity>,
        host: &str,
    ) -> Option<rustls::ClientConnection> {
        session_with_alpn(options, identity, host, &[])
    }
    pub(super) fn session_with_alpn(
        options: Option<&HttpsOptions>,
        identity: Option<&HttpsIdentity>,
        host: &str,
        alpn: &[&[u8]],
    ) -> Option<rustls::ClientConnection> {
        let default = HttpsOptions::default();
        let options = options.unwrap_or(&default);
        let host = match identity {
            Some(identity) if !identity.server_name.is_empty() => identity.server_name.as_str(),
            _ => host,
        };
        let host = host
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
            .filter(|inner| inner.parse::<std::net::IpAddr>().is_ok())
            .unwrap_or(host);
        let client = match identity {
            Some(identity) if !identity.client_certificate_chain_der.is_empty() => {
                let chain = identity
                    .client_certificate_chain_der
                    .iter()
                    .map(|der| CertificateDer::from(der.clone()))
                    .collect();
                let key = rustls::pki_types::PrivateKeyDer::try_from(
                    identity.client_private_key_der.clone(),
                )
                .ok()?;
                Some(crate::net::ul::client_certificate_resolver(chain, key, &provider_of()).ok()?)
            }
            _ => None,
        };
        let provider = Arc::new(provider_of());
        let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .ok()?;
        let builder = if options.insecure_skip_verify {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerification(provider)))
        } else {
            let roots: Vec<CertificateDer<'static>> =
                if options.system_roots && options.root_certificates_der.is_empty() {
                    crate::net::ul::system_root_certificates().ok()?
                } else {
                    options
                        .root_certificates_der
                        .iter()
                        .map(|der| CertificateDer::from(der.clone()))
                        .collect()
                };
            builder.dangerous().with_custom_certificate_verifier(
                crate::net::ul::https_server_verifier(roots, host, &provider),
            )
        };
        let mut config = match client {
            Some(resolver) => builder.with_client_cert_resolver(resolver),
            None => builder.with_no_client_auth(),
        };
        config.alpn_protocols = alpn.iter().map(|protocol| protocol.to_vec()).collect();
        let name = match ServerName::try_from(host.to_string()) {
            Ok(name) => name,
            Err(_) => {
                config.enable_sni = false;
                ServerName::IpAddress(std::net::IpAddr::from([0, 0, 0, 0]).into())
            }
        };
        rustls::ClientConnection::new(Arc::new(config), name).ok()
    }
}
#[derive(Clone, Default)]
pub struct HttpClientTrace {
    pub wrote_request: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    pub got_first_response_byte: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl std::fmt::Debug for HttpClientTrace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClientTrace")
            .field("wrote_request", &self.wrote_request.is_some())
            .field(
                "got_first_response_byte",
                &self.got_first_response_byte.is_some(),
            )
            .finish()
    }
}
impl HttpClientTrace {
    fn wrote(trace: Option<&HttpClientTrace>, failed: bool) {
        if let Some(report) = trace.and_then(|t| t.wrote_request.as_ref()) {
            report(failed);
        }
    }
}
#[derive(Clone)]
pub struct ConnectionPool {
    inner: Arc<std::sync::Mutex<PoolState>>,
}
struct PoolState {
    idle: Vec<(String, Stream, Instant)>,
    max_idle_per_host: usize,
    idle_timeout: Duration,
}
impl Default for ConnectionPool {
    fn default() -> Self {
        ConnectionPool::new()
    }
}
impl std::fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionPool")
            .field("idle", &self.idle_connections())
            .finish()
    }
}
impl ConnectionPool {
    pub fn new() -> ConnectionPool {
        ConnectionPool::with_limits(2, Duration::from_secs(90))
    }
    pub fn with_limits(max_idle_per_host: usize, idle_timeout: Duration) -> ConnectionPool {
        ConnectionPool {
            inner: Arc::new(std::sync::Mutex::new(PoolState {
                idle: Vec::new(),
                max_idle_per_host,
                idle_timeout,
            })),
        }
    }
    fn state(&self) -> std::sync::MutexGuard<'_, PoolState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(crate) fn idle_connections(&self) -> usize {
        self.state().idle.len()
    }
    pub fn close_idle_connections(&self) {
        self.state().idle.clear();
    }
    fn take(&self, key: &str) -> Option<Stream> {
        let mut state = self.state();
        let timeout = state.idle_timeout;
        state.idle.retain(|(_, _, since)| since.elapsed() < timeout);
        while let Some(i) = state.idle.iter().rposition(|(k, _, _)| k == key) {
            let (_, stream, _) = state.idle.remove(i);
            if idle_stream_open(&stream) {
                return Some(stream);
            }
        }
        None
    }
    fn put(&self, key: String, stream: Stream) {
        let mut state = self.state();
        if state.idle_timeout.is_zero() {
            return;
        }
        let held = state.idle.iter().filter(|(k, _, _)| *k == key).count();
        if held >= state.max_idle_per_host {
            if let Some(i) = state.idle.iter().position(|(k, _, _)| *k == key) {
                state.idle.remove(i);
            } else {
                return;
            }
        }
        state.idle.push((key, stream, Instant::now()));
    }
}
fn idle_stream_open(stream: &Stream) -> bool {
    let socket = stream.socket();
    if socket.set_nonblocking(true).is_err() {
        return false;
    }
    let mut probe = [0u8; 1];
    let open = matches!(
        socket.peek(&mut probe),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
    );
    socket.set_nonblocking(false).is_ok() && open
}
fn pool_key(url: &Url) -> String {
    format!(
        "{}://{}:{}",
        url.scheme.to_ascii_lowercase(),
        url.hostname().to_ascii_lowercase(),
        effective_port(url)
    )
}
struct Wire {
    stream: Stream,
    buf: Vec<u8>,
    pos: usize,
    ctx: Ctx,
    failure: Option<TransportCause>,
    received: bool,
    first_byte: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Wire {
    fn fill(&mut self) -> Result<usize, ReadErr> {
        if self.pos > 0 && self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
        }
        let mut chunk = [0u8; 16 << 10];
        loop {
            if let Some(e) = ctx_error(&self.ctx) {
                return Err(e);
            }
            let wait = match self.ctx.deadline() {
                Some(d) => d
                    .saturating_duration_since(Instant::now())
                    .clamp(Duration::from_millis(1), POLL),
                None => POLL,
            };
            let _ = self.stream.socket().set_read_timeout(Some(wait));
            match self.stream.read(&mut chunk) {
                Ok(n) => {
                    if n > 0 && !self.received {
                        self.received = true;
                        if let Some(report) = self.first_byte.take() {
                            report();
                        }
                    }
                    self.buf.extend_from_slice(&chunk[..n]);
                    return Ok(n);
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => {
                    self.failure = Some(io_cause(&e));
                    return Err(ReadErr::Net {
                        timeout: false,
                        text: e.to_string(),
                    });
                }
            }
        }
    }
    fn available(&self) -> &[u8] {
        &self.buf[self.pos..]
    }
    fn read_line(&mut self, limit: usize) -> Result<Vec<u8>, ReadErr> {
        loop {
            if let Some(i) = self.available().iter().position(|&c| c == b'\n') {
                let mut line = self.buf[self.pos..self.pos + i].to_vec();
                self.pos += i + 1;
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(line);
            }
            if self.available().len() > limit || self.fill()? == 0 {
                return Err(ReadErr::UnexpectedEof);
            }
        }
    }
    fn read_some(&mut self, out: &mut [u8]) -> Result<usize, ReadErr> {
        if self.available().is_empty() && self.fill()? == 0 {
            return Ok(0);
        }
        let n = out.len().min(self.available().len());
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
enum Framing {
    None,
    Length(u64),
    Chunked { remaining: u64, started: bool },
    Close,
}
pub(crate) struct Body {
    wire: Option<Wire>,
    framing: Framing,
    keep: Option<(ConnectionPool, String)>,
}
impl Body {
    pub(crate) fn empty() -> Body {
        Body {
            wire: None,
            framing: Framing::None,
            keep: None,
        }
    }
    fn release(&mut self) {
        if let (Some(wire), Some((pool, key))) = (self.wire.take(), self.keep.take())
            && wire.pos == wire.buf.len()
        {
            pool.put(key, wire.stream);
        }
    }
    fn step(&mut self, out: &mut [u8]) -> Result<usize, ReadErr> {
        let Some(wire) = self.wire.as_mut() else {
            return Err(ReadErr::Eof);
        };
        match &mut self.framing {
            Framing::None => Err(ReadErr::Eof),
            Framing::Close => match wire.read_some(out)? {
                0 => Err(ReadErr::Eof),
                n => Ok(n),
            },
            Framing::Length(remaining) => {
                if *remaining == 0 {
                    return Err(ReadErr::Eof);
                }
                let want = out
                    .len()
                    .min(usize::try_from(*remaining).unwrap_or(usize::MAX));
                match wire.read_some(&mut out[..want])? {
                    0 => Err(ReadErr::UnexpectedEof),
                    n => {
                        *remaining -= n as u64;
                        Ok(n)
                    }
                }
            }
            Framing::Chunked { remaining, started } => {
                if *remaining == 0 {
                    if *started {
                        let mut end = [0u8; 2];
                        let mut got = 0;
                        while got < 2 {
                            match wire.read_some(&mut end[got..])? {
                                0 => return Err(ReadErr::UnexpectedEof),
                                n => got += n,
                            }
                        }
                        if &end != b"\r\n" {
                            return Err(ReadErr::Other("malformed chunked encoding".into()));
                        }
                    }
                    *started = true;
                    let size = chunk_size(wire)?;
                    if size == 0 {
                        loop {
                            match wire.read_line(4096) {
                                Ok(line) if line.is_empty() => break,
                                Ok(_) => {}
                                Err(ReadErr::UnexpectedEof) => {
                                    return Err(ReadErr::Other(
                                        "http: unexpected EOF reading trailer".into(),
                                    ));
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        self.framing = Framing::None;
                        return Err(ReadErr::Eof);
                    }
                    *remaining = size;
                }
                let want = out
                    .len()
                    .min(usize::try_from(*remaining).unwrap_or(usize::MAX));
                match wire.read_some(&mut out[..want])? {
                    0 => Err(ReadErr::UnexpectedEof),
                    n => {
                        *remaining -= n as u64;
                        Ok(n)
                    }
                }
            }
        }
    }
}
fn chunk_size(wire: &mut Wire) -> Result<u64, ReadErr> {
    const MAX_LINE: usize = 4096;
    let other = |text: &str| ReadErr::Other(text.to_string());
    let line = loop {
        if let Some(i) = wire.available().iter().position(|&c| c == b'\n') {
            let line = wire.buf[wire.pos..wire.pos + i + 1].to_vec();
            wire.pos += i + 1;
            break line;
        }
        if wire.available().len() >= MAX_LINE {
            return Err(other("header line too long"));
        }
        if wire.fill()? == 0 {
            return Err(ReadErr::UnexpectedEof);
        }
    };
    match line.iter().position(|&c| c == b'\r') {
        None => return Err(other("chunked line ends with bare LF")),
        Some(i) if i != line.len() - 2 => return Err(other("invalid CR in chunked line")),
        Some(_) => {}
    }
    let mut line = &line[..line.len() - 2];
    if line.len() >= MAX_LINE {
        return Err(other("header line too long"));
    }
    while let Some((&last, rest)) = line.split_last() {
        if last != b' ' && last != b'\t' {
            break;
        }
        line = rest;
    }
    if let Some(i) = line.iter().position(|&c| c == b';') {
        line = &line[..i];
    }
    if line.is_empty() {
        return Err(other("empty hex number for chunk length"));
    }
    let mut size: u64 = 0;
    for (i, &b) in line.iter().enumerate() {
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(other("invalid byte in chunk length")),
        };
        if i == 16 {
            return Err(other("http chunk length too large"));
        }
        size = (size << 4) | u64::from(digit);
    }
    Ok(size)
}
impl ByteRead for Body {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        if buf.is_empty() {
            return (0, None);
        }
        match self.step(buf) {
            Ok(n) => (n, None),
            Err(e) => {
                if matches!(e, ReadErr::Eof) && self.keep.is_some() {
                    self.release();
                } else {
                    self.keep = None;
                }
                (0, Some(e))
            }
        }
    }
}
pub(crate) struct HttpResponse {
    pub status_code: i64,
    pub status: String,
    pub header: Header,
    pub body: Body,
    pub request_url: Url,
    pub content_length: i64,
}
pub(crate) struct HttpRequest<'a> {
    pub method: &'a str,
    pub url: &'a Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<&'a mut dyn ByteRead>,
    pub https: Option<&'a HttpsOptions>,
    pub identity: Option<&'a HttpsIdentity>,
}
fn transport(err: ReadErr) -> TransportError {
    match err {
        ReadErr::Deadline | ReadErr::Net { timeout: true, .. } => TransportError::Timeout,
        ReadErr::Canceled => TransportError::Canceled,
        _ => TransportError::Other,
    }
}
fn connect(
    url: &Url,
    https: Option<&HttpsOptions>,
    identity: Option<&HttpsIdentity>,
    ctx: &Ctx,
) -> Result<Stream, TransportError> {
    let secure = url.scheme == "https";
    if url.scheme != "http" && !secure {
        return Err(TransportError::Other);
    }
    let stream = connect_tcp(url, if secure { 443 } else { 80 }, ctx)?;
    if !secure {
        return Ok(Stream::Plain(stream));
    }
    secure_stream(stream, url, https, identity, ctx)
}
#[cfg(not(feature = "tls"))]
fn secure_stream(
    _stream: TcpStream,
    _url: &Url,
    _https: Option<&HttpsOptions>,
    _identity: Option<&HttpsIdentity>,
    _ctx: &Ctx,
) -> Result<Stream, TransportError> {
    Err(TransportError::Other)
}
#[cfg(feature = "tls")]
fn secure_stream(
    stream: TcpStream,
    url: &Url,
    https: Option<&HttpsOptions>,
    identity: Option<&HttpsIdentity>,
    ctx: &Ctx,
) -> Result<Stream, TransportError> {
    let Some(session) = tls::session(https, identity, url.hostname()) else {
        return Err(TransportError::Other);
    };
    Ok(Stream::Tls(Box::new(handshake(session, stream, ctx)?)))
}
#[cfg(feature = "tls")]
pub(super) fn secure_event_channel(
    stream: TcpStream,
    url: &Url,
    https: Option<&HttpsOptions>,
    identity: Option<&HttpsIdentity>,
    ctx: &Ctx,
) -> Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>, TransportError> {
    let Some(session) = tls::session_with_alpn(https, identity, url.hostname(), &[b"http/1.1"])
    else {
        return Err(TransportError::Other);
    };
    handshake(session, stream, ctx)
}
#[cfg(feature = "tls")]
#[derive(Default)]
struct Records {
    first: Vec<u8>,
    header: usize,
    length_high: u8,
    body: usize,
}
#[cfg(feature = "tls")]
impl Records {
    fn feed(&mut self, data: &[u8]) {
        for &byte in data {
            if self.first.len() < 5 {
                self.first.push(byte);
            }
            if self.body > 0 {
                self.body -= 1;
                continue;
            }
            match self.header {
                3 => {
                    self.length_high = byte;
                    self.header = 4;
                }
                4 => {
                    self.body = usize::from(self.length_high) << 8 | usize::from(byte);
                    self.header = 0;
                }
                seen => self.header = seen + 1,
            }
        }
    }
    fn at_boundary(&self) -> bool {
        self.header == 0 && self.body == 0
    }
    fn first_is_tls(&self) -> bool {
        matches!(self.first[0], 21 | 22) && self.first[1] < 0x10
    }
}
#[cfg(feature = "tls")]
struct Tap<'a> {
    stream: &'a mut TcpStream,
    records: &'a mut Records,
}
#[cfg(feature = "tls")]
impl Read for Tap<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.stream.read(buf)?;
        self.records.feed(&buf[..n]);
        Ok(n)
    }
}
#[cfg(feature = "tls")]
impl Write for Tap<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.stream.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}
#[cfg(feature = "tls")]
fn handshake_cause(e: &std::io::Error, records: &Records, ended: bool) -> TransportCause {
    use crate::net::ul::TlsFailureClass;
    let mut cause = TransportCause::default();
    if records.first.len() >= 5 && !records.first_is_tls() {
        cause.tls = Some(TlsFailureClass::NotTls);
        cause.scheme_mismatch = records.first == b"HTTP/";
        return cause;
    }
    if ended || e.kind() == std::io::ErrorKind::UnexpectedEof {
        cause.eof = records.at_boundary();
        cause.unexpected_eof = !cause.eof;
        return cause;
    }
    io_cause(e)
}
#[cfg(feature = "tls")]
fn handshake(
    mut session: rustls::ClientConnection,
    mut stream: TcpStream,
    ctx: &Ctx,
) -> Result<rustls::StreamOwned<rustls::ClientConnection, TcpStream>, TransportError> {
    crate::net::ul::reset_handshake_state();
    let mut records = Records::default();
    while session.is_handshaking() {
        if let Some(e) = ctx_error(ctx) {
            return Err(transport(e));
        }
        let _ = stream.set_read_timeout(Some(POLL));
        let _ = stream.set_write_timeout(Some(POLL));
        let mut tap = Tap {
            stream: &mut stream,
            records: &mut records,
        };
        match session.complete_io(&mut tap) {
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(e) => {
                let _ = session.write_tls(&mut stream);
                let mut ended = false;
                while !records.first.is_empty() && records.first.len() < 5 && !ended {
                    if let Some(e) = ctx_error(ctx) {
                        return Err(transport(e));
                    }
                    let mut byte = [0u8; 1];
                    match stream.read(&mut byte) {
                        Ok(0) => ended = true,
                        Ok(_) => records.feed(&byte),
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock
                                    | std::io::ErrorKind::TimedOut
                                    | std::io::ErrorKind::Interrupted
                            ) => {}
                        Err(e) => return Err(failed(io_cause(&e))),
                    }
                }
                return Err(failed(handshake_cause(&e, &records, ended)));
            }
        }
    }
    Ok(rustls::StreamOwned::new(session, stream))
}
fn connect_tcp(url: &Url, default_port: u16, ctx: &Ctx) -> Result<TcpStream, TransportError> {
    let host = url.hostname();
    let port: u16 = match url.port() {
        "" => default_port,
        port => port.parse().map_err(|_| TransportError::Other)?,
    };
    let addresses = (host, port).to_socket_addrs().map_err(|_| {
        failed(TransportCause {
            dial: true,
            name_lookup: true,
            ..TransportCause::default()
        })
    })?;
    let dialer = crate::net::ul::SourcePortDialer {
        timeout: Some(Duration::from_secs(30)),
        ..Default::default()
    };
    let mut last = TransportError::Other;
    for address in addresses {
        if let Some(e) = ctx_error(ctx) {
            return Err(transport(e));
        }
        match dialer.dial(ctx, &address.to_string()) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => last = TransportError::Timeout,
            Err(e) => {
                last = failed(TransportCause {
                    dial: true,
                    ..io_cause(&e)
                })
            }
        }
    }
    Err(last)
}
fn write_all(stream: &mut Stream, ctx: &Ctx, data: &[u8]) -> Result<(), ReadErr> {
    let mut sent = 0;
    while sent < data.len() {
        if let Some(e) = ctx_error(ctx) {
            return Err(e);
        }
        let _ = stream.socket().set_write_timeout(Some(POLL));
        let _ = stream.socket().set_read_timeout(Some(POLL));
        match stream.write(&data[sent..]) {
            Ok(0) => return Err(ReadErr::UnexpectedEof),
            Ok(n) => sent += n,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(e) => {
                return Err(ReadErr::Net {
                    timeout: false,
                    text: e.to_string(),
                });
            }
        }
    }
    Ok(())
}
enum Exchange {
    Failed(TransportError),
    Stale,
}
impl From<TransportError> for Exchange {
    fn from(e: TransportError) -> Exchange {
        Exchange::Failed(e)
    }
}
pub(crate) fn round_trip(
    request: &mut HttpRequest,
    ctx: &Ctx,
) -> Result<HttpResponse, TransportError> {
    if let Some(e) = ctx_error(ctx) {
        return Err(transport(e));
    }
    let pool = ctx.value::<ConnectionPool>().cloned();
    if let Some(pool) = &pool
        && let Some(stream) = pool.take(&pool_key(request.url))
    {
        match exchange(request, ctx, stream, Some(pool), true) {
            Ok(response) => return Ok(response),
            Err(Exchange::Failed(e)) => return Err(e),
            Err(Exchange::Stale) => {
                if let Some(e) = ctx_error(ctx) {
                    return Err(transport(e));
                }
            }
        }
    }
    let stream = connect(request.url, request.https, request.identity, ctx)?;
    match exchange(request, ctx, stream, pool.as_ref(), false) {
        Ok(response) => Ok(response),
        Err(Exchange::Failed(e)) => Err(e),
        Err(Exchange::Stale) => Err(TransportError::Other),
    }
}
fn exchange(
    request: &mut HttpRequest,
    ctx: &Ctx,
    mut stream: Stream,
    pool: Option<&ConnectionPool>,
    reused: bool,
) -> Result<HttpResponse, Exchange> {
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: dicom-rust\r\n",
        request.method,
        request.url.request_uri(),
        request.url.host
    );
    for (name, value) in &request.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(user) = &request.url.user
        && !request
            .headers
            .iter()
            .any(|(name, value)| name == "Authorization" && !value.is_empty())
    {
        let credentials = format!("{}:{}", user.username, user.password);
        head.push_str(&format!(
            "Authorization: Basic {}\r\n",
            crate::dicomjson::sizedparse::base64_encode(credentials.as_bytes())
        ));
    }
    if request.body.is_some() {
        head.push_str("Transfer-Encoding: chunked\r\n");
    }
    if pool.is_none() {
        head.push_str("Connection: close\r\n");
    }
    head.push_str("\r\n");
    let trace = ctx.value::<HttpClientTrace>().cloned();
    let mut written = write_all(&mut stream, ctx, head.as_bytes());
    if reused && matches!(written, Err(ReadErr::Net { .. } | ReadErr::UnexpectedEof)) {
        return Err(Exchange::Stale);
    }
    let replayable = request.body.is_none();
    if written.is_ok()
        && let Some(body) = request.body.as_mut()
    {
        let mut chunk = vec![0u8; 32 << 10];
        loop {
            let (n, err) = body.read(&mut chunk);
            if n > 0 {
                let mut frame = format!("{n:x}\r\n").into_bytes();
                frame.extend_from_slice(&chunk[..n]);
                frame.extend_from_slice(b"\r\n");
                written = write_all(&mut stream, ctx, &frame);
                if written.is_err() {
                    break;
                }
            }
            match err {
                None => {}
                Some(ReadErr::Eof) => {
                    written = write_all(&mut stream, ctx, b"0\r\n\r\n");
                    break;
                }
                Some(_) => {
                    HttpClientTrace::wrote(trace.as_ref(), true);
                    return Err(TransportError::Other.into());
                }
            }
        }
    }
    HttpClientTrace::wrote(trace.as_ref(), written.is_err());
    if let Err(e @ (ReadErr::Deadline | ReadErr::Canceled)) = &written {
        return Err(transport(e.clone()).into());
    }
    let mut wire = Wire {
        stream,
        buf: Vec::new(),
        pos: 0,
        ctx: ctx.clone(),
        failure: None,
        received: false,
        first_byte: trace.and_then(|t| t.got_first_response_byte),
    };
    loop {
        let broken = |e: ReadErr| match e {
            ReadErr::UnexpectedEof | ReadErr::Net { .. } => TransportError::ConnectionBroken,
            other => transport(other),
        };
        let status_line = match wire.read_line(MAX_RESPONSE_HEADER_BYTES) {
            Ok(line) => line,
            Err(ReadErr::UnexpectedEof | ReadErr::Net { .. })
                if !wire.received && reused && replayable =>
            {
                return Err(Exchange::Stale);
            }
            Err(ReadErr::UnexpectedEof) if !wire.received => {
                return Err(failed(TransportCause {
                    eof: true,
                    ..TransportCause::default()
                })
                .into());
            }
            Err(ReadErr::Net { .. }) if !wire.received => {
                return Err(failed(wire.failure.take().unwrap_or_default()).into());
            }
            Err(e) => return Err(broken(e).into()),
        };
        let status_line = String::from_utf8_lossy(&status_line).into_owned();
        let (version, status) = status_line
            .split_once(' ')
            .map_or((status_line.as_str(), ""), |(version, rest)| {
                (version, rest.trim_start_matches(' '))
            });
        let code = status.split(' ').next().unwrap_or("");
        let status = status.to_owned();
        if !version.starts_with("HTTP/") || code.len() != 3 {
            return Err(TransportError::ConnectionBroken.into());
        }
        let Ok(status_code) = code.parse::<i64>() else {
            return Err(TransportError::ConnectionBroken.into());
        };
        let mut raw: Vec<u8> = Vec::new();
        loop {
            let line = wire.read_line(MAX_RESPONSE_HEADER_BYTES).map_err(broken)?;
            raw.extend_from_slice(&line);
            raw.extend_from_slice(b"\r\n");
            if line.is_empty() {
                break;
            }
            if raw.len() > MAX_RESPONSE_HEADER_BYTES {
                return Err(TransportError::Other.into());
            }
        }
        let header = read_mime_header(&raw).map_err(|_| TransportError::ConnectionBroken)?;
        if (100..200).contains(&status_code) && status_code != 101 {
            continue;
        }
        let chunked = header
            .values("Transfer-Encoding")
            .iter()
            .any(|v| v.to_ascii_lowercase().contains("chunked"));
        let lengths = header.values("Content-Length");
        let length = match lengths.first() {
            None => None,
            Some(first) => {
                let first = trim_text(first);
                if lengths.iter().any(|l| trim_text(l) != first) {
                    return Err(TransportError::ConnectionBroken.into());
                }
                Some(first)
            }
        };
        let framing = if status_code == 204 || status_code == 304 || request.method == "HEAD" {
            Framing::None
        } else if chunked {
            Framing::Chunked {
                remaining: 0,
                started: false,
            }
        } else if let Some(length) = length {
            let digits = length.bytes().all(|b| b.is_ascii_digit());
            match length.parse::<u64>() {
                Ok(n) if digits && n <= i64::MAX as u64 => {
                    if n == 0 {
                        Framing::None
                    } else {
                        Framing::Length(n)
                    }
                }
                _ => return Err(TransportError::ConnectionBroken.into()),
            }
        } else {
            Framing::Close
        };
        let mut header = header;
        let closes = header.values("Connection").iter().any(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("close"))
        });
        if closes && !version.starts_with("HTTP/1.0") {
            header.del("Connection");
        }
        if !header.values("Transfer-Encoding").is_empty() {
            header.del("Transfer-Encoding");
            if chunked {
                header.del("Content-Length");
            }
        }
        let keep = pool
            .filter(|_| {
                !closes && !version.starts_with("HTTP/1.0") && !matches!(framing, Framing::Close)
            })
            .map(|p| (p.clone(), pool_key(request.url)));
        let content_length = match &framing {
            Framing::Length(n) => *n as i64,
            Framing::None => 0,
            Framing::Chunked { .. } | Framing::Close => -1,
        };
        let mut body = Body {
            wire: Some(wire),
            framing,
            keep,
        };
        if matches!(body.framing, Framing::None) && body.keep.is_some() {
            body.release();
        }
        return Ok(HttpResponse {
            status_code,
            status,
            header,
            content_length,
            body,
            request_url: request.url.clone(),
        });
    }
}
fn trim_text(value: &str) -> &str {
    value.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
}
fn effective_port(url: &Url) -> &str {
    match url.port() {
        "" => match url.scheme.to_ascii_lowercase().as_str() {
            "https" => "443",
            "http" => "80",
            _ => "",
        },
        port => port,
    }
}
pub(crate) fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme.eq_ignore_ascii_case(&right.scheme)
        && left.hostname().eq_ignore_ascii_case(right.hostname())
        && effective_port(left) == effective_port(right)
}
fn is_domain_or_subdomain(sub: &str, parent: &str) -> bool {
    if sub == parent {
        return true;
    }
    if sub.contains(':') || sub.contains('%') || !sub.ends_with(parent) {
        return false;
    }
    sub.as_bytes()[sub.len() - parent.len() - 1] == b'.'
}
const MAX_REDIRECTS: usize = 10;
pub(crate) fn send(
    request: HttpRequest,
    check: RedirectCheck,
    ctx: &Ctx,
) -> Result<HttpResponse, TransportError> {
    send_limited(request, check, MAX_REDIRECTS, None, ctx)
}
pub(crate) fn send_limited(
    mut request: HttpRequest,
    check: RedirectCheck,
    max_redirects: usize,
    replay: Option<&[u8]>,
    ctx: &Ctx,
) -> Result<HttpResponse, TransportError> {
    let initial_url = request.url.clone();
    let initial_headers = request.headers.clone();
    let had_body = request.body.is_some();
    let https = request.https;
    let identity = request.identity;
    let mut response = round_trip(&mut request, ctx)?;
    let mut method = request.method.to_string();
    let mut current = initial_url.clone();
    let mut strip_sensitive = false;
    let mut requests = 1;
    loop {
        let (redirect_method, include_body) = match response.status_code {
            301..=303 => {
                let next = if method == "GET" || method == "HEAD" {
                    method.clone()
                } else {
                    "GET".to_string()
                };
                (next, false)
            }
            307 | 308 => {
                if had_body && replay.is_none() {
                    return Ok(response);
                }
                (method.clone(), true)
            }
            _ => return Ok(response),
        };
        let location = response.header.get("Location").to_string();
        if location.is_empty() {
            return Ok(response);
        }
        let Ok(reference) = urlrules::parse(&location) else {
            return Err(TransportError::Other);
        };
        let target = current.resolve_reference(&reference);
        if !strip_sensitive
            && initial_url.host != target.host
            && !is_domain_or_subdomain(
                &target.hostname().to_ascii_lowercase(),
                &initial_url.hostname().to_ascii_lowercase(),
            )
        {
            strip_sensitive = true;
        }
        let refused = |limit: bool| TransportError::Redirect {
            location: location.clone(),
            limit,
        };
        if requests >= max_redirects {
            return Err(refused(true));
        }
        if matches!(
            check,
            RedirectCheck::SameOrigin | RedirectCheck::SameOriginNoFollow
        ) && (!same_origin(&initial_url, &target)
            || target.user.is_some()
            || !target.opaque.is_empty())
        {
            return Err(refused(false));
        }
        if matches!(
            check,
            RedirectCheck::NoFollow | RedirectCheck::SameOriginNoFollow
        ) {
            return Ok(response);
        }
        let headers: Vec<(String, String)> = initial_headers
            .iter()
            .filter(|(name, _)| {
                let sensitive = matches!(
                    name.as_str(),
                    "Authorization"
                        | "Www-Authenticate"
                        | "Cookie"
                        | "Cookie2"
                        | "Proxy-Authorization"
                        | "Proxy-Authenticate"
                );
                let body = matches!(
                    name.as_str(),
                    "Content-Encoding" | "Content-Language" | "Content-Location" | "Content-Type"
                );
                !(sensitive && strip_sensitive) && !(body && !include_body)
            })
            .cloned()
            .collect();
        method = redirect_method;
        current = target;
        requests += 1;
        let mut replayed = replay.filter(|_| include_body).map(SliceReader::new);
        let mut next = HttpRequest {
            method: &method,
            url: &current,
            headers,
            body: replayed.as_mut().map(|r| r as &mut dyn ByteRead),
            https,
            identity,
        };
        response = round_trip(&mut next, ctx).map_err(|inner| TransportError::After {
            method: method.clone(),
            url: current.string(),
            inner: Box::new(inner),
        })?;
    }
}
pub(crate) enum CheckedError<E> {
    Transport(TransportError),
    Refused(E),
}
#[allow(clippy::type_complexity)]
pub(crate) fn send_with_check<E>(
    request: HttpRequest,
    check: &mut dyn FnMut(&Url, usize, &mut Vec<(String, String)>) -> Result<(), E>,
    ctx: &Ctx,
) -> Result<(HttpResponse, Vec<(String, String)>), CheckedError<E>> {
    let initial_url = request.url.clone();
    let initial_headers = request.headers.clone();
    let https = request.https;
    let identity = request.identity;
    let method = request.method.to_string();
    let mut first = request;
    let mut response = round_trip(&mut first, ctx).map_err(CheckedError::Transport)?;
    let mut sent_headers = initial_headers.clone();
    let mut current = initial_url.clone();
    let mut requests = 1;
    loop {
        if !matches!(response.status_code, 301..=303 | 307 | 308) {
            return Ok((response, sent_headers));
        }
        let location = response.header.get("Location").to_string();
        if location.is_empty() {
            return Ok((response, sent_headers));
        }
        let Ok(reference) = urlrules::parse(&location) else {
            return Err(CheckedError::Transport(TransportError::Other));
        };
        let target = current.resolve_reference(&reference);
        let strip = initial_url.host != target.host
            && !is_domain_or_subdomain(
                &target.hostname().to_ascii_lowercase(),
                &initial_url.hostname().to_ascii_lowercase(),
            );
        let mut headers: Vec<(String, String)> = initial_headers
            .iter()
            .filter(|(name, _)| {
                !(strip
                    && matches!(
                        name.as_str(),
                        "Authorization" | "Www-Authenticate" | "Cookie" | "Cookie2"
                    ))
            })
            .cloned()
            .collect();
        check(&target, requests, &mut headers).map_err(CheckedError::Refused)?;
        current = target;
        requests += 1;
        let mut next = HttpRequest {
            method: &method,
            url: &current,
            headers: headers.clone(),
            body: None,
            https,
            identity,
        };
        response = round_trip(&mut next, ctx).map_err(|inner| {
            CheckedError::Transport(TransportError::After {
                method: method.clone(),
                url: current.string(),
                inner: Box::new(inner),
            })
        })?;
        sent_headers = headers;
    }
}
#[cfg(all(test, feature = "tls"))]
mod system_roots_tests {
    use super::*;
    #[test]
    fn builds_a_session_with_the_system_roots() {
        let system = HttpsOptions {
            system_roots: true,
            ..Default::default()
        };
        assert!(tls::session(Some(&system), None, "pacs.example.org").is_some());
        let explicit = HttpsOptions {
            root_certificates_der: vec![vec![0x30, 0x00]],
            system_roots: true,
            ..Default::default()
        };
        assert!(tls::session(Some(&explicit), None, "pacs.example.org").is_some());
        assert!(tls::session(None, None, "pacs.example.org").is_some());
        let named = HttpsIdentity {
            server_name: "pacs.example.org".into(),
            ..Default::default()
        };
        assert!(tls::session(None, Some(&named), "127.0.0.1").is_some());
        let broken = HttpsIdentity {
            client_certificate_chain_der: vec![vec![0x30, 0x00]],
            client_private_key_der: vec![0x30, 0x00],
            ..Default::default()
        };
        assert!(tls::session(None, Some(&broken), "pacs.example.org").is_none());
        assert!(!format!("{broken:?}").contains("48"));
    }
}
#[cfg(test)]
mod pool_tests {
    use super::*;
    use std::net::TcpListener;
    fn serve(
        listener: TcpListener,
        total: usize,
        answers: usize,
    ) -> std::thread::JoinHandle<usize> {
        std::thread::spawn(move || {
            let mut served = 0;
            let mut accepted = 0;
            while served < total {
                let Ok((mut socket, _)) = listener.accept() else {
                    break;
                };
                accepted += 1;
                let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));
                let mut pending = Vec::new();
                for _ in 0..answers {
                    while !pending.windows(4).any(|w| w == b"\r\n\r\n") {
                        let mut chunk = [0u8; 1024];
                        match socket.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => pending.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let Some(end) = pending.windows(4).position(|w| w == b"\r\n\r\n") else {
                        break;
                    };
                    let head = String::from_utf8_lossy(&pending[..end]).into_owned();
                    pending.drain(..end + 4);
                    let body = if head.contains("Connection: close") {
                        "closed"
                    } else {
                        "kept"
                    };
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    if socket.write_all(answer.as_bytes()).is_err() {
                        break;
                    }
                    served += 1;
                    if served == total {
                        break;
                    }
                }
            }
            accepted
        })
    }
    fn get(url: &Url, ctx: &Ctx) -> String {
        let request = HttpRequest {
            method: "GET",
            url,
            headers: Vec::new(),
            body: None,
            https: None,
            identity: None,
        };
        let mut response = match send(request, RedirectCheck::Default, ctx) {
            Ok(r) => r,
            Err(e) => return format!("error {e:?}"),
        };
        let (body, _) = super::super::multipart::read_all(&mut response.body);
        String::from_utf8_lossy(&body).into_owned()
    }
    fn local() -> Option<(TcpListener, Url)> {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        let url = urlrules::parse(&format!("http://127.0.0.1:{port}/x")).ok()?;
        Some((listener, url))
    }
    #[test]
    fn pooled_requests_share_one_connection() {
        let Some((listener, url)) = local() else {
            return;
        };
        let server = serve(listener, 3, 3);
        let pool = ConnectionPool::new();
        let ctx = Ctx::background().with_value(pool.clone());
        for _ in 0..3 {
            assert_eq!(get(&url, &ctx), "kept");
        }
        assert_eq!(pool.idle_connections(), 1);
        pool.close_idle_connections();
        assert_eq!(server.join().unwrap_or(0), 1);
    }
    #[test]
    fn a_closed_idle_connection_is_replaced() {
        let Some((listener, url)) = local() else {
            return;
        };
        let server = serve(listener, 2, 1);
        let pool = ConnectionPool::new();
        let ctx = Ctx::background().with_value(pool.clone());
        assert_eq!(get(&url, &ctx), "kept");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(get(&url, &ctx), "kept");
        pool.close_idle_connections();
        assert_eq!(server.join().unwrap_or(0), 2);
    }
    #[test]
    fn without_a_pool_every_request_closes() {
        let Some((listener, url)) = local() else {
            return;
        };
        let server = serve(listener, 2, 1);
        let ctx = Ctx::background();
        assert_eq!(get(&url, &ctx), "closed");
        assert_eq!(get(&url, &ctx), "closed");
        assert_eq!(server.join().unwrap_or(0), 2);
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GetError {
    Timeout,
    Canceled,
    ConnectionBroken,
    Failed(TransportCause),
    Other,
}
impl std::fmt::Display for GetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            GetError::Timeout => "HTTP GET timed out",
            GetError::Canceled => "context canceled",
            GetError::ConnectionBroken => "HTTP GET: connection broken before the response head",
            GetError::Failed(_) => "HTTP GET failed below HTTP",
            GetError::Other => "HTTP GET failed",
        })
    }
}
impl std::error::Error for GetError {}
impl From<TransportError> for GetError {
    fn from(e: TransportError) -> GetError {
        match e {
            TransportError::Timeout => GetError::Timeout,
            TransportError::Canceled => GetError::Canceled,
            TransportError::ConnectionBroken => GetError::ConnectionBroken,
            TransportError::Failed(cause) => GetError::Failed(*cause),
            TransportError::After { inner, .. } => GetError::from(*inner),
            TransportError::Redirect { .. } | TransportError::Other => GetError::Other,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct GetOptions {
    pub https: HttpsOptions,
    pub identity: Option<HttpsIdentity>,
}
pub struct GetResponse {
    pub status_code: i64,
    pub status: String,
    pub header: Header,
    pub content_length: i64,
    pub body: ResponseBody,
}
impl std::fmt::Debug for GetResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GetResponse")
            .field("status", &self.status)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}
pub struct ResponseBody(Body);
impl ByteRead for ResponseBody {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        self.0.read(buf)
    }
}
impl Read for ResponseBody {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        use std::io::{Error, ErrorKind};
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            match ByteRead::read(&mut self.0, buf) {
                (n, _) if n > 0 => return Ok(n),
                (_, None) => continue,
                (_, Some(ReadErr::Eof)) => return Ok(0),
                (_, Some(e)) => {
                    let kind = match &e {
                        ReadErr::UnexpectedEof => ErrorKind::UnexpectedEof,
                        ReadErr::Deadline | ReadErr::Net { timeout: true, .. } => {
                            ErrorKind::TimedOut
                        }
                        ReadErr::Canceled => ErrorKind::Interrupted,
                        _ => ErrorKind::Other,
                    };
                    return Err(Error::new(kind, e.to_string()));
                }
            }
        }
    }
}
pub fn http_get(
    ctx: &Ctx,
    url: &Url,
    headers: &[(String, String)],
    options: &GetOptions,
) -> Result<GetResponse, GetError> {
    let mut request = HttpRequest {
        method: "GET",
        url,
        headers: headers.to_vec(),
        body: None,
        https: Some(&options.https),
        identity: options.identity.as_ref(),
    };
    let response = round_trip(&mut request, ctx)?;
    Ok(GetResponse {
        status_code: response.status_code,
        status: response.status,
        header: response.header,
        content_length: response.content_length,
        body: ResponseBody(response.body),
    })
}
#[cfg(test)]
mod get_tests {
    use super::*;
    use std::net::TcpListener;
    fn serve_once(answer: &'static [u8]) -> (String, std::thread::JoinHandle<String>) {
        let Ok(listener) = TcpListener::bind("127.0.0.1:0") else {
            unreachable!("loopback listener")
        };
        let port = listener.local_addr().map_or(0, |a| a.port());
        let handle = std::thread::spawn(move || {
            let Ok((mut socket, _)) = listener.accept() else {
                return String::new();
            };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                match socket.read(&mut byte) {
                    Ok(1) => head.push(byte[0]),
                    _ => break,
                }
            }
            let _ = socket.write_all(answer);
            String::from_utf8_lossy(&head).into_owned()
        });
        (format!("http://user:pw@127.0.0.1:{port}/x?y=1"), handle)
    }
    #[test]
    fn get_streams_the_body() {
        let (url, server) =
            serve_once(b"HTTP/1.1  404 Not  Found\r\nContent-Length: 5\r\nX-A: b\r\n\r\nhello");
        let Ok(url) = urlrules::parse(&url) else {
            unreachable!("valid URL")
        };
        let headers = vec![("Accept".to_owned(), "application/dicom".to_owned())];
        let got = http_get(&Ctx::background(), &url, &headers, &GetOptions::default());
        let Ok(mut response) = got else {
            unreachable!("response {got:?}")
        };
        assert_eq!(response.status_code, 404);
        assert_eq!(response.status, "404 Not  Found");
        assert_eq!(response.content_length, 5);
        assert_eq!(response.header.get("X-A"), "b");
        let mut body = String::new();
        assert!(response.body.read_to_string(&mut body).is_ok());
        assert_eq!(body, "hello");
        let head = server.join().unwrap_or_default();
        assert!(head.starts_with("GET /x?y=1 HTTP/1.1\r\n"), "{head}");
        assert!(head.contains("Accept: application/dicom\r\n"), "{head}");
        assert!(
            head.contains("Authorization: Basic dXNlcjpwdw==\r\n"),
            "{head}"
        );
    }
    #[test]
    fn get_short_body_is_unexpected_eof() {
        let (url, server) = serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nabc");
        let Ok(url) = urlrules::parse(&url) else {
            unreachable!("valid URL")
        };
        let got = http_get(&Ctx::background(), &url, &[], &GetOptions::default());
        let Ok(mut response) = got else {
            unreachable!("response {got:?}")
        };
        let _ = server.join();
        let mut body = Vec::new();
        let err = response.body.read_to_end(&mut body);
        assert_eq!(body, b"abc");
        assert_eq!(
            err.map_err(|e| (e.kind(), e.to_string())),
            Err((
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF".to_owned()
            ))
        );
    }
    #[test]
    fn get_refused_is_a_dial_failure() {
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map_or(1, |a| a.port());
        let Ok(url) = urlrules::parse(&format!("http://127.0.0.1:{port}/")) else {
            unreachable!("valid URL")
        };
        match http_get(&Ctx::background(), &url, &[], &GetOptions::default()) {
            Err(GetError::Failed(cause)) => {
                assert!(cause.dial);
                assert!(cause.os_error.is_some());
            }
            other => unreachable!("{other:?}"),
        }
    }
}
