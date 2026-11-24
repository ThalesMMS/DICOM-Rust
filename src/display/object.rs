use super::lut::word_lut_depth;
use super::{
    DisplayError, Lut, LutOptions, ModalityLut, PaletteColorLut, PrintDensity, VoiFunction, VoiLut,
    valid_window_width,
};
use crate::object::{Object, Tag, Vr};
use crate::{fmath, numtext, textfmt};
use std::fmt;
const INVALID_DESCRIPTOR: &str = "dicom/display: invalid LUT descriptor";
const INVALID_DATA: &str = "dicom/display: LUT data does not match descriptor";
const UNSUPPORTED_SEGMENTED: &str = "dicom/display: unsupported segmented palette color LUT";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectErrorKind {
    InvalidLutDescriptor,
    InvalidLutData,
    UnsupportedSegmentedPalette,
    FunctionalGroupConflict,
    UnsupportedGsps,
    InvalidShutter,
    UnsupportedShutter,
    InvalidOverlay,
    InvalidCurve,
    UnsupportedTiff,
    Calibration,
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectError {
    pub kind: ObjectErrorKind,
    text: String,
}
impl ObjectError {
    pub(super) fn new(kind: ObjectErrorKind, text: impl Into<String>) -> ObjectError {
        ObjectError {
            kind,
            text: text.into(),
        }
    }
    fn from_display(err: DisplayError) -> ObjectError {
        let kind = match &err {
            DisplayError::InvalidLutData(_) | DisplayError::LegacyLut { data: true, .. } => {
                ObjectErrorKind::InvalidLutData
            }
            DisplayError::InvalidLutDescriptor(_) | DisplayError::LegacyLut { .. } => {
                ObjectErrorKind::InvalidLutDescriptor
            }
            DisplayError::Frame { .. } => ObjectErrorKind::Other,
        };
        ObjectError::new(kind, err.to_string())
    }
    fn invalid_descriptor() -> ObjectError {
        ObjectError::new(ObjectErrorKind::InvalidLutDescriptor, INVALID_DESCRIPTOR)
    }
    fn invalid_data() -> ObjectError {
        ObjectError::new(ObjectErrorKind::InvalidLutData, INVALID_DATA)
    }
    fn invalid_data_detail(detail: impl fmt::Display) -> ObjectError {
        ObjectError::new(
            ObjectErrorKind::InvalidLutData,
            format!("{INVALID_DATA}: {detail}"),
        )
    }
    pub fn join(errors: impl IntoIterator<Item = Option<ObjectError>>) -> Option<ObjectError> {
        let mut out: Option<ObjectError> = None;
        for err in errors.into_iter().flatten() {
            match &mut out {
                None => out = Some(err),
                Some(o) => {
                    o.text.push('\n');
                    o.text.push_str(&err.text);
                }
            }
        }
        out
    }
}
impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for ObjectError {}
pub(super) const TAG_RESCALE_INTERCEPT: Tag = Tag::new(0x0028, 0x1052);
pub(super) const TAG_RESCALE_SLOPE: Tag = Tag::new(0x0028, 0x1053);
const TAG_MODALITY_LUT_SEQUENCE: Tag = Tag::new(0x0028, 0x3000);
const TAG_LUT_DESCRIPTOR: Tag = Tag::new(0x0028, 0x3002);
const TAG_LUT_DATA: Tag = Tag::new(0x0028, 0x3006);
const TAG_WINDOW_CENTER: Tag = Tag::new(0x0028, 0x1050);
const TAG_WINDOW_WIDTH: Tag = Tag::new(0x0028, 0x1051);
const TAG_VOI_LUT_FUNCTION: Tag = Tag::new(0x0028, 0x1056);
const TAG_VOI_LUT_SEQUENCE: Tag = Tag::new(0x0028, 0x3010);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_PIXEL_VALUE_TRANSFORMATION_SEQUENCE: Tag = Tag::new(0x0028, 0x9145);
const TAG_FRAME_VOI_LUT_SEQUENCE: Tag = Tag::new(0x0028, 0x9132);
const TAG_WINDOW_EXPLANATION: Tag = Tag::new(0x0028, 0x1055);
const TAG_LUT_EXPLANATION: Tag = Tag::new(0x0028, 0x3003);
const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_PLANAR_CONFIGURATION: Tag = Tag::new(0x0028, 0x0006);
const TAG_PALETTE_DESCRIPTORS: [Tag; 3] = [
    Tag::new(0x0028, 0x1101),
    Tag::new(0x0028, 0x1102),
    Tag::new(0x0028, 0x1103),
];
const TAG_PALETTE_DATA: [Tag; 3] = [
    Tag::new(0x0028, 0x1201),
    Tag::new(0x0028, 0x1202),
    Tag::new(0x0028, 0x1203),
];
const TAG_SEGMENTED_PALETTE_DATA: [Tag; 3] = [
    Tag::new(0x0028, 0x1221),
    Tag::new(0x0028, 0x1222),
    Tag::new(0x0028, 0x1223),
];
const TAG_ICC_PROFILE: Tag = Tag::new(0x0028, 0x2000);
const TAG_COLOR_SPACE: Tag = Tag::new(0x0028, 0x2002);
const TAG_MIN_DENSITY: Tag = Tag::new(0x2010, 0x0120);
const TAG_MAX_DENSITY: Tag = Tag::new(0x2010, 0x0130);
const TAG_ILLUMINATION: Tag = Tag::new(0x2010, 0x015E);
const TAG_REFLECTED_AMBIENT_LIGHT: Tag = Tag::new(0x2010, 0x0160);
pub(super) fn u16_at(obj: &Object, raw: &[u8]) -> u16 {
    let pair = [raw[0], raw[1]];
    if obj.value_big_endian() {
        u16::from_be_bytes(pair)
    } else {
        u16::from_le_bytes(pair)
    }
}
pub(super) fn raw_uint16(obj: &Object, tag: Tag) -> Option<u16> {
    let raw = obj.get_raw(tag)?;
    (raw.len() >= 2).then(|| u16_at(obj, &raw))
}
pub(super) fn trim_space(s: &str) -> &str {
    s.trim()
}
fn derived_int(obj: &Object, tag: Tag) -> i64 {
    obj.get_ints(tag)
        .ok()
        .and_then(|v| v.first().copied())
        .unwrap_or(0)
}
pub fn modality_lut_from_object(
    obj: &Object,
    opts: LutOptions,
) -> (ModalityLut, Option<ObjectError>) {
    modality_lut_from(Some(obj), pixels_signed(Some(obj)), opts)
}
fn modality_lut_from(
    obj: Option<&Object>,
    signed_input: bool,
    opts: LutOptions,
) -> (ModalityLut, Option<ObjectError>) {
    let mut out = ModalityLut {
        slope: 1.0,
        intercept: 0.0,
        lut: None,
    };
    let Some(obj) = obj else {
        return (out, None);
    };
    if let Ok(slope) = obj.get_float(TAG_RESCALE_SLOPE)
        && slope != 0.0
    {
        out.slope = slope;
    }
    if let Ok(intercept) = obj.get_float(TAG_RESCALE_INTERCEPT) {
        out.intercept = intercept;
    }
    let items = obj
        .get_sequence(TAG_MODALITY_LUT_SEQUENCE)
        .unwrap_or_default();
    let Some(first) = items.first() else {
        return (out, None);
    };
    match lut_from_item(Some(first), signed_input, opts) {
        Ok(lut) => {
            out.lut = Some(lut);
            (out, None)
        }
        Err(e) => (out, Some(e)),
    }
}
pub fn voi_from_object(obj: &Object, opts: LutOptions) -> (VoiLut, Option<ObjectError>) {
    voi_from(Some(obj), voi_input_signed(Some(obj), Some(obj)), opts)
}
fn voi_from(
    obj: Option<&Object>,
    signed_input: bool,
    opts: LutOptions,
) -> (VoiLut, Option<ObjectError>) {
    let mut out = VoiLut {
        function: voi_function_from_object(obj),
        ..VoiLut::default()
    };
    let Some(obj) = obj else {
        return (out, None);
    };
    if let Ok(centers) = obj.get_floats(TAG_WINDOW_CENTER)
        && let Some(&center) = centers.first()
        && let Ok(widths) = obj.get_floats(TAG_WINDOW_WIDTH)
        && let Some(&width) = widths.first()
        && valid_window_width(out.function, width)
    {
        out.center = center;
        out.width = width;
    }
    let items = obj.get_sequence(TAG_VOI_LUT_SEQUENCE).unwrap_or_default();
    let Some(first) = items.first() else {
        return (out, None);
    };
    match lut_from_item(Some(first), signed_input, opts) {
        Ok(lut) => {
            out.lut = Some(lut);
            (out, None)
        }
        Err(e) => (out, Some(e)),
    }
}
fn voi_function_from_object(obj: Option<&Object>) -> VoiFunction {
    let Some(value) = obj.and_then(|o| o.get_string(TAG_VOI_LUT_FUNCTION)) else {
        return VoiFunction::Linear;
    };
    match textfmt::to_upper(trim_space(&value)).as_str() {
        "SIGMOID" => VoiFunction::Sigmoid,
        "LINEAR_EXACT" => VoiFunction::LinearExact,
        _ => VoiFunction::Linear,
    }
}
pub(super) fn pixels_signed(obj: Option<&Object>) -> bool {
    obj.is_some_and(|o| derived_int(o, TAG_PIXEL_REPRESENTATION) == 1)
}
fn voi_input_signed(root: Option<&Object>, modality: Option<&Object>) -> bool {
    let Some(modality) = modality else {
        return pixels_signed(root);
    };
    if modality
        .get_sequence(TAG_MODALITY_LUT_SEQUENCE)
        .is_some_and(|items| !items.is_empty())
    {
        return false;
    }
    let mut slope = 1.0;
    let mut intercept = 0.0;
    if let Ok(value) = modality.get_float(TAG_RESCALE_SLOPE)
        && value != 0.0
    {
        slope = value;
    }
    if let Ok(value) = modality.get_float(TAG_RESCALE_INTERCEPT) {
        intercept = value;
    }
    let mut bits_stored = 16;
    if let Some(root) = root {
        let value = derived_int(root, TAG_BITS_STORED);
        if value > 0 && value <= 32 {
            bits_stored = value as i32;
        }
    }
    let (low, high) = if pixels_signed(root) {
        (
            -(2f64.powi(bits_stored - 1)),
            2f64.powi(bits_stored - 1) - 1.0,
        )
    } else {
        (0.0, 2f64.powi(bits_stored) - 1.0)
    };
    fmath::min(slope * low, slope * high) + intercept < 0.0
}
pub fn lut_from_item(
    item: Option<&Object>,
    signed_input: bool,
    opts: LutOptions,
) -> Result<Lut, ObjectError> {
    let item = item.ok_or_else(ObjectError::invalid_descriptor)?;
    let elem = item
        .get(TAG_LUT_DESCRIPTOR)
        .ok_or_else(ObjectError::invalid_descriptor)?;
    let desc = elem
        .raw()
        .filter(|r| r.len() >= 6)
        .ok_or_else(ObjectError::invalid_descriptor)?;
    let (number_of_entries, first_mapped, bits_per_entry) =
        descriptor_values(item, desc, elem.vr() == Vr::SS || signed_input);
    let raw = item
        .get_raw(TAG_LUT_DATA)
        .filter(|r| !r.is_empty())
        .ok_or_else(ObjectError::invalid_data)?;
    Lut::from_data(
        &raw,
        item.value_big_endian(),
        number_of_entries,
        first_mapped,
        bits_per_entry,
        opts,
    )
    .map_err(ObjectError::from_display)
}
fn descriptor_values(obj: &Object, desc: &[u8], signed: bool) -> (i64, i64, i64) {
    let number_of_entries = i64::from(u16_at(obj, &desc[0..2]));
    let bits_per_entry = i64::from(u16_at(obj, &desc[4..6]));
    let raw_first = u16_at(obj, &desc[2..4]);
    let first_mapped = if signed {
        i64::from(raw_first as i16)
    } else {
        i64::from(raw_first)
    };
    (number_of_entries, first_mapped, bits_per_entry)
}
pub fn modality_lut_at(
    obj: &Object,
    frame_index: i64,
    opts: LutOptions,
) -> (ModalityLut, Option<ObjectError>) {
    let (item, _, conflict) =
        obj.functional_group_item(frame_index, TAG_PIXEL_VALUE_TRANSFORMATION_SEQUENCE);
    let Some(item) = item else {
        return modality_lut_from_object(obj, opts);
    };
    let (out, err) = modality_lut_from(Some(&item), pixels_signed(Some(obj)), opts);
    (out, ObjectError::join([conflict_error(conflict), err]))
}
fn conflict_error(conflict: Option<crate::object::Error>) -> Option<ObjectError> {
    conflict.map(|e| ObjectError::new(ObjectErrorKind::FunctionalGroupConflict, e.to_string()))
}
fn modality_source_at(obj: &Object, frame_index: i64) -> Object {
    let (item, _, _) =
        obj.functional_group_item(frame_index, TAG_PIXEL_VALUE_TRANSFORMATION_SEQUENCE);
    item.unwrap_or_else(|| obj.clone())
}
pub fn voi_at(obj: &Object, frame_index: i64, opts: LutOptions) -> (VoiLut, Option<ObjectError>) {
    let (item, _, conflict) = obj.functional_group_item(frame_index, TAG_FRAME_VOI_LUT_SEQUENCE);
    let modality = modality_source_at(obj, frame_index);
    let signed = voi_input_signed(Some(obj), Some(&modality));
    let (out, err) = voi_from(Some(item.as_ref().unwrap_or(obj)), signed, opts);
    (out, ObjectError::join([conflict_error(conflict), err]))
}
pub fn vois_at(
    obj: &Object,
    frame_index: i64,
    opts: LutOptions,
) -> (Vec<VoiLut>, Option<ObjectError>) {
    let (item, _, conflict) = obj.functional_group_item(frame_index, TAG_FRAME_VOI_LUT_SEQUENCE);
    let source = item.as_ref().unwrap_or(obj);
    let function = voi_function_from_object(Some(source));
    let mut out = Vec::new();
    let centers = source.get_floats(TAG_WINDOW_CENTER).unwrap_or_default();
    let widths = source.get_floats(TAG_WINDOW_WIDTH).unwrap_or_default();
    let explanations = source
        .get_strings(TAG_WINDOW_EXPLANATION)
        .unwrap_or_default();
    for (i, (&center, &width)) in centers.iter().zip(&widths).enumerate() {
        if !valid_window_width(function, width) {
            continue;
        }
        out.push(VoiLut {
            center,
            width,
            function,
            lut: None,
            explanation: explanations
                .get(i)
                .map(|e| trim_space(e).to_owned())
                .unwrap_or_default(),
        });
    }
    let mut errs = vec![conflict_error(conflict)];
    let modality = modality_source_at(obj, frame_index);
    let signed = voi_input_signed(Some(obj), Some(&modality));
    for item in source
        .get_sequence(TAG_VOI_LUT_SEQUENCE)
        .unwrap_or_default()
    {
        match lut_from_item(Some(&item), signed, opts) {
            Ok(lut) => out.push(VoiLut {
                function,
                lut: Some(lut),
                explanation: item
                    .get_string(TAG_LUT_EXPLANATION)
                    .map(|e| trim_space(&e).to_owned())
                    .unwrap_or_default(),
                ..VoiLut::default()
            }),
            Err(e) => errs.push(Some(e)),
        }
    }
    (out, ObjectError::join(errs))
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColorMetadata {
    pub photometric: String,
    pub samples_per_pixel: i64,
    pub planar_configuration: i64,
    pub planar_configuration_present: bool,
    pub color_space: String,
    pub icc_profile: Option<Vec<u8>>,
}
pub fn color_metadata_from_object(obj: &Object) -> ColorMetadata {
    let mut m = ColorMetadata::default();
    if let Some(p) = obj.get_string(TAG_PHOTOMETRIC) {
        m.photometric = trim_space(&p).to_owned();
    }
    if let Some(spp) = raw_uint16(obj, TAG_SAMPLES_PER_PIXEL) {
        m.samples_per_pixel = i64::from(spp);
    }
    if let Some(pc) = raw_uint16(obj, TAG_PLANAR_CONFIGURATION) {
        m.planar_configuration = i64::from(pc);
        m.planar_configuration_present = true;
    }
    if let Some(cs) = obj.get_string(TAG_COLOR_SPACE) {
        m.color_space = trim_space(&cs).to_owned();
    }
    if let Some(icc) = obj.get_raw(TAG_ICC_PROFILE)
        && !icc.is_empty()
    {
        m.icc_profile = Some(icc);
    }
    m
}
pub fn palette_from_object(
    obj: &Object,
    opts: LutOptions,
) -> Result<Option<PaletteColorLut>, ObjectError> {
    let signed = pixels_signed(Some(obj));
    let mut luts = Vec::with_capacity(3);
    for c in 0..3 {
        luts.push(palette_lut(
            obj,
            TAG_PALETTE_DESCRIPTORS[c],
            TAG_PALETTE_DATA[c],
            TAG_SEGMENTED_PALETTE_DATA[c],
            signed,
            opts,
        )?);
    }
    let blue = luts.pop().flatten();
    let green = luts.pop().flatten();
    let red = luts.pop().flatten();
    match (red, green, blue) {
        (Some(red), Some(green), Some(blue)) => Ok(Some(PaletteColorLut {
            red: Some(red),
            green: Some(green),
            blue: Some(blue),
        })),
        _ => Ok(None),
    }
}
fn palette_lut(
    obj: &Object,
    desc_tag: Tag,
    data_tag: Tag,
    segmented_tag: Tag,
    signed_input: bool,
    opts: LutOptions,
) -> Result<Option<Lut>, ObjectError> {
    let Some(elem) = obj.get(desc_tag) else {
        if obj.get_raw(data_tag).is_some() || obj.get_raw(segmented_tag).is_some() {
            return Err(ObjectError::invalid_descriptor());
        }
        return Ok(None);
    };
    let desc = elem
        .raw()
        .filter(|r| r.len() >= 6)
        .ok_or_else(ObjectError::invalid_descriptor)?;
    let (number_of_entries, first_mapped, bits_per_entry) =
        descriptor_values(obj, desc, elem.vr() == Vr::SS || signed_input);
    if let Some(data) = obj.get_raw(data_tag)
        && !data.is_empty()
    {
        return Lut::from_data(
            &data,
            obj.value_big_endian(),
            number_of_entries,
            first_mapped,
            bits_per_entry,
            opts,
        )
        .map(Some)
        .map_err(ObjectError::from_display);
    }
    let segmented = obj
        .get_raw(segmented_tag)
        .filter(|r| !r.is_empty())
        .ok_or_else(ObjectError::invalid_data)?;
    let entries = segmented_palette_entries(&segmented, number_of_entries, obj.value_big_endian())?;
    let count = if number_of_entries == 0 {
        1 << 16
    } else {
        number_of_entries
    };
    if entries.len() as i64 != count {
        return Err(ObjectError::invalid_data_detail(format_args!(
            "segmented palette expands to {} entries, want {count}",
            entries.len()
        )));
    }
    let (entries, bits_per_entry, adjustments) =
        word_lut_depth(entries, bits_per_entry, Vec::new(), opts, false)
            .map_err(ObjectError::from_display)?;
    Ok(Some(Lut {
        first_mapped,
        bits_per_entry,
        entries,
        adjustments,
    }))
}
fn segmented_palette_entries(
    data: &[u8],
    number_of_entries: i64,
    big_endian: bool,
) -> Result<Vec<u16>, ObjectError> {
    if !data.len().is_multiple_of(2) {
        return Err(ObjectError::invalid_data());
    }
    let count = if number_of_entries == 0 {
        1usize << 16
    } else {
        number_of_entries as usize
    };
    let words: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            if big_endian {
                u16::from_be_bytes([p[0], p[1]])
            } else {
                u16::from_le_bytes([p[0], p[1]])
            }
        })
        .collect();
    let mut expander = SegmentExpander {
        budget: words.len() as i64 + count as i64,
        words,
        count,
        segment_starts: None,
    };
    let mut entries = Vec::with_capacity(count);
    let mut i = 0;
    while i < expander.words.len() {
        i = expander.expand(i, &mut entries, true)?;
    }
    Ok(entries)
}
fn segment_end(words: &[u16], index: usize) -> Result<usize, ObjectError> {
    if index + 1 >= words.len() {
        return Err(ObjectError::invalid_data());
    }
    let mut end = index + 2;
    match words[index] {
        0 => end += usize::from(words[index + 1]),
        1 => end += 1,
        2 => end += 2,
        opcode => {
            return Err(ObjectError::invalid_data_detail(format_args!(
                "segmented palette opcode {opcode}"
            )));
        }
    }
    if end > words.len() {
        return Err(ObjectError::invalid_data());
    }
    Ok(end)
}
struct SegmentExpander {
    words: Vec<u16>,
    count: usize,
    budget: i64,
    segment_starts: Option<Vec<bool>>,
}
impl SegmentExpander {
    fn expand(
        &mut self,
        index: usize,
        entries: &mut Vec<u16>,
        allow_indirect: bool,
    ) -> Result<usize, ObjectError> {
        let next = segment_end(&self.words, index)?;
        self.budget -= 1;
        if self.budget < 0 {
            return Err(ObjectError::invalid_data_detail(format_args!(
                "segmented palette expands more segments than its {} words and {} entries allow",
                self.words.len(),
                self.count
            )));
        }
        let opcode = self.words[index];
        let length = usize::from(self.words[index + 1]);
        let data_index = index + 2;
        match opcode {
            0 => {
                if entries.len() + length > self.count {
                    return Err(ObjectError::invalid_data());
                }
                entries.extend_from_slice(&self.words[data_index..next]);
            }
            1 => {
                if entries.len() + length > self.count {
                    return Err(ObjectError::invalid_data());
                }
                if length == 0 {
                    return Ok(next);
                }
                let Some(&start) = entries.last() else {
                    return Err(ObjectError::invalid_data());
                };
                let end = self.words[data_index];
                for step in 1..=length {
                    entries.push(interpolate_segment_value(start, end, step, length));
                }
            }
            _ => {
                if !allow_indirect {
                    return Err(ObjectError::new(
                        ObjectErrorKind::UnsupportedSegmentedPalette,
                        format!(
                            "{UNSUPPORTED_SEGMENTED}: indirect segment cannot copy another indirect segment"
                        ),
                    ));
                }
                let offset =
                    u32::from(self.words[data_index]) | u32::from(self.words[data_index + 1]) << 16;
                let mut copy_index = (offset / 2) as usize;
                if offset % 2 != 0 || copy_index >= self.words.len() {
                    return Err(ObjectError::invalid_data());
                }
                if self.segment_starts.is_none() {
                    let mut starts = vec![false; self.words.len()];
                    let mut i = 0;
                    while i < self.words.len() {
                        starts[i] = true;
                        i = segment_end(&self.words, i)?;
                    }
                    self.segment_starts = Some(starts);
                }
                if !self.segment_starts.as_ref().is_some_and(|s| s[copy_index]) {
                    return Err(ObjectError::invalid_data_detail(format_args!(
                        "indirect offset {offset} is not a segment start"
                    )));
                }
                for _ in 0..length {
                    copy_index = self.expand(copy_index, entries, false)?;
                }
            }
        }
        Ok(next)
    }
}
fn interpolate_segment_value(start: u16, end: u16, step: usize, length: usize) -> u16 {
    let value =
        f64::from(start) + (f64::from(end) - f64::from(start)) * step as f64 / length as f64;
    value.round() as u16
}
pub fn print_density_from_object(obj: &Object) -> PrintDensity {
    let mut out = super::DEFAULT_PRINT_DENSITY;
    let read = |tag: Tag| -> Option<f64> {
        if let Some(raw) = obj.get_raw(tag)
            && raw.len() >= 2
        {
            return Some(f64::from(u16_at(obj, &raw)));
        }
        let text = obj.get_string(tag)?;
        numtext::parse_float(trim_space(&text)).ok()
    };
    if let Some(value) = read(TAG_MIN_DENSITY) {
        out.min_density = value / 100.0;
    }
    if let Some(value) = read(TAG_MAX_DENSITY)
        && value > 0.0
    {
        out.max_density = value / 100.0;
    }
    if let Some(value) = read(TAG_ILLUMINATION)
        && value > 0.0
    {
        out.illumination = value;
    }
    if let Some(value) = read(TAG_REFLECTED_AMBIENT_LIGHT) {
        out.reflected_ambient_light = value;
    }
    out.normalized()
}
