#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::roimask::RasterMask;
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::numtext::format_float;
use crate::render::grid::{DICOM_GRID_COORDINATE_TOLERANCE, nearest_index};
use crate::render::{GeometryTolerances, SliceGeometry, Vec3, VolumeGeometry};
pub const ERR_INVALID_GRID: &str = "roi: invalid resampling grid";
pub const ERR_INVALID_RESAMPLE: &str = "roi: invalid resampling request";
pub const ERR_RESAMPLE_TOO_LARGE: &str = "roi: resampling target exceeds the voxel budget";
pub const DEFAULT_MAX_RESAMPLE_VOXELS: i64 = 1 << 27;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Grid {
    pub columns: i64,
    pub rows: i64,
    pub slices: i64,
    pub origin: Vec3,
    pub row_dir: Vec3,
    pub col_dir: Vec3,
    pub slice_dir: Vec3,
    pub col_spacing: f64,
    pub row_spacing: f64,
    pub slice_spacing: f64,
    pub slice_offsets: Vec<f64>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexBox {
    pub min: [i64; 3],
    pub max: [i64; 3],
}
impl IndexBox {
    pub fn empty(&self) -> bool {
        self.max[0] <= self.min[0] || self.max[1] <= self.min[1] || self.max[2] <= self.min[2]
    }
    pub(crate) fn contains(&self, x: i64, y: i64, z: i64) -> bool {
        x >= self.min[0]
            && x < self.max[0]
            && y >= self.min[1]
            && y < self.max[1]
            && z >= self.min[2]
            && z < self.max[2]
    }
}
pub fn grid_from_slice(slice: &SliceGeometry, thickness: f64) -> Grid {
    let (row_dir, col_dir) = (slice.row_dir.normalize(), slice.col_dir.normalize());
    let mut normal = slice.normal.normalize();
    if normal.length() == 0.0 {
        normal = row_dir.cross(col_dir).normalize();
    }
    Grid {
        columns: slice.columns,
        rows: slice.rows,
        slices: 1,
        origin: slice.origin,
        row_dir,
        col_dir,
        slice_dir: normal,
        col_spacing: slice.col_spacing,
        row_spacing: slice.row_spacing,
        slice_spacing: thickness,
        slice_offsets: Vec::new(),
    }
}
pub fn grid_from_slices(slices: &[SliceGeometry]) -> Result<Grid, Error> {
    if slices.len() < 2 {
        return Err(Error::wrap(
            ERR_INVALID_GRID,
            format!("{} slices; use GridFromSlice for one plane", slices.len()),
        ));
    }
    let tolerance = GeometryTolerances::default_tolerances();
    let first = &slices[0];
    let mut grid = grid_from_slice(first, 0.0);
    grid.slices = slices.len() as i64;
    for (i, slice) in slices.iter().enumerate() {
        if slice.rows != first.rows
            || slice.columns != first.columns
            || !close_spacing(slice.row_spacing, first.row_spacing)
            || !close_spacing(slice.col_spacing, first.col_spacing)
        {
            return Err(Error::wrap(
                ERR_INVALID_GRID,
                format!("slice {i} size or spacing differs from slice 0"),
            ));
        }
        if slice.row_dir.normalize().dot(grid.row_dir) < tolerance.orientation_cos
            || slice.col_dir.normalize().dot(grid.col_dir) < tolerance.orientation_cos
        {
            return Err(Error::wrap(
                ERR_INVALID_GRID,
                format!("slice {i} orientation differs from slice 0"),
            ));
        }
    }
    let span = slices[slices.len() - 1].origin.sub(first.origin);
    let direction = span.normalize();
    let normal = grid.row_dir.cross(grid.col_dir).normalize();
    if span.length() == 0.0 || direction.dot(normal).abs() < 1e-3 {
        return Err(Error::wrap(
            ERR_INVALID_GRID,
            "slice origins do not advance out of the image plane",
        ));
    }
    grid.slice_dir = direction;
    let mut offsets = vec![0.0f64; slices.len()];
    let mut regular = true;
    let mean_gap = span.length() / (slices.len() - 1) as f64;
    let off_line =
        DICOM_GRID_COORDINATE_TOLERANCE * fmath::min(first.row_spacing, first.col_spacing);
    for (i, slice) in slices.iter().enumerate() {
        let delta = slice.origin.sub(first.origin);
        offsets[i] = delta.dot(direction);
        let off = delta.sub(direction.scale(offsets[i])).length();
        if off > off_line {
            return Err(Error::wrap(
                ERR_INVALID_GRID,
                format!(
                    "slice {i} origin is {} mm off the stack line",
                    format_float(off, 'g', 3)
                ),
            ));
        }
        if i > 0 {
            let gap = offsets[i] - offsets[i - 1];
            if !(gap > 1e-6) {
                return Err(Error::wrap(
                    ERR_INVALID_GRID,
                    format!("slice {i} is not after slice {} along the stack", i - 1),
                ));
            }
            if (gap - mean_gap).abs() > fmath::max(0.01, 1e-3 * mean_gap) {
                regular = false;
            }
        }
    }
    grid.slice_spacing = mean_gap;
    if !regular {
        grid.slice_offsets = offsets;
    }
    Ok(grid)
}
pub fn grid_from_volume_geometry(geometry: &VolumeGeometry) -> (Grid, Option<Error>) {
    let slices = geometry.slices();
    match slices.len() {
        0 => (
            Grid::default(),
            Some(Error::wrap(
                ERR_INVALID_GRID,
                "volume geometry has no slices",
            )),
        ),
        1 => {
            let grid = grid_from_slice(&slices[0], geometry.mean_spacing);
            let err = grid.validate().err();
            (grid, err)
        }
        _ => match grid_from_slices(slices) {
            Ok(grid) => (grid, None),
            Err(err) => (Grid::default(), Some(err)),
        },
    }
}
fn close_spacing(a: f64, b: f64) -> bool {
    (a - b).abs() <= fmath::max(1e-4, 1e-4 * b.abs())
}
impl Grid {
    pub fn validate(&self) -> Result<(), Error> {
        let g = self;
        if g.columns <= 0 || g.rows <= 0 || g.slices <= 0 {
            return Err(Error::wrap(
                ERR_INVALID_GRID,
                format!("size {}x{}x{}", g.columns, g.rows, g.slices),
            ));
        }
        let plane = g.columns.wrapping_mul(g.rows);
        if plane > i64::from(i32::MAX) || plane.wrapping_mul(g.slices) > i64::MAX / 4 {
            return Err(Error::wrap(
                ERR_INVALID_GRID,
                format!("size {}x{}x{} overflows", g.columns, g.rows, g.slices),
            ));
        }
        for value in [g.col_spacing, g.row_spacing] {
            if !(value > 0.0) || value.is_infinite() {
                return Err(Error::wrap(
                    ERR_INVALID_GRID,
                    "in-plane spacing must be positive and finite",
                ));
            }
        }
        if g.slice_offsets.is_empty() {
            if !(g.slice_spacing > 0.0) || g.slice_spacing.is_infinite() {
                return Err(Error::wrap(
                    ERR_INVALID_GRID,
                    "slice spacing (or single-slice thickness) must be positive and finite",
                ));
            }
        } else {
            if g.slice_offsets.len() as i64 != g.slices {
                return Err(Error::wrap(
                    ERR_INVALID_GRID,
                    format!(
                        "{} slice offsets for {} slices",
                        g.slice_offsets.len(),
                        g.slices
                    ),
                ));
            }
            for (i, &value) in g.slice_offsets.iter().enumerate() {
                if !value.is_finite() || i > 0 && !(value > g.slice_offsets[i - 1]) {
                    return Err(Error::wrap(
                        ERR_INVALID_GRID,
                        "slice offsets must be finite and strictly ascending",
                    ));
                }
            }
            if g.slices == 1 && !(g.slice_spacing > 0.0) {
                return Err(Error::wrap(
                    ERR_INVALID_GRID,
                    "a single slice needs a positive thickness",
                ));
            }
        }
        for v in [g.origin, g.row_dir, g.col_dir, g.slice_dir] {
            if !(v.x + v.y + v.z).is_finite() {
                return Err(Error::wrap(
                    ERR_INVALID_GRID,
                    "non-finite origin or direction",
                ));
            }
        }
        let (a, b, c) = (
            g.row_dir.normalize(),
            g.col_dir.normalize(),
            g.slice_dir.normalize(),
        );
        if a.length() == 0.0
            || b.length() == 0.0
            || c.length() == 0.0
            || a.cross(b).dot(c).abs() < 1e-6
        {
            return Err(Error::wrap(
                ERR_INVALID_GRID,
                "directions are zero or coplanar",
            ));
        }
        Ok(())
    }
    pub fn voxel_count(&self) -> i64 {
        self.columns
            .wrapping_mul(self.rows)
            .wrapping_mul(self.slices)
    }
    pub fn slice_offset(&self, z: i64) -> f64 {
        if !self.slice_offsets.is_empty() {
            return self.slice_offsets[z as usize];
        }
        z as f64 * self.slice_spacing
    }
    pub fn voxel_center(&self, x: i64, y: i64, z: i64) -> Vec3 {
        self.origin
            .add(self.row_dir.normalize().scale(x as f64 * self.col_spacing))
            .add(self.col_dir.normalize().scale(y as f64 * self.row_spacing))
            .add(self.slice_dir.normalize().scale(self.slice_offset(z)))
    }
    pub fn slice_geometry(&self, z: i64) -> SliceGeometry {
        let (row_dir, col_dir) = (self.row_dir.normalize(), self.col_dir.normalize());
        SliceGeometry {
            origin: self.voxel_center(0, 0, z),
            row_dir,
            col_dir,
            normal: row_dir.cross(col_dir).normalize(),
            row_spacing: self.row_spacing,
            col_spacing: self.col_spacing,
            rows: self.rows,
            columns: self.columns,
        }
    }
    pub fn continuous_index(&self, p: Vec3) -> (f64, f64, f64, bool) {
        let Ok(m) = GridMap::new(self) else {
            return (0.0, 0.0, 0.0, false);
        };
        let (u, v, s) = m.coords(p);
        (
            u / self.col_spacing,
            v / self.row_spacing,
            m.axes[2].continuous(s),
            m.inside(u, v, s),
        )
    }
    #[allow(dead_code)]
    pub(crate) fn crop(&self, b: IndexBox) -> Grid {
        let mut out = self.clone();
        out.origin = self.voxel_center(b.min[0], b.min[1], b.min[2]);
        out.columns = b.max[0] - b.min[0];
        out.rows = b.max[1] - b.min[1];
        out.slices = b.max[2] - b.min[2];
        if !self.slice_offsets.is_empty() {
            let base = self.slice_offsets[b.min[2] as usize];
            out.slice_offsets = (0..out.slices.max(0))
                .map(|i| self.slice_offsets[(b.min[2] + i) as usize] - base)
                .collect();
        }
        out
    }
}
#[derive(Debug, Clone, Default)]
pub(crate) struct GridAxis {
    pub(crate) n: i64,
    pub(crate) spacing: f64,
    pub(crate) offsets: Vec<f64>,
}
impl GridAxis {
    fn irregular(&self) -> bool {
        !self.offsets.is_empty()
    }
    fn low_gap(&self) -> f64 {
        if self.irregular() && self.n > 1 {
            return self.offsets[1] - self.offsets[0];
        }
        self.spacing
    }
    fn high_gap(&self) -> f64 {
        if self.irregular() && self.n > 1 {
            let n = self.n as usize;
            return self.offsets[n - 1] - self.offsets[n - 2];
        }
        self.spacing
    }
    pub(crate) fn edges(&self, i: i64) -> (f64, f64) {
        if !self.irregular() {
            return (
                (i as f64 - 0.5) * self.spacing,
                (i as f64 + 0.5) * self.spacing,
            );
        }
        let at = i as usize;
        let mut lo = self.offsets[at] - self.low_gap() / 2.0;
        if i > 0 {
            lo = (self.offsets[at - 1] + self.offsets[at]) / 2.0;
        }
        let mut hi = self.offsets[at] + self.high_gap() / 2.0;
        if i + 1 < self.n {
            hi = (self.offsets[at] + self.offsets[at + 1]) / 2.0;
        }
        (lo, hi)
    }
    pub(crate) fn extent(&self, i0: i64, i1: i64) -> (f64, f64) {
        let (lo, _) = self.edges(i0);
        let (_, hi) = self.edges(i1 - 1);
        (lo, hi)
    }
    pub(crate) fn continuous(&self, c: f64) -> f64 {
        if !self.irregular() {
            return c / self.spacing;
        }
        let n = self.n as usize;
        let (first, last) = (self.offsets[0], self.offsets[n - 1]);
        if self.n == 1 {
            return (c - first) / self.spacing;
        }
        if c <= first {
            return (c - first) / self.low_gap();
        }
        if c >= last {
            return (self.n - 1) as f64 + (c - last) / self.high_gap();
        }
        let (mut lo, mut hi) = (0usize, n - 1);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self.offsets[mid] <= c {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo as f64 + (c - self.offsets[lo]) / (self.offsets[hi] - self.offsets[lo])
    }
    pub(crate) fn nearest(&self, c: f64) -> i64 {
        nearest_index(self.continuous(c)).max(0).min(self.n - 1)
    }
}
#[derive(Debug, Clone)]
pub(crate) struct GridMap {
    pub(crate) grid: Grid,
    origin: Vec3,
    inv: [[f64; 3]; 3],
    pub(crate) axes: [GridAxis; 3],
    pub(crate) lo: [f64; 3],
    pub(crate) hi: [f64; 3],
}
impl GridMap {
    pub(crate) fn new(g: &Grid) -> Result<GridMap, Error> {
        g.validate()?;
        let (a, b, c) = (
            g.row_dir.normalize(),
            g.col_dir.normalize(),
            g.slice_dir.normalize(),
        );
        let m = [[a.x, b.x, c.x], [a.y, b.y, c.y], [a.z, b.z, c.z]];
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if det.abs() < 1e-6 {
            return Err(Error::wrap(ERR_INVALID_GRID, "degenerate directions"));
        }
        let mut inv = [[0.0f64; 3]; 3];
        inv[0][0] = (m[1][1] * m[2][2] - m[1][2] * m[2][1]) / det;
        inv[0][1] = (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / det;
        inv[0][2] = (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / det;
        inv[1][0] = (m[1][2] * m[2][0] - m[1][0] * m[2][2]) / det;
        inv[1][1] = (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / det;
        inv[1][2] = (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / det;
        inv[2][0] = (m[1][0] * m[2][1] - m[1][1] * m[2][0]) / det;
        inv[2][1] = (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / det;
        inv[2][2] = (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / det;
        let mut axes = [
            GridAxis {
                n: g.columns,
                spacing: g.col_spacing,
                offsets: Vec::new(),
            },
            GridAxis {
                n: g.rows,
                spacing: g.row_spacing,
                offsets: Vec::new(),
            },
            GridAxis {
                n: g.slices,
                spacing: g.slice_spacing,
                offsets: Vec::new(),
            },
        ];
        if !g.slice_offsets.is_empty() {
            axes[2].offsets = g.slice_offsets.clone();
            if g.slices > 1 {
                let n = g.slices as usize;
                axes[2].spacing =
                    (g.slice_offsets[n - 1] - g.slice_offsets[0]) / (g.slices - 1) as f64;
            }
        }
        let (mut lo, mut hi) = ([0.0f64; 3], [0.0f64; 3]);
        for i in 0..3 {
            (lo[i], hi[i]) = axes[i].extent(0, axes[i].n);
        }
        Ok(GridMap {
            grid: g.clone(),
            origin: g.origin,
            inv,
            axes,
            lo,
            hi,
        })
    }
    pub(crate) fn vector(&self, d: Vec3) -> (f64, f64, f64) {
        let inv = &self.inv;
        (
            inv[0][0] * d.x + inv[0][1] * d.y + inv[0][2] * d.z,
            inv[1][0] * d.x + inv[1][1] * d.y + inv[1][2] * d.z,
            inv[2][0] * d.x + inv[2][1] * d.y + inv[2][2] * d.z,
        )
    }
    pub(crate) fn coords(&self, p: Vec3) -> (f64, f64, f64) {
        self.vector(p.sub(self.origin))
    }
    pub(crate) fn inside(&self, u: f64, v: f64, s: f64) -> bool {
        u >= self.lo[0]
            && u < self.hi[0]
            && v >= self.lo[1]
            && v < self.hi[1]
            && s >= self.lo[2]
            && s < self.hi[2]
    }
    pub(crate) fn box_extent(&self, b: IndexBox) -> ([f64; 3], [f64; 3]) {
        let (mut lo, mut hi) = ([0.0f64; 3], [0.0f64; 3]);
        for i in 0..3 {
            (lo[i], hi[i]) = self.axes[i].extent(b.min[i], b.max[i]);
        }
        (lo, hi)
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RowLine {
    base: [f64; 3],
    step: [f64; 3],
}
impl RowLine {
    pub(crate) fn at(&self, x: i64) -> (f64, f64, f64) {
        let fx = x as f64;
        (
            self.base[0] + fx * self.step[0],
            self.base[1] + fx * self.step[1],
            self.base[2] + fx * self.step[2],
        )
    }
    pub(crate) fn column_range(&self, columns: i64, lo: [f64; 3], hi: [f64; 3]) -> (i64, i64) {
        let (mut start, mut end) = (0.0f64, columns as f64);
        for i in 0..3 {
            let (b, d) = (self.base[i], self.step[i]);
            if d.abs() < 1e-12 {
                if b < lo[i] || b >= hi[i] {
                    return (0, 0);
                }
                continue;
            }
            let (mut t0, mut t1) = ((lo[i] - b) / d, (hi[i] - b) / d);
            if t0 > t1 {
                (t0, t1) = (t1, t0);
            }
            start = fmath::max(start, t0);
            end = fmath::min(end, t1);
        }
        if !(end >= start) {
            return (0, 0);
        }
        let x0 = fmath::f64_to_i64(start.floor()).wrapping_sub(1).max(0);
        let x1 = fmath::f64_to_i64(end.ceil()).wrapping_add(1).min(columns);
        if x1 < x0 {
            return (0, 0);
        }
        (x0, x1)
    }
}
#[derive(Debug, Clone)]
pub(crate) struct TargetRows {
    row_dx: [f64; 3],
    row_dy: [f64; 3],
    slices: Vec<[f64; 3]>,
}
impl TargetRows {
    pub(crate) fn new(src: &GridMap, target: &Grid) -> TargetRows {
        let dx = src.vector(target.row_dir.normalize().scale(target.col_spacing));
        let dy = src.vector(target.col_dir.normalize().scale(target.row_spacing));
        let slices = (0..target.slices.max(0))
            .map(|z| {
                let c = src.coords(target.voxel_center(0, 0, z));
                [c.0, c.1, c.2]
            })
            .collect();
        TargetRows {
            row_dx: [dx.0, dx.1, dx.2],
            row_dy: [dy.0, dy.1, dy.2],
            slices,
        }
    }
    pub(crate) fn line(&self, y: i64, z: i64) -> RowLine {
        let mut out = RowLine::default();
        let fy = y as f64;
        for i in 0..3 {
            out.base[i] = self.slices[z as usize][i] + fy * self.row_dy[i];
        }
        out.step = self.row_dx;
        out
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResampleCoverage {
    pub columns: i64,
    pub rows: i64,
    pub slices: i64,
    pub masks: Vec<Option<RasterMask>>,
    pub slice_pixels: Vec<i64>,
    pub covered_voxels: i64,
}
impl ResampleCoverage {
    pub fn total_voxels(&self) -> i64 {
        self.columns
            .wrapping_mul(self.rows)
            .wrapping_mul(self.slices)
    }
    pub fn full(&self) -> bool {
        self.total_voxels() > 0 && self.covered_voxels == self.total_voxels()
    }
    pub fn partial(&self) -> bool {
        self.covered_voxels > 0 && self.covered_voxels < self.total_voxels()
    }
    pub fn fraction(&self) -> f64 {
        if self.total_voxels() == 0 {
            return 0.0;
        }
        self.covered_voxels as f64 / self.total_voxels() as f64
    }
    pub fn covered_slices(&self) -> Vec<i64> {
        self.slice_pixels
            .iter()
            .enumerate()
            .filter(|&(_, &n)| n > 0)
            .map(|(z, _)| z as i64)
            .collect()
    }
    pub fn slice_fraction(&self, z: i64) -> f64 {
        let plane = self.columns.wrapping_mul(self.rows);
        if z < 0 || z >= self.slice_pixels.len() as i64 || plane == 0 {
            return 0.0;
        }
        self.slice_pixels[z as usize] as f64 / plane as f64
    }
    pub fn covered(&self, x: i64, y: i64, z: i64) -> bool {
        if z < 0 || z >= self.masks.len() as i64 {
            return false;
        }
        self.masks[z as usize]
            .as_ref()
            .is_some_and(|mask| mask.get(x, y))
    }
}
pub(crate) fn for_each_line(
    ctx: Option<&dyn Cancellation>,
    lines: i64,
    mut line: impl FnMut(i64),
) -> Result<(), Error> {
    for i in 0..lines {
        if ctx_err(ctx).is_some() {
            break;
        }
        line(i);
    }
    match ctx_err(ctx) {
        Some(err) => Err(err),
        None => Ok(()),
    }
}
pub fn grid_coverage(
    ctx: Option<&dyn Cancellation>,
    source: &Grid,
    target: &Grid,
) -> Result<ResampleCoverage, Error> {
    let src = GridMap::new(source)?;
    target.validate()?;
    let rows = TargetRows::new(&src, target);
    let mut coverage = CoverageBuilder::new(target);
    for_each_line(ctx, target.rows * target.slices, |line| {
        let (y, z) = (line % target.rows, line / target.rows);
        let (x0, x1) = covered_run(&src, &rows.line(y, z), target.columns);
        coverage.set_row(y, z, x0, x1);
    })?;
    Ok(coverage.finish())
}
pub(crate) fn covered_run(src: &GridMap, line: &RowLine, columns: i64) -> (i64, i64) {
    let (mut x0, mut x1) = line.column_range(columns, src.lo, src.hi);
    let inside = |x: i64| {
        let (u, v, s) = line.at(x);
        src.inside(u, v, s)
    };
    while x0 < x1 && !inside(x0) {
        x0 += 1;
    }
    while x1 > x0 && !inside(x1 - 1) {
        x1 -= 1;
    }
    if x1 <= x0 {
        return (0, 0);
    }
    (x0, x1)
}
pub type PointMapper<'a> = &'a (dyn Fn(Vec3) -> Option<Vec3> + Sync);
pub(crate) fn map_row(
    src: &GridMap,
    target: &Grid,
    mapper: PointMapper<'_>,
    y: i64,
    z: i64,
    coords: &mut [[f64; 3]],
    inside: &mut [bool],
) {
    for x in 0..target.columns {
        let at = x as usize;
        inside[at] = false;
        let Some(q) = mapper(target.voxel_center(x, y, z)) else {
            continue;
        };
        let (u, v, s) = src.coords(q);
        coords[at] = [u, v, s];
        inside[at] = src.inside(u, v, s);
    }
}
pub(crate) struct CoverageBuilder {
    coverage: ResampleCoverage,
    row_counts: Vec<i32>,
}
impl CoverageBuilder {
    pub(crate) fn new(target: &Grid) -> CoverageBuilder {
        let slices = target.slices.max(0) as usize;
        CoverageBuilder {
            coverage: ResampleCoverage {
                columns: target.columns,
                rows: target.rows,
                slices: target.slices,
                masks: (0..slices)
                    .map(|_| Some(RasterMask::new(target.columns, target.rows)))
                    .collect(),
                slice_pixels: vec![0; slices],
                covered_voxels: 0,
            },
            row_counts: vec![0; (target.rows * target.slices).max(0) as usize],
        }
    }
    fn mask(&mut self, z: i64) -> &mut RasterMask {
        self.coverage.masks[z as usize]
            .as_mut()
            .expect("coverage masks exist until finish")
    }
    pub(crate) fn set_row(&mut self, y: i64, z: i64, x0: i64, x1: i64) {
        if x1 > x0 {
            self.mask(z).set_run(y, x0, x1);
            let at = (z * self.coverage.rows + y) as usize;
            self.row_counts[at] = (x1 - x0) as i32;
        }
    }
    pub(crate) fn set_mask(&mut self, y: i64, z: i64, inside: &[bool]) {
        let mut n = 0usize;
        let mut x = 0usize;
        while x < inside.len() {
            if !inside[x] {
                x += 1;
                continue;
            }
            let mut end = x + 1;
            while end < inside.len() && inside[end] {
                end += 1;
            }
            self.mask(z).set_run(y, x as i64, end as i64);
            n += end - x;
            x = end;
        }
        let at = (z * self.coverage.rows + y) as usize;
        self.row_counts[at] = n as i32;
    }
    pub(crate) fn finish(mut self) -> ResampleCoverage {
        let rows = self.coverage.rows;
        for z in 0..self.coverage.slices {
            let mut n = 0i64;
            for y in 0..rows {
                n += i64::from(self.row_counts[(z * rows + y) as usize]);
            }
            self.coverage.slice_pixels[z as usize] = n;
            self.coverage.covered_voxels += n;
            if n == 0 {
                self.coverage.masks[z as usize] = None;
            }
        }
        self.coverage
    }
}
