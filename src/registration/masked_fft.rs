use super::error::{RegError, Sentinel};
use super::image::Image;
use super::matrix::{finite, finite_vec};
use crate::fmath;
use crate::numeric::fft::{self, Complex};
use crate::optimize::Context;
use crate::render::Vec3;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MaskedFftSearch {
    pub min: Vec3,
    pub max: Vec3,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MaskedFftOptions {
    pub min_overlap: i64,
    pub min_overlap_fraction: f64,
    pub search: Option<MaskedFftSearch>,
    pub ambiguity_tolerance: f64,
    pub workers: i64,
    pub max_input_voxels: u64,
    pub max_padded_voxels: u64,
    pub max_bytes: u64,
    pub max_fft_work: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MaskedFftResult {
    pub index_shift: [i64; 3],
    pub translation_lps: Vec3,
    pub score: f64,
    pub overlap: i64,
    pub overlap_fraction: f64,
    pub ambiguous: bool,
    pub search_boundary: bool,
    pub linear_boundary: bool,
    pub usable: bool,
    pub padded_size: [i64; 3],
    pub scratch_bytes: u64,
    pub fft_work: u64,
}
#[derive(Default)]
struct Plan {
    size: [usize; 3],
    n: usize,
    line: usize,
    bytes: u64,
    work: u64,
    options: MaskedFftOptions,
}
fn fail(s: &str) -> RegError {
    RegError::new(Sentinel::MaskedFft, s)
}
fn product(size: [usize; 3]) -> Result<u64, RegError> {
    let mut n = 1u64;
    for d in size {
        if d < 1 || d as u64 > u64::MAX / n {
            return Err(fail("size overflow"));
        }
        n *= d as u64;
    }
    Ok(n)
}
fn same_geometry(a: &Image, b: &Image, origin: bool) -> bool {
    if origin && a.origin != b.origin {
        return false;
    }
    for r in 0..3 {
        for c in 0..3 {
            let (x, y) = (a.index_to_physical.0[r][c], b.index_to_physical.0[r][c]);
            if !finite(x) || !finite(y) || (x - y).abs() > 1e-10 * fmath::max(x.abs(), y.abs()) {
                return false;
            }
        }
    }
    finite_vec(a.origin) && finite_vec(b.origin)
}
type Inputs<'a> = (&'a Image, &'a Image, &'a Image, &'a Image);
fn admission<'a>(
    f: Option<&'a Image>,
    m: Option<&'a Image>,
    fm: Option<&'a Image>,
    mm: Option<&'a Image>,
    options: MaskedFftOptions,
) -> (Plan, Result<Inputs<'a>, RegError>) {
    let mut p = Plan {
        options,
        ..Plan::default()
    };
    let r = admit(&mut p, f, m, fm, mm);
    (p, r)
}
fn admit<'a>(
    p: &mut Plan,
    f: Option<&'a Image>,
    m: Option<&'a Image>,
    fm: Option<&'a Image>,
    mm: Option<&'a Image>,
) -> Result<Inputs<'a>, RegError> {
    let (Some(f), Some(m), Some(fm), Some(mm)) = (f, m, fm, mm) else {
        return Err(fail("two images and two masks required"));
    };
    let o = &mut p.options;
    if !same_geometry(f, m, false)
        || !same_geometry(f, fm, true)
        || !same_geometry(m, mm, true)
        || f.size != fm.size
        || m.size != mm.size
        || (f.size[2] == 1) != (m.size[2] == 1)
    {
        return Err(fail("incompatible image/mask grids"));
    }
    if o.min_overlap == 0 {
        o.min_overlap = 3;
    }
    if o.min_overlap_fraction == 0.0 {
        o.min_overlap_fraction = 0.1;
    }
    if o.ambiguity_tolerance == 0.0 {
        o.ambiguity_tolerance = 1e-8;
    }
    if o.min_overlap < 3
        || !finite(o.min_overlap_fraction)
        || o.min_overlap_fraction < 0.0
        || o.min_overlap_fraction > 1.0
        || !finite(o.ambiguity_tolerance)
        || o.ambiguity_tolerance < 0.0
        || o.ambiguity_tolerance > 2.0
        || o.workers < 0
    {
        return Err(fail("invalid options"));
    }
    if let Some(s) = &o.search
        && (!finite_vec(s.min)
            || !finite_vec(s.max)
            || s.min.x > s.max.x
            || s.min.y > s.max.y
            || s.min.z > s.max.z)
    {
        return Err(fail("invalid physical search"));
    }
    if o.max_input_voxels == 0 {
        o.max_input_voxels = 1 << 22;
    }
    if o.max_padded_voxels == 0 {
        o.max_padded_voxels = 1 << 22;
    }
    if o.max_bytes == 0 {
        o.max_bytes = 768 << 20;
    }
    if o.max_fft_work == 0 {
        o.max_fft_work = 1 << 32;
    }
    let fnn = product(f.size)?;
    let mn = product(m.size)?;
    if fnn > o.max_input_voxels
        || mn > o.max_input_voxels - fnn
        || fnn != f.data.len() as u64
        || mn != m.data.len() as u64
        || fnn != fm.data.len() as u64
        || mn != mm.data.len() as u64
    {
        return Err(fail("input voxel budget or sample lengths"));
    }
    let mut n = 1u64;
    let mut lg = 0u64;
    let max_int = i64::MAX as u64;
    for axis in 0..3 {
        let need = f.size[axis] as u64 + m.size[axis] as u64 - 1;
        let mut d = 1u64;
        while d < need {
            if d > max_int / 2 {
                return Err(fail("padding overflow"));
            }
            d *= 2;
            lg += 1;
        }
        if d > p.options.max_padded_voxels || n > p.options.max_padded_voxels / d {
            return Err(fail("padded voxel budget"));
        }
        n *= d;
        p.size[axis] = d as usize;
        p.line = p.line.max(d as usize);
    }
    if n > max_int || n > u64::MAX / 160 || p.line as u64 > (u64::MAX - 160 * n) / 16 {
        return Err(fail("scratch overflow"));
    }
    p.bytes = 160 * n + 16 * p.line as u64;
    if lg != 0 && n > u64::MAX / (12 * lg) {
        return Err(fail("FFT work overflow"));
    }
    p.work = 12 * n * lg;
    if p.bytes > p.options.max_bytes || p.work > p.options.max_fft_work {
        return Err(fail("scratch or FFT work budget"));
    }
    p.n = n as usize;
    Ok((f, m, fm, mm))
}
pub fn masked_fft_workspace(
    f: Option<&Image>,
    m: Option<&Image>,
    fm: Option<&Image>,
    mm: Option<&Image>,
    o: MaskedFftOptions,
) -> (u64, u64, Option<RegError>) {
    let (p, r) = admission(f, m, fm, mm, o);
    (p.bytes, p.work, r.err())
}
fn check(ctx: Context<'_>) -> Result<(), RegError> {
    ctx.err().map_err(|_| RegError::canceled())
}
fn mul(a: Complex, b: Complex) -> Complex {
    Complex::new(a.re * b.re - a.im * b.im, a.re * b.im + a.im * b.re)
}
fn axes(
    ctx: Context<'_>,
    a: &mut [Complex],
    size: [usize; 3],
    line: &mut [Complex],
    inverse: bool,
) -> Result<(), RegError> {
    for axis in 0..3 {
        let stride: usize = size[..axis].iter().product();
        let block = stride * size[axis];
        let mut start = 0;
        while start < a.len() {
            for offset in 0..stride {
                check(ctx)?;
                let v = &mut line[..size[axis]];
                for (k, x) in v.iter_mut().enumerate() {
                    if k & 1023 == 0 {
                        check(ctx)?;
                    }
                    *x = a[start + offset + k * stride];
                }
                check(ctx)?;
                fft::transform(v, inverse);
                check(ctx)?;
                for (k, x) in v.iter().enumerate() {
                    if k & 1023 == 0 {
                        check(ctx)?;
                    }
                    a[start + offset + k * stride] = *x;
                }
            }
            start += block;
        }
    }
    if inverse {
        let scale = Complex::new(1.0 / a.len() as f64, 0.0);
        for (i, x) in a.iter_mut().enumerate() {
            if i & 1023 == 0 {
                check(ctx)?;
            }
            *x = mul(*x, scale);
        }
    }
    check(ctx)
}
fn in_search(t: Vec3, s: Option<&MaskedFftSearch>) -> bool {
    match s {
        None => true,
        Some(s) => {
            t.x >= s.min.x
                && t.x <= s.max.x
                && t.y >= s.min.y
                && t.y <= s.max.y
                && t.z >= s.min.z
                && t.z <= s.max.z
        }
    }
}
fn translation(f: &Image, m: &Image, s: [i64; 3]) -> Vec3 {
    m.origin
        .sub(f.origin)
        .add(
            f.index_to_physical
                .mul_vec(Vec3::new(s[0] as f64, s[1] as f64, s[2] as f64)),
        )
}
pub fn estimate_masked_fft_translation(
    ctx: Context<'_>,
    f: Option<&Image>,
    m: Option<&Image>,
    fm: Option<&Image>,
    mm: Option<&Image>,
    o: MaskedFftOptions,
) -> Result<MaskedFftResult, RegError> {
    check(ctx)?;
    let (p, r) = admission(f, m, fm, mm, o);
    let (f, m, fm, mm) = r?;
    let mut counts = [0i64; 2];
    let mut centers = [0f64; 2];
    let mut scales = [0f64; 2];
    let sides = [(f, fm), (m, mm)];
    for (side, (im, mask)) in sides.iter().enumerate() {
        for (i, &v) in im.data.iter().enumerate() {
            if i & 1023 == 0 {
                check(ctx)?;
            }
            if !finite(f64::from(v)) || !finite(f64::from(mask.data[i])) {
                return Err(fail("nonfinite sample or mask"));
            }
            if mask.data[i] > 0.0 {
                counts[side] += 1;
                centers[side] += f64::from(v);
            }
        }
        if counts[side] == 0 {
            return Err(RegError::bare(Sentinel::MaskedFftNoOverlap));
        }
        centers[side] /= counts[side] as f64;
        for (i, &v) in im.data.iter().enumerate() {
            if i & 1023 == 0 {
                check(ctx)?;
            }
            if mask.data[i] > 0.0 {
                scales[side] = fmath::max(scales[side], (f64::from(v) - centers[side]).abs());
            }
        }
        if scales[side] == 0.0 {
            return Err(RegError::bare(Sentinel::MaskedFftVariance));
        }
    }
    check(ctx)?;
    let zero = Complex::default();
    let mut spectra: Vec<Vec<Complex>> = (0..6).map(|_| vec![zero; p.n]).collect();
    let mut scratch = vec![zero; p.n];
    let mut line = vec![zero; p.line];
    let mut moments: Vec<Vec<f64>> = (0..6).map(|_| vec![0.0; p.n]).collect();
    for (side, (im, mask)) in sides.iter().enumerate() {
        let s = im.size;
        for z in 0..s[2] {
            for y in 0..s[1] {
                check(ctx)?;
                for x in 0..s[0] {
                    if x & 1023 == 0 {
                        check(ctx)?;
                    }
                    let i = (z * s[1] + y) * s[0] + x;
                    if mask.data[i] <= 0.0 {
                        continue;
                    }
                    let j = (z * p.size[1] + y) * p.size[0] + x;
                    let v = (f64::from(im.data[i]) - centers[side]) / scales[side];
                    spectra[3 * side][j] = Complex::new(1.0, 0.0);
                    spectra[3 * side + 1][j] = Complex::new(v, 0.0);
                    spectra[3 * side + 2][j] = Complex::new(v * v, 0.0);
                }
            }
        }
    }
    for s in spectra.iter_mut() {
        axes(ctx, s, p.size, &mut line, false)?;
    }
    let pairs = [[0, 3], [1, 3], [0, 4], [2, 3], [0, 5], [1, 4]];
    for (k, pair) in pairs.iter().enumerate() {
        for (i, x) in scratch.iter_mut().enumerate() {
            if i & 1023 == 0 {
                check(ctx)?;
            }
            let a = spectra[pair[0]][i];
            *x = mul(Complex::new(a.re, -a.im), spectra[pair[1]][i]);
        }
        axes(ctx, &mut scratch, p.size, &mut line, true)?;
        for (i, v) in scratch.iter().enumerate() {
            if i & 1023 == 0 {
                check(ctx)?;
            }
            if !finite(v.re) || !finite(v.im) {
                return Err(fail("nonfinite FFT"));
            }
            moments[k][i] = v.re;
        }
    }
    let smaller = counts[0].min(counts[1]);
    let minimum = p
        .options
        .min_overlap
        .max((p.options.min_overlap_fraction * smaller as f64).ceil() as i64);
    let mut result = MaskedFftResult {
        padded_size: p.size.map(|d| d as i64),
        scratch_bytes: p.bytes,
        fft_work: p.work,
        ..MaskedFftResult::default()
    };
    let (mut found, mut overlap_seen) = (false, false);
    let mut ties = 0i64;
    let search = p.options.search.as_ref();
    let fs = f.size.map(|d| d as i64);
    let ms = m.size.map(|d| d as i64);
    let ps = p.size.map(|d| d as i64);
    for pass in 0..2 {
        for z in 1 - fs[2]..ms[2] {
            for y in 1 - fs[1]..ms[1] {
                for x in 1 - fs[0]..ms[0] {
                    check(ctx)?;
                    let s = [x, y, z];
                    let t = translation(f, m, s);
                    if !finite_vec(t) {
                        return Err(fail("nonfinite physical translation"));
                    }
                    if !in_search(t, search) {
                        continue;
                    }
                    let wrap = |v: i64, d: i64| if v < 0 { v + d } else { v };
                    let (ix, iy, iz) = (wrap(x, ps[0]), wrap(y, ps[1]), wrap(z, ps[2]));
                    let i = ((iz * ps[1] + iy) * ps[0] + ix) as usize;
                    let nf = moments[0][i].round();
                    if nf < minimum as f64 {
                        continue;
                    }
                    overlap_seen = true;
                    if nf > smaller as f64 || (moments[0][i] - nf).abs() > 1e-5 {
                        return Err(fail("unresolved FFT overlap"));
                    }
                    let vf = moments[3][i] - moments[1][i] * moments[1][i] / nf;
                    let vm = moments[4][i] - moments[2][i] * moments[2][i] / nf;
                    if vf <= 1e-10 * nf || vm <= 1e-10 * nf {
                        continue;
                    }
                    let score =
                        (moments[5][i] - moments[1][i] * moments[2][i] / nf) / (vf * vm).sqrt();
                    if !finite(score) || score < -1.0 - 1e-6 || score > 1.0 + 1e-6 {
                        return Err(fail("unresolved NCC"));
                    }
                    let score = fmath::max(-1.0, fmath::min(1.0, score));
                    if pass == 0 {
                        if !found || score > result.score + 1e-12 {
                            found = true;
                            result.index_shift = s;
                            result.translation_lps = t;
                            result.score = score;
                            result.overlap = nf as i64;
                            result.overlap_fraction = nf / smaller as f64;
                        }
                    } else if found && score >= result.score - p.options.ambiguity_tolerance {
                        ties += 1;
                    }
                }
            }
        }
    }
    if !found {
        if overlap_seen {
            return Err(RegError::bare(Sentinel::MaskedFftVariance));
        }
        return Err(RegError::bare(Sentinel::MaskedFftNoOverlap));
    }
    result.ambiguous = ties > 1;
    for axis in 0..3 {
        let d = result.index_shift[axis];
        if fs[axis].min(ms[axis]) > 1 && (d == 1 - fs[axis] || d == ms[axis] - 1) {
            result.linear_boundary = true;
        }
        if search.is_some() && (fs[axis] > 1 || ms[axis] > 1) {
            for step in [-1, 1] {
                let mut s = result.index_shift;
                s[axis] += step;
                if !in_search(translation(f, m, s), search) {
                    result.search_boundary = true;
                }
            }
        }
    }
    result.usable = !result.ambiguous && !result.linear_boundary;
    check(ctx)?;
    Ok(result)
}
