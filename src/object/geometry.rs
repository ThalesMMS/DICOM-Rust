#![forbid(unsafe_code)]
use super::error::Error;
use super::obj::Object;
use super::text::clean_text_value;
use super::types::{Tag, Value, Vr};
use crate::fmath;
use crate::numtext;
use crate::textfmt::to_upper;
pub const IMAGE_ORIENTATION_TOLERANCE: f64 = 1e-3;
const TAG_IMAGE_POSITION_PATIENT: Tag = Tag::new(0x0020, 0x0032);
const TAG_IMAGE_ORIENTATION_PATIENT: Tag = Tag::new(0x0020, 0x0037);
const TAG_PIXEL_SPACING: Tag = Tag::new(0x0028, 0x0030);
const TAG_SLICE_THICKNESS: Tag = Tag::new(0x0018, 0x0050);
const TAG_SPACING_BETWEEN_SLICES: Tag = Tag::new(0x0018, 0x0088);
pub(crate) const TAG_SHARED_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9229);
pub(crate) const TAG_PER_FRAME_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9230);
const TAG_PLANE_POSITION_SEQUENCE: Tag = Tag::new(0x0020, 0x9113);
const TAG_PLANE_ORIENTATION_SEQUENCE: Tag = Tag::new(0x0020, 0x9116);
const TAG_PIXEL_MEASURES_SEQUENCE: Tag = Tag::new(0x0028, 0x9110);
const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_FRAME_INCREMENT_POINTER: Tag = Tag::new(0x0028, 0x0009);
const TAG_SLICE_LOCATION_VECTOR: Tag = Tag::new(0x0018, 0x2005);
const TAG_DETECTOR_VECTOR: Tag = Tag::new(0x0054, 0x0020);
const TAG_DETECTOR_INFORMATION_SEQUENCE: Tag = Tag::new(0x0054, 0x0022);
const TAG_SLICE_VECTOR: Tag = Tag::new(0x0054, 0x0080);
const TAG_NUMBER_OF_SLICES: Tag = Tag::new(0x0054, 0x0081);
pub mod geometry_source {
    pub const ABSENT: &str = "";
    pub const IMAGE_PLANE: &str = "image-plane";
    pub const SHARED_FUNCTIONAL_GROUPS: &str = "shared-functional-groups";
    pub const PER_FRAME_FUNCTIONAL_GROUPS: &str = "per-frame-functional-groups";
    #[doc(hidden)]
    pub const NM_DETECTOR: &str = "nm-detector-information";
    #[doc(hidden)]
    pub const NM_SLICE_VECTOR: &str = "nm-slice-vector";
    #[doc(hidden)]
    pub const LEGACY_FRAME_INCREMENT: &str = "legacy-frame-increment";
}
pub mod finding {
    pub const NEGATIVE_SPACING_BETWEEN_SLICES: &str = "negative-spacing-between-slices";
    pub const SPACING_BETWEEN_SLICES_MISSING: &str = "spacing-between-slices-missing";
    pub const SLICE_THICKNESS_AS_SPACING: &str = "slice-thickness-as-spacing";
    pub const SLICE_VECTOR_MISSING: &str = "slice-vector-missing";
    pub const SLICE_VECTOR_OUT_OF_RANGE: &str = "slice-vector-out-of-range";
    pub const FRAME_ORDER_AS_SLICE_INDEX: &str = "frame-order-as-slice-index";
    pub const NM_DETECTOR_AMBIGUOUS: &str = "nm-detector-ambiguous";
    pub const NM_DETECTOR_GEOMETRY_MISSING: &str = "nm-detector-geometry-missing";
    pub const FRAME_INCREMENT_NOT_SPATIAL: &str = "frame-increment-not-spatial";
    pub const SLICE_LOCATION_VECTOR_INVALID: &str = "slice-location-vector-invalid";
}
pub mod spacing_values {
    pub const ABSENT: &str = "";
    pub const PAIR: &str = "pair";
    pub(crate) const SINGLE: &str = "single";
    pub const EXTRA: &str = "extra";
}
pub mod slice_spacing_source {
    pub const UNKNOWN: &str = "";
    #[doc(hidden)]
    pub const FROM_POSITIONS: &str = "positions";
    #[doc(hidden)]
    pub const FROM_SPACING_BETWEEN_SLICES: &str = "spacing-between-slices";
    #[doc(hidden)]
    pub const FROM_SLICE_THICKNESS: &str = "slice-thickness";
    pub const DEFAULT: &str = "default";
}
pub mod functional_group_source {
    pub const NONE: &str = "";
    pub const PER_FRAME: &str = "per-frame";
    pub const SHARED: &str = "shared";
}
pub mod spacing_semantic {
    pub const NONE: &str = "";
    pub const PATIENT: &str = "patient";
    pub const DETECTOR: &str = "detector";
    pub const CALIBRATED: &str = "calibrated";
    pub const RT_IMAGE_PLANE: &str = "rt-image-plane";
    pub const SCANNED: &str = "scanned";
    pub const UNKNOWN: &str = "unknown";
}
use geometry_source as gs;
use slice_spacing_source as sss;
pub const ERR_MALFORMED_SPACING: &str = "dicom: malformed spacing";
pub const ERR_FUNCTIONAL_GROUP_CONFLICT: &str =
    "dicom: functional group macro is in both the shared and the per-frame functional groups";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameGeometryOptions {
    pub derive_legacy_frame_positions: bool,
    pub slice_thickness_as_spacing: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FramePositionDerivation {
    pub origin: Vec<f64>,
    pub normal: Vec<f64>,
    pub offset: f64,
    pub slice_index: i64,
    pub step: f64,
    pub step_source: &'static str,
    pub recovered: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameGeometry {
    pub image_position_patient: Option<Vec<f64>>,
    pub image_orientation_patient: Option<Vec<f64>>,
    pub pixel_spacing: Option<Vec<f64>>,
    pub pixel_spacing_values: &'static str,
    pub pixel_spacing_err: Option<Error>,
    pub slice_thickness: f64,
    pub has_slice_thickness: bool,
    pub spacing_between_slices: f64,
    pub has_spacing_between_slices: bool,
    pub position_source: &'static str,
    pub orientation_source: &'static str,
    pub position_derivation: Option<FramePositionDerivation>,
    pub findings: Vec<&'static str>,
}
fn len_of(v: &Option<Vec<f64>>) -> usize {
    v.as_ref().map_or(0, Vec::len)
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliceSpacing {
    pub value: f64,
    pub source: &'static str,
    pub negative: bool,
}
pub const DEFAULT_SLICE_SPACING_MM: f64 = 1.0;
pub fn resolve_slice_spacing(spacing_between_slices: f64, slice_thickness: f64) -> SliceSpacing {
    if spacing_between_slices != 0.0 && spacing_between_slices.is_finite() {
        return SliceSpacing {
            value: spacing_between_slices.abs(),
            source: sss::FROM_SPACING_BETWEEN_SLICES,
            negative: spacing_between_slices < 0.0,
        };
    }
    if slice_thickness > 0.0 && slice_thickness.is_finite() {
        return SliceSpacing {
            value: slice_thickness,
            source: sss::FROM_SLICE_THICKNESS,
            negative: false,
        };
    }
    SliceSpacing {
        value: DEFAULT_SLICE_SPACING_MM,
        source: sss::DEFAULT,
        negative: false,
    }
}
impl FrameGeometry {
    pub fn slice_spacing(&self) -> SliceSpacing {
        let spacing = if self.has_spacing_between_slices {
            self.spacing_between_slices
        } else {
            0.0
        };
        let thickness = if self.has_slice_thickness {
            self.slice_thickness
        } else {
            0.0
        };
        resolve_slice_spacing(spacing, thickness)
    }
}
pub fn image_orientation_deviation(iop: &[f64]) -> Option<f64> {
    if iop.len() < 6 || iop[..6].iter().any(|v| !v.is_finite()) {
        return None;
    }
    let row = (iop[0] * iop[0] + iop[1] * iop[1] + iop[2] * iop[2]).sqrt();
    let column = (iop[3] * iop[3] + iop[4] * iop[4] + iop[5] * iop[5]).sqrt();
    let dot = iop[0] * iop[3] + iop[1] * iop[4] + iop[2] * iop[5];
    Some(fmath::max(
        (row - 1.0).abs(),
        fmath::max((column - 1.0).abs(), dot.abs()),
    ))
}
pub fn parse_spacing_pair(texts: &[String]) -> Result<(f64, f64, &'static str), Error> {
    let mut cleaned: Vec<String> = texts.iter().map(|t| clean_text_value(Vr::DS, t)).collect();
    while cleaned.last().is_some_and(String::is_empty) {
        cleaned.pop();
    }
    let mut parsed = Vec::with_capacity(2);
    for (i, text) in cleaned.iter().enumerate().take(2) {
        let (value, err) = numtext::parse_float_value(text);
        if err.is_some()
            || value.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater)
            || value.is_infinite()
        {
            return Err(Error::msg(format!(
                "{ERR_MALFORMED_SPACING}: value {} is {}",
                i + 1,
                crate::textfmt::quote(text)
            )));
        }
        parsed.push(value);
    }
    Ok(match cleaned.len() {
        0 => (0.0, 0.0, spacing_values::ABSENT),
        1 => (parsed[0], parsed[0], spacing_values::SINGLE),
        2 => (parsed[0], parsed[1], spacing_values::PAIR),
        _ => (parsed[0], parsed[1], spacing_values::EXTRA),
    })
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PixelSpacingResolution {
    pub row_spacing: f64,
    pub column_spacing: f64,
    pub tag: Tag,
    pub source: &'static str,
    pub semantic: &'static str,
    pub values: &'static str,
    pub calibration_type: String,
    pub calibration_description: String,
    pub magnification_factor: f64,
}
impl PixelSpacingResolution {
    pub fn valid(&self) -> bool {
        !self.semantic.is_empty() && self.row_spacing > 0.0 && self.column_spacing > 0.0
    }
    pub fn estimated_patient_spacing(&self) -> Option<(f64, f64)> {
        if self.semantic != spacing_semantic::DETECTOR
            || self.magnification_factor <= 0.0
            || !self.valid()
        {
            return None;
        }
        Some((
            self.row_spacing / self.magnification_factor,
            self.column_spacing / self.magnification_factor,
        ))
    }
}
#[derive(Clone, Copy)]
struct SpacingValue {
    row: f64,
    column: f64,
    values: &'static str,
}
#[derive(PartialEq, Eq)]
enum Family {
    Other,
    CrossSectional,
    Projection,
    RtImage,
    SecondaryCapture,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionalGroupConflict {
    pub macro_tag: Tag,
    pub frames: Vec<i64>,
}
fn finite_values(values: &[f64]) -> bool {
    values.iter().all(|v| v.is_finite())
}
fn slice_normal(iop: &Option<Vec<f64>>) -> Option<Vec<f64>> {
    let iop = iop.as_ref()?;
    if iop.len() < 6 || !finite_values(&iop[..6]) {
        return None;
    }
    let x = iop[1] * iop[5] - iop[2] * iop[4];
    let y = iop[2] * iop[3] - iop[0] * iop[5];
    let z = iop[0] * iop[4] - iop[1] * iop[3];
    let length = (x * x + y * y + z * z).sqrt();
    if length == 0.0 || !length.is_finite() {
        return None;
    }
    Some(vec![x / length, y / length, z / length])
}
fn set_derived_position(g: &mut FrameGeometry, source: &'static str, d: FramePositionDerivation) {
    let position = (0..3)
        .map(|a| d.origin[a] + d.offset * d.normal[a])
        .collect();
    g.image_position_patient = Some(position);
    g.position_source = source;
    g.position_derivation = Some(d);
}
fn overlay(dst: &mut FrameGeometry, src: FrameGeometry, source: &'static str) {
    if len_of(&src.image_position_patient) >= 3 {
        dst.image_position_patient = src.image_position_patient;
        dst.position_source = source;
    }
    if len_of(&src.image_orientation_patient) >= 6 {
        dst.image_orientation_patient = src.image_orientation_patient;
        dst.orientation_source = source;
    }
    if len_of(&src.pixel_spacing) >= 2 {
        dst.pixel_spacing = src.pixel_spacing;
        dst.pixel_spacing_values = src.pixel_spacing_values;
        dst.pixel_spacing_err = None;
    } else if src.pixel_spacing_err.is_some() {
        dst.pixel_spacing_err = src.pixel_spacing_err;
    }
    if src.has_slice_thickness {
        dst.slice_thickness = src.slice_thickness;
        dst.has_slice_thickness = true;
    }
    if src.has_spacing_between_slices {
        dst.spacing_between_slices = src.spacing_between_slices;
        dst.has_spacing_between_slices = true;
    }
}
impl Object {
    pub fn spacing_pair(&self, tag: Tag) -> Result<(f64, f64, &'static str), Error> {
        let Some(element) = self.get(tag) else {
            return Ok((0.0, 0.0, spacing_values::ABSENT));
        };
        if element.vr() != Vr::DS {
            return Err(Error::msg(format!(
                "{ERR_MALFORMED_SPACING}: {tag} has VR {}, want DS",
                element.vr()
            )));
        }
        let texts = self
            .lookup_strings(tag)
            .map_err(|e| Error::msg(format!("{ERR_MALFORMED_SPACING}: {tag}: {e}")))?;
        parse_spacing_pair(&texts)
    }
    pub fn frame_geometry(&self) -> FrameGeometry {
        let mut g = FrameGeometry::default();
        if let Ok(v) = self.get_floats(TAG_IMAGE_POSITION_PATIENT) {
            g.image_position_patient = Some(v);
            g.position_source = gs::IMAGE_PLANE;
        }
        if let Ok(v) = self.get_floats(TAG_IMAGE_ORIENTATION_PATIENT) {
            g.image_orientation_patient = Some(v);
            g.orientation_source = gs::IMAGE_PLANE;
        }
        match self.spacing_pair(TAG_PIXEL_SPACING) {
            Err(e) => g.pixel_spacing_err = Some(e),
            Ok((row, column, values)) if values != spacing_values::ABSENT => {
                g.pixel_spacing = Some(vec![row, column]);
                g.pixel_spacing_values = values;
            }
            Ok(_) => {}
        }
        if let Ok(v) = self.get_float(TAG_SLICE_THICKNESS) {
            g.slice_thickness = v;
            g.has_slice_thickness = true;
        }
        if let Ok(v) = self.get_float(TAG_SPACING_BETWEEN_SLICES) {
            g.spacing_between_slices = v;
            g.has_spacing_between_slices = true;
        }
        g
    }
    pub fn frame_geometry_at(&self, frame_index: i64) -> FrameGeometry {
        self.frame_geometry_at_with_options(frame_index, FrameGeometryOptions::default())
    }
    pub fn frame_geometry_at_with_options(
        &self,
        frame_index: i64,
        options: FrameGeometryOptions,
    ) -> FrameGeometry {
        let mut geometry = self.frame_geometry();
        for macro_tag in [
            TAG_PLANE_POSITION_SEQUENCE,
            TAG_PLANE_ORIENTATION_SEQUENCE,
            TAG_PIXEL_MEASURES_SEQUENCE,
        ] {
            let (item, source, _) = self.functional_group_item(frame_index, macro_tag);
            if let Some(item) = item {
                let src = if source == functional_group_source::PER_FRAME {
                    gs::PER_FRAME_FUNCTIONAL_GROUPS
                } else {
                    gs::SHARED_FUNCTIONAL_GROUPS
                };
                overlay(&mut geometry, item.frame_geometry(), src);
            }
        }
        if geometry.position_source != gs::SHARED_FUNCTIONAL_GROUPS
            && geometry.position_source != gs::PER_FRAME_FUNCTIONAL_GROUPS
        {
            self.derive_frame_position(frame_index, &mut geometry, options);
        }
        geometry
    }
    pub fn has_functional_groups(&self) -> bool {
        self.has(TAG_SHARED_FUNCTIONAL_GROUPS) || self.has(TAG_PER_FRAME_FUNCTIONAL_GROUPS)
    }
    pub fn functional_group_item(
        &self,
        frame_index: i64,
        macro_tag: Tag,
    ) -> (Option<Object>, &'static str, Option<Error>) {
        let (items, source, err) = self.functional_group_items(frame_index, macro_tag);
        (items.into_iter().next(), source, err)
    }
    pub fn functional_group_items(
        &self,
        frame_index: i64,
        macro_tag: Tag,
    ) -> (Vec<Object>, &'static str, Option<Error>) {
        let shared = self
            .sequence_item_at(TAG_SHARED_FUNCTIONAL_GROUPS, 0)
            .and_then(|g| g.get_sequence(macro_tag))
            .unwrap_or_default();
        let frame = self
            .sequence_item_at(TAG_PER_FRAME_FUNCTIONAL_GROUPS, frame_index)
            .and_then(|g| g.get_sequence(macro_tag))
            .unwrap_or_default();
        if !frame.is_empty() {
            let err = (!shared.is_empty())
                .then(|| Error::msg(format!("{ERR_FUNCTIONAL_GROUP_CONFLICT}: {macro_tag}")));
            return (frame, functional_group_source::PER_FRAME, err);
        }
        if !shared.is_empty() {
            return (shared, functional_group_source::SHARED, None);
        }
        (Vec::new(), functional_group_source::NONE, None)
    }
    pub fn functional_group_conflicts(&self) -> Vec<FunctionalGroupConflict> {
        let Some(shared) = self.sequence_item_at(TAG_SHARED_FUNCTIONAL_GROUPS, 0) else {
            return Vec::new();
        };
        let frames = self
            .get_sequence(TAG_PER_FRAME_FUNCTIONAL_GROUPS)
            .unwrap_or_default();
        let mut conflicts = Vec::new();
        for element in shared.elements() {
            if element.vr() != Vr::SQ {
                continue;
            }
            let macro_tag = element.tag();
            let frames: Vec<i64> = frames
                .iter()
                .enumerate()
                .filter(|(_, f)| f.has(macro_tag))
                .map(|(i, _)| i as i64)
                .collect();
            if !frames.is_empty() {
                conflicts.push(FunctionalGroupConflict { macro_tag, frames });
            }
        }
        conflicts.sort_by_key(|c| c.macro_tag);
        conflicts
    }
    pub fn get_tags(&self, tag: Tag) -> Option<Vec<Tag>> {
        let element = self.get(tag)?;
        if element.vr() != Vr::AT {
            return None;
        }
        let Value::Raw(raw) = &element.value else {
            return None;
        };
        if raw.is_empty() || raw.len() % 4 != 0 {
            return None;
        }
        let big = self.value_big_endian();
        let u16_at = |b: &[u8]| {
            if big {
                u16::from_be_bytes([b[0], b[1]])
            } else {
                u16::from_le_bytes([b[0], b[1]])
            }
        };
        Some(
            raw.as_chunks::<4>()
                .0
                .iter()
                .map(|c| Tag::new(u16_at(&c[..2]), u16_at(&c[2..])))
                .collect(),
        )
    }
    fn derive_frame_position(
        &self,
        frame_index: i64,
        g: &mut FrameGeometry,
        options: FrameGeometryOptions,
    ) {
        if frame_index < 0 {
            return;
        }
        let pointers = self
            .get_tags(TAG_FRAME_INCREMENT_POINTER)
            .unwrap_or_default();
        if self.is_nm_reconstruction(&pointers) {
            self.derive_nm_slice_position(frame_index, g, options);
            return;
        }
        if options.derive_legacy_frame_positions {
            self.derive_legacy_frame_position(frame_index, &pointers, g);
        }
    }
    fn is_nm_reconstruction(&self, pointers: &[Tag]) -> bool {
        if pointers.contains(&TAG_SLICE_VECTOR) {
            return true;
        }
        let Some(values) = self.get_strings(TAG_IMAGE_TYPE) else {
            return false;
        };
        if values.len() < 3 {
            return false;
        }
        matches!(
            to_upper(values[2].trim()).as_str(),
            "RECON TOMO" | "RECON GATED TOMO"
        )
    }
    fn nm_detector_item(&self, frame_index: i64) -> (Option<Object>, &'static str) {
        let items = match self.get_sequence(TAG_DETECTOR_INFORMATION_SEQUENCE) {
            Some(items) if !items.is_empty() => items,
            _ => return (None, ""),
        };
        if items.len() == 1 {
            return (items.into_iter().next(), "");
        }
        if let Ok(vector) = self.get_ints(TAG_DETECTOR_VECTOR)
            && (frame_index as usize) < vector.len()
        {
            let number = vector[frame_index as usize];
            if number >= 1 && number <= items.len() as i64 {
                return (items.into_iter().nth(number as usize - 1), "");
            }
        }
        (None, finding::NM_DETECTOR_AMBIGUOUS)
    }
    fn derive_nm_slice_position(
        &self,
        frame_index: i64,
        g: &mut FrameGeometry,
        options: FrameGeometryOptions,
    ) {
        let (detector, found) = self.nm_detector_item(frame_index);
        if !found.is_empty() {
            g.findings.push(found);
            return;
        }
        let mut origin: Option<Vec<f64>> = None;
        if let Some(detector) = detector {
            let item = detector.frame_geometry();
            if len_of(&item.image_orientation_patient) >= 6 {
                g.image_orientation_patient = item.image_orientation_patient;
                g.orientation_source = gs::NM_DETECTOR;
            }
            if let Some(ipp) = item.image_position_patient
                && ipp.len() >= 3
            {
                origin = Some(ipp[..3].to_vec());
            }
        }
        if origin.is_none()
            && let Some(ipp) = &g.image_position_patient
            && ipp.len() >= 3
        {
            origin = Some(ipp[..3].to_vec());
        }
        let normal = slice_normal(&g.image_orientation_patient);
        let (Some(origin), Some(normal)) = (origin, normal) else {
            g.findings.push(finding::NM_DETECTOR_GEOMETRY_MISSING);
            return;
        };
        if !finite_values(&origin) {
            g.findings.push(finding::NM_DETECTOR_GEOMETRY_MISSING);
            return;
        }
        let mut recovered = false;
        let slice_index;
        match self.get_ints(TAG_SLICE_VECTOR) {
            Ok(numbers) if (frame_index as usize) < numbers.len() && !numbers.is_empty() => {
                let mut limit = i64::from(i32::MAX);
                if let Ok(slices) = self.get_int(TAG_NUMBER_OF_SLICES)
                    && slices > 0
                {
                    limit = slices;
                }
                let (first, current) = (numbers[0], numbers[frame_index as usize]);
                if first < 1 || current < 1 || first > limit || current > limit {
                    g.findings.push(finding::SLICE_VECTOR_OUT_OF_RANGE);
                    return;
                }
                slice_index = current - first;
            }
            _ if options.derive_legacy_frame_positions => {
                slice_index = frame_index;
                recovered = true;
                g.findings.push(finding::FRAME_ORDER_AS_SLICE_INDEX);
            }
            _ => {
                g.findings.push(finding::SLICE_VECTOR_MISSING);
                return;
            }
        }
        let spacing = g.slice_spacing();
        let (step, step_source);
        if spacing.source == sss::FROM_SPACING_BETWEEN_SLICES {
            step = g.spacing_between_slices;
            step_source = spacing.source;
            if spacing.negative {
                g.findings.push(finding::NEGATIVE_SPACING_BETWEEN_SLICES);
            }
        } else if spacing.source == sss::FROM_SLICE_THICKNESS && options.slice_thickness_as_spacing
        {
            step = spacing.value;
            step_source = spacing.source;
            recovered = true;
            g.findings.push(finding::SLICE_THICKNESS_AS_SPACING);
        } else {
            g.findings.push(finding::SPACING_BETWEEN_SLICES_MISSING);
            return;
        }
        set_derived_position(
            g,
            gs::NM_SLICE_VECTOR,
            FramePositionDerivation {
                origin,
                normal,
                offset: slice_index as f64 * step,
                slice_index,
                step,
                step_source,
                recovered,
            },
        );
    }
    fn derive_legacy_frame_position(
        &self,
        frame_index: i64,
        pointers: &[Tag],
        g: &mut FrameGeometry,
    ) {
        if g.position_source != gs::IMAGE_PLANE
            || g.orientation_source != gs::IMAGE_PLANE
            || len_of(&g.image_position_patient) < 3
        {
            return;
        }
        let origin = g.image_position_patient.as_ref().expect("checked")[..3].to_vec();
        if !finite_values(&origin) {
            return;
        }
        let frames = match self.get_int(TAG_NUMBER_OF_FRAMES) {
            Ok(n) if n >= 2 && frame_index < n => n,
            _ => return,
        };
        let Some(normal) = slice_normal(&g.image_orientation_patient) else {
            return;
        };
        let mut d = FramePositionDerivation {
            origin,
            normal,
            recovered: true,
            ..FramePositionDerivation::default()
        };
        if pointers.is_empty() {
            let spacing = g.slice_spacing();
            if spacing.source == sss::FROM_SPACING_BETWEEN_SLICES {
                d.step = g.spacing_between_slices;
                if spacing.negative {
                    g.findings.push(finding::NEGATIVE_SPACING_BETWEEN_SLICES);
                }
            } else if spacing.source == sss::FROM_SLICE_THICKNESS {
                d.step = spacing.value;
            } else {
                g.findings.push(finding::SPACING_BETWEEN_SLICES_MISSING);
                return;
            }
            d.step_source = spacing.source;
            d.slice_index = frame_index;
            d.offset = frame_index as f64 * d.step;
        } else if pointers.len() == 1 && pointers[0] == TAG_SLICE_LOCATION_VECTOR {
            match self.get_floats(TAG_SLICE_LOCATION_VECTOR) {
                Ok(locations) if locations.len() as i64 == frames && finite_values(&locations) => {
                    d.offset = locations[frame_index as usize] - locations[0];
                }
                _ => {
                    g.findings.push(finding::SLICE_LOCATION_VECTOR_INVALID);
                    return;
                }
            }
        } else {
            g.findings.push(finding::FRAME_INCREMENT_NOT_SPATIAL);
            return;
        }
        set_derived_position(g, gs::LEGACY_FRAME_INCREMENT, d);
    }
    fn positive_spacing(&self, tag: Tag) -> Option<SpacingValue> {
        match self.spacing_pair(tag) {
            Ok((row, column, values)) if values != spacing_values::ABSENT => Some(SpacingValue {
                row,
                column,
                values,
            }),
            _ => None,
        }
    }
    fn functional_spacing(
        &self,
        frame_index: i64,
        macro_tag: Tag,
        tag: Tag,
    ) -> (Option<SpacingValue>, &'static str) {
        let (item, source, _) = self.functional_group_item(frame_index, macro_tag);
        if let Some(item) = item
            && let Some(values) = item.positive_spacing(tag)
        {
            return (Some(values), source);
        }
        (self.positive_spacing(tag), functional_group_source::NONE)
    }
    fn spacing_family(&self) -> Family {
        let sop = self
            .get_string(Tag::new(0x0008, 0x0016))
            .unwrap_or_default();
        let sop = sop.trim().trim_end_matches('\0');
        if sop == "1.2.840.10008.5.1.4.1.1.481.1" {
            return Family::RtImage;
        }
        if sop == "1.2.840.10008.5.1.4.1.1.7" || sop.starts_with("1.2.840.10008.5.1.4.1.1.7.") {
            return Family::SecondaryCapture;
        }
        if sop.starts_with("1.2.840.10008.5.1.4.1.1.12.") {
            return Family::Projection;
        }
        let modality = self
            .get_string(Tag::new(0x0008, 0x0060))
            .unwrap_or_default();
        match to_upper(modality.trim()).as_str() {
            "CT" | "MR" | "PT" | "NM" => return Family::CrossSectional,
            "CR" | "DX" | "MG" | "IO" | "PX" | "XA" | "RF" | "RG" | "DF" => {
                return Family::Projection;
            }
            "RTIMAGE" => return Family::RtImage,
            _ => {}
        }
        if self.has_functional_groups() {
            return Family::CrossSectional;
        }
        Family::Other
    }
    pub fn pixel_spacing_at(&self, frame_index: i64) -> PixelSpacingResolution {
        use spacing_semantic as sem;
        let calibration_type = to_upper(
            self.get_string(Tag::new(0x0028, 0x0A02))
                .unwrap_or_default()
                .trim(),
        );
        let description = self
            .get_string(Tag::new(0x0028, 0x0A04))
            .unwrap_or_default();
        let magnification = self.get_float(Tag::new(0x0018, 0x1114)).unwrap_or(0.0);
        let mut base = PixelSpacingResolution {
            calibration_type: calibration_type.clone(),
            calibration_description: description.trim().to_string(),
            ..PixelSpacingResolution::default()
        };
        if magnification > 0.0 {
            base.magnification_factor = magnification;
        }
        let with = |v: SpacingValue, tag: Tag, source: &'static str, semantic: &'static str| {
            let mut out = base.clone();
            out.row_spacing = v.row;
            out.column_spacing = v.column;
            out.values = v.values;
            out.tag = tag;
            out.source = source;
            out.semantic = semantic;
            out
        };
        let (pixel_spacing, ps_source) =
            self.functional_spacing(frame_index, TAG_PIXEL_MEASURES_SEQUENCE, TAG_PIXEL_SPACING);
        match self.spacing_family() {
            Family::CrossSectional => {
                if let Some(ps) = pixel_spacing {
                    return with(ps, TAG_PIXEL_SPACING, ps_source, sem::PATIENT);
                }
            }
            Family::Projection => {
                let imager_tag = Tag::new(0x0018, 0x1164);
                let (imager, imager_source) =
                    self.functional_spacing(frame_index, Tag::new(0x0028, 0x9443), imager_tag);
                match (pixel_spacing, imager) {
                    (Some(ps), _) if !calibration_type.is_empty() => {
                        return with(ps, TAG_PIXEL_SPACING, ps_source, sem::CALIBRATED);
                    }
                    (Some(ps), Some(im))
                        if (ps.row - im.row).abs() <= 1e-6
                            && (ps.column - im.column).abs() <= 1e-6 =>
                    {
                        return with(ps, TAG_PIXEL_SPACING, ps_source, sem::DETECTOR);
                    }
                    (Some(ps), _) => {
                        return with(ps, TAG_PIXEL_SPACING, ps_source, sem::UNKNOWN);
                    }
                    (None, Some(im)) => {
                        return with(im, imager_tag, imager_source, sem::DETECTOR);
                    }
                    (None, None) => {}
                }
            }
            Family::RtImage => {
                let tag = Tag::new(0x3002, 0x0011);
                if let Some(v) = self.positive_spacing(tag) {
                    return with(v, tag, functional_group_source::NONE, sem::RT_IMAGE_PLANE);
                }
            }
            Family::SecondaryCapture => {
                let tag = Tag::new(0x0018, 0x2010);
                let scanned = self.positive_spacing(tag);
                if let Some(ps) = pixel_spacing
                    && !calibration_type.is_empty()
                {
                    return with(ps, TAG_PIXEL_SPACING, ps_source, sem::CALIBRATED);
                }
                if let Some(v) = scanned {
                    return with(v, tag, functional_group_source::NONE, sem::SCANNED);
                }
            }
            Family::Other => {}
        }
        if let Some(ps) = pixel_spacing {
            return with(ps, TAG_PIXEL_SPACING, ps_source, sem::UNKNOWN);
        }
        base
    }
}
