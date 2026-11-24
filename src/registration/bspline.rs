use super::composite::Composite;
use super::displacement::DisplacementField;
use super::error::{RegError, Sentinel};
use super::image::{Image, ints_text};
use super::matrix::{Jacobian, Matrix3, finite, finite_slice, finite_vec};
use super::mattes::{
    EPSILON, LocalSupport, SparseJacobian, cubic_bspline, cubic_bspline_derivative,
};
use super::method::ctx_check;
use super::resample::Grid;
use super::transform::{Error, Kind, Transform, check_length, check_update, vec_from};
use crate::fmath;
use crate::numtext::format_float;
use crate::optimize::Context;
use crate::render::Vec3;
use std::any::Any;
const ORDER: i64 = 3;
const SUPPORT: usize = 64;
pub const BSPLINE_FIXED_PARAMETERS: usize = 18;
impl Kind {
    pub const BSPLINE: Kind = Kind("BSplineTransform");
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BSpline {
    size: [i64; 3],
    origin: Vec3,
    spacing: Vec3,
    direction: Matrix3,
    physical_to_index: Matrix3,
    coefficients: Vec<f64>,
}
#[derive(Debug, Clone, Copy, Default)]
struct Weights {
    start: [i64; 3],
    value: [[f64; 4]; 3],
    derive: [[f64; 4]; 3],
}
fn floats_text(v: &[f64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format_float(*x, 'g', -1)).collect();
    format!("[{}]", parts.join(" "))
}
impl BSpline {
    pub fn new(
        size: [i64; 3],
        origin: Vec3,
        spacing: Vec3,
        direction: Matrix3,
    ) -> Result<BSpline, Error> {
        let mut b = BSpline::default();
        b.set_grid(size, origin, spacing, direction)?;
        Ok(b)
    }
    #[doc(hidden)]
    pub fn for_domain(
        origin: Vec3,
        physical_dimensions: Vec3,
        direction: Matrix3,
        mesh_size: [i64; 3],
    ) -> Result<BSpline, Error> {
        let mut size = [0i64; 3];
        let mut spacing = [0f64; 3];
        let dims = [
            physical_dimensions.x,
            physical_dimensions.y,
            physical_dimensions.z,
        ];
        for axis in 0..3 {
            if mesh_size[axis] < 1 {
                return Err(Error::parameters(format!(
                    "B-spline mesh size {}",
                    ints_text(&mesh_size)
                )));
            }
            size[axis] = mesh_size[axis] + ORDER;
            spacing[axis] = dims[axis] / mesh_size[axis] as f64;
        }
        let shift = Vec3::new(spacing[0], spacing[1], spacing[2]).scale(-0.5 * (ORDER - 1) as f64);
        let grid_origin = origin.add(direction.mul_vec(shift));
        BSpline::new(
            size,
            grid_origin,
            Vec3::new(spacing[0], spacing[1], spacing[2]),
            direction,
        )
    }
    #[doc(hidden)]
    pub fn for_image(im: Option<&Image>, mesh_size: [i64; 3]) -> Result<BSpline, RegError> {
        let Some(im) = im else {
            return Err(RegError::new(Sentinel::Image, "nil image"));
        };
        let start = -0.5 - DOMAIN_MARGIN;
        let origin = im.point(Vec3::new(start, start, start));
        let extent =
            |axis: usize, spacing: f64| (im.size[axis] as f64 + 2.0 * DOMAIN_MARGIN) * spacing;
        let dims = Vec3::new(
            extent(0, im.spacing.x),
            extent(1, im.spacing.y),
            extent(2, im.spacing.z),
        );
        Ok(BSpline::for_domain(origin, dims, im.direction, mesh_size)?)
    }
    fn set_grid(
        &mut self,
        size: [i64; 3],
        origin: Vec3,
        spacing: Vec3,
        direction: Matrix3,
    ) -> Result<(), Error> {
        if size.iter().any(|&n| n < ORDER + 1) {
            return Err(Error::parameters(format!(
                "B-spline grid size {}, at least {} per axis",
                ints_text(&size),
                ORDER + 1
            )));
        }
        let count = size[0].wrapping_mul(size[1]).wrapping_mul(size[2]);
        if count / size[0] / size[1] != size[2] || count > 1 << 26 {
            return Err(Error::parameters(format!(
                "B-spline grid size {} is too large",
                ints_text(&size)
            )));
        }
        if !finite_vec(origin) || !direction.finite() {
            return Err(Error::parameters("non-finite B-spline grid geometry"));
        }
        let geometry = Image::geometry([1, 1, 1], origin, spacing, direction)
            .map_err(|e| Error::parameters(format!("B-spline grid: {e}")))?;
        if self.size != size || self.coefficients.len() as i64 != 3 * count {
            self.coefficients = vec![0.0; 3 * count as usize];
        }
        self.size = size;
        self.origin = origin;
        self.spacing = spacing;
        self.direction = direction;
        self.physical_to_index = geometry.physical_to_index;
        Ok(())
    }
    pub fn grid_size(&self) -> [i64; 3] {
        self.size
    }
    pub fn grid_origin(&self) -> Vec3 {
        self.origin
    }
    pub fn grid_spacing(&self) -> Vec3 {
        self.spacing
    }
    pub fn grid_direction(&self) -> Matrix3 {
        self.direction
    }
    pub fn mesh_size(&self) -> [i64; 3] {
        self.size.map(|n| n - ORDER)
    }
    pub(crate) fn count(&self) -> usize {
        (self.size[0] * self.size[1] * self.size[2]) as usize
    }
    fn row(&self, w: &Weights, k: usize, j: usize) -> usize {
        (((w.start[2] + k as i64) * self.size[1] + w.start[1] + j as i64) * self.size[0]
            + w.start[0]) as usize
    }
    fn support(&self, point: Vec3, derivatives: bool) -> Option<Weights> {
        let mut w = Weights::default();
        let index = self.physical_to_index.mul_vec(point.sub(self.origin));
        let position = [index.x, index.y, index.z];
        let low = 0.5 * (ORDER - 1) as f64;
        for axis in 0..3 {
            let mut v = position[axis];
            let high = self.size[axis] as f64 - 0.5 * (ORDER - 1) as f64 - 1.0;
            if almost_equal_ulps(v, high, 4) {
                v = next_after_n(high, -6);
            } else if v >= high || v < low || v.is_nan() {
                return None;
            }
            let start = fmath::f64_to_i64((v + 0.5 - ORDER as f64 / 2.0).floor());
            w.start[axis] = start;
            let u = v - start as f64;
            for k in 0..4 {
                w.value[axis][k] = cubic_bspline(u - k as f64);
                if derivatives {
                    w.derive[axis][k] = cubic_bspline_derivative(u - k as f64);
                }
            }
        }
        Some(w)
    }
    pub fn displacement(&self, point: Vec3) -> Vec3 {
        match self.support(point, false) {
            Some(w) => self.displacement_of(&w),
            None => Vec3::ZERO,
        }
    }
    fn displacement_of(&self, w: &Weights) -> Vec3 {
        let count = self.count();
        let mut d = [0f64; 3];
        for k in 0..4 {
            for j in 0..4 {
                let wkj = w.value[2][k] * w.value[1][j];
                let row = self.row(w, k, j);
                for i in 0..4 {
                    let weight = wkj * w.value[0][i];
                    let at = row + i;
                    d[0] += weight * self.coefficients[at];
                    d[1] += weight * self.coefficients[count + at];
                    d[2] += weight * self.coefficients[2 * count + at];
                }
            }
        }
        Vec3::new(d[0], d[1], d[2])
    }
    pub fn refine(&self, axes: [bool; 3]) -> Result<BSpline, Error> {
        let mut size = self.size;
        let mut origin = self.origin;
        let mut sp = [self.spacing.x, self.spacing.y, self.spacing.z];
        for axis in 0..3 {
            if !axes[axis] {
                continue;
            }
            size[axis] = 2 * (self.size[axis] - ORDER) + ORDER;
            let mut shift = [0f64; 3];
            shift[axis] = sp[axis] / 2.0;
            origin = origin.add(
                self.direction
                    .mul_vec(Vec3::new(shift[0], shift[1], shift[2])),
            );
            sp[axis] /= 2.0;
        }
        let mut out = BSpline::new(size, origin, Vec3::new(sp[0], sp[1], sp[2]), self.direction)?;
        let (count, out_count) = (self.count(), out.count());
        for d in 0..3 {
            let mut current = self.coefficients[d * count..(d + 1) * count].to_vec();
            let mut current_size = self.size;
            for axis in 0..3 {
                if axes[axis] {
                    (current, current_size) = refine_axis(&current, current_size, axis);
                }
            }
            out.coefficients[d * out_count..(d + 1) * out_count].copy_from_slice(&current);
        }
        Ok(out)
    }
    pub fn membrane_energy(&self, gradient: bool) -> (f64, Option<Vec<f64>>) {
        let count = self.count();
        let mut g = gradient.then(|| vec![0.0; self.coefficients.len()]);
        let size = self.size.map(|n| n as usize);
        let strides = [1, size[0], size[0] * size[1]];
        let spacing = [self.spacing.x, self.spacing.y, self.spacing.z];
        let scale = 1.0 / count as f64;
        let mut energy = 0.0;
        for d in 0..3 {
            let values = &self.coefficients[d * count..(d + 1) * count];
            for z in 0..size[2] {
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        let at = [x, y, z];
                        let i = x + y * strides[1] + z * strides[2];
                        for a in 0..3 {
                            if at[a] + 1 >= size[a] {
                                continue;
                            }
                            let j = i + strides[a];
                            let inverse = 1.0 / (spacing[a] * spacing[a]);
                            let diff = values[j] - values[i];
                            energy += scale * inverse * diff * diff;
                            if let Some(g) = g.as_mut() {
                                g[d * count + j] += 2.0 * scale * inverse * diff;
                                g[d * count + i] -= 2.0 * scale * inverse * diff;
                            }
                        }
                    }
                }
            }
        }
        (energy, g)
    }
    pub fn jacobian_bound(
        &self,
        ctx: Option<Context<'_>>,
    ) -> Result<BSplineJacobianBound, RegError> {
        let Some(ctx) = ctx.filter(|_| !self.coefficients.is_empty()) else {
            return Err(RegError::new(
                Sentinel::InvalidParameters,
                "Jacobian bound context/spline",
            ));
        };
        ctx_check(ctx)?;
        if !self.physical_to_index.finite() {
            return Err(RegError::new(
                Sentinel::InvalidParameters,
                "non-finite Jacobian-bound geometry",
            ));
        }
        let count = self.count();
        let size = self.size.map(|n| n as usize);
        let strides = [1, size[0], size[0] * size[1]];
        let mut delta = [[0f64; 3]; 3];
        for d in 0..3 {
            let v = &self.coefficients[d * count..(d + 1) * count];
            for z in 0..size[2] {
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        let i = x + y * strides[1] + z * strides[2];
                        if i & 1023 == 0 {
                            ctx_check(ctx)?;
                        }
                        if !finite(v[i]) {
                            return Err(RegError::new(
                                Sentinel::InvalidParameters,
                                "non-finite Jacobian-bound coefficient",
                            ));
                        }
                        let at = [x, y, z];
                        for a in 0..3 {
                            if at[a] + 1 < size[a] {
                                delta[d][a] = fmath::max(
                                    delta[d][a],
                                    abs_difference_upper(v[i + strides[a]], v[i]),
                                );
                            }
                        }
                    }
                }
            }
        }
        let q = gradient_norm_upper(&delta, &self.physical_to_index);
        let determinant_lower = determinant_lower_from_norm(q);
        let out = BSplineJacobianBound {
            displacement_gradient_norm_upper: q,
            positive_jacobian_certified: determinant_lower > 0.0
                && !determinant_lower.is_infinite(),
            determinant_lower,
            full_support_origin_mm: self.origin.add(self.direction.mul_vec(self.spacing)),
            full_support_dimensions_mm: Vec3::new(
                (self.size[0] - 3) as f64 * self.spacing.x,
                (self.size[1] - 3) as f64 * self.spacing.y,
                (self.size[2] - 3) as f64 * self.spacing.z,
            ),
            direction: self.direction,
        };
        ctx_check(ctx)?;
        Ok(out)
    }
}
const DOMAIN_MARGIN: f64 = 1.0 / 8.0;
fn refine_axis(values: &[f64], size: [i64; 3], axis: usize) -> (Vec<f64>, [i64; 3]) {
    const SUBDIVISION: [f64; 5] = [1.0 / 8.0, 4.0 / 8.0, 6.0 / 8.0, 4.0 / 8.0, 1.0 / 8.0];
    let mut fine = size;
    fine[axis] = 2 * (size[axis] - ORDER) + ORDER;
    let mut out = vec![0f64; (fine[0] * fine[1] * fine[2]) as usize];
    let strides = [1, size[0], size[0] * size[1]];
    let fine_strides = [1, fine[0], fine[0] * fine[1]];
    for z in 0..fine[2] {
        for y in 0..fine[1] {
            for x in 0..fine[0] {
                let at = [x, y, z];
                let k = at[axis];
                let mut sum = 0.0;
                let mut j = (k - 1) / 2;
                while j <= (k + 3) / 2 {
                    let r = k - 2 * j + 1;
                    if j >= 0 && j < size[axis] && (-2..=2).contains(&r) {
                        let mut coarse = at;
                        coarse[axis] = j;
                        sum += SUBDIVISION[(r + 2) as usize]
                            * values[(coarse[0] * strides[0]
                                + coarse[1] * strides[1]
                                + coarse[2] * strides[2])
                                as usize];
                    }
                    j += 1;
                }
                out[(x * fine_strides[0] + y * fine_strides[1] + z * fine_strides[2]) as usize] =
                    sum;
            }
        }
    }
    (out, fine)
}
impl Transform for BSpline {
    fn kind(&self) -> Kind {
        Kind::BSPLINE
    }
    fn transform_point(&self, point: Vec3) -> Vec3 {
        point.add(self.displacement(point))
    }
    fn number_of_parameters(&self) -> usize {
        self.coefficients.len()
    }
    fn parameters(&self) -> Vec<f64> {
        self.coefficients.clone()
    }
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error> {
        if parameters.len() != self.coefficients.len() {
            return Err(Error::parameters(format!(
                "B-spline parameters have {} values, want {}",
                parameters.len(),
                self.coefficients.len()
            )));
        }
        if !finite_slice(parameters) {
            return Err(Error::parameters(
                "B-spline parameters contain a non-finite value",
            ));
        }
        self.coefficients.copy_from_slice(parameters);
        Ok(())
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        let mut out = vec![
            self.size[0] as f64,
            self.size[1] as f64,
            self.size[2] as f64,
            self.origin.x,
            self.origin.y,
            self.origin.z,
            self.spacing.x,
            self.spacing.y,
            self.spacing.z,
        ];
        for row in &self.direction.0 {
            out.extend(row);
        }
        out
    }
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error> {
        check_length(fixed, BSPLINE_FIXED_PARAMETERS, "B-spline fixed parameters")?;
        let mut size = [0i64; 3];
        for axis in 0..3 {
            let v = fixed[axis];
            if v != v.trunc() || v < (ORDER + 1) as f64 || v > (1 << 20) as f64 {
                return Err(Error::parameters(format!(
                    "B-spline grid size {}",
                    floats_text(&fixed[..3])
                )));
            }
            size[axis] = v as i64;
        }
        let mut direction = Matrix3::default();
        for row in 0..3 {
            direction.0[row] = [fixed[9 + 3 * row], fixed[10 + 3 * row], fixed[11 + 3 * row]];
        }
        self.set_grid(
            size,
            vec_from(&fixed[3..6]),
            vec_from(&fixed[6..9]),
            direction,
        )
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        check_update(update, self.coefficients.len(), factor)?;
        for (c, u) in self.coefficients.iter_mut().zip(update) {
            *c += factor * u;
        }
        Ok(())
    }
    fn jacobian_with_respect_to_parameters(&self, point: Vec3, out: &mut Jacobian) {
        out.reset(self.coefficients.len());
        let mut sparse = SparseJacobian::default();
        self.sparse_jacobian_at(point, &mut sparse);
        for k in 0..sparse.n {
            for d in 0..3 {
                out.set(d, d * sparse.stride + sparse.indices[k], sparse.weights[k]);
            }
        }
    }
    fn jacobian_with_respect_to_position(&self, point: Vec3) -> Matrix3 {
        let mut out = Matrix3::identity();
        let Some(w) = self.support(point, true) else {
            return out;
        };
        let count = self.count();
        let mut per_index = [[0f64; 3]; 3];
        for k in 0..4 {
            for j in 0..4 {
                let row = self.row(&w, k, j);
                for i in 0..4 {
                    let gradient = [
                        w.derive[0][i] * w.value[1][j] * w.value[2][k],
                        w.value[0][i] * w.derive[1][j] * w.value[2][k],
                        w.value[0][i] * w.value[1][j] * w.derive[2][k],
                    ];
                    let at = row + i;
                    for d in 0..3 {
                        let c = self.coefficients[d * count + at];
                        if c == 0.0 {
                            continue;
                        }
                        for a in 0..3 {
                            per_index[d][a] += c * gradient[a];
                        }
                    }
                }
            }
        }
        let p = &self.physical_to_index.0;
        for d in 0..3 {
            for a in 0..3 {
                out.0[d][a] += per_index[d][0] * p[0][a]
                    + per_index[d][1] * p[1][a]
                    + per_index[d][2] * p[2][a];
            }
        }
        out
    }
    fn clone_box(&self) -> Box<dyn Transform> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
impl LocalSupport for BSpline {
    fn local_support(&self) -> bool {
        true
    }
    fn sparse_jacobian_at(&self, point: Vec3, out: &mut SparseJacobian) {
        out.n = 0;
        out.stride = self.count();
        out.post = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        out.indices.resize(SUPPORT, 0);
        out.weights.resize(SUPPORT, 0.0);
        let Some(w) = self.support(point, false) else {
            return;
        };
        let mut n = 0;
        for k in 0..4 {
            for j in 0..4 {
                let wkj = w.value[2][k] * w.value[1][j];
                let row = self.row(&w, k, j);
                for i in 0..4 {
                    out.indices[n] = row + i;
                    out.weights[n] = wkj * w.value[0][i];
                    n += 1;
                }
            }
        }
        out.n = n;
    }
}
struct CompositeLocal<'a>(&'a Composite);
impl LocalSupport for CompositeLocal<'_> {
    fn local_support(&self) -> bool {
        let c = self.0;
        let mut optimized = None;
        for i in 0..c.len() {
            if c.optimized(i) {
                if optimized.is_some() {
                    return false;
                }
                optimized = Some(i);
            }
        }
        let Some(i) = optimized else {
            return false;
        };
        local_of(c.transform(i)).is_some_and(|l| l.local_support())
    }
    fn sparse_jacobian_at(&self, point: Vec3, out: &mut SparseJacobian) {
        let c = self.0;
        out.n = 0;
        let mut point = point;
        let mut found = false;
        for i in 0..c.len() {
            let t = c.transform(i);
            if !found && c.optimized(i) {
                if let Some(l) = local_of(t) {
                    l.sparse_jacobian_at(point, out);
                }
                found = true;
            } else if found && out.n > 0 {
                out.post = t
                    .jacobian_with_respect_to_position(point)
                    .mul(&Matrix3(out.post))
                    .0;
            }
            point = t.transform_point(point);
        }
    }
}
pub(crate) fn local_of(t: &dyn Transform) -> Option<Box<dyn LocalSupport + '_>> {
    if let Some(b) = t.as_any().downcast_ref::<BSpline>() {
        return Some(Box::new(b.clone_ref()));
    }
    if let Some(c) = t.as_any().downcast_ref::<Composite>() {
        return Some(Box::new(CompositeLocal(c)));
    }
    None
}
struct BSplineRef<'a>(&'a BSpline);
impl BSpline {
    fn clone_ref(&self) -> BSplineRef<'_> {
        BSplineRef(self)
    }
}
impl LocalSupport for BSplineRef<'_> {
    fn local_support(&self) -> bool {
        true
    }
    fn sparse_jacobian_at(&self, point: Vec3, out: &mut SparseJacobian) {
        self.0.sparse_jacobian_at(point, out);
    }
}
pub fn transform_displacement_field(
    ctx: Context<'_>,
    t: Option<&dyn Transform>,
    grid: &Grid,
) -> Result<DisplacementField, RegError> {
    ctx_check(ctx)?;
    let Some(t) = t else {
        return Err(RegError::new(
            Sentinel::UnsupportedTransform,
            "nil transform",
        ));
    };
    let geometry = Image::geometry(grid.size, grid.origin, grid.spacing, grid.direction)?;
    let s = grid.size;
    let count = s[0].wrapping_mul(s[1]).wrapping_mul(s[2]);
    if count / s[0] / s[1] != s[2] {
        return Err(RegError::new(
            Sentinel::Image,
            format!("grid size {}", ints_text(&s)),
        ));
    }
    let size = s.map(|n| n as usize);
    let mut vectors = vec![Vec3::ZERO; size[0] * size[1] * size[2]];
    let mut bad = vec![false; size[2]];
    for z in 0..size[2] {
        if ctx.err().is_err() {
            break;
        }
        for y in 0..size[1] {
            if ctx.err().is_err() {
                break;
            }
            for x in 0..size[0] {
                let p = geometry.point(Vec3::new(x as f64, y as f64, z as f64));
                let q = t.transform_point(p);
                if !finite_vec(q) {
                    bad[z] = true;
                }
                vectors[(z * size[1] + y) * size[0] + x] = q.sub(p);
            }
        }
    }
    ctx_check(ctx)?;
    if let Some(z) = bad.iter().position(|b| *b) {
        return Err(RegError::new(
            Sentinel::InvalidParameters,
            format!("the transform is not finite on slice {z}"),
        ));
    }
    Ok(DisplacementField {
        size: grid.size,
        origin: grid.origin,
        spacing: grid.spacing,
        direction: grid.direction,
        vectors,
    })
}
fn almost_equal_ulps(a: f64, b: f64, ulps: i64) -> bool {
    if a == b {
        return true;
    }
    if a.is_nan() || b.is_nan() || a.is_sign_negative() != b.is_sign_negative() {
        return (a - b).abs() <= 0.1 * EPSILON;
    }
    let (ia, ib) = (a.to_bits() as i64, b.to_bits() as i64);
    let diff = ia.wrapping_sub(ib).wrapping_abs();
    diff <= ulps
}
pub(crate) fn next_toward(v: f64, up: bool) -> f64 {
    if up {
        fmath::next_up(v)
    } else {
        -fmath::next_up(-v)
    }
}
fn next_after_n(v: f64, n: i64) -> f64 {
    let up = n >= 0;
    let mut v = v;
    for _ in 0..n.abs() {
        v = next_toward(v, up);
    }
    v
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BSplineJacobianBound {
    pub displacement_gradient_norm_upper: f64,
    pub positive_jacobian_certified: bool,
    pub determinant_lower: f64,
    pub full_support_origin_mm: Vec3,
    pub full_support_dimensions_mm: Vec3,
    pub direction: Matrix3,
}
pub(crate) fn abs_difference_upper(a: f64, b: f64) -> f64 {
    if a == b {
        return 0.0;
    }
    next_toward((a - b).abs(), true)
}
fn multiply_upper(a: f64, b: f64) -> f64 {
    if a == 0.0 || b == 0.0 {
        return 0.0;
    }
    next_toward(a * b, true)
}
fn add_upper(a: f64, b: f64) -> f64 {
    if a == 0.0 {
        return b;
    }
    if b == 0.0 {
        return a;
    }
    next_toward(a + b, true)
}
pub(crate) fn multiply_lower(a: f64, b: f64) -> f64 {
    let v = a * b;
    if !(v > 0.0) {
        return 0.0;
    }
    next_toward(v, false)
}
pub(crate) fn gradient_norm_upper(delta: &[[f64; 3]; 3], m: &Matrix3) -> f64 {
    let mut q = 0.0;
    for row in delta {
        for j in 0..3 {
            let mut v = 0.0;
            for a in 0..3 {
                v = add_upper(v, multiply_upper(row[a], m.0[a][j].abs()));
            }
            if v > 0.0 {
                q = next_toward(fmath::hypot(q, v), true);
            }
        }
    }
    q
}
#[derive(Debug, Clone, Copy, Default)]
struct Interval {
    low: f64,
    high: f64,
}
fn interval_product(a: Interval, b: Interval) -> Interval {
    if a.low == 0.0 && a.high == 0.0 || b.low == 0.0 && b.high == 0.0 {
        return Interval::default();
    }
    let values = [
        a.low * b.low,
        a.low * b.high,
        a.high * b.low,
        a.high * b.high,
    ];
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in values {
        if !v.is_finite() {
            return Interval {
                low: f64::NEG_INFINITY,
                high: f64::INFINITY,
            };
        }
        lo = fmath::min(lo, v);
        hi = fmath::max(hi, v);
    }
    Interval {
        low: next_toward(lo, false),
        high: next_toward(hi, true),
    }
}
pub(crate) fn matrix_determinant_lower(m: &Matrix3) -> f64 {
    if !m.finite() {
        return f64::NEG_INFINITY;
    }
    const PERMUTATIONS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
        [1, 0, 2],
        [0, 2, 1],
    ];
    let mut sum = Interval::default();
    for (n, p) in PERMUTATIONS.iter().enumerate() {
        let mut v = Interval {
            low: 1.0,
            high: 1.0,
        };
        for (i, &j) in p.iter().enumerate() {
            v = interval_product(
                v,
                Interval {
                    low: m.0[i][j],
                    high: m.0[i][j],
                },
            );
        }
        if n >= 3 {
            v = Interval {
                low: -v.high,
                high: -v.low,
            };
        }
        if v.low.is_infinite() || v.high.is_infinite() {
            return f64::NEG_INFINITY;
        }
        if v.low == 0.0 && v.high == 0.0 {
            continue;
        }
        sum = Interval {
            low: next_toward(sum.low + v.low, false),
            high: next_toward(sum.high + v.high, true),
        };
    }
    sum.low
}
pub(crate) fn determinant_lower_from_norm(q: f64) -> f64 {
    if !(q < 1.0) {
        return 0.0;
    }
    let gap = next_toward(1.0 - q, false);
    multiply_lower(multiply_lower(gap, gap), gap)
}
