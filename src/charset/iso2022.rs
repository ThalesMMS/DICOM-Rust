use super::codec::{Codec, katakana_from_byte, katakana_to_byte};
use super::lookup::{
    eucjp_encode, gbk_decode, gbk_encode, jis0208_decode, jis0212_decode, ksx1001_decode,
    ksx1001_encode,
};
use super::{CharsetError, display_code};
use crate::textfmt::quote_rune;
use std::collections::HashMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Area {
    G0,
    G1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Ascii,
    JisRoman,
    SingleByte,
    JisX0201Katakana,
    JisX0208,
    JisX0212,
    KsX1001,
    Gb2312,
}
#[derive(Debug, Clone, PartialEq)]
struct Repertoire {
    code: &'static str,
    escape: &'static [u8],
    area: Area,
    kind: Kind,
    codec: Option<Codec>,
}
fn template(code: &str) -> Option<Repertoire> {
    use Area::*;
    use Kind::*;
    let (code, escape, area, kind, codec): (&'static str, &'static [u8], _, _, _) = match code {
        "ISO 2022 IR 6" => ("ISO 2022 IR 6", b"\x1b(B", G0, Ascii, Some(Codec::Default)),
        "ISO 2022 IR 14" => ("ISO 2022 IR 14", b"\x1b(J", G0, JisRoman, None),
        "ISO 2022 IR 13" => ("ISO 2022 IR 13", b"\x1b)I", G1, JisX0201Katakana, None),
        "ISO 2022 IR 87" => ("ISO 2022 IR 87", b"\x1b$B", G0, JisX0208, None),
        "ISO 2022 IR 100" => (
            "ISO 2022 IR 100",
            b"\x1b-A",
            G1,
            SingleByte,
            Some(Codec::Latin1),
        ),
        "ISO 2022 IR 101" => ("ISO 2022 IR 101", b"\x1b-B", G1, SingleByte, None),
        "ISO 2022 IR 109" => ("ISO 2022 IR 109", b"\x1b-C", G1, SingleByte, None),
        "ISO 2022 IR 110" => ("ISO 2022 IR 110", b"\x1b-D", G1, SingleByte, None),
        "ISO 2022 IR 126" => ("ISO 2022 IR 126", b"\x1b-F", G1, SingleByte, None),
        "ISO 2022 IR 127" => ("ISO 2022 IR 127", b"\x1b-G", G1, SingleByte, None),
        "ISO 2022 IR 138" => ("ISO 2022 IR 138", b"\x1b-H", G1, SingleByte, None),
        "ISO 2022 IR 144" => ("ISO 2022 IR 144", b"\x1b-L", G1, SingleByte, None),
        "ISO 2022 IR 148" => ("ISO 2022 IR 148", b"\x1b-M", G1, SingleByte, None),
        "ISO 2022 IR 149" => ("ISO 2022 IR 149", b"\x1b$)C", G1, KsX1001, None),
        "ISO 2022 IR 159" => ("ISO 2022 IR 159", b"\x1b$(D", G0, JisX0212, None),
        "ISO 2022 IR 166" => ("ISO 2022 IR 166", b"\x1b-T", G1, SingleByte, None),
        "ISO 2022 IR 58" => ("ISO 2022 IR 58", b"\x1b$)A", G1, Gb2312, None),
        _ => return None,
    };
    Some(Repertoire {
        code,
        escape,
        area,
        kind,
        codec,
    })
}
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Iso2022CharacterSet {
    reps: Vec<Repertoire>,
    initial_g0: usize,
    initial_g1: Option<usize>,
    allowed: Vec<usize>,
}
impl Iso2022CharacterSet {
    pub(super) fn build(codes: &[String], codecs: &[Codec]) -> Result<Option<Self>, CharsetError> {
        if !codes.iter().any(|c| c.starts_with("ISO 2022 ")) {
            return Ok(None);
        }
        let mut seen_codes: HashMap<String, usize> = HashMap::new();
        let mut resolved = Vec::with_capacity(codes.len());
        for (index, code) in codes.iter().enumerate() {
            let extension_code = iso2022_code(code);
            if let Some(previous) = seen_codes.get(&extension_code) {
                return Err(CharsetError::InvalidDeclaration(format!(
                    "value {} duplicates value {} ({})",
                    index + 1,
                    previous + 1,
                    display_code(code)
                )));
            }
            seen_codes.insert(extension_code.clone(), index);
            let Some(mut repertoire) = template(&extension_code) else {
                return Err(CharsetError::Unsupported(format!(
                    "{} is not a supported DICOM ISO 2022 repertoire",
                    crate::textfmt::quote(&display_code(code))
                )));
            };
            if repertoire.kind == Kind::SingleByte && repertoire.codec.is_none() {
                repertoire.codec = Some(codecs[index].clone());
            }
            resolved.push(repertoire);
        }
        let ir6 = template("ISO 2022 IR 6").expect("IR 6 template");
        let mut initial_g0 = ir6;
        let mut initial_g1 = None;
        match iso2022_code(&codes[0]).as_str() {
            "ISO 2022 IR 13" => {
                initial_g0 = template("ISO 2022 IR 14").expect("IR 14 template");
                initial_g1 = Some(resolved[0].clone());
            }
            "ISO 2022 IR 6" => {}
            _ => {
                if resolved[0].area == Area::G1 && resolved[0].kind == Kind::SingleByte {
                    initial_g1 = Some(resolved[0].clone());
                }
            }
        }
        let mut state = Iso2022CharacterSet {
            reps: Vec::new(),
            initial_g0: 0,
            initial_g1: None,
            allowed: Vec::new(),
        };
        state.initial_g0 = state.add(initial_g0);
        state.initial_g1 = initial_g1.map(|r| state.add(r));
        for repertoire in resolved {
            state.add(repertoire);
        }
        Ok(Some(state))
    }
    fn add(&mut self, repertoire: Repertoire) -> usize {
        if let Some(i) = self.reps.iter().position(|r| r.code == repertoire.code) {
            return i;
        }
        self.reps.push(repertoire);
        let index = self.reps.len() - 1;
        self.allowed.push(index);
        index
    }
    fn is_initial_state(&self, g0: usize, g1: Option<usize>) -> bool {
        g0 == self.initial_g0 && (self.initial_g1.is_none() || g1 == self.initial_g1)
    }
    fn escape_at(&self, text: &[u8]) -> Option<(usize, usize)> {
        self.allowed
            .iter()
            .find(|&&i| text.starts_with(self.reps[i].escape))
            .map(|&i| (i, self.reps[i].escape.len()))
    }
    pub(super) fn decode(
        &self,
        text: &[u8],
        person_name: bool,
        value_delimiter: bool,
    ) -> Result<String, CharsetError> {
        let (mut g0, mut g1) = (self.initial_g0, self.initial_g1);
        let mut pn_group = 0;
        let mut out = String::with_capacity(text.len());
        let mut offset = 0;
        while offset < text.len() {
            if text[offset] == 0x1B {
                if person_name && pn_group == 0 {
                    return Err(code_extension_error(
                        offset,
                        bounded(&text[offset..], 4),
                        "code extension is not permitted in the alphabetic PN component group",
                    ));
                }
                let Some((rep, size)) = self.escape_at(&text[offset..]) else {
                    return Err(code_extension_error(
                        offset,
                        bounded(&text[offset..], 4),
                        "unsupported, undeclared, or truncated escape sequence",
                    ));
                };
                if self.reps[rep].area == Area::G0 {
                    g0 = rep;
                } else {
                    g1 = Some(rep);
                }
                offset += size;
                continue;
            }
            let value = text[offset];
            if is_reset_byte(value, person_name, value_delimiter) {
                if !self.is_initial_state(g0, g1) {
                    return Err(code_extension_error(
                        offset,
                        &[value],
                        "initial repertoire was not restored before delimiter or control character",
                    ));
                }
                out.push(value as char);
                (g0, g1) = (self.initial_g0, self.initial_g1);
                if person_name && value == b'=' {
                    pn_group += 1;
                }
                offset += 1;
                continue;
            }
            if value < 0x20 || value == 0x7F || (0x80..0xA0).contains(&value) {
                return Err(code_extension_error(
                    offset,
                    &[value],
                    "control character is not permitted by DICOM text encoding",
                ));
            }
            if value == b' ' {
                out.push(' ');
                offset += 1;
                continue;
            }
            let rep = if value >= 0x80 {
                match g1 {
                    Some(r) => r,
                    None => {
                        return Err(code_extension_error(
                            offset,
                            &[value],
                            "no G1 repertoire is designated",
                        ));
                    }
                }
            } else {
                g0
            };
            let repertoire = &self.reps[rep];
            let size = repertoire.encoded_size(&text[offset..]).map_err(|reason| {
                code_extension_error(offset, bounded(&text[offset..], 2), reason)
            })?;
            let raw = &text[offset..offset + size];
            let decoded = repertoire
                .decode(raw)
                .map_err(|reason| code_extension_error(offset, raw, &reason))?;
            out.push_str(&decoded);
            offset += size;
        }
        if !self.is_initial_state(g0, g1) {
            return Err(code_extension_error(
                text.len(),
                &[],
                "initial repertoire was not restored before the end of the value",
            ));
        }
        Ok(out)
    }
    pub(super) fn encode(
        &self,
        text: &str,
        person_name: bool,
        value_delimiter: bool,
    ) -> Result<Vec<u8>, CharsetError> {
        let (mut g0, mut g1) = (self.initial_g0, self.initial_g1);
        let mut pn_group = 0;
        let mut encoded = Vec::with_capacity(text.len());
        for (offset, value) in text.char_indices() {
            if value.is_ascii() && is_reset_byte(value as u8, person_name, value_delimiter) {
                self.append_initial_designations(&mut encoded, g0, g1);
                encoded.push(value as u8);
                (g0, g1) = (self.initial_g0, self.initial_g1);
                if person_name && value == '=' {
                    pn_group += 1;
                }
                continue;
            }
            if value == ' ' {
                encoded.push(b' ');
                continue;
            }
            if let Some(raw) = self.reps[g0].encode_rune(value) {
                encoded.extend_from_slice(&raw);
                continue;
            }
            if let Some(raw) = g1.and_then(|g| self.reps[g].encode_rune(value)) {
                encoded.extend_from_slice(&raw);
                continue;
            }
            let Some((rep, raw)) = self.find_repertoire(value) else {
                return Err(CharsetError::Unrepresentable(format!(
                    "encode ISO 2022 rune {} at UTF-8 byte {offset}",
                    quote_rune(value)
                )));
            };
            if person_name && pn_group == 0 {
                return Err(CharsetError::Unrepresentable(format!(
                    "code extension for rune {} at UTF-8 byte {offset} is not permitted in the alphabetic PN component group",
                    quote_rune(value)
                )));
            }
            if self.reps[rep].area == Area::G0 {
                if g0 != rep {
                    encoded.extend_from_slice(self.reps[rep].escape);
                    g0 = rep;
                }
            } else if g1 != Some(rep) {
                encoded.extend_from_slice(self.reps[rep].escape);
                g1 = Some(rep);
            }
            encoded.extend_from_slice(&raw);
        }
        self.append_initial_designations(&mut encoded, g0, g1);
        Ok(encoded)
    }
    fn append_initial_designations(&self, encoded: &mut Vec<u8>, g0: usize, g1: Option<usize>) {
        if g0 != self.initial_g0 {
            encoded.extend_from_slice(self.reps[self.initial_g0].escape);
        }
        if let Some(initial) = self.initial_g1
            && g1 != Some(initial)
        {
            encoded.extend_from_slice(self.reps[initial].escape);
        }
    }
    fn find_repertoire(&self, value: char) -> Option<(usize, Vec<u8>)> {
        self.allowed
            .iter()
            .find_map(|&i| self.reps[i].encode_rune(value).map(|raw| (i, raw)))
    }
}
impl Repertoire {
    fn encoded_size(&self, text: &[u8]) -> Result<usize, &'static str> {
        let Some(&value) = text.first() else {
            return Err("truncated encoded character");
        };
        match self.kind {
            Kind::Ascii | Kind::JisRoman => {
                if !(0x20..=0x7E).contains(&value) {
                    return Err("byte is outside the G0 94-character area");
                }
                Ok(1)
            }
            Kind::SingleByte => {
                if value < 0xA0 {
                    return Err("byte is outside the G1 96-character area");
                }
                Ok(1)
            }
            Kind::JisX0201Katakana => {
                if !(0xA1..=0xDF).contains(&value) {
                    return Err("byte is outside the JIS X 0201 Katakana area");
                }
                Ok(1)
            }
            Kind::JisX0208 | Kind::JisX0212 => {
                if text.len() < 2 {
                    return Err("truncated two-byte G0 character");
                }
                if !(0x21..=0x7E).contains(&text[0]) || !(0x21..=0x7E).contains(&text[1]) {
                    return Err("bytes are outside the two-byte G0 94-character area");
                }
                Ok(2)
            }
            Kind::KsX1001 | Kind::Gb2312 => {
                if text.len() < 2 {
                    return Err("truncated two-byte G1 character");
                }
                if !(0xA1..=0xFE).contains(&text[0]) || !(0xA1..=0xFE).contains(&text[1]) {
                    return Err("bytes are outside the two-byte G1 94-character area");
                }
                if self.kind == Kind::Gb2312 && text[0] > 0xF7 {
                    return Err("bytes are outside the GB 2312 repertoire");
                }
                Ok(2)
            }
        }
    }
    fn decode(&self, raw: &[u8]) -> Result<String, String> {
        const UNDEFINED: &str = "undefined encoded character";
        let one = |c: Option<char>| c.map(String::from).ok_or_else(|| UNDEFINED.to_owned());
        match self.kind {
            Kind::Ascii => Ok(raw.iter().map(|&b| b as char).collect()),
            Kind::JisRoman => match raw {
                [0x5C] => Ok("¥".into()),
                [0x7E] => Ok("‾".into()),
                [b] => Ok((*b as char).to_string()),
                _ => Err("invalid JIS Roman character width".into()),
            },
            Kind::JisX0201Katakana => Ok(katakana_from_byte(raw[0]).to_string()),
            Kind::JisX0208 => one(jis0208_decode(raw[0] | 0x80, raw[1] | 0x80)),
            Kind::JisX0212 => one(jis0212_decode(raw[0] | 0x80, raw[1] | 0x80)),
            Kind::KsX1001 => one(ksx1001_decode(raw[0], raw[1])),
            Kind::Gb2312 => decode_gb2312(raw),
            Kind::SingleByte => {
                let Some(codec) = &self.codec else {
                    return Err("missing repertoire codec".into());
                };
                let decoded = codec.decode(raw).map_err(|e| e.to_string())?;
                if decoded.contains('\u{FFFD}') {
                    return Err(UNDEFINED.into());
                }
                Ok(decoded)
            }
        }
    }
    fn encode_rune(&self, value: char) -> Option<Vec<u8>> {
        let c = value as u32;
        match self.kind {
            Kind::Ascii => (0x20..=0x7E).contains(&c).then(|| vec![c as u8]),
            Kind::JisRoman => match value {
                '¥' => Some(vec![0x5C]),
                '‾' => Some(vec![0x7E]),
                _ => ((0x20..=0x7D).contains(&c) && c != 0x5C).then(|| vec![c as u8]),
            },
            Kind::JisX0201Katakana => katakana_to_byte(value).map(|b| vec![b]),
            Kind::JisX0208 => match eucjp_encode(value)? {
                Ok([a, b]) if in_94_byte_area(a) && in_94_byte_area(b) => {
                    Some(vec![a & 0x7F, b & 0x7F])
                }
                _ => None,
            },
            Kind::JisX0212 => match eucjp_encode(value)? {
                Err([a, b]) if in_94_byte_area(a) && in_94_byte_area(b) => {
                    Some(vec![a & 0x7F, b & 0x7F])
                }
                _ => None,
            },
            Kind::KsX1001 => ksx1001_encode(value).map(Vec::from),
            Kind::Gb2312 => encode_gb2312_rune(value),
            Kind::SingleByte => {
                let raw = self
                    .codec
                    .as_ref()?
                    .encode(value.encode_utf8(&mut [0; 4]))
                    .ok()?;
                (raw.len() == 1 && raw[0] >= 0xA0).then_some(raw)
            }
        }
    }
}
fn decode_gb2312(raw: &[u8]) -> Result<String, String> {
    let [first, second] = raw else {
        return Err("invalid GB 2312 character width".into());
    };
    if !is_gb2312_cell(*first, *second) {
        return Err("undefined GB 2312 character".into());
    }
    match (*first, *second) {
        (0xA1, 0xA4) => return Ok("\u{30FB}".into()),
        (0xA1, 0xAA) => return Ok("\u{2015}".into()),
        _ => {}
    }
    gbk_decode(*first, *second)
        .map(String::from)
        .ok_or_else(|| "undefined encoded character".into())
}
fn encode_gb2312_rune(value: char) -> Option<Vec<u8>> {
    match value {
        '\u{30FB}' => return Some(vec![0xA1, 0xA4]),
        '\u{2015}' => return Some(vec![0xA1, 0xAA]),
        '\u{00B7}' | '\u{2014}' => return None,
        _ => {}
    }
    let raw = gbk_encode(value)?;
    let [first, second] = raw;
    if !(0xA1..=0xF7).contains(&first) || !(0xA1..=0xFE).contains(&second) {
        return None;
    }
    is_gb2312_cell(first, second).then(|| raw.to_vec())
}
fn is_gb2312_cell(first: u8, second: u8) -> bool {
    if !(0xA1..=0xF7).contains(&first) || !(0xA1..=0xFE).contains(&second) {
        return false;
    }
    let (row, column) = (first - 0xA0, second - 0xA0);
    match row {
        1 | 3 => true,
        2 => {
            (17..=66).contains(&column)
                || (69..=78).contains(&column)
                || (81..=92).contains(&column)
        }
        4 => column <= 83,
        5 => column <= 86,
        6 => column <= 24 || (33..=56).contains(&column),
        7 => column <= 33 || (49..=81).contains(&column),
        8 => column <= 26 || (37..=73).contains(&column),
        9 => (4..=79).contains(&column),
        16..=54 => true,
        55 => column <= 89,
        _ => (56..=87).contains(&row),
    }
}
fn in_94_byte_area(value: u8) -> bool {
    (0xA1..=0xFE).contains(&value)
}
fn is_reset_byte(value: u8, person_name: bool, value_delimiter: bool) -> bool {
    match value {
        b'\\' => value_delimiter,
        b'\t' | b'\n' | 0x0C | b'\r' => true,
        b'^' | b'=' => person_name,
        _ => false,
    }
}
fn iso2022_code(code: &str) -> String {
    match code.strip_prefix("ISO IR ") {
        Some(rest) => format!("ISO 2022 IR {rest}"),
        None => code.to_owned(),
    }
}
fn bounded(sequence: &[u8], maximum: usize) -> &[u8] {
    &sequence[..sequence.len().min(maximum)]
}
fn code_extension_error(offset: usize, sequence: &[u8], reason: &str) -> CharsetError {
    CharsetError::CodeExtension {
        offset,
        sequence: sequence.to_vec(),
        reason: reason.to_owned(),
    }
}
