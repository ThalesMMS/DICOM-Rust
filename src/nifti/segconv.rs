use super::export::strict_slice_geometry;
use super::geometry::invert3;
use super::header::nearly_equal;
use super::label_table::{LabelTable, trim_space};
use super::{Sentinel, Volume, XformSource, finite, finite_positive};
use crate::fmath;
use crate::object::{File, Tag};
use crate::pixel::pixeldata;
use crate::render::grid::nearest_index;
use crate::render::stablesort::slice_stable;
use crate::render::{
    GeometryTolerances, SliceGeometry, Vec3, VolumeGeometry, build_volume_geometry,
};
use crate::roi::RasterMask;
use crate::seg::{self, ReferencedImage, RoiMask, Segment};
use std::collections::BTreeMap;
use std::fmt;
pub const DEFAULT_SEG_VOXEL_TOLERANCE: f64 = 0.01;
const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SegReferenceImage {
    pub geometry: SliceGeometry,
    pub image: ReferencedImage,
}
#[derive(Debug, Clone, Default)]
pub struct SegOptions {
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub references: Vec<SegReferenceImage>,
    pub segments: Option<BTreeMap<i64, Segment>>,
    pub time_index: i64,
    pub voxel_tolerance: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SegDocumentFailure<E = SegError> {
    pub error: E,
    pub report: SegReport,
}
impl<E: std::fmt::Display> std::fmt::Display for SegDocumentFailure<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl<E: std::error::Error> std::error::Error for SegDocumentFailure<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SegReport {
    pub axis: [i64; 3],
    pub flipped: [bool; 3],
    pub offset: [i64; 3],
    pub segment_numbers: Option<BTreeMap<i64, i64>>,
    pub frames: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub enum SegCause {
    Nifti(Sentinel),
    GeometryMismatch,
    InvalidLabel,
    OutsideReference,
    Empty,
    Document(seg::Error),
    Mask(String),
}
impl SegCause {
    pub fn text(&self) -> String {
        match self {
            SegCause::Nifti(s) => s.text().to_string(),
            SegCause::GeometryMismatch => {
                "dicom/nifti: labelmap grid does not match the referenced series".into()
            }
            SegCause::InvalidLabel => "dicom/nifti: invalid labelmap value".into(),
            SegCause::OutsideReference => "dicom/nifti: label outside the referenced series".into(),
            SegCause::Empty => "dicom/nifti: labelmap has no labels".into(),
            SegCause::Document(e) => e.to_string(),
            SegCause::Mask(text) => text.clone(),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct SegError {
    pub field: &'static str,
    pub cause: SegCause,
}
impl fmt::Display for SegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dicom/nifti: read seg ({})", self.field)
    }
}
impl std::error::Error for SegError {}
fn seg_error(field: &'static str, cause: SegCause) -> SegError {
    SegError { field, cause }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegReferencesError {
    pub text: String,
    pub sentinel: Sentinel,
}
impl fmt::Display for SegReferencesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for SegReferencesError {}
pub fn seg_references(
    files: &[Option<&File>],
) -> Result<Vec<SegReferenceImage>, SegReferencesError> {
    let mut references = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let Some(file) = file else {
            return Err(SegReferencesError {
                text: format!("{}: file {}", Sentinel::InvalidSource.text(), index + 1),
                sentinel: Sentinel::InvalidSource,
            });
        };
        let metadata =
            pixeldata::extract_metadata(&file.dataset).map_err(|e| SegReferencesError {
                text: format!(
                    "{}: file {}: {e}",
                    Sentinel::InvalidGeometry.text(),
                    index + 1
                ),
                sentinel: Sentinel::InvalidGeometry,
            })?;
        let uid = |tag| file.dataset.get_uid(tag).unwrap_or_default();
        let image = ReferencedImage {
            series_instance_uid: uid(TAG_SERIES_INSTANCE_UID),
            sop_class_uid: uid(TAG_SOP_CLASS_UID),
            sop_instance_uid: uid(TAG_SOP_INSTANCE_UID),
            frames: Vec::new(),
        };
        let frames = metadata.number_of_frames.max(1);
        for frame in 0..frames {
            let geometry = strict_slice_geometry(
                &file.dataset.frame_geometry_at(frame),
                i64::from(metadata.rows),
                i64::from(metadata.columns),
            )
            .ok_or_else(|| SegReferencesError {
                text: format!(
                    "file {} frame {}: {}",
                    index + 1,
                    frame + 1,
                    Sentinel::InvalidGeometry.text()
                ),
                sentinel: Sentinel::InvalidGeometry,
            })?;
            let mut reference = SegReferenceImage {
                geometry,
                image: image.clone(),
            };
            if frames > 1 {
                reference.image.frames = vec![frame + 1];
            }
            references.push(reference);
        }
    }
    Ok(references)
}
pub fn label_table_segments(table: &LabelTable) -> Option<BTreeMap<i64, Segment>> {
    if table.is_empty() {
        return None;
    }
    let mut segments = BTreeMap::new();
    for (&value, entry) in table {
        let mut name = String::from_utf8_lossy(trim_space(&entry.name)).into_owned();
        if name.is_empty() {
            name = format!("Label {value}");
        }
        segments.insert(
            value,
            Segment {
                label: name,
                algorithm_type: seg::ALGORITHM_MANUAL.into(),
                recommended_display_cielab: seg::rgb_to_dicom_cielab([
                    i64::from(entry.rgb[0]),
                    i64::from(entry.rgb[1]),
                    i64::from(entry.rgb[2]),
                ]),
                segmented_property_category: seg::generic_roi_category_code(),
                segmented_property_type: seg::generic_roi_type_code(),
                ..Segment::default()
            },
        );
    }
    Some(segments)
}
struct ReferenceGrid {
    references: Vec<SegReferenceImage>,
    volume: VolumeGeometry,
    columns: i64,
    rows: i64,
    index_to_lps: [Vec3; 3],
    origin: Vec3,
}
#[derive(Debug, Clone, Copy, Default)]
struct GridMapping {
    axis: [usize; 3],
    flipped: [bool; 3],
    offset: [i64; 3],
}
fn finite_vec(v: Vec3) -> bool {
    finite(v.x) && finite(v.y) && finite(v.z)
}
fn mismatch(field: &'static str) -> SegError {
    seg_error(field, SegCause::GeometryMismatch)
}
fn new_reference_grid(
    references: &[SegReferenceImage],
    single_slice_spacing: f64,
    tolerance: f64,
) -> Result<ReferenceGrid, SegError> {
    let Some(first) = references.first().map(|r| r.geometry) else {
        return Err(mismatch("references"));
    };
    let mut sorted = references.to_vec();
    slice_stable(&mut sorted, |a, b| {
        a.geometry.position_along(first.normal) < b.geometry.position_along(first.normal)
    });
    let mut grid = ReferenceGrid {
        origin: sorted[0].geometry.origin,
        references: sorted,
        volume: VolumeGeometry::default(),
        columns: first.columns,
        rows: first.rows,
        index_to_lps: [Vec3::default(); 3],
    };
    if grid.columns <= 0 || grid.rows <= 0 || grid.columns > 1 << 16 || grid.rows > 1 << 16 {
        return Err(mismatch("references"));
    }
    if !finite_vec(first.row_dir)
        || !finite_vec(first.col_dir)
        || !finite_vec(first.normal)
        || !finite_positive(first.col_spacing)
        || !finite_positive(first.row_spacing)
    {
        return Err(mismatch("references"));
    }
    grid.index_to_lps[0] = first.row_dir.normalize().scale(first.col_spacing);
    grid.index_to_lps[1] = first.col_dir.normalize().scale(first.row_spacing);
    let n = grid.references.len();
    if n == 1 {
        grid.index_to_lps[2] = first.normal.normalize().scale(single_slice_spacing);
    } else {
        let span = grid.references[n - 1]
            .geometry
            .origin
            .sub(grid.references[0].geometry.origin);
        grid.index_to_lps[2] = span.scale(1.0 / (n - 1) as f64);
    }
    let smallest = fmath::min(
        first.col_spacing,
        fmath::min(first.row_spacing, grid.index_to_lps[2].length()),
    );
    if !finite_positive(smallest) {
        return Err(mismatch("references"));
    }
    let mut slices = Vec::with_capacity(n);
    for (index, reference) in grid.references.iter().enumerate() {
        let g = reference.geometry;
        slices.push(g);
        let expected = grid.origin.add(grid.index_to_lps[2].scale(index as f64));
        if g.columns != grid.columns
            || g.rows != grid.rows
            || g.row_dir.normalize().dot(first.row_dir.normalize()) < 1.0 - 1e-9
            || g.col_dir.normalize().dot(first.col_dir.normalize()) < 1.0 - 1e-9
            || !nearly_equal(g.col_spacing, first.col_spacing, 1e-6)
            || !nearly_equal(g.row_spacing, first.row_spacing, 1e-6)
            || !finite_vec(g.origin)
            || g.origin.sub(expected).length() > tolerance * smallest
        {
            return Err(mismatch("references"));
        }
    }
    grid.volume = build_volume_geometry(&slices, GeometryTolerances::default());
    Ok(grid)
}
impl ReferenceGrid {
    fn mapping(&self, v: &Volume, tolerance: f64) -> Result<GridMapping, SegError> {
        let mut basis = [[0f64; 3]; 3];
        for column in 0..3 {
            let c = self.index_to_lps[column];
            basis[0][column] = c.x;
            basis[1][column] = c.y;
            basis[2][column] = c.z;
        }
        let Some(inverse) = invert3(&basis) else {
            return Err(mismatch("references"));
        };
        let a = v.geometry.index_to_lps;
        let to_grid = |p: Vec3| -> [f64; 3] {
            let world = a.transform_point(p).sub(self.origin);
            let mut out = [0f64; 3];
            for (r, slot) in out.iter_mut().enumerate() {
                *slot = inverse[r][0] * world.x + inverse[r][1] * world.y + inverse[r][2] * world.z;
            }
            out
        };
        let origin = to_grid(Vec3::default());
        let mut out = GridMapping::default();
        let mut used = [false; 3];
        for axis in 0..3 {
            let mut step = Vec3::default();
            match axis {
                0 => step.x = 1.0,
                1 => step.y = 1.0,
                _ => step.z = 1.0,
            }
            let moved = to_grid(step);
            let (mut best, mut best_value) = (None, 0.0f64);
            for r in 0..3 {
                let delta = moved[r] - origin[r];
                if delta.abs() > best_value.abs() {
                    (best, best_value) = (Some(r), delta);
                }
            }
            let Some(best) = best else {
                return Err(mismatch("xform"));
            };
            if used[best] || (best_value.abs() - 1.0).abs() > tolerance {
                return Err(mismatch("xform"));
            }
            used[best] = true;
            out.axis[axis] = best;
            out.flipped[axis] = best_value < 0.0;
        }
        for r in 0..3 {
            if !(origin[r].abs() <= i32::MAX as f64) {
                return Err(mismatch("xform"));
            }
            let rounded = nearest_index(origin[r]) as f64;
            if (origin[r] - rounded).abs() > tolerance {
                return Err(mismatch("xform"));
            }
            out.offset[r] = rounded as i64;
        }
        let d = v.dimensions;
        let extent = [(d[0] - 1) as f64, (d[1] - 1) as f64, (d[2] - 1) as f64];
        for corner in 0..8 {
            let mut p = [0f64; 3];
            for axis in 0..3 {
                if corner & (1 << axis) != 0 {
                    p[axis] = extent[axis];
                }
            }
            let actual = to_grid(Vec3::new(p[0], p[1], p[2]));
            let predicted = out.to_grid_float(p);
            for r in 0..3 {
                if (actual[r] - predicted[r]).abs() > tolerance {
                    return Err(mismatch("xform"));
                }
            }
        }
        Ok(out)
    }
}
impl GridMapping {
    #[allow(clippy::wrong_self_convention)]
    fn to_grid_float(&self, p: [f64; 3]) -> [f64; 3] {
        let mut out = [
            self.offset[0] as f64,
            self.offset[1] as f64,
            self.offset[2] as f64,
        ];
        for axis in 0..3 {
            if self.flipped[axis] {
                out[self.axis[axis]] -= p[axis];
            } else {
                out[self.axis[axis]] += p[axis];
            }
        }
        out
    }
    #[allow(clippy::wrong_self_convention)]
    fn to_grid(&self, source: [i64; 3]) -> [i64; 3] {
        let mut out = self.offset;
        for axis in 0..3 {
            if self.flipped[axis] {
                out[self.axis[axis]] -= source[axis];
            } else {
                out[self.axis[axis]] += source[axis];
            }
        }
        out
    }
    #[allow(clippy::wrong_self_convention)]
    fn to_nifti(&self, target: [i64; 3], dimensions: [i64; 4]) -> Option<[i64; 3]> {
        let mut out = [0i64; 3];
        for axis in 0..3 {
            let mut delta = target[self.axis[axis]] - self.offset[self.axis[axis]];
            if self.flipped[axis] {
                delta = -delta;
            }
            if delta < 0 || delta >= dimensions[axis] {
                return None;
            }
            out[axis] = delta;
        }
        Some(out)
    }
}
impl Volume {
    pub fn seg_document(
        &self,
        options: &SegOptions,
    ) -> Result<(seg::Document, SegReport), Box<SegDocumentFailure>> {
        let mut report = SegReport::default();
        macro_rules! fail {
            ($e:expr) => {
                return Err(Box::new(SegDocumentFailure { error: $e, report }))
            };
        }
        if self.bytes_per_voxel == 0 {
            fail!(seg_error(
                "volume",
                SegCause::Nifti(Sentinel::InvalidSource)
            ));
        }
        if self.geometry.source == XformSource::PixDim {
            fail!(mismatch("xform"));
        }
        if options.time_index < 0 || options.time_index >= self.dimensions[3] {
            fail!(seg_error(
                "time",
                SegCause::Nifti(Sentinel::UnsupportedDimensions)
            ));
        }
        let mut tolerance = options.voxel_tolerance;
        if tolerance == 0.0 {
            tolerance = DEFAULT_SEG_VOXEL_TOLERANCE;
        }
        if !finite_positive(tolerance) || tolerance >= 0.5 {
            fail!(mismatch("tolerance"));
        }
        let grid =
            match new_reference_grid(&options.references, self.geometry.spacing[2], tolerance) {
                Ok(g) => g,
                Err(e) => fail!(e),
            };
        let mapping = match grid.mapping(self, tolerance) {
            Ok(m) => m,
            Err(e) => fail!(e),
        };
        report.axis = mapping.axis.map(|a| a as i64);
        report.flipped = mapping.flipped;
        report.offset = mapping.offset;
        let labels = match self.validate_labels(options, &grid, &mapping) {
            Ok(l) => l,
            Err(e) => fail!(e),
        };
        let (segments, numbers) = match segment_definitions(&labels, options.segments.as_ref()) {
            Ok(s) => s,
            Err(e) => fail!(e),
        };
        report.segment_numbers = Some(numbers);
        let mut masks_by_segment: BTreeMap<i64, Vec<RoiMask>> = BTreeMap::new();
        let (columns, rows) = (grid.columns, grid.rows);
        let mut values = vec![0u16; (columns * rows) as usize];
        let mut buffer = vec![0u8; (columns * rows) as usize];
        let mut present = vec![false; 1 << 16];
        for (slice, reference) in grid.references.iter().enumerate() {
            values.fill(0);
            let mut slice_labels = Vec::new();
            for row in 0..rows {
                for column in 0..columns {
                    let Some(source) =
                        mapping.to_nifti([column, row, slice as i64], self.dimensions)
                    else {
                        continue;
                    };
                    let index = self.index(source[0], source[1], source[2], options.time_index);
                    let label = self.value(index as usize) as u16;
                    if label == 0 {
                        continue;
                    }
                    values[(row * columns + column) as usize] = label;
                    if !present[label as usize] {
                        present[label as usize] = true;
                        slice_labels.push(i64::from(label));
                    }
                }
            }
            slice_labels.sort_unstable();
            for &label in &slice_labels {
                present[label as usize] = false;
                for (b, &value) in buffer.iter_mut().zip(&values) {
                    *b = u8::from(i64::from(value) == label);
                }
                let mask = match RasterMask::from_bytes(columns, rows, &buffer) {
                    Ok(m) => m,
                    Err(e) => fail!(seg_error("mask", SegCause::Mask(e.to_string()))),
                };
                let segment = segments[&label].clone();
                masks_by_segment
                    .entry(segment.number)
                    .or_default()
                    .push(RoiMask {
                        segment,
                        slice_index: slice as i64,
                        source_image: reference.image.clone(),
                        geometry: reference.geometry,
                        mask: Some(mask),
                    });
            }
        }
        let masks: Vec<RoiMask> = masks_by_segment.into_values().flatten().collect();
        let document = match seg::from_roi_masks(&seg::FromRoiMasksOptions {
            sop_instance_uid: options.sop_instance_uid.clone(),
            study_instance_uid: options.study_instance_uid.clone(),
            series_instance_uid: options.series_instance_uid.clone(),
            frame_of_reference_uid: options.frame_of_reference_uid.clone(),
            rows,
            columns,
            geometry: grid.volume.clone(),
            masks,
        }) {
            Ok(d) => d,
            Err(e) => fail!(seg_error("document", SegCause::Document(e))),
        };
        report.frames = document.frames.len() as i64;
        Ok((document, report))
    }
    fn validate_labels(
        &self,
        options: &SegOptions,
        grid: &ReferenceGrid,
        mapping: &GridMapping,
    ) -> Result<Vec<i64>, SegError> {
        let mut present = std::collections::BTreeSet::new();
        let bounds = [grid.columns, grid.rows, grid.references.len() as i64];
        let d = self.dimensions;
        for k in 0..d[2] {
            for j in 0..d[1] {
                for i in 0..d[0] {
                    let value = self.value(self.index(i, j, k, options.time_index) as usize);
                    if value == 0.0 {
                        continue;
                    }
                    if !finite(value) || value != value.trunc() || value < 0.0 || value > 65535.0 {
                        return Err(seg_error("label", SegCause::InvalidLabel));
                    }
                    let label = value as i64;
                    if let Some(definitions) = &options.segments
                        && !definitions.contains_key(&label)
                    {
                        return Err(seg_error("label", SegCause::InvalidLabel));
                    }
                    let target = mapping.to_grid([i, j, k]);
                    for axis in 0..3 {
                        if target[axis] < 0 || target[axis] >= bounds[axis] {
                            return Err(seg_error("label", SegCause::OutsideReference));
                        }
                    }
                    present.insert(label);
                }
            }
        }
        if present.is_empty() {
            return Err(seg_error("label", SegCause::Empty));
        }
        Ok(present.into_iter().collect())
    }
}
type Definitions = (BTreeMap<i64, Segment>, BTreeMap<i64, i64>);
fn segment_definitions(
    labels: &[i64],
    definitions: Option<&BTreeMap<i64, Segment>>,
) -> Result<Definitions, SegError> {
    let invalid = || seg_error("segment", SegCause::InvalidLabel);
    let lookup = |label: i64| definitions.and_then(|d| d.get(&label));
    let mut segments = BTreeMap::new();
    let mut numbers = BTreeMap::new();
    let mut used = std::collections::BTreeSet::new();
    for &label in labels {
        let number = lookup(label).map_or(0, |s| s.number);
        if number == 0 {
            continue;
        }
        if !(1..=65535).contains(&number) || used.contains(&number) {
            return Err(invalid());
        }
        used.insert(number);
    }
    for (index, &label) in labels.iter().enumerate() {
        let mut segment = match lookup(label) {
            Some(s) => s.clone(),
            None => Segment {
                label: format!("Label {label}"),
                ..Segment::default()
            },
        };
        if segment.number == 0 {
            segment.number = index as i64 + 1;
            while used.contains(&segment.number) {
                segment.number += 1;
            }
        }
        if !(1..=65535).contains(&segment.number) {
            return Err(invalid());
        }
        used.insert(segment.number);
        numbers.insert(label, segment.number);
        segments.insert(label, segment);
    }
    Ok((segments, numbers))
}
