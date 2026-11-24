#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
mod bitio;
pub mod brotli;
mod brotli_tables;
mod codestream;
mod container;
pub mod dicom;
mod encode;
mod entropy;
mod entropy_enc;
mod icc;
mod jbrd;
mod jpegparse;
mod jpegwriter;
mod lossy;
mod modular;
mod recompress;
mod reconstruct;
mod render;
mod transforms;
mod vardct;
mod xyb;
pub use encode::encode_lossless;
pub use lossy::{
    DEFAULT_LOSSY_QUALITY, LossyOptions, MAX_LOSSY_QUALITY, MIN_LOSSY_QUALITY, encode_lossy,
};
pub use recompress::recompress_jpeg;
pub use reconstruct::reconstruct_jpeg;
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidCodestream,
    Unsupported,
    NoReconstructionData,
    NotReversible,
    Limit,
    Other,
}
pub(crate) const MAX_IMAGE_PIXELS: u64 = 1 << 28;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    detail: Option<String>,
}
impl Error {
    pub(crate) fn new(kind: ErrorKind, detail: Option<String>) -> Self {
        Error { kind, detail }
    }
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
    pub fn is_invalid_codestream(&self) -> bool {
        self.kind == ErrorKind::InvalidCodestream
    }
    pub fn is_unsupported(&self) -> bool {
        self.kind == ErrorKind::Unsupported
    }
    pub fn is_no_reconstruction_data(&self) -> bool {
        self.kind == ErrorKind::NoReconstructionData
    }
    pub fn is_not_reversible(&self) -> bool {
        self.kind == ErrorKind::NotReversible
    }
    pub fn is_limit(&self) -> bool {
        self.kind == ErrorKind::Limit
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let base = match self.kind {
            ErrorKind::InvalidCodestream => "jpegxl: invalid codestream",
            ErrorKind::Unsupported => "jpegxl: unsupported feature",
            ErrorKind::NoReconstructionData => "jpegxl: no JPEG reconstruction data",
            ErrorKind::NotReversible => "jpegxl: JPEG cannot be recompressed reversibly",
            ErrorKind::Limit => "jpegxl: resource limit exceeded",
            ErrorKind::Other => "",
        };
        match &self.detail {
            Some(d) if base.is_empty() => f.write_str(d),
            Some(d) => write!(f, "{base}: {d}"),
            None => f.write_str(base),
        }
    }
}
impl std::error::Error for Error {}
pub(crate) fn corrupt(detail: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidCodestream, Some(detail.into()))
}
pub(crate) fn unsupported(detail: impl Into<String>) -> Error {
    Error::new(ErrorKind::Unsupported, Some(detail.into()))
}
pub(crate) fn limit() -> Error {
    Error::new(ErrorKind::Limit, None)
}
pub(crate) fn truncated() -> Error {
    corrupt("truncated bit stream")
}
pub(crate) fn fault() -> Error {
    corrupt("internal decoder fault")
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub bits_per_sample: u32,
    pub samples: Vec<i32>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LosslessOptions {
    pub bits_per_sample: u32,
}
pub fn decode(data: &[u8]) -> Result<Image, Error> {
    codestream::decode(data, true)
}
