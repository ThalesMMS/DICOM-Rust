use super::Error;
use super::imaging_study::ImagingStudy;
use super::search_page::decode_page;
use super::smart::check_smart_scopes;
use crate::net::dicomweb::HttpsOptions;
use crate::net::dicomweb::http::{HttpRequest, round_trip};
use crate::net::dicomweb::mimeheader::parse_media_type;
use crate::net::dicomweb::multipart::{ByteRead, ReadErr, SliceReader};
use crate::net::dicomweb::urlrules::{self, Url, Values};
use crate::net::ul::{Ctx, Kind};
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
const MAX_SEARCH_URL_BYTES: usize = 32 << 10;
const MAX_SEARCH_FORM_BYTES: usize = 64 << 10;
const DEFAULT_MAX_PAGES: i64 = 32;
const DEFAULT_PAGE_BYTES: i64 = 8 << 20;
const DEFAULT_TOTAL_BYTES: i64 = 32 << 20;
const MAX_PAGE_BYTES: i64 = 64 << 20;
const MAX_TOTAL_BYTES: i64 = 128 << 20;
const MAX_PAGE_COUNT: i64 = 256;
const IMAGING_STUDY_TYPE: &str = "ImagingStudy";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SmartGrant {
    pub access_token: String,
    pub audience: String,
    pub scopes: String,
    pub context: String,
}
pub type AuthorizeFn = Arc<dyn Fn(&Ctx) -> Result<SmartGrant, crate::BoxError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct ClientOptions {
    pub timeout: Duration,
    pub max_pages: i64,
    pub max_page_bytes: i64,
    pub max_total_bytes: i64,
    pub root_certificates_der: Option<Vec<Vec<u8>>>,
    pub allow_loopback_http: bool,
    pub authorize: Option<AuthorizeFn>,
}
impl fmt::Debug for ClientOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientOptions")
            .field("timeout", &self.timeout)
            .field("max_pages", &self.max_pages)
            .field("max_page_bytes", &self.max_page_bytes)
            .field("max_total_bytes", &self.max_total_bytes)
            .field(
                "root_certificates_der",
                &self.root_certificates_der.as_ref().map(Vec::len),
            )
            .field("allow_loopback_http", &self.allow_loopback_http)
            .field("authorize", &self.authorize.is_some())
            .finish()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImagingSearch {
    pub parameters: Values,
    pub by_post: bool,
}
#[derive(Clone, Debug)]
pub struct Client {
    base: Url,
    https: HttpsOptions,
    options: ClientOptions,
}
fn is_loopback_literal(host: &str) -> bool {
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(a)) => a.is_loopback(),
        Ok(std::net::IpAddr::V6(a)) => a
            .to_ipv4_mapped()
            .map_or(a.is_loopback(), |v4| v4.is_loopback()),
        Err(_) => false,
    }
}
impl Client {
    pub fn new(base_url: &str, options: ClientOptions) -> Result<Client, Error> {
        let base = parse_base(base_url)?;
        match base.scheme.as_str() {
            "https" => {}
            "http" if options.allow_loopback_http && is_loopback_literal(base.hostname()) => {}
            _ => return Err(Error::Endpoint),
        }
        let mut options = options;
        if options.max_pages < 0
            || options.max_pages > MAX_PAGE_COUNT
            || options.max_page_bytes < 0
            || options.max_page_bytes > MAX_PAGE_BYTES
            || options.max_total_bytes < 0
            || options.max_total_bytes > MAX_TOTAL_BYTES
        {
            return Err(Error::Options);
        }
        if options.timeout.is_zero() {
            options.timeout = Duration::from_secs(30);
        }
        if options.max_pages == 0 {
            options.max_pages = DEFAULT_MAX_PAGES;
        }
        if options.max_page_bytes == 0 {
            options.max_page_bytes = DEFAULT_PAGE_BYTES;
        }
        if options.max_total_bytes == 0 {
            options.max_total_bytes = DEFAULT_TOTAL_BYTES;
        }
        let https = HttpsOptions {
            root_certificates_der: options.root_certificates_der.clone().unwrap_or_default(),
            system_roots: options.root_certificates_der.is_none(),
            insecure_skip_verify: false,
        };
        Ok(Client {
            base,
            https,
            options,
        })
    }
    pub fn search_imaging_studies(
        &self,
        ctx: &Ctx,
        search: &ImagingSearch,
    ) -> Result<Vec<ImagingStudy>, Error> {
        let ctx = ctx.with_timeout(self.options.timeout);
        let mut target = self.base.clone();
        target.path.push('/');
        target.path.push_str(IMAGING_STUDY_TYPE);
        let query = encode_search(&search.parameters)?;
        let mut body: Option<Vec<u8>> = None;
        if search.by_post {
            target.path.push_str("/_search");
            body = Some(query.into_bytes());
        } else {
            target.raw_query = query;
        }
        let mut results = Vec::new();
        let mut used: HashSet<String> = HashSet::new();
        let mut total_bytes: i64 = 0;
        let mut page: i64 = 0;
        loop {
            context(&ctx)?;
            if page >= self.options.max_pages {
                return Err(Error::PageLimit);
            }
            if total_bytes >= self.options.max_total_bytes {
                return Err(Error::Limit);
            }
            let by_post = page == 0 && search.by_post;
            if !by_post && !used.insert(target.string()) {
                return Err(Error::PageLoop);
            }
            let limit = self
                .options
                .max_page_bytes
                .min(self.options.max_total_bytes - total_bytes);
            let data = self.search_page(&ctx, &target, by_post, body.as_deref(), limit)?;
            total_bytes += data.len() as i64;
            let (studies, next) = decode_page(&ctx, &data)?;
            results.extend(studies);
            if next.is_empty() {
                return Ok(results);
            }
            target = self.resolve_next(&target, &next)?;
            body = None;
            page += 1;
        }
    }
    fn search_page(
        &self,
        ctx: &Ctx,
        target: &Url,
        by_post: bool,
        body: Option<&[u8]>,
        limit: i64,
    ) -> Result<Vec<u8>, Error> {
        let location = target.string();
        if location.len() > MAX_SEARCH_URL_BYTES {
            return Err(Error::Limit);
        }
        let method = if by_post { "POST" } else { "GET" };
        let route = target
            .path
            .strip_prefix(self.base.path.as_str())
            .unwrap_or(&target.path);
        let url = urlrules::parse(&location).map_err(|_| Error::Request)?;
        let mut headers = vec![("Accept".to_string(), "application/fhir+json".to_string())];
        if by_post {
            headers.push((
                "Content-Type".into(),
                "application/x-www-form-urlencoded".into(),
            ));
        }
        if let Some(authorize) = &self.options.authorize {
            let grant = authorize(ctx);
            context(ctx)?;
            let Ok(grant) = grant else {
                return Err(Error::Auth);
            };
            let authorized = parse_base(&grant.audience).is_ok_and(|audience| {
                same_origin(&self.base, &audience) && self.base.path == audience.path
            });
            if !authorized || !valid_bearer(&grant.access_token) {
                return Err(Error::Auth);
            }
            check_smart_scopes(&grant.scopes, &grant.context, method, route)?;
            headers.push((
                "Authorization".into(),
                format!("Bearer {}", grant.access_token),
            ));
        }
        let mut reader = body.map(SliceReader::new);
        let mut request = HttpRequest {
            method,
            url: &url,
            headers,
            body: reader.as_mut().map(|r| r as &mut dyn ByteRead),
            https: (url.scheme == "https").then_some(&self.https),
            identity: None,
        };
        let mut response = round_trip(&mut request, ctx).map_err(|_| context_or_transport(ctx))?;
        if response.status_code != 200 {
            return Err(Error::HttpStatus(response.status_code));
        }
        match parse_media_type(response.header.get("Content-Type")) {
            Ok(m)
                if m.media_type == "application/fhir+json"
                    || m.media_type == "application/json" => {}
            _ => return Err(Error::Response),
        }
        if response.content_length > limit {
            return Err(Error::Limit);
        }
        let cap = (limit + 1) as usize;
        let mut data = Vec::new();
        let mut buf = vec![0u8; 32 << 10];
        while data.len() < cap {
            let want = buf.len().min(cap - data.len());
            let (n, err) = response.body.read(&mut buf[..want]);
            data.extend_from_slice(&buf[..n]);
            match err {
                None => {}
                Some(ReadErr::Eof) => break,
                Some(_) => return Err(context_or_transport(ctx)),
            }
        }
        if data.len() as i64 > limit {
            return Err(Error::Limit);
        }
        Ok(data)
    }
    fn resolve_next(&self, previous: &Url, raw: &str) -> Result<Url, Error> {
        let reference = parse_safe_url(raw)?;
        let target = previous.resolve_reference(&reference);
        if !same_origin(&self.base, &target)
            || target.raw_query.is_empty()
            || target.path != format!("{}/{IMAGING_STUDY_TYPE}", self.base.path)
        {
            return Err(Error::Endpoint);
        }
        Ok(target)
    }
}
fn context(ctx: &Ctx) -> Result<(), Error> {
    ctx.err()
        .map_or(Ok(()), |kind: Kind| Err(Error::Context(kind)))
}
fn context_or_transport(ctx: &Ctx) -> Error {
    ctx.err().map_or(Error::Transport, Error::Context)
}
fn encode_search(values: &Values) -> Result<String, Error> {
    if values.0.len() > 256 {
        return Err(Error::Limit);
    }
    let mut total = 0usize;
    for (key, items) in &values.0 {
        if key.is_empty() || key.len() > MAX_SEARCH_FORM_BYTES {
            return Err(Error::Request);
        }
        for value in items {
            if value.len() > MAX_SEARCH_FORM_BYTES {
                return Err(Error::Limit);
            }
            total += key.len() + value.len() + 2;
            if total > MAX_SEARCH_FORM_BYTES {
                return Err(Error::Limit);
            }
        }
    }
    let query = values.encode();
    if query.len() > MAX_SEARCH_FORM_BYTES {
        return Err(Error::Limit);
    }
    Ok(query)
}
fn valid_bearer(token: &str) -> bool {
    if token.is_empty() || token.len() > 16 << 10 {
        return false;
    }
    let mut padding = false;
    for (i, b) in token.bytes().enumerate() {
        if b == b'=' && i > 0 {
            padding = true;
            continue;
        }
        if padding || !(b.is_ascii_alphanumeric() || b"-._~+/".contains(&b)) {
            return false;
        }
    }
    true
}
fn parse_base(raw: &str) -> Result<Url, Error> {
    let mut u = parse_safe_url(raw)?;
    if u.hostname().is_empty()
        || (u.scheme != "http" && u.scheme != "https")
        || !u.raw_query.is_empty()
        || u.force_query
    {
        return Err(Error::Endpoint);
    }
    if u.path.ends_with('/') {
        u.path.pop();
    }
    Ok(u)
}
fn parse_safe_url(raw: &str) -> Result<Url, Error> {
    if raw.is_empty() || raw.len() > MAX_SEARCH_URL_BYTES || raw.contains(['#', '\\']) {
        return Err(Error::Endpoint);
    }
    let u = urlrules::parse(raw).map_err(|_| Error::Endpoint)?;
    if !u.opaque.is_empty()
        || u.user.is_some()
        || u.host.ends_with(':')
        || !canonical_url_path(&u.escaped_path())
    {
        return Err(Error::Endpoint);
    }
    let host = u.hostname();
    if !host.is_ascii() || host.contains('%') {
        return Err(Error::Endpoint);
    }
    let port = u.port();
    if !port.is_empty() {
        match port.parse::<u16>() {
            Ok(value) if value != 0 && value.to_string() == port => {}
            _ => return Err(Error::Endpoint),
        }
    }
    if raw.bytes().any(|b| b <= b' ' || b == 127) {
        return Err(Error::Endpoint);
    }
    Ok(u)
}
fn canonical_url_path(path: &str) -> bool {
    if path.is_empty() || path == "/" {
        return true;
    }
    let path = path.strip_prefix('/').unwrap_or(path);
    let path = path.strip_suffix('/').unwrap_or(path);
    path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
    })
}
fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme == right.scheme
        && left.hostname().eq_ignore_ascii_case(right.hostname())
        && effective_port(left) == effective_port(right)
}
fn effective_port(u: &Url) -> &str {
    match u.port() {
        "" if u.scheme == "https" => "443",
        "" => "80",
        port => port,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bearer_tokens() {
        assert!(valid_bearer("abc=="));
        assert!(!valid_bearer("=abc"));
        assert!(!valid_bearer("a=b"));
        assert!(!valid_bearer(""));
    }
    #[test]
    fn base_urls() {
        assert!(parse_base("https://fhir.example/r4/").is_ok_and(|u| u.path == "/r4"));
        assert_eq!(
            parse_base("https://fhir.example/a/../b").err(),
            Some(Error::Endpoint)
        );
        assert_eq!(
            Client::new("http://127.0.0.1/r4", ClientOptions::default()).err(),
            Some(Error::Endpoint)
        );
    }
}
