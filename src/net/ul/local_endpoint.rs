use super::conn::Ctx;
use super::error::Kind;
use super::socket_buffers::SocketBuffers;
use socket2::{Domain, Protocol, Socket, Type};
use std::cell::RefCell;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalEndpointKind {
    NotLocalEndpoint,
    AddressInUse,
    AddressNotAvailable,
    NoBuffers,
}
#[cfg(windows)]
const NO_BUFFERS: Option<i32> = Some(10055);
#[cfg(any(target_os = "linux", target_os = "android"))]
const NO_BUFFERS: Option<i32> = Some(105);
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
))]
const NO_BUFFERS: Option<i32> = Some(55);
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)))]
const NO_BUFFERS: Option<i32> = None;
#[doc(hidden)]
pub fn classify_local_endpoint_error(err: &io::Error) -> LocalEndpointKind {
    let Some(code) = err.raw_os_error() else {
        return LocalEndpointKind::NotLocalEndpoint;
    };
    if Some(code) == NO_BUFFERS {
        return LocalEndpointKind::NoBuffers;
    }
    match err.kind() {
        io::ErrorKind::AddrInUse => LocalEndpointKind::AddressInUse,
        io::ErrorKind::AddrNotAvailable => LocalEndpointKind::AddressNotAvailable,
        _ => LocalEndpointKind::NotLocalEndpoint,
    }
}
pub fn local_endpoint_unavailable(err: &io::Error) -> bool {
    classify_local_endpoint_error(err) != LocalEndpointKind::NotLocalEndpoint
}
impl LocalEndpointKind {
    pub(crate) fn marker(self) -> Option<Kind> {
        match self {
            LocalEndpointKind::NotLocalEndpoint => None,
            LocalEndpointKind::AddressInUse => Some(Kind::AddressInUse),
            LocalEndpointKind::AddressNotAvailable => Some(Kind::AddressNotAvailable),
            LocalEndpointKind::NoBuffers => Some(Kind::NoBufferSpace),
        }
    }
}
#[doc(hidden)]
pub const SOURCE_PORT_RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(10),
    Duration::from_millis(50),
    Duration::from_millis(100),
];
fn split_host_port(address: &str) -> Option<(&str, &str)> {
    let i = address.rfind(':')?;
    let (host, port) = (&address[..i], &address[i + 1..]);
    if let Some(rest) = address.strip_prefix('[') {
        let end = rest.find(']')?;
        if end + 2 != i {
            return None;
        }
        return Some((&rest[..end], port));
    }
    if host.contains(':') {
        return None;
    }
    Some((host, port))
}
#[doc(hidden)]
pub fn valid_remote_address(destination: &str) -> bool {
    let Some((host, port)) = split_host_port(destination) else {
        return false;
    };
    let digits = port.strip_prefix(['+', '-']).unwrap_or(port);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if !matches!(port.parse::<i64>(), Ok(n) if n > 0 && n <= 65535) {
        return false;
    }
    !matches!(host.parse::<IpAddr>(), Ok(ip) if ip.is_unspecified())
}
#[doc(hidden)]
pub fn source_port_retryable(
    kind: LocalEndpointKind,
    local: Option<&SocketAddr>,
    destination: &str,
) -> bool {
    if local.is_some_and(|l| l.port() != 0) {
        return false;
    }
    match kind {
        LocalEndpointKind::AddressInUse | LocalEndpointKind::NoBuffers => true,
        LocalEndpointKind::AddressNotAvailable => {
            local.is_none_or(|l| l.ip().is_unspecified()) && valid_remote_address(destination)
        }
        LocalEndpointKind::NotLocalEndpoint => false,
    }
}
fn pause(ctx: &Ctx, delay: Duration) -> bool {
    let until = Instant::now() + delay;
    loop {
        if ctx.err().is_some() {
            return false;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        std::thread::sleep(left.min(Duration::from_millis(5)));
    }
}
#[doc(hidden)]
pub fn retry_unavailable_source_port<T, E>(
    ctx: &Ctx,
    local: Option<&SocketAddr>,
    destination: &str,
    kind_of: impl Fn(&E) -> LocalEndpointKind,
    mut dial: impl FnMut() -> Result<T, E>,
) -> Result<T, E> {
    let mut attempt = 0;
    loop {
        match dial() {
            Ok(value) => return Ok(value),
            Err(err) => {
                if attempt >= SOURCE_PORT_RETRY_DELAYS.len()
                    || !source_port_retryable(kind_of(&err), local, destination)
                    || !pause(ctx, SOURCE_PORT_RETRY_DELAYS[attempt])
                {
                    return Err(err);
                }
                attempt += 1;
            }
        }
    }
}
pub(crate) fn connect_socket(
    addr: &SocketAddr,
    local: Option<&SocketAddr>,
    buffers: &SocketBuffers,
    timeout: Option<Duration>,
) -> io::Result<TcpStream> {
    if local.is_none() && buffers.is_zero() && !(cfg!(windows) && addr.ip().is_loopback()) {
        return match timeout {
            Some(t) => TcpStream::connect_timeout(addr, t),
            None => TcpStream::connect(addr),
        };
    }
    if let Some(l) = local
        && l.is_ipv4() != addr.is_ipv4()
        && !l.ip().is_unspecified()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mismatched local address type",
        ));
    }
    let socket = Socket::new(
        Domain::for_address(*addr),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    buffers.control(&socket)?;
    if let Some(l) = local {
        let bind = if l.is_ipv4() == addr.is_ipv4() {
            *l
        } else if addr.is_ipv4() {
            SocketAddr::from(([0, 0, 0, 0], l.port()))
        } else {
            SocketAddr::from(([0u16; 8], l.port()))
        };
        socket.bind(&bind.into())?;
    }
    #[cfg(windows)]
    if addr.ip().is_loopback() {
        fail_fast_on_loopback(&socket);
    }
    match timeout {
        Some(t) => socket.connect_timeout(&(*addr).into(), t)?,
        None => socket.connect(&(*addr).into())?,
    }
    Ok(socket.into())
}
#[cfg(windows)]
#[allow(unsafe_code)]
fn fail_fast_on_loopback(socket: &Socket) {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        SIO_TCP_INITIAL_RTO, TCP_INITIAL_RTO_PARAMETERS, WSAIoctl,
    };
    for retransmissions in [0xFEu8, 1] {
        let params = TCP_INITIAL_RTO_PARAMETERS {
            Rtt: u16::MAX,
            MaxSynRetransmissions: retransmissions,
        };
        let mut out = 0u32;
        let status = unsafe {
            WSAIoctl(
                socket.as_raw_socket() as usize,
                SIO_TCP_INITIAL_RTO,
                std::ptr::from_ref(&params).cast(),
                size_of::<TCP_INITIAL_RTO_PARAMETERS>() as u32,
                std::ptr::null_mut(),
                0,
                &mut out,
                std::ptr::null_mut(),
                None,
            )
        };
        if status == 0 {
            return;
        }
    }
}
pub(crate) fn listen_socket(addr: &SocketAddr, buffers: &SocketBuffers) -> io::Result<TcpListener> {
    if buffers.is_zero() {
        return TcpListener::bind(addr);
    }
    let socket = Socket::new(
        Domain::for_address(*addr),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    buffers.control(&socket)?;
    socket.bind(&(*addr).into())?;
    socket.listen(1024)?;
    Ok(socket.into())
}
struct FirstAttempt {
    local: SocketAddr,
    collided: bool,
}
thread_local! {
    static FIRST_ATTEMPT: RefCell<Option<FirstAttempt>> = const { RefCell::new(None) };
}
pub fn with_source_port_collision<T>(local: SocketAddr, f: impl FnOnce() -> T) -> (T, bool) {
    FIRST_ATTEMPT.with(|slot| {
        *slot.borrow_mut() = Some(FirstAttempt {
            local,
            collided: false,
        });
    });
    let value = f();
    let collided = FIRST_ATTEMPT.with(|slot| slot.borrow_mut().take().is_some_and(|f| f.collided));
    (value, collided)
}
#[derive(Debug, Clone, Default)]
pub struct SourcePortDialer {
    pub local_address: Option<SocketAddr>,
    pub timeout: Option<Duration>,
    pub socket_buffers: SocketBuffers,
}
impl SourcePortDialer {
    pub fn dial(&self, ctx: &Ctx, address: &str) -> io::Result<TcpStream> {
        let started = Instant::now();
        let mut attempt = 0;
        retry_unavailable_source_port(
            ctx,
            self.local_address.as_ref(),
            address,
            classify_local_endpoint_error,
            || {
                attempt += 1;
                let forced = if attempt == 1 && self.local_address.is_none() {
                    FIRST_ATTEMPT.with(|slot| slot.borrow().as_ref().map(|f| f.local))
                } else {
                    None
                };
                let local = forced.or(self.local_address);
                let result = self.dial_once(ctx, address, local.as_ref(), started);
                if forced.is_some()
                    && let Err(err) = &result
                    && local_endpoint_unavailable(err)
                {
                    FIRST_ATTEMPT.with(|slot| {
                        if let Some(f) = slot.borrow_mut().as_mut() {
                            f.collided = true;
                        }
                    });
                }
                result
            },
        )
    }
    fn dial_once(
        &self,
        ctx: &Ctx,
        address: &str,
        local: Option<&SocketAddr>,
        started: Instant,
    ) -> io::Result<TcpStream> {
        if ctx.err().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "dial context ended",
            ));
        }
        let mut deadline = ctx.deadline();
        if let Some(t) = self.timeout {
            let own = started + t;
            deadline = Some(deadline.map_or(own, |d| d.min(own)));
        }
        let mut last: Option<io::Error> = None;
        for addr in address.to_socket_addrs()? {
            let left = match deadline {
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "i/o timeout"));
                    }
                    Some(left)
                }
                None => None,
            };
            match connect_socket(&addr, local, &self.socket_buffers, left) {
                Ok(stream) => return Ok(stream),
                Err(err) => last = Some(err),
            }
        }
        Err(last.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no address")))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn os(code_kind: io::ErrorKind) -> io::Error {
        #[cfg(windows)]
        let code = match code_kind {
            io::ErrorKind::AddrInUse => 10048,
            io::ErrorKind::AddrNotAvailable => 10049,
            _ => 10061,
        };
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let code = match code_kind {
            io::ErrorKind::AddrInUse => 98,
            io::ErrorKind::AddrNotAvailable => 99,
            _ => 111,
        };
        #[cfg(not(any(windows, target_os = "linux", target_os = "android")))]
        let code = match code_kind {
            io::ErrorKind::AddrInUse => 48,
            io::ErrorKind::AddrNotAvailable => 49,
            _ => 61,
        };
        io::Error::from_raw_os_error(code)
    }
    #[test]
    fn classifies_only_operating_system_errors() {
        use LocalEndpointKind as K;
        assert_eq!(
            classify_local_endpoint_error(&os(io::ErrorKind::AddrInUse)),
            K::AddressInUse
        );
        assert_eq!(
            classify_local_endpoint_error(&os(io::ErrorKind::AddrNotAvailable)),
            K::AddressNotAvailable
        );
        assert_eq!(
            classify_local_endpoint_error(&os(io::ErrorKind::ConnectionRefused)),
            K::NotLocalEndpoint
        );
        let synthetic = io::Error::new(io::ErrorKind::AddrInUse, "in use");
        assert_eq!(
            classify_local_endpoint_error(&synthetic),
            K::NotLocalEndpoint
        );
        if let Some(code) = NO_BUFFERS {
            let err = io::Error::from_raw_os_error(code);
            assert_eq!(classify_local_endpoint_error(&err), K::NoBuffers);
            assert!(local_endpoint_unavailable(&err));
        }
    }
    #[test]
    fn retryable_follows_the_configured_endpoint() {
        use LocalEndpointKind as K;
        let dest = "127.0.0.1:104";
        let port: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let ip: SocketAddr = "192.0.2.1:0".parse().unwrap();
        let any: SocketAddr = "0.0.0.0:0".parse().unwrap();
        assert!(source_port_retryable(K::AddressInUse, None, dest));
        assert!(source_port_retryable(K::NoBuffers, None, dest));
        assert!(source_port_retryable(K::AddressInUse, Some(&ip), dest));
        assert!(!source_port_retryable(K::AddressInUse, Some(&port), dest));
        assert!(!source_port_retryable(K::NoBuffers, Some(&port), dest));
        assert!(source_port_retryable(K::AddressNotAvailable, None, dest));
        assert!(source_port_retryable(
            K::AddressNotAvailable,
            Some(&any),
            dest
        ));
        assert!(!source_port_retryable(
            K::AddressNotAvailable,
            Some(&ip),
            dest
        ));
        assert!(!source_port_retryable(
            K::AddressNotAvailable,
            None,
            "127.0.0.1:0"
        ));
        assert!(!source_port_retryable(
            K::AddressNotAvailable,
            None,
            "0.0.0.0:104"
        ));
        assert!(!source_port_retryable(K::NotLocalEndpoint, None, dest));
    }
    #[test]
    fn valid_remote_addresses() {
        for ok in [
            "127.0.0.1:104",
            "example.org:11112",
            "[::1]:104",
            "host:65535",
        ] {
            assert!(valid_remote_address(ok), "{ok}");
        }
        for bad in [
            "127.0.0.1:0",
            "0.0.0.0:104",
            "[::]:104",
            "host",
            "host:",
            "host:x",
            "host:65536",
            "host:-1",
            "::1:104",
        ] {
            assert!(!valid_remote_address(bad), "{bad}");
        }
    }
    #[test]
    fn retries_an_unavailable_source_port_a_few_times() {
        let ctx = Ctx::background();
        let attempts = Cell::new(0);
        let result = retry_unavailable_source_port(
            &ctx,
            None,
            "127.0.0.1:104",
            classify_local_endpoint_error,
            || {
                attempts.set(attempts.get() + 1);
                if attempts.get() < 3 {
                    Err(os(io::ErrorKind::AddrInUse))
                } else {
                    Ok(attempts.get())
                }
            },
        );
        assert_eq!(result.unwrap(), 3);
        attempts.set(0);
        let result: io::Result<()> = retry_unavailable_source_port(
            &ctx,
            None,
            "127.0.0.1:104",
            classify_local_endpoint_error,
            || {
                attempts.set(attempts.get() + 1);
                Err(os(io::ErrorKind::AddrInUse))
            },
        );
        assert!(local_endpoint_unavailable(&result.unwrap_err()));
        assert_eq!(attempts.get(), SOURCE_PORT_RETRY_DELAYS.len() + 1);
    }
    #[test]
    fn does_not_retry_what_another_attempt_cannot_change() {
        let ctx = Ctx::background();
        let port: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        for (kind, local, dest) in [
            (io::ErrorKind::AddrInUse, Some(&port), "127.0.0.1:104"),
            (io::ErrorKind::ConnectionRefused, None, "127.0.0.1:104"),
            (io::ErrorKind::AddrNotAvailable, None, "127.0.0.1:0"),
        ] {
            let attempts = Cell::new(0);
            let result: io::Result<()> = retry_unavailable_source_port(
                &ctx,
                local,
                dest,
                classify_local_endpoint_error,
                || {
                    attempts.set(attempts.get() + 1);
                    Err(os(kind))
                },
            );
            assert!(result.is_err());
            assert_eq!(attempts.get(), 1, "{kind:?} {dest}");
        }
    }
    #[test]
    fn an_ended_context_stops_the_retry() {
        let token = super::super::conn::CancelToken::new();
        token.cancel();
        let ctx = Ctx::background().with_cancel(&token);
        let attempts = Cell::new(0);
        let result: io::Result<()> = retry_unavailable_source_port(
            &ctx,
            None,
            "127.0.0.1:104",
            classify_local_endpoint_error,
            || {
                attempts.set(attempts.get() + 1);
                Err(os(io::ErrorKind::AddrInUse))
            },
        );
        assert!(result.is_err());
        assert_eq!(attempts.get(), 1);
    }
    fn collision_server() -> (SocketAddr, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::Read;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { return };
                counter.fetch_add(1, Ordering::SeqCst);
                std::thread::spawn(move || {
                    let _ = conn.set_read_timeout(Some(Duration::from_millis(300)));
                    let mut buf = [0u8; 1024];
                    while matches!(conn.read(&mut buf), Ok(n) if n > 0) {}
                });
            }
        });
        (addr, accepted)
    }
    fn collision(server: &SocketAddr) -> Option<SocketAddr> {
        use std::io::Read;
        let mut conn = TcpStream::connect(server).unwrap();
        let local = conn.local_addr().unwrap();
        conn.shutdown(std::net::Shutdown::Write).unwrap();
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let mut rest = Vec::new();
        let _ = conn.read_to_end(&mut rest);
        drop(conn);
        let probe = connect_socket(
            server,
            Some(&local),
            &SocketBuffers::default(),
            Some(Duration::from_secs(5)),
        );
        match probe {
            Ok(_) => None,
            Err(err) => {
                assert!(
                    local_endpoint_unavailable(&err),
                    "a connection from an endpoint in TIME_WAIT failed with {err}"
                );
                Some(local)
            }
        }
    }
    #[test]
    fn dial_retries_a_real_source_port_collision() {
        let (server, accepted) = collision_server();
        let Some(local) = collision(&server) else {
            eprintln!("this system lets a new connection reuse an endpoint in TIME_WAIT");
            return;
        };
        let dialer = SourcePortDialer {
            timeout: Some(Duration::from_secs(5)),
            ..Default::default()
        };
        let ctx = Ctx::background();
        let (dialed, collided) =
            with_source_port_collision(local, || dialer.dial(&ctx, &server.to_string()));
        assert!(collided, "the first attempt did not meet the collision");
        let stream = dialed.expect("the dial tries again from a fresh port");
        assert_ne!(stream.local_addr().unwrap(), local);
        let deadline = Instant::now() + Duration::from_secs(2);
        while accepted.load(std::sync::atomic::Ordering::SeqCst) < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 2);
        let fixed = SourcePortDialer {
            local_address: Some(local),
            timeout: Some(Duration::from_secs(5)),
            ..Default::default()
        };
        let started = Instant::now();
        let (dialed, collided) =
            with_source_port_collision(local, || fixed.dial(&ctx, &server.to_string()));
        let err = dialed.expect_err("a configured source port in TIME_WAIT fails");
        assert!(local_endpoint_unavailable(&err));
        assert!(!collided);
        assert!(
            started.elapsed() < Duration::from_millis(150),
            "the dial was retried"
        );
    }
    #[test]
    fn binds_before_connect() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let local: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let buffers = SocketBuffers {
            send: 256 << 10,
            receive: 192 << 10,
        };
        let stream =
            connect_socket(&addr, Some(&local), &buffers, Some(Duration::from_secs(5))).unwrap();
        assert!(stream.local_addr().unwrap().ip().is_loopback());
        let effective = super::super::socket_buffers::effective_socket_buffers(&stream).unwrap();
        assert!(effective.send >= buffers.send && effective.receive >= buffers.receive);
        let (accepted, peer) = listener.accept().unwrap();
        assert_eq!(peer, stream.local_addr().unwrap());
        drop(accepted);
    }
}
