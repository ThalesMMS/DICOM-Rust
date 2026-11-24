use super::private_headers::{
    CANON_PAS_HEADER, CANON_PMTF_HEADER, PHILIPS_SDS_HEADER, PMTF_HEADER, TOSHIBA_CT3_HEADER,
    TOSHIBA_MEC_MR3_HEADER, TOSHIBA_PMTF_HEADER,
};
use super::{Error, Kind, Limits};
use crate::dictionary::reservations::private_tag;
use crate::numtext::format_float;
use crate::object::{DataSet, Element, Object, Tag, UNDEFINED_LENGTH, Value};
mod dataset;
mod mec;
mod pas;
mod sds;
pub use dataset::{decode_pmtf, decode_toshiba_ct3, pmtf_of, toshiba_ct3_of};
pub use mec::{
    MecMr3Options, MecMr3Record, decode_mec_mr3, decode_mec_mr3_with_options, toshiba_mec_mr3_of,
};
pub use pas::{canon_pas_of, decode_canon_pas};
pub use sds::{PhilipsSdsItem, SdsValue, decode_philips_sds, philips_sds_of};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Definition {
    pub kind: &'static str,
    pub title: &'static str,
    pub creator: &'static str,
    pub group: u16,
    pub offset: u8,
}
const fn spec(
    kind: &'static str,
    title: &'static str,
    creator: &'static str,
    group: u16,
    offset: u8,
) -> Definition {
    Definition {
        kind,
        title,
        creator,
        group,
        offset,
    }
}
const SPECS: &[Definition] = &[
    spec(
        PHILIPS_SDS_HEADER,
        "Philips SDS",
        "Philips MR Imaging DD 002",
        0x2005,
        0x32,
    ),
    spec(
        TOSHIBA_CT3_HEADER,
        "Toshiba CT Private Data 2",
        "TOSHIBA_MEC_CT3",
        0x7005,
        0x10,
    ),
    spec(
        TOSHIBA_MEC_MR3_HEADER,
        "Toshiba MEC MR3 Original Data",
        "TOSHIBA_MEC_MR3",
        0x700d,
        0x08,
    ),
    spec(
        PMTF_HEADER,
        "PMTF Information Data",
        "PMTF INFORMATION DATA",
        0x0029,
        1,
    ),
    spec(
        CANON_PMTF_HEADER,
        "Canon MEC MR3 PMTF",
        "CANON_MEC_MR3",
        0x0029,
        1,
    ),
    spec(
        TOSHIBA_PMTF_HEADER,
        "Toshiba MEC MR3 PMTF",
        "TOSHIBA_MEC_MR3",
        0x0029,
        1,
    ),
    spec(
        TOSHIBA_PMTF_HEADER,
        "Toshiba MEC MR3 PMTF",
        "TOSHIBA_MEC_MR3",
        0x0029,
        2,
    ),
    spec(
        CANON_PAS_HEADER,
        "Canon PAS Reproduct Information (XML)",
        "CANON_MEC_MR3^10",
        0x700d,
        0x19,
    ),
];
pub fn definitions() -> &'static [Definition] {
    SPECS
}
#[derive(Clone, Debug)]
pub struct Header {
    pub kind: &'static str,
    pub title: &'static str,
    pub tag: Tag,
    pub lines: Vec<String>,
    pub truncated: bool,
    pub err: Option<Error>,
}
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub kinds: Vec<String>,
    pub limits: Limits,
    pub max_lines: i64,
}
pub fn decode_private_headers(o: &Object, opts: &Options) -> Vec<Header> {
    let max = if opts.max_lines <= 0 {
        20000
    } else {
        usize::try_from(opts.max_lines).unwrap_or(usize::MAX)
    };
    let mut out = Vec::new();
    for spec in SPECS {
        if !opts.kinds.is_empty() && !opts.kinds.iter().any(|k| k == spec.kind) {
            continue;
        }
        let Ok(tag) = private_tag_required(o, spec.group, spec.creator, spec.offset) else {
            continue;
        };
        let mut h = Header {
            kind: spec.kind,
            title: spec.title,
            tag,
            lines: Vec::new(),
            truncated: false,
            err: None,
        };
        match render(o, spec, opts.limits.normalized()) {
            Ok(mut lines) => {
                if lines.len() > max {
                    lines.truncate(max);
                    h.truncated = true;
                }
                h.lines = lines;
            }
            Err(err) => h.err = Some(err),
        }
        out.push(h);
    }
    out
}
pub fn render_private_header(
    o: &Object,
    kind: &str,
    tag: Tag,
    limits: Limits,
) -> Result<Vec<String>, Error> {
    for spec in SPECS.iter().filter(|s| s.kind == kind) {
        if private_tag_required(o, spec.group, spec.creator, spec.offset) == Ok(tag) {
            return render(o, spec, limits.normalized());
        }
    }
    Err(Error::bare(Kind::NotFound))
}
fn render(o: &Object, spec: &Definition, l: Limits) -> Result<Vec<String>, Error> {
    match spec.kind {
        PHILIPS_SDS_HEADER => sds::lines(&philips_sds_of(o, l)?, l),
        TOSHIBA_CT3_HEADER => {
            let obj = toshiba_ct3_of(o, l)?;
            let mut lines = Vec::new();
            dataset::object_lines(&obj, &mut Budget::new(l), "", 0, &mut lines)?;
            Ok(lines)
        }
        TOSHIBA_MEC_MR3_HEADER => mec::lines(&toshiba_mec_mr3_of(o, l)?, l),
        PMTF_HEADER | CANON_PMTF_HEADER | TOSHIBA_PMTF_HEADER => {
            let items = pmtf_of(o, spec.creator, spec.offset, l)?;
            let mut b = Budget::new(l);
            let mut lines = Vec::new();
            for (i, item) in items.iter().enumerate() {
                b.line(&mut lines, format!("Item {i}"))?;
                dataset::object_lines(item, &mut b, "", 0, &mut lines)?;
            }
            Ok(lines)
        }
        CANON_PAS_HEADER => {
            let text = canon_pas_of(o, l)?.replace("\r\n", "\n");
            let text = text.trim_end_matches(['\n', '\0', ' ']);
            if text.is_empty() {
                return Err(Error::bare(Kind::Invalid));
            }
            Ok(text.split('\n').map(str::to_string).collect())
        }
        _ => Err(Error::bare(Kind::UnsupportedHeader)),
    }
}
fn suffixed(err: Error, detail: &str) -> Error {
    Error {
        kind: err.kind,
        text: format!("{err}: {detail}"),
    }
}
struct Budget {
    limits: Limits,
    entries: i64,
    bytes: i64,
    decoded: i64,
}
impl Budget {
    fn new(l: Limits) -> Budget {
        Budget {
            limits: l.normalized(),
            entries: 0,
            bytes: 0,
            decoded: 0,
        }
    }
    fn entries_left(&self) -> i64 {
        self.limits.max_entries - self.entries
    }
    fn take(&mut self, entries: i64, raw: i64, decoded: i64) -> Result<(), Error> {
        if entries < 0
            || raw < 0
            || decoded < 0
            || entries > self.entries_left()
            || raw > self.limits.max_bytes - self.bytes
            || decoded > self.limits.max_decoded_bytes - self.decoded
        {
            return Err(Error::bare(Kind::Limit));
        }
        self.entries += entries;
        self.bytes += raw;
        self.decoded += decoded;
        Ok(())
    }
    fn line(&mut self, lines: &mut Vec<String>, s: String) -> Result<(), Error> {
        self.take(1, 0, s.len() as i64)?;
        lines.push(s);
        Ok(())
    }
}
fn private_tag_required(o: &Object, group: u16, creator: &str, offset: u8) -> Result<Tag, Error> {
    match private_tag(o, group, creator, offset) {
        Ok(tag) if o.has(tag) => Ok(tag),
        _ => Err(Error::bare(Kind::NotFound)),
    }
}
fn private_value(
    o: &Object,
    group: u16,
    creator: &str,
    offset: u8,
    l: Limits,
) -> Result<Vec<u8>, Error> {
    let tag = private_tag_required(o, group, creator, offset)?;
    let Some(el) = o.get(tag) else {
        return Err(Error::bare(Kind::NotFound));
    };
    let Value::Raw(raw) = &el.value else {
        return Err(Error::bare(Kind::Invalid));
    };
    primitive_bytes(el, l.max_bytes)?;
    Ok(raw.clone())
}
fn private_sequence(o: &Object, tag: Tag, b: &mut Budget) -> Result<Vec<Object>, Error> {
    let Some(el) = o.get(tag) else {
        return Err(Error::bare(Kind::NotFound));
    };
    let Value::Sequence(items) = &el.value else {
        return Err(Error::new(Kind::Invalid, "expected sequence"));
    };
    check_sequence(items, 0, b)?;
    o.get_sequence(tag).ok_or(Error::bare(Kind::Invalid))
}
fn check_sequence(items: &[DataSet], depth: usize, b: &mut Budget) -> Result<(), Error> {
    if depth >= 64 {
        return Err(Error::new(Kind::Limit, "sequence depth"));
    }
    b.take(items.len() as i64, 0, 0)?;
    for item in items {
        b.take(item.elements.len() as i64, 0, 0)?;
        for el in &item.elements {
            if let Value::Sequence(nested) = &el.value {
                check_sequence(nested, depth + 1, b)?;
                continue;
            }
            let n = primitive_bytes(el, b.limits.max_bytes - b.bytes)?;
            b.take(0, n, 0)?;
        }
    }
    Ok(())
}
fn primitive_bytes(el: &Element, remaining: i64) -> Result<i64, Error> {
    if remaining < 0 {
        return Err(Error::bare(Kind::Limit));
    }
    let mut n: i64 = match &el.value {
        Value::Raw(raw) => {
            if raw.len() as i64 > remaining {
                return Err(Error::bare(Kind::Limit));
            }
            raw.len() as i64
        }
        Value::Strings(parts) => {
            let mut n = parts.len().saturating_sub(1) as i64;
            if n > remaining {
                return Err(Error::bare(Kind::Limit));
            }
            for part in parts {
                if part.len() as i64 > remaining - n {
                    return Err(Error::bare(Kind::Limit));
                }
                n += part.len() as i64;
            }
            if n % 2 != 0 {
                if n == remaining {
                    return Err(Error::bare(Kind::Limit));
                }
                n += 1;
            }
            n
        }
        _ => {
            return Err(Error::new(
                Kind::Invalid,
                "unresolved or unsupported primitive value",
            ));
        }
    };
    if el.header.length_set {
        if el.header.length == UNDEFINED_LENGTH {
            return Err(Error::new(
                Kind::Invalid,
                "undefined primitive value length",
            ));
        }
        let stored = i64::from(el.header.length);
        if stored > remaining {
            return Err(Error::bare(Kind::Limit));
        }
        n = n.max(stored);
    }
    Ok(n)
}
fn header_ascii(raw: &[u8]) -> Result<String, Error> {
    for &v in raw {
        if v > 127 {
            return Err(Error::new(
                Kind::UnsupportedHeader,
                "undeclared header text encoding",
            ));
        }
        if v < 32 && !matches!(v, b'\n' | b'\r' | b'\t') {
            return Err(Error::bare(Kind::Invalid));
        }
    }
    Ok(raw.iter().map(|&b| char::from(b)).collect())
}
fn rounded_digits(x: f64, n: usize) -> (String, i32) {
    let text = format_float(x, 'e', n as i32 - 1);
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let digits = mantissa.chars().filter(char::is_ascii_digit).collect();
    (digits, exponent.parse().unwrap_or(0))
}
fn shortest_digits32(f: f32) -> (String, i32) {
    let reads_back = |digits: &str, exp: i32| {
        format!("{}e{}", digits, exp - (digits.len() as i32 - 1)).parse::<f32>() == Ok(f)
    };
    for n in 1..9 {
        let (digits, exp) = rounded_digits(f64::from(f), n);
        if reads_back(&digits, exp) {
            return (digits, exp);
        }
        if let Ok(value) = digits.parse::<u64>() {
            for neighbour in [value + 1, value.saturating_sub(1)] {
                let digits = neighbour.to_string();
                if digits.len() == n && reads_back(&digits, exp) {
                    return (digits, exp);
                }
            }
        }
    }
    rounded_digits(f64::from(f), 9)
}
fn format_g32(f: f32) -> String {
    if f == 0.0 {
        return if f.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let (digits, exp) = shortest_digits32(f.abs());
    let digits = match digits.trim_end_matches('0') {
        "" => "0",
        trimmed => trimmed,
    };
    let mut out = String::new();
    if f < 0.0 {
        out.push('-');
    }
    if !(-4..6).contains(&exp) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push_str(&format!(
            "e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        ));
        return out;
    }
    let point = exp + 1;
    if point <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat(point.unsigned_abs() as usize));
        out.push_str(digits);
    } else if point as usize >= digits.len() {
        out.push_str(digits);
        out.push_str(&"0".repeat(point as usize - digits.len()));
    } else {
        out.push_str(&digits[..point as usize]);
        out.push('.');
        out.push_str(&digits[point as usize..]);
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_float32_like_go() {
        for (f, want) in [
            (1.5f32, "1.5"),
            (-0.0, "-0"),
            (0.1, "0.1"),
            (1e20, "1e+20"),
            (1e21, "1e+21"),
            (123456790.0, "1.2345679e+08"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-2.5e-10, "-2.5e-10"),
            (16777216.0, "1.6777216e+07"),
            (100000.0, "100000"),
            (1000000.0, "1e+06"),
            (25_147.313, "25147.312"),
            (f32::MAX, "3.4028235e+38"),
            (f32::MIN_POSITIVE, "1.1754944e-38"),
            (1e-45, "1e-45"),
            (8388608.0, "8.388608e+06"),
        ] {
            assert_eq!(format_g32(f), want);
        }
    }
    #[test]
    fn budget_refuses_past_its_limits() {
        let mut b = Budget::new(Limits {
            max_bytes: 4,
            max_decoded_bytes: 4,
            max_entries: 2,
        });
        assert!(b.take(2, 4, 4).is_ok());
        assert!(b.take(1, 0, 0).is_err());
        assert!(b.take(0, 0, 0).is_ok());
    }
}
