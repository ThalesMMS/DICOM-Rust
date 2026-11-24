#![forbid(unsafe_code)]
use super::conn::{Conn, Ctx};
use super::error::{Detail, Error, Kind, Result, TlsFailureClass};
use super::tls_profile::{
    TlsClientConfig, TlsServerConfig, VERSION_TLS13, client_auth_state, reset_handshake_state,
    take_verify_error,
};
use rustls::pki_types::{CertificateDer, ServerName};
use std::io::{Read, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TlsConnectionState {
    pub version: u16,
    pub cipher_suite: u16,
    pub handshake_complete: bool,
    pub did_resume: bool,
    pub peer_certificates: Vec<CertificateDer<'static>>,
    pub server_name: String,
}
#[derive(Debug, Clone)]
pub struct TlsHandshakeFailure {
    pub remote_addr: Option<std::net::SocketAddr>,
    pub not_tls: bool,
    pub err: Error,
}
impl std::fmt::Display for TlsHandshakeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.not_tls {
            f.write_str("TLS handshake failed: the peer did not start a TLS handshake")
        } else {
            f.write_str("TLS handshake failed")
        }
    }
}
impl std::error::Error for TlsHandshakeFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.err)
    }
}
struct ReadState {
    pending: Vec<u8>,
    eof: bool,
    eof_reported: bool,
    failed: Option<Error>,
}
pub(crate) struct TlsIo {
    session: Mutex<rustls::Connection>,
    read: Mutex<ReadState>,
    write: Mutex<Vec<u8>>,
    received: AtomicU64,
    handshake_received: u64,
    state: TlsConnectionState,
    client_auth: Option<(bool, bool)>,
}
impl std::fmt::Debug for TlsIo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsIo")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}
fn alert_text(alert: u8) -> String {
    let text = match alert {
        0 => "close notify",
        10 => "unexpected message",
        20 => "bad record MAC",
        21 => "decryption failed",
        22 => "record overflow",
        30 => "decompression failure",
        40 => "handshake failure",
        42 => "bad certificate",
        43 => "unsupported certificate",
        44 => "revoked certificate",
        45 => "expired certificate",
        46 => "unknown certificate",
        47 => "illegal parameter",
        48 => "unknown certificate authority",
        49 => "access denied",
        50 => "error decoding message",
        51 => "error decrypting message",
        60 => "export restriction",
        70 => "protocol version not supported",
        71 => "insufficient security level",
        80 => "internal error",
        86 => "inappropriate fallback",
        90 => "user canceled",
        100 => "no renegotiation",
        109 => "missing extension",
        110 => "unsupported extension",
        111 => "certificate unobtainable",
        112 => "unrecognized name",
        113 => "bad certificate status response",
        114 => "bad certificate hash value",
        115 => "unknown PSK identity",
        116 => "certificate required",
        120 => "no application protocol",
        121 => "encrypted client hello required",
        other => return format!("tls: alert({other})"),
    };
    format!("tls: {text}")
}
fn failure(class: TlsFailureClass, text: impl Into<String>) -> Error {
    Error::other(text).with_detail(Detail::Tls(class))
}
fn tls_error(e: rustls::Error) -> Error {
    if let Some(own) = take_verify_error() {
        return own;
    }
    use rustls::CertificateError as C;
    match e {
        rustls::Error::AlertReceived(alert) => {
            let code = u8::from(alert);
            failure(
                TlsFailureClass::RemoteAlert(code),
                format!("remote error: {}", alert_text(code)),
            )
        }
        rustls::Error::NoCertificatesPresented => failure(
            TlsFailureClass::NoClientCertificate,
            "tls: client didn't provide a certificate",
        ),
        rustls::Error::PeerIncompatible(reason) => {
            let name = format!("{reason:?}");
            let class = if name.contains("Tls12") || name.contains("Tls13") {
                TlsFailureClass::Version
            } else {
                TlsFailureClass::HandshakeFailed
            };
            failure(class, format!("tls: {name}"))
        }
        rustls::Error::InvalidCertificate(reason) => {
            let class = match reason {
                C::UnknownIssuer => TlsFailureClass::UnknownCa,
                C::Expired
                | C::ExpiredContext { .. }
                | C::NotValidYet
                | C::NotValidYetContext { .. } => TlsFailureClass::CertificateExpired,
                C::NotValidForName | C::NotValidForNameContext { .. } => {
                    TlsFailureClass::HostnameMismatch
                }
                C::InvalidPurpose | C::InvalidPurposeContext { .. } => {
                    TlsFailureClass::CertificateUsage
                }
                _ => TlsFailureClass::BadCertificate,
            };
            let err = failure(
                class,
                format!("tls: failed to verify certificate: {reason:?}"),
            );
            if matches!(reason, C::NotValidYet | C::NotValidYetContext { .. }) {
                err.also(Kind::TlsCertificateNotYetValid)
            } else {
                err
            }
        }
        other => failure(TlsFailureClass::HandshakeFailed, format!("tls: {other}")),
    }
}
#[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
pub(crate) fn tls_failure_of(e: rustls::Error) -> (TlsFailureClass, bool) {
    let err = tls_error(e);
    (
        err.tls_failure()
            .unwrap_or(TlsFailureClass::HandshakeFailed),
        err.tls_certificate_not_yet_valid(),
    )
}
fn closed_by_peer(err: &Error) -> bool {
    err.is(Kind::Eof) || err.is(Kind::UnexpectedEof) || err.is(Kind::PeerReset)
}
fn write_all_raw(conn: &Conn, data: &[u8], ctx: &Ctx) -> Result<()> {
    conn.raw_write(data, ctx).1
}
fn collect_output(session: &mut rustls::Connection, out: &mut Vec<u8>) {
    while session.wants_write() {
        if session.write_tls(out).is_err() {
            break;
        }
    }
}
fn first_record_is_tls(header: &[u8]) -> bool {
    let record_type = header[0];
    let version = u16::from_be_bytes([header[1], header[2]]);
    (record_type == 21 || record_type == 22) && version < 0x1000
}
fn handshake(
    conn: &Conn,
    mut session: rustls::Connection,
    ctx: &Ctx,
    requestor: bool,
) -> Result<TlsIo> {
    reset_handshake_state();
    let mut pending: Vec<u8> = Vec::new();
    let mut received = 0u64;
    let mut first_checked = false;
    let mut scratch = vec![0u8; 18 * 1024];
    loop {
        let mut out = Vec::new();
        collect_output(&mut session, &mut out);
        if !out.is_empty() {
            write_all_raw(conn, &out, ctx)?;
        }
        if !session.is_handshaking() {
            break;
        }
        let n = conn.raw_read(&mut scratch, ctx)?;
        if n == 0 {
            return Err(if pending.is_empty() {
                Error::eof()
            } else {
                Error::unexpected_eof()
            });
        }
        received += n as u64;
        pending.extend_from_slice(&scratch[..n]);
        if !first_checked {
            if pending.len() < 5 {
                continue;
            }
            if !first_record_is_tls(&pending) {
                return Err(failure(
                    TlsFailureClass::NotTls,
                    "tls: first record does not look like a TLS handshake",
                ));
            }
            first_checked = true;
        }
        while !pending.is_empty() {
            let mut cursor: &[u8] = &pending;
            let taken = session.read_tls(&mut cursor).unwrap_or_default();
            pending.drain(..taken);
            if let Err(e) = session.process_new_packets() {
                let mut out = Vec::new();
                collect_output(&mut session, &mut out);
                let wrote_alert = !out.is_empty()
                    && write_all_raw(
                        conn,
                        &out,
                        &Ctx::background().with_timeout(Duration::from_secs(1)),
                    )
                    .is_ok();
                let err = tls_error(e);
                if wrote_alert && !requestor {
                    conn.close_write_raw();
                    conn.drain_raw(Duration::from_millis(500), 1 << 20);
                }
                return Err(err);
            }
            if taken == 0 {
                break;
            }
        }
    }
    let version = session
        .protocol_version()
        .map(u16::from)
        .unwrap_or_default();
    let server_name = match &session {
        rustls::Connection::Server(s) => s.server_name().unwrap_or_default().to_string(),
        rustls::Connection::Client(_) => String::new(),
    };
    let state = TlsConnectionState {
        version,
        cipher_suite: session
            .negotiated_cipher_suite()
            .map(|s| u16::from(s.suite()))
            .unwrap_or_default(),
        handshake_complete: true,
        did_resume: matches!(
            session.handshake_kind(),
            Some(rustls::HandshakeKind::Resumed)
        ),
        peer_certificates: session
            .peer_certificates()
            .map(|c| c.to_vec())
            .unwrap_or_default(),
        server_name,
    };
    Ok(TlsIo {
        session: Mutex::new(session),
        read: Mutex::new(ReadState {
            pending,
            eof: false,
            eof_reported: false,
            failed: None,
        }),
        write: Mutex::new(Vec::new()),
        received: AtomicU64::new(received),
        handshake_received: received,
        state,
        client_auth: requestor.then(client_auth_state),
    })
}
fn tls_server_name(address: &str) -> &str {
    if let Some(rest) = address.strip_prefix('[') {
        if let Some(end) = rest.find(']')
            && rest[end + 1..].starts_with(':')
        {
            return &rest[..end];
        }
        return address;
    }
    match address.rfind(':') {
        Some(i) if !address[..i].contains(':') => &address[..i],
        _ => address,
    }
}
pub(crate) fn client_handshake(
    conn: &Conn,
    address: &str,
    config: &TlsClientConfig,
    ctx: &Ctx,
) -> Result<TlsIo> {
    let wrap = |e: Error| e.prefixed("dicom ul: TLS client handshake");
    let mut name = config.server_name.as_str();
    if name.is_empty() && config.policy.is_none() {
        name = tls_server_name(address);
    }
    let server_name = match ServerName::try_from(name.to_string()) {
        Ok(n) => n,
        Err(_) => ServerName::IpAddress(conn.peer_addr().ip().into()),
    };
    let session = rustls::ClientConnection::new(config.config.clone(), server_name)
        .map_err(|e| wrap(Error::other(format!("tls: {e}"))))?;
    let io = handshake(conn, rustls::Connection::Client(session), ctx, true)
        .map_err(|e| wrap(super::conn::association_context_error(ctx, e)))?;
    Ok(io)
}
pub(crate) fn server_handshake(conn: &Conn, config: &TlsServerConfig, ctx: &Ctx) -> Result<TlsIo> {
    let wrap = |e: Error| e.prefixed("dicom ul: TLS server handshake");
    let session = rustls::ServerConnection::new(config.config.clone())
        .map_err(|e| wrap(Error::other(format!("tls: {e}"))))?;
    let io = handshake(conn, rustls::Connection::Server(session), ctx, false)
        .map_err(|e| wrap(super::conn::association_context_error(ctx, e)))?;
    if io.state.did_resume
        && let Some(policy) = &config.policy
    {
        policy.reverify(&io.state.peer_certificates).map_err(wrap)?;
    }
    Ok(io)
}
impl TlsIo {
    pub(crate) fn state(&self) -> &TlsConnectionState {
        &self.state
    }
    fn flush(&self, conn: &Conn, ctx: &Ctx) -> Result<()> {
        let mut pending = self.write.lock().unwrap();
        collect_output(&mut self.session.lock().unwrap(), &mut pending);
        self.send_pending(conn, &mut pending, ctx)
    }
    fn send_pending(&self, conn: &Conn, pending: &mut Vec<u8>, ctx: &Ctx) -> Result<()> {
        if pending.is_empty() {
            return Ok(());
        }
        let (written, result) = conn.raw_write(pending, ctx);
        pending.drain(..written);
        result
    }
    pub(crate) fn read(&self, conn: &Conn, buf: &mut [u8], ctx: &Ctx) -> Result<usize> {
        let mut read = self.read.lock().unwrap();
        let mut scratch = [0u8; 16 * 1024];
        loop {
            if let Some(e) = &read.failed {
                return Err(e.clone());
            }
            {
                let mut session = self.session.lock().unwrap();
                match session.reader().read(buf) {
                    Ok(n) => return Ok(n),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(0),
                    Err(e) => {
                        let err = Error::other(format!("tls: {e}"));
                        read.failed = Some(err.clone());
                        return Err(err);
                    }
                }
                if read.eof {
                    if read.eof_reported {
                        return Ok(0);
                    }
                    read.eof_reported = true;
                    let mut empty: &[u8] = &[];
                    let _ = session.read_tls(&mut empty);
                    continue;
                }
                if !read.pending.is_empty() {
                    let mut cursor: &[u8] = &read.pending;
                    let taken = session.read_tls(&mut cursor).unwrap_or(0);
                    read.pending.drain(..taken);
                    let processed = session.process_new_packets();
                    let wants_write = session.wants_write();
                    drop(session);
                    if let Err(e) = processed {
                        let _ = self.flush(
                            conn,
                            &Ctx::background().with_timeout(Duration::from_millis(200)),
                        );
                        let err = tls_error(e);
                        read.failed = Some(err.clone());
                        return Err(err);
                    }
                    if wants_write {
                        let _ = self.flush(conn, ctx);
                    }
                    if taken == 0 {
                        let err = Error::other("tls: record overflow");
                        read.failed = Some(err.clone());
                        return Err(err);
                    }
                    continue;
                }
            }
            let n = conn.raw_read(&mut scratch, ctx)?;
            if n == 0 {
                read.eof = true;
            } else {
                self.received.fetch_add(n as u64, Ordering::SeqCst);
                read.pending.extend_from_slice(&scratch[..n]);
            }
        }
    }
    pub(crate) fn write_all(&self, conn: &Conn, data: &[u8], ctx: &Ctx) -> (usize, Result<()>) {
        let mut rest = data;
        let result = self.write_rest(conn, &mut rest, ctx);
        (data.len() - rest.len(), result)
    }
    fn write_rest(&self, conn: &Conn, data: &mut &[u8], ctx: &Ctx) -> Result<()> {
        let mut pending = self.write.lock().unwrap();
        self.send_pending(conn, &mut pending, ctx)?;
        while !data.is_empty() {
            {
                let mut session = self.session.lock().unwrap();
                let n = session
                    .writer()
                    .write(data)
                    .map_err(|e| Error::other(format!("tls: {e}")))?;
                if n == 0 && !session.wants_write() {
                    return Err(Error::sentinel(Kind::ShortWrite));
                }
                *data = &data[n..];
                collect_output(&mut session, &mut pending);
            }
            self.send_pending(conn, &mut pending, ctx)?;
        }
        Ok(())
    }
    pub(crate) fn close_notify(&self, conn: &Conn, timeout: Duration) {
        let Ok(mut pending) = self.write.try_lock() else {
            return;
        };
        {
            let Ok(mut session) = self.session.try_lock() else {
                return;
            };
            session.send_close_notify();
            collect_output(&mut session, &mut pending);
        }
        let ctx = Ctx::background().with_timeout(timeout);
        let _ = self.send_pending(conn, &mut pending, &ctx);
    }
    pub(crate) fn refusal(&self, conn: &Conn, err: Error) -> Error {
        let Some((requested, sent)) = self.client_auth else {
            return err;
        };
        if !requested || !closed_by_peer(&err) || self.state.version != VERSION_TLS13 {
            return err;
        }
        let ctx = Ctx::background().with_timeout(Duration::from_millis(100));
        let mut byte = [0u8; 1];
        let unread = self.read(conn, &mut byte, &ctx);
        if self.received.load(Ordering::SeqCst) != self.handshake_received {
            if let Err(alert) = unread
                && matches!(alert.tls_failure(), Some(TlsFailureClass::RemoteAlert(_)))
            {
                let text = format!("{}; {}", alert.text(), err.text());
                let mut kinds = alert.kinds().to_vec();
                kinds.extend(err.kinds().iter().copied());
                return Error::with_kinds(text, kinds).with_detail(alert.detail().clone());
            }
            return err;
        }
        if !err.is(Kind::Eof) && !err.is(Kind::UnexpectedEof) {
            return err;
        }
        let presented = if sent {
            "one was sent"
        } else {
            "none was sent"
        };
        Error::with_kinds(
            format!(
                "dicom ul: the TLS 1.3 peer asked for a client certificate, {presented}, and it closed the connection before answering: {}",
                err.text()
            ),
            err.kinds().to_vec(),
        )
        .with_detail(Detail::ClientCertificateRefused(sent))
    }
}
