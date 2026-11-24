use super::{CharsetError, Codec, SpecificCharacterSet};
pub const REPLACEMENT_CHARACTER: char = '\u{FFFD}';
impl SpecificCharacterSet {
    pub fn decode_replacing(&self, text: &[u8]) -> (String, bool) {
        if let Ok(decoded) = self.decode(text) {
            return (decoded, false);
        }
        if self.iso2022.is_some() {
            return read_default_repertoire_replacing(text);
        }
        let codec = self.value_codec();
        decode_replacing_with(|t| codec.decode(t), text)
    }
    pub fn decode_single_value_replacing(&self, text: &[u8]) -> (String, bool) {
        if let Ok(decoded) = self.decode_single_value(text) {
            return (decoded, false);
        }
        if self.iso2022.is_some() {
            return read_default_repertoire_replacing(text);
        }
        let codec = self.value_codec();
        decode_replacing_with(|t| codec.decode_single_value(t), text)
    }
    pub fn decode_person_name_replacing(&self, text: &[u8]) -> (String, bool) {
        if let Ok(decoded) = self.decode_person_name(text) {
            return (decoded, false);
        }
        if self.iso2022.is_some() {
            return read_default_repertoire_replacing(text);
        }
        let mut replaced = false;
        let groups: Vec<String> = text
            .split(|&b| b == b'=')
            .enumerate()
            .map(|(i, group)| {
                let codec: &Codec = self.person_name_codec(i);
                let (decoded, group_replaced) = decode_replacing_with(|t| codec.decode(t), group);
                replaced |= group_replaced;
                decoded
            })
            .collect();
        (groups.join("="), replaced)
    }
}
fn decode_replacing_with<F>(decode: F, text: &[u8]) -> (String, bool)
where
    F: Fn(&[u8]) -> Result<String, CharsetError>,
{
    let mut out = String::with_capacity(text.len());
    let (mut replaced, mut last_replaced) = (false, false);
    let mut rest = text;
    while !rest.is_empty() {
        let offset = match decode(rest) {
            Ok(decoded) => {
                out.push_str(&decoded);
                break;
            }
            Err(e) => match e.decode_offset() {
                Some(offset) if offset < rest.len() => offset,
                _ => return read_default_repertoire_replacing(text),
            },
        };
        let Ok(prefix) = decode(&rest[..offset]) else {
            return read_default_repertoire_replacing(text);
        };
        out.push_str(&prefix);
        if !prefix.is_empty() || !last_replaced {
            out.push(REPLACEMENT_CHARACTER);
        }
        (replaced, last_replaced) = (true, true);
        rest = &rest[offset + 1..];
    }
    (out, replaced)
}
pub fn read_default_repertoire_replacing(text: &[u8]) -> (String, bool) {
    let mut out = String::with_capacity(text.len());
    let (mut replaced, mut in_run, mut g0_text) = (false, false, true);
    let mut offset = 0;
    while offset < text.len() {
        let value = text[offset];
        if value == 0x1B {
            let size = escape_sequence_size(&text[offset..]);
            let sequence = &text[offset + 1..offset + size];
            if sequence == b"(B" || sequence == b"(J" {
                g0_text = true;
            } else if sequence.starts_with(b"(")
                || sequence == b"$@"
                || sequence == b"$B"
                || sequence.starts_with(b"$(")
            {
                g0_text = false;
            }
            offset += size;
            continue;
        }
        if g0_text
            && ((0x20..=0x7E).contains(&value) || matches!(value, b'\t' | b'\n' | 0x0C | b'\r'))
        {
            out.push(value as char);
            in_run = false;
        } else {
            if !in_run {
                out.push(REPLACEMENT_CHARACTER);
            }
            (replaced, in_run) = (true, true);
        }
        offset += 1;
    }
    (out, replaced)
}
fn escape_sequence_size(text: &[u8]) -> usize {
    let mut size = 1;
    while size < text.len() && (0x20..=0x2F).contains(&text[size]) {
        size += 1;
    }
    if size < text.len() && (0x30..=0x7E).contains(&text[size]) {
        size += 1;
    }
    size
}
