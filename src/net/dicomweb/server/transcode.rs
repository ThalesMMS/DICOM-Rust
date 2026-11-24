use super::super::mimeheader::parse_media_type;
use super::super::types::InstanceRef;
use super::http::{Request, ResponseWriter};
use super::media::{
    MediaPreference, default_frame_syntax, frame_media_matches_syntax, matching_media_preference,
    media_preference_specificity, media_type_with_transfer_syntax,
    normalized_dicom_part_content_type, parse_media_preferences, prepared_preferences_supported,
    web_service_syntax_allowed,
};
use super::spool::{
    MultipartWriter, ResponseSpool, activate_retrieve_directory, copy_bounded,
    deactivate_retrieve_directory, server_io_error,
};
use super::wado::{
    PartReader, RetrieveLevel, RetrievePart, RetrieveRequest, has_multipart_frame_preference,
    header_with, recover, retrieve_ref_matches, with_reader_closed,
};
use super::{
    Inner, ServerError, ServerLimits, ServerWriter, Slots, check_read_lease,
    classify_backend_error, ctx_err, lease_of, parse_frames, request_accept_header, resource_parts,
};
use crate::net::ul::Ctx;
use crate::object::parser::source::{ByteRead, MemFile};
use crate::object::parser::{
    Deferral, Disposition, ReaderOptions, SelectiveReader, SelectiveToken,
};
use crate::object::writer::{file_meta_mode, group_length, sequence_length};
use crate::object::{self, ElementHeader, File, Kind, Object, ReadFileOptions, Tag, Value, Vr};
use crate::pixel::frame_reader::{FrameReader, FrameReaderOptions};
use crate::pixel::pixeldata::{
    PixelErrorKind, extract_metadata, registered_codec_uids, total_size,
};
use crate::pixel::transcode::{
    self as tc, ErrorKind as TcKind, TranscodeOptions, UidPolicy, Verification,
};
use crate::transfer::{self, Syntax};
use std::cell::RefCell;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
const EXPLICIT_VR_LE: &str = "1.2.840.10008.1.2.1";
const DEFLATED_LE: &str = "1.2.840.10008.1.2.1.99";
const ENCAPSULATED_UNCOMPRESSED: &str = "1.2.840.10008.1.2.1.98";
const RETRIEVE_MEMORY_STAGE_BYTES: i64 = 8 << 20;
const RETRIEVE_LEASE_CHECK_BYTES: i64 = 256 << 10;
const RETRIEVE_BUFFER_BYTES: i64 = 64 << 10;
const CONVERTED_OBJECT_SLACK: i64 = 4 << 10;
#[derive(Clone, Debug, Default)]
pub struct RetrieveTranscodePolicy {
    pub options: TranscodeOptions,
    pub target_transfer_syntax_uids: Vec<String>,
    pub max_concurrent_jobs: i64,
    pub prefer_stored_syntax: bool,
}
pub(crate) struct PolicyState {
    pub(crate) policy: RetrieveTranscodePolicy,
    max_uid_bytes: i64,
    sem: Slots,
}
fn invalid(detail: &str) -> ServerError {
    ServerError::InvalidServerOptions(detail.to_string())
}
fn valid_ae_title(title: &str) -> bool {
    title.len() <= 16
        && !title.trim().is_empty()
        && title
            .bytes()
            .all(|b| (0x20..=0x7e).contains(&b) && b != b'\\')
}
pub(crate) fn normalize_policy(
    policy: Option<&RetrieveTranscodePolicy>,
    limits: &ServerLimits,
    uri: bool,
) -> Result<Option<RetrieveTranscodePolicy>, ServerError> {
    if policy.is_none() && !uri {
        return Ok(None);
    }
    let mut p = policy.cloned().unwrap_or_default();
    if p.max_concurrent_jobs < 0
        || p.max_concurrent_jobs > 64
        || p.target_transfer_syntax_uids.len() > 64
    {
        return Err(invalid("retrieval policy limit"));
    }
    if p.max_concurrent_jobs == 0 {
        p.max_concurrent_jobs = 2;
    }
    p.max_concurrent_jobs = p.max_concurrent_jobs.min(limits.max_concurrent_requests);
    let mut o = p.options.clone();
    let preamble = o.write_options.preamble.as_ref().map_or(0, Vec::len);
    if (preamble != 0 && preamble != 128)
        || o.offset_table > object::frame_writer::offset_table::EMPTY
        || o.durability > tc::durability::ATOMIC
        || o.max_fragment_bytes < 0
        || o.max_fragment_bytes > 0xFFFF_FFFE
        || (o.max_fragment_bytes != 0
            && (o.max_fragment_bytes < 2 || o.max_fragment_bytes % 2 != 0))
        || o.pipeline.workers < 0
        || o.pipeline.max_in_flight_frames < 0
        || o.pipeline.max_in_flight_bytes < 0
        || o.pipeline.max_encoded_frame_bytes < 0
    {
        return Err(invalid("retrieval output policy"));
    }
    let w = &o.write_options;
    if object::writer::validate_serialization_options(DEFLATED_LE, w.deflate_level, &w.padding)
        .is_err()
    {
        return Err(invalid("retrieval serialization policy"));
    }
    if w.sequence_length > sequence_length::DEFINED
        || w.group_length > group_length::REMOVE
        || w.meta_mode < file_meta_mode::UPDATE
        || w.meta_mode > file_meta_mode::PRESERVE
        || [
            &w.ae_titles.source,
            &w.ae_titles.sending,
            &w.ae_titles.receiving,
        ]
        .iter()
        .any(|t| !t.is_empty() && !valid_ae_title(t))
    {
        return Err(invalid("retrieval file policy"));
    }
    if o.allow_lossy
        || o.force_reencode
        || o.secondary_capture
        || o.uid_policy == UidPolicy::Always
        || o.skip_verification
        || o.max_pixel_error.is_some()
        || o.average_block_size != 0
        || o.extract_embedded_overlays
        || w.allow_partial
        || w.default_missing_transfer_syntax
        || !w.override_missing_transfer_syntax.is_empty()
        || w.omit_reconciled_data_set_sop_uids
    {
        return Err(invalid(
            "retrieval requires unchanged identity and verified samples",
        ));
    }
    o.uid_policy = UidPolicy::Preserve;
    if preamble == 0 {
        o.write_options.preamble = None;
    }
    let Ok(mut resolved) = tc::resolve_limits(o.limits) else {
        return Err(invalid("retrieval transcode limits"));
    };
    resolved.max_input_bytes = resolved
        .max_input_bytes
        .min(limits.max_part_bytes)
        .min(i64::MAX - 1);
    resolved.max_output_bytes = resolved
        .max_output_bytes
        .min(limits.max_part_bytes)
        .min(limits.max_response_bytes)
        .min(i64::MAX - 1);
    resolved.max_frames = resolved.max_frames.min(limits.max_frames);
    resolved.max_elements = resolved.max_elements.min(limits.max_json_values);
    resolved.max_depth = resolved.max_depth.min(limits.max_json_depth);
    resolved.max_duration_nanos = resolved.max_duration_nanos.min(limits.max_duration.0);
    o.limits = resolved;
    if tc::resolve_verification_options(o.clone()).is_err() {
        return Err(invalid("retrieval verification policy"));
    }
    if p.target_transfer_syntax_uids.is_empty() {
        p.target_transfer_syntax_uids = vec![EXPLICIT_VR_LE.to_string()];
    }
    let mut seen: Vec<&str> = Vec::new();
    for uid in &p.target_transfer_syntax_uids {
        if uid.len() as i64 > limits.max_uid_bytes
            || !web_service_syntax_allowed(uid)
            || object::transfer::normalize_uid(uid) != uid
            || seen.contains(&uid.as_str())
        {
            return Err(invalid("retrieval target syntax"));
        }
        seen.push(uid);
        if object::writer::validate_serialization_options(
            uid,
            o.write_options.deflate_level,
            &o.write_options.padding,
        )
        .is_err()
        {
            return Err(invalid("retrieval target serialization policy"));
        }
    }
    p.options = o;
    Ok(Some(p))
}
impl PolicyState {
    pub(crate) fn new(policy: RetrieveTranscodePolicy, limits: &ServerLimits) -> PolicyState {
        PolicyState {
            sem: Slots::new(policy.max_concurrent_jobs),
            max_uid_bytes: limits.max_uid_bytes,
            policy,
        }
    }
    fn valid_uid(&self, uid: &str) -> bool {
        uid.len() as i64 <= self.max_uid_bytes && object::is_valid_uid(uid.as_bytes())
    }
    fn has_decoder(&self, uid: &str) -> bool {
        self.policy
            .options
            .decoder
            .is_some_and(|r| r.decodes(uid) && registered_codec_uids().contains(&uid))
    }
    fn target_for(
        &self,
        source: &Syntax,
        stored_lossy: bool,
        accept: &[MediaPreference],
        frames: bool,
        multipart_required: bool,
        uri: bool,
    ) -> Result<Syntax, ServerError> {
        let mut candidates = self.policy.target_transfer_syntax_uids.clone();
        candidates.push(source.uid.to_string());
        self.target_among(
            &candidates,
            source,
            stored_lossy,
            accept,
            frames,
            multipart_required,
            uri,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn target_among(
        &self,
        candidates: &[String],
        source: &Syntax,
        stored_lossy: bool,
        accept: &[MediaPreference],
        frames: bool,
        multipart_required: bool,
        uri: bool,
    ) -> Result<Syntax, ServerError> {
        let stored = source.uid.as_ref();
        let mut best_quality = -1.0f64;
        let mut best_specificity = -1i32;
        let mut best: Option<Syntax> = None;
        for uid in candidates {
            let Some(syntax) = transfer::default_registry().get(uid) else {
                continue;
            };
            if !web_service_syntax_allowed(uid) {
                continue;
            }
            if uid != stored {
                if source.media_payload {
                    continue;
                }
                if source.encapsulated
                    && stored != ENCAPSULATED_UNCOMPRESSED
                    && !self.has_decoder(stored)
                {
                    continue;
                }
                if syntax.encapsulated && syntax.uid != ENCAPSULATED_UNCOMPRESSED {
                    let lossless = self
                        .policy
                        .options
                        .encoders
                        .as_ref()
                        .and_then(|r| r.capabilities_of(uid))
                        .is_some_and(|c| c.lossless);
                    if !lossless || !self.has_decoder(uid) {
                        continue;
                    }
                }
            }
            let mut media = "application/dicom";
            if frames {
                if syntax.media_payload {
                    continue;
                }
                media = frame_media_for_syntax(uid);
                if media.is_empty() {
                    continue;
                }
            }
            let filtered: Vec<MediaPreference> = accept
                .iter()
                .filter(|p| {
                    if p.transfer_syntax_uid.is_empty() && p.quality > 0.0 {
                        if frames {
                            let default = default_frame_syntax(&p.media_type);
                            if !default.is_empty() && uid != default {
                                return false;
                            }
                        } else if uid != EXPLICIT_VR_LE && !(uid == stored && stored_lossy && !uri)
                        {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect();
            let Some(preference) =
                matching_media_preference(&filtered, media, uid, multipart_required)
            else {
                continue;
            };
            let specificity = media_preference_specificity(&preference, media, uid).unwrap_or(0);
            let stored_tie = self.policy.prefer_stored_syntax
                && uid == stored
                && preference.quality == best_quality
                && specificity == best_specificity;
            if preference.quality > best_quality
                || (preference.quality == best_quality && specificity > best_specificity)
                || stored_tie
            {
                best = Some(syntax);
                best_quality = preference.quality;
                best_specificity = specificity;
            }
        }
        if best_quality < 0.0 {
            return Err(ServerError::Unsupported);
        }
        best.ok_or(ServerError::Unsupported)
    }
    #[allow(clippy::too_many_arguments)]
    fn admit_conversion(
        &self,
        ctx: &Ctx,
        file: &File,
        stored: &Syntax,
        target: Syntax,
        lossy: bool,
        accept: &[MediaPreference],
        frames: bool,
        multipart_required: bool,
        uri: bool,
    ) -> Result<Syntax, ServerError> {
        let err = match tc::check_decode_limits_deferred(file, &target.uid, &self.policy.options) {
            Ok(()) => return Ok(target),
            Err(e) => e,
        };
        ctx_err(ctx)?;
        check_read_lease(ctx)?;
        if !tc::is_limit_error(&err) {
            return Err(classify_transcode_error(ctx, &err));
        }
        self.target_among(
            &[stored.uid.to_string()],
            stored,
            lossy,
            accept,
            frames,
            multipart_required,
            uri,
        )
        .map_err(|_| ServerError::ResourceLimit)
    }
    fn single_uid(&self, obj: &Object, tag: Tag) -> Result<String, ServerError> {
        let Some(e) = obj.get(tag) else {
            return Err(ServerError::Backend);
        };
        if e.header.vr != Vr::UI {
            return Err(ServerError::Backend);
        }
        let mut maximum = self.max_uid_bytes.min(64);
        maximum += maximum % 2;
        if e.header.length_set && i64::from(e.header.length) > maximum {
            return Err(ServerError::Backend);
        }
        let uid = match &e.value {
            Value::Raw(raw) => {
                if raw.len() as i64 > maximum {
                    return Err(ServerError::Backend);
                }
                String::from_utf8_lossy(raw).into_owned()
            }
            Value::Strings(values) => {
                if values.len() != 1 || values[0].len() as i64 > maximum {
                    return Err(ServerError::Backend);
                }
                values[0].clone()
            }
            _ => return Err(ServerError::Backend),
        };
        let uid = object::transfer::normalize_uid(&uid).to_string();
        if !self.valid_uid(&uid) {
            return Err(ServerError::Backend);
        }
        Ok(uid)
    }
    fn read_options(&self, maximum: i64) -> ReadFileOptions {
        let l = &self.policy.options.limits;
        ReadFileOptions {
            max_element_bytes: maximum,
            max_pixel_data_bytes: maximum,
            max_total_bytes: maximum,
            max_elements: l.max_elements,
            max_sequence_depth: l.max_depth,
            ..ReadFileOptions::default()
        }
    }
    fn scan_uid_occurrences(
        &self,
        ctx: &Ctx,
        data: &Rc<[u8]>,
        header: &object::Part10Header,
        maximum: i64,
    ) -> Result<(), ServerError> {
        let l = &self.policy.options.limits;
        let mut opts = ReaderOptions {
            max_element_bytes: maximum,
            max_pixel_data_bytes: maximum,
            max_total_bytes: maximum,
            max_elements: l.max_elements,
            max_sequence_depth: l.max_depth,
            skip_pixel_data: true,
            ..ReaderOptions::default()
        };
        let mut maximum_uid = self.max_uid_bytes.min(64);
        maximum_uid += maximum_uid % 2;
        let mut scan = |source: Box<dyn ByteRead>,
                        sized: Option<(i64, i64)>,
                        syntax: object::Syntax,
                        base: i64,
                        tags: &'static [Tag]|
         -> Result<(), ServerError> {
            let seen: Rc<RefCell<Vec<Tag>>> = Rc::new(RefCell::new(Vec::new()));
            let seen_in = seen.clone();
            opts.base_offset = base;
            let select = Box::new(
                move |path: &[object::error::PathStep], h: &ElementHeader, _offset: i64| {
                    if path.len() == 1 && tags.contains(&h.tag) {
                        let mut seen = seen_in.borrow_mut();
                        if seen.contains(&h.tag)
                            || h.vr != Vr::UI
                            || h.is_undefined()
                            || i64::from(h.length) > maximum_uid
                        {
                            return Err(());
                        }
                        seen.push(h.tag);
                    }
                    if h.tag == Tag::PIXEL_DATA {
                        return Ok(Disposition::Skip);
                    }
                    if h.vr == Vr::SQ || h.is_undefined() || object::dict::is_private_creator(h.tag)
                    {
                        return Ok(Disposition::Materialize);
                    }
                    Ok(Disposition::Skip)
                },
            );
            let mut reader = SelectiveReader::new(source, sized, syntax, opts, select, 0)
                .map_err(|e| classify_read_error(ctx, &e))?;
            loop {
                match reader.next() {
                    Ok(_) => {}
                    Err(e) if e.is(Kind::Eof) => break,
                    Err(e) => return Err(classify_read_error(ctx, &e)),
                }
            }
            if tags.iter().any(|t| !seen.borrow().contains(t)) {
                return Err(ServerError::Backend);
            }
            ctx_err(ctx)
        };
        if header.data_set_offset < 132 || header.data_set_offset > maximum {
            return Err(ServerError::ResourceLimit);
        }
        const META_TAGS: [Tag; 3] = [
            Tag::new(0x0002, 0x0002),
            Tag::new(0x0002, 0x0003),
            Tag::new(0x0002, 0x0010),
        ];
        const DATA_TAGS: [Tag; 4] = [
            Tag::new(0x0008, 0x0016),
            Tag::new(0x0008, 0x0018),
            Tag::new(0x0020, 0x000d),
            Tag::new(0x0020, 0x000e),
        ];
        let offset = (header.data_set_offset as usize).min(data.len());
        let meta: Rc<[u8]> = Rc::from(&data[132.min(offset)..offset]);
        let meta_len = meta.len() as i64;
        let explicit_le = object::transfer::lookup(EXPLICIT_VR_LE).ok_or(ServerError::Backend)?;
        scan(
            Box::new(MemFile::new(meta)),
            Some((meta_len, 0)),
            explicit_le,
            132,
            &META_TAGS,
        )?;
        let syntax = header.transfer_syntax;
        if syntax.deflated {
            let (decoded, err, _) = object::inflate::inflate_raw(&data[offset..]);
            return scan(
                Box::new(object::inflate::Inflated::new(decoded, err)),
                None,
                syntax,
                header.data_set_offset,
                &DATA_TAGS,
            );
        }
        let file = MemFile::new(data.clone());
        file.seek(offset);
        scan(
            Box::new(file),
            Some((-1, offset as i64)),
            syntax,
            header.data_set_offset,
            &DATA_TAGS,
        )
    }
    fn read_prepared_source(
        &self,
        ctx: &Ctx,
        data: Vec<u8>,
        size: i64,
        maximum: i64,
        reference: &InstanceRef,
    ) -> Result<PreparedRetrieve, ServerError> {
        check_read_lease(ctx)?;
        if size < 0 || size > maximum {
            return Err(ServerError::ResourceLimit);
        }
        let mut gate = LeaseGate::default();
        let opts = self.read_options(maximum);
        let bytes: Rc<[u8]> = Rc::from(data.as_slice());
        ctx_err(ctx)?;
        if size > 0 {
            gate.read(ctx, size.min(RETRIEVE_BUFFER_BYTES))
                .map_err(|e| parse_read_refusal(ctx, e))?;
        }
        let header = object::read_part10_header_mem(&MemFile::new(bytes.clone()), &opts)
            .map_err(|e| classify_read_error(ctx, &e))?;
        let meta_instance = self.single_uid(&header.meta, Tag::new(0x0002, 0x0003));
        if meta_instance.as_deref() != Ok(reference.sop_instance_uid.as_str()) {
            return Err(ServerError::Backend);
        }
        let meta_class = self.single_uid(&header.meta, Tag::new(0x0002, 0x0002))?;
        let meta_syntax = self.single_uid(&header.meta, Tag::new(0x0002, 0x0010));
        if meta_syntax.as_deref() != Ok(header.transfer_syntax.uid) {
            return Err(ServerError::Backend);
        }
        gate.pass(ctx, size)
            .map_err(|e| parse_read_refusal(ctx, e))?;
        self.scan_uid_occurrences(ctx, &bytes, &header, maximum)?;
        gate.check(ctx, 0)?;
        let parsed = if header.transfer_syntax.deflated {
            let opts = ReadFileOptions {
                skip_pixel_data: true,
                ..opts
            };
            object::read_file_with_options(&data, opts)
        } else {
            let defer = Deferral {
                defer_pixel_data: true,
                inline_value_bytes_threshold: 64 << 10,
                defer_nested_values: true,
                ..Deferral::default()
            };
            object::deferred::read_file_deferred(&data, true, opts, &defer)
        };
        gate.pass(ctx, size)
            .map_err(|e| parse_read_refusal(ctx, e))?;
        let parsed = match parsed {
            Ok(file) => file,
            Err((_, e)) => {
                ctx_err(ctx)?;
                return Err(classify_read_error(ctx, &e));
            }
        };
        for (tag, expected) in [
            (
                Tag::new(0x0020, 0x000d),
                reference.study_instance_uid.as_str(),
            ),
            (
                Tag::new(0x0020, 0x000e),
                reference.series_instance_uid.as_str(),
            ),
            (
                Tag::new(0x0008, 0x0018),
                reference.sop_instance_uid.as_str(),
            ),
            (Tag::new(0x0008, 0x0016), meta_class.as_str()),
        ] {
            if self.single_uid(&parsed.dataset, tag).as_deref() != Ok(expected) {
                return Err(ServerError::Backend);
            }
        }
        gate.check(ctx, 0)?;
        Ok(PreparedRetrieve {
            data,
            parsed,
            reference: reference.clone(),
            size,
            gate,
        })
    }
    fn read_prepared_file(
        &self,
        ctx: &Ctx,
        path: &Path,
        maximum: i64,
        reference: &InstanceRef,
    ) -> Result<PreparedRetrieve, ServerError> {
        check_read_lease(ctx)?;
        let mut file = fs::File::open(path).map_err(|_| ServerError::Backend)?;
        let info = file.metadata().map_err(|_| ServerError::Backend)?;
        if !info.is_file() {
            return Err(ServerError::ResourceLimit);
        }
        let size = info.len() as i64;
        if size > maximum {
            check_read_lease(ctx)?;
            return Err(ServerError::ResourceLimit);
        }
        let mut data = Vec::with_capacity(size as usize);
        file.read_to_end(&mut data)
            .map_err(|_| ServerError::Backend)?;
        self.read_prepared_source(ctx, data, size, maximum, reference)
    }
}
#[derive(Default)]
struct LeaseGate {
    validated: bool,
    unchecked: i64,
}
impl LeaseGate {
    fn check(&mut self, ctx: &Ctx, read: i64) -> Result<(), ServerError> {
        self.unchecked += read;
        if self.validated && self.unchecked < RETRIEVE_LEASE_CHECK_BYTES {
            let Some(lease) = lease_of(ctx) else {
                return ctx_err(ctx);
            };
            lease.context_error(ctx)?;
            if !lease.valid_at(SystemTime::now()) {
                return Err(lease.deny());
            }
            return Ok(());
        }
        check_read_lease(ctx)?;
        self.validated = true;
        self.unchecked = 0;
        Ok(())
    }
    fn read(&mut self, ctx: &Ctx, n: i64) -> Result<(), ServerError> {
        self.check(ctx, 0)?;
        self.check(ctx, n)
    }
    fn pass(&mut self, ctx: &Ctx, size: i64) -> Result<(), ServerError> {
        let mut offset = RETRIEVE_BUFFER_BYTES;
        while offset < size {
            self.read(ctx, (size - offset).min(RETRIEVE_BUFFER_BYTES))?;
            offset += RETRIEVE_BUFFER_BYTES;
        }
        Ok(())
    }
}
struct PreparedRetrieve {
    data: Vec<u8>,
    parsed: File,
    reference: InstanceRef,
    size: i64,
    gate: LeaseGate,
}
struct GatedReader<'a> {
    data: &'a [u8],
    position: usize,
    gate: &'a mut LeaseGate,
    ctx: &'a Ctx,
}
impl Read for GatedReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.gate.check(self.ctx, 0).map_err(server_io_error)?;
        let n = buf.len().min(self.data.len() - self.position);
        buf[..n].copy_from_slice(&self.data[self.position..self.position + n]);
        self.position += n;
        self.gate
            .check(self.ctx, n as i64)
            .map_err(server_io_error)?;
        Ok(n)
    }
}
fn parse_read_refusal(ctx: &Ctx, err: ServerError) -> ServerError {
    match ctx_err(ctx) {
        Err(e) => e,
        Ok(()) => {
            let _ = err;
            ServerError::Backend
        }
    }
}
fn classify_read_error(ctx: &Ctx, err: &object::Error) -> ServerError {
    if let Err(e) = ctx_err(ctx) {
        return e;
    }
    if err.is(Kind::MaxTotalBytes) {
        return ServerError::ResourceLimit;
    }
    let text = err.to_string();
    for limit in [
        "dicom: maximum sequence nesting depth exceeded",
        "dicom: maximum element value size exceeded",
        "dicom: maximum element count exceeded",
        "dicom: maximum fragment count exceeded",
        "dicom: maximum Pixel Data size exceeded",
        "dicom: maximum total bytes exceeded",
    ] {
        if text.contains(limit) {
            return ServerError::ResourceLimit;
        }
    }
    ServerError::Backend
}
fn classify_transcode_error(ctx: &Ctx, err: &tc::Error) -> ServerError {
    if let Err(e) = ctx_err(ctx) {
        return e;
    }
    if tc::is_limit_error(err) {
        return ServerError::ResourceLimit;
    }
    for kind in [
        TcKind::Unsupported,
        TcKind::LossyDisallowed,
        TcKind::EncoderUnavailable,
        TcKind::CodecRegistryNil,
        TcKind::UnsupportedEncoderMetadata,
        TcKind::Pixel(PixelErrorKind::CodecNotFound),
        TcKind::Pixel(PixelErrorKind::PixelDataNotFound),
        TcKind::Pixel(PixelErrorKind::JpipRetrievalRequired),
        TcKind::Pixel(PixelErrorKind::MediaPayloadNotRenderable),
    ] {
        if err.is(kind) {
            return ServerError::Unsupported;
        }
    }
    ServerError::Backend
}
fn frame_media_for_syntax(uid: &str) -> &'static str {
    [
        "application/octet-stream",
        "application/x-deflate",
        "image/jpeg",
        "image/dicom-rle",
        "image/jls",
        "image/jp2",
        "image/jpx",
        "image/jphc",
        "image/jxl",
        "video/mpeg",
        "video/mp4",
        "video/h265",
    ]
    .into_iter()
    .find(|media| frame_media_matches_syntax(media, uid))
    .unwrap_or("")
}
fn syntax_implies_lossy(uid: &str) -> bool {
    tc::detect_data_set_lossy_state(&Object::new(), uid).lossy
}
fn retrieve_source_lossy(source: &File) -> bool {
    tc::detect_lossy_state(source).lossy || syntax_implies_lossy(source.transfer_syntax.uid)
}
fn full_syntax(uid: &str) -> Option<Syntax> {
    transfer::default_registry().get(object::transfer::normalize_uid(uid))
}
pub(crate) struct PreparedNegotiation {
    state: Arc<PolicyState>,
    accept: Vec<MediaPreference>,
    frames: bool,
    multipart: bool,
    is_uri: bool,
}
#[derive(Clone, Default)]
pub struct PreparedSource(pub(crate) Option<Arc<PreparedNegotiation>>);
impl std::fmt::Debug for PreparedSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() {
            "PreparedSource(set)"
        } else {
            "PreparedSource(none)"
        })
    }
}
impl PartialEq for PreparedSource {
    fn eq(&self, _other: &PreparedSource) -> bool {
        true
    }
}
pub trait SourceReader: Read + Seek {
    fn close(&mut self) -> Result<(), ServerError> {
        Ok(())
    }
}
impl SourceReader for io::Cursor<Vec<u8>> {}
pub type SourceOpen<'a> = dyn FnMut() -> Result<Option<Box<dyn SourceReader>>, ServerError> + 'a;
impl PreparedNegotiation {
    fn target_for(&self, stored: &Syntax, lossy: bool) -> Result<Syntax, ServerError> {
        self.state.target_for(
            stored,
            lossy,
            &self.accept,
            self.frames,
            self.multipart,
            self.is_uri,
        )
    }
    fn may_convert(&self, stored: &Syntax, lossy: bool) -> Result<bool, ServerError> {
        let Ok(target) = self.target_for(stored, lossy) else {
            return Err(ServerError::Unsupported);
        };
        if target.uid != stored.uid {
            return Ok(true);
        }
        if !lossy
            && let Ok(target) = self.target_for(stored, true)
            && target.uid != stored.uid
        {
            return Ok(true);
        }
        Ok(false)
    }
    fn converted_bound(
        &self,
        ctx: &Ctx,
        data: &[u8],
        stored: &Syntax,
        stored_size: i64,
    ) -> Result<Option<i64>, ServerError> {
        let l = &self.state.policy.options.limits;
        let opts = ReadFileOptions {
            max_element_bytes: stored_size,
            max_total_bytes: stored_size,
            max_elements: l.max_elements,
            max_sequence_depth: l.max_depth,
            ..ReadFileOptions::default()
        };
        let (file, stop) = read_prefix(ctx, data, &opts)?;
        if file.transfer_syntax.uid != stored.uid {
            return Err(ServerError::Backend);
        }
        let lossy = retrieve_source_lossy(&file);
        if !self.may_convert(stored, lossy)? {
            return Ok(None);
        }
        if let Ok(target) = self.target_for(stored, lossy)
            && target.uid != stored.uid
        {
            let delivered =
                self.admit_conversion(ctx, data, &file, &opts, stored, target, lossy)?;
            if delivered.uid == stored.uid {
                return Ok(None);
            }
        }
        let mut total = stored_size;
        if stored.deflated {
            let header = object::read_part10_header_mem(
                &MemFile::new(Rc::from(data)),
                &ReadFileOptions {
                    max_total_bytes: stored_size,
                    ..ReadFileOptions::default()
                },
            )
            .map_err(|e| classify_read_error(ctx, &e))?;
            let offset = (header.data_set_offset as usize).min(data.len());
            let (decoded, err, _) = object::inflate::inflate_raw(&data[offset..]);
            let inflated = if decoded.len() as i64 > l.max_output_bytes {
                l.max_output_bytes + 1
            } else if !err.is(Kind::Eof) {
                return Err(ServerError::Unsupported);
            } else {
                decoded.len() as i64
            };
            total = header.data_set_offset + inflated;
        }
        let (mut pixel_stored, mut pixel_out) = (0i64, 0i64);
        if let Some((header, offset)) = stop
            && is_pixel_data_tag(header.tag)
        {
            let header_bytes = if stored.explicit_vr { 12 } else { 8 };
            if !header.is_undefined() {
                pixel_stored = i64::from(header.length);
            } else if stored.encapsulated && !stored.deflated {
                let end = encapsulated_pixel_end(
                    data,
                    offset + header_bytes,
                    stored_size,
                    l.max_fragments,
                    stored.big_endian,
                )?;
                pixel_stored = end - offset - header_bytes;
            } else {
                return Err(ServerError::Backend);
            }
            pixel_out = pixel_stored + 1;
            if stored.encapsulated {
                pixel_out = decoded_pixel_bytes(&file.dataset)?;
            }
        }
        let non_pixel = total - pixel_stored;
        if non_pixel < 0 || pixel_out < 0 {
            return Err(ServerError::Backend);
        }
        if non_pixel > (i64::MAX - CONVERTED_OBJECT_SLACK - pixel_out) / 3 {
            return Err(ServerError::ResourceLimit);
        }
        Ok(Some(3 * non_pixel + pixel_out + CONVERTED_OBJECT_SLACK))
    }
    #[allow(clippy::too_many_arguments)]
    fn admit_conversion(
        &self,
        ctx: &Ctx,
        data: &[u8],
        file: &File,
        opts: &ReadFileOptions,
        stored: &Syntax,
        target: Syntax,
        lossy: bool,
    ) -> Result<Syntax, ServerError> {
        let whole;
        let mut file = file;
        if stored.encapsulated && !stored.deflated {
            let maximum = self.state.policy.options.limits.max_input_bytes;
            let opts = ReadFileOptions {
                max_pixel_data_bytes: maximum,
                max_total_bytes: maximum,
                ..*opts
            };
            let defer = Deferral {
                defer_pixel_data: true,
                inline_value_bytes_threshold: 64 << 10,
                defer_nested_values: true,
                ..Deferral::default()
            };
            let deferred = object::deferred::read_file_deferred(data, true, opts, &defer)
                .map_err(|(_, e)| classify_read_error(ctx, &e))?;
            if deferred.transfer_syntax.uid != stored.uid {
                return Err(ServerError::Backend);
            }
            if let Ok(f) = object::read_file_with_options(data, ReadFileOptions::default()) {
                whole = f;
                file = &whole;
            }
        }
        self.state.admit_conversion(
            ctx,
            file,
            stored,
            target,
            lossy,
            &self.accept,
            self.frames,
            self.multipart,
            self.is_uri,
        )
    }
}
fn read_prefix(
    ctx: &Ctx,
    data: &[u8],
    opts: &ReadFileOptions,
) -> Result<(File, Option<(ElementHeader, i64)>), ServerError> {
    let bytes: Rc<[u8]> = Rc::from(data);
    let header = object::read_part10_header_mem(&MemFile::new(bytes.clone()), opts)
        .map_err(|e| classify_read_error(ctx, &e))?;
    let syntax = header.transfer_syntax;
    let offset = (header.data_set_offset as usize).min(data.len());
    let data_set: Rc<[u8]>;
    let mut inflate_error = None;
    if syntax.deflated {
        let (decoded, err, _) = object::inflate::inflate_raw(&data[offset..]);
        data_set = Rc::from(decoded);
        inflate_error = Some(err);
    } else {
        data_set = Rc::from(&data[offset..]);
    }
    let plain = object::Syntax {
        deflated: false,
        ..syntax
    };
    let reader_opts = ReaderOptions {
        max_element_bytes: opts.max_element_bytes,
        max_total_bytes: opts.max_total_bytes,
        max_elements: opts.max_elements,
        max_sequence_depth: opts.max_sequence_depth,
        max_fragments: 200_000,
        base_offset: header.data_set_offset,
        ..ReaderOptions::default()
    };
    let threshold = Tag::new(0x7FE0, 0x0008);
    let select = Box::new(
        move |path: &[object::error::PathStep], h: &ElementHeader, _offset: i64| {
            if path.len() == 1 && h.tag >= threshold {
                return Ok(Disposition::Stop);
            }
            Ok(Disposition::Materialize)
        },
    );
    let source: Box<dyn ByteRead> = match inflate_error.clone() {
        Some(err) => Box::new(object::inflate::Inflated::new(data_set.to_vec(), err)),
        None => Box::new(MemFile::new(data_set.clone())),
    };
    let sized = if syntax.deflated {
        None
    } else {
        Some((data_set.len() as i64, 0))
    };
    let mut reader = SelectiveReader::new(source, sized, plain, reader_opts, select, 1_000_000)
        .map_err(|e| classify_read_error(ctx, &e))?;
    let mut stop = None;
    loop {
        match reader.next() {
            Ok(SelectiveToken::Stop { header, offset }) => {
                stop = Some((header, offset));
                break;
            }
            Ok(_) => {}
            Err(e) if e.is(Kind::Eof) => break,
            Err(e) => return Err(classify_read_error(ctx, &e)),
        }
    }
    let end = match stop {
        Some((_, at)) => ((at - header.data_set_offset).max(0) as usize).min(data_set.len()),
        None => data_set.len(),
    };
    let prefix: Rc<[u8]> = Rc::from(&data_set[..end]);
    let dataset = object::read_data_set_with_options(Box::new(MemFile::new(prefix)), plain, *opts)
        .map_err(|e| classify_read_error(ctx, &e))?;
    Ok((
        File {
            preamble: header.preamble,
            meta: header.meta,
            dataset,
            transfer_syntax: syntax,
            partial_read: None,
            incomplete_pixel_data: None,
        },
        stop,
    ))
}
fn is_pixel_data_tag(tag: Tag) -> bool {
    tag == Tag::PIXEL_DATA || tag == Tag::new(0x7FE0, 0x0008) || tag == Tag::new(0x7FE0, 0x0009)
}
fn encapsulated_pixel_end(
    data: &[u8],
    start: i64,
    stored_size: i64,
    max_fragments: i64,
    big_endian: bool,
) -> Result<i64, ServerError> {
    let mut position = start;
    for _ in 0..max_fragments.saturating_add(2) {
        if position < 0 || position + 8 > data.len() as i64 {
            return Err(ServerError::Backend);
        }
        let item = &data[position as usize..position as usize + 8];
        let u16_at = |i: usize| {
            let b = [item[i], item[i + 1]];
            if big_endian {
                u16::from_be_bytes(b)
            } else {
                u16::from_le_bytes(b)
            }
        };
        let raw = [item[4], item[5], item[6], item[7]];
        let length = if big_endian {
            u32::from_be_bytes(raw)
        } else {
            u32::from_le_bytes(raw)
        };
        let (group, element) = (u16_at(0), u16_at(2));
        position += 8;
        if group == 0xFFFE && element == 0xE0DD {
            return Ok(position);
        }
        if group == 0xFFFE && element == 0xE000 && length != object::UNDEFINED_LENGTH {
            position += i64::from(length);
            if position > stored_size {
                return Err(ServerError::Backend);
            }
        } else {
            return Err(ServerError::Backend);
        }
    }
    Err(ServerError::ResourceLimit)
}
fn decoded_pixel_bytes(dataset: &Object) -> Result<i64, ServerError> {
    let Ok(metadata) = extract_metadata(dataset) else {
        return Err(ServerError::Unsupported);
    };
    let frames = metadata.number_of_frames.max(1);
    if metadata.bits_allocated < 8 {
        let total = total_size(&metadata);
        if total <= 0 {
            return Err(ServerError::ResourceLimit);
        }
        return Ok(total + 1);
    }
    let frame = i64::from(metadata.rows)
        * i64::from(metadata.columns)
        * i64::from(metadata.samples_per_pixel)
        * ((i64::from(metadata.bits_allocated) + 7) / 8);
    if frame <= 0 {
        return Err(ServerError::Unsupported);
    }
    if frames > (i64::MAX - 1) / frame {
        return Err(ServerError::ResourceLimit);
    }
    Ok(frame * frames + 1)
}
impl RetrieveRequest {
    pub fn accepts_source(&self, transfer_syntax_uid: &str) -> bool {
        let Some(n) = &self.prepared.0 else {
            return self.accepts_original(transfer_syntax_uid);
        };
        let Some(stored) = full_syntax(transfer_syntax_uid) else {
            return false;
        };
        n.target_for(&stored, syntax_implies_lossy(&stored.uid))
            .is_ok()
    }
    pub fn source_output_bound(
        &self,
        ctx: &Ctx,
        transfer_syntax_uid: &str,
        stored_size: i64,
        open: Option<&mut SourceOpen<'_>>,
    ) -> Result<i64, ServerError> {
        if stored_size < 1 {
            return Err(ServerError::Backend);
        }
        let Some(n) = &self.prepared.0 else {
            if !self.accepts_original(transfer_syntax_uid) {
                return Err(ServerError::Unsupported);
            }
            return Ok(stored_size);
        };
        let limits = n.state.policy.options.limits;
        let Some(stored) = full_syntax(transfer_syntax_uid) else {
            return Err(ServerError::Unsupported);
        };
        if stored_size > limits.max_input_bytes {
            return Err(ServerError::ResourceLimit);
        }
        if !n.may_convert(&stored, syntax_implies_lossy(&stored.uid))? {
            return Ok(stored_size);
        }
        let Some(open) = open else {
            return Err(ServerError::Backend);
        };
        check_read_lease(ctx)?;
        let Some(mut source) = open()? else {
            return Err(ServerError::Backend);
        };
        let mut data = Vec::new();
        let read = ctx_err(ctx).and_then(|()| {
            source
                .seek(SeekFrom::Start(0))
                .and_then(|_| source.read_to_end(&mut data))
                .map_err(|_| ServerError::Backend)
        });
        let bound = read.and_then(|_| n.converted_bound(ctx, &data, &stored, stored_size));
        let result = (|| {
            let bound = match bound {
                Ok(b) => b,
                Err(e) => {
                    ctx_err(ctx)?;
                    return Err(e);
                }
            };
            check_read_lease(ctx)?;
            let Some(bound) = bound else {
                return Ok(stored_size);
            };
            if bound > limits.max_output_bytes {
                return Err(ServerError::ResourceLimit);
            }
            Ok(bound.max(stored_size))
        })();
        let _ = source.close();
        result
    }
}
struct JobDirectory {
    spool: String,
    path: Option<PathBuf>,
}
impl JobDirectory {
    fn ensure(&mut self) -> Result<PathBuf, ServerError> {
        if let Some(path) = &self.path {
            return Ok(path.clone());
        }
        let base = if self.spool.is_empty() {
            std::env::temp_dir()
        } else {
            PathBuf::from(&self.spool)
        };
        for _ in 0..10_000 {
            let n = uuid::Uuid::new_v4().as_u128() as u32;
            let path = base.join(format!(".dicomweb-retrieve-{n}"));
            #[allow(unused_mut)]
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    activate_retrieve_directory(&path);
                    self.path = Some(path.clone());
                    return Ok(path);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(ServerError::Backend),
            }
        }
        Err(ServerError::Backend)
    }
    fn file(
        &mut self,
        name: &str,
        write: impl FnOnce(&mut fs::File) -> Result<(), ServerError>,
    ) -> Result<PathBuf, ServerError> {
        let path = self.ensure()?.join(name);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut spool = options.open(&path).map_err(|_| ServerError::Backend)?;
        write(&mut spool)?;
        spool.flush().map_err(|_| ServerError::Backend)?;
        Ok(path)
    }
    fn remove(&mut self) -> bool {
        let Some(path) = self.path.take() else {
            return true;
        };
        let removed = match fs::remove_dir_all(&path) {
            Ok(()) => true,
            Err(e) => e.kind() == io::ErrorKind::NotFound,
        };
        deactivate_retrieve_directory(&path);
        removed
    }
}
impl Drop for JobDirectory {
    fn drop(&mut self) {
        self.remove();
    }
}
struct SlotGuard<'a>(&'a Slots);
impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}
type PreparedYield<'a> = dyn FnMut(&mut PreparedRetrieve, &Ctx) -> Result<(), ServerError> + 'a;
impl Inner {
    #[allow(clippy::too_many_arguments)]
    fn prepare_retrieve_part(
        &self,
        state: &PolicyState,
        ctx: &Ctx,
        request: &RetrieveRequest,
        part: &RetrievePart,
        reader: &mut dyn PartReader,
        frames: bool,
        uri: bool,
        yield_prepared: &mut PreparedYield<'_>,
    ) -> Result<(), ServerError> {
        check_read_lease(ctx)?;
        let l = state.policy.options.limits;
        if part.size < -1 || part.size > l.max_input_bytes {
            return Err(ServerError::ResourceLimit);
        }
        if normalized_dicom_part_content_type(&part.content_type, &part.transfer_syntax_uid)
            .is_err()
        {
            return Err(ServerError::Backend);
        }
        let mut declared = part.transfer_syntax_uid.clone();
        if !part.content_type.is_empty() {
            let media = parse_media_type(&part.content_type).map_err(|_| ServerError::Backend)?;
            if media.params.iter().any(|(k, _)| k != "transfer-syntax") {
                return Err(ServerError::Backend);
            }
            let uid = media.param("transfer-syntax");
            if !uid.is_empty() {
                if !declared.is_empty() && declared != uid {
                    return Err(ServerError::Backend);
                }
                declared = uid.to_string();
            }
        }
        let mut reference = part.reference.clone();
        if request.level == RetrieveLevel::Instance && reference == InstanceRef::default() {
            reference = InstanceRef {
                study_instance_uid: request.study_instance_uid.clone(),
                series_instance_uid: request.series_instance_uid.clone(),
                sop_instance_uid: request.sop_instance_uid.clone(),
            };
        }
        if !retrieve_ref_matches(request, &reference) {
            return Err(ServerError::Backend);
        }
        for uid in [
            &reference.study_instance_uid,
            &reference.series_instance_uid,
            &reference.sop_instance_uid,
        ] {
            if self.validate_uid(uid).is_err() {
                return Err(ServerError::Backend);
            }
        }
        loop {
            if state
                .sem
                .acquire_until(Instant::now() + Duration::from_millis(50), ctx)?
            {
                break;
            }
        }
        let _slot = SlotGuard(&state.sem);
        check_read_lease(ctx)?;
        let mut job = JobDirectory {
            spool: self.options.spool_directory.clone(),
            path: None,
        };
        let result = self.prepare_staged(
            state,
            ctx,
            request,
            part,
            reader,
            &reference,
            &declared,
            frames,
            uri,
            &mut job,
            yield_prepared,
        );
        if !job.remove() && result.is_ok() {
            return Err(ServerError::Backend);
        }
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn prepare_staged(
        &self,
        state: &PolicyState,
        ctx: &Ctx,
        request: &RetrieveRequest,
        part: &RetrievePart,
        reader: &mut dyn PartReader,
        reference: &InstanceRef,
        declared: &str,
        frames: bool,
        uri: bool,
        job: &mut JobDirectory,
        yield_prepared: &mut PreparedYield<'_>,
    ) -> Result<(), ServerError> {
        let l = state.policy.options.limits;
        let mut input: Option<PathBuf> = None;
        let mut source = if part.size >= 0 && part.size <= RETRIEVE_MEMORY_STAGE_BYTES {
            let mut staged: Vec<u8> = Vec::with_capacity(part.size as usize);
            let (written, copied) = copy_bounded(
                ctx,
                &mut |d: &[u8]| {
                    staged.extend_from_slice(d);
                    Ok(d.len())
                },
                reader,
                part.size,
            );
            match copied {
                Err(e) if e.is(&ServerError::ResourceLimit) => return Err(ServerError::Backend),
                Err(e) => return Err(e),
                Ok(()) => {}
            }
            if written != part.size {
                return Err(ServerError::Backend);
            }
            state.read_prepared_source(ctx, staged, part.size, l.max_input_bytes, reference)?
        } else {
            let path = job.file("source.dcm", |spool| {
                let (written, copied) = copy_bounded(
                    ctx,
                    &mut |d: &[u8]| {
                        spool
                            .write_all(d)
                            .map(|()| d.len())
                            .map_err(|e| ServerError::Other(e.to_string()))
                    },
                    reader,
                    l.max_input_bytes,
                );
                copied?;
                if part.size >= 0 && written != part.size {
                    return Err(ServerError::Backend);
                }
                Ok(())
            })?;
            let prepared = state.read_prepared_file(ctx, &path, l.max_input_bytes, reference)?;
            input = Some(path);
            prepared
        };
        let stored_uid = source.parsed.transfer_syntax.uid;
        if !declared.is_empty() && declared != stored_uid {
            return Err(ServerError::Backend);
        }
        let Some(stored) = full_syntax(stored_uid) else {
            return Err(ServerError::Backend);
        };
        let multipart_required = request.level != RetrieveLevel::Instance || frames;
        let lossy = retrieve_source_lossy(&source.parsed);
        let mut target = state.target_for(
            &stored,
            lossy,
            &request.accept,
            frames,
            multipart_required,
            uri,
        )?;
        if target.uid != stored.uid {
            let whole;
            let mut file = &source.parsed;
            if stored.encapsulated
                && !stored.deflated
                && let Ok(f) =
                    object::read_file_with_options(&source.data, ReadFileOptions::default())
            {
                whole = f;
                file = &whole;
            }
            target = state.admit_conversion(
                ctx,
                file,
                &stored,
                target,
                lossy,
                &request.accept,
                frames,
                multipart_required,
                uri,
            )?;
        }
        if target.uid == stored.uid {
            return yield_prepared(&mut source, ctx);
        }
        let input = match input {
            Some(path) => path,
            None => job.file("source.dcm", |spool| {
                spool
                    .write_all(&source.data)
                    .map_err(|e| ServerError::Other(e.to_string()))
            })?,
        };
        let output = job.ensure()?.join("output.dcm");
        check_read_lease(ctx)?;
        let report = tc::transcode_path(&input, &output, &target.uid, &state.policy.options)
            .map_err(|e| classify_transcode_error(ctx, &e))?;
        if report.identity_changed || report.lossy || report.verification == Verification::Skipped {
            return Err(ServerError::Unsupported);
        }
        let mut prepared = state.read_prepared_file(ctx, &output, l.max_output_bytes, reference)?;
        if prepared.parsed.transfer_syntax.uid != target.uid {
            return Err(ServerError::Backend);
        }
        yield_prepared(&mut prepared, ctx)
    }
    fn prepared_retrieve(
        &self,
        ctx: &Ctx,
        request: &RetrieveRequest,
        frames: bool,
        uri: bool,
        yield_prepared: &mut PreparedYield<'_>,
    ) -> Result<(), ServerError> {
        let multipart = request.level != RetrieveLevel::Instance || frames;
        if !prepared_preferences_supported(&request.accept, frames, multipart) {
            return Err(ServerError::Unsupported);
        }
        let Some(backend) = self
            .options
            .backend
            .as_ref()
            .and_then(|b| b.retrieve_backend())
        else {
            return Err(ServerError::status(
                501,
                "not_implemented",
                ServerError::Unsupported,
            ));
        };
        let Some(state) = self.retrieve.clone() else {
            return Err(ServerError::Backend);
        };
        let mut source_request = request.clone();
        source_request.accept = vec![MediaPreference {
            media_type: "application/dicom".into(),
            transfer_syntax_uid: "*".into(),
            quality: 1.0,
            multipart: request.level != RetrieveLevel::Instance,
        }];
        source_request.prepared = PreparedSource(Some(Arc::new(PreparedNegotiation {
            state: state.clone(),
            accept: request.accept.clone(),
            frames,
            multipart,
            is_uri: uri,
        })));
        let mut count = 0i64;
        let mut failure: Option<ServerError> = None;
        let result = {
            let mut yield_part = |mut part: RetrievePart| -> Result<(), ServerError> {
                let close = |part: &mut RetrievePart| {
                    if let Some(mut reader) = part.reader.take() {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            reader.close()
                        }));
                    }
                };
                if let Some(e) = &failure {
                    close(&mut part);
                    return Err(e.clone());
                }
                if count >= self.limits.max_results
                    || (request.level == RetrieveLevel::Instance && count > 0)
                {
                    close(&mut part);
                    failure = Some(ServerError::Backend);
                    return Err(ServerError::Backend);
                }
                let Some(mut reader) = part.reader.take() else {
                    failure = Some(ServerError::Backend);
                    return Err(ServerError::Backend);
                };
                let prepared = with_reader_closed(reader.as_mut(), |reader| {
                    self.prepare_retrieve_part(
                        &state,
                        ctx,
                        request,
                        &part,
                        reader,
                        frames,
                        uri,
                        yield_prepared,
                    )
                });
                match prepared {
                    Ok(()) => {
                        count += 1;
                        Ok(())
                    }
                    Err(e) => {
                        failure = Some(e.clone());
                        Err(e)
                    }
                }
            };
            check_read_lease(ctx)
                .and_then(|()| recover(|| backend.retrieve(ctx, &source_request, &mut yield_part)))
        };
        if let Some(e) = failure {
            return Err(e);
        }
        if let Err(e) = result {
            return Err(classify_backend_error(e));
        }
        if count == 0 {
            return Err(ServerError::NotFound);
        }
        Ok(())
    }
    pub(crate) fn serve_prepared_retrieve(
        &self,
        w: &mut ServerWriter<'_>,
        r: &mut Request,
        request: RetrieveRequest,
        uri: bool,
    ) -> Result<usize, (usize, ServerError)> {
        let ctx = r.context().clone();
        let instance = request.level == RetrieveLevel::Instance;
        let mut writer: Option<MultipartWriter> = None;
        let mut multipart_response = !instance;
        let mut staged = if instance {
            Some(
                ResponseSpool::with_prefix(
                    &self.options.spool_directory,
                    ".dicomweb-retrieve-response-",
                    self.limits.max_response_bytes,
                )
                .map_err(|e| (0, e))?,
            )
        } else {
            None
        };
        let mut content_type = String::new();
        let mut content_location = String::new();
        let mut count = 0usize;
        let remaining = w.remaining;
        let result = {
            let mut yield_prepared = |p: &mut PreparedRetrieve, ctx: &Ctx| {
                let syntax = p.parsed.transfer_syntax.uid;
                let preference = matching_media_preference(
                    &request.accept,
                    "application/dicom",
                    syntax,
                    multipart_response,
                )
                .ok_or(ServerError::Unsupported)?;
                if count == 0 && instance {
                    multipart_response = preference.multipart && !uri;
                }
                let location = self
                    .instance_location(&p.reference)
                    .map_err(|_| ServerError::Backend)?;
                let media = if uri {
                    "application/dicom".to_string()
                } else {
                    media_type_with_transfer_syntax("application/dicom", syntax)
                };
                if multipart_response && writer.is_none() {
                    let mw = MultipartWriter::new();
                    content_type = format!(
                        "multipart/related; type=\"application/dicom\"; boundary={}",
                        mw.boundary()
                    );
                    if staged.is_none() {
                        w.header().set("Content-Type", &content_type);
                    }
                    writer = Some(mw);
                }
                let mut write = |d: &[u8]| match staged.as_mut() {
                    Some(spool) => spool.write(d),
                    None => w.write(d),
                };
                if multipart_response {
                    if let Some(mw) = writer.as_mut() {
                        mw.create_part(&header_with(&media, &location), &mut write)?;
                    }
                } else {
                    content_type = media;
                    content_location = location;
                    if p.size > remaining {
                        return Err(ServerError::ResourceLimit);
                    }
                }
                let maximum = self.limits.max_part_bytes.min(p.size);
                let mut source = GatedReader {
                    data: &p.data,
                    position: 0,
                    gate: &mut p.gate,
                    ctx,
                };
                copy_bounded(ctx, &mut write, &mut source, maximum).1?;
                count += 1;
                Ok(())
            };
            self.prepared_retrieve(&ctx, &request, false, uri, &mut yield_prepared)
        };
        result.map_err(|e| (count, e))?;
        if let Some(mw) = writer.as_mut() {
            let closed = match staged.as_mut() {
                Some(spool) => mw.close(&mut |d: &[u8]| spool.write(d)),
                None => mw.close(&mut |d: &[u8]| w.write(d)),
            };
            closed.map_err(|e| (count, e))?;
        }
        if let Some(mut spool) = staged {
            spool.rewind().map_err(|e| (count, e))?;
            w.header().set("Content-Type", &content_type);
            if !content_location.is_empty() {
                w.header().set("Content-Location", &content_location);
            }
            w.header().set("Content-Length", &spool.size.to_string());
            let size = spool.size;
            let copied = match spool.reader() {
                Some(file) => copy_bounded(&ctx, &mut |d: &[u8]| w.write(d), file, size).1,
                None => Err(ServerError::Backend),
            };
            copied.map_err(|e| (count, e))?;
            spool.close().map_err(|e| (count, e))?;
        }
        Ok(count)
    }
    pub(crate) fn serve_prepared_frames(
        &self,
        w: &mut ServerWriter<'_>,
        r: &mut Request,
    ) -> Result<usize, (usize, ServerError)> {
        let fail = |e: ServerError| (0usize, e);
        let parts = resource_parts(&r.url.path);
        let reference = InstanceRef {
            study_instance_uid: parts[1].clone(),
            series_instance_uid: parts[3].clone(),
            sop_instance_uid: parts[5].clone(),
        };
        for uid in [
            &reference.study_instance_uid,
            &reference.series_instance_uid,
            &reference.sop_instance_uid,
        ] {
            self.validate_uid(uid).map_err(fail)?;
        }
        let frames = parse_frames(&parts[7], self.limits.max_frames).map_err(fail)?;
        let preferences =
            parse_media_preferences(&request_accept_header(r), "application/octet-stream");
        if !has_multipart_frame_preference(&preferences) {
            return Err(fail(ServerError::Unsupported));
        }
        let request = RetrieveRequest {
            level: RetrieveLevel::Instance,
            study_instance_uid: reference.study_instance_uid.clone(),
            series_instance_uid: reference.series_instance_uid.clone(),
            sop_instance_uid: reference.sop_instance_uid.clone(),
            accept: preferences,
            prepared: PreparedSource::default(),
        };
        let Some(state) = self.retrieve.clone() else {
            return Err(fail(ServerError::Backend));
        };
        let mut staged = ResponseSpool::with_prefix(
            &self.options.spool_directory,
            ".dicomweb-retrieve-response-",
            self.limits.max_response_bytes,
        )
        .map_err(fail)?;
        let ctx = r.context().clone();
        let mut writer: Option<MultipartWriter> = None;
        let mut content_type = String::new();
        let mut count = 0usize;
        let remaining = w.remaining;
        let result = {
            let mut yield_prepared = |p: &mut PreparedRetrieve, ctx: &Ctx| {
                let l = state.policy.options.limits;
                let options = FrameReaderOptions {
                    decoder: state.policy.options.decoder,
                    max_frames: l.max_frames,
                    max_fragments: l.max_fragments,
                    ..FrameReaderOptions::default()
                };
                let whole;
                let mut file = &p.parsed;
                if !p.parsed.transfer_syntax.deflated {
                    whole = object::read_file_with_options(&p.data, ReadFileOptions::default())
                        .map_err(|_| ServerError::Unsupported)?;
                    file = &whole;
                }
                let reader =
                    FrameReader::new(file, options).map_err(|_| ServerError::Unsupported)?;
                let mut total = 0i64;
                for n in &frames {
                    let length = reader
                        .raw_frame_length(n - 1)
                        .map_err(|_| ServerError::InvalidRequest)?;
                    if length < 0
                        || length > self.limits.max_part_bytes
                        || length > remaining - total
                    {
                        return Err(ServerError::ResourceLimit);
                    }
                    total += length;
                }
                let uid = reader.transfer_syntax().uid.to_string();
                let uid = uid.as_str();
                let media = frame_media_for_syntax(uid);
                if media.is_empty() {
                    return Err(ServerError::Unsupported);
                }
                let mut mw = MultipartWriter::new();
                content_type = format!(
                    "multipart/related; type=\"{media}\"; boundary={}",
                    mw.boundary()
                );
                let mut outcome = Ok(());
                for n in &frames {
                    let length = reader.raw_frame_length(n - 1).unwrap_or(-1);
                    if let Err(e) = check_read_lease(ctx) {
                        outcome = Err(e);
                        break;
                    }
                    let data = match reader.raw_frame(n - 1) {
                        Ok(d) => d,
                        Err(e) => {
                            outcome = Err(classify_transcode_error(ctx, &e));
                            break;
                        }
                    };
                    if data.len() as i64 != length {
                        outcome = Err(ServerError::Backend);
                        break;
                    }
                    let header = header_with(
                        &media_type_with_transfer_syntax(media, uid),
                        &self.frame_location(&reference, *n),
                    );
                    let written = mw
                        .create_part(&header, &mut |d: &[u8]| staged.write(d))
                        .and_then(|()| staged.write(&data).map(|_| ()));
                    if let Err(e) = written {
                        outcome = Err(e);
                        break;
                    }
                    count += 1;
                }
                writer = Some(mw);
                outcome
            };
            self.prepared_retrieve(&ctx, &request, true, false, &mut yield_prepared)
        };
        result.map_err(|e| (count, e))?;
        if let Some(mw) = writer.as_mut() {
            mw.close(&mut |d: &[u8]| staged.write(d))
                .map_err(|e| (count, e))?;
        }
        staged.rewind().map_err(|e| (count, e))?;
        w.header().set("Content-Type", &content_type);
        w.header().set("Content-Length", &staged.size.to_string());
        let size = staged.size;
        let copied = match staged.reader() {
            Some(file) => copy_bounded(&ctx, &mut |d: &[u8]| w.write(d), file, size).1,
            None => Err(ServerError::Backend),
        };
        copied.map_err(|e| (count, e))?;
        staged.close().map_err(|e| (count, e))?;
        Ok(count)
    }
}
