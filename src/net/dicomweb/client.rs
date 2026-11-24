use super::http::{self, Body, HttpRequest, HttpResponse, RedirectCheck, TransportError};
use super::mimeheader::{Header, to_lower, trim_space};
use super::multipart::{ByteRead, MultipartReader, PartReader, ReadErr, SliceReader, read_all};
use super::response::{
    self, DICOM_OBJECT_POLICY, FRAME_POLICY, MediaPolicy, ObjectPart, ParseLimits, PartsError,
    Response, StoreResult,
};
use super::types::{
    Cause, CauseClass, DEFAULT_MAX_BODY_BYTES, DEFAULT_TIMEOUT_NANOS, Endpoint, Error, ErrorKind,
    InstanceRef, METADATA_MEDIA_TYPE_DICOM_JSON, METADATA_MEDIA_TYPE_DICOM_XML,
    ResponseDecodeError, ResponseDecodeErrorKind as K, ResponseLimits, safe_http_status, safe_url,
};
use super::urlrules::{self, Url, Values};
use crate::dicomjson::{self, Dataset, element_string};
use crate::net::ul::{Ctx, Nanos};
use crate::textfmt::quote;
use crate::transfer;
use std::sync::Arc;
use std::time::{Duration, Instant};
pub(super) const ACCEPT_DICOM_JSON: &str = "application/dicom+json, application/json;q=0.9";
const ACCEPT_DICOM: &str = "application/dicom, multipart/related; type=\"application/dicom\"";
const DEFAULT_MAX_METADATA_PARTS: i64 = 10_000;
pub const ERR_BEARER_TOKEN_UNAVAILABLE: &str = "DICOMweb bearer token unavailable";
pub const ERR_BEARER_SIGN_IN_REQUIRED: &str =
    "DICOMweb bearer token unavailable: a new sign-in is required";
pub const ERR_INSECURE_BASIC_AUTH: &str = "DICOMweb Basic authentication requires verified HTTPS";
pub const ERR_REDIRECT_ORIGIN: &str = "DICOMweb redirect must use the original origin";
pub const ERR_RESPONSE_BODY_LIMIT: &str = "dicom dicomweb: response body exceeds the limit";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccessToken {
    value: String,
    pub expiry: Option<i128>,
}
impl AccessToken {
    pub fn new(value: &str, expiry: Option<i128>) -> Result<AccessToken, TokenError> {
        let value = trim_space(value);
        if value.is_empty() {
            return Err(TokenError::Unavailable);
        }
        Ok(AccessToken {
            value: value.to_string(),
            expiry,
        })
    }
    pub fn value(&self) -> &str {
        &self.value
    }
    fn expiry_text(&self, nanos: bool) -> Option<String> {
        let expiry = self.expiry?;
        let seconds = expiry.div_euclid(1_000_000_000) as i64;
        let fraction = expiry.rem_euclid(1_000_000_000) as u32;
        let time = chrono::DateTime::from_timestamp(seconds, 0)?;
        let mut text = time.format("%Y-%m-%dT%H:%M:%S").to_string();
        if nanos && fraction != 0 {
            let digits = format!("{fraction:09}");
            text.push('.');
            text.push_str(digits.trim_end_matches('0'));
        }
        text.push('Z');
        Some(text)
    }
    pub fn string(&self) -> String {
        if self.value.is_empty() {
            return "DICOMweb access token (empty)".into();
        }
        match self.expiry_text(false) {
            None => "DICOMweb access token (redacted, no expiry)".into(),
            Some(expiry) => format!("DICOMweb access token (redacted, expires {expiry})"),
        }
    }
    pub fn marshal_json(&self) -> String {
        match self.expiry_text(true) {
            None => "{}".into(),
            Some(expiry) => format!("{{\"expiry\":\"{expiry}\"}}"),
        }
    }
    fn valid_now(&self) -> bool {
        if self.value.is_empty() {
            return false;
        }
        match self.expiry {
            None => true,
            Some(expiry) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as i128);
                now < expiry
            }
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenError {
    Canceled,
    DeadlineExceeded,
    SignInRequired,
    Unavailable,
}
impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TokenError::Canceled => "context canceled",
            TokenError::DeadlineExceeded => "context deadline exceeded",
            TokenError::SignInRequired => ERR_BEARER_SIGN_IN_REQUIRED,
            TokenError::Unavailable => ERR_BEARER_TOKEN_UNAVAILABLE,
        })
    }
}
impl std::error::Error for TokenError {}
pub trait BearerTokenSource: Send + Sync {
    fn token(&self, ctx: &Ctx) -> Result<AccessToken, TokenError>;
    fn can_invalidate(&self) -> bool {
        false
    }
    fn invalidate(&self, _challenged: &AccessToken) {}
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RedirectPolicy {
    #[default]
    Default,
    Authenticated,
    SameOrigin,
    Invalid,
}
pub type OnRetry = Arc<dyn Fn(i64, i64, Nanos) + Send + Sync>;
#[derive(Clone, Default)]
pub struct RetryPolicy {
    pub max_attempts: i64,
    pub initial_backoff: Nanos,
    pub max_backoff: Nanos,
    pub multiplier: f64,
    pub jitter: f64,
    pub retry_status_codes: Option<Vec<i64>>,
    pub on_retry: Option<OnRetry>,
}
impl RetryPolicy {
    pub fn defaults() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            initial_backoff: Nanos(250_000_000),
            max_backoff: Nanos(5_000_000_000),
            multiplier: 2.0,
            jitter: 0.2,
            retry_status_codes: None,
            on_retry: None,
        }
    }
}
struct RetrySettings {
    attempts: i64,
    initial: i64,
    max: i64,
    factor: f64,
    jitter: f64,
    statuses: Vec<i64>,
    on_retry: Option<OnRetry>,
}
fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 || !x.is_finite() {
        return (x, 0);
    }
    let (x, shift) = if x.abs() < f64::MIN_POSITIVE {
        (x * (1u64 << 52) as f64, -52)
    } else {
        (x, 0)
    };
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32 - 1022;
    let frac = f64::from_bits((bits & !(0x7ff << 52)) | (1022 << 52));
    (frac, exp + shift)
}
fn ldexp(frac: f64, exp: i32) -> f64 {
    let mut value = frac;
    let mut exp = exp;
    while exp > 1000 {
        value *= 2f64.powi(1000);
        exp -= 1000;
    }
    while exp < -1000 {
        value *= 2f64.powi(-1000);
        exp += 1000;
    }
    value * 2f64.powi(exp)
}
fn pow_int(x: f64, n: i64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return x;
    }
    let (mut a1, mut ae) = (1.0f64, 0i32);
    let (mut x1, mut xe) = frexp(x);
    let mut i = n;
    while i != 0 {
        if !(-(1 << 12)..=(1 << 12)).contains(&xe) {
            return if xe < 0 { 0.0 } else { f64::INFINITY };
        }
        if i & 1 == 1 {
            a1 *= x1;
            ae += xe;
        }
        x1 *= x1;
        xe <<= 1;
        if x1 < 0.5 {
            x1 += x1;
            xe -= 1;
        }
        i >>= 1;
    }
    ldexp(a1, ae)
}
fn random_unit() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let bits = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    (bits >> 11) as f64 / (1u64 << 53) as f64
}
impl RetrySettings {
    fn from(policy: Option<&RetryPolicy>) -> Result<RetrySettings, String> {
        let Some(p) = policy else {
            return Ok(RetrySettings {
                attempts: 1,
                initial: 0,
                max: 0,
                factor: 0.0,
                jitter: 0.0,
                statuses: Vec::new(),
                on_retry: None,
            });
        };
        if p.max_attempts < 0
            || p.initial_backoff.0 < 0
            || p.max_backoff.0 < 0
            || p.multiplier < 0.0
            || p.multiplier.is_nan()
            || p.multiplier.is_infinite()
            || (p.multiplier != 0.0 && p.multiplier < 1.0)
            || p.jitter.is_nan()
            || p.jitter < 0.0
            || p.jitter > 1.0
        {
            return Err("dicomweb: retry policy values are out of range".into());
        }
        let mut settings = RetrySettings {
            attempts: p.max_attempts,
            initial: p.initial_backoff.0,
            max: p.max_backoff.0,
            factor: p.multiplier,
            jitter: p.jitter,
            statuses: Vec::new(),
            on_retry: p.on_retry.clone(),
        };
        if settings.attempts == 0 {
            settings.attempts = 3;
        }
        if settings.initial == 0 {
            settings.initial = 250_000_000;
        }
        if settings.max == 0 {
            settings.max = 5_000_000_000;
        }
        if settings.initial > settings.max {
            settings.initial = settings.max;
        }
        if settings.factor == 0.0 {
            settings.factor = 2.0;
        }
        let default_codes = [429, 502, 503, 504];
        let codes: &[i64] = p.retry_status_codes.as_deref().unwrap_or(&default_codes);
        for &code in codes {
            if !(100..=599).contains(&code) {
                return Err("dicomweb: retry status codes must be HTTP statuses".into());
            }
            settings.statuses.push(code);
        }
        Ok(settings)
    }
    fn backoff(&self, attempt: i64) -> i64 {
        let mut delay = self.initial as f64 * pow_int(self.factor, attempt - 1);
        if delay > self.max as f64 {
            delay = self.max as f64;
        }
        if self.jitter > 0.0 {
            delay -= delay * self.jitter * random_unit();
        }
        crate::fmath::f64_to_i64(delay)
    }
}
pub(super) fn retry_after(header: &Header) -> Option<i64> {
    let value = trim_space(header.get("Retry-After"));
    if value.is_empty() {
        return None;
    }
    if let Ok(seconds) = crate::dicomjson::sizedparse::parse_int(value, 64) {
        if seconds < 0 {
            return None;
        }
        return Some(seconds.saturating_mul(1_000_000_000));
    }
    let when = [
        "%a, %d %b %Y %H:%M:%S GMT",
        "%A, %d-%b-%y %H:%M:%S GMT",
        "%a %b %e %H:%M:%S %Y",
    ]
    .iter()
    .find_map(|format| chrono::NaiveDateTime::parse_from_str(value, format).ok())?;
    let delay = when.and_utc().signed_duration_since(chrono::Utc::now());
    Some(delay.num_nanoseconds().unwrap_or(i64::MAX).max(0))
}
#[derive(Clone, Default)]
pub struct Options {
    pub timeout: Nanos,
    pub max_body_bytes: i64,
    pub workitem_requester: String,
    pub redirect_policy: RedirectPolicy,
    pub response_limits: ResponseLimits,
    pub metadata_media_types: Vec<String>,
    pub max_metadata_parts: i64,
    pub basic_username: String,
    pub basic_password: String,
    pub bearer_token: String,
    pub bearer_token_source: Option<Arc<dyn BearerTokenSource>>,
    pub retry: Option<RetryPolicy>,
    pub https: Option<super::http::HttpsOptions>,
    pub https_identity: Option<super::http::HttpsIdentity>,
    pub cache: Option<Arc<super::clientx::ResponseCache>>,
}
impl Options {
    pub fn string(&self) -> String {
        format!(
            "DICOMweb options (custom_http_client=false, timeout={}, max_body_bytes={}, max_response_parts={}, max_part_bytes={}, max_part_header_bytes={}, max_multipart_depth={}, allowed_wado_media_types={}, metadata_media_types={}, max_metadata_parts={}, basic_auth={}, static_bearer={}, dynamic_bearer={}, retry={}, cache={})",
            crate::net::dimse::duration_text(self.timeout),
            self.max_body_bytes,
            self.response_limits.max_parts,
            self.response_limits.max_part_bytes,
            self.response_limits.max_part_header_bytes,
            self.response_limits.max_multipart_depth,
            self.response_limits.allowed_media_types.len(),
            self.metadata_media_types.len(),
            self.max_metadata_parts,
            !trim_space(&self.basic_username).is_empty(),
            !trim_space(&self.bearer_token).is_empty(),
            self.bearer_token_source.is_some(),
            self.retry.is_some(),
            self.cache.is_some(),
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorePayloadError {
    pub index: usize,
    pub path: String,
    pub sop_instance_uid: String,
    pub err: String,
}
impl std::fmt::Display for StorePayloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = [&self.path, &self.sop_instance_uid]
            .into_iter()
            .map(|v| trim_space(v))
            .find(|v| !v.is_empty())
            .unwrap_or("");
        write!(f, "STOW-RS object {name}: {}", self.err)
    }
}
impl std::error::Error for StorePayloadError {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    Web(Error),
    StorePayload(StorePayloadError),
    Read(ReadErr),
    Other(String),
    SearchPageLimit,
    SearchPagination,
    BulkDataTooLarge(Option<Box<ClientError>>),
    Joined(Vec<ClientError>),
}
impl ClientError {
    #[doc(hidden)]
    pub fn transport_cause(&self) -> Option<&super::types::TransportCause> {
        match self {
            ClientError::Web(e) => e.transport_cause(),
            ClientError::Read(ReadErr::Web(e)) => e.transport_cause(),
            ClientError::BulkDataTooLarge(Some(inner)) => inner.transport_cause(),
            ClientError::Joined(all) => all.iter().find_map(ClientError::transport_cause),
            _ => None,
        }
    }
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Web(e) => write!(f, "{e}"),
            ClientError::StorePayload(e) => write!(f, "{e}"),
            ClientError::Read(e) => write!(f, "{e}"),
            ClientError::Other(e) => f.write_str(e),
            ClientError::SearchPageLimit => f.write_str(super::clientx::ERR_SEARCH_PAGE_LIMIT),
            ClientError::SearchPagination => f.write_str(super::clientx::ERR_SEARCH_PAGINATION),
            ClientError::BulkDataTooLarge(None) => {
                f.write_str(crate::object::bulk::ERR_BULK_DATA_TOO_LARGE)
            }
            ClientError::BulkDataTooLarge(Some(e)) => {
                write!(f, "{}: {e}", crate::object::bulk::ERR_BULK_DATA_TOO_LARGE)
            }
            ClientError::Joined(errors) => {
                for (i, e) in errors.iter().enumerate() {
                    if i > 0 {
                        f.write_str(
                            "
",
                        )?;
                    }
                    write!(f, "{e}")?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ClientError::Web(e) => std::error::Error::source(e),
            ClientError::Read(e) => std::error::Error::source(e),
            _ => None,
        }
    }
}
impl From<Error> for ClientError {
    fn from(e: Error) -> Self {
        ClientError::Web(e)
    }
}
type RequestHeaders = Vec<(String, String)>;
#[derive(Clone, Debug, Default)]
pub(super) struct RequestOptions {
    pub headers: RequestHeaders,
    pub same_origin_redirect: bool,
    pub no_follow: bool,
}
pub type Payload = Box<dyn ByteRead + Send>;
pub(crate) type OpenPayload = Box<dyn Fn() -> Result<Payload, crate::BoxError> + Send + Sync>;
#[derive(Default)]
pub struct StoreInstance {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub path: String,
    pub data: Vec<u8>,
    pub reader: Option<Payload>,
    pub open: Option<OpenPayload>,
    pub size: i64,
}
#[derive(Clone, Debug, Default)]
pub struct VerifyResult {
    pub response: Response,
    pub duration: Duration,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FramePart {
    pub frame_number: i64,
    pub content_type: String,
    pub transfer_syntax_uid: String,
    pub data: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RetrieveOptions {
    pub transfer_syntax_uids: Vec<String>,
}
pub struct PartStream<'r> {
    pub frame_number: i64,
    pub content_type: String,
    pub transfer_syntax_uid: String,
    pub content_location: String,
    pub reader: &'r mut dyn ByteRead,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetadataResult {
    pub datasets: Option<Vec<Dataset>>,
    pub refs: Vec<InstanceRef>,
    pub request_url: String,
}
#[derive(Clone, Default)]
pub struct Client {
    pub endpoint: Endpoint,
    pub options: Options,
}
pub(super) fn web(kind: ErrorKind, raw_url: &str, status: i64, cause: Cause) -> ClientError {
    ClientError::Web(Error::new(kind, raw_url, status, Some(cause)))
}
pub(super) fn request_failure(text: &str) -> ClientError {
    web(ErrorKind::RequestFailure, "", 0, Cause::text(text))
}
pub(super) fn decode_error(resp: &Response, cause: Cause) -> ClientError {
    web(
        ErrorKind::DecodeResponse,
        &resp.url,
        resp.status_code,
        cause,
    )
}
pub(super) fn decode_kind(resp: &Response, err: ResponseDecodeError) -> ClientError {
    decode_error(resp, Cause::decode(err))
}
pub(super) fn status_error(resp: &Response) -> ClientError {
    let kind = if resp.status_code == 401 || resp.status_code == 403 {
        ErrorKind::AuthStatus
    } else {
        ErrorKind::HttpStatus
    };
    ClientError::Web(Error::new(kind, &resp.url, resp.status_code, None))
}
pub(super) fn read_cause(err: &ReadErr) -> Cause {
    let class = match err {
        ReadErr::Decode(e) => CauseClass::Decode(*e),
        ReadErr::Deadline | ReadErr::Net { timeout: true, .. } => CauseClass::Timeout,
        ReadErr::Canceled => CauseClass::Canceled,
        _ => CauseClass::Other,
    };
    Cause {
        text: err.to_string(),
        class,
    }
}
fn url_error_op(method: &str) -> String {
    let mut op = String::new();
    for (i, c) in method.chars().enumerate() {
        op.push(if i == 0 { c } else { c.to_ascii_lowercase() });
    }
    op
}
fn transport_error(method: &str, raw_url: &str, err: &TransportError, basic: bool) -> ClientError {
    let (failed_url, err) = match err {
        TransportError::After { url, inner, .. } => (url.as_str(), &**inner),
        other => (raw_url, other),
    };
    let (reported, class) = match err {
        TransportError::Timeout => (failed_url, CauseClass::Timeout),
        TransportError::Canceled => (failed_url, CauseClass::Canceled),
        TransportError::ConnectionBroken => (failed_url, CauseClass::ConnectionBroken),
        TransportError::Redirect { location, limit } => (
            location.as_str(),
            match (basic, *limit) {
                (true, limit) => CauseClass::InsecureBasicAuth { redirect: !limit },
                (false, false) => CauseClass::RedirectOrigin,
                (false, true) => CauseClass::Other,
            },
        ),
        TransportError::Failed(cause) => (failed_url, CauseClass::Transport(cause.clone())),
        TransportError::After { .. } | TransportError::Other => (failed_url, CauseClass::Other),
    };
    let kind = if class == CauseClass::Timeout {
        ErrorKind::Timeout
    } else {
        ErrorKind::RequestFailure
    };
    let text = format!(
        "{} {}: HTTP client operation failed",
        url_error_op(method),
        quote(&super::types::safe_diagnostic_url(reported))
    );
    web(kind, raw_url, 0, Cause { text, class })
}
pub(super) fn response_from_http(raw_url: &str, http: &HttpResponse) -> Response {
    Response {
        url: safe_url(raw_url),
        status_code: http.status_code,
        status: safe_http_status(http.status_code),
        header: http.header.clone(),
        body: Vec::new(),
    }
}
fn normalized_transfer_syntax_uids(uids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for uid in uids {
        let uid = trim_space(uid);
        if uid.is_empty() || out.iter().any(|u| u == uid) {
            continue;
        }
        out.push(uid.to_string());
    }
    out
}
fn accept_suffixes(groups: &[String]) -> Vec<String> {
    (0..groups.len()).map(|i| groups[i..].join(", ")).collect()
}
fn retrieve_object_accepts(opts: &RetrieveOptions) -> Vec<String> {
    let uids = normalized_transfer_syntax_uids(&opts.transfer_syntax_uids);
    if uids.is_empty() {
        return vec![ACCEPT_DICOM.to_string()];
    }
    let groups: Vec<String> = uids
        .iter()
        .map(|uid| {
            format!(
                "multipart/related; type=\"application/dicom\"; transfer-syntax={uid}, application/dicom; transfer-syntax={uid}"
            )
        })
        .collect();
    accept_suffixes(&groups)
}
fn retrieve_frame_media_type(uid: &str) -> &'static str {
    let any = |list: &[&transfer::Syntax]| list.iter().any(|s| s.uid == uid);
    if any(&[
        &transfer::JPEG_BASELINE,
        &transfer::JPEG_EXTENDED,
        &transfer::JPEG_LOSSLESS_NON_HIERARCHICAL,
        &transfer::JPEG_LOSSLESS_SV1,
    ]) {
        "image/jpeg"
    } else if any(&[
        &transfer::JPEG_LS_LOSSLESS,
        &transfer::JPEG_LS_NEAR_LOSSLESS,
    ]) {
        "image/jls"
    } else if any(&[&transfer::JPEG_2000_LOSSLESS_ONLY, &transfer::JPEG_2000]) {
        "image/jp2"
    } else if any(&[
        &transfer::JPEG_2000_PART2_LOSSLESS,
        &transfer::JPEG_2000_PART2,
    ]) {
        "image/jpx"
    } else if any(&[&transfer::RLE_LOSSLESS]) {
        "image/dicom-rle"
    } else {
        "application/octet-stream"
    }
}
fn retrieve_frames_accepts(opts: &RetrieveOptions) -> Vec<String> {
    let uids = normalized_transfer_syntax_uids(&opts.transfer_syntax_uids);
    if uids.is_empty() {
        return vec!["multipart/related; type=\"application/octet-stream\"".to_string()];
    }
    let groups: Vec<String> = uids
        .iter()
        .map(|uid| {
            format!(
                "multipart/related; type=\"{}\"; transfer-syntax={uid}",
                retrieve_frame_media_type(uid)
            )
        })
        .collect();
    accept_suffixes(&groups)
}
fn accept_refused(status: i64) -> bool {
    matches!(status, 406 | 415 | 400 | 500 | 501)
}
pub(super) struct BodyLimitReader<'a> {
    pub(super) body: &'a mut dyn ByteRead,
    pub(super) remaining: i64,
    pub(super) limit: i64,
    pub(super) resp: &'a Response,
    pub(super) raw: bool,
}
impl BodyLimitReader<'_> {
    fn source(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        if self.raw {
            return self.body.read(buf);
        }
        match self.body.read(buf) {
            (n, Some(ReadErr::UnexpectedEof)) => (
                n,
                Some(ReadErr::Decode(ResponseDecodeError::new(K::TruncatedBody))),
            ),
            other => other,
        }
    }
}
impl ByteRead for BodyLimitReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        if buf.is_empty() {
            return (0, None);
        }
        if self.remaining == 0 {
            let mut extra = [0u8; 1];
            let (n, err) = self.source(&mut extra);
            if n > 0 {
                return (
                    0,
                    Some(ReadErr::Web(Box::new(Error::new(
                        ErrorKind::RequestFailure,
                        &self.resp.url,
                        self.resp.status_code,
                        Some(body_limit_cause(self.limit)),
                    )))),
                );
            }
            return (0, err);
        }
        let want = (buf.len() as i64).min(self.remaining) as usize;
        let (n, err) = self.source(&mut buf[..want]);
        self.remaining -= n as i64;
        (n, err)
    }
}
pub(super) fn body_limit_cause(limit: i64) -> Cause {
    Cause {
        text: format!("{ERR_RESPONSE_BODY_LIMIT} of {limit} bytes"),
        class: CauseClass::BodyLimit,
    }
}
struct PartLimitReader<'a> {
    reader: &'a mut dyn ByteRead,
    remaining: i64,
    limit: i64,
}
impl ByteRead for PartLimitReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        if buf.is_empty() {
            return (0, None);
        }
        if self.remaining == 0 {
            let mut extra = [0u8; 1];
            let (n, err) = self.reader.read(&mut extra);
            if n > 0 {
                return (
                    0,
                    Some(ReadErr::Decode(ResponseDecodeError::with(
                        K::PartBytes,
                        self.limit,
                        self.limit.saturating_add(1),
                    ))),
                );
            }
            return (0, err.map(response::response_part_read_error));
        }
        let want = (buf.len() as i64).min(self.remaining) as usize;
        let (n, err) = self.reader.read(&mut buf[..want]);
        self.remaining -= n as i64;
        (n, err.map(response::response_part_read_error))
    }
}
fn transport_read_failure(err: &ReadErr) -> bool {
    matches!(err, ReadErr::Net { .. })
}
fn read_request_error(resp: &Response, err: &ReadErr) -> ClientError {
    let kind = if matches!(err, ReadErr::Net { timeout: true, .. }) {
        ErrorKind::Timeout
    } else {
        ErrorKind::RequestFailure
    };
    web(kind, &resp.url, 0, read_cause(err))
}
fn object_part_drain_error(resp: &Response, err: ReadErr) -> ClientError {
    match response::response_part_read_error(err) {
        ReadErr::Decode(e) => decode_kind(resp, e),
        ReadErr::Web(e) => ClientError::Web(*e),
        e if transport_read_failure(&e) => read_request_error(resp, &e),
        e => ClientError::Read(e),
    }
}
fn drain(reader: &mut dyn ByteRead) -> Option<ReadErr> {
    let mut sink = vec![0u8; 32 << 10];
    loop {
        match reader.read(&mut sink) {
            (_, None) => {}
            (_, Some(ReadErr::Eof)) => return None,
            (_, Some(e)) => return Some(e),
        }
    }
}
struct StowBody<'a> {
    instances: &'a mut [StoreInstance],
    boundary: String,
    index: usize,
    pending: Vec<u8>,
    offset: usize,
    payload: Option<Payload>,
    from_reader: bool,
    data_pos: Option<usize>,
    written: u64,
    finished: bool,
    failure: Option<StorePayloadError>,
}
impl Drop for StowBody<'_> {
    fn drop(&mut self) {
        self.release();
    }
}
impl StowBody<'_> {
    fn release(&mut self) {
        let payload = self.payload.take();
        self.data_pos = None;
        if self.from_reader
            && let Some(instance) = self.instances.get_mut(self.index)
        {
            instance.reader = payload;
        }
        self.from_reader = false;
    }
    fn fail(&mut self, err: String) -> (usize, Option<ReadErr>) {
        let instance = &self.instances[self.index];
        let failure = StorePayloadError {
            index: self.index,
            path: instance.path.clone(),
            sop_instance_uid: instance.sop_instance_uid.clone(),
            err,
        };
        let text = failure.to_string();
        self.failure = Some(failure);
        (0, Some(ReadErr::Other(text)))
    }
}
impl ByteRead for StowBody<'_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        loop {
            if self.offset < self.pending.len() {
                let n = buf.len().min(self.pending.len() - self.offset);
                buf[..n].copy_from_slice(&self.pending[self.offset..self.offset + n]);
                self.offset += n;
                return (n, None);
            }
            if self.failure.is_some() {
                return (0, Some(ReadErr::Other("STOW-RS payload failed".into())));
            }
            if self.finished {
                return (0, Some(ReadErr::Eof));
            }
            let step = if let Some(payload) = self.payload.as_mut() {
                Some(payload.read(buf))
            } else if let Some(pos) = self.data_pos.as_mut() {
                Some(SliceReaderAt(&self.instances[self.index].data, pos).read(buf))
            } else {
                None
            };
            if let Some(step) = step {
                match step {
                    (n, None) if n > 0 => {
                        self.written += n as u64;
                        return (n, None);
                    }
                    (_, None) => continue,
                    (n, Some(ReadErr::Eof)) => {
                        self.written += n as u64;
                        let empty = self.written == 0;
                        self.release();
                        if empty {
                            return self.fail("empty payload".into());
                        }
                        self.index += 1;
                        if n > 0 {
                            return (n, None);
                        }
                    }
                    (_, Some(e)) => {
                        self.release();
                        return self.fail(e.to_string());
                    }
                }
                continue;
            }
            if self.index >= self.instances.len() {
                self.pending = format!("\r\n--{}--\r\n", self.boundary).into_bytes();
                self.offset = 0;
                self.finished = true;
                continue;
            }
            let lead = if self.index == 0 { "" } else { "\r\n" };
            self.pending = format!(
                "{lead}--{}\r\nContent-Type: application/dicom\r\n\r\n",
                self.boundary
            )
            .into_bytes();
            self.offset = 0;
            self.written = 0;
            let instance = &mut self.instances[self.index];
            if let Some(open) = &instance.open {
                match open() {
                    Ok(reader) => self.payload = Some(reader),
                    Err(e) => {
                        self.pending.clear();
                        return self.fail(format!("open: {e}"));
                    }
                }
            } else if let Some(reader) = instance.reader.take() {
                self.payload = Some(reader);
                self.from_reader = true;
            } else {
                self.data_pos = Some(0);
            }
        }
    }
}
struct SliceReaderAt<'a>(&'a [u8], &'a mut usize);
impl SliceReaderAt<'_> {
    fn read(&mut self, buf: &mut [u8]) -> (usize, Option<ReadErr>) {
        if *self.1 >= self.0.len() {
            return (0, Some(ReadErr::Eof));
        }
        let n = buf.len().min(self.0.len() - *self.1);
        buf[..n].copy_from_slice(&self.0[*self.1..*self.1 + n]);
        *self.1 += n;
        (n, None)
    }
}
pub(super) fn random_boundary() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::with_capacity(64);
    while out.len() < 60 {
        let bits = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        out.push_str(&format!("{bits:016x}"));
    }
    out.truncate(60);
    out
}
fn reborrow<'s>(body: &'s mut Option<&mut dyn ByteRead>) -> Option<&'s mut (dyn ByteRead + 's)> {
    match body {
        Some(reader) => Some(&mut **reader),
        None => None,
    }
}
impl Client {
    pub(super) fn limits(&self) -> Result<ParseLimits, ClientError> {
        response::response_limits(&self.options.response_limits, self.options.max_body_bytes)
            .map_err(|e| request_failure(&e))
    }
    pub(super) fn max_body_bytes(&self) -> i64 {
        if self.options.max_body_bytes > 0 {
            self.options.max_body_bytes
        } else {
            DEFAULT_MAX_BODY_BYTES
        }
    }
    fn uses_basic_auth(&self) -> bool {
        self.options.bearer_token_source.is_none()
            && trim_space(&self.options.bearer_token).is_empty()
            && !trim_space(&self.options.basic_username).is_empty()
    }
    fn redirect_check_bound(&self) -> Result<bool, String> {
        let basic = self.uses_basic_auth();
        Ok(match self.options.redirect_policy {
            RedirectPolicy::Default => basic,
            RedirectPolicy::Authenticated => {
                basic
                    || self.options.bearer_token_source.is_some()
                    || !trim_space(&self.options.bearer_token).is_empty()
            }
            RedirectPolicy::SameOrigin => true,
            RedirectPolicy::Invalid => return Err("invalid DICOMweb redirect policy".into()),
        })
    }
    pub(super) fn redirect_check(
        &self,
        target: &Url,
        same_origin_redirect: bool,
    ) -> Result<RedirectCheck, ClientError> {
        let invalid = |text: String, class: CauseClass| {
            ClientError::Web(Error {
                kind: ErrorKind::InvalidEndpoint,
                url: String::new(),
                status_code: 0,
                status: String::new(),
                err: Some(Cause { text, class }),
            })
        };
        let basic = self.uses_basic_auth();
        let origin_bound = match self.options.redirect_policy {
            RedirectPolicy::Default => basic,
            RedirectPolicy::Authenticated => {
                basic
                    || self.options.bearer_token_source.is_some()
                    || !trim_space(&self.options.bearer_token).is_empty()
            }
            RedirectPolicy::SameOrigin => true,
            RedirectPolicy::Invalid => {
                return Err(invalid(
                    "invalid DICOMweb redirect policy".into(),
                    CauseClass::Other,
                ));
            }
        };
        if basic {
            let insecure = |detail: &str| {
                invalid(
                    format!("{ERR_INSECURE_BASIC_AUTH}: {detail}"),
                    CauseClass::InsecureBasicAuth { redirect: false },
                )
            };
            if !target.scheme.eq_ignore_ascii_case("https") || trim_space(&target.host).is_empty() {
                return Err(insecure("endpoint must use HTTPS"));
            }
            if target.user.is_some() {
                return Err(insecure("URL user information is not allowed"));
            }
            if matches!(&self.options.https, Some(https) if https.insecure_skip_verify) {
                return Err(insecure("TLS certificate verification is disabled"));
            }
            return Ok(RedirectCheck::SameOrigin);
        }
        Ok(if origin_bound || same_origin_redirect {
            RedirectCheck::SameOrigin
        } else {
            RedirectCheck::Default
        })
    }
    pub(super) fn request_headers(
        &self,
        ctx: &Ctx,
        url: &Url,
        content_type: &str,
        accept: &str,
    ) -> Result<(RequestHeaders, Option<AccessToken>), ClientError> {
        let mut headers = vec![("Accept".to_string(), accept.to_string())];
        if !content_type.is_empty() {
            headers.push(("Content-Type".to_string(), content_type.to_string()));
        }
        if let Some(source) = &self.options.bearer_token_source {
            let token_error = |e: TokenError| {
                let (text, class) = match e {
                    TokenError::Canceled => ("context canceled", CauseClass::Canceled),
                    TokenError::DeadlineExceeded => {
                        ("context deadline exceeded", CauseClass::Timeout)
                    }
                    TokenError::SignInRequired => {
                        (ERR_BEARER_SIGN_IN_REQUIRED, CauseClass::SignInRequired)
                    }
                    TokenError::Unavailable => {
                        (ERR_BEARER_TOKEN_UNAVAILABLE, CauseClass::TokenUnavailable)
                    }
                };
                web(
                    ErrorKind::AuthToken,
                    &url.string(),
                    0,
                    Cause {
                        text: text.to_string(),
                        class,
                    },
                )
            };
            let token = source.token(ctx).map_err(token_error)?;
            if !token.valid_now() {
                return Err(token_error(TokenError::Unavailable));
            }
            headers.push(("Authorization".into(), format!("Bearer {}", token.value())));
            return Ok((headers, Some(token)));
        }
        let token = trim_space(&self.options.bearer_token);
        let user = trim_space(&self.options.basic_username);
        if !token.is_empty() {
            headers.push(("Authorization".into(), format!("Bearer {token}")));
        } else if !user.is_empty() {
            let credentials = format!("{user}:{}", self.options.basic_password);
            headers.push((
                "Authorization".into(),
                format!(
                    "Basic {}",
                    crate::dicomjson::sizedparse::base64_encode(credentials.as_bytes())
                ),
            ));
        }
        Ok((headers, None))
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn do_http_attempt(
        &self,
        ctx: &Ctx,
        method: &str,
        url: &Url,
        mut body: Option<&mut dyn ByteRead>,
        content_type: &str,
        accept: &str,
        options: &RequestOptions,
    ) -> Result<HttpResponse, ClientError> {
        let request_ctx = if self.options.timeout.0 > 0 {
            ctx.with_deadline(Instant::now() + Duration::from_nanos(self.options.timeout.0 as u64))
        } else if ctx.deadline().is_none() {
            ctx.with_deadline(Instant::now() + Duration::from_nanos(DEFAULT_TIMEOUT_NANOS as u64))
        } else {
            ctx.clone()
        };
        let mut check = self.redirect_check(url, options.same_origin_redirect)?;
        if options.no_follow {
            check = match check {
                RedirectCheck::SameOrigin => RedirectCheck::SameOriginNoFollow,
                _ => RedirectCheck::NoFollow,
            };
        }
        let basic = self.uses_basic_auth();
        let raw_url = url.string();
        let (mut headers, token) = self.request_headers(&request_ctx, url, content_type, accept)?;
        headers.extend(options.headers.iter().cloned());
        let has_body = body.is_some();
        let request = HttpRequest {
            method,
            url,
            headers,
            body: reborrow(&mut body),
            https: self.options.https.as_ref(),
            identity: self.options.https_identity.as_ref(),
        };
        let mut response = http::send(request, check, &request_ctx)
            .map_err(|e| transport_error(method, &raw_url, &e, basic))?;
        if response.status_code == 401
            && let (Some(token), Some(source)) = (&token, &self.options.bearer_token_source)
            && source.can_invalidate()
        {
            source.invalidate(token);
            if !has_body {
                drop(response);
                let (mut headers, _) =
                    self.request_headers(&request_ctx, url, content_type, accept)?;
                headers.extend(options.headers.iter().cloned());
                let retry = HttpRequest {
                    method,
                    url,
                    headers,
                    body: None,
                    https: self.options.https.as_ref(),
                    identity: self.options.https_identity.as_ref(),
                };
                response = http::send(retry, check, &request_ctx)
                    .map_err(|e| transport_error(method, &raw_url, &e, basic))?;
            }
        }
        Ok(response)
    }
    pub(super) fn do_http(
        &self,
        ctx: &Ctx,
        method: &str,
        url: &Url,
        body: Option<&mut dyn ByteRead>,
        content_type: &str,
        accept: &str,
    ) -> Result<HttpResponse, ClientError> {
        let options = RequestOptions::default();
        self.do_http_opts(ctx, method, url, body, content_type, accept, &options)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn do_http_opts(
        &self,
        ctx: &Ctx,
        method: &str,
        url: &Url,
        mut body: Option<&mut dyn ByteRead>,
        content_type: &str,
        accept: &str,
        options: &RequestOptions,
    ) -> Result<HttpResponse, ClientError> {
        let mut settings =
            RetrySettings::from(self.options.retry.as_ref()).map_err(|e| request_failure(&e))?;
        if body.is_some() {
            settings.attempts = 1;
        }
        let mut attempt = 1;
        loop {
            let result = self.do_http_attempt(
                ctx,
                method,
                url,
                reborrow(&mut body),
                content_type,
                accept,
                options,
            );
            if attempt >= settings.attempts || ctx.err().is_some() {
                return result;
            }
            let mut status_code = 0;
            let mut delay;
            match result {
                Err(ClientError::Web(e))
                    if matches!(e.kind, ErrorKind::RequestFailure | ErrorKind::Timeout) =>
                {
                    delay = settings.backoff(attempt);
                }
                Err(e) => return Err(e),
                Ok(response) => {
                    status_code = response.status_code;
                    if !settings.statuses.contains(&status_code) {
                        return Ok(response);
                    }
                    delay = settings.backoff(attempt);
                    if let Some(requested) = retry_after(&response.header) {
                        if requested > settings.max {
                            return Ok(response);
                        }
                        delay = delay.max(requested);
                    }
                }
            }
            if let Some(on_retry) = &settings.on_retry {
                on_retry(attempt, status_code, Nanos(delay));
            }
            if let Some(err) = sleep_context(ctx, delay) {
                return Err(transport_error_text(&url.string(), err));
            }
            attempt += 1;
        }
    }
    fn do_accept_get(
        &self,
        ctx: &Ctx,
        url: &Url,
        accepts: &[String],
    ) -> Result<HttpResponse, ClientError> {
        if accepts.len() == 1 {
            return self.do_http(ctx, "GET", url, None, "", &accepts[0]);
        }
        let mut i = 0;
        loop {
            let mut response = self.do_http(ctx, "GET", url, None, "", &accepts[i])?;
            let last = i + 1 == accepts.len();
            if !accept_refused(response.status_code) || (last && response.status_code != 406) {
                return Ok(response);
            }
            response.body = Body::empty();
            if let Some(kind) = ctx.err() {
                let err = if matches!(kind, crate::net::ul::Kind::Canceled) {
                    ReadErr::Canceled
                } else {
                    ReadErr::Deadline
                };
                return Err(context_request_error(&url.string(), &err));
            }
            if last {
                return Ok(response);
            }
            i += 1;
        }
    }
    pub(super) fn buffer_response(
        &self,
        url: &Url,
        mut http: HttpResponse,
    ) -> (Response, Option<ClientError>) {
        let max = self.max_body_bytes();
        let mut resp = response_from_http(&url.string(), &http);
        let mut limited = response::LimitReader {
            reader: &mut http.body,
            remaining: max.saturating_add(1),
        };
        let (data, err) = read_all(&mut limited);
        if let Some(e) = err {
            let failure = web(
                ErrorKind::RequestFailure,
                &resp.url,
                resp.status_code,
                read_cause(&e),
            );
            return (resp, Some(failure));
        }
        if data.len() as i64 > max {
            let failure = web(
                ErrorKind::RequestFailure,
                &resp.url,
                resp.status_code,
                body_limit_cause(max),
            );
            return (resp, Some(failure));
        }
        resp.body = data;
        (resp, None)
    }
    fn do_get_accepts(
        &self,
        ctx: &Ctx,
        url: &Url,
        accepts: &[String],
    ) -> Result<(Response, String), ClientError> {
        self.limits()?;
        let mut key = None;
        if let Some(cache) = &self.options.cache {
            let origin_bound = match self.redirect_check_bound() {
                Ok(bound) => bound,
                Err(text) => {
                    return Err(ClientError::Web(Error::invalid_endpoint(&text)));
                }
            };
            let cache_key = super::clientx::ResponseCacheKey {
                url: url.string(),
                accept: accepts[0].clone(),
                origin_bound,
            };
            if let Some(found) = cache.get(&cache_key) {
                return Ok(found);
            }
            key = Some(cache_key);
        }
        let http = self.do_accept_get(ctx, url, accepts)?;
        let mut redirected = String::new();
        if http.request_url != *url {
            let mut received = http.request_url.clone();
            received.fragment.clear();
            received.raw_fragment.clear();
            redirected = received.string();
        }
        match self.buffer_response(url, http) {
            (resp, None) => {
                if let (Some(cache), Some(key)) = (&self.options.cache, key) {
                    cache.put(key, &resp, &redirected);
                }
                Ok((resp, redirected))
            }
            (_, Some(e)) => Err(e),
        }
    }
    pub(crate) fn metadata_accept_header(&self) -> String {
        let mut seen: Vec<&str> = Vec::new();
        let mut values: Vec<String> = Vec::new();
        for media_type in &self.options.metadata_media_types {
            if seen.contains(&media_type.as_str()) {
                continue;
            }
            seen.push(media_type);
            let mut value = match media_type.as_str() {
                METADATA_MEDIA_TYPE_DICOM_JSON => media_type.clone(),
                METADATA_MEDIA_TYPE_DICOM_XML => {
                    "multipart/related; type=\"application/dicom+xml\"".to_string()
                }
                _ => continue,
            };
            if !values.is_empty() {
                let quality = 1.0 - values.len() as f64 / 10.0;
                value.push_str(&format!(";q={quality:.1}"));
            }
            values.push(value);
        }
        if values.is_empty() {
            return ACCEPT_DICOM_JSON.to_string();
        }
        values.join(", ")
    }
    pub(super) fn validate_metadata_response_options(&self) -> Result<(), ClientError> {
        if self.options.max_metadata_parts < 0 {
            return Err(request_failure("MaxMetadataParts must not be negative"));
        }
        self.limits().map(|_| ())
    }
    pub(super) fn metadata_dataset_limit(&self, limits: &ParseLimits) -> i64 {
        let maximum = if self.options.max_metadata_parts == 0 {
            DEFAULT_MAX_METADATA_PARTS
        } else {
            self.options.max_metadata_parts
        };
        maximum.min(limits.max_parts)
    }
    pub(super) fn metadata_media_type_configured(&self, want: &str) -> bool {
        let configured = &self.options.metadata_media_types;
        if configured.is_empty() {
            return want == METADATA_MEDIA_TYPE_DICOM_JSON;
        }
        if configured.iter().any(|m| m == want) {
            return true;
        }
        let valid = configured
            .iter()
            .any(|m| m == METADATA_MEDIA_TYPE_DICOM_JSON || m == METADATA_MEDIA_TYPE_DICOM_XML);
        !valid && want == METADATA_MEDIA_TYPE_DICOM_JSON
    }
    fn datasets_from_metadata_response(
        &self,
        ctx: &Ctx,
        resp: &Response,
        require_content_location: bool,
    ) -> Result<Option<Vec<Dataset>>, Cause> {
        if let Some(kind) = ctx.err() {
            return Err(if matches!(kind, crate::net::ul::Kind::Canceled) {
                read_cause(&ReadErr::Canceled)
            } else {
                read_cause(&ReadErr::Deadline)
            });
        }
        let limits =
            response::response_limits(&self.options.response_limits, self.options.max_body_bytes)
                .map_err(Cause::text)?;
        let content_types = resp.header.values("Content-Type");
        let blank = trim_space(&String::from_utf8_lossy(&resp.body)).is_empty();
        if blank && content_types.is_empty() {
            return Ok(None);
        }
        let content_type = response::response_content_type(content_types).map_err(Cause::decode)?;
        let parsed = response::parse_response_media_type(&content_type).map_err(Cause::decode)?;
        if metadata_transfer_syntax(parsed.param("transfer-syntax")).is_err()
            || !metadata_charset_supported(parsed.param("charset"))
        {
            return Err(Cause::text("unsupported metadata Content-Type parameters"));
        }
        let unexpected = || Cause::decode(ResponseDecodeError::new(K::UnexpectedMedia));
        const JSON_TYPES: [&str; 3] = [
            METADATA_MEDIA_TYPE_DICOM_JSON,
            "application/json",
            "text/plain",
        ];
        match parsed.media_type.as_str() {
            METADATA_MEDIA_TYPE_DICOM_JSON | "application/json" | "text/plain" => {
                if !response::media_allowed(&parsed.media_type, &JSON_TYPES, &limits.allowed_media)
                {
                    return Err(unexpected());
                }
                if !self.metadata_media_type_configured(METADATA_MEDIA_TYPE_DICOM_JSON) {
                    return Err(Cause::text(
                        "server returned an unrequested DICOM JSON representation",
                    ));
                }
                if !metadata_parameters_supported(&parsed.params, &["charset", "transfer-syntax"]) {
                    return Err(Cause::text("unsupported metadata Content-Type parameters"));
                }
                if blank {
                    return Ok(None);
                }
                let maximum = self.metadata_dataset_limit(&limits);
                let mut datasets: Vec<Dataset> = Vec::new();
                let mut limit_error: Option<ResponseDecodeError> = None;
                let json_limits = dicomjson::Limits {
                    max_json_bytes: resp.body.len() as i64,
                    ..dicomjson::Limits::default()
                };
                let decoded =
                    dicomjson::decode_raw_datasets_text(&resp.body, json_limits, &mut |d| {
                        if require_content_location && datasets.len() as i64 >= maximum {
                            let e = ResponseDecodeError::with(
                                K::PartLimit,
                                maximum,
                                datasets.len() as i64 + 1,
                            );
                            limit_error = Some(e);
                            return Err(e.to_string());
                        }
                        datasets.push(d);
                        Ok(())
                    });
                if let Some(e) = limit_error {
                    return Err(Cause::decode(e));
                }
                decoded.map_err(Cause::text)?;
                Ok(Some(datasets))
            }
            "multipart/related" => {
                if limits.max_depth < 1 {
                    return Err(Cause::decode(ResponseDecodeError::with(
                        K::MultipartDepth,
                        limits.max_depth,
                        1,
                    )));
                }
                if !self.metadata_media_type_configured(METADATA_MEDIA_TYPE_DICOM_XML) {
                    return Err(Cause::text(
                        "server returned an unrequested DICOM XML representation",
                    ));
                }
                if !metadata_parameters_supported(
                    &parsed.params,
                    &[
                        "boundary",
                        "charset",
                        "start",
                        "start-info",
                        "transfer-syntax",
                        "type",
                    ],
                ) {
                    return Err(Cause::text(
                        "unsupported multipart metadata Content-Type parameters",
                    ));
                }
                match response::parse_response_media_type(parsed.param("type")) {
                    Ok(declared)
                        if response::media_allowed(
                            &declared.media_type,
                            XML_POLICY.parts,
                            &limits.allowed_media,
                        ) => {}
                    _ => return Err(unexpected()),
                }
                let boundary = trim_space(parsed.param("boundary"));
                if boundary.is_empty() {
                    return Err(Cause::decode(ResponseDecodeError::new(K::MissingBoundary)));
                }
                if !response::valid_multipart_boundary(boundary) {
                    return Err(Cause::decode(ResponseDecodeError::new(
                        K::MalformedMultipart,
                    )));
                }
                self.datasets_from_dicom_xml_multipart(
                    resp,
                    boundary,
                    parsed.param("transfer-syntax"),
                    require_content_location,
                    &limits,
                )
            }
            _ => Err(unexpected()),
        }
    }
    fn datasets_from_dicom_xml_multipart(
        &self,
        resp: &Response,
        boundary: &str,
        outer_transfer_syntax: &str,
        require_content_location: bool,
        limits: &ParseLimits,
    ) -> Result<Option<Vec<Dataset>>, Cause> {
        if self.options.max_metadata_parts < 0 {
            return Err(Cause::text("MaxMetadataParts must not be negative"));
        }
        let maximum_parts = self.metadata_dataset_limit(limits);
        let maximum_bytes = limits.max_part_bytes;
        let mut body = SliceReader::new(&resp.body);
        let mut reader = MultipartReader::new(&mut body, boundary, limits.max_header_bytes);
        let mut datasets: Vec<Dataset> = Vec::new();
        loop {
            let header = match reader.next_part() {
                Ok(header) => header,
                Err(ReadErr::Eof) => break,
                Err(e) => return Err(Cause::decode(response::multipart_decode_error(&e))),
            };
            if datasets.len() as i64 >= maximum_parts {
                return Err(Cause::decode(ResponseDecodeError::with(
                    K::PartLimit,
                    maximum_parts,
                    datasets.len() as i64 + 1,
                )));
            }
            response::validate_part_header(&header, limits).map_err(Cause::decode)?;
            let part =
                response::validate_part_media_type(header.get("Content-Type"), &XML_POLICY, limits)
                    .map_err(Cause::decode)?;
            if !part
                .media_type
                .eq_ignore_ascii_case(METADATA_MEDIA_TYPE_DICOM_XML)
                || !metadata_charset_supported(part.param("charset"))
                || !metadata_parameters_supported(&part.params, &["charset", "transfer-syntax"])
            {
                return Err(Cause::text(
                    "multipart DICOM XML response contains an invalid or heterogeneous part",
                ));
            }
            let mut part_syntax = trim_space(part.param("transfer-syntax")).to_string();
            if part_syntax.is_empty() {
                part_syntax = outer_transfer_syntax.to_string();
            } else if !outer_transfer_syntax.is_empty()
                && crate::object::transfer::normalize_uid(&part_syntax)
                    != crate::object::transfer::normalize_uid(outer_transfer_syntax)
            {
                return Err(Cause::text(
                    "multipart DICOM XML transfer syntax parameters conflict",
                ));
            }
            let syntax = metadata_transfer_syntax(&part_syntax).map_err(Cause::text)?;
            let mut location = InstanceRef::default();
            if require_content_location {
                location =
                    metadata_ref_from_content_location(&resp.url, header.get("Content-Location"))
                        .map_err(Cause::text)?;
            }
            let data = response::read_response_part(
                &mut PartReader {
                    multipart: &mut reader,
                },
                limits,
            )
            .map_err(|e| read_cause(&response::response_part_read_error(e)))?;
            let dataset =
                dataset_from_dicom_xml(&data, maximum_bytes, syntax).map_err(Cause::text)?;
            if require_content_location {
                let found = InstanceRef {
                    study_instance_uid: element_string(&dataset, "0020000D"),
                    series_instance_uid: element_string(&dataset, "0020000E"),
                    sop_instance_uid: element_string(&dataset, "00080018"),
                };
                if found != location {
                    return Err(Cause::text(
                        "DICOM XML metadata Content-Location does not match the dataset instance UIDs",
                    ));
                }
            }
            datasets.push(dataset);
        }
        if datasets.is_empty() {
            if require_content_location {
                return Ok(None);
            }
            return Err(Cause::text(
                "multipart DICOM XML response contains no parts",
            ));
        }
        if !require_content_location && datasets.len() == 1 && datasets[0].is_empty() {
            return Ok(None);
        }
        Ok(Some(datasets))
    }
    pub fn verify(&self, ctx: &Ctx) -> (VerifyResult, Option<ClientError>) {
        let mut result = VerifyResult::default();
        if let Err(e) = self.validate_metadata_response_options() {
            return (result, Some(e));
        }
        let mut params = Values::new();
        params.set("limit", "1");
        let url = match self.endpoint.study_search_url(&params) {
            Ok(url) => url,
            Err(e) => return (result, Some(e.into())),
        };
        let started = Instant::now();
        let accept = self.metadata_accept_header();
        let outcome = self
            .do_http(ctx, "GET", &url, None, "", &accept)
            .map(|http| self.buffer_response(&url, http));
        result.duration = started.elapsed();
        let resp = match outcome {
            Err(e) => return (result, Some(e)),
            Ok((resp, Some(e))) => {
                result.response = resp;
                return (result, Some(e));
            }
            Ok((resp, None)) => resp,
        };
        result.response = resp;
        let resp = &result.response;
        if resp.status_code == 204 {
            return (result, None);
        }
        if !(200..300).contains(&resp.status_code) {
            let e = status_error(resp);
            return (result, Some(e));
        }
        if trim_space(&String::from_utf8_lossy(&resp.body)).is_empty() {
            return (result, None);
        }
        if let Err(cause) = self.datasets_from_metadata_response(ctx, resp, false) {
            let e = decode_error(resp, cause);
            return (result, Some(e));
        }
        (result, None)
    }
    fn search_at(&self, ctx: &Ctx, url: &Url) -> Result<Option<Vec<Dataset>>, ClientError> {
        self.search_at_response(ctx, url)
            .map(|(datasets, _)| datasets)
    }
    pub(super) fn search_at_response(
        &self,
        ctx: &Ctx,
        url: &Url,
    ) -> Result<(Option<Vec<Dataset>>, Response), ClientError> {
        let accept = self.metadata_accept_header();
        let (resp, _) = self.do_get_accepts(ctx, url, &[accept])?;
        if resp.status_code == 204 {
            return Ok((None, resp));
        }
        if !(200..300).contains(&resp.status_code) {
            return Err(status_error(&resp));
        }
        match self.datasets_from_metadata_response(ctx, &resp, false) {
            Ok(datasets) => Ok((datasets, resp)),
            Err(cause) => Err(decode_error(&resp, cause)),
        }
    }
    pub fn search_studies(
        &self,
        ctx: &Ctx,
        params: &Values,
    ) -> Result<Option<Vec<Dataset>>, ClientError> {
        self.validate_metadata_response_options()?;
        let url = self.endpoint.study_search_url(params)?;
        self.search_at(ctx, &url)
    }
    pub fn search_series(
        &self,
        ctx: &Ctx,
        study: &str,
        params: &Values,
    ) -> Result<Option<Vec<Dataset>>, ClientError> {
        self.validate_metadata_response_options()?;
        let url = self.endpoint.series_search_url(study, params)?;
        self.search_at(ctx, &url)
    }
    pub fn search_instances(
        &self,
        ctx: &Ctx,
        study: &str,
        series: &str,
        params: &Values,
    ) -> Result<Option<Vec<Dataset>>, ClientError> {
        self.validate_metadata_response_options()?;
        let url = self.endpoint.instance_search_url(study, series, params)?;
        self.search_at(ctx, &url)
    }
    fn retrieve_metadata(
        &self,
        ctx: &Ctx,
        url: &Url,
        scope: &InstanceRef,
    ) -> Result<MetadataResult, ClientError> {
        let accept = self.metadata_accept_header();
        let (resp, redirected) = self.do_get_accepts(ctx, url, &[accept])?;
        if !(200..300).contains(&resp.status_code) {
            return Err(status_error(&resp));
        }
        let datasets = self
            .datasets_from_metadata_response(ctx, &resp, true)
            .map_err(|cause| decode_error(&resp, cause))?;
        let refs = response::instance_refs_from_datasets(datasets.as_deref().unwrap_or(&[]), scope)
            .map_err(|text| {
                decode_error(
                    &resp,
                    Cause {
                        text,
                        class: CauseClass::InvalidMetadataIdentity,
                    },
                )
            })?;
        let request_url = if redirected.is_empty() {
            url.string()
        } else {
            redirected
        };
        Ok(MetadataResult {
            datasets,
            refs,
            request_url,
        })
    }
    pub fn study_metadata(&self, ctx: &Ctx, study: &str) -> Result<MetadataResult, ClientError> {
        self.validate_metadata_response_options()?;
        let url = self.endpoint.study_metadata_url(study)?;
        let scope = InstanceRef {
            study_instance_uid: study.to_string(),
            ..InstanceRef::default()
        };
        self.retrieve_metadata(ctx, &url, &scope)
    }
    pub fn series_metadata(
        &self,
        ctx: &Ctx,
        study: &str,
        series: &str,
    ) -> Result<MetadataResult, ClientError> {
        self.validate_metadata_response_options()?;
        let url = self.endpoint.series_metadata_url(study, series)?;
        let scope = InstanceRef {
            study_instance_uid: study.to_string(),
            series_instance_uid: series.to_string(),
            ..InstanceRef::default()
        };
        self.retrieve_metadata(ctx, &url, &scope)
    }
    pub fn instance_metadata(
        &self,
        ctx: &Ctx,
        instance: &InstanceRef,
    ) -> Result<MetadataResult, ClientError> {
        self.validate_metadata_response_options()?;
        let url = self.endpoint.instance_metadata_url(instance)?;
        self.retrieve_metadata(ctx, &url, instance)
    }
    pub fn retrieve_instance(
        &self,
        ctx: &Ctx,
        instance: &InstanceRef,
        opts: &RetrieveOptions,
    ) -> Result<Vec<ObjectPart>, ClientError> {
        let limits = self.limits()?;
        let url = self.endpoint.instance_url(instance)?;
        let (resp, _) = self.do_get_accepts(ctx, &url, &retrieve_object_accepts(opts))?;
        if !(200..300).contains(&resp.status_code) {
            return Err(status_error(&resp));
        }
        response::object_parts_from_response(&resp, &limits, &DICOM_OBJECT_POLICY).map_err(|e| {
            match e {
                PartsError::Decode(d) => decode_kind(&resp, d),
                PartsError::Read(r) => decode_error(&resp, read_cause(&r)),
            }
        })
    }
    pub fn retrieve_instance_stream(
        &self,
        ctx: &Ctx,
        instance: &InstanceRef,
        opts: &RetrieveOptions,
        handle: &mut dyn FnMut(PartStream) -> Result<(), ClientError>,
    ) -> Result<(), ClientError> {
        let url = self.endpoint.instance_url(instance)?;
        self.retrieve_stream_at(ctx, &url, opts, false, handle)
    }
    #[doc(hidden)]
    pub fn retrieve_study_stream(
        &self,
        ctx: &Ctx,
        study: &str,
        opts: &RetrieveOptions,
        handle: &mut dyn FnMut(PartStream) -> Result<(), ClientError>,
    ) -> Result<(), ClientError> {
        let url = self.endpoint.study_url(study)?;
        self.retrieve_stream_at(ctx, &url, opts, true, handle)
    }
    #[doc(hidden)]
    pub fn retrieve_series_stream(
        &self,
        ctx: &Ctx,
        study: &str,
        series: &str,
        opts: &RetrieveOptions,
        handle: &mut dyn FnMut(PartStream) -> Result<(), ClientError>,
    ) -> Result<(), ClientError> {
        let url = self.endpoint.series_url(study, series)?;
        self.retrieve_stream_at(ctx, &url, opts, true, handle)
    }
    fn retrieve_stream_at(
        &self,
        ctx: &Ctx,
        url: &Url,
        opts: &RetrieveOptions,
        require_multipart: bool,
        handle: &mut dyn FnMut(PartStream) -> Result<(), ClientError>,
    ) -> Result<(), ClientError> {
        self.limits()?;
        let mut http = self.do_accept_get(ctx, url, &retrieve_object_accepts(opts))?;
        let resp = response_from_http(&url.string(), &http);
        if !(200..300).contains(&resp.status_code) {
            return Err(status_error(&resp));
        }
        let mut policy = DICOM_OBJECT_POLICY;
        policy.require_multipart = require_multipart;
        self.stream_object_parts(&resp, &mut http.body, &policy, handle)
    }
    pub fn retrieve_frames(
        &self,
        ctx: &Ctx,
        instance: &InstanceRef,
        frames: &[i64],
        opts: &RetrieveOptions,
    ) -> Result<Vec<FramePart>, ClientError> {
        let mut parts: Vec<FramePart> = Vec::with_capacity(frames.len().min(1024));
        self.retrieve_frames_stream(ctx, instance, frames, opts, &mut |part| {
            let (data, err) = read_all(part.reader);
            if let Some(e) = err {
                return Err(match e {
                    ReadErr::Web(w) => ClientError::Web(*w),
                    other => ClientError::Read(other),
                });
            }
            parts.push(FramePart {
                frame_number: part.frame_number,
                content_type: part.content_type,
                transfer_syntax_uid: part.transfer_syntax_uid,
                data,
            });
            Ok(())
        })?;
        Ok(parts)
    }
    pub(crate) fn retrieve_frames_stream(
        &self,
        ctx: &Ctx,
        instance: &InstanceRef,
        frames: &[i64],
        opts: &RetrieveOptions,
        handle: &mut dyn FnMut(PartStream) -> Result<(), ClientError>,
    ) -> Result<(), ClientError> {
        self.limits()?;
        let url = self.endpoint.frames_url(instance, frames)?;
        let mut http = self.do_accept_get(ctx, &url, &retrieve_frames_accepts(opts))?;
        let resp = response_from_http(&url.string(), &http);
        if !(200..300).contains(&resp.status_code) {
            return Err(status_error(&resp));
        }
        let mut count = 0usize;
        self.stream_object_parts(&resp, &mut http.body, &FRAME_POLICY, &mut |mut part| {
            if count >= frames.len() {
                return Err(ClientError::Read(ReadErr::Decode(
                    ResponseDecodeError::with(K::PartLimit, frames.len() as i64, count as i64 + 1),
                )));
            }
            part.frame_number = frames[count];
            count += 1;
            handle(part)
        })?;
        if count != frames.len() {
            return Err(decode_error(
                &resp,
                Cause::text(format!(
                    "RetrieveFrames returned {count} parts for {} frames",
                    frames.len()
                )),
            ));
        }
        Ok(())
    }
    pub(super) fn stream_object_parts(
        &self,
        resp: &Response,
        body: &mut dyn ByteRead,
        policy: &MediaPolicy,
        handle: &mut dyn FnMut(PartStream) -> Result<(), ClientError>,
    ) -> Result<(), ClientError> {
        let limits = self.limits()?;
        let content_type = response::response_content_type(resp.header.values("Content-Type"))
            .map_err(|e| decode_kind(resp, e))?;
        let outer = response::validate_outer_response_media_type(&content_type, policy, &limits)
            .map_err(|e| decode_kind(resp, e))?;
        let max = self.max_body_bytes();
        let mut limited_body = BodyLimitReader {
            body,
            remaining: max,
            limit: max,
            resp,
            raw: false,
        };
        let handled = |result: Result<(), ClientError>| match result {
            Err(ClientError::Read(ReadErr::Decode(e))) => Err(decode_kind(resp, e)),
            other => other,
        };
        if outer.media_type == "multipart/related" {
            let mut reader = MultipartReader::new(
                &mut limited_body,
                outer.param("boundary"),
                limits.max_header_bytes,
            );
            let mut count: i64 = 0;
            loop {
                let header = match reader.next_part() {
                    Ok(header) => header,
                    Err(ReadErr::Eof) => {
                        if count == 0 {
                            return Err(decode_kind(
                                resp,
                                ResponseDecodeError::new(K::MalformedMultipart),
                            ));
                        }
                        return Ok(());
                    }
                    Err(ReadErr::Web(e)) => return Err(ClientError::Web(*e)),
                    Err(e @ (ReadErr::Canceled | ReadErr::Deadline)) => {
                        return Err(ClientError::Read(e));
                    }
                    Err(e) if transport_read_failure(&e) => {
                        return Err(read_request_error(resp, &e));
                    }
                    Err(e) => {
                        return Err(decode_kind(resp, response::multipart_decode_error(&e)));
                    }
                };
                count += 1;
                if count > limits.max_parts {
                    return Err(decode_kind(
                        resp,
                        ResponseDecodeError::with(K::PartLimit, limits.max_parts, count),
                    ));
                }
                response::validate_part_header(&header, &limits)
                    .map_err(|e| decode_kind(resp, e))?;
                let part_content_type = header.get("Content-Type").to_string();
                response::validate_part_media_type(&part_content_type, policy, &limits)
                    .map_err(|e| decode_kind(resp, e))?;
                let mut part = PartReader {
                    multipart: &mut reader,
                };
                let mut limited = PartLimitReader {
                    reader: &mut part,
                    remaining: limits.max_part_bytes,
                    limit: limits.max_part_bytes,
                };
                handled(handle(PartStream {
                    frame_number: 0,
                    transfer_syntax_uid: response::transfer_syntax_from_content_type(
                        &part_content_type,
                    ),
                    content_type: part_content_type,
                    content_location: header.get("Content-Location").to_string(),
                    reader: &mut limited,
                }))?;
                if let Some(e) = drain(&mut limited) {
                    return Err(object_part_drain_error(resp, e));
                }
            }
        }
        let mut limited = PartLimitReader {
            reader: &mut limited_body,
            remaining: limits.max_part_bytes,
            limit: limits.max_part_bytes,
        };
        handled(handle(PartStream {
            frame_number: 0,
            transfer_syntax_uid: response::transfer_syntax_from_content_type(&content_type),
            content_type,
            content_location: resp.header.get("Content-Location").to_string(),
            reader: &mut limited,
        }))?;
        if let Some(e) = drain(&mut limited) {
            return Err(object_part_drain_error(resp, e));
        }
        Ok(())
    }
    pub fn store_instances(
        &self,
        ctx: &Ctx,
        instances: &mut [StoreInstance],
    ) -> (StoreResult, Option<ClientError>) {
        if instances.is_empty() {
            return (StoreResult::default(), None);
        }
        match self.endpoint.store_studies_url() {
            Ok(url) => self.store_instances_at(ctx, &url, instances),
            Err(e) => (StoreResult::default(), Some(e.into())),
        }
    }
    pub fn store_instances_to_study(
        &self,
        ctx: &Ctx,
        study: &str,
        instances: &mut [StoreInstance],
    ) -> (StoreResult, Option<ClientError>) {
        if instances.is_empty() {
            return (StoreResult::default(), None);
        }
        match self.endpoint.store_study_url(study) {
            Ok(url) => self.store_instances_at(ctx, &url, instances),
            Err(e) => (StoreResult::default(), Some(e.into())),
        }
    }
    fn store_instances_at(
        &self,
        ctx: &Ctx,
        url: &Url,
        instances: &mut [StoreInstance],
    ) -> (StoreResult, Option<ClientError>) {
        for (index, instance) in instances.iter().enumerate() {
            if instance.open.is_none() && instance.reader.is_none() && instance.data.is_empty() {
                let result = StoreResult {
                    response: Response {
                        url: safe_url(&url.string()),
                        ..Response::default()
                    },
                    ..StoreResult::default()
                };
                let failure = StorePayloadError {
                    index,
                    path: instance.path.clone(),
                    sop_instance_uid: instance.sop_instance_uid.clone(),
                    err: "empty payload".into(),
                };
                return (result, Some(ClientError::StorePayload(failure)));
            }
        }
        let boundary = random_boundary();
        let content_type =
            format!("multipart/related; type=\"application/dicom\"; boundary={boundary}");
        let mut body = StowBody {
            instances,
            boundary,
            index: 0,
            pending: Vec::new(),
            offset: 0,
            payload: None,
            from_reader: false,
            data_pos: None,
            written: 0,
            finished: false,
            failure: None,
        };
        let sent = self.do_http(
            ctx,
            "POST",
            url,
            Some(&mut body),
            &content_type,
            ACCEPT_DICOM_JSON,
        );
        let (resp, failure) = match sent {
            Ok(http) => self.buffer_response(url, http),
            Err(ClientError::Web(mut e)) => {
                if let (Some(failure), Some(cause)) = (&body.failure, e.err.as_mut()) {
                    cause.class = CauseClass::StorePayload(Box::new(super::types::PayloadRef {
                        index: failure.index,
                        path: failure.path.clone(),
                        sop_instance_uid: failure.sop_instance_uid.clone(),
                    }));
                }
                (Response::default(), Some(ClientError::Web(e)))
            }
            Err(e) => (Response::default(), Some(e)),
        };
        if let Some(e) = failure {
            return (
                StoreResult {
                    response: resp,
                    ..StoreResult::default()
                },
                Some(e),
            );
        }
        let success = (200..300).contains(&resp.status_code);
        match response::store_result_from_dicom_json(&resp.body) {
            Err(text) => {
                let failure = if success {
                    decode_error(&resp, Cause::text(text))
                } else {
                    status_error(&resp)
                };
                (
                    StoreResult {
                        response: resp,
                        ..StoreResult::default()
                    },
                    Some(failure),
                )
            }
            Ok(mut parsed) => {
                let failure = (!success).then(|| status_error(&resp));
                parsed.response = resp;
                (parsed, failure)
            }
        }
    }
}
const XML_POLICY: MediaPolicy = MediaPolicy {
    require_multipart: true,
    plain_single_media: false,
    frames: false,
    single: &[],
    parts: &[METADATA_MEDIA_TYPE_DICOM_XML],
};
pub(super) fn context_request_error(raw_url: &str, err: &ReadErr) -> ClientError {
    let kind = if matches!(err, ReadErr::Deadline) {
        ErrorKind::Timeout
    } else {
        ErrorKind::RequestFailure
    };
    web(kind, raw_url, 0, read_cause(err))
}
fn transport_error_text(raw_url: &str, err: ReadErr) -> ClientError {
    let mut cause = read_cause(&err);
    cause.text = format!("retry wait: {}", cause.text);
    let kind = if matches!(err, ReadErr::Deadline) {
        ErrorKind::Timeout
    } else {
        ErrorKind::RequestFailure
    };
    web(kind, raw_url, 0, cause)
}
pub(super) fn sleep_context(ctx: &Ctx, delay_nanos: i64) -> Option<ReadErr> {
    let context_error = |ctx: &Ctx| {
        ctx.err().map(|kind| {
            if matches!(kind, crate::net::ul::Kind::Canceled) {
                ReadErr::Canceled
            } else {
                ReadErr::Deadline
            }
        })
    };
    if delay_nanos <= 0 {
        return context_error(ctx);
    }
    let end = Instant::now() + Duration::from_nanos(delay_nanos as u64);
    loop {
        if let Some(e) = context_error(ctx) {
            return Some(e);
        }
        let now = Instant::now();
        if now >= end {
            return None;
        }
        std::thread::sleep((end - now).min(Duration::from_millis(20)));
    }
}
pub(super) fn metadata_transfer_syntax(
    value: &str,
) -> Result<Option<crate::object::transfer::Syntax>, String> {
    let uid = crate::object::transfer::normalize_uid(trim_space(value));
    if uid.is_empty() {
        return Ok(None);
    }
    let unsupported = || format!("unsupported metadata transfer syntax {}", quote(uid));
    if !response::web_service_transfer_syntax_allowed(uid) {
        return Err(unsupported());
    }
    crate::object::transfer::lookup(uid)
        .map(Some)
        .ok_or_else(unsupported)
}
pub(super) fn metadata_charset_supported(charset: &str) -> bool {
    let charset = trim_space(charset);
    charset.is_empty() || charset.eq_ignore_ascii_case("utf-8")
}
pub(super) fn metadata_parameters_supported(params: &[(String, String)], allowed: &[&str]) -> bool {
    params
        .iter()
        .all(|(name, _)| allowed.contains(&to_lower(name).as_str()))
}
fn metadata_ref_from_content_location(
    response_url: &str,
    value: &str,
) -> Result<InstanceRef, String> {
    if trim_space(value).is_empty() {
        return Err("DICOM XML metadata part is missing Content-Location".into());
    }
    let base = urlrules::parse(response_url)?;
    let location = match urlrules::parse(value) {
        Ok(l)
            if l.user.is_none()
                && l.fragment.is_empty()
                && l.raw_query.is_empty()
                && l.raw_path.is_empty() =>
        {
            l
        }
        _ => return Err("invalid DICOM XML metadata Content-Location".into()),
    };
    let resolved = base.resolve_reference(&location);
    if !resolved.scheme.eq_ignore_ascii_case(&base.scheme)
        || !resolved.host.eq_ignore_ascii_case(&base.host)
        || resolved.path.is_empty()
    {
        return Err("DICOM XML metadata Content-Location is not same-origin".into());
    }
    let not_instance = || {
        "DICOM XML metadata Content-Location does not identify an instance metadata resource"
            .to_string()
    };
    let parts: Vec<&str> = resolved.path.trim_matches('/').split('/').collect();
    if parts.len() < 7 {
        return Err(not_instance());
    }
    let parts = &parts[parts.len() - 7..];
    let valid = |uid: &str| crate::object::is_valid_uid(uid.as_bytes());
    if parts[0] != "studies"
        || parts[2] != "series"
        || parts[4] != "instances"
        || parts[6] != "metadata"
        || !valid(parts[1])
        || !valid(parts[3])
        || !valid(parts[5])
    {
        return Err(not_instance());
    }
    Ok(InstanceRef {
        study_instance_uid: parts[1].to_string(),
        series_instance_uid: parts[3].to_string(),
        sop_instance_uid: parts[5].to_string(),
    })
}
fn dataset_from_dicom_xml(
    data: &[u8],
    maximum_bytes: i64,
    syntax: Option<crate::object::transfer::Syntax>,
) -> Result<Dataset, String> {
    use crate::dicomxml;
    let mut options = dicomxml::UnmarshalOptions {
        transfer_syntax: syntax,
        allow_foreign_root_attributes: true,
        limits: dicomxml::Limits {
            max_xml_bytes: maximum_bytes,
            max_value_bytes: maximum_bytes,
            max_inline_binary_bytes: maximum_bytes,
            max_total_binary_bytes: maximum_bytes,
            ..dicomxml::Limits::default()
        },
        ..dicomxml::UnmarshalOptions::default()
    };
    let object = dicomxml::unmarshal_text(data, &mut options)?;
    let mut json_options = dicomjson::Options {
        omit_group_length: true,
        limits: dicomjson::Limits {
            max_json_bytes: maximum_bytes,
            max_inline_binary_bytes: maximum_bytes,
            max_total_binary_bytes: maximum_bytes,
            max_sequence_depth: 64,
            max_elements: DEFAULT_MAX_METADATA_PARTS * 100,
            max_sequence_items: DEFAULT_MAX_METADATA_PARTS * 100,
        },
        ..dicomjson::Options::default()
    };
    let encoded = dicomjson::marshal_text(Some(&object), &mut json_options)?;
    response::dataset_from_json(&encoded)
}
