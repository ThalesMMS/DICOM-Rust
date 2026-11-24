#![forbid(unsafe_code)]
#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
mod algorithm;
mod encapsulated;
mod frame_masks;
mod labelmap;
mod overlap;
mod read;
mod resample;
mod samples;
mod stream;
mod write;
use crate::derivation::DerivedCommon;
use crate::derivedio as dio;
use crate::dicomcolor;
use crate::object::Tag;
use crate::render::{SliceGeometry, VolumeGeometry};
use crate::roi::RasterMask;
use crate::roi::segmentation::Segmentation3D;
use crate::textfmt::quote;
pub use algorithm::AlgorithmIdentification;
pub use encapsulated::{PixelDataReadOptions, read_context};
pub use frame_masks::{FrameMask, FrameMaskExtractor};
pub use labelmap::{BinaryOptions, LabelMapOptions, to_binary, to_label_map};
pub use overlap::{OverlapReport, SegmentOverlap, overlaps};
pub use read::{read, read_header, read_with_options, read_with_options_report};
pub use resample::{ResampleOptions, Resampled, ResampledSegment, Resampler, resample};
pub use samples::{DEFAULT_FRACTIONAL_THRESHOLD, fractional_cutoff};
use std::collections::HashMap;
use std::fmt;
pub use stream::{
    DocumentFrames, FramePayload, FrameSource, StreamWriteOptions, StreamWriteResult, write_frames,
    write_frames_context,
};
pub use write::write;
pub const SEGMENTATION_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.4";
pub const LABEL_MAP_SEGMENTATION_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.7";
pub const SEGMENTATION_TYPE_BINARY: &str = "BINARY";
pub const SEGMENTATION_TYPE_FRACTIONAL: &str = "FRACTIONAL";
pub const SEGMENTATION_TYPE_LABEL_MAP: &str = "LABELMAP";
pub const DEFAULT_CONTENT_LABEL: &str = "SEGMENTATION";
pub const FRACTIONAL_TYPE_PROBABILITY: &str = "PROBABILITY";
pub const FRACTIONAL_TYPE_OCCUPANCY: &str = "OCCUPANCY";
pub const SEGMENTS_OVERLAP_YES: &str = "YES";
pub const SEGMENTS_OVERLAP_NO: &str = "NO";
pub const SEGMENTS_OVERLAP_UNDEFINED: &str = "UNDEFINED";
pub const ALGORITHM_MANUAL: &str = "MANUAL";
pub const ALGORITHM_AUTOMATIC: &str = "AUTOMATIC";
pub const ALGORITHM_SEMIAUTOMATIC: &str = "SEMIAUTOMATIC";
pub const MAX_READ_FRAMES: i64 = 100_000;
pub const MAX_READ_PIXELS: i64 = 1 << 30;
pub(crate) const MAX_READ_DIMENSION: i64 = (1 << 16) - 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    UnsupportedSopClass,
    UnsupportedSegmentationType,
    UnsupportedPixelData,
    UnsupportedFramePayload,
    ResourceLimitExceeded,
    InvalidObject,
    MissingReference,
    TruncatedPixelData,
    OverlappingSegments,
    InvalidEncapsulation,
    NoGeometry,
    Other,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::UnsupportedSopClass => "dicom/seg: unsupported SOP class",
            Kind::UnsupportedSegmentationType => "dicom/seg: unsupported segmentation type",
            Kind::UnsupportedPixelData => "dicom/seg: unsupported Pixel Data",
            Kind::UnsupportedFramePayload => "dicom/seg: unsupported frame payload",
            Kind::ResourceLimitExceeded => "dicom/seg: resource limit exceeded",
            Kind::InvalidObject => "dicom/seg: invalid object",
            Kind::MissingReference => "dicom/seg: missing reference",
            Kind::TruncatedPixelData => "dicom/seg: truncated Pixel Data",
            Kind::OverlappingSegments => "dicom/seg: segments overlap",
            Kind::InvalidEncapsulation => "dicom/seg: invalid encapsulated Pixel Data",
            Kind::NoGeometry => "dicom/seg: segmentation has no usable plane geometry",
            Kind::Other => "",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kinds: Vec<Kind>,
    text: String,
    roi: Option<&'static str>,
}
impl Error {
    pub fn sentinel(kind: Kind) -> Error {
        Error {
            kinds: vec![kind],
            text: kind.text().to_string(),
            roi: None,
        }
    }
    pub fn wrap(kind: Kind, detail: impl fmt::Display) -> Error {
        Error {
            kinds: vec![kind],
            text: format!("{}: {detail}", kind.text()),
            roi: None,
        }
    }
    pub fn other(text: impl fmt::Display) -> Error {
        Error {
            kinds: vec![Kind::Other],
            text: text.to_string(),
            roi: None,
        }
    }
    pub(crate) fn from_roi(e: crate::roi::Error) -> Error {
        Error {
            kinds: vec![Kind::Other],
            text: e.to_string(),
            roi: Some(e.sentinel_text()),
        }
    }
    pub(crate) fn with_prefix(&self, prefix: impl fmt::Display) -> Error {
        Error {
            kinds: self.kinds.clone(),
            text: format!("{prefix}: {}", self.text),
            roi: self.roi,
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }
    pub fn is_roi(&self, sentinel: &str) -> bool {
        self.roi.is_some_and(|s| !s.is_empty() && s == sentinel)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub(crate) fn invalid(detail: impl fmt::Display) -> Error {
    Error::wrap(Kind::InvalidObject, detail)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadOptions {
    pub allow_truncated_pixel_data: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadReport {
    pub pixel_data_bytes: i64,
    pub expected_pixel_data_bytes: i64,
    pub incomplete_frames: Vec<i64>,
}
impl ReadReport {
    pub fn truncated(&self) -> bool {
        !self.incomplete_frames.is_empty()
    }
}
pub(crate) const TAG_SOURCE_IMAGE_SEQUENCE: Tag = Tag::new(0x0008, 0x2112);
pub(crate) const TAG_DERIVATION_IMAGE_SEQUENCE: Tag = Tag::new(0x0008, 0x9124);
pub(crate) const TAG_REFERENCED_SERIES_SEQUENCE: Tag = Tag::new(0x0008, 0x1115);
pub(crate) const TAG_REFERENCED_INSTANCE_SEQUENCE: Tag = Tag::new(0x0008, 0x114A);
pub(crate) const TAG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0100);
pub(crate) const TAG_CODING_SCHEME_DESIGNATOR: Tag = Tag::new(0x0008, 0x0102);
pub(crate) const TAG_CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
pub(crate) const TAG_SLICE_THICKNESS: Tag = Tag::new(0x0018, 0x0050);
pub(crate) const TAG_SPACING_BETWEEN_SLICES: Tag = Tag::new(0x0018, 0x0088);
pub(crate) const TAG_IMAGE_POSITION_PATIENT: Tag = Tag::new(0x0020, 0x0032);
pub(crate) const TAG_IMAGE_ORIENTATION_PATIENT: Tag = Tag::new(0x0020, 0x0037);
pub(crate) const TAG_IN_STACK_POSITION_NUMBER: Tag = Tag::new(0x0020, 0x9057);
pub(crate) const TAG_STACK_ID: Tag = Tag::new(0x0020, 0x9056);
pub(crate) const TAG_FRAME_CONTENT_SEQUENCE: Tag = Tag::new(0x0020, 0x9111);
pub(crate) const TAG_SEGMENT_SEQUENCE: Tag = Tag::new(0x0062, 0x0002);
pub(crate) const TAG_SEGMENTATION_TYPE: Tag = Tag::new(0x0062, 0x0001);
pub(crate) const TAG_SEGMENTED_PROPERTY_CATEGORY_SEQ: Tag = Tag::new(0x0062, 0x0003);
pub(crate) const TAG_SEGMENT_NUMBER: Tag = Tag::new(0x0062, 0x0004);
pub(crate) const TAG_SEGMENT_LABEL: Tag = Tag::new(0x0062, 0x0005);
pub(crate) const TAG_SEGMENT_ALGORITHM_TYPE: Tag = Tag::new(0x0062, 0x0008);
pub(crate) const TAG_SEGMENT_ALGORITHM_NAME: Tag = Tag::new(0x0062, 0x0009);
pub(crate) const TAG_SEGMENT_IDENTIFICATION_SEQUENCE: Tag = Tag::new(0x0062, 0x000A);
pub(crate) const TAG_REFERENCED_SEGMENT_NUMBER: Tag = Tag::new(0x0062, 0x000B);
pub(crate) const TAG_RECOMMENDED_DISPLAY_CIELAB_VALUE: Tag = Tag::new(0x0062, 0x000D);
pub(crate) const TAG_SEGMENTED_PROPERTY_TYPE_SEQ: Tag = Tag::new(0x0062, 0x000F);
pub(crate) const TAG_MAXIMUM_FRACTIONAL_VALUE: Tag = Tag::new(0x0062, 0x000E);
pub(crate) const TAG_SEGMENTATION_FRACTIONAL_TYPE: Tag = Tag::new(0x0062, 0x0010);
pub(crate) const TAG_SEGMENTS_OVERLAP: Tag = Tag::new(0x0062, 0x0013);
pub(crate) const TAG_PIXEL_SPACING: Tag = Tag::new(0x0028, 0x0030);
pub(crate) const TAG_PIXEL_MEASURES_SEQUENCE: Tag = Tag::new(0x0028, 0x9110);
pub(crate) const TAG_DIMENSION_INDEX_VALUES: Tag = Tag::new(0x0020, 0x9157);
pub(crate) const TAG_DIMENSION_INDEX_POINTER: Tag = Tag::new(0x0020, 0x9165);
pub(crate) const TAG_FUNCTIONAL_GROUP_POINTER: Tag = Tag::new(0x0020, 0x9167);
pub(crate) const TAG_DIMENSION_INDEX_SEQUENCE: Tag = Tag::new(0x0020, 0x9222);
pub(crate) const TAG_PLANE_POSITION_SEQUENCE: Tag = Tag::new(0x0020, 0x9113);
pub(crate) const TAG_PLANE_ORIENTATION_SEQUENCE: Tag = Tag::new(0x0020, 0x9116);
pub(crate) const TAG_SHARED_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9229);
pub(crate) const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
pub(crate) const TAG_LOSSY_IMAGE_COMPRESSION: Tag = Tag::new(0x0028, 0x2110);
pub(crate) const TAG_DIMENSION_ORGANIZATION_SEQUENCE: Tag = Tag::new(0x0020, 0x9221);
pub(crate) const TAG_DIMENSION_ORGANIZATION_UID: Tag = Tag::new(0x0020, 0x9164);
pub(crate) const TAG_PER_FRAME_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9230);
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferencedImage {
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub frames: Vec<i64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodedEntry {
    pub code_value: String,
    pub coding_scheme_designator: String,
    pub code_meaning: String,
}
impl CodedEntry {
    pub fn new(value: &str, scheme: &str, meaning: &str) -> CodedEntry {
        CodedEntry {
            code_value: value.into(),
            coding_scheme_designator: scheme.into(),
            code_meaning: meaning.into(),
        }
    }
    pub(crate) fn is_zero(&self) -> bool {
        self.code_value.is_empty()
            && self.coding_scheme_designator.is_empty()
            && self.code_meaning.is_empty()
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Segment {
    pub number: i64,
    pub label: String,
    pub algorithm_type: String,
    pub algorithm_name: String,
    pub algorithm_identification: AlgorithmIdentification,
    pub recommended_display_cielab: [u16; 3],
    pub segmented_property_category: CodedEntry,
    pub segmented_property_type: CodedEntry,
}
pub(crate) fn trim_space(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{85}' || c == '\u{A0}')
}
impl Segment {
    pub fn validate_algorithm(&self) -> Result<(), Error> {
        let algorithm_type = trim_space(&self.algorithm_type);
        match algorithm_type {
            ALGORITHM_MANUAL => {}
            ALGORITHM_AUTOMATIC | ALGORITHM_SEMIAUTOMATIC => {
                if trim_space(&self.algorithm_name).is_empty() {
                    return Err(invalid(format!(
                        "segment {} is {algorithm_type} without a Segment Algorithm Name",
                        self.number
                    )));
                }
            }
            _ => {
                return Err(invalid(format!(
                    "segment {} algorithm type {}",
                    self.number,
                    quote(algorithm_type)
                )));
            }
        }
        self.algorithm_identification.validate()
    }
}
pub fn rgb_to_dicom_cielab(rgb: [i64; 3]) -> [u16; 3] {
    dicomcolor::rgb_to_cielab(rgb)
}
pub fn dicom_cielab_to_rgb(encoded: [u16; 3]) -> [i64; 3] {
    dicomcolor::cielab_to_rgb(encoded)
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub segment_number: i64,
    pub slice_index: i64,
    pub referenced_sop_instance_uid: String,
    pub referenced_frame_number: i64,
    pub geometry: SliceGeometry,
    pub mask: Option<RasterMask>,
    pub derivation_description: String,
    pub label_map: Vec<u16>,
    pub fractional: Vec<u8>,
}
pub type Identification = dio::Identification;
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub common: DerivedCommon,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub identification: Identification,
    pub content_date: String,
    pub content_time: String,
    pub rows: i64,
    pub columns: i64,
    pub segmentation_type: String,
    pub fractional_type: String,
    pub max_fractional_value: i64,
    pub label_map_bits_allocated: i64,
    pub segments_overlap: String,
    pub segments: Vec<Segment>,
    pub referenced_images: Vec<ReferencedImage>,
    pub geometry: VolumeGeometry,
    pub frames: Vec<Frame>,
    pub now: Option<crate::dcmtime::ZonedTime>,
}
#[derive(Debug, Clone, Default)]
pub struct FromRoiOptions {
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub segment: Segment,
    pub referenced_images: Vec<ReferencedImage>,
    pub segmentation: Option<Segmentation3D>,
}
#[derive(Debug, Clone, Default)]
pub struct RoiMask {
    pub segment: Segment,
    pub slice_index: i64,
    pub source_image: ReferencedImage,
    pub geometry: SliceGeometry,
    pub mask: Option<RasterMask>,
}
#[derive(Debug, Clone, Default)]
pub struct FromRoiMasksOptions {
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub rows: i64,
    pub columns: i64,
    pub geometry: VolumeGeometry,
    pub masks: Vec<RoiMask>,
}
pub fn from_roi(opts: &FromRoiOptions) -> Result<Document, Error> {
    let Some(segmentation) = &opts.segmentation else {
        return Err(invalid("segmentation is required"));
    };
    let mut segment = opts.segment.clone();
    if segment.number == 0 {
        segment.number = 1;
    }
    let mut masks = Vec::new();
    for slice in segmentation.slices() {
        let reference = if slice >= 0 && (slice as usize) < opts.referenced_images.len() {
            opts.referenced_images[slice as usize].clone()
        } else {
            ReferencedImage::default()
        };
        masks.push(RoiMask {
            segment: segment.clone(),
            slice_index: slice,
            source_image: reference,
            geometry: SliceGeometry::default(),
            mask: segmentation.mask_at(slice).cloned(),
        });
    }
    let mut doc = from_roi_masks(&FromRoiMasksOptions {
        sop_instance_uid: opts.sop_instance_uid.clone(),
        study_instance_uid: opts.study_instance_uid.clone(),
        series_instance_uid: opts.series_instance_uid.clone(),
        frame_of_reference_uid: opts.frame_of_reference_uid.clone(),
        rows: segmentation.rows,
        columns: segmentation.columns,
        geometry: segmentation.geometry.clone(),
        masks,
    })?;
    doc.referenced_images = opts.referenced_images.clone();
    Ok(doc)
}
pub fn from_roi_masks(opts: &FromRoiMasksOptions) -> Result<Document, Error> {
    if opts.masks.is_empty() {
        return Err(invalid("at least one ROI mask is required"));
    }
    let (mut rows, mut columns) = (opts.rows, opts.columns);
    if (rows == 0 || columns == 0)
        && let Some(mask) = opts.masks.iter().find_map(|m| m.mask.as_ref())
    {
        rows = mask.rows;
        columns = mask.columns;
    }
    if rows <= 0 || columns <= 0 {
        return Err(invalid("non-positive mask dimensions"));
    }
    let mut doc = Document {
        sop_class_uid: SEGMENTATION_STORAGE.to_string(),
        sop_instance_uid: opts.sop_instance_uid.clone(),
        study_instance_uid: opts.study_instance_uid.clone(),
        series_instance_uid: opts.series_instance_uid.clone(),
        frame_of_reference_uid: opts.frame_of_reference_uid.clone(),
        rows,
        columns,
        segmentation_type: SEGMENTATION_TYPE_BINARY.to_string(),
        geometry: opts.geometry.clone(),
        ..Document::default()
    };
    let mut segment_indexes: HashMap<i64, usize> = HashMap::new();
    let mut reference_indexes: HashMap<String, usize> = HashMap::new();
    for (i, input) in opts.masks.iter().enumerate() {
        let Some(mask) = &input.mask else {
            return Err(invalid(format!("ROI mask {i} is nil")));
        };
        if mask.is_empty() {
            return Err(invalid(format!("ROI mask {i} is empty")));
        }
        if mask.rows != rows || mask.columns != columns {
            return Err(invalid(format!(
                "ROI mask {i} dimensions {}x{} do not match {columns}x{rows}",
                mask.columns, mask.rows
            )));
        }
        let reference = normalize_reference(&input.source_image);
        if reference.series_instance_uid.is_empty()
            || reference.sop_class_uid.is_empty()
            || reference.sop_instance_uid.is_empty()
        {
            return Err(Error::wrap(
                Kind::MissingReference,
                format!("ROI mask {i} missing source image reference"),
            ));
        }
        let segment = normalize_segment(&input.segment, i as i64 + 1);
        if let Some(&existing) = segment_indexes.get(&segment.number) {
            if doc.segments[existing] != segment {
                return Err(invalid(format!(
                    "conflicting segment metadata for segment {}",
                    segment.number
                )));
            }
        } else {
            segment_indexes.insert(segment.number, doc.segments.len());
            doc.segments.push(segment.clone());
        }
        let key = reference_key(&reference);
        if !reference_indexes.contains_key(&key) {
            reference_indexes.insert(key, doc.referenced_images.len());
            doc.referenced_images.push(reference.clone());
        }
        doc.frames.push(Frame {
            segment_number: segment.number,
            slice_index: input.slice_index,
            referenced_sop_instance_uid: reference.sop_instance_uid.clone(),
            referenced_frame_number: reference.frames.first().copied().unwrap_or(0),
            geometry: input.geometry,
            mask: Some(mask.clone()),
            ..Frame::default()
        });
    }
    Ok(doc)
}
pub fn generic_roi_category_code() -> CodedEntry {
    CodedEntry::new("ROI", "99DRST", "Region of Interest")
}
pub fn generic_roi_type_code() -> CodedEntry {
    CodedEntry::new("ROI_MASK", "99DRST", "ROI mask")
}
pub(crate) fn normalize_segment(segment: &Segment, fallback_number: i64) -> Segment {
    let mut segment = segment.clone();
    if segment.number == 0 {
        segment.number = fallback_number;
    }
    segment.label = trim_space(&segment.label).to_string();
    if segment.label.is_empty() {
        segment.label = format!("Segment {}", segment.number);
    }
    segment.algorithm_type = trim_space(&segment.algorithm_type).to_string();
    segment.algorithm_name = trim_space(&segment.algorithm_name).to_string();
    if segment.algorithm_type.is_empty() {
        segment.algorithm_type = ALGORITHM_MANUAL.to_string();
    }
    segment.segmented_property_category = normalize_code(
        &segment.segmented_property_category,
        generic_roi_category_code(),
    );
    segment.segmented_property_type =
        normalize_code(&segment.segmented_property_type, generic_roi_type_code());
    segment
}
fn normalize_code(code: &CodedEntry, fallback: CodedEntry) -> CodedEntry {
    let code = CodedEntry::new(
        trim_space(&code.code_value),
        trim_space(&code.coding_scheme_designator),
        trim_space(&code.code_meaning),
    );
    if code.is_zero() { fallback } else { code }
}
fn normalize_reference(reference: &ReferencedImage) -> ReferencedImage {
    ReferencedImage {
        series_instance_uid: trim_space(&reference.series_instance_uid).to_string(),
        sop_class_uid: trim_space(&reference.sop_class_uid).to_string(),
        sop_instance_uid: trim_space(&reference.sop_instance_uid).to_string(),
        frames: reference.frames.clone(),
    }
}
fn reference_key(reference: &ReferencedImage) -> String {
    let mut b = format!(
        "{}|{}|{}",
        reference.series_instance_uid, reference.sop_class_uid, reference.sop_instance_uid
    );
    for frame in &reference.frames {
        b.push_str(&format!("|{frame}"));
    }
    b
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RoiOptions {
    pub fractional_threshold: f64,
}
pub fn to_roi(doc: &Document, segment_number: i64) -> Result<Segmentation3D, Error> {
    to_roi_with_options(doc, segment_number, RoiOptions::default())
}
pub fn to_roi_with_options(
    doc: &Document,
    mut segment_number: i64,
    opts: RoiOptions,
) -> Result<Segmentation3D, Error> {
    let threshold = samples::normalize_fractional_threshold(opts.fractional_threshold)?;
    if segment_number == 0
        && let Some(segment) = doc.segments.iter().find(|s| s.number != 0)
    {
        segment_number = segment.number;
    }
    let mut out = Segmentation3D::new(doc.geometry.clone(), doc.columns, doc.rows);
    for frame in &doc.frames {
        if frame.label_map.is_empty() && frame.segment_number != segment_number {
            continue;
        }
        if !frame.fractional.is_empty() || doc.segmentation_type == SEGMENTATION_TYPE_FRACTIONAL {
            let mask = samples::fractional_mask(doc, frame, threshold)?;
            out.set_mask(frame.slice_index, Some(mask));
        } else if let Some(mask) = &frame.mask {
            out.set_mask(frame.slice_index, Some(mask.clone()));
        } else if !frame.label_map.is_empty() {
            if doc.columns <= 0 {
                return Err(invalid("label-map frame has non-positive columns"));
            }
            let mut mask = RasterMask::new(doc.columns, doc.rows);
            for (idx, value) in frame.label_map.iter().enumerate() {
                if i64::from(*value) == segment_number {
                    let idx = idx as i64;
                    mask.set(idx % doc.columns, idx / doc.columns, true);
                }
            }
            out.set_mask(frame.slice_index, Some(mask));
        }
    }
    Ok(out)
}
pub(crate) fn validate_representable_segmentation_type(
    sop_class_uid: &str,
    segmentation_type: &str,
) -> Result<(), Error> {
    match sop_class_uid {
        SEGMENTATION_STORAGE => match segmentation_type {
            "" | SEGMENTATION_TYPE_BINARY | SEGMENTATION_TYPE_FRACTIONAL => Ok(()),
            _ => Err(Error::wrap(
                Kind::UnsupportedSegmentationType,
                format!("{} for Segmentation Storage", quote(segmentation_type)),
            )),
        },
        LABEL_MAP_SEGMENTATION_STORAGE => {
            if segmentation_type.is_empty() || segmentation_type == SEGMENTATION_TYPE_LABEL_MAP {
                return Ok(());
            }
            Err(Error::wrap(
                Kind::UnsupportedSegmentationType,
                format!(
                    "{} for Label Map Segmentation Storage",
                    quote(segmentation_type)
                ),
            ))
        }
        _ => Err(Error::wrap(Kind::UnsupportedSopClass, sop_class_uid)),
    }
}
pub(crate) fn segmentation_type(doc: &Document) -> &str {
    if !doc.segmentation_type.is_empty() {
        return &doc.segmentation_type;
    }
    if doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE {
        return SEGMENTATION_TYPE_LABEL_MAP;
    }
    SEGMENTATION_TYPE_BINARY
}
pub(crate) fn valid_slice_geometry(geometry: &SliceGeometry) -> bool {
    geometry.rows > 0
        && geometry.columns > 0
        && geometry.row_spacing > 0.0
        && geometry.col_spacing > 0.0
        && geometry.row_dir.length() > 0.0
        && geometry.col_dir.length() > 0.0
}
pub(crate) fn geometry_for_frame(
    geometry: &VolumeGeometry,
    frame: &Frame,
) -> Option<SliceGeometry> {
    if valid_slice_geometry(&frame.geometry) {
        return Some(frame.geometry);
    }
    let slices = geometry.slices();
    if frame.slice_index >= 0 && (frame.slice_index as usize) < slices.len() {
        return Some(slices[frame.slice_index as usize]);
    }
    if slices.len() == 1 {
        return Some(slices[0]);
    }
    None
}
pub(crate) fn positive_finite(value: f64) -> bool {
    value > 0.0 && value.is_finite()
}
#[derive(Debug, Clone, Default)]
struct SourceReferenceEntry {
    first: ReferencedImage,
    first_index: usize,
    first_selector_index: i64,
    wildcard: Option<(ReferencedImage, usize)>,
}
#[derive(Debug, Default)]
pub(crate) struct SourceReferenceIndex {
    by_sop_instance_uid: HashMap<String, usize>,
    entries: Vec<SourceReferenceEntry>,
    by_frame: HashMap<(String, i64), (ReferencedImage, usize)>,
}
fn duplicate_source_reference_error(
    sop_instance_uid: &str,
    frame_number: i64,
    first_index: usize,
    second_index: usize,
) -> Error {
    let selector = if frame_number > 0 {
        format!("frame {frame_number}")
    } else {
        "all frames".to_string()
    };
    invalid(format!(
        "referenced images {first_index} and {second_index} duplicate SOP Instance UID {sop_instance_uid} {selector}"
    ))
}
fn overlapping_source_reference_error(
    sop_instance_uid: &str,
    first_index: i64,
    second_index: usize,
) -> Error {
    invalid(format!(
        "referenced images {first_index} and {second_index} overlap wildcard and explicit frames for SOP Instance UID {sop_instance_uid}"
    ))
}
impl SourceReferenceIndex {
    pub(crate) fn new(refs: &[ReferencedImage]) -> Result<SourceReferenceIndex, Error> {
        let mut index = SourceReferenceIndex::default();
        for (ref_index, reference) in refs.iter().enumerate() {
            let missing = |what: &str| {
                Error::wrap(
                    Kind::MissingReference,
                    format!("referenced image {ref_index} missing {what}"),
                )
            };
            if reference.series_instance_uid.is_empty() {
                return Err(missing("Series Instance UID"));
            }
            if reference.sop_class_uid.is_empty() {
                return Err(missing("SOP Class UID"));
            }
            if reference.sop_instance_uid.is_empty() {
                return Err(missing("SOP Instance UID"));
            }
            let existing = index
                .by_sop_instance_uid
                .get(&reference.sop_instance_uid)
                .copied();
            let entry_index = match existing {
                Some(i) => i,
                None => {
                    index
                        .by_sop_instance_uid
                        .insert(reference.sop_instance_uid.clone(), index.entries.len());
                    index.entries.push(SourceReferenceEntry {
                        first: reference.clone(),
                        first_index: ref_index,
                        first_selector_index: -1,
                        wildcard: None,
                    });
                    index.entries.len() - 1
                }
            };
            let entry = &mut index.entries[entry_index];
            if existing.is_some()
                && (entry.first.series_instance_uid != reference.series_instance_uid
                    || entry.first.sop_class_uid != reference.sop_class_uid)
            {
                return Err(invalid(format!(
                    "referenced images {} and {ref_index} conflict for SOP Instance UID {}",
                    entry.first_index, reference.sop_instance_uid
                )));
            }
            if reference.frames.is_empty() {
                if let Some((_, wildcard_index)) = &entry.wildcard {
                    return Err(duplicate_source_reference_error(
                        &reference.sop_instance_uid,
                        0,
                        *wildcard_index,
                        ref_index,
                    ));
                }
                if entry.first_selector_index >= 0 {
                    return Err(overlapping_source_reference_error(
                        &reference.sop_instance_uid,
                        entry.first_selector_index,
                        ref_index,
                    ));
                }
                entry.wildcard = Some((reference.clone(), ref_index));
                entry.first_selector_index = ref_index as i64;
                continue;
            }
            if let Some((_, wildcard_index)) = &entry.wildcard {
                return Err(overlapping_source_reference_error(
                    &reference.sop_instance_uid,
                    *wildcard_index as i64,
                    ref_index,
                ));
            }
            for &frame_number in &reference.frames {
                if frame_number <= 0 {
                    return Err(invalid(format!(
                        "referenced image {ref_index} has non-positive frame number {frame_number}"
                    )));
                }
                let key = (reference.sop_instance_uid.clone(), frame_number);
                if let Some((_, previous)) = index.by_frame.get(&key) {
                    return Err(duplicate_source_reference_error(
                        &reference.sop_instance_uid,
                        frame_number,
                        *previous,
                        ref_index,
                    ));
                }
                index.by_frame.insert(key, (reference.clone(), ref_index));
                if entry.first_selector_index < 0 {
                    entry.first_selector_index = ref_index as i64;
                }
            }
        }
        Ok(index)
    }
    pub(crate) fn resolve_frames(&self, frames: &[Frame]) -> Result<Vec<ReferencedImage>, Error> {
        let mut resolved = vec![ReferencedImage::default(); frames.len()];
        for (frame_index, frame) in frames.iter().enumerate() {
            if frame.referenced_sop_instance_uid.is_empty() {
                if frame.referenced_frame_number != 0 {
                    return Err(Error::wrap(
                        Kind::MissingReference,
                        format!(
                            "frame {frame_index} has referenced frame number {} without a source SOP Instance UID",
                            frame.referenced_frame_number
                        ),
                    ));
                }
                continue;
            }
            let Some(reference) = self.lookup(frame) else {
                return Err(Error::wrap(
                    Kind::MissingReference,
                    format!(
                        "frame {frame_index} references unlisted source SOP Instance UID {} frame {}",
                        frame.referenced_sop_instance_uid, frame.referenced_frame_number
                    ),
                ));
            };
            resolved[frame_index] = reference;
        }
        Ok(resolved)
    }
    fn lookup(&self, frame: &Frame) -> Option<ReferencedImage> {
        let entry = &self.entries[*self
            .by_sop_instance_uid
            .get(&frame.referenced_sop_instance_uid)?];
        if frame.referenced_frame_number == 0 {
            return Some(entry.first.clone());
        }
        let key = (
            frame.referenced_sop_instance_uid.clone(),
            frame.referenced_frame_number,
        );
        if let Some((reference, _)) = self.by_frame.get(&key) {
            return Some(reference.clone());
        }
        entry.wildcard.as_ref().map(|(r, _)| r.clone())
    }
}
pub(crate) fn set_packed_bit_range(data: &mut [u8], start_bit: i64, end_bit: i64) {
    let mut start = start_bit.max(0);
    let end = end_bit.min(data.len() as i64 * 8);
    if start >= end {
        return;
    }
    let offset = start & 7;
    if offset != 0 {
        let count = (8 - offset).min(end - start);
        data[(start >> 3) as usize] |= low_bit_mask(count) << offset;
        start += count;
    }
    while end - start >= 8 {
        data[(start >> 3) as usize] = 0xFF;
        start += 8;
    }
    if start < end {
        data[(start >> 3) as usize] |= low_bit_mask(end - start);
    }
}
pub(crate) fn low_bit_mask(count: i64) -> u8 {
    if count <= 0 {
        0
    } else if count >= 8 {
        0xFF
    } else {
        (1u8 << count) - 1
    }
}
