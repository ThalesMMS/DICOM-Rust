mod client;
mod errors;
mod json;
mod jwk;
mod jwt;
mod login;
use self::json::Fields;
use super::http::{self, HttpRequest, HttpsIdentity, HttpsOptions, RedirectCheck, TransportError};
use super::multipart::{ByteRead, ReadErr, SliceReader};
use super::urlrules::{self, Url, Values};
use crate::net::ul::{Ctx, Kind};
pub use client::{
    Client, Config, MemoryRefreshTokenStore, OidcRefreshFn, RefreshTokenStore, Token,
    TokenExpectations, UserInfo,
};
pub use errors::{
    ERROR_CONFIGURATION, ERROR_DISCOVERY, ERROR_LOGIN, ERROR_REFRESH, ERROR_REVOCATION, Error,
    OidcError, Sentinel, safe_provider_code,
};
pub use jwk::{
    JsonWebKey, KeySet, MAX_JWKS_KEYS, MIN_RSA_KEY_BITS, PublicKey, RemoteKeySet,
    RemoteKeySetOptions, StaticKeySet, parse_jwks,
};
pub use jwt::{
    AccessPolicy, DEFAULT_CLOCK_SKEW, IdToken, MAX_TOKEN_BYTES, Verifier, VerifierConfig,
    bearer_token, supported_algorithms, token_from_context,
};
pub use login::{LoginPrompt, LoginSession, pkce_challenge, random_url_token};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
pub const WELL_KNOWN_PATH: &str = "/.well-known/openid-configuration";
pub const MAX_RESPONSE_BYTES: usize = 1 << 20;
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 5;
pub type Clock = std::sync::Arc<dyn Fn() -> SystemTime + Send + Sync>;
pub const FLOW_AUTHORIZATION_CODE: &str = "authorization_code";
pub const FLOW_DEVICE_CODE: &str = "device_code";
pub(crate) fn unix_nanos(t: SystemTime) -> i128 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub device_authorization_endpoint: String,
    pub revocation_endpoint: String,
    pub userinfo_endpoint: String,
    pub jwks_uri: String,
    pub code_challenge_methods: Option<Vec<String>>,
    pub id_token_signing_algs: Option<Vec<String>>,
}
impl Metadata {
    fn decode(raw: &[u8]) -> Option<Metadata> {
        let f = Fields::new(
            raw,
            &[
                "issuer",
                "authorization_endpoint",
                "token_endpoint",
                "device_authorization_endpoint",
                "revocation_endpoint",
                "userinfo_endpoint",
                "jwks_uri",
                "code_challenge_methods_supported",
                "id_token_signing_alg_values_supported",
            ],
        )?;
        Some(Metadata {
            issuer: f.string(0)?,
            authorization_endpoint: f.string(1)?,
            token_endpoint: f.string(2)?,
            device_authorization_endpoint: f.string(3)?,
            revocation_endpoint: f.string(4)?,
            userinfo_endpoint: f.string(5)?,
            jwks_uri: f.string(6)?,
            code_challenge_methods: f.strings(7)?,
            id_token_signing_algs: f.strings(8)?,
        })
    }
}
pub(crate) fn trim_space(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}'
        ) || (c as u32 > 0xff && c.is_whitespace())
    })
}
pub(crate) fn equal_fold(a: &str, b: &str) -> bool {
    let fold = |c: char| match c {
        '\u{17F}' => 's',
        '\u{212A}' => 'k',
        c => c.to_ascii_lowercase(),
    };
    a.chars().map(fold).eq(b.chars().map(fold))
}
pub(crate) fn contains_fold(values: &[String], want: &str) -> bool {
    values.iter().any(|v| equal_fold(trim_space(v), want))
}
pub fn normalize_issuer(raw: &str) -> Result<String, OidcError> {
    let raw = trim_space(raw);
    match urlrules::parse(raw) {
        Ok(u)
            if u.scheme == "https"
                && !u.host.is_empty()
                && u.user.is_none()
                && u.raw_query.is_empty()
                && u.fragment.is_empty() =>
        {
            Ok(raw.strip_suffix('/').unwrap_or(raw).to_string())
        }
        _ => Err(OidcError::Sentinel(Sentinel::NotConfigured)),
    }
}
pub fn validate_secure_endpoint(raw: &str) -> Result<(), OidcError> {
    match urlrules::parse(trim_space(raw)) {
        Ok(u)
            if u.scheme == "https"
                && !u.host.is_empty()
                && u.user.is_none()
                && u.fragment.is_empty() =>
        {
            Ok(())
        }
        _ => Err(OidcError::Sentinel(Sentinel::NotConfigured)),
    }
}
pub fn same_origin(left: &Url, right: &Url) -> bool {
    http::same_origin(left, right)
}
pub fn validate_metadata(issuer: &str, flow: &str, metadata: &Metadata) -> Result<(), OidcError> {
    let discovery = |op: &str| Err(errors::new_error(ERROR_DISCOVERY, op, 0, "", None));
    if metadata.issuer != issuer {
        return discovery("validate issuer");
    }
    let mut required = vec![&metadata.token_endpoint, &metadata.jwks_uri];
    match flow {
        FLOW_DEVICE_CODE => required.push(&metadata.device_authorization_endpoint),
        FLOW_AUTHORIZATION_CODE => {
            required.push(&metadata.authorization_endpoint);
            if !contains_fold(
                metadata.code_challenge_methods.as_deref().unwrap_or(&[]),
                "S256",
            ) {
                return discovery("validate PKCE");
            }
        }
        "" => {}
        _ => {
            return Err(errors::new_error(
                ERROR_CONFIGURATION,
                "validate flow",
                0,
                "",
                None,
            ));
        }
    }
    for endpoint in required {
        if validate_secure_endpoint(endpoint).is_err() {
            return discovery("validate endpoint");
        }
    }
    for endpoint in [
        &metadata.authorization_endpoint,
        &metadata.device_authorization_endpoint,
        &metadata.revocation_endpoint,
        &metadata.userinfo_endpoint,
    ] {
        if !trim_space(endpoint).is_empty() && validate_secure_endpoint(endpoint).is_err() {
            return discovery("validate endpoint");
        }
    }
    if let Some(algs) = &metadata.id_token_signing_algs
        && !algs.is_empty()
        && supported_algorithms(algs).is_none()
    {
        return discovery("validate ID token signing algorithms");
    }
    Ok(())
}
pub fn discover(
    ctx: &Ctx,
    http: Option<&HttpClient>,
    issuer: &str,
    flow: &str,
) -> Result<Metadata, OidcError> {
    let Ok(issuer) = normalize_issuer(issuer) else {
        return Err(errors::new_error(ERROR_DISCOVERY, "discovery", 0, "", None));
    };
    let default = HttpClient::default_client();
    discover_with(ctx, http.unwrap_or(&default), &issuer, flow)
}
pub(crate) fn discover_with(
    ctx: &Ctx,
    http: &HttpClient,
    issuer: &str,
    flow: &str,
) -> Result<Metadata, OidcError> {
    let (status, result) = http.get_json(
        ctx,
        &format!("{issuer}{WELL_KNOWN_PATH}"),
        "",
        Metadata::decode,
    );
    let metadata = match result {
        Err(fail) => return Err(errors::context_error(ERROR_DISCOVERY, "discovery", fail)),
        Ok(None) => {
            return Err(errors::new_error(
                ERROR_DISCOVERY,
                "discovery",
                status,
                "",
                None,
            ));
        }
        Ok(Some(m)) => m,
    };
    validate_metadata(issuer, flow, &metadata)?;
    Ok(metadata)
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpClient {
    pub https: Option<HttpsOptions>,
    pub timeout: Option<Duration>,
    pub identity: Option<HttpsIdentity>,
}
impl HttpClient {
    pub(crate) fn default_client() -> HttpClient {
        HttpClient {
            https: None,
            timeout: Some(DEFAULT_HTTP_TIMEOUT),
            identity: None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fail {
    Canceled,
    Deadline,
    Other,
}
fn fail_of(ctx: &Ctx) -> Fail {
    match ctx.err() {
        Some(Kind::Canceled) => Fail::Canceled,
        Some(_) => Fail::Deadline,
        None => Fail::Other,
    }
}
impl HttpClient {
    fn request_ctx(&self, ctx: &Ctx) -> Ctx {
        match self.timeout {
            Some(t) => ctx.with_timeout(t),
            None => ctx.clone(),
        }
    }
    fn send(
        &self,
        ctx: &Ctx,
        method: &str,
        endpoint: &str,
        headers: Vec<(String, String)>,
        body: Option<&[u8]>,
    ) -> Result<http::HttpResponse, Fail> {
        let Ok(url) = urlrules::parse(endpoint) else {
            return Err(Fail::Other);
        };
        let mut reader = body.map(SliceReader::new);
        let request = HttpRequest {
            method,
            url: &url,
            headers,
            body: reader.as_mut().map(|r| r as &mut dyn ByteRead),
            https: self.https.as_ref(),
            identity: self.identity.as_ref(),
        };
        http::send_limited(request, RedirectCheck::SameOrigin, MAX_REDIRECTS, body, ctx).map_err(
            |e| match e {
                TransportError::Canceled => Fail::Canceled,
                TransportError::Timeout => match fail_of(ctx) {
                    Fail::Other => Fail::Other,
                    other => other,
                },
                _ => fail_of(ctx),
            },
        )
    }
    pub(crate) fn get_json<T>(
        &self,
        ctx: &Ctx,
        endpoint: &str,
        bearer: &str,
        decode: impl FnOnce(&[u8]) -> Option<T>,
    ) -> (i64, Result<Option<T>, Fail>) {
        let ctx = self.request_ctx(ctx);
        let mut headers = vec![("Accept".to_string(), "application/json".to_string())];
        if !bearer.is_empty() {
            headers.push(("Authorization".to_string(), format!("Bearer {bearer}")));
        }
        let mut response = match self.send(&ctx, "GET", endpoint, headers, None) {
            Ok(r) => r,
            Err(f) => return (0, Err(f)),
        };
        let status = response.status_code;
        if !(200..300).contains(&status) {
            discard_bounded(&mut response.body);
            return (status, Ok(None));
        }
        match read_bounded(&mut response.body, &ctx) {
            Err(f) => (status, Err(f)),
            Ok(data) => match json::strict_value(&data).and_then(decode) {
                Some(v) => (status, Ok(Some(v))),
                None => (status, Err(Fail::Other)),
            },
        }
    }
    pub(crate) fn post_form(
        &self,
        ctx: &Ctx,
        endpoint: &str,
        form: &Values,
    ) -> Result<(Vec<u8>, i64, String), Fail> {
        let ctx = self.request_ctx(ctx);
        let body = form.encode();
        let headers = vec![
            ("Accept".to_string(), "application/json".to_string()),
            (
                "Content-Type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ),
        ];
        let mut response = self.send(&ctx, "POST", endpoint, headers, Some(body.as_bytes()))?;
        let status = response.status_code;
        let data = read_bounded(&mut response.body, &ctx)?;
        let mut code = String::new();
        if !(200..300).contains(&status)
            && let Some(raw) = json::unmarshal_value(&data)
            && let Some(f) = Fields::new_lenient(raw, &["error"])
        {
            code = safe_provider_code(&f.string_lenient(0));
        }
        Ok((data, status, code))
    }
    pub(crate) fn get_raw(&self, ctx: &Ctx, endpoint: &str, accept: &str) -> Result<Vec<u8>, Fail> {
        let ctx = self.request_ctx(ctx);
        let headers = vec![("Accept".to_string(), accept.to_string())];
        let mut response = self.send(&ctx, "GET", endpoint, headers, None)?;
        if !(200..300).contains(&response.status_code) {
            discard_bounded(&mut response.body);
            return Err(Fail::Other);
        }
        read_bounded(&mut response.body, &ctx)
    }
}
fn read_bounded(body: &mut http::Body, ctx: &Ctx) -> Result<Vec<u8>, Fail> {
    let mut out = Vec::new();
    let mut chunk = vec![0u8; 32 << 10];
    loop {
        let room = MAX_RESPONSE_BYTES + 1 - out.len();
        if room == 0 {
            break;
        }
        let want = room.min(chunk.len());
        let (n, err) = body.read(&mut chunk[..want]);
        out.extend_from_slice(&chunk[..n]);
        match err {
            None => {}
            Some(ReadErr::Eof) => break,
            Some(ReadErr::Canceled) => return Err(Fail::Canceled),
            Some(ReadErr::Deadline) => return Err(Fail::Deadline),
            Some(_) => return Err(fail_of(ctx)),
        }
    }
    if out.len() > MAX_RESPONSE_BYTES {
        return Err(Fail::Other);
    }
    Ok(out)
}
fn discard_bounded(body: &mut http::Body) {
    let mut chunk = [0u8; 4096];
    let mut left = 4096;
    while left > 0 {
        let (n, err) = body.read(&mut chunk[..left]);
        left -= n;
        if err.is_some() {
            break;
        }
    }
}
pub(crate) fn b64_encode(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}
pub(crate) fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut values = Vec::with_capacity(text.len());
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'\r' | b'\n' => continue,
            _ => return None,
        };
        values.push(v);
    }
    if values.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(values.len() * 3 / 4);
    for chunk in values.chunks(4) {
        let mut n: u32 = 0;
        for (i, v) in chunk.iter().enumerate() {
            n |= u32::from(*v) << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
    Some(out)
}
