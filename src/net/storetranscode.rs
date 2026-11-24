#![forbid(unsafe_code)]
use crate::index;
use crate::net::dimse::{AccessCheck, context_err};
use crate::net::dimse::{Cause, Error, Kind, Result};
use crate::net::dimse::{
    OpenedStoreSource, StoreDataSetWriter, StoreDescriptor, StoreMetadataReader, StoreSource,
    StoreSourceCloser, new_described_path_store_source, new_file_store_source,
    normalize_store_descriptor,
};
use crate::net::ul::Ctx;
use crate::object::parser::source::MemFile;
use crate::object::transfer::{Syntax, lookup, normalize_uid};
use crate::object::{self, File, ReadFileOptions, is_valid_uid};
use crate::pixel::transcode::{self, TranscodeLimits, TranscodeOptions};
use crate::transfer;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;
mod prepared;
mod source;
pub use prepared::*;
pub use source::*;
pub const ERR_OPTIONS: &str = "dicom: invalid store transcode options";
pub const ERR_RESOURCE_LIMIT: &str = "dicom: store transcode spool limit exceeded";
pub const ERR_PREPARATION: &str = "dicom: store transcode preparation failed";
const IMPLICIT_LE: &str = "1.2.840.10008.1.2";
const EXPLICIT_LE: &str = "1.2.840.10008.1.2.1";
const EXPLICIT_BE: &str = "1.2.840.10008.1.2.2";
const DEFLATED: &str = "1.2.840.10008.1.2.1.99";
const ENCAPSULATED_UNCOMPRESSED: &str = "1.2.840.10008.1.2.1.98";
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn options_error() -> Error {
    Error::new(ERR_OPTIONS)
}
pub fn is_options_error(err: &Error) -> bool {
    err.text() == ERR_OPTIONS
}
struct Failure {
    causes: Vec<Cause>,
    limit: bool,
    context: Option<Error>,
}
impl Failure {
    fn kind(kind: Kind) -> Failure {
        Failure {
            causes: vec![Cause::Dimse(kind)],
            limit: kind == Kind::StoreResourceLimit,
            context: None,
        }
    }
    fn plain() -> Failure {
        Failure {
            causes: Vec::new(),
            limit: false,
            context: None,
        }
    }
    fn limit() -> Failure {
        Failure {
            causes: Vec::new(),
            limit: true,
            context: None,
        }
    }
    fn context(err: Error) -> Failure {
        Failure {
            causes: Vec::new(),
            limit: false,
            context: Some(err),
        }
    }
    fn of(err: &Error) -> Failure {
        Failure {
            causes: err.causes().to_vec(),
            limit: err.is(Kind::StoreResourceLimit) || object_limit(err),
            context: None,
        }
    }
    fn object(err: object::Error) -> Failure {
        Failure::of(&Error::from(err))
    }
    fn transcode(err: &transcode::Error) -> Failure {
        use transcode::ErrorKind as K;
        Failure {
            causes: Vec::new(),
            limit: err.is(K::ResourceLimit) || err.is(K::DecompressResourceLimit),
            context: None,
        }
    }
    fn index(err: &index::Error) -> Failure {
        Failure {
            causes: Vec::new(),
            limit: err.is(index::Sentinel::ResourceLimit),
            context: None,
        }
    }
    fn into_error(self) -> Error {
        if let Some(err) = self.context {
            return err;
        }
        let mut causes = self.causes;
        if self.limit && !causes.contains(&Cause::Dimse(Kind::StoreResourceLimit)) {
            causes.push(Cause::Dimse(Kind::StoreResourceLimit));
        }
        Error::bare_with(ERR_PREPARATION, causes)
    }
}
fn object_limit(err: &Error) -> bool {
    err.is_obj(object::Kind::MaxTotalBytes) || limit_text(err.text())
}
fn limit_text(text: &str) -> bool {
    [
        "dicom: maximum total bytes exceeded",
        "dicom: maximum element value size exceeded",
        "dicom: maximum element count exceeded",
        "dicom: maximum fragment count exceeded",
        "dicom: maximum Pixel Data size exceeded",
        "dicom: maximum sequence nesting depth exceeded",
    ]
    .iter()
    .any(|limit| text.contains(limit))
}
pub struct NativeResources {
    max_preparations: i64,
    max_spool: i64,
    state: Mutex<(i64, i64)>,
    freed: Condvar,
}
impl std::fmt::Debug for NativeResources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeResources")
            .field("max_preparations", &self.max_preparations)
            .field("max_spool", &self.max_spool)
            .finish_non_exhaustive()
    }
}
impl NativeResources {
    pub fn new(
        max_concurrent_preparations: i64,
        max_spool_bytes: i64,
    ) -> Result<Arc<NativeResources>> {
        if max_concurrent_preparations <= 0 || max_spool_bytes <= 0 {
            return Err(options_error());
        }
        Ok(Arc::new(NativeResources {
            max_preparations: max_concurrent_preparations,
            max_spool: max_spool_bytes,
            state: Mutex::new((0, 0)),
            freed: Condvar::new(),
        }))
    }
    pub fn spool_bytes(&self) -> i64 {
        lock(&self.state).0
    }
    fn reserve(&self, n: i64) -> bool {
        let mut state = lock(&self.state);
        if n <= 0 || n > self.max_spool - state.0 {
            return false;
        }
        state.0 += n;
        true
    }
    fn release(&self, n: i64) {
        lock(&self.state).0 -= n;
    }
    fn enter(&self, ctx: &Ctx) -> Result<()> {
        let mut state = lock(&self.state);
        loop {
            if state.1 < self.max_preparations {
                state.1 += 1;
                return Ok(());
            }
            if let Some(e) = context_err(ctx) {
                return Err(e);
            }
            state = self
                .freed
                .wait_timeout(state, Duration::from_millis(20))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }
    fn leave(&self) {
        lock(&self.state).1 -= 1;
        self.freed.notify_one();
    }
}
#[derive(Clone, Default)]
pub struct NativeOptions {
    pub transcode: TranscodeOptions,
    pub dictionary_options: ReadFileOptions,
    pub spool_directory: String,
    pub resources: Option<Arc<NativeResources>>,
    pub before_source_read: Option<Arc<AccessCheck>>,
    pub before_store: Option<Arc<AccessCheck>>,
}
struct NativePath {
    path: String,
    expected_sha256: [u8; 32],
    expected_size: i64,
}
#[derive(Default)]
struct State {
    input: Option<StoreDescriptor>,
    descriptor: Option<StoreDescriptor>,
    digest: Option<[u8; 32]>,
}
pub struct NativeSource {
    base: Box<dyn StoreSource>,
    opts: NativeOptions,
    limits: TranscodeLimits,
    resources: Arc<NativeResources>,
    state: Mutex<State>,
    path: Option<NativePath>,
}
pub fn new_native_source(base: Box<dyn StoreSource>, opts: NativeOptions) -> Result<NativeSource> {
    if opts.transcode.allow_lossy || opts.transcode.force_reencode || opts.transcode.compress_icons
    {
        return Err(options_error());
    }
    let limits =
        transcode::resolve_limits(opts.transcode.limits).map_err(|e| Error::new(e.to_string()))?;
    if limits.max_input_bytes > i64::MAX - limits.max_output_bytes {
        return Err(options_error());
    }
    let resources = match &opts.resources {
        Some(resources) => resources.clone(),
        None => NativeResources::new(1, limits.max_input_bytes + limits.max_output_bytes)?,
    };
    let mut opts = opts;
    opts.transcode.limits = limits;
    Ok(NativeSource {
        base,
        opts,
        limits,
        resources,
        state: Mutex::new(State::default()),
        path: None,
    })
}
pub fn new_native_path_source(
    path: &str,
    descriptor: StoreDescriptor,
    expected_sha256: &str,
    opts: NativeOptions,
) -> Result<NativeSource> {
    let hash = decode_hex(trim_text(expected_sha256));
    let hash: [u8; 32] = match hash.and_then(|h| h.try_into().ok()) {
        Some(hash) => hash,
        None => return Err(options_error()),
    };
    if trim_text(path).is_empty() || descriptor.size <= 0 {
        return Err(options_error());
    }
    let expected_size = descriptor.size;
    let mut source = new_native_source(
        new_described_path_store_source(path, descriptor, None),
        opts,
    )?;
    source.path = Some(NativePath {
        path: path.to_string(),
        expected_sha256: hash,
        expected_size,
    });
    Ok(source)
}
fn trim_text(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{85}' || c == '\u{A0}')
}
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}
fn registered(uid: &str) -> Option<Syntax> {
    lookup(uid)
}
fn media_payload(uid: &str) -> bool {
    transfer::default_registry()
        .get(uid)
        .is_some_and(|syntax| syntax.media_payload)
}
fn incremental_profile(source: &Syntax, target: &Syntax, options: &TranscodeOptions) -> bool {
    source.is_little_endian()
        && !source.deflated
        && target.is_little_endian()
        && !target.deflated
        && !options.allow_lossy
        && !options.secondary_capture
        && options.uid_policy == transcode::UidPolicy::Default
        && options.uid_root.is_empty()
        && !options.compress_icons
}
struct SpoolDir(std::path::PathBuf);
impl SpoolDir {
    fn new(parent: &str) -> Option<SpoolDir> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let parent = if parent.is_empty() {
            std::env::temp_dir()
        } else {
            std::path::PathBuf::from(parent)
        };
        for _ in 0..16 {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos());
            let name = format!(
                "dicom-native-store-{}-{}-{nanos}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            );
            let path = parent.join(name);
            match std::fs::create_dir(&path) {
                Ok(()) => return Some(SpoolDir(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return None,
            }
        }
        None
    }
}
impl Drop for SpoolDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn same_descriptor(a: &StoreDescriptor, b: &StoreDescriptor) -> bool {
    a.sop_class_uid == b.sop_class_uid
        && a.sop_instance_uid == b.sop_instance_uid
        && a.transfer_syntax_uid == b.transfer_syntax_uid
        && a.size == b.size
        && a.pixel_data_header == b.pixel_data_header
        && a.writable_transfer_syntax_uids == b.writable_transfer_syntax_uids
}
fn native_logical_read_limit(limits: &TranscodeLimits, compressed_bytes: i64) -> i64 {
    if compressed_bytes <= 0 {
        return 1;
    }
    if limits.max_expansion_ratio <= limits.max_input_bytes / compressed_bytes {
        return limits
            .max_input_bytes
            .min(compressed_bytes * limits.max_expansion_ratio);
    }
    limits.max_input_bytes
}
struct Snapshot {
    data: Vec<u8>,
    offset: usize,
    digest: [u8; 32],
}
impl NativeSource {
    fn check_source_access(&self, ctx: &Ctx) -> Result<()> {
        let Some(check) = &self.opts.before_source_read else {
            return Ok(());
        };
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        let result = check(ctx);
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        result.map_err(|_| Error::sentinel(Kind::StoreAccessDenied))
    }
    fn inspect_inner(&self, ctx: &Ctx) -> Result<StoreDescriptor> {
        if let Some(e) = context_err(ctx) {
            return Err(e);
        }
        let mut state = lock(&self.state);
        if let Some(descriptor) = &state.descriptor {
            return Ok(descriptor.clone());
        }
        if self.path.is_none() {
            self.check_source_access(ctx)?;
        }
        let input = self
            .base
            .inspect(ctx)
            .map_err(|e| Failure::of(&e).into_error())?;
        let original = registered(&input.transfer_syntax_uid);
        let Some(original) = original.filter(|_| {
            is_valid_uid(input.sop_class_uid.as_bytes())
                && is_valid_uid(input.sop_instance_uid.as_bytes())
                && input.size >= 0
                && input.size <= self.limits.max_input_bytes
        }) else {
            return Err(Failure::kind(Kind::StoreInvalidSource).into_error());
        };
        let uid = original.uid.to_string();
        let mut output = input.clone();
        output.writable_transfer_syntax_uids = vec![uid.clone()];
        if output.size == 0 {
            output.size = self.limits.max_input_bytes;
        }
        let mut sizes = BTreeMap::new();
        sizes.insert(uid.clone(), output.size);
        let mut producible = [
            IMPLICIT_LE,
            EXPLICIT_LE,
            EXPLICIT_BE,
            DEFLATED,
            ENCAPSULATED_UNCOMPRESSED,
        ]
        .contains(&uid.as_str());
        if original.encapsulated
            && !media_payload(&uid)
            && !producible
            && let Some(registry) = self.opts.transcode.decoder
        {
            producible = registry.decodes(&uid);
        }
        if producible {
            for syntax in [EXPLICIT_LE, IMPLICIT_LE] {
                if syntax != uid {
                    output
                        .writable_transfer_syntax_uids
                        .push(syntax.to_string());
                    sizes.insert(syntax.to_string(), self.limits.max_output_bytes);
                }
            }
        }
        output.transfer_syntax_sizes = Some(sizes);
        state.input = Some(input);
        state.descriptor = Some(output.clone());
        Ok(output)
    }
    fn snapshot(
        &self,
        ctx: &Ctx,
        input: &StoreDescriptor,
    ) -> std::result::Result<Snapshot, Failure> {
        if let Some(path) = &self.path {
            return self.snapshot_path(ctx, path, input);
        }
        let mut opened = match self.base.open(ctx) {
            Ok(opened) => opened,
            Err(e) => return Err(Failure::of(&e)),
        };
        let close = opened.close.take();
        let result = (|| {
            let (Some(_), Some(write)) = (&close, &opened.write_data_set) else {
                return Err(Failure::kind(Kind::StoreSourceChanged));
            };
            if !same_descriptor(input, &opened.descriptor) {
                return Err(Failure::kind(Kind::StoreSourceChanged));
            }
            let original = registered(&input.transfer_syntax_uid)
                .ok_or_else(|| Failure::kind(Kind::StoreInvalidSource))?;
            let limit = self.limits.max_input_bytes;
            let mut data: Vec<u8> = Vec::new();
            let mut over = false;
            let written = {
                let mut sink = |chunk: &[u8]| -> Result<()> {
                    if let Some(e) = context_err(ctx) {
                        return Err(e);
                    }
                    if chunk.len() as i64 > limit - data.len() as i64 {
                        over = true;
                        return Err(Error::new(ERR_RESOURCE_LIMIT));
                    }
                    data.extend_from_slice(chunk);
                    Ok(())
                };
                write(ctx, &mut sink, &original)
            };
            if over {
                return Err(Failure::limit());
            }
            if let Err(e) = written {
                return Err(match context_err(ctx) {
                    Some(c) => Failure::context(c),
                    None => Failure::of(&e),
                });
            }
            let digest: [u8; 32] = Sha256::digest(&data).into();
            Ok(Snapshot {
                data,
                offset: 0,
                digest,
            })
        })();
        match (result, close.map(|close| close())) {
            (Ok(_), Some(Err(e))) => Err(Failure::of(&e)),
            (result, _) => result,
        }
    }
    fn snapshot_path(
        &self,
        ctx: &Ctx,
        path: &NativePath,
        input: &StoreDescriptor,
    ) -> std::result::Result<Snapshot, Failure> {
        let changed = || Failure::kind(Kind::StoreSourceChanged);
        let info = std::fs::symlink_metadata(&path.path).map_err(|_| changed())?;
        if !info.file_type().is_file() {
            return Err(changed());
        }
        if let Some(e) = context_err(ctx) {
            return Err(Failure::context(e));
        }
        let data = std::fs::read(&path.path).map_err(|_| changed())?;
        let size = data.len() as i64;
        if info.len() as i64 != path.expected_size {
            return Err(changed());
        }
        let limit = self.limits.max_input_bytes;
        if size > limit {
            return Err(Failure::limit());
        }
        let digest: [u8; 32] = Sha256::digest(&data).into();
        if digest != path.expected_sha256 || size != path.expected_size {
            return Err(changed());
        }
        let deflated = registered(&input.transfer_syntax_uid).is_some_and(|s| s.deflated);
        let mut index_options = index::default_options();
        index_options.limits.max_total_bytes = if deflated {
            native_logical_read_limit(&self.limits, size)
        } else {
            limit
        };
        index_options.limits.max_elements = self.limits.max_elements;
        index_options.limits.max_fragments = self.limits.max_fragments;
        index_options.limits.max_sequence_depth = self.limits.max_depth;
        let (indexed, err) = index::read_bytes(&path.path, &data, index_options);
        if let Some(e) = err {
            return Err(Failure::index(&e));
        }
        let record = &indexed.record;
        let meta = &record.file_meta;
        if trim_text(&record.instance.sop_class_uid) != input.sop_class_uid
            || trim_text(&record.instance.sop_instance_uid) != input.sop_instance_uid
            || normalize_uid(&meta.transfer_syntax_uid) != input.transfer_syntax_uid
            || (!meta.media_storage_sop_class_uid.is_empty()
                && trim_text(&meta.media_storage_sop_class_uid) != input.sop_class_uid)
            || (!meta.media_storage_sop_instance_uid.is_empty()
                && trim_text(&meta.media_storage_sop_instance_uid) != input.sop_instance_uid)
        {
            return Err(changed());
        }
        let mut checked = input.clone();
        checked.pixel_data_header = indexed.stopped_header;
        normalize_store_descriptor(checked).map_err(|e| Failure::of(&e))?;
        let mut read_options = self.opts.dictionary_options;
        read_options.max_total_bytes = limit;
        read_options.max_element_bytes = limit;
        read_options.max_elements = self.limits.max_elements;
        read_options.max_sequence_depth = self.limits.max_depth;
        let shared: Rc<[u8]> = Rc::from(data.as_slice());
        let header = object::read_part10_header_mem(&MemFile::new(shared), &read_options)
            .map_err(Failure::object)?;
        if header.data_set_offset < 0
            || header.data_set_offset >= size
            || header.transfer_syntax.uid != input.transfer_syntax_uid
        {
            return Err(changed());
        }
        Ok(Snapshot {
            data,
            offset: header.data_set_offset as usize,
            digest,
        })
    }
    fn native(
        &self,
        ctx: &Ctx,
        snapshot: &Snapshot,
        descriptor: &StoreDescriptor,
        original: &Syntax,
        target: &Syntax,
    ) -> std::result::Result<(Vec<u8>, usize), Failure> {
        let limits = &self.limits;
        let raw = &snapshot.data[snapshot.offset..];
        let mut read_options = self.opts.dictionary_options;
        read_options.max_total_bytes = if original.deflated {
            native_logical_read_limit(limits, raw.len() as i64)
        } else {
            limits.max_input_bytes
        };
        read_options.max_element_bytes = limits.max_input_bytes;
        read_options.max_pixel_data_bytes = limits.max_input_bytes;
        read_options.max_elements = limits.max_elements;
        read_options.max_fragments = limits.max_fragments;
        read_options.max_sequence_depth = limits.max_depth;
        let dataset = object::read_data_set_with_options(
            Box::new(MemFile::new(Rc::from(raw))),
            *original,
            read_options,
        )
        .map_err(Failure::object)?;
        let file = Arc::new(File {
            preamble: Vec::new(),
            meta: object::Object::default(),
            dataset,
            transfer_syntax: *original,
            partial_read: None,
            incomplete_pixel_data: None,
        });
        let actual = new_file_store_source(Some(file.clone()), 0).inspect(ctx);
        if !actual.is_ok_and(|actual| {
            actual.sop_class_uid == descriptor.sop_class_uid
                && actual.sop_instance_uid == descriptor.sop_instance_uid
        }) {
            return Err(Failure::kind(Kind::StoreSourceChanged));
        }
        if let Some(e) = context_err(ctx) {
            return Err(Failure::context(e));
        }
        if incremental_profile(original, target, &self.opts.transcode)
            && let Some(prepared) = self.incremental(ctx, snapshot, descriptor, original, target)?
        {
            return Ok(prepared);
        }
        let (converted, _) = transcode::transcode_data_set(
            &file.dataset,
            original.uid,
            target.uid,
            &self.opts.transcode,
        )
        .map_err(|e| Failure::transcode(&e))?;
        let bytes =
            object::writer::write_data_set(&converted, target.uid).map_err(Failure::object)?;
        if bytes.len() as i64 > limits.max_output_bytes {
            return Err(Failure::limit());
        }
        Ok((bytes, 0))
    }
    fn incremental(
        &self,
        ctx: &Ctx,
        snapshot: &Snapshot,
        descriptor: &StoreDescriptor,
        original: &Syntax,
        target: &Syntax,
    ) -> std::result::Result<Option<(Vec<u8>, usize)>, Failure> {
        let limits = &self.limits;
        if limits.max_output_bytes <= 0 {
            return Err(Failure::limit());
        }
        let dir = SpoolDir::new(&self.opts.spool_directory).ok_or_else(Failure::plain)?;
        let input_path = dir.0.join("original");
        let output_path = dir.0.join("native");
        std::fs::write(&input_path, &snapshot.data).map_err(|_| Failure::plain())?;
        let mut options = self.opts.transcode.clone();
        options.write_options = Default::default();
        let measured = transcode::transcode_data_set_path(
            &input_path,
            &output_path,
            original.uid,
            target.uid,
            &options,
            transcode::DataSetPathOptions {
                source_offset: snapshot.offset as i64,
                max_temporary_bytes: limits.max_output_bytes,
            },
        );
        if let Err(e) = measured {
            if e.is(transcode::ErrorKind::StreamingIneligible) {
                return Ok(None);
            }
            return Err(Failure::transcode(&e));
        }
        let data = std::fs::read(&output_path).map_err(|_| Failure::plain())?;
        drop(dir);
        let size = data.len() as i64;
        if size > limits.max_output_bytes {
            return Err(Failure::limit());
        }
        let read_options = ReadFileOptions {
            max_total_bytes: limits.max_output_bytes,
            max_element_bytes: limits.max_output_bytes,
            max_pixel_data_bytes: limits.max_output_bytes,
            max_elements: limits.max_elements,
            max_fragments: limits.max_fragments,
            max_sequence_depth: limits.max_depth,
            ..ReadFileOptions::default()
        };
        let shared: Rc<[u8]> = Rc::from(data.as_slice());
        let header = object::read_part10_header_mem(&MemFile::new(shared), &read_options)
            .map_err(Failure::object)?;
        if header.data_set_offset < 0
            || header.data_set_offset >= size
            || header.transfer_syntax.uid != target.uid
        {
            return Err(Failure::kind(Kind::StoreInvalidSource));
        }
        let output = object::read_file_with_options(&data, read_options)
            .map_err(|(_, e)| Failure::object(e))?;
        if let Some(e) = context_err(ctx) {
            return Err(Failure::context(e));
        }
        let actual = new_file_store_source(Some(Arc::new(output)), 0).inspect(ctx);
        if !actual.is_ok_and(|actual| {
            actual.sop_class_uid == descriptor.sop_class_uid
                && actual.sop_instance_uid == descriptor.sop_instance_uid
        }) {
            return Err(Failure::kind(Kind::StoreSourceChanged));
        }
        Ok(Some((data, header.data_set_offset as usize)))
    }
    fn open_target(
        &self,
        parent: &Ctx,
        target: &Syntax,
    ) -> std::result::Result<OpenedStoreSource, Failure> {
        let descriptor = self.inspect_inner(parent).map_err(Failure::context)?;
        let Some(target) = registered(target.uid).filter(|t| {
            descriptor
                .writable_transfer_syntax_uids
                .iter()
                .any(|u| *u == t.uid)
        }) else {
            return Err(Failure::kind(Kind::StoreTransferSyntax));
        };
        let limits = self.limits;
        let ctx =
            parent.with_timeout(Duration::from_nanos(limits.max_duration_nanos.max(0) as u64));
        let converts = target.uid != descriptor.transfer_syntax_uid;
        let mut reservation = limits.max_input_bytes;
        if converts {
            reservation += limits.max_output_bytes;
        }
        let resources = self.resources.clone();
        if !resources.reserve(reservation) {
            return Err(Failure::limit());
        }
        let prepared = (|| {
            resources.enter(&ctx).map_err(Failure::context)?;
            let result = (|| {
                self.check_source_access(&ctx).map_err(Failure::context)?;
                if !self.opts.spool_directory.is_empty()
                    && !std::path::Path::new(&self.opts.spool_directory).is_dir()
                {
                    return Err(Failure::plain());
                }
                let input = lock(&self.state)
                    .input
                    .clone()
                    .ok_or_else(|| Failure::kind(Kind::StoreInvalidSource))?;
                let snapshot = self.snapshot(&ctx, &input)?;
                {
                    let mut state = lock(&self.state);
                    match state.digest {
                        Some(known) if known != snapshot.digest => {
                            return Err(Failure::kind(Kind::StoreSourceChanged));
                        }
                        Some(_) => {}
                        None => state.digest = Some(snapshot.digest),
                    }
                }
                let payload = if converts {
                    let original = registered(&descriptor.transfer_syntax_uid)
                        .ok_or_else(|| Failure::kind(Kind::StoreInvalidSource))?;
                    self.native(&ctx, &snapshot, &descriptor, &original, &target)?
                } else {
                    let offset = snapshot.offset;
                    (snapshot.data, offset)
                };
                if let Some(e) = context_err(&ctx) {
                    return Err(Failure::context(e));
                }
                Ok(payload)
            })();
            resources.leave();
            result
        })();
        let (data, offset) = match prepared {
            Ok(payload) => payload,
            Err(failure) => {
                resources.release(reservation);
                return Err(failure);
            }
        };
        let handle: Arc<Mutex<Option<Arc<[u8]>>>> = Arc::new(Mutex::new(Some(Arc::from(data))));
        let target_uid = target.uid.to_string();
        let part10 = !converts && offset > 0;
        let write_handle = handle.clone();
        let write_uid = target_uid.clone();
        let write: StoreDataSetWriter = Arc::new(move |write_ctx, sink, syntax| {
            let Some(data) = lock(&write_handle).clone() else {
                return Err(Error::sentinel(Kind::StoreInvalidSource));
            };
            if normalize_uid(syntax.uid) != write_uid {
                return Err(Error::sentinel(Kind::StoreTransferSyntax));
            }
            let mut rest = &data[offset..];
            while !rest.is_empty() {
                if let Some(e) = context_err(write_ctx) {
                    return Err(Failure::of(&e).into_error());
                }
                let n = rest.len().min(64 << 10);
                sink(&rest[..n]).map_err(|e| Failure::of(&e).into_error())?;
                rest = &rest[n..];
            }
            Ok(())
        });
        let meta_handle = handle.clone();
        let read_metadata: StoreMetadataReader = Arc::new(move |read_ctx, mut options| {
            let Some(data) = lock(&meta_handle).clone() else {
                return Err(Error::sentinel(Kind::StoreInvalidSource));
            };
            if let Some(e) = context_err(read_ctx) {
                return Err(Error::bare_with(
                    "dicom index source failed",
                    e.causes().to_vec(),
                ));
            }
            options.raw_data_set = (!part10).then(|| target_uid.clone());
            let input = if part10 { &data[..] } else { &data[offset..] };
            match index::read_bytes("", input, options) {
                (result, None) => Ok(result),
                (_, Some(e)) => Err(Error::new(e.to_string())),
            }
        });
        let close: StoreSourceCloser = Box::new(move || {
            if lock(&handle).take().is_some() {
                resources.release(reservation);
            }
            Ok(())
        });
        Ok(OpenedStoreSource {
            descriptor,
            write_data_set: Some(write),
            close: Some(close),
            before_store: self.opts.before_store.clone(),
            read_metadata: Some(read_metadata),
        })
    }
}
impl StoreSource for NativeSource {
    fn inspect(&self, ctx: &Ctx) -> Result<StoreDescriptor> {
        self.inspect_inner(ctx)
    }
    fn open(&self, ctx: &Ctx) -> Result<OpenedStoreSource> {
        let descriptor = self.inspect_inner(ctx)?;
        let syntax = registered(&descriptor.transfer_syntax_uid)
            .ok_or_else(|| Failure::kind(Kind::StoreInvalidSource).into_error())?;
        self.open_target(ctx, &syntax).map_err(Failure::into_error)
    }
    fn open_for_transfer_syntax(
        &self,
        ctx: &Ctx,
        syntax: &Syntax,
    ) -> Option<Result<OpenedStoreSource>> {
        Some(self.open_target(ctx, syntax).map_err(Failure::into_error))
    }
}
