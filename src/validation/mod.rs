#![forbid(unsafe_code)]
mod engine;
mod hooks;
pub mod iod;
mod operation;
pub mod write;
use crate::charset::SpecificCharacterSet;
use crate::dcmtime;
use crate::object::text::{split_text_multiplicity, trim_text_value_bytes};
use crate::object::{DataSet, Element, Tag, Value, Vr, dict, is_valid_uid};
pub use engine::*;
pub use hooks::*;
pub use operation::*;
pub type Code = &'static str;
pub const CODE_VALUE_LENGTH: Code = "vr.value_length";
pub const CODE_VALUE_PADDING: Code = "vr.padding";
pub const CODE_RESERVED_BYTES: Code = "vr.reserved_bytes";
pub const CODE_VALUE_REPERTOIRE: Code = "vr.repertoire";
pub const CODE_VALUE_FORMAT: Code = "vr.format";
pub const CODE_VALUE_RANGE: Code = "vr.range";
pub const CODE_VALUE_KIND: Code = "vr.value_kind";
pub const CODE_VALUE_MULTIPLICITY: Code = "dictionary.vm";
pub const CODE_DICTIONARY_VR: Code = "dictionary.vr";
pub const CODE_DUPLICATE_ELEMENT: Code = "dataset.duplicate";
pub const CODE_ELEMENT_ORDER: Code = "dataset.order";
pub const CODE_REQUIRED_UID: Code = "dataset.required_uid";
pub const CODE_SEQUENCE_STRUCTURE: Code = "dataset.sequence_structure";
pub const CODE_FRAGMENT_STRUCTURE: Code = "dataset.fragment_structure";
pub const CODE_PIXEL_METADATA: Code = "dataset.pixel_metadata";
pub const CODE_DATA_SET_RULE: Code = "dataset.rule";
pub const CODE_FILE_META_MISMATCH: Code = "file_meta.mismatch";
pub const CODE_HOOK_DIAGNOSTIC: Code = "hook.diagnostic";
pub const CODE_HOOK_TRANSFORMED: Code = "hook.transformed";
pub const CODE_HOOK_FILTERED: Code = "hook.filtered";
pub const CODE_HOOK_SKIPPED: Code = "hook.skipped";
pub const CODE_HOOK_DEFERRED: Code = "hook.deferred";
pub const CODE_HOOK_PANIC: Code = "hook.panic";
pub const CODE_HOOK_TIMEOUT: Code = "hook.timeout";
pub const CODE_HOOK_ERROR: Code = "hook.error";
pub const CODE_HOOK_ACTION: Code = "hook.invalid_action";
pub const CODE_RESOURCE_LIMIT: Code = "validation.resource_limit";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
}
impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}
pub const NO_ITEM: i64 = -1;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PathStep {
    pub tag: Tag,
    pub item_index: i64,
}
impl PathStep {
    pub fn element(tag: Tag) -> PathStep {
        PathStep {
            tag,
            item_index: NO_ITEM,
        }
    }
}
pub type Path = Vec<PathStep>;
#[doc(hidden)]
pub fn path_string(path: &[PathStep]) -> String {
    let parts: Vec<String> = path
        .iter()
        .map(|s| {
            if s.item_index >= 0 {
                format!("{}[{}]", s.tag, s.item_index)
            } else {
                s.tag.to_string()
            }
        })
        .collect();
    parts.join("/")
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub path: Path,
    pub tag: Tag,
    pub vr: Option<Vr>,
    pub expected_vr: Vec<Vr>,
    pub rule: String,
    pub severity: Severity,
    pub code: Code,
    pub offset: i64,
    pub offset_set: bool,
    pub hook: String,
    pub message: String,
    pub(crate) occurrence: usize,
}
impl Finding {
    fn basic(tag: Tag, vr: Vr, code: Code, severity: Severity) -> Finding {
        Finding::new(Vec::new(), tag, Some(vr), code, severity, "")
    }
    pub fn new(
        path: Path,
        tag: Tag,
        vr: Option<Vr>,
        code: Code,
        severity: Severity,
        message: &str,
    ) -> Finding {
        Finding {
            path,
            tag,
            vr,
            expected_vr: Vec::new(),
            rule: String::new(),
            severity,
            code,
            offset: 0,
            offset_set: false,
            hook: String::new(),
            message: message.to_string(),
            occurrence: 0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Transformed,
    Filtered,
    Skipped,
    Deferred,
}
impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Transformed => "transformed",
            ChangeKind::Filtered => "filtered",
            ChangeKind::Skipped => "skipped",
            ChangeKind::Deferred => "deferred",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: Path,
    pub tag: Tag,
    pub hook: String,
    pub kind: ChangeKind,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub changes: Vec<Change>,
    pub truncated: bool,
    pub dropped: i64,
    pub findings_dropped: bool,
    pub errors_dropped: bool,
}
impl Report {
    pub fn has_errors(&self) -> bool {
        self.errors_dropped || self.findings.iter().any(|f| f.severity == Severity::Error)
    }
    pub fn count(&self, code: Code) -> usize {
        self.findings.iter().filter(|f| f.code == code).count()
    }
    pub(crate) fn has_findings(&self) -> bool {
        !self.findings.is_empty() || self.findings_dropped || self.errors_dropped
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub dictionary: bool,
    pub max_findings: usize,
}
impl Default for Options {
    fn default() -> Self {
        Options {
            dictionary: true,
            max_findings: 0,
        }
    }
}
const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
struct Session {
    opts: Options,
    max: usize,
    report: Report,
}
impl Session {
    fn add(&mut self, tag: Tag, vr: Vr, code: Code, severity: Severity) {
        if self.report.findings.len() >= self.max {
            self.report.truncated = true;
            return;
        }
        self.report
            .findings
            .push(Finding::basic(tag, vr, code, severity));
    }
    fn process(&mut self, ds: &DataSet, inherited: &Option<SpecificCharacterSet>) {
        let charset = resolve_character_set(ds, inherited);
        let mut seen = std::collections::HashSet::new();
        let mut previous: Option<Tag> = None;
        for element in &ds.elements {
            let tag = element.tag();
            if !seen.insert(tag) {
                self.add(tag, element.vr(), CODE_DUPLICATE_ELEMENT, Severity::Error);
            }
            if previous.is_some_and(|p| tag < p) {
                self.add(tag, element.vr(), CODE_ELEMENT_ORDER, Severity::Warning);
            }
            previous = Some(tag);
            self.validate_dictionary(element);
            self.validate_value(element, &charset);
            if let Value::Sequence(items) = &element.value {
                for item in items {
                    self.process(item, &charset);
                }
            }
        }
    }
    fn validate_dictionary(&mut self, element: &Element) {
        if !self.opts.dictionary {
            return;
        }
        let Some(entry) = dict::by_tag(element.tag()) else {
            return;
        };
        let expected = dict::vr_values(entry);
        if element.vr() != Vr::UN && !expected.is_empty() && !expected.contains(&element.vr()) {
            self.add(
                element.tag(),
                element.vr(),
                CODE_DICTIONARY_VR,
                Severity::Error,
            );
        }
    }
    fn validate_value(&mut self, element: &Element, charset: &Option<SpecificCharacterSet>) {
        let vr = element.vr();
        let tag = element.tag();
        let h = &element.header;
        if h.length_set && h.length != crate::object::UNDEFINED_LENGTH && h.length & 1 != 0 {
            self.add(tag, vr, CODE_VALUE_LENGTH, Severity::Error);
        }
        match &element.value {
            Value::Nil => {}
            Value::BulkData(uri) => {
                if uri.trim().is_empty() {
                    self.add(tag, vr, CODE_VALUE_FORMAT, Severity::Error);
                }
            }
            Value::Strings(values) => {
                if !vr.is_string_like() {
                    self.add(tag, vr, CODE_VALUE_KIND, Severity::Error);
                    return;
                }
                for component in values {
                    self.text_component(tag, vr, component.as_bytes(), false, charset);
                }
            }
            Value::Raw(raw) => {
                if vr.is_string_like() {
                    let n = raw.len();
                    let wrong_pad = n > 0
                        && n % 2 == 0
                        && raw[n - 1] != vr.padding_byte()
                        && (raw[n - 1] == 0 || raw[n - 1] == b' ');
                    if wrong_pad || (vr == Vr::UI && invalid_uid_padding(raw)) {
                        self.add(tag, vr, CODE_VALUE_PADDING, Severity::Error);
                    }
                    if vr.uses_specific_character_set() {
                        let Some(decoded) =
                            decode_encoded_text(charset, vr, trim_text_value_bytes(vr, raw))
                        else {
                            self.add(tag, vr, CODE_VALUE_REPERTOIRE, Severity::Error);
                            return;
                        };
                        for component in split_text_multiplicity(vr, &decoded) {
                            self.text_component(tag, vr, component.as_bytes(), false, charset);
                        }
                        return;
                    }
                    if raw_text_too_long(vr, raw) {
                        self.add(tag, vr, CODE_VALUE_LENGTH, Severity::Error);
                    }
                    for component in split_raw_multiplicity(vr, raw) {
                        self.text_component(tag, vr, &component, true, charset);
                    }
                    return;
                }
                if vr == Vr::SQ {
                    self.add(tag, vr, CODE_VALUE_KIND, Severity::Error);
                    return;
                }
                let width = binary_width(vr);
                if width > 0 && raw.len() % width != 0 {
                    self.add(tag, vr, CODE_VALUE_LENGTH, Severity::Error);
                }
            }
            Value::Sequence(_) => {
                if vr != Vr::SQ && vr != Vr::UN {
                    self.add(tag, vr, CODE_SEQUENCE_STRUCTURE, Severity::Error);
                }
            }
            Value::Fragments { offset_table, .. } => {
                if tag != Tag::PIXEL_DATA || (vr != Vr::OB && vr != Vr::OW) {
                    self.add(tag, vr, CODE_FRAGMENT_STRUCTURE, Severity::Error);
                }
                if offset_table.len() % 4 != 0 {
                    self.add(tag, vr, CODE_FRAGMENT_STRUCTURE, Severity::Error);
                }
            }
        }
    }
    fn text_component(
        &mut self,
        tag: Tag,
        vr: Vr,
        component: &[u8],
        encoded_raw: bool,
        charset: &Option<SpecificCharacterSet>,
    ) {
        let max = max_text_length(vr);
        if max > 0 && !encoded_raw && text_length(vr, component) > max {
            self.add(tag, vr, CODE_VALUE_LENGTH, Severity::Error);
        }
        if !valid_text_repertoire(vr, component, encoded_raw)
            || (encoded_raw
                && vr.uses_specific_character_set()
                && !valid_encoded_text(charset, vr, component))
        {
            self.add(tag, vr, CODE_VALUE_REPERTOIRE, Severity::Error);
        }
        let (valid, range) = validate_text_format(vr, component);
        if !valid {
            let code = if range {
                CODE_VALUE_RANGE
            } else {
                CODE_VALUE_FORMAT
            };
            self.add(tag, vr, code, Severity::Error);
        }
    }
}
fn resolve_character_set(
    ds: &DataSet,
    inherited: &Option<SpecificCharacterSet>,
) -> Option<SpecificCharacterSet> {
    for e in &ds.elements {
        if e.tag() != TAG_SPECIFIC_CHARACTER_SET {
            continue;
        }
        let values = string_values(e);
        return SpecificCharacterSet::parse(&values).ok();
    }
    inherited.clone()
}
fn string_values(e: &Element) -> Vec<String> {
    match &e.value {
        Value::Raw(raw) => crate::object::text::raw_string_values(e.vr(), raw)
            .into_iter()
            .map(|v| String::from_utf8_lossy(&v).into_owned())
            .collect(),
        Value::Strings(values) => values
            .iter()
            .map(|v| crate::object::text::trim_text_value(e.vr(), v).to_string())
            .collect(),
        _ => Vec::new(),
    }
}
fn decode_encoded_text(
    charset: &Option<SpecificCharacterSet>,
    vr: Vr,
    value: &[u8],
) -> Option<String> {
    let cs = charset.as_ref()?;
    if vr == Vr::PN {
        return cs.decode_person_name(value).ok();
    }
    if !vr.uses_text_value_delimiter() {
        return cs.decode_single_value(value).ok();
    }
    cs.decode(value).ok()
}
fn valid_encoded_text(charset: &Option<SpecificCharacterSet>, vr: Vr, value: &[u8]) -> bool {
    let Some(cs) = charset else {
        return false;
    };
    if value.contains(&0x1b) && !cs.names().iter().any(|n| n.starts_with("ISO 2022 ")) {
        return false;
    }
    if vr == Vr::PN {
        cs.decode_person_name(value).is_ok()
    } else {
        cs.decode(value).is_ok()
    }
}
pub fn validate_data_set(ds: &DataSet, opts: Options) -> Report {
    let mut s = Session {
        opts,
        max: if opts.max_findings == 0 {
            1000
        } else {
            opts.max_findings
        },
        report: Report::default(),
    };
    s.process(ds, &Some(SpecificCharacterSet::default_repertoire()));
    s.report
}
pub fn validate_element(element: &Element, opts: Options) -> Report {
    validate_data_set(
        &DataSet {
            elements: vec![element.clone()],
            item_offset: None,
        },
        opts,
    )
}
fn invalid_uid_padding(raw: &[u8]) -> bool {
    let raw = raw.strip_suffix(&[0]).unwrap_or(raw);
    raw.last().is_some_and(|&b| b == 0 || b == b' ')
}
fn raw_text_too_long(vr: Vr, raw: &[u8]) -> bool {
    let max = max_text_length(vr);
    if max == 0 || vr.uses_specific_character_set() {
        return false;
    }
    let mut raw = raw;
    let n = raw.len();
    if n > 0
        && n.is_multiple_of(2)
        && (raw[n - 1] == vr.padding_byte() || raw[n - 1] == b' ' || raw[n - 1] == 0)
    {
        raw = &raw[..n - 1];
    }
    if !vr.uses_text_value_delimiter() {
        return raw.len() > max;
    }
    raw.split(|&b| b == b'\\').any(|c| c.len() > max)
}
fn is_single_text_vr(vr: Vr) -> bool {
    matches!(vr, Vr::LT | Vr::ST | Vr::UT | Vr::UR)
}
fn split_raw_multiplicity(vr: Vr, raw: &[u8]) -> Vec<Vec<u8>> {
    if is_single_text_vr(vr) {
        return vec![trim_text_value_bytes(vr, raw).to_vec()];
    }
    let trimmed = trim_text_value_bytes(vr, raw);
    if trimmed.is_empty() {
        return Vec::new();
    }
    if !vr.uses_text_value_delimiter() {
        return vec![trimmed.to_vec()];
    }
    trimmed
        .split(|&b| b == b'\\')
        .map(|p| trim_text_value_bytes(vr, p).to_vec())
        .collect()
}
fn allows_text_line_controls(vr: Vr) -> bool {
    matches!(vr, Vr::LT | Vr::ST | Vr::UT)
}
pub(crate) fn max_text_length(vr: Vr) -> usize {
    match vr {
        Vr::AE | Vr::CS | Vr::DS | Vr::SH => 16,
        Vr::AS => 4,
        Vr::DA => 8,
        Vr::DT => 26,
        Vr::IS => 12,
        Vr::LO | Vr::PN | Vr::UI => 64,
        Vr::LT => 10240,
        Vr::ST => 1024,
        Vr::TM => 16,
        _ => 0,
    }
}
fn rune_count(b: &[u8]) -> usize {
    let mut n = 0;
    let mut rest = b;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                n += s.chars().count();
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                n += std::str::from_utf8(&rest[..valid]).map_or(0, |s| s.chars().count());
                let bad = e.error_len().unwrap_or(rest.len() - valid).max(1);
                n += bad;
                rest = &rest[valid + bad..];
            }
        }
    }
    n
}
fn text_length(vr: Vr, value: &[u8]) -> usize {
    if vr == Vr::PN {
        return value
            .split(|&b| b == b'=')
            .map(rune_count)
            .max()
            .unwrap_or(0);
    }
    rune_count(value)
}
fn binary_width(vr: Vr) -> usize {
    match vr {
        Vr::US | Vr::SS | Vr::OW => 2,
        Vr::AT | Vr::FL | Vr::SL | Vr::UL | Vr::OF | Vr::OL => 4,
        Vr::FD | Vr::SV | Vr::UV | Vr::OD | Vr::OV => 8,
        _ => 0,
    }
}
fn valid_text_repertoire(vr: Vr, value: &[u8], encoded_raw: bool) -> bool {
    let line = allows_text_line_controls(vr);
    if encoded_raw && vr.uses_specific_character_set() {
        return value.iter().all(|&b| {
            b == 0x1b
                || (line && matches!(b, b'\t' | b'\n' | 0x0c | b'\r'))
                || !(b < 0x20 || b == 0x7f)
        });
    }
    let Ok(s) = std::str::from_utf8(value) else {
        return false;
    };
    if vr.uses_specific_character_set() {
        return s.chars().all(|r| {
            (line && matches!(r, '\t' | '\n' | '\u{c}' | '\r'))
                || !((r as u32) < 0x20 || r == '\u{7f}')
        });
    }
    s.chars().all(|r| {
        (is_single_text_vr(vr) && r == '\\')
            || (line && matches!(r, '\t' | '\n' | '\u{c}' | '\r'))
            || !((r as u32) < 0x20 || (r as u32) > 0x7e || r == '\\')
    })
}
fn is_as(v: &[u8]) -> bool {
    v.len() == 4
        && v[..3].iter().all(u8::is_ascii_digit)
        && matches!(v[3], b'D' | b'W' | b'M' | b'Y')
}
fn is_cs(v: &[u8]) -> bool {
    v.iter()
        .all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_' || b == b' ')
}
fn is_ds(v: &[u8]) -> bool {
    let mut i = 0;
    if i < v.len() && (v[i] == b'+' || v[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < v.len() && v[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    if i < v.len() && v[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < v.len() && v[i].is_ascii_digit() {
            i += 1;
        }
        if int_digits == 0 && i == frac_start {
            return false;
        }
    } else if int_digits == 0 {
        return false;
    }
    if i < v.len() && (v[i] == b'E' || v[i] == b'e') {
        i += 1;
        if i < v.len() && (v[i] == b'+' || v[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < v.len() && v[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return false;
        }
    }
    i == v.len()
}
fn is_is(v: &[u8]) -> bool {
    let digits = match v.first() {
        Some(b'+') | Some(b'-') => &v[1..],
        _ => v,
    };
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}
fn valid_uri_reference(value: &[u8]) -> bool {
    let mut end = value.len();
    while end > 0 && value[end - 1] == b' ' {
        end -= 1;
    }
    let v = &value[..end];
    let mut i = 0;
    while i < v.len() {
        let c = v[i];
        if c.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=".contains(&c) {
        } else if c == b'%' {
            if i + 2 >= v.len() || !v[i + 1].is_ascii_hexdigit() || !v[i + 2].is_ascii_hexdigit() {
                return false;
            }
            i += 2;
        } else {
            return false;
        }
        i += 1;
    }
    true
}
fn trim_unicode_space(v: &[u8]) -> &[u8] {
    let ws = |b: &u8| matches!(*b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let start = v.iter().position(|b| !ws(b)).unwrap_or(v.len());
    let end = v.iter().rposition(|b| !ws(b)).map_or(start, |e| e + 1);
    &v[start..end.max(start)]
}
pub(crate) fn validate_text_format(vr: Vr, value: &[u8]) -> (bool, bool) {
    if value.is_empty() {
        return (true, false);
    }
    match vr {
        Vr::AE => return (value.iter().any(|&b| b != b' '), false),
        Vr::AS => return (is_as(value), false),
        Vr::CS => return (is_cs(value), false),
        Vr::UR => return (valid_uri_reference(value), false),
        _ => {}
    }
    if matches!(vr, Vr::DA | Vr::TM | Vr::DT) && value[0] == b' ' {
        return (false, false);
    }
    let text = || String::from_utf8_lossy(value).into_owned();
    match vr {
        Vr::DA => {
            let ok = std::str::from_utf8(value).is_ok() && dcmtime::parse_da(&text()).is_ok();
            (ok, !ok && value.len() == 8)
        }
        Vr::TM => {
            let ok = std::str::from_utf8(value).is_ok() && dcmtime::parse_tm(&text()).is_ok();
            (ok, !ok)
        }
        Vr::DT => {
            let ok = std::str::from_utf8(value).is_ok() && dcmtime::parse_datetime(&text()).is_ok();
            (ok, !ok)
        }
        Vr::DS => {
            let v = trim_unicode_space(value);
            if !is_ds(v) {
                return (false, false);
            }
            let parsed: f64 = std::str::from_utf8(v)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(f64::INFINITY);
            (parsed.is_finite(), !parsed.is_finite())
        }
        Vr::IS => {
            let v = trim_unicode_space(value);
            if !is_is(v) {
                return (false, false);
            }
            let ok = std::str::from_utf8(v)
                .ok()
                .and_then(|s| s.parse::<i32>().ok())
                .is_some();
            (ok, !ok)
        }
        Vr::UI => {
            let mut end = value.len();
            while end > 0 && (value[end - 1] == b' ' || value[end - 1] == 0) {
                end -= 1;
            }
            (is_valid_uid(&value[..end]), false)
        }
        Vr::PN => {
            let groups: Vec<&[u8]> = value.split(|&b| b == b'=').collect();
            if groups.len() > 3 {
                return (false, false);
            }
            for g in groups {
                if g.split(|&b| b == b'^').count() > 5 {
                    return (false, false);
                }
            }
            (true, false)
        }
        _ => (true, false),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats() {
        assert_eq!(validate_text_format(Vr::DS, b"1.5e3"), (true, false));
        assert_eq!(validate_text_format(Vr::DS, b"."), (false, false));
        assert_eq!(validate_text_format(Vr::DS, b"1e999"), (false, true));
        assert_eq!(validate_text_format(Vr::IS, b"99999999999"), (false, true));
        assert_eq!(validate_text_format(Vr::CS, b"abc"), (false, false));
        assert_eq!(validate_text_format(Vr::AS, b"012Y"), (true, false));
    }
}
