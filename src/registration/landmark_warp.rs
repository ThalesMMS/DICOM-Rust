use super::error::{RegError, Sentinel};
use super::matrix::{Matrix3, finite, finite_vec};
use super::method::ctx_check;
use crate::fmath;
use crate::optimize::Context;
use crate::render::Vec3;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LandmarkWarpOptions {
    pub regularization: f64,
    pub max_landmarks: i64,
    pub max_matrix_bytes: i64,
    pub max_scratch_bytes: i64,
    pub max_solve_work: i64,
    pub max_query_interactions: i64,
    pub max_queries: i64,
    pub max_query_bytes: i64,
    pub max_condition: f64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LandmarkWarpDiagnostics {
    pub condition: f64,
    pub matrix_bytes: i64,
    pub scratch_bytes: i64,
    pub solve_work: i64,
    pub scale_mm: f64,
    pub regularization_mm: f64,
    pub fre: f64,
    pub residuals: Option<Vec<f64>>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct LandmarkWarp3D {
    centers: Vec<Vec3>,
    weights: Vec<Vec3>,
    affine: [Vec3; 4],
    origin: Vec3,
    low: Vec3,
    high: Vec3,
    scale: f64,
    options: LandmarkWarpOptions,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LandmarkWarpResult {
    pub warp: Option<LandmarkWarp3D>,
    pub diagnostics: LandmarkWarpDiagnostics,
}
fn input() -> RegError {
    RegError::bare(Sentinel::LandmarkWarpInput)
}
fn limit() -> RegError {
    RegError::bare(Sentinel::LandmarkWarpLimit)
}
fn system() -> RegError {
    RegError::bare(Sentinel::LandmarkWarpSystem)
}
fn warp_options(o: LandmarkWarpOptions) -> Result<LandmarkWarpOptions, RegError> {
    let mut o = o;
    if !finite(o.regularization)
        || o.regularization < 0.0
        || o.max_landmarks < 0
        || o.max_matrix_bytes < 0
        || o.max_scratch_bytes < 0
        || o.max_solve_work < 0
        || o.max_query_interactions < 0
        || o.max_queries < 0
        || o.max_query_bytes < 0
        || !finite(o.max_condition)
        || o.max_condition < 0.0
    {
        return Err(input());
    }
    if o.max_landmarks == 0 {
        o.max_landmarks = 128;
    }
    if o.max_landmarks > 1024 {
        return Err(limit());
    }
    if o.max_matrix_bytes == 0 {
        o.max_matrix_bytes = 16 << 20;
    }
    if o.max_scratch_bytes == 0 {
        o.max_scratch_bytes = 16 << 20;
    }
    if o.max_solve_work == 0 {
        o.max_solve_work = 64 << 20;
    }
    if o.max_query_interactions == 0 {
        o.max_query_interactions = 1 << 20;
    }
    if o.max_queries == 0 {
        o.max_queries = 4096;
    }
    if o.max_query_bytes == 0 {
        o.max_query_bytes = 1 << 20;
    }
    if o.max_condition == 0.0 {
        o.max_condition = 1e10;
    }
    if o.max_condition < 1.0 {
        return Err(input());
    }
    Ok(o)
}
fn warp_norm(v: Vec3) -> f64 {
    fmath::hypot(fmath::hypot(v.x, v.y), v.z)
}
pub fn fit_landmark_warp_3d(
    ctx: Option<Context<'_>>,
    fixed: &[Vec3],
    moving: &[Vec3],
    o: LandmarkWarpOptions,
) -> (LandmarkWarpResult, Option<RegError>) {
    let mut result = LandmarkWarpResult::default();
    let err = fit(ctx, fixed, moving, o, &mut result).err();
    (result, err)
}
fn fit(
    ctx: Option<Context<'_>>,
    fixed: &[Vec3],
    moving: &[Vec3],
    o: LandmarkWarpOptions,
    result: &mut LandmarkWarpResult,
) -> Result<(), RegError> {
    let Some(ctx) = ctx else {
        return Err(input());
    };
    ctx_check(ctx)?;
    let o = warp_options(o)?;
    let n = fixed.len();
    if n != moving.len() || n < 4 {
        return Err(input());
    }
    if n as i64 > o.max_landmarks {
        return Err(limit());
    }
    let d = n as i64 + 4;
    let width = 2 * d + 3;
    let matrix_bytes = 8 * d * d;
    let scratch_bytes = 8 * d * (d + 3) + n as i64 * 104;
    let work = d * d * width;
    result.diagnostics = LandmarkWarpDiagnostics {
        matrix_bytes,
        scratch_bytes,
        solve_work: work,
        regularization_mm: o.regularization,
        ..Default::default()
    };
    if matrix_bytes > o.max_matrix_bytes
        || scratch_bytes > o.max_scratch_bytes
        || work > o.max_solve_work
    {
        return Err(limit());
    }
    let (mut low, mut high) = (fixed[0], fixed[0]);
    for (i, &p) in fixed.iter().enumerate() {
        ctx_check(ctx)?;
        if !finite_vec(p) || !finite_vec(moving[i]) {
            return Err(RegError::new(
                Sentinel::LandmarkWarpInput,
                format!("nonfinite pair {i}"),
            ));
        }
        low = Vec3::new(
            fmath::min(low.x, p.x),
            fmath::min(low.y, p.y),
            fmath::min(low.z, p.z),
        );
        high = Vec3::new(
            fmath::max(high.x, p.x),
            fmath::max(high.y, p.y),
            fmath::max(high.z, p.z),
        );
        for j in 0..i {
            if p == fixed[j] {
                return Err(RegError::new(
                    Sentinel::LandmarkWarpInput,
                    format!(
                        "duplicate fixed pair {j} and {i} (conflict={})",
                        moving[j] != moving[i]
                    ),
                ));
            }
        }
    }
    let origin = low.scale(0.5).add(high.scale(0.5));
    let span = high.sub(low);
    let scale = fmath::max(span.x, fmath::max(span.y, span.z));
    if !(scale > 0.0) || !finite(scale) {
        return Err(system());
    }
    result.diagnostics.scale_mm = scale;
    let mut w = LandmarkWarp3D {
        centers: vec![Vec3::ZERO; n],
        weights: vec![Vec3::ZERO; n],
        affine: [Vec3::ZERO; 4],
        origin,
        low,
        high,
        scale,
        options: o,
    };
    let (dim, stride) = (d as usize, width as usize);
    let mut a = vec![0f64; dim * stride];
    let mut norm = 0.0;
    let lambda = o.regularization / scale;
    if !finite(lambda) {
        return Err(system());
    }
    for (i, &p) in fixed.iter().enumerate() {
        ctx_check(ctx)?;
        w.centers[i] = p.sub(origin).scale(1.0 / scale);
        if !finite_vec(w.centers[i]) {
            return Err(system());
        }
    }
    for i in 0..dim {
        ctx_check(ctx)?;
        if i < n {
            let q = w.centers[i];
            for j in 0..n {
                a[i * stride + j] = -warp_norm(q.sub(w.centers[j]));
            }
            a[i * stride + i] += lambda;
            let affine = [1.0, q.x, q.y, q.z];
            for (j, &v) in affine.iter().enumerate() {
                a[i * stride + n + j] = v;
                a[(n + j) * stride + i] = v;
            }
            let rhs = moving[i].sub(fixed[i]).scale(1.0 / scale);
            if !finite_vec(rhs) {
                return Err(system());
            }
            a[i * stride + 2 * dim] = rhs.x;
            a[i * stride + 2 * dim + 1] = rhs.y;
            a[i * stride + 2 * dim + 2] = rhs.z;
        }
        a[i * stride + dim + i] = 1.0;
    }
    for i in 0..dim {
        let mut row = 0.0;
        for j in 0..dim {
            row += a[i * stride + j].abs();
        }
        norm = fmath::max(norm, row);
    }
    for k in 0..dim {
        ctx_check(ctx)?;
        let mut pivot = k;
        for i in k + 1..dim {
            if a[i * stride + k].abs() > a[pivot * stride + k].abs() {
                pivot = i;
            }
        }
        let pv = a[pivot * stride + k];
        if !finite(pv) || pv.abs() <= 1e-15 * norm {
            return Err(RegError::new(
                Sentinel::LandmarkWarpSystem,
                format!("rank/pivot {k}"),
            ));
        }
        if pivot != k {
            for j in 0..stride {
                a.swap(k * stride + j, pivot * stride + j);
            }
        }
        for j in 0..stride {
            a[k * stride + j] /= pv;
        }
        for i in 0..dim {
            ctx_check(ctx)?;
            if i == k {
                continue;
            }
            let f = a[i * stride + k];
            for j in 0..stride {
                a[i * stride + j] -= f * a[k * stride + j];
            }
        }
    }
    let mut inv_norm = 0.0;
    for i in 0..dim {
        let mut row = 0.0;
        for j in 0..dim {
            row += a[i * stride + dim + j].abs();
        }
        inv_norm = fmath::max(inv_norm, row);
    }
    let condition = norm * inv_norm;
    result.diagnostics.condition = condition;
    if !finite(condition) || condition > o.max_condition {
        return Err(RegError::new(
            Sentinel::LandmarkWarpSystem,
            format!(
                "normalized condition {}",
                crate::numtext::format_float(condition, 'g', -1)
            ),
        ));
    }
    for i in 0..dim {
        let v = Vec3::new(
            a[i * stride + 2 * dim],
            a[i * stride + 2 * dim + 1],
            a[i * stride + 2 * dim + 2],
        );
        if !finite_vec(v) {
            return Err(system());
        }
        if i < n {
            w.weights[i] = v;
        } else {
            w.affine[i - n] = v;
        }
    }
    result.diagnostics.residuals = Some(vec![0.0; n]);
    let mut rms = 0.0;
    for (i, &p) in fixed.iter().enumerate() {
        let got = w.point(ctx, p)?;
        let v = warp_norm(got.sub(moving[i]));
        if !finite(v) {
            return Err(system());
        }
        if let Some(r) = result.diagnostics.residuals.as_mut() {
            r[i] = v;
        }
        rms = fmath::hypot(rms, v);
    }
    result.diagnostics.fre = rms / (n as f64).sqrt();
    if !finite(result.diagnostics.fre) {
        return Err(system());
    }
    ctx_check(ctx)?;
    result.warp = Some(w);
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LandmarkWarpSample {
    pub mapped: Vec3,
    pub jacobian: Matrix3,
    pub determinant: f64,
    pub jacobian_defined: bool,
    pub non_positive_jacobian: bool,
    pub outside_landmark_box: bool,
}
impl LandmarkWarp3D {
    fn valid<'c>(&self, ctx: Option<Context<'c>>, p: Vec3) -> Result<Context<'c>, RegError> {
        let Some(ctx) = ctx else {
            return Err(input());
        };
        if self.centers.len() < 4 || !finite_vec(p) {
            return Err(input());
        }
        ctx_check(ctx)?;
        if self.centers.len() as i64 > self.options.max_query_interactions {
            return Err(limit());
        }
        Ok(ctx)
    }
    fn point(&self, ctx: Context<'_>, p: Vec3) -> Result<Vec3, RegError> {
        let q = p.sub(self.origin).scale(1.0 / self.scale);
        if !finite_vec(q) {
            return Err(input());
        }
        let mut v = self.affine[0]
            .add(self.affine[1].scale(q.x))
            .add(self.affine[2].scale(q.y))
            .add(self.affine[3].scale(q.z));
        for (i, c) in self.centers.iter().enumerate() {
            if i % 32 == 0 {
                ctx_check(ctx)?;
            }
            v = v.add(self.weights[i].scale(-warp_norm(q.sub(*c))));
        }
        let out = p.add(v.scale(self.scale));
        if !finite_vec(out) {
            return Err(system());
        }
        ctx_check(ctx)?;
        Ok(out)
    }
    pub fn point_context(&self, ctx: Option<Context<'_>>, p: Vec3) -> Result<Vec3, RegError> {
        let c = self.valid(ctx, p)?;
        self.point(c, p)
    }
    pub fn jacobian_context(&self, ctx: Option<Context<'_>>, p: Vec3) -> Result<Matrix3, RegError> {
        let ctx = self.valid(ctx, p)?;
        let q = p.sub(self.origin).scale(1.0 / self.scale);
        if !finite_vec(q) {
            return Err(input());
        }
        let mut j = Matrix3::identity();
        for c in 0..3 {
            let v = self.affine[c + 1];
            j.0[0][c] += v.x;
            j.0[1][c] += v.y;
            j.0[2][c] += v.z;
        }
        for (i, c) in self.centers.iter().enumerate() {
            if i % 32 == 0 {
                ctx_check(ctx)?;
            }
            let d = q.sub(*c);
            let r = warp_norm(d);
            let v = self.weights[i];
            if r == 0.0 {
                if v != Vec3::ZERO {
                    return Err(RegError::bare(Sentinel::LandmarkWarpDerivative));
                }
                continue;
            }
            let grad = [-d.x / r, -d.y / r, -d.z / r];
            for (k, g) in grad.iter().enumerate() {
                j.0[0][k] += v.x * g;
                j.0[1][k] += v.y * g;
                j.0[2][k] += v.z * g;
            }
        }
        if !j.finite() {
            return Err(system());
        }
        ctx_check(ctx)?;
        Ok(j)
    }
    pub fn inspect_context(
        &self,
        ctx: Option<Context<'_>>,
        points: &[Vec3],
    ) -> Result<Vec<LandmarkWarpSample>, RegError> {
        let Some(c) = ctx else {
            return Err(input());
        };
        if self.centers.len() < 4 || !(self.scale > 0.0) {
            return Err(input());
        }
        ctx_check(c)?;
        let n = points.len() as i64;
        if n > self.options.max_queries
            || n > self.options.max_query_interactions / self.centers.len() as i64
            || n > self.options.max_query_bytes / 112
        {
            return Err(limit());
        }
        for &p in points {
            self.valid(ctx, p)?;
        }
        let mut out = Vec::with_capacity(points.len());
        for &p in points {
            let v = self.point_context(ctx, p)?;
            let j = self.jacobian_context(ctx, p);
            if let Err(e) = &j
                && !e.is(Sentinel::LandmarkWarpDerivative)
            {
                return Err(e.clone());
            }
            let mut s = LandmarkWarpSample {
                mapped: v,
                outside_landmark_box: p.x < self.low.x
                    || p.x > self.high.x
                    || p.y < self.low.y
                    || p.y > self.high.y
                    || p.z < self.low.z
                    || p.z > self.high.z,
                ..Default::default()
            };
            if let Ok(j) = j {
                s.jacobian = j;
                s.determinant = j.determinant();
                if !finite(s.determinant) {
                    return Err(system());
                }
                s.jacobian_defined = true;
                s.non_positive_jacobian = s.determinant <= 0.0;
            }
            out.push(s);
        }
        ctx_check(c)?;
        Ok(out)
    }
}
