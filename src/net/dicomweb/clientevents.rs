use super::client::{Client, ClientError, status_error, web};
use super::clientups::WorkitemResponse;
use super::mimeheader::read_mime_header;
use super::multipart::ReadErr;
use super::response::Response;
use super::server::{WorkitemEvent, decode_workitem_event};
use super::types::{
    Cause, CauseClass, DEFAULT_TIMEOUT_NANOS, Error, ErrorKind, safe_http_status, safe_url,
};
use super::urlrules::Url;
use super::websocket::{
    CLOSE_NORMAL, Conn, OP_TEXT, WsError, accept_key, header_contains_token, new_client_key,
};
use crate::net::ul::{Ctx, Kind};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
pub const ERR_EVENT_CHANNEL_CLOSED: &str = "dicomweb: event channel hub closed";
#[doc(hidden)]
pub const ERR_EVENT_CHANNEL_TLS: &str = "dicomweb: TLS event channels are not supported";
const MAX_HANDSHAKE_HEADER_BYTES: usize = 1 << 20;
pub struct WorkitemEventChannel {
    conn: Arc<Conn>,
    events: Mutex<Receiver<WorkitemEvent>>,
    shared: Arc<Shared>,
    response: Response,
    reader: Mutex<Option<std::thread::JoinHandle<()>>>,
}
#[derive(Default)]
struct Shared {
    done: std::sync::atomic::AtomicBool,
    err: Mutex<Option<ClientError>>,
}
impl Shared {
    fn fail(&self, err: ClientError) {
        if let Ok(mut slot) = self.err.lock()
            && slot.is_none()
        {
            *slot = Some(err);
        }
    }
}
fn closed() -> ClientError {
    ClientError::Other(ERR_EVENT_CHANNEL_CLOSED.into())
}
fn request_error(raw_url: &str, text: String, timeout: bool) -> ClientError {
    let (kind, class) = if timeout {
        (ErrorKind::Timeout, CauseClass::Timeout)
    } else {
        (ErrorKind::RequestFailure, CauseClass::Other)
    };
    web(kind, raw_url, 0, Cause { text, class })
}
fn io_timeout(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}
impl WorkitemEventChannel {
    fn read_loop(conn: Arc<Conn>, sender: SyncSender<WorkitemEvent>, shared: Arc<Shared>) {
        loop {
            let (opcode, message) = match conn.read_message() {
                Ok(m) => m,
                Err(e) => {
                    let err = match e {
                        WsError::Closed | WsError::CloseFrame { .. } => closed(),
                        other => ClientError::Joined(vec![
                            closed(),
                            ClientError::Other(other.to_string()),
                        ]),
                    };
                    shared.fail(err);
                    return;
                }
            };
            if opcode != OP_TEXT {
                continue;
            }
            let Ok(event) = decode_workitem_event(&message) else {
                continue;
            };
            let mut pending = event;
            loop {
                match sender.try_send(pending) {
                    Ok(()) => break,
                    Err(TrySendError::Disconnected(_)) => return,
                    Err(TrySendError::Full(back)) => {
                        if shared.done.load(std::sync::atomic::Ordering::SeqCst) {
                            return;
                        }
                        pending = back;
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
            }
        }
    }
    pub fn response(&self) -> &Response {
        &self.response
    }
    fn close_error(&self) -> ClientError {
        self.shared
            .err
            .lock()
            .ok()
            .and_then(|e| e.clone())
            .unwrap_or_else(closed)
    }
    pub fn next(&self, ctx: &Ctx) -> Result<WorkitemEvent, ClientError> {
        let Ok(events) = self.events.lock() else {
            return Err(self.close_error());
        };
        loop {
            if let Some(kind) = ctx.err() {
                return Err(ClientError::Read(match kind {
                    Kind::Canceled => ReadErr::Canceled,
                    _ => ReadErr::Deadline,
                }));
            }
            let mut wait = Duration::from_millis(10);
            if let Some(d) = ctx.deadline() {
                wait = wait.min(d.saturating_duration_since(Instant::now()));
            }
            match events.recv_timeout(wait) {
                Ok(event) => return Ok(event),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err(self.close_error()),
            }
        }
    }
    pub fn close(&self) {
        self.shared
            .done
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.shared.fail(closed());
        self.conn.close(CLOSE_NORMAL, "");
        if let Ok(mut reader) = self.reader.lock()
            && let Some(handle) = reader.take()
        {
            let _ = handle.join();
        }
    }
}
impl Drop for WorkitemEventChannel {
    fn drop(&mut self) {
        self.close();
    }
}
impl Client {
    pub fn open_workitem_event_channel(
        &self,
        ctx: &Ctx,
        ae_title: &str,
    ) -> Result<WorkitemEventChannel, ClientError> {
        let u = self
            .endpoint
            .event_channel_url(ae_title)
            .map_err(ClientError::Web)?;
        self.open_workitem_event_channel_url(ctx, &u)
    }
    pub fn open_subscribed_workitem_event_channel(
        &self,
        ctx: &Ctx,
        ae_title: &str,
        response: &WorkitemResponse,
    ) -> Result<WorkitemEventChannel, ClientError> {
        let channels = self
            .endpoint
            .subscribed_event_channel_urls(ae_title, response)
            .map_err(ClientError::Web)?;
        let mut last = None;
        for target in &channels {
            match self.open_workitem_event_channel_url(ctx, target) {
                Ok(channel) => return Ok(channel),
                Err(e) => {
                    if ctx.err().is_some() {
                        return Err(e);
                    }
                    last = Some(e);
                }
            }
        }
        Err(last.unwrap_or_else(closed))
    }
    pub fn open_workitem_event_channel_url(
        &self,
        ctx: &Ctx,
        channel: &Url,
    ) -> Result<WorkitemEventChannel, ClientError> {
        let u = self
            .endpoint
            .event_channel_target(channel)
            .map_err(ClientError::Web)?;
        self.redirect_check(&u, false)?;
        let timeout = if self.options.timeout.0 > 0 {
            self.options.timeout.0
        } else {
            DEFAULT_TIMEOUT_NANOS
        };
        for _ in 0..2 {
            let mut handshake_deadline = Instant::now() + Duration::from_nanos(timeout as u64);
            if let Some(d) = ctx.deadline()
                && d < handshake_deadline
            {
                handshake_deadline = d;
            }
            let handshake_ctx = ctx.with_deadline(handshake_deadline);
            match self.open_event_channel(&handshake_ctx, &u) {
                Ok(channel) => return Ok(channel),
                Err((false, e)) => return Err(e),
                Err((true, _)) => {}
            }
        }
        Err(ClientError::Web(Error::new(
            ErrorKind::AuthStatus,
            &u.string(),
            401,
            None,
        )))
    }
    fn open_event_channel(
        &self,
        ctx: &Ctx,
        u: &Url,
    ) -> Result<WorkitemEventChannel, (bool, ClientError)> {
        let raw_url = u.string();
        let deadline = ctx.deadline();
        let stream = dial(u, deadline)
            .map_err(|(text, timeout)| (false, request_error(&raw_url, text, timeout)))?;
        let mut wire = if u.scheme == "https" {
            self.secure_wire(stream, u, ctx)
                .map_err(|(text, timeout)| (false, request_error(&raw_url, text, timeout)))?
        } else {
            Wire::Plain(stream)
        };
        let (headers, token) = match self.request_headers(ctx, u, "", "") {
            Ok(found) => found,
            Err(e) => {
                wire.close();
                return Err((false, e));
            }
        };
        let source_auth = token.is_some();
        let remaining = deadline.map(|d| {
            d.saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1))
        });
        let _ = wire.socket().set_read_timeout(remaining);
        let _ = wire.socket().set_write_timeout(remaining);
        let mut max_message = self.options.max_body_bytes;
        if max_message <= 0 || max_message > 64 << 20 {
            max_message = 64 << 20;
        }
        let key = new_client_key();
        let mut head = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: dicom-rust\r\n",
            u.request_uri(),
            u.host
        );
        for (name, value) in &headers {
            if name != "Accept" {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
        }
        head.push_str(&format!(
            "Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\nUpgrade: websocket\r\n\r\n"
        ));
        let fail = |e: std::io::Error| {
            (
                false,
                request_error(&raw_url, e.to_string(), io_timeout(&e)),
            )
        };
        let (status_code, header, buffered) = match wire.exchange(&head) {
            Ok(response) => response,
            Err(e) => {
                wire.close();
                return Err(match e {
                    HeadError::Io(e) => fail(e),
                    HeadError::Malformed(text) => (false, request_error(&raw_url, text, false)),
                });
            }
        };
        let response = Response {
            url: safe_url(&raw_url),
            status_code,
            status: safe_http_status(status_code),
            header,
            body: Vec::new(),
        };
        if status_code != 101 {
            wire.close();
            if status_code == 401
                && source_auth
                && let (Some(source), Some(token)) = (&self.options.bearer_token_source, &token)
                && source.can_invalidate()
            {
                source.invalidate(token);
                return Err((true, status_error(&response)));
            }
            return Err((false, status_error(&response)));
        }
        let h = &response.header;
        if !header_contains_token(h, "Upgrade", "websocket")
            || !header_contains_token(h, "Connection", "upgrade")
            || h.get("Sec-WebSocket-Accept") != accept_key(&key)
            || !h.get("Sec-WebSocket-Extensions").is_empty()
            || !h.get("Sec-WebSocket-Protocol").is_empty()
        {
            wire.close();
            return Err((
                false,
                web(
                    ErrorKind::DecodeResponse,
                    &response.url,
                    101,
                    Cause::text(WsError::Handshake.to_string()),
                ),
            ));
        }
        let _ = wire.socket().set_read_timeout(None);
        let _ = wire.socket().set_write_timeout(None);
        let conn = wire.into_conn(buffered, max_message).map_err(fail)?;
        let (sender, receiver) = mpsc::sync_channel(64);
        let shared = Arc::new(Shared::default());
        let reader_conn = conn.clone();
        let reader_shared = shared.clone();
        let handle = std::thread::spawn(move || {
            WorkitemEventChannel::read_loop(reader_conn, sender, reader_shared)
        });
        Ok(WorkitemEventChannel {
            conn,
            events: Mutex::new(receiver),
            shared,
            response,
            reader: Mutex::new(Some(handle)),
        })
    }
}
impl Client {
    #[cfg(feature = "tls")]
    fn secure_wire(&self, stream: TcpStream, u: &Url, ctx: &Ctx) -> Result<Wire, (String, bool)> {
        use super::http::TransportError;
        match super::http::secure_event_channel(
            stream,
            u,
            self.options.https.as_ref(),
            self.options.https_identity.as_ref(),
            ctx,
        ) {
            Ok(secure) => Ok(Wire::Tls(Box::new(secure))),
            Err(TransportError::Timeout) => Err(("context deadline exceeded".into(), true)),
            Err(TransportError::Canceled) => Err(("context canceled".into(), false)),
            Err(_) => Err(("tls: handshake failure".into(), false)),
        }
    }
    #[cfg(not(feature = "tls"))]
    fn secure_wire(
        &self,
        _stream: TcpStream,
        _u: &Url,
        _ctx: &Ctx,
    ) -> Result<Wire, (String, bool)> {
        Err((ERR_EVENT_CHANNEL_TLS.into(), false))
    }
}
enum Wire {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}
impl Wire {
    fn socket(&self) -> &TcpStream {
        match self {
            Wire::Plain(stream) => stream,
            #[cfg(feature = "tls")]
            Wire::Tls(secure) => &secure.sock,
        }
    }
    fn exchange(
        &mut self,
        head: &str,
    ) -> Result<(i64, super::mimeheader::Header, Vec<u8>), HeadError> {
        match self {
            Wire::Plain(stream) => {
                let mut stream: &TcpStream = stream;
                exchange_head(&mut stream, head)
            }
            #[cfg(feature = "tls")]
            Wire::Tls(secure) => exchange_head(secure.as_mut(), head),
        }
    }
    fn close(&mut self) {
        #[cfg(feature = "tls")]
        if let Wire::Tls(secure) = self {
            secure.conn.send_close_notify();
            while secure.conn.wants_write() {
                if secure.conn.write_tls(&mut secure.sock).is_err() {
                    break;
                }
            }
        }
        let _ = self.socket().shutdown(std::net::Shutdown::Both);
    }
    fn into_conn(self, buffered: Vec<u8>, max_message: i64) -> std::io::Result<Arc<Conn>> {
        match self {
            Wire::Plain(stream) => Conn::new(stream, buffered, true, max_message),
            #[cfg(feature = "tls")]
            Wire::Tls(secure) => {
                let rustls::StreamOwned { conn, sock } = *secure;
                Conn::new_tls(sock, conn.into(), buffered, true, max_message)
            }
        }
    }
}
fn exchange_head<S: Read + Write>(
    stream: &mut S,
    head: &str,
) -> Result<(i64, super::mimeheader::Header, Vec<u8>), HeadError> {
    stream.write_all(head.as_bytes()).map_err(HeadError::Io)?;
    let mut reader = BufReader::new(stream);
    let (status_code, header) = read_response_head(&mut reader)?;
    Ok((status_code, header, reader.buffer().to_vec()))
}
fn dial(u: &Url, deadline: Option<Instant>) -> Result<TcpStream, (String, bool)> {
    let host = u.hostname();
    let port: u16 = match u.port() {
        "" if u.scheme == "https" => 443,
        "" => 80,
        p => p
            .parse()
            .map_err(|_| (format!("dial tcp: address {p}: invalid port"), false))?,
    };
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|e| (format!("dial tcp: {e}"), false))?;
    let mut last = (format!("dial tcp: lookup {host}: no such host"), false);
    for address in addresses {
        let wait = match deadline {
            Some(d) => {
                let left = d.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(("dial tcp: i/o timeout".into(), true));
                }
                left
            }
            None => Duration::from_secs(30),
        };
        match TcpStream::connect_timeout(&address, wait) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = (format!("dial tcp {address}: {e}"), io_timeout(&e)),
        }
    }
    Err(last)
}
enum HeadError {
    Io(std::io::Error),
    Malformed(String),
}
fn read_response_head<R: Read>(
    reader: &mut BufReader<R>,
) -> Result<(i64, super::mimeheader::Header), HeadError> {
    let mut total = 0usize;
    let mut read_line = |reader: &mut BufReader<R>| -> Result<Vec<u8>, HeadError> {
        let mut line = Vec::new();
        let n = reader
            .by_ref()
            .take((MAX_HANDSHAKE_HEADER_BYTES - total.min(MAX_HANDSHAKE_HEADER_BYTES)) as u64 + 1)
            .read_until(b'\n', &mut line)
            .map_err(HeadError::Io)?;
        total += n;
        if !line.ends_with(b"\n") {
            return Err(HeadError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                if n == 0 { "EOF" } else { "unexpected EOF" },
            )));
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        Ok(line)
    };
    let status_line = String::from_utf8_lossy(&read_line(reader)?).into_owned();
    let (proto, rest) = status_line.split_once(' ').ok_or_else(|| {
        HeadError::Malformed(format!(
            "malformed HTTP response {}",
            crate::textfmt::quote(&status_line)
        ))
    })?;
    if !proto.starts_with("HTTP/") {
        return Err(HeadError::Malformed(format!(
            "malformed HTTP version {}",
            crate::textfmt::quote(proto)
        )));
    }
    let code = rest.split(' ').next().unwrap_or("");
    let status_code = match code.parse::<i64>() {
        Ok(c) if code.len() == 3 && c >= 0 => c,
        _ => {
            return Err(HeadError::Malformed(format!(
                "malformed HTTP status code {}",
                crate::textfmt::quote(code)
            )));
        }
    };
    let mut raw = Vec::new();
    loop {
        let line = read_line(reader)?;
        raw.extend_from_slice(&line);
        raw.extend_from_slice(b"\r\n");
        if line.is_empty() {
            break;
        }
    }
    let header = read_mime_header(&raw).map_err(HeadError::Malformed)?;
    Ok((status_code, header))
}
