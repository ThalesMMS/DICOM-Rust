#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use crate::numtext::{self, NumError, NumErrorKind};
fn num_error(func: &'static str, s: &str, kind: NumErrorKind) -> String {
    NumError {
        func,
        num: s.to_string(),
        kind,
    }
    .to_string()
}
pub fn parse_uint_base(s: &str, base: u32, bits: u32) -> Result<u64, String> {
    parse_uint_radix(s, base, bits).map_err(|k| num_error("parse_uint", s, k))
}
fn parse_uint_inner(s: &str, bits: u32) -> Result<u64, NumErrorKind> {
    parse_uint_radix(s, 10, bits)
}
fn parse_uint_radix(s: &str, base: u32, bits: u32) -> Result<u64, NumErrorKind> {
    if s.is_empty() {
        return Err(NumErrorKind::Syntax);
    }
    let max: u64 = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits).wrapping_sub(1)
    };
    let mut n: u64 = 0;
    for c in s.bytes() {
        let Some(d) = char::from(c).to_digit(base) else {
            return Err(NumErrorKind::Syntax);
        };
        let Some(m) = n.checked_mul(u64::from(base)) else {
            return Err(NumErrorKind::Range);
        };
        let Some(n1) = m.checked_add(u64::from(d)) else {
            return Err(NumErrorKind::Range);
        };
        if n1 > max {
            return Err(NumErrorKind::Range);
        }
        n = n1;
    }
    Ok(n)
}
pub fn parse_uint(s: &str, bits: u32) -> Result<u64, String> {
    parse_uint_inner(s, bits).map_err(|k| num_error("parse_uint", s, k))
}
pub fn parse_int(s: &str, bits: u32) -> Result<i64, String> {
    if s.is_empty() {
        return Err(num_error("parse_int", s, NumErrorKind::Syntax));
    }
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'+') => (false, &s[1..]),
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };
    let un = match parse_uint_inner(digits, bits) {
        Ok(v) => v,
        Err(NumErrorKind::Range) => u64::MAX,
        Err(k) => return Err(num_error("parse_int", s, k)),
    };
    let cutoff = 1u64 << bits.saturating_sub(1).min(63);
    if !neg && un >= cutoff || neg && un > cutoff {
        return Err(num_error("parse_int", s, NumErrorKind::Range));
    }
    Ok(if neg {
        (un as i64).wrapping_neg()
    } else {
        un as i64
    })
}
pub fn parse_float32(s: &str) -> Result<f64, String> {
    let (f64v, err) = numtext::parse_float_value(s);
    if let Some(e) = err
        && e.kind == NumErrorKind::Syntax
    {
        return Err(e.to_string());
    }
    let body = s.trim_start_matches(['+', '-']);
    let lower = body.to_ascii_lowercase();
    let v: f32 = if lower.starts_with("0x") || lower.starts_with("inf") || lower.starts_with("nan")
    {
        f64v as f32
    } else {
        match s.parse::<f32>() {
            Ok(v) => v,
            Err(_) => f64v as f32,
        }
    };
    if v.is_infinite() && !lower.starts_with("inf") {
        return Err(num_error("parse_float", s, NumErrorKind::Range));
    }
    Ok(f64::from(v))
}
fn decode_map(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c.wrapping_sub(b'A')),
        b'a'..=b'z' => Some(c.wrapping_sub(b'a').wrapping_add(26)),
        b'0'..=b'9' => Some(c.wrapping_sub(b'0').wrapping_add(52)),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub fn base64_decode(src: &[u8], strict: bool) -> Result<Vec<u8>, (usize, Vec<u8>)> {
    let mut out = Vec::with_capacity(src.len() / 4 * 3);
    let mut si = 0;
    while si < src.len() {
        let mut dbuf = [0u8; 4];
        let mut dlen = 4;
        let mut j = 0;
        let mut err: Option<usize> = None;
        while j < 4 {
            if si == src.len() {
                if j == 0 {
                    return Ok(out);
                }
                return Err((si - j, out));
            }
            let c = src[si];
            si += 1;
            if let Some(v) = decode_map(c) {
                dbuf[j] = v;
                j += 1;
                continue;
            }
            if c == b'\n' || c == b'\r' {
                continue;
            }
            if c != b'=' {
                return Err((si - 1, out));
            }
            match j {
                0 | 1 => return Err((si - 1, out)),
                2 => {
                    while si < src.len() && (src[si] == b'\n' || src[si] == b'\r') {
                        si += 1;
                    }
                    if si == src.len() {
                        return Err((src.len(), out));
                    }
                    if src[si] != b'=' {
                        return Err((si - 1, out));
                    }
                    si += 1;
                }
                _ => {}
            }
            while si < src.len() && (src[si] == b'\n' || src[si] == b'\r') {
                si += 1;
            }
            if si < src.len() {
                err = Some(si);
            }
            dlen = j;
            break;
        }
        let val = u32::from(dbuf[0]) << 18
            | u32::from(dbuf[1]) << 12
            | u32::from(dbuf[2]) << 6
            | u32::from(dbuf[3]);
        let b = [(val >> 16) as u8, (val >> 8) as u8, val as u8];
        match dlen {
            4 => out.extend_from_slice(&b),
            3 => {
                if strict && b[2] != 0 {
                    return Err((si - 1, out));
                }
                out.extend_from_slice(&b[..2]);
            }
            2 => {
                if strict && (b[1] != 0 || b[2] != 0) {
                    return Err((si - 2, out));
                }
                out.push(b[0]);
            }
            _ => {}
        }
        if let Some(e) = err {
            return Err((e, out));
        }
    }
    Ok(out)
}
const ENCODE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub fn base64_encode(src: &[u8]) -> String {
    let mut out = String::with_capacity(src.len().div_ceil(3) * 4);
    for chunk in src.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let v = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ENCODE[(v >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
