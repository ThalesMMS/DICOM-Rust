#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]
mod persistent;
mod scanner;
use crate::object::error::{Error as ObjectError, Kind as ObjectKind, PathStep};
use crate::object::inflate::{self, Inflated};
use crate::object::parser::source::{ByteRead, MemFile};
use crate::object::parser::{
    Disposition, ERR_MAX_SELECTIVE_TOKENS, ReaderOptions, SelectiveReader, SelectiveToken,
};
use crate::object::{
    Element, ElementHeader, Object, Part10Header, ReadFileOptions, Syntax, Tag, Vr,
    read_part10_header_mem, transfer,
};
pub use persistent::{
    ERR_PERSISTENT_INDEX, PersistentError, PersistentRecord, PersistentStore,
    PersistentStoreOptions, persistent_key,
};
pub use scanner::{
    MAX_SCAN_QUEUE_DEPTH, MAX_SCAN_WORKERS, ScanError, ScanErrorCode, ScanFilter, ScanOptions,
    ScanResult, SymlinkPolicy, scan,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
pub const DEFAULT_MAX_TOTAL_BYTES: i64 = 512 << 20;
pub const DEFAULT_MAX_SELECTED_BYTES: i64 = 1 << 20;
pub const DEFAULT_MAX_ELEMENTS: i64 = 200_000;
pub const DEFAULT_MAX_SEQUENCE_DEPTH: i64 = 64;
pub const DEFAULT_MAX_TOKENS: i64 = 1_000_000;
pub const DEFAULT_MAX_FRAGMENTS: i64 = 200_000;
pub const DEFAULT_MAX_SELECTED_VALUES: i64 = 4096;
pub const DEFAULT_MAX_DIAGNOSTICS: i64 = 256;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sentinel {
    InvalidOptions,
    Read,
    Close,
    ResourceLimit,
    Selector,
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        match self {
            Sentinel::InvalidOptions => "dicom index options are invalid",
            Sentinel::Read => "dicom index read failed",
            Sentinel::Close => "dicom index source close failed",
            Sentinel::ResourceLimit => "dicom index resource limit exceeded",
            Sentinel::Selector => "dicom index selector failed",
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum External {
    NotExist,
    Permission,
    Exist,
    Invalid,
}
pub type Profile = u32;
pub const PROFILE_CORE: Profile = 1;
pub const PROFILE_PATIENT: Profile = 2;
pub const PROFILE_DESCRIPTIONS: Profile = 4;
pub const PROFILE_GEOMETRY: Profile = 8;
pub const PROFILE_NONE: Profile = 1 << 31;
pub const PROFILE_ALL: Profile =
    PROFILE_CORE | PROFILE_PATIENT | PROFILE_DESCRIPTIONS | PROFILE_GEOMETRY;
#[derive(Clone, Default, PartialEq, Debug)]
pub struct SourceInfo {
    pub origin: String,
    pub size: i64,
}
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Patient {
    pub name: String,
    pub id: String,
    pub birth_date: String,
    pub sex: String,
}
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Study {
    pub instance_uid: String,
    pub accession_number: String,
    pub date: String,
    pub time: String,
    pub description: String,
}
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Series {
    pub instance_uid: String,
    pub modality: String,
    pub number: String,
    pub date: String,
    pub time: String,
    pub description: String,
}
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Instance {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub number: String,
    pub number_of_frames: i64,
}
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Geometry {
    pub rows: i64,
    pub columns: i64,
    pub pixel_spacing: Option<Vec<f64>>,
    pub image_position_patient: Option<Vec<f64>>,
    pub image_orientation_patient: Option<Vec<f64>>,
    pub slice_thickness: f64,
}
#[derive(Clone, Default, PartialEq, Debug)]
pub struct FileMeta {
    pub transfer_syntax_uid: String,
    pub transfer_syntax_name: String,
    pub media_storage_sop_class_uid: String,
    pub media_storage_sop_instance_uid: String,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagnosticCode {
    RawDataSet,
    MissingField,
    InvalidValue,
    Limit,
}
impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticCode::RawDataSet => "raw_dataset",
            DiagnosticCode::MissingField => "missing_field",
            DiagnosticCode::InvalidValue => "invalid_value",
            DiagnosticCode::Limit => "resource_limit",
        }
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub field: String,
    pub tag: Tag,
    pub offset: i64,
    pub offset_set: bool,
    pub message: String,
}
#[derive(Clone, PartialEq, Debug)]
pub struct MissingField {
    pub field: String,
    pub tag: Tag,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Occurrence {
    #[default]
    Last,
    First,
    All,
    Other(u8),
}
#[derive(Clone, PartialEq, Debug)]
pub struct SelectedElement {
    pub path: Vec<PathStep>,
    pub offset: i64,
    pub element: Element,
}
pub type SelectorHandler = Rc<dyn Fn(&SelectedElement) -> Result<(), ()>>;
#[derive(Clone, Default)]
pub struct Selector {
    pub id: String,
    pub path: Vec<Tag>,
    pub tag: Tag,
    pub occurrence: Occurrence,
    pub max_values: i64,
    pub max_value_bytes: i64,
    pub handle: Option<SelectorHandler>,
}
impl fmt::Debug for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Selector")
            .field("id", &self.id)
            .field("path", &self.path)
            .field("tag", &self.tag)
            .field("occurrence", &self.occurrence)
            .field("max_values", &self.max_values)
            .field("max_value_bytes", &self.max_value_bytes)
            .field("handle", &self.handle.is_some())
            .finish()
    }
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Limits {
    pub max_total_bytes: i64,
    pub max_selected_value_bytes: i64,
    pub max_elements: i64,
    pub max_sequence_depth: i64,
    pub max_tokens: i64,
    pub max_fragments: i64,
    pub max_selected_values: i64,
    pub max_diagnostics: i64,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum SequencePruning {
    #[default]
    Strict,
    Opaque,
    Other(u8),
}
#[derive(Clone, Default, Debug)]
pub struct Options {
    pub assume_sorted_tags: bool,
    pub sequence_pruning: SequencePruning,
    pub profile: Profile,
    pub selectors: Vec<Selector>,
    pub limits: Limits,
    pub raw_data_set: Option<String>,
    pub strict_reserved_bytes: bool,
    pub accept_odd_length: bool,
    pub allow_missing_meta_element_group_length: bool,
}
pub fn default_options() -> Options {
    Options {
        profile: PROFILE_CORE,
        limits: default_limits(),
        ..Default::default()
    }
}
fn default_limits() -> Limits {
    Limits {
        max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
        max_selected_value_bytes: DEFAULT_MAX_SELECTED_BYTES,
        max_elements: DEFAULT_MAX_ELEMENTS,
        max_sequence_depth: DEFAULT_MAX_SEQUENCE_DEPTH,
        max_tokens: DEFAULT_MAX_TOKENS,
        max_fragments: DEFAULT_MAX_FRAGMENTS,
        max_selected_values: DEFAULT_MAX_SELECTED_VALUES,
        max_diagnostics: DEFAULT_MAX_DIAGNOSTICS,
    }
}
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Record {
    pub source: SourceInfo,
    pub file_meta: FileMeta,
    pub patient: Option<Patient>,
    pub study: Study,
    pub series: Series,
    pub instance: Instance,
    pub geometry: Option<Geometry>,
    pub selected: BTreeMap<String, Vec<SelectedElement>>,
    pub missing: Vec<MissingField>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Default, PartialEq, Debug)]
pub struct IndexResult {
    pub record: Record,
    pub bytes_scanned: i64,
    pub stopped_before_pixel_data: bool,
    pub stopped_after_requested_tags: bool,
    pub stopped_header: Option<ElementHeader>,
    pub raw_data_set: bool,
}
#[derive(Clone, PartialEq, Debug)]
pub struct Error {
    text: String,
    pub stage: Option<String>,
    pub tag: Tag,
    pub offset: i64,
    pub offset_set: bool,
    pub code: Option<DiagnosticCode>,
    pub sentinels: Vec<Sentinel>,
    pub external: Option<External>,
    pub cause: Option<ObjectError>,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
impl Error {
    fn options(suffix: &str) -> Error {
        Error {
            text: format!("{}: {suffix}", Sentinel::InvalidOptions.text()),
            stage: None,
            tag: Tag::default(),
            offset: 0,
            offset_set: false,
            code: None,
            sentinels: vec![Sentinel::InvalidOptions],
            external: None,
            cause: None,
        }
    }
    fn staged(stage: &str) -> Error {
        let stage_text = if stage.trim().is_empty() {
            "input"
        } else {
            stage.trim()
        };
        Error {
            text: format!("dicom index {stage_text} failed"),
            stage: Some(stage.to_string()),
            tag: Tag::default(),
            offset: 0,
            offset_set: false,
            code: None,
            sentinels: vec![Sentinel::Read],
            external: None,
            cause: None,
        }
    }
    fn with(mut self, s: Sentinel) -> Error {
        if !self.sentinels.contains(&s) {
            self.sentinels.push(s);
        }
        self
    }
    fn with_cause(mut self, cause: ObjectError) -> Error {
        self.cause = Some(cause);
        self
    }
    fn limit(mut self) -> Error {
        self.code = Some(DiagnosticCode::Limit);
        self.with(Sentinel::ResourceLimit)
    }
    pub fn is(&self, s: Sentinel) -> bool {
        self.sentinels.contains(&s)
    }
    pub fn is_kind(&self, kind: ObjectKind) -> bool {
        self.cause.as_ref().is_some_and(|c| c.is(kind))
    }
    #[doc(hidden)]
    pub fn is_parser_resource_limit(&self) -> bool {
        self.cause.as_ref().is_some_and(is_parser_resource_limit)
    }
}
const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
mod tags {
    use crate::object::Tag;
    pub const STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
    pub const SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
    pub const MODALITY: Tag = Tag::new(0x0008, 0x0060);
    pub const SERIES_NUMBER: Tag = Tag::new(0x0020, 0x0011);
    pub const SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
    pub const SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
    pub const INSTANCE_NUMBER: Tag = Tag::new(0x0020, 0x0013);
    pub const NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
    pub const PATIENT_NAME: Tag = Tag::new(0x0010, 0x0010);
    pub const PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
    pub const PATIENT_BIRTH_DATE: Tag = Tag::new(0x0010, 0x0030);
    pub const PATIENT_SEX: Tag = Tag::new(0x0010, 0x0040);
    pub const ACCESSION_NUMBER: Tag = Tag::new(0x0008, 0x0050);
    pub const STUDY_DATE: Tag = Tag::new(0x0008, 0x0020);
    pub const STUDY_TIME: Tag = Tag::new(0x0008, 0x0030);
    pub const STUDY_DESCRIPTION: Tag = Tag::new(0x0008, 0x1030);
    pub const SERIES_DATE: Tag = Tag::new(0x0008, 0x0021);
    pub const SERIES_TIME: Tag = Tag::new(0x0008, 0x0031);
    pub const SERIES_DESCRIPTION: Tag = Tag::new(0x0008, 0x103E);
    pub const ROWS: Tag = Tag::new(0x0028, 0x0010);
    pub const COLUMNS: Tag = Tag::new(0x0028, 0x0011);
    pub const PIXEL_SPACING: Tag = Tag::new(0x0028, 0x0030);
    pub const IMAGE_POSITION_PATIENT: Tag = Tag::new(0x0020, 0x0032);
    pub const IMAGE_ORIENTATION_PATIENT: Tag = Tag::new(0x0020, 0x0037);
    pub const SLICE_THICKNESS: Tag = Tag::new(0x0018, 0x0050);
    pub const MEDIA_STORAGE_SOP_CLASS_UID: Tag = Tag::new(0x0002, 0x0002);
    pub const MEDIA_STORAGE_SOP_INSTANCE_UID: Tag = Tag::new(0x0002, 0x0003);
}
const BUILTIN_TAGS: [Tag; 25] = [
    tags::STUDY_INSTANCE_UID,
    tags::SERIES_INSTANCE_UID,
    tags::MODALITY,
    tags::SERIES_NUMBER,
    tags::SOP_CLASS_UID,
    tags::SOP_INSTANCE_UID,
    tags::INSTANCE_NUMBER,
    tags::NUMBER_OF_FRAMES,
    tags::PATIENT_NAME,
    tags::PATIENT_ID,
    tags::PATIENT_BIRTH_DATE,
    tags::PATIENT_SEX,
    tags::ACCESSION_NUMBER,
    tags::STUDY_DATE,
    tags::STUDY_TIME,
    tags::STUDY_DESCRIPTION,
    tags::SERIES_DATE,
    tags::SERIES_TIME,
    tags::SERIES_DESCRIPTION,
    tags::ROWS,
    tags::COLUMNS,
    tags::PIXEL_SPACING,
    tags::IMAGE_POSITION_PATIENT,
    tags::IMAGE_ORIENTATION_PATIENT,
    tags::SLICE_THICKNESS,
];
fn valid_selector_id(id: &str) -> bool {
    let b = id.as_bytes();
    if b.is_empty() || b.len() > 64 || !b[0].is_ascii_alphanumeric() {
        return false;
    }
    b[1..]
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}
pub fn normalize_options(mut options: Options) -> Result<Options, Error> {
    let defaults = default_limits();
    if matches!(options.sequence_pruning, SequencePruning::Other(_)) {
        return Err(Error::options("unknown sequence pruning policy"));
    }
    if options.profile == 0 {
        options.profile = PROFILE_CORE;
    }
    if options.profile & !(PROFILE_ALL | PROFILE_NONE) != 0
        || (options.profile & PROFILE_NONE != 0 && options.profile != PROFILE_NONE)
    {
        return Err(Error::options("unknown profile bits"));
    }
    normalize_limits(&mut options.limits, &defaults)?;
    if let Some(uid) = &options.raw_data_set {
        match transfer::lookup(uid) {
            Some(s) if s.supported => options.raw_data_set = Some(s.uid.to_string()),
            _ => {
                return Err(Error::options(
                    "raw transfer syntax UID must identify a registered supported syntax",
                ));
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    let limits = options.limits;
    for selector in &mut options.selectors {
        selector.id = selector.id.trim().to_string();
        if !valid_selector_id(&selector.id) || matches!(selector.occurrence, Occurrence::Other(_)) {
            return Err(Error::options("invalid selector"));
        }
        if !seen.insert(selector.id.clone()) {
            return Err(Error::options("duplicate selector ID"));
        }
        if selector.max_values < 0 || selector.max_value_bytes < 0 {
            return Err(Error::options("negative selector limit"));
        }
        if selector.max_values == 0 {
            selector.max_values = if selector.occurrence == Occurrence::All {
                limits.max_selected_values
            } else {
                1
            };
        }
        if selector.max_value_bytes == 0 {
            selector.max_value_bytes = limits.max_selected_value_bytes;
        }
    }
    Ok(options)
}
fn normalize_limits(l: &mut Limits, d: &Limits) -> Result<(), Error> {
    if l.max_total_bytes < 0
        || l.max_selected_value_bytes < 0
        || l.max_elements < 0
        || l.max_sequence_depth < 0
        || l.max_tokens < 0
        || l.max_fragments < 0
        || l.max_selected_values < 0
        || l.max_diagnostics < 0
    {
        return Err(Error::options("negative resource limit"));
    }
    let fill = |v: &mut i64, dv: i64| {
        if *v == 0 {
            *v = dv;
        }
    };
    fill(&mut l.max_total_bytes, d.max_total_bytes);
    fill(&mut l.max_selected_value_bytes, d.max_selected_value_bytes);
    fill(&mut l.max_elements, d.max_elements);
    fill(&mut l.max_sequence_depth, d.max_sequence_depth);
    fill(&mut l.max_tokens, d.max_tokens);
    fill(&mut l.max_fragments, d.max_fragments);
    fill(&mut l.max_selected_values, d.max_selected_values);
    fill(&mut l.max_diagnostics, d.max_diagnostics);
    Ok(())
}
fn builtin_tag_selected(profile: Profile, tag: Tag) -> bool {
    use tags::*;
    if profile & PROFILE_CORE != 0
        && [
            STUDY_INSTANCE_UID,
            SERIES_INSTANCE_UID,
            MODALITY,
            SERIES_NUMBER,
            SOP_CLASS_UID,
            SOP_INSTANCE_UID,
            INSTANCE_NUMBER,
            NUMBER_OF_FRAMES,
        ]
        .contains(&tag)
    {
        return true;
    }
    if profile & PROFILE_PATIENT != 0
        && [PATIENT_NAME, PATIENT_ID, PATIENT_BIRTH_DATE, PATIENT_SEX].contains(&tag)
    {
        return true;
    }
    if profile & PROFILE_DESCRIPTIONS != 0
        && [
            ACCESSION_NUMBER,
            STUDY_DATE,
            STUDY_TIME,
            STUDY_DESCRIPTION,
            SERIES_DATE,
            SERIES_TIME,
            SERIES_DESCRIPTION,
        ]
        .contains(&tag)
    {
        return true;
    }
    profile & PROFILE_GEOMETRY != 0
        && [
            ROWS,
            COLUMNS,
            PIXEL_SPACING,
            IMAGE_POSITION_PATIENT,
            IMAGE_ORIENTATION_PATIENT,
            SLICE_THICKNESS,
        ]
        .contains(&tag)
}
fn header_read_plan_maximum(options: &Options) -> Tag {
    let mut maximum = TAG_SPECIFIC_CHARACTER_SET;
    if !options.assume_sorted_tags {
        return maximum;
    }
    for tag in BUILTIN_TAGS {
        if builtin_tag_selected(options.profile, tag) && tag > maximum {
            maximum = tag;
        }
    }
    for selector in &options.selectors {
        let tag = selector.path.first().copied().unwrap_or(selector.tag);
        if tag > maximum {
            maximum = tag;
        }
    }
    maximum
}
fn sequence_reachable(selectors: &[Selector], path: &[PathStep]) -> bool {
    selectors.iter().any(|s| {
        s.path.len() >= path.len() && path.iter().zip(&s.path).all(|(step, t)| step.tag == *t)
    })
}
fn selector_matches(selector: &Selector, path: &[PathStep]) -> bool {
    if path.len() != selector.path.len() + 1 || path[path.len() - 1].tag != selector.tag {
        return false;
    }
    selector
        .path
        .iter()
        .zip(path)
        .all(|(t, step)| step.tag == *t && step.item_index >= 0)
}
fn is_pixel_value_tag(tag: Tag) -> bool {
    tag == Tag::PIXEL_DATA || tag == Tag::new(0x7FE0, 0x0008) || tag == Tag::new(0x7FE0, 0x0009)
}
fn is_parser_resource_limit(err: &ObjectError) -> bool {
    if err.is(ObjectKind::MaxTotalBytes) {
        return true;
    }
    let text = err.to_string();
    [
        "dicom: maximum element value size exceeded",
        "dicom: maximum element count exceeded",
        "dicom: maximum sequence nesting depth exceeded",
        "dicom: maximum fragment count exceeded",
        ERR_MAX_SELECTIVE_TOKENS,
    ]
    .iter()
    .any(|s| text.contains(s))
}
struct Match {
    selector: isize,
    path: Vec<PathStep>,
}
struct Collector {
    options: Options,
    maximum: Tag,
    standard: BTreeMap<Tag, Element>,
    selected: BTreeMap<String, Vec<SelectedElement>>,
    pending: Vec<Match>,
    selected_count: i64,
    select_err: bool,
}
impl Collector {
    fn select_header(
        &mut self,
        path: &[PathStep],
        header: &ElementHeader,
    ) -> Result<Disposition, ()> {
        if is_pixel_value_tag(header.tag) {
            return Ok(if path.len() == 1 {
                Disposition::Stop
            } else {
                Disposition::Skip
            });
        }
        if path.len() == 1 && self.options.assume_sorted_tags && header.tag > self.maximum {
            return Ok(Disposition::Stop);
        }
        if header.vr == Vr::SQ || header.is_undefined() {
            if self.options.sequence_pruning == SequencePruning::Opaque
                && (header.vr == Vr::SQ || (header.vr == Vr::UN && header.is_undefined()))
                && !sequence_reachable(&self.options.selectors, path)
            {
                return Ok(Disposition::SkipSequence);
            }
            return Ok(Disposition::Materialize);
        }
        if path.len() == 1
            && (header.tag == TAG_SPECIFIC_CHARACTER_SET
                || builtin_tag_selected(self.options.profile, header.tag))
        {
            self.pending.push(Match {
                selector: -1,
                path: path.to_vec(),
            });
        }
        for (i, selector) in self.options.selectors.iter().enumerate() {
            if selector_matches(selector, path) {
                if selector.occurrence == Occurrence::First
                    && self
                        .selected
                        .get(&selector.id)
                        .is_some_and(|v| !v.is_empty())
                {
                    continue;
                }
                if header.length as i64 > selector.max_value_bytes {
                    self.select_err = true;
                    return Err(());
                }
                self.pending.push(Match {
                    selector: i as isize,
                    path: path.to_vec(),
                });
            }
        }
        if self.pending.is_empty() {
            Ok(Disposition::Skip)
        } else {
            Ok(Disposition::Materialize)
        }
    }
    fn is_standard_top_level(&self, tag: Tag) -> bool {
        self.pending
            .iter()
            .any(|m| m.selector == -1 && m.path.len() == 1 && m.path[0].tag == tag)
    }
    fn capture_selected(&mut self, element: &Element, offset: i64) -> Result<(), Error> {
        let pending = std::mem::take(&mut self.pending);
        for m in &pending {
            if m.selector < 0 {
                continue;
            }
            let selector = &self.options.selectors[m.selector as usize];
            let mut values = self.selected.get(&selector.id).cloned().unwrap_or_default();
            let selected = SelectedElement {
                path: m.path.clone(),
                offset,
                element: element.clone(),
            };
            match selector.occurrence {
                Occurrence::First => {
                    if !values.is_empty() {
                        continue;
                    }
                    values.push(selected);
                }
                Occurrence::Last => {
                    if values.is_empty() {
                        values.push(selected);
                    } else {
                        *values.last_mut().expect("nonempty") = selected;
                    }
                }
                Occurrence::All => {
                    if values.len() as i64 >= selector.max_values {
                        self.pending = pending;
                        return Err(Error::staged("selector").limit());
                    }
                    values.push(selected);
                }
                Occurrence::Other(_) => {}
            }
            let previous = self.selected.get(&selector.id).map_or(0, Vec::len);
            if values.len() > previous {
                self.selected_count += (values.len() - previous) as i64;
                if self.selected_count > self.options.limits.max_selected_values {
                    self.pending = pending;
                    return Err(Error::staged("selector").limit());
                }
            }
            self.selected.insert(selector.id.clone(), values);
        }
        self.pending = pending;
        Ok(())
    }
}
pub struct OpenedSource {
    pub data: Rc<[u8]>,
    pub info: SourceInfo,
}
fn classify_io(err: &std::io::Error) -> Option<External> {
    use std::io::ErrorKind as K;
    match err.kind() {
        K::NotFound => Some(External::NotExist),
        K::PermissionDenied => Some(External::Permission),
        K::AlreadyExists => Some(External::Exist),
        K::InvalidInput => Some(External::Invalid),
        _ => None,
    }
}
fn source_error(external: Option<External>) -> Error {
    let mut e = Error::staged("source");
    e.external = external;
    e
}
pub fn open_path(path: &str) -> Result<OpenedSource, Error> {
    let meta = std::fs::metadata(path).map_err(|e| source_error(classify_io(&e)))?;
    let file = std::fs::File::open(path);
    if let Err(e) = &file {
        return Err(source_error(classify_io(e)));
    }
    if !meta.is_file() {
        return Err(source_error(None));
    }
    let data = std::fs::read(path).map_err(|e| source_error(classify_io(&e)))?;
    let size = data.len() as i64;
    Ok(OpenedSource {
        data: Rc::from(data),
        info: SourceInfo {
            origin: path.to_string(),
            size,
        },
    })
}
pub fn read_path(
    path: impl AsRef<std::path::Path>,
    options: Options,
) -> (IndexResult, Option<Error>) {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    let options = match normalize_options(options) {
        Ok(o) => o,
        Err(e) => return (IndexResult::default(), Some(e)),
    };
    match open_path(path) {
        Ok(opened) => read_opened(opened, options),
        Err(e) => (IndexResult::default(), Some(e)),
    }
}
pub fn read_bytes(origin: &str, data: &[u8], options: Options) -> (IndexResult, Option<Error>) {
    let options = match normalize_options(options) {
        Ok(o) => o,
        Err(e) => return (IndexResult::default(), Some(e)),
    };
    let opened = OpenedSource {
        data: Rc::from(data),
        info: SourceInfo {
            origin: origin.to_string(),
            size: data.len() as i64,
        },
    };
    read_opened(opened, options)
}
fn header_error(err: ObjectError) -> Error {
    let mut e = Error::staged("file meta");
    let resource = is_parser_resource_limit(&err);
    e = e.with_cause(err);
    if resource {
        e = e.limit();
    }
    e
}
fn index_parser_error(err: ObjectError) -> Error {
    let mut e = Error::staged("dataset");
    if let Some(info) = err.parse_info() {
        e.tag = info.tag;
        e.offset = info.offset;
        e.offset_set = true;
    }
    let resource = is_parser_resource_limit(&err);
    e = e.with_cause(err);
    if resource {
        e = e.limit();
    }
    e
}
fn read_opened(opened: OpenedSource, options: Options) -> (IndexResult, Option<Error>) {
    let file = MemFile::new(opened.data.clone());
    let mut record = Record {
        source: opened.info.clone(),
        ..Default::default()
    };
    let raw = options.raw_data_set.is_some();
    let (syntax, dataset_offset) = if let Some(uid) = &options.raw_data_set {
        let syntax = transfer::lookup(uid).expect("normalized");
        record.file_meta.transfer_syntax_uid = syntax.uid.to_string();
        record.file_meta.transfer_syntax_name = syntax.name.to_string();
        append_diagnostic(
            &mut record,
            options.limits.max_diagnostics,
            Diagnostic {
                code: DiagnosticCode::RawDataSet,
                severity: Severity::Warning,
                field: String::new(),
                tag: Tag::default(),
                offset: 0,
                offset_set: false,
                message: "input was parsed as an explicitly configured raw data set".to_string(),
            },
        );
        (syntax, 0)
    } else {
        if options.limits.max_total_bytes > 0 && options.limits.max_total_bytes < 132 {
            let e = Error::staged("preamble")
                .limit()
                .with_cause(ObjectError::sentinel(ObjectKind::MaxTotalBytes));
            return (
                IndexResult {
                    record,
                    ..Default::default()
                },
                Some(e),
            );
        }
        let header_opts = ReadFileOptions {
            max_element_bytes: options.limits.max_selected_value_bytes,
            max_total_bytes: options.limits.max_total_bytes,
            max_sequence_depth: options.limits.max_sequence_depth,
            max_elements: options.limits.max_elements,
            strict_reserved_bytes: options.strict_reserved_bytes,
            accept_odd_length: options.accept_odd_length,
            allow_missing_meta_element_group_length: options
                .allow_missing_meta_element_group_length,
            ..Default::default()
        };
        let header = match read_part10_header_mem(&file, &header_opts) {
            Ok(h) => h,
            Err(err) => {
                let err = if err.is(ObjectKind::Eof) || err.is(ObjectKind::UnexpectedEof) {
                    ObjectError::join(
                        Some(ObjectError::sentinel(ObjectKind::MissingPreamble)),
                        Some(err),
                    )
                    .expect("joined")
                } else {
                    err
                };
                return (
                    IndexResult {
                        record,
                        ..Default::default()
                    },
                    Some(header_error(err)),
                );
            }
        };
        populate_file_meta(&mut record, &header, &options);
        (header.transfer_syntax, header.data_set_offset)
    };
    let (source, sized): (Box<dyn ByteRead>, Option<(i64, i64)>) = if syntax.deflated {
        let (decoded, err, _) = inflate::inflate_raw(file.remaining());
        (Box::new(Inflated::new(decoded, err)), None)
    } else {
        (
            Box::new(file.clone()),
            Some((file.len() as i64, file.position() as i64)),
        )
    };
    let collector = Rc::new(RefCell::new(Collector {
        maximum: header_read_plan_maximum(&options),
        options: options.clone(),
        standard: BTreeMap::new(),
        selected: BTreeMap::new(),
        pending: Vec::new(),
        selected_count: 0,
        select_err: false,
    }));
    let reader_opts = ReaderOptions {
        max_element_bytes: options.limits.max_selected_value_bytes,
        max_total_bytes: options.limits.max_total_bytes,
        base_offset: dataset_offset,
        max_sequence_depth: options.limits.max_sequence_depth,
        max_elements: options.limits.max_elements,
        max_fragments: options.limits.max_fragments,
        strict_reserved_bytes: options.strict_reserved_bytes,
        accept_odd_length: options.accept_odd_length,
        ..Default::default()
    };
    let max_tokens = if options.sequence_pruning == SequencePruning::Opaque {
        options.limits.max_tokens
    } else {
        0
    };
    let select_collector = collector.clone();
    let mut reader = match SelectiveReader::new(
        source,
        sized,
        syntax,
        reader_opts,
        Box::new(move |path, header, _| select_collector.borrow_mut().select_header(path, header)),
        max_tokens,
    ) {
        Ok(r) => r,
        Err(err) => {
            let e = Error::staged("dataset").with_cause(err);
            return (
                IndexResult {
                    record,
                    ..Default::default()
                },
                Some(e),
            );
        }
    };
    let mut stopped = false;
    let mut stopped_after = false;
    let mut stopped_header = None;
    let mut token_count = 0i64;
    let fail = |record: &Record, reader: &SelectiveReader, e: Error| {
        (
            IndexResult {
                record: record.clone(),
                bytes_scanned: reader.position(),
                raw_data_set: raw,
                ..Default::default()
            },
            Some(e),
        )
    };
    loop {
        {
            let mut c = collector.borrow_mut();
            c.pending.clear();
            c.select_err = false;
        }
        let token = match reader.next() {
            Ok(t) => t,
            Err(err) if err.is(ObjectKind::Eof) => break,
            Err(err) => {
                let select_err = collector.borrow().select_err;
                let e = if select_err {
                    Error::staged("selector").limit().with_cause(err)
                } else {
                    index_parser_error(err)
                };
                record.selected = collector.borrow().selected.clone();
                return fail(&record, &reader, e);
            }
        };
        token_count += 1;
        if token_count > options.limits.max_tokens {
            record.selected = collector.borrow().selected.clone();
            return fail(&record, &reader, Error::staged("dataset").limit());
        }
        let tok = match token {
            SelectiveToken::Stop { header, .. } => {
                stopped = is_pixel_value_tag(header.tag);
                stopped_after = !stopped && options.assume_sorted_tags;
                stopped_header = Some(header);
                break;
            }
            SelectiveToken::Token(tok) => tok,
        };
        if tok.kind != crate::object::parser::TokenKind::Element {
            continue;
        }
        let Some(element) = tok.element else { continue };
        if element.value == crate::object::Value::Nil {
            continue;
        }
        let mut c = collector.borrow_mut();
        if c.is_standard_top_level(element.tag()) {
            c.standard.insert(element.tag(), element.clone());
        }
        if let Err(e) = c.capture_selected(&element, tok.offset) {
            record.selected = c.selected.clone();
            drop(c);
            return fail(&record, &reader, e);
        }
    }
    let c = collector.borrow();
    record.selected = c.selected.clone();
    populate_record(&mut record, &c.standard, &syntax, &options);
    drop(c);
    if let Err(e) = run_selector_handlers(&options.selectors, &record.selected) {
        return (
            IndexResult {
                record,
                bytes_scanned: reader.position(),
                stopped_before_pixel_data: stopped,
                raw_data_set: raw,
                ..Default::default()
            },
            Some(e),
        );
    }
    (
        IndexResult {
            record,
            bytes_scanned: reader.position(),
            stopped_before_pixel_data: stopped,
            stopped_after_requested_tags: stopped_after,
            stopped_header,
            raw_data_set: raw,
        },
        None,
    )
}
fn run_selector_handlers(
    selectors: &[Selector],
    selected: &BTreeMap<String, Vec<SelectedElement>>,
) -> Result<(), Error> {
    for selector in selectors {
        let Some(handle) = &selector.handle else {
            continue;
        };
        for element in selected.get(&selector.id).map(Vec::as_slice).unwrap_or(&[]) {
            let outcome =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(element)));
            if !matches!(outcome, Ok(Ok(()))) {
                return Err(Error::staged("selector").with(Sentinel::Selector));
            }
        }
    }
    Ok(())
}
fn populate_file_meta(record: &mut Record, header: &Part10Header, options: &Options) {
    record.file_meta.transfer_syntax_uid = header.transfer_syntax.uid.to_string();
    record.file_meta.transfer_syntax_name = header.transfer_syntax.name.to_string();
    let meta = &header.meta;
    if let Some(v) = read_uid(
        record,
        meta,
        "file_meta.media_storage_sop_class_uid",
        tags::MEDIA_STORAGE_SOP_CLASS_UID,
        options,
    ) {
        record.file_meta.media_storage_sop_class_uid = v;
    }
    if let Some(v) = read_uid(
        record,
        meta,
        "file_meta.media_storage_sop_instance_uid",
        tags::MEDIA_STORAGE_SOP_INSTANCE_UID,
        options,
    ) {
        record.file_meta.media_storage_sop_instance_uid = v;
    }
    let group_length = Tag::new(0x0002, 0x0000);
    if !meta.has(group_length) {
        missing(record, "file_meta.group_length", group_length, options);
    }
}
fn populate_record(
    record: &mut Record,
    elements: &BTreeMap<Tag, Element>,
    syntax: &Syntax,
    options: &Options,
) {
    use tags::*;
    let mut dataset = Object::from_elements(elements.values().cloned().collect());
    dataset.set_value_big_endian(syntax.big_endian);
    let ds = &dataset;
    if options.profile & PROFILE_PATIENT != 0 {
        let mut p = Patient::default();
        set(
            &mut p.name,
            read_string(record, ds, "patient.name", PATIENT_NAME, options),
        );
        set(
            &mut p.id,
            read_string(record, ds, "patient.id", PATIENT_ID, options),
        );
        set(
            &mut p.birth_date,
            read_string(
                record,
                ds,
                "patient.birth_date",
                PATIENT_BIRTH_DATE,
                options,
            ),
        );
        set(
            &mut p.sex,
            read_string(record, ds, "patient.sex", PATIENT_SEX, options),
        );
        record.patient = Some(p);
    }
    if options.profile & PROFILE_CORE != 0 {
        let v = read_uid(
            record,
            ds,
            "study.instance_uid",
            STUDY_INSTANCE_UID,
            options,
        );
        set(&mut record.study.instance_uid, v);
        let v = read_uid(
            record,
            ds,
            "series.instance_uid",
            SERIES_INSTANCE_UID,
            options,
        );
        set(&mut record.series.instance_uid, v);
        let v = read_string(record, ds, "series.modality", MODALITY, options);
        set(&mut record.series.modality, v);
        let v = read_string(record, ds, "series.number", SERIES_NUMBER, options);
        set(&mut record.series.number, v);
        let v = read_uid(record, ds, "instance.sop_class_uid", SOP_CLASS_UID, options);
        set(&mut record.instance.sop_class_uid, v);
        let v = read_uid(
            record,
            ds,
            "instance.sop_instance_uid",
            SOP_INSTANCE_UID,
            options,
        );
        set(&mut record.instance.sop_instance_uid, v);
        let v = read_string(record, ds, "instance.number", INSTANCE_NUMBER, options);
        set(&mut record.instance.number, v);
        if !ds.has(NUMBER_OF_FRAMES) {
            missing(
                record,
                "instance.number_of_frames",
                NUMBER_OF_FRAMES,
                options,
            );
        } else {
            match ds.get_int(NUMBER_OF_FRAMES) {
                Ok(v) => record.instance.number_of_frames = v,
                Err(_) => invalid(
                    record,
                    "instance.number_of_frames",
                    NUMBER_OF_FRAMES,
                    options,
                ),
            }
        }
    }
    if options.profile & PROFILE_DESCRIPTIONS != 0 {
        let fields: [(&str, Tag); 7] = [
            ("study.accession_number", ACCESSION_NUMBER),
            ("study.date", STUDY_DATE),
            ("study.time", STUDY_TIME),
            ("study.description", STUDY_DESCRIPTION),
            ("series.date", SERIES_DATE),
            ("series.time", SERIES_TIME),
            ("series.description", SERIES_DESCRIPTION),
        ];
        for (i, (field, tag)) in fields.iter().enumerate() {
            let v = read_string(record, ds, field, *tag, options);
            let target = match i {
                0 => &mut record.study.accession_number,
                1 => &mut record.study.date,
                2 => &mut record.study.time,
                3 => &mut record.study.description,
                4 => &mut record.series.date,
                5 => &mut record.series.time,
                _ => &mut record.series.description,
            };
            set(target, v);
        }
    }
    if options.profile & PROFILE_GEOMETRY != 0 {
        let mut g = Geometry::default();
        if let Some(v) = read_us(record, elements, syntax, "geometry.rows", ROWS, options) {
            g.rows = v;
        }
        if let Some(v) = read_us(
            record,
            elements,
            syntax,
            "geometry.columns",
            COLUMNS,
            options,
        ) {
            g.columns = v;
        }
        g.pixel_spacing = read_ds(record, ds, "geometry.pixel_spacing", PIXEL_SPACING, options);
        g.image_position_patient = read_ds(
            record,
            ds,
            "geometry.image_position_patient",
            IMAGE_POSITION_PATIENT,
            options,
        );
        g.image_orientation_patient = read_ds(
            record,
            ds,
            "geometry.image_orientation_patient",
            IMAGE_ORIENTATION_PATIENT,
            options,
        );
        if let Some(t) = read_ds(
            record,
            ds,
            "geometry.slice_thickness",
            SLICE_THICKNESS,
            options,
        ) && let Some(first) = t.first()
        {
            g.slice_thickness = *first;
        }
        record.geometry = Some(g);
    }
}
fn set(target: &mut String, value: Option<String>) {
    if let Some(v) = value {
        *target = v;
    }
}
fn read_string(
    record: &mut Record,
    ds: &Object,
    field: &str,
    tag: Tag,
    options: &Options,
) -> Option<String> {
    match ds.get_string(tag) {
        Some(v) => Some(v),
        None => {
            if ds.has(tag) {
                invalid(record, field, tag, options);
            } else {
                missing(record, field, tag, options);
            }
            None
        }
    }
}
fn read_uid(
    record: &mut Record,
    ds: &Object,
    field: &str,
    tag: Tag,
    options: &Options,
) -> Option<String> {
    match ds.get_uid(tag) {
        Some(v) => Some(v),
        None => {
            if ds.has(tag) {
                invalid(record, field, tag, options);
            } else {
                missing(record, field, tag, options);
            }
            None
        }
    }
}
fn read_us(
    record: &mut Record,
    elements: &BTreeMap<Tag, Element>,
    syntax: &Syntax,
    field: &str,
    tag: Tag,
    options: &Options,
) -> Option<i64> {
    let Some(element) = elements.get(&tag) else {
        missing(record, field, tag, options);
        return None;
    };
    match element.raw() {
        Some(raw) if element.vr() == Vr::US && raw.len() == 2 => Some(if syntax.big_endian {
            u16::from_be_bytes([raw[0], raw[1]])
        } else {
            u16::from_le_bytes([raw[0], raw[1]])
        } as i64),
        _ => {
            invalid(record, field, tag, options);
            None
        }
    }
}
fn read_ds(
    record: &mut Record,
    ds: &Object,
    field: &str,
    tag: Tag,
    options: &Options,
) -> Option<Vec<f64>> {
    if !ds.has(tag) {
        missing(record, field, tag, options);
        return None;
    }
    match ds.get_floats(tag) {
        Ok(v) => Some(v),
        Err(_) => {
            invalid(record, field, tag, options);
            None
        }
    }
}
fn missing(record: &mut Record, field: &str, tag: Tag, options: &Options) {
    record.missing.push(MissingField {
        field: field.to_string(),
        tag,
    });
    append_diagnostic(
        record,
        options.limits.max_diagnostics,
        Diagnostic {
            code: DiagnosticCode::MissingField,
            severity: Severity::Info,
            field: field.to_string(),
            tag,
            offset: 0,
            offset_set: false,
            message: "requested metadata field is missing".to_string(),
        },
    );
}
fn invalid(record: &mut Record, field: &str, tag: Tag, options: &Options) {
    append_diagnostic(
        record,
        options.limits.max_diagnostics,
        Diagnostic {
            code: DiagnosticCode::InvalidValue,
            severity: Severity::Warning,
            field: field.to_string(),
            tag,
            offset: 0,
            offset_set: false,
            message: "requested metadata field has an invalid encoded value".to_string(),
        },
    );
}
fn append_diagnostic(record: &mut Record, limit: i64, diagnostic: Diagnostic) {
    if limit > 0 && record.diagnostics.len() as i64 >= limit {
        return;
    }
    record.diagnostics.push(diagnostic);
}
