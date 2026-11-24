#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
use super::samples::cutoff_value;
use super::{
    Document, Error, Kind, LABEL_MAP_SEGMENTATION_STORAGE, SEGMENTATION_TYPE_BINARY,
    SEGMENTATION_TYPE_FRACTIONAL, SEGMENTATION_TYPE_LABEL_MAP, geometry_for_frame, invalid,
    valid_slice_geometry,
};
use crate::fmath;
use crate::render::SliceGeometry;
use crate::render::grid::{
    DICOM_GRID_COORDINATE_TOLERANCE, DICOM_GRID_DIRECTION_TOLERANCE, nearest_index,
};
use crate::roi::resample_fraction::{
    FractionResampleOptions, FractionStack, resample_fraction_mask,
};
use crate::roi::resample_grid::{
    Grid, PointMapper, ResampleCoverage, grid_coverage, grid_from_slice, grid_from_slices,
};
use crate::roi::resample_labels::{LabelResampleMode, LabelStack, MaskStack, resample_label_masks};
use crate::roi::{Cancellation, RasterMask};
use crate::textfmt::float_v;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Default)]
pub struct ResampleOptions {
    pub mode: LabelResampleMode<'static>,
    pub fractional_threshold: f64,
    pub slice_thickness: f64,
}
pub struct Resampler {
    grid: Grid,
    layout: &'static str,
    mode: LabelResampleMode<'static>,
    cut: f64,
    numbers: Vec<i64>,
    binary: BTreeMap<i64, MaskStack>,
    fractional: BTreeMap<i64, FractionStack>,
    labels: Option<LabelStack>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Resampled {
    pub grid: Grid,
    pub coverage: ResampleCoverage,
    pub segments: Vec<ResampledSegment>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResampledSegment {
    pub number: i64,
    pub masks: Option<Vec<Option<RasterMask>>>,
    pub voxels: i64,
    pub preserved: bool,
    pub vanished: bool,
}
pub fn resample(
    ctx: Option<&dyn Cancellation>,
    doc: &Document,
    target: &Grid,
    opts: ResampleOptions,
) -> Result<Resampled, Error> {
    Resampler::new_context(ctx, doc, opts)?.resample(ctx, target)
}
impl Resampler {
    pub fn new(doc: &Document, opts: ResampleOptions) -> Result<Resampler, Error> {
        Resampler::new_context(None, doc, opts)
    }
    pub fn new_context(
        ctx: Option<&dyn Cancellation>,
        doc: &Document,
        opts: ResampleOptions,
    ) -> Result<Resampler, Error> {
        let cancelled = |i: usize| -> Result<(), Error> {
            match ctx.and_then(|c| c.err()) {
                Some(e) if i.is_multiple_of(256) => Err(Error::other(e)),
                _ => Ok(()),
            }
        };
        if doc.rows <= 0 || doc.columns <= 0 {
            return Err(invalid(format!(
                "Rows={} Columns={}",
                doc.rows, doc.columns
            )));
        }
        let mut threshold = opts.fractional_threshold;
        if threshold == 0.0 {
            threshold = 0.5;
        }
        if !(threshold > 0.0 && threshold <= 1.0) {
            return Err(invalid(format!(
                "fractional threshold {} is outside (0, 1]",
                float_v(threshold)
            )));
        }
        let (grid, frame_slices) = resample_grid(doc, opts.slice_thickness)?;
        let mut mode = opts.mode;
        mode.mapper = None;
        let mut r = Resampler {
            grid: grid.clone(),
            layout: resample_layout(doc),
            mode,
            cut: 0.0,
            numbers: Vec::new(),
            binary: BTreeMap::new(),
            fractional: BTreeMap::new(),
            labels: None,
        };
        let pixels = (doc.rows * doc.columns) as usize;
        let slice_count = grid.slices.max(0) as usize;
        match r.layout {
            SEGMENTATION_TYPE_LABEL_MAP => {
                let mut slices: Vec<Option<Vec<u16>>> = vec![None; slice_count];
                for (i, frame) in doc.frames.iter().enumerate() {
                    cancelled(i)?;
                    if frame.label_map.is_empty() {
                        continue;
                    }
                    if frame.label_map.len() != pixels {
                        return Err(invalid(format!(
                            "label-map frame {i} has {} values",
                            frame.label_map.len()
                        )));
                    }
                    match &mut slices[frame_slices[i] as usize] {
                        slot @ None => *slot = Some(frame.label_map.clone()),
                        Some(merged) => {
                            for (out, &value) in merged.iter_mut().zip(&frame.label_map) {
                                if value != 0 {
                                    *out = value;
                                }
                            }
                        }
                    }
                }
                r.labels = Some(LabelStack::new(grid, slices).map_err(Error::from_roi)?);
            }
            SEGMENTATION_TYPE_FRACTIONAL => {
                if doc.max_fractional_value < 1 || doc.max_fractional_value > 255 {
                    return Err(invalid(format!(
                        "Maximum Fractional Value {} is outside 1-255",
                        doc.max_fractional_value
                    )));
                }
                r.cut = cutoff_value(doc.max_fractional_value, threshold) as f64;
                let mut stacks: BTreeMap<i64, Vec<Option<Vec<u8>>>> = BTreeMap::new();
                for (i, frame) in doc.frames.iter().enumerate() {
                    cancelled(i)?;
                    if frame.segment_number <= 0 {
                        continue;
                    }
                    let slices = stacks
                        .entry(frame.segment_number)
                        .or_insert_with(|| vec![None; slice_count]);
                    if frame.fractional.is_empty() {
                        continue;
                    }
                    if frame.fractional.len() != pixels {
                        return Err(invalid(format!(
                            "fractional frame {i} has {} samples",
                            frame.fractional.len()
                        )));
                    }
                    match &mut slices[frame_slices[i] as usize] {
                        slot @ None => *slot = Some(frame.fractional.clone()),
                        Some(merged) => {
                            for (out, &value) in merged.iter_mut().zip(&frame.fractional) {
                                *out = (*out).max(value);
                            }
                        }
                    }
                }
                for (number, slices) in stacks {
                    let stack =
                        FractionStack::new(grid.clone(), slices).map_err(Error::from_roi)?;
                    r.fractional.insert(number, stack);
                    r.numbers.push(number);
                }
            }
            _ => {
                let mut stacks: BTreeMap<i64, Vec<Option<RasterMask>>> = BTreeMap::new();
                for (i, frame) in doc.frames.iter().enumerate() {
                    cancelled(i)?;
                    if frame.segment_number <= 0 {
                        continue;
                    }
                    let masks = stacks
                        .entry(frame.segment_number)
                        .or_insert_with(|| vec![None; slice_count]);
                    let Some(mask) = frame.mask.as_ref().filter(|m| !m.is_empty()) else {
                        continue;
                    };
                    if mask.columns != doc.columns || mask.rows != doc.rows {
                        return Err(invalid(format!(
                            "frame {i} mask is {}x{}",
                            mask.columns, mask.rows
                        )));
                    }
                    match &mut masks[frame_slices[i] as usize] {
                        slot @ None => *slot = Some(mask.clone()),
                        Some(merged) => merged.union(mask),
                    }
                }
                for (number, masks) in stacks {
                    if number > 0xFFFF {
                        return Err(invalid(format!("segment number {number}")));
                    }
                    let stack = MaskStack::new(grid.clone(), masks, 1).map_err(Error::from_roi)?;
                    r.binary.insert(number, stack);
                    r.numbers.push(number);
                }
            }
        }
        r.numbers.sort_unstable();
        Ok(r)
    }
    pub fn grid(&self) -> &Grid {
        &self.grid
    }
    pub fn layout(&self) -> &'static str {
        self.layout
    }
    pub fn resample(
        &self,
        ctx: Option<&dyn Cancellation>,
        target: &Grid,
    ) -> Result<Resampled, Error> {
        self.resample_impl(ctx, target, None)
    }
    pub fn resample_mapped(
        &self,
        ctx: Option<&dyn Cancellation>,
        target: &Grid,
        mapper: PointMapper<'_>,
    ) -> Result<Resampled, Error> {
        self.resample_impl(ctx, target, Some(mapper))
    }
    fn resample_impl(
        &self,
        ctx: Option<&dyn Cancellation>,
        target: &Grid,
        mapper: Option<PointMapper<'_>>,
    ) -> Result<Resampled, Error> {
        let mut out = Resampled {
            grid: target.clone(),
            ..Resampled::default()
        };
        let mode = LabelResampleMode {
            interpolator: self.mode.interpolator,
            sigma_mm: self.mode.sigma_mm,
            cutoff_sigmas: self.mode.cutoff_sigmas,
            disable_preservation: self.mode.disable_preservation,
            max_voxels: self.mode.max_voxels,
            mapper,
        };
        let mut coverage_set = false;
        match self.layout {
            SEGMENTATION_TYPE_LABEL_MAP => {
                let Some(labels) = &self.labels else {
                    return Err(invalid("label stack is missing"));
                };
                let mut result =
                    resample_label_masks(ctx, labels, target, mode).map_err(Error::from_roi)?;
                let report = result.report;
                let numbers: BTreeSet<u16> = report
                    .counts
                    .keys()
                    .chain(report.source_labels.iter())
                    .copied()
                    .collect();
                for label in numbers {
                    out.segments.push(ResampledSegment {
                        number: i64::from(label),
                        masks: result.masks.remove(&label),
                        voxels: report.counts.get(&label).copied().unwrap_or(0),
                        preserved: report.preserved.contains(&label),
                        vanished: report.vanished.contains(&label),
                    });
                }
                out.coverage = report.coverage;
                coverage_set = true;
            }
            SEGMENTATION_TYPE_FRACTIONAL => {
                for number in &self.numbers {
                    let result = resample_fraction_mask(
                        ctx,
                        &self.fractional[number],
                        target,
                        self.cut,
                        FractionResampleOptions {
                            max_voxels: self.mode.max_voxels,
                            mapper,
                        },
                    )
                    .map_err(Error::from_roi)?;
                    out.coverage = result.coverage;
                    coverage_set = true;
                    out.segments.push(ResampledSegment {
                        number: *number,
                        masks: Some(result.masks),
                        voxels: result.count,
                        ..ResampledSegment::default()
                    });
                }
            }
            _ => {
                for number in &self.numbers {
                    let mut result = resample_label_masks(ctx, &self.binary[number], target, mode)
                        .map_err(Error::from_roi)?;
                    out.segments.push(ResampledSegment {
                        number: *number,
                        masks: result.masks.remove(&1),
                        voxels: result.report.counts.get(&1).copied().unwrap_or(0),
                        preserved: !result.report.preserved.is_empty(),
                        vanished: !result.report.vanished.is_empty(),
                    });
                    out.coverage = result.report.coverage;
                    coverage_set = true;
                }
            }
        }
        if !coverage_set && mapper.is_some() {
            let slices = vec![None; self.grid.slices.max(0) as usize];
            let empty = LabelStack::new(self.grid.clone(), slices).map_err(Error::from_roi)?;
            let result =
                resample_label_masks(ctx, &empty, target, mode).map_err(Error::from_roi)?;
            out.coverage = result.report.coverage;
            coverage_set = true;
        }
        if !coverage_set {
            out.coverage = grid_coverage(ctx, &self.grid, target).map_err(Error::from_roi)?;
        }
        out.segments.sort_by_key(|s| s.number);
        Ok(out)
    }
}
fn resample_layout(doc: &Document) -> &'static str {
    if doc.sop_class_uid == LABEL_MAP_SEGMENTATION_STORAGE
        || doc.segmentation_type == SEGMENTATION_TYPE_LABEL_MAP
    {
        return SEGMENTATION_TYPE_LABEL_MAP;
    }
    if doc.segmentation_type == SEGMENTATION_TYPE_FRACTIONAL {
        return SEGMENTATION_TYPE_FRACTIONAL;
    }
    for frame in &doc.frames {
        if !frame.label_map.is_empty() {
            return SEGMENTATION_TYPE_LABEL_MAP;
        }
        if !frame.fractional.is_empty() {
            return SEGMENTATION_TYPE_FRACTIONAL;
        }
    }
    SEGMENTATION_TYPE_BINARY
}
fn no_geometry(detail: impl std::fmt::Display) -> Error {
    Error::wrap(Kind::NoGeometry, detail)
}
fn resample_grid(doc: &Document, mut thickness: f64) -> Result<(Grid, Vec<i64>), Error> {
    if doc.frames.is_empty() {
        return Err(no_geometry("no frames"));
    }
    let mut planes = Vec::with_capacity(doc.frames.len());
    for (i, frame) in doc.frames.iter().enumerate() {
        let Some(g) = geometry_for_frame(&doc.geometry, frame).filter(valid_slice_geometry) else {
            return Err(no_geometry(format!("frame {i} has no plane geometry")));
        };
        if g.rows != doc.rows || g.columns != doc.columns {
            return Err(no_geometry(format!(
                "frame {i} plane is {}x{}, document {}x{}",
                g.columns, g.rows, doc.columns, doc.rows
            )));
        }
        planes.push(g);
    }
    if thickness <= 0.0 {
        thickness = doc.geometry.mean_spacing;
    }
    if thickness <= 0.0 {
        thickness = fmath::min(planes[0].row_spacing, planes[0].col_spacing);
    }
    if !doc.geometry.slices().is_empty()
        && let Ok(grid) = stack_grid(doc.geometry.slices(), thickness)
        && let Some(slices) = place_frames(&grid, &planes)
    {
        return Ok((grid, slices));
    }
    let stack = distinct_planes(&planes);
    let grid = stack_grid(&stack, thickness).map_err(no_geometry)?;
    let Some(slices) = place_frames(&grid, &planes) else {
        return Err(no_geometry("frames do not share one grid"));
    };
    Ok((grid, slices))
}
fn stack_grid(slices: &[SliceGeometry], thickness: f64) -> Result<Grid, crate::roi::Error> {
    if slices.len() == 1 {
        let grid = grid_from_slice(&slices[0], thickness);
        grid.validate()?;
        return Ok(grid);
    }
    grid_from_slices(slices)
}
fn place_frames(grid: &Grid, planes: &[SliceGeometry]) -> Option<Vec<i64>> {
    let mut out = Vec::with_capacity(planes.len());
    for plane in planes {
        let (_, _, z, _) = grid.continuous_index(plane.origin);
        let k = nearest_index(z);
        if k < 0 || k >= grid.slices {
            return None;
        }
        let frame = plane.grid(1.0)?;
        let slice = grid.slice_geometry(k).grid(1.0)?;
        if !slice.same_geometry(
            &frame,
            DICOM_GRID_COORDINATE_TOLERANCE,
            DICOM_GRID_DIRECTION_TOLERANCE,
        ) {
            return None;
        }
        out.push(k);
    }
    Some(out)
}
fn distinct_planes(planes: &[SliceGeometry]) -> Vec<SliceGeometry> {
    let normal = planes[0]
        .row_dir
        .normalize()
        .cross(planes[0].col_dir.normalize())
        .normalize();
    let mut sorted: Vec<SliceGeometry> = Vec::with_capacity(planes.len());
    for plane in planes {
        let key = plane.origin.dot(normal);
        let at = sorted
            .iter()
            .rposition(|p| !(key < p.origin.dot(normal)))
            .map_or(0, |i| i + 1);
        sorted.insert(at, *plane);
    }
    let mut out = vec![sorted[0]];
    for plane in &sorted[1..] {
        if plane.origin.sub(out[out.len() - 1].origin).length() > 1e-3 {
            out.push(*plane);
        }
    }
    if out.len() < 3 {
        return out;
    }
    let mut step = f64::INFINITY;
    for i in 1..out.len() {
        step = fmath::min(step, out[i].origin.sub(out[i - 1].origin).length());
    }
    let total = out[out.len() - 1].origin.sub(out[0].origin);
    let count = fmath::f64_to_i64((total.length() / step).round()).wrapping_add(1);
    let n = out.len() as i64;
    if count == n || count > 4 * n + 16 || count <= 0 {
        return out;
    }
    for i in 1..out.len() {
        let gap = out[i].origin.sub(out[i - 1].origin).length() / step;
        if (gap - gap.round()).abs() > 0.01 {
            return out;
        }
    }
    (0..count)
        .map(|k| {
            let mut plane = out[0];
            plane.origin = out[0]
                .origin
                .add(total.scale(k as f64 / (count - 1) as f64));
            plane
        })
        .collect()
}
#[allow(dead_code)]
const _: &str = SEGMENTATION_TYPE_BINARY;
