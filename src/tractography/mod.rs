#![forbid(unsafe_code)]
mod budget;
mod codec;
pub mod geom;
mod read;
mod urlrules;
mod validation;
mod write;
use crate::derivation::DerivedCommon;
use crate::object::Object;
pub use crate::spatialreg::Context;
pub use read::read;
pub use validation::validate;
pub use write::write;
pub const STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.6";
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    InvalidOptions,
    InvalidData,
    InvalidReference,
    UnsupportedValue,
    LimitExceeded,
    GeomInvalidOptions,
    GeomInvalidData,
    GeomLimitExceeded,
    Canceled,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::InvalidOptions => "dicom/tractography: invalid options",
            Kind::InvalidData => "dicom/tractography: invalid data",
            Kind::InvalidReference => "dicom/tractography: invalid reference",
            Kind::UnsupportedValue => "dicom/tractography: materialized value required",
            Kind::LimitExceeded => "dicom/tractography: limit exceeded",
            Kind::GeomInvalidOptions => "tractographygeom: invalid options",
            Kind::GeomInvalidData => "tractographygeom: invalid geometry or scalar data",
            Kind::GeomLimitExceeded => "tractographygeom: display limit exceeded",
            Kind::Canceled => "context canceled",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
}
impl Error {
    pub fn sentinel(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
        }
    }
    pub fn new(kind: Kind, detail: impl AsRef<str>) -> Error {
        Error {
            kind,
            text: format!("{}: {}", kind.text(), detail.as_ref()),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub(crate) type Result<T> = std::result::Result<T, Error>;
pub(crate) fn invalid(path: impl AsRef<str>) -> Error {
    Error::new(Kind::InvalidData, path)
}
pub(crate) fn limited(path: &str) -> Error {
    Error::new(Kind::LimitExceeded, path)
}
pub(crate) fn unsupported(path: &str) -> Error {
    Error::new(Kind::UnsupportedValue, path)
}
pub(crate) fn invalid_reference() -> Error {
    Error::sentinel(Kind::InvalidReference)
}
pub(crate) fn check(ctx: &Context) -> Result<()> {
    if ctx.is_canceled() {
        Err(Error::sentinel(Kind::Canceled))
    } else {
        Ok(())
    }
}
pub type SourceOrigin = u8;
pub const UNKNOWN_ORIGIN: SourceOrigin = 0;
pub const DICOM_IMAGES: SourceOrigin = 1;
pub const OTHER_SOURCE: SourceOrigin = 2;
pub type CodeKind = u8;
pub const SHORT_CODE: CodeKind = 0;
pub const LONG_CODE: CodeKind = 1;
pub const URN_CODE: CodeKind = 2;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodeContext {
    pub identifier: String,
    pub uid: String,
    pub mapping_resource: String,
    pub mapping_resource_uid: String,
    pub mapping_resource_name: String,
    pub version: String,
    pub extension_flag: String,
    pub local_version: String,
    pub extension_creator_uid: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
    pub kind: CodeKind,
    pub scheme_version: String,
    pub context: Option<CodeContext>,
    pub equivalent: Option<Vec<Code>>,
}
impl Code {
    pub fn from_code(value: &str, scheme: &str, meaning: &str) -> Code {
        Code {
            value: value.to_string(),
            scheme: scheme.to_string(),
            meaning: meaning.to_string(),
            ..Code::default()
        }
    }
    pub(crate) fn same_code(&self, other: &Code) -> bool {
        self.value == other.value && self.scheme == other.scheme && self.meaning == other.meaning
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Algorithm {
    pub family: Code,
    pub name_code: Option<Code>,
    pub name: String,
    pub version: String,
    pub parameters: String,
    pub source: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub points: Vec<[f32; 3]>,
    pub color: Option<[u16; 3]>,
    pub point_colors: Option<Vec<[u16; 3]>>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasurementValues {
    pub values: Vec<f32>,
    pub point_indices: Option<Vec<u32>>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measurement {
    pub quantity: Code,
    pub units: Code,
    pub tracks: Vec<MeasurementValues>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackStatistic {
    pub quantity: Code,
    pub modifier: Code,
    pub units: Code,
    pub values: Vec<f32>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackSetStatistic {
    pub quantity: Code,
    pub modifier: Code,
    pub units: Code,
    pub value: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackSet {
    pub number: u32,
    pub label: String,
    pub description: String,
    pub anatomy: Code,
    pub anatomy_modifiers: Option<Vec<Code>>,
    pub color: Option<[u16; 3]>,
    pub line_thickness: Option<f32>,
    pub tracks: Vec<Track>,
    pub measurements: Option<Vec<Measurement>>,
    pub track_statistics: Option<Vec<TrackStatistic>>,
    pub statistics: Option<Vec<TrackSetStatistic>>,
    pub acquisition: Option<Code>,
    pub model: Code,
    pub algorithms: Vec<Algorithm>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SopReference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImageReference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frames: Option<Vec<i32>>,
    pub segments: Option<Vec<u16>>,
}
#[derive(Clone, Debug, Default)]
pub struct Results {
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub series_number: i32,
    pub instance_number: i32,
    pub content_date: String,
    pub content_time: String,
    pub label: String,
    pub description: String,
    pub origin: SourceOrigin,
    pub track_sets: Vec<TrackSet>,
    pub references: Vec<ImageReference>,
    pub performed_procedure_step: Option<SopReference>,
    pub metadata: Option<Object>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: i64,
    pub max_value_bytes: i64,
    pub max_text_bytes: i64,
    pub max_track_sets: i64,
    pub max_tracks: i64,
    pub max_points: i64,
    pub max_values: i64,
    pub max_references: i64,
    pub max_elements: i64,
    pub max_items: i64,
    pub max_sequence_depth: i64,
}
impl Limits {
    pub fn defaults() -> Limits {
        Limits {
            max_bytes: 512 << 20,
            max_value_bytes: 128 << 20,
            max_text_bytes: 8 << 20,
            max_track_sets: 1024,
            max_tracks: 100_000,
            max_points: 8_000_000,
            max_values: 16_000_000,
            max_references: 100_000,
            max_elements: 100_000,
            max_items: 100_000,
            max_sequence_depth: 16,
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ReadOptions {
    pub limits: Limits,
    pub require_dicom_references: bool,
}
#[derive(Clone, Debug, Default)]
pub struct WriteOptions {
    pub limits: Limits,
    pub common: DerivedCommon,
}
