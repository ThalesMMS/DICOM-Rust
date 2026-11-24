#![forbid(unsafe_code)]
mod capture;
mod stream;
use crate::dcmtime::ZonedTime;
use crate::deid::trim_space;
use crate::derivation::{self, Derivation, DerivedCommon, Source};
use crate::derivedio as dio;
use crate::object::element_setters::ss_element;
use crate::object::inherit::{inherit_modules, new_uid, trim_nul_padding};
use crate::object::{Element, ElementHeader, File, Object, Tag, Value, Vr};
use crate::textfmt::float_v;
pub use capture::{
    CaptureImage, CaptureKind, FrameProducer, MULTI_FRAME_TRUE_COLOR_SECONDARY_CAPTURE_STORAGE,
    RgbCineSource, SecondaryCaptureRgbCineOptions, SecondaryCaptureRgbOptions,
    build_secondary_capture_rgb, build_secondary_capture_rgb_cine,
};
pub use stream::{Emit, SeriesWriteResult, SliceSource, StreamOptions, write_series};
pub const ERR_INVALID: &str = "dicom/imagebuild: invalid image";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    pub invalid: bool,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl Error {
    fn other(message: impl std::fmt::Display) -> Error {
        Error {
            message: message.to_string(),
            invalid: false,
        }
    }
    fn wrap(self, prefix: &str) -> Error {
        Error {
            message: format!("{prefix}: {}", self.message),
            invalid: self.invalid,
        }
    }
}
pub(crate) fn invalid(detail: impl AsRef<str>) -> Error {
    Error {
        message: format!("{ERR_INVALID}: {}", detail.as_ref()),
        invalid: true,
    }
}
impl From<derivation::Error> for Error {
    fn from(e: derivation::Error) -> Error {
        Error::other(e)
    }
}
impl From<crate::object::Error> for Error {
    fn from(e: crate::object::Error) -> Error {
        Error::other(e)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Iod(pub i64);
impl Iod {
    pub const CT: Iod = Iod(1);
    pub const MR: Iod = Iod(2);
    pub const SECONDARY_CAPTURE: Iod = Iod(3);
}
pub(crate) const CT_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.2";
pub(crate) const MR_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.4";
pub(crate) const SECONDARY_CAPTURE_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.7";
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub rows: i64,
    pub columns: i64,
    pub pixel_spacing: [f64; 2],
    pub row_direction: [f64; 3],
    pub column_direction: [f64; 3],
    pub slice_thickness: f64,
    pub frame_of_reference_uid: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Slice {
    pub values: Vec<f64>,
    pub position: [f64; 3],
    pub valid: Option<Vec<bool>>,
    pub sources: Vec<Source>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rescale {
    pub slope: f64,
    pub intercept: f64,
}
impl Rescale {
    fn plus_v(&self) -> String {
        format!(
            "{{Slope:{} Intercept:{}}}",
            float_v(self.slope),
            float_v(self.intercept)
        )
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Window {
    pub center: f64,
    pub width: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub iod: Iod,
    pub secondary_capture_image_plane: bool,
    pub common: DerivedCommon,
    pub series_instance_uid: String,
    pub sop_instance_uids: Vec<String>,
    pub series_number: i64,
    pub series_description: String,
    pub image_type: Vec<String>,
    pub derivation: Derivation,
    pub referenced_instances: Vec<Source>,
    pub rescale: Option<Rescale>,
    pub rescale_type: String,
    pub window: Option<Window>,
    pub content_time: Option<ZonedTime>,
}
impl Options {
    pub(crate) fn has_image_plane(&self) -> bool {
        self.iod != Iod::SECONDARY_CAPTURE || self.secondary_capture_image_plane
    }
}
pub(crate) fn local_now() -> ZonedTime {
    use chrono::{Datelike, Offset, Timelike};
    let now = chrono::Local::now();
    ZonedTime::from_fields(
        i64::from(now.year()),
        i64::from(now.month()),
        i64::from(now.day()),
        i64::from(now.hour()),
        i64::from(now.minute()),
        i64::from(now.second()),
        i64::from(now.nanosecond()),
        i64::from(now.offset().fix().local_minus_utc()),
    )
}
pub(crate) fn source_has(common: &DerivedCommon, tag: Tag) -> bool {
    common.source.as_ref().is_some_and(|s| s.has(tag))
}
pub fn build(geometry: &Geometry, slices: &[Slice], opts: &Options) -> Result<Vec<File>, Error> {
    validate(geometry, slices, opts)?;
    let (rescale, signed, padding) = choose_rescale(slices, opts)?;
    let mut series_uid = trim_space(&opts.series_instance_uid).to_string();
    if series_uid.is_empty() {
        series_uid = new_uid();
    }
    let mut frame_uid = trim_space(&geometry.frame_of_reference_uid).to_string();
    if frame_uid.is_empty()
        && opts.has_image_plane()
        && !source_has(&opts.common, TAG_FRAME_OF_REFERENCE_UID)
    {
        frame_uid = new_uid();
    }
    let mut study_uid = String::new();
    if !source_has(&opts.common, TAG_STUDY_INSTANCE_UID) {
        study_uid = new_uid();
    }
    let now = opts.content_time.unwrap_or_else(local_now);
    let mut files = Vec::with_capacity(slices.len());
    for (i, slice) in slices.iter().enumerate() {
        let file = build_instance_pixels(
            geometry,
            slice,
            i as i64 + 1,
            opts,
            rescale,
            signed,
            &series_uid,
            &study_uid,
            &frame_uid,
            now,
            padding,
        )
        .map_err(|e| e.wrap(&format!("slice {}", i + 1)))?;
        files.push(file);
    }
    Ok(files)
}
fn vec_v(values: &[f64]) -> String {
    let parts: Vec<String> = values.iter().map(|&v| float_v(v)).collect();
    format!("[{}]", parts.join(" "))
}
pub(crate) fn validate(geometry: &Geometry, slices: &[Slice], opts: &Options) -> Result<(), Error> {
    if opts.iod != Iod::CT && opts.iod != Iod::MR && opts.iod != Iod::SECONDARY_CAPTURE {
        return Err(invalid(format!("unknown IOD {}", opts.iod.0)));
    }
    if geometry.rows < 1
        || geometry.columns < 1
        || geometry.rows > u16::MAX as i64
        || geometry.columns > u16::MAX as i64
    {
        return Err(invalid(format!(
            "{}x{} pixels",
            geometry.columns, geometry.rows
        )));
    }
    if slices.is_empty() {
        return Err(invalid("no slices"));
    }
    validate_instance_uids(&opts.sop_instance_uids, slices.len())?;
    if opts.has_image_plane() {
        let s = geometry.pixel_spacing;
        if s.iter().any(|v| v.is_nan() || *v <= 0.0 || v.is_infinite()) {
            return Err(invalid(format!("pixel spacing {}", vec_v(&s))));
        }
        let (row, column) = (geometry.row_direction, geometry.column_direction);
        if (norm(row) - 1.0).abs() > 1e-3
            || (norm(column) - 1.0).abs() > 1e-3
            || dot(row, column).abs() > 1e-3
        {
            return Err(invalid("orientation is not two orthogonal unit vectors"));
        }
    }
    let pixels = geometry.rows * geometry.columns;
    for (i, slice) in slices.iter().enumerate() {
        if slice.values.len() as i64 != pixels {
            return Err(invalid(format!(
                "slice {} has {} values for {}x{} pixels",
                i + 1,
                slice.values.len(),
                geometry.columns,
                geometry.rows
            )));
        }
        if let Some(valid) = &slice.valid
            && valid.len() != slice.values.len()
        {
            return Err(invalid(format!(
                "slice {} has {} validity flags for {} values",
                i + 1,
                valid.len(),
                slice.values.len()
            )));
        }
        if slice.position.iter().any(|v| !v.is_finite()) {
            return Err(invalid(format!(
                "slice {} position {}",
                i + 1,
                vec_v(&slice.position)
            )));
        }
    }
    Ok(())
}
pub(crate) fn validate_instance_uids(ids: &[String], count: usize) -> Result<(), Error> {
    if ids.is_empty() {
        return Ok(());
    }
    if ids.len() != count {
        return Err(invalid("SOP identities do not match slice count"));
    }
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !crate::object::is_valid_uid(id.as_bytes()) || !seen.insert(id.as_str()) {
            return Err(invalid("invalid or duplicate SOP identity"));
        }
    }
    Ok(())
}
fn norm(v: [f64; 3]) -> f64 {
    dot(v, v).sqrt()
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn fmin(a: f64, b: f64) -> f64 {
    crate::fmath::min(a, b)
}
fn fmax(a: f64, b: f64) -> f64 {
    crate::fmath::max(a, b)
}
const MAX_U16: f64 = 65535.0;
const MIN_I16: f64 = -32768.0;
const MAX_I16: f64 = 32767.0;
fn choose_rescale(slices: &[Slice], opts: &Options) -> Result<(Rescale, bool, Option<i32>), Error> {
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut integral, mut padded) = (true, false);
    for slice in slices {
        for (i, &v) in slice.values.iter().enumerate() {
            if let Some(valid) = &slice.valid
                && !valid[i]
            {
                padded = true;
                continue;
            }
            if !v.is_finite() {
                return Err(invalid(format!("value {}", float_v(v))));
            }
            low = fmin(low, v);
            high = fmax(high, v);
            integral = integral && (v - v.round()).abs() < 1e-6;
        }
    }
    if low > high {
        return Err(invalid("no valid pixels"));
    }
    let (mut rescale, signed) = choose_rescale_range(low, high, integral, opts.rescale.as_ref())?;
    if !padded {
        return Ok((rescale, signed, None));
    }
    let identity = Rescale {
        slope: 1.0,
        intercept: 0.0,
    };
    if opts.iod == Iod::MR && rescale != identity {
        return Err(invalid(
            "padded classic MR requires integral values with identity rescale",
        ));
    }
    if opts.rescale.is_some() || rescale == identity {
        if let Some(padding) = spare_stored_code(low, high, rescale, signed) {
            return Ok((rescale, signed, Some(padding)));
        }
        if opts.rescale.is_some() || opts.iod == Iod::MR {
            return Err(invalid("no unused padding code in the selected rescale"));
        }
    }
    let mut padding = u16::MAX as i32;
    if high > low {
        rescale = Rescale {
            slope: (high - low) / (MAX_U16 - 1.0),
            intercept: low,
        };
        let dark = Rescale {
            slope: rescale.slope,
            intercept: low - rescale.slope,
        };
        if let Some(code) = spare_stored_code(low, high, dark, false)
            && code == 0
        {
            rescale = dark;
            padding = code;
        }
    }
    if rescale.slope <= 0.0 || rescale.slope.is_infinite() || rescale.intercept.is_infinite() {
        return Err(invalid("padded pixel range cannot be represented"));
    }
    Ok((rescale, false, Some(padding)))
}
fn spare_stored_code(low: f64, high: f64, rescale: Rescale, signed: bool) -> Option<i32> {
    let a = ((low - rescale.intercept) / rescale.slope).round();
    let b = ((high - rescale.intercept) / rescale.slope).round();
    let (minimum, maximum) = if signed {
        (MIN_I16, MAX_I16)
    } else {
        (0.0, MAX_U16)
    };
    let (lo, hi) = (fmin(a, b), fmax(a, b));
    if !(lo >= minimum && hi <= maximum) {
        return None;
    }
    if lo > minimum {
        return Some(minimum as i32);
    }
    if hi < maximum {
        return Some(maximum as i32);
    }
    None
}
pub(crate) fn choose_rescale_range(
    low: f64,
    high: f64,
    integral: bool,
    fixed: Option<&Rescale>,
) -> Result<(Rescale, bool), Error> {
    if let Some(fixed) = fixed {
        if fixed.slope == 0.0 || !fixed.slope.is_finite() || !fixed.intercept.is_finite() {
            return Err(invalid(format!("rescale {}", fixed.plus_v())));
        }
        let a = ((low - fixed.intercept) / fixed.slope).round();
        let b = ((high - fixed.intercept) / fixed.slope).round();
        let (lo, hi) = (crate::fmath::min(a, b), crate::fmath::max(a, b));
        if lo >= 0.0 && hi <= MAX_U16 {
            return Ok((*fixed, false));
        }
        if lo >= MIN_I16 && hi <= MAX_I16 {
            return Ok((*fixed, true));
        }
        return Err(invalid(format!(
            "values {} to {} do not fit 16 bits with {}",
            float_v(low),
            float_v(high),
            fixed.plus_v()
        )));
    }
    let one = |intercept| Rescale {
        slope: 1.0,
        intercept,
    };
    if integral && low >= 0.0 && high <= MAX_U16 {
        return Ok((one(0.0), false));
    }
    if integral && low >= MIN_I16 && high <= MAX_I16 {
        return Ok((one(0.0), true));
    }
    if high == low {
        return Ok((one(low), false));
    }
    Ok((
        Rescale {
            slope: (high - low) / MAX_U16,
            intercept: low,
        },
        false,
    ))
}
const fn t(group: u16, element: u16) -> Tag {
    Tag::new(group, element)
}
pub(crate) const TAG_IMAGE_TYPE: Tag = t(0x0008, 0x0008);
pub(crate) const TAG_CONTENT_DATE: Tag = t(0x0008, 0x0023);
pub(crate) const TAG_CONTENT_TIME: Tag = t(0x0008, 0x0033);
pub(crate) const TAG_CONVERSION_TYPE: Tag = t(0x0008, 0x0064);
pub(crate) const TAG_SERIES_DESCRIPTION: Tag = t(0x0008, 0x103e);
const TAG_SLICE_THICKNESS: Tag = t(0x0018, 0x0050);
pub(crate) const TAG_STUDY_INSTANCE_UID: Tag = t(0x0020, 0x000d);
pub(crate) const TAG_SERIES_NUMBER: Tag = t(0x0020, 0x0011);
pub(crate) const TAG_INSTANCE_NUMBER: Tag = t(0x0020, 0x0013);
pub(crate) const TAG_PATIENT_ORIENTATION: Tag = t(0x0020, 0x0020);
const TAG_IMAGE_POSITION: Tag = t(0x0020, 0x0032);
const TAG_IMAGE_ORIENTATION: Tag = t(0x0020, 0x0037);
pub(crate) const TAG_FRAME_OF_REFERENCE_UID: Tag = t(0x0020, 0x0052);
const TAG_LATERALITY: Tag = t(0x0020, 0x0060);
const TAG_PIXEL_SPACING: Tag = t(0x0028, 0x0030);
const TAG_PIXEL_PADDING_VALUE: Tag = t(0x0028, 0x0120);
const TAG_WINDOW_CENTER: Tag = t(0x0028, 0x1050);
const TAG_WINDOW_WIDTH: Tag = t(0x0028, 0x1051);
const TAG_RESCALE_INTERCEPT: Tag = t(0x0028, 0x1052);
const TAG_RESCALE_SLOPE: Tag = t(0x0028, 0x1053);
const TAG_RESCALE_TYPE: Tag = t(0x0028, 0x1054);
pub(crate) const TAG_MODALITY: Tag = t(0x0008, 0x0060);
const TAG_PATIENT_POSITION: Tag = t(0x0018, 0x5100);
const TAG_BODY_PART_EXAMINED: Tag = t(0x0018, 0x0015);
const TAG_REFERENCED_INSTANCE_SEQUENCE: Tag = t(0x0008, 0x114A);
const CT_ACQUISITION: &[Tag] = &[t(0x0018, 0x0060), t(0x0020, 0x0012), TAG_PATIENT_POSITION];
const MR_ACQUISITION: &[Tag] = &[
    t(0x0018, 0x0020),
    t(0x0018, 0x0021),
    t(0x0018, 0x0022),
    t(0x0018, 0x0023),
    t(0x0018, 0x0080),
    t(0x0018, 0x0081),
    t(0x0018, 0x0082),
    t(0x0018, 0x0091),
    t(0x0018, 0x0087),
    t(0x0018, 0x0085),
    t(0x0020, 0x0012),
    TAG_PATIENT_POSITION,
];
pub(crate) fn empty(tag: Tag, vr: Vr) -> Element {
    dio::raw(tag, vr, Vec::new())
}
pub(crate) fn pad2(v: i64) -> String {
    format!("{v:02}")
}
fn date_string(now: &ZonedTime) -> String {
    format!("{:04}{}{}", now.year(), pad2(now.month()), pad2(now.day()))
}
fn time_string(now: &ZonedTime) -> String {
    format!(
        "{}{}{}",
        pad2(now.hour()),
        pad2(now.minute()),
        pad2(now.second())
    )
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_instance_pixels(
    geometry: &Geometry,
    slice: &Slice,
    number: i64,
    opts: &Options,
    rescale: Rescale,
    signed: bool,
    series_uid: &str,
    study_uid: &str,
    frame_uid: &str,
    now: ZonedTime,
    padding: Option<i32>,
) -> Result<File, Error> {
    let sop_uid = if opts.sop_instance_uids.is_empty() {
        new_uid()
    } else {
        opts.sop_instance_uids[(number - 1) as usize].clone()
    };
    let (sop_class, modality) = match opts.iod {
        Iod::MR => (MR_IMAGE_STORAGE, "MR"),
        Iod::SECONDARY_CAPTURE => (SECONDARY_CAPTURE_IMAGE_STORAGE, "OT"),
        _ => (CT_IMAGE_STORAGE, "CT"),
    };
    let mut image_type = opts.image_type.clone();
    if image_type.is_empty() {
        image_type = vec!["DERIVED".into(), "SECONDARY".into()];
    }
    if image_type.len() == 2 {
        match opts.iod {
            Iod::CT => image_type.push("AXIAL".into()),
            Iod::MR => image_type.push("OTHER".into()),
            _ => {}
        }
    }
    let representation = u16::from(signed);
    let mut pixels = vec![0u8; 2 * slice.values.len()];
    let mut padded = false;
    let put = |pixels: &mut [u8], i: usize, v: u16| {
        pixels[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
    };
    for (i, &v) in slice.values.iter().enumerate() {
        if let (Some(p), Some(valid)) = (padding, &slice.valid)
            && !valid[i]
        {
            put(&mut pixels, i, p as u16);
            padded = true;
            continue;
        }
        let mut stored = ((v - rescale.intercept) / rescale.slope).round();
        if let Some(p) = padding {
            let (low, high) = if signed {
                (MIN_I16, MAX_I16)
            } else {
                (0.0, MAX_U16)
            };
            if !(stored >= low && stored <= high) || stored == p as f64 {
                return Err(invalid(format!(
                    "valid pixel {} does not fit the reserved-padding rescale",
                    i + 1
                )));
            }
        }
        if signed {
            stored = crate::fmath::max(MIN_I16, crate::fmath::min(MAX_I16, stored));
            put(&mut pixels, i, (stored as i64 as i16) as u16);
        } else {
            stored = crate::fmath::max(0.0, crate::fmath::min(MAX_U16, stored));
            put(&mut pixels, i, stored as i64 as u16);
        }
    }
    let mut elements = vec![
        dio::ui(dio::TAG_SOP_CLASS_UID, sop_class),
        dio::ui(dio::TAG_SOP_INSTANCE_UID, &sop_uid),
        dio::strings(TAG_IMAGE_TYPE, Vr::CS, &image_type),
        dio::str_elem(TAG_CONTENT_DATE, Vr::DA, &date_string(&now)),
        dio::str_elem(TAG_CONTENT_TIME, Vr::TM, &time_string(&now)),
        dio::cs(TAG_MODALITY, modality),
        dio::ui(dio::TAG_SERIES_INSTANCE_UID, series_uid),
        dio::is(TAG_INSTANCE_NUMBER, &[number]),
        dio::us(dio::TAG_SAMPLES_PER_PIXEL, &[1]),
        dio::cs(dio::TAG_PHOTOMETRIC_INTERPRETATION, "MONOCHROME2"),
        dio::us(dio::TAG_ROWS, &[geometry.rows as u16]),
        dio::us(dio::TAG_COLUMNS, &[geometry.columns as u16]),
        dio::us(dio::TAG_BITS_ALLOCATED, &[16]),
        dio::us(dio::TAG_BITS_STORED, &[16]),
        dio::us(dio::TAG_HIGH_BIT, &[15]),
        dio::us(dio::TAG_PIXEL_REPRESENTATION, &[representation]),
        Element {
            header: ElementHeader {
                tag: Tag::PIXEL_DATA,
                vr: Vr::OW,
                length: pixels.len() as u32,
                length_set: true,
            },
            value: Value::Raw(pixels),
        },
    ];
    if padded {
        let p = padding.unwrap_or(0);
        if signed {
            elements.push(ss_element(TAG_PIXEL_PADDING_VALUE, false, &[p as i16]));
        } else {
            elements.push(dio::us(TAG_PIXEL_PADDING_VALUE, &[p as u16]));
        }
    }
    let identity = Rescale {
        slope: 1.0,
        intercept: 0.0,
    };
    if opts.iod != Iod::MR || rescale != identity {
        elements.push(dio::ds(TAG_RESCALE_INTERCEPT, &[rescale.intercept]));
        elements.push(dio::ds(TAG_RESCALE_SLOPE, &[rescale.slope]));
    }
    if opts.series_number > 0 {
        elements.push(dio::is(TAG_SERIES_NUMBER, &[opts.series_number]));
    } else {
        elements.push(empty(TAG_SERIES_NUMBER, Vr::IS));
    }
    let description = trim_space(&opts.series_description);
    if !description.is_empty() {
        elements.push(dio::lo(TAG_SERIES_DESCRIPTION, description));
    }
    let mut rescale_type = trim_space(&opts.rescale_type);
    if rescale_type.is_empty() && opts.iod == Iod::SECONDARY_CAPTURE {
        rescale_type = "US";
    }
    if !rescale_type.is_empty() {
        elements.push(dio::lo(TAG_RESCALE_TYPE, rescale_type));
    }
    if let Some(w) = &opts.window
        && w.width > 0.0
    {
        elements.push(dio::ds(TAG_WINDOW_CENTER, &[w.center]));
        elements.push(dio::ds(TAG_WINDOW_WIDTH, &[w.width]));
    }
    if opts.iod == Iod::SECONDARY_CAPTURE {
        elements.push(dio::cs(TAG_CONVERSION_TYPE, "WSD"));
        elements.push(empty(TAG_PATIENT_ORIENTATION, Vr::CS));
    }
    if opts.has_image_plane() {
        let (row, column) = (geometry.row_direction, geometry.column_direction);
        elements.push(dio::ds(TAG_IMAGE_POSITION, &slice.position));
        elements.push(dio::ds(
            TAG_IMAGE_ORIENTATION,
            &[row[0], row[1], row[2], column[0], column[1], column[2]],
        ));
        elements.push(dio::ds(TAG_PIXEL_SPACING, &geometry.pixel_spacing));
        if geometry.slice_thickness > 0.0 {
            elements.push(dio::ds(TAG_SLICE_THICKNESS, &[geometry.slice_thickness]));
        } else {
            elements.push(empty(TAG_SLICE_THICKNESS, Vr::DS));
        }
        if !frame_uid.is_empty() {
            elements.push(dio::ui(TAG_FRAME_OF_REFERENCE_UID, frame_uid));
        }
    }
    if !study_uid.is_empty() {
        elements.push(dio::ui(TAG_STUDY_INSTANCE_UID, study_uid));
    }
    let mut dataset = dio::object(elements);
    copy_acquisition(&mut dataset, opts, modality);
    let modules = if opts.has_image_plane() {
        inherit_modules::ALL
    } else {
        inherit_modules::WITHOUT_FRAME_OF_REFERENCE
    };
    opts.common.apply(&mut dataset, modules)?;
    let mut step = opts.derivation.clone();
    step.sources.extend(slice.sources.iter().cloned());
    if !step.description.is_empty() || !step.codes.is_empty() || !step.sources.is_empty() {
        derivation::apply(Some(&mut dataset), &step)?;
    }
    if !opts.referenced_instances.is_empty() {
        let mut items = Vec::with_capacity(opts.referenced_instances.len());
        for reference in &opts.referenced_instances {
            if reference.purpose.is_none() {
                return Err(invalid(format!(
                    "referenced instance {} needs a purpose of reference",
                    reference.sop_instance_uid
                )));
            }
            items.push(reference.item()?);
        }
        dataset.put(dio::seq(TAG_REFERENCED_INSTANCE_SEQUENCE, items));
    }
    Ok(dio::file(sop_class, &sop_uid, Some(dataset))?)
}
fn copy_acquisition(dataset: &mut Object, opts: &Options, modality: &str) {
    let tags: &[Tag] = match opts.iod {
        Iod::CT => CT_ACQUISITION,
        Iod::MR => MR_ACQUISITION,
        _ => &[],
    };
    if let Some(source) = &opts.common.source {
        let m = source.lookup_string(TAG_MODALITY).unwrap_or_default();
        let same_modality = trim_space(&m) == modality;
        let anatomy = [TAG_LATERALITY, TAG_BODY_PART_EXAMINED];
        for (i, &tag) in anatomy.iter().chain(tags.iter()).enumerate() {
            if i >= anatomy.len() && !same_modality {
                continue;
            }
            if let Ok(values) = source.lookup_strings(tag) {
                let vr = source.get(tag).map(|e| e.vr()).unwrap_or(Vr::UN);
                dataset.put(Element {
                    header: ElementHeader {
                        tag,
                        vr,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Strings(trim_nul_padding(&values)),
                });
            }
        }
    }
    let mut defaults: Vec<Element> = Vec::new();
    if opts.iod != Iod::SECONDARY_CAPTURE {
        defaults.push(empty(TAG_PATIENT_POSITION, Vr::CS));
    }
    match opts.iod {
        Iod::CT => {
            defaults.push(empty(t(0x0018, 0x0060), Vr::DS));
            defaults.push(empty(t(0x0020, 0x0012), Vr::IS));
        }
        Iod::MR => {
            defaults.push(dio::cs(t(0x0018, 0x0020), "RM"));
            defaults.push(dio::cs(t(0x0018, 0x0021), "NONE"));
            for (tag, vr) in [
                (t(0x0018, 0x0022), Vr::CS),
                (t(0x0018, 0x0023), Vr::CS),
                (t(0x0018, 0x0080), Vr::DS),
                (t(0x0018, 0x0081), Vr::DS),
                (t(0x0018, 0x0091), Vr::IS),
            ] {
                defaults.push(empty(tag, vr));
            }
        }
        _ => {}
    }
    for element in defaults {
        if !dataset.has(element.tag()) {
            dataset.put(element);
        }
    }
}
