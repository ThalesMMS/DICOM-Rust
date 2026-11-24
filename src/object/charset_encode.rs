#![forbid(unsafe_code)]
use super::charset_conversion::{
    CharsetConversionFailure, CharsetConversionOptions, CharsetConversionReport, CharsetDiagnostic,
    CharsetReason, convert_text_to_utf8, pad_to_even, sanitize_terms, set_declaration,
    trim_unicode_space, un_path,
};
use super::dict;
use super::error::Error;
use super::obj::{Object, TAG_SPECIFIC_CHARACTER_SET, decode_text_values_with_default};
use super::text::{split_text_multiplicity, trim_text_value_bytes};
use super::types::{DataSet, Element, Value, Vr};
use crate::charset::{SpecificCharacterSet, read_default_repertoire_replacing};
use std::collections::HashMap;
use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::UnicodeNormalization;
pub const ERR_UNREPRESENTABLE_TEXT: &str =
    "dicom: text cannot be represented in the target character set";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnrepresentablePolicy {
    #[default]
    Fail,
    Transliterate,
    Discard,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodeReason {
    Transliterated,
    Discarded,
    Unrepresentable,
}
impl EncodeReason {
    pub fn as_str(self) -> &'static str {
        match self {
            EncodeReason::Transliterated => "characters-transliterated",
            EncodeReason::Discarded => "characters-discarded",
            EncodeReason::Unrepresentable => "unrepresentable-character",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeDiagnostic {
    pub path: String,
    pub reason: &'static str,
    pub declared: String,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CharsetEncodeOptions {
    pub unrepresentable: UnrepresentablePolicy,
    pub decode: CharsetConversionOptions,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CharsetEncodeReport {
    pub converted_values: i64,
    pub fallback_values: i64,
    pub kept_scopes: i64,
    pub diagnostics: Vec<EncodeDiagnostic>,
    pub diagnostics_truncated: bool,
    pub encoded_values: i64,
    pub transliterated_characters: i64,
    pub discarded_characters: i64,
}
impl CharsetEncodeReport {
    fn from_decode(report: CharsetConversionReport) -> Self {
        let diagnostics = report
            .diagnostics
            .into_iter()
            .map(|d: CharsetDiagnostic| EncodeDiagnostic {
                path: d.path,
                reason: CharsetReason::as_str(d.reason),
                declared: d.declared,
            })
            .collect();
        CharsetEncodeReport {
            converted_values: report.converted_values,
            fallback_values: report.fallback_values,
            kept_scopes: report.kept_scopes,
            diagnostics,
            diagnostics_truncated: report.diagnostics_truncated,
            ..Default::default()
        }
    }
}
#[derive(Clone, Debug)]
pub struct CharsetEncodeFailure {
    pub error: Error,
    pub report: CharsetEncodeReport,
}
impl std::fmt::Display for CharsetEncodeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}
impl std::error::Error for CharsetEncodeFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.error)
    }
}
fn lossy_chars(bytes: &[u8]) -> Vec<char> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.extend(s.chars());
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.extend(std::str::from_utf8(&rest[..valid]).unwrap_or("").chars());
                out.push('\u{FFFD}');
                rest = &rest[valid + 1..];
            }
        }
    }
    out
}
struct Encoder {
    charset: SpecificCharacterSet,
    terms: Vec<String>,
    policy: UnrepresentablePolicy,
    report: CharsetEncodeReport,
    max: i64,
    known: HashMap<char, bool>,
}
impl Encoder {
    fn note(&mut self, path: &str, reason: EncodeReason) {
        if self.report.diagnostics.len() as i64 >= self.max {
            self.report.diagnostics_truncated = true;
            return;
        }
        self.report.diagnostics.push(EncodeDiagnostic {
            path: path.to_string(),
            reason: reason.as_str(),
            declared: sanitize_terms(&self.terms),
        });
    }
    fn data_set(
        &mut self,
        elements: &[Element],
        path: &str,
        inherited_utf8: bool,
        top: bool,
    ) -> Result<Vec<Element>, Error> {
        let mut utf8 = inherited_utf8;
        let mut own = false;
        for element in elements {
            if element.tag() == TAG_SPECIFIC_CHARACTER_SET {
                own = true;
                let values = decode_text_values_with_default(element).unwrap_or_default();
                utf8 = values.len() == 1
                    && trim_unicode_space(&values[0]).eq_ignore_ascii_case("ISO_IR 192");
            }
        }
        let mut out = elements.to_vec();
        for element in out.iter_mut() {
            let element_path = un_path(path, element.tag());
            if let Value::Sequence(items) = &element.value {
                let mut converted_items = Vec::with_capacity(items.len());
                for (j, item) in items.iter().enumerate() {
                    let converted = self.data_set(
                        &item.elements,
                        &format!("{element_path}[{j}]"),
                        utf8,
                        false,
                    )?;
                    converted_items.push(DataSet {
                        elements: converted,
                        item_offset: item.item_offset,
                    });
                }
                element.value = Value::Sequence(converted_items);
                continue;
            }
            let vr = element.vr();
            if !vr.uses_specific_character_set() || dict::is_private_creator(element.tag()) {
                continue;
            }
            let source: Vec<char> = match &element.value {
                Value::Raw(raw) => {
                    if !utf8 {
                        continue;
                    }
                    let end = raw
                        .iter()
                        .rposition(|&b| b != b' ' && b != 0)
                        .map_or(0, |i| i + 1);
                    lossy_chars(&raw[..end])
                }
                Value::Strings(values) => values.join("\\").chars().collect(),
                _ => continue,
            };
            let text = self.text(&source, &element_path)?;
            let text = trim_trailing_spaces(vr, &text);
            let encoded = encode_text_value(vr, &text, &self.charset).map_err(|err| {
                Error::msg(format!("{ERR_UNREPRESENTABLE_TEXT}: {element_path}: {err}"))
            })?;
            let value = pad_to_even(vr, encoded);
            element.header.length = value.len() as u32;
            element.value = Value::Raw(value);
            self.report.encoded_values += 1;
        }
        if utf8 && (own || top) {
            if self.terms.is_empty() {
                out.retain(|e| e.tag() != TAG_SPECIFIC_CHARACTER_SET);
            } else {
                out = set_declaration(out, &self.terms);
            }
        }
        Ok(out)
    }
    fn text(&mut self, value: &[char], path: &str) -> Result<String, Error> {
        let mut b = String::new();
        let (mut transliterated, mut discarded) = (0, 0);
        for &r in value {
            if self.representable(r) {
                b.push(r);
                continue;
            }
            match self.policy {
                UnrepresentablePolicy::Transliterate => {
                    let s = self.approximate(r, 0);
                    b.push_str(&s);
                    transliterated += 1;
                }
                UnrepresentablePolicy::Discard => discarded += 1,
                UnrepresentablePolicy::Fail => {
                    self.note(path, EncodeReason::Unrepresentable);
                    return Err(Error::msg(format!("{ERR_UNREPRESENTABLE_TEXT}: {path}")));
                }
            }
        }
        if transliterated > 0 {
            self.report.transliterated_characters += transliterated;
            self.note(path, EncodeReason::Transliterated);
        }
        if discarded > 0 {
            self.report.discarded_characters += discarded;
            self.note(path, EncodeReason::Discarded);
        }
        Ok(b)
    }
    fn representable(&mut self, r: char) -> bool {
        if (r as u32) < 0x80 {
            return true;
        }
        if let Some(&ok) = self.known.get(&r) {
            return ok;
        }
        let s = r.to_string();
        let ok = match self.charset.encode(&s) {
            Ok(encoded) => self.charset.decode(&encoded).is_ok_and(|d| d == s),
            Err(_) => false,
        };
        self.known.insert(r, ok);
        ok
    }
    fn representable_string(&mut self, s: &str) -> bool {
        for r in s.chars() {
            if !self.representable(r) {
                return false;
            }
        }
        !s.is_empty()
    }
    fn approximate(&mut self, r: char, depth: usize) -> String {
        if let Some(replacement) = transliteration(r)
            && self.representable_string(replacement)
        {
            return replacement.to_string();
        }
        for candidate in preferred_transliterations(r) {
            if self.representable_string(candidate) {
                return candidate.to_string();
            }
        }
        if depth < 3 {
            let mut b = String::new();
            let mut changed = false;
            for part in nfkd15(r) {
                if is_mn15(part) {
                    changed = true;
                    continue;
                }
                if part == r {
                    continue;
                }
                changed = true;
                if self.representable(part) {
                    b.push(part);
                } else {
                    let s = self.approximate(part, depth + 1);
                    b.push_str(&s);
                }
            }
            if changed && !b.is_empty() && !b.contains('?') {
                return b;
            }
        }
        "?".to_string()
    }
}
const UNICODE16_ONLY: [u32; 85] = [
    0x897, 0xA7F1, 0x105C9, 0x105E4, 0x10D69, 0x10D6A, 0x10D6B, 0x10D6C, 0x10D6D, 0x10EFC, 0x11383,
    0x11385, 0x1138E, 0x11391, 0x113BB, 0x113BC, 0x113BD, 0x113BE, 0x113BF, 0x113C0, 0x113C5,
    0x113C7, 0x113C8, 0x113CE, 0x113D0, 0x113D2, 0x113E1, 0x113E2, 0x11F5A, 0x1611E, 0x1611F,
    0x16120, 0x16121, 0x16122, 0x16123, 0x16124, 0x16125, 0x16126, 0x16127, 0x16128, 0x16129,
    0x1612D, 0x1612E, 0x1612F, 0x16D68, 0x16D69, 0x16D6A, 0x1CCD6, 0x1CCD7, 0x1CCD8, 0x1CCD9,
    0x1CCDA, 0x1CCDB, 0x1CCDC, 0x1CCDD, 0x1CCDE, 0x1CCDF, 0x1CCE0, 0x1CCE1, 0x1CCE2, 0x1CCE3,
    0x1CCE4, 0x1CCE5, 0x1CCE6, 0x1CCE7, 0x1CCE8, 0x1CCE9, 0x1CCEA, 0x1CCEB, 0x1CCEC, 0x1CCED,
    0x1CCEE, 0x1CCEF, 0x1CCF0, 0x1CCF1, 0x1CCF2, 0x1CCF3, 0x1CCF4, 0x1CCF5, 0x1CCF6, 0x1CCF7,
    0x1CCF8, 0x1CCF9, 0x1E5EE, 0x1E5EF,
];
fn nfkd15(r: char) -> Vec<char> {
    if UNICODE16_ONLY.contains(&(r as u32)) {
        return vec![r];
    }
    std::iter::once(r).nfkd().collect()
}
fn is_mn15(r: char) -> bool {
    if r == '\u{1171E}' {
        return true;
    }
    !UNICODE16_ONLY.contains(&(r as u32))
        && get_general_category(r) == GeneralCategory::NonspacingMark
}
fn trim_trailing_spaces(vr: Vr, text: &str) -> String {
    if !vr.uses_text_value_delimiter() {
        return text.trim_end_matches(' ').to_string();
    }
    text.split('\\')
        .map(|v| v.trim_end_matches(' '))
        .collect::<Vec<_>>()
        .join("\\")
}
fn encode_text_value(
    vr: Vr,
    text: &str,
    charset: &SpecificCharacterSet,
) -> Result<Vec<u8>, crate::charset::CharsetError> {
    if vr == Vr::PN {
        charset.encode_person_name(text)
    } else if !vr.uses_text_value_delimiter() {
        charset.encode_single_value(text)
    } else {
        charset.encode(text)
    }
}
fn transliteration(r: char) -> Option<&'static str> {
    Some(match r {
        'Æ' => "AE",
        'æ' => "ae",
        'Ø' => "O",
        'ø' => "o",
        'Œ' => "OE",
        'œ' => "oe",
        'ß' => "ss",
        'Ð' => "D",
        'ð' => "d",
        'Đ' => "D",
        'đ' => "d",
        'Þ' => "Th",
        'þ' => "th",
        'Ł' => "L",
        'ł' => "l",
        'Ħ' => "H",
        'ħ' => "h",
        'ı' => "i",
        'Ŋ' => "N",
        'ŋ' => "n",
        'ĸ' => "q",
        'Ŧ' => "T",
        'ŧ' => "t",
        'ƒ' => "f",
        '\u{201B}' => "'",
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => "\"",
        '‹' => "<",
        '›' => ">",
        '«' => "<<",
        '»' => ">>",
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
        | '\u{2212}' => "-",
        '…' => "...",
        '•' => "o",
        '·' => ".",
        '′' => "'",
        '″' => "\"",
        '⁄' => "/",
        '€' => "EUR",
        '£' => "lb",
        '¥' => "yen",
        '¢' => "c",
        '©' => "(c)",
        '®' => "(R)",
        '™' => "TM",
        '°' => "^0",
        '±' => "+/-",
        '×' => "x",
        '÷' => ":",
        '¬' => "!",
        '¦' => "|",
        '§' => "S",
        '¶' => "P",
        '¡' => "!",
        '¿' => "?",
        '¤' => "?",
        'µ' => "u",
        '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2009}' => " ",
        '\u{200B}' => "",
        '①' => "(1)",
        '②' => "(2)",
        '③' => "(3)",
        '④' => "(4)",
        '⑤' => "(5)",
        '⑥' => "(6)",
        '⑦' => "(7)",
        '⑧' => "(8)",
        '⑨' => "(9)",
        '⑩' => "(10)",
        _ => return None,
    })
}
fn preferred_transliterations(r: char) -> &'static [&'static str] {
    match r {
        '\u{2018}' => &["`", "'"],
        '\u{2019}' => &["\u{00B4}", "'"],
        '\u{201A}' => &["\u{2018}", "`", "'"],
        _ => &[],
    }
}
pub fn convert_text_to_character_set(
    o: &Object,
    target: &str,
    opts: CharsetEncodeOptions,
) -> Result<(Object, CharsetEncodeReport), Box<CharsetEncodeFailure>> {
    let terms: Vec<String> = if trim_unicode_space(target).is_empty() {
        Vec::new()
    } else {
        target.split('\\').map(str::to_string).collect()
    };
    let charset = match SpecificCharacterSet::parse(&terms) {
        Ok(charset) => charset,
        Err(err) => {
            return Err(Box::new(CharsetEncodeFailure {
                error: Error::msg(err.to_string()),
                report: CharsetEncodeReport::default(),
            }));
        }
    };
    let (utf8, decode_report) = match convert_text_to_utf8(o, opts.decode) {
        Ok(ok) => ok,
        Err(failure) => {
            let CharsetConversionFailure { error, report } = *failure;
            return Err(Box::new(CharsetEncodeFailure {
                error,
                report: CharsetEncodeReport::from_decode(report),
            }));
        }
    };
    let mut e = Encoder {
        charset,
        terms,
        policy: opts.unrepresentable,
        report: CharsetEncodeReport::from_decode(decode_report),
        max: if opts.decode.max_diagnostics <= 0 {
            256
        } else {
            opts.decode.max_diagnostics
        },
        known: HashMap::new(),
    };
    match e.data_set(utf8.elements(), "", true, true) {
        Ok(elements) => {
            let mut out = Object::from_elements_with_text_options(
                elements,
                super::text::TextOptions::default(),
            );
            out.set_value_big_endian(o.value_big_endian());
            out.value_provider = o.value_provider.clone();
            Ok((out, e.report))
        }
        Err(error) => Err(Box::new(CharsetEncodeFailure {
            error,
            report: e.report,
        })),
    }
}
pub fn lookup_strings_replacing(
    o: &Object,
    tag: super::types::Tag,
) -> Result<(Vec<String>, bool), Error> {
    let Some(elem) = o.get(tag) else {
        return Err(Error::msg(format!("dicom: missing element {tag}")));
    };
    let err = match o.decode_text_values(elem) {
        Ok(values) => return Ok((values, false)),
        Err(err) => err,
    };
    let vr = elem.vr();
    let Value::Raw(raw) = &elem.value else {
        return Err(err);
    };
    if !vr.is_string_like() {
        return Err(err);
    }
    let text = trim_text_value_bytes(vr, raw);
    let charset = character_set_for_vr(o, vr);
    let (decoded, replaced) = match charset {
        Some(charset) if !dict::is_private_creator(elem.tag()) => {
            if vr == Vr::PN {
                charset.decode_person_name_replacing(text)
            } else if !vr.uses_text_value_delimiter() {
                charset.decode_single_value_replacing(text)
            } else {
                charset.decode_replacing(text)
            }
        }
        _ => read_default_repertoire_replacing(text),
    };
    Ok((split_text_multiplicity(vr, &decoded), replaced))
}
pub fn lookup_string_replacing(
    o: &Object,
    tag: super::types::Tag,
) -> Result<(String, bool), Error> {
    let (values, replaced) = lookup_strings_replacing(o, tag)?;
    Ok((values.into_iter().next().unwrap_or_default(), replaced))
}
fn character_set_for_vr(o: &Object, vr: Vr) -> Option<SpecificCharacterSet> {
    if !vr.uses_specific_character_set() {
        return Some(SpecificCharacterSet::default());
    }
    match o.character_set_state() {
        (cs, None) => Some(cs),
        (_, Some(_)) => {
            let text = o.text_options();
            text.allow_unsupported_charset_fallback
                .then(|| text.fallback_character_set.clone())
        }
    }
}
