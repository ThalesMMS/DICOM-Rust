#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::elem::{
    calculated_length, required_uid, sequence_element, string_value, string_values, valid_uid,
};
use super::error::{Error, ErrorKind};
use super::graph::{Node, build_directory_hierarchy, directory_record_data_set};
use super::keys::{
    DEFAULT_MAX_DIRECTORY_RECORDS, DEFAULT_MAX_RECORD_KEY_BYTES, DEFAULT_MAX_RECORD_KEY_DEPTH,
    DEFAULT_MAX_RECORD_KEY_ELEMENTS, DEFAULT_MAX_TOTAL_RECORD_KEY_BYTES, TAG_INSTANCE_NUMBER,
    TAG_MEDIA_STORAGE_SOP_CLASS_UID, TAG_MEDIA_STORAGE_SOP_INSTANCE_UID, TAG_TRANSFER_SYNTAX_UID,
    directory_key_rules, read_handle, strict_valid, validate_directory_keys,
};
use super::pathrules;
use super::platform::{self, FileIdentity, FileStamp};
use super::record_types::{RecordType, record_type_for_sop_class, root_file_record_type};
use super::snapshot::{
    DirectorySnapshot, merge_imported_directory, planned_directory_key_bytes,
    planned_directory_record_count,
};
use super::{
    Reference, TAG_DIRECTORY_RECORD_SEQUENCE, is_windows_reserved_file_name, open_referenced_file,
};
use crate::index::{self, IndexResult, Occurrence, Selector, Sentinel};
use crate::object::parser::source::MemFile;
use crate::object::text::normalize_uid;
use crate::object::{DataSet, ReadFileOptions, Tag, read_part10_header_mem};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
const DEFAULT_MAX_FILE_SET_FILES: i64 = 100_000;
const DEFAULT_MAX_FILE_SET_DIRECTORIES: i64 = 50_000;
const DEFAULT_MAX_FILE_SET_DEPTH: i64 = 64;
const DEFAULT_MAX_FILE_SET_PATH_BYTES: i64 = 4_096;
const DEFAULT_MAX_FILE_SET_DIAGNOSTICS: i64 = 1_000;
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub Vec<String>);
impl fmt::Display for FileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("\\"))
    }
}
impl FileId {
    pub fn components(&self) -> &[String] {
        &self.0
    }
}
pub fn new_file_id<S: AsRef<str>>(components: &[S]) -> Result<FileId, Error> {
    let id = FileId(components.iter().map(|c| c.as_ref().to_string()).collect());
    validate_authored_file_id(&id)?;
    if matches!(id.0.as_slice(), [only] if only == "DICOMDIR") {
        return Err(ErrorKind::InvalidFileId.into());
    }
    Ok(id)
}
pub fn parse_file_id(value: &str) -> Result<FileId, Error> {
    let value = value.trim();
    if value.is_empty()
        || value.contains('/')
        || pathrules::is_abs(value)
        || !pathrules::volume_name(value).is_empty()
        || value.starts_with('\\')
    {
        return Err(ErrorKind::InvalidFileId.into());
    }
    let parts: Vec<&str> = value.split('\\').collect();
    new_file_id(&parts)
}
pub(crate) fn file_id_from_relative_path(relative: &str) -> Result<FileId, Error> {
    if relative.is_empty()
        || relative == "."
        || pathrules::is_abs(relative)
        || !pathrules::volume_name(relative).is_empty()
    {
        return Err(ErrorKind::InvalidFileId.into());
    }
    let parts: Vec<&str> = relative.split(pathrules::sep_str()).collect();
    new_file_id(&parts)
}
pub(crate) fn validate_authored_file_id(id: &FileId) -> Result<(), Error> {
    if id.0.is_empty() || id.0.len() > 8 {
        return Err(ErrorKind::InvalidFileId.into());
    }
    for c in &id.0 {
        if c.is_empty()
            || c.len() > 8
            || c == "."
            || c == ".."
            || is_windows_reserved_file_name(c)
            || !c
                .bytes()
                .all(|b| b == b'_' || b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Err(ErrorKind::InvalidFileId.into());
        }
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EntryPolicy {
    #[default]
    Reject,
    Skip,
    Other(u8),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_files: i64,
    pub max_directories: i64,
    pub max_depth: i64,
    pub max_path_bytes: i64,
    pub max_diagnostics: i64,
    pub max_directory_records: i64,
    pub max_record_key_bytes: i64,
    pub max_total_record_key_bytes: i64,
    pub max_record_key_elements: i64,
    pub max_record_key_depth: i64,
}
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub file_set_uid: String,
    pub file_set_id: String,
    pub descriptor_file_id: FileId,
    pub descriptor_character_set: String,
    pub limits: Limits,
    pub index_options: index::Options,
    pub invent_missing_keys: bool,
    pub legacy_mrdr: bool,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ScanOptions {
    pub policy: EntryPolicy,
    pub limits: Limits,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DiagnosticCode(pub &'static str);
impl DiagnosticCode {
    pub const INVALID_FILE_ID: DiagnosticCode = DiagnosticCode("invalid_file_id");
    pub const INVALID_DICOM: DiagnosticCode = DiagnosticCode("invalid_dicom");
    pub const MISSING_KEY: DiagnosticCode = DiagnosticCode("missing_required_key");
    pub const UNSUPPORTED: DiagnosticCode = DiagnosticCode("unsupported_sop_class");
    pub const DUPLICATE: DiagnosticCode = DiagnosticCode("duplicate");
    pub const SOURCE_CHANGED: DiagnosticCode = DiagnosticCode("source_changed");
    pub const PROFILE_SOP_CLASS: DiagnosticCode = DiagnosticCode("profile_sop_class");
    pub const PROFILE_TRANSFER_SYNTAX: DiagnosticCode = DiagnosticCode("profile_transfer_syntax");
    pub const PROFILE_IMAGE: DiagnosticCode = DiagnosticCode("profile_image_constraint");
    pub const PROFILE_MISSING_KEY: DiagnosticCode = DiagnosticCode("profile_missing_key");
    pub const ICON_UNAVAILABLE: DiagnosticCode = DiagnosticCode("icon_unavailable");
}
impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub source_index: usize,
    pub message: String,
}
#[derive(Clone, Debug, Default)]
pub struct ScanReport {
    pub added: i64,
    pub skipped: i64,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug, Default)]
pub struct ValidationReport {
    pub valid: bool,
    pub statistics: Statistics,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileRecord {
    pub file_id: FileId,
    pub record_type: RecordType,
    pub specific_character_set: String,
    pub patient_name: String,
    pub patient_id: String,
    pub study_date: String,
    pub study_time: String,
    pub study_description: String,
    pub study_instance_uid: String,
    pub study_id: String,
    pub accession_number: String,
    pub modality: String,
    pub series_instance_uid: String,
    pub series_number: String,
    pub instance_number: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub transfer_syntax_uid: String,
    pub related_general_sop_class_uids: Vec<String>,
    pub invented_keys: Vec<String>,
    pub(crate) keys: DataSet,
    pub(crate) key_bytes: i64,
}
impl FileRecord {
    pub fn selection_keys(&self) -> DataSet {
        self.keys.clone()
    }
}
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub patient_id: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_instance_uid: String,
    pub modality: String,
    pub record_type: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    pub files: i64,
    pub instances: i64,
    pub bytes: i64,
    pub patients: i64,
    pub studies: i64,
    pub series: i64,
    pub by_type: BTreeMap<String, i64>,
    pub by_modality: BTreeMap<String, i64>,
}
#[derive(Clone, Debug)]
pub(crate) struct SourceSnapshot {
    pub stamp: FileStamp,
    pub record: FileRecord,
}
#[derive(Debug)]
pub struct FileSet {
    pub(crate) root: String,
    pub(crate) root_identity: FileIdentity,
    pub(crate) options: Options,
    pub(crate) uid: String,
    pub(crate) id: String,
    pub(crate) descriptor: FileId,
    pub(crate) descriptor_character_set: String,
    pub(crate) records: Vec<SourceSnapshot>,
    file_ids: HashSet<String>,
    sop_instances: HashSet<String>,
    uid_roles: HashMap<String, usize>,
    identities: HashSet<FileIdentity>,
    pub(crate) revision: u64,
    pub(crate) key_bytes: i64,
    pub(crate) directory: Option<Rc<DirectorySnapshot>>,
}
fn root_is_directory(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}
impl FileSet {
    pub fn new(root: &str, options: Options) -> Result<FileSet, Error> {
        let options = normalize_file_set_options(options)?;
        let root_abs = match pathrules::abs(root) {
            Ok(a) if !root.trim().is_empty() => a,
            _ => return Err(ErrorKind::InvalidOptions.into()),
        };
        if !root_is_directory(&root_abs) {
            return Err(Error::redacted(ErrorKind::InvalidOptions));
        }
        let Ok(root_identity) = platform::path_identity(&root_abs) else {
            return Err(Error::redacted(ErrorKind::InvalidOptions));
        };
        let mut uid = normalize_uid(&options.file_set_uid).to_string();
        if uid.is_empty() {
            uid = crate::object::inherit::new_uid();
        }
        if !valid_uid(&uid) {
            return Err(ErrorKind::InvalidOptions.into());
        }
        Ok(FileSet {
            root: root_abs,
            root_identity,
            id: options.file_set_id.clone(),
            descriptor: options.descriptor_file_id.clone(),
            descriptor_character_set: options.descriptor_character_set.clone(),
            options,
            uid,
            records: Vec::new(),
            file_ids: HashSet::new(),
            sop_instances: HashSet::new(),
            uid_roles: HashMap::new(),
            identities: HashSet::new(),
            revision: 0,
            key_bytes: 0,
            directory: None,
        })
    }
    pub fn root(&self) -> &str {
        &self.root
    }
    pub fn file_set_uid(&self) -> &str {
        &self.uid
    }
    pub fn file_set_id(&self) -> &str {
        &self.id
    }
    pub fn descriptor_file_id(&self) -> FileId {
        self.descriptor.clone()
    }
    pub fn descriptor_character_set(&self) -> &str {
        &self.descriptor_character_set
    }
    pub fn records(&self) -> Vec<FileRecord> {
        snapshot_file_records(&self.records)
            .into_iter()
            .map(|s| s.record)
            .collect()
    }
    pub(crate) fn root_identity_matches(&self) -> bool {
        root_is_directory(&self.root)
            && platform::path_identity(&self.root).is_ok_and(|id| id == self.root_identity)
    }
    pub fn add(&mut self, id: &FileId) -> Result<(), Error> {
        self.add_inner(id, true, None)
    }
    pub(crate) fn add_inner(
        &mut self,
        id: &FileId,
        verify_spelling: bool,
        recorded: Option<&FileRecord>,
    ) -> Result<(), Error> {
        if validate_authored_file_id(id).is_err()
            || matches!(id.0.as_slice(), [only] if only == "DICOMDIR")
        {
            return Err(ErrorKind::InvalidFileId.into());
        }
        if id.0.join(pathrules::sep_str()).len() as i64 > self.options.limits.max_path_bytes {
            return Err(ErrorKind::ResourceLimit.into());
        }
        if !self.root_identity_matches() {
            return Err(Error::redacted(ErrorKind::SourceChanged));
        }
        if verify_spelling && exact_file_id_spelling(&self.root, id).is_err() {
            return Err(Error::redacted(ErrorKind::InvalidRecord));
        }
        let reference = Reference {
            file_id: id.0.clone(),
            typ: String::new(),
        };
        let Ok((file, _)) = open_referenced_file(&self.root, &reference) else {
            return Err(Error::redacted(ErrorKind::InvalidRecord));
        };
        if !self.root_identity_matches() {
            return Err(Error::redacted(ErrorKind::SourceChanged));
        }
        let stamp = match platform::stamp(&file) {
            Ok(s) if s.regular => s,
            _ => return Err(Error::redacted(ErrorKind::InvalidRecord)),
        };
        let Ok(data) = read_handle(file) else {
            return Err(Error::redacted(ErrorKind::InvalidRecord));
        };
        let mut header_options = ReadFileOptions {
            max_element_bytes: 1 << 20,
            max_total_bytes: 16 << 20,
            max_sequence_depth: 4,
            max_elements: 256,
            ..Default::default()
        };
        let configured = self.options.index_options.limits.max_total_bytes;
        if configured > 0 && configured < header_options.max_total_bytes {
            header_options.max_total_bytes = configured;
        }
        let configured = self.options.index_options.limits.max_selected_value_bytes;
        if configured > 0 && configured < header_options.max_element_bytes {
            header_options.max_element_bytes = configured;
        }
        let header = match read_part10_header_mem(&MemFile::new(data.clone()), &header_options) {
            Ok(h) => h,
            Err(e) => {
                return Err(Error::redacted(
                    if super::keys::is_parser_resource_limit(&e) {
                        ErrorKind::ResourceLimit
                    } else {
                        ErrorKind::InvalidRecord
                    },
                ));
            }
        };
        let (meta_class, class_ok) = required_uid(&header.meta, TAG_MEDIA_STORAGE_SOP_CLASS_UID);
        let (meta_instance, instance_ok) =
            required_uid(&header.meta, TAG_MEDIA_STORAGE_SOP_INSTANCE_UID);
        let (meta_syntax, syntax_ok) = required_uid(&header.meta, TAG_TRANSFER_SYNTAX_UID);
        if !class_ok
            || !instance_ok
            || !syntax_ok
            || meta_class.is_empty()
            || meta_instance.is_empty()
            || meta_syntax.is_empty()
        {
            return Err(Error::redacted(ErrorKind::InvalidRecord));
        }
        let (result, err) = index::read_bytes("", &data, self.metadata_index_options());
        drop(data);
        if let Some(e) = err {
            return Err(Error::redacted(if e.is(Sentinel::ResourceLimit) {
                ErrorKind::ResourceLimit
            } else {
                ErrorKind::InvalidRecord
            }));
        }
        let mut record = file_record_from_index(
            id,
            &result,
            self.options.invent_missing_keys || recorded.is_some(),
        )?;
        if let Some(recorded) = recorded {
            adopt_invented_keys(&mut record, recorded);
        }
        let mut remaining = self
            .options
            .limits
            .max_total_record_key_bytes
            .saturating_sub(self.key_bytes);
        if let Some(d) = &self.directory {
            remaining = remaining.saturating_sub(d.key_bytes);
        }
        if remaining <= 0 {
            return Err(ErrorKind::ResourceLimit.into());
        }
        let mut key_limits = self.options.limits;
        if remaining < key_limits.max_record_key_bytes {
            key_limits.max_record_key_bytes = remaining;
        }
        self.read_directory_keys_with_limits(&mut record, &key_limits)?;
        let Ok((check, _)) = open_referenced_file(&self.root, &reference) else {
            return Err(ErrorKind::SourceChanged.into());
        };
        match platform::stamp(&check) {
            Ok(current)
                if current.identity == stamp.identity
                    && current.size == stamp.size
                    && current.modified == stamp.modified => {}
            _ => return Err(ErrorKind::SourceChanged.into()),
        }
        drop(check);
        validate_file_record(&record, Some(&self.options.limits))?;
        for uid in record.related_general_sop_class_uids.iter_mut() {
            *uid = normalize_uid(uid).to_string();
        }
        if self.uid == record.study_instance_uid
            || self.uid == record.series_instance_uid
            || self.uid == record.sop_instance_uid
        {
            return Err(Error::redacted(ErrorKind::InvalidRecord));
        }
        if self.records.len() as i64 >= self.options.limits.max_files {
            return Err(ErrorKind::ResourceLimit.into());
        }
        let imported = self.directory.as_ref().map_or(0, |d| d.key_bytes);
        if record.key_bytes
            > self
                .options
                .limits
                .max_total_record_key_bytes
                .saturating_sub(self.key_bytes)
                .saturating_sub(imported)
        {
            return Err(ErrorKind::ResourceLimit.into());
        }
        if self.file_ids.contains(&id.to_string())
            || self.sop_instances.contains(&record.sop_instance_uid)
            || self.identities.contains(&stamp.identity)
        {
            return Err(Error::redacted(ErrorKind::InvalidRecord));
        }
        for (role, uid) in record_uid_roles(&record).iter().enumerate() {
            if uid.is_empty() {
                continue;
            }
            if let Some(&previous) = self.uid_roles.get(*uid)
                && previous != role
            {
                return Err(Error::redacted(ErrorKind::InvalidRecord));
            }
        }
        self.key_bytes = self.key_bytes.saturating_add(record.key_bytes);
        self.file_ids.insert(id.to_string());
        self.sop_instances.insert(record.sop_instance_uid.clone());
        self.identities.insert(stamp.identity);
        for (role, uid) in record_uid_roles(&record).iter().enumerate() {
            if !uid.is_empty() {
                self.uid_roles.insert(uid.to_string(), role);
            }
        }
        self.records.push(SourceSnapshot { stamp, record });
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
    pub(crate) fn metadata_index_options(&self) -> index::Options {
        let mut opts = self.options.index_options.clone();
        opts.profile = index::PROFILE_ALL;
        opts.selectors.extend(required_file_set_selectors());
        opts
    }
    pub fn remove(&mut self, id: &FileId) -> bool {
        let Some(i) = self.records.iter().position(|s| &s.record.file_id == id) else {
            return false;
        };
        self.records.remove(i);
        self.rebuild_indexes();
        self.revision = self.revision.saturating_add(1);
        true
    }
    fn rebuild_indexes(&mut self) {
        self.key_bytes = 0;
        self.file_ids.clear();
        self.sop_instances.clear();
        self.uid_roles.clear();
        self.identities.clear();
        for stored in &self.records {
            self.key_bytes = self.key_bytes.saturating_add(stored.record.key_bytes);
            self.file_ids.insert(stored.record.file_id.to_string());
            self.sop_instances
                .insert(stored.record.sop_instance_uid.clone());
            self.identities.insert(stored.stamp.identity);
            for (role, uid) in record_uid_roles(&stored.record).iter().enumerate() {
                if !uid.is_empty() {
                    self.uid_roles.insert(uid.to_string(), role);
                }
            }
        }
    }
    pub fn query(&self, q: &Query) -> Vec<FileRecord> {
        self.records
            .iter()
            .map(|s| &s.record)
            .filter(|r| {
                !(!q.patient_id.is_empty() && r.patient_id != q.patient_id
                    || !q.study_instance_uid.is_empty()
                        && r.study_instance_uid != q.study_instance_uid
                    || !q.series_instance_uid.is_empty()
                        && r.series_instance_uid != q.series_instance_uid
                    || !q.sop_instance_uid.is_empty() && r.sop_instance_uid != q.sop_instance_uid
                    || !q.modality.is_empty() && r.modality != q.modality
                    || !q.record_type.is_empty() && r.record_type.as_str() != q.record_type)
            })
            .cloned()
            .collect()
    }
    pub fn statistics(&self) -> Statistics {
        statistics_for_records(&self.records)
    }
    pub fn validate(&self) -> (ValidationReport, Option<Error>) {
        if let Err(e) = self.validate_descriptor() {
            return (ValidationReport::default(), Some(e));
        }
        let records = snapshot_file_records(&self.records);
        let mut report = match self.validate_records_snapshot(&records) {
            Ok(r) => r,
            Err(pair) => {
                let (r, e) = *pair;
                return (r, Some(e));
            }
        };
        if let Err(e) = self.revalidate_sources(&records) {
            report.valid = false;
            report.diagnostics.push(Diagnostic {
                code: DiagnosticCode::SOURCE_CHANGED,
                source_index: e.source_index.unwrap_or(0),
                message: diagnostic_message(DiagnosticCode::SOURCE_CHANGED).to_string(),
            });
            return (report, Some(e));
        }
        (report, None)
    }
    pub(crate) fn validate_records_snapshot(
        &self,
        records: &[SourceSnapshot],
    ) -> Result<ValidationReport, Box<(ValidationReport, Error)>> {
        let limit = |_: ()| {
            Box::new((
                ValidationReport::default(),
                Error::from(ErrorKind::ResourceLimit),
            ))
        };
        let max = self.options.limits.max_diagnostics;
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let mut add = |d: Diagnostic| -> Result<(), ()> {
            if diagnostics.len() as i64 >= max {
                return Err(());
            }
            diagnostics.push(d);
            Ok(())
        };
        let mut seen_files: HashSet<String> = HashSet::new();
        let mut seen_sop: HashSet<String> = HashSet::new();
        let mut uid_roles: HashMap<String, usize> = HashMap::new();
        let mut patient_study: HashMap<String, String> = HashMap::new();
        let mut study_series: HashMap<String, String> = HashMap::new();
        let dup = |i: usize, message: &str| Diagnostic {
            code: DiagnosticCode::DUPLICATE,
            source_index: i,
            message: message.to_string(),
        };
        for (i, stored) in records.iter().enumerate() {
            let r = &stored.record;
            if self.uid == r.study_instance_uid
                || self.uid == r.series_instance_uid
                || self.uid == r.sop_instance_uid
            {
                add(dup(i, "UID identifies conflicting directory entities")).map_err(limit)?;
            }
            if let Err(e) = validate_file_record(r, Some(&self.options.limits)) {
                let code = if e.is(ErrorKind::MissingRequiredKey) {
                    DiagnosticCode::MISSING_KEY
                } else {
                    DiagnosticCode::INVALID_DICOM
                };
                add(Diagnostic {
                    code,
                    source_index: i,
                    message: diagnostic_message(code).to_string(),
                })
                .map_err(limit)?;
            }
            let file_key = r.file_id.to_string();
            if seen_files.contains(&file_key) || seen_sop.contains(&r.sop_instance_uid) {
                add(dup(i, "directory identifier is duplicated")).map_err(limit)?;
            }
            seen_files.insert(file_key);
            seen_sop.insert(r.sop_instance_uid.clone());
            for (role, uid) in record_uid_roles(r).iter().enumerate() {
                if uid.is_empty() {
                    continue;
                }
                match uid_roles.get(*uid) {
                    Some(&previous) if previous != role => {
                        add(dup(i, "UID identifies conflicting directory entities"))
                            .map_err(limit)?;
                    }
                    _ => {
                        uid_roles.insert(uid.to_string(), role);
                    }
                }
            }
            match patient_study.get(&r.study_instance_uid) {
                Some(patient) if patient != &r.patient_id => {
                    add(dup(i, "study has conflicting parent")).map_err(limit)?;
                }
                Some(_) => {}
                None => {
                    patient_study.insert(r.study_instance_uid.clone(), r.patient_id.clone());
                }
            }
            match study_series.get(&r.series_instance_uid) {
                Some(study) if study != &r.study_instance_uid => {
                    add(dup(i, "series has conflicting parent")).map_err(limit)?;
                }
                Some(_) => {}
                None => {
                    study_series
                        .insert(r.series_instance_uid.clone(), r.study_instance_uid.clone());
                }
            }
        }
        let directory = self.directory.as_deref();
        if let Err(e) = planned_directory_key_bytes(
            directory,
            records,
            self.options.limits.max_total_record_key_bytes,
        ) {
            return Err(Box::new((ValidationReport::default(), e)));
        }
        if let Err(e) = planned_directory_record_count(
            directory,
            records,
            self.options.limits.max_directory_records,
        ) {
            return Err(Box::new((ValidationReport::default(), e)));
        }
        let mut hierarchy_ok = true;
        match build_directory_hierarchy(records) {
            Ok(graph) => {
                if let Some(d) = directory
                    && merge_imported_directory(d, &graph).is_err()
                {
                    hierarchy_ok = false;
                }
            }
            Err(_) => hierarchy_ok = false,
        }
        if !hierarchy_ok {
            add(Diagnostic {
                code: DiagnosticCode::DUPLICATE,
                source_index: 0,
                message: "directory selection keys conflict".to_string(),
            })
            .map_err(limit)?;
        }
        let report = ValidationReport {
            valid: diagnostics.is_empty(),
            statistics: statistics_for_records(records),
            diagnostics,
        };
        if !report.valid {
            return Err(Box::new((report, ErrorKind::InvalidRecord.into())));
        }
        Ok(report)
    }
    pub(crate) fn validate_descriptor(&self) -> Result<(), Error> {
        if self.descriptor.0.is_empty() {
            return Ok(());
        }
        if !self.root_identity_matches() {
            return Err(Error::redacted(ErrorKind::SourceChanged));
        }
        if exact_file_id_spelling(&self.root, &self.descriptor).is_err() {
            return Err(Error::redacted(ErrorKind::InvalidRecord));
        }
        let reference = Reference {
            file_id: self.descriptor.0.clone(),
            typ: String::new(),
        };
        let Ok((file, _)) = open_referenced_file(&self.root, &reference) else {
            return Err(Error::redacted(ErrorKind::InvalidRecord));
        };
        if !self.root_identity_matches() {
            return Err(Error::redacted(ErrorKind::SourceChanged));
        }
        let stamp = match platform::stamp(&file) {
            Ok(s) if s.regular => s,
            _ => return Err(Error::redacted(ErrorKind::InvalidRecord)),
        };
        if self.identities.contains(&stamp.identity) {
            return Err(Error::redacted(ErrorKind::InvalidRecord));
        }
        Ok(())
    }
    pub fn scan(&mut self, options: &ScanOptions) -> (ScanReport, Option<Error>) {
        let mut report = ScanReport::default();
        if let EntryPolicy::Other(_) = options.policy {
            return (report, Some(ErrorKind::InvalidOptions.into()));
        }
        let mut limits = options.limits;
        if limits == Limits::default() {
            limits = self.options.limits;
        } else if let Err(e) = normalize_file_set_limits(&mut limits) {
            return (report, Some(e));
        }
        if !self.root_identity_matches() {
            return (report, Some(Error::redacted(ErrorKind::SourceChanged)));
        }
        let policy = options.policy;
        let mut directories: i64 = 1;
        if directories > limits.max_directories {
            return (report, Some(ErrorKind::ResourceLimit.into()));
        }
        let mut pending: Vec<(String, i64)> = vec![(String::new(), 0)];
        while let Some((relative_dir, depth_dir)) = pending.pop() {
            if depth_dir > limits.max_depth || relative_dir.len() as i64 > limits.max_path_bytes {
                return (report, Some(ErrorKind::ResourceLimit.into()));
            }
            let entries = match platform::list_directory(&self.root, &relative_dir) {
                Ok(entries) => entries,
                Err(_) => {
                    if let Err(e) = self.handle_scan_failure(
                        &mut report,
                        policy,
                        DiagnosticCode::INVALID_DICOM,
                        None,
                        &limits,
                    ) {
                        return (report, Some(e));
                    }
                    continue;
                }
            };
            for entry in entries {
                let relative = if relative_dir.is_empty() {
                    entry.name.clone()
                } else {
                    pathrules::join(&[&relative_dir, &entry.name])
                };
                let depth = depth_dir.saturating_add(1);
                if depth > limits.max_depth || relative.len() as i64 > limits.max_path_bytes {
                    return (report, Some(ErrorKind::ResourceLimit.into()));
                }
                if entry.is_dir {
                    directories = directories.saturating_add(1);
                    if directories > limits.max_directories {
                        return (report, Some(ErrorKind::ResourceLimit.into()));
                    }
                    pending.push((relative, depth));
                    continue;
                }
                if relative == "DICOMDIR" {
                    continue;
                }
                let id = file_id_from_relative_path(&relative);
                if let Ok(id) = &id
                    && *id == self.descriptor
                {
                    continue;
                }
                if report.added.saturating_add(report.skipped) >= limits.max_files {
                    return (report, Some(ErrorKind::ResourceLimit.into()));
                }
                let failure = if entry.is_symlink {
                    Some((
                        DiagnosticCode::INVALID_FILE_ID,
                        Some(Error::from(ErrorKind::InvalidFileId)),
                    ))
                } else if !entry.regular {
                    Some((DiagnosticCode::INVALID_DICOM, None))
                } else {
                    match id {
                        Err(e) => Some((DiagnosticCode::INVALID_FILE_ID, Some(e))),
                        Ok(id) => match self.add_inner(&id, false, None) {
                            Ok(()) => {
                                report.added = report.added.saturating_add(1);
                                None
                            }
                            Err(e) => {
                                let code = if e.is(ErrorKind::MissingRequiredKey) {
                                    DiagnosticCode::MISSING_KEY
                                } else if e.is(ErrorKind::UnsupportedSopClass) {
                                    DiagnosticCode::UNSUPPORTED
                                } else {
                                    DiagnosticCode::INVALID_DICOM
                                };
                                Some((code, Some(e)))
                            }
                        },
                    }
                };
                if let Some((code, cause)) = failure
                    && let Err(e) =
                        self.handle_scan_failure(&mut report, policy, code, cause.as_ref(), &limits)
                {
                    return (report, Some(e));
                }
            }
        }
        if !self.root_identity_matches() {
            return (report, Some(Error::redacted(ErrorKind::SourceChanged)));
        }
        (report, None)
    }
    fn handle_scan_failure(
        &self,
        report: &mut ScanReport,
        policy: EntryPolicy,
        code: DiagnosticCode,
        cause: Option<&Error>,
        limits: &Limits,
    ) -> Result<(), Error> {
        if policy == EntryPolicy::Reject {
            let mut class = ErrorKind::InvalidRecord;
            if let Some(cause) = cause {
                for candidate in [
                    ErrorKind::InvalidFileId,
                    ErrorKind::SourceChanged,
                    ErrorKind::ResourceLimit,
                    ErrorKind::UnsupportedSopClass,
                    ErrorKind::MissingRequiredKey,
                    ErrorKind::InvalidRecord,
                ] {
                    if cause.is(candidate) {
                        class = candidate;
                        break;
                    }
                }
            }
            return Err(Error::redacted(class));
        }
        report.skipped = report.skipped.saturating_add(1);
        if report.diagnostics.len() as i64 >= limits.max_diagnostics {
            return Err(ErrorKind::ResourceLimit.into());
        }
        report.diagnostics.push(Diagnostic {
            code,
            source_index: report
                .added
                .saturating_add(report.skipped)
                .saturating_sub(1) as usize,
            message: diagnostic_message(code).to_string(),
        });
        Ok(())
    }
}
fn normalize_file_set_options(mut options: Options) -> Result<Options, Error> {
    if options.file_set_id.len() > 16 || !portable_optional_label(&options.file_set_id) {
        return Err(ErrorKind::InvalidOptions.into());
    }
    if !options.descriptor_file_id.0.is_empty() {
        validate_authored_file_id(&options.descriptor_file_id)?;
        if matches!(options.descriptor_file_id.0.as_slice(), [only] if only == "DICOMDIR") {
            return Err(ErrorKind::InvalidFileId.into());
        }
        if options.descriptor_character_set.is_empty() {
        } else if options.descriptor_character_set.trim().is_empty()
            || crate::charset::SpecificCharacterSet::parse(&[options
                .descriptor_character_set
                .as_str()])
            .is_err()
        {
            return Err(ErrorKind::InvalidOptions.into());
        }
    } else if !options.descriptor_character_set.is_empty() {
        return Err(ErrorKind::InvalidOptions.into());
    }
    normalize_file_set_limits(&mut options.limits)?;
    Ok(options)
}
pub(crate) fn normalize_file_set_limits(l: &mut Limits) -> Result<(), Error> {
    if l.max_files < 0
        || l.max_directories < 0
        || l.max_depth < 0
        || l.max_path_bytes < 0
        || l.max_diagnostics < 0
        || l.max_directory_records < 0
        || l.max_record_key_bytes < 0
        || l.max_total_record_key_bytes < 0
        || l.max_record_key_elements < 0
        || l.max_record_key_depth < 0
        || l.max_record_key_depth > 128
    {
        return Err(ErrorKind::InvalidOptions.into());
    }
    let default = |v: &mut i64, d: i64| {
        if *v == 0 {
            *v = d;
        }
    };
    default(&mut l.max_directory_records, DEFAULT_MAX_DIRECTORY_RECORDS);
    default(&mut l.max_record_key_bytes, DEFAULT_MAX_RECORD_KEY_BYTES);
    default(
        &mut l.max_total_record_key_bytes,
        DEFAULT_MAX_TOTAL_RECORD_KEY_BYTES,
    );
    default(
        &mut l.max_record_key_elements,
        DEFAULT_MAX_RECORD_KEY_ELEMENTS,
    );
    default(&mut l.max_record_key_depth, DEFAULT_MAX_RECORD_KEY_DEPTH);
    default(&mut l.max_files, DEFAULT_MAX_FILE_SET_FILES);
    default(&mut l.max_directories, DEFAULT_MAX_FILE_SET_DIRECTORIES);
    default(&mut l.max_depth, DEFAULT_MAX_FILE_SET_DEPTH);
    default(&mut l.max_path_bytes, DEFAULT_MAX_FILE_SET_PATH_BYTES);
    default(&mut l.max_diagnostics, DEFAULT_MAX_FILE_SET_DIAGNOSTICS);
    Ok(())
}
pub(crate) fn portable_optional_label(value: &str) -> bool {
    value
        .bytes()
        .all(|c| c == b'_' || c.is_ascii_uppercase() || c.is_ascii_digit())
}
pub(crate) fn exact_file_id_spelling(root: &str, id: &FileId) -> Result<(), Error> {
    let mut parent = String::new();
    for (index, component) in id.0.iter().enumerate() {
        let entries = platform::list_directory(root, &parent)
            .map_err(|e| Error::other(super::error::io_text(&e)))?;
        let found = entries
            .iter()
            .any(|e| &e.name == component && (index.saturating_add(1) == id.0.len() || e.is_dir));
        if !found {
            return Err(ErrorKind::InvalidFileId.into());
        }
        parent = if parent.is_empty() {
            component.clone()
        } else {
            pathrules::join(&[&parent, component])
        };
    }
    Ok(())
}
pub(crate) fn validate_exact_file_id_spellings(root: &str, ids: &[FileId]) -> (i64, Option<Error>) {
    let mut wanted: HashMap<String, HashMap<String, (bool, usize)>> = HashMap::new();
    let mut parent_source: HashMap<String, usize> = HashMap::new();
    for (source, id) in ids.iter().enumerate() {
        let mut parent = String::new();
        for component in &id.0 {
            let e = parent_source.entry(parent.clone()).or_insert(source);
            if source < *e {
                *e = source;
            }
            let components = wanted.entry(parent.clone()).or_default();
            match components.get(component) {
                Some(&(_, existing)) if existing <= source => {}
                _ => {
                    components.insert(component.clone(), (false, source));
                }
            }
            parent = if parent.is_empty() {
                component.clone()
            } else {
                pathrules::join(&[&parent, component])
            };
        }
    }
    let sep = pathrules::sep_str();
    let mut parents: Vec<String> = wanted.keys().cloned().collect();
    let depth = |p: &str| {
        p.matches(sep)
            .count()
            .saturating_add(usize::from(!p.is_empty()))
    };
    parents.sort_by(|a, b| depth(a).cmp(&depth(b)).then_with(|| a.cmp(b)));
    for parent in parents {
        let entries = match platform::list_directory(root, &parent) {
            Ok(e) => e,
            Err(e) => {
                return (
                    parent_source.get(&parent).copied().unwrap_or(0) as i64,
                    Some(Error::other(super::error::io_text(&e))),
                );
            }
        };
        let components = wanted.get_mut(&parent).expect("parent");
        for entry in &entries {
            if let Some(c) = components.get_mut(&entry.name) {
                c.0 = true;
            }
        }
        let missing = components
            .values()
            .filter(|(found, _)| !found)
            .map(|(_, source)| *source)
            .min();
        if let Some(missing) = missing {
            return (missing as i64, Some(ErrorKind::InvalidFileId.into()));
        }
    }
    (-1, None)
}
const FILE_SET_SELECTOR_TAGS: &[(&str, u16, u16)] = &[
    ("charset", 0x0008, 0x0005),
    ("patient_name", 0x0010, 0x0010),
    ("patient_id", 0x0010, 0x0020),
    ("study_date", 0x0008, 0x0020),
    ("study_time", 0x0008, 0x0030),
    ("study_description", 0x0008, 0x1030),
    ("study_uid", 0x0020, 0x000D),
    ("study_id", 0x0020, 0x0010),
    ("accession", 0x0008, 0x0050),
    ("modality", 0x0008, 0x0060),
    ("series_uid", 0x0020, 0x000E),
    ("series_number", 0x0020, 0x0011),
    ("instance_number", 0x0020, 0x0013),
    ("sop_class", 0x0008, 0x0016),
    ("sop_instance", 0x0008, 0x0018),
    ("related_general_sop_class", 0x0008, 0x001A),
    ("series_date", 0x0008, 0x0021),
    ("acquisition_date", 0x0008, 0x0022),
    ("content_date", 0x0008, 0x0023),
    ("series_time", 0x0008, 0x0031),
    ("acquisition_time", 0x0008, 0x0032),
    ("content_time", 0x0008, 0x0033),
];
fn required_file_set_selectors() -> Vec<Selector> {
    FILE_SET_SELECTOR_TAGS
        .iter()
        .map(|&(id, g, e)| Selector {
            id: format!("dicomdir.{id}"),
            tag: Tag::new(g, e),
            occurrence: Occurrence::Last,
            ..Default::default()
        })
        .collect()
}
fn selected<'a>(result: &'a IndexResult, id: &str) -> &'a [index::SelectedElement] {
    result
        .record
        .selected
        .get(&format!("dicomdir.{id}"))
        .map_or(&[], Vec::as_slice)
}
fn selected_string(result: &IndexResult, id: &str) -> Option<String> {
    selected(result, id)
        .last()
        .map(|v| string_value(&v.element))
}
fn selected_strings(result: &IndexResult, id: &str) -> Option<Vec<String>> {
    selected(result, id)
        .last()
        .map(|v| string_values(&v.element))
}
fn selected_scalar_vm(result: &IndexResult, id: &str) -> bool {
    let values = selected(result, id);
    let [value] = values else {
        return false;
    };
    let e = &value.element;
    let strings = string_values(e);
    strings.len() == 1 || strings.is_empty() && calculated_length(e) == Some(0)
}
fn selected_optional_scalar_vm(result: &IndexResult, id: &str) -> bool {
    selected(result, id).is_empty() || selected_scalar_vm(result, id)
}
fn is_inventable(key: &str) -> bool {
    matches!(
        key,
        "patient_id"
            | "study_date"
            | "study_time"
            | "study_id"
            | "series_number"
            | "instance_number"
    )
}
pub(crate) fn file_record_from_index(
    id: &FileId,
    result: &IndexResult,
    invent: bool,
) -> Result<FileRecord, Error> {
    let fm = &result.record.file_meta;
    let class = normalize_uid(&fm.media_storage_sop_class_uid);
    let Some(record_type) = record_type_for_sop_class(class) else {
        return Err(ErrorKind::UnsupportedSopClass.into());
    };
    let root_type = root_file_record_type(record_type.as_str());
    for scalar in [
        "patient_id",
        "study_date",
        "study_time",
        "study_uid",
        "study_id",
        "modality",
        "series_uid",
        "series_number",
        "instance_number",
        "sop_class",
        "sop_instance",
    ] {
        if root_type && scalar != "sop_class" && scalar != "sop_instance" {
            continue;
        }
        if scalar == "instance_number" && record_type != RecordType::IMAGE {
            continue;
        }
        if selected(result, scalar).is_empty() {
            if invent && is_inventable(scalar) {
                continue;
            }
            return Err(Error::missing_key());
        }
        if !selected_scalar_vm(result, scalar) {
            return Err(ErrorKind::InvalidRecord.into());
        }
    }
    for scalar in ["patient_name", "study_description", "accession"] {
        if !selected_optional_scalar_vm(result, scalar) {
            return Err(ErrorKind::InvalidRecord.into());
        }
    }
    let inst = &result.record.instance;
    let data_set_class = normalize_uid(&inst.sop_class_uid);
    let meta_class = normalize_uid(&fm.media_storage_sop_class_uid);
    let data_set_instance = normalize_uid(&inst.sop_instance_uid);
    let meta_instance = normalize_uid(&fm.media_storage_sop_instance_uid);
    if meta_class.is_empty()
        || meta_instance.is_empty()
        || data_set_class.is_empty()
        || data_set_instance.is_empty()
        || fm.transfer_syntax_uid.is_empty()
    {
        return Err(Error::missing_key());
    }
    if data_set_class != meta_class || data_set_instance != meta_instance {
        return Err(ErrorKind::InvalidRecord.into());
    }
    let rec = &result.record;
    let mut record = FileRecord {
        file_id: id.clone(),
        record_type: record_type.clone(),
        study_instance_uid: rec.study.instance_uid.clone(),
        accession_number: rec.study.accession_number.clone(),
        study_date: rec.study.date.clone(),
        study_time: rec.study.time.clone(),
        study_description: rec.study.description.clone(),
        series_instance_uid: rec.series.instance_uid.clone(),
        modality: rec.series.modality.clone(),
        series_number: rec.series.number.clone(),
        sop_class_uid: meta_class.to_string(),
        sop_instance_uid: meta_instance.to_string(),
        instance_number: rec.instance.number.clone(),
        transfer_syntax_uid: fm.transfer_syntax_uid.clone(),
        ..Default::default()
    };
    if let Some(p) = &rec.patient {
        record.patient_name = p.name.clone();
        record.patient_id = p.id.clone();
    }
    record.specific_character_set = selected_string(result, "charset").unwrap_or_default();
    record.study_id = selected_string(result, "study_id").unwrap_or_default();
    record.related_general_sop_class_uids =
        selected_strings(result, "related_general_sop_class").unwrap_or_default();
    for field in [
        &mut record.patient_name,
        &mut record.patient_id,
        &mut record.study_date,
        &mut record.study_time,
        &mut record.study_description,
        &mut record.study_id,
        &mut record.accession_number,
        &mut record.modality,
        &mut record.series_number,
        &mut record.instance_number,
        &mut record.specific_character_set,
    ] {
        let trimmed = field.trim_end_matches([' ', '\0']).len();
        field.truncate(trimmed);
    }
    if invent && !root_type {
        invent_missing_keys(&mut record, result);
    }
    if root_type {
        for field in [
            &mut record.patient_id,
            &mut record.patient_name,
            &mut record.study_date,
            &mut record.study_time,
            &mut record.study_description,
            &mut record.study_instance_uid,
            &mut record.study_id,
            &mut record.accession_number,
            &mut record.series_instance_uid,
            &mut record.modality,
            &mut record.series_number,
            &mut record.instance_number,
        ] {
            field.clear();
        }
    }
    let has_instance_key = directory_key_rules(record_type.as_str())
        .iter()
        .any(|r| r.tag == TAG_INSTANCE_NUMBER);
    if !has_instance_key {
        record.instance_number.clear();
        record.invented_keys.retain(|k| k != "instance_number");
    }
    Ok(record)
}
fn invent_missing_keys(record: &mut FileRecord, result: &IndexResult) {
    let first_of = |fallback: &str, ids: &[&str]| -> String {
        for id in ids {
            if let Some(v) = selected_string(result, id)
                && !v.trim().is_empty()
            {
                return v.trim().to_string();
            }
        }
        fallback.to_string()
    };
    let mut invented = Vec::new();
    if record.patient_id.trim().is_empty() {
        record.patient_id = format!("PAT{}", uid_digest(&record.study_instance_uid));
        invented.push("patient_id");
    }
    if record.study_date.trim().is_empty() {
        record.study_date = first_of(
            "19000101",
            &["series_date", "acquisition_date", "content_date"],
        );
        invented.push("study_date");
    }
    if record.study_time.trim().is_empty() {
        record.study_time = first_of(
            "000000",
            &["series_time", "acquisition_time", "content_time"],
        );
        invented.push("study_time");
    }
    if record.study_id.trim().is_empty() {
        record.study_id = format!("S{}", uid_digest(&record.study_instance_uid));
        invented.push("study_id");
    }
    if record.series_number.trim().is_empty() {
        record.series_number = uid_number(&record.series_instance_uid);
        invented.push("series_number");
    }
    if record.instance_number.trim().is_empty() {
        record.instance_number = uid_number(&record.sop_instance_uid);
        invented.push("instance_number");
    }
    record
        .invented_keys
        .extend(invented.into_iter().map(str::to_string));
}
pub(crate) fn snapshot_file_records(records: &[SourceSnapshot]) -> Vec<SourceSnapshot> {
    let mut out = records.to_vec();
    for (key, fallback, date) in [
        ("study_date", "19000101", true),
        ("study_time", "000000", false),
    ] {
        #[derive(Clone)]
        struct StudyValue {
            value: String,
            explicit: bool,
            conflict: bool,
        }
        let get = |r: &FileRecord| -> String {
            if date {
                r.study_date.clone()
            } else {
                r.study_time.clone()
            }
        };
        let mut studies: HashMap<String, StudyValue> = HashMap::new();
        for stored in &out {
            let r = &stored.record;
            let value = get(r);
            let explicit = !r.invented_keys.iter().any(|k| k == key);
            let next = match studies.get(&r.study_instance_uid) {
                None => StudyValue {
                    value,
                    explicit,
                    conflict: false,
                },
                Some(prev) if explicit && !prev.explicit => StudyValue {
                    value,
                    explicit,
                    conflict: false,
                },
                Some(prev) => {
                    let mut p = prev.clone();
                    if explicit == p.explicit && value != p.value {
                        p.conflict = true;
                    }
                    p
                }
            };
            studies.insert(r.study_instance_uid.clone(), next);
        }
        for stored in out.iter_mut() {
            let r = &mut stored.record;
            if !r.invented_keys.iter().any(|k| k == key) {
                continue;
            }
            let Some(v) = studies.get(&r.study_instance_uid) else {
                continue;
            };
            let value = if v.conflict {
                fallback.to_string()
            } else {
                v.value.clone()
            };
            if date {
                r.study_date = value;
            } else {
                r.study_time = value;
            }
        }
    }
    out
}
pub(crate) fn adopt_invented_keys(dst: &mut FileRecord, src: &FileRecord) {
    for key in dst.invented_keys.clone() {
        let (d, s) = match key.as_str() {
            "patient_id" => (&mut dst.patient_id, &src.patient_id),
            "study_date" => (&mut dst.study_date, &src.study_date),
            "study_time" => (&mut dst.study_time, &src.study_time),
            "study_id" => (&mut dst.study_id, &src.study_id),
            "series_number" => (&mut dst.series_number, &src.series_number),
            "instance_number" => (&mut dst.instance_number, &src.instance_number),
            _ => continue,
        };
        if !s.is_empty() {
            *d = s.clone();
        }
    }
}
fn uid_sum(uid: &str) -> [u8; 32] {
    Sha256::digest(normalize_uid(uid).as_bytes()).into()
}
fn uid_digest(uid: &str) -> String {
    uid_sum(uid)[..6]
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect()
}
fn uid_number(uid: &str) -> String {
    let sum = uid_sum(uid);
    let v = u64::from_be_bytes([
        sum[0], sum[1], sum[2], sum[3], sum[4], sum[5], sum[6], sum[7],
    ]);
    (v % 999_999_999).saturating_add(1).to_string()
}
pub(crate) fn validate_file_record(
    record: &FileRecord,
    admission: Option<&Limits>,
) -> Result<(), Error> {
    if record.sop_class_uid.is_empty()
        || record.sop_instance_uid.is_empty()
        || record.transfer_syntax_uid.is_empty()
    {
        return Err(ErrorKind::MissingRequiredKey.into());
    }
    let root_type = root_file_record_type(record.record_type.as_str());
    if !root_type
        && (record.patient_id.is_empty()
            || record.study_date.is_empty()
            || record.study_time.is_empty()
            || record.study_instance_uid.is_empty()
            || record.study_id.is_empty()
            || record.modality.is_empty()
            || record.series_instance_uid.is_empty()
            || record.series_number.is_empty())
    {
        return Err(Error::missing_key());
    }
    if record.record_type == RecordType::IMAGE && record.instance_number.is_empty() {
        return Err(ErrorKind::InvalidRecord.into());
    }
    for uid in [
        &record.study_instance_uid,
        &record.series_instance_uid,
        &record.sop_class_uid,
        &record.sop_instance_uid,
        &record.transfer_syntax_uid,
    ] {
        if root_type && uid.is_empty() {
            continue;
        }
        if !valid_uid(normalize_uid(uid)) {
            return Err(ErrorKind::InvalidRecord.into());
        }
    }
    if !root_type
        && (record.study_instance_uid == record.series_instance_uid
            || record.study_instance_uid == record.sop_instance_uid
            || record.series_instance_uid == record.sop_instance_uid)
    {
        return Err(ErrorKind::InvalidRecord.into());
    }
    let mut validated = record.clone();
    for uid in validated.related_general_sop_class_uids.iter_mut() {
        let normalized = normalize_uid(uid).to_string();
        if !valid_uid(&normalized) {
            return Err(ErrorKind::InvalidRecord.into());
        }
        *uid = normalized;
    }
    let types: Vec<RecordType> = if root_type {
        vec![record.record_type.clone()]
    } else {
        vec![
            RecordType::PATIENT,
            RecordType::STUDY,
            RecordType::SERIES,
            record.record_type.clone(),
        ]
    };
    let items: Vec<DataSet> = types
        .into_iter()
        .map(|typ| {
            let node = Node::new(typ, validated.clone());
            directory_record_data_set(&node, 0, 0, None)
        })
        .collect();
    let dataset = DataSet {
        elements: vec![sequence_element(TAG_DIRECTORY_RECORD_SEQUENCE, items)],
        item_offset: None,
    };
    if !strict_valid(&dataset, 258, 100_000) {
        return Err(ErrorKind::InvalidRecord.into());
    }
    if !record.keys.elements.is_empty() {
        validate_directory_keys(
            record.record_type.as_str(),
            &record.keys,
            &record.sop_class_uid,
            admission,
        )?;
    }
    Ok(())
}
fn record_uid_roles(record: &FileRecord) -> [&str; 3] {
    [
        &record.study_instance_uid,
        &record.series_instance_uid,
        &record.sop_instance_uid,
    ]
}
pub(crate) fn statistics_for_records(records: &[SourceSnapshot]) -> Statistics {
    let mut stats = Statistics::default();
    let mut patients = HashSet::new();
    let mut studies = HashSet::new();
    let mut series = HashSet::new();
    for stored in records {
        let r = &stored.record;
        stats.files = stats.files.saturating_add(1);
        stats.instances = stats.instances.saturating_add(1);
        stats.bytes = stats.bytes.saturating_add(stored.stamp.size);
        if !r.patient_id.is_empty() {
            patients.insert(r.patient_id.clone());
        }
        if !r.study_instance_uid.is_empty() {
            studies.insert(r.study_instance_uid.clone());
        }
        if !r.series_instance_uid.is_empty() {
            series.insert(r.series_instance_uid.clone());
        }
        let by_type = stats.by_type.entry(r.record_type.to_string()).or_default();
        *by_type = by_type.saturating_add(1);
        let by_modality = stats.by_modality.entry(r.modality.clone()).or_default();
        *by_modality = by_modality.saturating_add(1);
    }
    stats.patients = patients.len() as i64;
    stats.studies = studies.len() as i64;
    stats.series = series.len() as i64;
    stats
}
pub(crate) fn diagnostic_message(code: DiagnosticCode) -> &'static str {
    match code {
        DiagnosticCode::INVALID_FILE_ID => "file ID is not portable",
        DiagnosticCode::UNSUPPORTED => "SOP class is not supported",
        DiagnosticCode::MISSING_KEY => "required directory key is missing",
        DiagnosticCode::DUPLICATE => "directory identifier is duplicated",
        DiagnosticCode::SOURCE_CHANGED => "source file changed",
        _ => "file is not a supported DICOM instance",
    }
}
pub(crate) fn sorted_file_records(records: &[SourceSnapshot]) -> Vec<SourceSnapshot> {
    let mut out = records.to_vec();
    let key = |r: &FileRecord| {
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            r.patient_id,
            r.study_instance_uid,
            r.series_instance_uid,
            r.record_type,
            r.sop_instance_uid,
            r.file_id
        )
    };
    out.sort_by_cached_key(|s| key(&s.record));
    out
}
