use super::distance3d::{ERR_INVALID_VOLUME_MASK, VolumeMask, ctx_check, ctx_poll};
use super::pdqsort::sort_float64s;
use super::signed_distance::{SignedDistanceOptions, signed_distance_axis};
use super::{Cancellation, Error};
use crate::fmath;
pub const ERR_MEDIAL_THICKNESS_LIMIT: &str = "roi: medial thickness resource limit";
pub const ERR_MEDIAL_THICKNESS_DOMAIN: &str = "roi: unsupported medial thickness domain";
pub const MEDIAL_THICKNESS_MAX_VOXELS: i64 = 1 << 20;
pub const MEDIAL_THICKNESS_MAX_BYTES: i64 = 256 << 20;
pub const MEDIAL_THICKNESS_MAX_COMPONENTS: i64 = 1024;
pub const MEDIAL_THICKNESS_METHOD: &str =
    "Lee 26-connected skeleton; 2 x foreground-boundary-centre distance";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MedialThicknessOptions {
    pub max_voxels: i64,
    pub max_components: i64,
    pub max_working_bytes: i64,
}
impl MedialThicknessOptions {
    fn normalized(mut self) -> Result<MedialThicknessOptions, Error> {
        if self.max_voxels == 0 {
            self.max_voxels = MEDIAL_THICKNESS_MAX_VOXELS;
        }
        if self.max_components == 0 {
            self.max_components = MEDIAL_THICKNESS_MAX_COMPONENTS;
        }
        if self.max_working_bytes == 0 {
            self.max_working_bytes = MEDIAL_THICKNESS_MAX_BYTES;
        }
        if self.max_voxels < 1
            || self.max_voxels > MEDIAL_THICKNESS_MAX_VOXELS
            || self.max_components < 1
            || self.max_components > MEDIAL_THICKNESS_MAX_COMPONENTS
            || self.max_working_bytes < 1
            || self.max_working_bytes > MEDIAL_THICKNESS_MAX_BYTES
        {
            return Err(Error::wrap(ERR_MEDIAL_THICKNESS_LIMIT, "invalid limits"));
        }
        Ok(self)
    }
}
pub fn medial_thickness_reservation(
    columns: i64,
    rows: i64,
    slices: i64,
    o: MedialThicknessOptions,
) -> Result<i64, Error> {
    let o = o.normalized()?;
    let (_, bytes) = medial_thickness_admission(columns, rows, slices, o)?;
    Ok(bytes)
}
fn medial_thickness_admission(
    columns: i64,
    rows: i64,
    slices: i64,
    o: MedialThicknessOptions,
) -> Result<(i64, i64), Error> {
    let (mut n, mut padded, mut longest) = (1i64, 1i64, 0i64);
    for d in [columns, rows, slices] {
        if d < 3 {
            return Err(Error::wrap(
                ERR_MEDIAL_THICKNESS_DOMAIN,
                "3D axes require at least three centres",
            ));
        }
        if d > o.max_voxels || n > o.max_voxels / d {
            return Err(Error::sentinel(ERR_MEDIAL_THICKNESS_LIMIT));
        }
        n *= d;
        padded *= d + 2;
        longest = longest.max(d);
    }
    let reserved = 160 * padded + 1024 * longest + 256 * o.max_components + (64 << 10);
    if reserved > o.max_working_bytes {
        return Err(Error::sentinel(ERR_MEDIAL_THICKNESS_LIMIT));
    }
    Ok((n, reserved))
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MedialThicknessSample {
    pub index: i64,
    pub component: i64,
    pub thickness_mm: f64,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MedialThicknessDistribution {
    pub count: i64,
    pub minimum_mm: f64,
    pub maximum_mm: f64,
    pub mean_mm: f64,
    pub median_mm: f64,
    pub p5_mm: f64,
    pub p95_mm: f64,
    pub histogram: [i64; 12],
}
impl Default for MedialThicknessDistribution {
    fn default() -> MedialThicknessDistribution {
        MedialThicknessDistribution {
            count: 0,
            minimum_mm: 0.0,
            maximum_mm: 0.0,
            mean_mm: 0.0,
            median_mm: 0.0,
            p5_mm: 0.0,
            p95_mm: 0.0,
            histogram: [0; 12],
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MedialThicknessComponent {
    pub id: i64,
    pub foreground_voxels: i64,
    pub distribution: MedialThicknessDistribution,
}
#[derive(Debug, Clone, PartialEq)]
pub struct MedialThicknessResult {
    pub skeleton: VolumeMask,
    pub spacing_mm: [f64; 3],
    pub samples: Vec<MedialThicknessSample>,
    pub components: Vec<MedialThicknessComponent>,
    pub distribution: MedialThicknessDistribution,
    pub reserved_bytes: i64,
}
impl MedialThicknessResult {
    pub fn at(&self, x: i64, y: i64, z: i64) -> Option<f64> {
        let s = &self.skeleton;
        if x < 0 || y < 0 || z < 0 || x >= s.columns || y >= s.rows || z >= s.slices {
            return None;
        }
        let i = s.index(x, y, z) as i64;
        let j = self.samples.partition_point(|sample| sample.index < i);
        if j == self.samples.len() || self.samples[j].index != i {
            return None;
        }
        Some(self.samples[j].thickness_mm)
    }
}
impl VolumeMask {
    pub fn medial_thickness(
        &self,
        ctx: Option<&dyn Cancellation>,
        o: MedialThicknessOptions,
    ) -> Result<MedialThicknessResult, Error> {
        ctx_check(ctx)?;
        let o = o.normalized()?;
        let (n, reserved) = medial_thickness_admission(self.columns, self.rows, self.slices, o)?;
        if self.voxels.len() as i64 != n {
            return Err(Error::sentinel(ERR_INVALID_VOLUME_MASK));
        }
        self.validate_context(ctx)?;
        let mut spacing = [0.0f64; 3];
        for (a, axis) in [&self.x, &self.y, &self.z].into_iter().enumerate() {
            spacing[a] = axis[1] - axis[0];
            signed_distance_axis(ctx, axis)?;
            for i in 2..axis.len() {
                if ((axis[i] - axis[i - 1]) - spacing[a]).abs() > 1e-6 * spacing[a] {
                    return Err(Error::wrap(
                        ERR_MEDIAL_THICKNESS_DOMAIN,
                        format_args!("irregular axis {a}"),
                    ));
                }
            }
        }
        let (columns, rows, slices) = (self.columns, self.rows, self.slices);
        let n = n as usize;
        let mut labels = vec![0i32; n];
        let mut queue: Vec<usize> = Vec::with_capacity(n);
        let mut components: Vec<MedialThicknessComponent> = Vec::new();
        for i in 0..n {
            if i & 4095 == 0 {
                ctx_poll(ctx)?;
            }
            if !self.voxels[i] {
                continue;
            }
            let p = i as i64;
            let (x, y, z) = (p % columns, p / columns % rows, p / (columns * rows));
            if x == 0 || y == 0 || z == 0 || x == columns - 1 || y == rows - 1 || z == slices - 1 {
                return Err(Error::wrap(
                    ERR_MEDIAL_THICKNESS_DOMAIN,
                    "foreground touches image edge",
                ));
            }
            if labels[i] != 0 {
                continue;
            }
            if components.len() as i64 == o.max_components {
                return Err(Error::sentinel(ERR_MEDIAL_THICKNESS_LIMIT));
            }
            let id = components.len() as i32 + 1;
            labels[i] = id;
            queue.clear();
            queue.push(i);
            let mut head = 0usize;
            while head < queue.len() {
                if head & 4095 == 0 {
                    ctx_poll(ctx)?;
                }
                let p = queue[head] as i64;
                let (x, y, z) = (p % columns, p / columns % rows, p / (columns * rows));
                for zz in (z - 1).max(0)..=(slices - 1).min(z + 1) {
                    for yy in (y - 1).max(0)..=(rows - 1).min(y + 1) {
                        for xx in (x - 1).max(0)..=(columns - 1).min(x + 1) {
                            let q = self.index(xx, yy, zz);
                            if self.voxels[q] && labels[q] == 0 {
                                labels[q] = id;
                                queue.push(q);
                            }
                        }
                    }
                }
                head += 1;
            }
            components.push(MedialThicknessComponent {
                id: i64::from(id),
                foreground_voxels: queue.len() as i64,
                distribution: MedialThicknessDistribution::default(),
            });
        }
        ctx_check(ctx)?;
        let skeleton = self.thin(ctx)?;
        let distance = self.signed_distance_transform(
            ctx,
            SignedDistanceOptions {
                inside_is_positive: true,
                squared_distance: false,
                max_voxels: o.max_voxels,
                max_bytes: o.max_working_bytes,
            },
        )?;
        let mut samples: Vec<MedialThicknessSample> = Vec::new();
        let mut values: Vec<f64> = Vec::new();
        let mut per_component: Vec<Vec<f64>> = vec![Vec::new(); components.len()];
        for (i, &on) in skeleton.voxels.iter().enumerate() {
            if i & 4095 == 0 {
                ctx_poll(ctx)?;
            }
            if !on {
                continue;
            }
            let v = 2.0 * distance[i];
            if !v.is_finite() || v < 0.0 || labels[i] == 0 {
                return Err(Error::wrap(
                    ERR_MEDIAL_THICKNESS_DOMAIN,
                    "invalid medial sample",
                ));
            }
            let id = i64::from(labels[i]);
            samples.push(MedialThicknessSample {
                index: i as i64,
                component: id,
                thickness_mm: v,
            });
            values.push(v);
            per_component[(id - 1) as usize].push(v);
        }
        let distribution = medial_distribution(&mut values);
        for (component, values) in components.iter_mut().zip(per_component.iter_mut()) {
            ctx_poll(ctx)?;
            component.distribution = medial_distribution(values);
        }
        ctx_check(ctx)?;
        Ok(MedialThicknessResult {
            skeleton,
            spacing_mm: spacing,
            samples,
            components,
            distribution,
            reserved_bytes: reserved,
        })
    }
}
fn medial_distribution(values: &mut [f64]) -> MedialThicknessDistribution {
    let nan = f64::NAN;
    let mut d = MedialThicknessDistribution {
        count: values.len() as i64,
        minimum_mm: nan,
        maximum_mm: nan,
        mean_mm: nan,
        median_mm: nan,
        p5_mm: nan,
        p95_mm: nan,
        histogram: [0; 12],
    };
    if values.is_empty() {
        return d;
    }
    sort_float64s(values);
    d.minimum_mm = values[0];
    d.maximum_mm = values[values.len() - 1];
    d.mean_mm = 0.0;
    for &v in values.iter() {
        d.mean_mm += v / values.len() as f64;
    }
    let quantile = |q: f64| -> f64 {
        let p = q * (values.len() - 1) as f64;
        let lo = fmath::f64_to_i64(p) as usize;
        let hi = (lo + 1).min(values.len() - 1);
        values[lo] + (p - lo as f64) * (values[hi] - values[lo])
    };
    d.median_mm = quantile(0.5);
    d.p5_mm = quantile(0.05);
    d.p95_mm = quantile(0.95);
    for &v in values.iter() {
        let mut bin = 0usize;
        if d.maximum_mm > d.minimum_mm {
            bin = fmath::f64_to_i64(12.0 * (v - d.minimum_mm) / (d.maximum_mm - d.minimum_mm))
                .min(11) as usize;
        }
        d.histogram[bin] += 1;
    }
    d
}
