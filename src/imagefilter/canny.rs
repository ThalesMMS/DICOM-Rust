use super::recursive_gaussian::{
    LANE_WIDTH, RecursiveGaussianOptions, gradient_recursive_gaussian, isotropic_sigma,
    validate_gaussian_source,
};
use super::volume::{Ctx, ErrorKind, FilterError, Volume, ctx_err, voxel_count};
use crate::fmath;
use crate::render::parallel;
fn limit_error(detail: impl AsRef<str>) -> FilterError {
    FilterError::new(
        ErrorKind::CannyResourceLimit,
        format!(
            "imagefilter: Canny resource limit exceeded: {}",
            detail.as_ref()
        ),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CannyParameters {
    pub sigma_mm: [f64; 3],
    pub lower_threshold: f64,
    pub upper_threshold: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CannyLimits {
    pub max_voxels: i64,
    pub max_bytes: i64,
}
const CANNY_MAX_VOXELS: i64 = 8 << 20;
const CANNY_MAX_BYTES: i64 = 512 << 20;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CannyOptions {
    pub parameters: CannyParameters,
    pub limits: CannyLimits,
    pub workers: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct CannyResult {
    pub edges: Volume,
    pub strength: Volume,
    pub edge_count: i64,
}
pub fn default_canny_parameters() -> CannyParameters {
    CannyParameters {
        sigma_mm: isotropic_sigma(1.0),
        ..Default::default()
    }
}
impl CannyParameters {
    pub fn normalize(self, dimensions: i64) -> Result<CannyParameters, FilterError> {
        let mut p = self;
        if dimensions != 2 && dimensions != 3 {
            return Err(FilterError::invalid("Canny dimensions"));
        }
        if p.sigma_mm == [0.0; 3] {
            p.sigma_mm = isotropic_sigma(1.0);
        }
        for (a, sigma) in p.sigma_mm.iter().enumerate().take(dimensions as usize) {
            if !(*sigma > 0.0 && *sigma <= f64::MAX) {
                return Err(FilterError::invalid(format!("Canny sigma on axis {a}")));
            }
        }
        if dimensions == 2 {
            p.sigma_mm[2] = 0.0;
        }
        if !(p.lower_threshold >= 0.0
            && p.upper_threshold >= p.lower_threshold
            && p.upper_threshold <= f64::MAX)
        {
            return Err(FilterError::invalid("Canny thresholds"));
        }
        Ok(p)
    }
}
impl CannyLimits {
    pub fn normalize(self) -> Result<CannyLimits, FilterError> {
        let mut l = self;
        if l.max_voxels < 0
            || l.max_voxels > CANNY_MAX_VOXELS
            || l.max_bytes < 0
            || l.max_bytes > CANNY_MAX_BYTES
        {
            return Err(FilterError::invalid("Canny limits"));
        }
        if l.max_voxels == 0 {
            l.max_voxels = CANNY_MAX_VOXELS;
        }
        if l.max_bytes == 0 {
            l.max_bytes = CANNY_MAX_BYTES;
        }
        Ok(l)
    }
}
pub fn canny_working_bytes(dims: [i64; 3], workers: i64) -> Result<i64, FilterError> {
    let n = match voxel_count(dims) {
        Some(n) if dims[0] >= 2 && dims[1] >= 2 && n <= CANNY_MAX_VOXELS && workers >= 1 => n,
        _ => return Err(limit_error("Canny dimensions/workers")),
    };
    let total = n * 64 + 4096;
    let scratch = canny_gaussian_scratch_bytes(dims, workers)?;
    Ok(total + scratch)
}
pub fn canny_gaussian_scratch_bytes(dims: [i64; 3], workers: i64) -> Result<i64, FilterError> {
    let n = match voxel_count(dims) {
        Some(n) if workers >= 1 && n <= 16 << 20 => n,
        _ => return Err(limit_error("Gaussian scratch dimensions")),
    };
    let lane_width = LANE_WIDTH as i64;
    let mut samples = [0i64; 3];
    let mut active = [0i64; 3];
    let mut max_workers = 0i64;
    for (a, d) in dims.iter().copied().enumerate() {
        if d == 1 {
            continue;
        }
        let (mut lanes, mut groups) = (n / d, 1);
        if a == 1 {
            (lanes, groups) = (dims[0], dims[2]);
        }
        let chunks = (lanes + lane_width - 1) / lane_width;
        active[a] = workers.min(groups * chunks);
        max_workers = max_workers.max(active[a]);
        samples[a] = d * lane_width.min(lanes);
    }
    let mut total = max_workers * (4 * lane_width * 8 + 128);
    for w in 0..max_workers {
        let mut s = 0i64;
        for a in 0..3 {
            if w < active[a] {
                s = s.max(samples[a]);
            }
        }
        total += s * 8;
    }
    Ok(total)
}
pub fn canny(
    ctx: Ctx<'_>,
    src: &Volume,
    options: CannyOptions,
) -> Result<CannyResult, FilterError> {
    ctx_err(ctx)?;
    let n = match voxel_count(src.dims) {
        Some(n) if src.data.len() == n as usize && src.dims[0] >= 2 && src.dims[1] >= 2 => n,
        _ => return Err(FilterError::invalid("Canny dimensions/data")),
    };
    let dimensions = if src.dims[2] == 1 { 2 } else { 3 };
    let p = options.parameters.normalize(dimensions)?;
    let limits = options.limits.normalize()?;
    if options.workers < 0 {
        return Err(FilterError::invalid("negative Canny workers"));
    }
    let workers = parallel::workers(options.workers).min(parallel::default_workers());
    let bytes = canny_working_bytes(src.dims, workers)?;
    if n > limits.max_voxels || bytes > limits.max_bytes {
        return Err(limit_error(format!("{n} voxels, {bytes} reserved bytes")));
    }
    for a in 0..dimensions as usize {
        let h = src.spacing_mm[a];
        let extent = src.dims[a] as f64 * h;
        if !(h > 0.0 && h <= f64::MAX) || !(extent <= f64::MAX.sqrt() / 4.0) {
            return Err(FilterError::invalid(format!(
                "Canny spacing/extent axis {a}"
            )));
        }
    }
    validate_gaussian_source(ctx, src)?;
    let n = n as usize;
    let flat = src.data.iter().all(|v| *v == src.data[0]);
    let like = |data: Vec<f32>| Volume {
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        data,
    };
    if flat {
        ctx_err(ctx)?;
        return Ok(CannyResult {
            edges: like(vec![0.0; n]),
            strength: like(vec![0.0; n]),
            edge_count: 0,
        });
    }
    let gradients = gradient_recursive_gaussian(
        ctx,
        src,
        RecursiveGaussianOptions {
            sigma_mm: p.sigma_mm,
            normalize_across_scale: false,
            workers,
        },
    )?;
    let mut magnitude = vec![0f64; n];
    for (i, slot) in magnitude.iter_mut().enumerate() {
        if i % 4096 == 0 {
            ctx_err(ctx)?;
        }
        let (x, y, z) = (
            f64::from(gradients[0].data[i]),
            f64::from(gradients[1].data[i]),
            f64::from(gradients[2].data[i]),
        );
        let m = fmath::hypot(fmath::hypot(x, y), z);
        if !(m <= f64::from(f32::MAX)) {
            return Err(FilterError::invalid("non-finite Canny gradient"));
        }
        *slot = m;
    }
    let mut peaks = canny_suppress(ctx, src.dims, src.spacing_mm, &gradients, &magnitude)?;
    let (edges, count) = canny_link(ctx, src.dims, &peaks, p.lower_threshold, p.upper_threshold)?;
    for (i, peak) in peaks.iter_mut().enumerate() {
        if i % 4096 == 0 {
            ctx_err(ctx)?;
        }
        if edges[i] == 0.0 {
            *peak = 0.0;
        }
    }
    ctx_err(ctx)?;
    Ok(CannyResult {
        edges: like(edges),
        strength: like(peaks),
        edge_count: count,
    })
}
fn canny_suppress(
    ctx: Ctx<'_>,
    dims: [i64; 3],
    h: [f64; 3],
    g: &[Volume; 3],
    m: &[f64],
) -> Result<Vec<f32>, FilterError> {
    let mut peaks = vec![0f32; m.len()];
    let (nx, ny) = (dims[0] as usize, dims[1] as usize);
    for (i, v) in m.iter().copied().enumerate() {
        if i % 4096 == 0 {
            ctx_err(ctx)?;
        }
        if v <= 0.0 {
            continue;
        }
        let mut dir = [0f64; 3];
        let mut largest = 0f64;
        for a in 0..3 {
            if dims[a] > 1 {
                dir[a] = f64::from(g[a].data[i]) / h[a];
                largest = fmath::max(largest, dir[a].abs());
            }
        }
        if !(largest > 0.0 && largest <= f64::MAX) {
            return Err(FilterError::invalid("Canny direction"));
        }
        let mut dominant = 0;
        for a in 1..3 {
            if dir[a].abs() > dir[dominant].abs() {
                dominant = a;
            }
        }
        let sign = if dir[dominant] < 0.0 { -1.0 } else { 1.0 };
        for d in dir.iter_mut() {
            *d *= sign / largest;
        }
        let pos = [
            (i % nx) as f64,
            (i / nx % ny) as f64,
            (i / (nx * ny)) as f64,
        ];
        let (mut plus, mut minus) = (pos, pos);
        for a in 0..3 {
            plus[a] += dir[a];
            minus[a] -= dir[a];
        }
        let negative = canny_interpolate(m, dims, minus);
        let positive = canny_interpolate(m, dims, plus);
        let mut plus_self = true;
        for a in 0..3 {
            let clamped = fmath::max(0.0, fmath::min((dims[a] - 1) as f64, plus[a]));
            if clamped != pos[a] {
                plus_self = false;
            }
        }
        let keep = if plus_self {
            v > negative
        } else {
            v >= negative && v > positive
        };
        if keep {
            peaks[i] = v as f32;
        }
    }
    Ok(peaks)
}
fn canny_interpolate(data: &[f64], dims: [i64; 3], p: [f64; 3]) -> f64 {
    let mut lo = [0i64; 3];
    let mut hi = [0i64; 3];
    let mut w = [0f64; 3];
    for a in 0..3 {
        let x = fmath::max(0.0, fmath::min((dims[a] - 1) as f64, p[a]));
        lo[a] = fmath::f64_to_i64(x);
        hi[a] = (lo[a] + 1).min(dims[a] - 1);
        w[a] = x - lo[a] as f64;
    }
    let mut sum = 0.0;
    for z in 0..2 {
        for y in 0..2 {
            for x in 0..2 {
                let mut pos = lo;
                let mut weight = 1.0;
                for (a, k) in [x, y, z].into_iter().enumerate() {
                    if k == 1 {
                        pos[a] = hi[a];
                        weight *= w[a];
                    } else {
                        weight *= 1.0 - w[a];
                    }
                }
                sum += weight * data[((pos[2] * dims[1] + pos[1]) * dims[0] + pos[0]) as usize];
            }
        }
    }
    sum
}
fn canny_link(
    ctx: Ctx<'_>,
    dims: [i64; 3],
    peaks: &[f32],
    lower: f64,
    upper: f64,
) -> Result<(Vec<f32>, i64), FilterError> {
    let mut edges = vec![0f32; peaks.len()];
    let mut queue = vec![0i32; peaks.len()];
    let (mut head, mut tail) = (0usize, 0usize);
    for (i, v) in peaks.iter().copied().enumerate() {
        if i % 4096 == 0 {
            ctx_err(ctx)?;
        }
        if v > 0.0 && f64::from(v) >= upper {
            edges[i] = 1.0;
            queue[tail] = i as i32;
            tail += 1;
        }
    }
    let (nx, ny, nz) = (dims[0], dims[1], dims[2]);
    let (zlo, zhi) = if nz == 1 { (0, 0) } else { (-1, 1) };
    while head < tail {
        if head % 4096 == 0 {
            ctx_err(ctx)?;
        }
        let i = i64::from(queue[head]);
        head += 1;
        let (x, y, z) = (i % nx, i / nx % ny, i / (nx * ny));
        for dz in zlo..=zhi {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (xx, yy, zz) = (x + dx, y + dy, z + dz);
                    if xx < 0 || yy < 0 || zz < 0 || xx >= nx || yy >= ny || zz >= nz {
                        continue;
                    }
                    let j = ((zz * ny + yy) * nx + xx) as usize;
                    if edges[j] == 0.0 && peaks[j] > 0.0 && f64::from(peaks[j]) >= lower {
                        edges[j] = 1.0;
                        queue[tail] = j as i32;
                        tail += 1;
                    }
                }
            }
        }
    }
    ctx_err(ctx)?;
    Ok((edges, tail as i64))
}
