#![forbid(unsafe_code)]
use crate::object::types::length_string;
use crate::object::{
    self, Element, File, Kind, Object, ReadFileOptions, SummaryOptions, Tag, UNDEFINED_LENGTH,
    Value, WalkOptions,
};
use crate::transfer;
use crate::validation::{self, Mode, Report, Severity, ValidationOptions};
use std::fmt;
pub const DEFAULT_MAX_TOTAL_BYTES: i64 = 512 << 20;
pub const DEFAULT_MAX_ELEMENT_BYTES: i64 = 32 << 20;
pub const DEFAULT_MAX_ELEMENTS: i64 = 200_000;
pub const DEFAULT_MAX_SEQUENCE_DEPTH: i64 = 64;
pub const DEFAULT_MAX_FINDINGS: i64 = 256;
pub const DEFAULT_TIMEOUT_NANOS: i64 = 15_000_000_000;
pub const CODE_CANCELED: &str = "inspection.canceled";
pub const CODE_TIMEOUT: &str = "inspection.timeout";
pub const CODE_RESOURCE_LIMIT: &str = "inspection.resource_limit";
pub const CODE_MALFORMED: &str = "inspection.malformed";
const ERR_INVALID_OPTIONS: &str = "dicom inspection options are invalid";
const ERR_RESOURCE_LIMIT: &str = "dicom inspection resource limit exceeded";
const ERR_MALFORMED: &str = "dicom input is malformed or unsupported";
const ERR_MISSING_DATASET: &str = "dicom input does not contain a data set";
const TEXT_MAX_TOTAL_BYTES: &str = "dicom: maximum total bytes exceeded";
const TEXT_MAX_ELEMENT_BYTES: &str = "dicom: maximum element value size exceeded";
const TEXT_MAX_ELEMENTS: &str = "dicom: maximum element count exceeded";
const LIMIT_TEXTS: [&str; 6] = [
    TEXT_MAX_TOTAL_BYTES,
    TEXT_MAX_ELEMENT_BYTES,
    TEXT_MAX_ELEMENTS,
    "dicom: maximum sequence nesting depth exceeded",
    "dicom: maximum fragment count exceeded",
    "dicom: maximum Pixel Data size exceeded",
];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidOptions,
    ResourceLimit,
    Malformed,
    NilFile,
    Open,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    message: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub max_total_bytes: i64,
    pub max_element_bytes: i64,
    pub max_elements: i64,
    pub max_sequence_depth: i64,
    pub max_findings: i64,
    pub timeout_nanos: i64,
}
impl Options {
    pub fn defaults() -> Options {
        Options {
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_element_bytes: DEFAULT_MAX_ELEMENT_BYTES,
            max_elements: DEFAULT_MAX_ELEMENTS,
            max_sequence_depth: DEFAULT_MAX_SEQUENCE_DEPTH,
            max_findings: DEFAULT_MAX_FINDINGS,
            timeout_nanos: DEFAULT_TIMEOUT_NANOS,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    File,
    Meta,
    Dataset,
}
impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::File => "file",
            Source::Meta => "meta",
            Source::Dataset => "dataset",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub source: Source,
    pub tag: String,
    pub path: String,
    pub code: String,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementSummary {
    pub source: Source,
    pub path: String,
    pub tag: String,
    pub vr: String,
    pub keyword: String,
    pub name: String,
    pub length: String,
    pub value: Vec<u8>,
    pub private: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub file_name: String,
    pub transfer_syntax_uid: String,
    pub transfer_syntax: String,
    pub payload_kind: String,
    pub media_type: String,
    pub patient_name: String,
    pub patient_id: String,
    pub issuer_of_patient_id: String,
    pub patient_birth_date: String,
    pub institution_name: String,
    pub study_date: String,
    pub study_time: String,
    pub series_date: String,
    pub series_time: String,
    pub study_description: String,
    pub modality: String,
    pub accession_number: String,
    pub series_description: String,
    pub protocol_name: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub series_number: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub instance_number: String,
    pub study_id: String,
    pub body_part_examined: String,
    pub referring_physician_name: String,
    pub performing_physician_name: String,
    pub element_count: i64,
    pub elements: Vec<ElementSummary>,
    pub finding_count: i64,
    pub findings: Vec<Finding>,
    pub findings_truncated: bool,
    pub dropped_findings: i64,
}
pub fn validate_options(opts: &Options) -> Result<(), Error> {
    if opts.max_total_bytes < 0
        || opts.max_element_bytes < 0
        || opts.max_elements < 0
        || opts.max_sequence_depth < 0
        || opts.max_findings < 0
        || opts.timeout_nanos < 0
    {
        return Err(Error {
            kind: ErrorKind::InvalidOptions,
            message: ERR_INVALID_OPTIONS.to_string(),
        });
    }
    Ok(())
}
fn normalize_options(opts: Options) -> Result<Options, Error> {
    if validate_options(&opts).is_err() {
        return Err(Error {
            kind: ErrorKind::InvalidOptions,
            message: format!("inspect DICOM file: {ERR_INVALID_OPTIONS}"),
        });
    }
    let defaults = Options::defaults();
    let pick = |value: i64, default: i64| if value == 0 { default } else { value };
    Ok(Options {
        max_total_bytes: pick(opts.max_total_bytes, defaults.max_total_bytes),
        max_element_bytes: pick(opts.max_element_bytes, defaults.max_element_bytes),
        max_elements: pick(opts.max_elements, defaults.max_elements),
        max_sequence_depth: pick(opts.max_sequence_depth, defaults.max_sequence_depth),
        max_findings: pick(opts.max_findings, defaults.max_findings),
        timeout_nanos: pick(opts.timeout_nanos, defaults.timeout_nanos),
    })
}
struct Failure {
    limit: bool,
    text: String,
}
impl Failure {
    fn object(err: &object::Error) -> Failure {
        let text = err.to_string();
        let limit = err.is(Kind::MaxTotalBytes)
            || err.is(Kind::WalkResourceLimit)
            || LIMIT_TEXTS.iter().any(|t| text.contains(t));
        Failure { limit, text }
    }
    fn sentinel(text: &str, limit: bool) -> Failure {
        Failure {
            limit,
            text: text.to_string(),
        }
    }
}
fn classify(failure: &Failure) -> (Finding, Error) {
    let (code, message, kind, sentinel) = if failure.limit {
        (
            CODE_RESOURCE_LIMIT,
            "inspection stopped at a configured resource limit",
            ErrorKind::ResourceLimit,
            ERR_RESOURCE_LIMIT,
        )
    } else {
        (
            CODE_MALFORMED,
            "the input could not be parsed as a trustworthy DICOM data set",
            ErrorKind::Malformed,
            ERR_MALFORMED,
        )
    };
    (
        Finding {
            severity: Severity::Error,
            source: Source::File,
            tag: String::new(),
            path: String::new(),
            code: code.to_string(),
            message: message.to_string(),
        },
        Error {
            kind,
            message: format!("inspect DICOM file: {sentinel}\n{}", failure.text),
        },
    )
}
fn failure_summary(file_name: &str, failure: &Failure) -> (Summary, Option<Error>) {
    let (finding, err) = classify(failure);
    (
        Summary {
            file_name: file_name.to_string(),
            findings: vec![finding],
            finding_count: 1,
            ..Summary::default()
        },
        Some(err),
    )
}
fn read_options(opts: &Options) -> ReadFileOptions {
    ReadFileOptions {
        max_total_bytes: opts.max_total_bytes,
        max_element_bytes: opts.max_element_bytes,
        max_elements: opts.max_elements,
        max_sequence_depth: opts.max_sequence_depth,
        skip_pixel_data: true,
        ..ReadFileOptions::default()
    }
}
#[doc(hidden)]
pub fn inspect_bytes(
    file_name: &str,
    data: Option<&[u8]>,
    opts: Options,
) -> (Summary, Option<Error>) {
    let normalized = match normalize_options(opts) {
        Ok(normalized) => normalized,
        Err(err) => return (Summary::default(), Some(err)),
    };
    let Some(data) = data else {
        return failure_summary(file_name, &Failure::sentinel(ERR_MISSING_DATASET, false));
    };
    match object::read_file_with_options(data, read_options(&normalized)) {
        Ok(file) => summarize(file_name, &file, &normalized),
        Err((_, err)) => failure_summary(file_name, &Failure::object(&err)),
    }
}
pub fn inspect_file(path: impl AsRef<std::path::Path>, opts: Options) -> (Summary, Option<Error>) {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    if let Err(err) = normalize_options(opts) {
        return (Summary::default(), Some(err));
    }
    let data = match std::fs::read(path) {
        Ok(data) => data,
        Err(err) => {
            let text = err.to_string();
            let text = text
                .rfind(" (os error ")
                .map_or(text.as_str(), |i| &text[..i]);
            return (
                Summary::default(),
                Some(Error {
                    kind: ErrorKind::Open,
                    message: format!("open DICOM file: open {path}: {text}"),
                }),
            );
        }
    };
    let base = std::path::Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    inspect_bytes(&base, Some(&data), opts)
}
pub fn summarize_file(
    file_name: &str,
    file: Option<&File>,
    opts: Options,
) -> (Summary, Option<Error>) {
    let Some(file) = file else {
        return (
            Summary::default(),
            Some(Error {
                kind: ErrorKind::NilFile,
                message: "inspect DICOM file: dicom: nil file".to_string(),
            }),
        );
    };
    match normalize_options(opts) {
        Ok(normalized) => summarize(file_name, file, &normalized),
        Err(err) => (Summary::default(), Some(err)),
    }
}
fn summarize(file_name: &str, file: &File, opts: &Options) -> (Summary, Option<Error>) {
    if let Err(failure) = validate_traversal(file, opts) {
        return failure_summary(file_name, &failure);
    }
    let metadata = file.metadata();
    let mut summary = Summary {
        file_name: file_name.to_string(),
        transfer_syntax_uid: metadata.transfer_syntax_uid.clone(),
        transfer_syntax: metadata.transfer_syntax_name,
        payload_kind: "image".to_string(),
        patient_name: metadata.patient_name,
        patient_id: metadata.patient_id,
        issuer_of_patient_id: metadata.issuer_of_patient_id.to_string(),
        patient_birth_date: metadata.patient_birth_date,
        institution_name: metadata.institution_name,
        study_date: metadata.study_date,
        study_time: metadata.study_time,
        series_date: metadata.series_date,
        series_time: metadata.series_time,
        study_description: metadata.study_description,
        modality: metadata.modality,
        accession_number: metadata.accession_number,
        series_description: metadata.series_description,
        protocol_name: metadata.protocol_name,
        study_instance_uid: metadata.study_instance_uid,
        series_instance_uid: metadata.series_instance_uid,
        series_number: metadata.series_number,
        sop_class_uid: metadata.sop_class_uid,
        sop_instance_uid: metadata.sop_instance_uid,
        instance_number: metadata.instance_number,
        study_id: metadata.study_id,
        body_part_examined: metadata.body_part_examined,
        referring_physician_name: metadata.referring_physician_name,
        performing_physician_name: metadata.performing_physician_name,
        ..Summary::default()
    };
    if transfer::is_video_transfer_syntax(&metadata.transfer_syntax_uid) {
        summary.payload_kind = "media".to_string();
        summary.media_type = "video".to_string();
    }
    let mut visited = 0i64;
    for (source, obj) in [(Source::Meta, &file.meta), (Source::Dataset, &file.dataset)] {
        if let Err(failure) = summarize_object(
            source,
            obj,
            "",
            0,
            opts,
            &mut visited,
            &mut summary.elements,
        ) {
            return failure_summary(file_name, &failure);
        }
    }
    summary.element_count = summary.elements.len() as i64;
    if let Err(failure) = append_conformance_findings(file, opts, &mut summary) {
        let (finding, err) = classify(&failure);
        append_operational_finding(&mut summary, finding, opts.max_findings);
        return (summary, Some(err));
    }
    (summary, None)
}
fn validate_traversal(file: &File, opts: &Options) -> Result<(), Failure> {
    let mut visited = 0i64;
    let mut total_bytes = 0i64;
    for obj in [&file.meta, &file.dataset] {
        if obj.is_empty() {
            continue;
        }
        let mut stopped: Option<Failure> = None;
        let walked = obj.walk_with(
            WalkOptions {
                max_depth: opts.max_sequence_depth,
                max_elements: opts.max_elements,
            },
            |_, element| {
                visited += 1;
                let checked = if visited > opts.max_elements {
                    Err(TEXT_MAX_ELEMENTS)
                } else {
                    bounded_element_bytes(element, opts.max_element_bytes).and_then(|size| {
                        if size > opts.max_total_bytes - total_bytes - 12 {
                            return Err(TEXT_MAX_TOTAL_BYTES);
                        }
                        total_bytes += size + 12;
                        Ok(())
                    })
                };
                checked.map_err(|text| {
                    stopped = Some(Failure::sentinel(text, true));
                    object::Error::msg(text)
                })
            },
        );
        if let Some(failure) = stopped {
            return Err(failure);
        }
        if let Err(err) = walked {
            return Err(Failure::object(&err));
        }
    }
    Ok(())
}
fn bounded_element_bytes(element: &Element, max: i64) -> Result<i64, &'static str> {
    let mut size = 0i64;
    match &element.value {
        Value::Nil => {
            if element.header.length != UNDEFINED_LENGTH {
                size = i64::from(element.header.length);
            }
        }
        Value::Raw(raw) => size = raw.len() as i64,
        Value::Strings(values) => {
            let mut total: usize = values.iter().map(String::len).sum();
            if !values.is_empty() {
                total += values.len() - 1;
            }
            size = (total + total % 2) as i64;
        }
        Value::Fragments {
            offset_table,
            fragments,
        } => {
            size = offset_table.len() as i64;
            for fragment in fragments {
                if fragment.len() as i64 > max - size {
                    return Err(TEXT_MAX_ELEMENT_BYTES);
                }
                size += fragment.len() as i64;
            }
        }
        Value::Sequence(_) | Value::BulkData(_) => {}
    }
    if size > max {
        return Err(TEXT_MAX_ELEMENT_BYTES);
    }
    Ok(size)
}
fn walk_limit(detail: &str) -> Failure {
    Failure::sentinel(
        &format!("dicom: Walk resource limit exceeded: {detail}"),
        true,
    )
}
fn summarize_object(
    source: Source,
    obj: &Object,
    prefix: &str,
    depth: i64,
    opts: &Options,
    visited: &mut i64,
    out: &mut Vec<ElementSummary>,
) -> Result<(), Failure> {
    if depth > opts.max_sequence_depth {
        return Err(walk_limit("sequence depth exceeds configured maximum"));
    }
    let elements = obj.sorted_elements();
    let rows = obj.summarize_elements(&SummaryOptions {
        source: source.as_str().to_string(),
        ..SummaryOptions::default()
    });
    for (i, row) in rows.into_iter().enumerate() {
        *visited += 1;
        if *visited > opts.max_elements {
            return Err(walk_limit("element count exceeds configured maximum"));
        }
        let tag = row.tag.to_string();
        let path = if prefix.is_empty() {
            tag.clone()
        } else {
            format!("{prefix}/{tag}")
        };
        out.push(ElementSummary {
            source,
            path: path.clone(),
            tag,
            vr: row.vr.as_str().to_string(),
            keyword: row.keyword,
            name: row.name,
            length: length_string(row.length),
            value: row.value,
            private: row.private,
        });
        let Some(Value::Sequence(items)) = elements.get(i).map(|e| &e.value) else {
            continue;
        };
        for (item_index, item) in items.iter().enumerate() {
            let mut child = Object::from_elements(item.elements.clone());
            child.set_value_big_endian(obj.value_big_endian());
            summarize_object(
                source,
                &child,
                &format!("{path}[{item_index}]"),
                depth + 1,
                opts,
                visited,
                out,
            )?;
        }
    }
    Ok(())
}
fn validation_failure(err: &validation::Error) -> Failure {
    Failure {
        limit: matches!(err, validation::Error::Limit(_)),
        text: err.to_string(),
    }
}
fn append_conformance_findings(
    file: &File,
    opts: &Options,
    summary: &mut Summary,
) -> Result<(), Failure> {
    let mut validation_opts = ValidationOptions {
        mode: Mode::Preserve,
        dictionary: true,
        max_findings: opts.max_findings,
        max_depth: opts.max_sequence_depth,
        max_elements: opts.max_elements,
        big_endian: file.transfer_syntax.big_endian,
        transfer_syntax_uid: file.transfer_syntax.uid.to_string(),
        ..ValidationOptions::default()
    };
    let meta = file.meta.to_data_set();
    let dataset = file.dataset.to_data_set();
    let (result, err) = validation::validate_data_set_with(&meta, &validation_opts);
    append_validation_report(summary, &result.report, Source::Meta, opts.max_findings);
    if let Some(err) = err {
        return Err(validation_failure(&err));
    }
    validation_opts.required_uids = vec![Tag::new(0x0008, 0x0016), Tag::new(0x0008, 0x0018)];
    let (result, err) = validation::validate_data_set_with(&dataset, &validation_opts);
    append_validation_report(summary, &result.report, Source::Dataset, opts.max_findings);
    if let Some(err) = err {
        return Err(validation_failure(&err));
    }
    let report =
        validation::validate_file_meta_consistency(&meta, &dataset, file.transfer_syntax.uid);
    append_validation_report(summary, &report, Source::Meta, opts.max_findings);
    Ok(())
}
fn append_validation_report(summary: &mut Summary, report: &Report, source: Source, max: i64) {
    for finding in &report.findings {
        if summary.findings.len() as i64 >= max {
            summary.findings_truncated = true;
            summary.dropped_findings += 1;
            continue;
        }
        summary.findings.push(Finding {
            severity: finding.severity,
            source,
            tag: if finding.tag.is_zero() {
                String::new()
            } else {
                finding.tag.to_string()
            },
            path: validation::path_string(&finding.path),
            code: finding.code.to_string(),
            message: finding.message.clone(),
        });
    }
    summary.finding_count = summary.findings.len() as i64;
    summary.findings_truncated = summary.findings_truncated || report.truncated;
    summary.dropped_findings += report.dropped;
}
fn append_operational_finding(summary: &mut Summary, finding: Finding, max: i64) {
    if (summary.findings.len() as i64) < max {
        summary.findings.push(finding);
    } else if let Some(last) = usize::try_from(max - 1)
        .ok()
        .and_then(|i| summary.findings.get_mut(i))
    {
        *last = finding;
        summary.findings_truncated = true;
        summary.dropped_findings += 1;
    }
    summary.finding_count = summary.findings.len() as i64;
}
