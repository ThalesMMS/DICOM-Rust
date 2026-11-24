use super::resample_grid::{
    CoverageBuilder, DEFAULT_MAX_RESAMPLE_VOXELS, ERR_INVALID_RESAMPLE, Grid, GridMap, IndexBox,
    PointMapper, ResampleCoverage, TargetRows, covered_run, for_each_line, map_row,
};
use super::resample_labels::{BoundsBuilder, check_target_budget};
use super::roimask::RasterMask;
use super::{Cancellation, Error};
use crate::fmath;
use crate::textfmt::float_v;
pub trait FractionSource {
    fn fraction_grid(&self) -> &Grid;
    fn fraction_at(&self, x: i64, y: i64, z: i64) -> f64;
    fn label_bounds(&self) -> Option<(IndexBox, bool)> {
        None
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FractionStack {
    grid: Grid,
    slices: Vec<Option<Vec<u8>>>,
    bounds: IndexBox,
    filled: bool,
}
impl FractionStack {
    pub fn new(grid: Grid, slices: Vec<Option<Vec<u8>>>) -> Result<FractionStack, Error> {
        grid.validate()?;
        if slices.len() as i64 != grid.slices {
            return Err(Error::wrap(
                ERR_INVALID_RESAMPLE,
                format!(
                    "{} fraction slices for {} grid slices",
                    slices.len(),
                    grid.slices
                ),
            ));
        }
        let mut b = BoundsBuilder::default();
        for (z, values) in slices.iter().enumerate() {
            let Some(values) = values else {
                continue;
            };
            if values.len() as i64 != grid.columns * grid.rows {
                return Err(Error::wrap(
                    ERR_INVALID_RESAMPLE,
                    format!(
                        "fraction slice {z} has {} values, want {}",
                        values.len(),
                        grid.columns * grid.rows
                    ),
                ));
            }
            for (i, &value) in values.iter().enumerate() {
                if value != 0 {
                    let i = i as i64;
                    b.add(i % grid.columns, i / grid.columns, z as i64);
                }
            }
        }
        let (bounds, filled) = b.index_box();
        Ok(FractionStack {
            grid,
            slices,
            bounds,
            filled,
        })
    }
}
impl FractionSource for FractionStack {
    fn fraction_grid(&self) -> &Grid {
        &self.grid
    }
    fn fraction_at(&self, x: i64, y: i64, z: i64) -> f64 {
        match &self.slices[z as usize] {
            None => 0.0,
            Some(values) => f64::from(values[(y * self.grid.columns + x) as usize]),
        }
    }
    fn label_bounds(&self) -> Option<(IndexBox, bool)> {
        Some((self.bounds, self.filled))
    }
}
#[derive(Clone, Copy, Default)]
pub struct FractionResampleOptions<'a> {
    pub max_voxels: i64,
    pub mapper: Option<PointMapper<'a>>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FractionResampleResult {
    pub grid: Grid,
    pub values: Vec<f32>,
    pub coverage: ResampleCoverage,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FractionMaskResult {
    pub grid: Grid,
    pub masks: Vec<Option<RasterMask>>,
    pub count: i64,
    pub coverage: ResampleCoverage,
}
pub fn resample_fractions(
    ctx: Option<&dyn Cancellation>,
    src: &dyn FractionSource,
    target: &Grid,
    opts: FractionResampleOptions<'_>,
) -> Result<FractionResampleResult, Error> {
    check_target_budget(target, fraction_budget(&opts))?;
    let mut values = vec![0f32; target.voxel_count() as usize];
    let coverage = resample_fractions_into(ctx, src, target, opts.mapper, |y, z, x0, row| {
        let at = ((z * target.rows + y) * target.columns + x0) as usize;
        for (out, &value) in values[at..].iter_mut().zip(row) {
            *out = value as f32;
        }
    })?;
    Ok(FractionResampleResult {
        grid: target.clone(),
        values,
        coverage,
    })
}
pub fn resample_fraction_mask(
    ctx: Option<&dyn Cancellation>,
    src: &dyn FractionSource,
    target: &Grid,
    threshold: f64,
    opts: FractionResampleOptions<'_>,
) -> Result<FractionMaskResult, Error> {
    if !threshold.is_finite() {
        return Err(Error::wrap(
            ERR_INVALID_RESAMPLE,
            format!("threshold {}", float_v(threshold)),
        ));
    }
    check_target_budget(target, fraction_budget(&opts))?;
    let mut masks: Vec<Option<RasterMask>> = (0..target.slices)
        .map(|_| Some(RasterMask::new(target.columns, target.rows)))
        .collect();
    let mut row_counts = vec![0i32; (target.rows * target.slices) as usize];
    let coverage = resample_fractions_into(ctx, src, target, opts.mapper, |y, z, x0, values| {
        let (mut start, mut n) = (-1i64, 0i32);
        for i in 0..=values.len() {
            let on = i < values.len() && values[i] > 0.0 && values[i] >= threshold - 1e-9;
            if on {
                n += 1;
            }
            if on && start < 0 {
                start = i as i64;
            } else if !on && start >= 0 {
                if let Some(mask) = masks[z as usize].as_mut() {
                    mask.set_run(y, x0 + start, x0 + i as i64);
                }
                start = -1;
            }
        }
        row_counts[(z * target.rows + y) as usize] = n;
    })?;
    let mut count = 0i64;
    for z in 0..target.slices {
        let mut n = 0i64;
        for y in 0..target.rows {
            n += i64::from(row_counts[(z * target.rows + y) as usize]);
        }
        count += n;
        if n == 0 {
            masks[z as usize] = None;
        }
    }
    Ok(FractionMaskResult {
        grid: target.clone(),
        masks,
        count,
        coverage,
    })
}
fn fraction_budget(opts: &FractionResampleOptions<'_>) -> i64 {
    if opts.max_voxels > 0 {
        return opts.max_voxels;
    }
    DEFAULT_MAX_RESAMPLE_VOXELS
}
fn resample_fractions_into(
    ctx: Option<&dyn Cancellation>,
    src: &dyn FractionSource,
    target: &Grid,
    mapper: Option<PointMapper<'_>>,
    mut emit: impl FnMut(i64, i64, i64, &[f64]),
) -> Result<ResampleCoverage, Error> {
    let source_grid = src.fraction_grid();
    let sm = GridMap::new(source_grid)?;
    let limits = [source_grid.columns, source_grid.rows, source_grid.slices];
    let full = IndexBox {
        min: [0; 3],
        max: limits,
    };
    let (bounds, filled) = src.label_bounds().unwrap_or((full, true));
    let (mut eval_lo, mut eval_hi) = ([0.0f64; 3], [0.0f64; 3]);
    if filled {
        let mut grown = bounds;
        for i in 0..3 {
            grown.min[i] = (bounds.min[i] - 1).max(0);
            grown.max[i] = (bounds.max[i] + 1).min(limits[i]);
        }
        (eval_lo, eval_hi) = sm.box_extent(grown);
    }
    let within = |u: f64, v: f64, s: f64| {
        u >= eval_lo[0]
            && u < eval_hi[0]
            && v >= eval_lo[1]
            && v < eval_hi[1]
            && s >= eval_lo[2]
            && s < eval_hi[2]
    };
    let rows = TargetRows::new(&sm, target);
    let mut coverage = CoverageBuilder::new(target);
    let columns = target.columns.max(0) as usize;
    let mut buffer = vec![0f64; columns];
    let (mut coords, mut inside) = (Vec::new(), Vec::new());
    if mapper.is_some() {
        coords = vec![[0.0f64; 3]; columns];
        inside = vec![false; columns];
    }
    for_each_line(ctx, target.rows * target.slices, |line| {
        let (y, z) = (line % target.rows, line / target.rows);
        if let Some(mapper) = mapper {
            map_row(&sm, target, mapper, y, z, &mut coords, &mut inside);
            coverage.set_mask(y, z, &inside);
            if !filled {
                return;
            }
            for x in 0..columns {
                buffer[x] = 0.0;
                let [u, v, s] = coords[x];
                if inside[x] && within(u, v, s) {
                    buffer[x] = trilinear(src, &sm, bounds, u, v, s);
                }
            }
            emit(y, z, 0, &buffer);
            return;
        }
        let l = rows.line(y, z);
        let (cx0, cx1) = covered_run(&sm, &l, target.columns);
        coverage.set_row(y, z, cx0, cx1);
        if !filled || cx1 <= cx0 {
            return;
        }
        let (r0, r1) = l.column_range(target.columns, eval_lo, eval_hi);
        let (r0, r1) = (r0.max(cx0), r1.min(cx1));
        if r1 <= r0 {
            return;
        }
        let values = &mut buffer[..(r1 - r0) as usize];
        for x in r0..r1 {
            let (u, v, s) = l.at(x);
            let mut value = 0.0;
            if within(u, v, s) {
                value = trilinear(src, &sm, bounds, u, v, s);
            }
            values[(x - r0) as usize] = value;
        }
        emit(y, z, r0, values);
    })?;
    Ok(coverage.finish())
}
fn trilinear(
    src: &dyn FractionSource,
    sm: &GridMap,
    bounds: IndexBox,
    u: f64,
    v: f64,
    s: f64,
) -> f64 {
    let (mut i0, mut i1) = ([0i64; 3], [0i64; 3]);
    let mut t = [0.0f64; 3];
    for (axis, c) in [u, v, s].into_iter().enumerate() {
        let a = &sm.axes[axis];
        let ci = fmath::min(fmath::max(a.continuous(c), 0.0), (a.n - 1) as f64);
        i0[axis] = fmath::f64_to_i64(ci.floor());
        i1[axis] = i0[axis].wrapping_add(1).min(a.n - 1);
        t[axis] = ci - i0[axis] as f64;
    }
    let mut sum = 0.0;
    for corner in 0..8 {
        let mut weight = 1.0f64;
        let mut idx = [0i64; 3];
        for axis in 0..3 {
            if corner & (1 << axis) != 0 {
                idx[axis] = i1[axis];
                weight *= t[axis];
            } else {
                idx[axis] = i0[axis];
                weight *= 1.0 - t[axis];
            }
        }
        if weight != 0.0 {
            let value = if bounds.contains(idx[0], idx[1], idx[2]) {
                src.fraction_at(idx[0], idx[1], idx[2])
            } else {
                0.0
            };
            sum += weight * value;
        }
    }
    sum
}
