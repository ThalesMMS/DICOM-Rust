use super::distance3d::VolumeMask;
use super::shape_intensity::{
    IntensityAccumulator, IntensityMoments, IntensityOrder, IntensityStatistics,
};
use super::texture::{TextureFeatures, TextureOptions};
use super::voi::VoxelValueFn;
use super::{Cancellation, Error, ctx_err};
use std::sync::Arc;
pub type IntensityEmit<'a> = dyn FnMut(f64) -> Result<(), Error> + 'a;
pub fn intensity_moments_visit<V>(
    ctx: Option<&dyn Cancellation>,
    visit: Option<V>,
) -> (IntensityMoments, Option<Error>)
where
    V: FnOnce(&mut IntensityEmit<'_>) -> Result<(), Error>,
{
    let mut a = IntensityAccumulator::default();
    if let Some(err) = ctx_err(ctx) {
        return (a.moments(), Some(err));
    }
    let Some(visit) = visit else {
        return (a.moments(), Some(Error::msg("roi: nil intensity visitor")));
    };
    let mut err = visit(&mut |v: f64| {
        if let Some(err) = ctx_err(ctx) {
            return Err(err);
        }
        a.add(v);
        Ok(())
    })
    .err();
    if err.is_none() {
        err = ctx_err(ctx);
    }
    (a.moments(), err)
}
pub fn intensity_statistics_visit<V>(
    ctx: Option<&dyn Cancellation>,
    maximum: i64,
    max_bytes: i64,
    visit: Option<V>,
) -> Result<IntensityStatistics, Error>
where
    V: FnOnce(&mut IntensityEmit<'_>) -> Result<(), Error>,
{
    let Some(visit) = visit.filter(|_| maximum >= 0 && max_bytes >= 0 && maximum <= max_bytes / 8)
    else {
        return Err(Error::msg(
            "roi: intensity sample limit exceeded or invalid visitor",
        ));
    };
    if let Some(err) = ctx_err(ctx) {
        return Err(err);
    }
    let mut values: Vec<f64> = Vec::new();
    let mut a = IntensityAccumulator::default();
    let mut refusal: Option<Error> = None;
    let mut err = visit(&mut |v: f64| {
        if let Some(r) = &refusal {
            return Err(r.clone());
        }
        if let Some(e) = ctx_err(ctx) {
            refusal = Some(e.clone());
            return Err(e);
        }
        if v.is_finite() {
            if values.len() as i64 == maximum {
                let e = Error::msg("roi: intensity sample limit exceeded");
                refusal = Some(e.clone());
                return Err(e);
            }
            values.push(v);
        }
        a.add(v);
        Ok(())
    })
    .err();
    if err.is_none() {
        err = refusal;
    }
    if err.is_none() {
        err = ctx_err(ctx);
    }
    if let Some(err) = err {
        return Err(err);
    }
    cancellable_intensity_sort(ctx, &mut values)?;
    let m = a.moments();
    let mut s = IntensityStatistics {
        label: 0,
        count: m.count,
        non_finite: m.non_finite,
        min: m.min,
        max: m.max,
        sum: m.sum,
        mean: m.mean,
        variance: m.variance,
        standard_deviation: m.standard_deviation,
        median: f64::NAN,
        order: Some(Arc::new(IntensityOrder::new_sorted(values, maximum))),
    };
    if let Some(median) = s.percentile(50.0) {
        s.median = median;
    }
    Ok(s)
}
fn cancellable_intensity_sort(ctx: Option<&dyn Cancellation>, v: &mut [f64]) -> Result<(), Error> {
    let mut steps = 0u64;
    let mut sift = |v: &mut [f64], mut root: usize, end: usize| -> Result<(), Error> {
        while root * 2 + 1 < end {
            steps += 1;
            if steps.is_multiple_of(1024)
                && let Some(err) = ctx_err(ctx)
            {
                return Err(err);
            }
            let mut child = root * 2 + 1;
            if child + 1 < end && v[child] < v[child + 1] {
                child += 1;
            }
            if v[root] >= v[child] {
                break;
            }
            v.swap(root, child);
            root = child;
        }
        Ok(())
    };
    let n = v.len();
    for start in (0..n / 2).rev() {
        sift(v, start, n)?;
    }
    for end in (1..n).rev() {
        v.swap(0, end);
        sift(v, 0, end)?;
    }
    match ctx_err(ctx) {
        Some(err) => Err(err),
        None => Ok(()),
    }
}
impl IntensityStatistics {
    pub fn sample_storage_bytes(&self) -> i64 {
        match &self.order {
            Some(order) => order.capacity().wrapping_mul(8),
            None => 0,
        }
    }
}
impl VolumeMask {
    pub fn summary(&self, ctx: Option<&dyn Cancellation>) -> Result<(i64, f64), Error> {
        self.validate_context(ctx)?;
        let mut count = 0i64;
        let mut volume = 0.0f64;
        for (i, &on) in self.voxels.iter().enumerate() {
            if i % 4096 == 0
                && let Some(err) = ctx_err(ctx)
            {
                return Err(err);
            }
            if on {
                count += 1;
                volume += self.voxel_volume(i);
            }
        }
        Ok((count, volume))
    }
    pub fn shape_workspace_bytes(&self, ctx: Option<&dyn Cancellation>) -> Result<i64, Error> {
        self.validate_context(ctx)?;
        let columns = self.columns as usize;
        let mut runs = 0u64;
        let mut previous = false;
        for (i, &on) in self.voxels.iter().enumerate() {
            if i % 4096 == 0
                && let Some(err) = ctx_err(ctx)
            {
                return Err(err);
            }
            if on && (!previous || i % columns == 0) {
                runs += 1;
            }
            previous = on;
        }
        const MAX: u64 = i64::MAX as u64;
        let mut total = 16384u64;
        let mut add = |value: u64| -> bool {
            if value > MAX - total {
                return false;
            }
            total += value;
            true
        };
        let mut mul = |a: u64, b: u64, c: u64| -> bool {
            if a != 0 && b > MAX / a {
                return false;
            }
            let v = a.wrapping_mul(b);
            if c != 0 && v > MAX / c {
                return false;
            }
            add(v.wrapping_mul(c))
        };
        let (nx, ny, nz) = (self.columns as u64, self.rows as u64, self.slices as u64);
        if !mul(nx, nz, 8)
            || !mul(nx, ny, 8)
            || !mul(ny, nz, 8)
            || !mul(runs, 1, 1024)
            || !mul(nx + ny + nz + 3, 1, 64)
        {
            return Err(Error::msg("roi: shape workspace size overflow"));
        }
        Ok(total as i64)
    }
}
impl VolumeMask {
    pub fn texture_features_bounded(
        &self,
        ctx: Option<&dyn Cancellation>,
        value_at: Option<&mut VoxelValueFn<'_>>,
        opts: TextureOptions,
        max_voxels: i64,
        max_bytes: i64,
    ) -> (TextureFeatures, Option<Error>) {
        if let Err(err) = self.validate_context(ctx) {
            return (TextureFeatures::default(), Some(err));
        }
        let opts = match opts.normalized() {
            Ok(o) => o,
            Err(err) => return (TextureFeatures::default(), Some(err)),
        };
        let n = self.count();
        let run = self.columns.max(self.rows).max(self.slices);
        let levels = opts.max_grey_levels as f64;
        let bytes = n as f64 * 128.0
            + self.voxels.len() as f64 * 4.0
            + 112.0 * (levels * levels + levels * run as f64)
            + 4096.0 * opts.max_grey_levels.wrapping_add(run).wrapping_add(1) as f64;
        if max_voxels <= 0 || n > max_voxels || max_bytes <= 0 || bytes > max_bytes as f64 {
            return (
                TextureFeatures::default(),
                Some(Error::msg("roi: texture workspace limit exceeded")),
            );
        }
        if let Some(err) = ctx_err(ctx) {
            return (TextureFeatures::default(), Some(err));
        }
        self.texture_features(ctx, value_at, opts)
    }
}
