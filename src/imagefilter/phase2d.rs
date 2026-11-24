use super::recursive_gaussian::Complex128;
use super::volume::{Ctx, ErrorKind, FilterError, Volume, ctx_err, voxel_count};
use crate::fmath;
use crate::numeric::fft;
use std::f64::consts::PI;
fn input() -> FilterError {
    FilterError::new(
        ErrorKind::Phase2DInput,
        "imagefilter: invalid phase/wavelet 2D input",
    )
}
fn input_detail(detail: &str) -> FilterError {
    FilterError::new(
        ErrorKind::Phase2DInput,
        format!("imagefilter: invalid phase/wavelet 2D input: {detail}"),
    )
}
fn limit() -> FilterError {
    FilterError::new(
        ErrorKind::Phase2DLimit,
        "imagefilter: phase/wavelet 2D resource limit",
    )
}
fn numeric() -> FilterError {
    FilterError::new(
        ErrorKind::Phase2DNumeric,
        "imagefilter: unrepresentable phase/wavelet 2D result",
    )
}
pub const WAVELET_REFLECT: u8 = 0;
pub const WAVELET_PERIODIC: u8 = 1;
pub const PHASE2D_BOTH: u8 = 0;
pub const PHASE2D_BRIGHT: u8 = 1;
pub const PHASE2D_DARK: u8 = 2;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Phase2DLimits {
    pub max_pixels: i64,
    pub max_padded_pixels: i64,
    pub max_scales: i64,
    pub max_working_bytes: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wavelet2DOptions {
    pub scales_mm: Vec<f64>,
    pub boundary: u8,
    pub limits: Phase2DLimits,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wavelet2DMetadata {
    pub experimental: bool,
    pub family: String,
    pub boundary: String,
    pub dims: [i64; 3],
    pub spacing_mm: [f64; 3],
    pub scales_mm: Vec<f64>,
    pub padded_dims: [i64; 2],
    pub crop_offset: [i64; 2],
    pub workers: i64,
    pub scratch_bytes: i64,
    pub working_bytes: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wavelet2DResult {
    pub bands: Vec<Volume>,
    pub lowpass: Option<Volume>,
    pub metadata: Wavelet2DMetadata,
}
#[doc(hidden)]
pub const WAVELET2D_FAMILY: &str = "undecimated-gaussian-difference-riesz-2d";
#[derive(Default, Clone, Copy)]
struct Plan {
    w: usize,
    h: usize,
    n: usize,
    pw: usize,
    ph: usize,
    pn: usize,
    ox: usize,
    oy: usize,
    scratch: i64,
    working: i64,
}
pub fn decompose_wavelet_2d(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    opts: &Wavelet2DOptions,
) -> Result<Wavelet2DResult, FilterError> {
    let p = admit(ctx, src, opts, false)?;
    let Some(src) = src else {
        return Err(input());
    };
    let spectrum = spectrum(ctx, src, &p)?;
    let mut out = Wavelet2DResult {
        bands: Vec::new(),
        lowpass: None,
        metadata: metadata(&p, src, opts),
    };
    let mut work = vec![Complex128::default(); p.pn];
    let mut previous = 0.0;
    for sigma in opts.scales_mm.iter().copied() {
        filter(ctx, &spectrum, &mut work, src, &p, previous, sigma, 0)?;
        out.bands.push(crop(ctx, &work, src, &p)?);
        previous = sigma;
    }
    filter(ctx, &spectrum, &mut work, src, &p, previous, previous, 0)?;
    out.lowpass = Some(crop(ctx, &work, src, &p)?);
    ctx_err(ctx)?;
    Ok(out)
}
pub fn reconstruct_wavelet_2d(
    ctx: Ctx<'_>,
    input_bank: Option<&Wavelet2DResult>,
    limits: Phase2DLimits,
) -> Result<Volume, FilterError> {
    let l = phase_limits(limits)?;
    let Some(bank) =
        input_bank.filter(|b| b.metadata.family == WAVELET2D_FAMILY && !b.bands.is_empty())
    else {
        return Err(input_detail("missing or incompatible wavelet bank"));
    };
    if bank.bands.len() as i64 > l.max_scales {
        return Err(limit());
    }
    let n = validate(ctx, bank.lowpass.as_ref(), l.max_pixels)?;
    let Some(low) = &bank.lowpass else {
        return Err(input());
    };
    if bank.metadata.dims != low.dims
        || bank.metadata.spacing_mm != low.spacing_mm
        || bank.metadata.scales_mm.len() != bank.bands.len()
    {
        return Err(input_detail("incompatible bank metadata"));
    }
    let mut previous = 0.0;
    for sigma in bank.metadata.scales_mm.iter().copied() {
        if !(sigma >= 1e-9 && sigma <= 1e9 && sigma > previous) {
            return Err(input());
        }
        previous = sigma;
    }
    if n as i64 * 4 > l.max_working_bytes {
        return Err(limit());
    }
    for band in &bank.bands {
        validate(ctx, Some(band), l.max_pixels)?;
        if band.dims != low.dims || band.spacing_mm != low.spacing_mm {
            return Err(input_detail("incompatible coefficient geometry"));
        }
    }
    let mut data = vec![0f32; n];
    for (i, v) in low.data.iter().enumerate() {
        if i & 1023 == 0 {
            ctx_err(ctx)?;
        }
        let mut sum = f64::from(*v);
        for band in &bank.bands {
            sum += f64::from(band.data[i]);
        }
        if !representable(sum) {
            return Err(numeric());
        }
        data[i] = sum as f32;
    }
    ctx_err(ctx)?;
    Ok(Volume {
        dims: low.dims,
        spacing_mm: low.spacing_mm,
        data,
    })
}
fn phase_limits(mut l: Phase2DLimits) -> Result<Phase2DLimits, FilterError> {
    if l.max_pixels < 0
        || l.max_pixels > 1 << 20
        || l.max_padded_pixels < 0
        || l.max_padded_pixels > 1 << 22
        || l.max_scales < 0
        || l.max_scales > 8
        || l.max_working_bytes < 0
        || l.max_working_bytes > 256 << 20
    {
        return Err(input());
    }
    if l.max_pixels == 0 {
        l.max_pixels = 1 << 20;
    }
    if l.max_padded_pixels == 0 {
        l.max_padded_pixels = 1 << 22;
    }
    if l.max_scales == 0 {
        l.max_scales = 8;
    }
    if l.max_working_bytes == 0 {
        l.max_working_bytes = 256 << 20;
    }
    Ok(l)
}
fn validate(ctx: Ctx<'_>, src: Option<&Volume>, max_pixels: i64) -> Result<usize, FilterError> {
    ctx_err(ctx)?;
    let Some(src) = src.filter(|s| s.dims[2] == 1 && s.dims[0] >= 2 && s.dims[1] >= 2) else {
        return Err(input_detail(
            "a regular 2D image with both axes >= 2 is required",
        ));
    };
    let n = match voxel_count(src.dims) {
        Some(n) if n <= max_pixels => n as usize,
        _ => return Err(limit()),
    };
    if src.data.len() != n {
        return Err(input());
    }
    for s in &src.spacing_mm[..2] {
        if !(*s >= 1e-9 && *s <= 1e9) {
            return Err(input_detail("physical spacing outside [1e-9,1e9] mm"));
        }
    }
    if !src.spacing_mm[2].is_finite() {
        return Err(input());
    }
    if src.data.iter().any(|v| !v.is_finite()) {
        return Err(input());
    }
    ctx_err(ctx)?;
    Ok(n)
}
fn admit(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    o: &Wavelet2DOptions,
    phase: bool,
) -> Result<Plan, FilterError> {
    let mut p = Plan::default();
    let l = phase_limits(o.limits)?;
    if o.boundary > WAVELET_PERIODIC || o.scales_mm.is_empty() {
        return Err(input());
    }
    if o.scales_mm.len() as i64 > l.max_scales {
        return Err(limit());
    }
    let mut previous = 0.0;
    for sigma in o.scales_mm.iter().copied() {
        if !(sigma >= 1e-9 && sigma <= 1e9 && sigma > previous) {
            return Err(input_detail(
                "increasing physical sigmas in [1e-9,1e9] mm required",
            ));
        }
        previous = sigma;
    }
    p.n = validate(ctx, src, l.max_pixels)?;
    let Some(src) = src else {
        return Err(input());
    };
    p.w = src.dims[0] as usize;
    p.h = src.dims[1] as usize;
    p.pw = p.w;
    p.ph = p.h;
    if o.boundary == WAVELET_PERIODIC {
        if p.w & (p.w - 1) != 0 || p.h & (p.h - 1) != 0 {
            return Err(input_detail("periodic dimensions must be radix-2"));
        }
    } else {
        let (mut domain, mut offset) = ([0usize; 2], [0usize; 2]);
        for axis in 0..2 {
            let n = src.dims[axis];
            let margin = (6.0 * previous / src.spacing_mm[axis]).ceil();
            if margin > l.max_padded_pixels as f64
                || n as f64 + 2.0 * margin > l.max_padded_pixels as f64
            {
                return Err(limit());
            }
            offset[axis] = margin as usize;
            domain[axis] = fft::next_power_of_two(n + 2 * offset[axis] as i64) as usize;
        }
        p.pw = domain[0];
        p.ph = domain[1];
        p.ox = offset[0];
        p.oy = offset[1];
    }
    if (p.pw as i64) * (p.ph as i64) > l.max_padded_pixels {
        return Err(limit());
    }
    p.pn = p.pw * p.ph;
    p.scratch = 32 * p.pn as i64 + 16 * p.pw.max(p.ph) as i64;
    let mut output = 4 * p.n as i64 * (o.scales_mm.len() as i64 + 1);
    if phase {
        p.scratch += 40 * p.n as i64;
        output = 17 * p.n as i64;
    }
    p.working = p.scratch + output + 8 * o.scales_mm.len() as i64;
    if p.working > l.max_working_bytes {
        return Err(limit());
    }
    ctx_err(ctx)?;
    Ok(p)
}
fn metadata(p: &Plan, src: &Volume, o: &Wavelet2DOptions) -> Wavelet2DMetadata {
    let boundary = if o.boundary == WAVELET_PERIODIC {
        "source-periodic"
    } else {
        "reflect-six-sigma-radix2-periodic-extension"
    };
    Wavelet2DMetadata {
        experimental: true,
        family: WAVELET2D_FAMILY.into(),
        boundary: boundary.into(),
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        scales_mm: o.scales_mm.clone(),
        padded_dims: [p.pw as i64, p.ph as i64],
        crop_offset: [p.ox as i64, p.oy as i64],
        workers: 1,
        scratch_bytes: p.scratch,
        working_bytes: p.working,
    }
}
fn reflect(i: i64, n: usize) -> usize {
    let period = 2 * (n as i64 - 1);
    let mut i = i % period;
    if i < 0 {
        i += period;
    }
    if i >= n as i64 {
        i = period - i;
    }
    i as usize
}
fn spectrum(ctx: Ctx<'_>, src: &Volume, p: &Plan) -> Result<Vec<Complex128>, FilterError> {
    let mut s = vec![Complex128::default(); p.pn];
    for y in 0..p.ph {
        ctx_err(ctx)?;
        let sy = reflect(y as i64 - p.oy as i64, p.h);
        for x in 0..p.pw {
            let sx = reflect(x as i64 - p.ox as i64, p.w);
            s[y * p.pw + x] = Complex128::new(f64::from(src.data[sy * p.w + sx]), 0.0);
        }
    }
    fft2(ctx, &mut s, p, false)?;
    Ok(s)
}
fn fft2(ctx: Ctx<'_>, a: &mut [Complex128], p: &Plan, inverse: bool) -> Result<(), FilterError> {
    let mut row = vec![fft::Complex::default(); p.pw];
    for y in 0..p.ph {
        ctx_err(ctx)?;
        for x in 0..p.pw {
            let c = a[y * p.pw + x];
            row[x] = fft::Complex::new(c.re, c.im);
        }
        fft::transform(&mut row, inverse);
        for x in 0..p.pw {
            a[y * p.pw + x] = Complex128::new(row[x].re, row[x].im);
        }
    }
    let mut col = vec![fft::Complex::default(); p.ph];
    for x in 0..p.pw {
        ctx_err(ctx)?;
        for y in 0..p.ph {
            let c = a[y * p.pw + x];
            col[y] = fft::Complex::new(c.re, c.im);
        }
        fft::transform(&mut col, inverse);
        for y in 0..p.ph {
            a[y * p.pw + x] = Complex128::new(col[y].re, col[y].im);
        }
    }
    if inverse {
        let scale = Complex128::new(1.0 / p.pn as f64, 0.0);
        for v in a.iter_mut() {
            *v = v.mul(scale);
        }
    }
    ctx_err(ctx)
}
#[allow(clippy::too_many_arguments)]
fn filter(
    ctx: Ctx<'_>,
    spectrum: &[Complex128],
    work: &mut [Complex128],
    src: &Volume,
    p: &Plan,
    previous: f64,
    sigma: f64,
    component: usize,
) -> Result<(), FilterError> {
    for y in 0..p.ph {
        ctx_err(ctx)?;
        let mut ky = y as i64;
        if ky > (p.ph / 2) as i64 {
            ky -= p.ph as i64;
        }
        let wy = 2.0 * PI * ky as f64 / (p.ph as f64 * src.spacing_mm[1]);
        for x in 0..p.pw {
            let mut kx = x as i64;
            if kx > (p.pw / 2) as i64 {
                kx -= p.pw as i64;
            }
            let wx = 2.0 * PI * kx as f64 / (p.pw as f64 * src.spacing_mm[0]);
            let r2 = wx * wx + wy * wy;
            let mut weight = fmath::exp(-0.5 * sigma * sigma * r2);
            if previous != sigma {
                weight = fmath::exp(-0.5 * previous * previous * r2) - weight;
            }
            let mut factor = Complex128::new(weight, 0.0);
            if component != 0 {
                let (k, nyquist) = if component == 2 {
                    (wy, y == p.ph / 2)
                } else {
                    (wx, x == p.pw / 2)
                };
                factor = Complex128::new(0.0, 0.0);
                if r2 > 0.0 && !nyquist {
                    factor = Complex128::new(0.0, -weight * k / r2.sqrt());
                }
            }
            work[y * p.pw + x] = spectrum[y * p.pw + x].mul(factor);
        }
    }
    fft2(ctx, work, p, true)
}
fn crop(ctx: Ctx<'_>, a: &[Complex128], src: &Volume, p: &Plan) -> Result<Volume, FilterError> {
    let mut data = vec![0f32; p.n];
    for y in 0..p.h {
        ctx_err(ctx)?;
        for x in 0..p.w {
            let v = a[(y + p.oy) * p.pw + x + p.ox].re;
            if !representable(v) {
                return Err(numeric());
            }
            data[y * p.w + x] = v as f32;
        }
    }
    Ok(Volume {
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        data,
    })
}
fn representable(v: f64) -> bool {
    !v.is_nan() && v.abs() <= f64::from(f32::MAX)
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PhaseSymmetry2DOptions {
    pub wavelet: Wavelet2DOptions,
    pub noise_threshold: f64,
    pub amplitude_floor: f64,
    pub polarity: u8,
}
#[derive(Debug, Clone, PartialEq)]
pub struct PhaseSymmetry2DResult {
    pub symmetry: Volume,
    pub amplitude: Volume,
    pub phase: Volume,
    pub scale_mm: Volume,
    pub valid: Vec<bool>,
    pub metadata: Wavelet2DMetadata,
    pub noise_threshold: f64,
    pub amplitude_floor: f64,
    pub polarity: u8,
}
pub fn phase_symmetry_2d(
    ctx: Ctx<'_>,
    src: Option<&Volume>,
    opts: &PhaseSymmetry2DOptions,
) -> Result<PhaseSymmetry2DResult, FilterError> {
    ctx_err(ctx)?;
    let floor = if opts.amplitude_floor == 0.0 {
        1e-12
    } else {
        opts.amplitude_floor
    };
    if !(floor > 0.0 && floor <= f64::MAX)
        || !(opts.noise_threshold >= 0.0 && opts.noise_threshold <= f64::MAX)
        || opts.polarity > PHASE2D_DARK
    {
        return Err(input());
    }
    let p = admit(ctx, src, &opts.wavelet, true)?;
    let Some(src) = src else {
        return Err(input());
    };
    let spectrum = spectrum(ctx, src, &p)?;
    let mut work = vec![Complex128::default(); p.pn];
    let n = p.n;
    let (mut even, mut rx) = (vec![0f64; n], vec![0f64; n]);
    let (mut energy, mut amplitude, mut peak) = (vec![0f64; n], vec![0f64; n], vec![0f64; n]);
    let vol = || Volume {
        dims: src.dims,
        spacing_mm: src.spacing_mm,
        data: vec![0f32; n],
    };
    let mut out = PhaseSymmetry2DResult {
        symmetry: vol(),
        amplitude: vol(),
        phase: vol(),
        scale_mm: vol(),
        valid: vec![false; n],
        metadata: metadata(&p, src, &opts.wavelet),
        noise_threshold: opts.noise_threshold,
        amplitude_floor: floor,
        polarity: opts.polarity,
    };
    let mut previous = 0.0;
    for sigma in opts.wavelet.scales_mm.iter().copied() {
        for component in 0..3 {
            filter(
                ctx, &spectrum, &mut work, src, &p, previous, sigma, component,
            )?;
            for y in 0..p.h {
                ctx_err(ctx)?;
                for x in 0..p.w {
                    let i = y * p.w + x;
                    let v = work[(y + p.oy) * p.pw + x + p.ox].re;
                    if !v.is_finite() {
                        return Err(numeric());
                    }
                    match component {
                        0 => even[i] = v,
                        1 => rx[i] = v,
                        _ => {
                            let odd = fmath::hypot(rx[i], v);
                            let a = fmath::hypot(even[i], odd);
                            amplitude[i] += a;
                            let polarity = match opts.polarity {
                                PHASE2D_BRIGHT => even[i],
                                PHASE2D_DARK => -even[i],
                                _ => even[i].abs(),
                            };
                            energy[i] += fmath::max(polarity - odd - opts.noise_threshold, 0.0);
                            if a > peak[i] {
                                peak[i] = a;
                                out.phase.data[i] = fmath::atan2(odd, even[i]) as f32;
                                out.scale_mm.data[i] = sigma as f32;
                            }
                        }
                    }
                }
            }
        }
        previous = sigma;
    }
    for (i, a) in amplitude.iter().copied().enumerate() {
        if !representable(a) {
            return Err(numeric());
        }
        out.amplitude.data[i] = a as f32;
        if a <= floor {
            out.phase.data[i] = 0.0;
            out.scale_mm.data[i] = 0.0;
            continue;
        }
        out.valid[i] = true;
        out.symmetry.data[i] = fmath::min(1.0, energy[i] / a) as f32;
    }
    ctx_err(ctx)?;
    Ok(out)
}
