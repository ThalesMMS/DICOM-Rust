use super::error::{RegError, Sentinel};
use super::euler::Euler3D;
use super::image::Image;
use super::matrix::{Matrix3, finite, finite_vec};
use super::method::ctx_check;
use crate::fmath;
use crate::optimize::Context;
use crate::render::Vec3;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PrincipalAxisOptions {
    pub intensity_offset: f64,
    pub max_voxels: i64,
    pub relative_gap: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PrincipalMoments {
    pub mass: f64,
    pub center: Vec3,
    pub covariance: Matrix3,
    pub values: [f64; 3],
    pub axes: Matrix3,
}
fn axes_error(detail: &str) -> RegError {
    RegError::new(Sentinel::PrincipalAxes, detail)
}
fn principal_options(o: PrincipalAxisOptions) -> Result<PrincipalAxisOptions, RegError> {
    let mut o = o;
    if !finite(o.intensity_offset)
        || o.max_voxels < 0
        || o.relative_gap < 0.0
        || !finite(o.relative_gap)
        || o.relative_gap >= 1.0
    {
        return Err(axes_error("options"));
    }
    if o.max_voxels == 0 {
        o.max_voxels = 1 << 26;
    }
    if o.relative_gap == 0.0 {
        o.relative_gap = 1e-6;
    }
    Ok(o)
}
fn index_point(im: &Image, i: usize) -> Vec3 {
    im.index_to_physical.mul_vec(Vec3::new(
        (i % im.size[0]) as f64,
        (i / im.size[0] % im.size[1]) as f64,
        (i / (im.size[0] * im.size[1])) as f64,
    ))
}
pub fn image_principal_moments(
    ctx: Context<'_>,
    im: Option<&Image>,
    opts: PrincipalAxisOptions,
) -> Result<PrincipalMoments, RegError> {
    ctx_check(ctx)?;
    let opts = principal_options(opts)?;
    let Some(im) = im else {
        return Err(axes_error("nil image"));
    };
    let mut n = 1i64;
    for &size in &im.size {
        let size = size as i64;
        if size < 1 || n > i64::MAX / size {
            return Err(axes_error("dimensions"));
        }
        n *= size;
    }
    if n != im.data.len() as i64 || n > opts.max_voxels {
        return Err(axes_error("sample count or finite voxel limit"));
    }
    let mut mass = 0.0;
    let mut mean = Vec3::ZERO;
    for (i, &value) in im.data.iter().enumerate() {
        if i & 1023 == 0 {
            ctx_check(ctx)?;
        }
        if !finite(f64::from(value)) {
            return Err(axes_error("non-finite intensity"));
        }
        let w = fmath::max(0.0, f64::from(value) + opts.intensity_offset);
        if !finite(w) {
            return Err(axes_error("weight overflow"));
        }
        if w == 0.0 {
            continue;
        }
        let p = index_point(im, i);
        if !finite_vec(p) || !finite(mass + w) {
            return Err(axes_error("physical moment overflow"));
        }
        mass += w;
        mean = mean.add(p.sub(mean).scale(w / mass));
        if !finite_vec(mean) {
            return Err(axes_error("center overflow"));
        }
    }
    if mass == 0.0 {
        return Err(RegError::bare(Sentinel::NoMass));
    }
    let mut covariance = [[0f64; 3]; 3];
    let mut compensation = [[0f64; 3]; 3];
    for (i, &value) in im.data.iter().enumerate() {
        if i & 1023 == 0 {
            ctx_check(ctx)?;
        }
        let w = fmath::max(0.0, f64::from(value) + opts.intensity_offset) / mass;
        if w == 0.0 {
            continue;
        }
        let p = index_point(im, i).sub(mean);
        let q = [p.x, p.y, p.z];
        for row in 0..3 {
            for col in row..3 {
                let v = w * q[row] * q[col];
                if !finite(v) {
                    return Err(axes_error("covariance overflow"));
                }
                let y = v - compensation[row][col];
                let next = covariance[row][col] + y;
                compensation[row][col] = (next - covariance[row][col]) - y;
                covariance[row][col] = next;
            }
        }
    }
    for row in 0..3 {
        for col in row..3 {
            covariance[col][row] = covariance[row][col];
        }
    }
    let covariance = Matrix3(covariance);
    let (values, axes) = principal_eigen(covariance)?;
    let center = im.origin.add(mean);
    if !finite_vec(center) {
        return Err(axes_error("physical center overflow"));
    }
    ctx_check(ctx)?;
    Ok(PrincipalMoments {
        mass,
        center,
        covariance,
        values,
        axes,
    })
}
fn principal_eigen(m: Matrix3) -> Result<([f64; 3], Matrix3), RegError> {
    let mut a = m.0;
    let mut scale = 0.0;
    for row in &a {
        for &v in row {
            if !finite(v) {
                return Err(RegError::bare(Sentinel::PrincipalAxes));
            }
            scale = fmath::max(scale, v.abs());
        }
    }
    if scale == 0.0 {
        return Ok(([0.0; 3], Matrix3::identity()));
    }
    for row in a.iter_mut() {
        for v in row.iter_mut() {
            *v /= scale;
        }
    }
    let mut axis = Matrix3::identity().0;
    let mut converged = false;
    for _ in 0..32 {
        let largest = fmath::max(a[0][1].abs(), fmath::max(a[0][2].abs(), a[1][2].abs()));
        if largest <= 1e-14 {
            converged = true;
            break;
        }
        for p in 0..2 {
            for q in p + 1..3 {
                if a[p][q].abs() <= 1e-15 {
                    continue;
                }
                let tau = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
                let t = 1f64.copysign(tau) / (tau.abs() + fmath::hypot(1.0, tau));
                let c = 1.0 / fmath::hypot(1.0, t);
                let s = t * c;
                let (app, aqq, apq) = (a[p][p], a[q][q], a[p][q]);
                a[p][p] = app - t * apq;
                a[q][q] = aqq + t * apq;
                a[p][q] = 0.0;
                a[q][p] = 0.0;
                for k in 0..3 {
                    if k != p && k != q {
                        let (x, y) = (a[k][p], a[k][q]);
                        a[k][p] = c * x - s * y;
                        a[p][k] = a[k][p];
                        a[k][q] = s * x + c * y;
                        a[q][k] = a[k][q];
                    }
                    let (x, y) = (axis[k][p], axis[k][q]);
                    axis[k][p] = c * x - s * y;
                    axis[k][q] = s * x + c * y;
                }
            }
        }
    }
    if !converged {
        return Err(axes_error("eigen convergence"));
    }
    let mut order = [0usize, 1, 2];
    order.sort_by(|&i, &j| {
        a[i][i]
            .partial_cmp(&a[j][j])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut values = [0f64; 3];
    let mut axes = [[0f64; 3]; 3];
    for (col, &k) in order.iter().enumerate() {
        let v = a[k][k];
        if v < -1e-12 {
            return Err(axes_error("negative covariance eigenvalue"));
        }
        values[col] = fmath::max(0.0, v) * scale;
        if !finite(values[col]) {
            return Err(RegError::bare(Sentinel::PrincipalAxes));
        }
        for row in 0..3 {
            axes[row][col] = axis[row][k];
        }
    }
    let mut axes = Matrix3(axes);
    if axes.determinant() < 0.0 {
        for row in 0..3 {
            axes.0[row][2] = -axes.0[row][2];
        }
    }
    Ok((values, axes))
}
pub fn principal_axis_candidates(
    ctx: Context<'_>,
    fixed: Option<&Image>,
    moving: Option<&Image>,
    opts: PrincipalAxisOptions,
) -> Result<[Euler3D; 4], RegError> {
    let opts = principal_options(opts)?;
    let f = image_principal_moments(ctx, fixed, opts)?;
    let m = image_principal_moments(ctx, moving, opts)?;
    for moments in [&f, &m] {
        let maximum = moments.values[2];
        if !(maximum > 0.0)
            || (moments.values[1] - moments.values[0]) / maximum <= opts.relative_gap
            || (moments.values[2] - moments.values[1]) / maximum <= opts.relative_gap
        {
            return Err(RegError::bare(Sentinel::AmbiguousPrincipalAxes));
        }
    }
    let signs = [
        [1.0, 1.0, 1.0],
        [1.0, -1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [-1.0, -1.0, 1.0],
    ];
    let mut out: [Euler3D; 4] = std::array::from_fn(|_| Euler3D::new());
    for (i, sign) in signs.iter().enumerate() {
        let rotation = m
            .axes
            .mul(&Matrix3([
                [sign[0], 0.0, 0.0],
                [0.0, sign[1], 0.0],
                [0.0, 0.0, sign[2]],
            ]))
            .mul(&f.axes.transpose());
        let mut t = Euler3D::new();
        t.set_matrix(&rotation)?;
        t.set_center(f.center)?;
        t.set_translation(m.center.sub(f.center))?;
        out[i] = t;
    }
    Ok(out)
}
