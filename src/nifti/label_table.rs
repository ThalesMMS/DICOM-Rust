use crate::fmath;
use crate::numtext;
use crate::object::error::quote_bytes;
use crate::textfmt::decode_rune;
use std::collections::BTreeMap;
pub const ERR_LABEL_TABLE: &str = "dicom/nifti: malformed label table";
pub const MAX_LABEL_TABLE_BYTES: usize = 4 << 20;
const MAX_LABEL_VALUE: i64 = 65535;
const MAX_LINE_BYTES: usize = 64 << 10;
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LabelEntry {
    pub name: Vec<u8>,
    pub rgb: [u8; 3],
}
pub type LabelTable = BTreeMap<i64, LabelEntry>;
fn is_space(byte_rune: Option<char>) -> bool {
    byte_rune.is_some_and(char::is_whitespace)
}
pub(crate) fn trim_space(mut s: &[u8]) -> &[u8] {
    while !s.is_empty() {
        let (rune, size) = decode_rune(s);
        if !is_space(rune) {
            break;
        }
        s = &s[size..];
    }
    'outer: while !s.is_empty() {
        for size in 1..=4.min(s.len()) {
            let tail = &s[s.len() - size..];
            if let Ok(text) = std::str::from_utf8(tail) {
                let mut chars = text.chars();
                if let (Some(c), None) = (chars.next(), chars.next())
                    && c.is_whitespace()
                {
                    s = &s[..s.len() - size];
                    continue 'outer;
                }
                break 'outer;
            }
        }
        break;
    }
    s
}
pub(crate) fn fields(s: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut at = 0;
    while at < s.len() {
        let (rune, size) = decode_rune(&s[at..]);
        if is_space(rune) {
            if let Some(begin) = start.take() {
                out.push(&s[begin..at]);
            }
        } else if start.is_none() {
            start = Some(at);
        }
        at += size.max(1);
    }
    if let Some(begin) = start {
        out.push(&s[begin..]);
    }
    out
}
pub fn read_label_table(data: &[u8]) -> Result<LabelTable, crate::MessageError> {
    read_label_table_text(data).map_err(crate::MessageError::from)
}
fn read_label_table_text(data: &[u8]) -> Result<LabelTable, String> {
    let too_large = || format!("{ERR_LABEL_TABLE}: larger than {MAX_LABEL_TABLE_BYTES} bytes");
    let data = &data[..data.len().min(MAX_LABEL_TABLE_BYTES + 1)];
    let mut table = LabelTable::new();
    let mut read = 0usize;
    let mut rest = data;
    let mut line = 1usize;
    while !rest.is_empty() {
        let (mut text, next) = match rest.iter().position(|&b| b == b'\n') {
            Some(newline) => (&rest[..newline], &rest[newline + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        if text.len() >= MAX_LINE_BYTES {
            return Err(format!("{ERR_LABEL_TABLE}: line too long"));
        }
        rest = next;
        if text.last() == Some(&b'\r') {
            text = &text[..text.len() - 1];
        }
        read += text.len() + 1;
        if read > MAX_LABEL_TABLE_BYTES + 1 {
            return Err(too_large());
        }
        let text = trim_space(text);
        if !(text.is_empty() || text[0] == b'#') {
            let (value, entry) = parse_label_line(text)
                .map_err(|err| format!("{ERR_LABEL_TABLE}: line {line}: {err}"))?;
            if value != 0 {
                table.insert(value, entry);
            }
        }
        line += 1;
    }
    if read > MAX_LABEL_TABLE_BYTES {
        return Err(too_large());
    }
    Ok(table)
}
fn parse_label_line(text: &[u8]) -> Result<(i64, LabelEntry), String> {
    if let Some(quote) = text.iter().position(|&b| b == b'"') {
        let end = text.iter().rposition(|&b| b == b'"').unwrap_or(quote);
        if end <= quote {
            return Err("unterminated label name".to_string());
        }
        let parts = fields(&text[..quote]);
        if parts.len() < 4 {
            return Err("expected value and R G B before the name".to_string());
        }
        let (value, rgb) = parse_value_rgb(&parts[..4])?;
        let name = trim_space(&text[quote + 1..end]).to_vec();
        return Ok((value, LabelEntry { name, rgb }));
    }
    let parts = fields(text);
    if parts.len() < 5 {
        return Err("expected value, name and R G B".to_string());
    }
    let (value, rgb) = parse_value_rgb(&[parts[0], parts[2], parts[3], parts[4]])?;
    let name = parts[1]
        .iter()
        .map(|&b| if b == b'_' { b' ' } else { b })
        .collect();
    Ok((value, LabelEntry { name, rgb }))
}
fn parse_value_rgb(parts: &[&[u8]]) -> Result<(i64, [u8; 3]), String> {
    let value = std::str::from_utf8(parts[0])
        .ok()
        .and_then(|text| numtext::parse_int(text).ok())
        .filter(|value| (0..=MAX_LABEL_VALUE).contains(value))
        .ok_or_else(|| {
            format!(
                "label value {} is not in 0..{MAX_LABEL_VALUE}",
                quote_bytes(parts[0])
            )
        })?;
    let mut rgb = [0u8; 3];
    for (slot, field) in rgb.iter_mut().zip(&parts[1..4]) {
        let component = std::str::from_utf8(field)
            .ok()
            .and_then(|text| numtext::parse_float(text).ok())
            .filter(|c| !c.is_nan() && *c >= 0.0 && *c <= 255.0)
            .ok_or_else(|| format!("colour component {} is not in 0..255", quote_bytes(field)))?;
        *slot = fmath::f64_to_u8(component.round());
    }
    Ok((value, rgb))
}
