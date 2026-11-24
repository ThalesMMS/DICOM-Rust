use super::Metadata;
use super::encapsulated::{self, Format, Limits, Recovery, TableAttribute, TableValue};
use super::frame_reader::{
    ERR_PARTIAL_DECODE_UNSUPPORTED, ERR_REGION_UNSUPPORTED, FrameRegion, Part, PartCodec,
    PartialDecodeCapabilities, PartialDecodeRequest, builtin_caps, builtin_j2k_part,
    builtin_region, part_codec, reduced_region,
};
use super::memory_admission::MemoryAdmission;
use super::pixeldata::{
    CodecRecovery, DIAGNOSTIC_BASIC_OFFSET_TABLE_IGNORED, Frames, PixelData, PixelError,
    PixelErrorKind, extract_metadata, frame_size, normalized_photometric,
};
use super::transcode::{DecoderRegistry, Error as PartError, ErrorKind as PartErrorKind};
use crate::net::ul::{CancelToken, Ctx, Kind as CtxKind};
use crate::object::{Element, ElementHeader, Object, Tag, UNDEFINED_LENGTH, Value, Vr};
use crate::textfmt;
use crate::transfer;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
pub const ERR_INVALID_CODEC_ROUTER: &str = "dicom: invalid codec router";
pub const ERR_NO_CODEC_BACKEND: &str = "dicom: no codec backend accepts the frame";
pub const ERR_CODEC_BACKENDS_DISAGREE: &str =
    "dicom: codec backends decoded the frames of one object differently";
pub const DIAGNOSTIC_CODEC_BACKEND_FALLBACK: &str = "codec-backend-fallback";
pub const DECLINE_NOT_ACCEPTED: &str = "not-accepted";
pub const DECLINE_DECODE_FAILED: &str = "decode-failed";
pub const SHADOW_REASON_DECODE_ERROR: &str = "decode-error";
pub const SHADOW_REASON_GEOMETRY: &str = "geometry";
pub const SHADOW_REASON_PHOTOMETRIC: &str = "photometric";
pub const SHADOW_REASON_SAMPLES: &str = "samples";
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
const TAG_ENCAPSULATED_TOTAL_LENGTH: Tag = Tag::new(0x7FE0, 0x0003);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
#[derive(Debug, Clone, Default)]
pub struct DecodeContext {
    pub ctx: Ctx,
    pub recovery: CodecRecovery,
}
pub(crate) fn context_error(ctx: &Ctx) -> Option<PixelError> {
    Some(match ctx.err()? {
        CtxKind::DeadlineExceeded => {
            PixelError::custom(PixelErrorKind::Other, "context deadline exceeded")
        }
        _ => PixelError::custom(PixelErrorKind::Other, "context canceled"),
    })
}
fn is_context_termination(err: &PixelError) -> bool {
    let text = err.to_string();
    text.ends_with("context canceled") || text.ends_with("context deadline exceeded")
}
pub trait Codec: Send + Sync {
    fn decode(
        &self,
        cx: &DecodeContext,
        pixel: &PixelData<'_>,
        obj: &Object,
    ) -> Result<Frames, PixelError>;
    fn region_decoder(&self) -> Option<&dyn FrameRegionDecoder> {
        None
    }
    fn partial_decoder(&self) -> Option<&dyn PartialFrameDecoder> {
        None
    }
}
pub trait FrameRegionDecoder: Send + Sync {
    fn decode_frame_region(
        &self,
        cx: &DecodeContext,
        codestream: &[u8],
        metadata: &Metadata,
        region: FrameRegion,
    ) -> Result<Frames, PartError>;
}
pub trait PartialFrameDecoder: Send + Sync {
    fn partial_decode_capabilities(
        &self,
        codestream: &[u8],
        metadata: &Metadata,
    ) -> Result<PartialDecodeCapabilities, PartError>;
    fn decode_frame_partial(
        &self,
        cx: &DecodeContext,
        codestream: &[u8],
        metadata: &Metadata,
        request: &PartialDecodeRequest,
    ) -> Result<Frames, PartError>;
}
fn region_unsupported() -> PartError {
    PartError::new(PartErrorKind::RegionUnsupported, ERR_REGION_UNSUPPORTED)
}
fn partial_unsupported() -> PartError {
    PartError::new(
        PartErrorKind::PartialDecodeUnsupported,
        ERR_PARTIAL_DECODE_UNSUPPORTED,
    )
}
fn as_pixel_error(e: &PartError) -> PixelError {
    let kind = match e.kind {
        PartErrorKind::Pixel(kind) => kind,
        _ => PixelErrorKind::Other,
    };
    PixelError::custom(kind, e.to_string())
}
fn decode_registered(
    codec: &dyn Codec,
    cx: &DecodeContext,
    pixel: &PixelData<'_>,
    obj: &Object,
) -> Result<Frames, PixelError> {
    if let Some(err) = context_error(&cx.ctx) {
        return Err(err);
    }
    codec.decode(cx, pixel, obj)
}
#[derive(Debug, Clone)]
pub struct RegistryCodec {
    pub registry: DecoderRegistry,
    pub uid: String,
}
impl RegistryCodec {
    pub fn new(registry: DecoderRegistry, uid: &str) -> RegistryCodec {
        RegistryCodec {
            registry,
            uid: transfer::normalize_uid(uid).to_string(),
        }
    }
    fn part_kind(&self) -> PartCodec {
        if !self.registry.decodes(&self.uid) {
            return PartCodec::None;
        }
        part_codec(self.registry, &self.uid)
    }
}
impl FrameRegionDecoder for RegistryCodec {
    fn decode_frame_region(
        &self,
        _cx: &DecodeContext,
        codestream: &[u8],
        metadata: &Metadata,
        region: FrameRegion,
    ) -> Result<Frames, PartError> {
        match builtin_region(self.part_kind(), &self.uid, codestream, metadata, region)? {
            Part::Frames(frames) => Ok(frames),
            Part::Unsupported => Err(region_unsupported()),
        }
    }
}
impl PartialFrameDecoder for RegistryCodec {
    fn partial_decode_capabilities(
        &self,
        codestream: &[u8],
        metadata: &Metadata,
    ) -> Result<PartialDecodeCapabilities, PartError> {
        builtin_caps(self.part_kind(), &self.uid, codestream, metadata)
    }
    fn decode_frame_partial(
        &self,
        _cx: &DecodeContext,
        codestream: &[u8],
        metadata: &Metadata,
        request: &PartialDecodeRequest,
    ) -> Result<Frames, PartError> {
        let level = request.reduction_level;
        let layers = request.quality_layers;
        let part = match self.part_kind() {
            PartCodec::Jpeg2000 => {
                builtin_j2k_part(codestream, metadata, request.region, level, layers)?
            }
            kind @ PartCodec::JpegLs if level == 0 && layers == 0 => {
                builtin_region(kind, &self.uid, codestream, metadata, request.region)?
            }
            _ => Part::Unsupported,
        };
        match part {
            Part::Frames(frames) => Ok(frames),
            Part::Unsupported => Err(partial_unsupported()),
        }
    }
}
impl Codec for RegistryCodec {
    fn decode(
        &self,
        cx: &DecodeContext,
        pixel: &PixelData<'_>,
        obj: &Object,
    ) -> Result<Frames, PixelError> {
        match self.registry {
            DecoderRegistry::Custom(r) => match r.get_codec(&self.uid) {
                Some(codec) => codec.decode(cx, pixel, obj),
                None => RegistryCodec::new(r.base(), &self.uid).decode(cx, pixel, obj),
            },
            DecoderRegistry::BuiltinWithJpegXl => super::pixeldata::decode_with_builtin_codec(
                &self.uid,
                pixel,
                obj,
                &cx.recovery,
                true,
            ),
            _ => super::pixeldata::decode_with_builtin_codec(
                &self.uid,
                pixel,
                obj,
                &cx.recovery,
                false,
            ),
        }
    }
    fn region_decoder(&self) -> Option<&dyn FrameRegionDecoder> {
        (self.part_kind() != PartCodec::None).then_some(self as &dyn FrameRegionDecoder)
    }
    fn partial_decoder(&self) -> Option<&dyn PartialFrameDecoder> {
        matches!(
            self.part_kind(),
            PartCodec::Jpeg2000 | PartCodec::Jpeg2000Part2 | PartCodec::JpegLs
        )
        .then_some(self as &dyn PartialFrameDecoder)
    }
}
pub struct CodecRegistry {
    base: DecoderRegistry,
    codecs: RwLock<HashMap<String, Arc<dyn Codec>>>,
}
impl fmt::Debug for CodecRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut uids: Vec<String> = self
            .codecs
            .read()
            .map(|c| c.keys().cloned().collect())
            .unwrap_or_default();
        uids.sort();
        f.debug_struct("CodecRegistry")
            .field("base", &self.base)
            .field("codecs", &uids)
            .finish()
    }
}
impl PartialEq for CodecRegistry {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for CodecRegistry {}
impl CodecRegistry {
    pub fn new(base: DecoderRegistry) -> CodecRegistry {
        CodecRegistry {
            base,
            codecs: RwLock::new(HashMap::new()),
        }
    }
    pub fn base(&self) -> DecoderRegistry {
        self.base
    }
    pub fn register_codec(&self, uid: &str, codec: Arc<dyn Codec>) -> Result<(), PixelError> {
        let uid = transfer::normalize_uid(uid);
        if uid.is_empty() {
            return Err(PixelError::custom(
                PixelErrorKind::CodecNotFound,
                "dicom: no pixel data codec registered for transfer syntax: empty transfer syntax UID",
            ));
        }
        if let Ok(mut codecs) = self.codecs.write() {
            codecs.insert(uid.to_string(), codec);
        }
        Ok(())
    }
    pub fn get_codec(&self, uid: &str) -> Option<Arc<dyn Codec>> {
        let uid = transfer::normalize_uid(uid);
        self.codecs.read().ok()?.get(uid).cloned()
    }
    pub fn overrides(&self, uid: &str) -> bool {
        self.get_codec(uid).is_some()
    }
    pub fn into_decoder_registry(self) -> DecoderRegistry {
        DecoderRegistry::Custom(Box::leak(Box::new(self)))
    }
    pub fn decode_frames(
        &self,
        cx: &DecodeContext,
        uid: &str,
        pixel: &PixelData<'_>,
        obj: &Object,
    ) -> Result<Frames, PixelError> {
        match self.get_codec(uid) {
            Some(codec) => {
                if let Some(err) = context_error(&cx.ctx) {
                    return Err(err);
                }
                let frames =
                    super::pixeldata::decode_frames_with_codec(uid, pixel, obj, &mut || {
                        decode_registered(codec.as_ref(), cx, pixel, obj)
                    })?;
                if let Some(err) = context_error(&cx.ctx) {
                    return Err(err);
                }
                Ok(frames)
            }
            None => self
                .base
                .decode_frames_with_recovery(uid, pixel, obj, &cx.recovery),
        }
    }
}
pub type AcceptFn = Arc<dyn Fn(&[u8], &Metadata) -> bool + Send + Sync>;
#[derive(Clone)]
pub struct CodecBackend {
    pub name: String,
    pub codec: Arc<dyn Codec>,
    pub accept: Option<AcceptFn>,
}
impl CodecBackend {
    fn accepts(&self, codestream: &[u8], metadata: &Metadata) -> bool {
        self.accept.as_ref().is_none_or(|a| a(codestream, metadata))
    }
}
impl fmt::Debug for CodecBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodecBackend")
            .field("name", &self.name)
            .field("accept", &self.accept.is_some())
            .finish()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowOutcome {
    Matched,
    Mismatched,
    Failed,
}
impl ShadowOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ShadowOutcome::Matched => "matched",
            ShadowOutcome::Mismatched => "mismatched",
            ShadowOutcome::Failed => "failed",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowReport {
    pub transfer_syntax_uid: String,
    pub frame: i64,
    pub primary: String,
    pub shadow: String,
    pub outcome: ShadowOutcome,
    pub reason: &'static str,
    pub rows: usize,
    pub columns: usize,
    pub samples: i64,
    pub differing_samples: i64,
    pub max_absolute_difference: i64,
    pub first_difference: i64,
}
pub type ShadowReporter = Arc<dyn Fn(&ShadowReport) + Send + Sync>;
#[derive(Clone)]
pub struct ShadowComparison {
    pub backend: CodecBackend,
    pub sample_every: i64,
    pub max_absolute_error: i64,
    pub report: Option<ShadowReporter>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendDecline {
    pub backend: String,
    pub reason: &'static str,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameCodecDecision {
    pub transfer_syntax_uid: String,
    pub frame: i64,
    pub backend: String,
    pub declined: Vec<BackendDecline>,
    pub fallback: bool,
    pub partial: bool,
    pub admitted_bytes: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodecRouterStats {
    pub frames: i64,
    pub fallbacks: i64,
    pub backends: BTreeMap<String, i64>,
    pub compared: i64,
    pub matched: i64,
    pub mismatched: i64,
    pub failed: i64,
    pub sampled_out: i64,
}
pub(crate) type FallbackFn = Arc<dyn Fn(&str, &PixelError) -> bool + Send + Sync>;
pub(crate) type FrameBytesFn = Arc<dyn Fn(usize, &Metadata) -> i64 + Send + Sync>;
pub(crate) type ObserveFn = Arc<dyn Fn(&FrameCodecDecision) + Send + Sync>;
#[derive(Clone, Default)]
pub struct CodecRouterOptions {
    pub transfer_syntax_uid: String,
    pub backends: Vec<CodecBackend>,
    pub fallback: Option<FallbackFn>,
    pub shadow: Option<ShadowComparison>,
    pub admission: Option<MemoryAdmission>,
    pub frame_bytes: Option<FrameBytesFn>,
    pub workers: i64,
    pub observe: Option<ObserveFn>,
}
pub struct CodecRouter {
    uid: String,
    format: Option<Format>,
    backends: Vec<CodecBackend>,
    fallback: Option<FallbackFn>,
    shadow: Option<ShadowComparison>,
    admission: Option<MemoryAdmission>,
    frame_bytes: Option<FrameBytesFn>,
    workers: usize,
    observe: Option<ObserveFn>,
    shadow_seq: AtomicI64,
    stats: Mutex<CodecRouterStats>,
}
fn invalid(detail: impl fmt::Display) -> PixelError {
    PixelError::custom(
        PixelErrorKind::Other,
        format!("{ERR_INVALID_CODEC_ROUTER}: {detail}"),
    )
}
fn valid_backend(backend: &CodecBackend) -> Result<(), PixelError> {
    if backend.name.trim().is_empty() {
        return Err(invalid("a backend without a name"));
    }
    Ok(())
}
fn size_mismatch(detail: impl fmt::Display) -> PixelError {
    PixelError::custom(
        PixelErrorKind::PixelDataSizeMismatch,
        format!("dicom: pixel data size does not match metadata: {detail}"),
    )
}
fn stream_frame_format(uid: &str) -> Option<Format> {
    match uid {
        "1.2.840.10008.1.2.4.50"
        | "1.2.840.10008.1.2.4.51"
        | "1.2.840.10008.1.2.4.57"
        | "1.2.840.10008.1.2.4.70" => Some(Format::JPEG),
        "1.2.840.10008.1.2.4.80" | "1.2.840.10008.1.2.4.81" => Some(Format::JPEGLS),
        "1.2.840.10008.1.2.4.90"
        | "1.2.840.10008.1.2.4.91"
        | "1.2.840.10008.1.2.4.201"
        | "1.2.840.10008.1.2.4.202"
        | "1.2.840.10008.1.2.4.203" => Some(Format::JPEG2000),
        _ => None,
    }
}
fn table_attribute(obj: &Object, tag: Tag) -> Option<TableAttribute<'_>> {
    let e = obj.get(tag)?;
    Some(TableAttribute {
        vr_ov: e.vr() == Vr::OV,
        value: match &e.value {
            Value::Raw(raw) => TableValue::Raw(raw),
            _ => TableValue::Unsupported("core.Value"),
        },
    })
}
fn one_frame_elements(obj: &Object) -> Vec<Element> {
    obj.elements()
        .iter()
        .filter(|e| {
            !matches!(
                e.tag(),
                TAG_PIXEL_DATA
                    | TAG_EXTENDED_OFFSET_TABLE
                    | TAG_EXTENDED_OFFSET_TABLE_LENGTHS
                    | TAG_ENCAPSULATED_TOTAL_LENGTH
            )
        })
        .map(|e| {
            if e.tag() == TAG_NUMBER_OF_FRAMES {
                Element {
                    header: ElementHeader {
                        tag: TAG_NUMBER_OF_FRAMES,
                        vr: Vr::IS,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Strings(vec!["1".to_string()]),
                }
            } else {
                e.clone()
            }
        })
        .collect()
}
fn decode_with(
    codec: &dyn Codec,
    cx: &DecodeContext,
    codestream: &[u8],
    base: &[Element],
) -> Result<Frames, PixelError> {
    let fragments = vec![codestream.to_vec()];
    let mut elements = base.to_vec();
    elements.push(Element {
        header: ElementHeader {
            tag: TAG_PIXEL_DATA,
            vr: Vr::OB,
            length: UNDEFINED_LENGTH,
            length_set: true,
        },
        value: Value::Fragments {
            offset_table: Vec::new(),
            fragments: fragments.clone(),
        },
    });
    let proxy = Object::from_elements(elements);
    let pixel = PixelData::Encapsulated {
        offset_table: &[],
        fragments: &fragments,
    };
    let frames = decode_registered(codec, cx, &pixel, &proxy)?;
    if frames.data.len() != 1 {
        return Err(size_mismatch(format!(
            "a backend returned {} frames for one",
            frames.data.len()
        )));
    }
    Ok(frames)
}
fn add_diagnostic(frames: &mut Frames, code: &'static str) {
    if !frames.diagnostics.contains(&code) {
        frames.diagnostics.push(code);
    }
}
fn compare_samples(a: &[u8], b: &[u8], metadata: &Metadata) -> (i64, i64, i64, i64) {
    let width = match metadata.bits_allocated {
        16 => 2,
        32 => 4,
        _ => 1,
    };
    let signed = metadata.pixel_representation == 1;
    let value = |data: &[u8]| -> i64 {
        match *data {
            [b0, b1] => {
                let v = u16::from_le_bytes([b0, b1]);
                if signed {
                    i64::from(v as i16)
                } else {
                    i64::from(v)
                }
            }
            [b0, b1, b2, b3] => {
                let v = u32::from_le_bytes([b0, b1, b2, b3]);
                if signed {
                    i64::from(v as i32)
                } else {
                    i64::from(v)
                }
            }
            [b0] => {
                if signed && metadata.bits_allocated == 8 {
                    i64::from(b0 as i8)
                } else {
                    i64::from(b0)
                }
            }
            _ => 0,
        }
    };
    let (mut samples, mut differing, mut worst, mut first) = (0i64, 0i64, 0i64, -1i64);
    for (sa, sb) in a.chunks_exact(width).zip(b.chunks_exact(width)) {
        samples = samples.saturating_add(1);
        let d = value(sa).saturating_sub(value(sb)).saturating_abs();
        if d == 0 {
            continue;
        }
        if first < 0 {
            first = samples.saturating_sub(1);
        }
        differing = differing.saturating_add(1);
        worst = worst.max(d);
    }
    (samples, differing, worst, first)
}
impl CodecRouter {
    pub fn new(options: CodecRouterOptions) -> Result<CodecRouter, PixelError> {
        let uid = transfer::normalize_uid(&options.transfer_syntax_uid).to_string();
        if uid.is_empty() {
            return Err(invalid("no transfer syntax"));
        }
        if options.backends.is_empty() {
            return Err(invalid("no backends"));
        }
        let mut names = std::collections::HashSet::new();
        for backend in &options.backends {
            valid_backend(backend)?;
            if !names.insert(backend.name.clone()) {
                return Err(invalid(format!(
                    "two backends named {}",
                    textfmt::quote(&backend.name)
                )));
            }
        }
        if options.workers < 0 {
            return Err(invalid(format!("{} workers", options.workers)));
        }
        let mut shadow = options.shadow;
        if let Some(s) = &mut shadow {
            valid_backend(&s.backend)?;
            if s.sample_every < 0 || s.max_absolute_error < 0 {
                return Err(invalid(format!(
                    "shadow sampling {}, tolerance {}",
                    s.sample_every, s.max_absolute_error
                )));
            }
            s.sample_every = s.sample_every.max(1);
        }
        Ok(CodecRouter {
            format: stream_frame_format(&uid),
            uid,
            backends: options.backends,
            fallback: options.fallback,
            shadow,
            admission: options.admission,
            frame_bytes: options.frame_bytes,
            workers: options.workers.max(1) as usize,
            observe: options.observe,
            shadow_seq: AtomicI64::new(0),
            stats: Mutex::new(CodecRouterStats::default()),
        })
    }
    pub fn register(self: &Arc<Self>, registry: &CodecRegistry) -> Result<(), PixelError> {
        registry.register_codec(&self.uid, self.clone())
    }
    pub fn transfer_syntax_uid(&self) -> &str {
        &self.uid
    }
    pub fn stats(&self) -> CodecRouterStats {
        self.lock_stats().clone()
    }
    fn lock_stats(&self) -> std::sync::MutexGuard<'_, CodecRouterStats> {
        self.stats.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn decode_frames(
        &self,
        cx: &DecodeContext,
        pixel: &PixelData<'_>,
        obj: &Object,
    ) -> Result<Frames, PixelError> {
        let PixelData::Encapsulated {
            offset_table,
            fragments,
        } = *pixel
        else {
            return Err(PixelError::custom(
                PixelErrorKind::IncompatiblePixelData,
                "dicom: pixel data is incompatible with transfer syntax: a codec router decodes encapsulated Pixel Data",
            ));
        };
        let metadata = extract_metadata(obj)?;
        let count = metadata.number_of_frames;
        let (codestreams, recovered) = self.frames_of(cx, offset_table, fragments, obj, count)?;
        let mut frame_metadata = metadata.clone();
        frame_metadata.number_of_frames = 1;
        let base = one_frame_elements(obj);
        let results = self.for_each_frame(cx, count as usize, &|cx: &DecodeContext, i| {
            let Some(codestream) = codestreams.get(i) else {
                return Err(PixelError::custom(
                    PixelErrorKind::Other,
                    format!("frame {i} of {} codestreams", codestreams.len()),
                ));
            };
            let codestream = codestream.as_ref().map_err(Clone::clone)?;
            self.decode_frame(cx, i as i64, codestream, &frame_metadata, &base)
        })?;
        let Some(first) = results.first() else {
            return Err(PixelError::custom(
                PixelErrorKind::Other,
                "no frame decoded",
            ));
        };
        let mut out = Frames {
            rows: first.rows,
            columns: first.columns,
            data: Vec::with_capacity(results.len()),
            photometric_interpretation: first.photometric_interpretation.clone(),
            diagnostics: Vec::new(),
        };
        if recovered {
            add_diagnostic(&mut out, DIAGNOSTIC_BASIC_OFFSET_TABLE_IGNORED);
        }
        let photometric = normalized_photometric(&out.photometric_interpretation);
        for (i, frames) in results.iter().enumerate() {
            if frames.rows != out.rows || frames.columns != out.columns {
                return Err(size_mismatch(format!(
                    "frame {i} is {}x{}, frame 0 {}x{}",
                    frames.columns, frames.rows, out.columns, out.rows
                )));
            }
            if normalized_photometric(&frames.photometric_interpretation) != photometric {
                return Err(PixelError::custom(
                    PixelErrorKind::Other,
                    format!(
                        "{ERR_CODEC_BACKENDS_DISAGREE}: frame {i} decoded as {}, frame 0 as {}",
                        textfmt::quote(&frames.photometric_interpretation),
                        textfmt::quote(&out.photometric_interpretation)
                    ),
                ));
            }
        }
        for frames in results {
            for code in &frames.diagnostics {
                add_diagnostic(&mut out, code);
            }
            out.data.extend(frames.data.into_iter().take(1));
        }
        Ok(out)
    }
    fn for_each_frame(
        &self,
        cx: &DecodeContext,
        count: usize,
        decode: &(dyn Fn(&DecodeContext, usize) -> Result<Frames, PixelError> + Sync),
    ) -> Result<Vec<Frames>, PixelError> {
        let workers = self.workers.min(count);
        if workers <= 1 {
            return (0..count).map(|i| decode(cx, i)).collect();
        }
        let token = CancelToken::new();
        let inner = DecodeContext {
            ctx: cx.ctx.with_cancel(&token),
            recovery: cx.recovery,
        };
        let next = AtomicUsize::new(0);
        let failure: Mutex<Option<PixelError>> = Mutex::new(None);
        let results: Mutex<Vec<Option<Frames>>> = Mutex::new(vec![None; count]);
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let i = next.fetch_add(1, Ordering::SeqCst);
                        if i >= count || inner.ctx.err().is_some() {
                            return;
                        }
                        match decode(&inner, i) {
                            Ok(frames) => {
                                if let Ok(mut r) = results.lock()
                                    && let Some(slot) = r.get_mut(i)
                                {
                                    *slot = Some(frames);
                                }
                            }
                            Err(err) => {
                                if let Ok(mut f) = failure.lock()
                                    && f.is_none()
                                {
                                    *f = Some(err);
                                    token.cancel();
                                }
                                return;
                            }
                        }
                    }
                });
            }
        });
        if let Some(err) = failure.into_inner().unwrap_or_else(|e| e.into_inner()) {
            return Err(err);
        }
        if let Some(err) = context_error(&cx.ctx) {
            return Err(err);
        }
        let results = results.into_inner().unwrap_or_else(|e| e.into_inner());
        results
            .into_iter()
            .map(|r| r.ok_or_else(|| PixelError::custom(PixelErrorKind::Other, "context canceled")))
            .collect()
    }
    #[allow(clippy::type_complexity)]
    fn frames_of(
        &self,
        cx: &DecodeContext,
        offset_table: &[u8],
        fragments: &[Vec<u8>],
        obj: &Object,
        count: i64,
    ) -> Result<(Vec<Result<Vec<u8>, PixelError>>, bool), PixelError> {
        if count < 1 {
            return Err(PixelError::custom(
                PixelErrorKind::InvalidMetadata,
                format!("dicom: invalid pixel data metadata: {count} frames"),
            ));
        }
        if let Some(format) = self.format {
            if let Some(err) = context_error(&cx.ctx) {
                return Err(err);
            }
            let refs: Vec<&[u8]> = fragments.iter().map(Vec::as_slice).collect();
            let plan = encapsulated::from_fragments(
                offset_table,
                &refs,
                crate::pixel::encapsulated::EotAttributes {
                    extended: table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE),
                    lengths: table_attribute(obj, TAG_EXTENDED_OFFSET_TABLE_LENGTHS),
                },
                count,
                format,
                Limits::default(),
                Recovery {
                    ignore_inconsistent_basic_offset_table: cx
                        .recovery
                        .ignore_inconsistent_basic_offset_table,
                },
            )
            .map_err(|e| {
                if e.is(encapsulated::ErrorKind::FrameCount) {
                    PixelError::custom(
                        PixelErrorKind::PixelDataSizeMismatch,
                        format!("dicom: pixel data size does not match metadata\n{e}"),
                    )
                } else {
                    PixelError::custom(PixelErrorKind::Other, e.to_string())
                }
            })?;
            let recovered = plan.recovery().basic_offset_table_ignored;
            let frames = (0..count as usize)
                .map(|i| {
                    plan.frame(i)
                        .map(|v| v.data.into_owned())
                        .map_err(|e| PixelError::custom(PixelErrorKind::Other, e.to_string()))
                })
                .collect();
            return Ok((frames, recovered));
        }
        if fragments.len() as i64 == count {
            return Ok((fragments.iter().map(|f| Ok(f.clone())).collect(), false));
        }
        if count == 1 && !fragments.is_empty() {
            return Ok((vec![Ok(fragments.concat())], false));
        }
        Err(size_mismatch(format!(
            "{} Items for {count} frames",
            fragments.len()
        )))
    }
    fn estimate(&self, codestream_bytes: usize, metadata: &Metadata) -> i64 {
        match &self.frame_bytes {
            Some(f) => f(codestream_bytes, metadata).max(1),
            None => (codestream_bytes as i64)
                .saturating_add(frame_size(metadata))
                .max(1),
        }
    }
    fn may_fall_back(&self, backend: &str, err: &PixelError) -> bool {
        if is_context_termination(err) {
            return false;
        }
        self.fallback.as_ref().is_none_or(|f| f(backend, err))
    }
    fn shadowed(&self, codestream: &[u8], metadata: &Metadata) -> bool {
        let Some(shadow) = &self.shadow else {
            return false;
        };
        if !shadow.backend.accepts(codestream, metadata) {
            return false;
        }
        let seq = self.shadow_seq.fetch_add(1, Ordering::SeqCst);
        if seq.checked_rem(shadow.sample_every).unwrap_or(0) == 0 {
            return true;
        }
        let mut stats = self.lock_stats();
        stats.sampled_out = stats.sampled_out.saturating_add(1);
        false
    }
    fn decode_frame(
        &self,
        cx: &DecodeContext,
        index: i64,
        codestream: &[u8],
        metadata: &Metadata,
        base: &[Element],
    ) -> Result<Frames, PixelError> {
        let shadow = self.shadowed(codestream, metadata);
        let mut need = self.estimate(codestream.len(), metadata);
        if shadow {
            need = need.saturating_mul(2);
        }
        let (_grant, admitted) = match &self.admission {
            None => (None, 0),
            Some(admission) => {
                let grant = admission
                    .acquire(&cx.ctx, need)
                    .map_err(|e| PixelError::custom(PixelErrorKind::Other, e.to_string()))?;
                (Some(grant), need)
            }
        };
        let mut decision = FrameCodecDecision {
            transfer_syntax_uid: self.uid.clone(),
            frame: index,
            admitted_bytes: admitted,
            ..FrameCodecDecision::default()
        };
        let mut failure = None;
        for backend in &self.backends {
            if !backend.accepts(codestream, metadata) {
                decision.declined.push(BackendDecline {
                    backend: backend.name.clone(),
                    reason: DECLINE_NOT_ACCEPTED,
                });
                continue;
            }
            match decode_with(backend.codec.as_ref(), cx, codestream, base) {
                Ok(mut frames) => {
                    decision.backend = backend.name.clone();
                    if decision.fallback {
                        add_diagnostic(&mut frames, DIAGNOSTIC_CODEC_BACKEND_FALLBACK);
                    }
                    if shadow
                        && let Some(s) = &self.shadow
                        && backend.name != s.backend.name
                    {
                        self.compare_shadow(
                            cx,
                            index,
                            codestream,
                            metadata,
                            base,
                            &backend.name,
                            &frames,
                        );
                    }
                    self.decided(&decision);
                    return Ok(frames);
                }
                Err(err) => {
                    if let Some(ctx_err) = context_error(&cx.ctx) {
                        return Err(ctx_err);
                    }
                    if !self.may_fall_back(&backend.name, &err) {
                        return Err(err);
                    }
                    failure = Some(err);
                    decision.declined.push(BackendDecline {
                        backend: backend.name.clone(),
                        reason: DECLINE_DECODE_FAILED,
                    });
                    decision.fallback = true;
                }
            }
        }
        if let Some(err) = failure {
            return Err(err);
        }
        Err(PixelError::custom(
            PixelErrorKind::NoCodecBackend,
            format!("{ERR_NO_CODEC_BACKEND}: frame {index} of {}", self.uid),
        ))
    }
    fn decided(&self, decision: &FrameCodecDecision) {
        let mut stats = self.lock_stats();
        stats.frames = stats.frames.saturating_add(1);
        let backend = stats.backends.entry(decision.backend.clone()).or_insert(0);
        *backend = backend.saturating_add(1);
        if decision.fallback {
            stats.fallbacks = stats.fallbacks.saturating_add(1);
        }
        if let Some(observe) = &self.observe {
            observe(decision);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn compare_shadow(
        &self,
        cx: &DecodeContext,
        index: i64,
        codestream: &[u8],
        metadata: &Metadata,
        base: &[Element],
        primary: &str,
        got: &Frames,
    ) {
        let Some(shadow) = &self.shadow else {
            return;
        };
        let mut report = ShadowReport {
            transfer_syntax_uid: self.uid.clone(),
            frame: index,
            primary: primary.to_string(),
            shadow: shadow.backend.name.clone(),
            outcome: ShadowOutcome::Matched,
            reason: "",
            rows: got.rows,
            columns: got.columns,
            samples: 0,
            differing_samples: 0,
            max_absolute_difference: 0,
            first_difference: -1,
        };
        let want = decode_with(shadow.backend.codec.as_ref(), cx, codestream, base);
        if cx.ctx.err().is_some() {
            return;
        }
        match want {
            Err(_) => {
                report.outcome = ShadowOutcome::Failed;
                report.reason = SHADOW_REASON_DECODE_ERROR;
            }
            Ok(want)
                if want.rows != got.rows
                    || want.columns != got.columns
                    || want.data.first().map(Vec::len) != got.data.first().map(Vec::len) =>
            {
                report.outcome = ShadowOutcome::Mismatched;
                report.reason = SHADOW_REASON_GEOMETRY;
            }
            Ok(want)
                if normalized_photometric(&want.photometric_interpretation)
                    != normalized_photometric(&got.photometric_interpretation) =>
            {
                report.outcome = ShadowOutcome::Mismatched;
                report.reason = SHADOW_REASON_PHOTOMETRIC;
            }
            Ok(want) => {
                let (got_plane, want_plane) = (
                    got.data.first().map_or(&[][..], Vec::as_slice),
                    want.data.first().map_or(&[][..], Vec::as_slice),
                );
                let (samples, differing, worst, first) =
                    compare_samples(got_plane, want_plane, metadata);
                report.samples = samples;
                report.differing_samples = differing;
                report.max_absolute_difference = worst;
                report.first_difference = first;
                if worst > shadow.max_absolute_error {
                    report.outcome = ShadowOutcome::Mismatched;
                    report.reason = SHADOW_REASON_SAMPLES;
                }
            }
        }
        let mut stats = self.lock_stats();
        stats.compared = stats.compared.saturating_add(1);
        let count = match report.outcome {
            ShadowOutcome::Matched => &mut stats.matched,
            ShadowOutcome::Mismatched => &mut stats.mismatched,
            ShadowOutcome::Failed => &mut stats.failed,
        };
        *count = count.saturating_add(1);
        if let Some(f) = &shadow.report {
            f(&report);
        }
    }
}
impl CodecRouter {
    pub fn decode_frame_region(
        &self,
        cx: &DecodeContext,
        codestream: &[u8],
        metadata: &Metadata,
        region: FrameRegion,
    ) -> Result<Frames, PartError> {
        self.decode_part(
            cx,
            codestream,
            metadata,
            (region.dx(), region.dy()),
            region_unsupported,
            &|codec: &dyn Codec| {
                codec
                    .region_decoder()
                    .map(|d| d.decode_frame_region(cx, codestream, metadata, region))
            },
        )
    }
    pub fn partial_decode_capabilities(
        &self,
        codestream: &[u8],
        metadata: &Metadata,
    ) -> Result<PartialDecodeCapabilities, PartError> {
        for backend in &self.backends {
            if !backend.accepts(codestream, metadata) {
                continue;
            }
            if let Some(decoder) = backend.codec.partial_decoder() {
                return decoder.partial_decode_capabilities(codestream, metadata);
            }
            return Ok(PartialDecodeCapabilities {
                region: backend.codec.region_decoder().is_some(),
                ..PartialDecodeCapabilities::default()
            });
        }
        Ok(PartialDecodeCapabilities::default())
    }
    pub fn decode_frame_partial(
        &self,
        cx: &DecodeContext,
        codestream: &[u8],
        metadata: &Metadata,
        request: &PartialDecodeRequest,
    ) -> Result<Frames, PartError> {
        let size = reduced_region(request.region, request.reduction_level);
        self.decode_part(
            cx,
            codestream,
            metadata,
            (size.dx(), size.dy()),
            partial_unsupported,
            &|codec: &dyn Codec| {
                codec
                    .partial_decoder()
                    .map(|d| d.decode_frame_partial(cx, codestream, metadata, request))
            },
        )
    }
    #[allow(clippy::type_complexity)]
    fn decode_part(
        &self,
        cx: &DecodeContext,
        codestream: &[u8],
        metadata: &Metadata,
        size: (i64, i64),
        unsupported: fn() -> PartError,
        call: &dyn Fn(&dyn Codec) -> Option<Result<Frames, PartError>>,
    ) -> Result<Frames, PartError> {
        let mut part = metadata.clone();
        part.rows = size.1.clamp(0, 0xFFFF) as u16;
        part.columns = size.0.clamp(0, 0xFFFF) as u16;
        let need = self.estimate(codestream.len(), &part);
        let (_grant, admitted) = match &self.admission {
            None => (None, 0),
            Some(admission) => {
                let grant = admission
                    .acquire(&cx.ctx, need)
                    .map_err(|e| PixelError::custom(PixelErrorKind::Other, e.to_string()))?;
                (Some(grant), need)
            }
        };
        let mut decision = FrameCodecDecision {
            transfer_syntax_uid: self.uid.clone(),
            frame: -1,
            partial: true,
            admitted_bytes: admitted,
            ..FrameCodecDecision::default()
        };
        let mut failure = None;
        for backend in &self.backends {
            if !backend.accepts(codestream, metadata) {
                decision.declined.push(BackendDecline {
                    backend: backend.name.clone(),
                    reason: DECLINE_NOT_ACCEPTED,
                });
                continue;
            }
            let err = match call(backend.codec.as_ref()) {
                None => return Err(unsupported()),
                Some(Ok(mut frames)) => {
                    decision.backend = backend.name.clone();
                    if decision.fallback {
                        add_diagnostic(&mut frames, DIAGNOSTIC_CODEC_BACKEND_FALLBACK);
                    }
                    self.decided(&decision);
                    return Ok(frames);
                }
                Some(Err(err)) => err,
            };
            if let Some(ctx_err) = context_error(&cx.ctx) {
                return Err(ctx_err.into());
            }
            if err.is(PartErrorKind::RegionUnsupported)
                || err.is(PartErrorKind::PartialDecodeUnsupported)
                || err.is(PartErrorKind::InvalidReduction)
                || err.is(PartErrorKind::InvalidQualityLayers)
            {
                return Err(err);
            }
            if !self.may_fall_back(&backend.name, &as_pixel_error(&err)) {
                return Err(err);
            }
            failure = Some(err);
            decision.declined.push(BackendDecline {
                backend: backend.name.clone(),
                reason: DECLINE_DECODE_FAILED,
            });
            decision.fallback = true;
        }
        if let Some(err) = failure {
            return Err(err);
        }
        Err(PixelError::custom(
            PixelErrorKind::NoCodecBackend,
            format!("{ERR_NO_CODEC_BACKEND}: a part of a frame of {}", self.uid),
        )
        .into())
    }
}
impl Codec for CodecRouter {
    fn decode(
        &self,
        cx: &DecodeContext,
        pixel: &PixelData<'_>,
        obj: &Object,
    ) -> Result<Frames, PixelError> {
        self.decode_frames(cx, pixel, obj)
    }
    fn region_decoder(&self) -> Option<&dyn FrameRegionDecoder> {
        Some(self)
    }
    fn partial_decoder(&self) -> Option<&dyn PartialFrameDecoder> {
        Some(self)
    }
}
impl FrameRegionDecoder for CodecRouter {
    fn decode_frame_region(
        &self,
        cx: &DecodeContext,
        codestream: &[u8],
        metadata: &Metadata,
        region: FrameRegion,
    ) -> Result<Frames, PartError> {
        CodecRouter::decode_frame_region(self, cx, codestream, metadata, region)
    }
}
impl PartialFrameDecoder for CodecRouter {
    fn partial_decode_capabilities(
        &self,
        codestream: &[u8],
        metadata: &Metadata,
    ) -> Result<PartialDecodeCapabilities, PartError> {
        CodecRouter::partial_decode_capabilities(self, codestream, metadata)
    }
    fn decode_frame_partial(
        &self,
        cx: &DecodeContext,
        codestream: &[u8],
        metadata: &Metadata,
        request: &PartialDecodeRequest,
    ) -> Result<Frames, PartError> {
        CodecRouter::decode_frame_partial(self, cx, codestream, metadata, request)
    }
}
