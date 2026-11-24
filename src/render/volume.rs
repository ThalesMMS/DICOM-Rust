use super::affine::{GeometryAffine, finite_positive_spacing, geometry_affine_pair};
use super::error::{ErrorKind, RenderError};
use super::frame::{
    Frame, decode_grayscale_f32, effective_frame_metadata, grayscale_display_frame,
    normalized_photometric, pixel_value_at, sanitized_rescale, spatial_sort_value,
};
use super::geometry::{
    GeometryDisposition, GeometryIssue, GeometryTolerances, SliceGeometry, VolumeGeometry,
    build_volume_geometry, classification_for_geometry_issue, classify_volume,
    spacing_within_tolerance,
};
use super::grid::{Grid, clamp_continuous_index};
use super::parallel::{Cancellation, ctx_err, run_background};
use super::stablesort::slice_stable;
use super::vec3::Vec3;
use super::window::{PreparedVoi, WindowLevel};
use crate::display;
use crate::dynamic::{self, Timeline};
use crate::fmath;
use crate::object::geometry::{
    IMAGE_ORIENTATION_TOLERANCE, SliceSpacing, image_orientation_deviation, resolve_slice_spacing,
    slice_spacing_source,
};
use crate::pixel::Metadata;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
#[derive(Debug, Default)]
pub struct Stack {
    pub uid: String,
    pub study_uid: String,
    pub modality: String,
    pub body_part: String,
    pub default_window: WindowLevel,
    pub pixel_spacing: Vec<f64>,
    pub slice_thickness: f64,
    pub spacing_between_slices: f64,
    pub frames: Vec<Option<Arc<Frame>>>,
    volume: Mutex<StackVolume>,
}
#[derive(Debug, Default)]
struct StackVolume {
    volume: Option<Arc<Volume>>,
    closed: bool,
}
impl Stack {
    pub fn new(frames: Vec<Option<Arc<Frame>>>) -> Stack {
        Stack {
            frames,
            ..Default::default()
        }
    }
    pub(crate) fn frame(&self, index: usize) -> Option<&Frame> {
        self.frames.get(index).and_then(|f| f.as_deref())
    }
    pub fn first_display_frame(&self) -> Option<&Arc<Frame>> {
        self.frames
            .iter()
            .flatten()
            .find(|f| f.decode_err.is_none() && !f.encapsulated)
    }
    pub fn displayable(&self) -> bool {
        self.first_display_frame().is_some()
    }
    pub fn volume(&self) -> Result<Arc<Volume>, RenderError> {
        let mut cached = self.volume.lock().unwrap_or_else(|e| e.into_inner());
        if cached.closed {
            return Err(RenderError::new(
                ErrorKind::VolumeStoreClosed,
                "render: volume store closed",
            ));
        }
        if let Some(v) = cached.volume.as_ref() {
            return Ok(v.clone());
        }
        let vol = build_volume_with_tolerances(self, GeometryTolerances::default_tolerances())?;
        cached.volume = Some(vol.clone());
        Ok(vol)
    }
    pub fn close(&self) {
        let volume = {
            let mut cached = self.volume.lock().unwrap_or_else(|e| e.into_inner());
            cached.closed = true;
            cached.volume.take()
        };
        if let Some(volume) = volume {
            volume.close();
        }
    }
}
pub fn first_renderable_slice(series: &Stack) -> Option<&Arc<Frame>> {
    series
        .frames
        .iter()
        .flatten()
        .find(|f| f.decode_err.is_none() && f.metadata.rows > 0 && f.metadata.columns > 0)
}
pub(crate) fn frame_pixel_spacing(values: &[f64]) -> Option<(f64, f64)> {
    let (row, column) = match values.len() {
        0 => return None,
        1 => (values[0], values[0]),
        _ => (values[0], values[1]),
    };
    (finite_positive_spacing(row) && finite_positive_spacing(column)).then_some((row, column))
}
pub(crate) fn series_pixel_spacing(series: &Stack) -> (f64, f64) {
    if let Some(s) = frame_pixel_spacing(&series.pixel_spacing) {
        return s;
    }
    series
        .frames
        .iter()
        .flatten()
        .find_map(|f| frame_pixel_spacing(&f.pixel_spacing))
        .unwrap_or((0.0, 0.0))
}
pub(crate) fn series_fallback_slice_spacing(series: &Stack) -> SliceSpacing {
    let spacing = resolve_slice_spacing(series.spacing_between_slices, series.slice_thickness);
    if spacing.source != slice_spacing_source::DEFAULT {
        return spacing;
    }
    for frame in series.frames.iter().flatten() {
        let spacing = resolve_slice_spacing(frame.spacing_between_slices, frame.slice_thickness);
        if spacing.source != slice_spacing_source::DEFAULT {
            return spacing;
        }
    }
    resolve_slice_spacing(0.0, 0.0)
}
pub(crate) fn series_slice_spacing(series: &Stack) -> f64 {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut count = 0i64;
    for frame in series.frames.iter().flatten() {
        let v = spatial_sort_value(frame);
        if v < lo {
            lo = v;
        }
        if v > hi {
            hi = v;
        }
        count += 1;
    }
    if count > 1 && hi > lo {
        let sp = (hi - lo) / (count - 1) as f64;
        if sp > 0.0 && sp.is_finite() {
            return sp;
        }
    }
    series_fallback_slice_spacing(series).value
}
fn orientation_basis(iop: &[f64]) -> (Vec3, Vec3, Vec3) {
    let row = Vec3::new(iop[0], iop[1], iop[2]).normalize();
    let raw = Vec3::new(iop[3], iop[4], iop[5]);
    let col = raw.sub(row.scale(raw.dot(row))).normalize();
    let normal = row.cross(col).normalize();
    (row, col, normal)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SliceGeometryFailure {
    Valid,
    GridMismatch,
    Invalid,
    MixedPositionSource,
    NonOrthonormal,
}
pub(crate) struct Collected {
    frame: Arc<Frame>,
    geometry: SliceGeometry,
    pub(crate) frame_index: i64,
    orientation_deviation: f64,
}
pub(crate) fn slice_geometry(
    sl: &Frame,
    ref_normal: Vec3,
    row_sp: f64,
    col_sp: f64,
) -> Option<SliceGeometry> {
    if sl.image_orientation.len() < 6 {
        return None;
    }
    let (row, col, normal) = orientation_basis(&sl.image_orientation);
    if row.is_zero()
        || col.is_zero()
        || normal.is_zero()
        || !row.finite()
        || !col.finite()
        || !normal.finite()
    {
        return None;
    }
    let origin = if sl.image_position.len() >= 3 {
        Vec3::new(
            sl.image_position[0],
            sl.image_position[1],
            sl.image_position[2],
        )
    } else if sl.slice_location_ok {
        let base = if ref_normal.is_zero() {
            normal
        } else {
            ref_normal
        };
        base.scale(sl.slice_location)
    } else {
        return None;
    };
    if !origin.finite() {
        return None;
    }
    let (row_spacing, col_spacing) =
        frame_pixel_spacing(&sl.pixel_spacing).unwrap_or((row_sp, col_sp));
    Some(SliceGeometry {
        origin,
        row_dir: row,
        col_dir: col,
        normal,
        row_spacing,
        col_spacing,
        rows: i64::from(sl.metadata.rows),
        columns: i64::from(sl.metadata.columns),
    })
}
pub(crate) fn collect_slice_geometries(
    frames: &[Option<Arc<Frame>>],
    ref_normal: Vec3,
    row_sp: f64,
    col_sp: f64,
    expected_rows: i64,
    expected_cols: i64,
) -> (Vec<Collected>, SliceGeometryFailure, i64) {
    let mut collected = Vec::with_capacity(frames.len());
    let mut position_source = 0u8;
    for (i, sl) in frames.iter().enumerate() {
        let Some(sl) = sl else { continue };
        if sl.decode_err.is_some() || sl.metadata.rows == 0 || sl.metadata.columns == 0 {
            continue;
        }
        let i = i as i64;
        if expected_rows > 0
            && expected_cols > 0
            && (i64::from(sl.metadata.rows) != expected_rows
                || i64::from(sl.metadata.columns) != expected_cols)
        {
            return (Vec::new(), SliceGeometryFailure::GridMismatch, i);
        }
        let Some(deviation) = image_orientation_deviation(&sl.image_orientation) else {
            return (Vec::new(), SliceGeometryFailure::Invalid, i);
        };
        if deviation > IMAGE_ORIENTATION_TOLERANCE {
            return (Vec::new(), SliceGeometryFailure::NonOrthonormal, i);
        }
        let Some(geometry) = slice_geometry(sl, ref_normal, row_sp, col_sp) else {
            return (Vec::new(), SliceGeometryFailure::Invalid, i);
        };
        let next = if sl.image_position.len() >= 3 { 1 } else { 2 };
        if position_source != 0 && position_source != next {
            return (Vec::new(), SliceGeometryFailure::MixedPositionSource, i);
        }
        position_source = next;
        collected.push(Collected {
            frame: sl.clone(),
            geometry,
            frame_index: i + 1,
            orientation_deviation: deviation,
        });
    }
    (collected, SliceGeometryFailure::Valid, -1)
}
fn geometry_issue_for_failure(failure: SliceGeometryFailure) -> GeometryIssue {
    match failure {
        SliceGeometryFailure::GridMismatch => GeometryIssue::DifferentPixelGrid,
        SliceGeometryFailure::MixedPositionSource => GeometryIssue::MixedPositionSource,
        SliceGeometryFailure::NonOrthonormal => GeometryIssue::NonOrthonormalOrientation,
        _ => GeometryIssue::InvalidGeometry,
    }
}
pub(crate) fn geometry_timeline(collected: &[Collected]) -> Timeline {
    let normal = collected.first().map_or(Vec3::ZERO, |c| c.geometry.normal);
    let frames: Vec<dynamic::FrameMetadata> = collected
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let mut frame = item.frame.temporal.clone();
            frame.frame_index = index as i64;
            if !frame.has_spatial_position {
                frame.spatial_position = item.geometry.position_along(normal);
                frame.has_spatial_position = true;
            }
            frame
        })
        .collect();
    dynamic::build(&frames)
}
fn apply_temporal_geometry_guardrail(geometry: &mut VolumeGeometry, collected: &[Collected]) {
    if !geometry_timeline(collected).dynamic {
        return;
    }
    geometry.temporal_interleaved = true;
    geometry.add_issue(GeometryIssue::TemporalInterleaving);
    geometry.classification = super::geometry::VolumeClassification::TemporalInterleaved;
}
fn geometry_failure_frame(
    reason: GeometryIssue,
    collected: &[Collected],
    tol: GeometryTolerances,
) -> i64 {
    if collected.is_empty() {
        return 0;
    }
    let tol = tol.normalized();
    let reference = collected[0].geometry;
    match reason {
        GeometryIssue::DuplicatePositions => {
            let mut positions: Vec<f64> = Vec::new();
            for candidate in collected {
                let position = candidate.geometry.position_along(reference.normal);
                if positions
                    .iter()
                    .any(|seen| (position - seen).abs() <= tol.position_abs)
                {
                    return candidate.frame_index;
                }
                positions.push(position);
            }
        }
        GeometryIssue::InconsistentNormals => {
            for c in &collected[1..] {
                if c.geometry.normal.dot(reference.normal) < tol.orientation_cos {
                    return c.frame_index;
                }
            }
        }
        GeometryIssue::MixedOrientation => {
            for c in &collected[1..] {
                let g = c.geometry;
                if g.normal.dot(reference.normal) >= tol.orientation_cos
                    && (g.row_dir.dot(reference.row_dir) < tol.orientation_cos
                        || g.col_dir.dot(reference.col_dir) < tol.orientation_cos)
                {
                    return c.frame_index;
                }
            }
        }
        GeometryIssue::InconsistentPixelSpacing => {
            for c in &collected[1..] {
                let g = c.geometry;
                if !spacing_within_tolerance(g.row_spacing, reference.row_spacing, &tol)
                    || !spacing_within_tolerance(g.col_spacing, reference.col_spacing, &tol)
                {
                    return c.frame_index;
                }
            }
        }
        _ => {}
    }
    0
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GeometryAssessment {
    pub geometry: VolumeGeometry,
    pub disposition: GeometryDisposition,
    pub fallback_reason: GeometryIssue,
    pub failure_frame: i64,
}
fn unsupported_geometry_assessment(
    reason: GeometryIssue,
    failure_frame: i64,
) -> GeometryAssessment {
    let mut geometry = VolumeGeometry {
        disposition: GeometryDisposition::Unsupported,
        primary_issue: reason,
        issues: Some(vec![reason]),
        ..Default::default()
    };
    geometry.classification = classification_for_geometry_issue(reason).0;
    GeometryAssessment {
        geometry,
        disposition: GeometryDisposition::Unsupported,
        fallback_reason: reason,
        failure_frame,
    }
}
impl Stack {
    pub fn geometry_assessment(&self) -> GeometryAssessment {
        let Some(first) = first_renderable_slice(self) else {
            return unsupported_geometry_assessment(GeometryIssue::InvalidGeometry, 0);
        };
        if first.image_orientation.len() < 6 {
            return unsupported_geometry_assessment(GeometryIssue::InvalidGeometry, 0);
        }
        let (row, col, ref_normal) = orientation_basis(&first.image_orientation);
        if row.is_zero() || col.is_zero() || ref_normal.is_zero() {
            return unsupported_geometry_assessment(GeometryIssue::InvalidGeometry, 0);
        }
        let (mut row_sp, mut col_sp) = series_pixel_spacing(self);
        let missing_spacing = !finite_positive_spacing(row_sp) || !finite_positive_spacing(col_sp);
        if !finite_positive_spacing(row_sp) {
            row_sp = 1.0;
        }
        if !finite_positive_spacing(col_sp) {
            col_sp = 1.0;
        }
        let (collected, failure, failed_index) = collect_slice_geometries(
            &self.frames,
            ref_normal,
            row_sp,
            col_sp,
            i64::from(first.metadata.rows),
            i64::from(first.metadata.columns),
        );
        if failure != SliceGeometryFailure::Valid || collected.is_empty() {
            let reason = geometry_issue_for_failure(failure);
            let frame = if failed_index >= 0 {
                failed_index + 1
            } else {
                0
            };
            let mut assessment = unsupported_geometry_assessment(reason, frame);
            if failure == SliceGeometryFailure::NonOrthonormal
                && let Some(f) = self.frame(failed_index as usize)
            {
                assessment.geometry.metrics.maximum_orientation_deviation =
                    image_orientation_deviation(&f.image_orientation).unwrap_or(0.0);
            }
            return assessment;
        }
        let geometries: Vec<SliceGeometry> = collected.iter().map(|c| c.geometry).collect();
        let tol = GeometryTolerances::default_tolerances();
        let mut geometry = build_volume_geometry(&geometries, tol);
        apply_temporal_geometry_guardrail(&mut geometry, &collected);
        if missing_spacing {
            geometry.add_issue(GeometryIssue::MissingPixelSpacing);
        }
        for item in &collected {
            geometry.metrics.maximum_orientation_deviation = fmath::max(
                geometry.metrics.maximum_orientation_deviation,
                item.orientation_deviation,
            );
        }
        geometry.classification = classify_volume(&geometry);
        geometry.finalize_disposition();
        let failure_frame = geometry_failure_frame(geometry.primary_issue, &collected, tol);
        GeometryAssessment {
            disposition: geometry.disposition,
            fallback_reason: geometry.primary_issue,
            failure_frame,
            geometry,
        }
    }
    pub fn volume_geometry(&self) -> Option<VolumeGeometry> {
        let assessment = self.geometry_assessment();
        (!assessment.geometry.slices().is_empty()).then_some(assessment.geometry)
    }
    pub fn geometry_frame_groups(&self) -> Option<Vec<GeometryFrameGroup>> {
        let first = first_renderable_slice(self)?;
        if first.image_orientation.len() < 6 {
            return None;
        }
        let (_, _, normal) = orientation_basis(&first.image_orientation);
        let (mut row_sp, mut col_sp) = series_pixel_spacing(self);
        if !finite_positive_spacing(row_sp) {
            row_sp = 1.0;
        }
        if !finite_positive_spacing(col_sp) {
            col_sp = 1.0;
        }
        let (collected, failure, _) = collect_slice_geometries(
            &self.frames,
            normal,
            row_sp,
            col_sp,
            i64::from(first.metadata.rows),
            i64::from(first.metadata.columns),
        );
        if failure != SliceGeometryFailure::Valid || collected.is_empty() {
            return None;
        }
        let timeline = geometry_timeline(&collected);
        let mut groups = Vec::new();
        for point in timeline.points() {
            for stack in &point.stacks {
                groups.push(GeometryFrameGroup {
                    temporal_ordinal: point.ordinal,
                    temporal_position: point.temporal_position,
                    has_temporal: point.has_position || timeline.dynamic,
                    stack_id: stack.id.clone(),
                    frames: stack
                        .frame_indices
                        .iter()
                        .filter(|i| **i >= 0 && (**i as usize) < collected.len())
                        .map(|i| collected[*i as usize].frame.clone())
                        .collect(),
                });
            }
        }
        Some(groups)
    }
}
#[derive(Debug, Clone)]
pub struct GeometryFrameGroup {
    pub temporal_ordinal: i64,
    pub temporal_position: i64,
    pub has_temporal: bool,
    pub stack_id: String,
    pub frames: Vec<Arc<Frame>>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GantryTiltCorrection {
    pub applied: bool,
    pub angle_degrees: f64,
    pub total_offset: Vec3,
    pub shear_per_mm: Vec3,
}
#[derive(Debug)]
pub(crate) enum Payload {
    F32(super::plane_buffer::Samples<f32>),
    Rgb(super::plane_buffer::Samples<f32>),
    I16 {
        data: super::plane_buffer::Samples<i16>,
        slope: f64,
        intercept: f64,
    },
    U16 {
        data: super::plane_buffer::Samples<u16>,
        slope: f64,
        intercept: f64,
    },
}
#[derive(Debug)]
pub struct Volume {
    pub cols: i64,
    pub rows: i64,
    pub depth: i64,
    pub origin: Vec3,
    pub axis_x: Vec3,
    pub axis_y: Vec3,
    pub normal: Vec3,
    pub col_spacing: f64,
    pub row_spacing: f64,
    pub slice_spacing: f64,
    slice_source: &'static str,
    positions: Vec<f64>,
    slice_origins: Vec<Vec3>,
    regular: bool,
    tilt: GantryTiltCorrection,
    geometry: VolumeGeometry,
    slices: Vec<Arc<Frame>>,
    uniform_step: OnceLock<f64>,
    pub(crate) hu_range: OnceLock<(f64, f64)>,
    payload: OnceLock<Result<Arc<Payload>, RenderError>>,
    regularized: OnceLock<Result<Arc<Volume>, RenderError>>,
    pub(crate) link: super::volume_link::VolumeLink,
}
fn frame_volume_components(frame: &Frame) -> u32 {
    if effective_frame_metadata(frame).samples_per_pixel == 3 {
        3
    } else {
        1
    }
}
pub fn build_volume(series: &Stack) -> Result<Arc<Volume>, RenderError> {
    build_volume_with_tolerances(series, GeometryTolerances::default_tolerances())
}
pub fn build_volume_with_tolerances(
    series: &Stack,
    tolerances: GeometryTolerances,
) -> Result<Arc<Volume>, RenderError> {
    if series.frames.is_empty() {
        return Err(RenderError::other("render: no series selected"));
    }
    let Some(first) = first_renderable_slice(series) else {
        return Err(RenderError::other(
            "render: series has no displayable images",
        ));
    };
    let rows = i64::from(first.metadata.rows);
    let cols = i64::from(first.metadata.columns);
    if first.image_orientation.len() < 6 {
        return Err(RenderError::other(
            "render: series lacks ImageOrientationPatient",
        ));
    }
    let (axis_x, axis_y, normal) = orientation_basis(&first.image_orientation);
    if axis_x.length() == 0.0 || axis_y.length() == 0.0 || normal.length() == 0.0 {
        return Err(RenderError::other("render: degenerate image orientation"));
    }
    let (mut row_sp, mut col_sp) = series_pixel_spacing(series);
    if !finite_positive_spacing(col_sp) {
        col_sp = 1.0;
    }
    if !finite_positive_spacing(row_sp) {
        row_sp = 1.0;
    }
    let (collected, failure, failed_index) =
        collect_slice_geometries(&series.frames, normal, row_sp, col_sp, rows, cols);
    match failure {
        SliceGeometryFailure::Valid => {}
        SliceGeometryFailure::GridMismatch => {
            return Err(RenderError::guardrail(
                GeometryIssue::DifferentPixelGrid,
                failed_index + 1,
            ));
        }
        SliceGeometryFailure::Invalid => {
            return Err(RenderError::guardrail(
                GeometryIssue::InvalidGeometry,
                failed_index + 1,
            ));
        }
        SliceGeometryFailure::MixedPositionSource => {
            return Err(RenderError::guardrail(
                GeometryIssue::MixedPositionSource,
                0,
            ));
        }
        SliceGeometryFailure::NonOrthonormal => {
            return Err(RenderError::guardrail(
                GeometryIssue::NonOrthonormalOrientation,
                failed_index + 1,
            ));
        }
    }
    let first_components = frame_volume_components(first);
    let mut coll: Vec<(Arc<Frame>, SliceGeometry, f64)> = Vec::with_capacity(collected.len());
    for entry in &collected {
        if frame_volume_components(&entry.frame) != first_components {
            return Err(RenderError::new(
                ErrorKind::InvalidVolumeSnapshot,
                "render: invalid volume snapshot: mixed scalar/color slices",
            ));
        }
        coll.push((
            entry.frame.clone(),
            entry.geometry,
            entry.geometry.position_along(normal),
        ));
    }
    if coll.is_empty() {
        return Err(RenderError::other("render: series lacks slice positions"));
    }
    slice_stable(&mut coll, |a, b| a.2 < b.2);
    let slices: Vec<Arc<Frame>> = coll.iter().map(|c| c.0.clone()).collect();
    let positions: Vec<f64> = coll.iter().map(|c| c.2 - coll[0].2).collect();
    let slice_origins: Vec<Vec3> = coll.iter().map(|c| c.1.origin).collect();
    let input: Vec<SliceGeometry> = collected.iter().map(|c| c.geometry).collect();
    let mut geometry = build_volume_geometry(&input, tolerances);
    apply_temporal_geometry_guardrail(&mut geometry, &collected);
    geometry.classification = classify_volume(&geometry);
    geometry.finalize_disposition();
    if geometry.disposition == GeometryDisposition::Unsupported {
        return Err(RenderError::guardrail(
            geometry.primary_issue,
            geometry_failure_frame(geometry.primary_issue, &collected, tolerances),
        ));
    }
    let mut slice_sp = 0.0;
    let mut slice_source = slice_spacing_source::FROM_POSITIONS;
    if coll.len() > 1 {
        slice_sp = (coll[coll.len() - 1].2 - coll[0].2) / (coll.len() - 1) as f64;
    }
    if slice_sp <= 0.0 || !slice_sp.is_finite() {
        let fallback = series_fallback_slice_spacing(series);
        slice_sp = fallback.value;
        slice_source = fallback.source;
    }
    Ok(Arc::new(Volume {
        cols,
        rows,
        depth: coll.len() as i64,
        origin: coll[0].1.origin,
        axis_x,
        axis_y,
        normal,
        col_spacing: col_sp,
        row_spacing: row_sp,
        slice_spacing: slice_sp,
        slice_source,
        positions,
        slice_origins,
        regular: geometry.regular,
        tilt: GantryTiltCorrection {
            applied: geometry.gantry_tilted,
            angle_degrees: geometry.gantry_tilt_angle_degrees,
            total_offset: geometry.gantry_tilt_offset,
            shear_per_mm: geometry.gantry_tilt_shear,
        },
        geometry,
        slices,
        uniform_step: OnceLock::new(),
        hu_range: OnceLock::new(),
        payload: OnceLock::new(),
        regularized: OnceLock::new(),
        link: Default::default(),
    }))
}
impl Volume {
    pub fn slice_spacing_source(&self) -> &'static str {
        self.slice_source
    }
    pub fn gantry_tilt_correction(&self) -> GantryTiltCorrection {
        self.tilt
    }
    pub fn geometry(&self) -> &VolumeGeometry {
        &self.geometry
    }
    pub fn components(&self) -> u32 {
        self.slices
            .first()
            .map_or(1, |f| frame_volume_components(f))
    }
    pub fn photometric(&self) -> &str {
        self.slices
            .first()
            .map_or("", |f| f.metadata.photometric_interpretation.as_str())
    }
    pub(crate) fn effective_slice_spacing(&self) -> f64 {
        if finite_positive_spacing(self.slice_spacing) {
            self.slice_spacing
        } else {
            1.0
        }
    }
    pub fn voxel_to_patient(&self, p: Vec3) -> Vec3 {
        if self.tilt.applied {
            return self
                .slice_origin_at(p.z)
                .add(self.axis_x.scale(p.x * self.col_spacing))
                .add(self.axis_y.scale(p.y * self.row_spacing));
        }
        let normal_dist = if self.regular {
            p.z * self.effective_slice_spacing()
        } else {
            self.index_to_position(p.z)
        };
        self.origin
            .add(self.axis_x.scale(p.x * self.col_spacing))
            .add(self.axis_y.scale(p.y * self.row_spacing))
            .add(self.normal.scale(normal_dist))
    }
    pub fn patient_to_voxel(&self, p: Vec3) -> Vec3 {
        let mut d = p.sub(self.origin);
        let mut z = d.dot(self.normal) / self.effective_slice_spacing();
        if !self.positions.is_empty() {
            z = self.position_to_index(d.dot(self.normal));
        }
        if self.tilt.applied {
            d = p.sub(self.slice_origin_at(z));
        }
        Vec3::new(
            d.dot(self.axis_x) / self.col_spacing,
            d.dot(self.axis_y) / self.row_spacing,
            z,
        )
    }
    pub(crate) fn voxel_line(&self, base: Vec3, dir: Vec3) -> VoxelLine<'_> {
        let mut line = VoxelLine {
            vol: self,
            base,
            dir,
            affine: false,
            x0: 0.0,
            y0: 0.0,
            pos0: 0.0,
            dx: 0.0,
            dy: 0.0,
            dpos: 0.0,
        };
        if !self.tilt.applied {
            let d = base.sub(self.origin);
            line.affine = true;
            line.x0 = d.dot(self.axis_x) / self.col_spacing;
            line.y0 = d.dot(self.axis_y) / self.row_spacing;
            line.pos0 = d.dot(self.normal);
            line.dx = dir.dot(self.axis_x) / self.col_spacing;
            line.dy = dir.dot(self.axis_y) / self.row_spacing;
            line.dpos = dir.dot(self.normal);
        }
        line
    }
    fn position_to_slice(&self, pos: f64) -> f64 {
        if self.positions.is_empty() {
            return pos / self.effective_slice_spacing();
        }
        let step = self.uniform_slice_step();
        if step > 0.0 {
            return pos / step;
        }
        self.position_to_index(pos)
    }
    fn uniform_slice_step(&self) -> f64 {
        *self.uniform_step.get_or_init(|| {
            let n = self.positions.len();
            if n < 2 || self.positions[0] != 0.0 {
                return 0.0;
            }
            let step = self.positions[n - 1] / (n - 1) as f64;
            if !(step > 0.0) {
                return 0.0;
            }
            let tolerance = 1e-9 * fmath::max(1.0, self.positions[n - 1]);
            for (k, pos) in self.positions.iter().enumerate() {
                if (pos - k as f64 * step).abs() > tolerance {
                    return 0.0;
                }
            }
            step
        })
    }
    fn slice_origin_at(&self, z: f64) -> Vec3 {
        let o = &self.slice_origins;
        let n = o.len();
        if !z.is_finite() {
            return o.first().copied().unwrap_or(self.origin);
        }
        if n == 0 {
            return self
                .origin
                .add(self.normal.scale(self.index_to_position(z)));
        }
        if n == 1 {
            return o[0];
        }
        if z <= 0.0 {
            return o[0].add(o[1].sub(o[0]).scale(z));
        }
        let last = (n - 1) as f64;
        if z >= last {
            return o[n - 1].add(o[n - 1].sub(o[n - 2]).scale(z - last));
        }
        let i = z.floor() as usize;
        let frac = z - i as f64;
        o[i].add(o[i + 1].sub(o[i]).scale(frac))
    }
    pub(crate) fn index_to_position(&self, z: f64) -> f64 {
        let p = &self.positions;
        let n = p.len();
        if n == 0 {
            return z * self.effective_slice_spacing();
        }
        if n == 1 {
            return p[0];
        }
        if z <= 0.0 {
            return p[0] + z * (p[1] - p[0]);
        }
        let last = (n - 1) as f64;
        if z >= last {
            return p[n - 1] + (z - last) * (p[n - 1] - p[n - 2]);
        }
        let i = z.floor() as usize;
        let frac = z - i as f64;
        p[i] + frac * (p[i + 1] - p[i])
    }
    fn position_to_index(&self, pos: f64) -> f64 {
        let p = &self.positions;
        let n = p.len();
        if n == 0 {
            return pos / self.effective_slice_spacing();
        }
        if n == 1 {
            return 0.0;
        }
        if pos <= p[0] {
            let span = p[1] - p[0];
            if span == 0.0 {
                return 0.0;
            }
            return (pos - p[0]) / span;
        }
        if pos >= p[n - 1] {
            let span = p[n - 1] - p[n - 2];
            if span == 0.0 {
                return (n - 1) as f64;
            }
            return (n - 1) as f64 + (pos - p[n - 1]) / span;
        }
        let (mut lo, mut hi) = (0usize, n - 1);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if p[mid] <= pos {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let span = p[hi] - p[lo];
        if span == 0.0 {
            return lo as f64;
        }
        lo as f64 + (pos - p[lo]) / span
    }
    pub fn voxel_to_normalized(&self, p: Vec3) -> Vec3 {
        let safe = |a: f64, b: f64| if b <= 0.0 { 0.0 } else { a / b };
        Vec3::new(
            safe(p.x, (self.cols - 1) as f64),
            safe(p.y, (self.rows - 1) as f64),
            safe(p.z, (self.depth - 1) as f64),
        )
    }
    pub fn normalized_to_voxel(&self, n: Vec3) -> Vec3 {
        Vec3::new(
            n.x * (self.cols - 1).max(0) as f64,
            n.y * (self.rows - 1).max(0) as f64,
            n.z * (self.depth - 1).max(0) as f64,
        )
    }
    pub fn center_voxel(&self) -> Vec3 {
        Vec3::new(
            (self.cols - 1) as f64 / 2.0,
            (self.rows - 1) as f64 / 2.0,
            (self.depth - 1) as f64 / 2.0,
        )
    }
    pub fn center(&self) -> Vec3 {
        self.voxel_to_patient(self.center_voxel())
    }
    pub fn grid(&self) -> Option<Grid> {
        if !self.regular || self.tilt.applied {
            return None;
        }
        let g = Grid {
            size: [self.cols, self.rows, self.depth],
            origin: self.origin,
            spacing: [
                self.col_spacing,
                self.row_spacing,
                self.effective_slice_spacing(),
            ],
            direction: [self.axis_x, self.axis_y, self.normal],
        };
        g.validate().is_ok().then_some(g)
    }
    pub fn value_at(self: &Arc<Self>, col: i64, row: i64, slice: i64) -> Option<(f64, String)> {
        if self.components() != 1
            || slice < 0
            || slice >= self.slices.len() as i64
            || col < 0
            || row < 0
            || col >= self.cols
            || row >= self.rows
        {
            return None;
        }
        if !self.geometry.requires_resampling
            && let Ok(sampler) = Sampler::direct(self.clone())
            && let Some(v) = sampler.value_at(col, row, slice)
        {
            return Some((v, self.photometric().to_string()));
        }
        let sl = &self.slices[slice as usize];
        let pv = pixel_value_at(Some(sl), col, row)?;
        Some((pv.rescaled, sl.metadata.photometric_interpretation.clone()))
    }
    pub(crate) fn payload(&self) -> Result<Arc<Payload>, RenderError> {
        if let Some(built) = self.payload.get() {
            return built.clone();
        }
        self.link.wait_while_building();
        self.payload_now()
    }
    pub(crate) fn payload_now(&self) -> Result<Arc<Payload>, RenderError> {
        self.payload
            .get_or_init(|| self.build_payload().map(Arc::new))
            .clone()
    }
    fn build_payload(&self) -> Result<Payload, RenderError> {
        if self.components() != 1 {
            return self.decode_f32().map(|v| Payload::Rgb(v.into()));
        }
        if let Some(plan) = plan_packed16(&self.slices) {
            return self.decode_packed16(plan);
        }
        self.decode_f32().map(|v| Payload::F32(v.into()))
    }
    fn checked_count(&self) -> Result<usize, RenderError> {
        checked_voxel_count(self.cols, self.rows, self.depth).ok_or_else(|| {
            RenderError::new(
                ErrorKind::InvalidVolumeSnapshot,
                "render: invalid volume snapshot: volume component dimensions overflow",
            )
        })
    }
    fn plane_samples(&self) -> usize {
        let plane = (self.cols * self.rows) as usize;
        if self.components() == 3 {
            plane * 3
        } else {
            plane
        }
    }
    pub(crate) fn decode_f32(&self) -> Result<Vec<f32>, RenderError> {
        let count = self.checked_count()?;
        let per_slice = self.plane_samples();
        let mut values = vec![
            0f32;
            if self.components() == 3 {
                count * 3
            } else {
                count
            }
        ];
        let all: Vec<usize> = (0..self.slices.len()).collect();
        self.decode_planes(None, &mut values, per_slice, &all, &|index, plane| {
            self.decode_f32_plane(index, plane)
        })?;
        Ok(values)
    }
    #[allow(clippy::type_complexity)]
    pub(crate) fn decode_planes<T: Send>(
        &self,
        ctx: Option<&dyn Cancellation>,
        buffer: &mut [T],
        per_slice: usize,
        indices: &[usize],
        decode: &(dyn Fn(usize, &mut [T]) -> Result<(), RenderError> + Sync),
    ) -> Result<(), RenderError> {
        if indices.is_empty() || per_slice == 0 {
            return ctx_err(ctx).map_or(Ok(()), Err);
        }
        let planes: Vec<Mutex<&mut [T]>> = buffer.chunks_mut(per_slice).map(Mutex::new).collect();
        let errors: Vec<Mutex<Option<RenderError>>> =
            indices.iter().map(|_| Mutex::new(None)).collect();
        let first = AtomicUsize::new(indices.len());
        let voxels = checked_voxel_count(self.cols, self.rows, self.depth).unwrap_or(usize::MAX);
        let workers = if voxels < 64 << 10 { 1 } else { 0 };
        let item = |item: i64| {
            let item = item as usize;
            if item > first.load(Ordering::SeqCst) {
                return;
            }
            let index = indices[item];
            let result = match planes.get(index) {
                Some(plane) => {
                    let mut plane = plane.lock().unwrap_or_else(|e| e.into_inner());
                    decode(index, &mut plane)
                }
                None => Err(RenderError::other(format!(
                    "render: volume frame {} is outside the volume",
                    index + 1
                ))),
            };
            if let Err(e) = result {
                *errors[item].lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                first.fetch_min(item, Ordering::SeqCst);
            }
        };
        run_background(ctx, indices.len() as i64, workers, 1, || &item)?;
        let failed = first.load(Ordering::SeqCst);
        if failed < indices.len()
            && let Some(e) = errors[failed]
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
        {
            return Err(e);
        }
        Ok(())
    }
    #[allow(unsafe_code, clippy::too_many_arguments)]
    #[allow(clippy::type_complexity)]
    pub(crate) unsafe fn decode_batches_shared<T: Send + Sync>(
        &self,
        ctx: Option<&dyn Cancellation>,
        buffer: &super::plane_buffer::PlaneBuffer<T>,
        per_slice: usize,
        order: &[usize],
        decode: &(dyn Fn(usize, &mut [T]) -> Result<(), RenderError> + Sync),
        batch_end: &mut dyn FnMut(usize) -> usize,
        after: &mut dyn FnMut(usize, usize) -> Result<(), RenderError>,
    ) -> Result<(), RenderError> {
        if order.is_empty() || per_slice == 0 {
            return ctx_err(ctx).map_or(Ok(()), Err);
        }
        let planes_in_buffer = buffer.len() / per_slice;
        let mut sorted = order.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() != order.len() || sorted.last().is_some_and(|&i| i >= planes_in_buffer) {
            return Err(RenderError::other(
                "render: a decode order names a slice twice or outside the volume",
            ));
        }
        let errors: Vec<Mutex<Option<RenderError>>> =
            order.iter().map(|_| Mutex::new(None)).collect();
        let first = AtomicUsize::new(order.len());
        let voxels = checked_voxel_count(self.cols, self.rows, self.depth).unwrap_or(usize::MAX);
        let workers = if voxels < 64 << 10 { 1 } else { 0 };
        let item = |position: usize| {
            if position > first.load(Ordering::SeqCst) {
                return;
            }
            let index = order[position];
            let plane = unsafe { buffer.range_mut(index * per_slice, per_slice) };
            if let Err(e) = decode(index, plane) {
                *errors[position].lock().unwrap_or_else(|e| e.into_inner()) = Some(e);
                first.fetch_min(position, Ordering::SeqCst);
            }
        };
        let mut checked = |start: usize, end: usize| {
            let failed = first.load(Ordering::SeqCst);
            if failed < end
                && let Some(e) = errors[failed]
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
            {
                return Err(e);
            }
            after(start, end)
        };
        super::decode_crew::run_batches_background(
            ctx,
            order.len(),
            workers,
            &item,
            batch_end,
            &mut checked,
        )
    }
    pub(crate) fn decode_f32_plane(
        &self,
        index: usize,
        values: &mut [f32],
    ) -> Result<(), RenderError> {
        let plane = (self.cols * self.rows) as usize;
        let colour = self.components() == 3;
        let slice = &self.slices[index];
        let slice = super::frame::load_frame(Some(slice)).map_err(|e| {
            RenderError::new(
                e.kind(),
                format!("render: load volume frame {}: {e}", index + 1),
            )
        })?;
        if colour {
            let rgba = super::frame::decode_color_rgba(&slice).map_err(|e| {
                RenderError::new(
                    e.kind(),
                    format!("render: decode color volume frame {}: {e}", index + 1),
                )
            })?;
            if rgba.len() != plane * 4 {
                return Err(RenderError::new(
                    ErrorKind::InvalidVolumeSnapshot,
                    "render: invalid volume snapshot: incomplete color frame",
                ));
            }
            for (pixel, out) in values
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .take(plane)
                .enumerate()
            {
                for (component, value) in out.iter_mut().enumerate() {
                    *value = f32::from(rgba[pixel * 4 + component]);
                }
            }
            return Ok(());
        }
        let decoded = decode_grayscale_f32(&slice).map_err(|e| {
            RenderError::new(
                e.kind(),
                format!("render: decode volume frame {}: {e}", index + 1),
            )
        })?;
        let n = decoded.len().min(plane).min(values.len());
        values[..n].copy_from_slice(&decoded[..n]);
        Ok(())
    }
    pub(crate) fn decode_stored_plane<T: display::StoredTarget>(
        &self,
        index: usize,
        stored: &mut [T],
    ) -> Result<(), RenderError> {
        let wrap = |e: String| {
            RenderError::other(format!("render: decode volume frame {}: {e}", index + 1))
        };
        let slice = super::frame::load_frame(Some(&self.slices[index])).map_err(|e| {
            RenderError::new(
                e.kind(),
                format!("render: load volume frame {}: {e}", index + 1),
            )
        })?;
        let metadata = effective_frame_metadata(&slice);
        let photometric = normalized_photometric(&metadata.photometric_interpretation);
        let frame = grayscale_display_frame(&slice, &metadata, &photometric, self.rows, self.cols)
            .map_err(|e| wrap(e.to_string()))?;
        display::decode_stored_into(stored, &frame).map_err(|e| wrap(e.to_string()))
    }
    pub(crate) fn packed16_layout(&self) -> Option<(f64, f64, bool)> {
        if self.components() != 1 {
            return None;
        }
        plan_packed16(&self.slices).map(|p| (p.slope, p.intercept, p.signed))
    }
    pub(crate) fn validate_packed16_layout(layout: (f64, f64, bool)) -> Result<(), RenderError> {
        Self::validate_packed_plan(Packed16Plan {
            slope: layout.0,
            intercept: layout.1,
            signed: layout.2,
        })
    }
    pub(crate) fn slice_origins(&self) -> &[Vec3] {
        &self.slice_origins
    }
    pub(crate) fn cached_payload(&self) -> Option<Result<Arc<Payload>, RenderError>> {
        self.payload.get().cloned()
    }
    pub(crate) fn set_payload(&self, payload: Arc<Payload>) -> bool {
        self.payload.set(Ok(payload)).is_ok()
    }
    pub fn adopt_stored16_le(
        &self,
        bytes: &[u8],
        signed: bool,
        slope: f64,
        intercept: f64,
    ) -> bool {
        let Some(plan) = plan_packed16(&self.slices) else {
            return false;
        };
        let plane = (self.cols * self.rows) as usize;
        let count = match self.checked_count() {
            Ok(count) => count,
            Err(_) => return false,
        };
        if self.components() != 1
            || plan.signed != signed
            || plan.slope != slope
            || plan.intercept != intercept
            || Self::validate_packed_plan(plan).is_err()
            || plane == 0
            || bytes.len() != count * 2
            || self.payload.get().is_some()
        {
            return false;
        }
        let all: Vec<usize> = (0..count / plane).collect();
        let samples = |index: usize| &bytes[index * plane * 2..(index + 1) * plane * 2];
        let payload = if signed {
            let mut data = vec![0i16; count];
            let copied = self.decode_planes(None, &mut data, plane, &all, &|index, dst| {
                for (d, b) in dst.iter_mut().zip(samples(index).as_chunks::<2>().0) {
                    *d = i16::from_le_bytes([b[0], b[1]]);
                }
                Ok(())
            });
            if copied.is_err() {
                return false;
            }
            Payload::I16 {
                data: data.into(),
                slope,
                intercept,
            }
        } else {
            let mut data = vec![0u16; count];
            let copied = self.decode_planes(None, &mut data, plane, &all, &|index, dst| {
                for (d, b) in dst.iter_mut().zip(samples(index).as_chunks::<2>().0) {
                    *d = u16::from_le_bytes([b[0], b[1]]);
                }
                Ok(())
            });
            if copied.is_err() {
                return false;
            }
            Payload::U16 {
                data: data.into(),
                slope,
                intercept,
            }
        };
        self.set_payload(Arc::new(payload))
    }
    pub fn adopt_modality_f32_le(&self, bytes: &[u8]) -> bool {
        let plane = (self.cols * self.rows) as usize;
        let count = match self.checked_count() {
            Ok(count) => count,
            Err(_) => return false,
        };
        if self.components() != 1
            || self.geometry.requires_resampling
            || plan_packed16(&self.slices).is_some()
            || plane == 0
            || bytes.len() != count * 4
            || self.payload.get().is_some()
        {
            return false;
        }
        let all: Vec<usize> = (0..count / plane).collect();
        let mut values = vec![0f32; count];
        let copied = self.decode_planes(None, &mut values, plane, &all, &|index, dst| {
            let from = &bytes[index * plane * 4..(index + 1) * plane * 4];
            for (d, b) in dst.iter_mut().zip(from.as_chunks::<4>().0) {
                *d = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
            Ok(())
        });
        copied.is_ok() && self.set_payload(Arc::new(Payload::F32(values.into())))
    }
    fn decode_packed16(&self, plan: Packed16Plan) -> Result<Payload, RenderError> {
        let count = self.checked_count()?;
        let plane = (self.cols * self.rows) as usize;
        let all: Vec<usize> = (0..self.slices.len()).collect();
        if plan.signed {
            let mut data = vec![0i16; count];
            self.decode_planes(None, &mut data, plane, &all, &|index, stored| {
                self.decode_stored_plane(index, stored)
            })?;
            Self::validate_packed_plan(plan)?;
            Ok(Payload::I16 {
                data: data.into(),
                slope: plan.slope,
                intercept: plan.intercept,
            })
        } else {
            let mut data = vec![0u16; count];
            self.decode_planes(None, &mut data, plane, &all, &|index, stored| {
                self.decode_stored_plane(index, stored)
            })?;
            Self::validate_packed_plan(plan)?;
            Ok(Payload::U16 {
                data: data.into(),
                slope: plan.slope,
                intercept: plan.intercept,
            })
        }
    }
    fn validate_packed_plan(plan: Packed16Plan) -> Result<(), RenderError> {
        let (minimum, maximum) = if plan.signed {
            (-32768.0, 32767.0)
        } else {
            (0.0, 65535.0)
        };
        for stored_value in [minimum, maximum] {
            let value: f64 = stored_value * plan.slope + plan.intercept;
            let value32 = value as f32;
            if value.is_nan() || value.is_infinite() || value32.is_nan() || value32.is_infinite() {
                return Err(RenderError::new(
                    ErrorKind::InvalidVolumeSnapshot,
                    "render: invalid volume snapshot: stored-value rescale exceeds finite float32 modality range",
                ));
            }
        }
        Ok(())
    }
    pub fn regular_grid(self: &Arc<Self>) -> Result<Arc<Volume>, RenderError> {
        if !self.geometry.requires_resampling {
            return Ok(self.clone());
        }
        self.regularized
            .get_or_init(|| regularize_patient_grid(self).map(Arc::new))
            .clone()
    }
}
#[derive(Debug, Clone, Copy)]
struct Packed16Plan {
    slope: f64,
    intercept: f64,
    signed: bool,
}
fn plan_packed16(slices: &[Arc<Frame>]) -> Option<Packed16Plan> {
    let mut plan: Option<Packed16Plan> = None;
    for slice in slices {
        let md = effective_frame_metadata(slice);
        let photometric = normalized_photometric(&md.photometric_interpretation);
        if md.bits_allocated != 16
            || md.samples_per_pixel > 1
            || (photometric != "MONOCHROME1" && photometric != "MONOCHROME2")
            || md.bits_stored == 0
            || md.bits_stored > 16
        {
            return None;
        }
        let (slope, intercept) = sanitized_rescale(slice.rescale);
        let signed = md.pixel_representation != 0;
        match plan {
            None => {
                plan = Some(Packed16Plan {
                    slope,
                    intercept,
                    signed,
                })
            }
            Some(p) => {
                if slope != p.slope || intercept != p.intercept || signed != p.signed {
                    return None;
                }
            }
        }
    }
    plan
}
pub(crate) fn checked_voxel_count(cols: i64, rows: i64, depth: i64) -> Option<usize> {
    if cols <= 0 || rows <= 0 || depth <= 0 || cols > i64::MAX / rows {
        return None;
    }
    let plane = cols * rows;
    if plane > i64::MAX / depth {
        return None;
    }
    Some((plane * depth) as usize)
}
pub(crate) struct VoxelLine<'a> {
    vol: &'a Volume,
    base: Vec3,
    dir: Vec3,
    affine: bool,
    x0: f64,
    y0: f64,
    pos0: f64,
    dx: f64,
    dy: f64,
    dpos: f64,
}
impl VoxelLine<'_> {
    pub(crate) fn at(&self, s: f64) -> Vec3 {
        if !self.affine {
            return self.vol.patient_to_voxel(self.base.add(self.dir.scale(s)));
        }
        Vec3::new(
            self.x0 + s * self.dx,
            self.y0 + s * self.dy,
            self.vol.position_to_slice(self.pos0 + s * self.dpos),
        )
    }
}
const VOXEL_BORDER_TOLERANCE: f64 = 1e-3;
const PACKED16_AFFINE_ERROR_BUDGET: f64 = 1e-8;
enum SamplerData {
    F32(Arc<Payload>),
    Decoded(Vec<f32>),
    Shared(Arc<Vec<f32>>),
}
#[allow(dead_code)]
pub(crate) struct Sampler {
    pub(crate) vol: Arc<Volume>,
    data: SamplerData,
    slope: f64,
    intercept: f64,
    packed16_affine: bool,
    pub(crate) cols: i64,
    pub(crate) rows: i64,
    pub(crate) depth: i64,
    pub(crate) photometric: String,
    invert: bool,
}
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)]
pub(crate) struct TextureSample {
    pub(crate) x0: i64,
    pub(crate) y0: i64,
    pub(crate) z0: i64,
    pub(crate) fx: f64,
    pub(crate) fy: f64,
    pub(crate) fz: f64,
    pub(crate) c: [f64; 8],
}
impl TextureSample {
    pub(crate) fn value(&self) -> f64 {
        let (fx, fy, fz) = (self.fx, self.fy, self.fz);
        let c = &self.c;
        let c00 = c[0] * (1.0 - fx) + c[1] * fx;
        let c10 = c[2] * (1.0 - fx) + c[3] * fx;
        let c01 = c[4] * (1.0 - fx) + c[5] * fx;
        let c11 = c[6] * (1.0 - fx) + c[7] * fx;
        let c0 = c00 * (1.0 - fy) + c10 * fy;
        let c1 = c01 * (1.0 - fy) + c11 * fy;
        c0 * (1.0 - fz) + c1 * fz
    }
}
impl Sampler {
    fn with_data(vol: Arc<Volume>, data: SamplerData) -> Sampler {
        let photometric = normalized_photometric(vol.photometric());
        let mut s = Sampler {
            cols: vol.cols,
            rows: vol.rows,
            depth: vol.depth,
            invert: photometric == "MONOCHROME1",
            photometric,
            vol,
            data,
            slope: 0.0,
            intercept: 0.0,
            packed16_affine: false,
        };
        if let SamplerData::F32(p) = &s.data {
            match p.as_ref() {
                Payload::I16 {
                    slope, intercept, ..
                }
                | Payload::U16 {
                    slope, intercept, ..
                } => {
                    let max_stored = if matches!(p.as_ref(), Payload::I16 { .. }) {
                        32768.0
                    } else {
                        65535.0
                    };
                    s.slope = *slope;
                    s.intercept = *intercept;
                    let scale = max_stored * slope.abs() + intercept.abs();
                    s.packed16_affine =
                        scale <= PACKED16_AFFINE_ERROR_BUDGET / (64.0 * f64::EPSILON);
                }
                Payload::F32(_) | Payload::Rgb(_) => {}
            }
        }
        s
    }
    pub(crate) fn new(vol: &Arc<Volume>) -> Result<Sampler, RenderError> {
        if vol.cols <= 0 || vol.rows <= 0 || vol.depth <= 0 {
            return Err(RenderError::other("render: invalid volume dimensions"));
        }
        let grid = vol.regular_grid()?;
        Sampler::direct(grid)
    }
    pub(crate) fn direct(vol: Arc<Volume>) -> Result<Sampler, RenderError> {
        let payload = vol.payload()?;
        Ok(Sampler::with_data(vol, SamplerData::F32(payload)))
    }
    pub(crate) fn decoded(vol: &Arc<Volume>) -> Option<Sampler> {
        if vol.cols <= 0 || vol.rows <= 0 || vol.depth <= 0 {
            return None;
        }
        let values = vol.decode_f32().ok()?;
        Some(Sampler::with_data(
            vol.clone(),
            SamplerData::Decoded(values),
        ))
    }
    fn packed_at(&self, offset: usize) -> Option<f64> {
        match &self.data {
            SamplerData::F32(p) => match p.as_ref() {
                Payload::I16 { data, .. } => {
                    Some(f64::from(*data.get(offset)?) * self.slope + self.intercept)
                }
                Payload::U16 { data, .. } => {
                    Some(f64::from(*data.get(offset)?) * self.slope + self.intercept)
                }
                Payload::F32(_) | Payload::Rgb(_) => None,
            },
            SamplerData::Decoded(_) | SamplerData::Shared(_) => None,
        }
    }
    fn rgb_at(&self, col: i64, row: i64, slice: i64) -> Option<f64> {
        let SamplerData::F32(p) = &self.data else {
            return None;
        };
        let Payload::Rgb(v) = p.as_ref() else {
            return None;
        };
        let index = (slice * self.rows * self.cols * 3 + row * self.cols * 3 + col) as usize;
        v.get(index).map(|x| f64::from(*x))
    }
    fn is_rgb(&self) -> bool {
        matches!(&self.data, SamplerData::F32(p) if matches!(p.as_ref(), Payload::Rgb(_)))
    }
    pub(crate) fn f32_values(&self) -> Option<&[f32]> {
        match &self.data {
            SamplerData::Decoded(v) => Some(v),
            SamplerData::Shared(v) => Some(v),
            SamplerData::F32(p) => match p.as_ref() {
                Payload::F32(v) => Some(v),
                _ => None,
            },
        }
    }
    pub(crate) fn value_at(&self, col: i64, row: i64, slice: i64) -> Option<f64> {
        if slice < 0
            || slice >= self.depth
            || col < 0
            || row < 0
            || col >= self.cols
            || row >= self.rows
        {
            return None;
        }
        let offset = ((slice * self.rows + row) * self.cols + col) as usize;
        if let Some(values) = self.f32_values() {
            return values.get(offset).map(|v| f64::from(*v));
        }
        if self.is_rgb() {
            return self.rgb_at(col, row, slice);
        }
        self.packed_at(offset)
    }
    pub(crate) fn value_or_zero(&self, col: i64, row: i64, slice: i64) -> f64 {
        self.value_at(col, row, slice).unwrap_or(0.0)
    }
    pub(crate) fn trilinear_at(&self, p: Vec3) -> Option<f64> {
        if p.x >= 0.0
            && p.y >= 0.0
            && p.z >= 0.0
            && p.x == p.x.trunc()
            && p.y == p.y.trunc()
            && p.z == p.z.trunc()
            && p.x < self.cols as f64
            && p.y < self.rows as f64
            && p.z < self.depth as f64
        {
            return self.value_at(p.x as i64, p.y as i64, p.z as i64);
        }
        if self.packed16_affine {
            return self.packed16_trilinear_at(p);
        }
        self.voxel_sample(p).map(|s| s.value())
    }
    fn clamp3(&self, p: Vec3) -> Option<Vec3> {
        Some(Vec3::new(
            clamp_continuous_index(p.x, self.cols, VOXEL_BORDER_TOLERANCE)?,
            clamp_continuous_index(p.y, self.rows, VOXEL_BORDER_TOLERANCE)?,
            clamp_continuous_index(p.z, self.depth, VOXEL_BORDER_TOLERANCE)?,
        ))
    }
    fn corners(&self, p: Vec3) -> (i64, i64, i64, [usize; 8], f64, f64, f64) {
        let (x0, y0, z0) = (p.x.floor() as i64, p.y.floor() as i64, p.z.floor() as i64);
        let (x1, y1, z1) = (
            (x0 + 1).min(self.cols - 1),
            (y0 + 1).min(self.rows - 1),
            (z0 + 1).min(self.depth - 1),
        );
        let plane = self.rows * self.cols;
        let r00 = z0 * plane + y0 * self.cols;
        let r10 = z0 * plane + y1 * self.cols;
        let r01 = z1 * plane + y0 * self.cols;
        let r11 = z1 * plane + y1 * self.cols;
        let idx = [
            r00 + x0,
            r00 + x1,
            r10 + x0,
            r10 + x1,
            r01 + x0,
            r01 + x1,
            r11 + x0,
            r11 + x1,
        ]
        .map(|v| v as usize);
        (
            x0,
            y0,
            z0,
            idx,
            p.x - x0 as f64,
            p.y - y0 as f64,
            p.z - z0 as f64,
        )
    }
    fn packed16_trilinear_at(&self, p: Vec3) -> Option<f64> {
        let p = self.clamp3(p)?;
        let (_, _, _, idx, fx, fy, fz) = self.corners(p);
        let SamplerData::F32(payload) = &self.data else {
            return None;
        };
        let c = match payload.as_ref() {
            Payload::I16 { data, .. } => idx.map(|i| f64::from(data[i])),
            Payload::U16 { data, .. } => idx.map(|i| f64::from(data[i])),
            Payload::F32(_) | Payload::Rgb(_) => return None,
        };
        let sample = TextureSample {
            fx,
            fy,
            fz,
            c,
            ..Default::default()
        };
        Some(sample.value() * self.slope + self.intercept)
    }
    pub(crate) fn voxel_sample(&self, p: Vec3) -> Option<TextureSample> {
        let p = self.clamp3(p)?;
        let (x0, y0, z0, idx, fx, fy, fz) = self.corners(p);
        let c = if let Some(v) = self.f32_values() {
            idx.map(|i| f64::from(v[i]))
        } else if self.is_rgb() {
            let (x1, y1, z1) = (
                (x0 + 1).min(self.cols - 1),
                (y0 + 1).min(self.rows - 1),
                (z0 + 1).min(self.depth - 1),
            );
            [
                self.value_or_zero(x0, y0, z0),
                self.value_or_zero(x1, y0, z0),
                self.value_or_zero(x0, y1, z0),
                self.value_or_zero(x1, y1, z0),
                self.value_or_zero(x0, y0, z1),
                self.value_or_zero(x1, y0, z1),
                self.value_or_zero(x0, y1, z1),
                self.value_or_zero(x1, y1, z1),
            ]
        } else {
            idx.map(|i| self.packed_at(i).unwrap_or(0.0))
        };
        Some(TextureSample {
            x0,
            y0,
            z0,
            fx,
            fy,
            fz,
            c,
        })
    }
    pub(crate) fn display_gray(&self, value: f64, mapper: &PreparedVoi) -> u8 {
        let gray = mapper.gray(value);
        if self.invert { 255 - gray } else { gray }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RegularGridGeometry {
    pub dimensions: [i64; 3],
    pub origin: Vec3,
    pub axis_x: Vec3,
    pub axis_y: Vec3,
    pub normal: Vec3,
    pub col_spacing: f64,
    pub row_spacing: f64,
    pub slice_spacing: f64,
    pub voxel_to_patient_affine: GeometryAffine,
    pub patient_to_voxel_affine: GeometryAffine,
}
pub fn plan_regular_grid_geometry(
    geometry: &VolumeGeometry,
) -> Result<RegularGridGeometry, RenderError> {
    let Some(first) = geometry.slices().first().copied() else {
        return Err(RenderError::other("render: cannot plan empty regular grid"));
    };
    if first.columns <= 0
        || first.rows <= 0
        || !finite_positive_spacing(first.col_spacing)
        || !finite_positive_spacing(first.row_spacing)
    {
        return Err(RenderError::other(
            "render: invalid source geometry for regular grid",
        ));
    }
    let (mut min_x, mut min_y, mut min_z) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y, mut max_z) =
        (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let base = first.origin;
    for slice in geometry.slices() {
        for x in [0.0, (first.columns - 1) as f64 * first.col_spacing] {
            for y in [0.0, (first.rows - 1) as f64 * first.row_spacing] {
                let point = slice
                    .origin
                    .add(geometry.row_dir.scale(x))
                    .add(geometry.col_dir.scale(y));
                let delta = point.sub(base);
                let (lx, ly, lz) = (
                    delta.dot(geometry.row_dir),
                    delta.dot(geometry.col_dir),
                    delta.dot(geometry.normal),
                );
                min_x = fmath::min(min_x, lx);
                max_x = fmath::max(max_x, lx);
                min_y = fmath::min(min_y, ly);
                max_y = fmath::max(max_y, ly);
                min_z = fmath::min(min_z, lz);
                max_z = fmath::max(max_z, lz);
            }
        }
    }
    if ![min_x, max_x, min_y, max_y, min_z, max_z]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(RenderError::other(
            "render: non-finite patient bounds during regularization",
        ));
    }
    let mut slice_spacing =
        super::geometry::regularized_slice_spacing(geometry.spacings(), geometry.missing_slices);
    if geometry.regularized_depth > 1 && max_z > min_z {
        slice_spacing = (max_z - min_z) / (geometry.regularized_depth - 1) as f64;
    }
    if !finite_positive_spacing(slice_spacing) {
        return Err(RenderError::other(
            "render: invalid slice spacing for regular grid",
        ));
    }
    let extent = |span: f64, step: f64| {
        fmath::f64_to_i64((span / step - 1e-12).ceil())
            .wrapping_add(1)
            .max(1)
    };
    let cols = extent(max_x - min_x, first.col_spacing);
    let rows = extent(max_y - min_y, first.row_spacing);
    let depth = extent(max_z - min_z, slice_spacing);
    if cols > 65535 || rows > 65535 {
        return Err(RenderError::other(format!(
            "render: regularized grid {cols}x{rows} exceeds DICOM frame limits"
        )));
    }
    if checked_voxel_count(cols, rows, depth).is_none() {
        return Err(RenderError::other(
            "render: regularized grid dimensions overflow",
        ));
    }
    let origin = base
        .add(geometry.row_dir.scale(min_x))
        .add(geometry.col_dir.scale(min_y))
        .add(geometry.normal.scale(min_z));
    let (forward, inverse, ok) = geometry_affine_pair(
        origin,
        geometry.row_dir.scale(first.col_spacing),
        geometry.col_dir.scale(first.row_spacing),
        geometry.normal.scale(slice_spacing),
    );
    if !ok {
        return Err(RenderError::other(
            "render: regularized grid affine is not invertible",
        ));
    }
    Ok(RegularGridGeometry {
        dimensions: [cols, rows, depth],
        origin,
        axis_x: geometry.row_dir,
        axis_y: geometry.col_dir,
        normal: geometry.normal,
        col_spacing: first.col_spacing,
        row_spacing: first.row_spacing,
        slice_spacing,
        voxel_to_patient_affine: forward,
        patient_to_voxel_affine: inverse,
    })
}
fn fill_regular_grid(
    source: &Volume,
    grid: &RegularGridGeometry,
    values: &mut [f32],
    sample: impl Fn(Vec3) -> Option<f64>,
) {
    let [cols, rows, depth] = grid.dimensions;
    let origin = grid.origin;
    let source_origin_voxel = source.patient_to_voxel(origin);
    let column_delta = source
        .patient_to_voxel(origin.add(source.axis_x.scale(source.col_spacing)))
        .sub(source_origin_voxel);
    let row_delta = source
        .patient_to_voxel(origin.add(source.axis_y.scale(source.row_spacing)))
        .sub(source_origin_voxel);
    for z in 0..depth {
        let slice_origin =
            source.patient_to_voxel(origin.add(source.normal.scale(z as f64 * grid.slice_spacing)));
        let plane_offset = (z * rows * cols) as usize;
        for y in 0..rows {
            let mut voxel = slice_origin.add(row_delta.scale(y as f64));
            for x in 0..cols {
                if let Some(value) = sample(voxel) {
                    values[plane_offset + (y * cols + x) as usize] = value as f32;
                }
                voxel = voxel.add(column_delta);
            }
        }
    }
}
fn regularize_patient_grid(source: &Arc<Volume>) -> Result<Volume, RenderError> {
    if source.cols <= 0 || source.rows <= 0 || source.depth <= 0 {
        return Err(RenderError::other("render: cannot regularize empty volume"));
    }
    let Some(sampler) = Sampler::decoded(source) else {
        return Err(RenderError::other(
            "render: cannot prepare source sampler for regularization",
        ));
    };
    let grid = plan_regular_grid_geometry(&source.geometry)?;
    let [cols, rows, depth] = grid.dimensions;
    let count = checked_voxel_count(cols, rows, depth)
        .ok_or_else(|| RenderError::other("render: regularized grid dimensions overflow"))?;
    let mut photometric = normalized_photometric(source.photometric());
    if photometric.is_empty() {
        photometric = "MONOCHROME2".into();
    }
    let mut default_window = WindowLevel::new(40.0, 400.0);
    if let Some(first) = source.slices.first()
        && first.default_window.width > 0.0
    {
        default_window = first.default_window.clone();
    }
    let mut values = vec![0f32; count];
    fill_regular_grid(source, &grid, &mut values, |p| sampler.trilinear_at(p));
    let mut frames = Vec::with_capacity(depth as usize);
    let mut positions = Vec::with_capacity(depth as usize);
    let mut origins = Vec::with_capacity(depth as usize);
    let mut geometries = Vec::with_capacity(depth as usize);
    for z in 0..depth {
        let slice_origin = grid
            .origin
            .add(source.normal.scale(z as f64 * grid.slice_spacing));
        let metadata = Metadata {
            rows: rows as u16,
            columns: cols as u16,
            samples_per_pixel: 1,
            bits_allocated: 32,
            bits_stored: 32,
            high_bit: 31,
            photometric_interpretation: photometric.clone(),
            ..Default::default()
        };
        frames.push(Arc::new(Frame {
            metadata,
            default_window: default_window.clone(),
            ..Default::default()
        }));
        positions.push(z as f64 * grid.slice_spacing);
        origins.push(slice_origin);
        geometries.push(SliceGeometry {
            origin: slice_origin,
            row_dir: source.axis_x,
            col_dir: source.axis_y,
            normal: source.normal,
            row_spacing: source.row_spacing,
            col_spacing: source.col_spacing,
            rows,
            columns: cols,
        });
    }
    let geometry = build_volume_geometry(&geometries, GeometryTolerances::default_tolerances());
    let target = Volume {
        cols,
        rows,
        depth,
        origin: grid.origin,
        axis_x: source.axis_x,
        axis_y: source.axis_y,
        normal: source.normal,
        col_spacing: source.col_spacing,
        row_spacing: source.row_spacing,
        slice_spacing: grid.slice_spacing,
        slice_source: slice_spacing_source::UNKNOWN,
        positions,
        slice_origins: origins,
        regular: true,
        tilt: GantryTiltCorrection::default(),
        geometry,
        slices: frames,
        uniform_step: OnceLock::new(),
        hu_range: OnceLock::new(),
        payload: OnceLock::new(),
        regularized: OnceLock::new(),
        link: Default::default(),
    };
    let _ = target
        .payload
        .set(Ok(Arc::new(Payload::F32(values.into()))));
    Ok(target)
}
impl Volume {
    pub(crate) fn cpr_regular_layout(&self) -> bool {
        self.regular && !self.tilt.applied && !self.geometry.requires_resampling
    }
    pub(crate) fn slice_positions(&self) -> &[f64] {
        &self.positions
    }
    pub(crate) fn slice_index_of_position(&self, pos: f64) -> f64 {
        self.position_to_index(pos)
    }
}
impl Volume {
    pub(crate) fn first_slice_default_window(&self) -> Option<WindowLevel> {
        self.slices.first().map(|f| f.default_window.clone())
    }
}
impl Sampler {
    pub(crate) fn shared(vol: Arc<Volume>, values: Arc<Vec<f32>>) -> Sampler {
        Sampler::with_data(vol, SamplerData::Shared(values))
    }
}
impl Volume {
    pub(crate) fn is_regular(&self) -> bool {
        self.regular
    }
    pub(crate) fn slice_of_position(&self, pos: f64) -> f64 {
        self.position_to_slice(pos)
    }
    pub(crate) fn uniform_slice_step_value(&self) -> f64 {
        self.uniform_slice_step()
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn derived_f32(
        source: &Volume,
        cols: i64,
        rows: i64,
        depth: i64,
        origin: Vec3,
        col_spacing: f64,
        row_spacing: f64,
        slice_spacing: f64,
        values: Vec<f32>,
    ) -> Volume {
        let mut photometric = normalized_photometric(source.photometric());
        if photometric.is_empty() {
            photometric = "MONOCHROME2".into();
        }
        let mut default_window = WindowLevel::new(40.0, 400.0);
        if let Some(first) = source.slices.first()
            && first.default_window.width > 0.0
        {
            default_window = first.default_window.clone();
        }
        let mut frames = Vec::with_capacity(depth.max(0) as usize);
        let mut positions = Vec::with_capacity(depth.max(0) as usize);
        let mut origins = Vec::with_capacity(depth.max(0) as usize);
        let mut geometries = Vec::with_capacity(depth.max(0) as usize);
        for z in 0..depth {
            let slice_origin = origin.add(source.normal.scale(z as f64 * slice_spacing));
            frames.push(Arc::new(Frame {
                metadata: Metadata {
                    rows: rows as u16,
                    columns: cols as u16,
                    samples_per_pixel: 1,
                    bits_allocated: 32,
                    bits_stored: 32,
                    high_bit: 31,
                    photometric_interpretation: photometric.clone(),
                    ..Default::default()
                },
                default_window: default_window.clone(),
                ..Default::default()
            }));
            positions.push(z as f64 * slice_spacing);
            origins.push(slice_origin);
            geometries.push(SliceGeometry {
                origin: slice_origin,
                row_dir: source.axis_x,
                col_dir: source.axis_y,
                normal: source.normal,
                row_spacing,
                col_spacing,
                rows,
                columns: cols,
            });
        }
        let geometry = build_volume_geometry(&geometries, GeometryTolerances::default_tolerances());
        let target = Volume {
            cols,
            rows,
            depth,
            origin,
            axis_x: source.axis_x,
            axis_y: source.axis_y,
            normal: source.normal,
            col_spacing,
            row_spacing,
            slice_spacing,
            slice_source: slice_spacing_source::UNKNOWN,
            positions,
            slice_origins: origins,
            regular: true,
            tilt: GantryTiltCorrection::default(),
            geometry,
            slices: frames,
            uniform_step: OnceLock::new(),
            hu_range: OnceLock::new(),
            payload: OnceLock::new(),
            regularized: OnceLock::new(),
            link: Default::default(),
        };
        let _ = target
            .payload
            .set(Ok(Arc::new(Payload::F32(values.into()))));
        target
    }
}
impl Volume {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn acquired_lattice(
        cols: i64,
        rows: i64,
        origin: Vec3,
        axis_x: Vec3,
        axis_y: Vec3,
        normal: Vec3,
        col_spacing: f64,
        row_spacing: f64,
        slice_spacing: f64,
        tilt_applied: bool,
        positions: Vec<f64>,
        slice_origins: Vec<Vec3>,
    ) -> Volume {
        Volume {
            cols,
            rows,
            depth: positions.len() as i64,
            origin,
            axis_x,
            axis_y,
            normal,
            col_spacing,
            row_spacing,
            slice_spacing,
            slice_source: slice_spacing_source::UNKNOWN,
            positions,
            slice_origins,
            regular: false,
            tilt: GantryTiltCorrection {
                applied: tilt_applied,
                ..GantryTiltCorrection::default()
            },
            geometry: VolumeGeometry::default(),
            slices: Vec::new(),
            uniform_step: OnceLock::new(),
            hu_range: OnceLock::new(),
            payload: OnceLock::new(),
            regularized: OnceLock::new(),
            link: Default::default(),
        }
    }
}
impl VoxelLine<'_> {
    pub(crate) fn affine_parts(&self) -> Option<(f64, f64, f64, f64, f64, f64)> {
        self.affine
            .then_some((self.x0, self.y0, self.pos0, self.dx, self.dy, self.dpos))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegularGridInterpolation {
    #[default]
    Linear,
    CubicBSpline,
}
impl RegularGridInterpolation {
    pub fn as_str(self) -> &'static str {
        match self {
            RegularGridInterpolation::Linear => "linear",
            RegularGridInterpolation::CubicBSpline => "cubic-bspline",
        }
    }
}
impl Volume {
    pub fn resample_regular_grid_context(
        self: &Arc<Self>,
        ctx: Option<&dyn super::parallel::Cancellation>,
        interpolation: RegularGridInterpolation,
    ) -> Result<(Vec<f32>, RegularGridGeometry), RenderError> {
        if self.cols <= 0 || self.rows <= 0 || self.depth <= 0 {
            return Err(RenderError::other(
                "render: cannot resample an empty volume",
            ));
        }
        let grid = plan_regular_grid_geometry(&self.geometry)?;
        let [cols, rows, depth] = grid.dimensions;
        let count = checked_voxel_count(cols, rows, depth)
            .ok_or_else(|| RenderError::other("render: regularized grid dimensions overflow"))?;
        let Some(sampler) = Sampler::decoded(self) else {
            return Err(RenderError::other(
                "render: cannot prepare source sampler for regularization",
            ));
        };
        if let Some(e) = super::parallel::ctx_err(ctx) {
            return Err(e);
        }
        let mut values = vec![0f32; count];
        match interpolation {
            RegularGridInterpolation::Linear => {
                fill_regular_grid(self, &grid, &mut values, |p| sampler.trilinear_at(p));
            }
            RegularGridInterpolation::CubicBSpline => {
                let read = |x: i64, y: i64, z: i64| sampler.value_or_zero(x, y, z);
                let spline = super::volume_bspline::CubicBSpline::new(
                    ctx,
                    [0, 0, 0],
                    [self.cols, self.rows, self.depth],
                    Some(&read),
                )?;
                let (c, r, d) = (self.cols, self.rows, self.depth);
                fill_regular_grid(self, &grid, &mut values, |p| {
                    let x = clamp_continuous_index(p.x, c, 1e-3)?;
                    let y = clamp_continuous_index(p.y, r, 1e-3)?;
                    let z = clamp_continuous_index(p.z, d, 1e-3)?;
                    Some(spline.at(Vec3::new(x, y, z)))
                });
            }
        }
        if let Some(e) = super::parallel::ctx_err(ctx) {
            return Err(e);
        }
        Ok((values, grid))
    }
}
