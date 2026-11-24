#![forbid(unsafe_code)]
use std::fmt;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumErrorKind {
    Syntax,
    Range,
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NumError {
    pub func: &'static str,
    pub num: String,
    pub kind: NumErrorKind,
}
impl fmt::Display for NumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cause = match self.kind {
            NumErrorKind::Syntax => "invalid syntax",
            NumErrorKind::Range => "value out of range",
        };
        write!(
            f,
            "{}: parsing {}: {}",
            self.func,
            crate::textfmt::quote(&self.num),
            cause
        )
    }
}
impl std::error::Error for NumError {}
fn syntax(func: &'static str, s: &str) -> NumError {
    NumError {
        func,
        num: s.to_string(),
        kind: NumErrorKind::Syntax,
    }
}
fn range(func: &'static str, s: &str) -> NumError {
    NumError {
        func,
        num: s.to_string(),
        kind: NumErrorKind::Range,
    }
}
pub fn parse_float(s: &str) -> Result<f64, NumError> {
    match parse_float_value(s) {
        (f, None) => Ok(f),
        (_, Some(e)) => Err(e),
    }
}
pub fn parse_float_value(s: &str) -> (f64, Option<NumError>) {
    const FN: &str = "parse_float";
    let (f, n, err) = atof64(s);
    if n != s.len() && err.as_ref().is_none_or(|e| e.kind != NumErrorKind::Syntax) {
        return (0.0, Some(syntax(FN, s)));
    }
    (f, err)
}
fn lower(c: u8) -> u8 {
    c | (b'x' - b'X')
}
fn common_prefix_len_ignore_case(s: &[u8], prefix: &[u8]) -> usize {
    let mut n = prefix.len().min(s.len());
    for i in 0..n {
        let mut c = s[i];
        if c.is_ascii_uppercase() {
            c += b'a' - b'A';
        }
        if c != prefix[i] {
            n = i;
            break;
        }
    }
    n
}
fn special(s: &[u8]) -> Option<(f64, usize)> {
    if s.is_empty() {
        return None;
    }
    let mut sign = 1.0;
    let mut nsign = 0;
    let mut rest = s;
    match s[0] {
        b'+' | b'-' | b'i' | b'I' => {
            if s[0] == b'+' || s[0] == b'-' {
                if s[0] == b'-' {
                    sign = -1.0;
                }
                nsign = 1;
                rest = &s[1..];
            }
            let mut n = common_prefix_len_ignore_case(rest, b"infinity");
            if 3 < n && n < 8 {
                n = 3;
            }
            if n == 3 || n == 8 {
                return Some((sign * f64::INFINITY, nsign + n));
            }
        }
        b'n' | b'N' if common_prefix_len_ignore_case(s, b"nan") == 3 => {
            return Some((f64::from_bits(0x7FF8_0000_0000_0001), 3));
        }
        _ => {}
    }
    None
}
struct ReadFloat {
    mantissa: u64,
    exp: i64,
    neg: bool,
    trunc: bool,
    hex: bool,
}
fn read_float(s: &[u8]) -> (usize, Option<ReadFloat>) {
    let mut i = 0;
    let mut underscores = false;
    let mut neg = false;
    if i >= s.len() {
        return (i, None);
    }
    if s[i] == b'+' {
        i += 1;
    } else if s[i] == b'-' {
        neg = true;
        i += 1;
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
    let (mut sawdot, mut sawdigits) = (false, false);
    let (mut nd, mut nd_mant, mut dp) = (0i64, 0i64, 0i64);
    let mut mantissa: u64 = 0;
    let mut trunc = false;
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
    if !sawdigits {
        return (i, None);
    }
    if !sawdot {
        dp = nd;
    }
    if base == 16 {
        dp *= 4;
        nd_mant *= 4;
    }
    if i < s.len() && lower(s[i]) == exp_char {
        i += 1;
        if i >= s.len() {
            return (i, None);
        }
        let mut esign = 1;
        if s[i] == b'+' {
            i += 1;
        } else if s[i] == b'-' {
            i += 1;
            esign = -1;
        }
        if i >= s.len() || !s[i].is_ascii_digit() {
            return (i, None);
        }
        let mut e: i64 = 0;
        while i < s.len() && (s[i].is_ascii_digit() || s[i] == b'_') {
            if s[i] == b'_' {
                underscores = true;
            } else if e < 10000 {
                e = e * 10 + i64::from(s[i] - b'0');
            }
            i += 1;
        }
        dp += e * esign;
    } else if base == 16 {
        return (i, None);
    }
    let exp = if mantissa != 0 { dp - nd_mant } else { 0 };
    if underscores && !underscore_ok(&s[..i]) {
        return (i, None);
    }
    (
        i,
        Some(ReadFloat {
            mantissa,
            exp,
            neg,
            trunc,
            hex,
        }),
    )
}
fn underscore_ok(mut s: &[u8]) -> bool {
    let mut saw = b'^';
    let mut i = 0;
    if !s.is_empty() && (s[0] == b'-' || s[0] == b'+') {
        s = &s[1..];
    }
    let mut hex = false;
    if s.len() >= 2 && s[0] == b'0' && matches!(lower(s[1]), b'b' | b'o' | b'x') {
        i = 2;
        saw = b'0';
        hex = lower(s[1]) == b'x';
    }
    while i < s.len() {
        let c = s[i];
        if c.is_ascii_digit() || hex && (b'a'..=b'f').contains(&lower(c)) {
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
fn atof_hex(
    s: &str,
    mut mantissa: u64,
    mut exp: i64,
    neg: bool,
    trunc: bool,
) -> (f64, Option<NumError>) {
    const MANTBITS: u32 = 52;
    const EXPBITS: u32 = 11;
    const BIAS: i64 = -1023;
    let max_exp = (1i64 << EXPBITS) + BIAS - 2;
    let min_exp = BIAS + 1;
    exp += i64::from(MANTBITS);
    while mantissa != 0 && mantissa >> (MANTBITS + 2) == 0 {
        mantissa <<= 1;
        exp -= 1;
    }
    if trunc {
        mantissa |= 1;
    }
    while mantissa >> (1 + MANTBITS + 2) != 0 {
        mantissa = mantissa >> 1 | mantissa & 1;
        exp += 1;
    }
    while mantissa > 1 && exp < min_exp - 2 {
        mantissa = mantissa >> 1 | mantissa & 1;
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
    let mut err = None;
    if exp > max_exp {
        mantissa = 1 << MANTBITS;
        exp = max_exp + 1;
        err = Some(range("parse_float", s));
    }
    let mut bits = mantissa & ((1 << MANTBITS) - 1);
    bits |= (((exp - BIAS) & ((1 << EXPBITS) - 1)) as u64) << MANTBITS;
    if neg {
        bits |= 1 << MANTBITS << EXPBITS;
    }
    (f64::from_bits(bits), err)
}
fn atof64(s: &str) -> (f64, usize, Option<NumError>) {
    const FN: &str = "parse_float";
    let b = s.as_bytes();
    if let Some((v, n)) = special(b) {
        return (v, n, None);
    }
    let (n, parsed) = read_float(b);
    let Some(r) = parsed else {
        return (0.0, n, Some(syntax(FN, s)));
    };
    if r.hex {
        let (f, err) = atof_hex(&s[..n], r.mantissa, r.exp, r.neg, r.trunc);
        return (f, n, err);
    }
    let text: String = s[..n].chars().filter(|&c| c != '_').collect();
    let f: f64 = match text.parse() {
        Ok(f) => f,
        Err(_) => return (0.0, n, Some(syntax(FN, s))),
    };
    if f.is_infinite() {
        return (f, n, Some(range(FN, &s[..n])));
    }
    (f, n, None)
}
pub fn parse_int(s: &str) -> Result<i64, NumError> {
    match parse_int_value(s) {
        (n, None) => Ok(n),
        (_, Some(e)) => Err(e),
    }
}
pub fn parse_int_value(s: &str) -> (i64, Option<NumError>) {
    const FN: &str = "parse_int";
    if s.is_empty() {
        return (0, Some(syntax(FN, s)));
    }
    let s0 = s;
    let mut neg = false;
    let mut rest = s;
    if let Some(r) = s.strip_prefix('+') {
        rest = r;
    } else if let Some(r) = s.strip_prefix('-') {
        neg = true;
        rest = r;
    }
    let (un, err) = parse_uint10(rest);
    if let Some(e) = err
        && e.kind != NumErrorKind::Range
    {
        return (0, Some(syntax(FN, s0)));
    }
    let cutoff: u64 = 1 << 63;
    if !neg && un >= cutoff {
        return ((cutoff - 1) as i64, Some(range(FN, s0)));
    }
    if neg && un > cutoff {
        return (i64::MIN, Some(range(FN, s0)));
    }
    let n = un as i64;
    (if neg { n.wrapping_neg() } else { n }, None)
}
fn parse_uint10(s: &str) -> (u64, Option<NumError>) {
    const FN: &str = "parse_uint";
    if s.is_empty() {
        return (0, Some(syntax(FN, s)));
    }
    let cutoff = u64::MAX / 10 + 1;
    let mut n: u64 = 0;
    for &c in s.as_bytes() {
        let d = if c.is_ascii_digit() {
            c - b'0'
        } else if lower(c).is_ascii_lowercase() {
            lower(c) - b'a' + 10
        } else {
            return (0, Some(syntax(FN, s)));
        };
        if d >= 10 {
            return (0, Some(syntax(FN, s)));
        }
        if n >= cutoff {
            return (u64::MAX, Some(range(FN, s)));
        }
        n *= 10;
        let n1 = n.wrapping_add(u64::from(d));
        if n1 < n {
            return (u64::MAX, Some(range(FN, s)));
        }
        n = n1;
    }
    (n, None)
}
fn decimal_digits(x: f64, significant: usize) -> (Vec<u8>, i32) {
    let x = x.abs();
    if x == 0.0 {
        return (Vec::new(), 0);
    }
    let s = format!("{x:.*e}", significant.saturating_sub(1));
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    while digits.last() == Some(&b'0') {
        digits.pop();
    }
    if digits.is_empty() {
        return (digits, 0);
    }
    (digits, exp + 1)
}
#[path = "numtext_pow10.rs"]
mod pow10tab;
type U128 = (u64, u64);
fn shr(x: u64, n: i32) -> u64 {
    if (0..64).contains(&n) { x >> n } else { 0 }
}
fn shl(x: u64, n: i32) -> u64 {
    if (0..64).contains(&n) { x << n } else { 0 }
}
fn mul_log10_2(x: i64) -> i64 {
    (x * 78913) >> 18
}
fn mul_log2_10(x: i64) -> i64 {
    (x * 108853) >> 15
}
fn mul_log10_2_minus_log10_4_over3(e: i64) -> i64 {
    (e * 631305 - 261663) >> 21
}
fn pow10(e: i64) -> (U128, i64) {
    let i = (e - i64::from(pow10tab::POW10_MIN)) as usize;
    debug_assert!(e <= i64::from(pow10tab::POW10_MAX));
    (pow10tab::POW10_TAB[i], 1 + mul_log2_10(e))
}
fn umul192_upper128(x: u64, y: U128) -> U128 {
    let r = u128::from(x) * u128::from(y.0);
    let t = ((u128::from(x) * u128::from(y.1)) >> 64) as u64;
    let s = r.wrapping_add(u128::from(t));
    ((s >> 64) as u64, s as u64)
}
fn umul192_lower128(x: u64, y: U128) -> U128 {
    let high = x.wrapping_mul(y.0);
    let hl = u128::from(x) * u128::from(y.1);
    (high.wrapping_add((hl >> 64) as u64), hl as u64)
}
fn dbox_mul_pow64(u: u64, phi: U128) -> (u64, bool) {
    let r = umul192_upper128(u, phi);
    (r.0, r.1 == 0)
}
fn dbox_parity64(mant2: u64, phi: U128, beta: i32) -> (bool, bool) {
    let r = umul192_lower128(mant2, phi);
    let parity = (shr(r.0, 64 - beta) & 1) != 0;
    let is_int = (shl(r.0, beta) | shr(r.1, 64 - beta)) == 0;
    (parity, is_int)
}
fn dbox_delta64(phi: U128, beta: i32) -> u32 {
    shr(phi.0, 64 - 1 - beta) as u32
}
const MANT_BITS: i32 = 52;
fn dbox_range64(phi: U128, beta: i32) -> (u64, u64) {
    let h = phi.0;
    let left = shr(
        h.wrapping_sub(h >> (MANT_BITS + 2)),
        64 - MANT_BITS - 1 - beta,
    );
    let right = shr(
        h.wrapping_add(h >> (MANT_BITS + 1)),
        64 - MANT_BITS - 1 - beta,
    );
    (left, right)
}
fn dbox_round_up64(phi: U128, beta: i32) -> u64 {
    shr(phi.0, 128 / 2 - MANT_BITS - 2 - beta).wrapping_add(1) / 2
}
fn dbox_pow64(k: i64, e: i64) -> (U128, i32) {
    let (mut phi, e1) = pow10(k);
    if !(0..=55).contains(&k) {
        phi.1 = phi.1.wrapping_add(1);
    }
    (phi, (e + e1 - 1) as i32)
}
fn trim_zeros(mut x: u64) -> (u64, i64) {
    let mut p = 0;
    while x != 0 && x.is_multiple_of(10) {
        x /= 10;
        p += 1;
    }
    (x, p)
}
fn dbox_digits(mant: u64, exp: i64) -> (Vec<u8>, i32) {
    let d = mant.to_string().into_bytes();
    let dp = d.len() as i64 + exp;
    (d, dp as i32)
}
fn dbox_ftoa64(mant: u64, exp: i64, denorm: bool) -> (Vec<u8>, i32) {
    if mant == 1 << MANT_BITS && !denorm {
        let k0 = -mul_log10_2_minus_log10_4_over3(exp);
        let (phi, beta) = dbox_pow64(k0, exp);
        let (mut xi, zi) = dbox_range64(phi, beta);
        if exp != 2 && exp != 3 {
            xi = xi.wrapping_add(1);
        }
        let q = zi / 10;
        if xi <= q.wrapping_mul(10) {
            let (q, zeros) = trim_zeros(q);
            return dbox_digits(q, -k0 + 1 + zeros);
        }
        let mut yru = dbox_round_up64(phi, beta);
        if exp == -77 && !yru.is_multiple_of(2) {
            yru -= 1;
        } else if yru < xi {
            yru += 1;
        }
        return dbox_digits(yru, -k0);
    }
    const P10K: u32 = 100;
    const P10K1: u64 = 1000;
    let k0 = -mul_log10_2(exp);
    let (phi, beta) = dbox_pow64(2 + k0, exp);
    let (zi, exact) = dbox_mul_pow64(shl(mant.wrapping_mul(2).wrapping_add(1), beta), phi);
    let (mut s, mut r) = (zi / P10K1, (zi % P10K1) as u32);
    let delta = dbox_delta64(phi, beta);
    if r < delta {
        if r != 0 || !exact || mant.is_multiple_of(2) {
            let (s, zeros) = trim_zeros(s);
            return dbox_digits(s, -k0 + 1 + zeros);
        }
        s = s.wrapping_sub(1);
        r = P10K * 10;
    } else if r == delta {
        let (parity, exact) = dbox_parity64(mant.wrapping_mul(2).wrapping_sub(1), phi, beta);
        if parity || (exact && mant.is_multiple_of(2)) {
            let (s, zeros) = trim_zeros(s);
            return dbox_digits(s, -k0 + 1 + zeros);
        }
    }
    let d = r.wrapping_add(P10K / 2).wrapping_sub(delta / 2);
    let (t, rho) = (d / P10K, d % P10K);
    let mut yru = s.wrapping_mul(10).wrapping_add(u64::from(t));
    if rho == 0 {
        let (parity, exact) = dbox_parity64(mant.wrapping_mul(2), phi, beta);
        if parity != (d.wrapping_sub(P10K / 2) % 2 != 0) || (exact && yru % 2 != 0) {
            yru = yru.wrapping_sub(1);
        }
    }
    dbox_digits(yru, -k0)
}
fn shortest_digits(x: f64) -> (Vec<u8>, i32) {
    let bits = x.to_bits();
    let mut exp = ((bits >> MANT_BITS) & 0x7ff) as i64;
    let mut mant = bits & ((1u64 << MANT_BITS) - 1);
    let mut denorm = false;
    if exp == 0 {
        exp += 1;
        denorm = true;
    } else {
        mant |= 1u64 << MANT_BITS;
    }
    exp -= 1023;
    dbox_ftoa64(mant, exp - i64::from(MANT_BITS), denorm)
}
fn fmt_e(out: &mut String, digits: &[u8], dp: i32, prec: i32, verb: char) {
    out.push(digits.first().map_or('0', |&d| d as char));
    if prec > 0 {
        out.push('.');
        let m = digits.len().min(prec as usize + 1);
        let mut i = 1;
        if i < m {
            out.extend(digits[i..m].iter().map(|&d| d as char));
            i = m;
        }
        while i <= prec as usize {
            out.push('0');
            i += 1;
        }
    }
    out.push(verb);
    let mut exp = if digits.is_empty() { 0 } else { dp - 1 };
    if exp < 0 {
        out.push('-');
        exp = -exp;
    } else {
        out.push('+');
    }
    if exp < 10 {
        out.push('0');
    }
    out.push_str(&exp.to_string());
}
fn fmt_f(out: &mut String, digits: &[u8], dp: i32, prec: i32) {
    if dp > 0 {
        let m = digits.len().min(dp as usize);
        out.extend(digits[..m].iter().map(|&d| d as char));
        for _ in m..dp as usize {
            out.push('0');
        }
    } else {
        out.push('0');
    }
    if prec > 0 {
        out.push('.');
        for i in 1..=prec {
            let j = dp + i - 1;
            let ch = if 0 <= j && (j as usize) < digits.len() {
                digits[j as usize] as char
            } else {
                '0'
            };
            out.push(ch);
        }
    }
}
pub fn format_float(x: f64, verb: char, prec: i32) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    let mut out = String::new();
    if x.is_sign_negative() {
        out.push('-');
    }
    let shortest = prec < 0;
    let lower = verb.to_ascii_lowercase();
    if lower == 'f' && !shortest {
        let s = format!("{:.*}", prec as usize, x.abs());
        out.push_str(&s);
        return out;
    }
    let mut prec = prec;
    let (digits, dp) = if shortest {
        let (digits, dp) = if x == 0.0 {
            (Vec::new(), 0)
        } else {
            shortest_digits(x)
        };
        let nd = digits.len() as i32;
        prec = match lower {
            'e' => (nd - 1).max(0),
            'f' => (nd - dp).max(0),
            _ => nd,
        };
        (digits, dp)
    } else {
        let n = match lower {
            'e' => prec + 1,
            _ => {
                if prec == 0 {
                    prec = 1;
                }
                prec
            }
        };
        decimal_digits(x, n as usize)
    };
    let nd = digits.len() as i32;
    match lower {
        'e' => fmt_e(&mut out, &digits, dp, prec, verb),
        'f' => fmt_f(&mut out, &digits, dp, prec),
        _ => {
            let mut eprec = prec;
            if eprec > nd && nd >= dp {
                eprec = nd;
            }
            if shortest {
                eprec = 6;
            }
            let exp = dp - 1;
            if exp < -4 || exp >= eprec {
                if prec > nd {
                    prec = nd;
                }
                let e = if verb == 'G' { 'E' } else { 'e' };
                fmt_e(&mut out, &digits, dp, prec - 1, e);
            } else {
                if prec > dp {
                    prec = nd;
                }
                fmt_f(&mut out, &digits, dp, (prec - dp).max(0));
            }
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_like_go() {
        assert_eq!(parse_float("1.5"), Ok(1.5));
        assert_eq!(parse_float("0x1p-2"), Ok(0.25));
        assert_eq!(parse_float("1_000"), Ok(1000.0));
        assert_eq!(parse_float("Infinity"), Ok(f64::INFINITY));
        assert!(parse_float("nan").unwrap().is_nan());
        assert_eq!(
            parse_float("1e400").unwrap_err().to_string(),
            "parse_float: parsing \"1e400\": value out of range"
        );
        assert_eq!(
            parse_float("abc").unwrap_err().to_string(),
            "parse_float: parsing \"abc\": invalid syntax"
        );
        assert_eq!(parse_int("-9223372036854775808"), Ok(i64::MIN));
        assert_eq!(
            parse_int("9223372036854775808").unwrap_err().to_string(),
            "parse_int: parsing \"9223372036854775808\": value out of range"
        );
        assert_eq!(
            parse_int("+").unwrap_err().to_string(),
            "parse_int: parsing \"+\": invalid syntax"
        );
    }
}
