use super::lookup::{
    Charmap, charmap_decode, charmap_encode, gb18030_decode_pointer, gb18030_encode_four,
    gbk_decode, gbk_encode,
};
use super::{CharsetError, display_code, tables};
use crate::textfmt::{quote, quote_rune};
#[derive(Debug, Clone, PartialEq)]
pub enum Codec {
    Default,
    Latin1,
    Utf8,
    JisX0201 {
        name: String,
    },
    Charmap {
        name: String,
        map: &'static Charmap,
    },
    Gb {
        name: String,
        gb18030: bool,
    },
    IsoSingleByte {
        name: String,
        base: &'static Charmap,
    },
    CodeExtensionOnly {
        name: String,
    },
}
impl Codec {
    pub(super) fn for_code(code: &str) -> Result<Codec, CharsetError> {
        let name = display_code(code);
        let charmap = |map: &'static Charmap| Codec::Charmap {
            name: name.clone(),
            map,
        };
        Ok(match code {
            "ISO IR 6" | "ISO 2022 IR 6" => Codec::Default,
            "ISO IR 100" | "ISO 2022 IR 100" => Codec::Latin1,
            "ISO IR 13" | "ISO 2022 IR 13" => Codec::JisX0201 { name },
            "ISO IR 192" => Codec::Utf8,
            "ISO IR 148" | "ISO 2022 IR 148" => Codec::IsoSingleByte {
                name,
                base: &tables::WINDOWS_1254,
            },
            "ISO IR 166" | "ISO 2022 IR 166" => Codec::IsoSingleByte {
                name,
                base: &tables::WINDOWS_874,
            },
            "ISO IR 101" | "ISO 2022 IR 101" => charmap(&tables::ISO_8859_2),
            "ISO IR 109" | "ISO 2022 IR 109" => charmap(&tables::ISO_8859_3),
            "ISO IR 110" | "ISO 2022 IR 110" => charmap(&tables::ISO_8859_4),
            "ISO IR 126" | "ISO 2022 IR 126" => charmap(&tables::ISO_8859_7),
            "ISO IR 127" | "ISO 2022 IR 127" => charmap(&tables::ISO_8859_6),
            "ISO IR 138" | "ISO 2022 IR 138" => charmap(&tables::ISO_8859_8),
            "ISO IR 144" | "ISO 2022 IR 144" => charmap(&tables::ISO_8859_5),
            "GBK" | "ISO 2022 IR 58" => Codec::Gb {
                name,
                gb18030: false,
            },
            "GB18030" => Codec::Gb {
                name,
                gb18030: true,
            },
            "ISO 2022 IR 149" | "ISO 2022 IR 159" | "ISO 2022 IR 87" => {
                Codec::CodeExtensionOnly { name }
            }
            _ => return Err(CharsetError::Unsupported(quote(&name))),
        })
    }
    pub fn name(&self) -> &str {
        match self {
            Codec::Default => "ISO_IR 6",
            Codec::Latin1 => "ISO_IR 100",
            Codec::Utf8 => "ISO_IR 192",
            Codec::JisX0201 { name }
            | Codec::Charmap { name, .. }
            | Codec::Gb { name, .. }
            | Codec::IsoSingleByte { name, .. }
            | Codec::CodeExtensionOnly { name } => name,
        }
    }
    pub fn decode(&self, text: &[u8]) -> Result<String, CharsetError> {
        match self {
            Codec::Default => match text.iter().position(|&b| b > 0x7F) {
                Some(offset) => Err(self.decode_error(offset, text[offset])),
                None => Ok(text.iter().map(|&b| b as char).collect()),
            },
            Codec::Latin1 => Ok(text.iter().map(|&b| b as char).collect()),
            Codec::Utf8 => match std::str::from_utf8(text) {
                Ok(s) => Ok(s.to_owned()),
                Err(e) => {
                    let offset = e.valid_up_to();
                    Err(self.decode_error(offset, text[offset]))
                }
            },
            Codec::JisX0201 { .. } => self.decode_jis_x0201(text, true),
            Codec::Charmap { map, .. } => Ok(text
                .iter()
                .map(|&b| charmap_decode(map, b).unwrap_or('\u{FFFD}'))
                .collect()),
            Codec::Gb { gb18030, .. } => {
                let decoded = decode_gb(text, *gb18030);
                if decoded.contains('\u{FFFD}')
                    && let Some(offset) = invalid_gb_byte(text, &decoded, *gb18030)
                {
                    let byte = text.get(offset).copied().unwrap_or(0);
                    return Err(self.decode_error(offset, byte));
                }
                Ok(decoded)
            }
            Codec::IsoSingleByte { base, .. } => {
                let mut out = String::with_capacity(text.len());
                for (offset, &b) in text.iter().enumerate() {
                    if b < 0xA0 {
                        out.push(b as char);
                        continue;
                    }
                    match charmap_decode(base, b) {
                        Some(r) => out.push(r),
                        None => return Err(self.decode_error(offset, b)),
                    }
                }
                Ok(out)
            }
            Codec::CodeExtensionOnly { name } => Err(CharsetError::Other(format!(
                "dicom: {name} is only decodable through ISO 2022 code extensions"
            ))),
        }
    }
    pub fn decode_single_value(&self, text: &[u8]) -> Result<String, CharsetError> {
        match self {
            Codec::JisX0201 { .. } => self.decode_jis_x0201(text, false),
            _ => self.decode(text),
        }
    }
    pub fn encode(&self, text: &str) -> Result<Vec<u8>, CharsetError> {
        let name = self.name();
        let rune_error = |r: char| {
            CharsetError::Unrepresentable(format!("encode {name} rune {}", quote_rune(r)))
        };
        match self {
            Codec::Default => text
                .chars()
                .map(|r| {
                    if u32::from(r) > 0x7F {
                        Err(CharsetError::Unrepresentable(format!(
                            "encode {name} rune {} outside the default repertoire",
                            quote_rune(r)
                        )))
                    } else {
                        Ok(r as u8)
                    }
                })
                .collect(),
            Codec::Latin1 => text
                .chars()
                .map(|r| u8::try_from(u32::from(r)).map_err(|_| rune_error(r)))
                .collect(),
            Codec::Utf8 => Ok(text.as_bytes().to_vec()),
            Codec::JisX0201 { .. } => self.encode_jis_x0201(text, true),
            Codec::Charmap { map, .. } => {
                let x_text_error = || {
                    CharsetError::Unrepresentable(format!(
                        "encode {name}: encoding: rune not supported by encoding."
                    ))
                };
                text.chars()
                    .map(|r| charmap_encode(map, r).ok_or_else(x_text_error))
                    .collect()
            }
            Codec::Gb { gb18030, .. } => {
                let mut out = Vec::with_capacity(text.len());
                for r in text.chars() {
                    if r.is_ascii() {
                        out.push(r as u8);
                    } else if r == '€' && !gb18030 {
                        out.push(0x80);
                    } else if let Some(pair) = gbk_encode(r) {
                        out.extend_from_slice(&pair);
                    } else if let Some(four) = gb18030_encode_four(r).filter(|_| *gb18030) {
                        out.extend_from_slice(&four);
                    } else {
                        return Err(CharsetError::Unrepresentable(format!(
                            "encode {name}: encoding: rune not supported by encoding."
                        )));
                    }
                }
                Ok(out)
            }
            Codec::IsoSingleByte { base, .. } => {
                let mut out = Vec::with_capacity(text.len());
                for r in text.chars() {
                    if u32::from(r) < 0xA0 {
                        out.push(r as u8);
                        continue;
                    }
                    match charmap_encode(base, r) {
                        Some(b) if b >= 0xA0 => out.push(b),
                        _ => return Err(rune_error(r)),
                    }
                }
                Ok(out)
            }
            Codec::CodeExtensionOnly { name } => Err(CharsetError::Other(format!(
                "dicom: {name} is only encodable through ISO 2022 code extensions"
            ))),
        }
    }
    pub fn encode_single_value(&self, text: &str) -> Result<Vec<u8>, CharsetError> {
        match self {
            Codec::JisX0201 { .. } => self.encode_jis_x0201(text, false),
            _ => self.encode(text),
        }
    }
    fn decode_error(&self, offset: usize, byte: u8) -> CharsetError {
        CharsetError::Decode {
            charset: self.name().to_owned(),
            offset,
            byte,
        }
    }
    fn decode_jis_x0201(&self, raw: &[u8], value_delimiter: bool) -> Result<String, CharsetError> {
        let mut out = String::with_capacity(raw.len());
        for (offset, &value) in raw.iter().enumerate() {
            match value {
                0x5C if !value_delimiter => out.push('¥'),
                0x7E => out.push('‾'),
                0x20..=0x7E => out.push(value as char),
                0xA1..=0xDF => out.push(katakana_from_byte(value)),
                _ => return Err(self.decode_error(offset, value)),
            }
        }
        Ok(out)
    }
    fn encode_jis_x0201(&self, text: &str, value_delimiter: bool) -> Result<Vec<u8>, CharsetError> {
        let mut out = Vec::with_capacity(text.len());
        for (offset, value) in text.char_indices() {
            match value {
                '¥' if !value_delimiter => out.push(0x5C),
                '‾' => out.push(0x7E),
                ' '..='}' if value != '\\' => out.push(value as u8),
                _ => match katakana_to_byte(value) {
                    Some(b) => out.push(b),
                    None => {
                        return Err(CharsetError::Unrepresentable(format!(
                            "encode {} rune {} at UTF-8 byte {offset}",
                            self.name(),
                            quote_rune(value)
                        )));
                    }
                },
            }
        }
        Ok(out)
    }
}
pub(super) fn katakana_from_byte(b: u8) -> char {
    char::from_u32(0xFF61 + u32::from(b) - 0xA1).unwrap_or('\u{FFFD}')
}
pub(super) fn katakana_to_byte(r: char) -> Option<u8> {
    let c = u32::from(r);
    if (0xFF61..=0xFF9F).contains(&c) {
        u8::try_from(c - 0xFF61 + 0xA1).ok()
    } else {
        None
    }
}
fn decode_gb(src: &[u8], gb18030: bool) -> String {
    let mut out = String::with_capacity(src.len());
    let mut n = 0;
    while n < src.len() {
        let c0 = src[n];
        let (r, size) = match c0 {
            0x00..=0x7F => (c0 as char, 1),
            0x80 => ('€', 1),
            0xFF => ('\u{FFFD}', 1),
            _ => match src.get(n + 1).copied() {
                None => ('\u{FFFD}', 1),
                Some(c1) if (0x40..0x7F).contains(&c1) || (0x80..0xFF).contains(&c1) => {
                    (gbk_decode(c0, c1).unwrap_or('\u{FFFD}'), 2)
                }
                Some(c1) if gb18030 && (0x30..0x40).contains(&c1) => {
                    match (src.get(n + 2).copied(), src.get(n + 3).copied()) {
                        (Some(c2), Some(c3))
                            if (0x81..0xFF).contains(&c2) && (0x30..0x3A).contains(&c3) =>
                        {
                            let pointer = ((u32::from(c0) - 0x81) * 10 + (u32::from(c1) - 0x30))
                                * 1260
                                + (u32::from(c2) - 0x81) * 10
                                + (u32::from(c3) - 0x30);
                            match gb18030_decode_pointer(pointer) {
                                Some(r) => (r, 4),
                                None => ('\u{FFFD}', 1),
                            }
                        }
                        _ => ('\u{FFFD}', 1),
                    }
                }
                Some(_) => ('\u{FFFD}', 1),
            },
        };
        out.push(r);
        n += size;
    }
    out
}
fn invalid_gb_byte(text: &[u8], decoded: &str, gb18030: bool) -> Option<usize> {
    const ENCODED_REPLACEMENT: &[u8] = b"\x84\x31\xa4\x37";
    let mut offset = 0;
    for r in decoded.chars() {
        if r == '\u{FFFD}' {
            if !gb18030
                || !text
                    .get(offset..)
                    .is_some_and(|t| t.starts_with(ENCODED_REPLACEMENT))
            {
                return Some(offset);
            }
            offset += ENCODED_REPLACEMENT.len();
        } else if text.get(offset).is_some_and(|&b| b <= 0x80) {
            offset += 1;
        } else if gb18030 && text.get(offset + 1).is_some_and(u8::is_ascii_digit) {
            offset += 4;
        } else {
            offset += 2;
        }
    }
    None
}
