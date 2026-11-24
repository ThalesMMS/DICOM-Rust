use super::volume::{Ctx, ErrorKind, FilterError, Volume, ctx_err, voxel_count};
use crate::fmath;
use sha2::{Digest, Sha256};
fn input_err() -> FilterError {
    FilterError::new(
        ErrorKind::PSFDeconvolutionInput,
        "imagefilter: invalid PSF deconvolution input",
    )
}
fn input_detail(detail: &str) -> FilterError {
    FilterError::new(
        ErrorKind::PSFDeconvolutionInput,
        format!("imagefilter: invalid PSF deconvolution input: {detail}"),
    )
}
fn limit_err() -> FilterError {
    FilterError::new(
        ErrorKind::PSFDeconvolutionLimit,
        "imagefilter: PSF deconvolution resource limit",
    )
}
fn numeric_err() -> FilterError {
    FilterError::new(
        ErrorKind::PSFDeconvolutionNumeric,
        "imagefilter: unsupported or nonfinite PSF restoration arithmetic",
    )
}
pub const PSF_ZERO_BOUNDARY: u8 = 0;
pub const PSF_CIRCULAR_BOUNDARY: u8 = 1;
pub const PSF_REQUIRE_UNIT_MASS: u8 = 0;
pub const PSF_NORMALIZE_MASS: u8 = 1;
pub const PSF_REJECT_NEGATIVE: u8 = 0;
pub const PSF_CLAMP_NEGATIVE: u8 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PSFDeconvolutionLimits {
    pub max_voxels: i64,
    pub max_psf_samples: i64,
    pub max_scratch_bytes: i64,
    pub max_working_bytes: i64,
    pub max_kernel_visits: i64,
    pub max_iterations: i64,
    pub max_source_references: i64,
    pub max_reference_bytes: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PSFDeconvolutionOptions {
    pub iterations: i64,
    pub relative_change: f64,
    pub epsilon: f64,
    pub boundary: u8,
    pub normalization: u8,
    pub negative_policy: u8,
    pub limits: PSFDeconvolutionLimits,
    pub source_references: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PSFDerivedMetadata {
    pub derived: bool,
    pub method: String,
    pub noise_model: String,
    pub stopping_reason: String,
    pub psf_sha256: String,
    pub source_references: Vec<String>,
    pub iterations_requested: i64,
    pub iterations_completed: i64,
    pub negative_clamped: i64,
    pub boundary: u8,
    pub normalization: u8,
    pub negative_policy: u8,
    pub psf_dims: [i64; 3],
    pub spacing_mm: [f64; 3],
    pub original_psf_mass: f64,
    pub epsilon: f64,
    pub relative_change: f64,
    pub intensity_scale: f64,
    pub scratch_bytes: i64,
    pub working_bytes: i64,
    pub fft_working_bytes: i64,
    pub kernel_visit_bound: i64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PSFDeconvolutionResult {
    pub restored: Volume,
    pub metadata: PSFDerivedMetadata,
    pub initial_rmse: f64,
    pub final_rmse: f64,
    pub initial_poisson_deviance: f64,
    pub final_poisson_deviance: f64,
}
fn finite(x: f64) -> bool {
    x.is_finite()
}
fn psf_options(o: &PSFDeconvolutionOptions) -> Result<PSFDeconvolutionOptions, FilterError> {
    let mut o = o.clone();
    let l = &mut o.limits;
    if o.iterations < 0
        || !finite(o.relative_change)
        || o.relative_change < 0.0
        || !finite(o.epsilon)
        || o.epsilon < 0.0
        || o.epsilon > 1.0
        || o.boundary > PSF_CIRCULAR_BOUNDARY
        || o.normalization > PSF_NORMALIZE_MASS
        || o.negative_policy > PSF_CLAMP_NEGATIVE
        || l.max_voxels < 0
        || l.max_psf_samples < 0
        || l.max_scratch_bytes < 0
        || l.max_working_bytes < 0
        || l.max_kernel_visits < 0
        || l.max_iterations < 0
        || l.max_source_references < 0
        || l.max_reference_bytes < 0
    {
        return Err(input_err());
    }
    if o.iterations == 0 {
        o.iterations = 20;
    }
    if o.epsilon == 0.0 {
        o.epsilon = 1e-12;
    }
    let l = &mut o.limits;
    for (v, def) in [
        (&mut l.max_voxels, 1i64 << 20),
        (&mut l.max_psf_samples, 4096),
        (&mut l.max_scratch_bytes, 64 << 20),
        (&mut l.max_working_bytes, 96 << 20),
        (&mut l.max_kernel_visits, 100 << 20),
        (&mut l.max_iterations, 128),
        (&mut l.max_source_references, 32),
        (&mut l.max_reference_bytes, 4096),
    ] {
        if *v == 0 {
            *v = def;
        }
    }
    if l.max_voxels > 16 << 20
        || l.max_psf_samples > 65536
        || l.max_iterations > 4096
        || l.max_source_references > 1024
        || l.max_reference_bytes > 1 << 20
    {
        return Err(limit_err());
    }
    if o.iterations > o.limits.max_iterations {
        return Err(limit_err());
    }
    Ok(o)
}
struct Prepared {
    y: Vec<f64>,
    k: Vec<f64>,
    meta: PSFDerivedMetadata,
    opts: PSFDeconvolutionOptions,
}
fn prepare(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    psf: Option<&Volume>,
    o: &PSFDeconvolutionOptions,
    restore: bool,
) -> Result<Prepared, FilterError> {
    if ctx.is_none() {
        return Err(input_err());
    }
    ctx_err(ctx)?;
    let opts = psf_options(o)?;
    let (Some(src), Some(psf)) = (src, psf) else {
        return Err(input_err());
    };
    let n = match voxel_count(src.dims) {
        Some(n) if src.data.len() as i64 == n => n,
        _ => return Err(input_err()),
    };
    let kn = match voxel_count(psf.dims) {
        Some(n) if psf.data.len() as i64 == n => n,
        _ => return Err(input_err()),
    };
    if n > opts.limits.max_voxels || kn > opts.limits.max_psf_samples {
        return Err(limit_err());
    }
    for axis in 0..3 {
        if !finite(src.spacing_mm[axis])
            || !finite(psf.spacing_mm[axis])
            || src.spacing_mm[axis] < 0.0
            || psf.spacing_mm[axis] < 0.0
        {
            return Err(input_err());
        }
        if psf.dims[axis] % 2 != 1 || psf.dims[axis] > src.dims[axis] {
            return Err(input_detail(&format!(
                "odd PSF support must fit source axis {axis}"
            )));
        }
        if src.dims[axis] > 1
            && (!(src.spacing_mm[axis] > 0.0)
                || !finite(src.spacing_mm[axis])
                || psf.spacing_mm[axis] != src.spacing_mm[axis])
        {
            return Err(input_detail(&format!("PSF/source spacing axis {axis}")));
        }
    }
    let mut refbytes: i64 = 0;
    if opts.source_references.len() as i64 > opts.limits.max_source_references {
        return Err(limit_err());
    }
    if restore && opts.source_references.is_empty() {
        return Err(input_err());
    }
    for (i, s) in opts.source_references.iter().enumerate() {
        if s.len() as i64 > opts.limits.max_reference_bytes {
            return Err(limit_err());
        }
        if trim_unicode_space(s).is_empty() {
            return Err(input_detail(&format!("source reference {i}")));
        }
        if refbytes > opts.limits.max_reference_bytes - s.len() as i64 {
            return Err(limit_err());
        }
        refbytes += s.len() as i64;
    }
    let scratch = n * 48 + kn * 8;
    let working = scratch + n * 4 + refbytes + opts.source_references.len() as i64 * 16;
    let convolutions = if restore { 2 * opts.iterations + 3 } else { 1 };
    let pairs = n * kn;
    if pairs > opts.limits.max_kernel_visits / convolutions
        || scratch > opts.limits.max_scratch_bytes
        || working > opts.limits.max_working_bytes
    {
        return Err(limit_err());
    }
    let (mut scale, mut mass) = (0.0f64, 0.0f64);
    let mut clamped = 0;
    for (i, v) in src.data.iter().enumerate() {
        let f = f64::from(*v);
        if !finite(f) || f < 0.0 && opts.negative_policy == PSF_REJECT_NEGATIVE {
            return Err(input_detail(&format!("source sample {i}")));
        }
        if f < 0.0 {
            clamped += 1;
        }
        scale = fmath::max(scale, f);
    }
    for (i, v) in psf.data.iter().enumerate() {
        let f = f64::from(*v);
        if !finite(f) || f < 0.0 {
            return Err(input_detail(&format!("PSF sample {i}")));
        }
        mass += f;
    }
    if !(mass > 0.0)
        || !finite(mass)
        || opts.normalization == PSF_REQUIRE_UNIT_MASS && (mass - 1.0).abs() > 1e-6
    {
        return Err(input_detail("PSF mass"));
    }
    if scale == 0.0 {
        scale = 1.0;
    }
    ctx_err(ctx)?;
    let y: Vec<f64> = src
        .data
        .iter()
        .map(|v| fmath::max(0.0, f64::from(*v)) / scale)
        .collect();
    let k: Vec<f64> = psf.data.iter().map(|v| f64::from(*v) / mass).collect();
    let mut digest = Sha256::new();
    for v in &psf.data {
        digest.update(v.to_bits().to_le_bytes());
    }
    let psf_sha256: String = digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let meta = PSFDerivedMetadata {
        derived: true,
        method: "Richardson-Lucy-direct".into(),
        noise_model: "independent-Poisson".into(),
        iterations_requested: opts.iterations,
        negative_clamped: clamped,
        boundary: opts.boundary,
        normalization: opts.normalization,
        negative_policy: opts.negative_policy,
        psf_dims: psf.dims,
        spacing_mm: src.spacing_mm,
        original_psf_mass: mass,
        epsilon: opts.epsilon,
        relative_change: opts.relative_change,
        intensity_scale: scale,
        scratch_bytes: scratch,
        working_bytes: working,
        kernel_visit_bound: pairs * convolutions,
        psf_sha256,
        source_references: opts.source_references.clone(),
        ..Default::default()
    };
    Ok(Prepared { y, k, meta, opts })
}
fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{85}' | '\u{A0}'
        ) || (c != '\u{FEFF}' && c.is_whitespace())
    })
}
pub fn richardson_lucy(
    ctx: Ctx<'_>,
    observed: Option<&Volume>,
    psf: Option<&Volume>,
    o: &PSFDeconvolutionOptions,
) -> Result<PSFDeconvolutionResult, FilterError> {
    let Prepared {
        y,
        k,
        mut meta,
        opts: o,
    } = prepare(ctx, observed, psf, o, true)?;
    let (Some(observed), Some(psf)) = (observed, psf) else {
        return Err(input_err());
    };
    let n = y.len();
    let mut x = vec![0f64; n];
    let mut pred = vec![0f64; n];
    let mut ratio = vec![0f64; n];
    let mut back = vec![0f64; n];
    let mut sensitivity = vec![0f64; n];
    let mut mean = 0.0;
    for v in &y {
        mean += v / n as f64;
    }
    for i in 0..n {
        x[i] = mean;
        ratio[i] = 1.0;
    }
    ctx_err(ctx)?;
    convolve(
        ctx,
        &ratio,
        &mut sensitivity,
        &k,
        observed.dims,
        psf.dims,
        o.boundary,
        true,
    )?;
    for s in &sensitivity {
        if !(*s > 0.0) || !finite(*s) {
            return Err(FilterError::new(
                ErrorKind::PSFDeconvolutionNumeric,
                "imagefilter: unsupported or nonfinite PSF restoration arithmetic: unobserved forward column",
            ));
        }
    }
    convolve(
        ctx,
        &x,
        &mut pred,
        &k,
        observed.dims,
        psf.dims,
        o.boundary,
        false,
    )?;
    let (initial_r, initial_d) = residual(ctx, &y, &pred, meta.intensity_scale, o.epsilon)?;
    meta.stopping_reason = "iteration-limit".into();
    if mean == 0.0 {
        meta.stopping_reason = "zero-input".into();
    } else {
        for iteration in 0..o.iterations {
            ctx_err(ctx)?;
            for i in 0..n {
                ratio[i] = y[i] / positive_prediction(pred[i], o.epsilon);
                if !finite(ratio[i]) {
                    return Err(numeric_err());
                }
            }
            convolve(
                ctx,
                &ratio,
                &mut back,
                &k,
                observed.dims,
                psf.dims,
                o.boundary,
                true,
            )?;
            let (mut difference, mut total) = (0.0, 0.0);
            for i in 0..n {
                let old = x[i];
                let v = old * back[i] / sensitivity[i];
                if v < 0.0 || !finite(v) {
                    return Err(numeric_err());
                }
                x[i] = v;
                difference += (v - old).abs();
                total += old;
            }
            convolve(
                ctx,
                &x,
                &mut pred,
                &k,
                observed.dims,
                psf.dims,
                o.boundary,
                false,
            )?;
            meta.iterations_completed = iteration + 1;
            if o.relative_change > 0.0
                && difference <= o.relative_change * fmath::max(total, o.epsilon)
            {
                meta.stopping_reason = "relative-L1".into();
                break;
            }
        }
    }
    let (final_r, final_d) = residual(ctx, &y, &pred, meta.intensity_scale, o.epsilon)?;
    if !finite(initial_r) || !finite(initial_d) || !finite(final_r) || !finite(final_d) {
        return Err(numeric_err());
    }
    let mut data = vec![0f32; n];
    for (i, v) in x.iter().enumerate() {
        let physical = v * meta.intensity_scale;
        if !finite(physical) || physical > f64::from(f32::MAX) {
            return Err(numeric_err());
        }
        data[i] = physical as f32;
    }
    ctx_err(ctx)?;
    Ok(PSFDeconvolutionResult {
        restored: Volume {
            dims: observed.dims,
            spacing_mm: observed.spacing_mm,
            data,
        },
        metadata: meta,
        initial_rmse: initial_r,
        final_rmse: final_r,
        initial_poisson_deviance: initial_d,
        final_poisson_deviance: final_d,
    })
}
#[doc(hidden)]
pub fn convolve_psf(
    ctx: Ctx<'_>,
    source: Option<&Volume>,
    psf: Option<&Volume>,
    o: &PSFDeconvolutionOptions,
) -> Result<Volume, FilterError> {
    let p = prepare(ctx, source, psf, o, false)?;
    let (Some(source), Some(psf)) = (source, psf) else {
        return Err(input_err());
    };
    let mut pred = vec![0f64; p.y.len()];
    convolve(
        ctx,
        &p.y,
        &mut pred,
        &p.k,
        source.dims,
        psf.dims,
        p.opts.boundary,
        false,
    )?;
    let mut data = vec![0f32; p.y.len()];
    for (i, v) in pred.iter().enumerate() {
        let value = v * p.meta.intensity_scale;
        if !finite(value) || value > f64::from(f32::MAX) {
            return Err(numeric_err());
        }
        data[i] = value as f32;
    }
    ctx_err(ctx)?;
    Ok(Volume {
        dims: source.dims,
        spacing_mm: source.spacing_mm,
        data,
    })
}
fn residual(
    ctx: Ctx<'_>,
    y: &[f64],
    pred: &[f64],
    scale: f64,
    epsilon: f64,
) -> Result<(f64, f64), FilterError> {
    let (mut norm, mut deviance) = (0.0, 0.0);
    for (i, v) in y.iter().copied().enumerate() {
        let p = pred[i];
        norm = fmath::hypot(norm, p - v);
        let mut d = p - v;
        if v > 0.0 {
            d += v * fmath::log(v / positive_prediction(p, epsilon));
        }
        deviance += fmath::max(0.0, 2.0 * d);
    }
    ctx_err(ctx)?;
    Ok((norm / (y.len() as f64).sqrt() * scale, deviance * scale))
}
fn positive_prediction(prediction: f64, epsilon: f64) -> f64 {
    if prediction > 0.0 {
        prediction
    } else {
        epsilon
    }
}
#[allow(clippy::too_many_arguments)]
fn convolve(
    ctx: Ctx<'_>,
    src: &[f64],
    dst: &mut [f64],
    k: &[f64],
    dims: [i64; 3],
    kdims: [i64; 3],
    border: u8,
    adjoint: bool,
) -> Result<(), FilterError> {
    ctx_err(ctx)?;
    dst.iter_mut().for_each(|v| *v = 0.0);
    let center = [kdims[0] / 2, kdims[1] / 2, kdims[2] / 2];
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            ctx_err(ctx)?;
            for x in 0..dims[0] {
                let oi = ((z * dims[1] + y) * dims[0] + x) as usize;
                for kz in 0..kdims[2] {
                    for ky in 0..kdims[1] {
                        for kx in 0..kdims[0] {
                            let ki = ((kz * kdims[1] + ky) * kdims[0] + kx) as usize;
                            let weight = k[ki];
                            if weight == 0.0 {
                                continue;
                            }
                            let (mut xx, mut yy, mut zz) =
                                (x - kx + center[0], y - ky + center[1], z - kz + center[2]);
                            if border == PSF_CIRCULAR_BOUNDARY {
                                xx = xx.rem_euclid(dims[0]);
                                yy = yy.rem_euclid(dims[1]);
                                zz = zz.rem_euclid(dims[2]);
                            } else if xx < 0
                                || xx >= dims[0]
                                || yy < 0
                                || yy >= dims[1]
                                || zz < 0
                                || zz >= dims[2]
                            {
                                continue;
                            }
                            let si = ((zz * dims[1] + yy) * dims[0] + xx) as usize;
                            if adjoint {
                                dst[si] += weight * src[oi];
                            } else {
                                dst[oi] += weight * src[si];
                            }
                        }
                    }
                }
            }
        }
    }
    ctx_err(ctx)
}
