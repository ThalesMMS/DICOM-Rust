use crate::textfmt::quote;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Path,
    PathSegment,
    Host,
    Zone,
    UserPassword,
    QueryComponent,
    Fragment,
}
fn should_escape(c: u8, mode: Mode) -> bool {
    if c.is_ascii_alphanumeric() {
        return false;
    }
    if matches!(mode, Mode::Host | Mode::Zone)
        && matches!(
            c,
            b'!' | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'['
                | b']'
                | b'<'
                | b'>'
                | b'"'
        )
    {
        return false;
    }
    match c {
        b'-' | b'_' | b'.' | b'~' => return false,
        b'$' | b'&' | b'+' | b',' | b'/' | b':' | b';' | b'=' | b'?' | b'@' => match mode {
            Mode::Path => return c == b'?',
            Mode::PathSegment => return matches!(c, b'/' | b';' | b',' | b'?'),
            Mode::UserPassword => return matches!(c, b'@' | b'/' | b'?' | b':'),
            Mode::QueryComponent => return true,
            Mode::Fragment => return false,
            Mode::Host | Mode::Zone => {}
        },
        _ => {}
    }
    !(mode == Mode::Fragment && matches!(c, b'!' | b'(' | b')' | b'*'))
}
fn unhex(c: u8) -> u8 {
    9 * (c >> 6) + (c & 15)
}
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
const RAW_BYTE_BASE: u32 = 0x10FF00;
fn from_go_bytes(mut bytes: &[u8]) -> String {
    let raw = |b: u8| char::from_u32(RAW_BYTE_BASE + u32::from(b)).unwrap_or('\u{FFFD}');
    let mut out = String::with_capacity(bytes.len());
    while !bytes.is_empty() {
        let (valid, rest) = match std::str::from_utf8(bytes) {
            Ok(text) => (text, &bytes[bytes.len()..]),
            Err(e) => {
                let (good, bad) = bytes.split_at(e.valid_up_to());
                (std::str::from_utf8(good).unwrap_or(""), bad)
            }
        };
        for c in valid.chars() {
            if u32::from(c) >= RAW_BYTE_BASE {
                let mut buf = [0u8; 4];
                for &b in c.encode_utf8(&mut buf).as_bytes() {
                    out.push(raw(b));
                }
            } else {
                out.push(c);
            }
        }
        if let Some((&b, tail)) = rest.split_first() {
            out.push(raw(b));
            bytes = tail;
        } else {
            bytes = rest;
        }
    }
    out
}
fn to_go_bytes(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        let code = u32::from(c);
        if code >= RAW_BYTE_BASE {
            out.push((code - RAW_BYTE_BASE) as u8);
        } else {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    out
}
fn unescape(s: &str, mode: Mode) -> Result<String, String> {
    let b = s.as_bytes();
    let mut n = 0;
    let mut has_plus = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                n += 1;
                if i + 2 >= b.len()
                    || !b[i + 1].is_ascii_hexdigit()
                    || !b[i + 2].is_ascii_hexdigit()
                {
                    let rest = &b[i..];
                    let rest = &rest[..rest.len().min(3)];
                    return Err(format!("invalid URL escape {}", quote(&lossy(rest))));
                }
                let triple = &b[i..i + 3];
                if mode == Mode::Host && unhex(b[i + 1]) < 8 && triple != b"%25" {
                    return Err(format!("invalid URL escape {}", quote(&lossy(triple))));
                }
                if mode == Mode::Zone {
                    let v = (unhex(b[i + 1]) << 4) | unhex(b[i + 2]);
                    if triple != b"%25" && v != b' ' && should_escape(v, Mode::Host) {
                        return Err(format!("invalid URL escape {}", quote(&lossy(triple))));
                    }
                }
                i += 3;
            }
            b'+' => {
                has_plus = mode == Mode::QueryComponent;
                i += 1;
            }
            c => {
                if matches!(mode, Mode::Host | Mode::Zone) && c < 0x80 && should_escape(c, mode) {
                    return Err(format!(
                        "invalid character {} in host name",
                        quote(&lossy(&b[i..=i]))
                    ));
                }
                i += 1;
            }
        }
    }
    if n == 0 && !has_plus {
        return Ok(s.to_string());
    }
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                out.push((unhex(b[i + 1]) << 4) | unhex(b[i + 2]));
                i += 2;
            }
            b'+' if mode == Mode::QueryComponent => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    Ok(from_go_bytes(&out))
}
fn escape(s: &str, mode: Mode) -> String {
    const UPPERHEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::with_capacity(s.len());
    for c in to_go_bytes(s) {
        if c == b' ' && mode == Mode::QueryComponent {
            out.push(b'+');
        } else if should_escape(c, mode) {
            out.push(b'%');
            out.push(UPPERHEX[usize::from(c >> 4)]);
            out.push(UPPERHEX[usize::from(c & 15)]);
        } else {
            out.push(c);
        }
    }
    lossy(&out)
}
pub fn query_escape(s: &str) -> String {
    escape(s, Mode::QueryComponent)
}
pub fn query_unescape(s: &str) -> Result<String, String> {
    unescape(s, Mode::QueryComponent)
}
pub fn path_escape(s: &str) -> String {
    escape(s, Mode::PathSegment)
}
pub fn path_unescape(s: &str) -> Result<String, String> {
    unescape(s, Mode::PathSegment)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Userinfo {
    pub username: String,
    pub password: String,
    pub password_set: bool,
}
impl Userinfo {
    fn string(&self) -> String {
        let mut s = escape(&self.username, Mode::UserPassword);
        if self.password_set {
            s.push(':');
            s.push_str(&escape(&self.password, Mode::UserPassword));
        }
        s
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub opaque: String,
    pub user: Option<Userinfo>,
    pub host: String,
    pub path: String,
    pub raw_path: String,
    pub omit_host: bool,
    pub force_query: bool,
    pub raw_query: String,
    pub fragment: String,
    pub raw_fragment: String,
}
fn get_scheme(raw: &str) -> Result<(&str, &str), String> {
    for (i, c) in raw.bytes().enumerate() {
        match c {
            b'a'..=b'z' | b'A'..=b'Z' => {}
            b'0'..=b'9' | b'+' | b'-' | b'.' => {
                if i == 0 {
                    return Ok(("", raw));
                }
            }
            b':' => {
                if i == 0 {
                    return Err("missing protocol scheme".into());
                }
                return Ok((&raw[..i], &raw[i + 1..]));
            }
            _ => return Ok(("", raw)),
        }
    }
    Ok(("", raw))
}
fn valid_optional_port(port: &str) -> bool {
    if port.is_empty() {
        return true;
    }
    let b = port.as_bytes();
    b[0] == b':' && b[1..].iter().all(u8::is_ascii_digit)
}
fn valid_userinfo(s: &str) -> bool {
    s.chars().all(|r| {
        r.is_ascii_alphanumeric()
            || matches!(
                r,
                '-' | '.'
                    | '_'
                    | ':'
                    | '~'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | '%'
                    | '@'
            )
    })
}
fn parse_addr(s: &str) -> Option<bool> {
    if s.contains(':') {
        let (addr, zone) = match s.split_once('%') {
            Some((a, z)) => (a, Some(z)),
            None => (s, None),
        };
        if zone == Some("") {
            return None;
        }
        return addr.parse::<std::net::Ipv6Addr>().ok().map(|_| false);
    }
    s.parse::<std::net::Ipv4Addr>().ok().map(|_| true)
}
fn parse_host(scheme: &str, host: &str) -> Result<String, String> {
    match host.rfind('[') {
        Some(open) if open > 0 => return Err("invalid IP-literal".into()),
        Some(_) => {
            let Some(close) = host.rfind(']') else {
                return Err("missing ']' in host".into());
            };
            let colon_port = &host[close + 1..];
            if !valid_optional_port(colon_port) {
                return Err(format!("invalid port {} after host", quote(colon_port)));
            }
            let port = unescape(colon_port, Mode::Host)?;
            let hostname = &host[1..close];
            let unescaped = match hostname.find("%25") {
                Some(zone) => {
                    let mut h = unescape(&hostname[..zone], Mode::Host)?;
                    h.push_str(&unescape(&hostname[zone..], Mode::Zone)?);
                    h
                }
                None => unescape(hostname, Mode::Host)?,
            };
            return match parse_addr(&unescaped) {
                None => Err(format!(
                    "invalid host: ParseAddr({}): unable to parse IP",
                    quote(&unescaped)
                )),
                Some(true) => Err("invalid IP-literal".into()),
                Some(false) => Ok(format!("[{unescaped}]{port}")),
            };
        }
        None => {}
    }
    if let Some(first) = host.find(':') {
        let _ = (scheme, first);
        let i = host.rfind(':').unwrap_or(first);
        let colon_port = &host[i..];
        if !valid_optional_port(colon_port) {
            return Err(format!("invalid port {} after host", quote(colon_port)));
        }
    }
    unescape(host, Mode::Host)
}
fn parse_authority(scheme: &str, authority: &str) -> Result<(Option<Userinfo>, String), String> {
    let Some(at) = authority.rfind('@') else {
        return Ok((None, parse_host(scheme, authority)?));
    };
    let host = parse_host(scheme, &authority[at + 1..])?;
    let userinfo = &authority[..at];
    if !valid_userinfo(userinfo) {
        return Err("net/url: invalid userinfo".into());
    }
    let user = match userinfo.split_once(':') {
        None => Userinfo {
            username: unescape(userinfo, Mode::UserPassword)?,
            ..Userinfo::default()
        },
        Some((name, password)) => Userinfo {
            username: unescape(name, Mode::UserPassword)?,
            password: unescape(password, Mode::UserPassword)?,
            password_set: true,
        },
    };
    Ok((Some(user), host))
}
fn valid_encoded(s: &str, mode: Mode) -> bool {
    s.bytes().all(|c| {
        matches!(
            c,
            b'!' | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
                | b'['
                | b']'
                | b'%'
        ) || !should_escape(c, mode)
    })
}
fn parse_inner(raw: &str, via_request: bool) -> Result<Url, String> {
    if raw.bytes().any(|b| b < b' ' || b == 0x7f) {
        return Err("net/url: invalid control character in URL".into());
    }
    if raw.is_empty() && via_request {
        return Err("empty url".into());
    }
    let mut url = Url::default();
    if raw == "*" {
        url.path = "*".into();
        return Ok(url);
    }
    let (scheme, rest) = get_scheme(raw)?;
    url.scheme = scheme.to_ascii_lowercase();
    let mut rest = rest;
    if rest.ends_with('?') && rest.matches('?').count() == 1 {
        url.force_query = true;
        rest = &rest[..rest.len() - 1];
    } else if let Some((before, query)) = rest.split_once('?') {
        rest = before;
        url.raw_query = query.to_string();
    }
    if !rest.starts_with('/') {
        if !url.scheme.is_empty() {
            url.opaque = rest.to_string();
            return Ok(url);
        }
        if via_request {
            return Err("invalid URI for request".into());
        }
        let segment = rest.split('/').next().unwrap_or("");
        if segment.contains(':') {
            return Err("first path segment in URL cannot contain colon".into());
        }
    }
    if (!url.scheme.is_empty() || (!via_request && !rest.starts_with("///")))
        && rest.starts_with("//")
    {
        let authority = &rest[2..];
        let (authority, path) = match authority.find('/') {
            Some(i) => (&authority[..i], &authority[i..]),
            None => (authority, ""),
        };
        let (user, host) = parse_authority(&url.scheme, authority)?;
        url.user = user;
        url.host = host;
        rest = path;
    } else if !url.scheme.is_empty() && rest.starts_with('/') {
        url.omit_host = true;
    }
    url.set_path(rest)?;
    Ok(url)
}
pub fn parse(raw: &str) -> Result<Url, String> {
    let (u, frag) = match raw.split_once('#') {
        Some((u, frag)) => (u, frag),
        None => (raw, ""),
    };
    let mut url = parse_inner(u, false).map_err(|e| format!("parse {}: {e}", quote(u)))?;
    if frag.is_empty() {
        return Ok(url);
    }
    url.set_fragment(frag)
        .map_err(|e| format!("parse {}: {e}", quote(raw)))?;
    Ok(url)
}
pub fn parse_request_uri(raw: &str) -> Result<Url, String> {
    parse_inner(raw, true).map_err(|e| format!("parse {}: {e}", quote(raw)))
}
fn resolve_path(base: &str, reference: &str) -> String {
    let full = if reference.is_empty() {
        base.to_string()
    } else if !reference.starts_with('/') {
        let i = base.rfind('/').map_or(0, |i| i + 1);
        format!("{}{}", &base[..i], reference)
    } else {
        reference.to_string()
    };
    if full.is_empty() {
        return String::new();
    }
    let mut dst = String::from("/");
    let mut first = true;
    let mut last = "";
    for elem in full.split('/') {
        last = elem;
        if elem == "." {
            first = false;
            continue;
        }
        if elem == ".." {
            let inner = dst[1..].to_string();
            dst.truncate(1);
            match inner.rfind('/') {
                None => first = true,
                Some(index) => dst.push_str(&inner[..index]),
            }
        } else {
            if !first {
                dst.push('/');
            }
            dst.push_str(elem);
            first = false;
        }
    }
    if last == "." || last == ".." {
        dst.push('/');
    }
    if dst.len() > 1 && dst.as_bytes()[1] == b'/' {
        dst.remove(0);
    }
    dst
}
impl Url {
    pub fn set_path(&mut self, p: &str) -> Result<(), String> {
        let path = unescape(p, Mode::Path)?;
        self.raw_path = if escape(&path, Mode::Path) == p {
            String::new()
        } else {
            p.to_string()
        };
        self.path = path;
        Ok(())
    }
    fn set_fragment(&mut self, f: &str) -> Result<(), String> {
        let frag = unescape(f, Mode::Fragment)?;
        self.raw_fragment = if escape(&frag, Mode::Fragment) == f {
            String::new()
        } else {
            f.to_string()
        };
        self.fragment = frag;
        Ok(())
    }
    pub fn escaped_path(&self) -> String {
        if !self.raw_path.is_empty()
            && valid_encoded(&self.raw_path, Mode::Path)
            && unescape(&self.raw_path, Mode::Path).as_deref() == Ok(self.path.as_str())
        {
            return self.raw_path.clone();
        }
        if self.path == "*" {
            return "*".into();
        }
        escape(&self.path, Mode::Path)
    }
    pub fn escaped_fragment(&self) -> String {
        if !self.raw_fragment.is_empty()
            && valid_encoded(&self.raw_fragment, Mode::Fragment)
            && unescape(&self.raw_fragment, Mode::Fragment).as_deref() == Ok(self.fragment.as_str())
        {
            return self.raw_fragment.clone();
        }
        escape(&self.fragment, Mode::Fragment)
    }
    pub fn string(&self) -> String {
        let mut buf = String::new();
        if !self.scheme.is_empty() {
            buf.push_str(&self.scheme);
            buf.push(':');
        }
        if !self.opaque.is_empty() {
            buf.push_str(&self.opaque);
        } else {
            if (!self.scheme.is_empty() || !self.host.is_empty() || self.user.is_some())
                && !(self.omit_host && self.host.is_empty() && self.user.is_none())
            {
                if !self.host.is_empty() || !self.path.is_empty() || self.user.is_some() {
                    buf.push_str("//");
                }
                if let Some(user) = &self.user {
                    buf.push_str(&user.string());
                    buf.push('@');
                }
                if !self.host.is_empty() {
                    buf.push_str(&escape(&self.host, Mode::Host));
                }
            }
            let path = self.escaped_path();
            if !path.is_empty() && !path.starts_with('/') && !self.host.is_empty() {
                buf.push('/');
            }
            if buf.is_empty() && path.split('/').next().unwrap_or("").contains(':') {
                buf.push_str("./");
            }
            buf.push_str(&path);
        }
        if self.force_query || !self.raw_query.is_empty() {
            buf.push('?');
            buf.push_str(&self.raw_query);
        }
        if !self.fragment.is_empty() {
            buf.push('#');
            buf.push_str(&self.escaped_fragment());
        }
        buf
    }
    pub fn request_uri(&self) -> String {
        let mut result = self.opaque.clone();
        if result.is_empty() {
            result = self.escaped_path();
            if result.is_empty() {
                result = "/".into();
            }
        } else if result.starts_with("//") {
            result = format!("{}:{result}", self.scheme);
        }
        if self.force_query || !self.raw_query.is_empty() {
            result.push('?');
            result.push_str(&self.raw_query);
        }
        result
    }
    fn split_host_port(&self) -> (&str, &str) {
        let host = self.host.as_str();
        let (mut name, mut port) = (host, "");
        if let Some(colon) = host.rfind(':')
            && valid_optional_port(&host[colon..])
        {
            name = &host[..colon];
            port = &host[colon + 1..];
        }
        if name.starts_with('[') && name.ends_with(']') {
            name = &name[1..name.len() - 1];
        }
        (name, port)
    }
    pub fn hostname(&self) -> &str {
        self.split_host_port().0
    }
    pub fn port(&self) -> &str {
        self.split_host_port().1
    }
    pub fn resolve_reference(&self, reference: &Url) -> Url {
        let mut url = reference.clone();
        if reference.scheme.is_empty() {
            url.scheme = self.scheme.clone();
        }
        if !reference.scheme.is_empty() || !reference.host.is_empty() || reference.user.is_some() {
            let _ = url.set_path(&resolve_path(&reference.escaped_path(), ""));
            return url;
        }
        if !reference.opaque.is_empty() {
            url.user = None;
            url.host.clear();
            url.path.clear();
            return url;
        }
        if reference.path.is_empty() && !reference.force_query && reference.raw_query.is_empty() {
            url.raw_query = self.raw_query.clone();
            if reference.fragment.is_empty() {
                url.fragment = self.fragment.clone();
                url.raw_fragment = self.raw_fragment.clone();
            }
        }
        if reference.path.is_empty() && !self.opaque.is_empty() {
            url.opaque = self.opaque.clone();
            url.user = None;
            url.host.clear();
            url.path.clear();
            return url;
        }
        url.host = self.host.clone();
        url.user = self.user.clone();
        let _ = url.set_path(&resolve_path(
            &self.escaped_path(),
            &reference.escaped_path(),
        ));
        url
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Values(pub Vec<(String, Vec<String>)>);
impl Values {
    pub fn new() -> Values {
        Values::default()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, key: &str) -> &str {
        self.values(key).first().map_or("", String::as_str)
    }
    pub fn values(&self, key: &str) -> &[String] {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map_or(&[], |(_, v)| v.as_slice())
    }
    pub fn has(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }
    pub fn set(&mut self, key: &str, value: &str) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, values)) => *values = vec![value.to_string()],
            None => self.0.push((key.to_string(), vec![value.to_string()])),
        }
    }
    pub fn add(&mut self, key: &str, value: &str) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, values)) => values.push(value.to_string()),
            None => self.0.push((key.to_string(), vec![value.to_string()])),
        }
    }
    pub fn del(&mut self, key: &str) {
        self.0.retain(|(k, _)| k != key);
    }
    pub fn encode(&self) -> String {
        let mut keys: Vec<&(String, Vec<String>)> = self.0.iter().collect();
        keys.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let mut buf = String::new();
        for (key, values) in keys {
            let escaped = query_escape(key);
            for value in values {
                if !buf.is_empty() {
                    buf.push('&');
                }
                buf.push_str(&escaped);
                buf.push('=');
                buf.push_str(&query_escape(value));
            }
        }
        buf
    }
}
pub fn parse_query(query: &str) -> (Values, Option<String>) {
    let mut values = Values::new();
    if query.matches('&').count() + 1 > 10_000 {
        return (
            values,
            Some("number of URL query parameters exceeded limit".into()),
        );
    }
    let mut err: Option<String> = None;
    for pair in query.split('&') {
        if pair.contains(';') {
            err = Some("invalid semicolon separator in query".into());
            continue;
        }
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = match query_unescape(key) {
            Ok(key) => key,
            Err(e) => {
                err.get_or_insert(e);
                continue;
            }
        };
        match query_unescape(value) {
            Ok(value) => values.add(&key, &value),
            Err(e) => {
                err.get_or_insert(e);
            }
        }
    }
    (values, err)
}
