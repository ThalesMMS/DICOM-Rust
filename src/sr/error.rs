use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    InvalidDocument,
    NoDataset,
    InvalidGraphicData,
    ResourceLimitExceeded,
    ReferenceResolution,
    StaleReferenceIndex,
    RelationshipConstraint,
    UnconstrainedSopClass,
    InvalidMeasurementReport,
    UnsupportedSrStorage,
    DocumentLifecycle,
    TemplateValidation,
    InvalidTemplateRegistry,
    ImageLibrary,
    NotKosRejection,
    MeasurementContext,
    MissingReference,
    NumericRepresentation,
    NilDocument,
    ContextGroupByReference,
    MetadataSnapshot,
    Object,
    MissingSopClassUid,
    MissingSopInstanceUid,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::InvalidDocument => "dicom/sr: invalid document",
            Kind::NoDataset => "dicom/sr: nil dataset",
            Kind::InvalidGraphicData => "dicom/sr: invalid Graphic Data",
            Kind::ResourceLimitExceeded => "dicom/sr: resource limit exceeded",
            Kind::ReferenceResolution => "dicom/sr: reference resolution failed",
            Kind::StaleReferenceIndex => "dicom/sr: stale reference index",
            Kind::RelationshipConstraint => "dicom/sr: relationship content constraint violated",
            Kind::UnconstrainedSopClass => {
                "dicom/sr: no relationship content constraints for SOP Class"
            }
            Kind::InvalidMeasurementReport => "dicom/sr: invalid measurement report",
            Kind::UnsupportedSrStorage => "dicom/sr: unsupported SR storage",
            Kind::DocumentLifecycle => "dicom/sr: document lifecycle operation not allowed",
            Kind::TemplateValidation => "dicom/sr: template validation failed",
            Kind::InvalidTemplateRegistry => "dicom/sr: invalid template registry",
            Kind::ImageLibrary => "dicom/sr: image library rejected",
            Kind::NotKosRejection => "dicom/sr: not a supported KOS rejection note",
            Kind::MeasurementContext => "dicom/sr: measurement context profile rejected",
            Kind::MissingReference => "dicom/sr: missing measurement reference",
            Kind::NumericRepresentation => "dicom/sr: numeric representation rejected",
            Kind::NilDocument => "dicom/sr: nil document",
            Kind::ContextGroupByReference => "dicom/sr: context group defined by reference",
            Kind::MetadataSnapshot => "dicom/sr: metadata snapshot rejected",
            Kind::Object => "",
            Kind::MissingSopClassUid => "dicom: missing SOP Class UID",
            Kind::MissingSopInstanceUid => "dicom: missing SOP Instance UID",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferenceErrorInfo {
    pub code: String,
    pub source: Vec<u32>,
    pub slot: Vec<u32>,
    pub target: Vec<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SrError {
    pub msg: String,
    pub kinds: Vec<Kind>,
    pub reference: Option<Box<ReferenceErrorInfo>>,
    pub code: Option<String>,
}
impl fmt::Display for SrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}
impl std::error::Error for SrError {}
impl SrError {
    pub fn new(kind: Kind, suffix: &str) -> SrError {
        SrError {
            msg: format!("{}{suffix}", kind.text()),
            kinds: vec![kind],
            reference: None,
            code: None,
        }
    }
    pub fn msg(msg: impl Into<String>) -> SrError {
        SrError {
            msg: msg.into(),
            kinds: Vec::new(),
            reference: None,
            code: None,
        }
    }
    #[doc(hidden)]
    pub fn invalid_document(suffix: &str) -> SrError {
        SrError::new(Kind::InvalidDocument, suffix)
    }
    pub fn relationship(suffix: &str) -> SrError {
        SrError::new(Kind::RelationshipConstraint, suffix)
    }
    pub fn unconstrained(suffix: &str) -> SrError {
        SrError::new(Kind::UnconstrainedSopClass, suffix)
    }
    pub fn coded(kind: Kind, code: &str) -> SrError {
        let mut e = SrError::new(kind, &format!(": {code}"));
        e.code = Some(code.into());
        e
    }
    pub fn reference(info: ReferenceErrorInfo) -> SrError {
        let mut e = SrError::new(Kind::ReferenceResolution, &format!(": {}", info.code));
        e.code = Some(info.code.clone());
        e.reference = Some(Box::new(info));
        e
    }
    pub fn prefixed(mut self, prefix: &str) -> SrError {
        self.msg = format!("{prefix}{}", self.msg);
        self
    }
    pub(crate) fn suffixed(mut self, suffix: &str) -> SrError {
        self.msg.push_str(suffix);
        self
    }
    pub fn wrapped(mut self, kind: Kind) -> SrError {
        self.msg = format!("{}: {}", kind.text(), self.msg);
        self.kinds.insert(0, kind);
        self
    }
    pub(crate) fn wrapped_v(self, kind: Kind) -> SrError {
        SrError::new(kind, &format!(": {}", self.msg))
    }
    pub(crate) fn coded_path(kind: Kind, code: &str, path: &str, limit_code: &str) -> SrError {
        let mut e = SrError::coded(kind, code);
        if super::image_library::valid_numeric_path(path) {
            e.msg.push_str(" at ");
            e.msg.push_str(path);
        }
        if code == limit_code {
            e.kinds.push(Kind::ResourceLimitExceeded);
        }
        e
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
            || (kind == Kind::ResourceLimitExceeded
                && (self.kinds.contains(&Kind::TemplateValidation)
                    || self.kinds.contains(&Kind::InvalidTemplateRegistry))
                && self
                    .code
                    .as_deref()
                    .is_some_and(|c| c.ends_with("resource_limit")))
    }
}
