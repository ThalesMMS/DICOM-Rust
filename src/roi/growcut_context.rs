use super::growcut::{
    GROW_CUT_BACKGROUND, GROW_CUT_FOREGROUND, GrowCutField, GrowCutPriorityQueue, GrowCutQueueItem,
    GrowCutValue2D, GrowCutValue3D, grow_cut_append_result_runs,
};
use super::roimask::RasterMask;
use super::{Cancellation, Error, ctx_err};
use std::collections::BTreeMap;
pub const ERR_GROW_CUT_INVALID_DIMENSIONS: &str = "roi: invalid GrowCut dimensions";
pub const ERR_GROW_CUT_DIMENSION_OVERFLOW: &str = "roi: GrowCut dimensions overflow";
pub const ERR_GROW_CUT_RESOURCE_LIMIT: &str = "roi: GrowCut resource limit exceeded";
pub const ERR_GROW_CUT_INVALID_SEEDS: &str = "roi: invalid GrowCut seeds";
pub const ERR_GROW_CUT_MISSING_SEEDS: &str =
    "roi: GrowCut requires foreground and background seeds";
pub const ERR_GROW_CUT_NON_FINITE_VALUE: &str = "roi: GrowCut value is not finite";
pub const ERR_GROW_CUT_CANCELLED: &str = "roi: GrowCut cancelled";
pub const ERR_GROW_CUT_INVALID_VALUE_SOURCE: &str = "roi: GrowCut value source is nil";
pub const ERR_GROW_CUT_INVALID_LIMITS: &str = "roi: invalid GrowCut resource limits";
const DEFAULT_GROW_CUT_MAX_VOXELS: i64 = 32 * 1024 * 1024;
const DEFAULT_GROW_CUT_MAX_BYTES: i64 = 768 * 1024 * 1024;
const DEFAULT_GROW_CUT_MAX_QUEUE_ITEMS: i64 = 4 * 1024 * 1024;
const GROW_CUT_DENSE_BYTES_PER_VOXEL: i64 = 18;
const GROW_CUT_QUEUE_ITEM_BYTES: i64 = 32;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GrowCutLimits {
    pub max_voxels: i64,
    pub max_bytes: i64,
    pub max_queue_items: i64,
}
pub fn default_grow_cut_limits() -> GrowCutLimits {
    GrowCutLimits {
        max_voxels: DEFAULT_GROW_CUT_MAX_VOXELS,
        max_bytes: DEFAULT_GROW_CUT_MAX_BYTES,
        max_queue_items: DEFAULT_GROW_CUT_MAX_QUEUE_ITEMS,
    }
}
fn normalize_grow_cut_limits(mut limits: GrowCutLimits) -> Result<GrowCutLimits, Error> {
    if limits.max_voxels < 0 || limits.max_bytes < 0 || limits.max_queue_items < 0 {
        return Err(Error::sentinel(ERR_GROW_CUT_INVALID_LIMITS));
    }
    let defaults = default_grow_cut_limits();
    if limits.max_voxels == 0 {
        limits.max_voxels = defaults.max_voxels;
    }
    if limits.max_bytes == 0 {
        limits.max_bytes = defaults.max_bytes;
    }
    if limits.max_queue_items == 0 {
        limits.max_queue_items = defaults.max_queue_items;
    }
    Ok(limits)
}
pub fn grow_cut_2d_context(
    ctx: Option<&dyn Cancellation>,
    columns: i64,
    rows: i64,
    foreground: Option<&RasterMask>,
    background: Option<&RasterMask>,
    value_at: Option<GrowCutValue2D<'_>>,
    limits: GrowCutLimits,
) -> Result<RasterMask, Error> {
    let Some(value_at) = value_at else {
        return Err(Error::sentinel(ERR_GROW_CUT_INVALID_VALUE_SOURCE));
    };
    let seeds = |mask: Option<&RasterMask>| -> BTreeMap<i64, RasterMask> {
        mask.map(|m| BTreeMap::from([(0, m.clone())]))
            .unwrap_or_default()
    };
    let mut result = grow_cut_labels_context(
        ctx,
        columns,
        rows,
        1,
        &seeds(foreground),
        &seeds(background),
        false,
        Some(&|x, y, _| value_at(x, y)),
        limits,
    )?;
    Ok(result
        .remove(&0)
        .unwrap_or_else(|| RasterMask::new(columns, rows)))
}
pub fn grow_cut_3d_context(
    ctx: Option<&dyn Cancellation>,
    columns: i64,
    rows: i64,
    slices: i64,
    seeds: GrowCutSeeds3D<'_>,
    value_at: Option<GrowCutValue3D<'_>>,
    limits: GrowCutLimits,
) -> Result<BTreeMap<i64, RasterMask>, Error> {
    grow_cut_labels_context(
        ctx,
        columns,
        rows,
        slices,
        seeds.foreground,
        seeds.background,
        true,
        value_at,
        limits,
    )
}
#[derive(Debug, Clone, Copy)]
pub struct GrowCutSeeds3D<'a> {
    pub foreground: &'a BTreeMap<i64, RasterMask>,
    pub background: &'a BTreeMap<i64, RasterMask>,
}
fn grow_cut_context_error(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match ctx_err(ctx) {
        Some(err) => Err(Error::wrap_err(ERR_GROW_CUT_CANCELLED, &err)),
        None => Ok(()),
    }
}
pub(crate) fn validate_grow_cut_dimensions(
    columns: i64,
    rows: i64,
    slices: i64,
    limits: &GrowCutLimits,
) -> Result<(i64, i64), Error> {
    if columns <= 0 || rows <= 0 || slices <= 0 {
        return Err(Error::sentinel(ERR_GROW_CUT_INVALID_DIMENSIONS));
    }
    if columns > i64::MAX / rows {
        return Err(Error::sentinel(ERR_GROW_CUT_DIMENSION_OVERFLOW));
    }
    let plane = columns * rows;
    if plane > i64::MAX / slices {
        return Err(Error::sentinel(ERR_GROW_CUT_DIMENSION_OVERFLOW));
    }
    let total = plane * slices;
    if total > limits.max_voxels {
        return Err(Error::wrap(
            ERR_GROW_CUT_RESOURCE_LIMIT,
            format!("{total} voxels exceeds {}", limits.max_voxels),
        ));
    }
    let estimated = grow_cut_estimated_bytes(total, limits.max_queue_items);
    if estimated.is_none_or(|bytes| bytes > limits.max_bytes) {
        return Err(Error::wrap(
            ERR_GROW_CUT_RESOURCE_LIMIT,
            format!(
                "estimated working set {} bytes exceeds {}",
                estimated.unwrap_or(0),
                limits.max_bytes
            ),
        ));
    }
    Ok((plane, total))
}
fn grow_cut_estimated_bytes(total: i64, queue_items: i64) -> Option<i64> {
    if total < 0 || queue_items < 0 {
        return None;
    }
    if queue_items > i64::MAX / GROW_CUT_QUEUE_ITEM_BYTES {
        return None;
    }
    let queue_bytes = queue_items * GROW_CUT_QUEUE_ITEM_BYTES;
    if total > (i64::MAX - queue_bytes) / GROW_CUT_DENSE_BYTES_PER_VOXEL {
        return None;
    }
    Some(total * GROW_CUT_DENSE_BYTES_PER_VOXEL + queue_bytes)
}
fn validate_grow_cut_seed_maps(
    columns: i64,
    rows: i64,
    slices: i64,
    foreground: &BTreeMap<i64, RasterMask>,
    background: &BTreeMap<i64, RasterMask>,
) -> Result<(), Error> {
    let foreground_count = validate_grow_cut_seed_map(columns, rows, slices, foreground)?;
    let background_count = validate_grow_cut_seed_map(columns, rows, slices, background)?;
    if foreground_count == 0 || background_count == 0 {
        return Err(Error::sentinel(ERR_GROW_CUT_MISSING_SEEDS));
    }
    Ok(())
}
fn validate_grow_cut_seed_map(
    columns: i64,
    rows: i64,
    slices: i64,
    masks: &BTreeMap<i64, RasterMask>,
) -> Result<i64, Error> {
    let mut count = 0i64;
    for (&slice, mask) in masks {
        if slice < 0 || slice >= slices || mask.columns != columns || mask.rows != rows {
            return Err(Error::wrap(
                ERR_GROW_CUT_INVALID_SEEDS,
                format!("slice {slice} has incompatible dimensions"),
            ));
        }
        let (mut last_y, mut last_end) = (-1i64, 0i64);
        let mut invalid = false;
        mask.for_each_run(|y, run| {
            if invalid {
                return;
            }
            if y < 0 || y >= rows || run.start < 0 || run.start >= run.end || run.end > columns {
                invalid = true;
                return;
            }
            if y != last_y {
                (last_y, last_end) = (y, 0);
            }
            if run.start < last_end {
                invalid = true;
                return;
            }
            last_end = run.end;
            count += run.end - run.start;
        });
        if invalid {
            return Err(Error::wrap(
                ERR_GROW_CUT_INVALID_SEEDS,
                format!("malformed run in slice {slice}"),
            ));
        }
    }
    Ok(count)
}
#[allow(clippy::too_many_arguments)]
fn grow_cut_labels_context(
    ctx: Option<&dyn Cancellation>,
    columns: i64,
    rows: i64,
    slices: i64,
    foreground: &BTreeMap<i64, RasterMask>,
    background: &BTreeMap<i64, RasterMask>,
    include_slice_neighbours: bool,
    value_at: Option<GrowCutValue3D<'_>>,
    limits: GrowCutLimits,
) -> Result<BTreeMap<i64, RasterMask>, Error> {
    grow_cut_context_error(ctx)?;
    let Some(value_at) = value_at else {
        return Err(Error::sentinel(ERR_GROW_CUT_INVALID_VALUE_SOURCE));
    };
    let normalized = normalize_grow_cut_limits(limits)?;
    let (plane, total) = validate_grow_cut_dimensions(columns, rows, slices, &normalized)?;
    validate_grow_cut_seed_maps(columns, rows, slices, foreground, background)?;
    let mut field = GrowCutField::new(total);
    for z in 0..slices {
        for y in 0..rows {
            grow_cut_context_error(ctx)?;
            for x in 0..columns {
                let Some(value) = value_at(x, y, z) else {
                    continue;
                };
                if !value.is_finite() {
                    return Err(Error::with_message(
                        ERR_GROW_CUT_NON_FINITE_VALUE,
                        format!("{ERR_GROW_CUT_NON_FINITE_VALUE} at ({x},{y},{z})"),
                    ));
                }
                field.set(z * plane + y * columns + x, value);
            }
        }
    }
    if field.minimum == f64::INFINITY {
        return Ok(BTreeMap::new());
    }
    let contrast = field.contrast();
    let (values, valid) = (&field.values, &field.valid);
    let mut labels = vec![0u8; total as usize];
    let mut strengths = vec![0.0f64; total as usize];
    let mut queue = GrowCutPriorityQueue::default();
    let mut next_order = 0i64;
    let push = |queue: &mut GrowCutPriorityQueue, item: GrowCutQueueItem| -> Result<(), Error> {
        if queue.len() >= normalized.max_queue_items {
            return Err(Error::wrap(
                ERR_GROW_CUT_RESOURCE_LIMIT,
                format!("queue reached {} items", normalized.max_queue_items),
            ));
        }
        queue.push(item);
        Ok(())
    };
    for (masks, label) in [
        (foreground, GROW_CUT_FOREGROUND),
        (background, GROW_CUT_BACKGROUND),
    ] {
        for (&z, mask) in masks {
            grow_cut_context_error(ctx)?;
            let mut seed_err: Option<Error> = None;
            mask.for_each_run(|y, run| {
                if seed_err.is_some() || y < 0 || y >= rows {
                    return;
                }
                for x in run.start..run.end {
                    if let Err(err) = grow_cut_context_error(ctx) {
                        seed_err = Some(err);
                        return;
                    }
                    let index = z * plane + y * columns + x;
                    let at = index as usize;
                    if !valid[at] || strengths[at] >= 1.0 {
                        continue;
                    }
                    labels[at] = label;
                    strengths[at] = 1.0;
                    let item = GrowCutQueueItem {
                        index,
                        strength: 1.0,
                        label,
                        order: next_order,
                    };
                    if let Err(err) = push(&mut queue, item) {
                        seed_err = Some(err);
                        return;
                    }
                    next_order += 1;
                }
            });
            if let Some(err) = seed_err {
                return Err(err);
            }
        }
    }
    if queue.len() == 0 {
        return Ok(BTreeMap::new());
    }
    while queue.len() > 0 {
        grow_cut_context_error(ctx)?;
        let item = queue.pop();
        let at = item.index as usize;
        if labels[at] != item.label || (strengths[at] - item.strength).abs() > 1e-12 {
            continue;
        }
        let z = item.index / plane;
        let remainder = item.index % plane;
        let y = remainder / columns;
        let x = remainder % columns;
        let mut visit_err: Option<Error> = None;
        let mut visit = |nx: i64, ny: i64, nz: i64| {
            if visit_err.is_some()
                || nx < 0
                || nx >= columns
                || ny < 0
                || ny >= rows
                || nz < 0
                || nz >= slices
            {
                return;
            }
            let neighbour = nz * plane + ny * columns + nx;
            let n = neighbour as usize;
            if !valid[n] || strengths[n] >= 1.0 {
                return;
            }
            let mut affinity = 1.0 - (values[at] - values[n]).abs() / contrast;
            if affinity < 0.0 {
                affinity = 0.0;
            }
            let attack = item.strength * affinity;
            if attack <= strengths[n] + 1e-12 {
                return;
            }
            labels[n] = item.label;
            strengths[n] = attack;
            let pushed = GrowCutQueueItem {
                index: neighbour,
                strength: attack,
                label: item.label,
                order: next_order,
            };
            if let Err(err) = push(&mut queue, pushed) {
                visit_err = Some(err);
                return;
            }
            next_order += 1;
        };
        visit(x - 1, y, z);
        visit(x + 1, y, z);
        visit(x, y - 1, z);
        visit(x, y + 1, z);
        if include_slice_neighbours {
            visit(x, y, z - 1);
            visit(x, y, z + 1);
        }
        if let Some(err) = visit_err {
            return Err(err);
        }
    }
    let mut result = BTreeMap::new();
    grow_cut_append_result_runs(&mut result, &labels, columns, rows, slices, plane);
    Ok(result)
}
