#![forbid(unsafe_code)]
mod codec;
mod error;
mod iso2022;
#[cfg_attr(not(feature = "render"), allow(dead_code))]
mod japanese;
mod lookup;
mod replacing;
#[rustfmt::skip]
mod tables;
pub use codec::Codec;
pub use error::CharsetError;
use iso2022::Iso2022CharacterSet;
#[cfg(feature = "render")]
pub(crate) use japanese::{decode_euc_jp, decode_shift_jis};
pub use replacing::{REPLACEMENT_CHARACTER, read_default_repertoire_replacing};
#[derive(Debug, Clone, PartialEq)]
pub struct SpecificCharacterSet {
    codecs: Vec<Codec>,
    iso2022: Option<Iso2022CharacterSet>,
}
impl Default for SpecificCharacterSet {
    fn default() -> Self {
        Self::default_repertoire()
    }
}
impl SpecificCharacterSet {
    pub(crate) fn default_repertoire() -> Self {
        SpecificCharacterSet {
            codecs: vec![Codec::Default],
            iso2022: None,
        }
    }
    pub fn iso_ir_100() -> Self {
        SpecificCharacterSet {
            codecs: vec![Codec::Latin1],
            iso2022: None,
        }
    }
    pub fn parse<S: AsRef<str>>(codes: &[S]) -> Result<Self, CharsetError> {
        let normalized = normalize_declaration(codes)?;
        if normalized.is_empty() {
            return Ok(Self::default_repertoire());
        }
        let codecs = normalized
            .iter()
            .map(|code| Codec::for_code(code))
            .collect::<Result<Vec<_>, _>>()?;
        let iso2022 = Iso2022CharacterSet::build(&normalized, &codecs)?;
        Ok(SpecificCharacterSet { codecs, iso2022 })
    }
    pub fn from_code(code: &str) -> Result<Self, CharsetError> {
        Self::parse(&[code])
    }
    #[doc(hidden)]
    pub fn uses_code_extensions(&self) -> bool {
        self.iso2022.is_some()
    }
    pub fn names(&self) -> Vec<String> {
        self.configured_codecs()
            .iter()
            .map(|c| c.name().to_owned())
            .collect()
    }
    pub fn name(&self) -> String {
        self.names().join("\\")
    }
    pub fn decode(&self, text: &[u8]) -> Result<String, CharsetError> {
        match &self.iso2022 {
            Some(s) => s.decode(text, false, true),
            None => self.value_codec().decode(text),
        }
    }
    pub fn encode(&self, text: &str) -> Result<Vec<u8>, CharsetError> {
        match &self.iso2022 {
            Some(s) => s.encode(text, false, true),
            None => self.value_codec().encode(text),
        }
    }
    pub fn decode_single_value(&self, text: &[u8]) -> Result<String, CharsetError> {
        match &self.iso2022 {
            Some(s) => s.decode(text, false, false),
            None => self.value_codec().decode_single_value(text),
        }
    }
    pub fn encode_single_value(&self, text: &str) -> Result<Vec<u8>, CharsetError> {
        match &self.iso2022 {
            Some(s) => s.encode(text, false, false),
            None => self.value_codec().encode_single_value(text),
        }
    }
    pub fn decode_person_name(&self, text: &[u8]) -> Result<String, CharsetError> {
        if let Some(s) = &self.iso2022 {
            return s.decode(text, true, true);
        }
        let mut decoded = Vec::new();
        for (i, group) in text.split(|&b| b == b'=').enumerate() {
            let codec = self.person_name_codec(i);
            let value = codec
                .decode(group)
                .map_err(|e| CharsetError::PersonNameGroup {
                    encode: false,
                    group: i,
                    codec: codec.name().to_owned(),
                    source: Box::new(e),
                })?;
            decoded.push(value);
        }
        Ok(decoded.join("="))
    }
    pub fn encode_person_name(&self, text: &str) -> Result<Vec<u8>, CharsetError> {
        if let Some(s) = &self.iso2022 {
            return s.encode(text, true, true);
        }
        let mut out = Vec::with_capacity(text.len());
        for (i, group) in text.split('=').enumerate() {
            let codec = self.person_name_codec(i);
            let raw = codec
                .encode(group)
                .map_err(|e| CharsetError::PersonNameGroup {
                    encode: true,
                    group: i,
                    codec: codec.name().to_owned(),
                    source: Box::new(e),
                })?;
            if i > 0 {
                out.push(b'=');
            }
            out.extend_from_slice(&raw);
        }
        Ok(out)
    }
    fn configured_codecs(&self) -> &[Codec] {
        if self.codecs.is_empty() {
            std::slice::from_ref(&Codec::Default)
        } else {
            &self.codecs
        }
    }
    fn value_codec(&self) -> &Codec {
        &self.configured_codecs()[0]
    }
    fn person_name_codec(&self, group: usize) -> &Codec {
        let codecs = self.configured_codecs();
        let index = match group {
            0 => 0,
            1 => 1.min(codecs.len() - 1),
            _ => 2.min(codecs.len() - 1),
        };
        &codecs[index]
    }
}
fn normalize_declaration<S: AsRef<str>>(codes: &[S]) -> Result<Vec<String>, CharsetError> {
    if codes.is_empty() {
        return Ok(Vec::new());
    }
    let mut normalized = Vec::with_capacity(codes.len());
    for (index, code) in codes.iter().enumerate() {
        let norm = normalize_code(code.as_ref());
        if index > 0 && norm.is_empty() {
            return Err(CharsetError::InvalidDeclaration(format!(
                "value {} is empty; only Value 1 may be empty",
                index + 1
            )));
        }
        normalized.push(norm);
    }
    if normalized.len() == 1 {
        if normalized[0].is_empty() {
            return Ok(Vec::new());
        }
        if normalized[0].starts_with("ISO 2022 ") {
            return Err(CharsetError::InvalidDeclaration(
                "a Code Extension term requires a multi-valued declaration".into(),
            ));
        }
        return Ok(normalized);
    }
    if normalized[0].is_empty() {
        normalized[0] = "ISO 2022 IR 6".into();
    }
    for (index, code) in normalized.iter().enumerate() {
        if !code.starts_with("ISO 2022 ") {
            return Err(CharsetError::InvalidDeclaration(format!(
                "value {} {} is not a Code Extension term",
                index + 1,
                crate::textfmt::quote(&display_code(code))
            )));
        }
    }
    if !is_iso2022_initial(&normalized[0]) {
        return Err(CharsetError::InvalidDeclaration(format!(
            "value 1 {} is not an allowed initial repertoire",
            crate::textfmt::quote(&display_code(&normalized[0]))
        )));
    }
    Ok(normalized)
}
fn is_iso2022_initial(code: &str) -> bool {
    matches!(
        code,
        "ISO 2022 IR 6"
            | "ISO 2022 IR 13"
            | "ISO 2022 IR 100"
            | "ISO 2022 IR 101"
            | "ISO 2022 IR 109"
            | "ISO 2022 IR 110"
            | "ISO 2022 IR 126"
            | "ISO 2022 IR 127"
            | "ISO 2022 IR 138"
            | "ISO 2022 IR 144"
            | "ISO 2022 IR 148"
            | "ISO 2022 IR 166"
    )
}
fn normalize_code(code: &str) -> String {
    let code = code.trim();
    if code.is_empty() {
        return String::new();
    }
    let code = crate::textfmt::to_upper(&code.replace('_', " "));
    code.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn display_code(code: &str) -> String {
    match code.strip_prefix("ISO IR ") {
        Some(rest) => format!("ISO_IR {rest}"),
        None => code.to_owned(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn cs(codes: &[&str]) -> SpecificCharacterSet {
        SpecificCharacterSet::parse(codes).unwrap()
    }
    #[test]
    fn japanese_person_name_round_trips() {
        let set = cs(&["", "ISO 2022 IR 87"]);
        let raw =
            b"Yamada^Tarou=\x1b$B;3ED\x1b(B^\x1b$BB@O:\x1b(B=\x1b$B$d$^$@\x1b(B^\x1b$B$?$m$&\x1b(B";
        let text = set.decode_person_name(raw).unwrap();
        assert_eq!(text, "Yamada^Tarou=山田^太郎=やまだ^たろう");
        assert_eq!(set.encode_person_name(&text).unwrap(), raw.to_vec());
    }
    #[test]
    fn declarations_are_validated() {
        assert!(SpecificCharacterSet::parse(&["ISO 2022 IR 87"]).is_err());
        assert!(SpecificCharacterSet::parse(&["ISO_IR 100", ""]).is_err());
        assert_eq!(cs(&[" iso_ir  100 "]).name(), "ISO_IR 100");
        assert_eq!(cs(&[] as &[&str]).name(), "ISO_IR 6");
    }
    #[test]
    fn latin1_and_utf8() {
        assert_eq!(cs(&["ISO_IR 100"]).decode(b"M\xfcller").unwrap(), "Müller");
        let err = cs(&["ISO_IR 192"]).decode(b"a\xc3\x28").unwrap_err();
        assert_eq!(
            err.to_string(),
            "dicom: decode ISO_IR 192 at byte 1: invalid byte 0xC3"
        );
    }
}
