use super::{Error, brotli, corrupt};
use crate::textfmt;
pub(crate) const SIGNATURE: [u8; 12] = [
    0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A,
];
const MAX_BOX_PAYLOAD: usize = 64 << 20;
#[derive(Default)]
pub(crate) struct Container<'a> {
    pub codestream: std::borrow::Cow<'a, [u8]>,
    pub jbrd: Option<std::borrow::Cow<'a, [u8]>>,
    pub exif: Option<std::borrow::Cow<'a, [u8]>>,
    pub xmp: Option<std::borrow::Cow<'a, [u8]>>,
}
fn box_type_quote(typ: &[u8]) -> String {
    let mut out = String::from("\"");
    let mut rest = typ;
    while let Some(&first) = rest.first() {
        let (r, width) = textfmt::decode_rune(rest);
        match r {
            Some(r) => {
                let q = textfmt::quote(&r.to_string());
                out.push_str(q.get(1..q.len().saturating_sub(1)).unwrap_or_default());
            }
            None => out.push_str(&format!("\\x{first:02x}")),
        }
        rest = rest.get(width.max(1)..).unwrap_or_default();
    }
    out.push('"');
    out
}
pub(crate) fn parse(data: &[u8]) -> Result<Container<'_>, Error> {
    use std::borrow::Cow;
    if data.starts_with(&[0xFF, 0x0A]) {
        return Ok(Container {
            codestream: Cow::Borrowed(data),
            ..Default::default()
        });
    }
    if !data.starts_with(&SIGNATURE) {
        return Err(corrupt("not a JPEG XL file"));
    }
    let mut c = Container {
        ..Default::default()
    };
    let mut parts: Option<Cow<'_, [u8]>> = None;
    let (mut saw_jxlc, mut saw_jxlp, mut last_part) = (false, false, false);
    let mut expect_index: u32 = 0;
    let mut pos = SIGNATURE.len();
    while let Some(rest) = data.get(pos..)
        && !rest.is_empty()
    {
        if rest == [0] {
            break;
        }
        let Some(&[s0, s1, s2, s3, t0, t1, t2, t3]) = rest.first_chunk::<8>() else {
            return Err(corrupt("truncated box header"));
        };
        let mut size = u64::from(u32::from_be_bytes([s0, s1, s2, s3]));
        let raw_typ = [t0, t1, t2, t3];
        let mut header = 8usize;
        match size {
            0 => size = rest.len() as u64,
            1 => {
                let Some(b) = rest.get(8..16).and_then(|b| <[u8; 8]>::try_from(b).ok()) else {
                    return Err(corrupt("truncated box header"));
                };
                size = u64::from_be_bytes(b);
                header = 16;
            }
            _ => {}
        }
        if size < header as u64 || size > rest.len() as u64 {
            return Err(corrupt(format!("box {} size", box_type_quote(&raw_typ))));
        }
        let body = rest.get(header..size as usize).unwrap_or_default();
        let mut payload: Cow<'_, [u8]> = Cow::Borrowed(body);
        pos = pos.saturating_add(size as usize);
        let mut typ: Vec<u8> = raw_typ.to_vec();
        if typ == b"brob" {
            let Some((inner, compressed)) = payload.split_first_chunk::<4>() else {
                return Err(corrupt("brob box too small"));
            };
            let inner = inner.to_vec();
            if inner == b"jxlc" || inner == b"jxlp" || inner == b"brob" {
                return Err(corrupt(format!(
                    "brob box wraps {}",
                    box_type_quote(&inner)
                )));
            }
            let decoded = brotli::decode(compressed, MAX_BOX_PAYLOAD)
                .map_err(|e| corrupt(format!("brob box: {e}")))?;
            typ = inner;
            payload = Cow::Owned(decoded);
        }
        match typ.as_slice() {
            b"jxlc" => {
                if saw_jxlc || saw_jxlp {
                    return Err(corrupt("multiple codestream boxes"));
                }
                saw_jxlc = true;
                parts = Some(payload);
            }
            b"jxlp" => {
                if saw_jxlc || last_part {
                    return Err(corrupt("unexpected jxlp box"));
                }
                let Some((&idx, part)) = payload.split_first_chunk::<4>() else {
                    return Err(corrupt("jxlp box too small"));
                };
                let idx = u32::from_be_bytes(idx);
                if idx & 0x7FFF_FFFF != expect_index {
                    return Err(corrupt("jxlp boxes out of order"));
                }
                expect_index = expect_index.wrapping_add(1);
                last_part = idx & 0x8000_0000 != 0;
                saw_jxlp = true;
                let mut joined = parts.take().map(Cow::into_owned).unwrap_or_default();
                joined.extend_from_slice(part);
                parts = Some(Cow::Owned(joined));
            }
            b"jbrd" => {
                if c.jbrd.is_some() {
                    return Err(corrupt("multiple jbrd boxes"));
                }
                c.jbrd = Some(payload);
            }
            b"Exif" if c.exif.is_none() => c.exif = Some(payload),
            b"xml " if c.xmp.is_none() => c.xmp = Some(payload),
            _ => {}
        }
    }
    if !saw_jxlc && !saw_jxlp {
        return Err(corrupt("no codestream box"));
    }
    if saw_jxlp && !last_part {
        return Err(corrupt("missing final jxlp box"));
    }
    c.codestream = parts.unwrap_or_default();
    Ok(c)
}
fn put_box(out: &mut Vec<u8>, typ: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len().saturating_add(8) as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(payload);
}
pub(crate) fn wrap(codestream: &[u8], jbrd: Option<&[u8]>) -> Vec<u8> {
    let mut out = SIGNATURE.to_vec();
    put_box(&mut out, b"ftyp", b"jxl \0\0\0\0jxl ");
    if let Some(j) = jbrd {
        put_box(&mut out, b"jbrd", j);
    }
    put_box(&mut out, b"jxlc", codestream);
    out
}
