#![forbid(unsafe_code)]
use super::error::Error;
use super::obj::Object;
use super::types::{DataSet, Element, Length, Value, Vr};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
pub const DEFAULT_MAX_BULK_DATA_VALUE_BYTES: i64 = 1 << 30;
pub const DEFAULT_MAX_BULK_DATA_TOTAL_BYTES: i64 = 2 << 30;
pub const ERR_BULK_DATA_URI: &str = "dicom: unsupported or invalid BulkDataURI";
pub const ERR_BULK_DATA_TOO_LARGE: &str = "dicom: Bulk Data value exceeds the configured limit";
fn uri_error(detail: &str) -> Error {
    Error::msg(format!("{ERR_BULK_DATA_URI}{detail}"))
}
fn too_large() -> Error {
    Error::msg(ERR_BULK_DATA_TOO_LARGE)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BulkDataSource {
    pub reader: Option<Vec<u8>>,
    pub size: i64,
}
pub type BulkDataResolver = Rc<dyn Fn(&str) -> Result<BulkDataSource, Error>>;
pub type BulkDataSizeFn = Rc<dyn Fn(&str) -> Result<i64, Error>>;
#[derive(Clone, Default)]
pub struct BulkDataWriteOptions {
    pub resolver: Option<BulkDataResolver>,
    pub size: Option<BulkDataSizeFn>,
}
pub trait BulkDataUriResolver {
    fn resolve_bulk_data_uri(&self, uri: &str, max_bytes: i64) -> Result<BulkDataSource, Error>;
}
impl<F> BulkDataUriResolver for F
where
    F: Fn(&str, i64) -> Result<BulkDataSource, Error>,
{
    fn resolve_bulk_data_uri(&self, uri: &str, max_bytes: i64) -> Result<BulkDataSource, Error> {
        self(uri, max_bytes)
    }
}
#[derive(Clone, Default)]
pub struct BulkDataUriResolvers(pub HashMap<String, Option<Rc<dyn BulkDataUriResolver>>>);
impl BulkDataUriResolver for BulkDataUriResolvers {
    fn resolve_bulk_data_uri(&self, uri: &str, max_bytes: i64) -> Result<BulkDataSource, Error> {
        let parsed = urlrules::parse(uri.trim()).map_err(|()| uri_error(""))?;
        match self.0.get(&to_lower(&parsed.scheme)) {
            Some(Some(resolver)) => resolver.resolve_bulk_data_uri(uri, max_bytes),
            _ => Err(uri_error(": no resolver for the URI scheme")),
        }
    }
}
fn to_lower(s: &str) -> String {
    s.to_lowercase()
}
fn lexical_clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
pub fn file_bulk_data_uri(path: &str, offset: i64, length: i64) -> Result<String, Error> {
    let path = if path.is_empty() { "." } else { path };
    let absolute = std::path::absolute(path).map_err(|e| Error::msg(e.to_string()))?;
    let absolute = lexical_clean(&absolute);
    let mut slashed = absolute.to_string_lossy().into_owned();
    if std::path::MAIN_SEPARATOR != '/' {
        slashed = slashed.replace(std::path::MAIN_SEPARATOR, "/");
    }
    if !slashed.starts_with('/') {
        slashed.insert(0, '/');
    }
    let mut out = String::from("file://");
    out.push_str(&urlrules::escape_path(slashed.as_bytes()));
    if length >= 0 {
        if offset < 0 {
            return Err(uri_error(": negative offset"));
        }
        out.push_str(&format!("?offset={offset}&length={length}"));
    }
    Ok(out)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileBulkDataResolver {
    pub root: String,
    pub max_bytes: i64,
}
fn effective_bulk_data_limit(configured: i64, requested: i64) -> i64 {
    let mut limit = configured;
    if limit <= 0 {
        limit = DEFAULT_MAX_BULK_DATA_VALUE_BYTES;
    }
    if requested > 0 && requested < limit {
        limit = requested;
    }
    limit
}
#[cfg(unix)]
fn os_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}
#[cfg(not(unix))]
fn os_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}
fn equal_fold_ascii(value: &[u8], word: &str) -> bool {
    let Ok(value) = std::str::from_utf8(value) else {
        return false;
    };
    let folded: String = value
        .chars()
        .map(|c| match c {
            '\u{017F}' => 's',
            '\u{212A}' => 'k',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    folded == word
}
fn parse_file_bulk_data_uri(uri: &str) -> Result<(PathBuf, i64, i64), Error> {
    let parsed = urlrules::parse(uri.trim()).map_err(|()| uri_error(""))?;
    if !equal_fold_ascii(parsed.scheme.as_bytes(), "file")
        || !parsed.opaque.is_empty()
        || parsed.user
        || !parsed.fragment.is_empty()
    {
        return Err(uri_error(""));
    }
    if !parsed.host.is_empty() && !equal_fold_ascii(&parsed.host, "localhost") {
        return Err(uri_error(": remote file hosts are not supported"));
    }
    let mut path: &[u8] = &parsed.path;
    if cfg!(windows) && path.len() >= 3 && path[0] == b'/' && path[2] == b':' {
        path = &path[1..];
    }
    let mut native = path.to_vec();
    if std::path::MAIN_SEPARATOR != '/' {
        for b in &mut native {
            if *b == b'/' {
                *b = std::path::MAIN_SEPARATOR as u8;
            }
        }
    }
    let native = os_path(&native);
    if native.as_os_str().is_empty() || !native.is_absolute() {
        return Err(uri_error(": file URIs must be absolute"));
    }
    let (query, failed) = urlrules::parse_query(&parsed.raw_query);
    if failed {
        return Err(uri_error(""));
    }
    let (mut offset, mut length) = (0i64, -1i64);
    for (name, values) in &query {
        if values.len() != 1 {
            return Err(uri_error(": repeated query parameter"));
        }
        let value = std::str::from_utf8(&values[0])
            .ok()
            .and_then(|v| v.parse::<i64>().ok());
        let Some(value) = value.filter(|v| *v >= 0) else {
            return Err(uri_error(
                ": offset and length must be non-negative integers",
            ));
        };
        match name.as_slice() {
            b"offset" => offset = value,
            b"length" => length = value,
            _ => return Err(uri_error(": unknown query parameter")),
        }
    }
    Ok((lexical_clean(&native), offset, length))
}
impl BulkDataUriResolver for FileBulkDataResolver {
    fn resolve_bulk_data_uri(&self, uri: &str, max_bytes: i64) -> Result<BulkDataSource, Error> {
        if self.root.trim().is_empty() {
            return Err(uri_error(": FileBulkDataResolver.Root is required"));
        }
        if self.max_bytes < 0 || max_bytes < 0 {
            return Err(Error::msg("dicom: Bulk Data limits cannot be negative"));
        }
        let limit = effective_bulk_data_limit(self.max_bytes, max_bytes);
        let (path, offset, mut length) = parse_file_bulk_data_uri(uri)?;
        let root = std::fs::canonicalize(&self.root)
            .map_err(|_| uri_error(": resolver root is unavailable"))?;
        let resolved = std::fs::canonicalize(&path)
            .map_err(|_| uri_error(": referenced file is unavailable"))?;
        if Path::new(&self.root).is_relative() || !resolved.starts_with(&root) {
            return Err(uri_error(": referenced file is outside the resolver root"));
        }
        let cannot_open = || uri_error(": referenced file cannot be opened");
        let info = std::fs::metadata(&resolved).map_err(|_| cannot_open())?;
        if !info.is_file() {
            return Err(uri_error(": referenced path is not a regular file"));
        }
        let mut file = std::fs::File::open(&resolved).map_err(|_| cannot_open())?;
        let size = info.len() as i64;
        if offset > size {
            return Err(uri_error(": offset is past the end of the file"));
        }
        if length < 0 {
            length = size - offset;
        }
        if length > size - offset {
            return Err(uri_error(": range is past the end of the file"));
        }
        if length > limit {
            return Err(too_large());
        }
        let mut data = Vec::with_capacity(length as usize);
        let read = file
            .seek(SeekFrom::Start(offset as u64))
            .and_then(|_| file.take(length as u64).read_to_end(&mut data));
        if read.is_err() {
            data.clear();
        }
        Ok(BulkDataSource {
            reader: Some(data),
            size: length,
        })
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResolveBulkDataOptions {
    pub max_value_bytes: i64,
    pub max_total_bytes: i64,
}
struct Resolution<'a> {
    resolver: &'a dyn BulkDataUriResolver,
    max_value: i64,
    max_total: i64,
    total: i64,
}
fn validate_resolved_length(vr: Vr, size: usize) -> Result<(), Error> {
    let width = match vr {
        Vr::AT | Vr::FL | Vr::OF | Vr::SL | Vr::UL | Vr::OL => 4,
        Vr::FD | Vr::OD | Vr::SV | Vr::UV | Vr::OV => 8,
        Vr::SS | Vr::US | Vr::OW => 2,
        _ => 1,
    };
    if !size.is_multiple_of(width) {
        return Err(Error::msg(format!(
            "dicom: resolved {vr} value length {size} is not a multiple of {width}"
        )));
    }
    Ok(())
}
impl Resolution<'_> {
    fn read(&mut self, uri: &str) -> Result<Vec<u8>, Error> {
        let mut limit = self.max_value;
        let remaining = self.max_total - self.total;
        if remaining < limit {
            limit = remaining;
        }
        if limit <= 0 {
            return Err(too_large());
        }
        let source = self.resolver.resolve_bulk_data_uri(uri, limit)?;
        let Some(mut data) = source.reader else {
            return Err(uri_error(": resolver returned a nil Reader"));
        };
        if source.size < 0 {
            return Err(uri_error(": resolver returned a negative size"));
        }
        if source.size > limit {
            return Err(too_large());
        }
        data.truncate((source.size as usize).saturating_add(1));
        if data.len() as i64 != source.size {
            return Err(Error::msg(format!(
                "dicom: Bulk Data source returned {} bytes, declared {}",
                data.len(),
                source.size
            )));
        }
        self.total += source.size;
        Ok(data)
    }
    fn element(&mut self, elem: &Element) -> Result<Option<Element>, Error> {
        match &elem.value {
            Value::BulkData(uri) => {
                let wrap = |e: Error| {
                    Error::wrap(&format!("dicom: resolve Bulk Data for {}", elem.tag()), e)
                };
                let data = self.read(uri).map_err(wrap)?;
                validate_resolved_length(elem.vr(), data.len()).map_err(wrap)?;
                let mut out = elem.clone();
                out.header.length = data.len() as Length;
                out.header.length_set = true;
                out.value = Value::Raw(data);
                Ok(Some(out))
            }
            Value::Sequence(items) => {
                let mut resolved: Option<Vec<DataSet>> = None;
                for (item_index, item) in items.iter().enumerate() {
                    let mut elements: Option<Vec<Element>> = None;
                    for (element_index, nested) in item.elements.iter().enumerate() {
                        if let Some(changed) = self.element(nested)? {
                            elements.get_or_insert_with(|| item.elements.clone())[element_index] =
                                changed;
                        }
                    }
                    if let Some(elements) = elements {
                        resolved.get_or_insert_with(|| items.clone())[item_index].elements =
                            elements;
                    }
                }
                Ok(resolved.map(|items| {
                    let mut out = elem.clone();
                    out.value = Value::Sequence(items);
                    out
                }))
            }
            _ => Ok(None),
        }
    }
}
pub fn resolve_bulk_data(
    obj: &mut Object,
    resolver: Option<&dyn BulkDataUriResolver>,
    opts: ResolveBulkDataOptions,
) -> Result<(), Error> {
    let Some(resolver) = resolver else {
        return Err(uri_error(": a resolver is required"));
    };
    if opts.max_value_bytes < 0 || opts.max_total_bytes < 0 {
        return Err(Error::msg("dicom: Bulk Data limits cannot be negative"));
    }
    let mut state = Resolution {
        resolver,
        max_value: effective_bulk_data_limit(opts.max_value_bytes, 0),
        max_total: if opts.max_total_bytes == 0 {
            DEFAULT_MAX_BULK_DATA_TOTAL_BYTES
        } else {
            opts.max_total_bytes
        },
        total: 0,
    };
    for elem in obj.sorted_elements() {
        if let Some(resolved) = state.element(&elem)? {
            obj.put(resolved);
        }
    }
    Ok(())
}
pub fn new_bulk_data_write_resolver(
    resolver: Option<Rc<dyn BulkDataUriResolver>>,
    max_bytes: i64,
) -> BulkDataResolver {
    let limit = effective_bulk_data_limit(max_bytes, 0);
    Rc::new(move |uri: &str| {
        let Some(resolver) = &resolver else {
            return Err(uri_error(": a resolver is required"));
        };
        let source = resolver.resolve_bulk_data_uri(uri, limit)?;
        if source.size < 0 || source.size > limit {
            return Err(too_large());
        }
        Ok(source)
    })
}
mod urlrules {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Path,
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
    fn unescape(s: &[u8], mode: Mode) -> Result<Vec<u8>, ()> {
        let mut i = 0;
        while i < s.len() {
            match s[i] {
                b'%' => {
                    if i + 2 >= s.len()
                        || !s[i + 1].is_ascii_hexdigit()
                        || !s[i + 2].is_ascii_hexdigit()
                    {
                        return Err(());
                    }
                    if mode == Mode::Host && unhex(s[i + 1]) < 8 && &s[i..i + 3] != b"%25" {
                        return Err(());
                    }
                    if mode == Mode::Zone {
                        let v = unhex(s[i + 1]) << 4 | unhex(s[i + 2]);
                        if &s[i..i + 3] != b"%25" && v != b' ' && should_escape(v, Mode::Host) {
                            return Err(());
                        }
                    }
                    i += 3;
                }
                c => {
                    if matches!(mode, Mode::Host | Mode::Zone) && c < 0x80 && should_escape(c, mode)
                    {
                        return Err(());
                    }
                    i += 1;
                }
            }
        }
        let mut out = Vec::with_capacity(s.len());
        let mut i = 0;
        while i < s.len() {
            match s[i] {
                b'%' => {
                    out.push(unhex(s[i + 1]) << 4 | unhex(s[i + 2]));
                    i += 2;
                }
                b'+' if mode == Mode::QueryComponent => out.push(b' '),
                c => out.push(c),
            }
            i += 1;
        }
        Ok(out)
    }
    pub(super) fn escape_path(s: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut out = String::with_capacity(s.len());
        for &c in s {
            if should_escape(c, Mode::Path) {
                out.push('%');
                out.push(HEX[usize::from(c >> 4)] as char);
                out.push(HEX[usize::from(c & 15)] as char);
            } else {
                out.push(c as char);
            }
        }
        out
    }
    #[derive(Default)]
    pub(super) struct Url {
        pub scheme: String,
        pub opaque: String,
        pub user: bool,
        pub host: Vec<u8>,
        pub path: Vec<u8>,
        pub raw_query: String,
        pub fragment: Vec<u8>,
    }
    fn get_scheme(raw: &str) -> Result<(&str, &str), ()> {
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
                        return Err(());
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
        port.starts_with(':') && port[1..].chars().all(|c| c.is_ascii_digit())
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
    fn parse_addr(s: &[u8]) -> Option<bool> {
        let s = std::str::from_utf8(s).ok()?;
        if let Some(i) = s.find('%') {
            let (addr, zone) = (&s[..i], &s[i + 1..]);
            if zone.is_empty() || !addr.contains(':') {
                return None;
            }
            return addr.parse::<std::net::Ipv6Addr>().ok().map(|_| false);
        }
        if s.contains(':') {
            return s.parse::<std::net::Ipv6Addr>().ok().map(|_| false);
        }
        s.parse::<std::net::Ipv4Addr>().ok().map(|_| true)
    }
    fn parse_host(host: &str) -> Result<Vec<u8>, ()> {
        match host.rfind('[') {
            Some(open) if open > 0 => return Err(()),
            Some(_) => {
                let close = host.rfind(']').ok_or(())?;
                let colon_port = &host[close + 1..];
                if !valid_optional_port(colon_port) {
                    return Err(());
                }
                let port = unescape(colon_port.as_bytes(), Mode::Host)?;
                let hostname = &host[1..close];
                let unescaped = match hostname.find("%25") {
                    Some(zone) => {
                        let mut out = unescape(&hostname.as_bytes()[..zone], Mode::Host)?;
                        out.extend(unescape(&hostname.as_bytes()[zone..], Mode::Zone)?);
                        out
                    }
                    None => unescape(hostname.as_bytes(), Mode::Host)?,
                };
                if parse_addr(&unescaped) != Some(false) {
                    return Err(());
                }
                let mut out = vec![b'['];
                out.extend(unescaped);
                out.push(b']');
                out.extend(port);
                return Ok(out);
            }
            None => {}
        }
        if let Some(last) = host.rfind(':')
            && !valid_optional_port(&host[last..])
        {
            return Err(());
        }
        unescape(host.as_bytes(), Mode::Host)
    }
    fn parse_authority(authority: &str) -> Result<(bool, Vec<u8>), ()> {
        let Some(at) = authority.rfind('@') else {
            return Ok((false, parse_host(authority)?));
        };
        let host = parse_host(&authority[at + 1..])?;
        let userinfo = &authority[..at];
        if !valid_userinfo(userinfo) {
            return Err(());
        }
        match userinfo.split_once(':') {
            None => {
                unescape(userinfo.as_bytes(), Mode::UserPassword)?;
            }
            Some((username, password)) => {
                unescape(username.as_bytes(), Mode::UserPassword)?;
                unescape(password.as_bytes(), Mode::UserPassword)?;
            }
        }
        Ok((true, host))
    }
    pub(super) fn parse(raw: &str) -> Result<Url, ()> {
        let (rest, frag) = raw.split_once('#').unwrap_or((raw, ""));
        if rest.bytes().any(|b| b < b' ' || b == 0x7f) {
            return Err(());
        }
        let mut url = Url::default();
        if rest == "*" {
            url.path = b"*".to_vec();
        } else {
            let (scheme, rest) = get_scheme(rest)?;
            url.scheme = scheme.to_ascii_lowercase();
            let mut rest = rest;
            if rest.ends_with('?') && rest.matches('?').count() == 1 {
                rest = &rest[..rest.len() - 1];
            } else if let Some((before, query)) = rest.split_once('?') {
                rest = before;
                url.raw_query = query.to_string();
            }
            if !rest.starts_with('/') {
                if !url.scheme.is_empty() {
                    url.opaque = rest.to_string();
                    return finish(url, frag);
                }
                let segment = rest.split('/').next().unwrap_or("");
                if segment.contains(':') {
                    return Err(());
                }
            }
            if (!url.scheme.is_empty() || !rest.starts_with("///")) && rest.starts_with("//") {
                let authority = &rest[2..];
                let (authority, path) = match authority.find('/') {
                    Some(i) => (&authority[..i], &authority[i..]),
                    None => (authority, ""),
                };
                (url.user, url.host) = parse_authority(authority)?;
                rest = path;
            }
            url.path = unescape(rest.as_bytes(), Mode::Path)?;
        }
        finish(url, frag)
    }
    fn finish(mut url: Url, frag: &str) -> Result<Url, ()> {
        if !frag.is_empty() {
            url.fragment = unescape(frag.as_bytes(), Mode::Fragment)?;
        }
        Ok(url)
    }
    pub(super) type QueryValues = Vec<(Vec<u8>, Vec<Vec<u8>>)>;
    pub(super) fn parse_query(query: &str) -> (QueryValues, bool) {
        let mut out: std::collections::BTreeMap<Vec<u8>, Vec<Vec<u8>>> = Default::default();
        let mut failed = query.matches('&').count() + 1 > 10000;
        if failed {
            return (Vec::new(), true);
        }
        for pair in query.split('&') {
            if pair.contains(';') {
                failed = true;
                continue;
            }
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let Ok(key) = unescape(key.as_bytes(), Mode::QueryComponent) else {
                failed = true;
                continue;
            };
            let Ok(value) = unescape(value.as_bytes(), Mode::QueryComponent) else {
                failed = true;
                continue;
            };
            out.entry(key).or_default().push(value);
        }
        (out.into_iter().collect(), failed)
    }
}
