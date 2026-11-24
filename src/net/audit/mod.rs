mod aggregate;
mod atna;
mod clinical;
mod durable;
mod durable_disk;
mod jsonwire;
mod privatefs;
mod profile;
mod receiver;
mod sink;
mod suppression;
mod syslog;
mod timetext;
pub use crate::net::ul::{CancelToken, Ctx, Nanos};
pub use aggregate::*;
pub use atna::*;
pub use clinical::*;
pub use durable::*;
pub use receiver::*;
pub use sink::*;
pub use suppression::*;
pub use syslog::*;
pub type Time = crate::dcmtime::ZonedTime;
pub fn format_rfc3339_nano(t: &Time) -> String {
    timetext::format_rfc3339_nano(t)
}
pub fn parse_rfc3339_nano(value: &str) -> Option<Time> {
    timetext::parse_rfc3339(value)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    AuditMessage,
    SyslogConfig,
    ClinicalAudit,
    DurableSpool,
    SpoolFull,
    SpoolCorrupt,
    SpoolClosed,
    SyslogRecord,
    Canceled,
    DeadlineExceeded,
    Transport,
}
pub const ERR_AUDIT_MESSAGE: &str = "dicom: invalid audit message";
pub const ERR_SYSLOG_CONFIG: &str = "dicom: invalid syslog audit configuration";
pub const ERR_CLINICAL_AUDIT: &str = "dicom: clinical audit refused";
pub const ERR_DURABLE_SPOOL: &str = "dicom: durable audit spool";
pub const ERR_SPOOL_FULL: &str = "dicom: durable audit spool full";
pub const ERR_SPOOL_CORRUPT: &str = "dicom: durable audit spool corrupt";
pub const ERR_SPOOL_CLOSED: &str = "dicom: durable audit spool closed";
pub const ERR_SYSLOG_RECORD: &str = "dicom: invalid syslog record";
#[derive(Debug, Clone, PartialEq, Eq)]
enum Shape {
    Plain,
    Clinical { code: String, field: String },
    Spool { code: String },
    Joined(Vec<Error>),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    text: String,
    kinds: Vec<ErrorKind>,
    shape: Shape,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub(crate) fn sentinel(kind: ErrorKind) -> Error {
        let text = match kind {
            ErrorKind::AuditMessage => ERR_AUDIT_MESSAGE,
            ErrorKind::SyslogConfig => ERR_SYSLOG_CONFIG,
            ErrorKind::ClinicalAudit => ERR_CLINICAL_AUDIT,
            ErrorKind::DurableSpool => ERR_DURABLE_SPOOL,
            ErrorKind::SpoolFull => ERR_SPOOL_FULL,
            ErrorKind::SpoolCorrupt => ERR_SPOOL_CORRUPT,
            ErrorKind::SpoolClosed => ERR_SPOOL_CLOSED,
            ErrorKind::SyslogRecord => ERR_SYSLOG_RECORD,
            ErrorKind::Canceled => "context canceled",
            ErrorKind::DeadlineExceeded => "context deadline exceeded",
            ErrorKind::Transport => "transport",
        };
        Error {
            text: text.to_string(),
            kinds: vec![kind],
            shape: Shape::Plain,
        }
    }
    pub(crate) fn wrap(kind: ErrorKind, detail: impl std::fmt::Display) -> Error {
        let mut e = Error::sentinel(kind);
        e.text = format!("{}: {detail}", e.text);
        e
    }
    pub(crate) fn clinical(code: &str, field: impl Into<String>) -> Error {
        let field = field.into();
        Error {
            text: format!("dicom: clinical audit {code} at {field}"),
            kinds: vec![ErrorKind::ClinicalAudit],
            shape: Shape::Clinical {
                code: code.to_string(),
                field,
            },
        }
    }
    pub(crate) fn spool(code: &str, kind: ErrorKind) -> Error {
        Error {
            text: format!("dicom: durable audit {code}"),
            kinds: vec![kind],
            shape: Shape::Spool {
                code: code.to_string(),
            },
        }
    }
    pub(crate) fn context(kind: crate::net::ul::Kind) -> Error {
        if kind == crate::net::ul::Kind::DeadlineExceeded {
            Error::sentinel(ErrorKind::DeadlineExceeded)
        } else {
            Error::sentinel(ErrorKind::Canceled)
        }
    }
    pub(crate) fn transport(text: impl std::fmt::Display) -> Error {
        Error {
            text: text.to_string(),
            kinds: vec![ErrorKind::Transport],
            shape: Shape::Plain,
        }
    }
    pub(crate) fn join(errors: Vec<Error>) -> Option<Error> {
        if errors.is_empty() {
            return None;
        }
        let text = errors
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let mut kinds = Vec::new();
        for e in &errors {
            for k in &e.kinds {
                if !kinds.contains(k) {
                    kinds.push(*k);
                }
            }
        }
        Some(Error {
            text,
            kinds,
            shape: Shape::Joined(errors),
        })
    }
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kinds.contains(&kind)
    }
    pub fn clinical_error(&self) -> Option<(&str, &str)> {
        match &self.shape {
            Shape::Clinical { code, field } => Some((code, field)),
            _ => None,
        }
    }
    pub fn spool_code(&self) -> Option<&str> {
        match &self.shape {
            Shape::Spool { code } => Some(code),
            Shape::Joined(errors) => errors.iter().find_map(Error::spool_code),
            _ => None,
        }
    }
    pub fn joined(&self) -> &[Error] {
        match &self.shape {
            Shape::Joined(errors) => errors,
            _ => &[],
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub type CallbackError = Box<dyn std::error::Error + Send + Sync>;
pub(crate) fn ctx_err(ctx: &Ctx) -> Option<Error> {
    ctx.err().map(Error::context)
}
pub(crate) fn split_host_port(hostport: &str) -> Option<(&str, &str)> {
    let i = hostport.rfind(':')?;
    let (mut j, mut k) = (0, 0);
    let host;
    if hostport.as_bytes()[0] == b'[' {
        let end = hostport.find(']')?;
        if end + 1 == hostport.len() || end + 1 != i {
            return None;
        }
        host = &hostport[1..end];
        j = 1;
        k = end + 1;
    } else {
        host = &hostport[..i];
        if host.contains(':') {
            return None;
        }
    }
    if hostport[j..].contains('[') || hostport[k..].contains(']') {
        return None;
    }
    Some((host, &hostport[i + 1..]))
}
pub(crate) fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(char::is_whitespace)
}
pub(crate) fn os_hostname() -> String {
    privatefs::hostname()
}
