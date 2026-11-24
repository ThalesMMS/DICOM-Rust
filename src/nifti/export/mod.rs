mod chunks;
mod plan;
mod source;
mod write;
use super::sidecar::Sidecar;
use crate::render::{GeometryAffine, GeometryIssue, GeometryTolerances};
pub use chunks::Chunks;
pub(crate) use plan::strict_slice_geometry;
pub use plan::{Plan, PlanLayout, plan_volume, write, write_files};
pub use source::{FilesSource, OpenedFile, PathSource, Source, SourceError, SourceReadOptions};
use std::fmt;
pub(crate) use write::spool_probe;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExportSentinel {
    InvalidSource,
    InvalidGeometry,
    MixedIdentity,
    UnsupportedPixels,
    UnsupportedScaling,
    UnsupportedTemporal,
    LimitExceeded,
    NIfTI2Required,
}
impl ExportSentinel {
    pub fn text(self) -> &'static str {
        match self {
            ExportSentinel::InvalidSource => "dicom/nifti: invalid source",
            ExportSentinel::InvalidGeometry => "dicom/nifti: invalid or unsupported geometry",
            ExportSentinel::MixedIdentity => "dicom/nifti: mixed series or frame of reference",
            ExportSentinel::UnsupportedPixels => "dicom/nifti: unsupported pixel representation",
            ExportSentinel::UnsupportedScaling => "dicom/nifti: unsupported scaling",
            ExportSentinel::UnsupportedTemporal => "dicom/nifti: unsupported temporal dimension",
            ExportSentinel::LimitExceeded => "dicom/nifti: resource limit exceeded",
            ExportSentinel::NIfTI2Required => "dicom/nifti: NIfTI-2 required",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Source,
    Identity,
    Geometry,
    Temporal,
    Pixels,
    Scaling,
    Limit,
    NIfTI2,
    Write,
    Canceled,
    Quadruped,
    MultipleStacks,
    IrregularTime,
    MissingTiming,
    SingleSlice,
    UnsupportedModality,
}
impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Source => "source",
            ErrorCode::Identity => "identity",
            ErrorCode::Geometry => "geometry",
            ErrorCode::Temporal => "temporal",
            ErrorCode::Pixels => "pixels",
            ErrorCode::Scaling => "scaling",
            ErrorCode::Limit => "limit",
            ErrorCode::NIfTI2 => "nifti2-required",
            ErrorCode::Write => "write",
            ErrorCode::Canceled => "canceled",
            ErrorCode::Quadruped => "quadruped-coordinate-system",
            ErrorCode::MultipleStacks => "multiple-stacks",
            ErrorCode::IrregularTime => "irregular-time",
            ErrorCode::MissingTiming => "missing-time-spacing",
            ErrorCode::SingleSlice => "single-slice",
            ErrorCode::UnsupportedModality => "unsupported-modality-transform",
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ExportError {
    pub code: ErrorCode,
    pub source_index: i64,
    pub frame_index: i64,
    pub geometry_issue: GeometryIssue,
    pub sentinels: Vec<ExportSentinel>,
    pub cause: Option<String>,
}
impl ExportError {
    pub fn is(&self, sentinel: ExportSentinel) -> bool {
        self.sentinels.contains(&sentinel)
    }
}
impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut location = String::new();
        if self.source_index >= 0 {
            location = format!(" at source {}", self.source_index + 1);
            if self.frame_index >= 0 {
                location += &format!(" frame {}", self.frame_index + 1);
            }
        }
        let mut code = self.code.as_str().to_string();
        if self.geometry_issue != GeometryIssue::None {
            code += &format!(" ({})", self.geometry_issue.as_str());
        }
        write!(f, "dicom/nifti: {code}{location}")
    }
}
impl std::error::Error for ExportError {}
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Export(ExportError),
    Header(super::Error),
}
impl Error {
    pub fn is(&self, sentinel: ExportSentinel) -> bool {
        match self {
            Error::Export(e) => e.is(sentinel),
            Error::Header(_) => false,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Export(e) => e.fmt(f),
            Error::Header(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Default)]
pub(crate) struct Cause {
    pub sentinels: Vec<ExportSentinel>,
    pub text: Option<String>,
}
impl Cause {
    pub fn text(text: impl Into<String>) -> Cause {
        Cause {
            sentinels: Vec::new(),
            text: Some(text.into()),
        }
    }
    pub fn sentinel(sentinel: ExportSentinel) -> Cause {
        Cause {
            sentinels: vec![sentinel],
            text: None,
        }
    }
}
pub(crate) fn export_error(
    code: ErrorCode,
    source_index: i64,
    frame_index: i64,
    sentinel: Option<ExportSentinel>,
    cause: Option<Cause>,
) -> Error {
    let mut sentinels = Vec::new();
    let mut text = None;
    if let Some(s) = sentinel {
        sentinels.push(s);
    }
    if let Some(cause) = cause {
        for s in cause.sentinels {
            if !sentinels.contains(&s) {
                sentinels.push(s);
            }
        }
        text = cause.text;
    }
    Error::Export(ExportError {
        code,
        source_index,
        frame_index,
        geometry_issue: GeometryIssue::None,
        sentinels,
        cause: text,
    })
}
pub(crate) fn bare(
    code: ErrorCode,
    source_index: i64,
    frame_index: i64,
    sentinel: ExportSentinel,
) -> Error {
    export_error(code, source_index, frame_index, Some(sentinel), None)
}
pub const COMPRESSION_NONE: u8 = 0;
pub const COMPRESSION_GZIP: u8 = 1;
#[doc(hidden)]
pub fn compression_name(c: u8) -> &'static str {
    if c == COMPRESSION_GZIP {
        "gzip"
    } else {
        "none"
    }
}
pub const SCALING_PRESERVE_UNIFORM: u8 = 0;
pub const SCALING_APPLY_FLOAT32: u8 = 1;
pub const SCALING_APPLY_FLOAT64: u8 = 2;
#[doc(hidden)]
pub fn scaling_policy_name(p: u8) -> &'static str {
    match p {
        SCALING_APPLY_FLOAT32 => "apply-float32",
        SCALING_APPLY_FLOAT64 => "apply-float64",
        _ => "preserve-uniform",
    }
}
pub const GEOMETRY_STRICT: u8 = 0;
pub const GEOMETRY_RESAMPLE_LINEAR: u8 = 1;
pub const GEOMETRY_RESAMPLE_CUBIC_BSPLINE: u8 = 2;
pub const SINGLE_SLICE_REJECT: u8 = 0;
pub const SINGLE_SLICE_PATIENT: u8 = 1;
pub const SINGLE_SLICE_IMAGE_GRID: u8 = 2;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Limits {
    pub max_instances: i64,
    pub max_frames: i64,
    pub max_voxels: u64,
    pub max_uncompressed_bytes: u64,
    pub max_spool_bytes: u64,
    pub max_resample_voxels: u64,
    pub max_in_memory_source_bytes: u64,
}
pub fn default_limits() -> Limits {
    Limits {
        max_instances: 100_000,
        max_frames: 1_000_000,
        max_voxels: 1 << 32,
        max_uncompressed_bytes: 16 << 30,
        max_spool_bytes: 16 << 30,
        max_resample_voxels: 256 << 20,
        max_in_memory_source_bytes: 512 << 20,
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Options {
    pub compression: u8,
    pub scaling: u8,
    pub geometry: u8,
    pub temporal_spacing_seconds: f64,
    pub single_slice: u8,
    pub tolerances: GeometryTolerances,
    pub limits: Limits,
    pub temp_dir: String,
}
pub fn default_options() -> Options {
    Options {
        tolerances: super::strict_geometry_tolerances(),
        limits: default_limits(),
        ..Options::default()
    }
}
pub const WARNING_PARAMETRIC_PADDING_AS_NAN: &str = "parametric-map-padding-written-as-nan";
pub const WARNING_PARAMETRIC_NAN_PRESERVED: &str = "parametric-map-nan-preserved";
pub const WARNING_PARAMETRIC_INFINITE_PRESERVED: &str = "parametric-map-infinite-preserved";
pub const WARNING_PARAMETRIC_OUTSIDE_MAPPING_AS_NAN: &str =
    "parametric-map-outside-mapping-written-as-nan";
pub const WARNING_PARAMETRIC_MAPPING_OVERFLOW_AS_NAN: &str =
    "parametric-map-mapping-overflow-written-as-nan";
pub const WARNING_SLICE_SPACING_FROM_THICKNESS: &str = "slice-spacing-from-slice-thickness";
pub const WARNING_SLICE_SPACING_DEFAULT: &str = "slice-spacing-default-1mm";
pub const WARNING_NO_PATIENT_GEOMETRY: &str = "no-patient-geometry";
pub const WARNING_PIXEL_SPACING_DEFAULT: &str = "pixel-spacing-default-1mm";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnmeasuredVoxels {
    pub padding: i64,
    pub nan: i64,
    pub infinite: i64,
    pub outside_mapping: i64,
    pub mapping_overflow: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    pub dimensions: [i64; 4],
    pub datatype: i16,
    pub bit_pix: i16,
    pub voxel_offset: i64,
    pub index_to_ras: GeometryAffine,
    pub scaling_policy: u8,
    pub scaling_slope: f64,
    pub scaling_intercept: f64,
    pub compression: u8,
    pub input_reordered: bool,
    pub resampled: bool,
    pub interpolation: String,
    pub warnings: Option<Vec<String>>,
    pub bytes_written: i64,
    pub sidecar: Sidecar,
    pub unmeasured_voxels: UnmeasuredVoxels,
    pub single_slice: bool,
    pub slice_spacing_source: &'static str,
    pub no_spatial_transform: bool,
    pub pixel_spacing_semantic: &'static str,
}
pub(crate) fn normalized_options(mut options: Options) -> Result<Options, Error> {
    let defaults = default_options();
    if options.compression > COMPRESSION_GZIP {
        return Err(bare(
            ErrorCode::Write,
            -1,
            -1,
            ExportSentinel::InvalidSource,
        ));
    }
    if options.scaling > SCALING_APPLY_FLOAT64 {
        return Err(bare(
            ErrorCode::Scaling,
            -1,
            -1,
            ExportSentinel::UnsupportedScaling,
        ));
    }
    if options.geometry > GEOMETRY_RESAMPLE_CUBIC_BSPLINE
        || options.single_slice > SINGLE_SLICE_IMAGE_GRID
    {
        return Err(bare(
            ErrorCode::Geometry,
            -1,
            -1,
            ExportSentinel::InvalidGeometry,
        ));
    }
    if options.tolerances == GeometryTolerances::default() {
        options.tolerances = defaults.tolerances;
    }
    let l = &mut options.limits;
    let d = defaults.limits;
    if l.max_instances == 0 {
        l.max_instances = d.max_instances;
    }
    if l.max_frames == 0 {
        l.max_frames = d.max_frames;
    }
    if l.max_voxels == 0 {
        l.max_voxels = d.max_voxels;
    }
    if l.max_uncompressed_bytes == 0 {
        l.max_uncompressed_bytes = d.max_uncompressed_bytes;
    }
    if l.max_spool_bytes == 0 {
        l.max_spool_bytes = d.max_spool_bytes;
    }
    if l.max_resample_voxels == 0 {
        l.max_resample_voxels = d.max_resample_voxels;
    }
    if l.max_in_memory_source_bytes == 0 {
        l.max_in_memory_source_bytes = d.max_in_memory_source_bytes;
    }
    if l.max_instances < 0 || l.max_frames < 0 {
        return Err(bare(
            ErrorCode::Limit,
            -1,
            -1,
            ExportSentinel::LimitExceeded,
        ));
    }
    Ok(options)
}
pub(crate) fn datatype_name(datatype: i16) -> &'static str {
    use super::*;
    match datatype {
        DATATYPE_UINT8 => "uint8",
        DATATYPE_INT8 => "int8",
        DATATYPE_INT16 => "int16",
        DATATYPE_UINT16 => "uint16",
        DATATYPE_INT32 => "int32",
        DATATYPE_UINT32 => "uint32",
        DATATYPE_FLOAT32 => "float32",
        DATATYPE_FLOAT64 => "float64",
        _ => "unknown",
    }
}
pub(crate) fn checked_mul(a: u64, b: u64) -> Option<u64> {
    if a != 0 && b > u64::MAX / a {
        return None;
    }
    Some(a.wrapping_mul(b))
}
pub(crate) fn checked_add(a: u64, b: u64) -> Option<u64> {
    if b > u64::MAX - a {
        return None;
    }
    Some(a + b)
}
