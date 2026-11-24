#![forbid(unsafe_code)]
use crate::object::{Element, Object, Tag, Value};
use std::fmt;
mod bodyparts;
mod classic;
mod concat_split;
mod concatenation;
mod legacy;
mod mosaic;
mod split;
mod uih_grid;
pub use classic::{
    ClassicOptions, ClassicReport, ContributingEquipment, UidForFrame, convert_to_classic,
};
pub use concat_split::{SplitWriteOptions, split_concatenation};
pub use concatenation::{
    Concatenation, ConcatenationGroup, ConcatenationInstance, ConcatenationPart, SplitOptions,
    group_concatenations, new_concatenation, plan_concatenation,
};
pub use legacy::{LegacyOptions, LegacyReport, convert_to_legacy_enhanced};
pub use mosaic::{
    DIAGNOSTIC_MOSAIC_CSA_CREATOR_MISSING, DIAGNOSTIC_MOSAIC_IMAGES_FROM_ACQUISITION_MATRIX,
    DIAGNOSTIC_MOSAIC_PROTOCOL_POSITION_MISMATCH, DIAGNOSTIC_MOSAIC_SLICE_NORMAL_INCONSISTENT,
    DIAGNOSTIC_MOSAIC_SLICE_NORMAL_MISSING, MosaicGeometry, MosaicOptions, mosaic_geometry_of,
};
pub use split::{
    MosaicReport, SplitMosaicOptions, SplitUihGridOptions, UidForSlice, UihGridReport,
    split_mosaic, split_uih_grid,
};
pub use uih_grid::{UihGridGeometry, UihGridOptions, uih_grid_geometry_of};
pub const ENHANCED_CT_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.2.1";
pub const LEGACY_CONVERTED_ENHANCED_CT_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.2.2";
pub const CT_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.2";
pub const ENHANCED_MR_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.4.1";
pub const LEGACY_CONVERTED_ENHANCED_MR_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.4.4";
pub const MR_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.4";
pub const PET_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.128";
pub const LEGACY_CONVERTED_ENHANCED_PET_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.128.1";
pub const ENHANCED_PET_IMAGE_STORAGE_UID: &str = "1.2.840.10008.5.1.4.1.1.130";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    InvalidOptions,
    InvalidSource,
    UnsupportedSopClass,
    Nonconformant,
    ResourceLimit,
    ConcatenationIncomplete,
    ConcatenationInconsistent,
    ConcatenationSplit,
    NotMosaic,
    NotUihGrid,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::InvalidOptions => "dicom/multiframe: invalid options",
            Kind::InvalidSource => "dicom/multiframe: invalid source",
            Kind::UnsupportedSopClass => "dicom/multiframe: unsupported SOP class",
            Kind::Nonconformant => "dicom/multiframe: non-conformant enhanced image",
            Kind::ResourceLimit => "dicom/multiframe: resource limit exceeded",
            Kind::ConcatenationIncomplete => "dicom/multiframe: concatenation incomplete",
            Kind::ConcatenationInconsistent => "dicom/multiframe: concatenation inconsistent",
            Kind::ConcatenationSplit => "dicom/multiframe: cannot split into a concatenation",
            Kind::NotMosaic => "dicom/multiframe: not a Siemens mosaic",
            Kind::NotUihGrid => "dicom/multiframe: not a UIH grid image",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    text: String,
    kinds: Vec<Kind>,
    cause: Option<String>,
}
impl Error {
    pub fn bare(kind: Kind) -> Error {
        Error {
            text: kind.text().to_string(),
            kinds: vec![kind],
            cause: None,
        }
    }
    pub fn wrap(kind: Kind, detail: impl AsRef<str>) -> Error {
        Error {
            text: format!("{}: {}", kind.text(), detail.as_ref()),
            kinds: vec![kind],
            cause: None,
        }
    }
    pub fn join(errors: Vec<Error>) -> Error {
        let mut kinds = Vec::new();
        for e in &errors {
            for k in &e.kinds {
                if !kinds.contains(k) {
                    kinds.push(*k);
                }
            }
        }
        Error {
            text: errors
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            kinds,
            cause: None,
        }
    }
    pub(crate) fn with_kinds(text: String, kinds: Vec<Kind>) -> Error {
        Error {
            text,
            kinds,
            cause: None,
        }
    }
    pub fn with_text(text: impl Into<String>) -> Error {
        Error::with_kinds(text.into(), Vec::new())
    }
    pub(crate) fn conversion(text: String, inner: Error) -> Error {
        Error {
            text,
            kinds: inner.kinds.clone(),
            cause: Some(inner.text),
        }
    }
    pub fn cause_text(&self) -> Option<&str> {
        self.cause.as_deref()
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }
    pub fn kinds(&self) -> &[Kind] {
        &self.kinds
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_frames: i64,
    pub max_pixels: u64,
    pub max_input_bytes: i64,
    pub max_output_bytes: i64,
    pub max_fragments: i64,
    pub max_elements: i64,
    pub max_depth: i64,
    pub max_expansion_ratio: i64,
    pub max_duration: i64,
}
pub fn default_limits() -> Limits {
    Limits {
        max_frames: 10000,
        max_pixels: 1 << 30,
        max_input_bytes: 1 << 30,
        max_output_bytes: 1 << 30,
        max_fragments: 100000,
        max_elements: 1000000,
        max_depth: 64,
        max_expansion_ratio: 1024,
        max_duration: 300_000_000_000,
    }
}
pub(crate) fn normalize_limits(mut limits: Limits) -> Result<Limits, Error> {
    if limits.max_frames < 0
        || limits.max_input_bytes < 0
        || limits.max_output_bytes < 0
        || limits.max_fragments < 0
        || limits.max_elements < 0
        || limits.max_depth < 0
        || limits.max_expansion_ratio < 0
        || limits.max_duration < 0
    {
        return Err(Error::wrap(
            Kind::InvalidOptions,
            "limits must not be negative",
        ));
    }
    let defaults = default_limits();
    if limits.max_frames == 0 {
        limits.max_frames = defaults.max_frames;
    }
    if limits.max_pixels == 0 {
        limits.max_pixels = defaults.max_pixels;
    }
    if limits.max_input_bytes == 0 {
        limits.max_input_bytes = defaults.max_input_bytes;
    }
    if limits.max_output_bytes == 0 {
        limits.max_output_bytes = defaults.max_output_bytes;
    }
    if limits.max_fragments == 0 {
        limits.max_fragments = defaults.max_fragments;
    }
    if limits.max_elements == 0 {
        limits.max_elements = defaults.max_elements;
    }
    if limits.max_depth == 0 {
        limits.max_depth = defaults.max_depth;
    }
    if limits.max_expansion_ratio == 0 {
        limits.max_expansion_ratio = defaults.max_expansion_ratio;
    }
    if limits.max_duration == 0 {
        limits.max_duration = defaults.max_duration;
    }
    Ok(limits)
}
pub fn target_sop_class_uid(source_sop_class_uid: &str) -> Option<&'static str> {
    match crate::siemens::trim_space(source_sop_class_uid) {
        ENHANCED_CT_IMAGE_STORAGE_UID | LEGACY_CONVERTED_ENHANCED_CT_IMAGE_STORAGE_UID => {
            Some(CT_IMAGE_STORAGE_UID)
        }
        ENHANCED_MR_IMAGE_STORAGE_UID | LEGACY_CONVERTED_ENHANCED_MR_IMAGE_STORAGE_UID => {
            Some(MR_IMAGE_STORAGE_UID)
        }
        ENHANCED_PET_IMAGE_STORAGE_UID | LEGACY_CONVERTED_ENHANCED_PET_IMAGE_STORAGE_UID => {
            Some(PET_IMAGE_STORAGE_UID)
        }
        _ => None,
    }
}
pub fn can_convert_sop_class(source_sop_class_uid: &str) -> bool {
    target_sop_class_uid(source_sop_class_uid).is_some()
}
pub(crate) const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
pub(crate) const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
pub(crate) const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
pub(crate) const TAG_SLICE_THICKNESS: Tag = Tag::new(0x0018, 0x0050);
pub(crate) const TAG_SPACING_BETWEEN_SLICES: Tag = Tag::new(0x0018, 0x0088);
pub(crate) const TAG_ACQUISITION_MATRIX: Tag = Tag::new(0x0018, 0x1310);
pub(crate) const TAG_IN_PLANE_PHASE_ENCODING_DIRECTION: Tag = Tag::new(0x0018, 0x1312);
pub(crate) const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
pub(crate) const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
pub(crate) const TAG_FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x0020, 0x0052);
pub(crate) const TAG_IMAGE_POSITION_PATIENT: Tag = Tag::new(0x0020, 0x0032);
pub(crate) const TAG_IMAGE_ORIENTATION_PATIENT: Tag = Tag::new(0x0020, 0x0037);
pub(crate) const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
pub(crate) const TAG_PHOTOMETRIC_INTERPRETATION: Tag = Tag::new(0x0028, 0x0004);
pub(crate) const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
pub(crate) const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
pub(crate) const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
pub(crate) const TAG_PIXEL_SPACING: Tag = Tag::new(0x0028, 0x0030);
pub(crate) const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
pub(crate) const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
pub(crate) const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
pub(crate) const TAG_REPRESENTATIVE_FRAME_NUMBER: Tag = Tag::new(0x0028, 0x6010);
pub(crate) const TAG_CONCATENATION_UID: Tag = Tag::new(0x0020, 0x9161);
pub(crate) const TAG_IN_CONCATENATION_NUMBER: Tag = Tag::new(0x0020, 0x9162);
pub(crate) const TAG_IN_CONCATENATION_TOTAL_NUMBER: Tag = Tag::new(0x0020, 0x9163);
pub(crate) const TAG_CONCATENATION_FRAME_OFFSET_NUMBER: Tag = Tag::new(0x0020, 0x9228);
pub(crate) const TAG_SOP_INSTANCE_UID_OF_CONCATENATION_SOURCE: Tag = Tag::new(0x0020, 0x0242);
pub(crate) const TAG_SHARED_FUNCTIONAL_GROUPS_SEQUENCE: Tag = Tag::new(0x5200, 0x9229);
pub(crate) const TAG_PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE: Tag = Tag::new(0x5200, 0x9230);
pub(crate) fn uint16_value(dataset: &Object, tag: Tag) -> Option<u16> {
    let element: &Element = dataset.get(tag)?;
    match &element.value {
        Value::Raw(raw) if raw.len() == 2 => Some(if dataset.value_big_endian() {
            u16::from_be_bytes([raw[0], raw[1]])
        } else {
            u16::from_le_bytes([raw[0], raw[1]])
        }),
        _ => None,
    }
}
pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn unit(v: [f64; 3]) -> ([f64; 3], bool) {
    let n = dot(v, v).sqrt();
    if n.partial_cmp(&1e-9) != Some(std::cmp::Ordering::Greater) || n.is_infinite() {
        return (v, false);
    }
    ([v[0] / n, v[1] / n, v[2] / n], true)
}
