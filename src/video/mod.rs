#![forbid(unsafe_code)]
use std::fmt;
mod avc;
mod export;
mod extract;
mod h264;
mod hevc;
mod import;
mod import_mp4;
mod import_mpeg;
mod mux;
mod nal;
mod ts;
pub use avc::{
    AvcCodecLimits, AvcDecoder, AvcEncodeOptions, AvcEncoder, AvcProfile, AvcTranscodeOptions,
    AvcTranscodeReport, BackendError, DecodedAvc, decode_avc_mp4, transcode_avc_mp4,
    transcode_avc_mp4_path,
};
pub use export::{CONTAINER_ANNEX_B, export_fragmented_mp4};
pub use extract::{
    CODEC_H264, CODEC_HEVC, CODEC_MPEG2, CONTAINER_MPEG_ELEMENTARY, CONTAINER_MPEG_TRANSPORT,
    DEFAULT_MAX_BYTES, Info, Limits, copy, inspect, metadata,
};
pub use import::{ImportLimits, SourceInfo, inspect_source};
pub use mux::{DEFAULT_MAX_SAMPLES_PER_FRAGMENT, MuxInfo, MuxOptions, mux_fragmented_mp4};
pub const DEFAULT_MAX_FRAGMENTS: usize = 131072;
pub const CONTAINER_MP4: &str = "MP4";
pub const CONTAINER_MPEG_PROGRAM: &str = "MPEG-PS/PES";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    NotVideoTransferSyntax,
    PixelDataUnavailable,
    MalformedFragments,
    Other,
    FrameRateRequired,
    CodecProfile,
    CodecBackend,
    LossyPolicy,
    MediaTooLarge,
    TooManyFragments,
    UnsupportedContainer,
    MalformedStream,
    UnsupportedStream,
    ImportMedia,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::NotVideoTransferSyntax => "dicom: transfer syntax is not supported DICOM video",
            Kind::PixelDataUnavailable => "dicom: video Pixel Data is unavailable",
            Kind::MalformedFragments => "dicom: malformed video fragment sequence",
            Kind::Other => "dicom: video error",
            Kind::FrameRateRequired => "dicom: video frame rate is required for MP4 muxing",
            Kind::CodecProfile => "dicom: unsupported or inconsistent video codec profile",
            Kind::CodecBackend => "dicom: video codec backend failed or is unavailable",
            Kind::LossyPolicy => "dicom: AVC re-encoding requires an explicit lossy policy",
            Kind::MediaTooLarge => "dicom: video media exceeds extraction limit",
            Kind::TooManyFragments => "dicom: video media exceeds fragment limit",
            Kind::UnsupportedContainer => "dicom: unsupported video container",
            Kind::MalformedStream => "dicom: malformed video elementary stream",
            Kind::UnsupportedStream => "dicom: unsupported video elementary stream",
            Kind::ImportMedia => "dicom: malformed or unsupported video import",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
}
impl Error {
    pub(crate) fn bare(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
        }
    }
    pub(crate) fn new(kind: Kind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            text: format!("{}: {detail}", kind.text()),
        }
    }
    pub(crate) fn other(text: impl Into<String>) -> Error {
        Error {
            kind: Kind::Other,
            text: text.into(),
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
pub(crate) fn malformed(detail: impl fmt::Display) -> Error {
    Error::new(Kind::MalformedStream, detail)
}
pub(crate) fn unsupported_stream(detail: impl fmt::Display) -> Error {
    Error::new(Kind::UnsupportedStream, detail)
}
pub(crate) fn import_error(what: &str) -> Error {
    Error::new(Kind::ImportMedia, what)
}
pub(crate) fn import_media() -> Error {
    Error::bare(Kind::ImportMedia)
}
pub(crate) fn too_large() -> Error {
    Error::bare(Kind::MediaTooLarge)
}
