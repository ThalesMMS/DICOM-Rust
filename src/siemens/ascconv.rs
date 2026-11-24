use super::{Error, Kind};
use crate::numtext::{self, NumError, NumErrorKind};
use crate::textfmt::quote;
use std::collections::HashMap;
const ASCCONV_BEGIN: &str = "### ASCCONV BEGIN";
const ASCCONV_END: &str = "### ASCCONV END";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolEntry {
    pub key: String,
    pub value: String,
}
#[derive(Clone, Debug, Default)]
pub struct Protocol {
    entries: Vec<ProtocolEntry>,
    index: HashMap<String, usize>,
}
pub fn parse_ascconv(text: &str) -> Result<Protocol, Error> {
    let Some(start) = text.find(ASCCONV_BEGIN) else {
        return Err(Error::bare(Kind::NoAscconv));
    };
    let mut body = &text[start..];
    body = match body.find('\n') {
        Some(n) => &body[n + 1..],
        None => "",
    };
    match body.find(ASCCONV_END) {
        Some(end) => body = &body[..end],
        None => {
            return Err(Error::new(
                Kind::NoAscconv,
                format!("missing {}", quote(ASCCONV_END)),
            ));
        }
    }
    let mut p = Protocol::default();
    for line in body.split('\n') {
        let line = super::trim_space(strip_comment(line.trim_end_matches('\r')));
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (super::trim_space(key), super::trim_space(value));
        if key.is_empty() {
            continue;
        }
        if let Some(&i) = p.index.get(key) {
            p.entries[i].value = value.to_string();
            continue;
        }
        p.index.insert(key.to_string(), p.entries.len());
        p.entries.push(ProtocolEntry {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(p)
}
fn strip_comment(line: &str) -> &str {
    let b = line.as_bytes();
    let mut quoted = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                while i + 1 < b.len() && b[i + 1] == b'"' {
                    i += 1;
                }
                quoted = !quoted;
            }
            b'#' if !quoted => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProtocolSlice {
    pub position: [f64; 3],
    pub normal: [f64; 3],
    pub thickness: f64,
    pub in_plane_rotation: f64,
}
pub const MAX_PROTOCOL_SLICES: i64 = 4096;
impl Protocol {
    pub fn entries(&self) -> &[ProtocolEntry] {
        &self.entries
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn value(&self, key: &str) -> Option<&str> {
        self.index.get(key).map(|&i| self.entries[i].value.as_str())
    }
    pub fn string(&self, key: &str) -> Option<String> {
        let value = self.value(key)?;
        for q in ["\"\"", "\""] {
            if value.len() >= 2 * q.len() && value.starts_with(q) && value.ends_with(q) {
                return Some(value[q.len()..value.len() - q.len()].to_string());
            }
        }
        Some(value.to_string())
    }
    pub fn float64(&self, key: &str) -> Result<(f64, bool), Error> {
        let Some(raw) = self.value(key) else {
            return Ok((0.0, false));
        };
        parse_protocol_number(raw)
            .map(|v| (v, true))
            .map_err(|e| Error::other(format!("dicom/siemens: ASCCONV {key}: {e}")))
    }
    pub fn int(&self, key: &str) -> Result<(i64, bool), Error> {
        let Some(raw) = self.value(key) else {
            return Ok((0, false));
        };
        parse_int_base0(raw)
            .map(|v| (v, true))
            .map_err(|e| Error::other(format!("dicom/siemens: ASCCONV {key}: {e}")))
    }
    pub fn slice_array(&self) -> Result<Vec<ProtocolSlice>, Error> {
        let (size, ok) = self.int("sSliceArray.lSize")?;
        if !ok || size <= 0 {
            return Ok(Vec::new());
        }
        if size > MAX_PROTOCOL_SLICES {
            return Err(Error::other(format!(
                "dicom/siemens: sSliceArray.lSize {size} exceeds {MAX_PROTOCOL_SLICES}"
            )));
        }
        let axes = ["dSag", "dCor", "dTra"];
        let mut slices = vec![ProtocolSlice::default(); size as usize];
        for (i, slice) in slices.iter_mut().enumerate() {
            let prefix = format!("sSliceArray.asSlice[{i}].");
            for (axis, name) in axes.iter().enumerate() {
                slice.position[axis] = self.float64(&format!("{prefix}sPosition.{name}"))?.0;
                slice.normal[axis] = self.float64(&format!("{prefix}sNormal.{name}"))?.0;
            }
            slice.thickness = self.float64(&format!("{prefix}dThickness"))?.0;
            slice.in_plane_rotation = self.float64(&format!("{prefix}dInPlaneRot"))?.0;
        }
        Ok(slices)
    }
}
fn parse_protocol_number(raw: &str) -> Result<f64, String> {
    if raw.starts_with("0x") || raw.starts_with("0X") {
        return parse_int_base0(raw)
            .map(|v| v as f64)
            .map_err(|e| e.to_string());
    }
    let (v, err) = numtext::parse_float_value(raw);
    if let Some(e) = err {
        return Err(e.to_string());
    }
    if !v.is_finite() {
        return Err("invalid syntax".to_string());
    }
    Ok(v)
}
pub(crate) fn parse_int_base0(s: &str) -> Result<i64, NumError> {
    let err = |kind| NumError {
        func: "parse_int",
        num: s.to_string(),
        kind,
    };
    if s.is_empty() {
        return Err(err(NumErrorKind::Syntax));
    }
    let (neg, body) = match s.as_bytes()[0] {
        b'+' => (false, &s[1..]),
        b'-' => (true, &s[1..]),
        _ => (false, s),
    };
    let un = parse_uint_base0(body).map_err(err)?;
    let cutoff: u64 = 1 << 63;
    if !neg && un >= cutoff {
        return Err(err(NumErrorKind::Range));
    }
    if neg && un > cutoff {
        return Err(err(NumErrorKind::Range));
    }
    Ok(if neg {
        (un as i64).wrapping_neg()
    } else {
        un as i64
    })
}
fn parse_uint_base0(s: &str) -> Result<u64, NumErrorKind> {
    if s.is_empty() {
        return Err(NumErrorKind::Syntax);
    }
    let b = s.as_bytes();
    let (base, digits, prefixed) =
        if b[0] == b'0' && b.len() >= 3 && matches!(b[1] | 0x20, b'x' | b'b' | b'o') {
            let base = match b[1] | 0x20 {
                b'x' => 16,
                b'b' => 2,
                _ => 8,
            };
            (base, &s[2..], true)
        } else if b[0] == b'0' && b.len() > 1 {
            (8, &s[1..], true)
        } else {
            (10, s, false)
        };
    let _ = prefixed;
    let mut n: u64 = 0;
    let mut underscores = false;
    for c in digits.bytes() {
        if c == b'_' {
            underscores = true;
            continue;
        }
        let d = match c {
            b'0'..=b'9' => (c - b'0') as u64,
            b'a'..=b'z' => (c - b'a' + 10) as u64,
            b'A'..=b'Z' => (c - b'A' + 10) as u64,
            _ => return Err(NumErrorKind::Syntax),
        };
        if d >= base {
            return Err(NumErrorKind::Syntax);
        }
        match n.checked_mul(base).and_then(|v| v.checked_add(d)) {
            Some(v) => n = v,
            None => return Err(NumErrorKind::Range),
        }
    }
    if underscores && !underscore_ok(s) {
        return Err(NumErrorKind::Syntax);
    }
    Ok(n)
}
fn underscore_ok(s: &str) -> bool {
    let b = s.as_bytes();
    let mut saw = b'^';
    let mut i = 0;
    let mut hex = false;
    if !b.is_empty() && (b[0] == b'-' || b[0] == b'+') {
        i = 1;
    }
    let base_prefix =
        b.len() - i >= 2 && b[i] == b'0' && matches!(b[i + 1] | 0x20, b'b' | b'o' | b'x');
    if base_prefix {
        hex = b[i + 1] | 0x20 == b'x';
        i += 2;
        saw = b'0';
    }
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() || (hex && (c | 0x20).is_ascii_hexdigit() && (c | 0x20) <= b'f') {
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
