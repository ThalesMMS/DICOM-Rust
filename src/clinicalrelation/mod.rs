#![forbid(unsafe_code)]
mod resolve;
use crate::{gsps, rtdose, rtstruct, sr};
pub use resolve::{resolve, resolve_dataset};
use std::fmt;
pub const SEGMENTATION_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.4";
pub const LABEL_MAP_SEGMENTATION_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.7";
pub const RT_DOSE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.2";
pub const PARAMETRIC_MAP_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.30";
pub const GRAYSCALE_PLANAR_MPR_VPS_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.11.6";
pub const COMPOSITING_PLANAR_MPR_VPS_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.11.7";
pub const VOLUME_RENDERING_VPS_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.11.9";
pub const SEGMENTED_VOLUME_RENDERING_VPS_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.11.10";
pub const MULTIPLE_VOLUME_RENDERING_VPS_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.11.11";
pub const SPATIAL_REGISTRATION_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.1";
pub const DEFORMABLE_SPATIAL_REGISTRATION_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.3";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    #[default]
    Image,
    Seg,
    SrMeasurement,
    Kos,
    Gsps,
    RtStruct,
    RtDose,
    ParametricMap,
    Vps,
    Registration,
    Other,
}
impl ObjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectKind::Image => "image",
            ObjectKind::Seg => "seg",
            ObjectKind::SrMeasurement => "sr_measurement",
            ObjectKind::Kos => "kos",
            ObjectKind::Gsps => "gsps",
            ObjectKind::RtStruct => "rtstruct",
            ObjectKind::RtDose => "rtdose",
            ObjectKind::ParametricMap => "parametric_map",
            ObjectKind::Vps => "vps",
            ObjectKind::Registration => "registration",
            ObjectKind::Other => "other",
        }
    }
}
impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReferenceRole {
    SourceImage,
    PresentationImage,
    KeyObject,
    ContourImage,
    RtObject,
    Segment,
    MeasurementImage,
    PresentationInput,
    RegistrationSource,
}
impl ReferenceRole {
    pub fn as_str(self) -> &'static str {
        match self {
            ReferenceRole::SourceImage => "source_image",
            ReferenceRole::PresentationImage => "presentation_image",
            ReferenceRole::KeyObject => "key_object",
            ReferenceRole::ContourImage => "contour_image",
            ReferenceRole::RtObject => "rt_object",
            ReferenceRole::Segment => "segment",
            ReferenceRole::MeasurementImage => "measurement_image",
            ReferenceRole::PresentationInput => "presentation_input",
            ReferenceRole::RegistrationSource => "registration_source",
        }
    }
}
impl fmt::Display for ReferenceRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub role: ReferenceRole,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub frames: Vec<i64>,
}
impl Reference {
    pub fn new(role: ReferenceRole) -> Reference {
        Reference {
            role,
            study_instance_uid: String::new(),
            series_instance_uid: String::new(),
            sop_class_uid: String::new(),
            sop_instance_uid: String::new(),
            frames: Vec::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    MissingStudyInstanceUid,
    InvalidStudyInstanceUid,
    MissingSeriesInstanceUid,
    InvalidSeriesInstanceUid,
    MissingSopClassUid,
    InvalidSopClassUid,
    MissingSopInstanceUid,
    InvalidSopInstanceUid,
    MalformedObject,
    NilDataset,
    ReaderNotPorted,
}
impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::MissingStudyInstanceUid => "missing_study_instance_uid",
            DiagnosticCode::InvalidStudyInstanceUid => "invalid_study_instance_uid",
            DiagnosticCode::MissingSeriesInstanceUid => "missing_series_instance_uid",
            DiagnosticCode::InvalidSeriesInstanceUid => "invalid_series_instance_uid",
            DiagnosticCode::MissingSopClassUid => "missing_sop_class_uid",
            DiagnosticCode::InvalidSopClassUid => "invalid_sop_class_uid",
            DiagnosticCode::MissingSopInstanceUid => "missing_sop_instance_uid",
            DiagnosticCode::InvalidSopInstanceUid => "invalid_sop_instance_uid",
            DiagnosticCode::MalformedObject => "malformed_object",
            DiagnosticCode::NilDataset => "nil_dataset",
            DiagnosticCode::ReaderNotPorted => "reader_not_ported",
        }
    }
}
impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub role: ReferenceRole,
    pub reference_index: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    pub kind: ObjectKind,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub source_frame_of_reference_uids: Vec<String>,
    pub registration_type: String,
    pub editable: bool,
    pub references: Vec<Reference>,
    pub diagnostics: Vec<Diagnostic>,
}
impl Resolution {
    pub fn referenced_sop_instance_uids(&self) -> Vec<String> {
        let mut out = Vec::new();
        for reference in &self.references {
            resolve::append_unique(&mut out, &reference.sop_instance_uid);
        }
        out
    }
    pub fn referenced_series_instance_uids(&self) -> Vec<String> {
        let mut out = Vec::new();
        for reference in &self.references {
            resolve::append_unique(&mut out, &reference.series_instance_uid);
        }
        out
    }
}
#[derive(Clone, Debug)]
pub struct SpatialRegError(pub crate::spatialreg::Error);
impl PartialEq for SpatialRegError {
    fn eq(&self, other: &SpatialRegError) -> bool {
        self.0.to_string() == other.0.to_string() && self.0.kinds() == other.0.kinds()
    }
}
impl Eq for SpatialRegError {}
impl fmt::Display for SpatialRegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for SpatialRegError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.0)
    }
}
pub const ERR_RESOLUTION: &str = "dicom/clinicalrelation: resolution failed";
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    Gsps(gsps::Error),
    Sr(sr::SrError),
    RtStruct(rtstruct::Error),
    RtDose(rtdose::Error),
    SpatialReg(SpatialRegError),
    Seg(crate::seg::Error),
    ParametricMap(crate::parametricmap::Error),
    Vps(crate::vps::Error),
    Message(String),
    NotPorted(&'static str),
}
impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Cause::Gsps(e) => e.fmt(f),
            Cause::Sr(e) => e.fmt(f),
            Cause::RtStruct(e) => e.fmt(f),
            Cause::RtDose(e) => e.fmt(f),
            Cause::SpatialReg(e) => e.fmt(f),
            Cause::Seg(e) => e.fmt(f),
            Cause::ParametricMap(e) => e.fmt(f),
            Cause::Vps(e) => e.fmt(f),
            Cause::Message(m) => f.write_str(m),
            Cause::NotPorted(reader) => {
                write!(f, "dicom-rust: {reader} is not ported")
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionError {
    pub code: DiagnosticCode,
    pub kind: ObjectKind,
    pub cause: Option<Cause>,
    pub resolution: Box<Resolution>,
}
impl ResolutionError {
    pub fn is_not_ported(&self) -> bool {
        self.code == DiagnosticCode::ReaderNotPorted
    }
}
impl fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cause {
            None => write!(f, "{ERR_RESOLUTION}: {} ({})", self.code, self.kind),
            Some(cause) => write!(
                f,
                "{ERR_RESOLUTION}: {} ({}): {cause}",
                self.code, self.kind
            ),
        }
    }
}
impl std::error::Error for ResolutionError {}
pub fn is_editable_sop_class(sop_class_uid: &str) -> bool {
    matches!(
        sop_class_uid,
        SEGMENTATION_STORAGE
            | LABEL_MAP_SEGMENTATION_STORAGE
            | gsps::GRAYSCALE_SOFTCOPY_PRESENTATION_STATE_STORAGE
            | sr::KEY_OBJECT_SELECTION_DOCUMENT_STORAGE
            | rtstruct::RT_STRUCTURE_SET_STORAGE
            | sr::measurement_report::ENHANCED_SR_STORAGE
            | sr::measurement_report::COMPREHENSIVE_SR_STORAGE
            | sr::measurement_report::COMPREHENSIVE_3D_SR_STORAGE
            | GRAYSCALE_PLANAR_MPR_VPS_STORAGE
            | COMPOSITING_PLANAR_MPR_VPS_STORAGE
            | VOLUME_RENDERING_VPS_STORAGE
            | SEGMENTED_VOLUME_RENDERING_VPS_STORAGE
            | MULTIPLE_VOLUME_RENDERING_VPS_STORAGE
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ReadRequirement {
    Identity = 0,
    CompleteModules = 1,
    PixelData = 2,
}
pub fn read_requirement_for_sop_class(sop_class_uid: &str) -> ReadRequirement {
    resolve::resolver_for_sop_class(&resolve::clean_uid(sop_class_uid)).0
}
pub fn resolves_from_identity(sop_class_uid: &str) -> bool {
    read_requirement_for_sop_class(sop_class_uid) == ReadRequirement::Identity
}
