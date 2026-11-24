#![forbid(unsafe_code)]
use super::controls::context_err;
use super::error::{Error, Kind, Result};
use super::mwl::*;
use super::mwl_scp::*;
use crate::net::ul::Ctx;
use crate::object::parser::source::ByteRead;
use crate::object::{self, Object, ReadFileOptions, Tag, Value, Vr};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
const DEFAULT_MAX_FILE_BYTES: i64 = 1 << 20;
const DEFAULT_MAX_FILES: i64 = 10_000;
pub(crate) type MwlFileErrorCallback = Arc<dyn Fn(&str, &Error) + Send + Sync>;
pub(crate) type MwlRejectedRecordCallback = Arc<dyn Fn(&Error) + Send + Sync>;
#[derive(Clone, Default)]
pub struct DirectoryModalityWorklistOptions {
    pub extension: String,
    pub ae_title: String,
    pub max_file_bytes: i64,
    pub max_files: i64,
    pub universal_query_matches_all: bool,
    pub on_file_error: Option<MwlFileErrorCallback>,
}
#[derive(Clone)]
struct CachedFile {
    size: u64,
    mod_time: Option<SystemTime>,
    record: Option<Object>,
}
pub struct DirectoryModalityWorklist {
    root: PathBuf,
    options: DirectoryModalityWorklistOptions,
    cache: Mutex<HashMap<String, HashMap<String, CachedFile>>>,
}
fn io_error(err: std::io::Error) -> Error {
    Error::new(err.to_string())
}
fn resolve(path: &Path) -> std::io::Result<PathBuf> {
    let resolved = fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        let text = resolved.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\")
            && !rest.starts_with("UNC\\")
        {
            return Ok(PathBuf::from(rest.to_string()));
        }
    }
    Ok(resolved)
}
pub fn new_directory_modality_worklist_handler(
    root: &str,
    mut options: DirectoryModalityWorklistOptions,
) -> Result<DirectoryModalityWorklist> {
    if options.max_file_bytes < 0 || options.max_files < 0 {
        return Err(Error::new(
            "dicom dimse: MWL directory limits must not be negative",
        ));
    }
    if options.extension.is_empty() {
        options.extension = ".wl".to_string();
    }
    if options.max_file_bytes == 0 {
        options.max_file_bytes = DEFAULT_MAX_FILE_BYTES;
    }
    if options.max_files == 0 {
        options.max_files = DEFAULT_MAX_FILES;
    }
    let abs = std::path::absolute(root).map_err(io_error)?;
    let resolved = resolve(&abs).map_err(io_error)?;
    let info = fs::metadata(&resolved).map_err(io_error)?;
    if !info.is_dir() {
        return Err(Error::new(format!(
            "dicom dimse: MWL root {root} is not a directory"
        )));
    }
    if !options.ae_title.is_empty() {
        ae_directory_name(&options.ae_title)?;
    }
    Ok(DirectoryModalityWorklist {
        root: resolved,
        options,
        cache: Mutex::new(HashMap::new()),
    })
}
fn ae_directory_name(ae_title: &str) -> Result<String> {
    let name = ae_title.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains("..")
        || name.contains(['/', '\\', ':', '\0'])
    {
        return Err(Error::wrap(
            Kind::ModalityWorklistUnknownAeTitle,
            "invalid AE title",
        ));
    }
    Ok(name.to_string())
}
fn file_ext(name: &str) -> &str {
    name.rfind('.').map_or("", |i| &name[i..])
}
fn equal_fold(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}
struct Bytes {
    data: Vec<u8>,
    pos: usize,
}
impl ByteRead for Bytes {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<object::Error>) {
        if self.pos >= self.data.len() {
            return (0, Some(object::Error::eof()));
        }
        let n = p.len().min(self.data.len() - self.pos);
        p[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        (n, None)
    }
}
impl DirectoryModalityWorklist {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn ae_titles(&self) -> Result<Vec<String>> {
        let mut names: Vec<(String, bool)> = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            names.push((entry.file_name().to_string_lossy().into_owned(), is_dir));
        }
        names.sort();
        Ok(names
            .into_iter()
            .filter(|(name, is_dir)| *is_dir && ae_directory_name(name).is_ok())
            .map(|(name, _)| name)
            .collect())
    }
    fn directory(&self, ae_title: &str) -> Result<PathBuf> {
        let name = ae_directory_name(ae_title)?;
        let path = self.root.join(&name);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => Ok(path),
            _ => Err(Error::sentinel(Kind::ModalityWorklistUnknownAeTitle)),
        }
    }
    pub fn records(&self, ctx: &Ctx, ae_title: &str) -> Result<Vec<Object>> {
        let dir = self.directory(ae_title)?;
        let entries = fs::read_dir(&dir)
            .map_err(|e| Error::wrap(Kind::ModalityWorklistProvider, e.to_string()))?;
        let mut files: Vec<(String, fs::Metadata)> = Vec::new();
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            let regular = entry.file_type().is_ok_and(|t| t.is_file());
            if !regular || !equal_fold(file_ext(&name), &self.options.extension) {
                continue;
            }
            let Ok(info) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            files.push((name, info));
            if files.len() as i64 > self.options.max_files {
                return Err(Error::sentinel(Kind::ModalityWorklistResourceLimit));
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let key = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let previous = self
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let mut current = HashMap::with_capacity(files.len());
        let mut records = Vec::with_capacity(files.len());
        for (name, info) in files {
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            let mod_time = info.modified().ok();
            let cached = match previous.get(&name) {
                Some(c) if c.size == info.len() && c.mod_time == mod_time => c.clone(),
                _ => {
                    let path = dir.join(&name);
                    let read = self.read_file(&path);
                    if let (Err(err), Some(cb)) = (&read, &self.options.on_file_error) {
                        cb(&path.to_string_lossy(), err);
                    }
                    CachedFile {
                        size: info.len(),
                        mod_time,
                        record: read.ok(),
                    }
                }
            };
            if let Some(record) = &cached.record {
                records.push(record.clone());
            }
            current.insert(name, cached);
        }
        self.cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, current);
        Ok(records)
    }
    fn read_file(&self, path: &Path) -> Result<Object> {
        let file = fs::File::open(path).map_err(io_error)?;
        let mut data = Vec::new();
        file.take(self.options.max_file_bytes as u64 + 1)
            .read_to_end(&mut data)
            .map_err(io_error)?;
        if data.len() as i64 > self.options.max_file_bytes {
            return Err(Error::wrap(
                Kind::ModalityWorklistResourceLimit,
                format!(
                    "worklist file exceeds {} bytes",
                    self.options.max_file_bytes
                ),
            ));
        }
        let options = ReadFileOptions {
            max_element_bytes: self.options.max_file_bytes,
            max_sequence_depth: 16,
            ..Default::default()
        };
        if data.len() >= 132 && &data[128..132] == b"DICM" {
            return match object::read_file_with_options(&data, options) {
                Ok(f) => Ok(f.dataset),
                Err((_, e)) => {
                    Err(Error::from(e).prefixed("worklist file is not a readable Part 10 file"))
                }
            };
        }
        let syntax =
            if data.len() >= 6 && data[4].is_ascii_uppercase() && data[5].is_ascii_uppercase() {
                object::transfer::EXPLICIT_VR_LITTLE_ENDIAN
            } else {
                object::transfer::IMPLICIT_VR_LITTLE_ENDIAN
            };
        let record =
            object::read_data_set_with_options(Box::new(Bytes { data, pos: 0 }), syntax, options)
                .map_err(|e| Error::from(e).prefixed("worklist file is not a readable data set"))?;
        if record.is_empty() {
            return Err(Error::new("worklist file holds no attributes"));
        }
        Ok(record)
    }
}
impl ModalityWorklistHandler for DirectoryModalityWorklist {
    fn find(
        &self,
        ctx: &Ctx,
        request: &ModalityWorklistRequest,
        yield_: &mut ModalityWorklistYield<'_>,
    ) -> Result<()> {
        let ae = if self.options.ae_title.is_empty() {
            request.called_ae_title.as_str()
        } else {
            self.options.ae_title.as_str()
        };
        let records = self.records(ctx, ae)?;
        InMemoryModalityWorklistHandler::from_records(
            records,
            InMemoryModalityWorklistOptions {
                universal_query_matches_all: self.options.universal_query_matches_all,
            },
        )
        .find(ctx, request, yield_)
    }
}
#[derive(Clone, Default)]
pub struct DirectoryModalityWorklistPolicyOptions {
    pub directory: DirectoryModalityWorklistOptions,
    pub lock_file: String,
    pub reject_incomplete_files: bool,
    pub request_file_path: String,
    pub max_request_bytes: i64,
    pub max_request_files: i64,
    pub max_stored_request_bytes: i64,
    pub on_rejected_record: Option<MwlRejectedRecordCallback>,
}
pub struct PolicyDirectoryModalityWorklist {
    directory: DirectoryModalityWorklist,
    options: DirectoryModalityWorklistPolicyOptions,
    requests: Mutex<(i64, i64)>,
}
fn resource_limit() -> Error {
    Error::sentinel(Kind::ModalityWorklistResourceLimit)
}
fn provider() -> Error {
    Error::sentinel(Kind::ModalityWorklistProvider)
}
fn nofollow_metadata(path: &Path) -> Result<fs::Metadata> {
    let m = fs::symlink_metadata(path).map_err(io_error)?;
    if m.file_type().is_symlink() {
        return Err(Error::new(format!(
            "{}: symbolic link refused",
            path.to_string_lossy()
        )));
    }
    Ok(m)
}
pub fn new_policy_directory_modality_worklist(
    root: &str,
    mut opts: DirectoryModalityWorklistPolicyOptions,
) -> Result<PolicyDirectoryModalityWorklist> {
    let lock = opts.lock_file.as_str();
    if !lock.is_empty() && (lock == "." || lock == ".." || lock.contains(['/', '\\', ':'])) {
        return Err(Error::new("MWL lockfile must be a single file name"));
    }
    if opts.max_request_bytes < 0 || opts.max_request_files < 0 || opts.max_stored_request_bytes < 0
    {
        return Err(resource_limit());
    }
    if opts.max_request_bytes == 0 {
        opts.max_request_bytes = 1 << 20;
    }
    if opts.max_request_files == 0 {
        opts.max_request_files = 10_000;
    }
    if opts.max_stored_request_bytes == 0 {
        opts.max_stored_request_bytes = 64 << 20;
    }
    if opts.max_request_bytes > 64 << 20
        || opts.max_request_files > 10_000
        || opts.max_stored_request_bytes > 1 << 30
    {
        return Err(resource_limit());
    }
    let directory = new_directory_modality_worklist_handler(root, opts.directory.clone())?;
    let mut files = 0i64;
    let mut bytes = 0i64;
    if !opts.request_file_path.is_empty() {
        let path = Path::new(&opts.request_file_path);
        let m = nofollow_metadata(path)?;
        if !m.is_dir() {
            return Err(Error::new(format!(
                "{}: not a directory",
                opts.request_file_path
            )));
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(path).map_err(io_error)? {
            entries.push(entry.map_err(io_error)?);
            if entries.len() as i64 > opts.max_request_files {
                break;
            }
        }
        if entries.len() as i64 > opts.max_request_files {
            return Err(resource_limit());
        }
        for entry in entries {
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                return Err(Error::new("request directory contains a nonregular entry"));
            }
            let size = entry.metadata().map_err(io_error)?.len() as i64;
            if size > opts.max_stored_request_bytes - bytes {
                return Err(resource_limit());
            }
            bytes += size;
            files += 1;
        }
    }
    Ok(PolicyDirectoryModalityWorklist {
        directory,
        options: opts,
        requests: Mutex::new((files, bytes)),
    })
}
fn acquire_read_lock(ctx: &Ctx, file: &fs::File) -> Result<()> {
    loop {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        match fs4::fs_std::FileExt::try_lock_shared(file) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) => return Err(io_error(e)),
        }
        let end = std::time::Instant::now() + Duration::from_millis(5);
        while std::time::Instant::now() < end {
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
impl PolicyDirectoryModalityWorklist {
    fn write_request(&self, ctx: &Ctx, ds: &Object) -> Result<()> {
        admit_mwl_policy_object(ds, self.options.max_request_bytes)?;
        let mut counters = self.requests.lock().unwrap_or_else(|p| p.into_inner());
        if counters.0 >= self.options.max_request_files
            || counters.1 >= self.options.max_stored_request_bytes
        {
            return Err(resource_limit());
        }
        let remaining = self
            .options
            .max_request_bytes
            .min(self.options.max_stored_request_bytes - counters.1);
        let encoded =
            object::writer::write_data_set(ds, object::transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid)
                .map_err(Error::from)?;
        if encoded.len() as i64 > remaining {
            return Err(resource_limit());
        }
        let dir = Path::new(&self.options.request_file_path);
        let temp = dir.join(format!(".mwl-query-{}", uuid::Uuid::new_v4().simple()));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(io_error)?;
            file.write_all(&encoded).map_err(io_error)?;
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            file.sync_all().map_err(io_error)?;
            drop(file);
            let uid = crate::object::inherit::new_uid();
            fs::hard_link(&temp, dir.join(format!("{uid}.query"))).map_err(io_error)
        })();
        let _ = fs::remove_file(&temp);
        result?;
        counters.0 += 1;
        counters.1 += encoded.len() as i64;
        Ok(())
    }
}
impl ModalityWorklistHandler for PolicyDirectoryModalityWorklist {
    fn find(
        &self,
        ctx: &Ctx,
        request: &ModalityWorklistRequest,
        yield_: &mut ModalityWorklistYield<'_>,
    ) -> Result<()> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        if !self.options.request_file_path.is_empty() {
            self.write_request(ctx, &request.raw_identifier)?;
        }
        let ae = if self.options.directory.ae_title.is_empty() {
            request.called_ae_title.as_str()
        } else {
            self.options.directory.ae_title.as_str()
        };
        let dir = self.directory.directory(ae)?;
        let mut lock_file = None;
        if !self.options.lock_file.is_empty() {
            let path = dir.join(&self.options.lock_file);
            let m = nofollow_metadata(&path)?;
            let file = fs::File::open(&path).map_err(io_error)?;
            if !m.is_file() {
                return Err(Error::new("MWL lockfile must be a regular file"));
            }
            acquire_read_lock(ctx, &file)?;
            lock_file = Some(file);
        }
        let result = (|| {
            let records = self.directory.records(ctx, ae)?;
            let mut kept = Vec::with_capacity(records.len());
            for record in records {
                if let Some(e) = context_err(ctx) {
                    return Err(e);
                }
                if self.options.reject_incomplete_files
                    && let Err(err) = validate_complete_modality_worklist_core_record(Some(&record))
                {
                    if let Some(cb) = &self.options.on_rejected_record {
                        cb(&err);
                    }
                    continue;
                }
                kept.push(record);
            }
            InMemoryModalityWorklistHandler::from_records(
                kept,
                InMemoryModalityWorklistOptions {
                    universal_query_matches_all: self.options.directory.universal_query_matches_all,
                },
            )
            .find(ctx, request, yield_)
        })();
        if let Some(file) = lock_file {
            let _ = fs4::fs_std::FileExt::unlock(&file);
        }
        result
    }
}
pub fn validate_complete_modality_worklist_core_record(record: Option<&Object>) -> Result<()> {
    let Some(record) = record else {
        return Err(provider());
    };
    admit_mwl_policy_object(record, 64 << 20)?;
    let charset = record.get(TAG_MWL_SPECIFIC_CHARACTER_SET).cloned();
    if let Some(e) = &charset {
        validate_result_element(e, None)?;
    }
    let charset = charset.as_ref();
    for tag in [
        TAG_MWL_PATIENT_NAME,
        TAG_MWL_PATIENT_ID,
        TAG_MWL_REQUESTED_PROCEDURE_ID,
    ] {
        match record.get(tag) {
            Some(e) if validate_result_element(e, charset).is_ok() => {}
            _ => {
                return Err(Error::wrap(
                    Kind::ModalityWorklistProvider,
                    format!("incomplete core attribute {tag}"),
                ));
            }
        }
    }
    let study_tag = Tag::new(0x0020, 0x000D);
    match record.get(study_tag) {
        Some(e) if e.vr() == Vr::UI && validate_result_element(e, charset).is_ok() => {}
        _ => return Err(provider()),
    }
    match record.get_uid(study_tag) {
        Some(uid) if object::is_valid_uid(uid.as_bytes()) => {}
        _ => {
            return Err(Error::wrap(
                Kind::ModalityWorklistProvider,
                "incomplete Study Instance UID",
            ));
        }
    }
    match record.get(TAG_MWL_SPS_SEQUENCE) {
        Some(e) if e.vr() == Vr::SQ => {}
        _ => return Err(provider()),
    }
    let step = match record.get_sequence(TAG_MWL_SPS_SEQUENCE) {
        Some(mut s) if s.len() == 1 => s.remove(0),
        _ => {
            return Err(Error::wrap(
                Kind::ModalityWorklistProvider,
                "exactly one SPS is required",
            ));
        }
    };
    for tag in [
        TAG_MWL_SCHEDULED_STATION_AE_TITLE,
        TAG_MWL_SPS_START_DATE,
        TAG_MWL_SPS_START_TIME,
        TAG_MWL_MODALITY,
        TAG_MWL_SPS_ID,
    ] {
        match step.get(tag) {
            Some(e) if validate_result_element(e, charset).is_ok() => {}
            _ => {
                return Err(Error::wrap(
                    Kind::ModalityWorklistProvider,
                    format!("incomplete SPS attribute {tag}"),
                ));
            }
        }
    }
    if let Some(e) = step.get(TAG_MWL_SCHEDULED_PROTOCOL_CODE_SEQUENCE) {
        validate_result_element(e, charset)?;
    }
    let code_tag = Tag::new(0x0032, 0x1064);
    let code_sequence_valid = |e: &object::Element| match &e.value {
        Value::Sequence(items) => {
            e.vr() == Vr::SQ && items.len() == 1 && validate_code_sequence(items, charset).is_ok()
        }
        _ => false,
    };
    if let Some(e) = record.get(code_tag)
        && !code_sequence_valid(e)
    {
        return Err(provider());
    }
    validate_step_description_alternative(&step.to_data_set(), charset)?;
    let description = record.get_string(TAG_MWL_REQUESTED_PROCEDURE_DESCRIPTION);
    if description.is_some_and(|d| !d.trim().is_empty()) {
        if let Some(e) = record.get(TAG_MWL_REQUESTED_PROCEDURE_DESCRIPTION) {
            validate_result_element(e, charset)?;
        }
    } else if !record.get(code_tag).is_some_and(code_sequence_valid) {
        return Err(Error::wrap(
            Kind::ModalityWorklistProvider,
            "requested procedure description/code alternative",
        ));
    }
    for tag in [
        Tag::new(0x0040, 0x0500),
        Tag::new(0x0010, 0x0041),
        Tag::new(0x0010, 0x0043),
        Tag::new(0x0010, 0x0011),
        Tag::new(0x0010, 0x0014),
        Tag::new(0x0038, 0x0100),
        Tag::new(0x0040, 0xA390),
        Tag::new(0x0010, 0x0101),
    ] {
        if record.has(tag) {
            return Err(Error::wrap(
                Kind::ModalityWorklistProvider,
                format!("optional complex profile {tag} is outside core completeness"),
            ));
        }
    }
    for tag in [
        Tag::new(0x0018, 0x990C),
        Tag::new(0x0018, 0x990D),
        Tag::new(0x0040, 0x0440),
    ] {
        if step.has(tag) {
            return Err(Error::wrap(
                Kind::ModalityWorklistProvider,
                format!("optional SPS complex profile {tag} is outside core completeness"),
            ));
        }
    }
    Ok(())
}
fn admit_mwl_policy_object(ds: &Object, max_bytes: i64) -> Result<()> {
    const MAX_ELEMENTS: i64 = 100_000;
    if ds.len() as i64 > MAX_ELEMENTS {
        return Err(resource_limit());
    }
    let mut budget = CloneBudget {
        remaining_elements: MAX_ELEMENTS,
        remaining_bytes: max_bytes,
        max_depth: 32,
        max_multiplicity: MAX_ELEMENTS,
        character_set: None,
    };
    fn walk(budget: &mut CloneBudget, elements: &[object::Element], depth: i64) -> Result<()> {
        if depth > budget.max_depth || elements.len() as i64 > budget.remaining_elements {
            return Err(resource_limit());
        }
        for e in elements {
            budget.consume_element()?;
            match &e.value {
                Value::Strings(_) | Value::Raw(_) | Value::Sequence(_) => {}
                _ => return Err(provider()),
            }
            reserve_policy_value(budget, &e.value)?;
            if let Value::Sequence(items) = &e.value {
                for item in items {
                    budget.consume_element()?;
                    walk(budget, &item.elements, depth + 2)?;
                }
            }
        }
        Ok(())
    }
    walk(&mut budget, ds.elements(), 0)
}
fn reserve_policy_value(budget: &mut CloneBudget, value: &Value) -> Result<()> {
    fn count(budget: &mut CloneBudget, n: i64) -> Result<()> {
        if n < 0 || n > budget.remaining_bytes {
            return Err(resource_limit());
        }
        budget.consume_bytes(n)
    }
    match value {
        Value::Strings(v) => {
            if v.len() as i64 > budget.max_multiplicity {
                return Err(resource_limit());
            }
            let mut parity = 0i64;
            if !v.is_empty() {
                count(budget, v.len() as i64 - 1)?;
                parity = (v.len() as i64 - 1) & 1;
            }
            for s in v {
                count(budget, s.len() as i64)?;
                parity ^= s.len() as i64 & 1;
            }
            budget.consume_bytes(parity)
        }
        Value::Raw(v) => count(budget, v.len() as i64),
        Value::Sequence(_) => budget.reserve_primitive_value(value),
        _ => Err(provider()),
    }
}
