use super::mimeheader::trim_space;
use super::urlrules;
use crate::textfmt::quote;
pub const DEFAULT_TIMEOUT_NANOS: i64 = 20_000_000_000;
pub const DEFAULT_MAX_BODY_BYTES: i64 = 512 << 20;
pub const DEFAULT_MAX_RESPONSE_PARTS: i64 = 100_000;
pub const DEFAULT_MAX_PART_BYTES: i64 = DEFAULT_MAX_BODY_BYTES;
pub const DEFAULT_MAX_PART_HEADER_BYTES: i64 = 64 << 10;
pub const DEFAULT_MAX_MULTIPART_DEPTH: i64 = 1;
pub const METADATA_MEDIA_TYPE_DICOM_JSON: &str = "application/dicom+json";
pub const METADATA_MEDIA_TYPE_DICOM_XML: &str = "application/dicom+xml";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Endpoint {
    pub base_url: String,
    pub qido_path: String,
    pub wado_path: String,
    pub stow_path: String,
    pub wado_uri_path: String,
    pub ups_path: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResponseLimits {
    pub max_parts: i64,
    pub max_part_bytes: i64,
    pub max_part_header_bytes: i64,
    pub max_multipart_depth: i64,
    pub allowed_media_types: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstanceRef {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidEndpoint,
    RequestFailure,
    Timeout,
    HttpStatus,
    AuthStatus,
    AuthToken,
    DecodeResponse,
}
impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::InvalidEndpoint => "invalid_endpoint",
            ErrorKind::RequestFailure => "request_failure",
            ErrorKind::Timeout => "timeout",
            ErrorKind::HttpStatus => "http_status",
            ErrorKind::AuthStatus => "auth_status",
            ErrorKind::AuthToken => "auth_token",
            ErrorKind::DecodeResponse => "decode_response",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseDecodeErrorKind {
    MalformedMediaType,
    MissingBoundary,
    UnexpectedMedia,
    PartLimit,
    PartBytes,
    HeaderBytes,
    MultipartDepth,
    MalformedMultipart,
    TruncatedBody,
    TransferEncoding,
}
impl ResponseDecodeErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MalformedMediaType => "malformed_media_type",
            Self::MissingBoundary => "missing_boundary",
            Self::UnexpectedMedia => "unexpected_media_type",
            Self::PartLimit => "part_limit",
            Self::PartBytes => "part_bytes",
            Self::HeaderBytes => "header_bytes",
            Self::MultipartDepth => "multipart_depth",
            Self::MalformedMultipart => "malformed_multipart",
            Self::TruncatedBody => "truncated_body",
            Self::TransferEncoding => "transfer_encoding",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResponseDecodeError {
    pub kind: ResponseDecodeErrorKind,
    pub limit: i64,
    pub value: i64,
}
impl ResponseDecodeError {
    pub fn new(kind: ResponseDecodeErrorKind) -> Self {
        ResponseDecodeError {
            kind,
            limit: 0,
            value: 0,
        }
    }
    pub fn with(kind: ResponseDecodeErrorKind, limit: i64, value: i64) -> Self {
        ResponseDecodeError { kind, limit, value }
    }
}
impl std::fmt::Display for ResponseDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ResponseDecodeErrorKind as K;
        match self.kind {
            K::MalformedMediaType => f.write_str("DICOMweb response has a malformed Content-Type"),
            K::MissingBoundary => f.write_str("DICOMweb multipart response has no boundary"),
            K::UnexpectedMedia => f.write_str("DICOMweb response has an incompatible media type"),
            K::PartLimit => write!(
                f,
                "DICOMweb multipart response exceeds {} parts",
                self.limit
            ),
            K::PartBytes => write!(f, "DICOMweb response part exceeds {} bytes", self.limit),
            K::HeaderBytes => write!(
                f,
                "DICOMweb response part headers exceed {} bytes",
                self.limit
            ),
            K::MultipartDepth => {
                write!(
                    f,
                    "DICOMweb multipart response exceeds depth {}",
                    self.limit
                )
            }
            K::MalformedMultipart => {
                f.write_str("DICOMweb multipart response is malformed or truncated")
            }
            K::TruncatedBody => f.write_str("DICOMweb response body is truncated"),
            K::TransferEncoding => {
                f.write_str("DICOMweb response part uses an unsupported Content-Transfer-Encoding")
            }
        }
    }
}
impl std::error::Error for ResponseDecodeError {}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PayloadRef {
    pub index: usize,
    pub path: String,
    pub sop_instance_uid: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum CauseClass {
    #[default]
    Other,
    Decode(ResponseDecodeError),
    SignInRequired,
    TokenUnavailable,
    InvalidMetadataIdentity,
    BodyLimit,
    InsecureBasicAuth {
        redirect: bool,
    },
    RedirectOrigin,
    Timeout,
    Canceled,
    ConnectionBroken,
    StorePayload(Box<PayloadRef>),
    BulkDataRange(Box<CauseClass>),
    Transport(Box<TransportCause>),
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct TransportCause {
    pub os_error: Option<i32>,
    pub dial: bool,
    pub name_lookup: bool,
    pub tls: Option<crate::net::ul::TlsFailureClass>,
    pub tls_certificate_not_yet_valid: bool,
    pub scheme_mismatch: bool,
    pub eof: bool,
    pub unexpected_eof: bool,
}
impl CauseClass {
    #[doc(hidden)]
    pub fn transport_cause(&self) -> Option<&TransportCause> {
        match self {
            CauseClass::Transport(cause) => Some(cause),
            CauseClass::BulkDataRange(inner) => inner.transport_cause(),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Cause {
    pub text: String,
    pub class: CauseClass,
}
impl std::fmt::Display for Cause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Cause {}
impl Cause {
    pub fn text(text: impl Into<String>) -> Cause {
        Cause {
            text: text.into(),
            class: CauseClass::Other,
        }
    }
    pub fn decode(err: ResponseDecodeError) -> Cause {
        Cause {
            text: err.to_string(),
            class: CauseClass::Decode(err),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub url: String,
    pub status_code: i64,
    pub status: String,
    pub err: Option<Cause>,
}
impl Error {
    #[doc(hidden)]
    pub fn invalid_endpoint(text: &str) -> Error {
        Error {
            kind: ErrorKind::InvalidEndpoint,
            url: String::new(),
            status_code: 0,
            status: String::new(),
            err: Some(Cause::text(text)),
        }
    }
    pub fn new(kind: ErrorKind, raw_url: &str, status_code: i64, err: Option<Cause>) -> Error {
        Error {
            kind,
            url: safe_diagnostic_url(raw_url),
            status_code,
            status: safe_http_status(status_code),
            err,
        }
    }
    #[doc(hidden)]
    pub fn transport_cause(&self) -> Option<&TransportCause> {
        self.err.as_ref()?.class.transport_cause()
    }
    pub fn os_error(&self) -> Option<i32> {
        self.transport_cause()?.os_error
    }
    pub fn is_name_lookup(&self) -> bool {
        self.transport_cause().is_some_and(|c| c.name_lookup)
    }
    pub fn is_dial(&self) -> bool {
        self.transport_cause().is_some_and(|c| c.dial)
    }
    #[doc(hidden)]
    pub fn tls_failure(&self) -> Option<crate::net::ul::TlsFailureClass> {
        self.transport_cause()?.tls
    }
    pub fn tls_certificate_not_yet_valid(&self) -> bool {
        self.transport_cause()
            .is_some_and(|c| c.tls_certificate_not_yet_valid)
    }
    pub fn is_scheme_mismatch(&self) -> bool {
        self.transport_cause().is_some_and(|c| c.scheme_mismatch)
    }
    pub fn is_connection_eof(&self) -> bool {
        self.transport_cause()
            .is_some_and(|c| c.eof || c.unexpected_eof)
    }
    #[doc(hidden)]
    pub fn debug_text(&self) -> String {
        format!(
            "DICOMweb error (kind={}, url={}, status_code={}, status={})",
            quote(self.kind.as_str()),
            quote(&safe_url(&self.url)),
            self.status_code,
            quote(&safe_http_status(self.status_code))
        )
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let target = safe_url(&self.url);
        let status = safe_http_status(self.status_code);
        match self.kind {
            ErrorKind::HttpStatus | ErrorKind::AuthStatus => {
                if status.is_empty() {
                    write!(f, "DICOMweb {target} returned an HTTP error")
                } else {
                    write!(f, "DICOMweb {target} returned {status}")
                }
            }
            ErrorKind::AuthToken => {
                if matches!(
                    self.err.as_ref().map(|c| &c.class),
                    Some(CauseClass::SignInRequired)
                ) {
                    write!(
                        f,
                        "DICOMweb authentication for {target} requires a new sign-in"
                    )
                } else {
                    write!(
                        f,
                        "DICOMweb authentication for {target} could not obtain a bearer token"
                    )
                }
            }
            ErrorKind::Timeout => write!(f, "DICOMweb {target} timed out"),
            ErrorKind::InvalidEndpoint => f.write_str("invalid DICOMweb endpoint"),
            ErrorKind::DecodeResponse => write!(f, "decode DICOMweb response from {target}"),
            ErrorKind::RequestFailure => {
                if status.is_empty() {
                    write!(f, "DICOMweb request to {target} failed")
                } else {
                    write!(f, "DICOMweb request to {target} failed after {status}")
                }
            }
        }
    }
}
impl std::error::Error for Error {}
const SAFE_ENDPOINT_LABEL: &str = "endpoint";
pub fn safe_url(value: &str) -> String {
    match urlrules::parse(trim_space(value)) {
        Ok(parsed) => safe_parsed_url(&parsed),
        Err(_) => SAFE_ENDPOINT_LABEL.to_string(),
    }
}
pub(crate) fn safe_parsed_url(value: &urlrules::Url) -> String {
    if value.host.is_empty()
        || !(value.scheme.eq_ignore_ascii_case("http")
            || value.scheme.eq_ignore_ascii_case("https"))
    {
        return SAFE_ENDPOINT_LABEL.to_string();
    }
    let mut clean = value.clone();
    clean.scheme = clean.scheme.to_ascii_lowercase();
    clean.user = None;
    clean.raw_query.clear();
    clean.force_query = false;
    clean.fragment.clear();
    clean.raw_fragment.clear();
    clean.opaque.clear();
    clean.string()
}
pub(crate) fn safe_diagnostic_url(value: &str) -> String {
    match urlrules::parse(trim_space(value)) {
        Ok(mut parsed) => {
            parsed.path.clear();
            parsed.raw_path.clear();
            safe_parsed_url(&parsed)
        }
        Err(_) => SAFE_ENDPOINT_LABEL.to_string(),
    }
}
pub fn status_text(code: i64) -> &'static str {
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Request Entity Too Large",
        414 => "Request URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Requested Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "",
    }
}
pub fn safe_http_status(code: i64) -> String {
    if code <= 0 {
        return String::new();
    }
    let text = status_text(code);
    if text.is_empty() {
        format!("HTTP {code}")
    } else {
        format!("{code} {text}")
    }
}
pub fn valid_ae_title(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 16 || trim_space(value).is_empty() {
        return false;
    }
    bytes
        .iter()
        .all(|&c| (0x20..=0x7E).contains(&c) && c != b'\\')
}
