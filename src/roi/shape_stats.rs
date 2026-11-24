#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::distance3d::VolumeMask;
use super::resample_labels::LabelVolume;
use super::roimask::{MaskRun, RasterMask};
use super::segment_stats::{slice_in_plane_spacing, slice_thickness};
use super::segmentation::Segmentation3D;
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::numeric::linalg::symmetric_eigen;
use crate::render::Vec3;
use std::collections::BTreeMap;
pub const ERR_INVALID_SHAPE_INPUT: &str = "roi: invalid shape statistics input";
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ShapeOptions {
    pub origin: Vec3,
    pub direction: [Vec3; 3],
    pub planar: bool,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OrientedBoundingBox {
    pub origin: Vec3,
    pub axes: Vec<Vec3>,
    pub size: Vec<f64>,
}
impl OrientedBoundingBox {
    pub fn vertices(&self) -> Vec<Vec3> {
        let n = self.axes.len();
        if n == 0 || self.size.len() != n {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(1usize << n);
        for v in 0..(1usize << n) {
            let mut p = self.origin;
            for k in 0..n {
                if v & (1 << k) != 0 {
                    p = p.add(self.axes[k].scale(self.size[k]));
                }
            }
            out.push(p);
        }
        out
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapeStatistics {
    pub label: u16,
    pub dimension: i64,
    pub voxel_count: i64,
    pub physical_size: f64,
    pub bounding_box_min: [i64; 3],
    pub bounding_box_max: [i64; 3],
    pub centroid: Vec3,
    pub principal_moments: Vec<f64>,
    pub principal_axes: Vec<Vec3>,
    pub elongation: f64,
    pub flatness: f64,
    pub equivalent_spherical_radius: f64,
    pub equivalent_spherical_perimeter: f64,
    pub equivalent_ellipsoid_diameter: Vec<f64>,
    pub feret_diameter: f64,
    pub feret_endpoints: [Vec3; 2],
    pub perimeter: f64,
    pub sphericity: f64,
    pub voxels_on_border: i64,
    pub perimeter_on_border: f64,
    pub perimeter_on_border_ratio: f64,
    pub oriented_bounding_box: OrientedBoundingBox,
}
impl ShapeStatistics {
    pub fn touches_border(&self) -> bool {
        self.voxels_on_border > 0
    }
}
impl VolumeMask {
    pub fn shape_statistics(&self, opts: ShapeOptions) -> Result<ShapeStatistics, Error> {
        self.validate()?;
        let g = ShapeGrid::new(
            opts.planar,
            [self.columns, self.rows, self.slices],
            [self.x.clone(), self.y.clone(), self.z.clone()],
            [1.0, 1.0, 1.0],
            opts,
        )?;
        let mut runs: Vec<ShapeRun> = Vec::new();
        let nx = self.columns;
        for z in 0..self.slices {
            for y in 0..self.rows {
                let start = self.index(0, y, z);
                let row = &self.voxels[start..start + nx as usize];
                append_row_runs(&mut runs, y, z, row);
            }
        }
        Ok(g.statistics(&runs))
    }
}
impl RasterMask {
    pub fn shape_statistics(
        &self,
        column_spacing: f64,
        row_spacing: f64,
        opts: ShapeOptions,
    ) -> Result<ShapeStatistics, Error> {
        if self.columns <= 0
            || self.rows <= 0
            || !(column_spacing > 0.0)
            || !(row_spacing > 0.0)
            || column_spacing.is_infinite()
            || row_spacing.is_infinite()
        {
            return Err(Error::wrap(ERR_INVALID_SHAPE_INPUT, "mask size or spacing"));
        }
        let g = ShapeGrid::new(
            true,
            [self.columns, self.rows, 1],
            [
                uniform_axis(self.columns, column_spacing),
                uniform_axis(self.rows, row_spacing),
                vec![0.0],
            ],
            [column_spacing, row_spacing, 1.0],
            opts,
        )?;
        let mut runs: Vec<ShapeRun> = Vec::new();
        self.for_each_run(|y, run| {
            append_clipped_run(&mut runs, y, 0, run, self.columns);
        });
        Ok(g.statistics(&runs))
    }
}
impl Segmentation3D {
    pub fn shape_statistics(&self) -> Result<ShapeStatistics, Error> {
        if self.columns <= 0 || self.rows <= 0 {
            return Err(Error::wrap(ERR_INVALID_SHAPE_INPUT, "segmentation size"));
        }
        let g = segmentation_shape_grid(self)?;
        let mut runs: Vec<ShapeRun> = Vec::new();
        for z in self.slices() {
            if z >= g.size[2] {
                continue;
            }
            self.masks[&z].for_each_run(|y, run| {
                if y < self.rows {
                    append_clipped_run(&mut runs, y, z, run, self.columns);
                }
            });
        }
        Ok(g.statistics(&runs))
    }
}
fn label_shape_grid(v: Option<&LabelVolume>, planar: bool) -> Result<ShapeGrid, Error> {
    let Some(v) = v else {
        return Err(Error::wrap(ERR_INVALID_SHAPE_INPUT, "nil label volume"));
    };
    let grid = &v.grid;
    if let Err(err) = grid.validate() {
        return Err(Error::wrap_err(ERR_INVALID_SHAPE_INPUT, &err));
    }
    if v.labels.len() as i64 != grid.voxel_count() {
        return Err(Error::wrap(
            ERR_INVALID_SHAPE_INPUT,
            format!(
                "{} labels for {} voxels",
                v.labels.len(),
                grid.voxel_count()
            ),
        ));
    }
    let z: Vec<f64> = (0..grid.slices).map(|i| grid.slice_offset(i)).collect();
    let mut thickness = grid.slice_spacing;
    if !(thickness > 0.0) {
        thickness = 1.0;
    }
    let opts = ShapeOptions {
        origin: grid.origin,
        direction: [
            grid.row_dir.normalize(),
            grid.col_dir.normalize(),
            grid.slice_dir.normalize(),
        ],
        planar: false,
    };
    ShapeGrid::new(
        planar,
        [grid.columns, grid.rows, grid.slices],
        [
            uniform_axis(grid.columns, grid.col_spacing),
            uniform_axis(grid.rows, grid.row_spacing),
            z,
        ],
        [grid.col_spacing, grid.row_spacing, thickness],
        opts,
    )
}
fn label_runs(
    ctx: Option<&dyn Cancellation>,
    v: &LabelVolume,
) -> Result<BTreeMap<u16, Vec<ShapeRun>>, Error> {
    let mut out: BTreeMap<u16, Vec<ShapeRun>> = BTreeMap::new();
    let (nx, ny) = (v.grid.columns, v.grid.rows);
    for z in 0..v.grid.slices {
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        for y in 0..ny {
            let start = ((z * ny + y) * nx) as usize;
            let row = &v.labels[start..start + nx as usize];
            let mut x = 0usize;
            while x < row.len() {
                let label = row[x];
                let mut end = x + 1;
                while end < row.len() && row[end] == label {
                    end += 1;
                }
                if label != 0 {
                    out.entry(label).or_default().push(ShapeRun {
                        y,
                        z,
                        x0: x as i64,
                        x1: end as i64 - 1,
                    });
                }
                x = end;
            }
        }
    }
    Ok(out)
}
pub fn label_shape_statistics(
    ctx: Option<&dyn Cancellation>,
    v: Option<&LabelVolume>,
    opts: ShapeOptions,
) -> Result<Vec<ShapeStatistics>, Error> {
    let g = label_shape_grid(v, opts.planar)?;
    let Some(v) = v else {
        return Err(Error::wrap(ERR_INVALID_SHAPE_INPUT, "nil label volume"));
    };
    let by_label = label_runs(ctx, v)?;
    let mut out = Vec::with_capacity(by_label.len());
    for (label, runs) in &by_label {
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        let mut stats = g.statistics(runs);
        stats.label = *label;
        out.push(stats);
    }
    Ok(out)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShapeRun {
    pub(crate) y: i64,
    pub(crate) z: i64,
    pub(crate) x0: i64,
    pub(crate) x1: i64,
}
fn append_row_runs(runs: &mut Vec<ShapeRun>, y: i64, z: i64, row: &[bool]) {
    let n = row.len();
    let mut x = 0usize;
    while x < n {
        if !row[x] {
            x += 1;
            continue;
        }
        let start = x;
        while x + 1 < n && row[x + 1] {
            x += 1;
        }
        runs.push(ShapeRun {
            y,
            z,
            x0: start as i64,
            x1: x as i64,
        });
        x += 1;
    }
}
fn append_clipped_run(runs: &mut Vec<ShapeRun>, y: i64, z: i64, run: MaskRun, columns: i64) {
    let (x0, x1) = (run.start.max(0), run.end.min(columns) - 1);
    if x1 < x0 {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.y == y
        && last.z == z
        && last.x1 >= x0 - 1
    {
        last.x1 = last.x1.max(x1);
        return;
    }
    runs.push(ShapeRun { y, z, x0, x1 });
}
fn uniform_axis(n: i64, step: f64) -> Vec<f64> {
    (0..n.max(0)).map(|i| i as f64 * step).collect()
}
#[derive(Debug, Clone)]
pub(crate) struct ShapeGrid {
    pub(crate) dim: usize,
    pub(crate) size: [i64; 3],
    pub(crate) coord: [Vec<f64>; 3],
    pub(crate) bound: [Vec<f64>; 3],
    pub(crate) space: [f64; 3],
    pub(crate) origin: Vec3,
    pub(crate) dir: [Vec3; 3],
}
impl ShapeGrid {
    fn new(
        planar: bool,
        size: [i64; 3],
        coords: [Vec<f64>; 3],
        mut single: [f64; 3],
        opts: ShapeOptions,
    ) -> Result<ShapeGrid, Error> {
        let mut g = ShapeGrid {
            dim: 3,
            size,
            coord: coords,
            bound: [Vec::new(), Vec::new(), Vec::new()],
            space: [0.0; 3],
            origin: opts.origin,
            dir: opts.direction,
        };
        if planar {
            if size[2] != 1 {
                return Err(Error::wrap(
                    ERR_INVALID_SHAPE_INPUT,
                    "planar statistics need exactly one slice",
                ));
            }
            g.dim = 2;
            g.coord[2] = vec![0.0];
            single[2] = 1.0;
        }
        for k in 0..3 {
            let c = &g.coord[k];
            if size[k] <= 0 || c.len() as i64 != size[k] {
                return Err(Error::wrap(
                    ERR_INVALID_SHAPE_INPUT,
                    format!(
                        "axis {} has {} coordinates for {} samples",
                        k,
                        c.len(),
                        size[k]
                    ),
                ));
            }
            for (i, &v) in c.iter().enumerate() {
                if v.is_nan() || v.is_infinite() || i > 0 && v <= c[i - 1] {
                    return Err(Error::wrap(
                        ERR_INVALID_SHAPE_INPUT,
                        format!("axis {k} coordinates must be finite and ascending"),
                    ));
                }
            }
            let bound = cell_boundaries(c, single[k]);
            let mut space = bound[1] - bound[0];
            let n = c.len();
            if n > 1 {
                space = (c[n - 1] - c[0]) / (n - 1) as f64;
            }
            g.bound[k] = bound;
            g.space[k] = space;
        }
        let used = &g.dir[..g.dim];
        let all_zero = used.iter().all(|d| *d == Vec3::default());
        if all_zero {
            g.dir = [
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ];
        } else {
            for (i, d) in used.iter().enumerate() {
                if (d.length() - 1.0).abs() > 1e-6 {
                    return Err(Error::wrap(
                        ERR_INVALID_SHAPE_INPUT,
                        format!("direction {i} is not a unit vector"),
                    ));
                }
                for (j, other) in used.iter().enumerate().take(i) {
                    if d.dot(*other).abs() > 1e-6 {
                        return Err(Error::wrap(
                            ERR_INVALID_SHAPE_INPUT,
                            format!("directions {j} and {i} are not orthogonal"),
                        ));
                    }
                }
            }
        }
        if !finite_vec(g.origin) {
            return Err(Error::wrap(ERR_INVALID_SHAPE_INPUT, "origin is not finite"));
        }
        Ok(g)
    }
    pub(crate) fn to_patient(&self, p: [f64; 3]) -> Vec3 {
        self.origin.add(self.to_patient_vector(p))
    }
    pub(crate) fn to_patient_vector(&self, v: [f64; 3]) -> Vec3 {
        let mut out = Vec3::default();
        for (dir, c) in self.dir.iter().zip(v).take(self.dim) {
            out = out.add(dir.scale(c));
        }
        out
    }
    fn width(&self, k: usize, i: i64) -> f64 {
        self.bound[k][i as usize + 1] - self.bound[k][i as usize]
    }
    pub(crate) fn statistics(&self, runs: &[ShapeRun]) -> ShapeStatistics {
        let g = self;
        let mut out = ShapeStatistics {
            dimension: g.dim as i64,
            ..ShapeStatistics::default()
        };
        if runs.is_empty() {
            return out;
        }
        let d = g.dim;
        let (cx, cy, cz) = (&g.coord[0], &g.coord[1], &g.coord[2]);
        let reference = [
            cx[runs[0].x0 as usize],
            cy[runs[0].y as usize],
            cz[runs[0].z as usize],
        ];
        let mut lo = [runs[0].x0, runs[0].y, runs[0].z];
        let mut hi = lo;
        let mut weight = 0.0f64;
        let mut first = [0.0f64; 3];
        let last = [g.size[0] - 1, g.size[1] - 1, g.size[2] - 1];
        for r in runs {
            let mut wyz = g.width(1, r.y);
            if d == 3 {
                wyz *= g.width(2, r.z);
            }
            let (dy, dz) = (
                cy[r.y as usize] - reference[1],
                cz[r.z as usize] - reference[2],
            );
            for x in r.x0..=r.x1 {
                let w = g.width(0, x) * wyz;
                weight += w;
                first[0] += w * (cx[x as usize] - reference[0]);
                first[1] += w * dy;
                first[2] += w * dz;
            }
            out.voxel_count += r.x1 - r.x0 + 1;
            lo[0] = lo[0].min(r.x0);
            hi[0] = hi[0].max(r.x1);
            lo[1] = lo[1].min(r.y);
            hi[1] = hi[1].max(r.y);
            lo[2] = lo[2].min(r.z);
            hi[2] = hi[2].max(r.z);
            g.border_contact(&mut out, *r, last);
        }
        out.physical_size = weight;
        out.bounding_box_min = lo;
        out.bounding_box_max = hi;
        let mut mean = [0.0f64; 3];
        for k in 0..3 {
            mean[k] = reference[k] + first[k] / weight;
        }
        if d == 2 {
            mean[2] = 0.0;
        }
        out.centroid = g.to_patient(mean);
        let mut cov = [[0.0f64; 3]; 3];
        for r in runs {
            let mut wyz = g.width(1, r.y);
            if d == 3 {
                wyz *= g.width(2, r.z);
            }
            let dy = cy[r.y as usize] - mean[1];
            let mut dz = cz[r.z as usize] - mean[2];
            if d == 2 {
                dz = 0.0;
            }
            for x in r.x0..=r.x1 {
                let w = g.width(0, x) * wyz;
                let dx = cx[x as usize] - mean[0];
                cov[0][0] += w * dx * dx;
                cov[0][1] += w * dx * dy;
                cov[0][2] += w * dx * dz;
                cov[1][1] += w * dy * dy;
                cov[1][2] += w * dy * dz;
                cov[2][2] += w * dz * dz;
            }
        }
        let mut matrix = vec![vec![0.0f64; d]; d];
        for (i, row) in matrix.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                let (a, b) = (i.min(j), i.max(j));
                *cell = cov[a][b] / weight;
            }
        }
        let (mut moments, vectors) = match symmetric_eigen(&matrix) {
            Ok(r) => r,
            Err(_) => {
                let mut vectors = vec![vec![0.0f64; d]; d];
                for (i, row) in vectors.iter_mut().enumerate() {
                    row[i] = 1.0;
                }
                (vec![0.0f64; d], vectors)
            }
        };
        let top = fmath::max(moments[d - 1], 0.0);
        for m in moments.iter_mut() {
            if *m <= 1e-12 * top {
                *m = 0.0;
            }
        }
        let axes = g.principal_axes(&vectors);
        out.principal_axes = axes.iter().map(|a| g.to_patient_vector(*a)).collect();
        if moments[0] > 0.0 {
            out.flatness = (moments[1] / moments[0]).sqrt();
        }
        if moments[d - 2] > 0.0 {
            out.elongation = (moments[d - 1] / moments[d - 2]).sqrt();
        }
        const PI: f64 = std::f64::consts::PI;
        if d == 3 {
            out.equivalent_spherical_radius = fmath::cbrt(3.0 * weight / (4.0 * PI));
            out.equivalent_spherical_perimeter =
                4.0 * PI * out.equivalent_spherical_radius * out.equivalent_spherical_radius;
        } else {
            out.equivalent_spherical_radius = (weight / PI).sqrt();
            out.equivalent_spherical_perimeter = 2.0 * PI * out.equivalent_spherical_radius;
        }
        out.equivalent_ellipsoid_diameter = vec![0.0; d];
        let mut det = 1.0f64;
        for &m in &moments {
            det *= m;
        }
        if det > 0.0 {
            det = fmath::pow(det, 1.0 / d as f64);
            for (i, &m) in moments.iter().enumerate() {
                out.equivalent_ellipsoid_diameter[i] =
                    2.0 * out.equivalent_spherical_radius * (m / det).sqrt();
            }
        }
        out.principal_moments = moments;
        let (feret, a, b) = g.feret_diameter(runs, lo, hi);
        out.feret_diameter = feret;
        out.feret_endpoints = [g.to_patient(a), g.to_patient(b)];
        out.perimeter = g.crofton_perimeter(runs, lo, hi);
        if out.perimeter > 0.0 {
            out.sphericity = out.equivalent_spherical_perimeter / out.perimeter;
            out.perimeter_on_border_ratio = out.perimeter_on_border / out.perimeter;
        }
        out.oriented_bounding_box =
            g.oriented_bounding_box(runs, mean, &axes, out.centroid, &out.principal_axes);
        out
    }
    fn border_contact(&self, out: &mut ShapeStatistics, r: ShapeRun, last: [i64; 3]) {
        let g = self;
        let n = r.x1 - r.x0 + 1;
        let (wy, mut wz) = (g.width(1, r.y), 1.0);
        if g.dim == 3 {
            wz = g.width(2, r.z);
        }
        let on_row_border =
            r.y == 0 || r.y == last[1] || g.dim == 3 && (r.z == 0 || r.z == last[2]);
        if on_row_border {
            out.voxels_on_border += n;
        } else {
            if r.x0 == 0 {
                out.voxels_on_border += 1;
            }
            if r.x1 == last[0] && (r.x0 != 0 || n > 1) {
                out.voxels_on_border += 1;
            }
        }
        if r.x0 == 0 {
            out.perimeter_on_border += wy * wz;
        }
        if r.x1 == last[0] {
            out.perimeter_on_border += wy * wz;
        }
        let run_width = g.bound[0][r.x1 as usize + 1] - g.bound[0][r.x0 as usize];
        for edge in [r.y == 0, r.y == last[1]] {
            if edge {
                out.perimeter_on_border += run_width * wz;
            }
        }
        if g.dim == 3 {
            for edge in [r.z == 0, r.z == last[2]] {
                if edge {
                    out.perimeter_on_border += run_width * wy;
                }
            }
        }
    }
    fn principal_axes(&self, vectors: &[Vec<f64>]) -> Vec<[f64; 3]> {
        let g = self;
        let d = g.dim;
        let mut axes = vec![[0.0f64; 3]; d];
        for k in 0..d {
            for i in 0..d {
                axes[k][i] = vectors[i][k];
            }
            let p = g.to_patient_vector(axes[k]);
            let comps = [p.x, p.y, p.z];
            let mut best = 0.0f64;
            for c in comps {
                best = fmath::max(best, c.abs());
            }
            for c in comps {
                if c.abs() >= best - 1e-9 {
                    if c < 0.0 {
                        for v in axes[k].iter_mut() {
                            *v = -*v;
                        }
                    }
                    break;
                }
            }
        }
        let det = if d == 3 {
            let (a, b, c) = (
                g.to_patient_vector(axes[0]),
                g.to_patient_vector(axes[1]),
                g.to_patient_vector(axes[2]),
            );
            a.cross(b).dot(c)
        } else {
            axes[0][0] * axes[1][1] - axes[0][1] * axes[1][0]
        };
        if det < 0.0 {
            for v in axes[d - 1].iter_mut() {
                *v = -*v;
            }
        }
        axes
    }
    fn oriented_bounding_box(
        &self,
        runs: &[ShapeRun],
        mean: [f64; 3],
        axes: &[[f64; 3]],
        centroid: Vec3,
        patient_axes: &[Vec3],
    ) -> OrientedBoundingBox {
        let g = self;
        let d = g.dim;
        let mut lo = vec![f64::INFINITY; d];
        let mut hi = vec![f64::NEG_INFINITY; d];
        for r in runs {
            let xs = [
                g.bound[0][r.x0 as usize] - mean[0],
                g.bound[0][r.x1 as usize + 1] - mean[0],
            ];
            let ys = [
                g.bound[1][r.y as usize] - mean[1],
                g.bound[1][r.y as usize + 1] - mean[1],
            ];
            let mut zs = [0.0, 0.0];
            if d == 3 {
                zs = [
                    g.bound[2][r.z as usize] - mean[2],
                    g.bound[2][r.z as usize + 1] - mean[2],
                ];
            }
            for k in 0..d {
                let a = axes[k];
                let (mut pmin, mut pmax) = (0.0f64, 0.0f64);
                for (i, span) in [xs, ys, zs].iter().enumerate() {
                    let (u, v) = (a[i] * span[0], a[i] * span[1]);
                    pmin += fmath::min(u, v);
                    pmax += fmath::max(u, v);
                }
                lo[k] = fmath::min(lo[k], pmin);
                hi[k] = fmath::max(hi[k], pmax);
            }
        }
        let mut b = OrientedBoundingBox {
            origin: centroid,
            axes: patient_axes.to_vec(),
            size: vec![0.0; d],
        };
        for k in 0..d {
            b.size[k] = hi[k] - lo[k];
            b.origin = b.origin.add(patient_axes[k].scale(lo[k]));
        }
        b
    }
}
fn finite_vec(v: Vec3) -> bool {
    [v.x, v.y, v.z]
        .iter()
        .all(|c| !c.is_nan() && !c.is_infinite())
}
fn cell_boundaries(c: &[f64], single: f64) -> Vec<f64> {
    let n = c.len();
    let mut b = vec![0.0f64; n + 1];
    if n == 1 {
        b[0] = c[0] - single / 2.0;
        b[1] = c[0] + single / 2.0;
        return b;
    }
    for i in 1..n {
        b[i] = (c[i - 1] + c[i]) / 2.0;
    }
    b[0] = c[0] - (c[1] - c[0]) / 2.0;
    b[n] = c[n - 1] + (c[n - 1] - c[n - 2]) / 2.0;
    b
}
fn segmentation_shape_grid(s: &Segmentation3D) -> Result<ShapeGrid, Error> {
    let geom = &s.geometry;
    let slices = s.slices();
    let geom_slices: &[crate::render::SliceGeometry] = geom.slices.as_deref().unwrap_or(&[]);
    let positions: &[f64] = geom.positions.as_deref().unwrap_or(&[]);
    let mut n = geom_slices.len() as i64;
    if n == 0 {
        n = positions.len() as i64;
    }
    if let (Some(&first), Some(&last)) = (slices.first(), slices.last()) {
        if first < 0 {
            return Err(Error::wrap(
                ERR_INVALID_SHAPE_INPUT,
                format!("negative slice index {first}"),
            ));
        }
        if last >= n {
            n = last + 1;
        }
    }
    if n == 0 {
        n = 1;
    }
    let (col_sp, row_sp) = slice_in_plane_spacing(geom, 0);
    let z: Vec<f64> = if positions.len() as i64 >= n {
        (0..n as usize)
            .map(|i| positions[i] - positions[0])
            .collect()
    } else {
        let mut step = geom.mean_spacing;
        if !(step > 0.0) {
            step = 1.0;
        }
        uniform_axis(n, step)
    };
    let mut opts = ShapeOptions {
        origin: Vec3::default(),
        direction: [geom.row_dir, geom.col_dir, geom.normal],
        planar: false,
    };
    if let Some(first) = geom_slices.first() {
        opts.origin = first.origin;
        if opts.direction[0] == Vec3::default() {
            opts.direction = [first.row_dir, first.col_dir, first.normal];
        }
    }
    if opts.direction[2] == Vec3::default() && opts.direction[0] != Vec3::default() {
        opts.direction[2] = opts.direction[0].cross(opts.direction[1]);
    }
    ShapeGrid::new(
        false,
        [s.columns, s.rows, n],
        [
            uniform_axis(s.columns, col_sp),
            uniform_axis(s.rows, row_sp),
            z,
        ],
        [col_sp, row_sp, slice_thickness(geom, 0)],
        opts,
    )
}
