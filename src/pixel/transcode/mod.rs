mod admission;
pub(crate) mod decompress;
mod encode;
mod output;
mod path;
mod trailing_repair;
mod verify;
use super::Metadata;
use super::pixeldata::{PixelData, PixelError, PixelErrorKind, extract_metadata, total_size};
use crate::object::{Element, File, Object, Tag, Value, Vr};
use crate::textfmt;
use crate::transfer::{self, Syntax};
pub use admission::{check_decode_limits, check_decode_limits_deferred, is_limit_error};
pub use decompress::{
    DIAGNOSTIC_BIT_COUNT_MASKS, DIAGNOSTIC_DEFAULT_PHOTOMETRIC,
    DIAGNOSTIC_DEFAULT_PLANAR_CONFIGURATION, DIAGNOSTIC_DEFAULT_SAMPLES_PER_PIXEL,
    DIAGNOSTIC_DEFINED_LENGTH_FRAGMENTS, DIAGNOSTIC_IGNORED_PLANAR_CONFIGURATION,
    DIAGNOSTIC_INCOMPLETE_PIXEL_DATA, DIAGNOSTIC_INVALID_PHOTOMETRIC_REPLACED,
    DIAGNOSTIC_JP2_CONTAINER_UNWRAPPED, DIAGNOSTIC_LIBIDO_DIMENSIONS, DIAGNOSTIC_LIBIDO_RGB24,
    DIAGNOSTIC_NATIVE_OW_BYTE_SWAP, DIAGNOSTIC_NATIVE_PLANAR_OVERRIDE,
    DIAGNOSTIC_NATIVE_UNDER_ENCAPSULATED, DIAGNOSTIC_NATIVE_WHOLE_IMAGE_PLANAR,
    DIAGNOSTIC_NUMBER_OF_FRAMES_FROM_PLANES, DIAGNOSTIC_PHOTOMETRIC_FROM_SAMPLES,
    DIAGNOSTIC_RECONCILED_WITH_CODESTREAM, DIAGNOSTIC_RETIRED_ARGB_TO_RGB,
    DIAGNOSTIC_RETIRED_CMYK_TO_RGB, DIAGNOSTIC_RETIRED_HSV_TO_RGB,
    DIAGNOSTIC_RLE_REVERSED_BYTE_SEGMENTS, DIAGNOSTIC_SAMPLES_FROM_PHOTOMETRIC,
    DIAGNOSTIC_UNPACKED_12_BIT, DIAGNOSTIC_WRONG_PALETTE_ATTRIBUTE_TAGS, DecodeRecovery,
    DecompressReport, ERR_NO_COMPLETE_FRAMES, IncompleteFrameRecovery, JpegColorConversion,
    MetadataReconciliation, NATIVE_PLANAR_BY_FRAME, NATIVE_PLANAR_DECLARED,
    NATIVE_PLANAR_INTERLEAVED, NATIVE_PLANAR_WHOLE_IMAGE, NativePlanarRecovery,
    decompress_data_set_with_recovery, decompress_file_with_recovery, detectable_decode_recovery,
};
pub use decompress::{
    DecompressLimits, DecompressOptions, decompress_data_set, decompress_file,
    default_decompress_limits,
};
pub use decompress::{
    LossyEvidence, LossyStateReport, detect_data_set_lossy_state, detect_lossy_state,
};
pub(crate) use encode::require_lossless_encoder;
pub use encode::{EncoderCapabilities, EncoderCounter, EncoderRegistry, FrameEncoder};
pub use output::{IconAction, IconReport};
pub use path::{
    DataSetPathOptions, DataSetPathReport, ERR_STREAMING_INELIGIBLE, PipelineOptions, durability,
    transcode_data_set_path, transcode_path,
};
use std::fmt;
pub use trailing_repair::{
    ERR_TRAILING_DATA_NOT_REPAIRABLE, ERR_TRAILING_DATA_REPAIR_LIMIT, TrailingDataRepairOptions,
    TrailingDataRepairReport, TrailingRepairError, repair_part10_trailing_data,
};
pub use verify::{
    ERR_COMPARISON_UNSUPPORTED, ERR_PIXEL_ERROR, ERR_VERIFICATION_OPTIONS, LossyVerificationError,
    LossyVerificationReport, resolve_verification_options,
};
pub(crate) const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
pub(crate) const TAG_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0008);
pub(crate) const TAG_DOUBLE_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0009);
pub(crate) const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
pub(crate) const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
pub(crate) const TAG_ENCAPSULATED_TOTAL_LENGTH: Tag = Tag::new(0x7FE0, 0x0003);
pub(crate) const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
pub(crate) const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
pub(crate) const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
pub(crate) const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
pub(crate) const TAG_PLANAR_CONFIGURATION: Tag = Tag::new(0x0028, 0x0006);
pub(crate) const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_META_TRANSFER_SYNTAX: Tag = Tag::new(0x0002, 0x0010);
const TAG_MEDIA_STORAGE_SOP_INSTANCE: Tag = Tag::new(0x0002, 0x0003);
const TAG_MEDIA_STORAGE_SOP_CLASS: Tag = Tag::new(0x0002, 0x0002);
pub(crate) const UID_EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
pub(crate) const UID_IMPLICIT_LE: &str = "1.2.840.10008.1.2";
pub(crate) const UID_EXPLICIT_BE: &str = "1.2.840.10008.1.2.2";
pub(crate) const UID_DEFLATED_LE: &str = "1.2.840.10008.1.2.1.99";
pub(crate) const UID_ENCAPSULATED_UNCOMPRESSED: &str = "1.2.840.10008.1.2.1.98";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Unsupported,
    ResourceLimit,
    LossyDisallowed,
    Transaction,
    Stage,
    UnknownTransferSyntax,
    Pixel(PixelErrorKind),
    DecompressResourceLimit,
    InvalidDecompressLimits,
    UnsupportedDecompression,
    CodecRegistryNil,
    EncoderUnavailable,
    EncoderCapabilitiesInvalid,
    UnsupportedEncoderMetadata,
    EncoderFailed,
    EncoderOutputInvalid,
    EmbeddedOverlay,
    PartialFile,
    FrameLayout,
    EncapsulatedResourceLimit,
    Other,
    SourceChanged,
    DestinationUnsafe,
    VerificationOptions,
    PixelError,
    ComparisonUnsupported,
    StreamingIneligible,
    BufferLayoutUnavailable,
    BufferTooSmall,
    InvalidRegion,
    InvalidReduction,
    InvalidQualityLayers,
    PartialDecodeUnsupported,
    RegionUnsupported,
    NativeRewrite,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    text: String,
    inner: Option<Box<Error>>,
    lossy: Option<(i64, u64, u64, i64)>,
}
pub(crate) const ERR_UNSUPPORTED: &str = "dicom: transfer syntax transcode is unsupported";
pub(crate) const ERR_RESOURCE_LIMIT: &str = "dicom: transcode resource limit exceeded";
pub(crate) const ERR_LOSSY_DISALLOWED: &str = "dicom: lossy transcode requires explicit opt-in";
pub(crate) const ERR_TRANSACTION: &str = "dicom: transcode transaction failed";
pub(crate) const ERR_PARTIAL_FILE: &str = "dicom: file is the result of a partial read";
pub(crate) const ERR_SOURCE_CHANGED: &str = "dicom: transcode source changed";
pub(crate) const ERR_DESTINATION_UNSAFE: &str = "dicom: transcode destination is unsafe";
impl Error {
    pub fn custom(kind: ErrorKind, text: impl Into<String>) -> Error {
        Error::new(kind, text)
    }
    pub(crate) fn new(kind: ErrorKind, text: impl Into<String>) -> Error {
        Error {
            kind,
            text: text.into(),
            inner: None,
            lossy: None,
        }
    }
    pub(crate) fn unsupported(detail: impl fmt::Display) -> Error {
        Error::new(
            ErrorKind::Unsupported,
            format!("{ERR_UNSUPPORTED}: {detail}"),
        )
    }
    pub(crate) fn limit(field: &str) -> Error {
        Error::new(
            ErrorKind::ResourceLimit,
            format!("{ERR_RESOURCE_LIMIT}: {field}"),
        )
    }
    pub(crate) fn stage(stage: &str, inner: Error) -> Error {
        Error {
            kind: ErrorKind::Stage,
            text: format!("dicom: transcode failed during {stage}"),
            inner: Some(Box::new(inner)),
            lossy: None,
        }
    }
    pub(crate) fn wrapping(kind: ErrorKind, text: impl Into<String>, inner: Error) -> Error {
        Error {
            kind,
            text: text.into(),
            inner: Some(Box::new(inner)),
            lossy: None,
        }
    }
    pub(crate) fn pixel(kind: PixelErrorKind, detail: impl fmt::Display) -> Error {
        Error::new(
            ErrorKind::Pixel(kind),
            format!("{}: {detail}", pixel_base(kind)),
        )
    }
    pub(crate) fn pixel_bare(kind: PixelErrorKind) -> Error {
        Error::new(ErrorKind::Pixel(kind), pixel_base(kind))
    }
    pub(crate) fn is_resource_limit(&self) -> bool {
        self.is(ErrorKind::ResourceLimit)
    }
    #[doc(hidden)]
    pub fn stage_parts(&self) -> Option<(&str, &Error)> {
        let inner = self.inner.as_deref()?;
        if self.kind == ErrorKind::Stage {
            let stage = self
                .text
                .strip_prefix("dicom: transcode failed during ")
                .unwrap_or_default();
            return Some((stage, inner));
        }
        inner.stage_parts()
    }
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kind == kind || self.inner.as_ref().is_some_and(|e| e.is(kind))
    }
    pub fn lossy_verification(&self) -> Option<LossyVerificationError> {
        match self.lossy {
            Some((frame, measured, limit, average_block_size)) => Some(LossyVerificationError {
                frame,
                measured: f64::from_bits(measured),
                limit: f64::from_bits(limit),
                average_block_size,
            }),
            None => self.inner.as_ref().and_then(|e| e.lossy_verification()),
        }
    }
    pub(crate) fn with_lossy(mut self, e: &LossyVerificationError) -> Error {
        self.lossy = Some((
            e.frame,
            e.measured.to_bits(),
            e.limit.to_bits(),
            e.average_block_size,
        ));
        self
    }
}
fn pixel_base(kind: PixelErrorKind) -> &'static str {
    match kind {
        PixelErrorKind::PixelDataNotFound => "dicom: pixel data element not found",
        PixelErrorKind::MissingMetadata => "dicom: missing required pixel data metadata",
        PixelErrorKind::InvalidMetadata => "dicom: invalid pixel data metadata",
        PixelErrorKind::IncompatiblePixelData => {
            "dicom: pixel data is incompatible with transfer syntax"
        }
        PixelErrorKind::PixelDataSizeMismatch => "dicom: pixel data size does not match metadata",
        PixelErrorKind::UnsupportedValueType => "dicom: pixel data is not a supported value type",
        _ => "",
    }
}
impl From<PixelError> for Error {
    fn from(e: PixelError) -> Error {
        let text = e.to_string();
        match e.cause() {
            Some(cause) => Error::wrapping(
                ErrorKind::Pixel(e.kind),
                text.clone(),
                Error::new(ErrorKind::Pixel(cause), text),
            ),
            None => Error::new(ErrorKind::Pixel(e.kind), text),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TranscodeLimits {
    pub max_frames: i64,
    pub max_pixels: u64,
    pub max_input_bytes: i64,
    pub max_output_bytes: i64,
    pub max_fragments: i64,
    pub max_elements: i64,
    pub max_depth: i64,
    pub max_expansion_ratio: i64,
    pub max_duration_nanos: i64,
}
pub fn default_limits() -> TranscodeLimits {
    TranscodeLimits {
        max_frames: 10000,
        max_pixels: 1 << 30,
        max_input_bytes: 1 << 30,
        max_output_bytes: 1 << 30,
        max_fragments: 100000,
        max_elements: 1000000,
        max_depth: 64,
        max_expansion_ratio: 1024,
        max_duration_nanos: 5 * 60 * 1_000_000_000,
    }
}
#[doc(hidden)]
pub fn resolve_limits(mut l: TranscodeLimits) -> Result<TranscodeLimits, Error> {
    let d = default_limits();
    if l.max_frames == 0 {
        l.max_frames = d.max_frames;
    }
    if l.max_pixels == 0 {
        l.max_pixels = d.max_pixels;
    }
    if l.max_input_bytes == 0 {
        l.max_input_bytes = d.max_input_bytes;
    }
    if l.max_output_bytes == 0 {
        l.max_output_bytes = d.max_output_bytes;
    }
    if l.max_fragments == 0 {
        l.max_fragments = d.max_fragments;
    }
    if l.max_elements == 0 {
        l.max_elements = d.max_elements;
    }
    if l.max_depth == 0 {
        l.max_depth = d.max_depth;
    }
    if l.max_expansion_ratio == 0 {
        l.max_expansion_ratio = d.max_expansion_ratio;
    }
    if l.max_duration_nanos == 0 {
        l.max_duration_nanos = d.max_duration_nanos;
    }
    if l.max_frames < 0
        || l.max_pixels > i64::MAX as u64
        || l.max_input_bytes < 0
        || l.max_output_bytes < 0
        || l.max_fragments < 0
        || l.max_elements < 0
        || l.max_depth < 0
        || l.max_expansion_ratio < 0
        || l.max_duration_nanos < 0
    {
        return Err(Error::new(
            ErrorKind::ResourceLimit,
            format!("{ERR_RESOURCE_LIMIT}: invalid limits"),
        ));
    }
    Ok(l)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderRegistry {
    Builtin,
    BuiltinWithoutJpeg2000,
    BuiltinWithJpegXl,
    Custom(&'static crate::pixel::codec_router::CodecRegistry),
}
impl DecoderRegistry {
    pub fn check(self, uid: &str) -> Result<(), PixelError> {
        match self {
            DecoderRegistry::Custom(r) if r.overrides(uid) => Ok(()),
            DecoderRegistry::Custom(r) => r.base().check(uid),
            DecoderRegistry::Builtin => crate::pixel::pixeldata::check_codec_availability(uid),
            DecoderRegistry::BuiltinWithoutJpeg2000 => {
                crate::pixel::pixeldata::check_builtin_codec_availability(uid)
            }
            DecoderRegistry::BuiltinWithJpegXl => {
                crate::pixel::pixeldata::check_codec_availability_with_jpegxl(uid)
            }
        }
    }
    pub fn decode_frames(
        self,
        uid: &str,
        pixel: &PixelData<'_>,
        obj: &Object,
    ) -> Result<super::pixeldata::Frames, PixelError> {
        match self {
            DecoderRegistry::Custom(r) => r.decode_frames(
                &crate::pixel::codec_router::DecodeContext::default(),
                uid,
                pixel,
                obj,
            ),
            DecoderRegistry::BuiltinWithJpegXl => {
                super::pixeldata::decode_frames_with_jpegxl(uid, pixel, obj)
            }
            DecoderRegistry::BuiltinWithoutJpeg2000 if !self.decodes_frames_of(uid, pixel) => {
                Err(self
                    .check(uid)
                    .err()
                    .unwrap_or_else(|| PixelError::new(PixelErrorKind::CodecNotFound, "")))
            }
            _ => super::pixeldata::decode_frames(uid, pixel, obj),
        }
    }
    pub(crate) fn decode_frames_with_recovery(
        self,
        uid: &str,
        pixel: &PixelData<'_>,
        obj: &Object,
        recovery: &super::pixeldata::CodecRecovery,
    ) -> Result<super::pixeldata::Frames, PixelError> {
        match self {
            DecoderRegistry::Custom(r) => r.decode_frames(
                &crate::pixel::codec_router::DecodeContext {
                    recovery: *recovery,
                    ..Default::default()
                },
                uid,
                pixel,
                obj,
            ),
            DecoderRegistry::BuiltinWithJpegXl => {
                super::pixeldata::decode_frames_with_recovery_and_jpegxl(uid, pixel, obj, recovery)
            }
            DecoderRegistry::BuiltinWithoutJpeg2000 if !self.decodes_frames_of(uid, pixel) => {
                self.decode_frames(uid, pixel, obj)
            }
            _ => super::pixeldata::decode_frames_with_recovery(uid, pixel, obj, recovery),
        }
    }
    fn decodes_frames_of(self, uid: &str, pixel: &PixelData<'_>) -> bool {
        !(self == DecoderRegistry::BuiltinWithoutJpeg2000
            && matches!(pixel, PixelData::Encapsulated { .. })
            && (DecoderRegistry::Builtin.decodes(uid)
                || crate::transfer::is_jpeg_xl_transfer_syntax(uid))
            && !self.decodes(uid))
    }
    pub fn decodes(self, uid: &str) -> bool {
        self.check(uid).is_ok()
    }
    pub(crate) fn refuse(registry: Option<DecoderRegistry>, uid: &str) -> Result<(), Error> {
        let uid = crate::transfer::normalize_uid(uid);
        if let Some(DecoderRegistry::Custom(r)) = registry {
            if r.overrides(uid) {
                return Ok(());
            }
            return DecoderRegistry::refuse(Some(r.base()), uid);
        }
        if registry == Some(DecoderRegistry::BuiltinWithoutJpeg2000)
            && crate::transfer::default_registry()
                .get(uid)
                .is_some_and(|s| s.requires_codec())
            && DecoderRegistry::Builtin.decodes(uid)
        {
            DecoderRegistry::BuiltinWithoutJpeg2000.check(uid)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodestreamTranscoder {
    #[cfg(feature = "jpeg-xl")]
    JpegXlRecompressor,
}
impl CodestreamTranscoder {
    #[doc(hidden)]
    pub fn can_transcode(self, source_uid: &str, target_uid: &str) -> bool {
        #[cfg(not(feature = "jpeg-xl"))]
        let _ = (source_uid, target_uid);
        match self {
            #[cfg(feature = "jpeg-xl")]
            CodestreamTranscoder::JpegXlRecompressor => {
                crate::codec::jpegxl::dicom::can_transcode_codestream(source_uid, target_uid)
            }
        }
    }
    #[cfg(feature = "jpeg-xl")]
    pub fn transcode(
        self,
        source_uid: &str,
        target_uid: &str,
        pixel: &PixelData<'_>,
        obj: &Object,
    ) -> Result<Vec<Vec<u8>>, crate::codec::jpegxl::dicom::DicomError> {
        match self {
            CodestreamTranscoder::JpegXlRecompressor => {
                crate::codec::jpegxl::dicom::transcode_codestreams(
                    source_uid, target_uid, pixel, obj,
                )
            }
        }
    }
    #[cfg(not(feature = "jpeg-xl"))]
    pub fn transcode(
        self,
        _source_uid: &str,
        _target_uid: &str,
        _pixel: &PixelData<'_>,
        _obj: &Object,
    ) -> Result<Vec<Vec<u8>>, std::convert::Infallible> {
        match self {}
    }
}
#[cfg_attr(not(feature = "jpeg-xl"), allow(unreachable_code))]
pub(super) fn uses_codestream_transcoder(
    mode: Mode,
    source: &Syntax,
    target: &Syntax,
    options: &TranscodeOptions,
    legacy_repair: bool,
) -> Option<CodestreamTranscoder> {
    let transcoder = options.codestream_transcoder?;
    (mode == Mode::DecodeEncode
        && !options.force_reencode
        && !legacy_repair
        && source.encapsulated
        && source.uid != UID_ENCAPSULATED_UNCOMPRESSED
        && transcoder.can_transcode(&source.uid, &target.uid))
    .then_some(transcoder)
}
const ERR_CODESTREAM_OUTPUT_INVALID: &str = "dicom: codestream transcoder returned invalid output";
#[cfg_attr(not(feature = "jpeg-xl"), allow(unreachable_code))]
fn transcode_codestreams(
    dataset: &Object,
    metadata: &Metadata,
    pixel: &PixelData<'_>,
    source: &Syntax,
    target: &Syntax,
    transcoder: CodestreamTranscoder,
    limits: &TranscodeLimits,
) -> Result<(Object, TranscodeReport), Error> {
    let stage = |inner: Error| Error::stage("codestream", inner);
    let fragments = transcoder
        .transcode(&source.uid, &target.uid, pixel, dataset)
        .map_err(|e| stage(Error::new(ErrorKind::Other, e.to_string())))?;
    let invalid = |detail: String| {
        stage(Error::new(
            ErrorKind::Other,
            format!("{ERR_CODESTREAM_OUTPUT_INVALID}: {detail}"),
        ))
    };
    if fragments.len() as i64 != metadata.number_of_frames {
        return Err(invalid(format!(
            "{} frames for NumberOfFrames={}",
            fragments.len(),
            metadata.number_of_frames
        )));
    }
    if let Some(i) = fragments.iter().position(Vec::is_empty) {
        return Err(invalid(format!("empty frame {i}")));
    }
    let (draft, encoded_bytes) =
        encode::rewrite_encoded(dataset, metadata, fragments, false, None, limits)?;
    let result = clone_detached(&draft, &output_clone_limits(*limits))?;
    Ok((
        result,
        TranscodeReport {
            frames: metadata.number_of_frames,
            native_bytes: total_size(metadata),
            encoded_bytes,
            ..TranscodeReport::default()
        },
    ))
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlanarOutput {
    #[default]
    Default,
    Iod,
    Interleaved,
    Planar,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UidPolicy {
    #[default]
    Default,
    Always,
    Preserve,
}
#[derive(Debug, Clone, Default)]
pub struct TranscodeOptions {
    pub decoder: Option<DecoderRegistry>,
    pub encoders: Option<EncoderRegistry>,
    pub limits: TranscodeLimits,
    pub allow_lossy: bool,
    pub force_reencode: bool,
    pub max_fragment_bytes: i64,
    pub offset_table: u8,
    pub planar_output: PlanarOutput,
    pub secondary_capture: bool,
    pub uid_root: String,
    pub uid_policy: UidPolicy,
    pub compress_icons: bool,
    pub extract_embedded_overlays: bool,
    pub skip_verification: bool,
    pub max_pixel_error: Option<f64>,
    pub average_block_size: i64,
    pub durability: u8,
    pub pipeline: PipelineOptions,
    pub write_options: crate::object::writer::WriteFileOptions,
    pub codestream_transcoder: Option<CodestreamTranscoder>,
    pub rewrite: Option<crate::pixel::native_rewrite::NativeRewrite>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Verification {
    #[default]
    NotRequired,
    PixelExact,
    Decode,
    Skipped,
    PixelError,
}
impl Verification {
    pub fn as_str(self) -> &'static str {
        match self {
            Verification::NotRequired => "not-required",
            Verification::PixelExact => "pixel-exact",
            Verification::Decode => "decode",
            Verification::Skipped => "skipped",
            Verification::PixelError => "pixel-error",
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscodeReport {
    pub identity_changed: bool,
    pub frames: i64,
    pub native_bytes: i64,
    pub encoded_bytes: i64,
    pub pixel_data_preserved: bool,
    pub lossy: bool,
    pub embedded_overlays_extracted: i64,
    pub decode_diagnostics: Vec<String>,
    pub encode_compatibility_profiles: Vec<String>,
    pub icons: Vec<IconReport>,
    pub verification: Verification,
    pub lossy_verification: Option<LossyVerificationReport>,
    pub durability: u8,
    pub file_synced: bool,
    pub directory_synced: bool,
    pub worker_limit: i64,
    pub peak_in_flight_frames: i64,
    pub peak_in_flight_bytes: i64,
    pub streaming: bool,
    pub rewrite: Option<crate::pixel::native_rewrite::NativeRewriteReport>,
}
fn registry_get(uid: &str) -> Option<Syntax> {
    transfer::default_registry().get(uid)
}
pub(crate) fn canonical_source(uid: &str) -> Result<Syntax, Error> {
    let uid = transfer::normalize_uid(uid);
    if uid.is_empty() {
        return Err(Error::pixel(
            PixelErrorKind::IncompatiblePixelData,
            "source transfer syntax is empty",
        ));
    }
    registry_get(uid).ok_or_else(|| {
        Error::new(
            ErrorKind::UnknownTransferSyntax,
            format!("dicom: unknown transfer syntax: {}", textfmt::quote(uid)),
        )
    })
}
pub(crate) fn canonical_target(uid: &str) -> Result<Syntax, Error> {
    let uid = transfer::normalize_uid(uid);
    if uid.is_empty() {
        return Err(Error::unsupported("target transfer syntax is empty"));
    }
    let Some(got) = registry_get(uid) else {
        return Err(Error::unsupported("target transfer syntax is unknown"));
    };
    if !got.supported && !got.requires_codec() {
        return Err(Error::unsupported("target transfer syntax is unsupported"));
    }
    Ok(got)
}
pub(crate) fn file_source_syntax(file: &File) -> Result<Syntax, Error> {
    if file.partial_read.is_some() || file.incomplete_pixel_data.is_some() {
        return Err(Error::new(ErrorKind::PartialFile, ERR_PARTIAL_FILE));
    }
    let file_uid = transfer::normalize_uid(file.transfer_syntax.uid).to_string();
    let meta_uid = file
        .meta
        .get_string(TAG_META_TRANSFER_SYNTAX)
        .map(|u| transfer::normalize_uid(&u).to_string())
        .unwrap_or_default();
    if !file_uid.is_empty() && !meta_uid.is_empty() && file_uid != meta_uid {
        return Err(Error::pixel(
            PixelErrorKind::IncompatiblePixelData,
            "file transfer syntax does not match File Meta Information",
        ));
    }
    if !file_uid.is_empty() {
        return canonical_source(&file_uid);
    }
    if !meta_uid.is_empty() {
        return canonical_source(&meta_uid);
    }
    Err(Error::pixel(
        PixelErrorKind::IncompatiblePixelData,
        "source transfer syntax is empty",
    ))
}
pub(crate) fn replace_element(elements: &mut Vec<Element>, replacement: Element) {
    if let Some(e) = elements.iter_mut().find(|e| e.tag() == replacement.tag()) {
        *e = replacement;
    } else {
        elements.push(replacement);
    }
}
pub(crate) fn remove_elements(elements: &mut Vec<Element>, tags: &[Tag]) {
    elements.retain(|e| !tags.contains(&e.tag()));
}
pub(crate) fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: crate::object::ElementHeader::new(tag, vr, raw.len() as u32),
        value: Value::Raw(raw),
    }
}
pub(crate) fn uint16_element(tag: Tag, value: u16) -> Element {
    raw_element(tag, Vr::US, value.to_le_bytes().to_vec())
}
pub(crate) fn strings_element(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
    crate::derivedio::strings(tag, vr, &values)
}
pub(crate) fn fragments_element(offset_table: Vec<u8>, fragments: Vec<Vec<u8>>) -> Element {
    Element {
        header: crate::object::ElementHeader::new(
            TAG_PIXEL_DATA,
            Vr::OB,
            crate::object::UNDEFINED_LENGTH,
        ),
        value: Value::Fragments {
            offset_table,
            fragments,
        },
    }
}
pub(crate) fn object_from(elements: Vec<Element>, big_endian: bool) -> Object {
    let mut obj = Object::from_elements(elements);
    obj.set_value_big_endian(big_endian);
    obj
}
pub(crate) fn pixel_view(e: &Element) -> Result<PixelData<'_>, Error> {
    match &e.value {
        Value::Raw(raw) => Ok(PixelData::Native(raw)),
        Value::Fragments {
            offset_table,
            fragments,
        } => Ok(PixelData::Encapsulated {
            offset_table,
            fragments,
        }),
        _ => Err(Error::new(
            ErrorKind::Other,
            "dicom: pixel data is not a supported value type",
        )),
    }
}
pub(crate) fn payload_bytes(pixel: &PixelData<'_>) -> i64 {
    match pixel {
        PixelData::Native(raw) => raw.len() as i64,
        PixelData::Encapsulated {
            offset_table,
            fragments,
        } => {
            let mut total = offset_table.len() as i64;
            for f in fragments.iter() {
                if f.len() as i64 > i64::MAX.saturating_sub(total) {
                    return i64::MAX;
                }
                total = total.saturating_add(f.len() as i64);
            }
            total
        }
    }
}
pub(super) fn has_any_pixel_data(obj: &Object) -> bool {
    [
        TAG_PIXEL_DATA,
        TAG_FLOAT_PIXEL_DATA,
        TAG_DOUBLE_FLOAT_PIXEL_DATA,
    ]
    .iter()
    .any(|t| obj.has(*t))
}
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Usage {
    pub elements: i64,
    pub bytes: i64,
}
pub(crate) fn measure(obj: &Object, limits: &TranscodeLimits) -> Result<Usage, Error> {
    struct State<'l> {
        limits: &'l TranscodeLimits,
        elements: i64,
        fragments: i64,
        bytes: i64,
    }
    impl State<'_> {
        fn add(&mut self, n: i64) -> Result<(), Error> {
            if n < 0 || n > self.limits.max_output_bytes.saturating_sub(self.bytes) {
                return Err(Error::limit("output_bytes"));
            }
            self.bytes = self.bytes.saturating_add(n);
            Ok(())
        }
        fn walk(&mut self, elements: &[Element], depth: i64) -> Result<(), Error> {
            if depth > self.limits.max_depth
                || elements.len() as i64 > self.limits.max_elements.saturating_sub(self.elements)
            {
                return Err(Error::limit("elements"));
            }
            self.elements = self.elements.saturating_add(elements.len() as i64);
            for e in elements {
                match &e.value {
                    Value::Nil => {
                        return Err(Error::unsupported("deferred value is unavailable"));
                    }
                    Value::Strings(values) => {
                        if values.len() as i64
                            > self.limits.max_elements.saturating_sub(self.elements)
                        {
                            return Err(Error::limit("elements"));
                        }
                        self.elements = self.elements.saturating_add(values.len() as i64);
                        self.add(strings_len(values))?;
                    }
                    Value::Sequence(items) => {
                        if items.len() as i64
                            > self.limits.max_elements.saturating_sub(self.elements)
                        {
                            return Err(Error::limit("elements"));
                        }
                        self.elements = self.elements.saturating_add(items.len() as i64);
                        for item in items {
                            self.walk(&item.elements, depth.saturating_add(1))?;
                        }
                    }
                    Value::Fragments {
                        offset_table,
                        fragments,
                    } => {
                        if fragments.len() as i64
                            > self.limits.max_fragments.saturating_sub(self.fragments)
                        {
                            return Err(Error::limit("fragments"));
                        }
                        self.fragments = self.fragments.saturating_add(fragments.len() as i64);
                        self.add(offset_table.len() as i64)?;
                        for f in fragments {
                            self.add(f.len() as i64)?;
                        }
                    }
                    Value::BulkData(uri) => self.add(uri.len() as i64)?,
                    Value::Raw(raw) => self.add(raw.len() as i64)?,
                }
            }
            Ok(())
        }
    }
    let mut s = State {
        limits,
        elements: 0,
        fragments: 0,
        bytes: 0,
    };
    s.walk(obj.elements(), 1)?;
    Ok(Usage {
        elements: s.elements,
        bytes: s.bytes,
    })
}
pub(crate) fn clone_detached(obj: &Object, limits: &TranscodeLimits) -> Result<Object, Error> {
    struct State<'l> {
        limits: &'l TranscodeLimits,
        elements: i64,
        fragments: i64,
        bytes: i64,
    }
    impl State<'_> {
        fn add(&mut self, n: i64) -> Result<(), Error> {
            if n < 0 || n > self.limits.max_input_bytes.saturating_sub(self.bytes) {
                return Err(Error::limit("input_bytes"));
            }
            self.bytes = self.bytes.saturating_add(n);
            Ok(())
        }
        fn elements(&mut self, elements: &[Element], depth: i64) -> Result<(), Error> {
            if depth > self.limits.max_depth
                || elements.len() as i64 > self.limits.max_elements.saturating_sub(self.elements)
            {
                return Err(Error::limit("elements"));
            }
            self.elements = self.elements.saturating_add(elements.len() as i64);
            for e in elements {
                match &e.value {
                    Value::Nil => {
                        return Err(Error::unsupported("deferred value is unavailable"));
                    }
                    Value::Raw(raw) => self.add(raw.len() as i64)?,
                    Value::Strings(values) => {
                        if values.len() as i64
                            > self.limits.max_elements.saturating_sub(self.elements)
                        {
                            return Err(Error::limit("elements"));
                        }
                        self.elements = self.elements.saturating_add(values.len() as i64);
                        self.add(strings_len(values))?;
                    }
                    Value::Sequence(items) => {
                        if items.len() as i64
                            > self.limits.max_elements.saturating_sub(self.elements)
                        {
                            return Err(Error::limit("elements"));
                        }
                        self.elements = self.elements.saturating_add(items.len() as i64);
                        for item in items {
                            self.elements(&item.elements, depth.saturating_add(1))?;
                        }
                    }
                    Value::Fragments {
                        offset_table,
                        fragments,
                    } => {
                        if fragments.len() as i64
                            > self.limits.max_fragments.saturating_sub(self.fragments)
                        {
                            return Err(Error::limit("fragments"));
                        }
                        self.fragments = self.fragments.saturating_add(fragments.len() as i64);
                        self.add(offset_table.len() as i64)?;
                        for f in fragments {
                            self.add(f.len() as i64)?;
                        }
                    }
                    Value::BulkData(uri) => self.add(uri.len() as i64)?,
                }
            }
            Ok(())
        }
    }
    let mut s = State {
        limits,
        elements: 0,
        fragments: 0,
        bytes: 0,
    };
    s.elements(obj.elements(), 1)?;
    Ok(object_from(obj.elements().to_vec(), obj.value_big_endian()))
}
pub(crate) fn equivalent_clone_limits(mut l: TranscodeLimits) -> TranscodeLimits {
    if l.max_output_bytes < l.max_input_bytes {
        l.max_input_bytes = l.max_output_bytes;
    }
    l
}
pub(crate) fn output_clone_limits(mut l: TranscodeLimits) -> TranscodeLimits {
    l.max_input_bytes = l.max_output_bytes;
    l
}
pub(super) fn input_limits(l: &TranscodeLimits) -> TranscodeLimits {
    let mut i = *l;
    i.max_output_bytes = l.max_input_bytes;
    i
}
pub(crate) fn has_nested_pixel_data(obj: &Object, limits: &TranscodeLimits) -> Result<bool, Error> {
    let mut stack: Vec<(&[Element], i64, bool)> = vec![(obj.elements(), 1, false)];
    let mut seen = 0i64;
    while let Some((elements, depth, nested)) = stack.pop() {
        if depth > limits.max_depth
            || elements.len() as i64 > limits.max_elements.saturating_sub(seen)
        {
            return Err(Error::limit("elements"));
        }
        seen = seen.saturating_add(elements.len() as i64);
        for e in elements {
            if nested
                && matches!(
                    e.tag(),
                    TAG_PIXEL_DATA | TAG_FLOAT_PIXEL_DATA | TAG_DOUBLE_FLOAT_PIXEL_DATA
                )
            {
                return Ok(true);
            }
            let Value::Sequence(items) = &e.value else {
                continue;
            };
            if items.len() as i64
                > limits
                    .max_elements
                    .saturating_sub(seen)
                    .saturating_sub(stack.len() as i64)
            {
                return Err(Error::limit("elements"));
            }
            for item in items.iter().rev() {
                stack.push((&item.elements, depth.saturating_add(1), true));
            }
        }
    }
    Ok(false)
}
fn validate_endian_conversion(obj: &Object, limits: &TranscodeLimits) -> Result<(), Error> {
    let mut stack: Vec<(&[Element], i64)> = vec![(obj.elements(), 1)];
    let mut seen = 0i64;
    while let Some((elements, depth)) = stack.pop() {
        if depth > limits.max_depth
            || elements.len() as i64 > limits.max_elements.saturating_sub(seen)
        {
            return Err(Error::limit("elements"));
        }
        seen = seen.saturating_add(elements.len() as i64);
        for e in elements {
            if e.vr() == Vr::UN && e.raw().is_some_and(|r| !r.is_empty()) {
                return Err(Error::unsupported("endian conversion of VR UN"));
            }
            let Value::Sequence(items) = &e.value else {
                continue;
            };
            if items.len() as i64
                > limits
                    .max_elements
                    .saturating_sub(seen)
                    .saturating_sub(stack.len() as i64)
            {
                return Err(Error::limit("elements"));
            }
            for item in items.iter().rev() {
                stack.push((&item.elements, depth.saturating_add(1)));
            }
        }
    }
    Ok(())
}
pub(super) fn validate_metadata_bounds(
    m: &Metadata,
    limits: &TranscodeLimits,
) -> Result<(), Error> {
    if m.number_of_frames <= 0 || m.number_of_frames > limits.max_frames {
        return Err(Error::limit("frames"));
    }
    let pixels = u64::from(m.rows).saturating_mul(u64::from(m.columns));
    if pixels > 0 && m.number_of_frames as u64 > u64::MAX.checked_div(pixels).unwrap_or(u64::MAX) {
        return Err(Error::limit("pixels"));
    }
    if pixels.saturating_mul(m.number_of_frames as u64) > limits.max_pixels {
        return Err(Error::limit("pixels"));
    }
    Ok(())
}
fn validate_native_input(
    obj: &Object,
    source: &Syntax,
    limits: &TranscodeLimits,
) -> Result<(Option<Metadata>, i64), Error> {
    let integer = obj.get(TAG_PIXEL_DATA);
    let float = obj.get(TAG_FLOAT_PIXEL_DATA);
    let double = obj.get(TAG_DOUBLE_FLOAT_PIXEL_DATA);
    let present = [integer.is_some(), float.is_some(), double.is_some()]
        .iter()
        .filter(|b| **b)
        .count();
    if present == 0 {
        return Ok((None, 0));
    }
    if present != 1 {
        return Err(Error::unsupported("multiple Pixel Data representations"));
    }
    if let Some(e) = integer {
        let pixel = pixel_view(e)?;
        if matches!(pixel, PixelData::Encapsulated { .. }) != source.encapsulated {
            return Err(Error::pixel(
                PixelErrorKind::IncompatiblePixelData,
                "transfer syntax and Pixel Data representation differ",
            ));
        }
        let bytes = payload_bytes(&pixel);
        if bytes > limits.max_input_bytes {
            return Err(Error::limit("input_bytes"));
        }
        let Ok(m) = extract_metadata(obj) else {
            return Ok((None, bytes));
        };
        validate_metadata_bounds(&m, limits)?;
        return Ok((Some(m), bytes));
    }
    let Some(e) = double.or(float) else {
        return Ok((None, 0));
    };
    let raw = match &e.value {
        Value::Raw(raw) if !source.encapsulated => raw,
        _ => {
            return Err(Error::pixel(
                PixelErrorKind::IncompatiblePixelData,
                "Float Pixel Data must use a native value",
            ));
        }
    };
    if raw.len() as i64 > limits.max_input_bytes {
        return Err(Error::limit("input_bytes"));
    }
    let Ok(m) = extract_metadata(obj) else {
        return Ok((None, raw.len() as i64));
    };
    validate_metadata_bounds(&m, limits)?;
    Ok((Some(m), raw.len() as i64))
}
pub(crate) fn strings_len(values: &[String]) -> i64 {
    let mut total: i64 = values.iter().map(|v| v.len() as i64).sum();
    if !values.is_empty() {
        total = total.saturating_add((values.len() as i64).saturating_sub(1));
    }
    total.saturating_add(total % 2)
}
fn validate_pixel_data<'a>(
    obj: &'a Object,
    source: &Syntax,
    limits: &TranscodeLimits,
) -> Result<(Metadata, PixelData<'a>), Error> {
    let integer = obj.get(TAG_PIXEL_DATA);
    let present = [
        integer.is_some(),
        obj.has(TAG_FLOAT_PIXEL_DATA),
        obj.has(TAG_DOUBLE_FLOAT_PIXEL_DATA),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    let Some(integer) = integer.filter(|_| present == 1) else {
        return Err(Error::unsupported(
            "integer Pixel Data is required and Float/Double Float Pixel Data are unsupported",
        ));
    };
    let pixel = pixel_view(integer)?;
    if source.encapsulated != matches!(pixel, PixelData::Encapsulated { .. }) {
        return Err(Error::pixel(
            PixelErrorKind::IncompatiblePixelData,
            "transfer syntax and Pixel Data representation differ",
        ));
    }
    let m = extract_metadata(obj)?;
    if m.number_of_frames <= 0 || m.number_of_frames > limits.max_frames {
        return Err(Error::limit("frames"));
    }
    let pixels = u64::from(m.rows).saturating_mul(u64::from(m.columns));
    if pixels > 0 && m.number_of_frames as u64 > u64::MAX.checked_div(pixels).unwrap_or(u64::MAX) {
        return Err(Error::limit("pixels"));
    }
    if pixels.saturating_mul(m.number_of_frames as u64) > limits.max_pixels {
        return Err(Error::limit("pixels"));
    }
    let fragment_count = match &pixel {
        PixelData::Encapsulated { fragments, .. } => fragments.len() as i64,
        PixelData::Native(_) => 0,
    };
    if fragment_count > limits.max_fragments || payload_bytes(&pixel) > limits.max_input_bytes {
        return Err(Error::limit("input_bytes"));
    }
    Ok((m, pixel))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    NativeClone,
    EquivalentClone,
    DecodeNative,
    DecodeEncapsulatedUncompressed,
    DecodeEncode,
    MissingPixelData,
}
pub(super) fn build_plan(source: &Syntax, target: &Syntax, force: bool, has_pixel: bool) -> Mode {
    if !source.encapsulated && !target.encapsulated {
        Mode::NativeClone
    } else if !has_pixel {
        Mode::MissingPixelData
    } else if source.uid == target.uid && !force {
        Mode::EquivalentClone
    } else if target.uid == UID_ENCAPSULATED_UNCOMPRESSED {
        Mode::DecodeEncapsulatedUncompressed
    } else if matches!(
        target.uid.as_ref(),
        UID_IMPLICIT_LE | UID_EXPLICIT_LE | UID_DEFLATED_LE | UID_EXPLICIT_BE
    ) {
        Mode::DecodeNative
    } else {
        Mode::DecodeEncode
    }
}
pub fn transcode_file(
    file: &File,
    target: &str,
    options: &TranscodeOptions,
) -> Result<(File, TranscodeReport), Error> {
    transcode_file_with_recovery(file, target, options, &DecodeRecovery::default())
}
#[doc(hidden)]
pub fn transcode_file_with_recovery(
    file: &File,
    target: &str,
    options: &TranscodeOptions,
    recovery: &DecodeRecovery,
) -> Result<(File, TranscodeReport), Error> {
    let limits = resolve_limits(options.limits)?;
    if !file.preamble.is_empty() && file.preamble.len() != 128 {
        return Err(Error::unsupported("invalid Part 10 preamble"));
    }
    let il = input_limits(&limits);
    let usage = measure(&file.dataset, &il)?;
    if file.preamble.len() as i64 > limits.max_input_bytes.saturating_sub(usage.bytes) {
        return Err(Error::limit("input_bytes"));
    }
    let used = usage.bytes.saturating_add(file.preamble.len() as i64);
    let mut meta_limits = il;
    meta_limits.max_output_bytes = limits.max_input_bytes.saturating_sub(used);
    meta_limits.max_elements = limits.max_elements.saturating_sub(usage.elements);
    measure(&file.meta, &meta_limits)?;
    let source = file_source_syntax(file)?;
    let (dataset, report) =
        transcode_data_set_with_recovery(&file.dataset, &source.uid, target, options, recovery)?;
    let output = output_file(file, dataset, &report, target, &limits)?;
    Ok((output, report))
}
pub(super) fn output_file(
    file: &File,
    dataset: Object,
    report: &TranscodeReport,
    target: &str,
    limits: &TranscodeLimits,
) -> Result<File, Error> {
    let target = canonical_target(target)?;
    let mut elements = file.meta.elements().to_vec();
    replace_element(
        &mut elements,
        strings_element(
            TAG_META_TRANSFER_SYNTAX,
            Vr::UI,
            vec![target.uid.to_string()],
        ),
    );
    let mut meta = Object::from_elements(elements);
    if (report.lossy || report.identity_changed)
        && let Some(uid) = dataset.get_string(TAG_SOP_INSTANCE_UID)
    {
        meta.put(strings_element(
            TAG_MEDIA_STORAGE_SOP_INSTANCE,
            Vr::UI,
            vec![uid],
        ));
    }
    if report.identity_changed {
        let class = dataset.get_string(TAG_SOP_CLASS_UID).unwrap_or_default();
        if !class.is_empty() {
            meta.put(strings_element(
                TAG_MEDIA_STORAGE_SOP_CLASS,
                Vr::UI,
                vec![class],
            ));
        }
    }
    let usage = measure(&dataset, limits)?;
    if file.preamble.len() as i64 > limits.max_output_bytes.saturating_sub(usage.bytes) {
        return Err(Error::limit("output_bytes"));
    }
    let used = usage.bytes.saturating_add(file.preamble.len() as i64);
    let mut meta_limits = *limits;
    meta_limits.max_input_bytes = limits.max_output_bytes.saturating_sub(used);
    meta_limits.max_elements = limits.max_elements.saturating_sub(usage.elements);
    let meta = clone_detached(&meta, &meta_limits)?;
    let syntax = crate::object::transfer::lookup(&target.uid)
        .unwrap_or(crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN);
    Ok(File {
        preamble: file.preamble.clone(),
        meta,
        dataset,
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    })
}
pub fn transcode_data_set(
    dataset: &Object,
    source: &str,
    target: &str,
    options: &TranscodeOptions,
) -> Result<(Object, TranscodeReport), Error> {
    transcode_data_set_with_recovery(dataset, source, target, options, &DecodeRecovery::default())
}
pub(crate) fn transcode_data_set_with_recovery(
    dataset: &Object,
    source: &str,
    target: &str,
    options: &TranscodeOptions,
    recovery: &DecodeRecovery,
) -> Result<(Object, TranscodeReport), Error> {
    let resolved = verify::resolve_verification_options(options.clone())?;
    let options = &resolved;
    if options.secondary_capture {
        let limits = resolve_limits(options.limits)?;
        measure(dataset, &input_limits(&limits))?;
    }
    output::validate_output_options(dataset, target, options)?;
    let (mut result, mut report) =
        output::transcode_with_icons_recovering(dataset, source, target, options, recovery)?;
    output::apply_materialized_output_options(dataset, &mut result, target, options, &mut report)?;
    Ok((result, report))
}
pub(crate) fn transcode_top_recovering(
    dataset: &Object,
    source: &str,
    target: &str,
    options: &TranscodeOptions,
    recovery: &DecodeRecovery,
) -> Result<(Object, TranscodeReport), Error> {
    let mut recovery = *recovery;
    let declared = transfer::default_registry().get(transfer::normalize_uid(source));
    if recovery.defined_length_fragments {
        recovery.defined_length_fragments = false;
        if let Some(declared) = &declared
            && let Some(split) = decompress::defined_length_fragments(dataset, declared)
        {
            let (output, mut report) =
                transcode_top_recovering(&split, source, target, options, &recovery)?;
            report
                .decode_diagnostics
                .insert(0, DIAGNOSTIC_DEFINED_LENGTH_FRAGMENTS.into());
            return Ok((output, report));
        }
    }
    if recovery.native_pixel_data_under_encapsulated {
        recovery.native_pixel_data_under_encapsulated = false;
        if let Some(declared) = &declared
            && decompress::native_under_encapsulated(dataset, declared)
        {
            let (output, mut report) =
                transcode_top_recovering(dataset, UID_EXPLICIT_LE, target, options, &recovery)?;
            report
                .decode_diagnostics
                .insert(0, DIAGNOSTIC_NATIVE_UNDER_ENCAPSULATED.into());
            return Ok((output, report));
        }
    }
    if let Some(rewrite) = options.rewrite {
        let mut inner = options.clone();
        inner.rewrite = None;
        let (native, decode_report) =
            transcode_top_recovering(dataset, source, UID_EXPLICIT_LE, &inner, &recovery)?;
        let (rewritten, rewrite_report) = crate::pixel::native_rewrite::rewrite_native(
            &crate::net::ul::Ctx::background(),
            &native,
            &rewrite,
        )
        .map_err(|e| Error::stage("rewrite", rewrite_error(e)))?;
        let (output, mut report) = transcode_top_recovering(
            &rewritten,
            UID_EXPLICIT_LE,
            target,
            &inner,
            &DecodeRecovery::default(),
        )?;
        report.rewrite = Some(rewrite_report);
        let mut diagnostics = decode_report.decode_diagnostics;
        diagnostics.append(&mut report.decode_diagnostics);
        report.decode_diagnostics = diagnostics;
        return Ok((output, report));
    }
    let limits = resolve_limits(options.limits)?;
    measure(dataset, &input_limits(&limits))?;
    let source = canonical_source(source)?;
    let target = canonical_target(target)?;
    let legacy_repair = recovery.requires_legacy_pixel_repair();
    let force = options.force_reencode || legacy_repair;
    let has_pixels = has_any_pixel_data(dataset);
    let mut mode = build_plan(&source, &target, force, has_pixels);
    if legacy_repair && has_pixels && mode == Mode::NativeClone {
        mode = Mode::DecodeNative;
    }
    if (source.uid != target.uid || force)
        && (source.encapsulated || target.encapsulated)
        && has_nested_pixel_data(dataset, &limits)?
    {
        return Err(Error::unsupported("nested Pixel Data transcode"));
    }
    match mode {
        Mode::NativeClone => {
            if source.big_endian != target.big_endian {
                validate_endian_conversion(dataset, &limits)?;
            }
            let (metadata, pixel_bytes) = validate_native_input(dataset, &source, &limits)?;
            return clone_native(dataset, &source, &target, metadata, pixel_bytes, &limits);
        }
        Mode::MissingPixelData => return Err(Error::pixel_bare(PixelErrorKind::PixelDataNotFound)),
        _ => {}
    }
    let (metadata, pixel) = validate_pixel_data(dataset, &source, &limits)?;
    if mode != Mode::EquivalentClone && metadata.bits_allocated < 8 {
        return Err(Error::unsupported(
            "per-frame transcode with BitsAllocated below 8",
        ));
    }
    if mode == Mode::EquivalentClone {
        let clone = clone_detached(dataset, &equivalent_clone_limits(limits))?;
        return Ok((
            clone,
            TranscodeReport {
                frames: metadata.number_of_frames,
                native_bytes: total_size(&metadata),
                encoded_bytes: payload_bytes(&pixel),
                pixel_data_preserved: true,
                ..TranscodeReport::default()
            },
        ));
    }
    if let Some(transcoder) =
        uses_codestream_transcoder(mode, &source, &target, options, legacy_repair)
    {
        return transcode_codestreams(
            dataset, &metadata, &pixel, &source, &target, transcoder, &limits,
        );
    }
    if source.encapsulated
        && source.uid != UID_ENCAPSULATED_UNCOMPRESSED
        && options.decoder.is_none()
    {
        return Err(decompress::codec_registry_nil(&source.uid));
    }
    DecoderRegistry::refuse(options.decoder, &source.uid)?;
    let (decompressed, decode_diagnostics) = if recovery == DecodeRecovery::default() {
        decompress::decompress_for_transcode(
            dataset,
            &source,
            options.decoder,
            decompress::transcode_decompress_limits(&limits),
        )?
    } else {
        decompress::decompress_for_transcode_recovering(
            dataset,
            &source,
            options.decoder,
            decompress::transcode_decompress_limits(&limits),
            &recovery,
        )?
    };
    let metadata = extract_metadata(&decompressed)?;
    let native_bytes = total_size(&metadata);
    if native_bytes <= 0 || native_bytes > limits.max_output_bytes {
        return Err(Error::limit("native_bytes"));
    }
    let report = TranscodeReport {
        frames: metadata.number_of_frames,
        native_bytes,
        decode_diagnostics,
        ..TranscodeReport::default()
    };
    match mode {
        Mode::DecodeNative => {
            if target.uid == UID_EXPLICIT_BE {
                let mut elements = decompressed.elements().to_vec();
                swap_elements(&mut elements, false)?;
                let result = object_from(elements, true);
                let result = clone_detached(&result, &output_clone_limits(limits))?;
                return Ok((result, report));
            }
            let mut result = clone_detached(&decompressed, &output_clone_limits(limits))?;
            result.set_value_big_endian(false);
            Ok((result, report))
        }
        Mode::DecodeEncapsulatedUncompressed => {
            let frames = super::pixeldata::extract_native_frames(&decompressed)?;
            encode::encapsulate_frames(&decompressed, &metadata, frames.data, &limits, report)
        }
        _ => {
            let (moved, extracted) =
                output::move_embedded_overlays(&decompressed, options.extract_embedded_overlays)?;
            let mut report = report;
            report.embedded_overlays_extracted = extracted;
            encode::transcode_with_encoder(&moved, &metadata, &target, options, &limits, report)
        }
    }
}
fn rewrite_error(e: crate::pixel::native_rewrite::RewriteError) -> Error {
    use crate::pixel::native_rewrite::RewriteError;
    match e {
        RewriteError::Pixel(p) => Error::from(p),
        RewriteError::Unsupported(_) => Error::new(ErrorKind::NativeRewrite, e.to_string()),
        RewriteError::Other(_) | RewriteError::Context(_) => {
            Error::new(ErrorKind::Other, e.to_string())
        }
    }
}
fn clone_native(
    dataset: &Object,
    source: &Syntax,
    target: &Syntax,
    metadata: Option<Metadata>,
    pixel_bytes: i64,
    limits: &TranscodeLimits,
) -> Result<(Object, TranscodeReport), Error> {
    let mut clone = clone_detached(dataset, &equivalent_clone_limits(*limits))?;
    if source.big_endian != target.big_endian {
        let mut elements = clone.elements().to_vec();
        swap_elements(&mut elements, source.big_endian)?;
        clone = Object::from_elements(elements);
    }
    clone.set_value_big_endian(target.big_endian);
    let mut report = TranscodeReport {
        pixel_data_preserved: true,
        encoded_bytes: pixel_bytes,
        ..TranscodeReport::default()
    };
    if let Some(m) = metadata {
        report.frames = m.number_of_frames;
        report.native_bytes = total_size(&m);
    }
    Ok((clone, report))
}
pub(crate) fn swap_elements(
    elements: &mut [Element],
    source_big_endian: bool,
) -> Result<(), Error> {
    let width = pixel_swap_width(elements, source_big_endian)?;
    for e in elements.iter_mut() {
        swap_value(e, width, source_big_endian)?;
    }
    Ok(())
}
fn swap_value(e: &mut Element, pixel_width: usize, source_big_endian: bool) -> Result<(), Error> {
    let tag = e.tag();
    let vr = e.vr();
    match &mut e.value {
        Value::Raw(raw) => {
            let mut width = vr.byte_order_width();
            if tag == TAG_PIXEL_DATA && vr == Vr::OW && pixel_width > width {
                width = pixel_width;
            }
            if width <= 1 {
                return Ok(());
            }
            if !raw.len().is_multiple_of(width) {
                return Err(Error::pixel(
                    PixelErrorKind::PixelDataSizeMismatch,
                    format_args!(
                        "{tag} {} value has {} bytes, not a multiple of {width}-byte values",
                        vr.as_str(),
                        raw.len()
                    ),
                ));
            }
            *raw = crate::valueencode::reverse_fixed_width(raw, width);
            Ok(())
        }
        Value::Sequence(items) => {
            for item in items.iter_mut() {
                let width = pixel_swap_width(&item.elements, source_big_endian)?;
                for e in item.elements.iter_mut() {
                    swap_value(e, width, source_big_endian)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
fn pixel_swap_width(elements: &[Element], big_endian: bool) -> Result<usize, Error> {
    for e in elements {
        if e.tag() != TAG_BITS_ALLOCATED {
            continue;
        }
        let Value::Raw(raw) = &e.value else {
            return Ok(0);
        };
        if raw.len() != 2 {
            return Err(Error::pixel(
                PixelErrorKind::InvalidMetadata,
                format_args!("BitsAllocated has {} bytes", raw.len()),
            ));
        }
        let &[b0, b1] = raw.as_slice() else {
            return Err(Error::pixel(
                PixelErrorKind::InvalidMetadata,
                format_args!("BitsAllocated has {} bytes", raw.len()),
            ));
        };
        let pair = [b0, b1];
        let bits = if big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        };
        return Ok(match bits {
            32 => 4,
            64 => 8,
            _ => 0,
        });
    }
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits_resolve_defaults_and_refuse_negative() {
        assert_eq!(
            resolve_limits(TranscodeLimits::default()),
            Ok(default_limits())
        );
        let bad = TranscodeLimits {
            max_frames: -1,
            ..TranscodeLimits::default()
        };
        assert_eq!(
            resolve_limits(bad).unwrap_err().to_string(),
            "dicom: transcode resource limit exceeded: invalid limits"
        );
    }
    #[test]
    fn plan_modes() {
        let get = |u: &str| registry_get(u).unwrap_or_default();
        let ele = get(UID_EXPLICIT_LE);
        let rle = get("1.2.840.10008.1.2.5");
        assert_eq!(build_plan(&ele, &ele, false, true), Mode::NativeClone);
        assert_eq!(build_plan(&rle, &rle, false, true), Mode::EquivalentClone);
        assert_eq!(build_plan(&rle, &ele, false, true), Mode::DecodeNative);
        assert_eq!(build_plan(&ele, &rle, false, false), Mode::MissingPixelData);
        assert_eq!(build_plan(&ele, &rle, false, true), Mode::DecodeEncode);
    }
}
