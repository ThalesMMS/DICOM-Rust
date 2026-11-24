use super::affine::inverse_affine_err;
use super::{Context, Error, Grid, Kind, Registration, finite, finite_vec};
use crate::fmath;
use crate::numtext::format_float;
use crate::render::affine::GeometryAffine;
use crate::render::vec3::Vec3;
use std::sync::RwLock;
pub const DEFAULT_INVERSE_ITERATIONS: i64 = 50;
pub const DEFAULT_INVERSE_MAX_RESIDUAL: f64 = 0.1;
pub const DEFAULT_INVERSE_TOLERANCE: f64 = 1e-4;
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InverseOptions {
    pub maximum_iterations: i64,
    pub tolerance: f64,
    pub max_residual: f64,
}
impl InverseOptions {
    pub(crate) fn normalized(mut self) -> InverseOptions {
        if self.maximum_iterations <= 0 {
            self.maximum_iterations = DEFAULT_INVERSE_ITERATIONS;
        }
        if !(self.tolerance > 0.0) {
            self.tolerance = DEFAULT_INVERSE_TOLERANCE;
        }
        if !(self.max_residual > 0.0) {
            self.max_residual = DEFAULT_INVERSE_MAX_RESIDUAL;
        }
        self
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InverseResult {
    pub registered: Vec3,
    pub residual: f64,
    pub iterations: i64,
    pub jacobian_determinant: f64,
}
pub(crate) type Matrix3 = [[f64; 3]; 3];
pub(crate) fn linear_part(m: &GeometryAffine) -> Matrix3 {
    let m = &m.0;
    [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]]
}
pub(crate) fn mul_vec(m: &Matrix3, v: Vec3) -> Vec3 {
    Vec3::new(
        m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
        m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
        m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
    )
}
pub(crate) fn determinant3(m: &Matrix3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}
#[derive(Clone, Copy, Debug)]
struct InverseConstants {
    pre: GeometryAffine,
    post: GeometryAffine,
    pre_inverse: GeometryAffine,
    post_inverse: GeometryAffine,
    pre_inverse_linear: Matrix3,
    pre_linear: Matrix3,
    pre_determinant: f64,
}
fn prepare_inverse_constants(r: &Registration) -> Result<InverseConstants, Error> {
    let (pre, post) = (r.pre.values, r.post.values);
    if !pre.finite() || !post.finite() {
        return Err(Error::new(
            Kind::InvalidObject,
            "pre or post matrix is invalid",
        ));
    }
    let post_inverse = inverse_affine_err(&post)?;
    let pre_inverse = inverse_affine_err(&pre)?;
    let pre_linear = linear_part(&pre);
    Ok(InverseConstants {
        pre,
        post,
        pre_inverse,
        post_inverse,
        pre_inverse_linear: linear_part(&pre_inverse),
        pre_linear,
        pre_determinant: determinant3(&pre_linear),
    })
}
impl Grid {
    pub(crate) fn gradient(&self, point: Vec3) -> Result<Matrix3, Error> {
        self.sample(point)?;
        let delta = point.sub(self.origin);
        let directions = [
            self.row_direction,
            self.column_direction,
            self.normal_direction,
        ];
        let spacing = [self.spacing.x, self.spacing.y, self.spacing.z];
        let dims = self.dimensions.map(i64::from);
        let mut lower = [0i64; 3];
        let mut fraction = [0f64; 3];
        for axis in 0..3 {
            let maximum = f64::from(self.dimensions[axis] - 1);
            let coordinate = fmath::max(
                0.0,
                fmath::min(maximum, delta.dot(directions[axis]) / spacing[axis]),
            );
            lower[axis] = coordinate.floor() as i64;
            if lower[axis] >= dims[axis] - 1 {
                lower[axis] = (dims[axis] - 2).max(0);
            }
            fraction[axis] = coordinate - lower[axis] as f64;
        }
        let mut out: Matrix3 = [[0.0; 3]; 3];
        for axis in 0..3 {
            if self.dimensions[axis] < 2 {
                continue;
            }
            let mut derivative = Vec3::ZERO;
            for corner in 0..4 {
                let mut index = [0i64; 3];
                let mut weight = 1.0;
                let mut bit = 0;
                for other in 0..3 {
                    if other == axis {
                        continue;
                    }
                    let f = fraction[other];
                    index[other] = lower[other];
                    if (corner >> bit) & 1 == 1 {
                        if lower[other] + 1 >= dims[other] {
                            weight = 0.0;
                        }
                        index[other] += 1;
                        weight *= f;
                    } else {
                        weight *= 1.0 - f;
                    }
                    bit += 1;
                }
                if weight == 0.0 {
                    continue;
                }
                index[axis] = lower[axis];
                let low = self.vector_at(index[0], index[1], index[2]);
                index[axis] += 1;
                let high = self.vector_at(index[0], index[1], index[2]);
                let (Some(low), Some(high)) = (low, high) else {
                    return Err(Error::new(
                        Kind::UndefinedDomain,
                        format!("vector near ({},{},{})", index[0], index[1], index[2]),
                    ));
                };
                derivative = derivative.add(high.sub(low).scale(weight));
            }
            let d = directions[axis].scale(1.0 / spacing[axis]);
            for (i, component) in [derivative.x, derivative.y, derivative.z]
                .into_iter()
                .enumerate()
            {
                out[i][0] += component * d.x;
                out[i][1] += component * d.y;
                out[i][2] += component * d.z;
            }
        }
        Ok(out)
    }
}
impl Registration {
    pub fn map_source_to_registered(&self, point: Vec3) -> (Vec3, Option<Error>) {
        let (result, err) = self.invert_point(point, InverseOptions::default());
        (result.registered, err)
    }
    pub fn invert_point(
        &self,
        source: Vec3,
        opts: InverseOptions,
    ) -> (InverseResult, Option<Error>) {
        let opts = opts.normalized();
        if !finite_vec(source) {
            return (
                InverseResult::default(),
                Some(Error::new(Kind::InvalidObject, "non-finite source point")),
            );
        }
        let constants = match prepare_inverse_constants(self) {
            Ok(c) => c,
            Err(e) => return (InverseResult::default(), Some(e)),
        };
        self.invert_with_constants(None, source, opts, &constants)
    }
    fn invert_with_constants(
        &self,
        ctx: Option<&Context>,
        source: Vec3,
        opts: InverseOptions,
        c: &InverseConstants,
    ) -> (InverseResult, Option<Error>) {
        let target = c.post_inverse.transform_point(source);
        let deformed = |x: Vec3| -> Result<Vec3, Error> {
            let out = c.pre.transform_point(x);
            match &self.grid {
                None => Ok(out),
                Some(grid) => Ok(out.add(grid.sample(x)?)),
            }
        };
        let residual_of = |y: Vec3| c.post.transform_point(y).sub(source).length();
        let mut x = c.pre_inverse.transform_point(target);
        let mut y = match deformed(x) {
            Ok(y) => y,
            Err(e) => return (InverseResult::default(), Some(e)),
        };
        let mut residual = residual_of(y);
        let mut step = 1.0f64;
        let mut iterations = 0i64;
        while iterations < opts.maximum_iterations && residual > opts.tolerance {
            if let Some(Err(e)) = ctx.map(Context::err) {
                return (InverseResult::default(), Some(e));
            }
            iterations += 1;
            let update = mul_vec(&c.pre_inverse_linear, target.sub(y));
            let next = x.add(update.scale(step));
            let next_y = match deformed(next) {
                Ok(v) => v,
                Err(e) => {
                    if step > 1.0 / 64.0 {
                        step /= 2.0;
                        continue;
                    }
                    return (InverseResult::default(), Some(e));
                }
            };
            let next_residual = residual_of(next_y);
            if next_residual >= residual {
                if step > 1.0 / 64.0 {
                    step /= 2.0;
                    continue;
                }
                break;
            }
            x = next;
            y = next_y;
            residual = next_residual;
            step = fmath::min(1.0, step * 2.0);
        }
        let mut result = InverseResult {
            registered: x,
            residual,
            iterations,
            jacobian_determinant: 0.0,
        };
        let determinant = match self.inverse_jacobian(x, &c.pre_linear, c.pre_determinant) {
            Ok(d) => d,
            Err(e) => return (result, Some(e)),
        };
        result.jacobian_determinant = determinant;
        if !(determinant > 0.0) {
            return (
                result,
                Some(Error::new(
                    Kind::Folding,
                    format!(
                        "Jacobian determinant {} at the inverse",
                        format_float(determinant, 'g', 3)
                    ),
                )),
            );
        }
        if residual > opts.max_residual {
            return (
                result,
                Some(Error::new(
                    Kind::InverseNotConverged,
                    format!(
                        "residual {} mm after {iterations} iterations",
                        format_float(residual, 'g', 3)
                    ),
                )),
            );
        }
        (result, None)
    }
    pub(crate) fn jacobian_determinant(&self, x: Vec3) -> Result<f64, Error> {
        let a = linear_part(&self.pre.values);
        let base = determinant3(&a);
        self.inverse_jacobian(x, &a, base)
    }
    fn inverse_jacobian(&self, x: Vec3, a: &Matrix3, base: f64) -> Result<f64, Error> {
        if base == 0.0 || !finite(base) {
            return Err(Error::new(Kind::InvalidObject, "singular pre-matrix"));
        }
        let Some(grid) = &self.grid else {
            return Ok(1.0);
        };
        let gradient = grid.gradient(x)?;
        let mut sum: Matrix3 = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                sum[i][j] = a[i][j] + gradient[i][j];
            }
        }
        Ok(determinant3(&sum) / base)
    }
    pub fn prepare_inverse(&self, opts: InverseOptions) -> Result<PreparedInverse, Error> {
        let constants = prepare_inverse_constants(self)?;
        let snapshot = Registration {
            pre: self.pre.clone(),
            post: self.post.clone(),
            grid: self.grid.clone(),
            ..Registration::default()
        };
        let retained = snapshot
            .grid
            .as_ref()
            .map_or(0, |g| g.vectors.len() as u64 * 12);
        Ok(PreparedInverse {
            state: RwLock::new(Some(PreparedState {
                registration: snapshot,
                constants,
                retained_grid_bytes: retained,
            })),
            options: opts.normalized(),
        })
    }
}
struct PreparedState {
    registration: Registration,
    constants: InverseConstants,
    retained_grid_bytes: u64,
}
pub struct PreparedInverse {
    state: RwLock<Option<PreparedState>>,
    options: InverseOptions,
}
fn closed() -> Error {
    Error::sentinel(Kind::InverseClosed)
}
fn batch_error(index: usize, result: InverseResult, err: Error) -> Error {
    err.prefixed(format!("dicom/spatialreg: inverse batch point {index}"))
        .with_batch(index, result)
}
impl PreparedInverse {
    pub fn invert_point(&self, ctx: &Context, source: Vec3) -> (InverseResult, Option<Error>) {
        let zero = InverseResult::default();
        if let Err(e) = ctx.err() {
            return (zero, Some(e));
        }
        let guard = self.state.read().unwrap_or_else(|p| p.into_inner());
        let Some(state) = guard.as_ref() else {
            return (zero, Some(closed()));
        };
        if !finite_vec(source) {
            return (
                zero,
                Some(Error::new(Kind::InvalidObject, "non-finite source point")),
            );
        }
        let (result, err) = state.registration.invert_with_constants(
            Some(ctx),
            source,
            self.options,
            &state.constants,
        );
        if let Err(canceled) = ctx.err() {
            return (zero, Some(canceled));
        }
        (result, err)
    }
    pub fn invert_points(
        &self,
        ctx: &Context,
        points: &[Vec3],
    ) -> Result<Vec<InverseResult>, Error> {
        ctx.err()?;
        let guard = self.state.read().unwrap_or_else(|p| p.into_inner());
        let Some(state) = guard.as_ref() else {
            return Err(closed());
        };
        let mut out = Vec::with_capacity(points.len());
        for (i, &source) in points.iter().enumerate() {
            ctx.err()?;
            if !finite_vec(source) {
                return Err(batch_error(
                    i,
                    InverseResult::default(),
                    Error::new(Kind::InvalidObject, "non-finite source point"),
                ));
            }
            let (result, err) = state.registration.invert_with_constants(
                Some(ctx),
                source,
                self.options,
                &state.constants,
            );
            if let Err(canceled) = ctx.err() {
                return Err(batch_error(i, InverseResult::default(), canceled));
            }
            if let Some(err) = err {
                return Err(batch_error(i, result, err));
            }
            out.push(result);
        }
        ctx.err()?;
        Ok(out)
    }
    pub fn retained_grid_bytes(&self) -> u64 {
        let guard = self.state.read().unwrap_or_else(|p| p.into_inner());
        guard.as_ref().map_or(0, |s| s.retained_grid_bytes)
    }
    pub fn close(&self) {
        let mut guard = self.state.write().unwrap_or_else(|p| p.into_inner());
        *guard = None;
    }
}
