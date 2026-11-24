#![allow(clippy::needless_range_loop)]
use super::histogram::{
    ERR_NOT_BIMODAL, ERR_TOO_MANY_BINS, Histogram, HistogramOptions, Values, new_histogram,
};
use crate::fmath;
use crate::roi::Error;
use crate::textfmt::quote;
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Method(pub String);
pub const OTSU: &str = "otsu";
pub const HUANG: &str = "huang";
pub const INTERMODES: &str = "intermodes";
pub const MINIMUM: &str = "minimum";
pub const ISO_DATA: &str = "isodata";
pub const KITTLER_ILLINGWORTH: &str = "kittler-illingworth";
pub const LI: &str = "li";
pub const MAX_ENTROPY: &str = "max-entropy";
pub const MEAN: &str = "mean";
pub const MOMENTS: &str = "moments";
pub const RENYI_ENTROPY: &str = "renyi-entropy";
pub const SHANBHAG: &str = "shanbhag";
pub const TRIANGLE: &str = "triangle";
pub const YEN: &str = "yen";
const METHOD_LABELS: [(&str, &str); 14] = [
    (OTSU, "Otsu"),
    (HUANG, "Huang (fuzzy)"),
    (INTERMODES, "Intermodes"),
    (MINIMUM, "Minimum"),
    (ISO_DATA, "IsoData (Ridler–Calvard)"),
    (KITTLER_ILLINGWORTH, "Kittler–Illingworth (minimum error)"),
    (LI, "Li (minimum cross entropy)"),
    (MAX_ENTROPY, "Maximum entropy (Kapur)"),
    (MEAN, "Mean"),
    (MOMENTS, "Moments (Tsai)"),
    (RENYI_ENTROPY, "Rényi entropy"),
    (SHANBHAG, "Shanbhag"),
    (TRIANGLE, "Triangle"),
    (YEN, "Yen"),
];
pub fn methods() -> Vec<Method> {
    METHOD_LABELS
        .iter()
        .map(|(name, _)| Method::new(name))
        .collect()
}
impl Method {
    pub fn new(name: &str) -> Method {
        Method(name.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn label(&self) -> String {
        METHOD_LABELS
            .iter()
            .find(|(name, _)| *name == self.0)
            .map_or_else(|| self.0.clone(), |(_, label)| (*label).to_string())
    }
}
fn trim_space(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}'
        ) || (c as u32 > 0xff && c.is_whitespace())
    })
}
fn equal_fold_target(s: &str, target: &str) -> bool {
    let mut a = s.chars();
    let mut b = target.chars();
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(c), Some(d)) => {
                let same = c == d
                    || (d.is_ascii_alphabetic() && c.eq_ignore_ascii_case(&d))
                    || (matches!(d, 'k' | 'K') && c == '\u{212a}')
                    || (matches!(d, 's' | 'S') && c == '\u{17f}')
                    || (d == 'é' && c == 'É');
                if !same {
                    return false;
                }
            }
            _ => return false,
        }
    }
}
pub fn parse_method(s: &str) -> Result<Method, Error> {
    let s = trim_space(s);
    for (name, label) in METHOD_LABELS {
        if equal_fold_target(s, name) || equal_fold_target(s, label) {
            return Ok(Method::new(name));
        }
    }
    Err(Error::msg(format!(
        "autothreshold: unknown method {}",
        quote(s)
    )))
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Threshold {
    pub bin: i64,
    pub value: f64,
    pub center: f64,
}
pub fn class_of(thresholds: &[Threshold], v: f64) -> i64 {
    thresholds.iter().filter(|t| v >= t.value).count() as i64
}
const MAX_QUADRATIC_BINS: usize = 4096;
pub fn compute(
    values: Option<Values<'_>>,
    method: &Method,
    opts: HistogramOptions,
) -> Result<Threshold, Error> {
    new_histogram(values, opts)?.threshold(method)
}
impl Histogram {
    fn threshold_at(&self, bin: usize) -> Threshold {
        Threshold {
            bin: bin as i64,
            value: self.bin_lower(bin as i64 + 1),
            center: self.bin_center(bin as i64),
        }
    }
    pub fn threshold(&self, method: &Method) -> Result<Threshold, Error> {
        let (lo, hi) = self.span()?;
        let name = method.as_str();
        if matches!(name, HUANG | SHANBHAG | INTERMODES | MINIMUM)
            && hi - lo + 1 > MAX_QUADRATIC_BINS
        {
            return Err(Error::wrap(
                ERR_TOO_MANY_BINS,
                format!(
                    "{} on {} occupied bins, at most {}",
                    name,
                    hi - lo + 1,
                    MAX_QUADRATIC_BINS
                ),
            ));
        }
        let p = self.normalized();
        let (lo_i, hi_i) = (lo as i64, hi as i64);
        let k: i64 = match name {
            OTSU => otsu(&p, lo, hi) as i64,
            HUANG => huang(&p, lo, hi) as i64,
            INTERMODES | MINIMUM => modes(&self.counts, lo, hi, name == INTERMODES)? as i64,
            ISO_DATA => iso_data(&p, lo, hi) as i64,
            KITTLER_ILLINGWORTH => kittler_illingworth(&p, lo, hi) as i64,
            LI => li(&p, lo, hi),
            MAX_ENTROPY => renyi(&p, lo, hi, 1.0) as i64,
            MEAN => fmath::f64_to_i64(mean(&p, lo, hi).floor()),
            MOMENTS => moments(&p, lo, hi) as i64,
            RENYI_ENTROPY => renyi_combined(&p, lo, hi),
            SHANBHAG => shanbhag(&p, lo, hi) as i64,
            TRIANGLE => triangle(&p, lo, hi) as i64,
            YEN => yen(&p, lo, hi) as i64,
            _ => {
                return Err(Error::msg(format!(
                    "autothreshold: unknown method {}",
                    quote(name)
                )));
            }
        };
        Ok(self.threshold_at(clamp_split(k, lo_i, hi_i) as usize))
    }
    pub fn multi_otsu(&self, classes: i64) -> Result<Vec<Threshold>, Error> {
        if classes < 2 {
            return Err(Error::msg(format!(
                "autothreshold: multi-Otsu needs at least 2 classes, got {classes}"
            )));
        }
        let (lo, hi) = self.span()?;
        let occupied = self.counts[lo..=hi].iter().filter(|&&c| c > 0.0).count();
        if (occupied as i64) < classes {
            return Err(Error::msg(format!(
                "autothreshold: {classes} classes need at least as many occupied bins, found {occupied}"
            )));
        }
        if hi - lo + 1 > MAX_QUADRATIC_BINS {
            return Err(Error::wrap(
                ERR_TOO_MANY_BINS,
                format!("multi-Otsu on {} occupied bins", hi - lo + 1),
            ));
        }
        let bins = multi_otsu(&self.normalized(), lo, hi, classes as usize);
        Ok(bins.into_iter().map(|k| self.threshold_at(k)).collect())
    }
    fn normalized(&self) -> Vec<f64> {
        let total = self.total();
        self.counts.iter().map(|&c| c / total).collect()
    }
}
fn clamp_split(k: i64, lo: i64, hi: i64) -> i64 {
    if k < lo {
        return lo;
    }
    if k > hi - 1 {
        return hi - 1;
    }
    k
}
fn mean(p: &[f64], lo: usize, hi: usize) -> f64 {
    let mut m = 0.0;
    for i in lo..=hi {
        m += i as f64 * p[i];
    }
    m
}
fn otsu(p: &[f64], lo: usize, hi: usize) -> usize {
    let total = mean(p, lo, hi);
    let (mut best, mut best_k) = (-1.0, lo);
    let (mut w0, mut s0) = (0.0, 0.0);
    for k in lo..hi {
        w0 += p[k];
        s0 += k as f64 * p[k];
        let w1 = 1.0 - w0;
        if w0 <= 0.0 || w1 <= 0.0 {
            continue;
        }
        let d = total * w0 - s0;
        let v = d * d / (w0 * w1);
        if v > best {
            (best, best_k) = (v, k);
        }
    }
    best_k
}
fn multi_otsu(p: &[f64], lo: usize, hi: usize, classes: usize) -> Vec<usize> {
    let n = hi - lo + 1;
    let mut prefix_p = vec![0.0f64; n + 1];
    let mut prefix_s = vec![0.0f64; n + 1];
    for i in 0..n {
        prefix_p[i + 1] = prefix_p[i] + p[lo + i];
        prefix_s[i + 1] = prefix_s[i] + i as f64 * p[lo + i];
    }
    let term = |a: usize, b: usize| -> f64 {
        let w = prefix_p[b] - prefix_p[a];
        if w <= 0.0 {
            return 0.0;
        }
        let s = prefix_s[b] - prefix_s[a];
        s * s / w
    };
    let mut best = vec![vec![f64::NEG_INFINITY; n + 1]; classes];
    let mut from = vec![vec![0usize; n + 1]; classes];
    for j in 1..=n {
        best[0][j] = term(0, j);
    }
    for c in 1..classes {
        for j in c + 1..=n {
            for i in c..j {
                let v = best[c - 1][i] + term(i, j);
                if v > best[c][j] {
                    best[c][j] = v;
                    from[c][j] = i;
                }
            }
        }
    }
    let mut splits = vec![0usize; classes - 1];
    let mut j = n;
    for c in (1..classes).rev() {
        let i = from[c][j];
        splits[c - 1] = (lo + i).saturating_sub(1);
        j = i;
    }
    splits
}
fn huang(p: &[f64], lo: usize, hi: usize) -> usize {
    let c = (hi - lo) as f64;
    let shannon = |u: f64| -> f64 {
        if u <= 0.0 || u >= 1.0 {
            return 0.0;
        }
        -u * fmath::log(u) - (1.0 - u) * fmath::log(1.0 - u)
    };
    let (mut best, mut best_k) = (f64::INFINITY, lo);
    let (mut w0, mut s0) = (0.0, 0.0);
    let (mut total_w, mut total_s) = (0.0, 0.0);
    for i in lo..=hi {
        total_w += p[i];
        total_s += i as f64 * p[i];
    }
    for k in lo..hi {
        w0 += p[k];
        s0 += k as f64 * p[k];
        if w0 <= 0.0 || total_w - w0 <= 0.0 {
            continue;
        }
        let (m0, m1) = (s0 / w0, (total_s - s0) / (total_w - w0));
        let mut e = 0.0;
        for i in lo..=hi {
            if p[i] == 0.0 {
                continue;
            }
            let m = if i <= k { m0 } else { m1 };
            e += p[i] * shannon(1.0 / (1.0 + (i as f64 - m).abs() / c));
        }
        if e < best {
            (best, best_k) = (e, k);
        }
    }
    best_k
}
fn smooth_to_peaks(c: &[f64], lo: usize, hi: usize) -> Result<(Vec<f64>, Vec<usize>), Error> {
    const MAX_ITERATIONS: usize = 10000;
    let mut y = c[lo..=hi].to_vec();
    let mut next = vec![0.0f64; y.len()];
    let mut peaks: Vec<usize> = Vec::new();
    let mut iteration = 0;
    loop {
        if iteration == MAX_ITERATIONS {
            return Err(Error::wrap(
                ERR_NOT_BIMODAL,
                format!(
                    "still {} peaks after {} smoothings",
                    peaks.len(),
                    MAX_ITERATIONS
                ),
            ));
        }
        let n = y.len();
        for i in 0..n {
            let left = y[i.saturating_sub(1)];
            let right = y[(i + 1).min(n - 1)];
            next[i] = (left + y[i] + right) / 3.0;
        }
        std::mem::swap(&mut y, &mut next);
        peaks = peaks_of(&y);
        if peaks.len() < 3 {
            break;
        }
        iteration += 1;
    }
    if peaks.len() != 2 {
        return Err(Error::wrap(
            ERR_NOT_BIMODAL,
            format!("{} peak", peaks.len()),
        ));
    }
    Ok((y, peaks))
}
fn peaks_of(y: &[f64]) -> Vec<usize> {
    let mut peaks = Vec::new();
    let (mut rising, mut rose) = (true, false);
    for i in 0..y.len().saturating_sub(1) {
        if rising && y[i + 1] < y[i] {
            peaks.push(i);
            rising = false;
        } else if y[i + 1] > y[i] {
            rising = true;
            rose = true;
        }
    }
    if rising && rose && y.len() > 1 {
        peaks.push(y.len() - 1);
    }
    peaks
}
fn modes(counts: &[f64], lo: usize, hi: usize, intermodes: bool) -> Result<usize, Error> {
    let (y, peaks) = smooth_to_peaks(counts, lo, hi)?;
    if intermodes {
        return Ok(lo + (peaks[0] + peaks[1]) / 2);
    }
    let mut k = peaks[0];
    for i in peaks[0] + 1..=peaks[1] {
        if y[i] < y[k] {
            k = i;
        }
    }
    Ok(lo + k)
}
fn iso_data(p: &[f64], lo: usize, hi: usize) -> usize {
    let (mut total_w, mut total_s) = (0.0, 0.0);
    for i in lo..=hi {
        total_w += p[i];
        total_s += i as f64 * p[i];
    }
    let (mut w0, mut s0) = (0.0, 0.0);
    for k in lo..hi {
        w0 += p[k];
        s0 += k as f64 * p[k];
        if w0 <= 0.0 {
            continue;
        }
        let mid = (s0 / w0 + (total_s - s0) / (total_w - w0)) / 2.0;
        if mid >= k as f64 && mid < (k + 1) as f64 {
            return k;
        }
    }
    hi - 1
}
fn kittler_illingworth(p: &[f64], lo: usize, hi: usize) -> usize {
    let (mut total_w, mut total_s, mut total_q) = (0.0, 0.0, 0.0);
    for i in lo..=hi {
        let x = (i - lo) as f64;
        total_w += p[i];
        total_s += x * p[i];
        total_q += x * x * p[i];
    }
    let (mut best, mut best_k) = (f64::INFINITY, lo);
    let (mut w0, mut s0, mut q0) = (0.0, 0.0, 0.0);
    for k in lo..hi {
        let x = (k - lo) as f64;
        w0 += p[k];
        s0 += x * p[k];
        q0 += x * x * p[k];
        let w1 = total_w - w0;
        if w0 <= 0.0 || w1 <= 0.0 {
            continue;
        }
        let (m0, m1) = (s0 / w0, (total_s - s0) / w1);
        let v0 = fmath::max(q0 / w0 - m0 * m0, 0.0) + 1.0 / 12.0;
        let v1 = fmath::max((total_q - q0) / w1 - m1 * m1, 0.0) + 1.0 / 12.0;
        let j = w0 * fmath::log(v0) + w1 * fmath::log(v1)
            - 2.0 * (w0 * fmath::log(w0) + w1 * fmath::log(w1));
        if j < best {
            (best, best_k) = (j, k);
        }
    }
    best_k
}
fn li(p: &[f64], lo: usize, hi: usize) -> i64 {
    const MAX_ITERATIONS: usize = 1000;
    let mut next = mean(p, lo, hi) - lo as f64;
    let mut current = -1.0;
    let mut iteration = 0;
    while (next - current).abs() > 0.5 && iteration < MAX_ITERATIONS {
        current = next;
        let (mut w0, mut s0, mut w1, mut s1) = (0.0, 0.0, 0.0, 0.0);
        for i in lo..=hi {
            let x = (i - lo) as f64;
            if x > current {
                w1 += p[i];
                s1 += x * p[i];
            } else {
                w0 += p[i];
                s0 += x * p[i];
            }
        }
        if w0 <= 0.0 || w1 <= 0.0 {
            break;
        }
        let (m0, m1) = (s0 / w0, s1 / w1);
        if m0 == 0.0 {
            break;
        }
        next = (m0 - m1) / (fmath::log(m0) - fmath::log(m1));
        iteration += 1;
    }
    (lo as i64).wrapping_add(fmath::f64_to_i64(next.floor()))
}
fn renyi(p: &[f64], lo: usize, hi: usize, alpha: f64) -> usize {
    let f = |v: f64| -> f64 {
        if v <= 0.0 {
            return 0.0;
        }
        if alpha == 1.0 {
            return v * fmath::log(v);
        }
        fmath::pow(v, alpha)
    };
    let entropy = |w: f64, s: f64| -> f64 {
        if alpha == 1.0 {
            return fmath::log(w) - s / w;
        }
        (fmath::log(s) - alpha * fmath::log(w)) / (1.0 - alpha)
    };
    let (mut total_w, mut total_f) = (0.0, 0.0);
    for i in lo..=hi {
        total_w += p[i];
        total_f += f(p[i]);
    }
    let (mut best, mut best_k) = (f64::NEG_INFINITY, lo);
    let (mut w0, mut f0) = (0.0, 0.0);
    for k in lo..hi {
        w0 += p[k];
        f0 += f(p[k]);
        let (w1, f1) = (total_w - w0, total_f - f0);
        if w0 <= 0.0 || w1 <= 0.0 || (alpha != 1.0 && (f0 <= 0.0 || f1 <= 0.0)) {
            continue;
        }
        let h = entropy(w0, f0) + entropy(w1, f1);
        if h > best {
            (best, best_k) = (h, k);
        }
    }
    best_k
}
fn renyi_combined(p: &[f64], lo: usize, hi: usize) -> i64 {
    let mut t = [
        renyi(p, lo, hi, 0.5),
        renyi(p, lo, hi, 1.0),
        renyi(p, lo, hi, 2.0),
    ];
    if t[0] > t[1] {
        t.swap(0, 1);
    }
    if t[1] > t[2] {
        t.swap(1, 2);
    }
    if t[0] > t[1] {
        t.swap(0, 1);
    }
    let cumulative = |k: usize| -> f64 {
        let mut s = 0.0;
        for &v in &p[lo..=k] {
            s += v;
        }
        s
    };
    let near = |a: usize, b: usize| a.abs_diff(b) <= 5;
    let (b1, b2, b3) = if near(t[0], t[1]) && !near(t[1], t[2]) {
        (0.0, 1.0, 3.0)
    } else if !near(t[0], t[1]) && near(t[1], t[2]) {
        (3.0, 1.0, 0.0)
    } else {
        (1.0, 2.0, 1.0)
    };
    let (p1, p3) = (cumulative(t[0]), cumulative(t[2]));
    let omega = p3 - p1;
    let (x1, x2, x3) = ((t[0] - lo) as f64, (t[1] - lo) as f64, (t[2] - lo) as f64);
    let combined =
        x1 * (p1 + omega * b1 / 4.0) + x2 * omega * b2 / 4.0 + x3 * (1.0 - p3 + omega * b3 / 4.0);
    (lo as i64).wrapping_add(fmath::f64_to_i64((combined + 1e-7).floor()))
}
fn moments(p: &[f64], lo: usize, hi: usize) -> usize {
    let m = mean(p, lo, hi);
    let (mut m2, mut m3) = (0.0, 0.0);
    for i in lo..=hi {
        let d = i as f64 - m;
        m2 += d * d * p[i];
        m3 += d * d * d * p[i];
    }
    let sd = m2.sqrt();
    let skew = m3 / (sd * sd * sd);
    let root = (skew * skew + 4.0).sqrt();
    let (z0, z1) = ((skew - root) / 2.0, (skew + root) / 2.0);
    let p0 = z1 / (z1 - z0);
    let mut cumulative = 0.0;
    for k in lo..hi {
        cumulative += p[k];
        if cumulative >= p0 {
            return k;
        }
    }
    hi - 1
}
fn shanbhag(p: &[f64], lo: usize, hi: usize) -> usize {
    let (mut best, mut best_k) = (f64::INFINITY, lo);
    for k in lo..hi {
        let mut w0 = 0.0;
        for &v in &p[lo..=k] {
            w0 += v;
        }
        let w1 = 1.0 - w0;
        if w0 <= 0.0 || w1 <= 0.0 {
            continue;
        }
        let (mut e0, mut tail) = (0.0, 0.0);
        for i in (lo..=k).rev() {
            tail += p[i];
            if p[i] > 0.0 {
                e0 -= p[i] * fmath::log(0.5 + tail / (2.0 * w0));
            }
        }
        let (mut e1, mut head) = (0.0, 0.0);
        for &v in &p[k + 1..=hi] {
            head += v;
            if v > 0.0 {
                e1 -= v * fmath::log(0.5 + head / (2.0 * w1));
            }
        }
        let d = (e0 / w0 - e1 / w1).abs();
        if d < best {
            (best, best_k) = (d, k);
        }
    }
    best_k
}
fn triangle(p: &[f64], lo: usize, hi: usize) -> usize {
    let mut peak = lo;
    for i in lo..=hi {
        if p[i] > p[peak] {
            peak = i;
        }
    }
    let height = p[peak];
    let (mut best, mut best_k) = (f64::NEG_INFINITY, lo);
    if hi - peak > peak - lo {
        let width = (hi - peak) as f64;
        for i in (peak + 1..=hi).rev() {
            let d = height * (hi - i) as f64 - width * p[i];
            if d > best {
                (best, best_k) = (d, i);
            }
        }
        return best_k;
    }
    let width = (peak - lo) as f64;
    for i in lo..peak {
        let d = height * (i - lo) as f64 - width * p[i];
        if d > best {
            (best, best_k) = (d, i);
        }
    }
    best_k
}
fn yen(p: &[f64], lo: usize, hi: usize) -> usize {
    let mut total_q = 0.0;
    for &v in &p[lo..=hi] {
        total_q += v * v;
    }
    let (mut best, mut best_k) = (f64::NEG_INFINITY, lo);
    let (mut w0, mut q0) = (0.0, 0.0);
    for k in lo..hi {
        w0 += p[k];
        q0 += p[k] * p[k];
        let (w1, q1) = (1.0 - w0, total_q - q0);
        if w0 <= 0.0 || w1 <= 0.0 || q0 <= 0.0 || q1 <= 0.0 {
            continue;
        }
        let c = -fmath::log(q0) - fmath::log(q1) + 2.0 * fmath::log(w0) + 2.0 * fmath::log(w1);
        if c > best {
            (best, best_k) = (c, k);
        }
    }
    best_k
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_stay_within_the_folding_rules() {
        for (name, label) in METHOD_LABELS {
            for c in name.chars().chain(label.chars()) {
                assert!(c.is_ascii() || c == 'é' || c == '–', "{c:?}");
            }
        }
    }
    #[test]
    fn parse_ignores_case() {
        assert_eq!(parse_method("  OTSU\n").unwrap().as_str(), OTSU);
        assert_eq!(
            parse_method("RÉNYI ENTROPY").unwrap().as_str(),
            RENYI_ENTROPY
        );
        assert_eq!(parse_method("\u{17f}hanbhag").unwrap().as_str(), SHANBHAG);
        assert_eq!(
            parse_method("\u{212a}ittler-illingworth").unwrap().as_str(),
            KITTLER_ILLINGWORTH
        );
        assert!(parse_method("ots\u{131}").is_err());
        assert_eq!(Method::new("nope").label(), "nope");
    }
}
