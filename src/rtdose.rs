#![forbid(unsafe_code)]
use crate::derivedio as d;
use crate::fmath;
use crate::numtext::{parse_float, parse_int};
use crate::object::geometry::spacing_values;
use crate::object::{self, Object, Tag, Value, Vr, is_valid_uid};
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{NativeFrames, extract_native_frames};
use crate::render::{self, GeometryTolerances, SliceGeometry, Vec3, VolumeGeometry};
use crate::rtstruct::{self, RasterizeOptions, StructureSet};
use crate::textfmt::{quote, to_upper};
use std::fmt;
pub const RT_DOSE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.2";
const RT_STRUCTURE_SET_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.3";
pub const DOSE_UNITS_GRAY: &str = "GY";
pub const DOSE_UNITS_RELATIVE: &str = "RELATIVE";
pub const DOSE_TYPE_PHYSICAL: &str = "PHYSICAL";
pub const DOSE_TYPE_EFFECTIVE: &str = "EFFECTIVE";
pub const DOSE_TYPE_ERROR: &str = "ERROR";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    UnsupportedSopClass,
    InvalidObject,
    GeometryMismatch,
    UnsafeSemantics,
    MemoryLimit,
    Rtstruct(rtstruct::Kind),
}
impl ErrorKind {
    fn text(self) -> &'static str {
        match self {
            ErrorKind::UnsupportedSopClass => "dicom/rtdose: unsupported SOP class",
            ErrorKind::InvalidObject => "dicom/rtdose: invalid object",
            ErrorKind::GeometryMismatch => "dicom/rtdose: geometry mismatch",
            ErrorKind::UnsafeSemantics => "dicom/rtdose: unsafe dose semantics",
            ErrorKind::MemoryLimit => "dicom/rtdose: memory limit exceeded",
            ErrorKind::Rtstruct(_) => "",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    message: String,
}
impl Error {
    fn of(kind: ErrorKind) -> Error {
        Error {
            kind,
            message: kind.text().to_string(),
        }
    }
    fn new(kind: ErrorKind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            message: format!("{}: {detail}", kind.text()),
        }
    }
    fn rtstruct(err: rtstruct::Error) -> Error {
        Error {
            kind: ErrorKind::Rtstruct(err.kind),
            message: err.to_string(),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
const TAG_IMAGE_POSITION_PATIENT: Tag = Tag::new(0x0020, 0x0032);
const TAG_IMAGE_ORIENTATION_PATIENT: Tag = Tag::new(0x0020, 0x0037);
const TAG_SLICE_THICKNESS: Tag = Tag::new(0x0018, 0x0050);
const TAG_PIXEL_SPACING: Tag = Tag::new(0x0028, 0x0030);
const TAG_DOSE_UNITS: Tag = Tag::new(0x3004, 0x0002);
const TAG_DOSE_TYPE: Tag = Tag::new(0x3004, 0x0004);
const TAG_DOSE_SUMMATION_TYPE: Tag = Tag::new(0x3004, 0x000A);
const TAG_GRID_FRAME_OFFSET_VECTOR: Tag = Tag::new(0x3004, 0x000C);
const TAG_DOSE_GRID_SCALING: Tag = Tag::new(0x3004, 0x000E);
const TAG_DVH_TYPE: Tag = Tag::new(0x3004, 0x0001);
const TAG_DVH_SEQUENCE: Tag = Tag::new(0x3004, 0x0050);
const TAG_DVH_DOSE_SCALING: Tag = Tag::new(0x3004, 0x0052);
const TAG_DVH_VOLUME_UNITS: Tag = Tag::new(0x3004, 0x0054);
const TAG_DVH_NUMBER_OF_BINS: Tag = Tag::new(0x3004, 0x0056);
const TAG_DVH_DATA: Tag = Tag::new(0x3004, 0x0058);
const TAG_DVH_REFERENCED_ROI_SEQUENCE: Tag = Tag::new(0x3004, 0x0060);
const TAG_DVH_ROI_CONTRIBUTION_TYPE: Tag = Tag::new(0x3004, 0x0062);
const TAG_DVH_MINIMUM_DOSE: Tag = Tag::new(0x3004, 0x0070);
const TAG_DVH_MAXIMUM_DOSE: Tag = Tag::new(0x3004, 0x0072);
const TAG_DVH_MEAN_DOSE: Tag = Tag::new(0x3004, 0x0074);
const TAG_REFERENCED_ROI_NUMBER: Tag = Tag::new(0x3006, 0x0084);
const TAG_REFERENCED_RT_PLAN_SEQUENCE: Tag = Tag::new(0x300C, 0x0002);
const TAG_REFERENCED_STRUCTURE_SET_SEQ: Tag = Tag::new(0x300C, 0x0060);
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reference {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DvhRoiContribution {
    pub number: i64,
    pub r#type: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DvhItemDiagnostic {
    pub item: i64,
    pub tag: Tag,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EmbeddedDvh {
    pub r#type: String,
    pub dose_units: String,
    pub dose_type: String,
    pub dose_scaling: f64,
    pub volume_units: String,
    pub number_of_bins: i64,
    pub data: Vec<f64>,
    pub referenced_rois: Vec<i64>,
    pub contribution_type: String,
    pub roi_contributions: Vec<DvhRoiContribution>,
    pub minimum_dose: f64,
    pub maximum_dose: f64,
    pub mean_dose: f64,
}
#[derive(Debug, Clone, Default)]
pub struct Dose {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub units: String,
    pub r#type: String,
    pub summation_type: String,
    pub grid_scaling: f64,
    pub slice_thickness: f64,
    pub rows: i64,
    pub columns: i64,
    pub frames: i64,
    pub geometry: VolumeGeometry,
    pub values: Vec<f64>,
    pub maximum: f64,
    pub references: Vec<Reference>,
    pub embedded_dvhs: Vec<EmbeddedDvh>,
    pub dvh_diagnostics: Vec<DvhItemDiagnostic>,
}
fn upper(value: &str) -> String {
    to_upper(value.trim())
}
fn finite_positive(value: f64) -> bool {
    value > 0.0 && value.is_finite()
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-5
}
fn checked_product(values: &[i64]) -> Option<i64> {
    let mut product: i64 = 1;
    for &value in values {
        if value <= 0 || product > i64::MAX / value {
            return None;
        }
        product *= value;
    }
    Some(product)
}
fn floats(obj: &Object, tag: Tag) -> Vec<f64> {
    d::floats(Some(obj), tag).unwrap_or_default()
}
fn metadata_text(m: &Metadata) -> String {
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
pub fn read_dataset(obj: Option<&Object>) -> Result<Dose, Error> {
    let Some(obj) = obj else {
        return Err(Error::new(ErrorKind::InvalidObject, "dataset is nil"));
    };
    let sop_class_uid = d::clean_uid(obj, d::TAG_SOP_CLASS_UID);
    if sop_class_uid != RT_DOSE_STORAGE {
        return Err(Error::new(ErrorKind::UnsupportedSopClass, &sop_class_uid));
    }
    let mut out = Dose {
        sop_class_uid,
        sop_instance_uid: d::clean_uid(obj, d::TAG_SOP_INSTANCE_UID),
        study_instance_uid: d::clean_uid(obj, d::TAG_STUDY_INSTANCE_UID),
        series_instance_uid: d::clean_uid(obj, d::TAG_SERIES_INSTANCE_UID),
        frame_of_reference_uid: d::clean_uid(obj, d::TAG_FRAME_OF_REFERENCE_UID),
        units: upper(&d::clean_string(obj, TAG_DOSE_UNITS)),
        r#type: upper(&d::clean_string(obj, TAG_DOSE_TYPE)),
        summation_type: upper(&d::clean_string(obj, TAG_DOSE_SUMMATION_TYPE)),
        ..Dose::default()
    };
    validate_semantics(&out)?;
    let scaling = floats(obj, TAG_DOSE_GRID_SCALING);
    if scaling.len() != 1 || !finite_positive(scaling[0]) {
        return Err(Error::new(
            ErrorKind::InvalidObject,
            "DoseGridScaling must be one positive finite value",
        ));
    }
    out.grid_scaling = scaling[0];
    let thickness = floats(obj, TAG_SLICE_THICKNESS);
    if !thickness.is_empty() {
        if thickness.len() != 1 || !finite_positive(thickness[0]) {
            return Err(Error::new(
                ErrorKind::GeometryMismatch,
                "SliceThickness must be positive when present",
            ));
        }
        out.slice_thickness = thickness[0];
    }
    let native = extract_native_frames(obj).map_err(|err| {
        Error::new(
            ErrorKind::InvalidObject,
            format_args!("native Pixel Data: {err}"),
        )
    })?;
    let meta = &native.metadata;
    out.rows = i64::from(meta.rows);
    out.columns = i64::from(meta.columns);
    out.frames = meta.number_of_frames;
    let representation: u16 = if out.r#type == DOSE_TYPE_ERROR { 1 } else { 0 };
    if out.rows <= 0
        || out.columns <= 0
        || out.frames <= 0
        || meta.samples_per_pixel != 1
        || (meta.bits_allocated != 16 && meta.bits_allocated != 32)
        || meta.bits_stored == 0
        || meta.bits_stored > meta.bits_allocated
        || meta.high_bit.wrapping_add(1) < meta.bits_stored
        || meta.pixel_representation != representation
    {
        return Err(Error::new(
            ErrorKind::InvalidObject,
            format_args!("unsupported RT Dose pixel metadata {}", metadata_text(meta)),
        ));
    }
    let voxel_count = match checked_product(&[out.rows, out.columns, out.frames]) {
        Some(count) if count <= 64 * 1024 * 1024 => count,
        _ => {
            return Err(Error::new(
                ErrorKind::MemoryLimit,
                format_args!(
                    "dose grid has {} x {} x {} voxels",
                    out.columns, out.rows, out.frames
                ),
            ));
        }
    };
    let (geometries, reverse) = dose_geometries(obj, out.rows, out.columns, out.frames)?;
    out.geometry =
        render::build_volume_geometry(&geometries, GeometryTolerances::default_tolerances());
    if out.geometry.slices().len() as i64 != out.frames
        || !out.geometry.orientation_stable
        || out.geometry.gantry_tilted
    {
        return Err(Error::new(
            ErrorKind::GeometryMismatch,
            "unstable RT Dose volume",
        ));
    }
    (out.values, out.maximum) =
        decode_values(&native, obj.value_big_endian(), out.grid_scaling, reverse)?;
    if out.values.len() as i64 != voxel_count {
        return Err(Error::new(
            ErrorKind::InvalidObject,
            format_args!(
                "decoded {} values, expected {voxel_count}",
                out.values.len()
            ),
        ));
    }
    out.references = read_references(obj);
    (out.embedded_dvhs, out.dvh_diagnostics) =
        read_embedded_dvh_items(&d::sequence(obj, TAG_DVH_SEQUENCE), &out.units, &out.r#type);
    Ok(out)
}
fn validate_semantics(dose: &Dose) -> Result<(), Error> {
    if dose.sop_instance_uid.is_empty()
        || dose.series_instance_uid.is_empty()
        || dose.study_instance_uid.is_empty()
        || dose.frame_of_reference_uid.is_empty()
    {
        return Err(Error::new(
            ErrorKind::InvalidObject,
            "missing RT Dose identity or frame of reference",
        ));
    }
    if dose.units != DOSE_UNITS_GRAY && dose.units != DOSE_UNITS_RELATIVE {
        return Err(Error::new(
            ErrorKind::UnsafeSemantics,
            format_args!("unsupported DoseUnits {}", quote(&dose.units)),
        ));
    }
    if ![DOSE_TYPE_PHYSICAL, DOSE_TYPE_EFFECTIVE, DOSE_TYPE_ERROR].contains(&dose.r#type.as_str()) {
        return Err(Error::new(
            ErrorKind::UnsafeSemantics,
            format_args!("unsupported DoseType {}", quote(&dose.r#type)),
        ));
    }
    if dose.summation_type.is_empty() {
        return Err(Error::new(
            ErrorKind::UnsafeSemantics,
            "missing DoseSummationType",
        ));
    }
    Ok(())
}
fn vec_is_zero(v: Vec3) -> bool {
    v.x == 0.0 && v.y == 0.0 && v.z == 0.0
}
fn dose_geometries(
    obj: &Object,
    rows: i64,
    columns: i64,
    frames: i64,
) -> Result<(Vec<SliceGeometry>, bool), Error> {
    let mismatch = |detail: &str| Error::new(ErrorKind::GeometryMismatch, detail);
    let position = floats(obj, TAG_IMAGE_POSITION_PATIENT);
    let orientation = floats(obj, TAG_IMAGE_ORIENTATION_PATIENT);
    let spacing = obj.spacing_pair(TAG_PIXEL_SPACING);
    let mut offsets = floats(obj, TAG_GRID_FRAME_OFFSET_VECTOR);
    let (row_spacing, col_spacing) = match spacing {
        Ok((row, col, values))
            if position.len() == 3
                && orientation.len() == 6
                && values != spacing_values::ABSENT =>
        {
            (row, col)
        }
        _ => return Err(mismatch("incomplete RT Dose patient geometry")),
    };
    if frames == 1 && offsets.is_empty() {
        offsets = vec![0.0];
    }
    if offsets.len() as i64 != frames {
        return Err(Error::new(
            ErrorKind::GeometryMismatch,
            format_args!(
                "GridFrameOffsetVector length {}, frames {frames}",
                offsets.len()
            ),
        ));
    }
    if position
        .iter()
        .chain(&orientation)
        .chain(&offsets)
        .any(|v| !v.is_finite())
    {
        return Err(mismatch("non-finite patient geometry"));
    }
    let row_raw = Vec3 {
        x: orientation[0],
        y: orientation[1],
        z: orientation[2],
    };
    let col_raw = Vec3 {
        x: orientation[3],
        y: orientation[4],
        z: orientation[5],
    };
    if (row_raw.length() - 1.0).abs() > 1e-4 || (col_raw.length() - 1.0).abs() > 1e-4 {
        return Err(mismatch(
            "ImageOrientationPatient directions are not unit vectors",
        ));
    }
    let row_dir = row_raw.normalize();
    let col_dir = col_raw.normalize();
    let normal = row_dir.cross(col_dir).normalize();
    if vec_is_zero(row_dir)
        || vec_is_zero(col_dir)
        || vec_is_zero(normal)
        || row_dir.dot(col_dir).abs() > 1e-4
    {
        return Err(mismatch("invalid ImageOrientationPatient"));
    }
    let origin = Vec3 {
        x: position[0],
        y: position[1],
        z: position[2],
    };
    if !offsets.is_empty() && offsets[0].abs() > 1e-5 {
        let axial_identity = near(row_dir.x, 1.0)
            && near(row_dir.y, 0.0)
            && near(row_dir.z, 0.0)
            && near(col_dir.x, 0.0)
            && near(col_dir.y, 1.0)
            && near(col_dir.z, 0.0)
            && near(offsets[0], origin.z);
        if !axial_identity {
            return Err(mismatch("ambiguous GridFrameOffsetVector"));
        }
        for offset in &mut offsets {
            *offset -= origin.z;
        }
    }
    let mut reverse = false;
    if offsets.len() > 1 {
        let increasing = offsets.windows(2).all(|w| w[1] > w[0]);
        let decreasing = offsets.windows(2).all(|w| w[1] < w[0]);
        if !increasing && !decreasing {
            return Err(mismatch("GridFrameOffsetVector is not strictly monotonic"));
        }
        reverse = decreasing;
        if reverse {
            offsets.reverse();
        }
    }
    let out = offsets
        .iter()
        .map(|&offset| SliceGeometry {
            origin: origin.add(normal.scale(offset)),
            row_dir,
            col_dir,
            normal,
            row_spacing,
            col_spacing,
            rows,
            columns,
        })
        .collect();
    Ok((out, reverse))
}
fn decode_values(
    native: &NativeFrames,
    big_endian: bool,
    scaling: f64,
    reverse: bool,
) -> Result<(Vec<f64>, f64), Error> {
    let meta = &native.metadata;
    let Some(per_frame) = checked_product(&[i64::from(meta.rows), i64::from(meta.columns)]) else {
        return Err(Error::of(ErrorKind::MemoryLimit));
    };
    let per_frame = per_frame as usize;
    let frames = native.data.len();
    let mut out = vec![0.0f64; per_frame * frames];
    let mut maximum = f64::NEG_INFINITY;
    let bytes_per_sample = usize::from(meta.bits_allocated / 8);
    let signed = meta.pixel_representation == 1;
    for (source_index, frame) in native.data.iter().enumerate() {
        let target_index = if reverse {
            frames - 1 - source_index
        } else {
            source_index
        };
        if frame.len() != per_frame * bytes_per_sample {
            return Err(Error::new(
                ErrorKind::InvalidObject,
                format_args!("frame {source_index} has {} bytes", frame.len()),
            ));
        }
        for (pixel_index, sample) in frame.chunks_exact(bytes_per_sample).enumerate() {
            let raw = if bytes_per_sample == 2 {
                let b = [sample[0], sample[1]];
                u64::from(if big_endian {
                    u16::from_be_bytes(b)
                } else {
                    u16::from_le_bytes(b)
                })
            } else {
                let b = [sample[0], sample[1], sample[2], sample[3]];
                u64::from(if big_endian {
                    u32::from_be_bytes(b)
                } else {
                    u32::from_le_bytes(b)
                })
            };
            let value = stored_value(raw, meta.bits_stored, meta.high_bit, signed);
            let scaled = value as f64 * scaling;
            if !scaled.is_finite() {
                return Err(Error::new(
                    ErrorKind::InvalidObject,
                    "non-finite scaled dose",
                ));
            }
            out[target_index * per_frame + pixel_index] = scaled;
            if scaled > maximum {
                maximum = scaled;
            }
        }
    }
    Ok((out, maximum))
}
fn stored_value(raw: u64, bits_stored: u16, high_bit: u16, signed: bool) -> i64 {
    let shift = high_bit.wrapping_add(1).wrapping_sub(bits_stored);
    let raw = raw.checked_shr(u32::from(shift)).unwrap_or(0);
    let mask = 1u64
        .checked_shl(u32::from(bits_stored))
        .unwrap_or(0)
        .wrapping_sub(1);
    let raw = raw & mask;
    let sign = 1u64
        .checked_shl(u32::from(bits_stored.wrapping_sub(1)))
        .unwrap_or(0);
    if signed && raw & sign != 0 {
        return (raw | !mask) as i64;
    }
    raw as i64
}
fn read_references(obj: &Object) -> Vec<Reference> {
    let mut out: Vec<Reference> = Vec::new();
    for sequence_tag in [
        TAG_REFERENCED_RT_PLAN_SEQUENCE,
        TAG_REFERENCED_STRUCTURE_SET_SEQ,
    ] {
        for item in d::sequence(obj, sequence_tag) {
            let reference = Reference {
                sop_class_uid: d::clean_uid(&item, d::TAG_REF_SOP_CLASS_UID),
                sop_instance_uid: d::clean_uid(&item, d::TAG_REF_SOP_INSTANCE_UID),
            };
            if reference.sop_instance_uid.is_empty()
                || out
                    .iter()
                    .any(|existing| existing.sop_instance_uid == reference.sop_instance_uid)
            {
                continue;
            }
            out.push(reference);
        }
    }
    out.sort_by(|a, b| a.sop_instance_uid.cmp(&b.sop_instance_uid));
    out
}
fn interpolate_dose(a: f64, b: f64, fraction: f64) -> f64 {
    if (a < 0.0) != (b < 0.0) {
        return a * (1.0 - fraction) + b * fraction;
    }
    a + (b - a) * fraction
}
fn to_lower(s: &str) -> String {
    s.chars()
        .map(|c| {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                _ => c,
            }
        })
        .collect()
}
impl Dose {
    pub fn value_at(&self, column: i64, row: i64, frame: i64) -> Option<f64> {
        if column < 0
            || column >= self.columns
            || row < 0
            || row >= self.rows
            || frame < 0
            || frame >= self.frames
        {
            return None;
        }
        let index = (frame.wrapping_mul(self.rows).wrapping_add(row))
            .wrapping_mul(self.columns)
            .wrapping_add(column);
        usize::try_from(index)
            .ok()
            .and_then(|i| self.values.get(i).copied())
    }
    pub fn sample_patient(&self, point: Vec3) -> Option<f64> {
        let slices = self.geometry.slices();
        let positions = self.geometry.positions();
        if slices.len() as i64 != self.frames
            || positions.len() as i64 != self.frames
            || self.frames == 0
        {
            return None;
        }
        let first = &slices[0];
        let delta = point.sub(first.origin);
        let column = render::grid::clamp_continuous_index(
            delta.dot(first.row_dir) / first.col_spacing,
            self.columns,
            0.0,
        )?;
        let row = render::grid::clamp_continuous_index(
            delta.dot(first.col_dir) / first.row_spacing,
            self.rows,
            0.0,
        )?;
        let position = self.clamp_position(point.dot(self.geometry.normal));
        let (frame0, frame1, fraction) = bracket(positions, position)?;
        let column0 = fmath::f64_to_i64(column.floor());
        let row0 = fmath::f64_to_i64(row.floor());
        let column1 = (column0 + 1).min(self.columns - 1);
        let row1 = (row0 + 1).min(self.rows - 1);
        let column_fraction = column - column0 as f64;
        let row_fraction = row - row0 as f64;
        let sample_frame = |frame: i64| -> Option<f64> {
            let v00 = self.value_at(column0, row0, frame)?;
            let v10 = self.value_at(column1, row0, frame)?;
            let v01 = self.value_at(column0, row1, frame)?;
            let v11 = self.value_at(column1, row1, frame)?;
            let top = interpolate_dose(v00, v10, column_fraction);
            let bottom = interpolate_dose(v01, v11, column_fraction);
            Some(interpolate_dose(top, bottom, row_fraction))
        };
        let v0 = sample_frame(frame0 as i64)?;
        let v1 = sample_frame(frame1 as i64)?;
        Some(interpolate_dose(v0, v1, fraction))
    }
    pub fn display_unit(&self) -> String {
        if self.units == DOSE_UNITS_GRAY {
            match self.r#type.as_str() {
                DOSE_TYPE_PHYSICAL => return "Gy (physical)".to_string(),
                DOSE_TYPE_EFFECTIVE => return "Gy (effective/biological)".to_string(),
                DOSE_TYPE_ERROR => return "Gy (error)".to_string(),
                _ => {}
            }
        }
        if self.units == DOSE_UNITS_RELATIVE {
            return "relative".to_string();
        }
        to_lower(&self.units)
    }
    pub fn provenance_label(&self) -> String {
        format!(
            "RTDOSE {} · {} · {}",
            self.summation_type,
            self.r#type,
            self.display_unit()
        )
    }
    fn clamp_position(&self, position: f64) -> f64 {
        let positions = self.geometry.positions();
        let (first, last) = (positions[0], positions[positions.len() - 1]);
        if position >= first && position <= last {
            return position;
        }
        let half = dose_slice_thickness(self, 0) / 2.0;
        if position < first && position >= first - half {
            return first;
        }
        let half = dose_slice_thickness(self, positions.len() as i64 - 1) / 2.0;
        if position > last && position < last + half {
            return last;
        }
        position
    }
}
fn bracket(values: &[f64], target: f64) -> Option<(usize, usize, f64)> {
    let n = values.len();
    if n == 0 || target < values[0] - 1e-6 || target > values[n - 1] + 1e-6 {
        return None;
    }
    if n == 1 {
        return ((target - values[0]).abs() <= 1e-5).then_some((0, 0, 0.0));
    }
    let index = values.partition_point(|&v| v < target);
    if index == 0 {
        return Some((0, 0, 0.0));
    }
    if index == n {
        return Some((n - 1, n - 1, 0.0));
    }
    if (values[index] - target).abs() <= 1e-9 {
        return Some((index, index, 0.0));
    }
    let (lower, upper) = (index - 1, index);
    let width = values[upper] - values[lower];
    if width <= 0.0 {
        return None;
    }
    Some((lower, upper, (target - values[lower]) / width))
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DvhOptions {
    pub bins: i64,
    pub max_bins: i64,
    pub max_voxels: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DvhBin {
    pub dose: f64,
    pub volume_cc: f64,
    pub volume_percent: f64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DvhResult {
    pub roi_number: i64,
    pub roi_name: String,
    pub dose_unit: String,
    pub summation_type: String,
    pub voxel_count: i64,
    pub volume_cc: f64,
    pub minimum: f64,
    pub maximum: f64,
    pub mean: f64,
    pub d95: f64,
    pub d50: f64,
    pub d2: f64,
    pub partial_overlap: bool,
    pub bins: Vec<DvhBin>,
    pub provenance: String,
}
pub fn compute_dvh(
    dose: Option<&Dose>,
    set: Option<&StructureSet>,
    roi_number: i64,
    opts: DvhOptions,
) -> Result<DvhResult, Error> {
    let (Some(dose), Some(set)) = (dose, set) else {
        return Err(Error::new(
            ErrorKind::InvalidObject,
            "dose and RTSTRUCT are required",
        ));
    };
    if dose.frame_of_reference_uid.is_empty()
        || set.frame_of_reference_uid.is_empty()
        || dose.frame_of_reference_uid != set.frame_of_reference_uid
    {
        return Err(Error::new(
            ErrorKind::GeometryMismatch,
            "RT Dose and RTSTRUCT frame of reference differ",
        ));
    }
    let roi_name = set
        .rois
        .iter()
        .find(|item| item.number == roi_number)
        .map_or("", |item| item.name.as_str());
    if roi_name.is_empty() {
        return Err(Error {
            kind: ErrorKind::Rtstruct(rtstruct::Kind::MissingReference),
            message: format!("dicom/rtstruct: missing reference: ROI {roi_number}"),
        });
    }
    let mut opts = opts;
    if opts.max_voxels <= 0 {
        opts.max_voxels = 64 * 1024 * 1024;
    }
    let total_grid = checked_product(&[dose.columns, dose.rows, dose.frames]);
    match total_grid {
        Some(total) if total <= opts.max_voxels => {}
        _ => {
            return Err(Error::new(
                ErrorKind::MemoryLimit,
                format_args!(
                    "{} dose voxels exceed limit {}",
                    total_grid.unwrap_or(0),
                    opts.max_voxels
                ),
            ));
        }
    }
    if opts.max_bins <= 0 {
        opts.max_bins = 4096;
    }
    if opts.bins <= 0 {
        opts.bins = 512;
    }
    if opts.bins > opts.max_bins {
        return Err(Error::new(
            ErrorKind::MemoryLimit,
            format_args!("{} bins exceed limit {}", opts.bins, opts.max_bins),
        ));
    }
    let bins = opts.bins as usize;
    let segmentation = rtstruct::rasterize(
        set,
        &RasterizeOptions {
            roi_number,
            geometry: dose.geometry.clone(),
            columns: dose.columns,
            rows: dose.rows,
        },
    )
    .map_err(Error::rtstruct)?;
    if segmentation.voxel_count() == 0 {
        return Err(Error::new(
            ErrorKind::GeometryMismatch,
            format_args!("ROI {roi_number} does not overlap the dose grid"),
        ));
    }
    let mut lower = 0.0f64;
    let upper = if dose.maximum.is_nan() {
        f64::NAN
    } else if dose.maximum > 0.0 {
        dose.maximum
    } else {
        0.0
    };
    if dose.r#type == DOSE_TYPE_ERROR {
        for &value in &dose.values {
            if value.is_nan() {
                lower = f64::NAN;
            } else if value < lower {
                lower = value;
            }
        }
    }
    let scale = if (upper - lower) == f64::INFINITY {
        0.5
    } else {
        1.0
    };
    let scaled_lower = lower * scale;
    let span = upper * scale - scaled_lower;
    let mut result = DvhResult {
        roi_number,
        roi_name: roi_name.to_string(),
        dose_unit: dose.display_unit(),
        summation_type: dose.summation_type.clone(),
        minimum: f64::INFINITY,
        maximum: f64::NEG_INFINITY,
        partial_overlap: roi_extends_outside_dose(set, roi_number, dose),
        provenance: dose.provenance_label(),
        ..DvhResult::default()
    };
    let mut histogram = vec![0.0f64; bins];
    let slices = dose.geometry.slices();
    for frame in segmentation.slices() {
        let Some(mask) = segmentation.mask_at(frame) else {
            continue;
        };
        let thickness = dose_slice_thickness(dose, frame);
        if thickness <= 0.0 {
            return Err(Error::new(
                ErrorKind::GeometryMismatch,
                "slice thickness is unavailable",
            ));
        }
        let Some(slice) = usize::try_from(frame).ok().and_then(|i| slices.get(i)) else {
            return Err(Error::of(ErrorKind::GeometryMismatch));
        };
        let voxel_volume_cc = thickness * slice.row_spacing * slice.col_spacing / 1000.0;
        let mut invalid = false;
        mask.for_each_pixel(|column, row| {
            if invalid {
                return;
            }
            let Some(value) = dose.value_at(column, row, frame) else {
                invalid = true;
                return;
            };
            result.voxel_count += 1;
            result.volume_cc += voxel_volume_cc;
            result.mean += value * voxel_volume_cc;
            if value < result.minimum {
                result.minimum = value;
            }
            if value > result.maximum {
                result.maximum = value;
            }
            let mut index: i64 = 0;
            if span > 0.0 {
                index = fmath::f64_to_i64(
                    ((value * scale - scaled_lower) / span * (bins - 1) as f64).floor(),
                );
            }
            let index = index.min(bins as i64 - 1).max(0);
            histogram[index as usize] += voxel_volume_cc;
        });
        if invalid {
            return Err(Error::of(ErrorKind::GeometryMismatch));
        }
    }
    if result.volume_cc <= 0.0 {
        return Err(Error::new(ErrorKind::GeometryMismatch, "zero-volume ROI"));
    }
    result.mean /= result.volume_cc;
    result.bins = vec![DvhBin::default(); bins];
    let mut cumulative = 0.0f64;
    for index in (0..bins).rev() {
        cumulative += histogram[index];
        let dose_value = if bins > 1 {
            interpolate_dose(lower, upper, index as f64 / (bins - 1) as f64)
        } else {
            lower
        };
        result.bins[index] = DvhBin {
            dose: dose_value,
            volume_cc: cumulative,
            volume_percent: cumulative / result.volume_cc * 100.0,
        };
    }
    result.d95 = dose_at_volume(&result.bins, 95.0);
    result.d50 = dose_at_volume(&result.bins, 50.0);
    result.d2 = dose_at_volume(&result.bins, 2.0);
    Ok(result)
}
fn dose_at_volume(bins: &[DvhBin], percent: f64) -> f64 {
    let mut value = 0.0;
    for bin in bins {
        if bin.volume_percent + 1e-9 >= percent {
            value = bin.dose;
        }
    }
    value
}
fn dose_slice_thickness(dose: &Dose, index: i64) -> f64 {
    let positions = dose.geometry.positions();
    let Some(i) = usize::try_from(index).ok().filter(|&i| i < positions.len()) else {
        return 0.0;
    };
    if positions.len() == 1 {
        return dose.slice_thickness;
    }
    if i == 0 {
        return (positions[1] - positions[0]).abs();
    }
    if i == positions.len() - 1 {
        return (positions[i] - positions[i - 1]).abs();
    }
    (positions[i + 1] - positions[i - 1]).abs() / 2.0
}
fn roi_extends_outside_dose(set: &StructureSet, roi_number: i64, dose: &Dose) -> bool {
    let slices = dose.geometry.slices();
    let positions = dose.geometry.positions();
    let Some(first) = slices.first() else {
        return true;
    };
    let first_thickness = dose_slice_thickness(dose, 0);
    let last_thickness = dose_slice_thickness(dose, positions.len() as i64 - 1);
    if first_thickness <= 0.0 || last_thickness <= 0.0 {
        return true;
    }
    let min_position = positions[0] - first_thickness / 2.0;
    let max_position = positions[positions.len() - 1] + last_thickness / 2.0;
    for item in set.rois.iter().filter(|item| item.number == roi_number) {
        for contour in &item.contours {
            for point in &contour.points {
                let patient = Vec3 {
                    x: point.x,
                    y: point.y,
                    z: point.z,
                };
                let delta = patient.sub(first.origin);
                let column = delta.dot(first.row_dir) / first.col_spacing;
                let row = delta.dot(first.col_dir) / first.row_spacing;
                let position = patient.dot(dose.geometry.normal);
                if !render::grid::continuous_index_inside(column, dose.columns)
                    || !render::grid::continuous_index_inside(row, dose.rows)
                    || position < min_position
                    || position >= max_position
                {
                    return true;
                }
            }
        }
    }
    false
}
const MAX_DVH_BINS_PER_ITEM: i64 = 1 << 20;
const MAX_DVH_BINS_PER_REPORT: i64 = 4 << 20;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EmbeddedDvhReport {
    pub structure_set: Reference,
    pub histograms: Vec<EmbeddedDvh>,
    pub diagnostics: Vec<DvhItemDiagnostic>,
}
pub fn read_embedded_dvhs(obj: Option<&Object>) -> Result<EmbeddedDvhReport, Error> {
    let Some(obj) = obj else {
        return Err(Error::new(ErrorKind::InvalidObject, "dataset is nil"));
    };
    if dvh_string(obj, d::TAG_SOP_CLASS_UID, Vr::UI, 64) != RT_DOSE_STORAGE {
        return Err(Error::of(ErrorKind::UnsupportedSopClass));
    }
    let structures = d::sequence(obj, TAG_REFERENCED_STRUCTURE_SET_SEQ);
    if structures.len() != 1 {
        return Err(Error::new(
            ErrorKind::InvalidObject,
            "expected one Referenced Structure Set item",
        ));
    }
    let reference = Reference {
        sop_class_uid: dvh_string(&structures[0], d::TAG_REF_SOP_CLASS_UID, Vr::UI, 64),
        sop_instance_uid: dvh_string(&structures[0], d::TAG_REF_SOP_INSTANCE_UID, Vr::UI, 64),
    };
    if reference.sop_class_uid != RT_STRUCTURE_SET_STORAGE
        || !is_valid_uid(reference.sop_instance_uid.as_bytes())
    {
        return Err(Error::new(
            ErrorKind::InvalidObject,
            "invalid Referenced Structure Set identity",
        ));
    }
    let items = d::sequence(obj, TAG_DVH_SEQUENCE);
    if items.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidObject,
            "expected one nonempty DVH Sequence",
        ));
    }
    let (histograms, diagnostics) = read_embedded_dvh_items(&items, "", "");
    Ok(EmbeddedDvhReport {
        structure_set: reference,
        histograms,
        diagnostics,
    })
}
fn read_embedded_dvh_items(
    items: &[Object],
    default_units: &str,
    default_type: &str,
) -> (Vec<EmbeddedDvh>, Vec<DvhItemDiagnostic>) {
    let mut histograms = Vec::new();
    let mut diagnostics = Vec::new();
    let mut remaining_bins = MAX_DVH_BINS_PER_REPORT;
    for (i, item) in items.iter().enumerate() {
        match read_embedded_dvh_item(
            item,
            default_units,
            default_type,
            remaining_bins.min(MAX_DVH_BINS_PER_ITEM),
        ) {
            Ok(entry) => {
                remaining_bins -= entry.number_of_bins;
                histograms.push(entry);
            }
            Err(tag) => diagnostics.push(DvhItemDiagnostic {
                item: i as i64 + 1,
                tag,
            }),
        }
    }
    (histograms, diagnostics)
}
fn read_embedded_dvh_item(
    item: &Object,
    default_units: &str,
    default_type: &str,
    max_bins: i64,
) -> Result<EmbeddedDvh, Tag> {
    let mut entry = EmbeddedDvh {
        r#type: upper(&dvh_string(item, TAG_DVH_TYPE, Vr::CS, 16)),
        dose_units: upper(&dvh_string(item, TAG_DOSE_UNITS, Vr::CS, 16)),
        dose_type: upper(&dvh_string(item, TAG_DOSE_TYPE, Vr::CS, 16)),
        volume_units: upper(&dvh_string(item, TAG_DVH_VOLUME_UNITS, Vr::CS, 16)),
        ..EmbeddedDvh::default()
    };
    if !item.has(TAG_DOSE_UNITS) {
        entry.dose_units = default_units.to_string();
    }
    if !item.has(TAG_DOSE_TYPE) {
        entry.dose_type = default_type.to_string();
    }
    if !["DIFFERENTIAL", "CUMULATIVE", "NATURAL"].contains(&entry.r#type.as_str()) {
        return Err(TAG_DVH_TYPE);
    }
    if ![DOSE_UNITS_GRAY, DOSE_UNITS_RELATIVE].contains(&entry.dose_units.as_str()) {
        return Err(TAG_DOSE_UNITS);
    }
    if ![DOSE_TYPE_PHYSICAL, DOSE_TYPE_EFFECTIVE, DOSE_TYPE_ERROR]
        .contains(&entry.dose_type.as_str())
    {
        return Err(TAG_DOSE_TYPE);
    }
    if !["CM3", "PERCENT", "PER_U"].contains(&entry.volume_units.as_str()) {
        return Err(TAG_DVH_VOLUME_UNITS);
    }
    let bins = match dvh_integer(item, TAG_DVH_NUMBER_OF_BINS) {
        Some(bins) if bins > 0 && bins <= max_bins => bins,
        _ => return Err(TAG_DVH_NUMBER_OF_BINS),
    };
    let scaling = match dvh_float(item, TAG_DVH_DOSE_SCALING) {
        Some(scaling) if scaling > 0.0 => scaling,
        _ => return Err(TAG_DVH_DOSE_SCALING),
    };
    let data = read_dvh_pairs(item, bins as usize * 2).ok_or(TAG_DVH_DATA)?;
    for pair in data.as_chunks::<2>().0 {
        if !(pair[0] * scaling).is_finite() || pair[1] < 0.0 {
            return Err(TAG_DVH_DATA);
        }
    }
    entry.number_of_bins = bins;
    entry.dose_scaling = scaling;
    entry.data = data;
    for tag in [
        TAG_DVH_MINIMUM_DOSE,
        TAG_DVH_MAXIMUM_DOSE,
        TAG_DVH_MEAN_DOSE,
    ] {
        let Some(element) = item.get(tag) else {
            continue;
        };
        if object::element_length(element) == 0 {
            continue;
        }
        let value = dvh_float(item, tag).ok_or(tag)?;
        if tag == TAG_DVH_MINIMUM_DOSE {
            entry.minimum_dose = value;
        } else if tag == TAG_DVH_MAXIMUM_DOSE {
            entry.maximum_dose = value;
        } else {
            entry.mean_dose = value;
        }
    }
    let rois = d::sequence(item, TAG_DVH_REFERENCED_ROI_SEQUENCE);
    if rois.is_empty() {
        return Err(TAG_DVH_REFERENCED_ROI_SEQUENCE);
    }
    for roi in &rois {
        let number =
            dvh_integer(roi, TAG_REFERENCED_ROI_NUMBER).ok_or(TAG_REFERENCED_ROI_NUMBER)?;
        let contribution = upper(&dvh_string(roi, TAG_DVH_ROI_CONTRIBUTION_TYPE, Vr::CS, 16));
        if contribution != "INCLUDED" && contribution != "EXCLUDED" {
            return Err(TAG_DVH_ROI_CONTRIBUTION_TYPE);
        }
        entry.referenced_rois.push(number);
        entry.roi_contributions.push(DvhRoiContribution {
            number,
            r#type: contribution,
        });
    }
    entry.contribution_type = entry.roi_contributions[0].r#type.clone();
    Ok(entry)
}
fn dvh_string(item: &Object, tag: Tag, vr: Vr, max_length: u32) -> String {
    let Some(element) = item.get(tag) else {
        return String::new();
    };
    if element.vr() != vr || object::element_length(element) > max_length {
        return String::new();
    }
    match item.lookup_strings(tag) {
        Ok(mut values) if values.len() == 1 => values.remove(0),
        _ => String::new(),
    }
}
fn dvh_integer(item: &Object, tag: Tag) -> Option<i64> {
    let text = dvh_string(item, tag, Vr::IS, 12);
    parse_int(text.trim_matches(' '))
        .ok()
        .filter(|v| i32::try_from(*v).is_ok())
}
fn dvh_float(item: &Object, tag: Tag) -> Option<f64> {
    parse_dvh_decimal(&dvh_string(item, tag, Vr::DS, 16))
}
fn parse_dvh_decimal(value: &str) -> Option<f64> {
    if value.len() > 16 || value.contains(['x', 'X', '_']) {
        return None;
    }
    parse_float(value.trim_matches(' '))
        .ok()
        .filter(|v| v.is_finite())
}
fn read_dvh_pairs(item: &Object, count: usize) -> Option<Vec<f64>> {
    let element = item.get(TAG_DVH_DATA)?;
    match &element.value {
        Value::Raw(raw) => {
            let vr = element.vr();
            if (vr != Vr::DS && (vr != Vr::UN || raw.len() <= 65534))
                || raw.len() > count * 17
                || raw.iter().filter(|&&b| b == b'\\').count() != count - 1
            {
                return None;
            }
            let mut raw: &[u8] = raw;
            if raw.len().is_multiple_of(2)
                && let Some(trimmed) = raw.strip_suffix(b" ")
            {
                raw = trimmed;
            }
            raw.split(|&b| b == b'\\')
                .map(|part| std::str::from_utf8(part).ok().and_then(parse_dvh_decimal))
                .collect()
        }
        Value::Strings(values) => {
            if element.vr() != Vr::DS || values.len() != count {
                return None;
            }
            values.iter().map(|v| parse_dvh_decimal(v)).collect()
        }
        _ => None,
    }
}
