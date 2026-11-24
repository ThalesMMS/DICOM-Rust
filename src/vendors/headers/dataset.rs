use super::{Budget, private_sequence, private_tag_required, private_value, suffixed};
use crate::object::inflate::inflate_raw;
use crate::object::parser::source::MemFile;
use crate::object::parser::{Reader, ReaderOptions, TokenKind};
use crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
use crate::object::{
    self, Element, Object, ReadFileOptions, Syntax, Value, read_data_set_with_options,
    read_part10_header_mem,
};
use crate::vendors::{Error, Kind, Limits};
use std::rc::Rc;
const PARSER_LIMITS: &[&str] = &[
    "dicom: maximum sequence nesting depth exceeded",
    "dicom: maximum element count exceeded",
    "dicom: maximum element value size exceeded",
    "dicom: maximum fragment count exceeded",
];
fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn data_set_error(err: object::Error) -> Error {
    let text = err.to_string();
    if err.is(object::Kind::MaxTotalBytes) || PARSER_LIMITS.iter().any(|l| text.contains(l)) {
        return Error::new(Kind::Limit, "embedded dataset");
    }
    Error::new(Kind::Invalid, format!("embedded dataset: {text}"))
}
fn read_options(l: Limits) -> ReadFileOptions {
    ReadFileOptions {
        max_element_bytes: l.max_bytes,
        max_total_bytes: l.max_bytes,
        max_elements: l.max_entries,
        max_fragments: l.max_entries,
        max_sequence_depth: 128,
        ..ReadFileOptions::default()
    }
}
fn mem(raw: &[u8]) -> MemFile {
    MemFile::new(Rc::from(raw))
}
pub fn decode_toshiba_ct3(raw: &[u8], limits: Limits) -> Result<Object, Error> {
    let limits = limits.normalized();
    if raw.len() as i64 > limits.max_bytes {
        return Err(Error::bare(Kind::Limit));
    }
    if raw.len() < 24 {
        return Err(Error::bare(Kind::Invalid));
    }
    for (i, element) in [0u16, 1].into_iter().enumerate() {
        let p = &raw[i * 12..];
        if le16(p) != 0 || le16(&p[2..]) != element || le32(&p[4..]) != 4 {
            return Err(Error::new(Kind::Invalid, "CT3 wrapper"));
        }
    }
    if le32(&raw[8..]) != 12 || u64::from(le32(&raw[20..])) != (raw.len() - 24) as u64 {
        return Err(Error::new(Kind::Invalid, "CT3 length"));
    }
    decode_data_set(&raw[24..], limits)
}
pub fn toshiba_ct3_of(o: &Object, l: Limits) -> Result<Object, Error> {
    let l = l.normalized();
    let raw = private_value(o, 0x7005, "TOSHIBA_MEC_CT3", 0x10, l)?;
    decode_toshiba_ct3(&raw, l)
}
fn decode_data_set(raw: &[u8], mut l: Limits) -> Result<Object, Error> {
    let mut syntax = EXPLICIT_VR_LITTLE_ENDIAN;
    let owned: Vec<u8>;
    let mut raw = raw;
    if raw.len() >= 132 && &raw[128..132] == b"DICM" {
        let meta_entries = preflight_file_meta(raw, l)?;
        let header = read_part10_header_mem(&mem(raw), &read_options(l)).map_err(data_set_error)?;
        if header.data_set_offset < 0 || header.data_set_offset > raw.len() as i64 {
            return Err(Error::bare(Kind::Invalid));
        }
        syntax = header.transfer_syntax;
        raw = &raw[header.data_set_offset as usize..];
        l.max_entries -= meta_entries;
        if l.max_entries <= 0 {
            return Err(Error::bare(Kind::Limit));
        }
        if syntax.deflated {
            let limit = l.max_decoded_bytes.min(l.max_bytes);
            let (decoded, err, consumed) = inflate_raw(raw);
            if decoded.len() as i64 > limit {
                return Err(Error::bare(Kind::Limit));
            }
            if !err.is_exactly(object::Kind::Eof) {
                return Err(Error::new(Kind::Invalid, "embedded deflate"));
            }
            let remaining = raw.len() - consumed;
            if remaining != 0 && (remaining != 1 || consumed % 2 != 1 || raw[consumed] != 0) {
                return Err(Error::new(Kind::Invalid, "embedded deflate trailing bytes"));
            }
            owned = decoded;
            raw = &owned;
            syntax = EXPLICIT_VR_LITTLE_ENDIAN;
        }
    }
    if raw.len() as i64 > l.max_bytes {
        return Err(Error::bare(Kind::Limit));
    }
    preflight_tokens(raw, syntax, l, false)?;
    read_data_set_with_options(Box::new(mem(raw)), syntax, read_options(l)).map_err(data_set_error)
}
pub fn decode_pmtf(raw: &[u8], l: Limits) -> Result<Object, Error> {
    let l = l.normalized();
    if raw.len() as i64 > l.max_bytes {
        return Err(Error::bare(Kind::Limit));
    }
    if raw.is_empty() || !raw.len().is_multiple_of(2) {
        return Err(Error::bare(Kind::Invalid));
    }
    let rev: Vec<u8> = raw.iter().rev().copied().collect();
    preflight_tokens(&rev, EXPLICIT_VR_LITTLE_ENDIAN, l, false)?;
    read_data_set_with_options(
        Box::new(mem(&rev)),
        EXPLICIT_VR_LITTLE_ENDIAN,
        read_options(l),
    )
    .map_err(data_set_error)
}
pub fn pmtf_of(o: &Object, creator: &str, offset: u8, l: Limits) -> Result<Vec<Object>, Error> {
    if !matches!(
        creator,
        "PMTF INFORMATION DATA" | "CANON_MEC_MR3" | "TOSHIBA_MEC_MR3"
    ) {
        return Err(Error::bare(Kind::UnsupportedHeader));
    }
    if offset != 1 && (creator != "TOSHIBA_MEC_MR3" || offset != 2) {
        return Err(Error::bare(Kind::UnsupportedHeader));
    }
    let tag = private_tag_required(o, 0x0029, creator, offset)?;
    let mut b = Budget::new(l);
    let items = private_sequence(o, tag, &mut b)?;
    if items.is_empty() {
        return Err(Error::bare(Kind::Invalid));
    }
    let mut out = Vec::new();
    for item in &items {
        let raw = private_value(item, 0x0029, creator, 0x90, b.limits)
            .map_err(|e| suffixed(e, "PMTF item"))?;
        let mut remaining = b.limits;
        remaining.max_entries -= b.entries;
        if remaining.max_entries <= 0 {
            return Err(Error::bare(Kind::Limit));
        }
        let parsed = decode_pmtf(&raw, remaining)?;
        charge_elements(parsed.elements(), 0, &mut b)?;
        out.push(parsed);
    }
    Ok(out)
}
fn charge_elements(els: &[Element], depth: usize, b: &mut Budget) -> Result<(), Error> {
    if depth >= 64 {
        return Err(Error::bare(Kind::Limit));
    }
    b.take(els.len() as i64, 0, 0)?;
    for el in els {
        if let Value::Sequence(items) = &el.value {
            b.take(items.len() as i64, 0, 0)?;
            for item in items {
                charge_elements(&item.elements, depth + 1, b)?;
            }
        }
    }
    Ok(())
}
fn byte_list_text(b: &[u8]) -> String {
    let parts: Vec<String> = b.iter().map(u8::to_string).collect();
    format!("[{}]", parts.join(" "))
}
fn value_repr(value: &Value) -> String {
    match value {
        Value::Fragments {
            offset_table,
            fragments,
        } => {
            let parts: Vec<String> = fragments.iter().map(|f| byte_list_text(f)).collect();
            format!("{{{} [{}]}}", byte_list_text(offset_table), parts.join(" "))
        }
        Value::Strings(values) => format!("[{}]", values.join(" ")),
        Value::BulkData(uri) => format!("{{{uri}}}"),
        _ => "<nil>".to_string(),
    }
}
pub(super) fn object_lines(
    o: &Object,
    b: &mut Budget,
    prefix: &str,
    depth: usize,
    lines: &mut Vec<String>,
) -> Result<(), Error> {
    if depth >= 64 {
        return Err(Error::bare(Kind::Limit));
    }
    for el in o.elements() {
        b.take(1, 0, 0)?;
        let p = format!("{prefix}{}", el.tag());
        if let Value::Sequence(items) = &el.value {
            b.take(items.len() as i64, 0, 0)?;
            for (i, item) in o.get_sequence(el.tag()).iter().flatten().enumerate() {
                object_lines(item, b, &format!("{p}[{i}]."), depth + 1, lines)?;
            }
            continue;
        }
        let value = if el.vr().is_string_like() {
            match o.lookup_string(el.tag()) {
                Ok(s) => s,
                Err(_) => return Err(Error::new(Kind::Invalid, "embedded text")),
            }
        } else if let Value::Raw(raw) = &el.value {
            let n = raw.len().min(32);
            let mut hex: String = raw[..n].iter().map(|v| format!("{v:02x}")).collect();
            if n < raw.len() {
                hex.push_str("...");
            }
            hex
        } else {
            value_repr(&el.value)
        };
        b.line(lines, format!("{p} {} [{value}]", el.vr()))?;
    }
    Ok(())
}
fn preflight_file_meta(raw: &[u8], l: Limits) -> Result<i64, Error> {
    if raw.len() < 144
        || le16(&raw[132..]) != 2
        || le16(&raw[134..]) != 0
        || &raw[136..138] != b"UL"
        || le16(&raw[138..]) != 4
    {
        return Err(Error::new(Kind::Invalid, "embedded File Meta group length"));
    }
    let end = 144 + u64::from(le32(&raw[140..]));
    if end > raw.len() as u64 {
        return Err(Error::new(Kind::Invalid, "embedded File Meta length"));
    }
    preflight_tokens(&raw[132..end as usize], EXPLICIT_VR_LITTLE_ENDIAN, l, true)
}
fn preflight_tokens(raw: &[u8], syntax: Syntax, l: Limits, meta_only: bool) -> Result<i64, Error> {
    let opts = ReaderOptions {
        max_element_bytes: l.max_bytes,
        max_total_bytes: l.max_bytes,
        max_sequence_depth: 128,
        max_elements: l.max_entries,
        max_fragments: l.max_entries,
        ..ReaderOptions::default()
    };
    let mut r = Reader::new(
        Box::new(mem(raw)),
        Some((raw.len() as i64, 0)),
        syntax,
        opts,
    );
    r.set_no_dictionary(true);
    let mut n: i64 = 0;
    loop {
        let token = match r.next() {
            Ok(token) => token,
            Err(err) if err.is_exactly(object::Kind::Eof) => return Ok(n),
            Err(err) => return Err(data_set_error(err)),
        };
        if n >= l.max_entries {
            return Err(Error::bare(Kind::Limit));
        }
        let tag = token.element.as_ref().map_or(token.tag(), Element::tag);
        if meta_only && (token.kind != TokenKind::Element || tag.group != 2) {
            return Err(Error::new(
                Kind::Invalid,
                "embedded File Meta must contain primitive group 0002 elements",
            ));
        }
        n += 1;
    }
}
