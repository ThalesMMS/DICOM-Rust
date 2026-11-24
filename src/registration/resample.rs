use super::displacement::{DisplacementField, sample_vectors};
use super::error::{RegError, Sentinel};
use super::image::{Image, ints_text};
use super::matrix::{Matrix3, finite, finite_vec};
use super::transform::Transform;
use crate::fmath;
use crate::render::parallel::{Cancellation, ctx_err};
use crate::render::volume_bspline::CubicBSpline;
use crate::render::{GeometryAffine, Vec3};
use std::rc::Rc;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Grid {
    pub size: [i64; 3],
    pub origin: Vec3,
    pub spacing: Vec3,
    pub direction: Matrix3,
}
impl Image {
    pub fn grid(&self) -> Grid {
        Grid {
            size: self.size.map(|n| n as i64),
            origin: self.origin,
            spacing: self.spacing,
            direction: self.direction,
        }
    }
}
impl Grid {
    pub fn point(&self, index: Vec3) -> Vec3 {
        let scale = Matrix3([
            [self.spacing.x, 0.0, 0.0],
            [0.0, self.spacing.y, 0.0],
            [0.0, 0.0, self.spacing.z],
        ]);
        self.origin.add(self.direction.mul(&scale).mul_vec(index))
    }
    pub fn same_as(&self, other: &Grid, tolerance: f64) -> bool {
        if self.size != other.size
            || self.origin.sub(other.origin).length() > tolerance
            || self.spacing.sub(other.spacing).length() > tolerance
        {
            return false;
        }
        for row in 0..3 {
            for column in 0..3 {
                if (self.direction.0[row][column] - other.direction.0[row][column]).abs()
                    > tolerance
                {
                    return false;
                }
            }
        }
        true
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Interpolator(pub i64);
impl Interpolator {
    pub const LINEAR: Interpolator = Interpolator(0);
    pub const NEAREST: Interpolator = Interpolator(1);
    pub const LABEL_GAUSSIAN: Interpolator = Interpolator(2);
    pub const CUBIC_BSPLINE: Interpolator = Interpolator(3);
    pub const LANCZOS_WINDOWED_SINC: Interpolator = Interpolator(4);
    pub const WELCH_WINDOWED_SINC: Interpolator = Interpolator(5);
    fn windowed_sinc(self) -> bool {
        self == Interpolator::LANCZOS_WINDOWED_SINC || self == Interpolator::WELCH_WINDOWED_SINC
    }
}
const BSPLINE_REGION_MARGIN: i64 = 8;
pub const LABEL_GAUSSIAN_ALPHA: f64 = 4.0;
const WINDOWED_SINC_RADIUS: i64 = 5;
const WINDOWED_SINC_TAPS: usize = 2 * WINDOWED_SINC_RADIUS as usize;
const DEFAULT_WINDOWED_SINC_OUTPUT_BYTES: i64 = 512 << 20;
pub type Mapping = Rc<dyn Fn(Vec3) -> (Vec3, bool)>;
pub fn transform_mapping(t: Option<&dyn Transform>) -> Result<Mapping, RegError> {
    let Some(t) = t else {
        return Err(RegError::new(Sentinel::Resample, "nil transform"));
    };
    let clone = t.clone_box();
    Ok(Rc::new(move |p| {
        let q = clone.transform_point(p);
        (q, finite_vec(q))
    }))
}
pub fn field_mapping(f: Option<&DisplacementField>) -> Result<Mapping, RegError> {
    let Some(f) = f else {
        return Err(RegError::new(Sentinel::Resample, "nil displacement field"));
    };
    let grid = f
        .grid()
        .map_err(|e| RegError::wrap_text(Sentinel::Resample, &e))?;
    let count = f.size[0].wrapping_mul(f.size[1]).wrapping_mul(f.size[2]);
    if f.vectors.len() as i64 != count {
        return Err(RegError::new(
            Sentinel::Resample,
            format!(
                "{} vectors for size {}",
                f.vectors.len(),
                ints_text(&f.size)
            ),
        ));
    }
    let vectors = f.vectors.clone();
    Ok(Rc::new(move |p| {
        let index = grid.continuous_index(p);
        if !grid.inside(index) {
            return (Vec3::ZERO, false);
        }
        let q = p.add(sample_vectors(&vectors, grid.size, index));
        (q, finite_vec(q))
    }))
}
pub fn compose_mappings(first: Mapping, second: Mapping) -> Mapping {
    Rc::new(move |p| match first(p) {
        (q, true) => second(q),
        _ => (Vec3::ZERO, false),
    })
}
#[derive(Default)]
pub struct ResampleOptions<'a> {
    pub interpolator: Interpolator,
    pub max_output_bytes: i64,
    pub padding: f32,
    pub label_sigma: Vec3,
    pub progress: Option<Box<dyn FnMut(i64, i64) + 'a>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SliceRange {
    pub first: i64,
    pub last: i64,
}
impl SliceRange {
    pub fn is_empty(&self) -> bool {
        self.first > self.last
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Resampled {
    pub image: Image,
    pub inside: Vec<bool>,
    pub outside: i64,
    pub source_slices: Vec<SliceRange>,
}
pub fn resample_affine(
    ctx: Option<&dyn Cancellation>,
    moving: Option<&Image>,
    grid: &Grid,
    fixed_to_moving: &GeometryAffine,
    opts: ResampleOptions<'_>,
) -> Result<Resampled, RegError> {
    if !fixed_to_moving.finite() {
        return Err(RegError::new(
            Sentinel::Resample,
            "invalid fixed-to-moving affine",
        ));
    }
    let mut identity = [0.0; 16];
    identity[0] = 1.0;
    identity[5] = 1.0;
    identity[10] = 1.0;
    identity[15] = 1.0;
    if fixed_to_moving.0 == identity {
        let m: Mapping = Rc::new(|p| (p, true));
        return resample_inner(ctx, moving, grid, Some(&m), opts, true);
    }
    let affine = *fixed_to_moving;
    let m: Mapping = Rc::new(move |p| (affine.transform_point(p), true));
    resample(ctx, moving, grid, Some(&m), opts)
}
pub fn resample(
    ctx: Option<&dyn Cancellation>,
    moving: Option<&Image>,
    grid: &Grid,
    mapping: Option<&Mapping>,
    opts: ResampleOptions<'_>,
) -> Result<Resampled, RegError> {
    resample_inner(ctx, moving, grid, mapping, opts, false)
}
fn check(ctx: Option<&dyn Cancellation>) -> Result<(), RegError> {
    match ctx_err(ctx) {
        Some(e) => Err(e.into()),
        None => Ok(()),
    }
}
fn resample_inner(
    ctx: Option<&dyn Cancellation>,
    moving: Option<&Image>,
    grid: &Grid,
    mapping: Option<&Mapping>,
    mut opts: ResampleOptions<'_>,
    identity_affine: bool,
) -> Result<Resampled, RegError> {
    let (Some(moving), Some(mapping)) = (moving, mapping) else {
        return Err(RegError::new(
            Sentinel::Resample,
            "nil moving image or mapping",
        ));
    };
    check(ctx)?;
    let mut target = Image::geometry(grid.size, grid.origin, grid.spacing, grid.direction)
        .map_err(|e| RegError::wrap_text(Sentinel::Resample, &e))?;
    if !finite(f64::from(opts.padding)) {
        return Err(RegError::new(Sentinel::Resample, "non-finite padding"));
    }
    let interpolator = opts.interpolator;
    let mut gaussian = None;
    match interpolator.0 {
        0 | 1 | 3 | 4 | 5 => {}
        2 => gaussian = Some(LabelGaussian::new(moving, opts.label_sigma)?),
        n => {
            return Err(RegError::new(
                Sentinel::Resample,
                format!("unknown interpolator {n}"),
            ));
        }
    }
    let count = resample_output_count(grid.size, &opts)?;
    let mut copy_grid = false;
    if identity_affine
        && (interpolator == Interpolator::LINEAR || interpolator == Interpolator::NEAREST)
        && *grid == moving.grid()
    {
        copy_grid = moving.exact_identity_grid(ctx);
        check(ctx)?;
    }
    let mut spline = None;
    if interpolator == Interpolator::CUBIC_BSPLINE {
        spline = mapped_bspline(ctx, moving, &target, mapping)?;
    }
    let sinc = interpolator.windowed_sinc().then_some(WindowedSinc {
        image: moving,
        window: interpolator,
    });
    let size = target.size;
    let plane = size[0] * size[1];
    let mut data = vec![0f32; count];
    let mut inside = vec![false; count];
    let mut source_slices = vec![SliceRange::default(); size[2]];
    let mut outside = 0i64;
    let mut done = 0i64;
    let m = &target.index_to_physical.0;
    let x_step = Vec3::new(m[0][0], m[1][0], m[2][0]);
    let y_step = Vec3::new(m[0][1], m[1][1], m[2][1]);
    let z_step = Vec3::new(m[0][2], m[1][2], m[2][2]);
    for z in 0..size[2] {
        check(ctx)?;
        if copy_grid {
            let (start, end) = (z * plane, (z + 1) * plane);
            copy_identity_slice(
                &mut data[start..end],
                &mut inside[start..end],
                &moving.data[start..end],
                interpolator,
            );
            source_slices[z] = SliceRange {
                first: z as i64,
                last: z as i64,
            };
        } else {
            let mut sources = SliceRange {
                first: i64::MAX,
                last: i64::MIN,
            };
            let mut missed = 0i64;
            let z_term = z_step.scale(z as f64);
            for y in 0..size[1] {
                let y_term = y_step.scale(y as f64);
                let row = z * plane + y * size[0];
                for x in 0..size[0] {
                    if sinc.is_some() {
                        check(ctx)?;
                    }
                    let i = row + x;
                    let fixed = x_step
                        .scale(x as f64)
                        .add(y_term)
                        .add(z_term)
                        .add(target.origin);
                    let (p, true) = mapping(fixed) else {
                        data[i] = opts.padding;
                        missed += 1;
                        continue;
                    };
                    let index = moving.continuous_index(p);
                    if !moving.inside(index) {
                        data[i] = opts.padding;
                        missed += 1;
                        continue;
                    }
                    let (value, first, last) = match interpolator.0 {
                        1 => {
                            let at = moving.nearest(index);
                            let v = f64::from(moving.at(at[0], at[1], at[2]));
                            (v, at[2] as i64, at[2] as i64)
                        }
                        2 => match &gaussian {
                            Some(g) => g.at(index),
                            None => (0.0, 0, 0),
                        },
                        3 => {
                            let index = moving.clamp_index(index);
                            let v = spline.as_ref().map_or(0.0, |s| s.at(index));
                            let (first, last) = bspline_slices(moving, index.z);
                            (v, first, last)
                        }
                        4 | 5 => match &sinc {
                            Some(s) => s.at(ctx, index)?,
                            None => (0.0, 0, 0),
                        },
                        _ => {
                            let (v, first, last) = moving.linear_at_with_slices(index);
                            (v, first as i64, last as i64)
                        }
                    };
                    data[i] = value as f32;
                    inside[i] = true;
                    sources.first = sources.first.min(first);
                    sources.last = sources.last.max(last);
                }
            }
            if sinc.is_some() {
                check(ctx)?;
            }
            outside += missed;
            source_slices[z] = sources;
        }
        if let Some(progress) = opts.progress.as_mut() {
            done += 1;
            progress(done, size[2] as i64);
        }
    }
    check(ctx)?;
    target.data = data;
    Ok(Resampled {
        image: target,
        inside,
        outside,
        source_slices,
    })
}
impl Image {
    fn exact_identity_grid(&self, ctx: Option<&dyn Cancellation>) -> bool {
        for row in &self.index_to_physical.0 {
            let mut nonzero = 0;
            for &step in row {
                if step == 0.0 {
                    continue;
                }
                if fmath::frexp(step.abs()).0 != 0.5 {
                    return false;
                }
                nonzero += 1;
            }
            if nonzero != 1 {
                return false;
            }
        }
        for (axis, &n) in self.size.iter().enumerate() {
            if n as u64 > 1 << 52 {
                return false;
            }
            for i in 0..n {
                if i % 1024 == 0 && ctx_err(ctx).is_some() {
                    return false;
                }
                let mut coordinates = [0.0; 3];
                coordinates[axis] = i as f64;
                let index = Vec3::new(coordinates[0], coordinates[1], coordinates[2]);
                if self.continuous_index(self.point(index)) != index {
                    return false;
                }
            }
        }
        true
    }
}
fn copy_identity_slice(
    dst: &mut [f32],
    inside: &mut [bool],
    src: &[f32],
    interpolator: Interpolator,
) {
    let linear = interpolator == Interpolator::LINEAR;
    for ((d, flag), &value) in dst.iter_mut().zip(inside.iter_mut()).zip(src) {
        *d = if linear {
            (0.0 + f64::from(value)) as f32
        } else {
            value
        };
        *flag = true;
    }
}
fn resample_output_count(size: [i64; 3], opts: &ResampleOptions<'_>) -> Result<usize, RegError> {
    if opts.max_output_bytes < 0 {
        return Err(RegError::new(
            Sentinel::Resample,
            "negative output byte limit",
        ));
    }
    let max_int = i64::MAX as u64;
    let mut count = 1u64;
    for &n in &size {
        if n < 1 || count > max_int / n as u64 {
            return Err(RegError::new(
                Sentinel::Resample,
                "output sample count overflows int",
            ));
        }
        count *= n as u64;
    }
    const BYTES_PER_SAMPLE: u64 = 5;
    const BYTES_PER_RANGE: u64 = 16;
    if size[2] as u64 > max_int / BYTES_PER_RANGE {
        return Err(RegError::new(
            Sentinel::Resample,
            "output slice ranges overflow int",
        ));
    }
    let range_bytes = size[2] as u64 * BYTES_PER_RANGE;
    if count > (max_int - range_bytes) / BYTES_PER_SAMPLE {
        return Err(RegError::new(
            Sentinel::Resample,
            "output payload bytes overflow int",
        ));
    }
    let bytes = count * BYTES_PER_SAMPLE + range_bytes;
    let mut limit = opts.max_output_bytes;
    if limit == 0 && opts.interpolator.windowed_sinc() {
        limit = DEFAULT_WINDOWED_SINC_OUTPUT_BYTES;
    }
    if limit > 0 && bytes > limit as u64 {
        return Err(RegError::new(
            Sentinel::Resample,
            format!("output payload {bytes} bytes exceeds limit {limit}"),
        ));
    }
    Ok(count as usize)
}
fn mapped_bspline(
    ctx: Option<&dyn Cancellation>,
    moving: &Image,
    target: &Image,
    mapping: &Mapping,
) -> Result<Option<CubicBSpline>, RegError> {
    let size = target.size;
    let mut lo = [i64::MAX; 3];
    let mut hi = [i64::MIN; 3];
    for z in 0..size[2] {
        if ctx_err(ctx).is_some() {
            break;
        }
        for y in 0..size[1] {
            for x in 0..size[0] {
                let (p, true) = mapping(target.point(Vec3::new(x as f64, y as f64, z as f64)))
                else {
                    continue;
                };
                let index = moving.continuous_index(p);
                if !finite_vec(index) || !moving.inside(index) {
                    continue;
                }
                let index = moving.clamp_index(index);
                for (axis, value) in [index.x, index.y, index.z].into_iter().enumerate() {
                    let floor = fmath::f64_to_i64(value.floor());
                    lo[axis] = lo[axis].min(floor);
                    hi[axis] = hi[axis].max(floor);
                }
            }
        }
    }
    check(ctx)?;
    if lo[0] > hi[0] {
        return Ok(None);
    }
    let mut first = [0i64; 3];
    let mut extent = [0i64; 3];
    for axis in 0..3 {
        first[axis] = (lo[axis] - 1 - BSPLINE_REGION_MARGIN).max(0);
        let last = (hi[axis] + 2 + BSPLINE_REGION_MARGIN).min(moving.size[axis] as i64 - 1);
        extent[axis] = last - first[axis] + 1;
    }
    let sample = |x: i64, y: i64, z: i64| f64::from(moving.at(x as usize, y as usize, z as usize));
    match CubicBSpline::new(ctx, first, extent, Some(&sample)) {
        Ok(s) => Ok(Some(s)),
        Err(e) => {
            check(ctx)?;
            Err(RegError::wrap_text(Sentinel::Resample, &e))
        }
    }
}
fn bspline_slices(im: &Image, z: f64) -> (i64, i64) {
    const NEGLIGIBLE: f64 = 1e-12;
    let base = fmath::f64_to_i64(z.floor());
    let t = z - base as f64;
    let (mut first, mut last) = (base - 1, base + 2);
    if (1.0 - t) * (1.0 - t) * (1.0 - t) / 6.0 <= NEGLIGIBLE {
        first = base;
    }
    if t * t * t / 6.0 <= NEGLIGIBLE {
        last = base + 1;
    }
    (first.max(0), last.min(im.size[2] as i64 - 1))
}
struct LabelGaussian<'a> {
    image: &'a Image,
    scale: [f64; 3],
    radius: [i64; 3],
}
impl<'a> LabelGaussian<'a> {
    fn new(im: &'a Image, sigma: Vec3) -> Result<LabelGaussian<'a>, RegError> {
        let sigma = if sigma == Vec3::ZERO {
            im.spacing
        } else {
            sigma
        };
        let spacing = [im.spacing.x, im.spacing.y, im.spacing.z];
        let mut g = LabelGaussian {
            image: im,
            scale: [0.0; 3],
            radius: [0; 3],
        };
        for (axis, s) in [sigma.x, sigma.y, sigma.z].into_iter().enumerate() {
            if !(s > 0.0) || s.is_infinite() {
                return Err(RegError::new(
                    Sentinel::Resample,
                    format!("label sigma {}", sigma.value_text()),
                ));
            }
            let in_index = s / spacing[axis];
            g.scale[axis] = 1.0 / (std::f64::consts::SQRT_2 * in_index);
            g.radius[axis] = fmath::f64_to_i64((LABEL_GAUSSIAN_ALPHA * in_index).ceil());
        }
        Ok(g)
    }
    fn at(&self, index: Vec3) -> (f64, i64, i64) {
        let im = self.image;
        let centre = [index.x, index.y, index.z];
        let mut lo = [0i64; 3];
        let mut hi = [0i64; 3];
        let mut weights: [Vec<f64>; 3] = Default::default();
        for axis in 0..3 {
            let c = fmath::f64_to_i64((centre[axis] + 0.5).floor());
            lo[axis] = c.wrapping_sub(self.radius[axis]).max(0);
            hi[axis] = c
                .wrapping_add(self.radius[axis])
                .min(im.size[axis] as i64 - 1);
            for i in lo[axis]..=hi[axis] {
                let a = (i as f64 - 0.5 - centre[axis]) * self.scale[axis];
                let b = (i as f64 + 0.5 - centre[axis]) * self.scale[axis];
                weights[axis].push((fmath::erf(b) - fmath::erf(a)) / 2.0);
            }
        }
        let mut shares: Vec<(f32, f64)> = Vec::new();
        for k in lo[2]..=hi[2] {
            let wz = weights[2][(k - lo[2]) as usize];
            for j in lo[1]..=hi[1] {
                let wyz = wz * weights[1][(j - lo[1]) as usize];
                for i in lo[0]..=hi[0] {
                    let w = wyz * weights[0][(i - lo[0]) as usize];
                    if w <= 0.0 {
                        continue;
                    }
                    let label = im.at(i as usize, j as usize, k as usize);
                    match shares.iter_mut().find(|s| s.0 == label) {
                        Some(s) => s.1 += w,
                        None => shares.push((label, w)),
                    }
                }
            }
        }
        if shares.is_empty() {
            let at = im.nearest(index);
            return (
                f64::from(im.at(at[0], at[1], at[2])),
                at[2] as i64,
                at[2] as i64,
            );
        }
        let mut best = shares[0];
        for s in &shares[1..] {
            if s.1 > best.1 || (s.1 == best.1 && s.0 < best.0) {
                best = *s;
            }
        }
        (f64::from(best.0), lo[2], hi[2])
    }
}
struct WindowedSinc<'a> {
    image: &'a Image,
    window: Interpolator,
}
impl WindowedSinc<'_> {
    fn weight(&self, distance: f64) -> f64 {
        if distance == 0.0 {
            return 1.0;
        }
        let radius = WINDOWED_SINC_RADIUS as f64;
        if distance.abs() >= radius || distance == distance.trunc() {
            return 0.0;
        }
        let t = std::f64::consts::PI * distance;
        let kernel = fmath::sin(t) / t;
        if self.window == Interpolator::LANCZOS_WINDOWED_SINC {
            let window = t / radius;
            return kernel * (fmath::sin(window) / window);
        }
        kernel * (1.0 - distance * distance / (radius * radius))
    }
    fn at(&self, ctx: Option<&dyn Cancellation>, index: Vec3) -> Result<(f64, i64, i64), RegError> {
        let mut at = [[0usize; WINDOWED_SINC_TAPS]; 3];
        let mut weights = [[0f64; WINDOWED_SINC_TAPS]; 3];
        for (axis, q) in [index.x, index.y, index.z].into_iter().enumerate() {
            let start = fmath::f64_to_i64(q.floor()) + 1 - WINDOWED_SINC_RADIUS;
            for tap in 0..WINDOWED_SINC_TAPS {
                let pixel = start + tap as i64;
                weights[axis][tap] = self.weight(q - pixel as f64);
                at[axis][tap] = pixel.max(0).min(self.image.size[axis] as i64 - 1) as usize;
            }
        }
        let (mut first, mut last) = (i64::MAX, i64::MIN);
        let mut value = 0.0;
        for k in 0..WINDOWED_SINC_TAPS {
            if weights[2][k].abs() > 1e-12 {
                first = first.min(at[2][k] as i64);
                last = last.max(at[2][k] as i64);
            }
            for j in 0..WINDOWED_SINC_TAPS {
                check(ctx)?;
                for i in 0..WINDOWED_SINC_TAPS {
                    let (x, y, z) = (at[0][i], at[1][j], at[2][k]);
                    let pixel = f64::from(self.image.at(x, y, z));
                    if !finite(pixel) {
                        return Err(RegError::new(
                            Sentinel::Resample,
                            format!("non-finite moving sample at ({x},{y},{z})"),
                        ));
                    }
                    value += pixel * weights[0][i] * weights[1][j] * weights[2][k];
                }
            }
        }
        if !finite(value) || !finite(f64::from(value as f32)) {
            return Err(RegError::new(
                Sentinel::Resample,
                "windowed-sinc result is not finite float32",
            ));
        }
        Ok((value, first, last))
    }
}
pub fn subtract(
    minuend: Option<&Image>,
    subtrahend: Option<&Image>,
    valid: Option<&[bool]>,
    padding: f32,
) -> Result<Image, RegError> {
    let (Some(minuend), Some(subtrahend)) = (minuend, subtrahend) else {
        return Err(RegError::new(Sentinel::Resample, "nil image"));
    };
    if !minuend.grid().same_as(&subtrahend.grid(), 1e-4) {
        return Err(RegError::new(
            Sentinel::Resample,
            "the images do not share a grid",
        ));
    }
    if let Some(valid) = valid
        && valid.len() != minuend.data.len()
    {
        return Err(RegError::new(
            Sentinel::Resample,
            format!(
                "{} validity flags for {} samples",
                valid.len(),
                minuend.data.len()
            ),
        ));
    }
    if !finite(f64::from(padding)) {
        return Err(RegError::new(Sentinel::Resample, "non-finite padding"));
    }
    let data = (0..minuend.data.len())
        .map(|i| match valid {
            Some(v) if !v[i] => padding,
            _ => minuend.data[i] - subtrahend.data[i],
        })
        .collect();
    Image::new(
        minuend.size.map(|n| n as i64),
        data,
        minuend.origin,
        minuend.spacing,
        minuend.direction,
    )
}
