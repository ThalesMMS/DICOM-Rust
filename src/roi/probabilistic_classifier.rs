use super::{Cancellation, Error};
use crate::dicomjson::jsonwire;
use crate::fmath;
use crate::fmath::rand::Rand;
use sha2::{Digest, Sha256};
pub const ERR_PROBABILISTIC_INPUT: &str = "roi: invalid probabilistic classification input";
pub const ERR_PROBABILISTIC_LIMIT: &str = "roi: probabilistic classification resource limit";
pub const ERR_PROBABILISTIC_EMPTY_CLASS: &str = "roi: empty Gaussian mixture class";
#[derive(Debug, Clone)]
pub struct ScalarClassificationInput<'a> {
    pub dimensions: [i64; 3],
    pub spacing_mm: [f64; 3],
    pub values: &'a [f64],
    pub mask: Option<&'a [bool]>,
    pub units: String,
    pub normalization: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GaussianClass {
    pub mean: f64,
    pub variance: f64,
    pub weight: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProbabilisticLimits {
    pub max_voxels: i64,
    pub max_classes: i64,
    pub max_iterations: i64,
    pub max_working_bytes: i64,
}
pub fn default_probabilistic_limits() -> ProbabilisticLimits {
    ProbabilisticLimits {
        max_voxels: 1 << 20,
        max_classes: 8,
        max_iterations: 256,
        max_working_bytes: 256 << 20,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MrfOptions {
    pub beta: f64,
    pub contrast_sigma: f64,
    pub tolerance: f64,
    pub damping: f64,
    pub iterations: i64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProbabilisticOptions {
    pub classes: i64,
    pub em_iterations: i64,
    pub seed: i64,
    pub variance_floor: f64,
    pub em_tolerance: f64,
    pub initial: Vec<GaussianClass>,
    pub priors: Vec<f64>,
    pub skip_non_finite: bool,
    pub units: String,
    pub normalization: String,
    pub mrf: MrfOptions,
    pub limits: ProbabilisticLimits,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProbabilisticResult {
    pub dimensions: [i64; 3],
    pub spacing_mm: [f64; 3],
    pub classes: Vec<GaussianClass>,
    pub priors: Vec<f64>,
    pub labels: Vec<u16>,
    pub posteriors: Vec<f64>,
    pub log_likelihood: f64,
    pub em_iterations: i64,
    pub mrf_iterations: i64,
    pub em_converged: bool,
    pub mrf_converged: bool,
    pub selected_voxels: i64,
    pub skipped_non_finite: i64,
    pub working_bytes: i64,
    pub source_sha256: String,
    pub parameters_sha256: String,
    pub method: String,
    pub units: String,
    pub normalization: String,
}
struct Work<'a> {
    input: &'a ScalarClassificationInput<'a>,
    o: ProbabilisticOptions,
    valid: Vec<bool>,
    n: usize,
    k: usize,
    count: i64,
    skipped: i64,
    bytes: i64,
    ctx: &'a dyn Cancellation,
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn input_err() -> Error {
    Error::sentinel(ERR_PROBABILISTIC_INPUT)
}
fn limit_err() -> Error {
    Error::sentinel(ERR_PROBABILISTIC_LIMIT)
}
fn empty_err() -> Error {
    Error::sentinel(ERR_PROBABILISTIC_EMPTY_CLASS)
}
fn poll(ctx: &dyn Cancellation) -> Result<(), Error> {
    match ctx.err() {
        Some(e) => Err(Error::from(e)),
        None => Ok(()),
    }
}
fn check(ctx: Option<&dyn Cancellation>) -> Result<&dyn Cancellation, Error> {
    let Some(c) = ctx else {
        return Err(Error::wrap(ERR_PROBABILISTIC_INPUT, "nil context"));
    };
    poll(c)?;
    Ok(c)
}
fn prepare<'a>(
    ctx: Option<&'a dyn Cancellation>,
    input: &'a ScalarClassificationInput<'a>,
    o: &ProbabilisticOptions,
) -> Result<Work<'a>, Error> {
    let ctx = check(ctx)?;
    let mut o = o.clone();
    let d = default_probabilistic_limits();
    let mut l = o.limits;
    if l.max_voxels < 0 || l.max_classes < 0 || l.max_iterations < 0 || l.max_working_bytes < 0 {
        return Err(input_err());
    }
    if l.max_voxels == 0 {
        l.max_voxels = d.max_voxels;
    }
    if l.max_classes == 0 {
        l.max_classes = d.max_classes;
    }
    if l.max_iterations == 0 {
        l.max_iterations = d.max_iterations;
    }
    if l.max_working_bytes == 0 {
        l.max_working_bytes = d.max_working_bytes;
    }
    o.limits = l;
    if l.max_iterations > 4096 {
        return Err(limit_err());
    }
    let mut n: i64 = 1;
    for (a, &x) in input.dimensions.iter().enumerate() {
        if x <= 0 || x > l.max_voxels / n {
            return Err(limit_err());
        }
        n *= x;
        let s = input.spacing_mm[a];
        if !finite(s) || s < 1e-9 || s > 1e9 {
            return Err(input_err());
        }
    }
    if input.values.len() as i64 != n
        || input.mask.is_some_and(|m| m.len() as i64 != n)
        || o.classes < 1
        || o.classes > 64
    {
        return Err(input_err());
    }
    if o.classes > l.max_classes {
        return Err(limit_err());
    }
    if input.units.len() > 256
        || input.normalization.len() > 256
        || input.units.is_empty()
        || input.normalization.is_empty()
        || input.units != o.units
        || input.normalization != o.normalization
    {
        return Err(Error::wrap(
            ERR_PROBABILISTIC_INPUT,
            "units/normalization mismatch",
        ));
    }
    if o.em_iterations == 0 {
        o.em_iterations = 100;
    }
    if o.variance_floor == 0.0 {
        o.variance_floor = 1e-6;
    }
    if o.em_tolerance == 0.0 {
        o.em_tolerance = 1e-8;
    }
    let mut m = o.mrf;
    if m.beta < 0.0
        || m.beta > 1e9
        || !finite(m.beta)
        || m.contrast_sigma < 0.0
        || !finite(m.contrast_sigma)
        || m.iterations < 0
    {
        return Err(input_err());
    }
    if m.beta > 0.0 && m.iterations == 0 {
        m.iterations = 20;
    }
    if m.tolerance == 0.0 {
        m.tolerance = 1e-6;
    }
    if m.damping == 0.0 {
        m.damping = 0.5;
    }
    o.mrf = m;
    if o.em_iterations < 1 || o.em_iterations > l.max_iterations || m.iterations > l.max_iterations
    {
        return Err(limit_err());
    }
    if !finite(o.variance_floor)
        || o.variance_floor < 1e-100
        || o.variance_floor > 1e200
        || !finite(o.em_tolerance)
        || o.em_tolerance <= 0.0
        || !finite(m.tolerance)
        || m.tolerance <= 0.0
        || !finite(m.damping)
        || m.damping <= 0.0
        || m.damping > 1.0
    {
        return Err(input_err());
    }
    if !o.initial.is_empty() && o.initial.len() as i64 != o.classes {
        return Err(input_err());
    }
    if !o.priors.is_empty() && o.priors.len() as i64 != o.classes {
        return Err(input_err());
    }
    let k = o.classes;
    if n > (i64::MAX - k * 1024 - 16384) / (k * 24 + 3) {
        return Err(limit_err());
    }
    if n > i64::MAX / k {
        return Err(limit_err());
    }
    let bytes = n * (k * 24 + 3) + k * 1024 + 16384;
    if bytes > l.max_working_bytes {
        return Err(limit_err());
    }
    if !o.initial.is_empty() {
        model(&mut o.initial, o.variance_floor)?;
    }
    if !o.priors.is_empty() {
        let mut sum = 0.0;
        for &p in &o.priors {
            if !finite(p) || p < 0.0 {
                return Err(input_err());
            }
            sum += p;
        }
        if !finite(sum) || sum <= 0.0 {
            return Err(input_err());
        }
    }
    let n = n as usize;
    let mut w = Work {
        input,
        o,
        valid: vec![false; n],
        n,
        k: k as usize,
        count: 0,
        skipped: 0,
        bytes,
        ctx,
    };
    for (i, &x) in input.values.iter().enumerate() {
        if i % 256 == 0 {
            poll(ctx)?;
        }
        if input.mask.is_some_and(|m| !m[i]) {
            continue;
        }
        if !finite(x) {
            if !w.o.skip_non_finite {
                return Err(input_err());
            }
            w.skipped += 1;
            continue;
        }
        if x.abs() > 1e100 {
            return Err(Error::wrap(
                ERR_PROBABILISTIC_INPUT,
                "scalar magnitude exceeds numeric domain",
            ));
        }
        w.valid[i] = true;
        w.count += 1;
    }
    if w.count == 0 {
        return Err(empty_err());
    }
    Ok(w)
}
fn model(c: &mut [GaussianClass], floor: f64) -> Result<(), Error> {
    let mut sum = 0.0;
    for v in c.iter_mut() {
        if !finite(v.mean)
            || v.mean.abs() > 1e100
            || !finite(v.variance)
            || v.variance < 0.0
            || v.variance > 1e201
            || !finite(v.weight)
            || v.weight <= 0.0
        {
            return Err(input_err());
        }
        v.variance = fmath::max(v.variance, floor);
        sum += v.weight;
    }
    if !finite(sum) || sum <= 0.0 {
        return Err(input_err());
    }
    for v in c.iter_mut() {
        v.weight /= sum;
    }
    let less = |a: &GaussianClass, b: &GaussianClass| {
        if a.mean != b.mean {
            return a.mean < b.mean;
        }
        if a.variance != b.variance {
            return a.variance < b.variance;
        }
        a.weight < b.weight
    };
    c.sort_by(|a, b| {
        if less(a, b) {
            std::cmp::Ordering::Less
        } else if less(b, a) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    Ok(())
}
fn scores_log(
    x: f64,
    c: &[GaussianClass],
    priors: Option<&[f64]>,
    s: &mut [f64],
    mut log_posterior: Option<&mut [f64]>,
) -> f64 {
    let mut mx = f64::NEG_INFINITY;
    let log_two_pi = fmath::log(2.0 * std::f64::consts::PI);
    for (j, v) in c.iter().enumerate() {
        let p = match priors {
            Some(pr) => pr[j],
            None => v.weight,
        };
        let delta = (x - v.mean) / v.variance.sqrt();
        s[j] = fmath::log(p) - 0.5 * (log_two_pi + fmath::log(v.variance) + delta * delta);
        mx = fmath::max(mx, s[j]);
    }
    let mut sum = 0.0;
    for &v in s.iter() {
        sum += fmath::exp(v - mx);
    }
    let log_sum = fmath::log(sum);
    for j in 0..s.len() {
        let v = s[j];
        if let Some(lp) = log_posterior.as_deref_mut() {
            lp[j] = v - mx - log_sum;
        }
        s[j] = fmath::exp(v - mx) / sum;
    }
    mx + log_sum
}
fn seed(w: &Work) -> Result<Vec<GaussianClass>, Error> {
    let mut c = vec![GaussianClass::default(); w.k];
    let mut r = Rand::new(w.o.seed);
    let target = r.intn(w.count).unwrap_or(0);
    let mut chosen: i64 = 0;
    let (mut mean, mut m2, mut count) = (0.0, 0.0, 0.0);
    for (i, &x) in w.input.values.iter().enumerate() {
        if i % 256 == 0 {
            poll(w.ctx)?;
        }
        if !w.valid[i] {
            continue;
        }
        if chosen == target {
            c[0].mean = x;
        }
        chosen += 1;
        count += 1.0;
        let delta = x - mean;
        mean += delta / count;
        m2 += delta * (x - mean);
    }
    let variance = fmath::max(m2 / count, w.o.variance_floor);
    let dist_of = |x: f64, c: &[GaussianClass], j: usize| {
        let mut dist = f64::INFINITY;
        for ca in c.iter().take(j) {
            let d = (x - ca.mean) / variance.sqrt();
            dist = fmath::min(dist, d * d);
        }
        dist
    };
    for j in 1..w.k {
        let mut total = 0.0;
        for (i, &x) in w.input.values.iter().enumerate() {
            if i % 256 == 0 {
                poll(w.ctx)?;
            }
            if !w.valid[i] {
                continue;
            }
            total += dist_of(x, &c, j);
        }
        if !finite(total) || total <= 0.0 {
            return Err(empty_err());
        }
        let mut pick = r.float64() * total;
        for (i, &x) in w.input.values.iter().enumerate() {
            if i % 256 == 0 {
                poll(w.ctx)?;
            }
            if !w.valid[i] {
                continue;
            }
            let dist = dist_of(x, &c, j);
            pick -= dist;
            if dist > 0.0 {
                c[j].mean = x;
            }
            if dist > 0.0 && pick <= 0.0 {
                c[j].mean = x;
                break;
            }
        }
    }
    for v in c.iter_mut() {
        v.variance = variance;
        v.weight = 1.0 / w.k as f64;
    }
    Ok(c)
}
pub fn fit_gaussian_mixture(
    ctx: Option<&dyn Cancellation>,
    input: &ScalarClassificationInput,
    o: &ProbabilisticOptions,
) -> Result<ProbabilisticResult, Error> {
    let w = prepare(ctx, input, o)?;
    if w.count < w.k as i64 {
        return Err(empty_err());
    }
    let mut c = w.o.initial.clone();
    if c.is_empty() {
        c = seed(&w)?;
    }
    model(&mut c, w.o.variance_floor)?;
    let k = w.k;
    let mut scores = vec![0.0; k];
    let mut mass = vec![0.0; k];
    let mut means = vec![0.0; k];
    let mut m2 = vec![0.0; k];
    let mut previous = f64::NEG_INFINITY;
    let mut iterations = 0;
    let mut converged = false;
    for it in 0..w.o.em_iterations {
        mass.fill(0.0);
        means.fill(0.0);
        m2.fill(0.0);
        let mut ll = 0.0;
        for (i, &x) in input.values.iter().enumerate() {
            if i % 256 == 0 {
                poll(w.ctx)?;
            }
            if !w.valid[i] {
                continue;
            }
            ll += scores_log(x, &c, None, &mut scores, None);
            for (j, &p) in scores.iter().enumerate() {
                let next = mass[j] + p;
                if p == 0.0 {
                    continue;
                }
                let delta = x - means[j];
                means[j] += p / next * delta;
                m2[j] += p * delta * (x - means[j]);
                mass[j] = next;
            }
        }
        if !finite(ll) {
            return Err(input_err());
        }
        if it > 0 && (ll - previous).abs() <= w.o.em_tolerance * (1.0 + previous.abs()) {
            converged = true;
            break;
        }
        previous = ll;
        for j in 0..k {
            if mass[j] < 1e-12 {
                return Err(empty_err());
            }
            c[j] = GaussianClass {
                mean: means[j],
                variance: fmath::max(m2[j] / mass[j], w.o.variance_floor),
                weight: mass[j] / w.count as f64,
            };
        }
        iterations += 1;
    }
    model(&mut c, w.o.variance_floor)?;
    let mut result = finish(&w, c)?;
    result.em_iterations = iterations;
    result.em_converged = converged;
    Ok(result)
}
pub fn classify_gaussian_bayes(
    ctx: Option<&dyn Cancellation>,
    input: &ScalarClassificationInput,
    model_in: &[GaussianClass],
    o: &ProbabilisticOptions,
) -> Result<ProbabilisticResult, Error> {
    let mut o = o.clone();
    if o.classes == 0 {
        o.classes = model_in.len() as i64;
    }
    let w = prepare(ctx, input, &o)?;
    if model_in.len() != w.k {
        return Err(input_err());
    }
    let mut c = model_in.to_vec();
    model(&mut c, w.o.variance_floor)?;
    finish(&w, c)
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn json_float(out: &mut String, f: f64) -> Result<(), Error> {
    if !f.is_finite() {
        let text = if f.is_nan() {
            "NaN"
        } else if f > 0.0 {
            "+Inf"
        } else {
            "-Inf"
        };
        return Err(Error::msg(format!("json: unsupported value: {text}")));
    }
    out.push_str(&jsonwire::format_float(f));
    Ok(())
}
fn json_classes(out: &mut String, c: &[GaussianClass], null_if_empty: bool) -> Result<(), Error> {
    if c.is_empty() && null_if_empty {
        out.push_str("null");
        return Ok(());
    }
    out.push('[');
    for (i, v) in c.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"Mean\":");
        json_float(out, v.mean)?;
        out.push_str(",\"Variance\":");
        json_float(out, v.variance)?;
        out.push_str(",\"Weight\":");
        json_float(out, v.weight)?;
        out.push('}');
    }
    out.push(']');
    Ok(())
}
fn parameters_json(o: &ProbabilisticOptions, c: &[GaussianClass]) -> Result<String, Error> {
    let mut s = String::new();
    s.push_str("{\"Options\":{\"Classes\":");
    s.push_str(&o.classes.to_string());
    s.push_str(",\"EMIterations\":");
    s.push_str(&o.em_iterations.to_string());
    s.push_str(",\"Seed\":");
    s.push_str(&o.seed.to_string());
    s.push_str(",\"VarianceFloor\":");
    json_float(&mut s, o.variance_floor)?;
    s.push_str(",\"EMTolerance\":");
    json_float(&mut s, o.em_tolerance)?;
    s.push_str(",\"Initial\":");
    json_classes(&mut s, &o.initial, true)?;
    s.push_str(",\"Priors\":");
    if o.priors.is_empty() {
        s.push_str("null");
    } else {
        s.push('[');
        for (i, &p) in o.priors.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            json_float(&mut s, p)?;
        }
        s.push(']');
    }
    s.push_str(",\"SkipNonFinite\":");
    s.push_str(if o.skip_non_finite { "true" } else { "false" });
    s.push_str(",\"Units\":");
    jsonwire::append_string(&mut s, &o.units);
    s.push_str(",\"Normalization\":");
    jsonwire::append_string(&mut s, &o.normalization);
    s.push_str(",\"MRF\":{\"Beta\":");
    json_float(&mut s, o.mrf.beta)?;
    s.push_str(",\"ContrastSigma\":");
    json_float(&mut s, o.mrf.contrast_sigma)?;
    s.push_str(",\"Tolerance\":");
    json_float(&mut s, o.mrf.tolerance)?;
    s.push_str(",\"Damping\":");
    json_float(&mut s, o.mrf.damping)?;
    s.push_str(&format!(",\"Iterations\":{}}}", o.mrf.iterations));
    let l = o.limits;
    s.push_str(&format!(
        ",\"Limits\":{{\"MaxVoxels\":{},\"MaxClasses\":{},\"MaxIterations\":{},\"MaxWorkingBytes\":{}}}}}",
        l.max_voxels, l.max_classes, l.max_iterations, l.max_working_bytes
    ));
    s.push_str(",\"Model\":");
    json_classes(&mut s, c, true)?;
    s.push('}');
    Ok(s)
}
fn finish(w: &Work, c: Vec<GaussianClass>) -> Result<ProbabilisticResult, Error> {
    let k = w.k;
    let mut priors: Vec<f64> = c.iter().map(|v| v.weight).collect();
    if !w.o.priors.is_empty() {
        priors.copy_from_slice(&w.o.priors);
        let mut sum = 0.0;
        for &p in &priors {
            if !finite(p) || p < 0.0 {
                return Err(input_err());
            }
            sum += p;
        }
        if !finite(sum) || sum <= 0.0 {
            return Err(input_err());
        }
        for p in priors.iter_mut() {
            *p /= sum;
        }
    }
    let input = w.input;
    let mut r = ProbabilisticResult {
        dimensions: input.dimensions,
        spacing_mm: input.spacing_mm,
        classes: c,
        priors,
        labels: vec![0; w.n],
        posteriors: vec![0.0; w.n * k],
        selected_voxels: w.count,
        skipped_non_finite: w.skipped,
        working_bytes: w.bytes,
        method: "scalar Gaussian Bayes".into(),
        units: input.units.clone(),
        normalization: input.normalization.clone(),
        ..Default::default()
    };
    let mut scores = vec![0.0; k];
    let mrf = w.o.mrf.beta > 0.0;
    let mut log_unary = if mrf {
        vec![0.0; r.posteriors.len()]
    } else {
        Vec::new()
    };
    for (i, &x) in input.values.iter().enumerate() {
        if i % 256 == 0 {
            poll(w.ctx)?;
        }
        if !w.valid[i] {
            continue;
        }
        let row = if mrf {
            Some(&mut log_unary[i * k..(i + 1) * k])
        } else {
            None
        };
        r.log_likelihood += scores_log(x, &r.classes, Some(&r.priors), &mut scores, row);
        r.posteriors[i * k..(i + 1) * k].copy_from_slice(&scores);
    }
    if mrf {
        mrf_mean_field(w, &mut r, &log_unary)?;
        r.method = "scalar Gaussian Bayes + physical Potts mean-field".into();
    }
    for i in 0..r.labels.len() {
        if i % 256 == 0 {
            poll(w.ctx)?;
        }
        if !w.valid[i] {
            continue;
        }
        let mut best = 0;
        for j in 1..k {
            if r.posteriors[i * k + j] > r.posteriors[i * k + best] {
                best = j;
            }
        }
        r.labels[i] = (best + 1) as u16;
    }
    let mut sh = Sha256::new();
    for &d in &input.dimensions {
        sh.update((d as u64).to_le_bytes());
    }
    for &s in &input.spacing_mm {
        sh.update(s.to_bits().to_le_bytes());
    }
    for (i, &x) in input.values.iter().enumerate() {
        if i % 256 == 0 {
            poll(w.ctx)?;
        }
        sh.update(x.to_bits().to_le_bytes());
        if input.mask.is_none_or(|m| m[i]) {
            sh.update([1u8]);
        } else {
            sh.update([0u8]);
        }
    }
    for id in [&input.units, &input.normalization] {
        sh.update((id.len() as u64).to_le_bytes());
        sh.update(id.as_bytes());
    }
    r.source_sha256 = hex(&sh.finalize());
    let payload = parameters_json(&w.o, &r.classes)?;
    r.parameters_sha256 = hex(&Sha256::digest(payload.as_bytes()));
    poll(w.ctx)?;
    Ok(r)
}
fn mrf_mean_field(w: &Work, r: &mut ProbabilisticResult, unary: &[f64]) -> Result<(), Error> {
    let k = w.k;
    let mut next = vec![0.0; r.posteriors.len()];
    let mut scores = vec![0.0; k];
    let dims = w.input.dimensions;
    let (nx, ny) = (dims[0] as usize, dims[1] as usize);
    let plane = nx * ny;
    let m = w.o.mrf;
    for iteration in 0..m.iterations {
        let mut change: f64 = 0.0;
        for i in 0..w.n {
            if i % 128 == 0 {
                poll(w.ctx)?;
            }
            if !w.valid[i] {
                continue;
            }
            scores.copy_from_slice(&unary[i * k..(i + 1) * k]);
            let pos = [i % nx, (i / nx) % ny, i / plane];
            for (axis, step) in [1, nx, plane].into_iter().enumerate() {
                let coord = pos[axis] as i64;
                for direction in [-1i64, 1] {
                    if coord + direction < 0 || coord + direction >= dims[axis] {
                        continue;
                    }
                    let neighbor = if direction < 0 { i - step } else { i + step };
                    if !w.valid[neighbor] {
                        continue;
                    }
                    let mut weight = m.beta / w.input.spacing_mm[axis];
                    if m.contrast_sigma > 0.0 {
                        let d = (w.input.values[i] - w.input.values[neighbor]) / m.contrast_sigma;
                        weight *= fmath::exp(-0.5 * d * d);
                    }
                    for (j, s) in scores.iter_mut().enumerate() {
                        *s += weight * r.posteriors[neighbor * k + j];
                    }
                }
            }
            let mut mx = f64::NEG_INFINITY;
            for &v in &scores {
                mx = fmath::max(mx, v);
            }
            let mut sum = 0.0;
            for s in scores.iter_mut() {
                *s = fmath::exp(*s - mx);
                sum += *s;
            }
            for (j, &s) in scores.iter().enumerate() {
                let index = i * k + j;
                let p = (1.0 - m.damping) * r.posteriors[index] + m.damping * s / sum;
                next[index] = p;
                change = fmath::max(change, (p - r.posteriors[index]).abs());
            }
        }
        std::mem::swap(&mut r.posteriors, &mut next);
        r.mrf_iterations = iteration + 1;
        if change <= m.tolerance {
            r.mrf_converged = true;
            break;
        }
    }
    poll(w.ctx)
}
