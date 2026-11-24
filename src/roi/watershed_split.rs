#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::connected_components::Connectivity3D;
use super::distance3d::VolumeMask;
use super::grayscale_reconstruction::{
    ERR_INVALID_MORPHOLOGY, ERR_MORPHOLOGY_TOO_LARGE, GrayscaleOptions, extended_maxima,
};
use super::resample_grid::{Grid, IndexBox};
use super::resample_labels::LabelVolume;
use super::watershed::{WatershedOptions, label_components, watershed_from_markers};
use super::{Cancellation, Error};
use crate::fmath;
use crate::imagefilter::Volume;
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SplitOptions {
    pub height_mm: f64,
    pub connectivity: Connectivity3D,
    pub max_voxels: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SplitResult {
    pub labels: LabelVolume,
    pub region_voxels: Vec<i64>,
}
impl SplitResult {
    pub fn regions(&self) -> i64 {
        self.region_voxels.len() as i64
    }
}
pub fn split_touching_objects(
    ctx: Option<&dyn Cancellation>,
    grid: &Grid,
    mask: &VolumeMask,
    opts: SplitOptions,
) -> Result<SplitResult, Error> {
    if !(opts.height_mm > 0.0) || opts.height_mm.is_infinite() {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!(
                "split height {} mm must be positive and finite",
                float_v(opts.height_mm)
            ),
        ));
    }
    let gray_opts = GrayscaleOptions {
        connectivity: opts.connectivity,
        max_voxels: opts.max_voxels,
    }
    .normalize()?;
    mask.validate()?;
    if grid.columns != mask.columns || grid.rows != mask.rows || grid.slices != mask.slices {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!(
                "grid {}x{}x{}, mask {}x{}x{}",
                grid.columns, grid.rows, grid.slices, mask.columns, mask.rows, mask.slices
            ),
        ));
    }
    let mut result = SplitResult {
        labels: LabelVolume::new(grid.clone())?,
        region_voxels: Vec::new(),
    };
    let Some(mut bx) = volume_mask_bounds(mask) else {
        return Ok(result);
    };
    for (axis, n) in [mask.columns, mask.rows, mask.slices]
        .into_iter()
        .enumerate()
    {
        bx.min[axis] = (bx.min[axis] - 1).max(0);
        bx.max[axis] = (bx.max[axis] + 1).min(n);
    }
    let sub = crop_volume_mask(mask, bx);
    let n = sub.voxels.len() as i64;
    if n > gray_opts.max_voxels {
        return Err(Error::wrap(
            ERR_MORPHOLOGY_TOO_LARGE,
            format_args!(
                "the mask's bounding box has {} voxels, budget {}",
                n, gray_opts.max_voxels
            ),
        ));
    }
    let distance = sub.distance_transform(ctx, false)?;
    let mut relief = Volume {
        dims: [sub.columns, sub.rows, sub.slices],
        spacing_mm: [axis_step(&sub.x), axis_step(&sub.y), axis_step(&sub.z)],
        data: vec![0.0f32; sub.voxels.len()],
    };
    let mut peak = 0.0f64;
    for (i, &inside) in sub.voxels.iter().enumerate() {
        if inside {
            peak = fmath::max(peak, distance[i]);
        }
    }
    if peak.is_infinite() {
        for (i, &inside) in mask.voxels.iter().enumerate() {
            if inside {
                result.labels.labels[i] = 1;
            }
        }
        result.region_voxels = vec![mask.count()];
        return Ok(result);
    }
    let outside = (-(peak + opts.height_mm + 1.0)) as f32;
    for (i, &inside) in sub.voxels.iter().enumerate() {
        relief.data[i] = if inside { distance[i] as f32 } else { outside };
    }
    let mut peaks = extended_maxima(ctx, &relief, opts.height_mm, gray_opts)?;
    for (i, &inside) in sub.voxels.iter().enumerate() {
        peaks.voxels[i] = peaks.voxels[i] && inside;
    }
    peaks.x = sub.x.clone();
    peaks.y = sub.y.clone();
    peaks.z = sub.z.clone();
    let sub_grid = grid.crop(bx);
    let (markers, count) = label_components(ctx, &sub_grid, &peaks, gray_opts.connectivity)?;
    let count = count as usize;
    for v in relief.data.iter_mut() {
        *v = -*v;
    }
    let flood = watershed_from_markers(
        ctx,
        &relief,
        &markers,
        WatershedOptions {
            connectivity: gray_opts.connectivity,
            mark_watershed_line: false,
            mask: Some(&sub),
            max_voxels: gray_opts.max_voxels,
        },
    )?;
    if flood.unreached != 0 {
        return Err(Error::wrap(
            ERR_INVALID_MORPHOLOGY,
            format_args!(
                "{} mask voxels were not reached by any marker",
                flood.unreached
            ),
        ));
    }
    let mut sizes = vec![0i64; count + 1];
    for &l in &flood.labels.labels {
        sizes[l as usize] += 1;
    }
    let mut order: Vec<usize> = (1..=count).collect();
    order.sort_by(|&a, &b| sizes[b].cmp(&sizes[a]));
    let mut rank = vec![0u16; count + 1];
    result.region_voxels = vec![0i64; count];
    for (k, &l) in order.iter().enumerate() {
        rank[l] = (k + 1) as u16;
        result.region_voxels[k] = sizes[l];
    }
    let (sx, sy) = (sub.columns as usize, sub.rows as usize);
    for z in 0..sub.slices as usize {
        for y in 0..sy {
            let src = &flood.labels.labels[(z * sy + y) * sx..(z * sy + y + 1) * sx];
            let dst = (((bx.min[2] + z as i64) * mask.rows + bx.min[1] + y as i64) * mask.columns
                + bx.min[0]) as usize;
            for (x, &l) in src.iter().enumerate() {
                result.labels.labels[dst + x] = rank[l as usize];
            }
        }
    }
    Ok(result)
}
fn volume_mask_bounds(m: &VolumeMask) -> Option<IndexBox> {
    let mut bx = IndexBox {
        min: [m.columns, m.rows, m.slices],
        max: [0; 3],
    };
    let mut found = false;
    let mut i = 0usize;
    for z in 0..m.slices {
        for y in 0..m.rows {
            for x in 0..m.columns {
                if m.voxels[i] {
                    found = true;
                    for (axis, v) in [x, y, z].into_iter().enumerate() {
                        bx.min[axis] = bx.min[axis].min(v);
                        bx.max[axis] = bx.max[axis].max(v + 1);
                    }
                }
                i += 1;
            }
        }
    }
    found.then_some(bx)
}
fn crop_volume_mask(m: &VolumeMask, bx: IndexBox) -> VolumeMask {
    let columns = bx.max[0] - bx.min[0];
    let rows = bx.max[1] - bx.min[1];
    let slices = bx.max[2] - bx.min[2];
    let mut out = VolumeMask {
        columns,
        rows,
        slices,
        x: m.x[bx.min[0] as usize..bx.max[0] as usize].to_vec(),
        y: m.y[bx.min[1] as usize..bx.max[1] as usize].to_vec(),
        z: m.z[bx.min[2] as usize..bx.max[2] as usize].to_vec(),
        voxels: vec![false; (columns * rows * slices) as usize],
    };
    let w = columns as usize;
    for z in 0..slices {
        for y in 0..rows {
            let src = m.index(bx.min[0], bx.min[1] + y, bx.min[2] + z);
            let dst = ((z * rows + y) * columns) as usize;
            out.voxels[dst..dst + w].copy_from_slice(&m.voxels[src..src + w]);
        }
    }
    out
}
fn axis_step(c: &[f64]) -> f64 {
    if c.len() < 2 {
        return 1.0;
    }
    (c[c.len() - 1] - c[0]) / (c.len() - 1) as f64
}
