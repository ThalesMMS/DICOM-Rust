use super::admission::source_frame_reader_options;
use super::decompress::{check_admission, codec_registry_nil, transcode_decompress_limits};
use super::encode::{
    EncodeState, apply_lossy_metadata, encode_frame, encoded_metadata_elements, resolve,
};
use super::output::{
    apply_identity, explicit_encapsulation, jpeg2000_part2, output_planar, put_planar,
    reorder_frame, validate_output_options, validate_output_policy,
};
use super::verify::{
    add_lossy, resolve_verification_options, validate_readback, validate_target_verification_codec,
    verify_transcoded_frame,
};
use super::{
    ERR_DESTINATION_UNSAFE, ERR_RESOURCE_LIMIT, ERR_SOURCE_CHANGED, Error, ErrorKind, Mode,
    PlanarOutput, TAG_DOUBLE_FLOAT_PIXEL_DATA, TAG_FLOAT_PIXEL_DATA, TAG_PHOTOMETRIC,
    TAG_PIXEL_DATA, TAG_PLANAR_CONFIGURATION, TAG_SAMPLES_PER_PIXEL, TranscodeLimits,
    TranscodeOptions, TranscodeReport, UID_ENCAPSULATED_UNCOMPRESSED, Verification, build_plan,
    canonical_target, clone_detached, file_source_syntax, has_any_pixel_data,
    has_nested_pixel_data, object_from, output_clone_limits, output_file, remove_elements,
    resolve_limits, transcode_file,
};
use crate::object::frame_writer::{self, FrameSource, FrameWriteOptions};
use crate::object::writer::{self, group_length};
use crate::object::{Element, File, Object, ReadFileOptions, Tag, Value};
use crate::pixel::Metadata;
use crate::pixel::frame_reader::{FrameReader, FrameReaderOptions};
use crate::pixel::pixeldata::{PixelErrorKind, extract_metadata, frame_size, total_size};
use crate::transfer::Syntax;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const DEFAULT_IN_FLIGHT_BYTES: i64 = 256 << 20;
pub mod durability {
    pub const DURABLE: u8 = 0;
    pub const ATOMIC: u8 = 1;
    pub fn as_str(value: u8) -> &'static str {
        match value {
            DURABLE => "durable",
            ATOMIC => "atomic",
            _ => "invalid",
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineOptions {
    pub workers: i64,
    pub max_in_flight_frames: i64,
    pub max_in_flight_bytes: i64,
    pub max_encoded_frame_bytes: i64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DataSetPathOptions {
    pub source_offset: i64,
    pub max_temporary_bytes: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DataSetPathReport {
    pub report: TranscodeReport,
    pub peak_temporary_bytes: i64,
}
fn transaction(stage: &str) -> Error {
    Error::stage(
        stage,
        Error::new(ErrorKind::Transaction, super::ERR_TRANSACTION),
    )
}
fn stage_limit(stage: &str) -> Error {
    Error::stage(
        stage,
        Error::new(ErrorKind::ResourceLimit, ERR_RESOURCE_LIMIT),
    )
}
fn unsafe_destination() -> Error {
    Error::new(ErrorKind::DestinationUnsafe, ERR_DESTINATION_UNSAFE)
}
fn bare_limit() -> Error {
    Error::new(ErrorKind::ResourceLimit, ERR_RESOURCE_LIMIT)
}
fn other(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Other, e.to_string())
}
fn validate_policy(o: &TranscodeOptions) -> Result<(), Error> {
    validate_output_policy(o)?;
    let p = &o.pipeline;
    if p.workers < 0
        || p.max_in_flight_frames < 0
        || p.max_in_flight_bytes < 0
        || p.max_encoded_frame_bytes < 0
    {
        return Err(Error::new(
            ErrorKind::ResourceLimit,
            format!("{ERR_RESOURCE_LIMIT}: invalid pipeline limits"),
        ));
    }
    if o.durability > durability::ATOMIC {
        return Err(Error::unsupported("invalid durability policy"));
    }
    Ok(())
}
fn snapshot_write_options(o: &TranscodeOptions, target: &str) -> Result<(), Error> {
    let target = canonical_target(target)?;
    writer::validate_serialization_options(
        &target.uid,
        o.write_options.deflate_level,
        &o.write_options.padding,
    )
    .map_err(other)
}
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Snapshot {
    path: PathBuf,
    pub(super) len: u64,
    modified: Option<std::time::SystemTime>,
}
fn snapshot(path: &Path) -> std::io::Result<Snapshot> {
    let meta = fs::metadata(path)?;
    Ok(Snapshot {
        path: fs::canonicalize(path)?,
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}
#[derive(Default)]
struct TemporaryBudget {
    maximum: i64,
    used: i64,
    peak: i64,
}
impl TemporaryBudget {
    fn reserve(&mut self, n: i64) -> Result<(), Error> {
        if n < 0 || n > self.maximum.saturating_sub(self.used) {
            return Err(bare_limit());
        }
        self.used = self.used.saturating_add(n);
        self.peak = self.peak.max(self.used);
        Ok(())
    }
}
pub(super) struct Txn {
    source_path: PathBuf,
    pub(super) source: Snapshot,
    pub(super) source_bytes: Vec<u8>,
    pub(super) parent: PathBuf,
    pub(super) destination: PathBuf,
    pub(super) previous: Option<Snapshot>,
    durability: u8,
    file_synced: bool,
    directory_synced: bool,
    raw: Option<Syntax>,
    temporary: Option<TemporaryBudget>,
}
fn split_parent(path: &Path) -> (PathBuf, std::ffi::OsString) {
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    (parent, name)
}
fn begin(source_path: &str, destination_path: &str) -> Result<Txn, Error> {
    begin_bounded(source_path, destination_path, None)
}
pub(super) fn begin_bounded(
    source_path: &str,
    destination_path: &str,
    max_read: Option<u64>,
) -> Result<Txn, Error> {
    let source_fail = || transaction("source");
    let source_abs = std::path::absolute(source_path).map_err(|_| source_fail())?;
    let (source_dir, source_name) = split_parent(&source_abs);
    let source_dir = fs::canonicalize(&source_dir).map_err(|_| source_fail())?;
    let source_abs = source_dir.join(source_name);
    let destination_abs =
        std::path::absolute(destination_path).map_err(|_| unsafe_destination())?;
    let (parent, destination_name) = split_parent(&destination_abs);
    let parent = fs::canonicalize(&parent).map_err(|_| unsafe_destination())?;
    let destination = parent.join(&destination_name);
    if source_abs == destination || destination_name.is_empty() {
        return Err(unsafe_destination());
    }
    let link = fs::symlink_metadata(&source_abs).map_err(|_| source_fail())?;
    if !link.is_file() {
        return Err(source_fail());
    }
    let source = snapshot(&source_abs).map_err(|_| source_fail())?;
    let source_bytes = if max_read.is_some_and(|max| source.len > max) {
        Vec::new()
    } else {
        fs::read(&source_abs).map_err(|_| source_fail())?
    };
    if !fs::metadata(&parent).is_ok_and(|m| m.is_dir()) {
        return Err(unsafe_destination());
    }
    let previous = match fs::symlink_metadata(&destination) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(unsafe_destination()),
        Ok(meta) => {
            if !meta.is_file() {
                return Err(unsafe_destination());
            }
            let snap = snapshot(&destination).map_err(|_| unsafe_destination())?;
            if snap.path == source.path {
                return Err(unsafe_destination());
            }
            Some(snap)
        }
    };
    Ok(Txn {
        source_path: source_abs,
        source,
        source_bytes,
        parent,
        destination,
        previous,
        durability: durability::DURABLE,
        file_synced: false,
        directory_synced: false,
        raw: None,
        temporary: None,
    })
}
pub(super) struct Staged {
    pub(super) path: PathBuf,
    pub(super) bytes: Vec<u8>,
    pub(super) published: bool,
}
impl Drop for Staged {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn temp_name() -> String {
    format!(".dicom-transcode-{}", uuid::Uuid::new_v4().simple())
}
impl Txn {
    pub(super) fn stage(
        &mut self,
        bytes: Result<Vec<u8>, Error>,
        limits: &TranscodeLimits,
    ) -> Result<Staged, Error> {
        let path = self.parent.join(temp_name());
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| transaction("temporary"))?;
        let mut staged = Staged {
            path,
            bytes: Vec::new(),
            published: false,
        };
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) if e.is(ErrorKind::ResourceLimit) => return Err(stage_limit("write")),
            Err(_) => return Err(transaction("write")),
        };
        if bytes.len() as i64 > limits.max_output_bytes {
            return Err(stage_limit("write"));
        }
        if let Some(budget) = &mut self.temporary
            && budget.reserve(bytes.len() as i64).is_err()
        {
            return Err(stage_limit("write"));
        }
        file.write_all(&bytes).map_err(|_| transaction("write"))?;
        if self.durability == durability::DURABLE {
            file.sync_all().map_err(|_| transaction("write"))?;
            self.file_synced = true;
        }
        staged.bytes = bytes;
        Ok(staged)
    }
    pub(super) fn precommit(&self, staged: &Staged) -> Result<(), Error> {
        match snapshot(&self.source_path) {
            Ok(s) if s == self.source => {}
            _ => return Err(Error::new(ErrorKind::SourceChanged, ERR_SOURCE_CHANGED)),
        }
        if !fs::metadata(&self.parent).is_ok_and(|m| m.is_dir()) {
            return Err(unsafe_destination());
        }
        let current = match fs::symlink_metadata(&self.destination) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(unsafe_destination()),
            Ok(_) => snapshot(&self.destination).ok(),
        };
        if current != self.previous {
            return Err(unsafe_destination());
        }
        match fs::metadata(&staged.path) {
            Ok(m) if m.len() == staged.bytes.len() as u64 => Ok(()),
            _ => Err(transaction("temporary")),
        }
    }
    fn publish(&mut self, staged: &mut Staged) -> Result<(), Error> {
        fs::rename(&staged.path, &self.destination).map_err(|_| transaction("publish"))?;
        staged.published = true;
        if self.durability == durability::DURABLE {
            sync_directory(&self.parent).map_err(|_| transaction("durability"))?;
            self.directory_synced = cfg!(unix);
        }
        Ok(())
    }
    fn apply_report(&self, report: &mut TranscodeReport) {
        report.durability = self.durability;
        report.file_synced = self.file_synced;
        report.directory_synced = self.directory_synced;
    }
}
#[cfg(unix)]
pub(super) fn sync_directory(parent: &Path) -> std::io::Result<()> {
    fs::File::open(parent)?.sync_all()
}
#[cfg(not(unix))]
pub(super) fn sync_directory(_parent: &Path) -> std::io::Result<()> {
    Ok(())
}
fn output_read_options(limits: &TranscodeLimits) -> ReadFileOptions {
    ReadFileOptions {
        max_element_bytes: limits.max_output_bytes,
        max_total_bytes: limits.max_output_bytes,
        max_sequence_depth: limits.max_depth,
        max_elements: limits.max_elements,
        max_fragments: limits.max_fragments,
        ..ReadFileOptions::default()
    }
}
fn validate_staged(staged: &Staged, limits: &TranscodeLimits) -> Result<(), Error> {
    let fail = || transaction("readback");
    let readback =
        crate::object::read_file_with_options(&staged.bytes, output_read_options(limits))
            .map_err(|_| fail())?;
    let syntax = file_source_syntax(&readback).map_err(|_| fail())?;
    if has_any_pixel_data(&readback.dataset) {
        validate_readback(&readback.dataset, &syntax, limits).map_err(|_| fail())?;
    } else if syntax.encapsulated {
        return Err(fail());
    }
    Ok(())
}
fn validate_staged_stream(
    staged: &Staged,
    limits: &TranscodeLimits,
    syntax: &Syntax,
    expected_items: i64,
) -> Result<(), Error> {
    let fail = || transaction("readback");
    let mut options = output_read_options(limits);
    options.max_fragments = i64::MAX;
    let readback =
        crate::object::read_file_with_options(&staged.bytes, options).map_err(|_| fail())?;
    match file_source_syntax(&readback) {
        Ok(s) if s.uid == syntax.uid => {}
        _ => return Err(fail()),
    }
    let dataset = &readback.dataset;
    let Some(pixel) = dataset.get(TAG_PIXEL_DATA) else {
        return Err(fail());
    };
    if dataset.has(TAG_FLOAT_PIXEL_DATA) || dataset.has(TAG_DOUBLE_FLOAT_PIXEL_DATA) {
        return Err(fail());
    }
    let items = match &pixel.value {
        Value::Fragments { fragments, .. } => Some((fragments.len() as i64).saturating_add(1)),
        _ => None,
    };
    if items.is_some() != syntax.encapsulated || extract_metadata(dataset).is_err() {
        return Err(fail());
    }
    if let Some(items) = items
        && (items != expected_items || items.saturating_sub(1) > limits.max_fragments)
    {
        return Err(fail());
    }
    Ok(())
}
pub fn transcode_path(
    source_path: impl AsRef<std::path::Path>,
    destination_path: impl AsRef<std::path::Path>,
    target: &str,
    options: &TranscodeOptions,
) -> Result<TranscodeReport, Error> {
    let source_path = source_path.as_ref().to_string_lossy();
    let source_path: &str = &source_path;
    let destination_path = destination_path.as_ref().to_string_lossy();
    let destination_path: &str = &destination_path;
    snapshot_write_options(options, target)?;
    let options = resolve_verification_options(options.clone())?;
    validate_policy(&options)?;
    let limits = resolve_limits(options.limits)?;
    let mut txn = begin(source_path, destination_path)?;
    txn.durability = options.durability;
    if let Some(result) = stream(&mut txn, target, &options, &limits) {
        return result;
    }
    let read_options = ReadFileOptions {
        max_element_bytes: limits.max_input_bytes,
        max_total_bytes: limits.max_input_bytes,
        max_sequence_depth: limits.max_depth,
        max_elements: limits.max_elements,
        max_fragments: limits.max_fragments,
        ..ReadFileOptions::default()
    };
    let file = crate::object::read_file_with_options(&txn.source_bytes, read_options)
        .map_err(|_| transaction("read"))?;
    let source_syntax = file_source_syntax(&file)?;
    let target_syntax = canonical_target(target)?;
    validate_target_verification_codec(&source_syntax, &target_syntax, &options)?;
    let (output, mut report) = transcode_file(&file, target, &options)?;
    if !report.pixel_data_preserved && options.skip_verification {
        report.verification = Verification::Skipped;
    } else if !report.lossy && !report.pixel_data_preserved {
        super::output::verify_lossless(
            &file.dataset,
            &source_syntax,
            &output.dataset,
            &target_syntax,
            options.decoder,
            &limits,
        )?;
        report.verification = Verification::PixelExact;
    } else if report.lossy && !report.pixel_data_preserved && report.lossy_verification.is_none() {
        let mut output_limits = limits;
        output_limits.max_input_bytes = limits.max_output_bytes;
        super::output::canonical_native_bytes(
            &output.dataset,
            &target_syntax,
            options.decoder,
            &output_limits,
        )?;
        report.verification = Verification::Decode;
    }
    let bytes = writer::write_file_with_options(&output, &options.write_options).map_err(other);
    let mut staged = txn.stage(bytes, &limits)?;
    validate_staged(&staged, &limits)?;
    txn.precommit(&staged)?;
    txn.publish(&mut staged)?;
    txn.apply_report(&mut report);
    Ok(report)
}
#[doc(hidden)]
pub const ERR_STREAMING_INELIGIBLE: &str = "pixeldata: raw dataset needs resident transcode route";
pub fn transcode_data_set_path(
    source_path: impl AsRef<std::path::Path>,
    destination_path: impl AsRef<std::path::Path>,
    source: &str,
    target: &str,
    options: &TranscodeOptions,
    data_set_options: DataSetPathOptions,
) -> Result<DataSetPathReport, Error> {
    let source_path = source_path.as_ref().to_string_lossy();
    let source_path: &str = &source_path;
    let destination_path = destination_path.as_ref().to_string_lossy();
    let destination_path: &str = &destination_path;
    let ineligible = || Error::new(ErrorKind::StreamingIneligible, ERR_STREAMING_INELIGIBLE);
    if data_set_options.source_offset < 0 || data_set_options.max_temporary_bytes < 0 {
        return Err(bare_limit());
    }
    if options.allow_lossy
        || options.secondary_capture
        || options.uid_policy != super::UidPolicy::Default
        || !options.uid_root.is_empty()
    {
        return Err(ineligible());
    }
    let Some(source) =
        crate::transfer::default_registry().get(crate::transfer::normalize_uid(source))
    else {
        return Err(Error::new(ErrorKind::Unsupported, super::ERR_UNSUPPORTED));
    };
    let target_syntax = canonical_target(target)?;
    if target_syntax.encapsulated
        && target_syntax.uid != UID_ENCAPSULATED_UNCOMPRESSED
        && let Some(registry) = &options.encoders
        && registry
            .capabilities_of(&target_syntax.uid)
            .is_some_and(|c| !c.lossless)
    {
        return Err(ineligible());
    }
    if source.big_endian
        || source.deflated
        || target_syntax.big_endian
        || target_syntax.deflated
        || options.rewrite.is_some()
        || options.compress_icons
    {
        return Err(ineligible());
    }
    snapshot_write_options(options, target)?;
    let options = resolve_verification_options(options.clone())?;
    validate_policy(&options)?;
    let limits = resolve_limits(options.limits)?;
    let mut txn = begin(source_path, destination_path)?;
    let size = txn.source_bytes.len() as i64;
    if data_set_options.source_offset >= size
        || size.saturating_sub(data_set_options.source_offset) > limits.max_input_bytes
    {
        return Err(bare_limit());
    }
    txn.source_bytes = txn
        .source_bytes
        .get(data_set_options.source_offset as usize..)
        .unwrap_or_default()
        .to_vec();
    let maximum = if data_set_options.max_temporary_bytes == 0 {
        2 << 30
    } else {
        data_set_options.max_temporary_bytes
    };
    txn.raw = Some(source);
    txn.temporary = Some(TemporaryBudget {
        maximum,
        ..TemporaryBudget::default()
    });
    txn.durability = options.durability;
    let result = stream(&mut txn, target, &options, &limits);
    let peak = txn.temporary.as_ref().map_or(0, |t| t.peak);
    match result {
        None => Err(ineligible()),
        Some(Ok(report)) => Ok(DataSetPathReport {
            report,
            peak_temporary_bytes: peak,
        }),
        Some(Err(e)) => Err(e),
    }
}
fn read_raw(txn: &Txn, syntax: &Syntax, limits: &TranscodeLimits) -> Result<File, Error> {
    let options = ReadFileOptions {
        max_total_bytes: limits.max_input_bytes,
        max_element_bytes: limits.max_input_bytes,
        max_pixel_data_bytes: limits.max_input_bytes,
        max_sequence_depth: limits.max_depth,
        max_elements: limits.max_elements,
        max_fragments: limits.max_fragments,
        ..ReadFileOptions::default()
    };
    let object_syntax = crate::object::transfer::lookup(&syntax.uid)
        .ok_or_else(|| Error::new(ErrorKind::Unsupported, super::ERR_UNSUPPORTED))?;
    let reader = Box::new(crate::object::parser::source::MemFile::new(
        std::rc::Rc::from(txn.source_bytes.as_slice()),
    ));
    let dataset =
        crate::object::read_data_set_with_options(reader, object_syntax, options).map_err(other)?;
    Ok(File {
        preamble: Vec::new(),
        meta: Object::new(),
        dataset,
        transfer_syntax: object_syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    })
}
fn may_have_embedded_overlays(dataset: &Object) -> bool {
    (0x6000u16..=0x601e)
        .step_by(2)
        .any(|g| dataset.has(Tag::new(g, 0x0010)) && !dataset.has(Tag::new(g, 0x3000)))
}
fn output_frame_reader_options(
    o: &TranscodeOptions,
    limits: &TranscodeLimits,
) -> FrameReaderOptions {
    FrameReaderOptions {
        decoder: o.decoder,
        max_frames: limits.max_frames,
        max_fragments: limits.max_fragments,
        limits: super::DecompressLimits {
            max_pixels: limits.max_pixels as i64,
            max_input_bytes: limits.max_output_bytes,
            max_native_bytes: limits.max_output_bytes,
            max_expansion_ratio: limits.max_expansion_ratio,
            ..super::DecompressLimits::default()
        },
    }
}
struct Pipeline {
    options: PipelineOptions,
    workers: i64,
    encoded_cap: i64,
    native_bytes: i64,
    encode: bool,
    verify: bool,
    planar_copy: bool,
}
type Produced = (
    super::encode::Encoded,
    Option<super::LossyVerificationReport>,
);
impl Pipeline {
    fn new(
        o: &TranscodeOptions,
        count: i64,
        native_bytes: i64,
        max_output: i64,
        encode: bool,
    ) -> Result<Pipeline, Error> {
        validate_policy(o)?;
        let mut p = o.pipeline;
        if p.workers == 0 {
            p.workers = 1;
        }
        let workers = p.workers.min(count).max(1);
        if p.max_in_flight_frames == 0 {
            p.max_in_flight_frames = workers.min(32);
        }
        if p.max_in_flight_bytes == 0 {
            p.max_in_flight_bytes = DEFAULT_IN_FLIGHT_BYTES;
        }
        let mut cap = 0;
        if encode {
            cap = p.max_encoded_frame_bytes;
            if cap == 0 {
                if native_bytes <= 0 || native_bytes > (i64::MAX - 65536) / 4 {
                    return Err(Error::limit("frame_bytes"));
                }
                cap = max_output.min(native_bytes.saturating_mul(4).saturating_add(65536));
            }
            if cap <= 0 || cap > max_output {
                return Err(Error::limit("encoded_frame_bytes"));
            }
        }
        Ok(Pipeline {
            options: p,
            workers,
            encoded_cap: cap,
            native_bytes,
            encode,
            verify: !o.skip_verification,
            planar_copy: o.planar_output != PlanarOutput::Default,
        })
    }
    fn charge(&self, raw: Result<i64, Error>) -> Result<i64, Error> {
        let raw = raw?;
        if raw <= 0 || self.native_bytes <= 0 {
            return Err(Error::limit("frame_bytes"));
        }
        let (mut native_copies, mut encoded_copies) = (4i64, 2i64);
        if self.verify && self.encode {
            native_copies = 8;
            encoded_copies = 3;
        }
        if self.planar_copy {
            native_copies = native_copies.saturating_add(1);
        }
        if self.native_bytes > i64::MAX.checked_div(native_copies).unwrap_or(0)
            || self.encoded_cap > i64::MAX.checked_div(encoded_copies).unwrap_or(0)
        {
            return Err(Error::limit("in_flight_bytes"));
        }
        let charge = native_copies.saturating_mul(self.native_bytes);
        let encoded = encoded_copies.saturating_mul(self.encoded_cap);
        if raw > i64::MAX.saturating_sub(charge)
            || encoded > i64::MAX.saturating_sub(charge).saturating_sub(raw)
        {
            return Err(Error::limit("in_flight_bytes"));
        }
        Ok(charge.saturating_add(raw).saturating_add(encoded))
    }
    fn run<T>(
        &self,
        count: i64,
        report: &mut TranscodeReport,
        raw_size: &dyn Fn(i64) -> Result<i64, Error>,
        produce: &mut dyn FnMut(i64) -> Result<T, Error>,
        consume: &mut dyn FnMut(i64, T) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut start = 0;
        while start < count {
            let mut end = start;
            let mut bytes = 0i64;
            while end < count && end.saturating_sub(start) < self.options.max_in_flight_frames {
                let charge = self.charge(raw_size(end))?;
                if charge > self.options.max_in_flight_bytes.saturating_sub(bytes) {
                    if end == start {
                        return Err(Error::limit("in_flight_bytes"));
                    }
                    break;
                }
                bytes = bytes.saturating_add(charge);
                end = end.saturating_add(1);
            }
            let mut results = Vec::with_capacity(end.saturating_sub(start) as usize);
            for i in start..end {
                results.push(produce(i)?);
            }
            let len = results.len() as i64;
            report.peak_in_flight_frames = report.peak_in_flight_frames.max(len);
            report.peak_in_flight_bytes = report.peak_in_flight_bytes.max(bytes);
            report.worker_limit = report.worker_limit.max(self.workers.min(len));
            for (slot, result) in results.into_iter().enumerate() {
                consume(start.saturating_add(slot as i64), result)?;
            }
            start = end;
        }
        Ok(())
    }
}
struct StreamOutput {
    report: TranscodeReport,
    output: File,
    frames: Option<(Vec<Vec<u8>>, FrameWriteOptions)>,
    output_fragments: i64,
    write_error: Option<Error>,
}
fn stream(
    txn: &mut Txn,
    target: &str,
    o: &TranscodeOptions,
    limits: &TranscodeLimits,
) -> Option<Result<TranscodeReport, Error>> {
    if o.rewrite.is_some() {
        return None;
    }
    let raw = txn.raw.clone();
    let file = if let Some(syntax) = &raw {
        match read_raw(txn, syntax, limits) {
            Ok(f) => f,
            Err(e) => return Some(Err(e)),
        }
    } else {
        if txn.source_bytes.len() as i64 > limits.max_input_bytes {
            return None;
        }
        let options = ReadFileOptions {
            max_element_bytes: limits.max_input_bytes,
            max_total_bytes: i64::MAX,
            max_sequence_depth: limits.max_depth,
            max_elements: limits.max_elements,
            max_fragments: i64::MAX,
            ..ReadFileOptions::default()
        };
        let defer = crate::object::deferred::DeferOptions {
            defer_pixel_data: true,
            ..Default::default()
        };
        crate::object::deferred::read_file_deferred(&txn.source_bytes, true, options, &defer)
            .ok()?;
        let options = ReadFileOptions {
            max_elements: 0,
            ..options
        };
        crate::object::read_file_with_options(&txn.source_bytes, options).ok()?
    };
    let source = file_source_syntax(&file).ok()?;
    let target = canonical_target(target).ok()?;
    if jpeg2000_part2(&source.uid) || jpeg2000_part2(&target.uid) {
        return None;
    }
    if let Err(e) = validate_target_verification_codec(&source, &target, o) {
        return Some(Err(e));
    }
    let little = |s: &Syntax| !s.big_endian && !s.deflated;
    if !little(&source)
        || !little(&target)
        || !file.preamble.is_empty() && file.preamble.len() != 128
    {
        return None;
    }
    let dataset = &file.dataset;
    if let Err(e) = validate_output_options(dataset, &target.uid, o) {
        return Some(Err(e));
    }
    let pixel = dataset.get(TAG_PIXEL_DATA)?;
    if dataset.has(TAG_FLOAT_PIXEL_DATA) || dataset.has(TAG_DOUBLE_FLOAT_PIXEL_DATA) {
        return None;
    }
    let mut mode = build_plan(&source, &target, o.force_reencode, true);
    if mode == Mode::NativeClone && o.planar_output != PlanarOutput::Default {
        mode = Mode::DecodeNative;
    }
    if mode == Mode::MissingPixelData
        || super::uses_codestream_transcoder(mode, &source, &target, o, false).is_some()
    {
        return None;
    }
    match has_nested_pixel_data(dataset, limits) {
        Ok(false) => {}
        Ok(true) => return None,
        Err(e) => return if raw.is_some() { Some(Err(e)) } else { None },
    }
    let metadata = match extract_metadata(dataset) {
        Ok(m) => m,
        Err(e) => {
            return if raw.is_some() {
                Some(Err(e.into()))
            } else {
                None
            };
        }
    };
    if metadata.bits_allocated < 8
        || mode == Mode::DecodeEncode && may_have_embedded_overlays(dataset)
    {
        return None;
    }
    let mut s = Stream {
        file: &file,
        source,
        target,
        mode,
        o,
        limits,
        metadata,
        input_bytes: 0,
        items: 0,
        frames: None,
    };
    match mode {
        Mode::NativeClone => {
            let Value::Raw(value) = &pixel.value else {
                return None;
            };
            if (value.len() as i64)
                < frame_size(&s.metadata).saturating_mul(s.metadata.number_of_frames)
            {
                return None;
            }
            s.input_bytes = value.len() as i64;
        }
        Mode::EquivalentClone => {
            let Value::Fragments {
                offset_table,
                fragments,
            } = &pixel.value
            else {
                return None;
            };
            if fragments.len() as i64 > limits.max_fragments {
                return if raw.is_some() {
                    Some(Err(bare_limit()))
                } else {
                    None
                };
            }
            s.items = fragments.len() as i64;
            s.input_bytes = fragments.iter().fold(offset_table.len() as i64, |t, f| {
                t.saturating_add(f.len() as i64)
            });
            if explicit_encapsulation(o) {
                match FrameReader::new(&file, output_frame_reader_options(o, limits)) {
                    Ok(r) => s.frames = Some(r),
                    Err(e) => return Some(Err(e)),
                }
            }
        }
        _ => match FrameReader::new(&file, source_frame_reader_options(o, limits)) {
            Ok(r) => {
                s.input_bytes = r.input_bytes();
                s.frames = Some(r);
            }
            Err(e) => {
                if raw.is_some() && !e.is(ErrorKind::FrameLayout) {
                    return Some(Err(e));
                }
                return None;
            }
        },
    }
    Some(s.run(txn))
}
struct Stream<'f> {
    file: &'f File,
    source: Syntax,
    target: Syntax,
    mode: Mode,
    o: &'f TranscodeOptions,
    limits: &'f TranscodeLimits,
    metadata: Metadata,
    input_bytes: i64,
    items: i64,
    frames: Option<FrameReader<'f>>,
}
impl Stream<'_> {
    fn reader(&self) -> Result<&FrameReader<'_>, Error> {
        self.frames
            .as_ref()
            .ok_or_else(|| other("pixeldata: no frame reader"))
    }
    fn run(&mut self, txn: &mut Txn) -> Result<TranscodeReport, Error> {
        let result = match self.mode {
            Mode::NativeClone | Mode::EquivalentClone => self.clone_output(),
            Mode::DecodeNative | Mode::DecodeEncapsulatedUncompressed => {
                let on_write = self.o.pipeline.workers <= 1
                    && self.o.write_options.group_length == group_length::PRESERVE;
                self.decode(txn, on_write)
            }
            _ => self.encode(txn),
        };
        let mut out = result?;
        out.report.streaming = true;
        let mut items = self.metadata.number_of_frames.saturating_add(1);
        if out.output_fragments > 0 {
            items = out.output_fragments.saturating_add(1);
        } else if self.mode == Mode::EquivalentClone {
            items = self.items.saturating_add(1);
        }
        let bytes = match out.write_error.take() {
            Some(e) => Err(e),
            None => match &out.frames {
                Some((frames, options)) => frame_writer::write_file_frames(
                    &out.output,
                    FrameSource {
                        frames,
                        packed_bits: None,
                    },
                    options,
                )
                .map_err(other),
                None => writer::write_file_with_options(&out.output, &self.o.write_options)
                    .map_err(other),
            },
        };
        let mut staged = txn.stage(bytes, self.limits)?;
        validate_staged_stream(&staged, self.limits, &self.target, items)?;
        txn.precommit(&staged)?;
        txn.publish(&mut staged)?;
        let mut report = out.report;
        txn.apply_report(&mut report);
        Ok(report)
    }
    fn frame_write_options(&self, lengths: &[i64]) -> Result<(FrameWriteOptions, i64), Error> {
        if !self.target.encapsulated {
            return Ok((
                FrameWriteOptions {
                    write: self.o.write_options.clone(),
                    ..FrameWriteOptions::default()
                },
                0,
            ));
        }
        let mut sizes = Vec::with_capacity(lengths.len());
        for &n in lengths {
            if n <= 0 {
                return Err(Error::pixel_bare(PixelErrorKind::PixelDataSizeMismatch));
            }
            sizes.push(n as u64);
        }
        let max_fragment = self.o.max_fragment_bytes as u64;
        frame_writer::plan(
            &sizes,
            max_fragment,
            self.o.offset_table,
            self.limits.max_fragments,
            frame_writer::single_fragment(&self.target.uid),
        )
        .map_err(|e| {
            if e == "fragment count exceeds limit" {
                Error::limit("fragments")
            } else {
                Error::unsupported(format!("output fragment layout: {e}"))
            }
        })?;
        let fragments = sizes
            .iter()
            .map(|&n| match max_fragment {
                0 => 1,
                m => n
                    .saturating_sub(1)
                    .checked_div(m)
                    .unwrap_or(0)
                    .saturating_add(1) as i64,
            })
            .fold(0i64, i64::saturating_add);
        Ok((
            FrameWriteOptions {
                write: self.o.write_options.clone(),
                offset_table: self.o.offset_table,
                max_fragment_bytes: self.o.max_fragment_bytes,
                max_fragments: self.limits.max_fragments,
            },
            fragments,
        ))
    }
    fn clone_output(&self) -> Result<StreamOutput, Error> {
        let mut report = TranscodeReport {
            pixel_data_preserved: true,
            frames: self.metadata.number_of_frames,
            native_bytes: total_size(&self.metadata),
            encoded_bytes: self.input_bytes,
            ..TranscodeReport::default()
        };
        let source_dataset = &self.file.dataset;
        let mut dataset = source_dataset.clone();
        dataset.set_value_big_endian(false);
        if self.input_bytes > self.limits.max_output_bytes {
            return Err(Error::limit("output_bytes"));
        }
        apply_identity(source_dataset, &mut dataset, &mut report, self.o)?;
        let mut elements = dataset.elements().to_vec();
        remove_elements(&mut elements, &[TAG_PIXEL_DATA]);
        let explicit = self.source.encapsulated && explicit_encapsulation(self.o);
        let mut header = Object::from_elements(elements);
        if explicit {
            header = Object::from_elements(encoded_metadata_elements(&header, None));
        }
        let mut output = output_file(self.file, header, &report, &self.target.uid, self.limits)?;
        if !explicit {
            output.dataset = dataset;
            return Ok(StreamOutput {
                report,
                output,
                frames: None,
                output_fragments: 0,
                write_error: None,
            });
        }
        let reader = self.reader()?;
        let count = reader.metadata().number_of_frames;
        if count > self.limits.max_frames
            || u64::from(self.metadata.rows)
                .saturating_mul(u64::from(self.metadata.columns))
                .saturating_mul(count as u64)
                > self.limits.max_pixels
        {
            return Err(Error::limit("pixels_or_frames"));
        }
        let cap = if self.o.pipeline.max_in_flight_bytes == 0 {
            DEFAULT_IN_FLIGHT_BYTES
        } else {
            self.o.pipeline.max_in_flight_bytes
        };
        let mut peak = 0;
        let mut lengths = Vec::with_capacity(count as usize);
        for i in 0..count {
            let n = reader.raw_frame_length(i)?;
            if n > cap {
                return Err(Error::limit("in_flight_bytes"));
            }
            peak = peak.max(n);
            lengths.push(n);
        }
        report.worker_limit = 1;
        report.peak_in_flight_frames = 1;
        report.peak_in_flight_bytes = peak;
        let (options, fragments) = self.frame_write_options(&lengths)?;
        let mut frames = Vec::with_capacity(count as usize);
        let mut write_error = None;
        for i in 0..count {
            match reader.raw_frame(i) {
                Ok(f) => frames.push(f),
                Err(e) => {
                    write_error = Some(e);
                    break;
                }
            }
        }
        Ok(StreamOutput {
            report,
            output,
            frames: Some((frames, options)),
            output_fragments: fragments,
            write_error,
        })
    }
    fn check_decode_budget(&self) -> Result<(), Error> {
        check_admission(
            &transcode_decompress_limits(self.limits),
            &self.metadata,
            &self.source,
            self.input_bytes,
        )
    }
    fn preflight(&self, encode: bool) -> Result<(), Error> {
        let reader = self.reader()?;
        let pipeline = Pipeline::new(
            self.o,
            self.metadata.number_of_frames,
            frame_size(&self.metadata),
            self.limits.max_output_bytes,
            encode,
        )?;
        let charge = pipeline.charge(reader.raw_frame_length(0))?;
        if charge > pipeline.options.max_in_flight_bytes {
            return Err(Error::limit("in_flight_bytes"));
        }
        Ok(())
    }
    fn check_registry(&self) -> Result<(), Error> {
        if self.source.encapsulated
            && self.source.uid != UID_ENCAPSULATED_UNCOMPRESSED
            && self.o.decoder.is_none()
        {
            return Err(codec_registry_nil(&self.source.uid));
        }
        super::DecoderRegistry::refuse(self.o.decoder, &self.source.uid)
    }
    fn native_header(&self, first: &Object) -> Object {
        let mut elements = first.elements().to_vec();
        remove_elements(&mut elements, &[TAG_PIXEL_DATA, TAG_NUMBER_OF_FRAMES]);
        if let Some(original) = self.file.dataset.get(TAG_NUMBER_OF_FRAMES) {
            elements.push(original.clone());
        }
        let vr = first
            .get(TAG_PIXEL_DATA)
            .map_or(crate::object::Vr::OB, Element::vr);
        elements.push(super::raw_element(TAG_PIXEL_DATA, vr, Vec::new()));
        object_from(elements, false)
    }
    fn native_output_header(&self, decoded: &Object) -> Result<Object, Error> {
        let mut header = self.native_header(decoded);
        if self.o.planar_output == PlanarOutput::Default {
            return Ok(header);
        }
        let m = extract_metadata(&header)?;
        let target = output_planar(&header, &m, self.o.planar_output)?;
        if m.samples_per_pixel > 1 {
            put_planar(&mut header, target);
        }
        Ok(header)
    }
    fn native_output_frame(&self, decoded: &Object, frame: Vec<u8>) -> Result<Vec<u8>, Error> {
        if self.o.planar_output == PlanarOutput::Default {
            return Ok(frame);
        }
        let m = extract_metadata(decoded)?;
        let target = output_planar(decoded, &m, self.o.planar_output)?;
        if m.samples_per_pixel == 1 {
            return Ok(frame);
        }
        reorder_frame(&frame, &m, target)
    }
    fn next_frame(&self, i: i64, layout: &Object, detail: &str) -> Result<Vec<u8>, Error> {
        let (decoded, frame, _) = self.reader()?.decode_frame_object(i)?;
        if !same_decoded_layout(layout, &decoded) {
            return Err(Error::pixel(PixelErrorKind::IncompatiblePixelData, detail));
        }
        Ok(frame)
    }
    fn decode(&self, txn: &mut Txn, on_write: bool) -> Result<StreamOutput, Error> {
        self.check_registry()?;
        self.check_decode_budget()?;
        self.preflight(false)?;
        let reader = self.reader()?;
        let (first, first_frame, diagnostics) = reader.decode_frame_object(0)?;
        let decoded_header = self.native_header(&first);
        let header = self.native_output_header(&first)?;
        let first_frame = self.native_output_frame(&decoded_header, first_frame)?;
        let metadata = extract_metadata(&header)?;
        let native_bytes = total_size(&metadata);
        if native_bytes <= 0 || native_bytes > self.limits.max_output_bytes {
            return Err(Error::limit("native_bytes"));
        }
        let size = frame_size(&metadata);
        let count = metadata.number_of_frames;
        let pipeline = Pipeline::new(self.o, count, size, self.limits.max_output_bytes, false)?;
        let mut report = TranscodeReport {
            frames: count,
            native_bytes,
            decode_diagnostics: diagnostics,
            ..TranscodeReport::default()
        };
        let mut frames = Vec::with_capacity(count as usize);
        let mut write_error = None;
        let mut first = Some(first_frame);
        let mut produce_frame = |i: i64| -> Result<Vec<u8>, Error> {
            let frame = if i == 0 {
                first.take().unwrap_or_default()
            } else {
                let frame = self.next_frame(i, &decoded_header, "decoded frame layout")?;
                self.native_output_frame(&decoded_header, frame)?
            };
            if frame.len() as i64 != size {
                return Err(Error::pixel(
                    PixelErrorKind::PixelDataSizeMismatch,
                    "decoded frame size",
                ));
            }
            Ok(frame)
        };
        if on_write {
            let mut peak = 0;
            for i in 0..count {
                let charge = pipeline.charge(reader.raw_frame_length(i))?;
                if charge > pipeline.options.max_in_flight_bytes {
                    return Err(Error::limit("in_flight_bytes"));
                }
                peak = peak.max(charge);
            }
            report.worker_limit = 1;
            report.peak_in_flight_frames = 1;
            report.peak_in_flight_bytes = peak;
            for i in 0..count {
                match produce_frame(i) {
                    Ok(f) => frames.push(f),
                    Err(e) => {
                        write_error = Some(e);
                        break;
                    }
                }
            }
        } else {
            let mut spooled = 0i64;
            let raw_size = |i: i64| reader.raw_frame_length(i);
            let budget = &mut txn.temporary;
            let mut consume = |_: i64, data: Vec<u8>| -> Result<(), Error> {
                if data.len() as i64 > self.limits.max_output_bytes.saturating_sub(spooled) {
                    return Err(Error::limit("output_bytes"));
                }
                if let Some(b) = budget.as_mut() {
                    b.reserve(data.len() as i64)?;
                }
                spooled = spooled.saturating_add(data.len() as i64);
                frames.push(data);
                Ok(())
            };
            pipeline.run(
                count,
                &mut report,
                &raw_size,
                &mut produce_frame,
                &mut consume,
            )?;
        }
        let mut dataset = if self.mode == Mode::DecodeEncapsulatedUncompressed {
            report.encoded_bytes = native_bytes;
            Object::from_elements(encoded_metadata_elements(&header, None))
        } else {
            header
        };
        dataset = clone_detached(&dataset, &output_clone_limits(*self.limits))?;
        dataset.set_value_big_endian(false);
        apply_identity(&self.file.dataset, &mut dataset, &mut report, self.o)?;
        let output = output_file(self.file, dataset, &report, &self.target.uid, self.limits)?;
        let lengths = vec![size; count as usize];
        let (options, fragments) = self.frame_write_options(&lengths)?;
        Ok(StreamOutput {
            report,
            output,
            frames: Some((frames, options)),
            output_fragments: fragments,
            write_error,
        })
    }
    fn encode(&self, txn: &mut Txn) -> Result<StreamOutput, Error> {
        self.check_registry()?;
        self.check_decode_budget()?;
        self.preflight(true)?;
        let reader = self.reader()?;
        let (first, first_frame, diagnostics) = reader.decode_frame_object(0)?;
        let header = self.native_header(&first);
        let metadata = extract_metadata(&header)?;
        let native_bytes = total_size(&metadata);
        if native_bytes <= 0 || native_bytes > self.limits.max_output_bytes {
            return Err(Error::limit("native_bytes"));
        }
        let mut report = TranscodeReport {
            frames: metadata.number_of_frames,
            native_bytes,
            decode_diagnostics: diagnostics,
            ..TranscodeReport::default()
        };
        let resolved = resolve(&self.target, &metadata, self.o)?;
        let size = frame_size(&metadata);
        let count = metadata.number_of_frames;
        let pipeline = Pipeline::new(self.o, count, size, self.limits.max_output_bytes, true)?;
        if count > self.limits.max_fragments {
            return Err(Error::limit("fragments"));
        }
        let lossless = resolved.caps.lossless;
        let mut first = Some(first_frame);
        let mut state = EncodeState::default();
        let mut fragments: Vec<Vec<u8>> = Vec::with_capacity(count as usize);
        let mut verification = None;
        {
            let raw_size = |i: i64| reader.raw_frame_length(i);
            let mut produce = |i: i64| -> Result<Produced, Error> {
                let frame = if i == 0 {
                    first.take().unwrap_or_default()
                } else {
                    self.next_frame(
                        i,
                        &header,
                        &format!(
                            "frame {} decodes to another color layout",
                            i.saturating_add(1)
                        ),
                    )?
                };
                if frame.len() as i64 != size {
                    return Err(Error::pixel(
                        PixelErrorKind::PixelDataSizeMismatch,
                        "decoded frame size",
                    ));
                }
                let encoded = encode_frame(&resolved, &metadata, pipeline.encoded_cap, &frame)?;
                let mut measured = None;
                if !self.o.skip_verification {
                    let delta = (encoded.photometric.clone(), encoded.planar);
                    let verify =
                        object_from(encoded_metadata_elements(&header, Some(&delta)), false);
                    measured = verify_transcoded_frame(
                        &header,
                        &verify,
                        &encoded.data,
                        &frame,
                        &self.target,
                        self.o,
                        self.limits,
                        i as usize,
                        lossless,
                    )?;
                }
                Ok((encoded, measured))
            };
            let budget = &mut txn.temporary;
            let mut consume = |i: i64, (encoded, measured): Produced| -> Result<(), Error> {
                let data = state.accept(
                    &resolved,
                    &metadata,
                    self.limits.max_output_bytes,
                    i as usize,
                    encoded,
                )?;
                add_lossy(&mut verification, measured);
                if let Some(b) = budget.as_mut() {
                    b.reserve(data.len() as i64)?;
                }
                fragments.push(data);
                Ok(())
            };
            pipeline.run(count, &mut report, &raw_size, &mut produce, &mut consume)?;
        }
        report.lossy_verification = verification;
        report.verification = if self.o.skip_verification {
            Verification::Skipped
        } else if lossless {
            Verification::PixelExact
        } else if report.lossy_verification.is_some() {
            Verification::PixelError
        } else {
            Verification::Decode
        };
        let delta = (state.photometric.clone(), state.planar);
        if !state.profile.is_empty() {
            report.encode_compatibility_profiles = vec![state.profile.clone()];
        }
        let draft = Object::from_elements(encoded_metadata_elements(&header, Some(&delta)));
        let mut result = clone_detached(&draft, &output_clone_limits(*self.limits))?;
        result.set_value_big_endian(false);
        report.encoded_bytes = state.total;
        report.lossy = !lossless;
        if report.lossy {
            apply_lossy_metadata(
                &mut result,
                native_bytes,
                state.total,
                &resolved.caps.lossy_method,
                &self.o.uid_root,
            )?;
        }
        apply_identity(&self.file.dataset, &mut result, &mut report, self.o)?;
        let output = output_file(self.file, result, &report, &self.target.uid, self.limits)?;
        let lengths: Vec<i64> = fragments.iter().map(|f| f.len() as i64).collect();
        let (options, output_fragments) = self.frame_write_options(&lengths)?;
        Ok(StreamOutput {
            report,
            output,
            frames: Some((fragments, options)),
            output_fragments,
            write_error: None,
        })
    }
}
fn same_decoded_layout(first: &Object, decoded: &Object) -> bool {
    [
        TAG_PHOTOMETRIC,
        TAG_SAMPLES_PER_PIXEL,
        TAG_PLANAR_CONFIGURATION,
    ]
    .iter()
    .all(|&tag| match (first.get(tag), decoded.get(tag)) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.raw() == b.raw() && first.get_string(tag) == decoded.get_string(tag)
        }
        _ => false,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pipeline_charges() {
        let o = TranscodeOptions::default();
        let p = Pipeline::new(&o, 3, 100, 1 << 20, true).unwrap();
        assert_eq!(p.encoded_cap, 4 * 100 + 65536);
        assert_eq!(p.charge(Ok(10)).unwrap(), 8 * 100 + 10 + 3 * p.encoded_cap);
        assert!(p.charge(Ok(0)).unwrap_err().is_resource_limit());
    }
    #[test]
    fn durability_names() {
        assert_eq!(durability::as_str(1), "atomic");
        assert_eq!(durability::as_str(7), "invalid");
    }
}
