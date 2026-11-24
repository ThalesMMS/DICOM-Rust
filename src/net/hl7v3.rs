#![forbid(unsafe_code)]
use crate::dicomxml::xmlwire::{Decoder, Name, Tok, XmlError};
use crate::encapdoc::{self, PayloadLimits, inspect_payload};
use crate::net::dicomweb::http::{HttpRequest, RedirectCheck, send};
use crate::net::dicomweb::mimeheader::{parse_media_type, trim_space};
use crate::net::dicomweb::multipart::{ByteRead, ReadErr, SliceReader};
use crate::net::dicomweb::urlrules::{self, Url};
use crate::net::dicomweb::{HttpClientTrace, HttpsOptions};
use crate::net::ul::{Ctx, Kind as CtxKind};
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
const DEFAULT_MAX_BYTES: i64 = 4 << 20;
const MAX_PAYLOAD_BYTES: i64 = 64 << 20;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Endpoint,
    InsecureEndpoint,
    Options,
    Request,
    RequestTooLarge,
    Transport,
    Redirect,
    Status,
    Response,
    ResponseTooLarge,
    Context(CtxKind),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::Endpoint => "hl7v3: invalid CDA endpoint",
            Error::InsecureEndpoint => "hl7v3: CDA endpoint requires verified HTTPS",
            Error::Options => "hl7v3: invalid CDA transport limits",
            Error::Request => "hl7v3: invalid CDA document",
            Error::RequestTooLarge => "hl7v3: CDA document exceeds a size or structure limit",
            Error::Transport => "hl7v3: HTTP exchange failed",
            Error::Redirect => "hl7v3: HTTP redirection response refused",
            Error::Status => "hl7v3: peer returned an unsuccessful HTTP status",
            Error::Response => "hl7v3: invalid XML response",
            Error::ResponseTooLarge => "hl7v3: response exceeds a size or structure limit",
            Error::Context(CtxKind::Canceled) => "context canceled",
            Error::Context(_) => "context deadline exceeded",
        })
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryState {
    NotSent,
    Uncertain,
    Rejected,
}
impl DeliveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            DeliveryState::NotSent => "not-sent",
            DeliveryState::Uncertain => "uncertain",
            DeliveryState::Rejected => "rejected",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeliveryError {
    pub state: DeliveryState,
    pub status_code: i64,
    pub cause: Error,
}
impl fmt::Display for DeliveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.status_code != 0 {
            return write!(
                f,
                "{} ({}, HTTP {})",
                self.cause,
                self.state.as_str(),
                self.status_code
            );
        }
        write!(f, "{} ({})", self.cause, self.state.as_str())
    }
}
impl std::error::Error for DeliveryError {}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub timeout: Duration,
    pub max_request_bytes: i64,
    pub max_response_bytes: i64,
    pub root_certificates_der: Option<Vec<Vec<u8>>>,
    pub allow_loopback_http: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    pub status_code: i64,
    pub xml: Option<Vec<u8>>,
}
#[derive(Clone, Debug)]
pub struct Client {
    url: Url,
    https: HttpsOptions,
    options: Options,
}
fn is_loopback_literal(host: &str) -> bool {
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(a)) => a.is_loopback(),
        Ok(std::net::IpAddr::V6(a)) => {
            a.is_loopback() || a.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        Err(_) => false,
    }
}
impl Client {
    pub fn new(endpoint: &str, options: Options) -> Result<Client, Error> {
        let parsed = urlrules::parse(endpoint).map_err(|_| Error::Endpoint)?;
        if !parsed.opaque.is_empty()
            || parsed.hostname().is_empty()
            || parsed.user.is_some()
            || endpoint.contains('#')
            || endpoint.chars().any(char::is_whitespace)
            || parsed.host.ends_with(':')
        {
            return Err(Error::Endpoint);
        }
        let port = parsed.port();
        if !port.is_empty() && !port.parse::<u16>().is_ok_and(|p| p != 0) {
            return Err(Error::Endpoint);
        }
        match parsed.scheme.as_str() {
            "https" => {}
            "http" => {
                if !options.allow_loopback_http || !is_loopback_literal(parsed.hostname()) {
                    return Err(Error::InsecureEndpoint);
                }
            }
            _ => return Err(Error::Endpoint),
        }
        let mut options = options;
        if options.max_request_bytes < 0
            || options.max_request_bytes > MAX_PAYLOAD_BYTES
            || options.max_response_bytes < 0
            || options.max_response_bytes > MAX_PAYLOAD_BYTES
        {
            return Err(Error::Options);
        }
        if options.timeout.is_zero() {
            options.timeout = Duration::from_secs(30);
        }
        if options.max_request_bytes == 0 {
            options.max_request_bytes = DEFAULT_MAX_BYTES;
        }
        if options.max_response_bytes == 0 {
            options.max_response_bytes = DEFAULT_MAX_BYTES;
        }
        let https = HttpsOptions {
            root_certificates_der: options.root_certificates_der.clone().unwrap_or_default(),
            system_roots: options.root_certificates_der.is_none(),
            insecure_skip_verify: false,
        };
        let url = urlrules::parse(&parsed.string()).map_err(|_| Error::Endpoint)?;
        Ok(Client {
            url,
            https,
            options,
        })
    }
    pub fn submit(&self, ctx: &Ctx, document: &[u8]) -> Result<Response, DeliveryError> {
        let fail = |state, status_code, cause| DeliveryError {
            state,
            status_code,
            cause,
        };
        let ctx = ctx.with_timeout(self.options.timeout);
        if let Some(kind) = ctx.err() {
            return Err(fail(DeliveryState::NotSent, 0, Error::Context(kind)));
        }
        if document.len() as i64 > self.options.max_request_bytes {
            return Err(fail(DeliveryState::NotSent, 0, Error::RequestTooLarge));
        }
        if let Err(err) =
            inspect_payload("text/xml", without_bom(document), PayloadLimits::default())
        {
            let cause = if err.is(encapdoc::Kind::TooLarge) {
                Error::RequestTooLarge
            } else {
                Error::Request
            };
            return Err(fail(DeliveryState::NotSent, 0, context_or(&ctx, cause)));
        }
        let got_conn = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&got_conn);
        let ctx = ctx.with_value(HttpClientTrace {
            wrote_request: Some(Arc::new(move |_| flag.store(true, Ordering::SeqCst))),
            got_first_response_byte: None,
        });
        let mut body = SliceReader::new(document);
        let request = HttpRequest {
            method: "POST",
            url: &self.url,
            headers: vec![
                (
                    "Content-Type".into(),
                    "application/xml; charset=utf-8".into(),
                ),
                ("Accept".into(), "application/xml, text/xml".into()),
                ("Accept-Encoding".into(), "identity".into()),
            ],
            body: Some(&mut body as &mut dyn ByteRead),
            https: (self.url.scheme == "https").then_some(&self.https),
            identity: None,
        };
        let mut resp = match send(request, RedirectCheck::NoFollow, &ctx) {
            Ok(resp) => resp,
            Err(_) => {
                let state = if got_conn.load(Ordering::SeqCst) {
                    DeliveryState::Uncertain
                } else {
                    DeliveryState::NotSent
                };
                return Err(fail(state, 0, context_or(&ctx, Error::Transport)));
            }
        };
        let status = resp.status_code;
        if !(200..300).contains(&status) {
            let cause = if (300..400).contains(&status) {
                Error::Redirect
            } else {
                Error::Status
            };
            return Err(fail(DeliveryState::Rejected, status, cause));
        }
        let limit = self.options.max_response_bytes;
        if resp.content_length > limit {
            return Err(fail(
                DeliveryState::Uncertain,
                status,
                Error::ResponseTooLarge,
            ));
        }
        let mut data = Vec::new();
        let mut buf = vec![0u8; 32 << 10];
        let cap = (limit + 1) as usize;
        while data.len() < cap {
            let want = buf.len().min(cap - data.len());
            let (n, err) = resp.body.read(&mut buf[..want]);
            data.extend_from_slice(&buf[..n]);
            match err {
                None => {}
                Some(ReadErr::Eof) => break,
                Some(_) => {
                    return Err(fail(
                        DeliveryState::Uncertain,
                        status,
                        context_or(&ctx, Error::Transport),
                    ));
                }
            }
        }
        let uncertain = |cause| fail(DeliveryState::Uncertain, status, cause);
        if data.len() as i64 > limit {
            return Err(uncertain(Error::ResponseTooLarge));
        }
        if let Some(kind) = ctx.err() {
            return Err(uncertain(Error::Context(kind)));
        }
        if data.is_empty() {
            return Ok(Response {
                status_code: status,
                xml: None,
            });
        }
        let encodings = resp.header.values("Content-Encoding");
        if encodings.len() > 1
            || (encodings.len() == 1 && !trim_space(&encodings[0]).eq_ignore_ascii_case("identity"))
        {
            return Err(uncertain(Error::Response));
        }
        let content_types = resp.header.values("Content-Type");
        if content_types.len() != 1 {
            return Err(uncertain(Error::Response));
        }
        match parse_media_type(&content_types[0]) {
            Ok(m)
                if (m.media_type == "application/xml"
                    || m.media_type == "text/xml"
                    || m.media_type.ends_with("+xml"))
                    && m.params
                        .iter()
                        .find(|(k, _)| k == "charset")
                        .is_none_or(|(_, v)| v.eq_ignore_ascii_case("utf-8")) => {}
            _ => return Err(uncertain(Error::Response)),
        }
        validate_xml_response(&ctx, without_bom(&data)).map_err(uncertain)?;
        Ok(Response {
            status_code: status,
            xml: Some(data),
        })
    }
}
fn context_or(ctx: &Ctx, fallback: Error) -> Error {
    ctx.err().map_or(fallback, Error::Context)
}
fn without_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(data)
}
fn no_charset_reader(label: &str, _: &[u8]) -> Result<Vec<u8>, String> {
    Err(format!(
        "xml: encoding {label:?} declared but no charset reader is set"
    ))
}
fn validate_xml_response(ctx: &Ctx, data: &[u8]) -> Result<(), Error> {
    const MAX_DEPTH: i64 = 64;
    const MAX_TOKENS: usize = 100_000;
    let mut decoder = Decoder::new(data.to_vec(), None, no_charset_reader);
    let mut saw_root = false;
    let mut depth: i64 = 0;
    for count in 0.. {
        if let Some(kind) = ctx.err() {
            return Err(Error::Context(kind));
        }
        let token = match decoder.token() {
            Err(XmlError::Eof) if saw_root && depth == 0 => {
                return ctx.err().map_or(Ok(()), |k| Err(Error::Context(k)));
            }
            Err(_) => return Err(Error::Response),
            Ok(t) => t,
        };
        if count >= MAX_TOKENS {
            return Err(Error::ResponseTooLarge);
        }
        match token {
            Tok::Start(_, attrs) => {
                if depth == 0 {
                    if saw_root {
                        return Err(Error::Response);
                    }
                    saw_root = true;
                }
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(Error::ResponseTooLarge);
                }
                let mut seen: Vec<&Name> = Vec::with_capacity(attrs.len());
                for a in &attrs {
                    if seen.contains(&&a.name) {
                        return Err(Error::Response);
                    }
                    seen.push(&a.name);
                }
            }
            Tok::End(_) => depth -= 1,
            Tok::CharData(text) => {
                if depth == 0
                    && text
                        .iter()
                        .any(|c| !matches!(c, b' ' | b'\t' | b'\r' | b'\n'))
                {
                    return Err(Error::Response);
                }
            }
            Tok::Directive(_) => return Err(Error::Response),
            Tok::ProcInst(target, _) if target != "xml" || count != 0 => {
                return Err(Error::Response);
            }
            Tok::ProcInst(..) | Tok::Comment(_) => {}
        }
    }
    Ok(())
}
