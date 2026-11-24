use super::ber::decode_packet;
use crate::qrmatch::simple_fold_char;
use crate::textfmt::{decode_rune, runes};
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Atv {
    pub typ: Vec<u8>,
    pub value: Vec<u8>,
}
pub(crate) type Rdn = Vec<Atv>;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Dn {
    pub rdns: Vec<Rdn>,
}
fn push_rune(out: &mut Vec<u8>, c: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}
fn strip_spaces(v: &[u8]) -> Vec<u8> {
    let start = v.iter().position(|&c| c != b' ').unwrap_or(v.len());
    let end = v.iter().rposition(|&c| c != b' ').map_or(start, |i| i + 1);
    let mut out = v[start..end.max(start)].to_vec();
    if out.last() == Some(&b'\\') && v.last() == Some(&b' ') {
        out.push(b' ');
    }
    out
}
fn hex_val(c: char) -> Option<u8> {
    c.to_digit(16).filter(|_| c.is_ascii()).map(|d| d as u8)
}
fn decode_string(v: &[u8]) -> Result<Vec<u8>, ()> {
    let s = runes(&strip_spaces(v));
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c != '\\' {
            push_rune(&mut out, c);
            i += 1;
            continue;
        }
        if i + 1 >= s.len() {
            return Err(());
        }
        if " \"#+,;<=>\\".contains(s[i + 1]) {
            push_rune(&mut out, s[i + 1]);
            i += 2;
            continue;
        }
        if i + 2 >= s.len() {
            return Err(());
        }
        match (hex_val(s[i + 1]), hex_val(s[i + 2])) {
            (Some(h), Some(l)) => out.push(h << 4 | l),
            _ => return Err(()),
        }
        i += 3;
    }
    Ok(out)
}
fn decode_encoded_string(v: &[u8]) -> Result<Vec<u8>, ()> {
    if !v.len().is_multiple_of(2) {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(v.len() / 2);
    for pair in v.chunks(2) {
        let h = (pair[0] as char).to_digit(16).ok_or(())?;
        let l = (pair[1] as char).to_digit(16).ok_or(())?;
        bytes.push((h << 4 | l) as u8);
    }
    Ok(decode_packet(&bytes)?.data)
}
fn set_value(v: &[u8]) -> Result<Vec<u8>, ()> {
    if v.first() == Some(&b'#') {
        decode_encoded_string(&v[1..])
    } else {
        decode_string(v)
    }
}
fn blank(v: &[u8]) -> bool {
    runes(v).iter().all(|c| c.is_whitespace())
}
pub(crate) fn parse_dn(s: &[u8]) -> Result<Dn, ()> {
    let mut dn = Dn::default();
    if blank(s) {
        return Ok(dn);
    }
    let mut rdn: Rdn = Vec::new();
    let mut typ: Vec<u8> = Vec::new();
    let mut escaping = false;
    let mut start = 0;
    for (i, &c) in s.iter().enumerate() {
        if escaping {
            escaping = false;
        } else if c == b'\\' {
            escaping = true;
        } else if c == b'=' && typ.is_empty() {
            typ = decode_string(&s[start..i])?;
            start = i + 1;
        } else if c == b',' || c == b'+' || c == b';' {
            if typ.is_empty() {
                return Err(());
            }
            let value = set_value(&s[start..i])?;
            start = i + 1;
            rdn.push(Atv {
                typ: std::mem::take(&mut typ),
                value,
            });
            if c != b'+' {
                dn.rdns.push(std::mem::take(&mut rdn));
            }
        }
    }
    if typ.is_empty() {
        return Err(());
    }
    let value = set_value(&s[start..])?;
    rdn.push(Atv { typ, value });
    dn.rdns.push(rdn);
    Ok(dn)
}
fn encode_string(v: &[u8], is_value: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len());
    for (i, &c) in v.iter().enumerate() {
        let special = (i == 0 && (c == b' ' || c == b'#'))
            || (i == v.len() - 1 && c == b' ')
            || b"\"+,;<>\\".contains(&c)
            || (!is_value && c == b'=');
        if special {
            out.push(b'\\');
            out.push(c);
        } else if !(b' '..=b'~').contains(&c) {
            out.extend_from_slice(format!("\\{c:02x}").as_bytes());
        } else {
            out.push(c);
        }
    }
    out
}
fn fold_rune(mut r: char) -> char {
    loop {
        let r2 = simple_fold_char(r);
        if r2 <= r {
            return r;
        }
        r = r2;
    }
}
fn fold_string(v: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len());
    for c in runes(v) {
        push_rune(
            &mut out,
            if c.is_ascii() {
                c.to_ascii_lowercase()
            } else {
                fold_rune(c)
            },
        );
    }
    out
}
fn atv_string(a: &Atv) -> Vec<u8> {
    let mut out = encode_string(&fold_string(&a.typ), false);
    out.push(b'=');
    out.extend(encode_string(&a.value, true));
    out
}
pub(crate) fn dn_string(rdns: &[Rdn]) -> Vec<u8> {
    let parts: Vec<Vec<u8>> = rdns
        .iter()
        .map(|rdn| {
            let mut attrs: Vec<Vec<u8>> = rdn.iter().map(atv_string).collect();
            attrs.sort();
            attrs.join(&b'+')
        })
        .collect();
    parts.join(&b',')
}
pub(crate) fn equal_fold(s: &[u8], t: &[u8]) -> bool {
    let (a, b) = (runes(s), runes(t));
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(&x, &y)| {
        if x == y {
            return true;
        }
        let (lo, hi) = if y < x { (y, x) } else { (x, y) };
        if hi.is_ascii() {
            return lo.is_ascii_uppercase() && hi as u32 == lo as u32 + 32;
        }
        let mut r = simple_fold_char(lo);
        while r != lo && r < hi {
            r = simple_fold_char(r);
        }
        r == hi
    })
}
impl Atv {
    fn equal(&self, other: &Atv) -> bool {
        equal_fold(&self.typ, &other.typ) && self.value == other.value
    }
}
fn rdn_equal(a: &Rdn, b: &Rdn) -> bool {
    a.len() == b.len()
        && b.iter().all(|x| a.iter().any(|y| y.equal(x)))
        && a.iter().all(|x| b.iter().any(|y| y.equal(x)))
}
impl Dn {
    pub fn equal(&self, other: &Dn) -> bool {
        self.rdns.len() == other.rdns.len()
            && self
                .rdns
                .iter()
                .zip(&other.rdns)
                .all(|(a, b)| rdn_equal(a, b))
    }
    pub(crate) fn ancestor_of(&self, other: &Dn) -> bool {
        if self.rdns.len() >= other.rdns.len() {
            return false;
        }
        let tail = &other.rdns[other.rdns.len() - self.rdns.len()..];
        self.rdns.iter().zip(tail).all(|(a, b)| rdn_equal(a, b))
    }
}
pub(crate) fn lower_text(v: &[u8]) -> Vec<u8> {
    if v.is_ascii() {
        return v.to_ascii_lowercase();
    }
    let mut out = Vec::with_capacity(v.len());
    for c in runes(v) {
        let lower = if c == '\u{130}' {
            'i'
        } else {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        };
        push_rune(&mut out, lower);
    }
    out
}
fn json_bytes(out: &mut String, v: &[u8]) {
    out.push('"');
    let mut i = 0;
    while i < v.len() {
        let (r, width) = decode_rune(&v[i..]);
        match r {
            None => out.push_str("\\ufffd"),
            Some(c) => {
                let mut one = String::new();
                crate::dicomjson::jsonwire::append_string(&mut one, c.encode_utf8(&mut [0u8; 4]));
                out.push_str(&one[1..one.len() - 1]);
            }
        }
        i += width;
    }
    out.push('"');
}
pub(crate) fn dn_key(value: &[u8]) -> Result<String, ()> {
    let dn = parse_dn(value)?;
    dn_key_of(&dn)
}
fn dn_key_of(dn: &Dn) -> Result<String, ()> {
    if dn.rdns.is_empty() || dn.rdns.len() > 32 {
        return Err(());
    }
    let mut out = String::from("[");
    for (i, rdn) in dn.rdns.iter().enumerate() {
        let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = rdn
            .iter()
            .map(|a| (lower_text(&a.typ), a.value.clone()))
            .collect();
        pairs.sort();
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        for (j, (t, v)) in pairs.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push('[');
            json_bytes(&mut out, t);
            out.push(',');
            json_bytes(&mut out, v);
            out.push(']');
        }
        out.push(']');
    }
    out.push(']');
    Ok(out)
}
pub(crate) fn parent_key(value: &[u8]) -> Result<String, ()> {
    let dn = parse_dn(value)?;
    if dn.rdns.len() < 2 {
        return Err(());
    }
    dn_key(&dn_string(&dn.rdns[1..]))
}
