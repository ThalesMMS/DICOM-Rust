#![forbid(unsafe_code)]
use super::profile::ProfileError;
use std::fmt;
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum ErrorKind {
    InvalidFileId,
    InvalidOptions,
    MissingRequiredKey,
    SourceChanged,
    ResourceLimit,
    UnsupportedSopClass,
    InvalidRecord,
    DicomdirWrite,
    Transaction,
    LegacyMrdrRequiresOptIn,
    ProfileViolation,
    UnknownProfile,
    IconUnavailable,
    ShortWrite,
}
impl ErrorKind {
    pub fn text(self) -> &'static str {
        match self {
            ErrorKind::InvalidFileId => "dicomdir: invalid file ID",
            ErrorKind::InvalidOptions => "dicomdir: invalid options",
            ErrorKind::MissingRequiredKey => "dicomdir: required directory key is missing",
            ErrorKind::SourceChanged => "dicomdir: source changed",
            ErrorKind::ResourceLimit => "dicomdir: resource limit exceeded",
            ErrorKind::UnsupportedSopClass => "dicomdir: unsupported SOP class",
            ErrorKind::InvalidRecord => "dicomdir: invalid directory record",
            ErrorKind::DicomdirWrite => "dicomdir: write failed",
            ErrorKind::Transaction => "dicomdir: transaction failed",
            ErrorKind::LegacyMrdrRequiresOptIn => {
                "dicomdir: legacy MRDR requires explicit compatibility mode"
            }
            ErrorKind::ProfileViolation => "dicomdir: file set violates the application profile",
            ErrorKind::UnknownProfile => "dicomdir: unknown application profile",
            ErrorKind::IconUnavailable => "dicomdir: icon image could not be rendered",
            ErrorKind::ShortWrite => "short write",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Error {
    text: String,
    kinds: Vec<ErrorKind>,
    profile: Option<Box<ProfileError>>,
    pub(crate) source_index: Option<usize>,
}
impl Error {
    pub fn sentinel(kind: ErrorKind) -> Error {
        Error {
            text: kind.text().to_string(),
            kinds: vec![kind],
            profile: None,
            source_index: None,
        }
    }
    pub fn other(text: impl Into<String>) -> Error {
        Error {
            text: text.into(),
            kinds: Vec::new(),
            profile: None,
            source_index: None,
        }
    }
    pub(crate) fn wrapped(text: String, kind: ErrorKind) -> Error {
        Error {
            text,
            kinds: vec![kind],
            profile: None,
            source_index: None,
        }
    }
    pub(crate) fn redacted(kind: ErrorKind) -> Error {
        Error {
            text: "dicomdir: operation failed".to_string(),
            kinds: vec![kind],
            profile: None,
            source_index: None,
        }
    }
    pub(crate) fn source_validation(index: usize) -> Error {
        Error {
            text: "dicomdir: source validation failed".to_string(),
            kinds: vec![ErrorKind::SourceChanged],
            profile: None,
            source_index: Some(index),
        }
    }
    pub(crate) fn join(a: ErrorKind, b: ErrorKind) -> Error {
        Error {
            text: format!("{}\n{}", a.text(), b.text()),
            kinds: vec![a, b],
            profile: None,
            source_index: None,
        }
    }
    pub(crate) fn missing_key() -> Error {
        Error::join(ErrorKind::InvalidRecord, ErrorKind::MissingRequiredKey)
    }
    pub(crate) fn open(relative: &str, err: std::io::Error) -> Error {
        Error::other(format!(
            "dicomdir: open referenced file {}: {}",
            crate::textfmt::quote(relative),
            io_text(&err)
        ))
    }
    pub(crate) fn profile(err: ProfileError) -> Error {
        Error {
            text: err.to_string(),
            kinds: vec![ErrorKind::ProfileViolation],
            profile: Some(Box::new(err)),
            source_index: None,
        }
    }
    pub(crate) fn unknown_profile(profile: &str) -> Error {
        Error {
            text: format!(
                "{}: {}",
                ErrorKind::UnknownProfile.text(),
                crate::textfmt::quote(profile)
            ),
            kinds: vec![ErrorKind::UnknownProfile],
            profile: None,
            source_index: None,
        }
    }
    pub(crate) fn icon(source: usize, cause: &str) -> Error {
        Error {
            text: format!(
                "source {source}: {}: {cause}",
                ErrorKind::IconUnavailable.text()
            ),
            kinds: vec![ErrorKind::IconUnavailable],
            profile: None,
            source_index: None,
        }
    }
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kinds.contains(&kind)
    }
    pub fn kinds(&self) -> &[ErrorKind] {
        &self.kinds
    }
    pub fn profile_error(&self) -> Option<&ProfileError> {
        self.profile.as_deref()
    }
}
impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Error {
        Error::sentinel(kind)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub(crate) fn io_text(err: &std::io::Error) -> String {
    let text = err.to_string();
    match text.rfind(" (os error ") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}
