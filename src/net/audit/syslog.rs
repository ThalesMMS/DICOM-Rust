#![forbid(unsafe_code)]
use super::timetext;
use super::{
    ClinicalPolicy, Ctx, Error, ErrorKind, Message, Nanos, OUTCOME_SUCCESS, Result, Time, ctx_err,
};
use crate::net::ul::{Conn, SourcePortDialer};
use std::collections::VecDeque;
use std::net::{TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};
pub const SYSLOG_TLS: &str = "tls";
pub const SYSLOG_UDP: &str = "udp";
pub const SYSLOG_TCP: &str = "tcp";
pub const DEFAULT_SYSLOG_SPOOL_MESSAGES: i64 = 1000;
pub const DEFAULT_SYSLOG_SPOOL_BYTES: i64 = 16 << 20;
pub const DEFAULT_SYSLOG_RETRY: Nanos = Nanos(5_000_000_000);
pub const MAX_SYSLOG_UDP_MESSAGE: usize = 65507;
const SYSLOG_FACILITY_AUTH_PRIV: i64 = 10;
#[cfg(feature = "tls")]
pub type SyslogTls = crate::net::ul::TlsClientConfig;
#[cfg(not(feature = "tls"))]
#[derive(Debug, Clone)]
pub enum SyslogTls {}
pub enum SyslogSocket {
    Stream(TcpStream),
    Datagram(UdpSocket),
}
pub type SyslogDial = Arc<dyn Fn(&Ctx, &str, &str) -> std::io::Result<SyslogSocket> + Send + Sync>;
#[derive(Clone, Default)]
pub struct SyslogOptions {
    pub network: String,
    pub address: String,
    pub tls: Option<SyslogTls>,
    pub app_name: String,
    pub hostname: String,
    pub max_spool_messages: i64,
    pub max_spool_bytes: i64,
    pub retry_interval: Nanos,
    pub dial: Option<SyslogDial>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyslogStats {
    pub spooled: i64,
    pub spooled_bytes: i64,
    pub dropped: i64,
}
pub(crate) enum Link {
    Stream(Box<Conn>),
    Datagram(UdpSocket, AtomicBool),
}
impl Link {
    pub(crate) fn close(&self) {
        match self {
            Link::Stream(conn) => conn.close(),
            Link::Datagram(_, closed) => closed.store(true, Ordering::SeqCst),
        }
    }
}
#[derive(Default)]
struct Spool {
    records: VecDeque<Arc<Vec<u8>>>,
    bytes: i64,
    dropped: i64,
    conn: Option<Arc<Link>>,
}
#[derive(Default)]
pub(crate) struct Wake {
    set: Mutex<bool>,
    changed: Condvar,
}
impl Wake {
    pub(crate) fn signal(&self) {
        *self.set.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.changed.notify_all();
    }
    pub(crate) fn wait(&self, ctx: &Ctx, timer: Option<Instant>) -> bool {
        let mut set = self.set.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if ctx.err().is_some() {
                return false;
            }
            if *set {
                *set = false;
                return true;
            }
            let mut slice = Duration::from_millis(10);
            if let Some(t) = timer {
                let now = Instant::now();
                if now >= t {
                    return false;
                }
                slice = slice.min(t - now);
            }
            set = self
                .changed
                .wait_timeout(set, slice)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}
#[derive(Default)]
pub struct SyslogLogger {
    options: SyslogOptions,
    spool: Mutex<Spool>,
    wake: Wake,
    flush: Mutex<()>,
}
fn severity_of(message: &Message) -> i64 {
    if message.event.outcome_indicator != OUTCOME_SUCCESS {
        4
    } else {
        5
    }
}
pub(crate) fn check_transport(network: &str, address: &str, has_tls: bool) -> Result<()> {
    match network {
        SYSLOG_TLS => {
            if !has_tls {
                return Err(Error::wrap(
                    ErrorKind::SyslogConfig,
                    "TLS needs a TLS client configuration",
                ));
            }
        }
        SYSLOG_UDP | SYSLOG_TCP => {}
        _ => {
            return Err(Error::wrap(
                ErrorKind::SyslogConfig,
                format!("network {}", crate::textfmt::quote(network)),
            ));
        }
    }
    if address.is_empty() {
        return Err(Error::wrap(ErrorKind::SyslogConfig, "no address"));
    }
    Ok(())
}
fn syslog_field(value: &str) -> String {
    let out: String = value
        .bytes()
        .filter(|c| (33..=126).contains(c))
        .take(48)
        .map(char::from)
        .collect();
    if out.is_empty() { "-".to_string() } else { out }
}
pub(crate) fn syslog_record(
    hostname: &str,
    app_name: &str,
    severity: i64,
    at: &Time,
    payload: &[u8],
) -> Vec<u8> {
    let host = if hostname.is_empty() {
        super::os_hostname()
    } else {
        hostname.to_string()
    };
    let app = if app_name.is_empty() {
        "dcm-rust"
    } else {
        app_name
    };
    let mut record = format!(
        "<{}>1 {} {} {} {} IHE+RFC-3881 - \u{feff}",
        SYSLOG_FACILITY_AUTH_PRIV * 8 + severity,
        timetext::format_syslog_utc(at),
        syslog_field(&host),
        syslog_field(app),
        std::process::id()
    )
    .into_bytes();
    record.extend_from_slice(payload);
    record
}
pub(crate) fn dial_link(
    network: &str,
    address: &str,
    tls: Option<&SyslogTls>,
    dial: Option<&SyslogDial>,
    ctx: &Ctx,
) -> Result<Arc<Link>> {
    let transport = if network == SYSLOG_UDP { "udp" } else { "tcp" };
    let socket = match dial {
        Some(dial) => dial(ctx, transport, address),
        None => default_dial(ctx, transport, address),
    };
    let socket = match socket {
        Ok(socket) => socket,
        Err(e) => return Err(ctx_err(ctx).unwrap_or_else(|| Error::transport(e))),
    };
    let link = match socket {
        SyslogSocket::Datagram(socket) => Link::Datagram(socket, AtomicBool::new(false)),
        SyslogSocket::Stream(stream) => {
            let conn = Conn::new(stream).map_err(Error::transport)?;
            if network == SYSLOG_TLS {
                start_tls(&conn, address, tls, ctx)?;
            }
            Link::Stream(Box::new(conn))
        }
    };
    Ok(Arc::new(link))
}
#[cfg(feature = "tls")]
fn start_tls(conn: &Conn, address: &str, tls: Option<&SyslogTls>, ctx: &Ctx) -> Result<()> {
    let Some(tls) = tls else {
        return Err(Error::wrap(
            ErrorKind::SyslogConfig,
            "TLS needs a TLS client configuration",
        ));
    };
    let mut config = tls.clone();
    if config.server_name.is_empty()
        && let Some((host, _)) = super::split_host_port(address)
    {
        config.server_name = host.to_string();
    }
    if let Err(e) = conn.start_tls_client(address, &config, ctx) {
        conn.close();
        return Err(ctx_err(ctx).unwrap_or_else(|| Error::transport(e)));
    }
    Ok(())
}
#[cfg(not(feature = "tls"))]
fn start_tls(_: &Conn, _: &str, _: Option<&SyslogTls>, _: &Ctx) -> Result<()> {
    Err(Error::wrap(
        ErrorKind::SyslogConfig,
        "TLS needs a TLS client configuration",
    ))
}
fn default_dial(ctx: &Ctx, network: &str, address: &str) -> std::io::Result<SyslogSocket> {
    if network == "udp" {
        let mut last = std::io::Error::new(std::io::ErrorKind::InvalidInput, "no address");
        for target in address.to_socket_addrs()? {
            let local = if target.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            };
            match UdpSocket::bind(local).and_then(|s| s.connect(target).map(|()| s)) {
                Ok(socket) => return Ok(SyslogSocket::Datagram(socket)),
                Err(e) => last = e,
            }
        }
        return Err(last);
    }
    SourcePortDialer::default()
        .dial(ctx, address)
        .map(SyslogSocket::Stream)
}
pub(crate) fn send_link(network: &str, link: &Link, record: &[u8], ctx: &Ctx) -> Result<()> {
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    let bounded = if ctx.deadline().is_some() {
        ctx.clone()
    } else {
        ctx.with_timeout(Duration::from_secs(30))
    };
    match link {
        Link::Datagram(socket, closed) => {
            if closed.load(Ordering::SeqCst) {
                return Err(Error::transport("use of closed network connection"));
            }
            let timeout = bounded
                .deadline()
                .map(|d| d.saturating_duration_since(Instant::now()))
                .filter(|d| !d.is_zero())
                .unwrap_or(Duration::from_millis(1));
            let _ = socket.set_write_timeout(Some(timeout));
            match socket.send(record) {
                Ok(n) if n != record.len() => Err(Error::transport("short write")),
                Ok(_) => Ok(()),
                Err(e) => Err(ctx_err(ctx).unwrap_or_else(|| Error::transport(e))),
            }
        }
        Link::Stream(conn) => {
            debug_assert!(network != SYSLOG_UDP);
            let mut framed = format!("{} ", record.len()).into_bytes();
            framed.extend_from_slice(record);
            if let Err(e) = conn.write_all_ctx(&framed, &bounded) {
                return Err(ctx_err(ctx).unwrap_or_else(|| Error::transport(e)));
            }
            match ctx_err(ctx) {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
    }
}
impl SyslogLogger {
    pub fn new(options: SyslogOptions) -> SyslogLogger {
        SyslogLogger {
            options,
            ..Default::default()
        }
    }
    pub fn options(&self) -> &SyslogOptions {
        &self.options
    }
    fn lock(&self) -> MutexGuard<'_, Spool> {
        self.spool.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn log(&self, message: &Message) -> Result<()> {
        let payload = message.marshal()?;
        let at = timetext::parse_rfc3339(&message.event.date_time).unwrap_or_else(Time::now_local);
        self.log_raw(severity_of(message), &at, &payload)
    }
    pub fn log_raw(&self, severity: i64, at: &Time, payload: &[u8]) -> Result<()> {
        self.spool_record(severity, at, payload, false)
    }
    pub fn log_clinical(&self, policy: &ClinicalPolicy, message: &Message) -> Result<()> {
        let payload = message.marshal_clinical(policy)?;
        let at = timetext::parse_rfc3339(&message.event.date_time).unwrap_or_default();
        if self
            .spool_record(severity_of(message), &at, &payload, true)
            .is_err()
        {
            return Err(Error::clinical("spool", "Logger"));
        }
        Ok(())
    }
    fn check(&self) -> Result<()> {
        check_transport(
            &self.options.network,
            &self.options.address,
            self.options.tls.is_some(),
        )
    }
    fn limits(&self) -> (i64, i64) {
        let o = &self.options;
        (
            if o.max_spool_messages <= 0 {
                DEFAULT_SYSLOG_SPOOL_MESSAGES
            } else {
                o.max_spool_messages
            },
            if o.max_spool_bytes <= 0 {
                DEFAULT_SYSLOG_SPOOL_BYTES
            } else {
                o.max_spool_bytes
            },
        )
    }
    fn spool_record(&self, severity: i64, at: &Time, payload: &[u8], strict: bool) -> Result<()> {
        self.check()?;
        let o = &self.options;
        let record = syslog_record(&o.hostname, &o.app_name, severity, at, payload);
        let size = record.len() as i64;
        if o.network == SYSLOG_UDP && record.len() > MAX_SYSLOG_UDP_MESSAGE {
            return Err(Error::wrap(
                ErrorKind::SyslogConfig,
                format!("{size}-byte message exceeds a UDP datagram"),
            ));
        }
        let (max_messages, max_bytes) = self.limits();
        if size > max_bytes {
            return Err(Error::wrap(
                ErrorKind::SyslogConfig,
                format!("{size}-byte message exceeds MaxSpoolBytes"),
            ));
        }
        {
            let mut spool = self.lock();
            if strict
                && (spool.records.len() as i64 >= max_messages || size > max_bytes - spool.bytes)
            {
                return Err(Error::clinical("spool", "Logger"));
            }
            spool.records.push_back(Arc::new(record));
            spool.bytes += size;
            while spool.records.len() as i64 > max_messages || spool.bytes > max_bytes {
                if let Some(oldest) = spool.records.pop_front() {
                    spool.bytes -= oldest.len() as i64;
                }
                spool.dropped += 1;
            }
        }
        self.wake.signal();
        Ok(())
    }
    pub fn stats(&self) -> SyslogStats {
        let spool = self.lock();
        SyslogStats {
            spooled: spool.records.len() as i64,
            spooled_bytes: spool.bytes,
            dropped: spool.dropped,
        }
    }
    pub fn flush(&self, ctx: &Ctx) -> Result<()> {
        let _flushing = self.flush.lock().unwrap_or_else(|e| e.into_inner());
        self.check()?;
        let o = &self.options;
        loop {
            let (record, conn) = {
                let spool = self.lock();
                match spool.records.front() {
                    None => return Ok(()),
                    Some(record) => (record.clone(), spool.conn.clone()),
                }
            };
            let conn = match conn {
                Some(conn) => conn,
                None => {
                    let conn =
                        dial_link(&o.network, &o.address, o.tls.as_ref(), o.dial.as_ref(), ctx)?;
                    self.lock().conn = Some(conn.clone());
                    conn
                }
            };
            if let Err(e) = send_link(&o.network, &conn, &record, ctx) {
                {
                    let mut spool = self.lock();
                    if spool.conn.as_ref().is_some_and(|c| Arc::ptr_eq(c, &conn)) {
                        spool.conn = None;
                    }
                }
                conn.close();
                return Err(e);
            }
            let mut spool = self.lock();
            if spool
                .records
                .front()
                .is_some_and(|head| Arc::ptr_eq(head, &record))
            {
                spool.bytes -= record.len() as i64;
                spool.records.pop_front();
            }
        }
    }
    pub fn run(&self, ctx: &Ctx) -> Error {
        let retry = self
            .options
            .retry_interval
            .positive()
            .unwrap_or(Duration::from_secs(5));
        let mut wait = retry;
        loop {
            let timer = if self.flush(ctx).is_err() {
                let timer = Instant::now() + wait;
                wait = (2 * wait).min(Duration::from_secs(60));
                Some(timer)
            } else {
                wait = retry;
                None
            };
            self.wake.wait(ctx, timer);
            if let Some(e) = ctx_err(ctx) {
                self.close();
                return e;
            }
        }
    }
    pub fn close(&self) {
        let conn = self.lock().conn.take();
        if let Some(conn) = conn {
            conn.close();
        }
    }
}
