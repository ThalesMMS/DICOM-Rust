#![forbid(unsafe_code)]
use super::outcome::PresentationContextOutcome;
use super::pdu::AssociationRj;
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    InvalidPdu,
    PduTooLarge,
    InvalidPduSize,
    LengthOverflow,
    UnsupportedPdu,
    InvalidPduField,
    MissingPduField,
    InvalidPduItem,
    InvalidUserItem,
    InvalidAeTitle,
    InvalidPcid,
    AssociationRejected,
    AssociationAborted,
    UnexpectedPdu,
    UnrecognizedPdu,
    AssociationTimeout,
    NoAcceptedPresentationContexts,
    InvalidUserIdentity,
    UnsupportedUserIdentityType,
    UserIdentityRejected,
    ExtendedNegotiationRejected,
    InvalidPresentationContextRule,
    CalledAeRegistry,
    CalledAeRegistryLimit,
    AdmissionHandler,
    AssociationExclusivelyOwned,
    MessageWriteNotStarted,
    AssociationServerStopped,
    UnknownTlsProfile,
    WeakTlsCertificate,
    TlsCertificateRevoked,
    TlsRevocationUnknown,
    TlsCrlInvalid,
    TransportConfiguration,
    HttpConnect,
    HttpConnectHeaderLimit,
    SocketBuffersUnsupported,
    UnsupportedTransferSyntax,
    Eof,
    UnexpectedEof,
    NoProgress,
    ShortWrite,
    Closed,
    Canceled,
    DeadlineExceeded,
    NetTimeout,
    PeerReset,
    Syscall,
    AddressInUse,
    AddressNotAvailable,
    NoBufferSpace,
    Other,
    NameLookup,
    TlsCertificateNotYetValid,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::InvalidPdu => "dicom ul: invalid PDU",
            Kind::PduTooLarge => "dicom ul: PDU too large",
            Kind::InvalidPduSize => "dicom ul: invalid maximum PDU size",
            Kind::LengthOverflow => "dicom ul: length overflow",
            Kind::UnsupportedPdu => "dicom ul: unsupported PDU",
            Kind::InvalidPduField => "dicom ul: invalid PDU field",
            Kind::MissingPduField => "dicom ul: missing PDU field",
            Kind::InvalidPduItem => "dicom ul: invalid PDU item",
            Kind::InvalidUserItem => "dicom ul: invalid user information item",
            Kind::InvalidAeTitle => "dicom ul: invalid AE title",
            Kind::InvalidPcid => "dicom ul: invalid presentation context ID",
            Kind::AssociationRejected => "dicom ul: association rejected",
            Kind::AssociationAborted => "dicom ul: association aborted",
            Kind::UnexpectedPdu => "dicom ul: unexpected PDU",
            Kind::UnrecognizedPdu => "dicom ul: unrecognized PDU type",
            Kind::AssociationTimeout => "dicom ul: association timeout",
            Kind::NoAcceptedPresentationContexts => "dicom ul: no accepted presentation contexts",
            Kind::InvalidUserIdentity => "dicom ul: invalid user identity",
            Kind::UnsupportedUserIdentityType => "dicom ul: unsupported user identity type",
            Kind::UserIdentityRejected => "dicom ul: user identity rejected",
            Kind::ExtendedNegotiationRejected => "dicom ul: extended negotiation rejected",
            Kind::InvalidPresentationContextRule => "dicom ul: invalid presentation context rule",
            Kind::CalledAeRegistry => "dicom ul: invalid Called AE registry",
            Kind::CalledAeRegistryLimit => "dicom ul: Called AE registry resource limit",
            Kind::AdmissionHandler => "dicom ul: admission handler failed",
            Kind::AssociationExclusivelyOwned => {
                "dicom ul: association has an exclusive operation owner"
            }
            Kind::MessageWriteNotStarted => "dicom ul: message write did not start",
            Kind::AssociationServerStopped => "dicom ul: association server is stopped",
            Kind::UnknownTlsProfile => "dicom ul: unknown TLS profile",
            Kind::WeakTlsCertificate => "dicom ul: TLS certificate below profile requirements",
            Kind::TlsCertificateRevoked => "certificate revoked",
            Kind::TlsRevocationUnknown => "certificate revocation unknown",
            Kind::TlsCrlInvalid => "invalid CRL material",
            Kind::TransportConfiguration => "dicom ul: invalid transport configuration",
            Kind::HttpConnect => "dicom ul: HTTP CONNECT failed",
            Kind::HttpConnectHeaderLimit => "dicom ul: HTTP CONNECT response headers exceed limit",
            Kind::SocketBuffersUnsupported => {
                "dicom ul: socket buffer sizes are not supported on this connection"
            }
            Kind::UnsupportedTransferSyntax => "dicom: unsupported transfer syntax",
            Kind::Eof => "EOF",
            Kind::UnexpectedEof => "unexpected EOF",
            Kind::NoProgress => "multiple Read calls return no data or error",
            Kind::ShortWrite => "short write",
            Kind::Closed => "use of closed network connection",
            Kind::Canceled => "context canceled",
            Kind::DeadlineExceeded => "context deadline exceeded",
            Kind::NetTimeout => "i/o timeout",
            Kind::PeerReset => "connection reset by peer",
            Kind::Syscall => "system call error",
            Kind::AddressInUse => "address already in use",
            Kind::AddressNotAvailable => "cannot assign requested address",
            Kind::NoBufferSpace => "no buffer space available",
            Kind::Other => "error",
            Kind::NameLookup => "no such host",
            Kind::TlsCertificateNotYetValid => "x509: certificate is not yet valid",
        }
    }
}
#[derive(Debug, Clone, Default)]
pub enum Detail {
    #[default]
    None,
    Rejection(AssociationRj),
    Abort(u8, u8),
    NoAccepted(Vec<PresentationContextOutcome>),
    Revocation(TlsRevocationError),
    Tls(TlsFailureClass),
    ClientCertificateRefused(bool),
    HttpConnect(i64),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TlsRevocationKind {
    Revoked,
    Missing,
    Expired,
    NotYetValid,
    IssuerMismatch,
    BadSignature,
    Unsupported,
    InvalidMaterial,
    Limit,
}
impl TlsRevocationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TlsRevocationKind::Revoked => "revoked",
            TlsRevocationKind::Missing => "missing",
            TlsRevocationKind::Expired => "expired",
            TlsRevocationKind::NotYetValid => "not-yet-valid",
            TlsRevocationKind::IssuerMismatch => "issuer-mismatch",
            TlsRevocationKind::BadSignature => "bad-signature",
            TlsRevocationKind::Unsupported => "unsupported",
            TlsRevocationKind::InvalidMaterial => "invalid-material",
            TlsRevocationKind::Limit => "limit",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TlsRevocationError {
    pub kind: TlsRevocationKind,
    pub chain_index: i32,
    pub certificate_index: i32,
    pub list_index: i32,
}
impl std::fmt::Display for TlsRevocationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "certificate revocation check failed: {} (chain {}, certificate {}, list {})",
            self.kind.as_str(),
            self.chain_index,
            self.certificate_index,
            self.list_index
        )
    }
}
impl std::error::Error for TlsRevocationError {}
impl TlsRevocationError {
    pub fn new(kind: TlsRevocationKind, list_index: i32) -> TlsRevocationError {
        TlsRevocationError {
            kind,
            chain_index: -1,
            certificate_index: -1,
            list_index,
        }
    }
    pub fn sentinel(&self) -> Kind {
        match self.kind {
            TlsRevocationKind::Revoked => Kind::TlsCertificateRevoked,
            TlsRevocationKind::Unsupported
            | TlsRevocationKind::InvalidMaterial
            | TlsRevocationKind::Limit => Kind::TlsCrlInvalid,
            _ => Kind::TlsRevocationUnknown,
        }
    }
}
impl From<TlsRevocationError> for Error {
    fn from(e: TlsRevocationError) -> Error {
        Error {
            text: format!("certificate revocation: {}", e.kind.as_str()),
            kinds: vec![e.sentinel()],
            detail: Detail::Revocation(e),
            os_error: None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TlsFailureClass {
    RemoteAlert(u8),
    Version,
    HandshakeFailed,
    NoClientCertificate,
    UnknownCa,
    CertificateExpired,
    HostnameMismatch,
    CertificateUsage,
    BadCertificate,
    NotTls,
}
impl TlsFailureClass {
    pub fn is_certificate_or_record_failure(self) -> bool {
        matches!(
            self,
            TlsFailureClass::UnknownCa
                | TlsFailureClass::CertificateExpired
                | TlsFailureClass::HostnameMismatch
                | TlsFailureClass::CertificateUsage
                | TlsFailureClass::BadCertificate
                | TlsFailureClass::NotTls
        )
    }
}
#[derive(Debug, Clone)]
pub struct Error {
    text: String,
    kinds: Vec<Kind>,
    detail: Detail,
    os_error: Option<i32>,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn sentinel(kind: Kind) -> Error {
        Error {
            text: kind.text().to_string(),
            kinds: vec![kind],
            detail: Detail::None,
            os_error: None,
        }
    }
    pub fn wrap(kind: Kind, message: impl fmt::Display) -> Error {
        Error {
            text: format!("{}: {message}", kind.text()),
            kinds: vec![kind],
            detail: Detail::None,
            os_error: None,
        }
    }
    pub fn other(text: impl Into<String>) -> Error {
        Error {
            text: text.into(),
            kinds: vec![Kind::Other],
            detail: Detail::None,
            os_error: None,
        }
    }
    pub fn with_kinds(text: impl Into<String>, kinds: Vec<Kind>) -> Error {
        Error {
            text: text.into(),
            kinds,
            detail: Detail::None,
            os_error: None,
        }
    }
    pub fn prefixed(self, prefix: impl fmt::Display) -> Error {
        Error {
            text: format!("{prefix}: {}", self.text),
            ..self
        }
    }
    #[doc(hidden)]
    pub fn wrap_text(kind: Kind, err: &Error) -> Error {
        Error::wrap(kind, &err.text)
    }
    pub fn wrap_err(kind: Kind, err: Error) -> Error {
        let mut kinds = vec![kind];
        kinds.extend(err.kinds.iter().copied());
        Error {
            text: format!("{}: {}", kind.text(), err.text),
            kinds,
            detail: err.detail,
            os_error: err.os_error,
        }
    }
    pub fn also(mut self, kind: Kind) -> Error {
        if !self.kinds.contains(&kind) {
            self.kinds.push(kind);
        }
        self
    }
    pub fn join(errors: impl IntoIterator<Item = Option<Error>>) -> Option<Error> {
        let mut out: Option<Error> = None;
        for err in errors.into_iter().flatten() {
            out = Some(match out {
                None => err,
                Some(mut joined) => {
                    joined.text.push('\n');
                    joined.text.push_str(&err.text);
                    for k in err.kinds {
                        if !joined.kinds.contains(&k) {
                            joined.kinds.push(k);
                        }
                    }
                    if matches!(joined.detail, Detail::None) {
                        joined.detail = err.detail;
                    }
                    if joined.os_error.is_none() {
                        joined.os_error = err.os_error;
                    }
                    joined
                }
            });
        }
        out
    }
    pub fn eof() -> Error {
        Error::sentinel(Kind::Eof)
    }
    pub fn unexpected_eof() -> Error {
        Error::sentinel(Kind::UnexpectedEof)
    }
    pub fn closed() -> Error {
        Error::sentinel(Kind::Closed)
    }
    pub fn local_endpoint_unavailable(&self) -> bool {
        self.is(Kind::AddressInUse)
            || self.is(Kind::AddressNotAvailable)
            || self.is(Kind::NoBufferSpace)
    }
    pub fn rejection(rj: AssociationRj) -> Error {
        Error {
            text: rejection_text(&rj),
            kinds: vec![Kind::AssociationRejected],
            detail: Detail::Rejection(rj),
            os_error: None,
        }
    }
    pub fn abort(source: u8, reason: u8) -> Error {
        Error {
            text: format!(
                "{}: source={source} reason={reason}",
                Kind::AssociationAborted.text()
            ),
            kinds: vec![Kind::AssociationAborted],
            detail: Detail::Abort(source, reason),
            os_error: None,
        }
    }
    #[doc(hidden)]
    pub fn no_accepted(outcomes: Vec<PresentationContextOutcome>) -> Error {
        let text = if outcomes.is_empty() {
            Kind::NoAcceptedPresentationContexts.text().to_string()
        } else {
            let parts: Vec<String> = outcomes.iter().map(|o| o.explain()).collect();
            format!(
                "{}: {}",
                Kind::NoAcceptedPresentationContexts.text(),
                parts.join("; ")
            )
        };
        Error {
            text,
            kinds: vec![Kind::NoAcceptedPresentationContexts],
            detail: Detail::NoAccepted(outcomes),
            os_error: None,
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }
    pub fn kinds(&self) -> &[Kind] {
        &self.kinds
    }
    pub fn detail(&self) -> &Detail {
        &self.detail
    }
    pub fn with_detail(mut self, detail: Detail) -> Error {
        self.detail = detail;
        self
    }
    pub fn as_revocation(&self) -> Option<&TlsRevocationError> {
        match &self.detail {
            Detail::Revocation(e) => Some(e),
            _ => None,
        }
    }
    pub fn os_error(&self) -> Option<i32> {
        self.os_error
    }
    pub fn with_os_error(mut self, code: Option<i32>) -> Error {
        self.os_error = code;
        self
    }
    pub fn is_name_lookup(&self) -> bool {
        self.is(Kind::NameLookup)
    }
    pub fn tls_certificate_not_yet_valid(&self) -> bool {
        self.is(Kind::TlsCertificateNotYetValid)
    }
    #[doc(hidden)]
    pub fn tls_failure(&self) -> Option<TlsFailureClass> {
        match &self.detail {
            Detail::Tls(c) => Some(*c),
            _ => None,
        }
    }
    pub fn as_rejection(&self) -> Option<&AssociationRj> {
        match &self.detail {
            Detail::Rejection(rj) => Some(rj),
            _ => None,
        }
    }
    #[doc(hidden)]
    pub fn rejected_outcomes(&self) -> Option<Vec<PresentationContextOutcome>> {
        match &self.detail {
            Detail::NoAccepted(o) => Some(o.clone()),
            _ => None,
        }
    }
    pub fn text(&self) -> &str {
        &self.text
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub fn rejection_description(rj: &AssociationRj) -> String {
    use super::pdu::*;
    let result = match rj.result {
        ASSOCIATE_RJ_RESULT_PERMANENT => "rejected-permanent".to_string(),
        ASSOCIATE_RJ_RESULT_TRANSIENT => "rejected-transient".to_string(),
        r => format!("result {r}"),
    };
    let mut source = format!("source {}", rj.source);
    let mut reason = format!("reason {}", rj.reason);
    match rj.source {
        ASSOCIATE_RJ_SOURCE_SERVICE_USER => {
            source = "service-user".to_string();
            match rj.reason {
                1 => reason = "no-reason-given".to_string(),
                2 => reason = "application-context-name-not-supported".to_string(),
                3 => reason = "calling-AE-title-not-recognized".to_string(),
                7 => reason = "called-AE-title-not-recognized".to_string(),
                _ => {}
            }
        }
        ASSOCIATE_RJ_SOURCE_SERVICE_PROVIDER_ACSE => {
            source = "service-provider (ACSE)".to_string();
            match rj.reason {
                1 => reason = "no-reason-given".to_string(),
                2 => reason = "protocol-version-not-supported".to_string(),
                _ => {}
            }
        }
        ASSOCIATE_RJ_SOURCE_SERVICE_PROVIDER_PRESENTATION => {
            source = "service-provider (presentation)".to_string();
            match rj.reason {
                1 => reason = "temporary-congestion".to_string(),
                2 => reason = "local-limit-exceeded".to_string(),
                _ => {}
            }
        }
        _ => {}
    }
    format!("{result}, {source}, {reason}")
}
#[doc(hidden)]
pub fn rejection_text(rj: &AssociationRj) -> String {
    format!(
        "{}: {} (result={} source={} reason={})",
        Kind::AssociationRejected.text(),
        rejection_description(rj),
        rj.result,
        rj.source,
        rj.reason
    )
}
