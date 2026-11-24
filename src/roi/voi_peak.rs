use super::voi::{
    ERR_EMPTY_VOI, ERR_INVALID_VOI, GridTransform, IndexBox, MAX_VOI_VOXELS, Sphere, VoiMask,
    VoiRasterOptions, VoiWeighting, VoxelValueFn, finite_positive, primitive_index_box,
    rasterize_box, sphere_radius_for_volume,
};
use super::{Cancellation, Error, ctx_err};
use crate::fmath;
use crate::render::Vec3;
use crate::textfmt::float_v;
use std::fmt;
pub const DEFAULT_PEAK_VOLUME_MM3: f64 = 1000.0;
pub const MAX_PEAK_VOXEL_READS: u64 = 1 << 30;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PeakSearch(pub i64);
impl PeakSearch {
    pub const VOI: PeakSearch = PeakSearch(0);
    pub const AROUND_MAXIMUM: PeakSearch = PeakSearch(1);
    pub const AT_MAXIMUM: PeakSearch = PeakSearch(2);
}
impl fmt::Display for PeakSearch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            PeakSearch::AROUND_MAXIMUM => "around the maximum",
            PeakSearch::AT_MAXIMUM => "at the maximum",
            _ => "whole VOI",
        })
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PeakOptions {
    pub volume_mm3: f64,
    pub centre_inside_kernel: bool,
    pub supersampling: i64,
    pub search: PeakSearch,
    pub search_radius_mm: f64,
    pub allow_incomplete_sphere: bool,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PeakResult {
    pub mean: f64,
    pub center: Vec3,
    pub center_voxel: [i64; 3],
    pub radius_mm: f64,
    pub kernel_voxels: i64,
    pub kernel_volume_mm3: f64,
    pub volume_mm3: f64,
    pub weighting: VoiWeighting,
    pub search: PeakSearch,
    pub maximum: f64,
    pub maximum_voxel: [i64; 3],
    pub candidates: i64,
    pub skipped_incomplete: i64,
    pub incomplete: bool,
    pub covered_volume_mm3: f64,
}
pub const ERR_NO_PEAK: &str = "roi: no complete peak sphere is centred inside the VOI";
#[derive(Debug, Clone, Copy)]
struct KernelVoxel {
    di: i64,
    dj: i64,
    dk: i64,
    weight: f64,
}
fn peak_kernel(
    ctx: Option<&dyn Cancellation>,
    t: &GridTransform,
    radius: f64,
    weighting: VoiWeighting,
    supersampling: i64,
) -> Result<Vec<KernelVoxel>, Error> {
    let sphere = Sphere {
        center: t.grid.origin,
        radius_mm: radius,
    };
    let (lo, hi) = primitive_index_box(t, &sphere);
    let b = IndexBox {
        lo: [
            fmath::f64_to_i64(lo.x.floor()),
            fmath::f64_to_i64(lo.y.floor()),
            fmath::f64_to_i64(lo.z.floor()),
        ],
        hi: [
            fmath::f64_to_i64(hi.x.ceil()),
            fmath::f64_to_i64(hi.y.ceil()),
            fmath::f64_to_i64(hi.z.ceil()),
        ],
    };
    if b.count() > MAX_VOI_VOXELS {
        return Err(Error::wrap(
            ERR_INVALID_VOI,
            format!("the peak sphere covers more than {MAX_VOI_VOXELS} voxels"),
        ));
    }
    let mut kernel: Vec<KernelVoxel> = Vec::new();
    rasterize_box(
        ctx,
        t,
        &sphere,
        b,
        weighting,
        supersampling,
        &mut |i, j, k, weight, _| {
            kernel.push(KernelVoxel {
                di: i,
                dj: j,
                dk: k,
                weight,
            });
        },
    )?;
    Ok(kernel)
}
#[derive(Debug, Clone, Copy)]
struct PeakCandidate {
    voxel: [i64; 3],
    value: f64,
}
impl VoiMask {
    pub fn peak(
        &self,
        ctx: Option<&dyn Cancellation>,
        value_at: Option<&mut VoxelValueFn<'_>>,
        opts: PeakOptions,
    ) -> (PeakResult, Option<Error>) {
        let m = self;
        let fail = |err: Error| (PeakResult::default(), Some(err));
        let Some(value_at) = value_at else {
            return fail(Error::wrap(
                ERR_INVALID_VOI,
                "peak needs a mask and a value accessor",
            ));
        };
        let t = match m.grid.transform() {
            Ok(t) => t,
            Err(err) => return fail(err),
        };
        let mut volume = opts.volume_mm3;
        if volume == 0.0 {
            volume = DEFAULT_PEAK_VOLUME_MM3;
        }
        if !finite_positive(volume) {
            return fail(Error::wrap(
                ERR_INVALID_VOI,
                format!(
                    "peak volume {} mm³ must be finite and positive",
                    float_v(opts.volume_mm3)
                ),
            ));
        }
        match opts.search {
            PeakSearch::VOI | PeakSearch::AT_MAXIMUM => {}
            PeakSearch::AROUND_MAXIMUM => {
                if !finite_positive(opts.search_radius_mm) {
                    return fail(Error::wrap(
                        ERR_INVALID_VOI,
                        format!(
                            "peak search radius {} mm must be finite and positive",
                            float_v(opts.search_radius_mm)
                        ),
                    ));
                }
            }
            _ => {
                return fail(Error::wrap(
                    ERR_INVALID_VOI,
                    format!("unknown peak search {}", opts.search.0),
                ));
            }
        }
        let weighting = if opts.centre_inside_kernel {
            VoiWeighting::CENTRE_INSIDE
        } else {
            VoiWeighting::PARTIAL_VOLUME
        };
        let supersampling = match (VoiRasterOptions {
            weighting,
            supersampling: opts.supersampling,
        })
        .resolved_supersampling()
        {
            Ok(n) => n,
            Err(err) => return fail(err),
        };
        let radius = sphere_radius_for_volume(volume);
        let kernel = match peak_kernel(ctx, &t, radius, weighting, supersampling) {
            Ok(k) => k,
            Err(err) => return fail(err),
        };
        if kernel.is_empty() {
            return fail(Error::wrap(
                ERR_INVALID_VOI,
                format!(
                    "the {} mm³ peak sphere contains no voxel centre; use partial-volume weights",
                    float_v(volume)
                ),
            ));
        }
        let mut kernel_weight = 0.0f64;
        for voxel in &kernel {
            kernel_weight += voxel.weight;
        }
        let voxel_volume = m.grid.voxel_volume_mm3();
        let mut result = PeakResult {
            radius_mm: radius,
            volume_mm3: volume,
            weighting,
            search: opts.search,
            kernel_voxels: kernel.len() as i64,
            kernel_volume_mm3: kernel_weight * voxel_volume,
            ..PeakResult::default()
        };
        let mut candidates: Vec<PeakCandidate> = Vec::with_capacity(m.voxels.len());
        let mut max_index: Option<usize> = None;
        for voxel in &m.voxels {
            if !voxel.centre_inside {
                continue;
            }
            let Some(v) = value_at(voxel.column, voxel.row, voxel.slice)
                .filter(|v| !v.is_nan() && !v.is_infinite())
            else {
                continue;
            };
            candidates.push(PeakCandidate {
                voxel: [voxel.column, voxel.row, voxel.slice],
                value: v,
            });
            if max_index.is_none_or(|at| v > candidates[at].value) {
                max_index = Some(candidates.len() - 1);
            }
        }
        let Some(max_index) = max_index else {
            return (result, Some(Error::sentinel(ERR_EMPTY_VOI)));
        };
        let maximum = candidates[max_index];
        result.maximum = maximum.value;
        result.maximum_voxel = maximum.voxel;
        let point = |voxel: [i64; 3]| -> Vec3 {
            m.grid
                .index_to_point(voxel[0] as f64, voxel[1] as f64, voxel[2] as f64)
        };
        let max_point = point(maximum.voxel);
        match opts.search {
            PeakSearch::AT_MAXIMUM => candidates = vec![maximum],
            PeakSearch::AROUND_MAXIMUM => {
                candidates
                    .retain(|c| point(c.voxel).sub(max_point).length() <= opts.search_radius_mm);
            }
            _ => {}
        }
        if (candidates.len() as u64).wrapping_mul(kernel.len() as u64) > MAX_PEAK_VOXEL_READS {
            return (
                result,
                Some(Error::wrap(
                    ERR_INVALID_VOI,
                    format!(
                        "{} candidate centres × {} sphere voxels exceed {} reads; search around the maximum",
                        candidates.len(),
                        kernel.len(),
                        MAX_PEAK_VOXEL_READS
                    ),
                )),
            );
        }
        let mut found = false;
        for (n, c) in candidates.iter().enumerate() {
            if n & 255 == 0
                && let Some(err) = ctx_err(ctx)
            {
                return fail(err);
            }
            result.candidates += 1;
            let (mut sum, mut weight, mut complete) = (0.0f64, 0.0f64, true);
            for kv in &kernel {
                let (i, j, k) = (
                    c.voxel[0].wrapping_add(kv.di),
                    c.voxel[1].wrapping_add(kv.dj),
                    c.voxel[2].wrapping_add(kv.dk),
                );
                if !t.in_grid(i, j, k) {
                    complete = false;
                    continue;
                }
                let Some(v) = value_at(i, j, k).filter(|v| !v.is_nan() && !v.is_infinite()) else {
                    complete = false;
                    continue;
                };
                sum += kv.weight * v;
                weight += kv.weight;
            }
            if !complete && !opts.allow_incomplete_sphere {
                result.skipped_incomplete += 1;
                continue;
            }
            if weight == 0.0 {
                result.skipped_incomplete += 1;
                continue;
            }
            let mean = sum / weight;
            if found && mean.partial_cmp(&result.mean) != Some(std::cmp::Ordering::Greater) {
                continue;
            }
            found = true;
            result.mean = mean;
            result.center_voxel = c.voxel;
            result.incomplete = !complete;
            result.covered_volume_mm3 = weight * voxel_volume;
        }
        if !found {
            return (result, Some(Error::sentinel(ERR_NO_PEAK)));
        }
        result.center = point(result.center_voxel);
        (result, None)
    }
}
