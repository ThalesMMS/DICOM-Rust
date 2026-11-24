#![forbid(unsafe_code)]
use super::types::{Length, Tag, UNDEFINED_LENGTH, Vr, length_string};
use std::fmt;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Eof,
    UnexpectedEof,
    MaxTotalBytes,
    ContainerOverrun,
    IncompletePixelData,
    IncompletePixelDataOptions,
    PartialRead,
    FileMeta,
    DataSet,
    MissingTransferSyntax,
    UnknownTransferSyntax,
    UnsupportedTransferSyntax,
    MissingPreamble,
    InvalidFileMetaGroupLength,
    InvalidFileMetaGroupLengthValue,
    DeflateCorrupt,
    Other,
    WalkResourceLimit,
    InvalidWalkOptions,
    NilFile,
    MissingSopClassUid,
    MissingSopInstanceUid,
    SopClassMismatch,
    InvalidSopClassUid,
    InvalidSopClassPolicy,
    FrameSink,
    MissingFrameMetadata,
    InvalidFrameMetadata,
    FrameDataSizeMismatch,
    LegacySyntaxFrameSink,
    ContextCanceled,
    WritePlanLimit,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::Eof => "EOF",
            Kind::UnexpectedEof => "unexpected EOF",
            Kind::MaxTotalBytes => "dicom: maximum total bytes exceeded",
            Kind::ContainerOverrun => "dicom: length exceeds the enclosing sequence or item",
            Kind::IncompletePixelData => "dicom: incomplete Pixel Data",
            Kind::IncompletePixelDataOptions => {
                "dicom: incompatible incomplete Pixel Data recovery options"
            }
            Kind::PartialRead => "dicom: partial read: the data set ends early or is malformed",
            Kind::FileMeta => "dicom: file meta error",
            Kind::DataSet => "dicom: data set error",
            Kind::MissingTransferSyntax => {
                "dicom: missing TransferSyntaxUID in file meta information"
            }
            Kind::UnknownTransferSyntax => "dicom: unknown transfer syntax",
            Kind::UnsupportedTransferSyntax => "dicom: unsupported transfer syntax",
            Kind::MissingPreamble => "dicom: missing Part 10 preamble or DICM marker",
            Kind::InvalidFileMetaGroupLength => "dicom: invalid file meta group length element",
            Kind::InvalidFileMetaGroupLengthValue => "dicom: invalid file meta group length value",
            Kind::DeflateCorrupt | Kind::Other => "",
            Kind::WalkResourceLimit => "dicom: Walk resource limit exceeded",
            Kind::InvalidWalkOptions => "dicom: invalid Walk options",
            Kind::NilFile => "dicom: nil file",
            Kind::MissingSopClassUid => "dicom: missing SOP Class UID",
            Kind::MissingSopInstanceUid => "dicom: missing SOP Instance UID",
            Kind::SopClassMismatch => "dicom: File Meta and data set SOP Class UIDs differ",
            Kind::InvalidSopClassUid => "dicom: invalid or unavailable SOP Class UID value",
            Kind::InvalidSopClassPolicy => "dicom: invalid SOP Class resolution policy",
            Kind::FrameSink => "dicom: frame sink failed",
            Kind::MissingFrameMetadata => "dicom: missing required frame metadata",
            Kind::InvalidFrameMetadata => "dicom: invalid frame metadata",
            Kind::FrameDataSizeMismatch => {
                "dicom: native Pixel Data size does not match frame metadata"
            }
            Kind::LegacySyntaxFrameSink => {
                "dicom: legacy transfer syntaxes cannot stream frames to a sink"
            }
            Kind::ContextCanceled => "context canceled",
            Kind::WritePlanLimit => "dicom: sequence write planning limit exceeded",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    ReadTag,
    ReadVr,
    ReadLength,
    ReadReserved,
    ValidateReserved,
    ReadValue,
    CheckDepth,
    CheckElementCount,
    CheckFragmentCount,
    CheckTotalBytes,
}
impl Op {
    pub fn text(self) -> &'static str {
        match self {
            Op::ReadTag => "read tag",
            Op::ReadVr => "read VR",
            Op::ReadLength => "read length",
            Op::ReadReserved => "read reserved bytes",
            Op::ValidateReserved => "validate reserved bytes",
            Op::ReadValue => "read value",
            Op::CheckDepth => "check depth",
            Op::CheckElementCount => "check element count",
            Op::CheckFragmentCount => "check fragment count",
            Op::CheckTotalBytes => "check total bytes",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParseInfo {
    pub op: Op,
    pub offset: i64,
    pub tag: Tag,
    pub length: Length,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PathStep {
    pub tag: Tag,
    pub item_index: i64,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PartialContainer {
    pub tag: Tag,
    pub item_index: i64,
    pub offset: i64,
    pub retained: bool,
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct PartialDataSet {
    pub path: Vec<PathStep>,
    pub containers: Vec<PartialContainer>,
}
#[doc(hidden)]
pub fn path_string(path: &[PathStep]) -> String {
    path.iter()
        .map(|s| {
            if s.item_index >= 0 {
                format!("{}[{}]", s.tag, s.item_index)
            } else {
                s.tag.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Error {
    msg: String,
    kinds: Vec<Kind>,
    bare: bool,
    parse: Option<ParseInfo>,
    partial: Option<Box<PartialDataSet>>,
}
impl Error {
    pub fn sentinel(kind: Kind) -> Error {
        Error {
            msg: kind.text().to_string(),
            kinds: vec![kind],
            bare: true,
            parse: None,
            partial: None,
        }
    }
    pub fn eof() -> Error {
        Error::sentinel(Kind::Eof)
    }
    pub fn unexpected_eof() -> Error {
        Error::sentinel(Kind::UnexpectedEof)
    }
    pub fn msg(msg: impl Into<String>) -> Error {
        Error {
            msg: msg.into(),
            kinds: vec![],
            bare: false,
            parse: None,
            partial: None,
        }
    }
    pub fn new_kind(kind: Kind, msg: impl Into<String>) -> Error {
        Error {
            msg: msg.into(),
            kinds: vec![kind],
            bare: true,
            parse: None,
            partial: None,
        }
    }
    pub(crate) fn kind_suffix(kind: Kind, suffix: &str) -> Error {
        Error {
            msg: format!("{}{}", kind.text(), suffix),
            kinds: vec![kind],
            bare: false,
            parse: None,
            partial: None,
        }
    }
    pub fn wrap(prefix: &str, inner: Error) -> Error {
        Error {
            msg: format!("{prefix}: {}", inner.msg),
            bare: false,
            ..inner
        }
    }
    pub fn stage(kind: Kind, inner: Error) -> Error {
        let mut kinds = vec![kind];
        kinds.extend(inner.kinds.iter().copied());
        Error {
            msg: format!("{}: {}", kind.text(), inner.msg),
            kinds,
            bare: false,
            ..inner
        }
    }
    pub fn join(a: Option<Error>, b: Option<Error>) -> Option<Error> {
        match (a, b) {
            (None, None) => None,
            (Some(a), None) | (None, Some(a)) => Some(Error { bare: false, ..a }),
            (Some(a), Some(b)) => {
                let mut kinds = a.kinds.clone();
                kinds.extend(b.kinds.iter().copied());
                Some(Error {
                    msg: format!("{}\n{}", a.msg, b.msg),
                    kinds,
                    bare: false,
                    parse: a.parse.or(b.parse),
                    partial: a.partial.or(b.partial),
                })
            }
        }
    }
    pub(crate) fn relabel(msg: &str, inner: Error) -> Error {
        Error {
            msg: msg.to_string(),
            bare: false,
            ..inner
        }
    }
    pub fn parse(
        op: Op,
        offset: i64,
        tag: Tag,
        vr: Option<Vr>,
        length: Length,
        inner: Option<Error>,
    ) -> Error {
        let mut msg = format!("dicom: {} at offset {offset}", op.text());
        if !tag.is_zero() {
            msg.push_str(&format!(" for tag {tag}"));
        }
        if let Some(vr) = vr {
            msg.push_str(&format!(" with VR {vr}"));
        }
        if length != 0 || length == UNDEFINED_LENGTH || op == Op::ReadValue {
            msg.push_str(&format!(" with length {}", length_string(length)));
        }
        let info = ParseInfo {
            op,
            offset,
            tag,
            length,
        };
        match inner {
            Some(inner) => {
                msg.push_str(&format!(": {}", inner.msg));
                Error {
                    msg,
                    bare: false,
                    parse: Some(info),
                    ..inner
                }
            }
            None => Error {
                msg,
                kinds: vec![],
                bare: false,
                parse: Some(info),
                partial: None,
            },
        }
    }
    pub(crate) fn partial_data_set(
        path: Vec<PathStep>,
        containers: Vec<PartialContainer>,
        inner: Error,
    ) -> Error {
        let msg = format!(
            "dicom: incomplete data set at {}: {}",
            path_string(&path),
            inner.msg
        );
        Error {
            msg,
            bare: false,
            partial: Some(Box::new(PartialDataSet { path, containers })),
            ..inner
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }
    pub fn is_exactly(&self, kind: Kind) -> bool {
        self.bare && self.kinds.first() == Some(&kind)
    }
    #[doc(hidden)]
    pub fn parse_info(&self) -> Option<ParseInfo> {
        self.parse
    }
    pub(crate) fn partial_info(&self) -> Option<&PartialDataSet> {
        self.partial.as_deref()
    }
    pub(crate) fn partial_info_mut(&mut self) -> Option<&mut PartialDataSet> {
        self.partial.as_deref_mut()
    }
    pub fn message(&self) -> &str {
        &self.msg
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[doc(hidden)]
pub fn quote_bytes(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for chunk in bytes.utf8_chunks() {
        let valid = crate::textfmt::quote(chunk.valid());
        out.push_str(&valid[1..valid.len() - 1]);
        for b in chunk.invalid() {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out.push('"');
    out
}
