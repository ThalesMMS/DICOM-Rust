use super::demons::{compose_field, exponentiate, gaussian_kernel, new_field};
use super::displacement::{DisplacementField, DisplacementFieldTransform, sample_vectors};
use super::error::{RegError, Sentinel};
use super::image::Image;
use super::matrix::{finite, finite_vec};
use super::method::{CtxAdapter, Method, ctx_check};
use super::metric::MetricDomain;
use super::neighborhood::{NeighborhoodCorrelation, NeighborhoodCorrelationOptions, mask_at};
use super::transform::Transform;
use crate::fmath;
use crate::optimize::Context;
use crate::render::Vec3;
use crate::spatialreg::{self, InverseOptions, Kind as SpatialKind};
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SyNLevel {
    pub shrink: i64,
    pub iterations: i64,
    pub smoothing_sigma_mm: f64,
}
#[derive(Default)]
pub struct SyNOptions<'a> {
    pub levels: Vec<SyNLevel>,
    pub correlation: NeighborhoodCorrelationOptions,
    pub fixed_mask: Option<&'a Image>,
    pub moving_mask: Option<&'a Image>,
    pub learning_rate_mm: f64,
    pub update_sigma_mm: f64,
    pub regularization: f64,
    pub maximum_half_gradient: f64,
    pub minimum_improvement: f64,
    pub max_backtracks: i64,
    pub max_voxels: i64,
    pub max_working_bytes: i64,
    pub max_work: i64,
    pub inverse: InverseOptions,
    pub progress: Option<Box<dyn FnMut(SyNProgress) + 'a>>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyNProgress {
    pub level: i64,
    pub iteration: i64,
    pub value: f64,
    pub energy: f64,
    pub eligible_centers: i64,
    pub step_mm: f64,
    pub half_gradient_bound: f64,
    pub stop: String,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SyNResult {
    pub middle_to_fixed: Option<DisplacementField>,
    pub middle_to_moving: Option<DisplacementField>,
    pub fixed_to_moving: Option<DisplacementField>,
    pub moving_to_fixed: Option<DisplacementField>,
    pub forward_valid: Vec<bool>,
    pub inverse_valid: Vec<bool>,
    pub levels: Vec<SyNProgress>,
    pub estimated_working_bytes: i64,
    pub maximum_inverse_residual_mm: f64,
    pub forward_gradient_bound: f64,
    pub inverse_gradient_bound: f64,
    pub forward_certified_cells: i64,
    pub inverse_certified_cells: i64,
}
type Masks<'a> = (Option<&'a Image>, Option<&'a Image>);
type Progress<'a> = Option<Box<dyn FnMut(SyNProgress) + 'a>>;
fn bare(s: Sentinel) -> RegError {
    RegError::bare(s)
}
struct Prepared {
    levels: Vec<SyNLevel>,
    learning_rate_mm: f64,
    update_sigma_mm: f64,
    regularization: f64,
    maximum_half_gradient: f64,
    minimum_improvement: f64,
    max_backtracks: i64,
    inverse: InverseOptions,
}
pub fn syn(
    ctx: Context<'_>,
    fixed: Option<&Image>,
    moving: Option<&Image>,
    mut options: SyNOptions<'_>,
) -> Result<SyNResult, RegError> {
    ctx_check(ctx)?;
    let (o, estimated) = prepare_syn(ctx, fixed, moving, &options)?;
    let (Some(fixed), Some(moving)) = (fixed, moving) else {
        return Err(bare(Sentinel::SyN));
    };
    let adapter = CtxAdapter(ctx);
    let mut progress = options.progress.take();
    let mut hf: Option<DisplacementField> = None;
    let mut hm: Option<DisplacementField> = None;
    let mut result = SyNResult {
        estimated_working_bytes: estimated,
        ..SyNResult::default()
    };
    for (level, l) in o.levels.iter().enumerate() {
        ctx_check(ctx)?;
        let method = Method {
            fixed: Some(fixed),
            moving: Some(moving),
            ..Method::default()
        };
        let (sf, sm) = method.smooth(&adapter, l.smoothing_sigma_mm)?;
        let lf = sf.as_ref().unwrap_or(fixed);
        let lm = sm.as_ref().unwrap_or(moving);
        let factors = |im: &Image| [0, 1, 2].map(|a| l.shrink.min(im.size[a] as i64));
        let lf = lf.downsample(factors(lf))?;
        let lm = lm.downsample(factors(lm))?;
        let mut f = syn_refine(ctx, hf.as_ref(), &lf)?;
        let mut m = syn_refine(ctx, hm.as_ref(), &lf)?;
        let config = NeighborhoodCorrelation::new(
            Some(&lf),
            Some(&lm),
            MetricDomain::default(),
            options.correlation,
        )?;
        let last = syn_level(
            ctx,
            &lf,
            &lm,
            &mut f,
            &mut m,
            &config,
            level as i64,
            l.iterations,
            &o,
            (options.fixed_mask, options.moving_mask),
            &mut progress,
        )?;
        hf = Some(f);
        hm = Some(m);
        result.levels.push(last);
    }
    ctx_check(ctx)?;
    let (Some(hf), Some(hm)) = (hf, hm) else {
        return Err(bare(Sentinel::SyN));
    };
    let (forward, forward_valid, residual) =
        syn_compose_inverse(ctx, &hf, &hm, fixed, moving, o.inverse)?;
    let (inverse, inverse_valid, residual2) =
        syn_compose_inverse(ctx, &hm, &hf, moving, fixed, o.inverse)?;
    result.maximum_inverse_residual_mm = fmath::max(residual, residual2);
    let (fb, fc) = syn_gradient_certificate(ctx, &forward)?;
    let (ib, ic) = syn_gradient_certificate(ctx, &inverse)?;
    result.forward_gradient_bound = fb;
    result.forward_certified_cells = fc;
    result.inverse_gradient_bound = ib;
    result.inverse_certified_cells = ic;
    if fc == 0 || ic == 0 {
        return Err(bare(Sentinel::TooFewSamples));
    }
    if !finite(fb) || !finite(ib) || fb >= 1.0 || ib >= 1.0 {
        return Err(bare(Sentinel::SyNFolding));
    }
    ctx_check(ctx)?;
    result.middle_to_fixed = Some(hf);
    result.middle_to_moving = Some(hm);
    result.fixed_to_moving = Some(forward);
    result.moving_to_fixed = Some(inverse);
    result.forward_valid = forward_valid;
    result.inverse_valid = inverse_valid;
    Ok(result)
}
fn prepare_syn(
    ctx: Context<'_>,
    fixed: Option<&Image>,
    moving: Option<&Image>,
    o: &SyNOptions<'_>,
) -> Result<(Prepared, i64), RegError> {
    let syn_err = || bare(Sentinel::SyN);
    let limit = || bare(Sentinel::SyNResourceLimit);
    let (Some(fixed), Some(moving)) = (fixed, moving) else {
        return Err(syn_err());
    };
    if o.max_voxels < 0 || o.max_working_bytes < 0 || o.max_work < 0 || o.max_backtracks < 0 {
        return Err(syn_err());
    }
    let max_voxels = if o.max_voxels == 0 {
        1_000_000
    } else {
        o.max_voxels
    };
    let max_working_bytes = if o.max_working_bytes == 0 {
        256 << 20
    } else {
        o.max_working_bytes
    };
    let max_work = if o.max_work == 0 {
        10_000_000_000
    } else {
        o.max_work
    };
    let max_backtracks = if o.max_backtracks == 0 {
        12
    } else {
        o.max_backtracks
    };
    if max_backtracks > 20 || o.levels.len() > 16 {
        return Err(limit());
    }
    let levels = if o.levels.is_empty() {
        vec![SyNLevel {
            shrink: 1,
            iterations: 50,
            smoothing_sigma_mm: 0.0,
        }]
    } else {
        o.levels.clone()
    };
    let sp = fixed.spacing;
    let learning_rate_mm = if o.learning_rate_mm == 0.0 {
        0.5 * fmath::min(sp.x, fmath::min(sp.y, sp.z))
    } else {
        o.learning_rate_mm
    };
    let maximum_half_gradient = if o.maximum_half_gradient == 0.0 {
        0.3
    } else {
        o.maximum_half_gradient
    };
    let minimum_improvement = if o.minimum_improvement == 0.0 {
        1e-8
    } else {
        o.minimum_improvement
    };
    if !finite(learning_rate_mm)
        || learning_rate_mm <= 0.0
        || !finite(o.update_sigma_mm)
        || o.update_sigma_mm < 0.0
        || !finite(o.regularization)
        || o.regularization < 0.0
        || !finite(maximum_half_gradient)
        || maximum_half_gradient <= 0.0
        || maximum_half_gradient >= 1.0
        || !finite(minimum_improvement)
        || minimum_improvement < 0.0
    {
        return Err(syn_err());
    }
    let mut inverse = o.inverse;
    if inverse.maximum_iterations < 0
        || !finite(inverse.tolerance)
        || inverse.tolerance < 0.0
        || !finite(inverse.max_residual)
        || inverse.max_residual < 0.0
    {
        return Err(syn_err());
    }
    if inverse.maximum_iterations == 0 {
        inverse.maximum_iterations = 50;
    }
    if inverse.maximum_iterations > 1000 {
        return Err(limit());
    }
    if inverse.tolerance == 0.0 {
        inverse.tolerance = 1e-5;
    }
    if inverse.max_residual == 0.0 {
        inverse.max_residual = 0.001;
    }
    if inverse.tolerance > inverse.max_residual {
        return Err(syn_err());
    }
    for s in [sp.x, sp.y, sp.z] {
        let r = o.update_sigma_mm / s;
        if !finite(r) || r > 10.0 {
            return Err(limit());
        }
    }
    let mut biggest = 0i64;
    for im in [Some(fixed), Some(moving), o.fixed_mask, o.moving_mask]
        .into_iter()
        .flatten()
    {
        if im.data.len() as i64 > max_voxels {
            return Err(limit());
        }
        if !im.direction.finite() {
            return Err(syn_err());
        }
        let gram = im.direction.transpose().mul(&im.direction);
        for r in 0..3 {
            for c in 0..3 {
                let want = if r == c { 1.0 } else { 0.0 };
                if (gram.0[r][c] - want).abs() > 1e-8 {
                    return Err(syn_err());
                }
            }
        }
        if im.direction.determinant() <= 0.0 {
            return Err(syn_err());
        }
        biggest = biggest.max(im.data.len() as i64);
        for (i, &v) in im.data.iter().enumerate() {
            if i % 4096 == 0 {
                ctx_check(ctx)?;
            }
            if !finite(f64::from(v)) {
                return Err(syn_err());
            }
        }
    }
    let config = NeighborhoodCorrelation::new(
        Some(fixed),
        Some(moving),
        MetricDomain::default(),
        o.correlation,
    )?;
    if biggest > max_working_bytes / 640 {
        return Err(limit());
    }
    let mut estimated = biggest * 640;
    if config.neighborhood > (max_working_bytes - estimated) / 80 {
        return Err(limit());
    }
    estimated += config.neighborhood * 80;
    let mut work = 0i64;
    let mut previous = i64::MAX;
    for (i, l) in levels.iter().enumerate() {
        if l.shrink < 1
            || l.shrink > previous
            || l.iterations < 1
            || l.iterations > 500
            || !finite(l.smoothing_sigma_mm)
            || l.smoothing_sigma_mm < 0.0
        {
            return Err(syn_err());
        }
        previous = l.shrink;
        if i == levels.len() - 1 && l.shrink != 1 {
            return Err(syn_err());
        }
        let mut count = 1i64;
        for &size in &fixed.size {
            let size = size as i64;
            let f = l.shrink.min(size);
            count *= (size + f - 1) / f;
        }
        let charge =
            config.neighborhood * (max_backtracks + 2) + 512 + inverse.maximum_iterations * 64;
        if count > max_work / charge || count * charge > (max_work - work) / l.iterations {
            return Err(limit());
        }
        work += count * charge * l.iterations;
    }
    Ok((
        Prepared {
            levels,
            learning_rate_mm,
            update_sigma_mm: o.update_sigma_mm,
            regularization: o.regularization,
            maximum_half_gradient,
            minimum_improvement,
            max_backtracks,
            inverse,
        },
        estimated,
    ))
}
fn syn_refine(
    ctx: Context<'_>,
    previous: Option<&DisplacementField>,
    grid: &Image,
) -> Result<DisplacementField, RegError> {
    let mut next = new_field(grid);
    let Some(previous) = previous else {
        return Ok(next);
    };
    let g = previous.grid()?;
    let psize = previous.size.map(|n| n as usize);
    for i in 0..next.vectors.len() {
        if i % 512 == 0 {
            ctx_check(ctx)?;
        }
        let p = syn_point(grid, i);
        next.vectors[i] = sample_vectors(&previous.vectors, psize, g.continuous_index(p));
    }
    syn_zero_boundary(&mut next.vectors, next.size);
    Ok(next)
}
fn syn_point(grid: &Image, i: usize) -> Vec3 {
    let s = grid.size;
    grid.point(Vec3::new(
        (i % s[0]) as f64,
        (i / s[0] % s[1]) as f64,
        (i / (s[0] * s[1])) as f64,
    ))
}
#[derive(Default)]
struct Warped {
    fixed: Vec<f64>,
    moving: Vec<f64>,
    valid: Vec<bool>,
    fixed_gradient: Vec<Vec3>,
    moving_gradient: Vec<Vec3>,
}
fn field_transform(field: &DisplacementField, grid: &Image) -> DisplacementFieldTransform {
    DisplacementFieldTransform::from_parts(field.clone(), grid.clone())
}
#[allow(clippy::too_many_arguments)]
fn syn_warp(
    ctx: Context<'_>,
    fixed: &Image,
    moving: &Image,
    hf: &DisplacementField,
    hm: &DisplacementField,
    masks: Masks<'_>,
    gradients: bool,
) -> Result<Warped, RegError> {
    let grid = hf.grid().unwrap_or_default();
    let n = hf.vectors.len();
    let mut w = Warped {
        fixed: vec![0.0; n],
        moving: vec![0.0; n],
        valid: vec![false; n],
        ..Warped::default()
    };
    let mut transforms = None;
    if gradients {
        w.fixed_gradient = vec![Vec3::ZERO; n];
        w.moving_gradient = vec![Vec3::ZERO; n];
        transforms = Some((field_transform(hf, &grid), field_transform(hm, &grid)));
    }
    for i in 0..n {
        if i % 256 == 0 {
            ctx_check(ctx)?;
        }
        let p = syn_point(&grid, i);
        let (pf, pm) = (p.add(hf.vectors[i]), p.add(hm.vectors[i]));
        let (fi, mi) = (fixed.continuous_index(pf), moving.continuous_index(pm));
        if !fixed.inside(fi) || !moving.inside(mi) {
            continue;
        }
        let fv = mask_at(masks.0, pf)?;
        let mv = mask_at(masks.1, pm)?;
        if !fv || !mv {
            continue;
        }
        w.fixed[i] = fixed.linear_at(fi);
        w.moving[i] = moving.linear_at(mi);
        w.valid[i] = true;
        if let Some((tf, tm)) = &transforms {
            w.fixed_gradient[i] = tf
                .jacobian_with_respect_to_position(p)
                .transpose()
                .mul_vec(fixed.gradient_at(fi));
            w.moving_gradient[i] = tm
                .jacobian_with_respect_to_position(p)
                .transpose()
                .mul_vec(moving.gradient_at(mi));
            if !finite_vec(w.fixed_gradient[i]) || !finite_vec(w.moving_gradient[i]) {
                return Err(bare(Sentinel::SyN));
            }
        }
    }
    Ok(w)
}
type Correlation = (f64, Vec<f64>, Vec<f64>, i64);
fn syn_correlation(
    ctx: Context<'_>,
    w: &Warped,
    size: [i64; 3],
    radius: [i64; 3],
    variance: f64,
    derivatives: bool,
) -> Result<Correlation, RegError> {
    let n_all = w.fixed.len();
    let (mut df, mut dm) = if derivatives {
        (vec![0.0; n_all], vec![0.0; n_all])
    } else {
        (Vec::new(), Vec::new())
    };
    let nhood = (2 * radius[0] + 1) * (2 * radius[1] + 1) * (2 * radius[2] + 1);
    let mut indices: Vec<usize> = Vec::with_capacity(nhood.max(0) as usize);
    let mut sum = 0.0;
    let mut eligible = 0i64;
    for i in 0..n_all {
        ctx_check(ctx)?;
        if !w.valid[i] {
            continue;
        }
        let i64i = i as i64;
        let (x, y, z) = (
            i64i % size[0],
            i64i / size[0] % size[1],
            i64i / (size[0] * size[1]),
        );
        indices.clear();
        let (mut mean_f, mut mean_m) = (0.0, 0.0);
        for k in (z - radius[2]).max(0)..=(size[2] - 1).min(z + radius[2]) {
            for j in (y - radius[1]).max(0)..=(size[1] - 1).min(y + radius[1]) {
                for h in (x - radius[0]).max(0)..=(size[0] - 1).min(x + radius[0]) {
                    let at = ((k * size[1] + j) * size[0] + h) as usize;
                    if !w.valid[at] {
                        continue;
                    }
                    indices.push(at);
                    let n = indices.len() as f64;
                    mean_f += (w.fixed[at] - mean_f) / n;
                    mean_m += (w.moving[at] - mean_m) / n;
                }
            }
        }
        if indices.len() < 2 {
            continue;
        }
        let (mut ff, mut mm, mut fm) = (0.0, 0.0, 0.0);
        for &at in &indices {
            let (f, m) = (w.fixed[at] - mean_f, w.moving[at] - mean_m);
            ff += f * f;
            mm += m * m;
            fm += f * m;
        }
        let n = indices.len() as f64;
        if ff / n <= variance || mm / n <= variance {
            continue;
        }
        let rho = (fm / ff.sqrt()) / mm.sqrt();
        sum -= rho * rho;
        eligible += 1;
        if derivatives {
            let (sf, sm) = (-2.0 * (fm / mm) / ff, -2.0 * (fm / ff) / mm);
            for &at in &indices {
                let (f, m) = (w.fixed[at] - mean_f, w.moving[at] - mean_m);
                df[at] += sf * (m - fm / ff * f);
                dm[at] += sm * (f - fm / mm * m);
            }
        }
    }
    if eligible == 0 {
        return Err(bare(Sentinel::TooFewSamples));
    }
    for v in df.iter_mut().chain(dm.iter_mut()) {
        *v /= eligible as f64;
    }
    Ok((sum / eligible as f64, df, dm, eligible))
}
#[allow(clippy::too_many_arguments)]
fn syn_level(
    ctx: Context<'_>,
    fixed: &Image,
    moving: &Image,
    hf: &mut DisplacementField,
    hm: &mut DisplacementField,
    config: &NeighborhoodCorrelation<'_>,
    level: i64,
    iterations: i64,
    o: &Prepared,
    masks: Masks<'_>,
    progress: &mut Progress<'_>,
) -> Result<SyNProgress, RegError> {
    let mut last = SyNProgress {
        level,
        iteration: -1,
        ..SyNProgress::default()
    };
    let variance = config.options.minimum_variance;
    for iteration in 0..iterations {
        let w = syn_warp(ctx, fixed, moving, hf, hm, masks, true)?;
        let (value, df, dm, centers) =
            syn_correlation(ctx, &w, hf.size, config.radius, variance, true)?;
        let (pf, gf) = syn_membrane(ctx, hf, true)?;
        let (pm, gm) = syn_membrane(ctx, hm, true)?;
        let energy = value + o.regularization * (pf + pm);
        let grid = hf.grid().unwrap_or_default();
        let (tf, tm) = (field_transform(hf, &grid), field_transform(hm, &grid));
        let mut uf = vec![Vec3::ZERO; df.len()];
        let mut um = vec![Vec3::ZERO; dm.len()];
        for i in 0..uf.len() {
            let p = syn_point(&grid, i);
            uf[i] = w.fixed_gradient[i].scale(-df[i]).sub(
                tf.jacobian_with_respect_to_position(p)
                    .transpose()
                    .mul_vec(gf[i])
                    .scale(o.regularization),
            );
            um[i] = w.moving_gradient[i].scale(-dm[i]).sub(
                tm.jacobian_with_respect_to_position(p)
                    .transpose()
                    .mul_vec(gm[i])
                    .scale(o.regularization),
            );
        }
        syn_smooth(ctx, &mut uf, hf.size, hf.spacing, o.update_sigma_mm)?;
        syn_smooth(ctx, &mut um, hm.size, hm.spacing, o.update_sigma_mm)?;
        syn_zero_boundary(&mut uf, hf.size);
        syn_zero_boundary(&mut um, hm.size);
        let mut largest = 0.0;
        for i in 0..uf.len() {
            largest = fmath::max(largest, fmath::max(uf[i].length(), um[i].length()));
        }
        if !finite(largest) || !finite(energy) {
            return Err(bare(Sentinel::SyN));
        }
        last = SyNProgress {
            level,
            iteration,
            value,
            energy,
            eligible_centers: centers,
            stop: "stationary".to_string(),
            ..SyNProgress::default()
        };
        if largest < 1e-15 {
            if let Some(cb) = progress.as_mut() {
                cb(last.clone());
            }
            ctx_check(ctx)?;
            return Ok(last);
        }
        let mut accepted = false;
        for attempt in 0..o.max_backtracks {
            ctx_check(ctx)?;
            let step = o.learning_rate_mm / fmath::pow(2.0, attempt as f64);
            let sf: Vec<Vec3> = uf.iter().map(|u| u.scale(step / largest)).collect();
            let sm: Vec<Vec3> = um.iter().map(|u| u.scale(step / largest)).collect();
            let mut cf = hf.clone();
            let mut cm = hm.clone();
            let ef = exponentiate(&sf, fixed);
            compose_field(&mut cf, &ef, fixed);
            ctx_check(ctx)?;
            let em = exponentiate(&sm, fixed);
            compose_field(&mut cm, &em, fixed);
            for i in 0..cf.vectors.len() {
                if !finite_vec(cf.vectors[i]) || !finite_vec(cm.vectors[i]) {
                    return Err(bare(Sentinel::SyN));
                }
            }
            let (bf, _) = syn_gradient_certificate(ctx, &cf)?;
            let (bm, _) = syn_gradient_certificate(ctx, &cm)?;
            let bound = fmath::max(bf, bm);
            if !finite(bound) || bound >= o.maximum_half_gradient {
                continue;
            }
            let candidate = syn_warp(ctx, fixed, moving, &cf, &cm, masks, false)?;
            let (next, _, _, _) =
                syn_correlation(ctx, &candidate, cf.size, config.radius, variance, false)?;
            let (pf, _) = syn_membrane(ctx, &cf, false)?;
            let (pm, _) = syn_membrane(ctx, &cm, false)?;
            let next_energy = next + o.regularization * (pf + pm);
            if !finite(next_energy) || next_energy >= energy - o.minimum_improvement {
                continue;
            }
            hf.vectors = cf.vectors;
            hm.vectors = cm.vectors;
            last.value = next;
            last.step_mm = step;
            last.half_gradient_bound = bound;
            last.stop = "iteration-limit".to_string();
            last.energy = next_energy;
            accepted = true;
            break;
        }
        if !accepted {
            last.stop = "no-admissible-descent".to_string();
        }
        if let Some(cb) = progress.as_mut() {
            cb(last.clone());
        }
        ctx_check(ctx)?;
        if !accepted {
            return Ok(last);
        }
    }
    Ok(last)
}
fn syn_membrane(
    ctx: Context<'_>,
    field: &DisplacementField,
    derivatives: bool,
) -> Result<(f64, Vec<Vec3>), RegError> {
    let mut gradient = if derivatives {
        vec![Vec3::ZERO; field.vectors.len()]
    } else {
        Vec::new()
    };
    let s = field.size.map(|n| n as usize);
    let strides = [1, s[0], s[0] * s[1]];
    let sp = [field.spacing.x, field.spacing.y, field.spacing.z];
    let mut energy = 0.0;
    let mut edges = 0i64;
    for (i, &v) in field.vectors.iter().enumerate() {
        if i % 512 == 0 {
            ctx_check(ctx)?;
        }
        for (a, &stride) in strides.iter().enumerate() {
            if i / stride % s[a] == s[a] - 1 {
                continue;
            }
            let j = i + stride;
            let delta = field.vectors[j].sub(v);
            let weight = 1.0 / (sp[a] * sp[a]);
            energy += delta.dot(delta) * weight;
            edges += 1;
            if derivatives {
                let g = delta.scale(2.0 * weight);
                gradient[i] = gradient[i].sub(g);
                gradient[j] = gradient[j].add(g);
            }
        }
    }
    if edges == 0 {
        return Ok((0.0, gradient));
    }
    for g in gradient.iter_mut() {
        *g = g.scale(1.0 / edges as f64);
    }
    Ok((energy / edges as f64, gradient))
}
fn syn_zero_boundary(v: &mut [Vec3], size: [i64; 3]) {
    let s = size.map(|n| n as usize);
    for (i, u) in v.iter_mut().enumerate() {
        let (x, y, z) = (i % s[0], i / s[0] % s[1], i / (s[0] * s[1]));
        if s[0] > 1 && (x == 0 || x == s[0] - 1)
            || s[1] > 1 && (y == 0 || y == s[1] - 1)
            || s[2] > 1 && (z == 0 || z == s[2] - 1)
        {
            *u = Vec3::ZERO;
        }
    }
}
fn syn_smooth(
    ctx: Context<'_>,
    v: &mut [Vec3],
    size: [i64; 3],
    spacing: Vec3,
    sigma: f64,
) -> Result<(), RegError> {
    if sigma == 0.0 {
        return ctx_check(ctx);
    }
    let size = size.map(|n| n as usize);
    let mut scratch = vec![Vec3::ZERO; v.len()];
    let sp = [spacing.x, spacing.y, spacing.z];
    let strides = [1, size[0], size[0] * size[1]];
    for axis in 0..3 {
        if size[axis] < 2 {
            continue;
        }
        let kernel = gaussian_kernel(sigma / sp[axis], 0.001, 16);
        let radius = (kernel.len() / 2) as i64;
        for i in 0..v.len() {
            if i % 256 == 0 {
                ctx_check(ctx)?;
            }
            let at = i / strides[axis] % size[axis];
            let start = i - at * strides[axis];
            let mut sum = Vec3::ZERO;
            for (k, &weight) in kernel.iter().enumerate() {
                let j = (at as i64 + k as i64 - radius).clamp(0, size[axis] as i64 - 1) as usize;
                sum = sum.add(v[start + j * strides[axis]].scale(weight));
            }
            scratch[i] = sum;
        }
        v.copy_from_slice(&scratch);
    }
    ctx_check(ctx)
}
fn syn_gradient_certificate(
    ctx: Context<'_>,
    field: &DisplacementField,
) -> Result<(f64, i64), RegError> {
    let s = field.size.map(|n| n as usize);
    let sp = [field.spacing.x, field.spacing.y, field.spacing.z];
    let mut largest = 0.0;
    let mut cells = 0i64;
    let inverse = field.direction.inverse()?;
    let gram = inverse.transpose().mul(&inverse);
    let mut direction_bound = 0.0;
    for row in &gram.0 {
        let mut sum = 0.0;
        for v in row {
            sum += v.abs();
        }
        direction_bound = fmath::max(direction_bound, sum);
    }
    let direction_bound = f64::sqrt(direction_bound) * (1.0 + 1e-12);
    let at = |x: usize, y: usize, z: usize| field.vectors[(z * s[1] + y) * s[0] + x];
    let span = |a: usize| 1usize.min(s[a].saturating_sub(1));
    for z in 0..1.max(s[2] as i64 - 1) as usize {
        for y in 0..1.max(s[1] as i64 - 1) as usize {
            for x in 0..1.max(s[0] as i64 - 1) as usize {
                if x % 128 == 0 {
                    ctx_check(ctx)?;
                }
                let mut valid = true;
                for dz in 0..=span(2) {
                    for dy in 0..=span(1) {
                        for dx in 0..=span(0) {
                            if !finite_vec(at(x + dx, y + dy, z + dz)) {
                                valid = false;
                            }
                        }
                    }
                }
                if !valid {
                    continue;
                }
                cells += 1;
                let mut bounds = [[0f64; 3]; 3];
                let origin = [x, y, z];
                for dz in 0..=span(2) {
                    for dy in 0..=span(1) {
                        for dx in 0..=span(0) {
                            let c = [x + dx, y + dy, z + dz];
                            let base = at(c[0], c[1], c[2]);
                            for a in 0..3 {
                                if s[a] < 2 || c[a] != origin[a] {
                                    continue;
                                }
                                let mut hi = c;
                                hi[a] += 1;
                                let d = at(hi[0], hi[1], hi[2]).sub(base).scale(1.0 / sp[a]);
                                for (comp, v) in [d.x, d.y, d.z].into_iter().enumerate() {
                                    bounds[comp][a] = fmath::max(bounds[comp][a], v.abs());
                                }
                            }
                        }
                    }
                }
                let mut sum = 0.0;
                for row in &bounds {
                    for b in row {
                        sum += b * b;
                    }
                }
                largest = fmath::max(largest, f64::sqrt(sum));
            }
        }
    }
    ctx_check(ctx)?;
    Ok((largest * direction_bound, cells))
}
type Composed = (DisplacementField, Vec<bool>, f64);
fn syn_compose_inverse(
    ctx: Context<'_>,
    half: &DisplacementField,
    other: &DisplacementField,
    source: &Image,
    target: &Image,
    options: InverseOptions,
) -> Result<Composed, RegError> {
    let g = half.spatial_grid()?;
    let h = other.spatial_grid()?;
    let r = spatialreg::Registration {
        pre: spatialreg::Matrix::identity(),
        post: spatialreg::Matrix::identity(),
        grid: Some(g),
        ..spatialreg::Registration::default()
    };
    let mut field = new_field(source);
    let mut valid = vec![false; field.vectors.len()];
    let payload_nan = f64::from_bits(0x7FF8_0000_0000_0001);
    let nan = Vec3::new(payload_nan, payload_nan, payload_nan);
    let mut maximum = 0.0;
    for i in 0..field.vectors.len() {
        if i % 64 == 0 {
            ctx_check(ctx)?;
        }
        let p = syn_point(source, i);
        let (inverse, err) = r.invert_point(p, options);
        if let Some(e) = err {
            if !e.is(SpatialKind::OutsideDomain) && !e.is(SpatialKind::InverseNotConverged) {
                return Err(e.into());
            }
            field.vectors[i] = nan;
            continue;
        }
        let sampled = h.sample(inverse.registered);
        let mapped = inverse
            .registered
            .add(*sampled.as_ref().unwrap_or(&Vec3::ZERO));
        if sampled.is_err() || !target.inside(target.continuous_index(mapped)) {
            field.vectors[i] = nan;
            continue;
        }
        field.vectors[i] = mapped.sub(p);
        valid[i] = true;
        maximum = fmath::max(maximum, inverse.residual);
    }
    if !finite(maximum) {
        return Err(RegError::new(Sentinel::SyN, "inverse residual"));
    }
    if !valid.iter().any(|&v| v) {
        return Err(bare(Sentinel::TooFewSamples));
    }
    ctx_check(ctx)?;
    Ok((field, valid, maximum))
}
