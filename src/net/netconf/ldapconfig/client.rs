use super::ber::{
    self, CLASS_APPLICATION, FrameError, PAGING_OID, Packet, decode_packet, read_frame,
    validate_envelope,
};
use super::dn::Dn;
use super::model::{Attribute, Entry, admit_entry, ctx_error, map_device, single};
use super::{
    Config, Diagnostic, Endpoint, Error, NORMATIVE_EDITION, Options, Snapshot, normalized,
    validated,
};
use crate::net::ul::Ctx;
use std::collections::HashSet;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
const DEVICE_FILTER: (&str, &str) = ("objectClass", "dicomDevice");
const CHILD_CLASSES: [&str; 3] = [
    "dicomNetworkConnection",
    "dicomNetworkAE",
    "dicomTransferCapability",
];
const ATTRIBUTES: [&str; 14] = [
    "objectClass",
    "dicomDeviceName",
    "dicomDescription",
    "dicomInstalled",
    "dicomHostname",
    "dicomPort",
    "dicomTLSCipherSuite",
    "dicomAETitle",
    "dicomNetworkConnectionReference",
    "dicomAssociationInitiator",
    "dicomAssociationAcceptor",
    "dicomSOPClass",
    "dicomTransferRole",
    "dicomTransferSyntax",
];
enum Stream {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}
impl Stream {
    fn tcp(&self) -> &TcpStream {
        match self {
            Stream::Plain(s) => s,
            #[cfg(feature = "tls")]
            Stream::Tls(s) => &s.sock,
        }
    }
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            #[cfg(feature = "tls")]
            Stream::Tls(s) => s.read(buf),
        }
    }
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            Stream::Plain(s) => s.write_all(data),
            #[cfg(feature = "tls")]
            Stream::Tls(s) => {
                s.write_all(data)?;
                s.flush()
            }
        }
    }
    fn close(&mut self) {
        #[cfg(feature = "tls")]
        if let Stream::Tls(s) = self {
            s.conn.send_close_notify();
            let _ = s.flush();
        }
        let _ = self.tcp().shutdown(std::net::Shutdown::Both);
    }
}
fn arm(tcp: &TcpStream, deadline: Option<Instant>) -> io::Result<()> {
    if let Some(d) = deadline {
        let left = d.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        tcp.set_read_timeout(Some(left))?;
        tcp.set_write_timeout(Some(left))?;
    }
    Ok(())
}
fn read_exact(
    stream: &mut Stream,
    deadline: Option<Instant>,
    mut buf: &mut [u8],
) -> Result<(), FrameError> {
    while !buf.is_empty() {
        arm(stream.tcp(), deadline).map_err(|_| FrameError::Io)?;
        match stream.read(buf) {
            Ok(0) => return Err(FrameError::Io),
            Ok(n) => buf = &mut buf[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(FrameError::Io),
        }
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LdapError {
    Code(u16),
    Other,
}
pub(crate) struct SearchResult {
    pub entries: Vec<Entry>,
    pub referrals: usize,
    pub controls: Vec<Vec<u8>>,
}
pub(crate) struct Session {
    pub ctx: Ctx,
    stream: Stream,
    o: Options,
    used: i64,
    entries: i64,
    failure: Option<FrameError>,
    next_id: i64,
}
fn envelope(id: i64, op: Vec<u8>, controls: Option<Vec<u8>>) -> Vec<u8> {
    let mut body = ber::integer(0x02, id);
    body.extend(op);
    if let Some(c) = controls {
        body.extend(c);
    }
    ber::tlv(0x30, &body)
}
fn octet(v: &[u8]) -> Vec<u8> {
    ber::tlv(0x04, v)
}
pub(crate) fn equality(attr: &str, value: &[u8]) -> Vec<u8> {
    let mut body = octet(attr.as_bytes());
    body.extend(octet(value));
    ber::tlv(0xa3, &body)
}
pub(crate) fn and_filter(parts: &[Vec<u8>]) -> Vec<u8> {
    ber::tlv(0xa0, &parts.concat())
}
fn or_filter(parts: &[Vec<u8>]) -> Vec<u8> {
    ber::tlv(0xa1, &parts.concat())
}
fn ldap_error(p: &Packet) -> Result<(), LdapError> {
    if let Some(r) = p.children.get(1)
        && r.class == CLASS_APPLICATION
        && r.constructed
        && r.children.len() >= 3
    {
        let tag = r.children[0].tag;
        if tag == 2 || tag == 10 {
            let code = r.children[0].int().unwrap_or(0) as u16;
            if code == 0 {
                return Ok(());
            }
            if r.children[1].tag == 4 && r.children[2].tag == 4 {
                return Err(LdapError::Code(code));
            }
        }
    }
    Err(LdapError::Other)
}
fn paging_cookie(control: &Packet) -> Option<Vec<u8>> {
    let value = control.children.last()?;
    let seq = decode_packet(&value.data).ok()?;
    Some(seq.children.get(1)?.data.clone())
}
impl Session {
    fn deadline(&self) -> Option<Instant> {
        self.ctx.deadline()
    }
    fn next_frame(&mut self) -> Result<Vec<u8>, ()> {
        if self.failure.is_some() {
            return Err(());
        }
        let deadline = self.deadline();
        let o = self.o.clone();
        let stream = &mut self.stream;
        let frame = read_frame(
            &mut |buf| read_exact(stream, deadline, buf),
            &o,
            o.max_wire_bytes - self.used,
        );
        let result = frame.and_then(|frame| {
            let op = validate_envelope(&frame, &o).map_err(|e| match e {
                Error::Limit => FrameError::Limit,
                _ => FrameError::Protocol,
            })?;
            if op == 4 {
                self.entries += 1;
                if self.entries > o.max_entries || frame.len() as i64 > o.max_entry_bytes {
                    return Err(FrameError::Limit);
                }
            }
            Ok(frame)
        });
        match result {
            Ok(frame) => {
                self.used += frame.len() as i64;
                Ok(frame)
            }
            Err(e) => {
                self.failure = Some(e);
                Err(())
            }
        }
    }
    fn send(&mut self, op: Vec<u8>, controls: Option<Vec<u8>>) -> Result<i64, LdapError> {
        if self.failure.is_some() {
            return Err(LdapError::Other);
        }
        self.next_id += 1;
        let id = self.next_id;
        let data = envelope(id, op, controls);
        arm(self.stream.tcp(), self.deadline()).map_err(|_| LdapError::Other)?;
        self.stream.write_all(&data).map_err(|_| LdapError::Other)?;
        Ok(id)
    }
    fn response(&mut self, id: i64) -> Result<Packet, LdapError> {
        loop {
            let frame = self.next_frame().map_err(|_| LdapError::Other)?;
            let p = decode_packet(&frame).map_err(|_| LdapError::Other)?;
            if p.children[0].int() == Some(id) {
                return Ok(p);
            }
        }
    }
    pub fn bind(&mut self, name: &str, password: &str) -> Result<(), LdapError> {
        let mut body = ber::integer(0x02, 3);
        body.extend(octet(name.as_bytes()));
        body.extend(ber::tlv(0x80, password.as_bytes()));
        let id = self.send(ber::tlv(0x60, &body), None)?;
        let p = self.response(id)?;
        ldap_error(&p)
    }
    pub fn search(
        &mut self,
        base: &[u8],
        size_limit: i64,
        filter: Vec<u8>,
        attributes: &[&str],
        paging: Option<(i64, &[u8])>,
    ) -> Result<SearchResult, LdapError> {
        let time_limit = (self.o.timeout.0 + 999_999_999) / 1_000_000_000;
        let mut body = octet(base);
        body.extend(ber::integer(0x0a, 2));
        body.extend(ber::integer(0x0a, 0));
        body.extend(ber::integer(0x02, size_limit));
        body.extend(ber::integer(0x02, time_limit));
        body.extend(ber::boolean(false));
        body.extend(filter);
        let attrs: Vec<u8> = attributes
            .iter()
            .flat_map(|a| octet(a.as_bytes()))
            .collect();
        body.extend(ber::tlv(0x30, &attrs));
        let controls = paging.map(|(size, cookie)| {
            let mut value = ber::integer(0x02, size);
            value.extend(octet(cookie));
            let mut control = octet(PAGING_OID.as_bytes());
            control.extend(octet(&ber::tlv(0x30, &value)));
            ber::tlv(0xa0, &ber::tlv(0x30, &control))
        });
        let id = self.send(ber::tlv(0x63, &body), controls)?;
        let mut result = SearchResult {
            entries: Vec::new(),
            referrals: 0,
            controls: Vec::new(),
        };
        loop {
            let p = self.response(id)?;
            let op = &p.children[1];
            match op.tag {
                4 => {
                    let mut entry = Entry {
                        dn: op.children[0].str().unwrap_or_default().to_vec(),
                        attributes: Vec::new(),
                    };
                    if let Some(list) = op.children.get(1) {
                        for attr in &list.children {
                            entry.attributes.push(Attribute {
                                name: attr.children[0].str().unwrap_or_default().to_vec(),
                                values: attr.children[1]
                                    .children
                                    .iter()
                                    .map(|v| v.str().unwrap_or_default().to_vec())
                                    .collect(),
                            });
                        }
                    }
                    result.entries.push(entry);
                }
                5 => {
                    ldap_error(&p)?;
                    if let Some(controls) = p.children.get(2) {
                        for control in &controls.children {
                            result
                                .controls
                                .push(paging_cookie(control).ok_or(LdapError::Other)?);
                        }
                    }
                    return Ok(result);
                }
                19 if op.children.is_empty() => return Err(LdapError::Other),
                19 => result.referrals += 1,
                _ => {}
            }
        }
    }
    pub fn normalize(&self, err: LdapError) -> Error {
        if let Some(e) = ctx_error(&self.ctx) {
            return e;
        }
        match self.failure {
            Some(FrameError::Limit) => return Error::Limit,
            Some(FrameError::Protocol) => return Error::Protocol,
            _ => {}
        }
        match err {
            LdapError::Code(49) => Error::Authentication,
            LdapError::Code(10) => Error::Referral,
            LdapError::Code(4) | LdapError::Code(11) => Error::Limit,
            _ => Error::Transport,
        }
    }
}
fn negotiate_start_tls(
    tcp: &mut TcpStream,
    o: &Options,
    deadline: Option<Instant>,
) -> Result<(), Error> {
    let op = ber::tlv(0x77, &ber::tlv(0x80, b"1.3.6.1.4.1.1466.20037"));
    let request = envelope(1, op, None);
    arm(tcp, deadline).map_err(|_| Error::Transport)?;
    tcp.write_all(&request).map_err(|_| Error::Transport)?;
    let mut stream = Stream::Plain(tcp.try_clone().map_err(|_| Error::Transport)?);
    let frame = read_frame(
        &mut |buf| read_exact(&mut stream, deadline, buf),
        o,
        o.max_wire_bytes.min(16 << 10),
    )
    .map_err(|e| match e {
        FrameError::Limit => Error::Limit,
        _ => Error::Transport,
    })?;
    match validate_envelope(&frame, o) {
        Ok(24) => {}
        _ => return Err(Error::Protocol),
    }
    let p = decode_packet(&frame).map_err(|_| Error::Protocol)?;
    if p.children[0].int() != Some(1) {
        return Err(Error::Protocol);
    }
    if p.children[1].children[0].int() != Some(0) {
        return Err(Error::Transport);
    }
    Ok(())
}
#[cfg(feature = "tls")]
fn tls_client_config(
    c: &Config,
    host: &str,
) -> Result<(std::sync::Arc<rustls::ClientConfig>, String), ()> {
    use rustls::pki_types::CertificateDer;
    use std::sync::Arc;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    let mut name = host.to_string();
    if let Some(t) = &c.tls {
        for der in &t.root_certificates {
            let _ = roots.add(CertificateDer::from(der.clone()));
        }
        if t.system_roots && t.root_certificates.is_empty() {
            roots = crate::net::ul::system_root_store().map_err(|_| ())?.0;
        }
        if !t.server_name.is_empty() {
            name = t.server_name.clone();
        }
    }
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|_| ())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok((Arc::new(config), name))
}
#[cfg(feature = "tls")]
fn handshake(
    mut tcp: TcpStream,
    c: &Config,
    host: &str,
    deadline: Option<Instant>,
) -> Result<Stream, ()> {
    use rustls::pki_types::ServerName;
    let (config, name) = tls_client_config(c, host)?;
    let server = ServerName::try_from(name).map_err(|_| ())?;
    let mut conn = rustls::ClientConnection::new(config, server).map_err(|_| ())?;
    while conn.is_handshaking() {
        arm(&tcp, deadline).map_err(|_| ())?;
        conn.complete_io(&mut tcp).map_err(|_| ())?;
    }
    Ok(Stream::Tls(Box::new(rustls::StreamOwned::new(conn, tcp))))
}
#[cfg(not(feature = "tls"))]
fn handshake(_: TcpStream, _: &Config, _: &str, _: Option<Instant>) -> Result<Stream, ()> {
    Err(())
}
fn dial(ctx: &Ctx, host: &str, port: &str, deadline: Option<Instant>) -> Result<TcpStream, ()> {
    let port: u16 = port.parse().map_err(|_| ())?;
    let address = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let dialer = crate::net::ul::SourcePortDialer {
        timeout: deadline.is_none().then_some(Duration::from_secs(20)),
        ..Default::default()
    };
    dialer.dial(ctx, &address).map_err(|_| ())
}
pub(crate) fn with_session<T>(
    ctx: &Ctx,
    c: &Config,
    endpoint: &Endpoint,
    o: &Options,
    run: impl FnOnce(&mut Session) -> Result<T, Error>,
) -> Result<T, Error> {
    let ctx = ctx.with_timeout(Duration::from_nanos(o.timeout.0 as u64));
    if let Some(e) = ctx_error(&ctx) {
        return Err(e);
    }
    let deadline = ctx.deadline();
    let fail = |e: Error| ctx_error(&ctx).unwrap_or(e);
    let port = match (endpoint.port.as_str(), endpoint.scheme.as_str()) {
        ("", "ldaps") => "636",
        ("", _) => "389",
        (p, _) => p,
    };
    let mut tcp = dial(&ctx, &endpoint.host, port, deadline).map_err(|_| fail(Error::Transport))?;
    let secure = endpoint.scheme == "ldaps" || !c.allow_anonymous_cleartext;
    let stream = if secure {
        if endpoint.scheme == "ldap"
            && let Err(e) = negotiate_start_tls(&mut tcp, o, deadline)
        {
            let _ = tcp.shutdown(std::net::Shutdown::Both);
            return Err(fail(e));
        }
        match handshake(
            tcp.try_clone().map_err(|_| fail(Error::Transport))?,
            c,
            &endpoint.host,
            deadline,
        ) {
            Ok(s) => s,
            Err(()) => {
                let _ = tcp.shutdown(std::net::Shutdown::Both);
                return Err(fail(Error::Transport));
            }
        }
    } else {
        Stream::Plain(tcp)
    };
    let mut session = Session {
        ctx,
        stream,
        o: o.clone(),
        used: 0,
        entries: 0,
        failure: None,
        next_id: 0,
    };
    let result = (|| {
        if !c.bind_dn.is_empty() {
            let (bind, password) = (c.bind_dn.clone(), c.password.clone());
            session
                .bind(&bind, &password)
                .map_err(|e| session.normalize(e))?;
        }
        let value = run(&mut session)?;
        match ctx_error(&session.ctx) {
            Some(e) => Err(e),
            None => Ok(value),
        }
    })();
    session.stream.close();
    result
}
pub fn read(ctx: &Ctx, c: &Config, options: &Options) -> Result<Snapshot, Error> {
    let o = normalized(options.clone())?;
    let (endpoint, root) = validated(c)?;
    with_session(ctx, c, &endpoint, &o, |session| {
        read_devices(session, c.base_dn.as_bytes(), &root, &o)
    })
}
struct Budget {
    pages: i64,
    entries: i64,
    bytes: i64,
}
fn paged_search(
    session: &mut Session,
    budget: &mut Budget,
    base: &[u8],
    filter: &[u8],
    root: &Dn,
    o: &Options,
) -> Result<Vec<Entry>, Error> {
    let mut result = Vec::new();
    let mut cookie: Vec<u8> = Vec::new();
    let size_limit = o.max_entries - budget.entries + 1;
    loop {
        if let Some(e) = ctx_error(&session.ctx) {
            return Err(e);
        }
        budget.pages += 1;
        if budget.pages > o.max_pages {
            return Err(Error::Limit);
        }
        let response = session
            .search(
                base,
                size_limit,
                filter.to_vec(),
                &ATTRIBUTES,
                Some((o.page_size, &cookie)),
            )
            .map_err(|e| session.normalize(e))?;
        if response.referrals > 0 {
            return Err(Error::Referral);
        }
        let progressed = !response.entries.is_empty();
        for entry in response.entries {
            budget.entries += 1;
            if budget.entries > o.max_entries {
                return Err(Error::Limit);
            }
            let weight = admit_entry(&entry, root, o)?;
            if weight > o.max_model_bytes - budget.bytes {
                return Err(Error::Limit);
            }
            budget.bytes += weight;
            result.push(entry);
        }
        let Some(page) = response.controls.into_iter().next() else {
            break;
        };
        if page.len() > 4096 {
            return Err(Error::Protocol);
        }
        if page.is_empty() {
            break;
        }
        if !progressed {
            return Err(Error::Protocol);
        }
        cookie = page;
    }
    Ok(result)
}
fn skipped_device_name(entry: &Entry) -> String {
    single(entry, "dicomDeviceName", true)
        .unwrap_or_else(|_| crate::textfmt::runes(&entry.dn).into_iter().collect())
}
fn read_devices(
    session: &mut Session,
    base: &[u8],
    root: &Dn,
    o: &Options,
) -> Result<Snapshot, Error> {
    let mut budget = Budget {
        pages: 0,
        entries: 0,
        bytes: 0,
    };
    let device = equality(DEVICE_FILTER.0, DEVICE_FILTER.1.as_bytes());
    let filter = if o.device_name.is_empty() {
        device
    } else {
        and_filter(&[
            device,
            equality("dicomDeviceName", o.device_name.as_bytes()),
        ])
    };
    let children_filter = or_filter(
        &CHILD_CLASSES
            .iter()
            .map(|class| equality("objectClass", class.as_bytes()))
            .collect::<Vec<_>>(),
    );
    let devices = paged_search(session, &mut budget, base, &filter, root, o)?;
    if devices.len() as i64 > o.max_devices {
        return Err(Error::Limit);
    }
    let mut snapshot = Snapshot {
        edition: NORMATIVE_EDITION.to_string(),
        ..Snapshot::default()
    };
    let mut seen = HashSet::new();
    for entry in &devices {
        if let Some(e) = ctx_error(&session.ctx) {
            return Err(e);
        }
        match super::dn::dn_key(&entry.dn) {
            Ok(key) if seen.insert(key.clone()) => {}
            _ => return Err(Error::Invalid),
        }
        if !o.device_name.is_empty() {
            match single(entry, "dicomDeviceName", true) {
                Ok(name) if name == o.device_name => {}
                _ => return Err(Error::Invalid),
            }
        }
        let children = paged_search(session, &mut budget, &entry.dn, &children_filter, root, o)?;
        let device = match map_device(&session.ctx, entry, &children, o) {
            Ok(device) => device,
            Err(err) => {
                let invalid = match &err {
                    Error::Invalid => true,
                    Error::Netconf(e) => e.is(crate::net::netconf::Cause::Invalid),
                    _ => false,
                };
                if ctx_error(&session.ctx).is_some() || !invalid {
                    return Err(err);
                }
                if !o.device_name.is_empty() {
                    return Err(Error::Invalid);
                }
                snapshot.skipped.push(Diagnostic {
                    code: "device-invalid".into(),
                    object: skipped_device_name(entry),
                });
                continue;
            }
        };
        if !device.model.application_entities.is_empty() {
            snapshot.devices.push(device);
        } else if device.model.schema_version == 0 {
            snapshot.skipped.push(Diagnostic {
                code: "application-entities-absent".into(),
                object: skipped_device_name(entry),
            });
        }
    }
    if let Some(e) = ctx_error(&session.ctx) {
        return Err(e);
    }
    Ok(snapshot)
}
#[cfg(all(test, feature = "tls"))]
mod system_roots_tests {
    use super::super::TlsConfig;
    use super::*;
    #[test]
    fn builds_the_configuration_with_the_system_roots() {
        let mut c = Config {
            url: "ldaps://ldap.example.org".into(),
            tls: Some(TlsConfig {
                system_roots: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let (_, name) = tls_client_config(&c, "ldap.example.org").expect("configuration");
        assert_eq!(name, "ldap.example.org");
        if let Some(t) = c.tls.as_mut() {
            t.server_name = "directory.example.org".into();
        }
        let (_, name) = tls_client_config(&c, "ldap.example.org").expect("configuration");
        assert_eq!(name, "directory.example.org");
        c.tls = None;
        assert!(tls_client_config(&c, "ldap.example.org").is_ok());
    }
}
#[cfg(test)]
mod source_port_tests {
    use super::*;
    use std::io::Read;
    use std::net::{SocketAddr, TcpListener};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[test]
    fn session_dial_retries_a_source_port_collision() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server: SocketAddr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
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
        let mut conn = TcpStream::connect(server).unwrap();
        let local = conn.local_addr().unwrap();
        conn.shutdown(std::net::Shutdown::Write).unwrap();
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let mut rest = Vec::new();
        let _ = conn.read_to_end(&mut rest);
        drop(conn);
        let probe = crate::net::ul::SourcePortDialer {
            local_address: Some(local),
            timeout: Some(Duration::from_secs(5)),
            ..Default::default()
        }
        .dial(&Ctx::background(), &server.to_string());
        if probe.is_ok() {
            eprintln!("this system lets a new connection reuse an endpoint in TIME_WAIT");
            return;
        }
        let config = Config {
            url: format!("ldap://{server}"),
            base_dn: "dc=example,dc=org".into(),
            allow_anonymous_cleartext: true,
            ..Default::default()
        };
        let options = Options {
            timeout: crate::net::ul::Nanos::from_secs(3),
            ..Default::default()
        };
        let (result, collided) = crate::net::ul::with_source_port_collision(local, || {
            read(&Ctx::background(), &config, &options)
        });
        assert!(result.is_err());
        assert!(collided, "the session dial did not meet the collision");
        let deadline = Instant::now() + Duration::from_secs(2);
        while accepted.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(accepted.load(Ordering::SeqCst), 2);
    }
}
