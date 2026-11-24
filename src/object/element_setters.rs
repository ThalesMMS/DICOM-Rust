#![forbid(unsafe_code)]
use super::error::Error;
use super::obj::{Object, TAG_SPECIFIC_CHARACTER_SET};
use super::text::PersonName;
use super::types::{DataSet, Element, ElementHeader, Tag, UNDEFINED_LENGTH, Value, Vr};
use crate::dcmtime::{self, PrecisionLevel, ZonedTime};
use crate::numtext::{format_float, parse_float};
use crate::textfmt::float_v;
use crate::validation;
pub const ERR_INVALID_ELEMENT_VALUE: &str = "dicom: invalid element value";
const MAX_DECIMAL_STRING_LENGTH: usize = 16;
fn ftoa_g(value: f64, precision: i32) -> String {
    let precision = match precision {
        -2 => -1,
        p if p < 0 => 6,
        p => p,
    };
    format_float(value, 'g', precision)
}
fn ftoa_e(value: f64, precision: i32) -> String {
    format_float(value, 'e', if precision < 0 { 6 } else { precision })
}
pub fn format_ds(value: f64) -> Result<String, Error> {
    if !value.is_finite() {
        return Err(Error::msg(format!(
            "{ERR_INVALID_ELEMENT_VALUE}: DS cannot encode {}",
            float_v(value)
        )));
    }
    let s = compact_exponent(&ftoa_g(value, -2));
    if s.len() <= MAX_DECIMAL_STRING_LENGTH {
        return Ok(s);
    }
    for digits in (1..=17).rev() {
        let general = compact_exponent(&ftoa_g(value, digits));
        let exponential = compact_exponent(&ftoa_e(value, digits - 1));
        for s in [general, exponential] {
            if s.len() <= MAX_DECIMAL_STRING_LENGTH && parse_float(&s).is_ok() {
                return Ok(s);
            }
        }
    }
    Err(Error::msg(format!(
        "{ERR_INVALID_ELEMENT_VALUE}: DS cannot encode {} in 16 bytes",
        float_v(value)
    )))
}
fn compact_exponent(s: &str) -> String {
    let Some(i) = s.find('e') else {
        return s.to_string();
    };
    let (mantissa, exponent) = (&s[..i], &s[i + 1..]);
    let (sign, exponent) = if let Some(rest) = exponent.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", exponent.strip_prefix('+').unwrap_or(exponent))
    };
    let exponent = exponent.trim_start_matches('0');
    if exponent.is_empty() {
        return mantissa.to_string();
    }
    format!("{mantissa}e{sign}{exponent}")
}
pub fn format_is(value: i64) -> Result<String, Error> {
    if value < i64::from(i32::MIN) || value > i64::from(i32::MAX) {
        return Err(Error::msg(format!(
            "{ERR_INVALID_ELEMENT_VALUE}: IS value {value} is outside the signed 32-bit range"
        )));
    }
    Ok(value.to_string())
}
fn strings_element(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(values),
    }
}
fn finding_message(code: validation::Code) -> &'static str {
    match code {
        validation::CODE_VALUE_LENGTH => "text component exceeds the VR length limit",
        validation::CODE_VALUE_REPERTOIRE => {
            "text component contains characters outside the VR repertoire"
        }
        validation::CODE_VALUE_FORMAT | validation::CODE_VALUE_RANGE => {
            "text component does not satisfy the VR format"
        }
        validation::CODE_VALUE_KIND => "string value is incompatible with the encoded VR",
        validation::CODE_DUPLICATE_ELEMENT => "duplicate element in the same data set",
        _ => "validation finding",
    }
}
fn validated_string(
    tag: Tag,
    vr: Vr,
    values: &[String],
    character_set: Option<&Element>,
) -> Result<Element, Error> {
    if !vr.is_string_like() {
        return Err(Error::msg(format!(
            "{ERR_INVALID_ELEMENT_VALUE}: {vr} is not a string VR"
        )));
    }
    let element = strings_element(tag, vr, values.to_vec());
    let mut elements = vec![element.clone()];
    if let Some(cs) = character_set
        && cs.tag() != tag
    {
        elements.insert(0, cs.clone());
    }
    let report = validation::validate_data_set(
        &DataSet {
            elements,
            item_offset: None,
        },
        validation::Options {
            dictionary: false,
            max_findings: 0,
        },
    );
    if let Some(finding) = report
        .findings
        .iter()
        .find(|f| f.severity == validation::Severity::Error && f.tag == tag)
    {
        return Err(Error::msg(format!(
            "{ERR_INVALID_ELEMENT_VALUE}: {tag} {vr}: {}",
            finding_message(finding.code)
        )));
    }
    Ok(element)
}
pub fn string_element(tag: Tag, vr: Vr, values: &[String]) -> Result<Element, Error> {
    validated_string(tag, vr, values, None)
}
pub fn ds_element(tag: Tag, values: &[f64]) -> Result<Element, Error> {
    let mut out = Vec::with_capacity(values.len());
    for (i, &value) in values.iter().enumerate() {
        out.push(format_ds(value).map_err(|e| Error::wrap(&format!("{tag} value {i}"), e))?);
    }
    Ok(strings_element(tag, Vr::DS, out))
}
pub fn is_element(tag: Tag, values: &[i64]) -> Result<Element, Error> {
    let mut out = Vec::with_capacity(values.len());
    for (i, &value) in values.iter().enumerate() {
        out.push(format_is(value).map_err(|e| Error::wrap(&format!("{tag} value {i}"), e))?);
    }
    Ok(strings_element(tag, Vr::IS, out))
}
fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, raw.len() as u32),
        value: Value::Raw(raw),
    }
}
macro_rules! binary_element {
    ($(#[$doc:meta])* $name:ident, $t:ty, $vr:expr, |$v:ident| $bits:expr) => {
        $(#[$doc])*
        pub fn $name(tag: Tag, big: bool, values: &[$t]) -> Element {
            let mut raw = Vec::with_capacity(values.len() * std::mem::size_of::<$t>());
            for &$v in values {
                let bits = $bits;
                if big {
                    raw.extend_from_slice(&bits.to_be_bytes());
                } else {
                    raw.extend_from_slice(&bits.to_le_bytes());
                }
            }
            raw_element(tag, $vr, raw)
        }
    };
}
binary_element!(us_element, u16, Vr::US, |v| v);
binary_element!(ss_element, i16, Vr::SS, |v| v as u16);
binary_element!(ul_element, u32, Vr::UL, |v| v);
binary_element!(sl_element, i32, Vr::SL, |v| v as u32);
binary_element!(fl_element, f32, Vr::FL, |v| v.to_bits());
binary_element!(fd_element, f64, Vr::FD, |v| v.to_bits());
pub fn at_element(tag: Tag, big: bool, tags: &[Tag]) -> Element {
    let mut raw = Vec::with_capacity(4 * tags.len());
    for t in tags {
        for v in [t.group, t.element] {
            if big {
                raw.extend_from_slice(&v.to_be_bytes());
            } else {
                raw.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    raw_element(tag, Vr::AT, raw)
}
fn time_precision(t: &ZonedTime) -> PrecisionLevel {
    if t.nanosecond() >= 1000 {
        PrecisionLevel::FULL
    } else {
        PrecisionLevel::SECONDS
    }
}
pub fn date_element(tag: Tag, t: ZonedTime) -> Element {
    let date = dcmtime::Date {
        time: t,
        precision: PrecisionLevel::FULL,
        is_nema: false,
    };
    strings_element(tag, Vr::DA, vec![date.dcm()])
}
pub fn time_element(tag: Tag, t: ZonedTime) -> Element {
    let time = dcmtime::Time {
        time: t,
        precision: time_precision(&t),
    };
    strings_element(tag, Vr::TM, vec![time.dcm()])
}
pub fn date_time_element(tag: Tag, t: ZonedTime) -> Element {
    let datetime = dcmtime::Datetime {
        time: t,
        precision: time_precision(&t),
        no_offset: false,
    };
    strings_element(tag, Vr::DT, vec![datetime.dcm()])
}
pub fn pn_element(tag: Tag, names: &[PersonName]) -> Result<Element, Error> {
    let values: Vec<String> = names.iter().map(PersonName::to_dicom_string).collect();
    string_element(tag, Vr::PN, &values)
}
pub fn sequence_element(tag: Tag, items: &[Option<&Object>]) -> Element {
    let items = items
        .iter()
        .map(|item| match item {
            Some(o) => DataSet {
                elements: o.sorted_elements(),
                item_offset: o.to_data_set().item_offset,
            },
            None => DataSet::default(),
        })
        .collect();
    Element {
        header: ElementHeader::new(tag, Vr::SQ, UNDEFINED_LENGTH),
        value: Value::Sequence(items),
    }
}
impl Object {
    pub fn set_strings(&mut self, tag: Tag, vr: Vr, values: &[String]) -> Result<(), Error> {
        let character_set = self.get(TAG_SPECIFIC_CHARACTER_SET).cloned();
        let element = validated_string(tag, vr, values, character_set.as_ref())?;
        self.put(element);
        Ok(())
    }
    pub fn set_ds(&mut self, tag: Tag, values: &[f64]) -> Result<(), Error> {
        self.put(ds_element(tag, values)?);
        Ok(())
    }
    pub fn set_is(&mut self, tag: Tag, values: &[i64]) -> Result<(), Error> {
        self.put(is_element(tag, values)?);
        Ok(())
    }
    pub fn set_pn(&mut self, tag: Tag, names: &[PersonName]) -> Result<(), Error> {
        let values: Vec<String> = names.iter().map(PersonName::to_dicom_string).collect();
        self.set_strings(tag, Vr::PN, &values)
    }
    pub fn set_us(&mut self, tag: Tag, values: &[u16]) {
        let big = self.value_big_endian();
        self.put(us_element(tag, big, values));
    }
    pub fn set_ss(&mut self, tag: Tag, values: &[i16]) {
        let big = self.value_big_endian();
        self.put(ss_element(tag, big, values));
    }
    pub fn set_ul(&mut self, tag: Tag, values: &[u32]) {
        let big = self.value_big_endian();
        self.put(ul_element(tag, big, values));
    }
    pub fn set_sl(&mut self, tag: Tag, values: &[i32]) {
        let big = self.value_big_endian();
        self.put(sl_element(tag, big, values));
    }
    pub fn set_fl(&mut self, tag: Tag, values: &[f32]) {
        let big = self.value_big_endian();
        self.put(fl_element(tag, big, values));
    }
    pub fn set_fd(&mut self, tag: Tag, values: &[f64]) {
        let big = self.value_big_endian();
        self.put(fd_element(tag, big, values));
    }
    pub fn set_at(&mut self, tag: Tag, tags: &[Tag]) {
        let big = self.value_big_endian();
        self.put(at_element(tag, big, tags));
    }
    pub fn set_date(&mut self, tag: Tag, t: ZonedTime) {
        self.put(date_element(tag, t));
    }
    pub fn set_time(&mut self, tag: Tag, t: ZonedTime) {
        self.put(time_element(tag, t));
    }
    pub fn set_date_time(&mut self, tag: Tag, t: ZonedTime) {
        self.put(date_time_element(tag, t));
    }
    pub fn set_sequence(&mut self, tag: Tag, items: &[Option<&Object>]) {
        self.put(sequence_element(tag, items));
    }
}
