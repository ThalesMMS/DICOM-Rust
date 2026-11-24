use super::cache::{CacheStats, ResponseCache};
use super::policy::{CompiledPolicy, Credential, Policy, compile_policy, parse_endpoint_url};
use super::{ChainError, ClientError, Error, ErrorKind, Sentinel};
use crate::net::dicomweb::AccessToken;
use crate::net::dicomweb::http::{self, CheckedError, HttpRequest, HttpResponse, HttpsOptions};
use crate::net::dicomweb::mimeheader::parse_media_type;
use crate::net::dicomweb::multipart::{ByteRead, ReadErr};
use crate::net::dicomweb::urlrules::Url;
use crate::net::ul::{Ctx, Kind};
use std::sync::Arc;
use std::time::Duration;
const DEFAULT_MAX_RESPONSE_BYTES: i64 = 256 << 20;
const DEFAULT_MAX_RETRIES: i64 = 2;
const MAX_REDIRECTS: usize = 10;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(5);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub width: i64,
    pub height: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Region {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ByteRange {
    pub start: i64,
    pub end: i64,
}
pub type StreamType = String;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reference {
    pub pixel_data_provider_url: String,
    pub htj2k: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub reference: Reference,
    pub frame: i64,
    pub size: Size,
    pub region: Region,
    pub range: Option<ByteRange>,
    pub stream_type: StreamType,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    pub data: Vec<u8>,
    pub content_type: String,
    pub partial: bool,
    pub cache_hit: bool,
}
pub type RetryDelay = Arc<dyn Fn(i64, Option<i64>) -> Duration + Send + Sync>;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Registry {
    #[default]
    Default,
    Empty,
}
#[derive(Clone, Default)]
pub struct Config {
    pub policy: Policy,
    pub max_response_bytes: i64,
    pub max_cache_bytes: i64,
    pub max_retries: i64,
    pub registry: Registry,
    pub retry_delay: Option<RetryDelay>,
    pub https: Option<HttpsOptions>,
}
pub struct Client {
    pub(crate) policy: CompiledPolicy,
    max_response_bytes: i64,
    max_retries: i64,
    pub(crate) registry: Registry,
    retry_delay: Option<RetryDelay>,
    https: Option<HttpsOptions>,
    cache: ResponseCache,
}
struct Attempt {
    retry: bool,
    status: Option<i64>,
    retry_after: String,
}
impl Client {
    pub fn new(config: Config) -> Result<Client, ChainError> {
        let policy = compile_policy(&config.policy)?;
        let max_response_bytes = if config.max_response_bytes <= 0 {
            DEFAULT_MAX_RESPONSE_BYTES
        } else {
            config.max_response_bytes
        };
        if config.max_retries < 0 {
            return Err(ChainError::plain("jpip: max retries cannot be negative"));
        }
        let max_retries = if config.max_retries == 0 {
            DEFAULT_MAX_RETRIES
        } else {
            config.max_retries
        };
        Ok(Client {
            policy,
            max_response_bytes,
            max_retries,
            registry: config.registry,
            retry_delay: config.retry_delay,
            https: config.https,
            cache: ResponseCache::new(config.max_cache_bytes),
        })
    }
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }
    pub fn retrieve(&self, ctx: &Ctx, request: &Request) -> Result<Response, ClientError> {
        let target = request_url(request)?;
        let credential = self.policy.authorize(&target)?;
        let key = cache_key(&target, request.range);
        if let Some(response) = self.cache.get(&key) {
            return Ok(response);
        }
        let mut attempt = 0;
        loop {
            if let Some(e) = context_error(ctx) {
                return Err(e);
            }
            let (result, info) = self.retrieve_once(ctx, &target, request.range, &credential, true);
            match result {
                Ok(response) => {
                    self.cache.put(&key, &response);
                    return Ok(response);
                }
                Err(e) => {
                    if !info.retry || attempt == self.max_retries {
                        return Err(e);
                    }
                }
            }
            let delay = self.delay(attempt, &info);
            wait_context(ctx, delay)?;
            attempt += 1;
        }
    }
    fn retrieve_once(
        &self,
        ctx: &Ctx,
        target: &Url,
        byte_range: Option<ByteRange>,
        credential: &Credential,
        allow_bearer_replay: bool,
    ) -> (Result<Response, ClientError>, Attempt) {
        let mut info = Attempt {
            retry: false,
            status: None,
            retry_after: String::new(),
        };
        let mut headers = vec![(
            "Accept".to_string(),
            "image/jp2, image/jph, image/jphc, image/jpp-stream, image/jpt-stream".to_string(),
        )];
        if let Err(e) = apply_credential(ctx, &mut headers, credential) {
            return (Err(e.into()), info);
        }
        if let Some(r) = byte_range {
            headers.push(("Range".into(), format!("bytes={}-{}", r.start, r.end)));
        }
        let request = HttpRequest {
            method: "GET",
            url: target,
            headers,
            body: None,
            https: self.https.as_ref(),
            identity: None,
        };
        let sent = http::send_with_check(
            request,
            &mut |url: &Url,
                  via: usize,
                  headers: &mut Vec<(String, String)>|
             -> Result<(), Error> {
                if via >= MAX_REDIRECTS {
                    return Err(Error::sentinel(
                        ErrorKind::RedirectDenied,
                        "redirect",
                        Sentinel::RedirectDenied,
                    ));
                }
                let credential = self.policy.authorize(url).map_err(|e| {
                    Error::new(
                        ErrorKind::RedirectDenied,
                        "redirect",
                        ChainError::join(
                            ChainError::sentinel(Sentinel::RedirectDenied),
                            jpip_error_value(&e),
                        ),
                    )
                })?;
                headers.retain(|(name, _)| name != "Authorization");
                apply_credential(ctx, headers, &credential)
            },
            ctx,
        );
        let (mut resp, sent_headers) = match sent {
            Ok(r) => r,
            Err(CheckedError::Refused(e)) => {
                if let Some(e) = context_error(ctx) {
                    return (Err(e), info);
                }
                return (Err(e.into()), info);
            }
            Err(CheckedError::Transport(e)) => {
                if let Some(e) = context_error(ctx) {
                    return (Err(e), info);
                }
                drop(e);
                info.retry = true;
                return (
                    Err(Error::sentinel(ErrorKind::Offline, "retrieve", Sentinel::Offline).into()),
                    info,
                );
            }
        };
        if resp.status_code == 401
            && allow_bearer_replay
            && self.invalidate_bearer_challenge(&resp, &sent_headers)
        {
            discard(&mut resp);
            return self.retrieve_once(ctx, target, byte_range, credential, false);
        }
        let status = resp.status_code;
        info.status = Some(status);
        info.retry_after = resp.header.get("Retry-After").to_string();
        if !(200..=299).contains(&status) {
            info.retry = retryable_status(status);
            discard(&mut resp);
            return (
                Err(
                    Error::sentinel(ErrorKind::HttpStatus, "retrieve", Sentinel::HttpStatus)
                        .status(status)
                        .into(),
                ),
                info,
            );
        }
        let partial_error = || {
            Error::sentinel(
                ErrorKind::PartialResponse,
                "retrieve",
                Sentinel::PartialResponse,
            )
            .status(status)
        };
        if byte_range.is_none() && status == 206 {
            return (Err(partial_error().into()), info);
        }
        let partial = status == 206;
        let content_range = resp.header.get("Content-Range").to_string();
        if byte_range.is_some() && partial && content_range.trim().is_empty() {
            return (Err(partial_error().into()), info);
        }
        let content_type = normalize_content_type(resp.header.get("Content-Type"));
        if !SUPPORTED_CONTENT_TYPES.contains(&content_type.as_str()) {
            return (
                Err(Error::sentinel(
                    ErrorKind::UnsupportedContentType,
                    "retrieve",
                    Sentinel::UnsupportedContentType,
                )
                .content_type(&content_type)
                .into()),
                info,
            );
        }
        let too_large = |size: i64| {
            Error::sentinel(
                ErrorKind::ResponseTooLarge,
                "retrieve",
                Sentinel::ResponseTooLarge,
            )
            .sized(self.max_response_bytes, size)
        };
        if resp.content_length > self.max_response_bytes {
            return (Err(too_large(resp.content_length).into()), info);
        }
        let read_error = |size: i64| {
            let mut e = Error::sentinel(
                ErrorKind::PartialResponse,
                "read response",
                Sentinel::PartialResponse,
            );
            e.size = size;
            e
        };
        let Ok(data) = read_limited(&mut resp.body, (self.max_response_bytes + 1) as usize) else {
            return (Err(read_error(0).into()), info);
        };
        if data.len() as i64 > self.max_response_bytes {
            return (Err(too_large(data.len() as i64).into()), info);
        }
        if resp.content_length >= 0 && data.len() as i64 != resp.content_length {
            return (Err(read_error(data.len() as i64).into()), info);
        }
        if partial
            && let Err(e) = validate_content_range(&content_range, byte_range, data.len() as i64)
        {
            return (Err(e.into()), info);
        }
        if let Err(e) = validate_representation(&content_type, &data, partial) {
            return (Err(e.into()), info);
        }
        (
            Ok(Response {
                data,
                content_type,
                partial,
                cache_hit: false,
            }),
            info,
        )
    }
    fn invalidate_bearer_challenge(&self, resp: &HttpResponse, sent: &[(String, String)]) -> bool {
        let Ok(credential) = self.policy.authorize(&resp.request_url) else {
            return false;
        };
        let Some(bearer) = &credential.bearer else {
            return false;
        };
        if !bearer.can_invalidate() {
            return false;
        }
        let authorization = sent
            .iter()
            .find(|(name, _)| name == "Authorization")
            .map_or("", |(_, v)| v.as_str());
        let Some(token) = authorization.strip_prefix("Bearer ") else {
            return false;
        };
        let Ok(challenged) = AccessToken::new(token.trim(), None) else {
            return false;
        };
        bearer.invalidate(&challenged);
        true
    }
    fn delay(&self, attempt: i64, info: &Attempt) -> Duration {
        if let Some(f) = &self.retry_delay {
            return f(attempt, info.status).min(MAX_RETRY_DELAY);
        }
        if info.status.is_some()
            && let Some(delay) = retry_after(&info.retry_after)
        {
            return delay;
        }
        let delay = Duration::from_millis(100) * (1u32 << attempt.min(5));
        delay.min(MAX_RETRY_DELAY)
    }
}
fn jpip_error_value(e: &Error) -> ChainError {
    let mut out = ChainError::plain(e.to_string());
    if let Some(inner) = &e.err {
        for s in [Sentinel::PolicyDenied, Sentinel::InvalidRequest] {
            if inner.is(s) {
                out = out.with(s);
            }
        }
    }
    out
}
const SUPPORTED_CONTENT_TYPES: [&str; 5] = [
    "image/jp2",
    "image/jpp-stream",
    "image/jpt-stream",
    "image/jph",
    "image/jphc",
];
fn context_error(ctx: &Ctx) -> Option<ClientError> {
    match ctx.err()? {
        Kind::Canceled => Some(ClientError::Context(ChainError::sentinel(
            Sentinel::Canceled,
        ))),
        _ => Some(ClientError::Context(ChainError::sentinel(
            Sentinel::DeadlineExceeded,
        ))),
    }
}
fn apply_credential(
    ctx: &Ctx,
    headers: &mut Vec<(String, String)>,
    credential: &Credential,
) -> Result<(), Error> {
    if let Some(bearer) = &credential.bearer {
        let value = match bearer.token(ctx) {
            Ok(token) if !token.value().trim().is_empty() => token.value().to_string(),
            _ => {
                return Err(Error::sentinel(
                    ErrorKind::Authentication,
                    "authorize",
                    Sentinel::BearerTokenUnavailable,
                ));
            }
        };
        set_header(headers, "Authorization", format!("Bearer {value}"));
        return Ok(());
    }
    if !credential.username.is_empty() || !credential.password.is_empty() {
        let raw = format!("{}:{}", credential.username, credential.password);
        set_header(
            headers,
            "Authorization",
            format!(
                "Basic {}",
                crate::dicomjson::sizedparse::base64_encode(raw.as_bytes())
            ),
        );
    }
    Ok(())
}
fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    headers.retain(|(n, _)| n != name);
    headers.push((name.to_string(), value));
}
fn discard(resp: &mut HttpResponse) {
    let mut left = 4096usize;
    let mut buf = [0u8; 1024];
    while left > 0 {
        let want = left.min(buf.len());
        let (n, err) = resp.body.read(&mut buf[..want]);
        left -= n;
        if err.is_some() || n == 0 {
            break;
        }
    }
}
fn read_limited(body: &mut dyn ByteRead, limit: usize) -> Result<Vec<u8>, ReadErr> {
    let mut out = Vec::new();
    let mut buf = vec![0u8; 32 << 10];
    while out.len() < limit {
        let want = (limit - out.len()).min(buf.len());
        let (n, err) = body.read(&mut buf[..want]);
        out.extend_from_slice(&buf[..n]);
        match err {
            None => {}
            Some(ReadErr::Eof) => return Ok(out),
            Some(e) => return Err(e),
        }
    }
    Ok(out)
}
fn partial_range_error(size: i64) -> Error {
    let mut e = Error::sentinel(
        ErrorKind::PartialResponse,
        "validate content range",
        Sentinel::PartialResponse,
    );
    e.size = size;
    e
}
fn validate_content_range(
    value: &str,
    requested: Option<ByteRange>,
    size: i64,
) -> Result<(), Error> {
    let Some(requested) = requested else {
        return Err(partial_range_error(0));
    };
    let value = value.trim();
    if !value.to_lowercase().starts_with("bytes ") {
        return Err(partial_range_error(0));
    }
    let rest = value["bytes ".len()..].trim();
    let Some((range, total)) = rest.split_once('/') else {
        return Err(partial_range_error(0));
    };
    let Some((start, end)) = range.split_once('-') else {
        return Err(partial_range_error(0));
    };
    let (start, end) = (parse_int(start), parse_int(end));
    let (Some(start), Some(end)) = (start, end) else {
        return Err(partial_range_error(size));
    };
    if start != requested.start || end < start || end > requested.end || size != end - start + 1 {
        return Err(partial_range_error(size));
    }
    if total == "*" {
        if end != requested.end {
            return Err(partial_range_error(size));
        }
        return Ok(());
    }
    match parse_int(total) {
        Some(total) if total > end => {
            if end != requested.end && end != total - 1 {
                return Err(partial_range_error(size));
            }
            Ok(())
        }
        _ => Err(partial_range_error(size)),
    }
}
fn parse_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}
pub(crate) fn request_url(request: &Request) -> Result<Url, Error> {
    let invalid = || {
        Error::sentinel(
            ErrorKind::InvalidRequest,
            "build request",
            Sentinel::InvalidRequest,
        )
    };
    if request.frame < 0 {
        return Err(invalid());
    }
    let s = request.size;
    if s.width < 0 || s.height < 0 || (s.width == 0) != (s.height == 0) {
        return Err(invalid());
    }
    let r = request.region;
    let has_region = r.width != 0 || r.height != 0 || r.x != 0 || r.y != 0;
    if has_region && (r.x < 0 || r.y < 0 || r.width <= 0 || r.height <= 0) {
        return Err(invalid());
    }
    if let Some(range) = request.range
        && (range.start < 0 || range.end < range.start)
    {
        return Err(invalid());
    }
    let t = request.stream_type.as_str();
    if !t.is_empty() && t != "jpp-stream" && t != "jpt-stream" {
        return Err(invalid());
    }
    let mut u =
        parse_endpoint_url(&request.reference.pixel_data_provider_url).map_err(|_| invalid())?;
    let (mut query, _) = crate::net::dicomweb::urlrules::parse_query(&u.raw_query);
    query.set("stream", &(request.frame + 1).to_string());
    if s.width > 0 {
        query.set("fsiz", &format!("{},{},closest", s.width, s.height));
    }
    if has_region {
        query.set("roff", &format!("{},{}", r.x, r.y));
        query.set("rsiz", &format!("{},{}", r.width, r.height));
    }
    if !t.is_empty() {
        query.set("type", t);
    }
    u.raw_query = query.encode();
    Ok(u)
}
fn cache_key(u: &Url, byte_range: Option<ByteRange>) -> String {
    let mut key = u.string();
    if let Some(r) = byte_range {
        key.push_str(&format!("\x00{}-{}", r.start, r.end));
    }
    key
}
fn normalize_content_type(value: &str) -> String {
    match parse_media_type(value.trim()) {
        Ok(m) => m.media_type.to_lowercase(),
        Err(_) => value.split(';').next().unwrap_or("").trim().to_lowercase(),
    }
}
fn validate_representation(content_type: &str, data: &[u8], partial: bool) -> Result<(), Error> {
    let corrupt = || {
        Error::sentinel(
            ErrorKind::CorruptResponse,
            "validate response",
            Sentinel::CorruptResponse,
        )
        .content_type(content_type)
    };
    if data.is_empty() {
        return Err(corrupt());
    }
    if partial {
        return Ok(());
    }
    if matches!(content_type, "image/jp2" | "image/jph" | "image/jphc") {
        let codestream = data.len() >= 2 && data[0] == 0xff && data[1] == 0x4f;
        let container = data.len() >= 12
            && &data[..4] == b"\x00\x00\x00\x0c"
            && &data[4..8] == b"jP  "
            && &data[8..12] == b"\r\n\x87\n";
        if !codestream && !container {
            return Err(corrupt());
        }
    }
    Ok(())
}
fn retryable_status(status: i64) -> bool {
    status == 408 || status == 429 || status >= 500
}
fn retry_after(value: &str) -> Option<Duration> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(seconds) = parse_int(value).filter(|&s| s >= 0) {
        let nanos = seconds.wrapping_mul(1_000_000_000);
        if nanos <= 0 {
            return Some(Duration::ZERO);
        }
        return Some(Duration::from_nanos(nanos as u64).min(MAX_RETRY_DELAY));
    }
    let when = parse_http_time(value)?;
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) as i128;
    let delay = when * 1_000_000_000 - now;
    if delay <= 0 {
        return Some(Duration::ZERO);
    }
    Some(Duration::from_nanos(
        delay.min(MAX_RETRY_DELAY.as_nanos() as i128) as u64,
    ))
}
fn parse_http_time(value: &str) -> Option<i128> {
    use chrono::NaiveDateTime;
    for format in [
        "%a, %d %b %Y %H:%M:%S GMT",
        "%A, %d-%b-%y %H:%M:%S GMT",
        "%a %b %e %H:%M:%S %Y",
    ] {
        if let Ok(t) = NaiveDateTime::parse_from_str(value, format) {
            return Some(i128::from(t.and_utc().timestamp()));
        }
    }
    None
}
fn wait_context(ctx: &Ctx, delay: Duration) -> Result<(), ClientError> {
    if delay.is_zero() {
        return match context_error(ctx) {
            Some(e) => Err(e),
            None => Ok(()),
        };
    }
    let until = std::time::Instant::now() + delay;
    loop {
        if let Some(e) = context_error(ctx) {
            return Err(e);
        }
        let now = std::time::Instant::now();
        if now >= until {
            return Ok(());
        }
        std::thread::sleep((until - now).min(Duration::from_millis(5)));
    }
}
