use super::voi::clamp;
use super::{DisplayError, Gray16Image, Lut};
use crate::{fmath, textfmt};
use std::cmp::Ordering;
use std::fmt;
pub const GSDF_MIN_JND: f64 = 1.0;
pub const GSDF_MAX_JND: f64 = 1023.0;
pub const DEFAULT_ILLUMINATION: f64 = 2000.0;
pub const DEFAULT_REFLECTED_AMBIENT_LIGHT: f64 = 10.0;
const INVALID_CURVE: &str = "dicom/display: invalid characteristic curve";
const UNSUPPORTED_PRESENTATION_LUT: &str = "dicom/display: unsupported Presentation LUT";
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationError {
    InvalidCurve(String),
    UnsupportedPresentationLut(String),
    LineTooLong,
    Lut(DisplayError),
}
impl fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalibrationError::InvalidCurve(detail) => write!(f, "{INVALID_CURVE}: {detail}"),
            CalibrationError::UnsupportedPresentationLut(detail) => {
                write!(f, "{UNSUPPORTED_PRESENTATION_LUT}: {detail}")
            }
            CalibrationError::LineTooLong => f.write_str("line too long"),
            CalibrationError::Lut(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for CalibrationError {}
fn invalid(detail: String) -> CalibrationError {
    CalibrationError::InvalidCurve(detail)
}
pub fn gsdf_luminance(j: f64) -> f64 {
    const A: f64 = -1.3011877;
    const B: f64 = -2.5840191e-2;
    const C: f64 = 8.0242636e-2;
    const D: f64 = -1.0320229e-1;
    const E: f64 = 1.3646699e-1;
    const F: f64 = 2.8745620e-2;
    const G: f64 = -2.5468404e-2;
    const H: f64 = -3.1978977e-3;
    const K: f64 = 1.2992634e-4;
    const M: f64 = 1.3635334e-3;
    let x = fmath::log(j);
    let x2 = x * x;
    let x3 = x2 * x;
    let x4 = x3 * x;
    let x5 = x4 * x;
    fmath::pow(
        10.0,
        (A + C * x + E * x2 + G * x3 + M * x4) / (1.0 + B * x + D * x2 + F * x3 + H * x4 + K * x5),
    )
}
pub fn gsdf_jnd_index(l: f64) -> f64 {
    const COEFFICIENTS: [f64; 9] = [
        71.498068,
        94.593053,
        41.912053,
        9.8247004,
        0.28175407,
        -1.1878455,
        -0.18014349,
        0.14710899,
        -0.017046845,
    ];
    let x = fmath::log10(l);
    let (mut out, mut power) = (0.0, 1.0);
    for coefficient in COEFFICIENTS {
        out += coefficient * power;
        power *= x;
    }
    out
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DeviceType {
    #[default]
    Monitor,
    Printer,
}
impl DeviceType {
    pub fn from_index(value: i64) -> DeviceType {
        if value == 1 {
            DeviceType::Printer
        } else {
            DeviceType::Monitor
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharacteristicCurve {
    pub device_type: DeviceType,
    pub max_ddl: i64,
    pub ddl: Vec<i64>,
    pub values: Vec<f64>,
    pub ambient_light: f64,
    pub illumination: f64,
    pub polynomial_order: i64,
}
const MAX_LINE: usize = 64 * 1024;
impl CharacteristicCurve {
    pub fn parse(text: &str, device_type: DeviceType) -> Result<Self, CalibrationError> {
        let mut curve = CharacteristicCurve {
            device_type,
            max_ddl: -1,
            ..CharacteristicCurve::default()
        };
        let mut lines: Vec<&str> = text.split('\n').collect();
        if lines.last() == Some(&"") {
            lines.pop();
        }
        for (index, raw) in lines.into_iter().enumerate() {
            if raw.len() >= MAX_LINE {
                return Err(CalibrationError::LineTooLong);
            }
            let line = index + 1;
            let mut text = raw.strip_suffix('\r').unwrap_or(raw);
            if let Some(i) = text.find('#') {
                text = &text[..i];
            }
            let fields: Vec<&str> = text.split_whitespace().collect();
            if fields.is_empty() {
                continue;
            }
            if fields.len() != 2 {
                return Err(invalid(format!("line {line}: want two fields")));
            }
            let value = match parse_go_float(fields[1]) {
                Some(v) if v.is_finite() => v,
                _ => {
                    return Err(invalid(format!(
                        "line {line}: bad value {}",
                        textfmt::quote(fields[1])
                    )));
                }
            };
            match fields[0].to_ascii_lowercase().as_str() {
                "max" => curve.max_ddl = fmath::f64_to_i64(value),
                "amb" => curve.ambient_light = value,
                "lum" => curve.illumination = value,
                "ord" => curve.polynomial_order = fmath::f64_to_i64(value),
                _ => {
                    let ddl: i64 = fields[0].parse().map_err(|_| {
                        invalid(format!(
                            "line {line}: unknown key {}",
                            textfmt::quote(fields[0])
                        ))
                    })?;
                    curve.ddl.push(ddl);
                    curve.values.push(value);
                }
            }
        }
        curve.validate()?;
        Ok(curve)
    }
    pub fn validate(&self) -> Result<(), CalibrationError> {
        if self.max_ddl < 1 || self.max_ddl > 65535 {
            return Err(invalid(format!(
                "maximum DDL {} outside 1..65535",
                self.max_ddl
            )));
        }
        if self.ddl.len() != self.values.len() || self.ddl.len() < 2 {
            return Err(invalid("need at least two DDL/value pairs".into()));
        }
        for (i, &ddl) in self.ddl.iter().enumerate() {
            if ddl < 0 || ddl > self.max_ddl || (i > 0 && ddl <= self.ddl[i - 1]) {
                return Err(invalid(format!("DDL {ddl} out of order or range")));
            }
            if self.values[i] < 0.0 {
                return Err(invalid(format!("negative value at DDL {ddl}")));
            }
        }
        if self.polynomial_order < 0
            || (self.polynomial_order > 0 && self.polynomial_order >= self.ddl.len() as i64)
        {
            return Err(invalid(format!(
                "polynomial order {} needs more points",
                self.polynomial_order
            )));
        }
        Ok(())
    }
    pub fn interpolated(&self) -> Vec<f64> {
        let count = (self.max_ddl + 1) as usize;
        let xs: Vec<f64> = self.ddl.iter().map(|&d| d as f64).collect();
        let ys = &self.values;
        if self.polynomial_order > 0 {
            let max_ddl = self.max_ddl as f64;
            let coefficients = polynomial_fit(&xs, ys, self.polynomial_order as usize, max_ddl);
            return (0..count)
                .map(|ddl| {
                    let x = ddl as f64 / max_ddl;
                    let (mut value, mut power) = (0.0, 1.0);
                    for &c in &coefficients {
                        value += c * power;
                        power *= x;
                    }
                    fmath::max(0.0, value)
                })
                .collect();
        }
        let second = natural_spline(&xs, ys);
        let last = xs.len() - 1;
        (0..count)
            .map(|ddl| {
                let x = ddl as f64;
                if x <= xs[0] {
                    return ys[0];
                }
                if x >= xs[last] {
                    return ys[last];
                }
                let i = xs.partition_point(|&v| v < x);
                if xs[i] == x {
                    return ys[i];
                }
                let (lo, hi) = (i - 1, i);
                let span = xs[hi] - xs[lo];
                let a = (xs[hi] - x) / span;
                let b = (x - xs[lo]) / span;
                fmath::max(
                    0.0,
                    a * ys[lo]
                        + b * ys[hi]
                        + ((a * a * a - a) * second[lo] + (b * b * b - b) * second[hi])
                            * span
                            * span
                            / 6.0,
                )
            })
            .collect()
    }
}
fn natural_spline(xs: &[f64], ys: &[f64]) -> Vec<f64> {
    let n = xs.len();
    let mut second = vec![0.0; n];
    let mut u = vec![0.0; n];
    for i in 1..n - 1 {
        let sig = (xs[i] - xs[i - 1]) / (xs[i + 1] - xs[i - 1]);
        let p = sig * second[i - 1] + 2.0;
        second[i] = (sig - 1.0) / p;
        let slope =
            (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]) - (ys[i] - ys[i - 1]) / (xs[i] - xs[i - 1]);
        u[i] = (6.0 * slope / (xs[i + 1] - xs[i - 1]) - sig * u[i - 1]) / p;
    }
    second[n - 1] = 0.0;
    for i in (0..n - 1).rev() {
        second[i] = second[i] * second[i + 1] + u[i];
    }
    second
}
fn polynomial_fit(xs: &[f64], ys: &[f64], order: usize, scale: f64) -> Vec<f64> {
    let n = order + 1;
    let mut matrix = vec![vec![0.0; n + 1]; n];
    for (k, &x) in xs.iter().enumerate() {
        let x = x / scale;
        let mut powers = vec![0.0; 2 * n];
        powers[0] = 1.0;
        for p in 1..powers.len() {
            powers[p] = powers[p - 1] * x;
        }
        for i in 0..n {
            for j in 0..n {
                matrix[i][j] += powers[i + j];
            }
            matrix[i][n] += powers[i] * ys[k];
        }
    }
    for col in 0..n {
        let mut pivot = col;
        for row in col + 1..n {
            if matrix[row][col].abs() > matrix[pivot][col].abs() {
                pivot = row;
            }
        }
        matrix.swap(col, pivot);
        if matrix[col][col] == 0.0 {
            continue;
        }
        for row in 0..n {
            if row == col {
                continue;
            }
            let factor = matrix[row][col] / matrix[col][col];
            for j in col..=n {
                matrix[row][j] -= factor * matrix[col][j];
            }
        }
    }
    (0..n)
        .map(|i| {
            if matrix[i][i] != 0.0 {
                matrix[i][n] / matrix[i][i]
            } else {
                0.0
            }
        })
        .collect()
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayCalibration {
    pub curve: Option<CharacteristicCurve>,
    pub ambient_light: f64,
    pub illumination: f64,
    pub min_density: f64,
    pub max_density: f64,
}
impl DisplayCalibration {
    fn ambient(&self) -> f64 {
        if self.ambient_light > 0.0 {
            return self.ambient_light;
        }
        if self.ambient_light < 0.0 {
            return 0.0;
        }
        match &self.curve {
            Some(c) if c.ambient_light > 0.0 => c.ambient_light,
            Some(c) if c.device_type == DeviceType::Printer => DEFAULT_REFLECTED_AMBIENT_LIGHT,
            _ => 0.0,
        }
    }
    fn illumination(&self) -> f64 {
        if self.illumination > 0.0 {
            return self.illumination;
        }
        match &self.curve {
            Some(c) if c.illumination > 0.0 => c.illumination,
            _ => DEFAULT_ILLUMINATION,
        }
    }
    fn valid_curve(&self) -> Result<&CharacteristicCurve, CalibrationError> {
        let curve = self
            .curve
            .as_ref()
            .ok_or_else(|| invalid("nil curve".into()))?;
        curve.validate()?;
        Ok(curve)
    }
    fn printer(&self) -> bool {
        self.curve
            .as_ref()
            .is_some_and(|c| c.device_type == DeviceType::Printer)
    }
    pub fn luminances(&self) -> Result<Vec<f64>, CalibrationError> {
        let curve = self.valid_curve()?;
        Ok(curve
            .interpolated()
            .into_iter()
            .map(|v| self.luminance(v))
            .collect())
    }
    fn luminance(&self, value: f64) -> f64 {
        if self.printer() {
            return self.ambient() + self.illumination() * fmath::pow(10.0, -value);
        }
        value + self.ambient()
    }
    pub(super) fn luminance_range(&self) -> Result<(f64, f64), CalibrationError> {
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        for &value in &self.valid_curve()?.values {
            let l = self.luminance(value);
            low = fmath::min(low, l);
            high = fmath::max(high, l);
        }
        if self.printer() {
            if self.max_density > 0.0 {
                low = fmath::max(
                    low,
                    self.ambient() + self.illumination() * fmath::pow(10.0, -self.max_density),
                );
            }
            if self.min_density > 0.0 {
                high = fmath::min(
                    high,
                    self.ambient() + self.illumination() * fmath::pow(10.0, -self.min_density),
                );
            }
        }
        if high.partial_cmp(&low) != Some(Ordering::Greater) || low <= 0.0 {
            return Err(invalid(format!(
                "luminance range {}..{} cd/m²",
                format_g4(low),
                format_g4(high)
            )));
        }
        Ok((low, high))
    }
    pub fn lut(
        &self,
        function: DisplayFunction,
        input_bits: i64,
    ) -> Result<DisplayLut, CalibrationError> {
        match function {
            DisplayFunction::Gsdf => self.gsdf_lut(input_bits),
            DisplayFunction::Cielab => self.cielab_lut(input_bits),
        }
    }
    pub fn gsdf_lut(&self, input_bits: i64) -> Result<DisplayLut, CalibrationError> {
        if !(1..=16).contains(&input_bits) {
            return Err(invalid(format!("input bits {input_bits} outside 1..16")));
        }
        let luminances = self.luminances()?;
        let (low, high) = self.luminance_range()?;
        let j_min = clamp(gsdf_jnd_index(low), GSDF_MIN_JND, GSDF_MAX_JND);
        let j_max = clamp(gsdf_jnd_index(high), GSDF_MIN_JND, GSDF_MAX_JND);
        let count = 1usize << input_bits;
        let mut order: Vec<usize> = (0..luminances.len()).collect();
        order.sort_by(|&a, &b| {
            luminances[a]
                .partial_cmp(&luminances[b])
                .unwrap_or(Ordering::Equal)
        });
        let entries = (0..count)
            .map(|p| {
                let target =
                    gsdf_luminance(j_min + p as f64 * (j_max - j_min) / (count - 1) as f64);
                let i = order.partition_point(|&d| {
                    matches!(
                        luminances[d].partial_cmp(&target),
                        None | Some(Ordering::Less)
                    )
                });
                let mut best = order[i.min(order.len() - 1)];
                if i > 0
                    && (i == order.len()
                        || target - luminances[order[i - 1]] <= luminances[order[i]] - target)
                {
                    best = order[i - 1];
                }
                best as u16
            })
            .collect();
        Ok(DisplayLut {
            function: DisplayFunction::Gsdf,
            input_bits,
            max_ddl: self.curve.as_ref().map_or(0, |c| c.max_ddl),
            entries,
            min_luminance: low,
            max_luminance: high,
            min_jnd: j_min,
            max_jnd: j_max,
            min_lightness: 0.0,
            max_lightness: 0.0,
        })
    }
}
fn format_g4(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    let sign = if x.is_sign_negative() { "-" } else { "" };
    let (digits, dp) = if x == 0.0 {
        (Vec::new(), 0i64)
    } else {
        let e = format!("{:.3e}", x.abs());
        let (mantissa, exponent) = e.split_once('e').expect("exponent form");
        let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
        while digits.last() == Some(&b'0') {
            digits.pop();
        }
        (digits, exponent.parse::<i64>().expect("exponent") + 1)
    };
    let nd = digits.len() as i64;
    let digit = |i: i64| -> char {
        if (0..nd).contains(&i) {
            digits[i as usize] as char
        } else {
            '0'
        }
    };
    let mut eprec = 4;
    if eprec > nd && nd >= dp {
        eprec = nd;
    }
    let exp = dp - 1;
    let mut out = String::from(sign);
    if exp < -4 || exp >= eprec {
        let prec = 4.min(nd) - 1;
        out.push(digit(0));
        if prec > 0 {
            out.push('.');
            for i in 1..=prec {
                out.push(digit(i));
            }
        }
        out.push('e');
        out.push(if exp < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exp.abs()));
        return out;
    }
    let prec = if 4 > dp { nd } else { 4 };
    let decimals = (prec - dp).max(0);
    if dp > 0 {
        for i in 0..dp {
            out.push(digit(i));
        }
    } else {
        out.push('0');
    }
    if decimals > 0 {
        out.push('.');
        for i in 0..decimals {
            out.push(digit(dp + i));
        }
    }
    out
}
fn parse_go_float(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let r = read_float(b)?;
    if r.consumed != b.len() {
        return None;
    }
    if r.hex {
        return Some(atof_hex(r.mantissa, r.exp, r.neg, r.trunc));
    }
    let digits: String = s[r.digits_start..r.digits_end]
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    let text = format!(
        "{}{}e{}",
        if r.neg { "-" } else { "" },
        digits,
        r.exponent - r.fraction_digits
    );
    text.parse().ok()
}
struct ReadFloat {
    mantissa: u64,
    exp: i64,
    neg: bool,
    trunc: bool,
    hex: bool,
    consumed: usize,
    digits_start: usize,
    digits_end: usize,
    fraction_digits: i64,
    exponent: i64,
}
fn read_float(s: &[u8]) -> Option<ReadFloat> {
    let lower = |c: u8| c.to_ascii_lowercase();
    let mut i = 0;
    let mut neg = false;
    let mut underscores = false;
    if i >= s.len() {
        return None;
    }
    match s[i] {
        b'+' => i += 1,
        b'-' => {
            i += 1;
            neg = true;
        }
        _ => {}
    }
    let mut base: u64 = 10;
    let mut max_mant_digits = 19;
    let mut exp_char = b'e';
    let mut hex = false;
    if i + 2 < s.len() && s[i] == b'0' && lower(s[i + 1]) == b'x' {
        base = 16;
        max_mant_digits = 16;
        i += 2;
        exp_char = b'p';
        hex = true;
    }
    let digits_start = i;
    let (mut sawdot, mut sawdigits) = (false, false);
    let (mut nd, mut nd_mant, mut dp) = (0i64, 0i64, 0i64);
    let mut mantissa: u64 = 0;
    let mut trunc = false;
    let mut fraction_digits = 0i64;
    while i < s.len() {
        let c = s[i];
        if c == b'_' {
            underscores = true;
        } else if c == b'.' {
            if sawdot {
                break;
            }
            sawdot = true;
            dp = nd;
        } else if c.is_ascii_digit() {
            sawdigits = true;
            if sawdot {
                fraction_digits += 1;
            }
            if c == b'0' && nd == 0 {
                dp -= 1;
            } else {
                nd += 1;
                if nd_mant < max_mant_digits {
                    mantissa = mantissa * base + u64::from(c - b'0');
                    nd_mant += 1;
                } else if c != b'0' {
                    trunc = true;
                }
            }
        } else if base == 16 && (b'a'..=b'f').contains(&lower(c)) {
            sawdigits = true;
            nd += 1;
            if nd_mant < max_mant_digits {
                mantissa = mantissa * 16 + u64::from(lower(c) - b'a' + 10);
                nd_mant += 1;
            } else {
                trunc = true;
            }
        } else {
            break;
        }
        i += 1;
    }
    let digits_end = i;
    if !sawdigits {
        return None;
    }
    if !sawdot {
        dp = nd;
    }
    if base == 16 {
        dp *= 4;
        nd_mant *= 4;
    }
    let mut exponent = 0i64;
    if i < s.len() && lower(s[i]) == exp_char {
        i += 1;
        if i >= s.len() {
            return None;
        }
        let mut esign = 1;
        match s[i] {
            b'+' => i += 1,
            b'-' => {
                i += 1;
                esign = -1;
            }
            _ => {}
        }
        if i >= s.len() || !s[i].is_ascii_digit() {
            return None;
        }
        let mut e = 0i64;
        while i < s.len() && (s[i].is_ascii_digit() || s[i] == b'_') {
            if s[i] == b'_' {
                underscores = true;
            } else if e < 10000 {
                e = e * 10 + i64::from(s[i] - b'0');
            }
            i += 1;
        }
        exponent = e * esign;
        dp += exponent;
    } else if base == 16 {
        return None;
    }
    let exp = if mantissa != 0 { dp - nd_mant } else { 0 };
    if underscores && !underscore_ok(&s[..i]) {
        return None;
    }
    Some(ReadFloat {
        mantissa,
        exp,
        neg,
        trunc,
        hex,
        consumed: i,
        digits_start,
        digits_end,
        fraction_digits,
        exponent,
    })
}
fn underscore_ok(s: &[u8]) -> bool {
    let lower = |c: u8| c.to_ascii_lowercase();
    let mut saw = b'^';
    let mut i = 0;
    let s = match s.first() {
        Some(b'-' | b'+') => &s[1..],
        _ => s,
    };
    let mut hex = false;
    if s.len() >= 2 && s[0] == b'0' && matches!(lower(s[1]), b'b' | b'o' | b'x') {
        i = 2;
        saw = b'0';
        hex = lower(s[1]) == b'x';
    }
    while i < s.len() {
        let c = s[i];
        if c.is_ascii_digit() || (hex && (b'a'..=b'f').contains(&lower(c))) {
            saw = b'0';
        } else if c == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
        } else {
            if saw == b'_' {
                return false;
            }
            saw = b'!';
        }
        i += 1;
    }
    saw != b'_'
}
fn atof_hex(mut mantissa: u64, mut exp: i64, neg: bool, trunc: bool) -> f64 {
    const MANTBITS: u32 = 52;
    const BIAS: i64 = -1023;
    const MAX_EXP: i64 = (1 << 11) + BIAS - 2;
    const MIN_EXP: i64 = BIAS + 1;
    exp += i64::from(MANTBITS);
    while mantissa != 0 && mantissa >> (MANTBITS + 2) == 0 {
        mantissa <<= 1;
        exp -= 1;
    }
    if trunc {
        mantissa |= 1;
    }
    while mantissa >> (1 + MANTBITS + 2) != 0 {
        mantissa = (mantissa >> 1) | (mantissa & 1);
        exp += 1;
    }
    while mantissa > 1 && exp < MIN_EXP - 2 {
        mantissa = (mantissa >> 1) | (mantissa & 1);
        exp += 1;
    }
    let mut round = mantissa & 3;
    mantissa >>= 2;
    round |= mantissa & 1;
    exp += 2;
    if round == 3 {
        mantissa += 1;
        if mantissa == 1 << (1 + MANTBITS) {
            mantissa >>= 1;
            exp += 1;
        }
    }
    if mantissa >> MANTBITS == 0 {
        exp = BIAS;
    }
    if exp > MAX_EXP {
        mantissa = 1 << MANTBITS;
        exp = MAX_EXP + 1;
    }
    let mut bits = mantissa & ((1 << MANTBITS) - 1);
    bits |= (((exp - BIAS) & ((1 << 11) - 1)) as u64) << MANTBITS;
    if neg {
        bits |= 1 << 63;
    }
    f64::from_bits(bits)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DisplayFunction {
    #[default]
    Gsdf,
    Cielab,
}
impl DisplayFunction {
    pub fn from_index(value: i64) -> Result<DisplayFunction, CalibrationError> {
        match value {
            0 => Ok(DisplayFunction::Gsdf),
            1 => Ok(DisplayFunction::Cielab),
            _ => Err(invalid(format!("unknown display function {value}"))),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            DisplayFunction::Cielab => "CIELAB",
            DisplayFunction::Gsdf => "GSDF",
        }
    }
}
impl fmt::Display for DisplayFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayLut {
    pub function: DisplayFunction,
    pub input_bits: i64,
    pub max_ddl: i64,
    pub entries: Vec<u16>,
    pub min_luminance: f64,
    pub max_luminance: f64,
    pub min_jnd: f64,
    pub max_jnd: f64,
    pub min_lightness: f64,
    pub max_lightness: f64,
}
impl DisplayLut {
    pub fn map(&self, p: i64) -> u16 {
        if self.entries.is_empty() {
            return 0;
        }
        self.entries[p.clamp(0, self.entries.len() as i64 - 1) as usize]
    }
    pub fn apply_gray(&self, pixels: &mut [u8]) -> Result<(), CalibrationError> {
        if self.input_bits != 8 {
            return Err(invalid(format!(
                "ApplyGray needs an 8-bit LUT, have {} bits",
                self.input_bits
            )));
        }
        let table: Vec<u8> = self.entries[..256]
            .iter()
            .map(|&e| scale_ddl(e, self.max_ddl, 255) as u8)
            .collect();
        for p in pixels {
            *p = table[*p as usize];
        }
        Ok(())
    }
    pub fn apply_gray16(&self, image: &Gray16Image) -> Gray16Image {
        let shift = 16 - self.input_bits;
        let pixels = image
            .pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|s| {
                let p = (u16::from_be_bytes([s[0], s[1]]) >> shift) as usize;
                (scale_ddl(self.entries[p], self.max_ddl, 65535) as u16).to_be_bytes()
            })
            .collect();
        Gray16Image {
            width: image.width,
            height: image.height,
            pixels,
        }
    }
}
fn scale_ddl(ddl: u16, max_ddl: i64, max_out: i64) -> i64 {
    if max_ddl == max_out || max_ddl <= 0 {
        return i64::from(ddl);
    }
    fmath::f64_to_i64((f64::from(ddl) * max_out as f64 / max_ddl as f64).round())
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PrintDensity {
    pub min_density: f64,
    pub max_density: f64,
    pub illumination: f64,
    pub reflected_ambient_light: f64,
}
pub const DEFAULT_PRINT_DENSITY: PrintDensity = PrintDensity {
    min_density: 0.20,
    max_density: 3.00,
    illumination: DEFAULT_ILLUMINATION,
    reflected_ambient_light: DEFAULT_REFLECTED_AMBIENT_LIGHT,
};
impl PrintDensity {
    pub(super) fn normalized(mut self) -> PrintDensity {
        if self.illumination <= 0.0 {
            self.illumination = DEFAULT_ILLUMINATION;
        }
        if self.reflected_ambient_light < 0.0 {
            self.reflected_ambient_light = DEFAULT_REFLECTED_AMBIENT_LIGHT;
        }
        if self.max_density <= 0.0 {
            self.max_density = DEFAULT_PRINT_DENSITY.max_density;
        }
        if self.min_density < 0.0 || self.min_density >= self.max_density {
            self.min_density = DEFAULT_PRINT_DENSITY.min_density;
        }
        self
    }
    pub fn luminance(&self, od: f64) -> f64 {
        let d = self.normalized();
        d.reflected_ambient_light + d.illumination * fmath::pow(10.0, -od)
    }
    pub fn lin_od_presentation_lut(&self, input_bits: i64) -> Result<Lut, CalibrationError> {
        let d = self.normalized();
        d.lin_od_lut(
            input_bits,
            gsdf_jnd_index(d.luminance(d.max_density)),
            gsdf_jnd_index(d.luminance(d.min_density)),
        )
    }
    pub fn lin_od_presentation_lut_for(
        &self,
        display: Option<&DisplayLut>,
        input_bits: i64,
    ) -> Result<Lut, CalibrationError> {
        let Some(display) = display.filter(|d| d.max_jnd > d.min_jnd) else {
            return Err(CalibrationError::UnsupportedPresentationLut(
                "display LUT without a JND range".into(),
            ));
        };
        self.normalized()
            .lin_od_lut(input_bits, display.min_jnd, display.max_jnd)
    }
    fn lin_od_lut(&self, input_bits: i64, j_min: f64, j_max: f64) -> Result<Lut, CalibrationError> {
        if !(1..=16).contains(&input_bits) {
            return Err(CalibrationError::UnsupportedPresentationLut(format!(
                "input bits {input_bits} outside 1..16"
            )));
        }
        let count = 1i64 << input_bits;
        let entries = (0..count)
            .map(|i| {
                let od = self.min_density
                    + i as f64 * (self.max_density - self.min_density) / (count - 1) as f64;
                let unit = (gsdf_jnd_index(self.luminance(od)) - j_min) / (j_max - j_min);
                (clamp(unit, 0.0, 1.0) * 65535.0).round() as u16
            })
            .collect();
        Lut::new(&[count & 0xFFFF, 0, 16], entries).map_err(CalibrationError::Lut)
    }
    pub fn optical_density(&self, p: i64, max_p: i64) -> f64 {
        let d = self.normalized();
        if max_p <= 0 {
            return d.max_density;
        }
        let j_min = gsdf_jnd_index(d.luminance(d.max_density));
        let j_max = gsdf_jnd_index(d.luminance(d.min_density));
        let l = gsdf_luminance(j_min + p as f64 * (j_max - j_min) / max_p as f64);
        -fmath::log10((l - d.reflected_ambient_light) / d.illumination)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn number_text() {
        assert_eq!(format_g4(0.5), "0.5");
        assert_eq!(format_g4(250.0), "250");
        assert_eq!(format_g4(1234.5), "1234");
        assert_eq!(format_g4(123456.0), "1.235e+05");
        assert_eq!(format_g4(0.000_123_4), "0.0001234");
        assert_eq!(format_g4(f64::INFINITY), "+Inf");
        assert_eq!(parse_go_float("0x1.8p3"), Some(12.0));
        assert_eq!(parse_go_float(".5e2"), Some(50.0));
        assert_eq!(parse_go_float("1_0"), Some(10.0));
        assert_eq!(parse_go_float("1_"), None);
        assert_eq!(parse_go_float("0x_1p-2"), Some(0.25));
        assert_eq!(parse_go_float("inf"), None);
    }
    #[test]
    fn linear_monitor_gsdf() {
        let curve = CharacteristicCurve::parse("max 255\n0 0.5\n255 250\n", DeviceType::Monitor);
        let lut = DisplayCalibration {
            curve: Some(curve.unwrap()),
            ..DisplayCalibration::default()
        }
        .gsdf_lut(8)
        .unwrap();
        assert_eq!((lut.map(0), lut.map(255)), (0, 255));
    }
}
