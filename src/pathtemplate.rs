#![forbid(unsafe_code)]
use crate::dcmtime::{self, ZonedTime};
use crate::numtext::format_float;
use crate::object::{self, Element, Object, Tag, Value, Vr};
use crate::textfmt::{is_print, to_upper};
use md5::Md5;
use sha2::{Digest, Sha256};
use std::fmt;
use unicode_general_category::{GeneralCategory, get_general_category};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Syntax,
    Modifier,
    Missing,
    Multiplicity,
    Value,
    UnsafePath,
    LimitExceeded,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::Syntax => "dicom pathtemplate: invalid template syntax",
            Kind::Modifier => "dicom pathtemplate: unsupported modifier",
            Kind::Missing => "dicom pathtemplate: attribute/component is absent or empty",
            Kind::Multiplicity => "dicom pathtemplate: ambiguous value multiplicity",
            Kind::Value => "dicom pathtemplate: unsupported or invalid attribute value",
            Kind::UnsafePath => "dicom pathtemplate: unsafe relative path",
            Kind::LimitExceeded => "dicom pathtemplate: resource limit exceeded",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    pub tag: Option<Tag>,
}
impl Error {
    fn new(kind: Kind) -> Error {
        Error { kind, tag: None }
    }
    fn at(self, tag: Tag) -> Error {
        Error {
            kind: self.kind,
            tag: Some(tag),
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
    }
}
impl From<Kind> for Error {
    fn from(kind: Kind) -> Error {
        Error::new(kind)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.tag {
            Some(tag) => write!(f, "{tag}: {}", self.kind.text()),
            None => f.write_str(self.kind.text()),
        }
    }
}
impl std::error::Error for Error {}
fn is_graphic(r: char) -> bool {
    is_print(r) || get_general_category(r) == GeneralCategory::SpaceSeparator
}
fn forbidden(r: char) -> bool {
    !is_graphic(r) || "<>:\"/\\|?*".contains(r)
}
fn device_name(name: &str) -> bool {
    let base = match name.find('.') {
        Some(i) => &name[..i],
        None => name,
    };
    let base = to_upper(base.trim_end_matches(' '));
    if matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    if base.starts_with("COM") || base.starts_with("LPT") {
        let suffix = &base.as_bytes()[3..];
        return suffix.len() == 1 && suffix[0].is_ascii_digit()
            || suffix == "¹".as_bytes()
            || suffix == "²".as_bytes()
            || suffix == "³".as_bytes();
    }
    false
}
pub fn sanitize_attribute(value: &str) -> Result<String, Error> {
    sanitize(value, 4096, 255)
}
fn sanitize(value: &str, max_input: i64, max_component: i64) -> Result<String, Error> {
    if value.len() as i64 > max_input {
        return Err(Kind::LimitExceeded.into());
    }
    let replaced: String = value
        .chars()
        .map(|r| if forbidden(r) { '_' } else { r })
        .collect();
    let mut result = replaced
        .trim_end_matches(|r: char| r == '.' || r.is_whitespace())
        .to_string();
    if result.is_empty() || result == "." || result == ".." {
        result = "_".into();
    }
    if device_name(&result) {
        result.insert(0, '_');
    }
    if result.len() as i64 > max_component {
        return Err(Kind::LimitExceeded.into());
    }
    Ok(result)
}
pub fn validate_relative_path(value: &str) -> Result<(), Error> {
    validate_path(value, 4096, 255, 64)
}
fn validate_path(
    value: &str,
    max_output: i64,
    max_component: i64,
    max_components: i64,
) -> Result<(), Error> {
    if value.len() as i64 > max_output || value.matches('/').count() as i64 >= max_components {
        return Err(Kind::LimitExceeded.into());
    }
    if value.is_empty() {
        return Err(Kind::UnsafePath.into());
    }
    for component in value.split('/') {
        if component.is_empty() || component == "." || component == ".." || device_name(component) {
            return Err(Kind::UnsafePath.into());
        }
        if component.len() as i64 > max_component {
            return Err(Kind::LimitExceeded.into());
        }
        if component
            .chars()
            .next_back()
            .is_some_and(|last| last == '.' || last.is_whitespace())
        {
            return Err(Kind::UnsafePath.into());
        }
        if component.chars().any(forbidden) {
            return Err(Kind::UnsafePath.into());
        }
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MissingPolicy {
    #[default]
    Error,
    Substitute,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VmPolicy {
    #[default]
    Error,
    First,
}
pub trait KeywordDictionary {
    fn by_keyword(&self, keyword: &str) -> Option<Tag>;
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StandardDictionary;
impl KeywordDictionary for StandardDictionary {
    fn by_keyword(&self, keyword: &str) -> Option<Tag> {
        let lowered: String = keyword
            .chars()
            .map(|c| {
                let mut low = c.to_lowercase();
                match (low.next(), low.next()) {
                    (Some(l), None) => l,
                    _ if c == '\u{130}' => 'i',
                    _ => c,
                }
            })
            .collect();
        object::dict::by_keyword(&lowered).map(|(tag, _)| tag)
    }
}
#[derive(Clone, Default)]
pub struct Options<'a> {
    pub dictionary: Option<&'a dyn KeywordDictionary>,
    pub missing: MissingPolicy,
    pub replacement: String,
    pub multiplicity: VmPolicy,
    pub max_template_bytes: i64,
    pub max_output_bytes: i64,
    pub max_attribute_bytes: i64,
    pub max_expansions: i64,
    pub max_component_bytes: i64,
    pub max_components: i64,
}
impl fmt::Debug for Options<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Options")
            .field("dictionary", &self.dictionary.is_some())
            .field("missing", &self.missing)
            .field("replacement", &self.replacement)
            .field("multiplicity", &self.multiplicity)
            .field("max_template_bytes", &self.max_template_bytes)
            .field("max_output_bytes", &self.max_output_bytes)
            .field("max_attribute_bytes", &self.max_attribute_bytes)
            .field("max_expansions", &self.max_expansions)
            .field("max_component_bytes", &self.max_component_bytes)
            .field("max_components", &self.max_components)
            .finish()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Modifier {
    None,
    Sha256,
    Md5,
    Date,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Field {
    tag: Tag,
    index: Option<usize>,
    modifier: Modifier,
    days: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Part {
    Literal(String),
    Field(Field),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    parts: Vec<Part>,
    missing: MissingPolicy,
    multiplicity: VmPolicy,
    max_output_bytes: i64,
    max_attribute_bytes: i64,
    max_component_bytes: i64,
    max_components: i64,
    replacement: String,
}
pub fn compile(pattern: &str, opts: &Options<'_>) -> Result<Template, Error> {
    let mut limits = [
        opts.max_template_bytes,
        opts.max_output_bytes,
        opts.max_attribute_bytes,
        opts.max_expansions,
        opts.max_component_bytes,
        opts.max_components,
    ];
    for (value, default) in limits.iter_mut().zip([4096, 4096, 4096, 128, 255, 64]) {
        if *value < 0 {
            return Err(Kind::LimitExceeded.into());
        }
        if *value == 0 {
            *value = default;
        }
    }
    let [
        max_template_bytes,
        max_output_bytes,
        max_attribute_bytes,
        max_expansions,
        max_component_bytes,
        max_components,
    ] = limits;
    if pattern.len() as i64 > max_template_bytes {
        return Err(Kind::LimitExceeded.into());
    }
    let standard = StandardDictionary;
    let dict: &dyn KeywordDictionary = opts.dictionary.unwrap_or(&standard);
    let replacement = if opts.replacement.is_empty() {
        "_"
    } else {
        &opts.replacement
    };
    let replacement = sanitize(replacement, max_attribute_bytes, max_component_bytes)?;
    let mut parts = Vec::new();
    let mut literal: Vec<u8> = Vec::new();
    let mut skeleton = String::new();
    fn flush(literal: &mut Vec<u8>, parts: &mut Vec<Part>, skeleton: &mut String) {
        if !literal.is_empty() {
            let value = String::from_utf8_lossy(literal).into_owned();
            skeleton.push_str(&value);
            parts.push(Part::Literal(value));
            literal.clear();
        }
    }
    let bytes = pattern.as_bytes();
    let mut expansions = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                if bytes.get(i + 1) == Some(&b'{') {
                    literal.push(b'{');
                    i += 2;
                    continue;
                }
                flush(&mut literal, &mut parts, &mut skeleton);
                let Some(end) = bytes[i + 1..].iter().position(|&b| b == b'}') else {
                    return Err(Kind::Syntax.into());
                };
                let end = end + i + 1;
                expansions += 1;
                if expansions > max_expansions {
                    return Err(Kind::LimitExceeded.into());
                }
                parts.push(Part::Field(parse_field(&pattern[i + 1..end], dict)?));
                skeleton.push('x');
                i = end + 1;
            }
            b'}' => {
                if bytes.get(i + 1) != Some(&b'}') {
                    return Err(Kind::Syntax.into());
                }
                literal.push(b'}');
                i += 2;
            }
            b => {
                literal.push(b);
                i += 1;
            }
        }
    }
    flush(&mut literal, &mut parts, &mut skeleton);
    validate_path(
        &skeleton,
        max_output_bytes,
        max_component_bytes,
        max_components,
    )?;
    Ok(Template {
        parts,
        missing: opts.missing,
        multiplicity: opts.multiplicity,
        max_output_bytes,
        max_attribute_bytes,
        max_component_bytes,
        max_components,
        replacement,
    })
}
fn parse_hex_tag(selector: &str) -> Option<Tag> {
    if selector.len() != 8 || !selector.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(selector, 16).ok()?;
    Some(Tag::new((value >> 16) as u16, value as u16))
}
fn parse_field(body: &str, dict: &dyn KeywordDictionary) -> Result<Field, Error> {
    if body.contains(['{', '}', ' ', '/']) {
        return Err(Kind::Syntax.into());
    }
    let args: Vec<&str> = body.splitn(4, ',').collect();
    if args.len() > 3 {
        return Err(Kind::Syntax.into());
    }
    let mut selector = args[0];
    let mut field = Field {
        tag: Tag::new(0, 0),
        index: None,
        modifier: Modifier::None,
        days: 0,
    };
    if let Some(i) = selector.find('[') {
        if !selector.ends_with(']') {
            return Err(Kind::Syntax.into());
        }
        let index = &selector[i + 1..selector.len() - 1];
        if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Kind::Syntax.into());
        }
        let n: u64 = index.parse().map_err(|_| Error::new(Kind::Syntax))?;
        if n > 0x7FFF_FFFF {
            return Err(Kind::Syntax.into());
        }
        field.index = Some(n as usize);
        selector = &selector[..i];
    }
    if selector.is_empty() || selector.contains(['[', ']']) {
        return Err(Kind::Syntax.into());
    }
    field.tag = match parse_hex_tag(selector) {
        Some(tag) => tag,
        None => dict.by_keyword(selector).ok_or(Error::new(Kind::Syntax))?,
    };
    if args.len() == 1 {
        return Ok(field);
    }
    match args[1] {
        "sha256" | "hash" | "md5" => {
            if args.len() != 2 {
                return Err(Kind::Modifier.into());
            }
            field.modifier = if args[1] == "md5" {
                Modifier::Md5
            } else {
                Modifier::Sha256
            };
        }
        "date" => {
            field.modifier = Modifier::Date;
            if args.len() == 3 {
                let value = args[2].as_bytes();
                if value.len() < 3
                    || (value[0] != b'+' && value[0] != b'-')
                    || value[value.len() - 1] != b'd'
                {
                    return Err(Kind::Modifier.into());
                }
                let digits = &value[1..value.len() - 1];
                if !digits.iter().all(u8::is_ascii_digit) {
                    return Err(Kind::Modifier.into());
                }
                let magnitude = digits.iter().try_fold(0i64, |acc, &d| {
                    let next = acc * 10 + i64::from(d - b'0');
                    (next <= 3_652_425).then_some(next)
                });
                let Some(magnitude) = magnitude else {
                    return Err(Kind::Modifier.into());
                };
                field.days = if value[0] == b'-' {
                    -magnitude
                } else {
                    magnitude
                };
            }
        }
        _ => return Err(Kind::Modifier.into()),
    }
    Ok(field)
}
impl Template {
    pub fn format(&self, src: Option<&Object>) -> Result<String, Error> {
        let mut output = String::new();
        for part in &self.parts {
            let value = match part {
                Part::Literal(literal) => literal.clone(),
                Part::Field(field) => {
                    let selected = self.attribute(src, field).map_err(|e| e.at(field.tag))?;
                    match selected {
                        None => {
                            if self.missing != MissingPolicy::Substitute {
                                return Err(Error::new(Kind::Missing).at(field.tag));
                            }
                            self.replacement.clone()
                        }
                        Some((selected, vr)) => {
                            let modified =
                                modify(&selected, vr, field).map_err(|e| e.at(field.tag))?;
                            sanitize(&modified, self.max_output_bytes, self.max_component_bytes)
                                .map_err(|e| e.at(field.tag))?
                        }
                    }
                }
            };
            if value.len() as i64 > self.max_output_bytes - output.len() as i64 {
                return Err(Kind::LimitExceeded.into());
            }
            output.push_str(&value);
        }
        validate_path(
            &output,
            self.max_output_bytes,
            self.max_component_bytes,
            self.max_components,
        )?;
        Ok(output)
    }
}
fn modify(value: &str, vr: Vr, field: &Field) -> Result<String, Error> {
    match field.modifier {
        Modifier::Sha256 => Ok(hex(&Sha256::digest(value.as_bytes()))),
        Modifier::Md5 => Ok(hex(&Md5::digest(value.as_bytes()))),
        Modifier::Date => {
            if vr != Vr::DA || value.len() != 8 {
                return Err(Kind::Value.into());
            }
            let mut date = dcmtime::parse_date(value).map_err(|_| Error::new(Kind::Value))?;
            let t = date.time;
            let (year, month, day) = t.date();
            date.time = ZonedTime::from_fields(
                year,
                month,
                day + field.days,
                t.hour(),
                t.minute(),
                t.second(),
                t.nanosecond(),
                t.offset(),
            );
            if !(0..=9999).contains(&date.time.year()) {
                return Err(Kind::Value.into());
            }
            Ok(date.dcm())
        }
        Modifier::None => Ok(value.to_string()),
    }
}
fn hex(bytes: &[u8]) -> String {
    use fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}
fn check_payload(e: &Element, max: i64) -> Result<(), Error> {
    match &e.value {
        Value::Raw(raw) => {
            if raw.len() as i64 > max {
                return Err(Kind::LimitExceeded.into());
            }
            Ok(())
        }
        Value::Strings(values) => {
            if !values.is_empty() && values.len() as i64 - 1 > max {
                return Err(Kind::LimitExceeded.into());
            }
            let mut total = (values.len() as i64 - 1).max(0);
            for s in values {
                if s.len() as i64 > max - total {
                    return Err(Kind::LimitExceeded.into());
                }
                total += s.len() as i64;
            }
            Ok(())
        }
        _ => Err(Kind::Value.into()),
    }
}
impl Template {
    fn value_index(&self, count: usize, field: &Field) -> Result<Option<usize>, Error> {
        if count == 0 {
            return Ok(None);
        }
        if let Some(index) = field.index {
            return Ok((index < count).then_some(index));
        }
        if count > 1 && self.multiplicity != VmPolicy::First {
            return Err(Kind::Multiplicity.into());
        }
        Ok(Some(0))
    }
    fn attribute(
        &self,
        src: Option<&Object>,
        field: &Field,
    ) -> Result<Option<(String, Vr)>, Error> {
        let Some(src) = src else {
            return Ok(None);
        };
        let Some(e) = src.get(field.tag) else {
            return Ok(None);
        };
        check_payload(e, self.max_attribute_bytes)?;
        let vr = e.vr();
        if vr.is_string_like() {
            if matches!(e.value, Value::Raw(_))
                && vr.uses_specific_character_set()
                && let Some(declaration) = src.get(object::TAG_SPECIFIC_CHARACTER_SET)
            {
                check_payload(declaration, self.max_attribute_bytes)?;
            }
            let values = src
                .lookup_strings(field.tag)
                .map_err(|_| Error::new(Kind::Value))?;
            let Some(index) = self.value_index(values.len(), field)? else {
                return Ok(None);
            };
            let value = &values[index];
            if value.len() as i64 > self.max_attribute_bytes {
                return Err(Kind::LimitExceeded.into());
            }
            if value.is_empty() {
                return Ok(None);
            }
            return Ok(Some((value.clone(), vr)));
        }
        let Some(value) = self.numeric(src, e, field)? else {
            return Ok(None);
        };
        if value.len() as i64 > self.max_attribute_bytes {
            return Err(Kind::LimitExceeded.into());
        }
        Ok(Some((value, vr)))
    }
    fn numeric(&self, src: &Object, e: &Element, field: &Field) -> Result<Option<String>, Error> {
        let vr = e.vr();
        let width = numeric_width(vr);
        let raw = match &e.value {
            Value::Raw(raw) if width != 0 && raw.len() % width == 0 => raw,
            _ => return Err(Kind::Value.into()),
        };
        let Some(index) = self.value_index(raw.len() / width, field)? else {
            return Ok(None);
        };
        let value_error = |_| Error::new(Kind::Value);
        let text = match vr {
            Vr::UV => {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&raw[index * 8..index * 8 + 8]);
                let value = if src.value_big_endian() {
                    u64::from_be_bytes(bytes)
                } else {
                    u64::from_le_bytes(bytes)
                };
                value.to_string()
            }
            Vr::US | Vr::SS | Vr::UL | Vr::SL | Vr::SV => {
                let values = src.get_ints(e.tag()).map_err(value_error)?;
                values
                    .get(index)
                    .ok_or(Error::new(Kind::Value))?
                    .to_string()
            }
            Vr::FL | Vr::FD => {
                let values = src.get_floats(e.tag()).map_err(value_error)?;
                let value = *values.get(index).ok_or(Error::new(Kind::Value))?;
                if vr == Vr::FL {
                    format_float32(value as f32)
                } else {
                    format_float(value, 'g', -1)
                }
            }
            Vr::AT => {
                let values = src.get_tags(e.tag()).ok_or(Error::new(Kind::Value))?;
                let tag = values.get(index).ok_or(Error::new(Kind::Value))?;
                format!("{:04X}{:04X}", tag.group, tag.element)
            }
            _ => return Err(Kind::Value.into()),
        };
        Ok(Some(text))
    }
}
fn numeric_width(vr: Vr) -> usize {
    match vr {
        Vr::US | Vr::SS => 2,
        Vr::UL | Vr::SL | Vr::FL | Vr::AT => 4,
        Vr::UV | Vr::SV | Vr::FD => 8,
        _ => 0,
    }
}
fn format_float32(x: f32) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "+Inf" } else { "-Inf" }.into();
    }
    let mut out = String::new();
    if x.is_sign_negative() {
        out.push('-');
    }
    if x == 0.0 {
        out.push('0');
        return out;
    }
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let nd = digits.len() as i32;
    let digit = |i: i32| digits.get(i as usize).map_or('0', |&d| d as char);
    if !(-4..6).contains(&exp) {
        out.push(digit(0));
        if nd > 1 {
            out.push('.');
            (1..nd).for_each(|i| out.push(digit(i)));
        }
        out.push('e');
        out.push(if exp < 0 { '-' } else { '+' });
        let magnitude = exp.unsigned_abs();
        if magnitude < 10 {
            out.push('0');
        }
        out.push_str(&magnitude.to_string());
    } else if exp < 0 {
        out.push_str("0.");
        (0..-exp - 1).for_each(|_| out.push('0'));
        (0..nd).for_each(|i| out.push(digit(i)));
    } else {
        (0..=exp).for_each(|i| out.push(digit(i)));
        if nd > exp + 1 {
            out.push('.');
            (exp + 1..nd).for_each(|i| out.push(digit(i)));
        }
    }
    out
}
