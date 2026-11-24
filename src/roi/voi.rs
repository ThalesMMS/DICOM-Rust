#![allow(clippy::neg_cmp_op_on_partial_ord)]
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::render::Vec3;
use crate::render::volume_snapshot::VolumeDescriptor;
use crate::textfmt::float_v;
use std::fmt;
use std::sync::Arc;
pub const MAX_VOI_VOXELS: i64 = 1 << 24;
pub const DEFAULT_VOI_SUPERSAMPLING: i64 = 5;
pub const MAX_VOI_SUPERSAMPLING: i64 = 16;
const ORTHONORMAL_TOLERANCE: f64 = 1e-6;
const ONE_MINUS_TWO_SURFACE_TOLERANCE: f64 = 0.999999998;
const ONE_PLUS_SURFACE_TOLERANCE: f64 = 1.000000001;
const FOUR_THIRDS_PI: f64 = 4.188790204786391;
pub const ERR_INVALID_VOI: &str = "roi: invalid volume of interest";
pub const ERR_EMPTY_VOI: &str = "roi: the VOI contains no voxel with a value";
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VoxelGrid {
    pub columns: i64,
    pub rows: i64,
    pub slices: i64,
    pub origin: Vec3,
    pub column_step: Vec3,
    pub row_step: Vec3,
    pub slice_step: Vec3,
}
pub fn voxel_grid_from_descriptor(d: &VolumeDescriptor) -> (VoxelGrid, Option<Error>) {
    let a = &d.index_to_patient_lps.0;
    let g = VoxelGrid {
        columns: i64::from(d.dimensions[0]),
        rows: i64::from(d.dimensions[1]),
        slices: i64::from(d.dimensions[2]),
        origin: Vec3::new(a[3], a[7], a[11]),
        column_step: Vec3::new(a[0], a[4], a[8]),
        row_step: Vec3::new(a[1], a[5], a[9]),
        slice_step: Vec3::new(a[2], a[6], a[10]),
    };
    let err = g.validate().err();
    (g, err)
}
impl VoxelGrid {
    pub fn validate(&self) -> Result<(), Error> {
        self.transform().map(|_| ())
    }
    pub fn index_to_point(&self, i: f64, j: f64, k: f64) -> Vec3 {
        self.origin
            .add(self.column_step.scale(i))
            .add(self.row_step.scale(j))
            .add(self.slice_step.scale(k))
    }
    pub fn point_to_index(&self, p: Vec3) -> Option<Vec3> {
        self.transform().ok().map(|t| t.to_index(p))
    }
    pub fn voxel_volume_mm3(&self) -> f64 {
        self.column_step
            .dot(self.row_step.cross(self.slice_step))
            .abs()
    }
    pub(crate) fn transform(&self) -> Result<GridTransform, Error> {
        let g = self;
        if g.columns <= 0 || g.rows <= 0 || g.slices <= 0 {
            return Err(Error::wrap(
                ERR_INVALID_VOI,
                format!(
                    "grid dimensions {}x{}x{} must be positive",
                    g.columns, g.rows, g.slices
                ),
            ));
        }
        for v in [g.origin, g.column_step, g.row_step, g.slice_step] {
            if !finite_vec3(v) {
                return Err(Error::wrap(ERR_INVALID_VOI, "grid geometry must be finite"));
            }
        }
        let (c, r, s) = (g.column_step, g.row_step, g.slice_step);
        let det = c.dot(r.cross(s));
        let scale = c.length() * r.length() * s.length();
        if !(scale > 0.0) || det.abs() <= 1e-12 * scale {
            return Err(Error::wrap(
                ERR_INVALID_VOI,
                "grid steps must be non-zero and linearly independent",
            ));
        }
        let mut t = GridTransform {
            grid: *g,
            inverse: [
                r.cross(s).scale(1.0 / det),
                s.cross(c).scale(1.0 / det),
                c.cross(r).scale(1.0 / det),
            ],
            half_diagonal: 0.0,
        };
        for sign in [[1.0, 1.0], [1.0, -1.0], [-1.0, 1.0], [-1.0, -1.0]] {
            t.half_diagonal = fmath::max(
                t.half_diagonal,
                c.add(r.scale(sign[0])).add(s.scale(sign[1])).length() / 2.0,
            );
        }
        Ok(t)
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct GridTransform {
    pub(crate) grid: VoxelGrid,
    pub(crate) inverse: [Vec3; 3],
    pub(crate) half_diagonal: f64,
}
impl GridTransform {
    #[allow(clippy::wrong_self_convention)]
    pub(crate) fn to_index(&self, p: Vec3) -> Vec3 {
        let d = p.sub(self.grid.origin);
        Vec3::new(
            self.inverse[0].dot(d),
            self.inverse[1].dot(d),
            self.inverse[2].dot(d),
        )
    }
    pub(crate) fn in_grid(&self, i: i64, j: i64, k: i64) -> bool {
        let g = &self.grid;
        i >= 0 && j >= 0 && k >= 0 && i < g.columns && j < g.rows && k < g.slices
    }
}
fn finite_vec3(p: Vec3) -> bool {
    [p.x, p.y, p.z]
        .iter()
        .all(|v| !v.is_nan() && !v.is_infinite())
}
pub trait VoiPrimitive: fmt::Debug + Send + Sync {
    fn validate(&self) -> Result<(), Error>;
    fn contains(&self, p: Vec3) -> bool;
    fn distance_bound(&self, p: Vec3) -> f64;
    fn bounds(&self) -> (Vec3, Vec3);
    fn volume_mm3(&self) -> f64;
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Sphere {
    pub center: Vec3,
    pub radius_mm: f64,
}
pub fn sphere_radius_for_volume(volume_mm3: f64) -> f64 {
    fmath::cbrt(3.0 * volume_mm3 / (4.0 * std::f64::consts::PI))
}
impl VoiPrimitive for Sphere {
    fn validate(&self) -> Result<(), Error> {
        if !finite_vec3(self.center) {
            return Err(Error::wrap(ERR_INVALID_VOI, "sphere centre must be finite"));
        }
        if !finite_positive(self.radius_mm) {
            return Err(Error::wrap(
                ERR_INVALID_VOI,
                format!(
                    "sphere radius {} mm must be finite and positive",
                    float_v(self.radius_mm)
                ),
            ));
        }
        Ok(())
    }
    fn contains(&self, p: Vec3) -> bool {
        let d = p.sub(self.center);
        d.dot(d) < self.radius_mm * self.radius_mm * ONE_MINUS_TWO_SURFACE_TOLERANCE
    }
    fn distance_bound(&self, p: Vec3) -> f64 {
        p.sub(self.center).length() - self.radius_mm
    }
    fn bounds(&self) -> (Vec3, Vec3) {
        let r = Vec3::new(self.radius_mm, self.radius_mm, self.radius_mm);
        (self.center.sub(r), self.center.add(r))
    }
    fn volume_mm3(&self) -> f64 {
        FOUR_THIRDS_PI * fmath::pow(self.radius_mm, 3.0)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ellipsoid {
    pub center: Vec3,
    pub radii_mm: [f64; 3],
    pub axes: [Vec3; 3],
}
fn triple_v(v: [f64; 3]) -> String {
    format!("[{} {} {}]", float_v(v[0]), float_v(v[1]), float_v(v[2]))
}
impl Ellipsoid {
    fn normalized_radius(&self, p: Vec3) -> f64 {
        let axes = primitive_axes(self.axes);
        let d = p.sub(self.center);
        let mut sum = 0.0f64;
        for (i, axis) in axes.iter().enumerate() {
            let u = d.dot(*axis) / self.radii_mm[i];
            sum += u * u;
        }
        sum.sqrt()
    }
}
impl VoiPrimitive for Ellipsoid {
    fn validate(&self) -> Result<(), Error> {
        if !finite_vec3(self.center) {
            return Err(Error::wrap(
                ERR_INVALID_VOI,
                "ellipsoid centre must be finite",
            ));
        }
        for r in self.radii_mm {
            if !finite_positive(r) {
                return Err(Error::wrap(
                    ERR_INVALID_VOI,
                    format!(
                        "ellipsoid radii {} mm must be finite and positive",
                        triple_v(self.radii_mm)
                    ),
                ));
            }
        }
        validate_axes(self.axes)
    }
    fn contains(&self, p: Vec3) -> bool {
        let axes = primitive_axes(self.axes);
        let d = p.sub(self.center);
        let mut sum = 0.0f64;
        for (i, axis) in axes.iter().enumerate() {
            let u = d.dot(*axis);
            sum += u * u / (self.radii_mm[i] * self.radii_mm[i]);
        }
        sum < ONE_MINUS_TWO_SURFACE_TOLERANCE
    }
    fn distance_bound(&self, p: Vec3) -> f64 {
        (self.normalized_radius(p) - 1.0)
            * fmath::min(
                self.radii_mm[0],
                fmath::min(self.radii_mm[1], self.radii_mm[2]),
            )
    }
    fn bounds(&self) -> (Vec3, Vec3) {
        let axes = primitive_axes(self.axes);
        let mut half = [0.0f64; 3];
        for (k, h) in half.iter_mut().enumerate() {
            let mut sum = 0.0f64;
            for (i, axis) in axes.iter().enumerate() {
                let c = self.radii_mm[i] * vec_component(*axis, k);
                sum += c * c;
            }
            *h = sum.sqrt();
        }
        let h = Vec3::new(half[0], half[1], half[2]);
        (self.center.sub(h), self.center.add(h))
    }
    fn volume_mm3(&self) -> f64 {
        FOUR_THIRDS_PI * self.radii_mm[0] * self.radii_mm[1] * self.radii_mm[2]
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VoiBox {
    pub center: Vec3,
    pub size_mm: [f64; 3],
    pub axes: [Vec3; 3],
}
impl VoiPrimitive for VoiBox {
    fn validate(&self) -> Result<(), Error> {
        if !finite_vec3(self.center) {
            return Err(Error::wrap(ERR_INVALID_VOI, "box centre must be finite"));
        }
        for s in self.size_mm {
            if !finite_positive(s) {
                return Err(Error::wrap(
                    ERR_INVALID_VOI,
                    format!(
                        "box size {} mm must be finite and positive",
                        triple_v(self.size_mm)
                    ),
                ));
            }
        }
        validate_axes(self.axes)
    }
    fn contains(&self, p: Vec3) -> bool {
        let axes = primitive_axes(self.axes);
        let d = p.sub(self.center);
        for (i, axis) in axes.iter().enumerate() {
            let half = self.size_mm[i] / 2.0;
            if d.dot(*axis).abs() > half * ONE_PLUS_SURFACE_TOLERANCE {
                return false;
            }
        }
        true
    }
    fn distance_bound(&self, p: Vec3) -> f64 {
        let axes = primitive_axes(self.axes);
        let d = p.sub(self.center);
        let mut bound = f64::NEG_INFINITY;
        for (i, axis) in axes.iter().enumerate() {
            bound = fmath::max(bound, d.dot(*axis).abs() - self.size_mm[i] / 2.0);
        }
        bound
    }
    fn bounds(&self) -> (Vec3, Vec3) {
        let axes = primitive_axes(self.axes);
        let mut half = [0.0f64; 3];
        for (k, h) in half.iter_mut().enumerate() {
            for (i, axis) in axes.iter().enumerate() {
                *h += self.size_mm[i] / 2.0 * vec_component(*axis, k).abs();
            }
        }
        let h = Vec3::new(half[0], half[1], half[2]);
        (self.center.sub(h), self.center.add(h))
    }
    fn volume_mm3(&self) -> f64 {
        self.size_mm[0] * self.size_mm[1] * self.size_mm[2]
    }
}
fn primitive_axes(axes: [Vec3; 3]) -> [Vec3; 3] {
    if axes == [Vec3::default(); 3] {
        return [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
    }
    axes
}
fn validate_axes(axes: [Vec3; 3]) -> Result<(), Error> {
    if axes == [Vec3::default(); 3] {
        return Ok(());
    }
    for (i, a) in axes.iter().enumerate() {
        if !finite_vec3(*a) || (a.length() - 1.0).abs() > ORTHONORMAL_TOLERANCE {
            return Err(Error::wrap(
                ERR_INVALID_VOI,
                format!("axis {i} must be a unit vector"),
            ));
        }
        for b in &axes[i + 1..] {
            if a.dot(*b).abs() > ORTHONORMAL_TOLERANCE {
                return Err(Error::wrap(ERR_INVALID_VOI, "axes must be orthogonal"));
            }
        }
    }
    Ok(())
}
fn vec_component(v: Vec3, k: usize) -> f64 {
    match k {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}
pub(crate) fn finite_positive(v: f64) -> bool {
    v > 0.0 && !v.is_infinite()
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct VoiWeighting(pub i64);
impl VoiWeighting {
    pub const CENTRE_INSIDE: VoiWeighting = VoiWeighting(0);
    pub const PARTIAL_VOLUME: VoiWeighting = VoiWeighting(1);
}
impl fmt::Display for VoiWeighting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if *self == VoiWeighting::PARTIAL_VOLUME {
            "partial volume"
        } else {
            "voxel centre"
        })
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VoiRasterOptions {
    pub weighting: VoiWeighting,
    pub supersampling: i64,
}
impl VoiRasterOptions {
    pub(crate) fn resolved_supersampling(&self) -> Result<i64, Error> {
        if self.weighting != VoiWeighting::CENTRE_INSIDE
            && self.weighting != VoiWeighting::PARTIAL_VOLUME
        {
            return Err(Error::wrap(
                ERR_INVALID_VOI,
                format!("unknown weighting {}", self.weighting.0),
            ));
        }
        let mut n = self.supersampling;
        if n == 0 {
            n = DEFAULT_VOI_SUPERSAMPLING;
        }
        if !(1..=MAX_VOI_SUPERSAMPLING).contains(&n) {
            return Err(Error::wrap(
                ERR_INVALID_VOI,
                format!(
                    "supersampling {} must be in [1, {}]",
                    self.supersampling, MAX_VOI_SUPERSAMPLING
                ),
            ));
        }
        Ok(n)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VoiVoxel {
    pub column: i64,
    pub row: i64,
    pub slice: i64,
    pub weight: f64,
    pub centre_inside: bool,
}
#[derive(Debug, Clone, Default)]
pub struct VoiMask {
    pub grid: VoxelGrid,
    pub primitive: Option<Arc<dyn VoiPrimitive>>,
    pub weighting: VoiWeighting,
    pub voxels: Vec<VoiVoxel>,
    pub clipped: bool,
    pub analytic_volume_mm3: f64,
}
impl VoiMask {
    pub fn volume_mm3(&self) -> f64 {
        let mut sum = 0.0f64;
        for v in &self.voxels {
            sum += v.weight;
        }
        sum * self.grid.voxel_volume_mm3()
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct IndexBox {
    pub(crate) lo: [i64; 3],
    pub(crate) hi: [i64; 3],
}
impl IndexBox {
    pub(crate) fn empty(&self) -> bool {
        self.lo[0] > self.hi[0] || self.lo[1] > self.hi[1] || self.lo[2] > self.hi[2]
    }
    pub(crate) fn count(&self) -> i64 {
        if self.empty() {
            return 0;
        }
        let mut n = 1i64;
        for i in 0..3 {
            n = n.wrapping_mul(self.hi[i].wrapping_sub(self.lo[i]).wrapping_add(1));
            if n > MAX_VOI_VOXELS {
                return MAX_VOI_VOXELS + 1;
            }
        }
        n
    }
}
pub(crate) fn primitive_index_box(t: &GridTransform, primitive: &dyn VoiPrimitive) -> (Vec3, Vec3) {
    let (pmin, pmax) = primitive.bounds();
    let mut lo = Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for corner in 0..8 {
        let mut p = pmin;
        if corner & 1 != 0 {
            p.x = pmax.x;
        }
        if corner & 2 != 0 {
            p.y = pmax.y;
        }
        if corner & 4 != 0 {
            p.z = pmax.z;
        }
        let idx = t.to_index(p);
        lo = Vec3::new(
            fmath::min(lo.x, idx.x),
            fmath::min(lo.y, idx.y),
            fmath::min(lo.z, idx.z),
        );
        hi = Vec3::new(
            fmath::max(hi.x, idx.x),
            fmath::max(hi.y, idx.y),
            fmath::max(hi.z, idx.z),
        );
    }
    (lo, hi)
}
fn clipped_index_box(t: &GridTransform, lo: Vec3, hi: Vec3) -> IndexBox {
    let dims = [t.grid.columns, t.grid.rows, t.grid.slices];
    let (los, his) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
    let mut b = IndexBox {
        lo: [0; 3],
        hi: [0; 3],
    };
    for i in 0..3 {
        b.lo[i] = fmath::f64_to_i64(fmath::max(0.0, los[i].floor()));
        b.hi[i] = fmath::f64_to_i64(fmath::min((dims[i] - 1) as f64, his[i].ceil()));
    }
    b
}
pub fn rasterize_voi(
    ctx: Option<&dyn Cancellation>,
    grid: VoxelGrid,
    primitive: Option<Arc<dyn VoiPrimitive>>,
    opts: VoiRasterOptions,
) -> Result<VoiMask, Error> {
    let t = grid.transform()?;
    let Some(primitive) = primitive else {
        return Err(Error::wrap(ERR_INVALID_VOI, "nil primitive"));
    };
    primitive.validate()?;
    let supersampling = opts.resolved_supersampling()?;
    let (lo, hi) = primitive_index_box(&t, primitive.as_ref());
    let mut voxels: Vec<VoiVoxel> = Vec::new();
    let clipped = lo.x < -0.5
        || lo.y < -0.5
        || lo.z < -0.5
        || hi.x > grid.columns as f64 - 0.5
        || hi.y > grid.rows as f64 - 0.5
        || hi.z > grid.slices as f64 - 0.5;
    let analytic_volume_mm3 = primitive.volume_mm3();
    let b = clipped_index_box(&t, lo, hi);
    if b.count() > MAX_VOI_VOXELS {
        return Err(Error::wrap(
            ERR_INVALID_VOI,
            format!("the VOI covers more than {MAX_VOI_VOXELS} voxels"),
        ));
    }
    rasterize_box(
        ctx,
        &t,
        primitive.as_ref(),
        b,
        opts.weighting,
        supersampling,
        &mut |i, j, k, weight, centre| {
            voxels.push(VoiVoxel {
                column: i,
                row: j,
                slice: k,
                weight,
                centre_inside: centre,
            });
        },
    )?;
    Ok(VoiMask {
        grid,
        primitive: Some(primitive),
        weighting: opts.weighting,
        voxels,
        clipped,
        analytic_volume_mm3,
    })
}
pub(crate) fn rasterize_box(
    ctx: Option<&dyn Cancellation>,
    t: &GridTransform,
    primitive: &dyn VoiPrimitive,
    b: IndexBox,
    weighting: VoiWeighting,
    supersampling: i64,
    emit: &mut dyn FnMut(i64, i64, i64, f64, bool),
) -> Result<(), Error> {
    if b.empty() {
        return Ok(());
    }
    let g = &t.grid;
    let mut offsets: Vec<Vec3> = Vec::new();
    if weighting == VoiWeighting::PARTIAL_VOLUME {
        let u = |n: i64| -> f64 { (n as f64 + 0.5) / supersampling as f64 - 0.5 };
        for c in 0..supersampling {
            for bb in 0..supersampling {
                for a in 0..supersampling {
                    offsets.push(
                        g.column_step
                            .scale(u(a))
                            .add(g.row_step.scale(u(bb)))
                            .add(g.slice_step.scale(u(c))),
                    );
                }
            }
        }
    }
    for k in b.lo[2]..=b.hi[2] {
        for j in b.lo[1]..=b.hi[1] {
            if let Some(err) = ctx_err(ctx) {
                return Err(err);
            }
            for i in b.lo[0]..=b.hi[0] {
                let p = g.index_to_point(i as f64, j as f64, k as f64);
                if weighting == VoiWeighting::CENTRE_INSIDE {
                    if primitive.contains(p) {
                        emit(i, j, k, 1.0, true);
                    }
                    continue;
                }
                let bound = primitive.distance_bound(p);
                if bound >= t.half_diagonal {
                    continue;
                }
                let centre = primitive.contains(p);
                if -bound >= t.half_diagonal {
                    emit(i, j, k, 1.0, centre);
                    continue;
                }
                let mut inside = 0i64;
                for offset in &offsets {
                    if primitive.contains(p.add(*offset)) {
                        inside += 1;
                    }
                }
                if inside > 0 {
                    emit(i, j, k, inside as f64 / offsets.len() as f64, centre);
                }
            }
        }
    }
    Ok(())
}
pub type VoxelValueFn<'a> = dyn FnMut(i64, i64, i64) -> Option<f64> + 'a;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct VoiStatistics {
    pub voxel_count: i64,
    pub missing_voxels: i64,
    pub weight_sum: f64,
    pub volume_mm3: f64,
    pub sum: f64,
    pub mean: f64,
    pub variance: f64,
    pub std_dev: f64,
    pub median: f64,
    pub min: f64,
    pub max: f64,
    pub max_voxel: [i64; 3],
    pub max_point: Vec3,
}
#[derive(Debug, Clone, Copy)]
struct WeightedValue {
    value: f64,
    weight: f64,
    index: usize,
}
impl VoiMask {
    pub fn statistics(
        &self,
        value_at: Option<&mut VoxelValueFn<'_>>,
    ) -> (VoiStatistics, Option<Error>) {
        let m = self;
        let Some(value_at) = value_at else {
            return (
                VoiStatistics::default(),
                Some(Error::wrap(
                    ERR_INVALID_VOI,
                    "statistics need a mask and a value accessor",
                )),
            );
        };
        let mut stats = VoiStatistics::default();
        let mut values: Vec<WeightedValue> = Vec::with_capacity(m.voxels.len());
        for (index, voxel) in m.voxels.iter().enumerate() {
            match value_at(voxel.column, voxel.row, voxel.slice) {
                Some(v) if !v.is_nan() && !v.is_infinite() => values.push(WeightedValue {
                    value: v,
                    weight: voxel.weight,
                    index,
                }),
                _ => stats.missing_voxels += 1,
            }
        }
        if values.is_empty() {
            return (stats, Some(Error::sentinel(ERR_EMPTY_VOI)));
        }
        stats.voxel_count = values.len() as i64;
        let (mut mean, mut m2) = (0.0f64, 0.0f64);
        let extremes_from_centre = values.iter().any(|v| m.voxels[v.index].centre_inside);
        let mut first = true;
        for v in &values {
            stats.weight_sum += v.weight;
            stats.sum += v.weight * v.value;
            let delta = v.value - mean;
            mean += delta * v.weight / stats.weight_sum;
            m2 += v.weight * delta * (v.value - mean);
            let voxel = m.voxels[v.index];
            if extremes_from_centre && !voxel.centre_inside {
                continue;
            }
            if first || v.value < stats.min {
                stats.min = v.value;
            }
            if first || v.value > stats.max {
                stats.max = v.value;
                stats.max_voxel = [voxel.column, voxel.row, voxel.slice];
            }
            first = false;
        }
        stats.mean = mean;
        stats.variance = fmath::max(0.0, m2 / stats.weight_sum);
        stats.std_dev = stats.variance.sqrt();
        stats.volume_mm3 = stats.weight_sum * m.grid.voxel_volume_mm3();
        stats.max_point = m.grid.index_to_point(
            stats.max_voxel[0] as f64,
            stats.max_voxel[1] as f64,
            stats.max_voxel[2] as f64,
        );
        stats.median = weighted_median(&mut values, stats.weight_sum);
        (stats, None)
    }
}
fn weighted_median(values: &mut [WeightedValue], total: f64) -> f64 {
    values.sort_by(|a, b| {
        a.value
            .partial_cmp(&b.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let half = total / 2.0;
    let tolerance = 1e-12 * total;
    let mut cumulative = 0.0f64;
    for (i, v) in values.iter().enumerate() {
        cumulative += v.weight;
        if (cumulative - half).abs() <= tolerance && i + 1 < values.len() {
            return (v.value + values[i + 1].value) / 2.0;
        }
        if cumulative > half {
            return v.value;
        }
    }
    values[values.len() - 1].value
}
