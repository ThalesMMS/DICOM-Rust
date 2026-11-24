use super::hooks::{
    ChangeKindCode, HookChain, HookEvent, HookPoint, HookResult, RuleFinding, valid_name,
};
use super::{
    CODE_DATA_SET_RULE, CODE_DICTIONARY_VR, CODE_DUPLICATE_ELEMENT, CODE_ELEMENT_ORDER,
    CODE_FILE_META_MISMATCH, CODE_FRAGMENT_STRUCTURE, CODE_PIXEL_METADATA, CODE_REQUIRED_UID,
    CODE_SEQUENCE_STRUCTURE, CODE_VALUE_FORMAT, CODE_VALUE_KIND, CODE_VALUE_LENGTH,
    CODE_VALUE_MULTIPLICITY, CODE_VALUE_PADDING, CODE_VALUE_RANGE, CODE_VALUE_REPERTOIRE, Change,
    Code, Finding, Path, PathStep, Report, Severity, binary_width, decode_encoded_text,
    invalid_uid_padding, max_text_length, path_string, raw_text_too_long, resolve_character_set,
    split_raw_multiplicity, text_length, valid_encoded_text, valid_text_repertoire,
    validate_text_format,
};
use crate::charset::SpecificCharacterSet;
use crate::object::text::{normalize_uid, split_text_multiplicity, trim_text_value_bytes};
use crate::object::{DataSet, Element, Tag, UNDEFINED_LENGTH, Value, Vr, dict, standard_names};
use crate::textfmt::quote;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
pub const DEFAULT_MAX_FINDINGS: i64 = 256;
pub const DEFAULT_MAX_DEPTH: i64 = 64;
pub const DEFAULT_MAX_ELEMENTS: i64 = 1_000_000;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Preserve,
    Warn,
    Strict,
}
#[derive(Clone, Debug)]
pub struct ElementContext {
    pub element: Element,
    pub data_set: DataSet,
    pub path: Path,
    pub big_endian: bool,
}
pub type VrResolver = Arc<dyn Fn(&ElementContext, dict::Entry) -> Vec<Vr> + Send + Sync>;
#[derive(Clone, Debug)]
pub struct DataSetContext {
    pub data_set: DataSet,
    pub path: Path,
    pub dictionary: bool,
    pub big_endian: bool,
    pub transfer_syntax_uid: String,
}
pub type DataSetRule = Arc<dyn Fn(&DataSetContext) -> Vec<RuleFinding> + Send + Sync>;
#[derive(Clone)]
pub struct DataSetRuleRegistration {
    pub name: String,
    pub rule: Option<DataSetRule>,
}
#[derive(Clone, Default)]
pub struct ValidationOptions {
    pub mode: Mode,
    pub dictionary: bool,
    pub max_findings: i64,
    pub stop_first: bool,
    pub max_depth: i64,
    pub max_elements: i64,
    pub big_endian: bool,
    pub required_uids: Vec<Tag>,
    pub resolve_vr: Option<VrResolver>,
    pub hooks: Option<Arc<HookChain>>,
    pub transfer_syntax_uid: String,
    pub data_set_rules: Vec<DataSetRuleRegistration>,
}
#[derive(Clone, Debug, Default)]
pub struct ValidationResult {
    pub data_set: DataSet,
    pub report: Report,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPolicy(String),
    Limit(String),
    Failed(Report),
    Hook {
        hook: String,
        code: Code,
        action: bool,
    },
}
impl Error {
    pub fn is_validation_failed(&self) -> bool {
        matches!(self, Error::Failed(_))
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidPolicy(d) if d.is_empty() => {
                f.write_str("dicom validation policy is invalid")
            }
            Error::InvalidPolicy(d) => write!(f, "dicom validation policy is invalid: {d}"),
            Error::Limit(d) if d.is_empty() => {
                f.write_str("dicom validation resource limit exceeded")
            }
            Error::Limit(d) => write!(f, "dicom validation resource limit exceeded: {d}"),
            Error::Failed(report) => {
                if report.errors_dropped {
                    return f.write_str(
                        "dicom validation failed with errors omitted by the report limit",
                    );
                }
                match report.findings.len() {
                    1 => f.write_str("dicom validation failed with 1 finding"),
                    n => write!(f, "dicom validation failed with {n} findings"),
                }
            }
            Error::Hook { hook, code, .. } => {
                write!(f, "dicom validation hook {} failed ({code})", quote(hook))
            }
        }
    }
}
impl std::error::Error for Error {}
pub(crate) struct ReportBuilder {
    mode: Mode,
    max: i64,
    stop_first: bool,
    pub(crate) report: Report,
    pub(crate) write_budget: Option<super::write::SharedBudget>,
    pub(crate) limit_err: Option<Error>,
}
impl ReportBuilder {
    pub(crate) fn new(opts: &ValidationOptions) -> Result<ReportBuilder, Error> {
        if opts.max_findings < 0 || opts.max_depth < 0 || opts.max_elements < 0 {
            return Err(Error::InvalidPolicy(String::new()));
        }
        let max = if opts.max_findings == 0 {
            DEFAULT_MAX_FINDINGS
        } else {
            opts.max_findings
        };
        let mut seen = HashSet::new();
        for registration in &opts.data_set_rules {
            if !valid_name(&registration.name) || registration.rule.is_none() {
                return Err(Error::InvalidPolicy(String::new()));
            }
            if !seen.insert(registration.name.clone()) {
                return Err(Error::InvalidPolicy(String::new()));
            }
        }
        Ok(ReportBuilder {
            mode: opts.mode,
            max,
            stop_first: opts.stop_first,
            report: Report::default(),
            write_budget: None,
            limit_err: None,
        })
    }
    pub(crate) fn add_with(&mut self, mut finding: Finding, severity: Option<Severity>) {
        if self.stop_first && self.report.has_findings() {
            return;
        }
        finding.severity = severity.unwrap_or(Severity::Error);
        if self.mode == Mode::Warn && finding.severity == Severity::Error {
            finding.severity = Severity::Warning;
        }
        if (self.report.findings.len() + self.report.changes.len()) as i64 >= self.max {
            self.report.truncated = true;
            self.report.dropped += 1;
            self.report.findings_dropped = true;
            self.report.errors_dropped =
                self.report.errors_dropped || finding.severity == Severity::Error;
            return;
        }
        if let Some(budget) = &self.write_budget {
            let count = self.report.findings.len() as i64 + 1;
            if let Err(e) = budget.borrow_mut().admit_finding(&finding, count) {
                self.limit_err = Some(e);
                return;
            }
        }
        self.report.findings.push(finding);
    }
    pub(crate) fn add(&mut self, finding: Finding) {
        let severity = finding.severity;
        self.add_with(finding, Some(severity));
    }
    pub(crate) fn change(&mut self, change: Change) {
        if (self.report.findings.len() + self.report.changes.len()) as i64 >= self.max {
            self.report.truncated = true;
            self.report.dropped += 1;
            return;
        }
        if let Some(budget) = &self.write_budget {
            let count = self.report.changes.len() as i64 + 1;
            if let Err(e) = budget.borrow_mut().admit_change(&change, count) {
                self.limit_err = Some(e);
                return;
            }
        }
        self.report.changes.push(change);
    }
    pub(crate) fn should_stop(&self, opts: &ValidationOptions) -> bool {
        opts.stop_first && self.report.has_findings()
    }
}
pub(crate) fn add_hook_result(builder: &mut ReportBuilder, result: &HookResult) {
    for finding in &result.findings {
        builder.add(finding.clone());
    }
    for change in &result.changes {
        builder.change(change.clone());
        let (code, message) = change.kind.code_message();
        let mut finding = Finding::new(
            change.path.clone(),
            change.tag,
            None,
            code,
            Severity::Info,
            message,
        );
        finding.hook = change.hook.clone();
        builder.add(finding);
    }
}
pub fn validate_options(opts: &ValidationOptions) -> Result<(), Error> {
    ReportBuilder::new(opts).map(|_| ())
}
pub fn report_from_hook_result(
    opts: &ValidationOptions,
    result: &HookResult,
) -> Result<Report, Error> {
    let mut builder = ReportBuilder::new(opts)?;
    add_hook_result(&mut builder, result);
    Ok(builder.report)
}
pub fn merge_reports(opts: &ValidationOptions, reports: &[Report]) -> Result<Report, Error> {
    let mut builder = ReportBuilder::new(opts)?;
    for report in reports {
        for finding in &report.findings {
            builder.add(finding.clone());
        }
        for change in &report.changes {
            builder.change(change.clone());
        }
        builder.report.dropped += report.dropped;
        builder.report.truncated = builder.report.truncated || report.truncated;
        builder.report.findings_dropped =
            builder.report.findings_dropped || report.findings_dropped;
        builder.report.errors_dropped =
            builder.report.errors_dropped || opts.mode != Mode::Warn && report.errors_dropped;
    }
    Ok(builder.report)
}
pub(crate) struct Session<'a> {
    pub(crate) opts: &'a ValidationOptions,
    pub(crate) builder: ReportBuilder,
    visited: i64,
    max_depth: i64,
    max_elements: i64,
    report_only: bool,
    pub(crate) write_budget: Option<super::write::SharedBudget>,
    limit_err: Option<Error>,
}
type Charset = Option<SpecificCharacterSet>;
fn append_path(prefix: &[PathStep], step: PathStep) -> Path {
    let mut p = prefix.to_vec();
    p.push(step);
    p
}
impl<'a> Session<'a> {
    pub(crate) fn new(
        opts: &'a ValidationOptions,
        builder: ReportBuilder,
        report_only: bool,
    ) -> Session<'a> {
        Session {
            opts,
            builder,
            visited: 0,
            max_depth: if opts.max_depth == 0 {
                DEFAULT_MAX_DEPTH
            } else {
                opts.max_depth
            },
            max_elements: if opts.max_elements == 0 {
                DEFAULT_MAX_ELEMENTS
            } else {
                opts.max_elements
            },
            report_only,
            write_budget: None,
            limit_err: None,
        }
    }
    fn stop(&self) -> bool {
        self.builder.should_stop(self.opts)
    }
    fn add(&mut self, path: &[PathStep], element: &Element, code: Code, message: &str) {
        self.add_sev(path, element, code, Severity::Error, message);
    }
    fn add_sev(
        &mut self,
        path: &[PathStep],
        element: &Element,
        code: Code,
        severity: Severity,
        message: &str,
    ) {
        self.builder.add(Finding::new(
            path.to_vec(),
            element.tag(),
            Some(element.vr()),
            code,
            severity,
            message,
        ));
    }
    fn run_element_hook(
        &mut self,
        point: HookPoint,
        path: &[PathStep],
        element: &Element,
    ) -> Result<HookResult, Error> {
        let Some(hooks) = &self.opts.hooks else {
            return Ok(HookResult::default());
        };
        let event = HookEvent {
            point,
            path: path.to_vec(),
            element: Some(element.clone()),
            ..HookEvent::default()
        };
        let (result, err) = hooks.run_inner(&event, self.opts.stop_first);
        add_hook_result(&mut self.builder, &result);
        match err {
            Some(e) => Err(e),
            None => Ok(result),
        }
    }
    pub(crate) fn run_data_set_hook(
        &mut self,
        point: HookPoint,
        path: &[PathStep],
        ds: &DataSet,
    ) -> Result<(), Error> {
        let Some(hooks) = &self.opts.hooks else {
            return Ok(());
        };
        let event = HookEvent {
            point,
            path: path.to_vec(),
            data_set: Some(ds.clone()),
            ..HookEvent::default()
        };
        let (result, err) = hooks.run_inner(&event, self.opts.stop_first);
        add_hook_result(&mut self.builder, &result);
        match err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    fn set_finding_occurrence(&mut self, start: usize, path: &[PathStep], occurrence: usize) {
        let key = path_string(path);
        for f in &mut self.builder.report.findings[start..] {
            if path_string(&f.path) == key {
                f.occurrence = occurrence;
            }
        }
    }
    pub(crate) fn process_elements(
        &mut self,
        ds: &DataSet,
        prefix: &[PathStep],
        depth: i64,
        inherited: &Charset,
    ) -> Result<Vec<Element>, Error> {
        if depth > self.max_depth {
            return Err(Error::Limit(format!("depth exceeds {}", self.max_depth)));
        }
        let charset = resolve_character_set(ds, inherited);
        let mut seen = HashSet::new();
        let mut occurrences: HashMap<Tag, usize> = HashMap::new();
        let mut result: Vec<Element> = Vec::new();
        let mut previous: Option<Tag> = None;
        for original in &ds.elements {
            if let Some(e) = &self.builder.limit_err {
                return Err(e.clone());
            }
            if self.stop() {
                if !self.report_only {
                    result.push(original.clone());
                }
                continue;
            }
            self.visited += 1;
            if self.visited > self.max_elements {
                return Err(Error::Limit(format!(
                    "element count exceeds {}",
                    self.max_elements
                )));
            }
            let mut path = append_path(prefix, PathStep::element(original.tag()));
            let occurrence = *occurrences.get(&original.tag()).unwrap_or(&0);
            occurrences.insert(original.tag(), occurrence + 1);
            let finding_start = self.builder.report.findings.len();
            let mut element = original.clone();
            if !seen.insert(element.tag()) {
                self.add(
                    &path,
                    &element,
                    CODE_DUPLICATE_ELEMENT,
                    "duplicate element in the same data set",
                );
            }
            if previous.is_some_and(|p| element.tag() < p) {
                self.add_sev(
                    &path,
                    &element,
                    CODE_ELEMENT_ORDER,
                    Severity::Warning,
                    "element is encoded out of tag order",
                );
            }
            previous = Some(element.tag());
            if self.stop() {
                self.set_finding_occurrence(finding_start, &path, occurrence);
                if !self.report_only {
                    result.push(element);
                }
                continue;
            }
            let is_sequence = matches!(element.value, Value::Sequence(_));
            if !is_sequence {
                let hook = self.run_element_hook(HookPoint::AfterElement, &path, &element)?;
                if let Some(replacement) = hook.element {
                    element = replacement;
                    path.last_mut().unwrap().tag = element.tag();
                }
                if hook.filter {
                    self.set_finding_occurrence(finding_start, &path, occurrence);
                    continue;
                }
                if self.stop() {
                    self.set_finding_occurrence(finding_start, &path, occurrence);
                    if !self.report_only {
                        result.push(element);
                    }
                    continue;
                }
            }
            self.validate_element(ds, &path, &element, &charset);
            if self.stop() {
                self.set_finding_occurrence(finding_start, &path, occurrence);
                if !self.report_only {
                    result.push(element);
                }
                continue;
            }
            if let Value::Sequence(items) = &element.value {
                let mut new_items = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let item_prefix = append_path(
                        prefix,
                        PathStep {
                            tag: element.tag(),
                            item_index: index as i64,
                        },
                    );
                    let children =
                        self.process_elements(item, &item_prefix, depth + 1, &charset)?;
                    let mut new_item = item.clone();
                    if !self.report_only {
                        new_item.elements = children;
                    }
                    if !self.stop() {
                        self.run_data_set_hook(HookPoint::ItemComplete, &item_prefix, &new_item)?;
                    }
                    new_items.push(new_item);
                }
                element.value = Value::Sequence(new_items);
                if !self.stop() {
                    let seq =
                        self.run_element_hook(HookPoint::SequenceComplete, &path, &element)?;
                    if let Some(replacement) = seq.element {
                        element = replacement;
                        path.last_mut().unwrap().tag = element.tag();
                    }
                    if seq.filter {
                        self.set_finding_occurrence(finding_start, &path, occurrence);
                        continue;
                    }
                    let after = self.run_element_hook(HookPoint::AfterElement, &path, &element)?;
                    if let Some(replacement) = after.element {
                        element = replacement;
                        path.last_mut().unwrap().tag = element.tag();
                    }
                    if after.filter {
                        self.set_finding_occurrence(finding_start, &path, occurrence);
                        continue;
                    }
                }
            }
            self.set_finding_occurrence(finding_start, &path, occurrence);
            if !self.report_only {
                result.push(element);
            }
        }
        if !self.stop() {
            let validated = if self.report_only {
                ds.clone()
            } else {
                DataSet {
                    elements: result.clone(),
                    item_offset: ds.item_offset,
                }
            };
            self.run_data_set_rules(&validated, prefix);
        }
        if let Some(e) = &self.limit_err {
            return Err(e.clone());
        }
        if self.report_only {
            return Ok(ds.elements.clone());
        }
        Ok(result)
    }
    fn run_data_set_rules(&mut self, ds: &DataSet, path: &[PathStep]) {
        let opts = self.opts;
        for registration in &opts.data_set_rules {
            if self.stop() {
                return;
            }
            if let Some(budget) = &self.write_budget {
                let admitted = {
                    let mut b = budget.borrow_mut();
                    b.admit_copy(ds, 1).and_then(|_| {
                        b.admit_event(&HookEvent {
                            path: path.to_vec(),
                            ..HookEvent::default()
                        })
                    })
                };
                if let Err(e) = admitted {
                    self.limit_err = Some(e.clone());
                    self.builder.limit_err = Some(e);
                    return;
                }
            }
            let Some(rule) = &registration.rule else {
                continue;
            };
            let context = DataSetContext {
                data_set: ds.clone(),
                path: path.to_vec(),
                dictionary: opts.dictionary,
                big_endian: opts.big_endian,
                transfer_syntax_uid: opts.transfer_syntax_uid.clone(),
            };
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rule(&context)));
            let Ok(findings) = outcome else {
                let mut f = Finding::new(
                    path.to_vec(),
                    Tag::default(),
                    None,
                    CODE_DATA_SET_RULE,
                    Severity::Warning,
                    "data set rule failed without exposing its error",
                );
                f.rule = registration.name.clone();
                self.builder.add(f);
                continue;
            };
            for rf in findings {
                if let Some(budget) = &self.write_budget {
                    use super::write::{size, write_product, write_scale};
                    let reserved =
                        write_product((path.len() + rf.path.len()) as i64, size::PATH_STEP)
                            .and_then(|b| write_scale(b, 3))
                            .and_then(|b| budget.borrow_mut().reserve(0, b + 128, 0));
                    if let Err(e) = reserved {
                        self.limit_err = Some(e.clone());
                        self.builder.limit_err = Some(e);
                        return;
                    }
                }
                if self.stop() {
                    return;
                }
                let full_path = if rf.path.is_empty() {
                    if rf.tag != Tag::default() {
                        append_path(path, PathStep::element(rf.tag))
                    } else {
                        path.to_vec()
                    }
                } else {
                    let mut p = path.to_vec();
                    p.extend(rf.path.iter().copied());
                    p
                };
                let mut f = Finding::new(
                    full_path,
                    rf.tag,
                    rf.vr,
                    rf.code.unwrap_or(CODE_DATA_SET_RULE),
                    Severity::Error,
                    "data set rule reported a validation finding",
                );
                f.rule = registration.name.clone();
                self.builder.add_with(f, rf.severity);
            }
        }
    }
    fn validate_element(
        &mut self,
        ds: &DataSet,
        path: &[PathStep],
        element: &Element,
        charset: &Charset,
    ) {
        self.validate_dictionary(ds, path, element, charset);
        self.validate_value(path, element, charset);
        if element.tag() == Tag::PIXEL_DATA {
            self.validate_pixel_data(path, element);
        }
    }
    fn validate_dictionary(
        &mut self,
        ds: &DataSet,
        path: &[PathStep],
        element: &Element,
        charset: &Charset,
    ) {
        if !self.opts.dictionary {
            return;
        }
        let Some(entry) = dict::by_tag(element.tag()) else {
            return;
        };
        if let Some(budget) = &self.write_budget {
            let vm_len = dict::vm(element.tag()).unwrap_or("").len() as i64;
            if let Err(e) = budget.borrow_mut().reserve(0, vm_len * 64 + 64 * 16, 0) {
                self.limit_err = Some(e);
                return;
            }
        }
        let mut expected: Vec<Vr> = Vec::new();
        if let Some(resolve) = &self.opts.resolve_vr {
            expected = resolve(
                &ElementContext {
                    element: element.clone(),
                    data_set: ds.clone(),
                    path: path.to_vec(),
                    big_endian: self.opts.big_endian,
                },
                entry,
            );
        }
        if expected.is_empty() {
            expected = dict::vr_values(entry).to_vec();
        }
        if let Some(budget) = &self.write_budget {
            let reserved =
                super::write::write_product(expected.len() as i64, super::write::size::VR)
                    .and_then(|b| budget.borrow_mut().reserve(0, b, 0));
            if let Err(e) = reserved {
                self.limit_err = Some(e.clone());
                self.builder.limit_err = Some(e);
                return;
            }
        }
        if element.vr() != Vr::UN && !expected.is_empty() && !expected.contains(&element.vr()) {
            let mut f = Finding::new(
                path.to_vec(),
                element.tag(),
                Some(element.vr()),
                CODE_DICTIONARY_VR,
                Severity::Error,
                "encoded VR does not match the data dictionary",
            );
            f.expected_vr = expected;
            f.rule = standard_names(element.tag())
                .map(|(keyword, _)| keyword.to_string())
                .unwrap_or_default();
            self.builder.add(f);
        }
        let vm_spec = dict::vm(element.tag()).unwrap_or("");
        if vm_spec.is_empty() {
            return;
        }
        let vm = value_multiplicity(element, charset);
        if vm == 0 {
            return;
        }
        if !matches_vm(vm_spec, vm) {
            let mut f = Finding::new(
                path.to_vec(),
                element.tag(),
                Some(element.vr()),
                CODE_VALUE_MULTIPLICITY,
                Severity::Error,
                "value multiplicity does not match the data dictionary",
            );
            f.rule = vm_spec.to_string();
            self.builder.add(f);
        }
    }
    fn validate_value(&mut self, path: &[PathStep], element: &Element, charset: &Charset) {
        let vr = element.vr();
        let h = &element.header;
        if h.length_set && h.length != UNDEFINED_LENGTH && h.length & 1 != 0 {
            self.add(
                path,
                element,
                CODE_VALUE_LENGTH,
                "encoded value length is odd",
            );
        }
        match &element.value {
            Value::Nil => {}
            Value::Strings(values) => {
                if !vr.is_string_like() {
                    self.add(
                        path,
                        element,
                        CODE_VALUE_KIND,
                        "string value is incompatible with the encoded VR",
                    );
                    return;
                }
                for component in values {
                    self.text_component(path, element, component.as_bytes(), false, charset);
                }
            }
            Value::Raw(raw) => {
                if vr.is_string_like() {
                    let n = raw.len();
                    if n > 0
                        && n % 2 == 0
                        && raw[n - 1] != vr.padding_byte()
                        && (raw[n - 1] == 0 || raw[n - 1] == b' ')
                    {
                        self.add(
                            path,
                            element,
                            CODE_VALUE_PADDING,
                            "value uses the wrong trailing padding byte",
                        );
                    } else if vr == Vr::UI && invalid_uid_padding(raw) {
                        self.add(
                            path,
                            element,
                            CODE_VALUE_PADDING,
                            "UID value carries padding beyond one trailing NUL",
                        );
                    }
                    if vr.uses_specific_character_set() {
                        let Some(decoded) =
                            decode_encoded_text(charset, vr, trim_text_value_bytes(vr, raw))
                        else {
                            self.add(
                                path,
                                element,
                                CODE_VALUE_REPERTOIRE,
                                "text component contains characters outside the VR repertoire",
                            );
                            return;
                        };
                        for component in split_text_multiplicity(vr, &decoded) {
                            self.text_component(
                                path,
                                element,
                                component.as_bytes(),
                                false,
                                charset,
                            );
                        }
                        return;
                    }
                    if raw_text_too_long(vr, raw) {
                        self.add(
                            path,
                            element,
                            CODE_VALUE_LENGTH,
                            "text component exceeds the VR length limit",
                        );
                    }
                    for component in split_raw_multiplicity(vr, raw) {
                        self.text_component(path, element, &component, true, charset);
                    }
                    return;
                }
                if vr == Vr::SQ {
                    self.add(
                        path,
                        element,
                        CODE_VALUE_KIND,
                        "raw value is incompatible with sequence VR",
                    );
                    return;
                }
                let width = binary_width(vr);
                if width > 0 && raw.len() % width != 0 {
                    self.add(
                        path,
                        element,
                        CODE_VALUE_LENGTH,
                        "binary value length is not a multiple of the VR width",
                    );
                }
            }
            Value::Sequence(_) => {
                if vr != Vr::SQ && vr != Vr::UN {
                    self.add(
                        path,
                        element,
                        CODE_SEQUENCE_STRUCTURE,
                        "sequence value requires SQ or implicit-VR UN",
                    );
                }
            }
            Value::Fragments { offset_table, .. } => {
                if element.tag() != Tag::PIXEL_DATA || (vr != Vr::OB && vr != Vr::OW) {
                    self.add(
                        path,
                        element,
                        CODE_FRAGMENT_STRUCTURE,
                        "fragment sequence is not valid for this tag and VR",
                    );
                }
                if offset_table.len() % 4 != 0 {
                    self.add(
                        path,
                        element,
                        CODE_FRAGMENT_STRUCTURE,
                        "basic offset table length is not a multiple of four",
                    );
                }
            }
            Value::BulkData(uri) => {
                if uri.trim().is_empty() {
                    self.add(
                        path,
                        element,
                        CODE_VALUE_FORMAT,
                        "bulk data reference URI is empty",
                    );
                }
            }
        }
    }
    fn text_component(
        &mut self,
        path: &[PathStep],
        element: &Element,
        component: &[u8],
        encoded_raw: bool,
        charset: &Charset,
    ) {
        let vr = element.vr();
        let max = max_text_length(vr);
        if max > 0 && !encoded_raw && text_length(vr, component) > max {
            self.add(
                path,
                element,
                CODE_VALUE_LENGTH,
                "text component exceeds the VR length limit",
            );
        }
        if !valid_text_repertoire(vr, component, encoded_raw)
            || (encoded_raw
                && vr.uses_specific_character_set()
                && !valid_encoded_text(charset, vr, component))
        {
            self.add(
                path,
                element,
                CODE_VALUE_REPERTOIRE,
                "text component contains characters outside the VR repertoire",
            );
        }
        let (valid, range) = validate_text_format(vr, component);
        if !valid {
            let code = if range {
                CODE_VALUE_RANGE
            } else {
                CODE_VALUE_FORMAT
            };
            self.add(
                path,
                element,
                code,
                "text component does not satisfy the VR format",
            );
        }
    }
    fn validate_pixel_data(&mut self, path: &[PathStep], element: &Element) {
        let h = &element.header;
        match &element.value {
            Value::Fragments { .. } => {
                if h.length != UNDEFINED_LENGTH && h.length_set {
                    self.add(
                        path,
                        element,
                        CODE_PIXEL_METADATA,
                        "encapsulated Pixel Data requires undefined length",
                    );
                }
            }
            Value::Raw(_) | Value::Nil if h.length_set && h.length == UNDEFINED_LENGTH => {
                self.add(
                    path,
                    element,
                    CODE_PIXEL_METADATA,
                    "native Pixel Data requires defined length",
                );
            }
            _ => {}
        }
    }
    pub(crate) fn validate_required_uids(&mut self, ds: &DataSet) {
        let opts = self.opts;
        for &tag in &opts.required_uids {
            let value = data_set_string(ds, tag);
            if value.as_deref().is_none_or(|v| v.trim().is_empty()) {
                self.builder.add(Finding::new(
                    vec![PathStep::element(tag)],
                    tag,
                    Some(Vr::UI),
                    CODE_REQUIRED_UID,
                    Severity::Error,
                    "required UID element is missing or empty",
                ));
                if self.stop() {
                    break;
                }
            }
        }
    }
}
pub(crate) fn check_clone_limits(
    ds: &DataSet,
    max_depth: i64,
    max_elements: i64,
) -> Result<(), Error> {
    fn walk(
        ds: &DataSet,
        depth: i64,
        max_depth: i64,
        max: i64,
        visited: &mut i64,
    ) -> Result<(), Error> {
        if depth > max_depth {
            return Err(Error::Limit(format!("depth exceeds {max_depth}")));
        }
        let count_err = || Error::Limit(format!("element/item count exceeds {max}"));
        if ds.elements.len() as i64 > max - *visited {
            return Err(count_err());
        }
        *visited += ds.elements.len() as i64;
        for e in &ds.elements {
            if let Value::Sequence(items) = &e.value {
                if items.len() as i64 > max - *visited {
                    return Err(count_err());
                }
                *visited += items.len() as i64;
                for item in items {
                    walk(item, depth + 1, max_depth, max, visited)?;
                }
            }
        }
        Ok(())
    }
    let mut visited = 0;
    walk(ds, 0, max_depth, max_elements, &mut visited)
}
#[doc(hidden)]
pub fn validate_data_set_with(
    ds: &DataSet,
    opts: &ValidationOptions,
) -> (ValidationResult, Option<Error>) {
    let builder = match ReportBuilder::new(opts) {
        Ok(b) => b,
        Err(e) => return (ValidationResult::default(), Some(e)),
    };
    let mut s = Session::new(opts, builder, false);
    if let Err(e) = check_clone_limits(ds, s.max_depth, s.max_elements) {
        return (ValidationResult::default(), Some(e));
    }
    let mut working = ds.clone();
    if let Err(e) = s.run_data_set_hook(HookPoint::PreValidation, &[], &working) {
        return (
            ValidationResult {
                data_set: working,
                report: s.builder.report,
            },
            Some(e),
        );
    }
    let default_charset = Some(SpecificCharacterSet::default_repertoire());
    let mut err = None;
    match s.process_elements(&working, &[], 0, &default_charset) {
        Ok(elements) => working.elements = elements,
        Err(e) => {
            working.elements = Vec::new();
            err = Some(e);
        }
    }
    if err.is_none() && !s.stop() {
        s.validate_required_uids(&working);
    }
    for point in [HookPoint::DataSetComplete, HookPoint::PostValidation] {
        if s.stop() {
            continue;
        }
        if let Err(e) = s.run_data_set_hook(point, &[], &working)
            && err.is_none()
        {
            err = Some(e);
        }
    }
    let result = ValidationResult {
        data_set: working,
        report: s.builder.report,
    };
    if err.is_some() {
        return (result, err);
    }
    if opts.mode == Mode::Strict && result.report.has_errors() {
        let report = result.report.clone();
        return (result, Some(Error::Failed(report)));
    }
    (result, None)
}
pub fn validate_element_with(
    element: &Element,
    opts: &ValidationOptions,
) -> (Report, Option<Error>) {
    let (result, err) = validate_data_set_with(
        &DataSet {
            elements: vec![element.clone()],
            item_offset: None,
        },
        opts,
    );
    (result.report, err)
}
pub fn validate_file(
    meta: &DataSet,
    ds: &DataSet,
    syntax_uid: &str,
    opts: &ValidationOptions,
) -> (Report, Option<Error>) {
    let mut opts = opts.clone();
    opts.transfer_syntax_uid = syntax_uid.to_string();
    let mut meta_opts = opts.clone();
    meta_opts.required_uids = Vec::new();
    let (meta_result, err) = validate_data_set_with(meta, &meta_opts);
    if let Some(e) = err
        && !e.is_validation_failed()
    {
        return (meta_result.report, Some(e));
    }
    if opts.stop_first && meta_result.report.has_findings() {
        if opts.mode == Mode::Strict && meta_result.report.has_errors() {
            let report = meta_result.report.clone();
            return (meta_result.report, Some(Error::Failed(report)));
        }
        return (meta_result.report, None);
    }
    let (data_result, data_err) = validate_data_set_with(ds, &opts);
    if let Some(e) = data_err
        && !e.is_validation_failed()
    {
        let merged =
            merge_reports(&opts, &[meta_result.report, data_result.report]).unwrap_or_default();
        return (merged, Some(e));
    }
    let consistency =
        validate_file_meta_consistency(&meta_result.data_set, &data_result.data_set, syntax_uid);
    let report = merge_reports(
        &opts,
        &[meta_result.report, data_result.report, consistency],
    )
    .unwrap_or_default();
    if opts.mode == Mode::Strict && report.has_errors() {
        let failed = report.clone();
        return (report, Some(Error::Failed(failed)));
    }
    (report, None)
}
pub fn validate_file_meta_consistency(meta: &DataSet, ds: &DataSet, syntax_uid: &str) -> Report {
    let mut report = Report::default();
    for (meta_tag, data_tag) in [
        (Tag::new(0x0002, 0x0002), Tag::new(0x0008, 0x0016)),
        (Tag::new(0x0002, 0x0003), Tag::new(0x0008, 0x0018)),
    ] {
        let m = data_set_string(meta, meta_tag);
        let d = data_set_string(ds, data_tag);
        let mu = m.as_deref().map(normalize_uid).unwrap_or("");
        let du = d.as_deref().map(normalize_uid).unwrap_or("");
        if m.is_none() || d.is_none() || mu.is_empty() || du.is_empty() || mu != du {
            report.findings.push(Finding::new(
                vec![PathStep::element(meta_tag)],
                meta_tag,
                Some(Vr::UI),
                CODE_FILE_META_MISMATCH,
                Severity::Error,
                "File Meta UID does not match the data set",
            ));
        }
    }
    let tag = Tag::new(0x0002, 0x0010);
    let declared = data_set_string(meta, tag);
    let du = declared.as_deref().map(normalize_uid).unwrap_or("");
    let eu = normalize_uid(syntax_uid);
    if declared.is_none() || du.is_empty() || eu.is_empty() || du != eu {
        report.findings.push(Finding::new(
            vec![PathStep::element(tag)],
            tag,
            Some(Vr::UI),
            CODE_FILE_META_MISMATCH,
            Severity::Error,
            "File Meta Transfer Syntax UID does not match the supplied syntax",
        ));
    }
    report
}
pub(crate) fn data_set_string(ds: &DataSet, tag: Tag) -> Option<String> {
    let e = ds.elements.iter().rev().find(|e| e.tag() == tag)?;
    Some(element_string_value(e))
}
pub(crate) fn element_string_value(e: &Element) -> String {
    use crate::object::text::trim_text_value;
    match &e.value {
        Value::Raw(raw) => String::from_utf8_lossy(trim_text_value_bytes(e.vr(), raw)).into_owned(),
        Value::Strings(values) => values
            .first()
            .map(|v| trim_text_value(e.vr(), v).to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}
fn numeric_width(vr: Vr) -> usize {
    match vr {
        Vr::US | Vr::SS => 2,
        Vr::AT | Vr::FL | Vr::SL | Vr::UL => 4,
        Vr::FD | Vr::SV | Vr::UV => 8,
        _ => 0,
    }
}
fn value_multiplicity(element: &Element, charset: &Charset) -> usize {
    let vr = element.vr();
    match &element.value {
        Value::Nil => 0,
        Value::Strings(values) => values.len(),
        Value::Raw(raw) => {
            if raw.is_empty() {
                return 0;
            }
            if vr.is_string_like() {
                if vr.uses_specific_character_set() && charset.is_some() {
                    return match decode_encoded_text(charset, vr, trim_text_value_bytes(vr, raw)) {
                        Some(decoded) => split_text_multiplicity(vr, &decoded).len(),
                        None => 1,
                    };
                }
                return split_raw_multiplicity(vr, raw).len();
            }
            match numeric_width(vr) {
                0 => 1,
                w => raw.len() / w,
            }
        }
        _ => 1,
    }
}
pub(crate) fn matches_vm(spec: &str, count: usize) -> bool {
    let count = count as i64;
    let parts: Vec<&str> = spec.split('-').collect();
    if parts.len() > 2 {
        return true;
    }
    let Some(minimum) = parse_decimal_int(parts[0]) else {
        return true;
    };
    if minimum <= 0 {
        return true;
    }
    if parts.len() == 1 {
        return count == minimum;
    }
    let maximum = parts[1];
    if maximum == "n" {
        return count >= minimum;
    }
    if let Some(m) = maximum.strip_suffix('n') {
        let Some(multiple) = parse_decimal_int(m) else {
            return true;
        };
        if multiple <= 0 {
            return true;
        }
        return count >= minimum && count % multiple == 0;
    }
    let Some(max) = parse_decimal_int(maximum) else {
        return true;
    };
    if max < minimum {
        return true;
    }
    count >= minimum && count <= max
}
fn parse_decimal_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}
impl ChangeKindCode for super::ChangeKind {
    fn code_message(self) -> (Code, &'static str) {
        use super::ChangeKind::*;
        use super::{
            CODE_HOOK_DEFERRED, CODE_HOOK_FILTERED, CODE_HOOK_SKIPPED, CODE_HOOK_TRANSFORMED,
        };
        match self {
            Transformed => (
                CODE_HOOK_TRANSFORMED,
                "hook explicitly transformed an element",
            ),
            Filtered => (CODE_HOOK_FILTERED, "hook explicitly filtered an element"),
            Skipped => (
                CODE_HOOK_SKIPPED,
                "hook explicitly skipped an element value",
            ),
            Deferred => (
                CODE_HOOK_DEFERRED,
                "hook explicitly deferred an element value",
            ),
        }
    }
}
