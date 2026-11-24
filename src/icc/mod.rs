#![forbid(unsafe_code)]
mod build;
mod cache;
pub mod iccprofile;
mod transform;
use crate::fmath;
use crate::textfmt::quote;
pub use build::{ADOBE_RGB_1998, MatrixTrc};
pub use cache::{CacheOptions, CacheStats, CachedTransform, TransformCache};
use std::collections::HashMap;
use std::fmt;
pub use transform::{D50, Transform};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IccError {
    Invalid(String),
    Unsupported(String),
    CacheResourceLimit,
}
impl fmt::Display for IccError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IccError::Invalid(d) => write!(f, "icc: invalid profile: {d}"),
            IccError::Unsupported(d) => write!(f, "icc: unsupported profile: {d}"),
            IccError::CacheResourceLimit => f.write_str("icc: transform cache resource limit"),
        }
    }
}
impl std::error::Error for IccError {}
fn invalid(detail: impl Into<String>) -> IccError {
    IccError::Invalid(detail.into())
}
fn unsupported(detail: impl Into<String>) -> IccError {
    IccError::Unsupported(detail.into())
}
const HEADER_SIZE: i64 = 128;
const MAX_TAGS: i64 = 1024;
pub const MAX_PROFILE_BYTES: i64 = 64 << 20;
#[derive(Debug, Clone)]
pub struct Profile {
    pub version: u32,
    pub class: [u8; 4],
    pub color_space: [u8; 4],
    pub pcs: [u8; 4],
    pub rendering_intent: u32,
    pub illuminant: [f64; 3],
    data: Vec<u8>,
    tags: HashMap<[u8; 4], (usize, usize)>,
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}
fn sig(b: &[u8]) -> [u8; 4] {
    [b[0], b[1], b[2], b[3]]
}
fn printable(s: &[u8]) -> String {
    s.iter()
        .map(|&c| {
            if (32..=126).contains(&c) {
                c as char
            } else {
                '?'
            }
        })
        .collect()
}
fn s15_fixed16(b: &[u8]) -> f64 {
    be32(b) as i32 as f64 / 65536.0
}
impl Profile {
    pub fn parse(data: &[u8]) -> Result<Profile, IccError> {
        if (data.len() as i64) < HEADER_SIZE + 4 {
            return Err(invalid(format!(
                "{} bytes is shorter than the header and tag count",
                data.len()
            )));
        }
        let size = i64::from(be32(&data[0..4]));
        if size < HEADER_SIZE + 4 || size > data.len() as i64 || size > MAX_PROFILE_BYTES {
            return Err(invalid(format!(
                "declared size {size} for {} bytes",
                data.len()
            )));
        }
        let data = &data[..size as usize];
        if &data[36..40] != b"acsp" {
            return Err(invalid("missing the acsp signature"));
        }
        let mut p = Profile {
            version: be32(&data[8..12]),
            class: sig(&data[12..16]),
            color_space: sig(&data[16..20]),
            pcs: sig(&data[20..24]),
            rendering_intent: be32(&data[64..68]),
            illuminant: [0.0; 3],
            data: data.to_vec(),
            tags: HashMap::new(),
        };
        let major = p.version >> 24;
        if !(2..=5).contains(&major) {
            return Err(invalid(format!("version {major}")));
        }
        for i in 0..3 {
            p.illuminant[i] = s15_fixed16(&data[68 + 4 * i..]);
        }
        let count = i64::from(be32(&data[128..132]));
        if count > MAX_TAGS || HEADER_SIZE + 4 + 12 * count > size {
            return Err(invalid(format!("{count} tags do not fit")));
        }
        let table_end = HEADER_SIZE + 4 + 12 * count;
        for i in 0..count as usize {
            let entry = &data[132 + 12 * i..];
            let signature = sig(&entry[0..4]);
            let offset = i64::from(be32(&entry[4..8]));
            let length = i64::from(be32(&entry[8..12]));
            if offset < table_end || length < 8 || offset + length > size {
                return Err(invalid(format!(
                    "tag {} at {offset}+{length} is outside the profile",
                    quote(&printable(&signature))
                )));
            }
            if p.tags.contains_key(&signature) {
                return Err(invalid(format!(
                    "tag {} appears twice",
                    quote(&printable(&signature))
                )));
            }
            p.tags
                .insert(signature, (offset as usize, (offset + length) as usize));
        }
        if &p.pcs != b"XYZ " && &p.pcs != b"Lab " && &p.class != b"link" {
            return Err(invalid(format!(
                "connection space {}",
                quote(&printable(&p.pcs))
            )));
        }
        Ok(p)
    }
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
    pub fn has(&self, signature: &[u8]) -> bool {
        signature.len() == 4 && self.tags.contains_key(&sig(signature))
    }
    fn tag_data(&self, signature: &[u8; 4]) -> Option<&[u8]> {
        self.tags.get(signature).map(|&(a, b)| &self.data[a..b])
    }
    fn tag(&self, signature: &str, types: &[&[u8; 4]]) -> Result<&[u8], IccError> {
        let key = sig(signature.as_bytes());
        let Some(data) = self.tag_data(&key) else {
            return Err(invalid(format!("missing tag {}", quote(signature))));
        };
        let kind = sig(&data[0..4]);
        if types.iter().any(|t| **t == kind) {
            return Ok(data);
        }
        Err(unsupported(format!(
            "tag {} has type {}",
            quote(signature),
            quote(&printable(&kind))
        )))
    }
    #[doc(hidden)]
    pub fn description_bytes(&self) -> Vec<u8> {
        let Some(data) = self.tag_data(b"desc") else {
            return Vec::new();
        };
        match &data[0..4] {
            b"desc" => {
                if data.len() < 12 {
                    return Vec::new();
                }
                let n = i64::from(be32(&data[8..12]));
                if n <= 0 || 12 + n > data.len() as i64 {
                    return Vec::new();
                }
                trim_nul(data[12..12 + n as usize].to_vec())
            }
            b"mluc" => {
                if data.len() < 28 {
                    return Vec::new();
                }
                let length = i64::from(be32(&data[20..24]));
                let offset = i64::from(be32(&data[24..28]));
                if length < 0 || offset < 0 || offset + length > data.len() as i64 {
                    return Vec::new();
                }
                let (offset, end) = (offset as usize, (offset + length) as usize);
                let mut s = String::new();
                let mut i = offset;
                while i + 1 < end {
                    let unit = u32::from(be16(&data[i..]));
                    s.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
                    i += 2;
                }
                trim_nul(s.into_bytes())
            }
            _ => Vec::new(),
        }
    }
    pub fn description(&self) -> String {
        lossy_text(&self.description_bytes())
    }
}
#[doc(hidden)]
pub fn lossy_text(bytes: &[u8]) -> String {
    crate::textfmt::runes(bytes).into_iter().collect()
}
fn trim_nul(mut s: Vec<u8>) -> Vec<u8> {
    while s.last() == Some(&0) {
        s.pop();
    }
    s
}
#[derive(Debug, Clone, PartialEq)]
enum Curve {
    Identity,
    Gamma(f64),
    Table(Vec<f64>),
    Parametric {
        kind: u16,
        g: f64,
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        e: f64,
        f: f64,
    },
}
impl Curve {
    fn eval(&self, x: f64) -> f64 {
        match self {
            Curve::Identity => x,
            Curve::Gamma(g) => fmath::pow(clamp01(x), *g),
            Curve::Table(t) => interpolate(t, clamp01(x)),
            &Curve::Parametric {
                kind,
                g,
                a,
                b,
                c,
                d,
                e,
                f,
            } => {
                let x = clamp01(x);
                let pow = |v: f64| if v <= 0.0 { 0.0 } else { fmath::pow(v, g) };
                let mut y = 0.0;
                match kind {
                    0 => y = pow(x),
                    1 => {
                        if x >= -b / a {
                            y = pow(a * x + b);
                        }
                    }
                    2 => {
                        y = c;
                        if x >= -b / a {
                            y = pow(a * x + b) + c;
                        }
                    }
                    3 => {
                        y = if x >= d { pow(a * x + b) } else { c * x };
                    }
                    4 => {
                        y = if x >= d {
                            pow(a * x + b) + e
                        } else {
                            c * x + f
                        };
                    }
                    _ => {}
                }
                clamp01(y)
            }
        }
    }
}
fn interpolate(t: &[f64], x: f64) -> f64 {
    if t.len() == 1 {
        return t[0];
    }
    let pos = x * (t.len() - 1) as f64;
    let i = pos as usize;
    if i >= t.len() - 1 {
        return t[t.len() - 1];
    }
    let f = pos - i as f64;
    t[i] + (t[i + 1] - t[i]) * f
}
fn pad4(n: usize) -> usize {
    (n + 3) & !3
}
fn parse_curve(data: &[u8]) -> Result<(Curve, usize), IccError> {
    if data.len() < 12 {
        return Err(invalid("truncated curve"));
    }
    match &data[0..4] {
        b"curv" => {
            let n = be32(&data[8..12]) as usize;
            if n > (data.len() - 12) / 2 {
                return Err(invalid(format!("curve of {n} entries is truncated")));
            }
            let size = pad4(12 + 2 * n);
            match n {
                0 => return Ok((Curve::Identity, size)),
                1 => return Ok((Curve::Gamma(f64::from(be16(&data[12..14])) / 256.0), size)),
                _ => {}
            }
            let table = (0..n)
                .map(|i| f64::from(be16(&data[12 + 2 * i..])) / 65535.0)
                .collect();
            Ok((Curve::Table(table), size))
        }
        b"para" => {
            let kind = be16(&data[8..10]);
            const COUNTS: [usize; 5] = [1, 3, 4, 5, 7];
            if kind as usize >= COUNTS.len() {
                return Err(unsupported(format!("parametric curve function {kind}")));
            }
            let n = COUNTS[kind as usize];
            if data.len() < 12 + 4 * n {
                return Err(invalid("truncated parametric curve"));
            }
            let mut v = [0.0f64; 7];
            for (i, slot) in v.iter_mut().enumerate().take(n) {
                *slot = s15_fixed16(&data[12 + 4 * i..]);
            }
            let mut a = v[1];
            if kind == 0 {
                a = 1.0;
            }
            if (kind == 1 || kind == 2) && a == 0.0 {
                return Err(invalid("parametric curve with a = 0"));
            }
            Ok((
                Curve::Parametric {
                    kind,
                    g: v[0],
                    a,
                    b: v[2],
                    c: v[3],
                    d: v[4],
                    e: v[5],
                    f: v[6],
                },
                pad4(12 + 4 * n),
            ))
        }
        other => Err(unsupported(format!(
            "curve type {}",
            quote(&printable(other))
        ))),
    }
}
fn clamp01(x: f64) -> f64 {
    if x < 0.0 || x.is_nan() {
        0.0
    } else if x > 1.0 {
        1.0
    } else {
        x
    }
}
