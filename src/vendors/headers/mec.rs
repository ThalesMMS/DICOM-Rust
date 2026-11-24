use super::{Budget, format_g32, header_ascii, private_value};
use crate::charset::{decode_euc_jp, decode_shift_jis};
use crate::numtext::format_float;
use crate::object::Object;
use crate::vendors::{Cursor, Error, Kind, Limits, trim_right_bytes};
#[derive(Clone, Debug, Default)]
pub struct MecMr3Options {
    pub limits: Limits,
    pub text_encoding: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MecMr3Record {
    pub group: u8,
    pub key: u32,
    pub kind: u32,
    pub values: Vec<String>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8,
    ShiftJis,
    EucJp,
}
pub fn decode_mec_mr3(raw: &[u8], l: Limits) -> Result<Vec<MecMr3Record>, Error> {
    decode_mec_mr3_with_options(
        raw,
        &MecMr3Options {
            limits: l,
            text_encoding: String::new(),
        },
    )
}
pub fn decode_mec_mr3_with_options(
    raw: &[u8],
    opts: &MecMr3Options,
) -> Result<Vec<MecMr3Record>, Error> {
    let mut b = Budget::new(opts.limits);
    b.take(0, raw.len() as i64, 0)?;
    let enc = match opts.text_encoding.as_str() {
        "" | "UTF-8" => Encoding::Utf8,
        "Shift-JIS" => Encoding::ShiftJis,
        "EUC-JP" => Encoding::EucJp,
        _ => return Err(Error::bare(Kind::UnsupportedHeader)),
    };
    let mut c = Cursor { data: raw, pos: 0 };
    let mut records = Vec::new();
    let mut groups = 6;
    let mut group = 1;
    while group <= groups {
        if group == 6 {
            let n = c.uint32()?;
            if n == 0 || n > 250 {
                return Err(Error::bare(Kind::Invalid));
            }
            groups = 5 + n;
        }
        let count = c.uint32()?;
        if group <= 6 && count == 0 {
            return Err(Error::bare(Kind::Invalid));
        }
        if i64::from(count) > b.entries_left() {
            return Err(Error::bare(Kind::Limit));
        }
        if count as usize > c.remaining() / 32 {
            return Err(Error::bare(Kind::Invalid));
        }
        b.take(1 + i64::from(count), 0, 0)?;
        for _ in 0..count {
            let key = c.uint32()?;
            let kind = c.uint32()?;
            let n = c.uint32()? as usize;
            let mut sig = [0u32; 5];
            for word in &mut sig {
                *word = c.uint32()?;
            }
            if sig[0] != 0 || sig[2] != 12 || sig[3] != 0 {
                return Err(Error::new(Kind::Invalid, "MEC record signature"));
            }
            if n > c.remaining() {
                return Err(Error::bare(Kind::Invalid));
            }
            let payload = &raw[c.pos..c.pos + n];
            c.pos += n;
            let values = decode_values(kind, payload, enc, &mut b)
                .map_err(|e| e.prefixed(format!("MEC group {group} key {key:08x}")))?;
            records.push(MecMr3Record {
                group: group as u8,
                key,
                kind,
                values,
            });
        }
        group += 1;
    }
    if c.remaining() == 1 && raw[c.pos] == 0 {
        c.pos += 1;
    }
    if c.remaining() != 0 {
        return Err(Error::new(Kind::Invalid, "MEC trailing bytes"));
    }
    Ok(records)
}
fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn invalid<T>() -> Result<T, Error> {
    Err(Error::bare(Kind::Invalid))
}
struct Values<'a> {
    b: &'a mut Budget,
    values: Vec<String>,
}
impl Values<'_> {
    fn add(&mut self, s: String) -> Result<(), Error> {
        self.b.take(1, 0, s.len() as i64)?;
        self.values.push(s);
        Ok(())
    }
}
fn fixed_ranges(kind: u32, p: &[u8]) -> Option<Vec<(usize, usize)>> {
    match kind {
        0x1f4100 => (p.len() == 136 && le32(p) == 0 && p[134] == 0 && p[135] == 0)
            .then(|| vec![(4, 69), (69, 134)]),
        0x1f4300 => (p.len() == 436 && le32(p) == 0).then(|| {
            vec![
                (4, 21),
                (21, 30),
                (30, 44),
                (44, 73),
                (73, 329),
                (329, 394),
                (394, 411),
                (411, 414),
                (414, 432),
            ]
        }),
        0x1f4400 => (p.len() == 516).then(|| {
            vec![
                (0, 65),
                (65, 86),
                (86, 342),
                (342, 359),
                (359, 424),
                (424, 489),
            ]
        }),
        _ => (p.len() == 325).then(|| (0..5).map(|i| (i * 65, (i + 1) * 65)).collect()),
    }
}
fn decode_values(kind: u32, p: &[u8], enc: Encoding, b: &mut Budget) -> Result<Vec<String>, Error> {
    let mut out = Values {
        b,
        values: Vec::new(),
    };
    match kind {
        0xff000400 | 0xff002a00 | 0xff000800 | 0xff002900 | 0xff002400 | 0xff002800 | 0x500
        | 0x600 => {
            let width = if kind == 0xff002900 { 8 } else { 4 };
            if p.is_empty() || !p.len().is_multiple_of(width) {
                return invalid();
            }
            let single = matches!(kind, 0xff000400 | 0xff002a00 | 0xff000800 | 0xff002900);
            if single && p.len() != width {
                return invalid();
            }
            if kind == 0x500 && !p.len().is_multiple_of(8)
                || kind == 0x600 && !p.len().is_multiple_of(12)
            {
                return invalid();
            }
            if (p.len() / width) as i64 > out.b.entries_left() {
                return Err(Error::bare(Kind::Limit));
            }
            for v in p.chunks_exact(width) {
                let u = le32(v);
                let s = match kind {
                    0xff000400 => u.to_string(),
                    0xff002a00 => {
                        if u > 1 {
                            return invalid();
                        }
                        (u == 1).to_string()
                    }
                    0xff002400 => (u as i32).to_string(),
                    _ if width == 8 => {
                        let f =
                            f64::from_le_bytes([v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]]);
                        if !f.is_finite() {
                            return invalid();
                        }
                        format_float(f, 'g', -1)
                    }
                    _ => {
                        let f = f32::from_bits(u);
                        if !f.is_finite() {
                            return invalid();
                        }
                        format_g32(f)
                    }
                };
                out.add(s)?;
            }
        }
        0xff002c00 => out.add(mec_text(p, enc)?)?,
        0x300 => {
            if p.starts_with(&[0xdf, 0xff, 0x79]) {
                let framed = (19..=259).contains(&p.len())
                    && usize::from(p[3]) == p.len() - 4
                    && p[4] == 1
                    && p[5] == 9
                    && p[6] == 0
                    && &p[7..16] == b"ISO8859-1"
                    && p[16] == 2
                    && usize::from(p[17]) == p.len() - 19
                    && p[18] == 0;
                if !framed {
                    return invalid();
                }
                let q = &p[19..];
                if std::str::from_utf8(q).is_err() && enc == Encoding::Utf8 {
                    return Err(Error::new(
                        Kind::UnsupportedHeader,
                        "legacy marked text needs explicit encoding",
                    ));
                }
                let text = mec_text(q, enc)?;
                out.add(format!("ISO8859-1 marker: {text}"))?;
                return Ok(out.values);
            }
            out.add(header_ascii(trim_right_bytes(p, b"\0"))?)?;
        }
        0xe00 => {
            let s = header_ascii(trim_right_bytes(p, b"\0"))?;
            if s.len() != 19 {
                return invalid();
            }
            if !s
                .bytes()
                .all(|c| c.is_ascii_digit() || matches!(c, b'/' | b',' | b':'))
            {
                return invalid();
            }
            out.add(s)?;
        }
        0x1f4000 => {
            if p.is_empty() || !p.len().is_multiple_of(340) {
                return invalid();
            }
            for record in p.as_chunks::<340>().0 {
                out.add(le32(record).to_string())?;
                for field in record[4..].as_chunks::<48>().0 {
                    out.add(mec_text(field, enc)?)?;
                }
            }
        }
        0x1f4100 | 0x1f4300 | 0x1f4400 | 0x1f4600 => {
            let Some(ranges) = fixed_ranges(kind, p) else {
                return invalid();
            };
            for (from, to) in ranges {
                out.add(fixed_ascii(&p[from..to])?)?;
            }
            if kind == 0x1f4300 {
                out.add(le32(&p[432..]).to_string())?;
            }
            if kind == 0x1f4400 {
                for flag in p[492..516].as_chunks::<4>().0 {
                    let u = le32(flag);
                    if u > 1 {
                        return invalid();
                    }
                    out.add((u == 1).to_string())?;
                }
            }
        }
        0xff002300 => {
            if p.len() as i64 > (out.b.limits.max_decoded_bytes - out.b.decoded) / 2 {
                return Err(Error::bare(Kind::Limit));
            }
            let hex: String = p.iter().map(|v| format!("{v:02x}")).collect();
            out.add(format!("charset bytes: {hex} (not interpreted)"))?;
        }
        _ => {
            return Err(Error::new(
                Kind::UnsupportedHeader,
                format!("MEC type {kind:08x}"),
            ));
        }
    }
    Ok(out.values)
}
fn nul_padded(p: &[u8]) -> Result<Option<&[u8]>, Error> {
    let Some(k) = p.iter().position(|&v| v == 0) else {
        return Ok(None);
    };
    if p[k..].iter().any(|&v| v != 0) {
        return invalid();
    }
    Ok(Some(&p[..k]))
}
fn fixed_ascii(p: &[u8]) -> Result<String, Error> {
    match nul_padded(p)? {
        Some(text) => header_ascii(text),
        None => invalid(),
    }
}
fn mec_text(p: &[u8], enc: Encoding) -> Result<String, Error> {
    let p = nul_padded(p)?.unwrap_or(p);
    let s = match enc {
        Encoding::Utf8 => match std::str::from_utf8(p) {
            Ok(s) => s.to_string(),
            Err(_) => {
                return Err(Error::new(
                    Kind::UnsupportedHeader,
                    "MEC text requires explicit encoding",
                ));
            }
        },
        Encoding::ShiftJis => decode_shift_jis(p).ok_or(Error::bare(Kind::Invalid))?,
        Encoding::EucJp => decode_euc_jp(p).ok_or(Error::bare(Kind::Invalid))?,
    };
    if s.contains('\u{FFFD}') {
        return invalid();
    }
    Ok(s)
}
pub fn toshiba_mec_mr3_of(o: &Object, l: Limits) -> Result<Vec<MecMr3Record>, Error> {
    let l = l.normalized();
    let raw = private_value(o, 0x700d, "TOSHIBA_MEC_MR3", 0x08, l)?;
    decode_mec_mr3(&raw, l)
}
pub(super) fn lines(records: &[MecMr3Record], l: Limits) -> Result<Vec<String>, Error> {
    let mut b = Budget::new(l);
    let mut lines = Vec::new();
    for r in records {
        b.line(
            &mut lines,
            format!(
                "({:02x},{:08x}) type={:08x} [{}]",
                r.group,
                r.key,
                r.kind,
                r.values.join("\u{5C}")
            ),
        )?;
    }
    Ok(lines)
}
