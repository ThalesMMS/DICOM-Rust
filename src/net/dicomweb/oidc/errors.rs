use super::Fail;
use std::fmt;
pub const ERROR_CONFIGURATION: &str = "configuration";
pub const ERROR_DISCOVERY: &str = "discovery";
pub const ERROR_LOGIN: &str = "login";
pub const ERROR_REFRESH: &str = "refresh";
pub const ERROR_REVOCATION: &str = "revocation";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sentinel {
    NotConfigured,
    Discovery,
    Login,
    Refresh,
    Revocation,
    NoRefreshToken,
    InvalidToken,
    MalformedToken,
    UnsupportedAlgorithm,
    KeyNotFound,
    InvalidSignature,
    TokenExpired,
    TokenNotYetValid,
    IssuerMismatch,
    AudienceMismatch,
    NonceMismatch,
    SubjectMismatch,
    AccessTokenHashMismatch,
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        match self {
            Sentinel::NotConfigured => "oidc: not configured",
            Sentinel::Discovery => "oidc: provider discovery failed",
            Sentinel::Login => "oidc: login failed",
            Sentinel::Refresh => "oidc: token refresh failed",
            Sentinel::Revocation => "oidc: token revocation failed",
            Sentinel::NoRefreshToken => "oidc: no refresh token",
            Sentinel::InvalidToken => "oidc: invalid token",
            Sentinel::MalformedToken => "oidc: invalid token: malformed",
            Sentinel::UnsupportedAlgorithm => "oidc: invalid token: unsupported algorithm",
            Sentinel::KeyNotFound => "oidc: invalid token: signing key not found",
            Sentinel::InvalidSignature => "oidc: invalid token: signature mismatch",
            Sentinel::TokenExpired => "oidc: invalid token: expired",
            Sentinel::TokenNotYetValid => "oidc: invalid token: not yet valid",
            Sentinel::IssuerMismatch => "oidc: invalid token: issuer mismatch",
            Sentinel::AudienceMismatch => "oidc: invalid token: audience mismatch",
            Sentinel::NonceMismatch => "oidc: invalid token: nonce mismatch",
            Sentinel::SubjectMismatch => "oidc: invalid token: subject mismatch",
            Sentinel::AccessTokenHashMismatch => "oidc: invalid token: access token hash mismatch",
        }
    }
    fn wraps_invalid(self) -> bool {
        !matches!(
            self,
            Sentinel::NotConfigured
                | Sentinel::Discovery
                | Sentinel::Login
                | Sentinel::Refresh
                | Sentinel::Revocation
                | Sentinel::NoRefreshToken
                | Sentinel::InvalidToken
        )
    }
    pub fn is(self, target: Sentinel) -> bool {
        self == target || (target == Sentinel::InvalidToken && self.wraps_invalid())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: String,
    pub operation: String,
    pub status_code: i64,
    pub provider_code: String,
    pub err: Option<Sentinel>,
    pub cause: Option<Sentinel>,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let operation = super::trim_space(&self.operation);
        let operation = if operation.is_empty() {
            self.kind.as_str()
        } else {
            operation
        };
        write!(f, "oidc: {operation} failed")?;
        if self.status_code != 0 {
            write!(f, " (HTTP {})", self.status_code)?;
        }
        if !self.provider_code.is_empty() {
            write!(f, " (provider error {})", self.provider_code)?;
        }
        if let Some(cause) = self.cause {
            write!(f, ": {}", cause.text())?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {}
impl Error {
    pub fn is(&self, target: Sentinel) -> bool {
        self.err.is_some_and(|e| e.is(target)) || self.cause.is_some_and(|c| c.is(target))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OidcError {
    Sentinel(Sentinel),
    Protocol(Box<Error>),
    Canceled,
    DeadlineExceeded,
    Other(String),
}
impl OidcError {
    pub fn is(&self, target: Sentinel) -> bool {
        match self {
            OidcError::Sentinel(s) => s.is(target),
            OidcError::Protocol(e) => e.is(target),
            _ => false,
        }
    }
    pub fn protocol(&self) -> Option<&Error> {
        match self {
            OidcError::Protocol(e) => Some(e),
            _ => None,
        }
    }
}
impl fmt::Display for OidcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OidcError::Sentinel(s) => f.write_str(s.text()),
            OidcError::Protocol(e) => e.fmt(f),
            OidcError::Canceled => f.write_str("context canceled"),
            OidcError::DeadlineExceeded => f.write_str("context deadline exceeded"),
            OidcError::Other(text) => f.write_str(text),
        }
    }
}
impl std::error::Error for OidcError {}
pub(crate) fn sentinel_for(kind: &str) -> Sentinel {
    match kind {
        ERROR_CONFIGURATION => Sentinel::NotConfigured,
        ERROR_DISCOVERY => Sentinel::Discovery,
        ERROR_REFRESH => Sentinel::Refresh,
        ERROR_REVOCATION => Sentinel::Revocation,
        _ => Sentinel::Login,
    }
}
fn safe_cause(cause: Option<&OidcError>) -> Option<Sentinel> {
    let cause = cause?;
    [
        Sentinel::MalformedToken,
        Sentinel::UnsupportedAlgorithm,
        Sentinel::KeyNotFound,
        Sentinel::InvalidSignature,
        Sentinel::TokenExpired,
        Sentinel::TokenNotYetValid,
        Sentinel::IssuerMismatch,
        Sentinel::AudienceMismatch,
        Sentinel::NonceMismatch,
        Sentinel::SubjectMismatch,
        Sentinel::AccessTokenHashMismatch,
        Sentinel::InvalidToken,
        Sentinel::NoRefreshToken,
    ]
    .into_iter()
    .find(|s| cause.is(*s))
}
pub(crate) fn new_error(
    kind: &str,
    operation: &str,
    status: i64,
    provider_code: &str,
    cause: Option<&OidcError>,
) -> OidcError {
    OidcError::Protocol(Box::new(Error {
        kind: kind.to_string(),
        operation: operation.to_string(),
        status_code: status,
        provider_code: safe_provider_code(provider_code),
        err: Some(sentinel_for(kind)),
        cause: safe_cause(cause),
    }))
}
pub(crate) fn new_error_sentinel(kind: &str, operation: &str, cause: Sentinel) -> OidcError {
    new_error(kind, operation, 0, "", Some(&OidcError::Sentinel(cause)))
}
pub(crate) fn context_error(kind: &str, operation: &str, fail: Fail) -> OidcError {
    match fail {
        Fail::Canceled => OidcError::Canceled,
        Fail::Deadline => OidcError::DeadlineExceeded,
        Fail::Other => new_error(kind, operation, 0, "", None),
    }
}
pub fn safe_provider_code(value: &str) -> String {
    let value = super::trim_space(value);
    if value.is_empty() || value.len() > 64 {
        return String::new();
    }
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        value.to_string()
    } else {
        String::new()
    }
}
