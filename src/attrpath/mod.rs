#![forbid(unsafe_code)]
mod edit;
mod path;
mod script;
mod session;
pub use edit::{
    DEFAULT_MAX_CREATED_ITEMS, Match, Options, create, element_from_string, find, insert, modify,
    remove,
};
pub use path::{MAX_ITEM_INDEX, MAX_PATH_LENGTH, MAX_SEGMENTS, Path, Segment, parse};
pub use script::{Edit, MAX_SCRIPT_BYTES, MAX_SCRIPT_EDITS, Operation, Script, parse_script};
pub use session::{Condition, EditLimits, EditSession, MAX_CONDITION_LENGTH, parse_condition};
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Syntax,
    NotFound,
    NotSequence,
    Value,
    UnknownVr,
    Limit,
    Private(crate::dictionary::reservations::PrivateError),
}
impl Kind {
    pub fn text(self) -> String {
        match self {
            Kind::Syntax => "dicom/attrpath: invalid path".into(),
            Kind::NotFound => "dicom/attrpath: attribute not found".into(),
            Kind::NotSequence => "dicom/attrpath: attribute is not a sequence".into(),
            Kind::Value => "dicom/attrpath: invalid value".into(),
            Kind::UnknownVr => "dicom/attrpath: unknown VR".into(),
            Kind::Limit => "dicom/attrpath: limit exceeded".into(),
            Kind::Private(p) => p.to_string(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
}
impl Error {
    pub(crate) fn new(kind: Kind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            text: format!("{}: {detail}", kind.text()),
        }
    }
    pub(crate) fn bare(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text(),
        }
    }
    pub(crate) fn prefixed(self, prefix: impl fmt::Display) -> Error {
        Error {
            kind: self.kind,
            text: format!("{prefix}: {}", self.text),
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
impl From<crate::dictionary::reservations::PrivateError> for Error {
    fn from(p: crate::dictionary::reservations::PrivateError) -> Error {
        Error::bare(Kind::Private(p))
    }
}
