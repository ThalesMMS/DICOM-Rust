use super::displacement::{DisplacementField, sample_vectors};
use super::error::{RegError, Sentinel};
use super::image::Image;
use super::method::ctx_check;
use crate::fmath;
use crate::optimize::Context;
use crate::render::Vec3;
use crate::spatialreg;
use crate::textfmt::float_v;
#[derive(Default)]
pub struct DemonsOptions<'a> {
    pub shrinks: Vec<i64>,
    pub iterations: i64,
    pub field_sigma: f64,
    pub update_sigma: f64,
    pub maximum_error: f64,
    pub maximum_kernel_width: i64,
    pub intensity_difference_threshold: f64,
    pub diffeomorphic: bool,
    pub maximum_update_step: f64,
    pub progress: Option<Box<dyn FnMut(DemonsProgress) + 'a>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DemonsProgress {
    pub level: i64,
    pub iteration: i64,
    pub mean_squares: f64,
    pub rms_change: f64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DemonsResult {
    pub field: Option<DisplacementField>,
    pub levels: Vec<DemonsProgress>,
}
fn invalid(detail: impl AsRef<str>) -> RegError {
    RegError::new(Sentinel::Demons, detail)
}
pub fn demons(
    ctx: Context<'_>,
    fixed: Option<&Image>,
    moving: Option<&Image>,
    mut opts: DemonsOptions<'_>,
) -> Result<DemonsResult, RegError> {
    let (Some(fixed), Some(moving)) = (fixed, moving) else {
        return Err(invalid("missing image"));
    };
    let shrinks = if opts.shrinks.is_empty() {
        vec![1]
    } else {
        opts.shrinks.clone()
    };
    let iterations = if opts.iterations <= 0 {
        50
    } else {
        opts.iterations
    };
    let field_sigma = if opts.field_sigma == 0.0 {
        1.0
    } else {
        opts.field_sigma
    };
    let threshold = if opts.intensity_difference_threshold == 0.0 {
        0.001
    } else {
        opts.intensity_difference_threshold
    };
    let max_step = if opts.maximum_update_step == 0.0 {
        0.5
    } else {
        opts.maximum_update_step
    };
    if field_sigma < 0.0 || opts.update_sigma < 0.0 || threshold < 0.0 || max_step < 0.0 {
        return Err(invalid("negative option"));
    }
    if opts.maximum_error == 0.0 {
        opts.maximum_error = 0.1;
    }
    if opts.maximum_kernel_width == 0 {
        opts.maximum_kernel_width = 30;
    }
    if !(opts.maximum_error > 0.0 && opts.maximum_error < 1.0) || opts.maximum_kernel_width < 1 {
        return Err(invalid(format!(
            "Gaussian maximum error {}, kernel width {}",
            float_v(opts.maximum_error),
            opts.maximum_kernel_width
        )));
    }
    let mut field: Option<DisplacementField> = None;
    let mut result = DemonsResult::default();
    for (level, &shrink) in shrinks.iter().enumerate() {
        if shrink < 1 {
            return Err(invalid(format!("shrink {shrink}")));
        }
        let size = fixed.size();
        let factors = [0, 1, 2].map(|a| shrink.min(size[a] as i64));
        let level_fixed = fixed.downsample(factors)?;
        let mut next = new_field(&level_fixed);
        if let Some(previous) = &field {
            let previous_grid = previous.grid()?;
            let psize = previous_grid.size;
            let [nx, ny, nz] = level_fixed.size;
            for z in 0..nz {
                for y in 0..ny {
                    for x in 0..nx {
                        let p = level_fixed.point(Vec3::new(x as f64, y as f64, z as f64));
                        next.vectors[(z * ny + y) * nx + x] = sample_vectors(
                            &previous.vectors,
                            psize,
                            previous_grid.continuous_index(p),
                        );
                    }
                }
            }
        }
        let level_field = field.insert(next);
        let last = demons_level(
            ctx,
            &level_fixed,
            moving,
            level_field,
            level as i64,
            iterations,
            field_sigma,
            threshold,
            max_step,
            &mut opts,
        )?;
        result.levels.push(last);
    }
    result.field = field;
    Ok(result)
}
pub(crate) fn new_field(im: &Image) -> DisplacementField {
    let size = im.size();
    DisplacementField {
        size: size.map(|n| n as i64),
        origin: im.origin(),
        spacing: im.spacing(),
        direction: im.direction(),
        vectors: vec![Vec3::ZERO; size[0] * size[1] * size[2]],
    }
}
fn spacing_array(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
#[allow(clippy::too_many_arguments)]
fn demons_level(
    ctx: Context<'_>,
    fixed: &Image,
    moving: &Image,
    field: &mut DisplacementField,
    level: i64,
    iterations: i64,
    field_sigma: f64,
    threshold: f64,
    max_step: f64,
    opts: &mut DemonsOptions<'_>,
) -> Result<DemonsProgress, RegError> {
    let size = fixed.size();
    let n = size[0] * size[1] * size[2];
    let spacing = spacing_array(fixed.spacing());
    let mut normalizer = 0.0;
    let mut dims = 0i64;
    for (axis, s) in spacing.iter().enumerate() {
        if size[axis] > 1 {
            normalizer += s * s;
            dims += 1;
        }
    }
    normalizer /= dims.max(1) as f64;
    let fixed_gradient = image_gradient(fixed);
    let mut min_spacing = f64::INFINITY;
    for (axis, &s) in spacing.iter().enumerate() {
        if size[axis] > 1 {
            min_spacing = fmath::min(min_spacing, s);
        }
    }
    let field_kernel = gaussian_kernel(field_sigma, opts.maximum_error, opts.maximum_kernel_width);
    let mut update = vec![Vec3::ZERO; n];
    let mut warped = vec![0f32; n];
    let mut inside = vec![false; n];
    let mut last = DemonsProgress::default();
    for iteration in 1..=iterations {
        ctx_check(ctx)?;
        if field_sigma > 0.0 {
            smooth_vectors(&mut field.vectors, size, &field_kernel);
        }
        for z in 0..size[2] {
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let i = (z * size[1] + y) * size[0] + x;
                    let p = fixed
                        .point(Vec3::new(x as f64, y as f64, z as f64))
                        .add(field.vectors[i]);
                    let value = moving.interpolate(p);
                    warped[i] = value.unwrap_or(0.0) as f32;
                    inside[i] = value.is_some();
                }
            }
        }
        let moving_gradient = if opts.diffeomorphic {
            let warped_image = Image::new(
                size.map(|v| v as i64),
                warped.clone(),
                fixed.origin(),
                fixed.spacing(),
                fixed.direction(),
            )?;
            Some(image_gradient(&warped_image))
        } else {
            None
        };
        let (mut sum_squares, mut sum_change, mut count) = (0.0, 0.0, 0i64);
        for z in 0..size[2] {
            let (mut local_squares, mut local_change, mut local_count) = (0.0, 0.0, 0i64);
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let i = (z * size[1] + y) * size[0] + x;
                    update[i] = Vec3::ZERO;
                    if !inside[i] {
                        continue;
                    }
                    let speed = f64::from(fixed.at(x, y, z)) - f64::from(warped[i]);
                    local_squares += speed * speed;
                    local_count += 1;
                    let mut gradient = fixed_gradient[i];
                    if let Some(mg) = &moving_gradient {
                        gradient = gradient.add(mg[i]).scale(0.5);
                    }
                    let denominator = speed * speed / normalizer + gradient.dot(gradient);
                    if speed.abs() < threshold || denominator < 1e-9 {
                        continue;
                    }
                    let mut u = gradient.scale(speed / denominator);
                    if opts.diffeomorphic {
                        u = u.scale(2.0);
                        let length = u.length();
                        if length > max_step * min_spacing {
                            u = u.scale(max_step * min_spacing / length);
                        }
                    }
                    update[i] = u;
                    local_change += u.dot(u);
                }
            }
            sum_squares += local_squares;
            sum_change += local_change;
            count += local_count;
        }
        if opts.update_sigma > 0.0 {
            let kernel = gaussian_kernel(
                opts.update_sigma,
                opts.maximum_error,
                opts.maximum_kernel_width,
            );
            smooth_vectors(&mut update, size, &kernel);
        }
        if opts.diffeomorphic {
            let exponential = exponentiate(&update, fixed);
            compose_field(field, &exponential, fixed);
        } else {
            for (v, u) in field.vectors.iter_mut().zip(&update) {
                *v = v.add(*u);
            }
        }
        last = DemonsProgress {
            level,
            iteration,
            ..DemonsProgress::default()
        };
        if count > 0 {
            last.mean_squares = sum_squares / count as f64;
            last.rms_change = (sum_change / count as f64).sqrt();
        }
        if let Some(progress) = opts.progress.as_mut() {
            progress(last);
        }
    }
    Ok(last)
}
pub(crate) fn image_gradient(im: &Image) -> Vec<Vec3> {
    let size = im.size();
    let spacing = spacing_array(im.spacing());
    let direction = im.direction();
    let mut out = vec![Vec3::ZERO; size[0] * size[1] * size[2]];
    for z in 0..size[2] {
        for y in 0..size[1] {
            for x in 0..size[0] {
                let mut g = [0f64; 3];
                let at = [x, y, z];
                for axis in 0..3 {
                    if size[axis] < 2 {
                        continue;
                    }
                    let (mut lo, mut hi) = (at, at);
                    lo[axis] = at[axis].saturating_sub(1);
                    hi[axis] = (size[axis] - 1).min(at[axis] + 1);
                    g[axis] = (f64::from(im.at(hi[0], hi[1], hi[2]))
                        - f64::from(im.at(lo[0], lo[1], lo[2])))
                        / ((hi[axis] - lo[axis]) as f64 * spacing[axis]);
                }
                out[(z * size[1] + y) * size[0] + x] =
                    direction.mul_vec(Vec3::new(g[0], g[1], g[2]));
            }
        }
    }
    out
}
pub(crate) fn exponentiate(v: &[Vec3], grid: &Image) -> Vec<Vec3> {
    let spacing = grid.spacing();
    let min_spacing = fmath::min(spacing.x, fmath::min(spacing.y, spacing.z));
    let mut largest = 0.0;
    for u in v {
        largest = fmath::max(largest, u.length());
    }
    let mut squarings = 0i64;
    while largest / fmath::pow(2.0, squarings as f64) > 0.5 * min_spacing && squarings < 16 {
        squarings += 1;
    }
    let scale = fmath::pow(2.0, -(squarings as f64));
    let mut out: Vec<Vec3> = v.iter().map(|u| u.scale(scale)).collect();
    let size = grid.size();
    let mut next = vec![Vec3::ZERO; v.len()];
    for _ in 0..squarings {
        for z in 0..size[2] {
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let i = (z * size[1] + y) * size[0] + x;
                    let p = grid
                        .point(Vec3::new(x as f64, y as f64, z as f64))
                        .add(out[i]);
                    next[i] = out[i].add(sample_vectors(&out, size, grid.continuous_index(p)));
                }
            }
        }
        std::mem::swap(&mut out, &mut next);
    }
    out
}
pub(crate) fn compose_field(field: &mut DisplacementField, exponential: &[Vec3], grid: &Image) {
    let size = field.size.map(|n| n as usize);
    let mut composed = vec![Vec3::ZERO; field.vectors.len()];
    for z in 0..size[2] {
        for y in 0..size[1] {
            for x in 0..size[0] {
                let i = (z * size[1] + y) * size[0] + x;
                let p = grid
                    .point(Vec3::new(x as f64, y as f64, z as f64))
                    .add(exponential[i]);
                composed[i] = exponential[i].add(sample_vectors(
                    &field.vectors,
                    size,
                    grid.continuous_index(p),
                ));
            }
        }
    }
    field.vectors.copy_from_slice(&composed);
}
pub(crate) fn gaussian_kernel(sigma: f64, max_error: f64, max_width: i64) -> Vec<f64> {
    let t = sigma * sigma;
    let et = fmath::exp(-t);
    let mut half = vec![et * bessel_i0(t), et * bessel_i1(t)];
    let mut sum = half[0] + 2.0 * half[1];
    let mut i = 2usize;
    while sum < 1.0 - max_error && i as i64 <= max_width {
        let next = half[i - 2] - 2.0 * (i - 1) as f64 / t * half[i - 1];
        if next <= 1e-12 {
            break;
        }
        half.push(next);
        sum += 2.0 * next;
        i += 1;
    }
    let mut kernel = vec![0f64; 2 * half.len() - 1];
    let mid = half.len() - 1;
    for (i, c) in half.iter().enumerate() {
        kernel[mid + i] = c / sum;
        kernel[mid - i] = c / sum;
    }
    kernel
}
fn bessel_i0(x: f64) -> f64 {
    let (mut term, mut sum) = (1.0f64, 1.0f64);
    let mut k = 1i64;
    while k < 60 && term > 1e-17 * sum {
        term *= (x / 2.0) * (x / 2.0) / (k * k) as f64;
        sum += term;
        k += 1;
    }
    sum
}
fn bessel_i1(x: f64) -> f64 {
    let mut term = x / 2.0;
    let mut sum = term;
    let mut k = 1i64;
    while k < 60 && term > 1e-17 * sum {
        term *= (x / 2.0) * (x / 2.0) / (k * (k + 1)) as f64;
        sum += term;
        k += 1;
    }
    sum
}
pub(crate) fn smooth_vectors(vectors: &mut [Vec3], size: [usize; 3], kernel: &[f64]) {
    let radius = (kernel.len() / 2) as i64;
    let strides = [1, size[0], size[0] * size[1]];
    let mut scratch = vec![Vec3::ZERO; vectors.len()];
    for axis in 0..3 {
        let n = size[axis];
        if n < 2 {
            continue;
        }
        let stride = strides[axis];
        for line in 0..vectors.len() / n {
            let start = match axis {
                0 => line * size[0],
                1 => line % size[0] + line / size[0] * size[0] * size[1],
                _ => line,
            };
            for i in 0..n {
                let mut sum = Vec3::ZERO;
                for (k, &w) in kernel.iter().enumerate() {
                    let j = (i as i64 + k as i64 - radius).clamp(0, n as i64 - 1) as usize;
                    sum = sum.add(vectors[start + j * stride].scale(w));
                }
                scratch[start + i * stride] = sum;
            }
        }
        vectors.copy_from_slice(&scratch);
    }
}
impl DisplacementField {
    pub fn spatial_grid(&self) -> Result<spatialreg::Grid, RegError> {
        let d = &self.direction.0;
        let row = Vec3::new(d[0][0], d[1][0], d[2][0]);
        let column = Vec3::new(d[0][1], d[1][1], d[2][1]);
        let normal = Vec3::new(d[0][2], d[1][2], d[2][2]);
        if row.cross(column).dot(normal) <= 0.0 {
            return Err(invalid("the field's axes are left-handed"));
        }
        Ok(spatialreg::Grid::new(
            self.origin,
            row,
            column,
            self.spacing,
            self.size.map(|n| n as u32),
            &self.vectors,
        )?)
    }
}
