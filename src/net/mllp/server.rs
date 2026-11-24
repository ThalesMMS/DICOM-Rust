use super::stream::Conn;
use super::{
    Error, IoWriter, MLLP1, MLLP2, Reader, Release, Sentinel, io_error, release_valid,
    transport_phase_error, validate_frame_payload, write_confirmation, write_frame,
};
use crate::hl7::{
    self, AckOptions, ERROR_CODE_APPLICATION_INTERNAL_ERROR, ERROR_CODE_DATA_TYPE,
    ERROR_CODE_REQUIRED_FIELD_MISSING, ERROR_CODE_TABLE_VALUE_NOT_FOUND, FieldExt, Message,
    ParseOptions,
};
use std::net::{Shutdown as NetShutdown, SocketAddr, TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
pub struct Request {
    pub message: Message,
    pub remote_addr: Option<SocketAddr>,
    #[cfg(feature = "tls")]
    pub tls: Option<crate::net::ul::TlsConnectionState>,
}
#[cfg(feature = "tls")]
type ServerTls = Option<Arc<rustls::ServerConfig>>;
#[cfg(not(feature = "tls"))]
type ServerTls = Option<std::convert::Infallible>;
#[cfg(feature = "tls")]
type State = Option<crate::net::ul::TlsConnectionState>;
#[cfg(not(feature = "tls"))]
type State = ();
pub type Handler = Arc<dyn Fn(&Request) -> Option<Message> + Send + Sync>;
pub type OriginalAckHandler = Arc<dyn Fn(&Request, &[u8]) -> Result<Vec<u8>, Error> + Send + Sync>;
pub type OnError = Arc<dyn Fn(Option<SocketAddr>, &Error) + Send + Sync>;
#[derive(Clone, Default)]
pub struct ServerOptions {
    pub release: Release,
    pub transport_ack_timeout: Duration,
    pub handler: Option<Handler>,
    pub original_ack_handler: Option<OriginalAckHandler>,
    pub max_frame_bytes: i64,
    pub parse_options: ParseOptions,
    pub idle_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub max_connections: usize,
    pub on_error: Option<OnError>,
}
impl ServerOptions {
    fn with_defaults(mut self) -> ServerOptions {
        let secs = |d: Duration, s: u64| {
            if d.is_zero() {
                Duration::from_secs(s)
            } else {
                d
            }
        };
        self.transport_ack_timeout = secs(self.transport_ack_timeout, 30);
        if self.max_frame_bytes <= 0 {
            self.max_frame_bytes = super::DEFAULT_MAX_FRAME_BYTES;
        }
        if self.parse_options.max_bytes <= 0 || self.parse_options.max_bytes > self.max_frame_bytes
        {
            self.parse_options.max_bytes = self.max_frame_bytes;
        }
        self.idle_timeout = secs(self.idle_timeout, 300);
        self.read_timeout = secs(self.read_timeout, 30);
        self.write_timeout = secs(self.write_timeout, 30);
        if self.max_connections == 0 {
            self.max_connections = 64;
        }
        self
    }
}
#[derive(Default)]
pub struct Shutdown {
    flag: AtomicBool,
    conns: Mutex<Vec<TcpStream>>,
}
impl Shutdown {
    pub fn new() -> Shutdown {
        Shutdown::default()
    }
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let conns = self.conns.lock().unwrap_or_else(|e| e.into_inner());
        for c in conns.iter() {
            let _ = c.shutdown(NetShutdown::Both);
        }
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    fn track(&self, conn: &TcpStream) {
        if let Ok(c) = conn.try_clone() {
            let mut conns = self.conns.lock().unwrap_or_else(|e| e.into_inner());
            conns.push(c);
        }
        if self.is_cancelled() {
            let _ = conn.shutdown(NetShutdown::Both);
        }
    }
}
fn report(options: &ServerOptions, remote: Option<SocketAddr>, err: &Error) {
    if let Some(f) = &options.on_error {
        f(remote, err);
    }
}
pub fn serve(
    listener: TcpListener,
    options: ServerOptions,
    shutdown: Arc<Shutdown>,
) -> Result<(), Error> {
    serve_with(listener, options, shutdown, None)
}
#[cfg(feature = "tls")]
#[doc(hidden)]
pub fn serve_tls(
    listener: TcpListener,
    options: ServerOptions,
    shutdown: Arc<Shutdown>,
    tls: Arc<rustls::ServerConfig>,
) -> Result<(), Error> {
    serve_with(listener, options, shutdown, Some(tls))
}
#[cfg_attr(not(feature = "tls"), allow(clippy::clone_on_copy, clippy::unit_arg))]
fn serve_with(
    listener: TcpListener,
    options: ServerOptions,
    shutdown: Arc<Shutdown>,
    tls: ServerTls,
) -> Result<(), Error> {
    if !release_valid(options.release) {
        return Err(Error::Sentinel(Sentinel::UnsupportedRelease));
    }
    if options.handler.is_none() == options.original_ack_handler.is_none() {
        return Err(Error::Text(
            "mllp: exactly one server handler is required".into(),
        ));
    }
    if options.original_ack_handler.is_some() && options.release != MLLP1 {
        return Err(Error::Sentinel(Sentinel::UnsupportedRelease));
    }
    let options = Arc::new(options.with_defaults());
    listener.set_nonblocking(true).map_err(io_error)?;
    let active = Arc::new(AtomicUsize::new(0));
    let mut threads = Vec::new();
    let result = loop {
        if shutdown.is_cancelled() {
            break Ok(());
        }
        let conn = match listener.accept() {
            Ok((conn, _)) => conn,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if shutdown.is_cancelled() {
                    break Ok(());
                }
                break Err(io_error(e));
            }
        };
        let _ = conn.set_nonblocking(false);
        if active.fetch_add(1, Ordering::SeqCst) >= options.max_connections {
            active.fetch_sub(1, Ordering::SeqCst);
            report(
                &options,
                conn.peer_addr().ok(),
                &Error::Text("mllp: connection limit reached".into()),
            );
            continue;
        }
        shutdown.track(&conn);
        let (options, shutdown, active) = (options.clone(), shutdown.clone(), active.clone());
        let tls = tls.clone();
        threads.push(std::thread::spawn(move || {
            serve_conn(conn, &options, &shutdown, &tls);
            active.fetch_sub(1, Ordering::SeqCst);
        }));
        threads.retain(|t| !t.is_finished());
    };
    shutdown.cancel();
    for t in threads {
        let _ = t.join();
    }
    result
}
fn timeout(d: Duration) -> Option<Duration> {
    Some(d.max(Duration::from_millis(1)))
}
fn open(conn: TcpStream, options: &ServerOptions, tls: &ServerTls) -> Option<(Conn, State)> {
    #[cfg(feature = "tls")]
    if let Some(config) = tls {
        let remote = conn.peer_addr().ok();
        let raw = conn.try_clone().ok()?;
        if let Err(e) = conn
            .set_read_timeout(timeout(options.read_timeout))
            .and_then(|_| conn.set_write_timeout(timeout(options.read_timeout)))
        {
            report(options, remote, &io_error(e));
            return None;
        }
        return match super::stream::server_handshake(conn, config) {
            Ok((conn, state)) => Some((conn, Some(state))),
            Err(e) => {
                let e = io_error(e);
                report(
                    options,
                    remote,
                    &Error::Wrapped {
                        text: format!("mllp: TLS handshake: {e}"),
                        wrapped: vec![e],
                    },
                );
                let _ = raw.shutdown(NetShutdown::Both);
                None
            }
        };
    }
    #[cfg(not(feature = "tls"))]
    let _ = tls;
    let _ = options;
    Some((Conn::plain(conn), State::default()))
}
#[cfg_attr(not(feature = "tls"), allow(clippy::clone_on_copy, clippy::unit_arg))]
fn serve_conn(conn: TcpStream, options: &ServerOptions, shutdown: &Shutdown, tls: &ServerTls) {
    let remote = conn.peer_addr().ok();
    let Some((conn, state)) = open(conn, options, tls) else {
        return;
    };
    let Ok(read_half) = conn.try_clone() else {
        return;
    };
    let mut reader = Reader::new(read_half, options.max_frame_bytes);
    if let Ok(timer) = conn.try_clone() {
        let read_timeout = options.read_timeout;
        reader.on_start = Some(Box::new(move || {
            timer
                .set_read_timeout(timeout(read_timeout))
                .map_err(io_error)
        }));
    }
    let mut writer = &conn;
    loop {
        if shutdown.is_cancelled() {
            break;
        }
        if let Err(e) = conn.set_read_timeout(timeout(options.idle_timeout)) {
            report(options, remote, &io_error(e));
            break;
        }
        let payload = match reader.read_frame() {
            Ok(p) => p,
            Err(err) => {
                if !shutdown.is_cancelled() && !err.is(Sentinel::Eof) {
                    if options.release == MLLP2
                        && (err.is(Sentinel::Framing) || err.is(Sentinel::FrameTooLarge))
                    {
                        let _ = server_confirmation(&conn, &mut writer, options, shutdown, false);
                    }
                    report(options, remote, &err);
                }
                break;
            }
        };
        if shutdown.is_cancelled() {
            break;
        }
        if options.release == MLLP2
            && let Err(e) = server_confirmation(&conn, &mut writer, options, shutdown, true)
        {
            if !shutdown.is_cancelled() {
                report(
                    options,
                    remote,
                    &transport_phase_error("request transport confirmation", e),
                );
            }
            break;
        }
        let Some(response) = respond(&payload, remote, state.clone(), options) else {
            break;
        };
        if shutdown.is_cancelled() {
            break;
        }
        if options.release == MLLP2 && response.len() as i64 > options.max_frame_bytes {
            report(options, remote, &Error::Sentinel(Sentinel::FrameTooLarge));
            break;
        }
        if let Err(e) = conn.set_write_timeout(timeout(options.write_timeout)) {
            report(options, remote, &io_error(e));
            break;
        }
        if let Err(e) = write_frame(&mut IoWriter(&mut writer), &response) {
            if !shutdown.is_cancelled() {
                report(
                    options,
                    remote,
                    &Error::Wrapped {
                        text: format!("mllp: write response: {e}"),
                        wrapped: vec![e],
                    },
                );
            }
            break;
        }
        if options.release == MLLP2 {
            if shutdown.is_cancelled() {
                break;
            }
            if let Err(e) = conn.set_read_timeout(timeout(options.transport_ack_timeout)) {
                report(options, remote, &io_error(e));
                break;
            }
            if let Err(e) = reader.read_confirmation() {
                if !shutdown.is_cancelled() {
                    report(
                        options,
                        remote,
                        &transport_phase_error("response transport confirmation", e),
                    );
                }
                break;
            }
        }
    }
    let _ = conn.shutdown();
}
fn server_confirmation(
    conn: &Conn,
    writer: &mut &Conn,
    options: &ServerOptions,
    shutdown: &Shutdown,
    accepted: bool,
) -> Result<(), Error> {
    if shutdown.is_cancelled() {
        return Err(Error::Text("context canceled".into()));
    }
    let d = options.transport_ack_timeout.min(options.write_timeout);
    conn.set_write_timeout(timeout(d)).map_err(io_error)?;
    write_confirmation(&mut IoWriter(writer), accepted)
}
fn respond(
    payload: &[u8],
    remote: Option<SocketAddr>,
    state: State,
    options: &ServerOptions,
) -> Option<Vec<u8>> {
    let message = match hl7::parse_with_options(payload, &options.parse_options) {
        Ok(m) => m,
        Err(err) => {
            let limit = err.is(hl7::Sentinel::Limit);
            report(options, remote, &Error::Hl7(err));
            let header = parse_header_only(payload, &options.parse_options)?;
            let code = if limit {
                ERROR_CODE_APPLICATION_INTERNAL_ERROR
            } else {
                ERROR_CODE_DATA_TYPE
            };
            let ack = hl7::new_ack(
                &header,
                "AR",
                &AckOptions {
                    error_code: code.into(),
                    text: "Message could not be parsed".into(),
                    ..AckOptions::default()
                },
            );
            return marshal_server_response(&header, &ack, remote, options);
        }
    };
    #[cfg(not(feature = "tls"))]
    let () = state;
    let request = Request {
        message,
        remote_addr: remote,
        #[cfg(feature = "tls")]
        tls: state,
    };
    if let Some(handler) = &options.original_ack_handler {
        return respond_original_ack(&request, payload, handler, options);
    }
    let handler = options.handler.as_ref()?;
    let response = match catch_unwind(AssertUnwindSafe(|| handler(&request))) {
        Ok(r) => r,
        Err(_) => {
            report(
                options,
                remote,
                &Error::Text("mllp: handler panicked".into()),
            );
            Some(hl7::new_ack(
                &request.message,
                "AR",
                &AckOptions {
                    error_code: ERROR_CODE_APPLICATION_INTERNAL_ERROR.into(),
                    text: "Application internal error".into(),
                    ..AckOptions::default()
                },
            ))
        }
    };
    let response =
        response.unwrap_or_else(|| hl7::new_ack(&request.message, "AA", &AckOptions::default()));
    marshal_server_response(&request.message, &response, remote, options)
}
fn marshal_server_response(
    original: &Message,
    response: &Message,
    remote: Option<SocketAddr>,
    options: &ServerOptions,
) -> Option<Vec<u8>> {
    let marshal = |m: &Message| {
        if options.release == MLLP2 || options.original_ack_handler.is_some() {
            m.marshal_with_limit(options.max_frame_bytes)
        } else {
            m.marshal()
        }
    };
    let encoded = marshal(response);
    match &encoded {
        Err(e) if e.is(hl7::Sentinel::Limit) => {
            report(options, remote, &Error::Sentinel(Sentinel::FrameTooLarge));
            return None;
        }
        Ok(data)
            if !data.is_empty()
                && !data
                    .iter()
                    .any(|&c| c == super::START_BLOCK || c == super::END_BLOCK) =>
        {
            return Some(data.clone());
        }
        _ => {}
    }
    let fallback = hl7::new_ack(
        original,
        "AE",
        &AckOptions {
            error_code: ERROR_CODE_APPLICATION_INTERNAL_ERROR.into(),
            text: "Application internal error".into(),
            ..AckOptions::default()
        },
    );
    match marshal(&fallback) {
        Err(e) if e.is(hl7::Sentinel::Limit) => {
            report(options, remote, &Error::Sentinel(Sentinel::FrameTooLarge));
            None
        }
        Err(_) => None,
        Ok(data) => Some(data),
    }
}
fn parse_header_only(payload: &[u8], options: &ParseOptions) -> Option<Message> {
    let line = match payload.iter().position(|&c| c == b'\r' || c == b'\n') {
        Some(end) => &payload[..end],
        None => payload,
    };
    hl7::parse_with_options(line, options).ok()
}
fn respond_original_ack(
    request: &Request,
    wire: &[u8],
    handler: &OriginalAckHandler,
    options: &ServerOptions,
) -> Option<Vec<u8>> {
    let fail = |code: &str, hl7_code: &str, message: &str, err: &str| {
        report(options, request.remote_addr, &Error::Text(err.into()));
        let ack = hl7::new_ack(
            &request.message,
            code,
            &AckOptions {
                error_code: hl7_code.into(),
                text: message.into(),
                ..AckOptions::default()
            },
        );
        marshal_server_response(&request.message, &ack, request.remote_addr, options)
    };
    match hl7::read_ack_policy(&request.message) {
        Ok(p) if !p.enhanced() => {}
        _ => {
            return fail(
                "AR",
                ERROR_CODE_TABLE_VALUE_NOT_FOUND,
                "Original acknowledgment mode required",
                "mllp: original acknowledgment mode is required",
            );
        }
    }
    let field: &[hl7::Repetition] = match request.message.segment("MSH") {
        Some(msh) => msh.field(10),
        None => &[],
    };
    let Some(control_id) = single_ack_value(field) else {
        return fail(
            "AR",
            ERROR_CODE_REQUIRED_FIELD_MISSING,
            "Message control ID required",
            "mllp: original acknowledgment requires one message control ID",
        );
    };
    let response = match catch_unwind(AssertUnwindSafe(|| handler(request, wire))) {
        Ok(Ok(r)) => r,
        _ => {
            return fail(
                "AE",
                ERROR_CODE_APPLICATION_INTERNAL_ERROR,
                "Application could not confirm acceptance",
                "mllp: original acknowledgment handler failed",
            );
        }
    };
    if !valid_original_ack(&response, &control_id, &options.parse_options) {
        return fail(
            "AE",
            ERROR_CODE_APPLICATION_INTERNAL_ERROR,
            "Application could not confirm acceptance",
            "mllp: invalid encoded original acknowledgment",
        );
    }
    Some(response)
}
fn valid_original_ack(response: &[u8], control_id: &str, opts: &ParseOptions) -> bool {
    if response.len() as i64 > opts.max_bytes || validate_frame_payload(response).is_err() {
        return false;
    }
    let Ok(message) = hl7::parse_with_options(response, opts) else {
        return false;
    };
    if message.header().message_type.code != "ACK" {
        return false;
    }
    match hl7::read_ack_policy(&message) {
        Ok(p) if !p.enhanced() => {}
        _ => return false,
    }
    let msas = message.all("MSA");
    if msas.len() != 1 {
        return false;
    }
    if single_ack_value(msas[0].field(2)).as_deref() != Some(control_id) {
        return false;
    }
    matches!(
        single_ack_value(msas[0].field(1)).as_deref(),
        Some("AA" | "AE" | "AR")
    )
}
fn single_ack_value(field: &[hl7::Repetition]) -> Option<String> {
    let value = field.string();
    (!value.is_empty()
        && !field.is_null()
        && field.len() == 1
        && field[0].len() == 1
        && field[0][0].len() == 1)
        .then(|| value.to_owned())
}
