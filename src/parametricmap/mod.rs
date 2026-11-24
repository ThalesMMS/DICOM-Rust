#![forbid(unsafe_code)]
mod mappings;
mod write;
use crate::derivedio as dio;
use crate::display::WindowDomain;
use crate::fmath;
use crate::numtext::{self, format_float};
use crate::object::geometry::FrameGeometry;
use crate::object::{Element, File, Object, Tag, Value, Vr, element_length};
use crate::pixel::pixeldata::{self, NativeFrames};
use crate::pixel::{Metadata, SampleFormat, sample_format};
use crate::render::grid::{clamp_continuous_index, nearest_index_inside};
use crate::render::{GeometryTolerances, SliceGeometry, Vec3};
use crate::roi::{RasterMask, stats_2d};
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
pub use write::{DEFAULT_CONTENT_LABEL, Document, DocumentFrame, Identification, write};
pub const PARAMETRIC_MAP_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.30";
const GO_NAN: f64 = f64::from_bits(0x7FF8_0000_0000_0001);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    UnsupportedSopClass,
    InvalidObject,
    MissingMapping,
    AmbiguousMapping,
    OutsideMapping,
    UnsupportedDimensions,
    GeometryMismatch,
    NonFinite,
    MemoryLimit,
    PayloadUnavailable,
    InvalidPadding,
    MappingOverflow,
    FunctionalGroupConflict,
    InvalidPixelPadding,
    Other,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::UnsupportedSopClass => "dicom/parametricmap: unsupported SOP class",
            Kind::InvalidObject => "dicom/parametricmap: invalid object",
            Kind::MissingMapping => "dicom/parametricmap: missing real world value mapping",
            Kind::AmbiguousMapping => "dicom/parametricmap: ambiguous real world value mapping",
            Kind::OutsideMapping => "dicom/parametricmap: outside real world value mapping domain",
            Kind::UnsupportedDimensions => "dicom/parametricmap: unsupported dimensions",
            Kind::GeometryMismatch => "dicom/parametricmap: geometry mismatch",
            Kind::NonFinite => "dicom/parametricmap: non-finite quantitative value",
            Kind::MemoryLimit => "dicom/parametricmap: memory limit exceeded",
            Kind::PayloadUnavailable => "dicom/parametricmap: pixel payload was not retained",
            Kind::InvalidPadding => "dicom/parametricmap: invalid pixel padding",
            Kind::MappingOverflow => {
                "dicom/parametricmap: non-finite quantitative value: mapping overflow"
            }
            Kind::FunctionalGroupConflict => crate::object::geometry::ERR_FUNCTIONAL_GROUP_CONFLICT,
            Kind::InvalidPixelPadding => "dicom: invalid pixel padding",
            Kind::Other => "",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kinds: Vec<Kind>,
    text: String,
}
impl Error {
    pub fn sentinel(kind: Kind) -> Error {
        let mut kinds = vec![kind];
        if kind == Kind::MappingOverflow {
            kinds.push(Kind::NonFinite);
        }
        Error {
            kinds,
            text: kind.text().to_string(),
        }
    }
    pub fn wrap(kind: Kind, detail: impl fmt::Display) -> Error {
        let mut e = Error::sentinel(kind);
        e.text = format!("{}: {detail}", kind.text());
        e
    }
    pub fn other(text: impl fmt::Display) -> Error {
        Error {
            kinds: vec![Kind::Other],
            text: text.to_string(),
        }
    }
    pub(crate) fn prefixed(mut self, prefix: impl fmt::Display) -> Error {
        self.text = format!("{prefix}: {}", self.text);
        self
    }
    pub(crate) fn under(mut self, kind: Kind) -> Error {
        self.text = format!("{}: {}", kind.text(), self.text);
        self.kinds.insert(0, kind);
        self
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub(crate) const TAG_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0008);
const TAG_DOUBLE_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0009);
pub(crate) const TAG_SHARED_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9229);
pub(crate) const TAG_PER_FRAME_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9230);
pub(crate) const TAG_REAL_WORLD_VALUE_MAPPING_SEQ: Tag = Tag::new(0x0040, 0x9096);
pub(crate) const TAG_RWV_FIRST_MAPPED: Tag = Tag::new(0x0040, 0x9216);
pub(crate) const TAG_RWV_LAST_MAPPED: Tag = Tag::new(0x0040, 0x9211);
pub(crate) const TAG_DOUBLE_RWV_FIRST_MAPPED: Tag = Tag::new(0x0040, 0x9214);
pub(crate) const TAG_DOUBLE_RWV_LAST_MAPPED: Tag = Tag::new(0x0040, 0x9213);
pub(crate) const TAG_RWV_LUT_DATA: Tag = Tag::new(0x0040, 0x9212);
pub(crate) const TAG_RWV_INTERCEPT: Tag = Tag::new(0x0040, 0x9224);
pub(crate) const TAG_RWV_SLOPE: Tag = Tag::new(0x0040, 0x9225);
pub(crate) const TAG_MEASUREMENT_UNITS_CODE_SEQ: Tag = Tag::new(0x0040, 0x08EA);
pub(crate) const TAG_QUANTITY_DEFINITION_SEQ: Tag = Tag::new(0x0040, 0x9220);
pub(crate) const TAG_CONCEPT_NAME_CODE_SEQ: Tag = Tag::new(0x0040, 0xA043);
pub(crate) const TAG_CONCEPT_CODE_SEQ: Tag = Tag::new(0x0040, 0xA168);
pub(crate) const TAG_CODE_VALUE: Tag = Tag::new(0x0008, 0x0100);
pub(crate) const TAG_CODING_SCHEME_DESIGNATOR: Tag = Tag::new(0x0008, 0x0102);
pub(crate) const TAG_CODING_SCHEME_VERSION: Tag = Tag::new(0x0008, 0x0103);
pub(crate) const TAG_CODE_MEANING: Tag = Tag::new(0x0008, 0x0104);
pub(crate) const TAG_DIMENSION_INDEX_SEQ: Tag = Tag::new(0x0020, 0x9222);
pub(crate) const TAG_DIMENSION_INDEX_POINTER: Tag = Tag::new(0x0020, 0x9165);
pub(crate) const TAG_DIMENSION_INDEX_VALUES: Tag = Tag::new(0x0020, 0x9157);
pub(crate) const TAG_FRAME_CONTENT_SEQ: Tag = Tag::new(0x0020, 0x9111);
const TAG_IN_STACK_POSITION_NUMBER: Tag = Tag::new(0x0020, 0x9057);
pub(crate) const TAG_IMAGE_POSITION_PATIENT: Tag = Tag::new(0x0020, 0x0032);
pub(crate) const TAG_REFERENCED_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x1150);
pub(crate) const TAG_REFERENCED_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x1155);
const TAG_REFERENCED_FRAME_NUMBER: Tag = Tag::new(0x0008, 0x1160);
const TAG_INTEGER_PIXEL_PADDING_VALUE: Tag = Tag::new(0x0028, 0x0120);
const TAG_INTEGER_PIXEL_PADDING_RANGE_LIMIT: Tag = Tag::new(0x0028, 0x0121);
const TAG_FLOAT_PIXEL_PADDING_VALUE: Tag = Tag::new(0x0028, 0x0122);
const TAG_FLOAT_PIXEL_PADDING_RANGE_LIMIT: Tag = Tag::new(0x0028, 0x0124);
const TAG_DOUBLE_FLOAT_PIXEL_PADDING_VALUE: Tag = Tag::new(0x0028, 0x0123);
const TAG_DOUBLE_FLOAT_PIXEL_PADDING_RANGE_LIMIT: Tag = Tag::new(0x0028, 0x0125);
const MAX_VOXELS: i64 = 64 * 1024 * 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PayloadKind {
    #[default]
    None,
    Float32,
    Float64,
    Integer,
}
impl PayloadKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PayloadKind::None => "",
            PayloadKind::Float32 => "float32",
            PayloadKind::Float64 => "float64",
            PayloadKind::Integer => "integer",
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Code {
    pub value: String,
    pub scheme: String,
    pub meaning: String,
}
impl Code {
    pub fn new(value: &str, scheme: &str, meaning: &str) -> Code {
        Code {
            value: value.into(),
            scheme: scheme.into(),
            meaning: meaning.into(),
        }
    }
    pub fn string(&self) -> String {
        let meaning = trim_unicode_space(&self.meaning);
        if !meaning.is_empty() {
            return meaning.to_string();
        }
        if self.value.is_empty() {
            return String::new();
        }
        if self.scheme.is_empty() {
            return self.value.clone();
        }
        format!("{}:{}", self.scheme, self.value)
    }
}
pub(crate) fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{85}' || c == '\u{A0}')
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub frames: Vec<i64>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mapping {
    pub first_value: f64,
    pub last_value: f64,
    pub has_range: bool,
    pub label: String,
    pub slope: f64,
    pub intercept: f64,
    pub lut: Vec<f64>,
    pub units: Code,
    pub quantity: Code,
    pub(crate) units_version: String,
}
pub(crate) fn finite(v: f64) -> bool {
    v.is_finite()
}
pub(crate) fn positive_finite(v: f64) -> bool {
    v > 0.0 && finite(v)
}
pub(crate) fn finite_vec(v: Vec3) -> bool {
    finite(v.x) && finite(v.y) && finite(v.z)
}
fn g8(v: f64) -> String {
    format_float(v, 'g', 8)
}
impl Mapping {
    pub fn apply(&self, stored: f64) -> Result<f64, Error> {
        if stored.is_nan() {
            return Ok(GO_NAN);
        }
        if self.has_range
            && finite(stored)
            && (stored < self.first_value || stored > self.last_value)
        {
            return Err(Error::sentinel(Kind::OutsideMapping));
        }
        let value;
        if !self.lut.is_empty() {
            let Some(index) =
                nearest_index_inside(stored - self.first_value, self.lut.len() as i64)
            else {
                return Err(Error::wrap(
                    Kind::MissingMapping,
                    format!(
                        "stored value {} outside mapping [{}, {}]",
                        g8(stored),
                        g8(self.first_value),
                        g8(self.last_value)
                    ),
                ));
            };
            value = self.lut[index as usize];
        } else {
            value = stored * self.slope + self.intercept;
            if stored.is_infinite() {
                return Ok(value);
            }
        }
        if !finite(value) {
            let detail = format!("mapped value {}", g8(value));
            if self.lut.is_empty() && finite(self.slope) && finite(self.intercept) {
                return Err(Error::wrap(Kind::MappingOverflow, detail));
            }
            return Err(Error::wrap(Kind::NonFinite, detail));
        }
        Ok(value)
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Padding {
    pub value: f64,
    pub range_limit: f64,
}
impl Padding {
    pub fn contains(&self, stored: f64) -> bool {
        if self.value.is_nan() {
            return stored.is_nan();
        }
        stored >= min_nan(self.value, self.range_limit)
            && stored <= max_nan(self.value, self.range_limit)
    }
}
fn min_nan(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    fmath::min(a, b)
}
fn max_nan(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    fmath::max(a, b)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VoxelCounts {
    pub valid: i64,
    pub padding: i64,
    pub nan: i64,
    pub infinite: i64,
    pub outside_mapping: i64,
    pub mapping_overflow: i64,
}
impl VoxelCounts {
    pub fn unmeasured(&self) -> i64 {
        self.padding + self.nan + self.infinite + self.outside_mapping + self.mapping_overflow
    }
    pub fn add(&mut self, other: VoxelCounts) {
        self.valid += other.valid;
        self.padding += other.padding;
        self.nan += other.nan;
        self.infinite += other.infinite;
        self.outside_mapping += other.outside_mapping;
        self.mapping_overflow += other.mapping_overflow;
    }
}
pub fn measured(value: f64) -> bool {
    finite(value)
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub geometry: SliceGeometry,
    pub dimension_index_values: Option<Vec<i64>>,
    pub mapping: Mapping,
    pub additional_mappings: Vec<Mapping>,
}
#[derive(Debug, Default)]
struct Cache {
    frames: HashMap<i64, Vec<f64>>,
    order: Vec<i64>,
    bytes: i64,
    limit: i64,
}
impl Cache {
    fn touch(&mut self, index: i64) {
        if let Some(i) = self.order.iter().position(|&v| v == index) {
            self.order.remove(i);
            self.order.push(index);
        }
    }
    fn trim(&mut self) {
        while self.bytes > self.limit && !self.order.is_empty() {
            self.evict_oldest();
        }
    }
    fn evict_oldest(&mut self) {
        let index = self.order.remove(0);
        if let Some(values) = self.frames.remove(&index) {
            self.bytes -= values.len() as i64 * 8;
        }
    }
}
#[derive(Debug, Default)]
pub struct Map {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub rows: i64,
    pub columns: i64,
    pub number_of_frames: i64,
    pub payload: PayloadKind,
    pub frames: Vec<Frame>,
    pub references: Vec<Reference>,
    pub units: Code,
    pub quantity: Code,
    pub padding: Option<Padding>,
    big_endian: bool,
    float_raw: Vec<u8>,
    native: Option<NativeFrames>,
    cache: Mutex<Cache>,
    payload_retained: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadOptions {
    pub mapping_label: String,
}
pub fn read(file: &File) -> Result<Map, Error> {
    read_with_options(file, &ReadOptions::default())
}
pub fn read_with_options(file: &File, options: &ReadOptions) -> Result<Map, Error> {
    read_dataset_impl(
        &file.dataset,
        true,
        options,
        file.incomplete_pixel_data.is_some(),
    )
}
pub fn read_dataset(obj: &Object) -> Result<Map, Error> {
    read_dataset_impl(obj, true, &ReadOptions::default(), false)
}
pub fn read_dataset_with_options(obj: &Object, options: &ReadOptions) -> Result<Map, Error> {
    read_dataset_impl(obj, true, options, false)
}
pub fn read_metadata(file: &File) -> Result<Map, Error> {
    read_metadata_with_options(file, &ReadOptions::default())
}
pub fn read_metadata_with_options(file: &File, options: &ReadOptions) -> Result<Map, Error> {
    read_dataset_impl(&file.dataset, false, options, false)
}
fn metadata_plus_v(m: &Metadata) -> String {
    format!(
        "{{Rows:{} Columns:{} SamplesPerPixel:{} BitsAllocated:{} BitsStored:{} HighBit:{} PixelRepresentation:{} PlanarConfiguration:{} PlanarConfigurationPresent:{} NumberOfFrames:{} PhotometricInterpretation:{}}}",
        m.rows,
        m.columns,
        m.samples_per_pixel,
        m.bits_allocated,
        m.bits_stored,
        m.high_bit,
        m.pixel_representation,
        m.planar_configuration,
        m.planar_configuration_present,
        m.number_of_frames,
        m.photometric_interpretation
    )
}
fn read_dataset_impl(
    obj: &Object,
    retain_payload: bool,
    options: &ReadOptions,
    incomplete_pixel_data: bool,
) -> Result<Map, Error> {
    let sop = dio::clean_uid(obj, dio::TAG_SOP_CLASS_UID);
    if sop != PARAMETRIC_MAP_STORAGE {
        return Err(Error::wrap(Kind::UnsupportedSopClass, sop));
    }
    let rows = dio::int(obj, dio::TAG_ROWS);
    let columns = dio::int(obj, dio::TAG_COLUMNS);
    let mut frames = dio::int(obj, dio::TAG_NUMBER_OF_FRAMES);
    if frames == 0 {
        frames = 1;
    }
    if rows <= 0 || columns <= 0 || frames <= 0 {
        return Err(Error::wrap(
            Kind::InvalidObject,
            format!("invalid dimensions {columns}x{rows}x{frames}"),
        ));
    }
    let voxels = match checked_product(&[rows, columns, frames]) {
        Some(v) if v <= MAX_VOXELS => v,
        _ => {
            return Err(Error::wrap(
                Kind::MemoryLimit,
                format!("{columns}x{rows}x{frames} samples"),
            ));
        }
    };
    let mut out = Map {
        sop_class_uid: PARAMETRIC_MAP_STORAGE.to_string(),
        sop_instance_uid: dio::clean_uid(obj, dio::TAG_SOP_INSTANCE_UID),
        study_instance_uid: dio::clean_uid(obj, dio::TAG_STUDY_INSTANCE_UID),
        series_instance_uid: dio::clean_uid(obj, dio::TAG_SERIES_INSTANCE_UID),
        frame_of_reference_uid: dio::clean_uid(obj, dio::TAG_FRAME_OF_REFERENCE_UID),
        rows,
        columns,
        number_of_frames: frames,
        big_endian: obj.value_big_endian(),
        cache: Mutex::new(Cache {
            limit: 32 << 20,
            ..Cache::default()
        }),
        payload_retained: retain_payload,
        ..Map::default()
    };
    if out.sop_instance_uid.is_empty()
        || out.study_instance_uid.is_empty()
        || out.series_instance_uid.is_empty()
        || out.frame_of_reference_uid.is_empty()
    {
        return Err(Error::wrap(
            Kind::InvalidObject,
            "missing identity or FrameOfReferenceUID",
        ));
    }
    if let Some(element) = obj.get(TAG_FLOAT_PIXEL_DATA) {
        let length = i64::from(element_length(element));
        if length != voxels * 4 {
            return Err(Error::wrap(
                Kind::InvalidObject,
                format!("Float Pixel Data length {length}, want {}", voxels * 4),
            ));
        }
        out.payload = PayloadKind::Float32;
        if retain_payload {
            out.float_raw = read_pixel_payload(element, length)?;
        }
    } else if let Some(element) = obj.get(TAG_DOUBLE_FLOAT_PIXEL_DATA) {
        let length = i64::from(element_length(element));
        if length != voxels * 8 {
            return Err(Error::wrap(
                Kind::InvalidObject,
                format!(
                    "Double Float Pixel Data length {length}, want {}",
                    voxels * 8
                ),
            ));
        }
        out.payload = PayloadKind::Float64;
        if retain_payload {
            out.float_raw = read_pixel_payload(element, length)?;
        }
    } else {
        let metadata = pixeldata::extract_metadata(obj).map_err(|e| {
            Error::wrap(
                Kind::InvalidObject,
                format!("integer Pixel Data metadata: {e}"),
            )
        })?;
        if metadata.samples_per_pixel != 1
            || metadata.pixel_representation > 1
            || !matches!(metadata.bits_allocated, 8 | 16 | 32)
        {
            return Err(Error::wrap(
                Kind::InvalidObject,
                format!(
                    "unsupported integer pixel metadata {}",
                    metadata_plus_v(&metadata)
                ),
            ));
        }
        if metadata.bits_stored == 0
            || metadata.bits_stored > metadata.bits_allocated
            || metadata.high_bit >= metadata.bits_allocated
            || i64::from(metadata.high_bit) + 1 < i64::from(metadata.bits_stored)
        {
            return Err(Error::wrap(
                Kind::InvalidObject,
                format!(
                    "unsupported integer bit layout {}",
                    metadata_plus_v(&metadata)
                ),
            ));
        }
        let Some(element) = obj.get(dio::TAG_PIXEL_DATA) else {
            return Err(Error::wrap(
                Kind::InvalidObject,
                "integer Pixel Data is missing",
            ));
        };
        let frame_bytes = rows * columns * i64::from(metadata.bits_allocated / 8);
        let want_bytes = frame_bytes * frames;
        let length = i64::from(element_length(element));
        if length != want_bytes && !(want_bytes % 2 == 1 && length == want_bytes + 1) {
            return Err(Error::wrap(
                Kind::InvalidObject,
                format!("integer Pixel Data length {length}, want {want_bytes}"),
            ));
        }
        out.payload = PayloadKind::Integer;
        out.padding = read_integer_padding(obj, sample_format(&metadata, out.big_endian))?;
        if retain_payload {
            if incomplete_pixel_data {
                return Err(Error::other(
                    "integer Pixel Data: dicom: file is the result of a partial read",
                )
                .under(Kind::InvalidObject));
            }
            let native = pixeldata::extract_native_frames(obj).map_err(|e| {
                Error::other(format!("integer Pixel Data: {e}")).under(Kind::InvalidObject)
            })?;
            if native.data.len() as i64 != frames {
                return Err(Error::wrap(
                    Kind::InvalidObject,
                    format!("{} encoded frames for {frames} frames", native.data.len()),
                ));
            }
            for (index, frame) in native.data.iter().enumerate() {
                if frame.len() as i64 != frame_bytes {
                    return Err(Error::wrap(
                        Kind::InvalidObject,
                        format!(
                            "frame {} has {} bytes, want {frame_bytes}",
                            index + 1,
                            frame.len()
                        ),
                    ));
                }
            }
            out.native = Some(native);
        }
    }
    if out.payload != PayloadKind::Integer {
        out.padding = read_float_padding(obj, out.payload)?;
    }
    let per_frame = dio::sequence(obj, TAG_PER_FRAME_FUNCTIONAL_GROUPS);
    if !per_frame.is_empty() && per_frame.len() as i64 != frames {
        return Err(Error::wrap(
            Kind::InvalidObject,
            format!("{} per-frame groups for {frames} frames", per_frame.len()),
        ));
    }
    out.frames = vec![Frame::default(); frames as usize];
    mappings::read_frame_mappings(obj, &per_frame, &mut out, options)?;
    for index in 0..frames {
        let geometry = frame_geometry(&obj.frame_geometry_at(index), rows, columns)
            .map_err(|e| e.prefixed(format!("frame {}", index + 1)))?;
        out.frames[index as usize].geometry = geometry;
    }
    read_dimensions(obj, &per_frame, &mut out.frames)?;
    let cos = GeometryTolerances::default_tolerances().orientation_cos;
    for index in 1..out.frames.len() {
        if out.frames[index]
            .geometry
            .row_dir
            .dot(out.frames[0].geometry.row_dir)
            < cos
            || out.frames[index]
                .geometry
                .col_dir
                .dot(out.frames[0].geometry.col_dir)
                < cos
        {
            return Err(Error::wrap(
                Kind::UnsupportedDimensions,
                "frame orientation varies",
            ));
        }
    }
    out.references = read_references(obj)?;
    Ok(out)
}
struct IntegerPadding {
    value: i64,
    range_limit: i64,
    has_range: bool,
}
fn pixel_padding_from_object(
    obj: &Object,
    format: SampleFormat,
) -> Result<Option<IntegerPadding>, Error> {
    let (lo, hi) = if format.signed {
        (
            -(1i64 << (format.bits_stored - 1)),
            (1i64 << (format.bits_stored - 1)) - 1,
        )
    } else {
        (0, (1i64 << format.bits_stored) - 1)
    };
    let Some(value) = padding_value(obj, TAG_INTEGER_PIXEL_PADDING_VALUE, format.signed)? else {
        return Ok(None);
    };
    let mut padding = IntegerPadding {
        value,
        range_limit: 0,
        has_range: false,
    };
    if let Some(limit) = padding_value(obj, TAG_INTEGER_PIXEL_PADDING_RANGE_LIMIT, format.signed)? {
        padding.range_limit = limit;
        padding.has_range = true;
    }
    for v in [padding.value, padding.range_limit] {
        if v < lo || v > hi {
            return Err(Error::wrap(
                Kind::InvalidPixelPadding,
                format!(
                    "value outside the {}-bit {} range",
                    format.bits_stored,
                    if format.signed { "signed" } else { "unsigned" }
                ),
            ));
        }
    }
    Ok(Some(padding))
}
fn padding_value(obj: &Object, tag: Tag, signed: bool) -> Result<Option<i64>, Error> {
    let Some(elem) = obj.get(tag) else {
        return Ok(None);
    };
    match elem.vr() {
        Vr::US => {
            if signed {
                return Err(Error::wrap(
                    Kind::InvalidPixelPadding,
                    format!("{tag} is US for signed samples"),
                ));
            }
        }
        Vr::SS => {
            if !signed {
                return Err(Error::wrap(
                    Kind::InvalidPixelPadding,
                    format!("{tag} is SS for unsigned samples"),
                ));
            }
        }
        Vr::UN => {}
        vr => {
            return Err(Error::wrap(
                Kind::InvalidPixelPadding,
                format!("{tag} has VR {vr}"),
            ));
        }
    }
    if let Some(raw) = elem.raw()
        && raw.len() == 2
    {
        let v = if obj.value_big_endian() {
            u16::from_be_bytes([raw[0], raw[1]])
        } else {
            u16::from_le_bytes([raw[0], raw[1]])
        };
        return Ok(Some(if signed {
            i64::from(v as i16)
        } else {
            i64::from(v)
        }));
    }
    Err(Error::wrap(
        Kind::InvalidPixelPadding,
        format!("{tag} must hold one 16-bit value"),
    ))
}
fn read_integer_padding(obj: &Object, format: SampleFormat) -> Result<Option<Padding>, Error> {
    let padding =
        pixel_padding_from_object(obj, format).map_err(|e| e.under(Kind::InvalidPadding))?;
    let Some(padding) = padding else {
        if obj.has(TAG_INTEGER_PIXEL_PADDING_RANGE_LIMIT) {
            return Err(Error::wrap(
                Kind::InvalidPadding,
                format!(
                    "{TAG_INTEGER_PIXEL_PADDING_RANGE_LIMIT} present without {TAG_INTEGER_PIXEL_PADDING_VALUE}"
                ),
            ));
        }
        return Ok(None);
    };
    let limit = if padding.has_range {
        padding.range_limit
    } else {
        padding.value
    };
    Ok(Some(Padding {
        value: padding.value as f64,
        range_limit: limit as f64,
    }))
}
fn read_float_padding(obj: &Object, payload: PayloadKind) -> Result<Option<Padding>, Error> {
    let (value_tag, limit_tag, vr) = match payload {
        PayloadKind::Float32 => (
            TAG_FLOAT_PIXEL_PADDING_VALUE,
            TAG_FLOAT_PIXEL_PADDING_RANGE_LIMIT,
            Vr::FL,
        ),
        PayloadKind::Float64 => (
            TAG_DOUBLE_FLOAT_PIXEL_PADDING_VALUE,
            TAG_DOUBLE_FLOAT_PIXEL_PADDING_RANGE_LIMIT,
            Vr::FD,
        ),
        _ => return Ok(None),
    };
    let value = padding_float(obj, value_tag, vr)?;
    let limit = padding_float(obj, limit_tag, vr)?;
    match (value, limit) {
        (None, None) => Ok(None),
        (None, Some(_)) => Err(Error::wrap(
            Kind::InvalidPadding,
            format!("{limit_tag} present without {value_tag}"),
        )),
        (Some(_), None) => Err(Error::wrap(
            Kind::InvalidPadding,
            format!("{value_tag} present without the Type 1C {limit_tag}"),
        )),
        (Some(value), Some(limit)) => {
            if value.is_nan() != limit.is_nan() {
                return Err(Error::wrap(
                    Kind::InvalidPadding,
                    format!("NaN {value_tag} paired with a number"),
                ));
            }
            Ok(Some(Padding {
                value,
                range_limit: limit,
            }))
        }
    }
}
fn padding_float(obj: &Object, tag: Tag, vr: Vr) -> Result<Option<f64>, Error> {
    let Some(element) = obj.get(tag) else {
        return Ok(None);
    };
    if element.vr() != vr {
        return Err(Error::wrap(
            Kind::InvalidPadding,
            format!("{tag} has VR {}, want {vr}", element.vr()),
        ));
    }
    match dio::lookup_floats(Some(obj), tag) {
        Ok(values) if values.len() == 1 => Ok(Some(values[0])),
        _ => Err(Error::wrap(
            Kind::InvalidPadding,
            format!("{tag} must hold one value"),
        )),
    }
}
fn frame_geometry(g: &FrameGeometry, rows: i64, columns: i64) -> Result<SliceGeometry, Error> {
    let (Some(ipp), Some(iop), Some(ps)) = (
        g.image_position_patient.as_deref(),
        g.image_orientation_patient.as_deref(),
        g.pixel_spacing.as_deref(),
    ) else {
        return Err(Error::wrap(
            Kind::GeometryMismatch,
            "incomplete patient geometry",
        ));
    };
    if ipp.len() != 3
        || iop.len() != 6
        || ps.len() != 2
        || !positive_finite(ps[0])
        || !positive_finite(ps[1])
    {
        return Err(Error::wrap(
            Kind::GeometryMismatch,
            "incomplete patient geometry",
        ));
    }
    let row_raw = Vec3::new(iop[0], iop[1], iop[2]);
    let col_raw = Vec3::new(iop[3], iop[4], iop[5]);
    let origin = Vec3::new(ipp[0], ipp[1], ipp[2]);
    if !finite_vec(origin)
        || !finite_vec(row_raw)
        || !finite_vec(col_raw)
        || (row_raw.length() - 1.0).abs() > 1e-4
        || (col_raw.length() - 1.0).abs() > 1e-4
        || row_raw.dot(col_raw).abs() > 1e-4
    {
        return Err(Error::wrap(Kind::GeometryMismatch, "invalid orientation"));
    }
    let normal = row_raw.cross(col_raw).normalize();
    if normal == Vec3::ZERO {
        return Err(Error::wrap(
            Kind::GeometryMismatch,
            "degenerate orientation",
        ));
    }
    Ok(SliceGeometry {
        origin,
        row_dir: row_raw,
        col_dir: col_raw,
        normal,
        row_spacing: ps[0],
        col_spacing: ps[1],
        rows,
        columns,
    })
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Statistics {
    pub count: i64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std_dev: f64,
    pub unmeasured: i64,
}
impl Map {
    fn frame_index_error(index: i64) -> Error {
        Error::wrap(Kind::InvalidObject, format!("frame {index}"))
    }
    pub fn frame_values(&self, index: i64) -> Result<Vec<f64>, Error> {
        if index < 0 || index >= self.number_of_frames {
            return Err(Map::frame_index_error(index));
        }
        if !self.payload_retained {
            return Err(Error::sentinel(Kind::PayloadUnavailable));
        }
        {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(values) = cache.frames.get(&index) {
                let out = values.clone();
                cache.touch(index);
                return Ok(out);
            }
        }
        let mut values = vec![0.0; (self.rows * self.columns) as usize];
        self.frame_values_into(index, &mut values)?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = cache.frames.get(&index) {
            return Ok(existing.clone());
        }
        cache.bytes += values.len() as i64 * 8;
        cache.frames.insert(index, values.clone());
        cache.order.push(index);
        cache.trim();
        Ok(values)
    }
    pub fn frame_values_into(&self, index: i64, destination: &mut [f64]) -> Result<(), Error> {
        self.decode_frame_into(index, destination, None).map(|_| ())
    }
    pub fn decode_frame_into(
        &self,
        index: i64,
        values: &mut [f64],
        mut padding: Option<&mut [bool]>,
    ) -> Result<VoxelCounts, Error> {
        if index < 0 || index >= self.number_of_frames {
            return Err(Map::frame_index_error(index));
        }
        if !self.payload_retained {
            return Err(Error::sentinel(Kind::PayloadUnavailable));
        }
        let count = (self.rows * self.columns) as usize;
        if values.len() < count {
            return Err(Error::wrap(
                Kind::InvalidObject,
                format!("destination has {} values, want {count}", values.len()),
            ));
        }
        if let Some(p) = padding.as_deref()
            && p.len() < count
        {
            return Err(Error::wrap(
                Kind::InvalidObject,
                format!("padding mask has {} entries, want {count}", p.len()),
            ));
        }
        let frame = &self.frames[index as usize];
        let mut counts = VoxelCounts::default();
        for pixel in 0..count {
            let stored = self.stored_value(index as usize, pixel)?;
            let is_padding = self.padding.is_some_and(|p| p.contains(stored));
            if let Some(p) = padding.as_deref_mut() {
                p[pixel] = is_padding;
            }
            if is_padding {
                values[pixel] = GO_NAN;
                counts.padding += 1;
                continue;
            }
            if stored.is_nan() {
                values[pixel] = GO_NAN;
                counts.nan += 1;
                continue;
            }
            match frame.apply_mapping(stored) {
                Err(e) => {
                    if e.is(Kind::OutsideMapping) {
                        counts.outside_mapping += 1;
                    } else if e.is(Kind::MappingOverflow) {
                        counts.mapping_overflow += 1;
                    } else {
                        return Err(e.prefixed(format!("frame {} pixel {pixel}", index + 1)));
                    }
                    values[pixel] = GO_NAN;
                }
                Ok(value) => {
                    values[pixel] = value;
                    if stored.is_infinite() {
                        counts.infinite += 1;
                    } else {
                        counts.valid += 1;
                    }
                }
            }
        }
        Ok(counts)
    }
    fn stored_value(&self, frame: usize, pixel: usize) -> Result<f64, Error> {
        let count = (self.rows * self.columns) as usize;
        match self.payload {
            PayloadKind::Float32 => {
                let o = (frame * count + pixel) * 4;
                let b = [
                    self.float_raw[o],
                    self.float_raw[o + 1],
                    self.float_raw[o + 2],
                    self.float_raw[o + 3],
                ];
                let bits = if self.big_endian {
                    u32::from_be_bytes(b)
                } else {
                    u32::from_le_bytes(b)
                };
                Ok(f64::from(f32::from_bits(bits)))
            }
            PayloadKind::Float64 => {
                let o = (frame * count + pixel) * 8;
                let mut b = [0u8; 8];
                b.copy_from_slice(&self.float_raw[o..o + 8]);
                let bits = if self.big_endian {
                    u64::from_be_bytes(b)
                } else {
                    u64::from_le_bytes(b)
                };
                Ok(f64::from_bits(bits))
            }
            PayloadKind::Integer => {
                let Some(native) = &self.native else {
                    return Err(Error::sentinel(Kind::InvalidObject));
                };
                let meta = &native.metadata;
                let width = usize::from(meta.bits_allocated / 8);
                let raw = &native.data[frame][pixel * width..];
                let mut unsigned: u64 = match width {
                    1 => u64::from(raw[0]),
                    2 => {
                        let b = [raw[0], raw[1]];
                        u64::from(if self.big_endian {
                            u16::from_be_bytes(b)
                        } else {
                            u16::from_le_bytes(b)
                        })
                    }
                    4 => {
                        let b = [raw[0], raw[1], raw[2], raw[3]];
                        u64::from(if self.big_endian {
                            u32::from_be_bytes(b)
                        } else {
                            u32::from_le_bytes(b)
                        })
                    }
                    _ => {
                        return Err(Error::wrap(
                            Kind::InvalidObject,
                            format!("integer width {width}"),
                        ));
                    }
                };
                let shift = meta.high_bit + 1 - meta.bits_stored;
                unsigned >>= shift;
                let mask = (1u64 << meta.bits_stored) - 1;
                unsigned &= mask;
                if meta.pixel_representation == 1
                    && unsigned & (1u64 << (meta.bits_stored - 1)) != 0
                {
                    return Ok((unsigned | !mask) as i64 as f64);
                }
                Ok(unsigned as f64)
            }
            PayloadKind::None => Err(Error::sentinel(Kind::InvalidObject)),
        }
    }
    fn sorted_positions(&self) -> Vec<f64> {
        let mut positions: Vec<f64> = self
            .frames
            .iter()
            .map(|f| f.geometry.position_along(f.geometry.normal))
            .collect();
        positions.sort_by(f64::total_cmp);
        positions
    }
    pub fn sample_patient(&self, point: Vec3) -> Result<Option<f64>, Error> {
        if !finite_vec(point) {
            return Err(Error::sentinel(Kind::InvalidObject));
        }
        let (mut best, mut distance) = (-1i64, f64::INFINITY);
        for (index, frame) in self.frames.iter().enumerate() {
            let d = point
                .sub(frame.geometry.origin)
                .dot(frame.geometry.normal)
                .abs();
            if d < distance {
                best = index as i64;
                distance = d;
            }
        }
        let mut tolerance = 0.5;
        if self.frames.len() > 1 {
            let positions = self.sorted_positions();
            let mut min_spacing = f64::INFINITY;
            for index in 1..positions.len() {
                let spacing = positions[index] - positions[index - 1];
                if spacing > 0.0 && spacing < min_spacing {
                    min_spacing = spacing;
                }
            }
            if finite(min_spacing) {
                tolerance = min_spacing / 2.0 + 1e-5;
            }
        }
        if best < 0 || distance > tolerance {
            return Ok(None);
        }
        let g = self.frames[best as usize].geometry;
        let delta = point.sub(g.origin);
        let column =
            clamp_continuous_index(delta.dot(g.row_dir) / g.col_spacing, self.columns, 0.0);
        let row = clamp_continuous_index(delta.dot(g.col_dir) / g.row_spacing, self.rows, 0.0);
        let (Some(column), Some(row)) = (column, row) else {
            return Ok(None);
        };
        let values = self.frame_values(best)?;
        Ok(bilinear(&values, self.columns, self.rows, column, row))
    }
    pub fn resample(&self, target: &SliceGeometry) -> Result<(Vec<f64>, Vec<bool>), Error> {
        if target.rows <= 0
            || target.columns <= 0
            || !finite_vec(target.origin)
            || !finite_vec(target.row_dir)
            || !finite_vec(target.col_dir)
            || !positive_finite(target.row_spacing)
            || !positive_finite(target.col_spacing)
            || (target.row_dir.length() - 1.0).abs() > 1e-4
            || (target.col_dir.length() - 1.0).abs() > 1e-4
            || target.row_dir.dot(target.col_dir).abs() > 1e-4
        {
            return Err(Error::wrap(
                Kind::GeometryMismatch,
                "invalid target geometry",
            ));
        }
        let cos = GeometryTolerances::default_tolerances().orientation_cos;
        let (mut best, mut distance) = (-1i64, f64::INFINITY);
        for (index, frame) in self.frames.iter().enumerate() {
            let source = &frame.geometry;
            if source.row_dir.dot(target.row_dir).abs() < cos
                || source.col_dir.dot(target.col_dir).abs() < cos
            {
                continue;
            }
            let d = target.origin.sub(source.origin).dot(source.normal).abs();
            if d < distance {
                best = index as i64;
                distance = d;
            }
        }
        let mut tolerance = 0.5;
        if self.frames.len() > 1 {
            let positions = self.sorted_positions();
            for index in 1..positions.len() {
                let spacing = positions[index] - positions[index - 1];
                if spacing > 0.0 && (tolerance == 0.5 || spacing / 2.0 < tolerance) {
                    tolerance = spacing / 2.0 + 1e-5;
                }
            }
        }
        if best < 0 || distance > tolerance {
            return Err(Error::wrap(
                Kind::GeometryMismatch,
                "target plane does not overlap the map",
            ));
        }
        let source = self.frames[best as usize].geometry;
        let source_values = self.frame_values(best)?;
        let count = match checked_product(&[target.rows, target.columns]) {
            Some(c) if c <= MAX_VOXELS => c as usize,
            _ => return Err(Error::sentinel(Kind::MemoryLimit)),
        };
        let mut values = vec![0.0; count];
        let mut valid = vec![false; count];
        for row in 0..target.rows {
            for column in 0..target.columns {
                let point = target
                    .origin
                    .add(target.row_dir.scale(column as f64 * target.col_spacing))
                    .add(target.col_dir.scale(row as f64 * target.row_spacing));
                let delta = point.sub(source.origin);
                let x = clamp_continuous_index(
                    delta.dot(source.row_dir) / source.col_spacing,
                    self.columns,
                    0.0,
                );
                let y = clamp_continuous_index(
                    delta.dot(source.col_dir) / source.row_spacing,
                    self.rows,
                    0.0,
                );
                let (Some(x), Some(y)) = (x, y) else {
                    continue;
                };
                let Some(value) = bilinear(&source_values, self.columns, self.rows, x, y) else {
                    continue;
                };
                let index = (row * target.columns + column) as usize;
                values[index] = value;
                valid[index] = true;
            }
        }
        Ok((values, valid))
    }
    pub fn mask_statistics(
        &self,
        frame: i64,
        mask: Option<&RasterMask>,
    ) -> Result<Statistics, Error> {
        let Some(mask) = mask.filter(|m| m.rows == self.rows && m.columns == self.columns) else {
            return Err(Error::wrap(Kind::GeometryMismatch, "ROI mask dimensions"));
        };
        let values = self.frame_values(frame)?;
        let mut unmeasured = 0;
        let stats = stats_2d(mask, |x, y| {
            let value = values[(y * self.columns + x) as usize];
            if !finite(value) {
                unmeasured += 1;
                return None;
            }
            Some(value)
        });
        Ok(Statistics {
            count: stats.count,
            min: stats.min,
            max: stats.max,
            mean: stats.mean,
            std_dev: stats.std_dev,
            unmeasured,
        })
    }
    pub fn set_cache_limit_bytes(&self, limit: i64) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.limit = limit.max(0);
        cache.trim();
    }
    pub fn cached_bytes(&self) -> i64 {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).bytes
    }
    pub fn evict_bytes(&self, target: i64) -> i64 {
        if target <= 0 {
            return 0;
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let before = cache.bytes;
        while cache.bytes > 0 && before - cache.bytes < target && !cache.order.is_empty() {
            cache.evict_oldest();
        }
        before - cache.bytes
    }
    pub fn window_domain(&self, frame: i64) -> Result<WindowDomain, Error> {
        if !self.payload_retained
            || frame < 0
            || frame >= self.frames.len() as i64
            || self.rows <= 0
            || self.columns <= 0
        {
            return Err(Error::wrap(Kind::InvalidObject, "window domain frame"));
        }
        let count = self.rows * self.columns;
        let n = count.min(4096);
        let mut values = Vec::with_capacity(n as usize);
        let f = &self.frames[frame as usize];
        for sample in 0..n {
            let mut pixel = 0;
            if n > 1 {
                pixel =
                    sample * ((count - 1) / (n - 1)) + sample * ((count - 1) % (n - 1)) / (n - 1);
            }
            let stored = self.stored_value(frame as usize, pixel as usize)?;
            if !measured(stored) || self.padding.is_some_and(|p| p.contains(stored)) {
                continue;
            }
            match f.apply_mapping(stored) {
                Ok(value) => values.push(value),
                Err(e) => {
                    if e.is(Kind::OutsideMapping) || e.is(Kind::MappingOverflow) {
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        Ok(WindowDomain::measured(&values, &f.mapping.units.string()))
    }
    #[doc(hidden)]
    pub fn payload_retained(&self) -> bool {
        self.payload_retained
    }
}
fn bilinear(values: &[f64], columns: i64, rows: i64, x: f64, y: f64) -> Option<f64> {
    let (x0, y0) = (fmath::f64_to_i64(x.floor()), fmath::f64_to_i64(y.floor()));
    let (mut x1, mut y1) = ((x0 + 1).min(columns - 1), (y0 + 1).min(rows - 1));
    let (fx, fy) = (x - x0 as f64, y - y0 as f64);
    if fx == 0.0 {
        x1 = x0;
    }
    if fy == 0.0 {
        y1 = y0;
    }
    let at = |yy: i64, xx: i64| values[(yy * columns + xx) as usize];
    let (a, b, c, d) = (at(y0, x0), at(y0, x1), at(y1, x0), at(y1, x1));
    if !finite(a) || !finite(b) || !finite(c) || !finite(d) {
        return None;
    }
    let top = interpolate_finite(a, b, fx);
    let bottom = interpolate_finite(c, d, fx);
    Some(interpolate_finite(top, bottom, fy))
}
fn interpolate_finite(a: f64, b: f64, weight: f64) -> f64 {
    let delta = b - a;
    if delta.is_infinite() {
        return (1.0 - weight) * a + weight * b;
    }
    a + delta * weight
}
fn first_item(obj: Option<&Object>, tag: Tag) -> Option<Object> {
    obj.and_then(|o| o.sequence_item_at(tag, 0))
}
fn read_tag(obj: &Object, tag: Tag) -> Option<Tag> {
    let raw = obj.get(tag)?.raw()?;
    if raw.len() < 4 {
        return None;
    }
    let u = |i: usize| {
        if obj.value_big_endian() {
            u16::from_be_bytes([raw[i], raw[i + 1]])
        } else {
            u16::from_le_bytes([raw[i], raw[i + 1]])
        }
    };
    Some(Tag::new(u(0), u(2)))
}
fn read_dimensions(obj: &Object, per_frame: &[Object], frames: &mut [Frame]) -> Result<(), Error> {
    let dimensions = dio::sequence(obj, TAG_DIMENSION_INDEX_SEQ);
    if dimensions.is_empty() {
        return Ok(());
    }
    let mut pointers = Vec::with_capacity(dimensions.len());
    for item in &dimensions {
        let Some(tag) = read_tag(item, TAG_DIMENSION_INDEX_POINTER) else {
            return Err(Error::wrap(
                Kind::UnsupportedDimensions,
                "invalid DimensionIndexPointer",
            ));
        };
        pointers.push(tag);
    }
    for index in 0..frames.len() {
        if index >= per_frame.len() {
            return Err(Error::wrap(
                Kind::UnsupportedDimensions,
                "dimensions require per-frame groups",
            ));
        }
        let content = first_item(Some(&per_frame[index]), TAG_FRAME_CONTENT_SEQ);
        let values = dio::ints(content.as_ref(), TAG_DIMENSION_INDEX_VALUES).unwrap_or_default();
        if values.len() != pointers.len() {
            return Err(Error::wrap(
                Kind::UnsupportedDimensions,
                format!("frame {} dimension value count", index + 1),
            ));
        }
        frames[index].dimension_index_values = Some(values);
    }
    for (dimension, pointer) in pointers.iter().enumerate() {
        if *pointer == TAG_IMAGE_POSITION_PATIENT || *pointer == TAG_IN_STACK_POSITION_NUMBER {
            continue;
        }
        let value_at = |f: &Frame| f.dimension_index_values.as_ref().map(|v| v[dimension]);
        let first = value_at(&frames[0]);
        if frames[1..].iter().any(|f| value_at(f) != first) {
            return Err(Error::wrap(
                Kind::UnsupportedDimensions,
                format!("varying dimension {pointer}"),
            ));
        }
    }
    Ok(())
}
fn sprint_ints(values: &[i64]) -> String {
    let parts: Vec<String> = values.iter().map(i64::to_string).collect();
    format!("[{}]", parts.join(" "))
}
fn read_references(obj: &Object) -> Result<Vec<Reference>, Error> {
    fn walk(current: &Object, seen: &mut HashMap<String, Reference>, failed: &mut bool) {
        let uid = dio::clean_uid(current, TAG_REFERENCED_SOP_INSTANCE_UID);
        if !uid.is_empty() {
            let mut reference = Reference {
                sop_class_uid: dio::clean_uid(current, TAG_REFERENCED_SOP_CLASS_UID),
                sop_instance_uid: uid.clone(),
                frames: Vec::new(),
            };
            if current.has(TAG_REFERENCED_FRAME_NUMBER) {
                let parts = current
                    .get_strings(TAG_REFERENCED_FRAME_NUMBER)
                    .unwrap_or_default();
                if parts.is_empty() {
                    *failed = true;
                    return;
                }
                for part in &parts {
                    match numtext::parse_int(trim_unicode_space(part)) {
                        Ok(n) if n > 0 => reference.frames.push(n),
                        _ => {
                            *failed = true;
                            return;
                        }
                    }
                }
            }
            let key = format!("{uid}|{}", sprint_ints(&reference.frames));
            seen.insert(key, reference);
        }
        for element in current.elements() {
            if element.vr() != Vr::SQ {
                continue;
            }
            if let Some(items) = current.get_sequence(element.tag()) {
                for item in &items {
                    walk(item, seen, failed);
                }
            }
        }
    }
    let mut seen = HashMap::new();
    let mut failed = false;
    walk(obj, &mut seen, &mut failed);
    if failed {
        return Err(Error::wrap(
            Kind::InvalidObject,
            "invalid referenced frame number",
        ));
    }
    let mut out: Vec<Reference> = seen.into_values().collect();
    out.sort_by(|a, b| {
        a.sop_instance_uid
            .cmp(&b.sop_instance_uid)
            .then_with(|| sprint_ints(&a.frames).cmp(&sprint_ints(&b.frames)))
    });
    Ok(out)
}
fn read_pixel_payload(element: &Element, length: i64) -> Result<Vec<u8>, Error> {
    match &element.value {
        Value::Raw(raw) => {
            if raw.len() as i64 != length {
                return Err(Error::wrap(
                    Kind::InvalidObject,
                    "inconsistent pixel payload length",
                ));
            }
            Ok(raw.clone())
        }
        _ => Err(Error::wrap(
            Kind::InvalidObject,
            format!("read deferred pixel payload: copied 0 of {length} bytes: <nil>"),
        )),
    }
}
pub(crate) fn read_code(item: Option<&Object>) -> Code {
    match item {
        None => Code::default(),
        Some(item) => Code {
            value: dio::clean_string(item, TAG_CODE_VALUE),
            scheme: dio::clean_string(item, TAG_CODING_SCHEME_DESIGNATOR),
            meaning: dio::clean_string(item, TAG_CODE_MEANING),
        },
    }
}
pub(crate) fn read_quantity(item: &Object) -> Code {
    for quantity_item in dio::sequence(item, TAG_QUANTITY_DEFINITION_SEQ) {
        for tag in [TAG_CONCEPT_CODE_SEQ, TAG_CONCEPT_NAME_CODE_SEQ] {
            let code = read_code(first_item(Some(&quantity_item), tag).as_ref());
            if !code.string().is_empty() {
                return code;
            }
        }
    }
    Code::default()
}
pub(crate) fn checked_product(values: &[i64]) -> Option<i64> {
    let mut product: i64 = 1;
    for &value in values {
        if value <= 0 || product > i64::MAX / value {
            return None;
        }
        product *= value;
    }
    Some(product)
}
