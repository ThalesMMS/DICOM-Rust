#![forbid(unsafe_code)]
use super::error::Kind;
#[cfg(feature = "net")]
use super::error::{Error, Result};
#[cfg(feature = "net")]
use std::io::{self, Read, Write};
#[cfg(feature = "net")]
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
#[cfg(feature = "net")]
use std::sync::PoisonError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Nanos(pub i64);
impl Nanos {
    pub const ZERO: Nanos = Nanos(0);
    pub fn from_millis(ms: i64) -> Self {
        Nanos(ms.saturating_mul(1_000_000))
    }
    pub fn from_secs(s: i64) -> Self {
        Nanos(s.saturating_mul(1_000_000_000))
    }
    pub fn from_std(d: Duration) -> Self {
        Nanos(d.as_nanos().min(i64::MAX as u128) as i64)
    }
    pub fn is_negative(self) -> bool {
        self.0 < 0
    }
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
    pub fn positive(self) -> Option<Duration> {
        (self.0 > 0).then(|| Duration::from_nanos(self.0 as u64))
    }
}
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_canceled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}
#[derive(Debug, Clone, Default)]
pub struct Ctx {
    deadline: Option<Instant>,
    cancels: Vec<CancelToken>,
    pub(crate) active_transfer: bool,
    pub(crate) no_idle: bool,
    pub(crate) read_progress: Option<Duration>,
    pub(crate) write_progress: Option<Duration>,
    pub(crate) operation_token: Option<u64>,
    values: CtxValues,
}
#[derive(Clone, Default)]
struct CtxValues(Vec<(std::any::TypeId, Arc<dyn std::any::Any + Send + Sync>)>);
impl std::fmt::Debug for CtxValues {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CtxValues({})", self.0.len())
    }
}
impl Ctx {
    pub fn with_value<T: std::any::Any + Send + Sync>(&self, value: T) -> Ctx {
        let mut c = self.clone();
        let id = std::any::TypeId::of::<T>();
        c.values.0.retain(|(k, _)| *k != id);
        c.values.0.push((id, Arc::new(value)));
        c
    }
    pub fn value<T: std::any::Any + Send + Sync>(&self) -> Option<&T> {
        let id = std::any::TypeId::of::<T>();
        self.values
            .0
            .iter()
            .find(|(k, _)| *k == id)
            .and_then(|(_, v)| v.downcast_ref::<T>())
    }
    pub fn background() -> Ctx {
        Ctx::default()
    }
    pub fn with_cancel(&self, token: &CancelToken) -> Ctx {
        let mut c = self.clone();
        c.cancels.push(token.clone());
        c
    }
    pub fn with_end_of(&self, other: &Ctx) -> Ctx {
        let mut c = self.clone();
        c.cancels.extend(other.cancels.iter().cloned());
        if let Some(d) = other.deadline {
            c.deadline = Some(c.deadline.map_or(d, |own| own.min(d)));
        }
        c
    }
    pub fn with_deadline(&self, deadline: Instant) -> Ctx {
        let mut c = self.clone();
        c.deadline = Some(match self.deadline {
            Some(d) if d < deadline => d,
            _ => deadline,
        });
        c
    }
    pub fn with_timeout(&self, timeout: Duration) -> Ctx {
        self.with_deadline(Instant::now() + timeout)
    }
    pub fn with_optional_timeout(&self, timeout: Nanos) -> Ctx {
        match timeout.positive() {
            Some(d) => self.with_timeout(d),
            None => self.clone(),
        }
    }
    pub fn without_cancel(&self) -> Ctx {
        let mut c = self.clone();
        c.deadline = None;
        c.cancels.clear();
        c
    }
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
    pub fn err(&self) -> Option<Kind> {
        if self.cancels.iter().any(CancelToken::is_canceled) {
            return Some(Kind::Canceled);
        }
        match self.deadline {
            Some(d) if Instant::now() >= d => Some(Kind::DeadlineExceeded),
            _ => None,
        }
    }
    pub(crate) fn can_end(&self) -> bool {
        self.deadline.is_some() || !self.cancels.is_empty()
    }
    pub fn with_active_transfer(&self) -> Ctx {
        let mut c = self.clone();
        c.active_transfer = true;
        c
    }
    pub fn without_idle_timeout(&self) -> Ctx {
        let mut c = self.clone();
        c.no_idle = true;
        c
    }
    pub fn with_read_progress_timeout(&self, timeout: Nanos) -> Ctx {
        let mut c = self.clone();
        if let Some(d) = timeout.positive() {
            c.read_progress = Some(d);
        }
        c
    }
    pub fn with_write_progress_timeout(&self, timeout: Nanos) -> Ctx {
        let mut c = self.clone();
        if let Some(d) = timeout.positive() {
            c.write_progress = Some(d);
        }
        c
    }
}
#[cfg(feature = "net")]
pub(crate) fn ended_context_error(ctx: &Ctx) -> Option<Error> {
    match ctx.err()? {
        Kind::DeadlineExceeded => Some(context_deadline_error()),
        _ => Some(Error::sentinel(Kind::Canceled)),
    }
}
#[cfg(feature = "net")]
pub(crate) fn context_deadline_error() -> Error {
    Error::wrap(Kind::AssociationTimeout, Kind::DeadlineExceeded.text())
        .also(Kind::AssociationTimeout)
}
#[cfg(feature = "net")]
pub(crate) fn association_context_error(ctx: &Ctx, err: Error) -> Error {
    if ctx.err() == Some(Kind::DeadlineExceeded) {
        return context_deadline_error();
    }
    if err.is(Kind::NetTimeout) {
        return Error::wrap(Kind::AssociationTimeout, err.text());
    }
    err
}
#[cfg(feature = "net")]
pub(crate) fn io_error(
    err: &io::Error,
    op: &str,
    addrs: Option<(SocketAddr, SocketAddr)>,
) -> Error {
    let message = match err.raw_os_error() {
        Some(_) => {
            let text = err.to_string();
            let text = match text.find(" (os error") {
                Some(i) => text[..i].to_string(),
                None => text,
            };
            let syscall = match (cfg!(windows), op) {
                (true, "read") => "wsarecv",
                (true, "write") => "wsasend",
                (_, other) => other,
            };
            format!("{syscall}: {text}")
        }
        None => err.to_string(),
    };
    let text = match addrs {
        Some((l, r)) => format!("{op} tcp {l}->{r}: {message}"),
        None => format!("{op} tcp: {message}"),
    };
    let kinds = match err.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => vec![Kind::NetTimeout],
        io::ErrorKind::NotConnected => vec![Kind::Closed],
        io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe => vec![Kind::Other, Kind::PeerReset, Kind::Syscall],
        _ if err.raw_os_error().is_some() => vec![Kind::Other, Kind::Syscall],
        _ => vec![Kind::Other],
    };
    Error::with_kinds(text, kinds).with_os_error(err.raw_os_error())
}
#[cfg(feature = "net")]
const POLL: Duration = Duration::from_millis(20);
#[cfg(feature = "net")]
#[derive(Debug)]
pub struct Conn {
    stream: TcpStream,
    local: SocketAddr,
    peer: SocketAddr,
    closed: AtomicBool,
    prefetched: std::sync::Mutex<Vec<u8>>,
    has_prefetched: AtomicBool,
    pdu_readers: std::sync::atomic::AtomicUsize,
    #[cfg(feature = "tls")]
    tls: std::sync::OnceLock<super::tls_conn::TlsIo>,
}
#[cfg(feature = "net")]
impl Conn {
    pub fn new(stream: TcpStream) -> io::Result<Conn> {
        let local = stream.local_addr()?;
        let peer = stream.peer_addr()?;
        let _ = stream.set_nodelay(true);
        Ok(Conn {
            stream,
            local,
            peer,
            closed: AtomicBool::new(false),
            prefetched: std::sync::Mutex::new(Vec::new()),
            has_prefetched: AtomicBool::new(false),
            pdu_readers: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(feature = "tls")]
            tls: std::sync::OnceLock::new(),
        })
    }
    pub(crate) fn set_prefetched(&self, data: Vec<u8>) {
        if !data.is_empty() {
            *self
                .prefetched
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = data;
            self.has_prefetched.store(true, Ordering::SeqCst);
        }
    }
    #[cfg(feature = "tls")]
    pub(crate) fn start_tls_client(
        &self,
        address: &str,
        config: &super::tls_profile::TlsClientConfig,
        ctx: &Ctx,
    ) -> Result<()> {
        let io = super::tls_conn::client_handshake(self, address, config, ctx)?;
        let _ = self.tls.set(io);
        Ok(())
    }
    #[cfg(feature = "tls")]
    pub(crate) fn start_tls_server(
        &self,
        config: &super::tls_profile::TlsServerConfig,
        ctx: &Ctx,
    ) -> Result<()> {
        let io = super::tls_conn::server_handshake(self, config, ctx)?;
        let _ = self.tls.set(io);
        Ok(())
    }
    #[cfg(feature = "tls")]
    pub(crate) fn tls(&self) -> Option<&super::tls_conn::TlsIo> {
        self.tls.get()
    }
    #[cfg(feature = "tls")]
    pub fn tls_connection_state(&self) -> Option<&super::tls_conn::TlsConnectionState> {
        self.tls.get().map(|io| io.state())
    }
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }
    pub fn effective_socket_buffers(&self) -> Result<super::socket_buffers::SocketBuffers> {
        #[cfg(feature = "tls")]
        if self.tls.get().is_some() {
            return Err(super::socket_buffers::unsupported());
        }
        super::socket_buffers::effective_socket_buffers(&self.stream)
    }
    #[doc(hidden)]
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer
    }
    fn addrs(&self) -> Option<(SocketAddr, SocketAddr)> {
        Some((self.local, self.peer))
    }
    fn closed_error(&self, op: &str) -> Error {
        Error::with_kinds(
            format!(
                "{op} tcp {}->{}: use of closed network connection",
                self.local, self.peer
            ),
            vec![Kind::Closed],
        )
    }
    pub fn read_ctx(&self, buf: &mut [u8], ctx: &Ctx) -> Result<usize> {
        #[cfg(feature = "tls")]
        if let Some(tls) = self.tls.get() {
            return tls.read(self, buf, ctx);
        }
        self.raw_read(buf, ctx)
    }
    pub(crate) fn raw_read(&self, buf: &mut [u8], ctx: &Ctx) -> Result<usize> {
        if self.has_prefetched.load(Ordering::SeqCst) {
            let mut queued = self
                .prefetched
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !queued.is_empty() {
                let n = queued.len().min(buf.len());
                buf[..n].copy_from_slice(&queued[..n]);
                queued.drain(..n);
                if queued.is_empty() {
                    self.has_prefetched.store(false, Ordering::SeqCst);
                }
                return Ok(n);
            }
        }
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return Err(self.closed_error("read"));
            }
            if let Some(e) = ended_context_error(ctx) {
                return Err(e);
            }
            let slice = match ctx.deadline() {
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if ctx.cancels.is_empty() {
                        left
                    } else {
                        left.min(POLL)
                    }
                }
                None if ctx.can_end() => POLL,
                None => POLL * 10,
            };
            let slice = slice.max(Duration::from_millis(1));
            let _ = self.stream.set_read_timeout(Some(slice));
            match (&self.stream).read(buf) {
                Ok(n) => return Ok(n),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    if self.closed.load(Ordering::SeqCst) {
                        return Err(self.closed_error("read"));
                    }
                    if let Some(c) = ended_context_error(ctx) {
                        return Err(c);
                    }
                    return Err(association_context_error(
                        ctx,
                        io_error(&e, "read", self.addrs()),
                    ));
                }
            }
        }
    }
    #[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
    pub(crate) fn write_all_ctx(&self, data: &[u8], ctx: &Ctx) -> Result<()> {
        self.write_counted(data, ctx).1
    }
    pub(crate) fn write_counted(&self, data: &[u8], ctx: &Ctx) -> (usize, Result<()>) {
        #[cfg(feature = "tls")]
        if let Some(tls) = self.tls.get() {
            return tls.write_all(self, data, ctx);
        }
        self.raw_write(data, ctx)
    }
    pub(crate) fn raw_write(&self, mut data: &[u8], ctx: &Ctx) -> (usize, Result<()>) {
        let total = data.len();
        let result = self.raw_write_inner(&mut data, ctx);
        (total - data.len(), result)
    }
    fn raw_write_inner(&self, data: &mut &[u8], ctx: &Ctx) -> Result<()> {
        while !data.is_empty() {
            if self.closed.load(Ordering::SeqCst) {
                return Err(self.closed_error("write"));
            }
            if let Some(e) = ended_context_error(ctx) {
                return Err(e);
            }
            let slice = match ctx.deadline() {
                Some(d) => d.saturating_duration_since(Instant::now()).min(POLL * 10),
                None => POLL * 10,
            }
            .max(Duration::from_millis(1));
            let _ = self.stream.set_write_timeout(Some(slice));
            match (&self.stream).write(data) {
                Ok(0) => return Err(Error::sentinel(Kind::ShortWrite)),
                Ok(n) => *data = &data[n..],
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => {
                    if self.closed.load(Ordering::SeqCst) {
                        return Err(self.closed_error("write"));
                    }
                    if let Some(c) = ended_context_error(ctx) {
                        return Err(c);
                    }
                    return Err(association_context_error(
                        ctx,
                        io_error(&e, "write", self.addrs()),
                    ));
                }
            }
        }
        Ok(())
    }
    pub(crate) fn close_write(&self) {
        #[cfg(feature = "tls")]
        if let Some(tls) = self.tls.get() {
            if !self.closed.load(Ordering::SeqCst) {
                tls.close_notify(self, Duration::from_secs(5));
            }
            return;
        }
        self.close_write_raw();
    }
    pub(crate) fn close_write_raw(&self) {
        let _ = self.stream.shutdown(Shutdown::Write);
    }
    pub fn close(&self) {
        #[cfg(feature = "tls")]
        if let Some(tls) = self.tls.get()
            && !self.closed.load(Ordering::SeqCst)
        {
            tls.close_notify(self, Duration::from_millis(500));
        }
        if !self.closed.swap(true, Ordering::SeqCst) {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }
    #[cfg(feature = "tls")]
    pub(crate) fn drain_raw(&self, timeout: Duration, limit: u64) {
        let ctx = Ctx::background().with_timeout(timeout);
        let mut buf = vec![0u8; 16 * 1024];
        let mut total = 0u64;
        while total < limit {
            match self.raw_read(&mut buf, &ctx) {
                Ok(0) | Err(_) => return,
                Ok(n) => total += n as u64,
            }
        }
    }
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    pub(crate) fn drain(&self, deadline: Instant, limit: u64) {
        while self.pdu_readers.load(Ordering::SeqCst) > 0
            && Instant::now() < deadline
            && !self.closed.load(Ordering::SeqCst)
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        let ctx = Ctx::background().with_deadline(deadline);
        let mut buf = vec![0u8; 32 * 1024];
        let mut total = 0u64;
        while total < limit {
            match self.read_ctx(&mut buf, &ctx) {
                Ok(0) | Err(_) => return,
                Ok(n) => total += n as u64,
            }
        }
    }
}
#[cfg(feature = "net")]
pub(crate) struct PduReadGuard<'a>(&'a Conn);
#[cfg(feature = "net")]
impl<'a> PduReadGuard<'a> {
    pub(crate) fn new(conn: &'a Conn) -> PduReadGuard<'a> {
        conn.pdu_readers.fetch_add(1, Ordering::SeqCst);
        PduReadGuard(conn)
    }
}
#[cfg(feature = "net")]
impl Drop for PduReadGuard<'_> {
    fn drop(&mut self) {
        self.0.pdu_readers.fetch_sub(1, Ordering::SeqCst);
    }
}
#[cfg(feature = "net")]
impl Drop for Conn {
    fn drop(&mut self) {
        self.close();
    }
}
#[cfg(feature = "net")]
pub(crate) struct CtxReader<'a> {
    pub conn: &'a Conn,
    pub ctx: &'a Ctx,
}
#[cfg(feature = "net")]
impl super::reader::ByteRead for CtxReader<'_> {
    fn read_avail(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.conn.read_ctx(buf, self.ctx)
    }
}
