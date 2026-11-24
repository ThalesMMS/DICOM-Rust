#![forbid(unsafe_code)]
use super::coercion::*;
use super::controls::{AccessCheck, context_err};
use super::error::{Error, Kind, Result};
use crate::dicomdir::pathrules;
use crate::index;
use crate::net::ul::Ctx;
use crate::object::deferred::{DeferOptions, read_file_deferred};
use crate::object::parser::source::MemFile;
use crate::object::{
    self, ElementHeader, File, Object, ReadFileOptions, Syntax, Tag, element_length, is_valid_uid,
};
use crate::transfer::{NativeStorePreference, proposed_store_transfer_syntax_uids};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const TAG_MEDIA_STORAGE_SOP_CLASS_UID: Tag = Tag::new(0x0002, 0x0002);
const TAG_MEDIA_STORAGE_SOP_INSTANCE_UID: Tag = Tag::new(0x0002, 0x0003);
const TAG_TRANSFER_SYNTAX_UID: Tag = Tag::new(0x0002, 0x0010);
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0008);
const TAG_DOUBLE_FLOAT_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0009);
const DEFLATED_UID: &str = "1.2.840.10008.1.2.1.99";
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoreDescriptor {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub transfer_syntax_uid: String,
    pub writable_transfer_syntax_uids: Vec<String>,
    pub size: i64,
    pub transfer_syntax_sizes: Option<BTreeMap<String, i64>>,
    pub origin: String,
    pub pixel_data_header: Option<ElementHeader>,
}
pub type StoreSink<'a> = dyn FnMut(&[u8]) -> Result<()> + 'a;
pub type StoreDataSetWriter =
    Arc<dyn Fn(&Ctx, &mut StoreSink<'_>, &Syntax) -> Result<()> + Send + Sync>;
pub(crate) type StoreMetadataReader =
    Arc<dyn Fn(&Ctx, index::Options) -> Result<index::IndexResult> + Send + Sync>;
pub(crate) type StoreSourceCloser = Box<dyn FnOnce() -> Result<()> + Send>;
#[derive(Default)]
pub struct OpenedStoreSource {
    pub descriptor: StoreDescriptor,
    pub write_data_set: Option<StoreDataSetWriter>,
    pub close: Option<StoreSourceCloser>,
    pub before_store: Option<Arc<AccessCheck>>,
    pub read_metadata: Option<StoreMetadataReader>,
}
impl std::fmt::Debug for OpenedStoreSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedStoreSource")
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}
pub trait StoreSource {
    fn inspect(&self, ctx: &Ctx) -> Result<StoreDescriptor>;
    fn open(&self, ctx: &Ctx) -> Result<OpenedStoreSource>;
    fn open_for_transfer_syntax(
        &self,
        _ctx: &Ctx,
        _syntax: &Syntax,
    ) -> Option<Result<OpenedStoreSource>> {
        None
    }
}
fn invalid_source() -> Error {
    Error::sentinel(Kind::StoreInvalidSource)
}
fn object_sentinel(kind: object::Kind) -> Error {
    Error::bare_with(kind.text(), vec![super::error::Cause::Obj(kind)])
}
fn index_result(
    (result, err): (index::IndexResult, Option<index::Error>),
) -> Result<index::IndexResult> {
    match err {
        None => Ok(result),
        Some(e) => Err(Error::new(e.to_string())),
    }
}
fn classify_store_source_error(ctx: &Ctx, err: Option<Error>) -> Error {
    if let Some(e) = context_err(ctx) {
        return e;
    }
    match err {
        Some(e) if e.is_canceled() || e.is_deadline() => e,
        _ => invalid_source(),
    }
}
pub(crate) fn store_syntax_uid_present(values: &[String], target: &str) -> bool {
    values.iter().any(|v| v == target)
}
fn normalize_uid(uid: &str) -> &str {
    object::transfer::normalize_uid(uid)
}
pub(crate) fn normalize_store_descriptor(mut d: StoreDescriptor) -> Result<StoreDescriptor> {
    d.sop_class_uid = trim_unicode_space(&d.sop_class_uid).to_string();
    d.sop_instance_uid = trim_unicode_space(&d.sop_instance_uid).to_string();
    d.transfer_syntax_uid = normalize_uid(&d.transfer_syntax_uid).to_string();
    if !is_valid_uid(d.sop_class_uid.as_bytes())
        || !is_valid_uid(d.sop_instance_uid.as_bytes())
        || !is_valid_uid(d.transfer_syntax_uid.as_bytes())
        || d.size < 0
    {
        return Err(invalid_source());
    }
    if object::transfer::lookup(&d.transfer_syntax_uid).is_none() {
        return Err(Error::sentinel(Kind::StoreTransferSyntax));
    }
    let mut writable: Vec<String> = Vec::with_capacity(d.writable_transfer_syntax_uids.len() + 1);
    for value in &d.writable_transfer_syntax_uids {
        let uid = normalize_uid(value);
        if uid.is_empty() || writable.iter().any(|w| w == uid) {
            continue;
        }
        if object::transfer::lookup(uid).is_none() {
            return Err(Error::sentinel(Kind::StoreTransferSyntax));
        }
        writable.push(uid.to_string());
    }
    if writable.is_empty() {
        writable.push(d.transfer_syntax_uid.clone());
    }
    d.writable_transfer_syntax_uids = writable;
    if let Some(sizes) = d.transfer_syntax_sizes.take() {
        let mut bounds: BTreeMap<String, i64> = BTreeMap::new();
        for (key, bound) in &sizes {
            let uid = normalize_uid(key);
            if *bound <= 0 || !store_syntax_uid_present(&d.writable_transfer_syntax_uids, uid) {
                return Err(invalid_source());
            }
            if bounds.contains_key(uid) {
                return Err(invalid_source());
            }
            bounds.insert(uid.to_string(), *bound);
        }
        let own = bounds.get(&d.transfer_syntax_uid).copied().unwrap_or(0);
        if bounds.len() != d.writable_transfer_syntax_uids.len()
            || (d.size > 0 && own != 0 && own != d.size)
        {
            return Err(invalid_source());
        }
        d.transfer_syntax_sizes = Some(bounds);
    }
    validate_store_pixel_header(&d)?;
    Ok(d)
}
fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{85}' | '\u{A0}'
        ) || (c > '\u{FF}' && c.is_whitespace())
    })
}
fn validate_store_pixel_header(d: &StoreDescriptor) -> Result<()> {
    let Some(header) = d.pixel_data_header else {
        return Ok(());
    };
    let Some(syntax) = object::transfer::lookup(&d.transfer_syntax_uid) else {
        return Err(Error::sentinel(Kind::StoreTransferSyntax));
    };
    let integer = header.tag == TAG_PIXEL_DATA;
    let float = header.tag == TAG_FLOAT_PIXEL_DATA || header.tag == TAG_DOUBLE_FLOAT_PIXEL_DATA;
    if !integer && !float {
        return Err(invalid_source());
    }
    if float {
        if header.is_undefined() || syntax.encapsulated {
            return Err(invalid_source());
        }
        return Ok(());
    }
    if syntax.encapsulated {
        if !header.is_undefined() || (header.vr != object::Vr::OB && header.vr != object::Vr::OW) {
            return Err(invalid_source());
        }
        return Ok(());
    }
    if header.is_undefined() {
        return Err(invalid_source());
    }
    Ok(())
}
pub(crate) fn store_pixel_data_header(dataset: &Object) -> Result<Option<ElementHeader>> {
    let mut found: Option<ElementHeader> = None;
    for tag in [
        TAG_PIXEL_DATA,
        TAG_FLOAT_PIXEL_DATA,
        TAG_DOUBLE_FLOAT_PIXEL_DATA,
    ] {
        let Some(element) = dataset.get(tag) else {
            continue;
        };
        if found.is_some() {
            return Err(invalid_source());
        }
        let mut header = element.header;
        if !header.length_set {
            header.length = element_length(element);
            header.length_set = true;
        }
        found = Some(header);
    }
    Ok(found)
}
fn descriptor_from_file(
    file: Option<&File>,
    size: i64,
    origin: &str,
    native_fallbacks: bool,
) -> Result<StoreDescriptor> {
    let Some(file) = file else {
        return Err(invalid_source());
    };
    let class = match file.dataset.get_uid(TAG_SOP_CLASS_UID) {
        Some(u) if !trim_unicode_space(&u).is_empty() => u,
        _ => {
            return Err(object_sentinel(object::Kind::MissingSopClassUid));
        }
    };
    let instance = match file.dataset.get_uid(TAG_SOP_INSTANCE_UID) {
        Some(u) if !trim_unicode_space(&u).is_empty() => u,
        _ => {
            return Err(object_sentinel(object::Kind::MissingSopInstanceUid));
        }
    };
    let mut ts = normalize_uid(file.transfer_syntax.uid).to_string();
    let meta_ts = file.meta.get_uid(TAG_TRANSFER_SYNTAX_UID);
    if ts.is_empty()
        && let Some(value) = &meta_ts
    {
        ts = normalize_uid(value).to_string();
    }
    if ts.is_empty() {
        return Err(object_sentinel(object::Kind::MissingTransferSyntax));
    }
    if let Some(value) = file.meta.get_uid(TAG_MEDIA_STORAGE_SOP_CLASS_UID)
        && !trim_unicode_space(&value).is_empty()
        && trim_unicode_space(&value) != trim_unicode_space(&class)
    {
        return Err(invalid_source());
    }
    if let Some(value) = file.meta.get_uid(TAG_MEDIA_STORAGE_SOP_INSTANCE_UID)
        && !trim_unicode_space(&value).is_empty()
        && trim_unicode_space(&value) != trim_unicode_space(&instance)
    {
        return Err(invalid_source());
    }
    if let Some(value) = &meta_ts
        && !normalize_uid(value).is_empty()
        && normalize_uid(value) != ts
    {
        return Err(invalid_source());
    }
    let writable = if native_fallbacks {
        proposed_store_transfer_syntax_uids(&ts, NativeStorePreference::SourceFirst)
    } else {
        vec![ts.clone()]
    };
    let pixel = store_pixel_data_header(&file.dataset)?;
    normalize_store_descriptor(StoreDescriptor {
        sop_class_uid: class,
        sop_instance_uid: instance,
        transfer_syntax_uid: ts,
        writable_transfer_syntax_uids: writable,
        size,
        transfer_syntax_sizes: None,
        origin: origin.to_string(),
        pixel_data_header: pixel,
    })
}
fn descriptor_from_index(
    result: &index::IndexResult,
    origin: &str,
    preference: NativeStorePreference,
) -> Result<StoreDescriptor> {
    let record = &result.record;
    let class = trim_unicode_space(&record.instance.sop_class_uid).to_string();
    let instance = trim_unicode_space(&record.instance.sop_instance_uid).to_string();
    let ts = normalize_uid(&record.file_meta.transfer_syntax_uid).to_string();
    let media = trim_unicode_space(&record.file_meta.media_storage_sop_class_uid);
    if !media.is_empty() && media != class {
        return Err(invalid_source());
    }
    let media = trim_unicode_space(&record.file_meta.media_storage_sop_instance_uid);
    if !media.is_empty() && media != instance {
        return Err(invalid_source());
    }
    let writable = proposed_store_transfer_syntax_uids(&ts, preference);
    normalize_store_descriptor(StoreDescriptor {
        sop_class_uid: class,
        sop_instance_uid: instance,
        transfer_syntax_uid: ts,
        writable_transfer_syntax_uids: writable,
        size: record.source.size,
        transfer_syntax_sizes: None,
        origin: origin.to_string(),
        pixel_data_header: result.stopped_header,
    })
}
#[doc(hidden)]
pub fn write_object_data_set(
    sink: &mut StoreSink<'_>,
    dataset: &Object,
    syntax: &Syntax,
) -> Result<()> {
    let bytes = object::writer::write_data_set(dataset, syntax.uid).map_err(Error::from)?;
    for chunk in bytes.chunks(4096) {
        sink(chunk)?;
    }
    Ok(())
}
fn copy_store_data_set(ctx: &Ctx, sink: &mut StoreSink<'_>, source: &[u8]) -> Result<()> {
    let mut rest = source;
    loop {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        if rest.is_empty() {
            return Ok(());
        }
        let n = rest.len().min(64 << 10);
        sink(&rest[..n])?;
        rest = &rest[n..];
    }
}
#[derive(Clone, Default)]
pub struct StorePathSourceOptions {
    pub index_options: index::Options,
    pub read_options: ReadFileOptions,
    pub native_preference: NativeStorePreference,
}
pub type StoreFileOpener = Arc<dyn Fn(&Ctx) -> Result<Option<Vec<u8>>> + Send + Sync>;
struct PathStoreSource {
    path: String,
    root: String,
    relative: String,
    options: StorePathSourceOptions,
    descriptor: Option<StoreDescriptor>,
    open_source: Option<StoreFileOpener>,
}
pub fn new_path_store_source(
    path: &str,
    options: Option<StorePathSourceOptions>,
) -> Box<dyn StoreSource> {
    Box::new(PathStoreSource {
        path: path.to_string(),
        root: String::new(),
        relative: String::new(),
        options: options.unwrap_or_default(),
        descriptor: None,
        open_source: None,
    })
}
pub fn new_rooted_path_store_source(
    root: &str,
    relative: &str,
    options: Option<StorePathSourceOptions>,
) -> Box<dyn StoreSource> {
    let root = pathrules::clean(root);
    let relative = pathrules::clean(relative);
    Box::new(PathStoreSource {
        path: pathrules::join(&[&root, &relative]),
        root,
        relative,
        options: options.unwrap_or_default(),
        descriptor: None,
        open_source: None,
    })
}
fn described_path_store_source(
    path: &str,
    descriptor: StoreDescriptor,
    options: Option<StorePathSourceOptions>,
) -> PathStoreSource {
    let options = options.unwrap_or_default();
    let mut copy = descriptor;
    copy.writable_transfer_syntax_uids =
        proposed_store_transfer_syntax_uids(&copy.transfer_syntax_uid, options.native_preference);
    PathStoreSource {
        path: path.to_string(),
        root: String::new(),
        relative: String::new(),
        options,
        descriptor: Some(copy),
        open_source: None,
    }
}
pub fn new_described_path_store_source(
    path: &str,
    descriptor: StoreDescriptor,
    options: Option<StorePathSourceOptions>,
) -> Box<dyn StoreSource> {
    Box::new(described_path_store_source(path, descriptor, options))
}
pub fn new_described_file_store_source(
    descriptor: StoreDescriptor,
    open: Option<StoreFileOpener>,
    options: Option<StorePathSourceOptions>,
) -> Box<dyn StoreSource> {
    let path = if open.is_some() {
        descriptor.origin.clone()
    } else {
        String::new()
    };
    let mut source = described_path_store_source(&path, descriptor, options);
    source.open_source = open;
    Box::new(source)
}
fn is_link(path: &Path) -> std::io::Result<bool> {
    Ok(std::fs::symlink_metadata(path)?.file_type().is_symlink())
}
#[cfg(unix)]
fn link_owned_by_root(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.uid() == 0)
}
#[cfg(not(unix))]
fn link_owned_by_root(_path: &Path) -> bool {
    false
}
fn read_regular_no_follow(path: &Path) -> Result<Vec<u8>> {
    let mut current = PathBuf::new();
    let mut below_root = false;
    for component in path.components() {
        current.push(component);
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => continue,
            _ => {}
        }
        below_root = true;
        if is_link(&current).map_err(|_| invalid_source())? && !link_owned_by_root(&current) {
            return Err(invalid_source());
        }
    }
    if !below_root {
        return Err(invalid_source());
    }
    let meta = std::fs::metadata(path).map_err(|_| invalid_source())?;
    if !meta.is_file() {
        return Err(invalid_source());
    }
    std::fs::read(path).map_err(|_| invalid_source())
}
fn open_store_path_no_follow(path: &str) -> Result<Vec<u8>> {
    let abs = pathrules::abs(path).map_err(|_| invalid_source())?;
    #[cfg(windows)]
    {
        if pathrules::volume_name(&abs).is_empty() {
            return Err(invalid_source());
        }
        read_regular_no_follow(Path::new(&abs))
    }
    #[cfg(not(windows))]
    {
        let parent = std::fs::canonicalize(pathrules::dir(&abs)).map_err(|_| invalid_source())?;
        let target = parent.join(pathrules::base(&abs));
        if is_link(&target).map_err(|_| invalid_source())? && !link_owned_by_root(&target) {
            return Err(invalid_source());
        }
        let meta = std::fs::metadata(&target).map_err(|_| invalid_source())?;
        if !meta.is_file() {
            return Err(invalid_source());
        }
        std::fs::read(&target).map_err(|_| invalid_source())
    }
}
fn open_store_rooted_path_no_follow(root: &str, relative: &str) -> Result<Vec<u8>> {
    if !pathrules::is_abs(root) {
        return Err(invalid_source());
    }
    let joined = pathrules::join(&[root, relative]);
    #[cfg(windows)]
    {
        open_store_path_no_follow(&joined)
    }
    #[cfg(not(windows))]
    {
        read_regular_no_follow(Path::new(&joined))
    }
}
impl PathStoreSource {
    fn unusable(&self) -> bool {
        self.open_source.is_none() && trim_unicode_space(&self.path).is_empty()
    }
    fn open_regular_path(&self, ctx: &Ctx) -> Result<Vec<u8>> {
        if let Some(open) = &self.open_source {
            return match open(ctx)? {
                Some(data) => Ok(data),
                None => Err(invalid_source()),
            };
        }
        if self.root.is_empty() {
            return open_store_path_no_follow(&self.path);
        }
        let parent_prefix = format!("..{}", pathrules::sep_str());
        if pathrules::is_abs(&self.relative)
            || self.relative == "."
            || self.relative == ".."
            || self.relative.starts_with(&parent_prefix)
        {
            return Err(invalid_source());
        }
        open_store_rooted_path_no_follow(&self.root, &self.relative)
    }
    fn index(&self, ctx: &Ctx, data: &[u8]) -> Result<index::IndexResult> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        let (result, err) = index::read_bytes(&self.path, data, self.options.index_options.clone());
        match err {
            None => Ok(result),
            Some(_) => Err(classify_store_source_error(ctx, None)),
        }
    }
}
fn store_reencode_read_options(
    size: i64,
    deflated: bool,
    read_options: ReadFileOptions,
) -> (ReadFileOptions, DeferOptions) {
    const EXPANSION: i64 = 1024;
    let mut read = read_options;
    if read.max_total_bytes == 0 {
        if deflated && size <= i64::MAX / EXPANSION {
            read.max_total_bytes = size * EXPANSION;
        } else if size < i64::MAX {
            read.max_total_bytes = size + 1;
        }
    }
    if read.max_element_bytes == 0 {
        read.max_element_bytes = read.max_total_bytes;
        if read.max_element_bytes <= 0 {
            read.max_element_bytes = 64 << 20;
        }
    }
    let limits = index::default_options().limits;
    if read.max_sequence_depth == 0 {
        read.max_sequence_depth = limits.max_sequence_depth;
    }
    if read.max_elements == 0 {
        read.max_elements = limits.max_elements;
    }
    if read.max_fragments == 0 {
        read.max_fragments = limits.max_fragments;
    }
    let defer = DeferOptions {
        defer_pixel_data: !deflated,
        inline_value_bytes_threshold: 1 << 20,
        ..Default::default()
    };
    (read, defer)
}
impl StoreSource for PathStoreSource {
    fn inspect(&self, ctx: &Ctx) -> Result<StoreDescriptor> {
        if self.unusable() {
            return Err(invalid_source());
        }
        if let Some(d) = &self.descriptor {
            return normalize_store_descriptor(d.clone());
        }
        let data = self
            .open_regular_path(ctx)
            .map_err(|e| classify_store_source_error(ctx, Some(e)))?;
        let result = self.index(ctx, &data)?;
        descriptor_from_index(&result, &self.path, self.options.native_preference)
    }
    fn open(&self, ctx: &Ctx) -> Result<OpenedStoreSource> {
        if self.unusable() {
            return Err(invalid_source());
        }
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        let data = self
            .open_regular_path(ctx)
            .map_err(|e| classify_store_source_error(ctx, Some(e)))?;
        let indexed = self.index(ctx, &data)?;
        let descriptor =
            descriptor_from_index(&indexed, &self.path, self.options.native_preference)?;
        let shared: Rc<[u8]> = Rc::from(data);
        let header = object::read_part10_header_mem(
            &MemFile::new(shared.clone()),
            &self.options.read_options,
        )
        .map_err(|_| classify_store_source_error(ctx, None))?;
        let offset = header.data_set_offset.max(0) as usize;
        let handle: Arc<Mutex<Option<Arc<[u8]>>>> = Arc::new(Mutex::new(Some(Arc::from(&*shared))));
        let read_options = self.options.read_options;
        let source_ts = descriptor.transfer_syntax_uid.clone();
        let writable = descriptor.writable_transfer_syntax_uids.clone();
        let write_handle = handle.clone();
        let write: StoreDataSetWriter = Arc::new(move |write_ctx, sink, syntax| {
            let requested = normalize_uid(syntax.uid);
            if !store_syntax_uid_present(&writable, requested) {
                return Err(Error::sentinel(Kind::StoreTransferSyntax));
            }
            let Some(data) = write_handle.lock().unwrap().clone() else {
                return Err(invalid_source());
            };
            if requested != source_ts {
                let deflated = normalize_uid(&source_ts) == DEFLATED_UID;
                let (read, defer) =
                    store_reencode_read_options(data.len() as i64, deflated, read_options);
                let parsed = read_file_deferred(&data, true, read, &defer)
                    .map_err(|_| classify_store_source_error(write_ctx, None))?;
                if let Some(e) = context_err(write_ctx) {
                    return Err(e);
                }
                return write_object_data_set(sink, &parsed.dataset, syntax);
            }
            copy_store_data_set(write_ctx, sink, data.get(offset..).unwrap_or(&[]))
        });
        let meta_handle = handle.clone();
        let read_metadata: StoreMetadataReader = Arc::new(move |_ctx, mut options| {
            options.raw_data_set = None;
            match meta_handle.lock().unwrap().clone() {
                Some(data) => index_result(index::read_bytes("", &data, options)),
                None => Err(Error::new("read: file already closed")),
            }
        });
        let close: StoreSourceCloser = Box::new(move || {
            handle.lock().unwrap().take();
            Ok(())
        });
        Ok(OpenedStoreSource {
            descriptor,
            write_data_set: Some(write),
            close: Some(close),
            before_store: None,
            read_metadata: Some(read_metadata),
        })
    }
}
pub type StoreObjectOpener = Arc<dyn Fn(&Ctx) -> (Option<Arc<File>>, Option<Error>) + Send + Sync>;
struct ObjectStoreSource {
    descriptor: StoreDescriptor,
    file: Option<Arc<File>>,
    opener: Option<StoreObjectOpener>,
    owned: bool,
}
pub fn new_file_store_source(file: Option<Arc<File>>, size: i64) -> Box<dyn StoreSource> {
    let descriptor = descriptor_from_file(file.as_deref(), size, "", true).unwrap_or_default();
    Box::new(ObjectStoreSource {
        descriptor,
        file,
        opener: None,
        owned: false,
    })
}
pub fn new_data_set_store_source(
    dataset: Object,
    syntax: Syntax,
    size: i64,
) -> Box<dyn StoreSource> {
    new_file_store_source(
        Some(Arc::new(File {
            preamble: Vec::new(),
            meta: Object::new(),
            dataset,
            transfer_syntax: syntax,
            partial_read: None,
            incomplete_pixel_data: None,
        })),
        size,
    )
}
pub fn new_lazy_store_source(
    descriptor: StoreDescriptor,
    opener: StoreObjectOpener,
) -> Box<dyn StoreSource> {
    let mut copy = descriptor;
    copy.writable_transfer_syntax_uids = proposed_store_transfer_syntax_uids(
        &copy.transfer_syntax_uid,
        NativeStorePreference::SourceFirst,
    );
    Box::new(ObjectStoreSource {
        descriptor: copy,
        file: None,
        opener: Some(opener),
        owned: true,
    })
}
impl StoreSource for ObjectStoreSource {
    fn inspect(&self, ctx: &Ctx) -> Result<StoreDescriptor> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        if self.opener.is_none() {
            return descriptor_from_file(
                self.file.as_deref(),
                self.descriptor.size,
                &self.descriptor.origin,
                true,
            );
        }
        normalize_store_descriptor(self.descriptor.clone())
    }
    fn open(&self, ctx: &Ctx) -> Result<OpenedStoreSource> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        let mut file = self.file.clone();
        if let Some(opener) = &self.opener {
            let (opened, err) = opener(ctx);
            if let Some(e) = err {
                return Err(classify_store_source_error(ctx, Some(e)));
            }
            if opened.is_none() {
                return Err(invalid_source());
            }
            file = opened;
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
        }
        let descriptor = descriptor_from_file(
            file.as_deref(),
            self.descriptor.size,
            &self.descriptor.origin,
            true,
        )?;
        let _ = self.owned;
        let Some(file) = file else {
            return Err(invalid_source());
        };
        let write: StoreDataSetWriter =
            Arc::new(move |_ctx, sink, syntax| write_object_data_set(sink, &file.dataset, syntax));
        Ok(OpenedStoreSource {
            descriptor,
            write_data_set: Some(write),
            close: Some(Box::new(|| Ok(()))),
            before_store: None,
            read_metadata: None,
        })
    }
}
pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(invalid_source()),
    }
}
pub(crate) fn inspect_store_source(ctx: &Ctx, source: &dyn StoreSource) -> Result<StoreDescriptor> {
    normalize_store_descriptor(guarded(|| source.inspect(ctx))?)
}
pub(crate) fn open_store_source(
    ctx: &Ctx,
    source: Option<&dyn StoreSource>,
) -> Result<OpenedStoreSource> {
    let Some(source) = source else {
        return Err(invalid_source());
    };
    guarded(|| source.open(ctx))
}
pub(crate) fn open_store_source_for_syntax(
    ctx: &Ctx,
    source: Option<&dyn StoreSource>,
    syntax: &Syntax,
) -> Result<OpenedStoreSource> {
    let Some(source) = source else {
        return Err(invalid_source());
    };
    guarded(|| match source.open_for_transfer_syntax(ctx, syntax) {
        Some(r) => r,
        None => source.open(ctx),
    })
}
pub(crate) fn store_representation_size(d: &StoreDescriptor, uid: &str) -> i64 {
    match &d.transfer_syntax_sizes {
        Some(sizes) => sizes.get(uid).copied().unwrap_or(0),
        None => d.size,
    }
}
pub(crate) fn store_minimum_representation_size(d: &StoreDescriptor, offered: &[String]) -> i64 {
    match &d.transfer_syntax_sizes {
        None => d.size,
        Some(sizes) => offered
            .iter()
            .map(|uid| sizes.get(uid).copied().unwrap_or(0))
            .fold(i64::MAX, i64::min),
    }
}
pub(crate) fn store_writer_representation_bound(d: &StoreDescriptor, uid: &str) -> i64 {
    match &d.transfer_syntax_sizes {
        Some(sizes) => sizes.get(uid).copied().unwrap_or(0),
        None => 0,
    }
}
pub(crate) fn close_store_source(close: Option<StoreSourceCloser>) -> Result<()> {
    match close {
        None => Err(invalid_source()),
        Some(close) => guarded(close),
    }
}
pub(crate) fn validate_opened_store_source(
    planned: &StoreDescriptor,
    opened: &OpenedStoreSource,
) -> Result<()> {
    if opened.write_data_set.is_none() || opened.close.is_none() {
        return Err(invalid_source());
    }
    let actual = normalize_store_descriptor(opened.descriptor.clone())?;
    if actual.sop_class_uid != planned.sop_class_uid
        || actual.sop_instance_uid != planned.sop_instance_uid
        || actual.transfer_syntax_uid != planned.transfer_syntax_uid
        || (planned.size > 0 && actual.size != planned.size)
    {
        return Err(Error::sentinel(Kind::StoreSourceChanged));
    }
    if actual.writable_transfer_syntax_uids != planned.writable_transfer_syntax_uids
        || actual.transfer_syntax_sizes != planned.transfer_syntax_sizes
    {
        return Err(Error::sentinel(Kind::StoreSourceChanged));
    }
    if planned.pixel_data_header.is_some() && actual.pixel_data_header != planned.pixel_data_header
    {
        return Err(Error::sentinel(Kind::StoreSourceChanged));
    }
    Ok(())
}
type CoercedPayloads = BTreeMap<String, Arc<Vec<u8>>>;
#[derive(Default)]
struct CoercedState {
    descriptor: Option<StoreDescriptor>,
    payloads: Option<Arc<BTreeMap<String, Arc<Vec<u8>>>>>,
    closed: bool,
    opened: i64,
    charged: i64,
}
pub struct CoercedStoreSource {
    base: Box<dyn StoreSource>,
    opts: StoreCoercionOptions,
    state: Arc<Mutex<CoercedState>>,
}
pub fn new_coerced_store_source(
    base: Option<Box<dyn StoreSource>>,
    opts: StoreCoercionOptions,
) -> Result<CoercedStoreSource> {
    let Some(base) = base else {
        return Err(invalid_source());
    };
    let (opts, err) = normalize_store_coercion_options(opts);
    if let Some(e) = err {
        return Err(e);
    }
    Ok(CoercedStoreSource {
        base,
        opts,
        state: Arc::new(Mutex::new(CoercedState::default())),
    })
}
fn budget_reserve(budget: &Option<Arc<StoreCoercionBudget>>, maximum: i64) -> Result<i64> {
    match budget {
        Some(b) => b.reserve(maximum),
        None => Ok(maximum),
    }
}
fn budget_release(budget: &Option<Arc<StoreCoercionBudget>>, bytes: i64) {
    if let Some(b) = budget {
        b.release(bytes);
    }
}
impl CoercedStoreSource {
    fn lock(&self) -> std::sync::MutexGuard<'_, CoercedState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn close(&self) -> Result<()> {
        let mut st = self.lock();
        st.closed = true;
        st.payloads = None;
        st.descriptor = None;
        if st.opened == 0 {
            budget_release(&self.opts.budget, st.charged);
            st.charged = 0;
        }
        Ok(())
    }
    fn prepare(&self, ctx: &Ctx, reserved: i64) -> Result<(StoreDescriptor, CoercedPayloads, i64)> {
        let coerce = |cause: Error| store_coercion_error(cause);
        let mut opts = self.opts.clone();
        opts.clone_options.max_total_bytes = reserved;
        let input = inspect_store_source(ctx, self.base.as_ref()).map_err(coerce)?;
        let input = normalize_store_descriptor(input).map_err(coerce)?;
        let mut output = input.clone();
        if !store_syntax_uid_present(
            &input.writable_transfer_syntax_uids,
            &output.transfer_syntax_uid,
        ) {
            output.transfer_syntax_uid = input.writable_transfer_syntax_uids[0].clone();
        }
        let mut sizes: BTreeMap<String, i64> = BTreeMap::new();
        let mut payloads: BTreeMap<String, Arc<Vec<u8>>> = BTreeMap::new();
        let mut total: i64 = 0;
        for (i, uid) in input.writable_transfer_syntax_uids.iter().enumerate() {
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            let syntax = object::transfer::lookup(uid)
                .unwrap_or(object::transfer::EXPLICIT_VR_LITTLE_ENDIAN);
            let opened = match self.base.open_for_transfer_syntax(ctx, &syntax) {
                Some(r) => r,
                None => open_store_source(ctx, Some(self.base.as_ref())),
            }
            .map_err(coerce)?;
            let mut opened = opened;
            if let Err(e) = validate_opened_store_source(&input, &opened) {
                let _ = close_store_source(opened.close.take());
                return Err(coerce(e));
            }
            let mut options = opts.clone();
            options.original_class = input.sop_class_uid.clone();
            options.original_instance = input.sop_instance_uid.clone();
            let writer = opened.write_data_set.clone().unwrap();
            let encode = move |write_ctx: &Ctx, syntax: &Syntax| -> Result<Vec<u8>> {
                let mut buffer = Vec::new();
                writer(
                    write_ctx,
                    &mut |chunk: &[u8]| {
                        buffer.extend_from_slice(chunk);
                        Ok(())
                    },
                    syntax,
                )?;
                Ok(buffer)
            };
            let prepared = prepare_store_coercion(ctx, &encode, syntax, &options);
            let closed = close_store_source(opened.close.take());
            let file = match (prepared, closed) {
                (Ok((file, _)), Ok(())) => file,
                (Err(e), Ok(())) => return Err(coerce(e)),
                (Ok(_), Err(e)) => return Err(coerce(e)),
                (Err(e), Err(c)) => {
                    return Err(coerce(Error::join([Some(e), Some(c)]).unwrap()));
                }
            };
            let (class, instance) = coerced_store_identity(&file.dataset).unwrap_or_default();
            if i == 0 {
                output.sop_class_uid = class;
                output.sop_instance_uid = instance;
            } else if class != output.sop_class_uid || instance != output.sop_instance_uid {
                return Err(coerce(Error::sentinel(Kind::StoreCoercionIdentity)));
            }
            let maximum = reserved - total;
            let bytes = object::writer::write_data_set(&file.dataset, syntax.uid)
                .map_err(|e| coerce(Error::from(e)))?;
            if let Some(e) = context_err(ctx) {
                return Err(coerce(e));
            }
            if bytes.len() as i64 > maximum {
                return Err(coerce(Error::sentinel(Kind::StoreResourceLimit)));
            }
            let length = bytes.len() as i64;
            payloads.insert(uid.clone(), Arc::new(bytes));
            sizes.insert(uid.clone(), length);
            total += length;
            if *uid == output.transfer_syntax_uid {
                output.size = length;
                output.pixel_data_header =
                    store_pixel_data_header(&file.dataset).map_err(coerce)?;
            }
        }
        output.transfer_syntax_sizes = Some(sizes);
        let output = normalize_store_descriptor(output).map_err(coerce)?;
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        Ok((output, payloads, total))
    }
}
impl StoreSource for CoercedStoreSource {
    fn inspect(&self, ctx: &Ctx) -> Result<StoreDescriptor> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        let mut st = self.lock();
        if st.closed {
            return Err(invalid_source());
        }
        if let Some(d) = &st.descriptor {
            return Ok(d.clone());
        }
        let reserved = budget_reserve(&self.opts.budget, self.opts.clone_options.max_total_bytes)
            .map_err(store_coercion_error)?;
        match self.prepare(ctx, reserved) {
            Ok((output, payloads, total)) => {
                st.descriptor = Some(output.clone());
                st.payloads = Some(Arc::new(payloads));
                st.charged = total;
                budget_release(&self.opts.budget, reserved - total);
                Ok(output)
            }
            Err(e) => {
                budget_release(&self.opts.budget, reserved);
                Err(e)
            }
        }
    }
    fn open(&self, ctx: &Ctx) -> Result<OpenedStoreSource> {
        let descriptor = self.inspect(ctx)?;
        let syntax = object::transfer::lookup(&descriptor.transfer_syntax_uid)
            .unwrap_or(object::transfer::EXPLICIT_VR_LITTLE_ENDIAN);
        match self.open_for_transfer_syntax(ctx, &syntax) {
            Some(r) => r,
            None => Err(invalid_source()),
        }
    }
    fn open_for_transfer_syntax(
        &self,
        ctx: &Ctx,
        syntax: &Syntax,
    ) -> Option<Result<OpenedStoreSource>> {
        let descriptor = match self.inspect(ctx) {
            Ok(d) => d,
            Err(e) => return Some(Err(e)),
        };
        if let Some(e) = context_err(ctx) {
            return Some(Err(e));
        }
        let uid = normalize_uid(syntax.uid).to_string();
        let (payloads, selected) = {
            let mut st = self.lock();
            if st.closed {
                return Some(Err(invalid_source()));
            }
            let Some(payloads) = st.payloads.clone() else {
                return Some(Err(invalid_source()));
            };
            let Some(selected) = payloads.get(&uid).cloned() else {
                return Some(Err(Error::sentinel(Kind::StoreTransferSyntax)));
            };
            st.opened += 1;
            (payloads, selected)
        };
        type View = Option<(Arc<BTreeMap<String, Arc<Vec<u8>>>>, Arc<Vec<u8>>)>;
        let view: Arc<Mutex<View>> = Arc::new(Mutex::new(Some((payloads, selected))));
        let write_view = view.clone();
        let write: StoreDataSetWriter = Arc::new(move |write_ctx, sink, target| {
            let guard = write_view.lock().unwrap();
            let Some((payloads, _)) = guard.as_ref() else {
                return Err(invalid_source());
            };
            let Some(payload) = payloads.get(normalize_uid(target.uid)) else {
                return Err(Error::sentinel(Kind::StoreTransferSyntax));
            };
            copy_store_data_set(write_ctx, sink, payload)
        });
        let meta_view = view.clone();
        let meta_uid = uid.clone();
        let read_metadata: StoreMetadataReader = Arc::new(move |_ctx, mut options| {
            let guard = meta_view.lock().unwrap();
            let Some((_, selected)) = guard.as_ref() else {
                return Err(invalid_source());
            };
            options.raw_data_set = Some(meta_uid.clone());
            index_result(index::read_bytes("", selected, options))
        });
        let state = self.state.clone();
        let budget = self.opts.budget.clone();
        let close: StoreSourceCloser = Box::new(move || {
            let mut guard = view.lock().unwrap();
            if guard.is_none() {
                return Ok(());
            }
            *guard = None;
            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            st.opened -= 1;
            if st.closed && st.opened == 0 {
                budget_release(&budget, st.charged);
                st.charged = 0;
            }
            Ok(())
        });
        Some(Ok(OpenedStoreSource {
            descriptor,
            write_data_set: Some(write),
            close: Some(close),
            before_store: None,
            read_metadata: Some(read_metadata),
        }))
    }
}
