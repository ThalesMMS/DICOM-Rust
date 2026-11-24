#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::advanced_geometry::finite_number;
use super::roimask::{MaskRun, RasterMask, clamp_span, intersect_run_lists, union_run_lists};
use super::segmentation::Segmentation3D;
use super::{Cancellation, Error, ImagePoint, ctx_err};
use crate::fmath;
use crate::render::VolumeGeometry;
use crate::render::grid::nearest_index;
pub fn brush(mask: &mut RasterMask, cx: i64, cy: i64, radius: i64, on: bool) {
    if radius < 0 {
        return;
    }
    let r2 = radius.wrapping_mul(radius);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy)) <= r2 {
                mask.set(cx.wrapping_add(dx), cy.wrapping_add(dy), on);
            }
        }
    }
}
impl RasterMask {
    pub fn brush_segment(&mut self, first: ImagePoint, last: ImagePoint, radius: i64, on: bool) {
        if self.columns == 0 || self.rows == 0 || radius < 0 {
            return;
        }
        if first == last {
            brush(self, first.x, first.y, radius, on);
            return;
        }
        let dx = last.x as f64 - first.x as f64;
        let dy = last.y as f64 - first.y as f64;
        let x_major = dx.abs() >= dy.abs();
        let (start, end, size, delta) = if x_major {
            (first.x, last.x, self.columns, dx)
        } else {
            (first.y, last.y, self.rows, dy)
        };
        let low = start.min(end).max(-radius);
        let high = start
            .max(end)
            .min((size - 1).min(i64::MAX - radius) + radius);
        let mut center = low;
        while center <= high {
            let t = (center as f64 - start as f64) / delta;
            if x_major {
                brush(
                    self,
                    center,
                    nearest_index(first.y as f64 + dy * t),
                    radius,
                    on,
                );
            } else {
                brush(
                    self,
                    nearest_index(first.x as f64 + dx * t),
                    center,
                    radius,
                    on,
                );
            }
            if center == high {
                break;
            }
            center += 1;
        }
    }
}
pub fn threshold_mask(
    columns: i64,
    rows: i64,
    lo: f64,
    hi: f64,
    value_at: Option<&mut dyn FnMut(i64, i64) -> Option<f64>>,
) -> RasterMask {
    let mut mask = RasterMask::new(columns, rows);
    let Some(value_at) = value_at else {
        return mask;
    };
    if !(lo <= hi) {
        return mask;
    }
    let mut selected = |x: i64, y: i64| -> bool {
        match value_at(x, y) {
            Some(v) => finite_number(v) && !(v < lo) && !(v > hi),
            None => false,
        }
    };
    for y in 0..rows {
        let mut x = 0i64;
        while x < columns {
            if !selected(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x < columns {
                if !selected(x, y) {
                    break;
                }
                x += 1;
            }
            mask.set_run(y, start, x);
        }
    }
    mask
}
pub fn threshold_segmentation(
    geometry: VolumeGeometry,
    columns: i64,
    rows: i64,
    lo: f64,
    hi: f64,
    value_at: Option<&mut dyn FnMut(i64, i64, i64) -> Option<f64>>,
) -> Segmentation3D {
    let slices = geometry.slices.as_ref().map_or(0, Vec::len) as i64;
    let mut seg = Segmentation3D::new(geometry, columns, rows);
    let Some(value_at) = value_at else {
        return seg;
    };
    for slice in 0..slices {
        let mask = threshold_mask(
            columns,
            rows,
            lo,
            hi,
            Some(&mut |x: i64, y: i64| value_at(x, y, slice)),
        );
        seg.set_mask(slice, Some(mask));
    }
    seg
}
pub fn interpolate_mask(first: &RasterMask, last: &RasterMask, t: f64) -> Option<RasterMask> {
    interpolate_mask_with_workspace(first, last, t, &mut None)
}
fn interpolate_mask_with_workspace(
    first: &RasterMask,
    last: &RasterMask,
    t: f64,
    workspace: &mut Option<MaskInterpolationWorkspace>,
) -> Option<RasterMask> {
    if first.columns != last.columns || first.rows != last.rows {
        return None;
    }
    if t <= 0.0 {
        return Some(first.clone());
    }
    if t >= 1.0 {
        return Some(last.clone());
    }
    let mut out = RasterMask::new(first.columns, first.rows);
    if first.columns == 0 || first.rows == 0 {
        return Some(out);
    }
    let (first_bounds, last_bounds) = match (mask_bounds(first), mask_bounds(last)) {
        (None, None) => return Some(out),
        (None, Some(_)) => return Some(last.clone()),
        (Some(_), None) => return Some(first.clone()),
        (Some(a), Some(b)) => (a, b),
    };
    let target_bounds = interpolate_bounds(first_bounds, last_bounds, t);
    let first_aligned = remap_mask_to_bounds(first, first_bounds, target_bounds);
    let last_aligned = remap_mask_to_bounds(last, last_bounds, target_bounds);
    if !workspace
        .as_ref()
        .is_some_and(|w| w.columns == first.columns && w.rows == first.rows)
    {
        *workspace = Some(MaskInterpolationWorkspace::new(first.columns, first.rows));
    }
    let w = workspace.as_mut().expect("a workspace");
    let mut first_field = std::mem::take(&mut w.first_field);
    let mut last_field = std::mem::take(&mut w.last_field);
    signed_distance_field_into(&mut first_field, &first_aligned, w);
    signed_distance_field_into(&mut last_field, &last_aligned, w);
    let below = |idx: usize| (1.0 - t) * first_field[idx] + t * last_field[idx] < 0.0;
    let columns = first.columns as usize;
    for y in 0..first.rows as usize {
        let row_start = y * columns;
        let mut x = 0usize;
        while x < columns {
            if below(row_start + x) {
                x += 1;
                continue;
            }
            let start = x;
            while x < columns {
                if below(row_start + x) {
                    break;
                }
                x += 1;
            }
            out.set_run(y as i64, start as i64, x as i64);
        }
    }
    w.first_field = first_field;
    w.last_field = last_field;
    Some(out)
}
pub fn interpolate_between_slices(
    seg: &mut Segmentation3D,
    first_slice: i64,
    last_slice: i64,
) -> i64 {
    interpolate_between_slices_context(None, seg, first_slice, last_slice).0
}
pub fn interpolate_between_slices_context(
    ctx: Option<&dyn Cancellation>,
    seg: &mut Segmentation3D,
    mut first_slice: i64,
    mut last_slice: i64,
) -> (i64, Option<Error>) {
    if let Some(err) = ctx_err(ctx) {
        return (0, Some(err));
    }
    if first_slice == last_slice {
        return (0, None);
    }
    if first_slice > last_slice {
        std::mem::swap(&mut first_slice, &mut last_slice);
    }
    if last_slice.wrapping_sub(first_slice) < 2 {
        return (0, None);
    }
    let Some(first) = seg.mask_at(first_slice).filter(|m| !m.is_empty()).cloned() else {
        return (0, None);
    };
    let Some(last) = seg.mask_at(last_slice).filter(|m| !m.is_empty()).cloned() else {
        return (0, None);
    };
    if first.columns != last.columns || first.rows != last.rows {
        return (0, None);
    }
    let mut written = 0i64;
    let span = last_slice.wrapping_sub(first_slice) as f64;
    let mut workspace = Some(MaskInterpolationWorkspace::new(first.columns, first.rows));
    let mut slice = first_slice + 1;
    while slice < last_slice {
        if let Some(err) = ctx_err(ctx) {
            return (written, Some(err));
        }
        let mask = interpolate_mask_with_workspace(
            &first,
            &last,
            slice.wrapping_sub(first_slice) as f64 / span,
            &mut workspace,
        );
        if let Some(mask) = mask.filter(|m| !m.is_empty()) {
            seg.set_mask(slice, Some(mask));
            written += 1;
        }
        slice += 1;
    }
    (written, None)
}
#[derive(Debug, Clone, Copy)]
struct RasterMaskBounds {
    min_x: i64,
    min_y: i64,
    max_x: i64,
    max_y: i64,
}
fn mask_bounds(mask: &RasterMask) -> Option<RasterMaskBounds> {
    if mask.is_empty() {
        return None;
    }
    let mut bounds = RasterMaskBounds {
        min_x: mask.columns,
        min_y: mask.rows,
        max_x: -1,
        max_y: -1,
    };
    mask.for_each_run(|y, run| {
        bounds.min_x = bounds.min_x.min(run.start);
        bounds.max_x = bounds.max_x.max(run.end - 1);
        bounds.min_y = bounds.min_y.min(y);
        bounds.max_y = bounds.max_y.max(y);
    });
    (bounds.max_x >= bounds.min_x && bounds.max_y >= bounds.min_y).then_some(bounds)
}
fn interpolate_bounds(first: RasterMaskBounds, last: RasterMaskBounds, t: f64) -> RasterMaskBounds {
    RasterMaskBounds {
        min_x: nearest_index(lerp_float(first.min_x as f64, last.min_x as f64, t)),
        min_y: nearest_index(lerp_float(first.min_y as f64, last.min_y as f64, t)),
        max_x: nearest_index(lerp_float(first.max_x as f64, last.max_x as f64, t)),
        max_y: nearest_index(lerp_float(first.max_y as f64, last.max_y as f64, t)),
    }
}
fn remap_mask_to_bounds(
    mask: &RasterMask,
    from: RasterMaskBounds,
    to: RasterMaskBounds,
) -> RasterMask {
    let mut out = RasterMask::new(mask.columns, mask.rows);
    mask.for_each_run(|y, run| {
        let mapped_y = remap_mask_coordinate(y, from.min_y, from.max_y, to.min_y, to.max_y);
        let mut mapped_start =
            remap_mask_coordinate(run.start, from.min_x, from.max_x, to.min_x, to.max_x);
        let mut mapped_end =
            remap_mask_coordinate(run.end - 1, from.min_x, from.max_x, to.min_x, to.max_x);
        if mapped_start > mapped_end {
            std::mem::swap(&mut mapped_start, &mut mapped_end);
        }
        out.set_run(mapped_y, mapped_start, mapped_end.wrapping_add(1));
    });
    out
}
fn remap_mask_coordinate(
    value: i64,
    from_min: i64,
    from_max: i64,
    to_min: i64,
    to_max: i64,
) -> i64 {
    if from_max == from_min {
        return nearest_index((to_min as f64 + to_max as f64) / 2.0);
    }
    let position = value.wrapping_sub(from_min) as f64 / from_max.wrapping_sub(from_min) as f64;
    nearest_index(lerp_float(to_min as f64, to_max as f64, position))
}
fn lerp_float(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
struct MaskInterpolationWorkspace {
    columns: i64,
    rows: i64,
    features: Vec<bool>,
    tmp: Vec<f64>,
    distance_scratch: Vec<f64>,
    first_field: Vec<f64>,
    last_field: Vec<f64>,
    line_values: Vec<f64>,
    line_output: Vec<f64>,
    locations: Vec<usize>,
    boundaries: Vec<f64>,
}
impl MaskInterpolationWorkspace {
    fn new(columns: i64, rows: i64) -> MaskInterpolationWorkspace {
        let total = (columns * rows) as usize;
        let line_length = columns.max(rows) as usize;
        MaskInterpolationWorkspace {
            columns,
            rows,
            features: vec![false; total],
            tmp: vec![0.0; total],
            distance_scratch: vec![0.0; total],
            first_field: vec![0.0; total],
            last_field: vec![0.0; total],
            line_values: vec![0.0; line_length],
            line_output: vec![0.0; line_length],
            locations: vec![0; line_length],
            boundaries: vec![0.0; line_length + 1],
        }
    }
}
fn signed_distance_field_into(
    field: &mut [f64],
    mask: &RasterMask,
    workspace: &mut MaskInterpolationWorkspace,
) {
    workspace.features.fill(false);
    let columns = mask.columns as usize;
    mask.for_each_run(|y, run| {
        let row_start = y as usize * columns;
        for x in run.start..run.end {
            workspace.features[row_start + x as usize] = true;
        }
    });
    let max_distance = fmath::hypot(mask.columns as f64, mask.rows as f64);
    euclidean_distance_to_features_into(
        field,
        false,
        mask.columns,
        mask.rows,
        max_distance,
        workspace,
    );
    for feature in workspace.features.iter_mut() {
        *feature = !*feature;
    }
    euclidean_distance_to_features_into(
        field,
        true,
        mask.columns,
        mask.rows,
        max_distance,
        workspace,
    );
    for (index, &outside) in workspace.features.iter().enumerate() {
        if outside {
            field[index] = -field[index];
        } else {
            field[index] = workspace.distance_scratch[index];
        }
    }
}
fn euclidean_distance_to_features_into(
    out: &mut [f64],
    into_scratch: bool,
    columns: i64,
    rows: i64,
    max_distance: f64,
    workspace: &mut MaskInterpolationWorkspace,
) {
    if columns <= 0 || rows <= 0 {
        return;
    }
    const LARGE_DISTANCE: f64 = 1e12;
    let (columns, rows) = (columns as usize, rows as usize);
    let w = workspace;
    for y in 0..rows {
        for x in 0..columns {
            w.line_values[x] = if w.features[y * columns + x] {
                0.0
            } else {
                LARGE_DISTANCE
            };
        }
        distance_transform_1d_into(
            &mut w.line_output[..columns],
            &w.line_values[..columns],
            &mut w.locations[..columns],
            &mut w.boundaries[..columns + 1],
        );
        w.tmp[y * columns..(y + 1) * columns].copy_from_slice(&w.line_output[..columns]);
    }
    for x in 0..columns {
        for y in 0..rows {
            w.line_values[y] = w.tmp[y * columns + x];
        }
        distance_transform_1d_into(
            &mut w.line_output[..rows],
            &w.line_values[..rows],
            &mut w.locations[..rows],
            &mut w.boundaries[..rows + 1],
        );
        for y in 0..rows {
            let mut distance = w.line_output[y].sqrt();
            if distance > max_distance {
                distance = max_distance;
            }
            if into_scratch {
                w.distance_scratch[y * columns + x] = distance;
            } else {
                out[y * columns + x] = distance;
            }
        }
    }
}
fn distance_transform_1d_into(
    out: &mut [f64],
    values: &[f64],
    locations: &mut [usize],
    boundaries: &mut [f64],
) {
    let n = values.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    locations[0] = 0;
    boundaries[0] = f64::NEG_INFINITY;
    boundaries[1] = f64::INFINITY;
    for q in 1..n {
        let mut s = distance_parabola_intersection(values, q, locations[k]);
        while s <= boundaries[k] {
            k -= 1;
            s = distance_parabola_intersection(values, q, locations[k]);
        }
        k += 1;
        locations[k] = q;
        boundaries[k] = s;
        boundaries[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, value) in out.iter_mut().enumerate().take(n) {
        while boundaries[k + 1] < q as f64 {
            k += 1;
        }
        let delta = (q as i64 - locations[k] as i64) as f64;
        *value = delta * delta + values[locations[k]];
    }
}
fn distance_parabola_intersection(values: &[f64], q: usize, p: usize) -> f64 {
    let qf = q as f64;
    let pf = p as f64;
    ((values[q] + qf * qf) - (values[p] + pf * pf)) / (2.0 * qf - 2.0 * pf)
}
pub fn dilate(mask: &RasterMask) -> RasterMask {
    let mut out = RasterMask::new(mask.columns, mask.rows);
    for (y, runs) in mask.runs.iter().enumerate() {
        let mut expanded = expand_run_list(runs, mask.columns);
        if y > 0 {
            expanded = union_run_lists(&expanded, &mask.runs[y - 1], mask.columns);
        }
        if (y as i64) + 1 < mask.rows {
            expanded = union_run_lists(&expanded, &mask.runs[y + 1], mask.columns);
        }
        out.runs[y] = expanded;
    }
    out
}
fn expand_run_list(runs: &[MaskRun], columns: i64) -> Vec<MaskRun> {
    let mut out: Vec<MaskRun> = Vec::with_capacity(runs.len());
    for run in runs {
        let (start, end) = clamp_span(run.start - 1, run.end + 1, columns);
        if start >= end {
            continue;
        }
        if let Some(last) = out.last_mut()
            && start <= last.end
        {
            if end > last.end {
                last.end = end;
            }
            continue;
        }
        out.push(MaskRun { start, end });
    }
    out
}
pub fn erode(mask: &RasterMask) -> RasterMask {
    let mut out = RasterMask::new(mask.columns, mask.rows);
    let mut y = 1usize;
    while (y as i64) + 1 < mask.rows {
        let mut horizontal: Vec<MaskRun> = Vec::with_capacity(mask.runs[y].len());
        for run in &mask.runs[y] {
            if run.start + 1 < run.end - 1 {
                horizontal.push(MaskRun {
                    start: run.start + 1,
                    end: run.end - 1,
                });
            }
        }
        let horizontal = intersect_run_lists(&horizontal, &mask.runs[y - 1]);
        out.runs[y] = intersect_run_lists(&horizontal, &mask.runs[y + 1]);
        y += 1;
    }
    out
}
