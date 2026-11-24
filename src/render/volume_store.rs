#![forbid(unsafe_code)]
use super::affine::{GeometryAffine, finite_positive_spacing};
use super::error::{ErrorKind, RenderError};
use super::parallel::{Cancellation, ctx_err};
use super::plane_buffer::le_bytes;
use crate::numtext::format_float;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
pub const VOLUME_SNAPSHOT_CONTRACT_VERSION: u32 = 1;
pub const VOLUME_SNAPSHOT_CONTRACT_VERSION_V2: u32 = 2;
pub const VOLUME_SNAPSHOT_HEADER_SIZE_V1: u32 = 360;
const ERR_INVALID: &str = "render: invalid volume snapshot";
const ERR_CLOSED: &str = "render: volume store closed";
const ERR_NOT_FOUND: &str = "render: volume generation not found";
const ERR_RELEASED: &str = "render: volume lease released";
const ERR_BUDGET: &str = "render: volume store byte budget exceeded";
fn invalid(detail: impl std::fmt::Display) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVolumeSnapshot,
        format!("{ERR_INVALID}: {detail}"),
    )
}
fn closed_err() -> RenderError {
    RenderError::new(ErrorKind::VolumeStoreClosed, ERR_CLOSED)
}
fn not_found(detail: impl std::fmt::Display) -> RenderError {
    RenderError::new(
        ErrorKind::VolumeNotFound,
        format!("{ERR_NOT_FOUND}: {detail}"),
    )
}
fn released_err() -> RenderError {
    RenderError::new(ErrorKind::VolumeLeaseReleased, ERR_RELEASED)
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct VolumeScalarFormat(pub u32);
impl VolumeScalarFormat {
    pub const UNKNOWN: VolumeScalarFormat = VolumeScalarFormat(0);
    pub const F32_MODALITY_LE: VolumeScalarFormat = VolumeScalarFormat(1);
    pub const I16_STORED_LE: VolumeScalarFormat = VolumeScalarFormat(2);
    pub const U16_STORED_LE: VolumeScalarFormat = VolumeScalarFormat(3);
    pub fn name(self) -> &'static str {
        match self.0 {
            1 => "F32_MODALITY_LE",
            2 => "I16_STORED_LE",
            3 => "U16_STORED_LE",
            _ => "UNKNOWN",
        }
    }
    fn scalar_size(self) -> Option<u64> {
        match self.0 {
            1 => Some(4),
            2 | 3 => Some(2),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct VolumeSampleDomain(pub u32);
impl VolumeSampleDomain {
    pub const UNKNOWN: VolumeSampleDomain = VolumeSampleDomain(0);
    pub const MODALITY: VolumeSampleDomain = VolumeSampleDomain(1);
    pub const STORED: VolumeSampleDomain = VolumeSampleDomain(2);
    pub fn name(self) -> &'static str {
        match self.0 {
            1 => "MODALITY",
            2 => "STORED",
            _ => "UNKNOWN",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct VolumeDerivation(pub u32);
impl VolumeDerivation {
    pub const UNKNOWN: VolumeDerivation = VolumeDerivation(0);
    pub const NORMALIZED: VolumeDerivation = VolumeDerivation(1);
    pub const REGULARIZED: VolumeDerivation = VolumeDerivation(2);
    pub const VR_PREFILTERED: VolumeDerivation = VolumeDerivation(3);
    pub fn name(self) -> &'static str {
        match self.0 {
            1 => "normalized",
            2 => "regularized",
            3 => "vr_prefiltered",
            _ => "unknown",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct VolumeMemoryKind(pub u32);
impl VolumeMemoryKind {
    pub const UNKNOWN: VolumeMemoryKind = VolumeMemoryKind(0);
    pub const RAW_FRAMES: VolumeMemoryKind = VolumeMemoryKind(1);
    pub const NORMALIZED: VolumeMemoryKind = VolumeMemoryKind(2);
    pub const REGULARIZED: VolumeMemoryKind = VolumeMemoryKind(3);
    pub const RGBA: VolumeMemoryKind = VolumeMemoryKind(4);
    pub const BACKEND_COPY: VolumeMemoryKind = VolumeMemoryKind(5);
    pub const VR_PREPARED: VolumeMemoryKind = VolumeMemoryKind(6);
    pub fn name(self) -> &'static str {
        match self.0 {
            1 => "raw_frames",
            2 => "normalized_volume",
            3 => "regularized_volume",
            6 => "vr_prepared_volume",
            4 => "rgba_render_buffer",
            5 => "backend_copy",
            _ => "unknown",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VolumeDescriptor {
    pub contract_version: u32,
    pub header_size: u32,
    pub volume_generation: u64,
    pub parent_generation: u64,
    pub derivation: VolumeDerivation,
    pub dimensions: [u32; 3],
    pub components: u32,
    pub scalar_format: VolumeScalarFormat,
    pub sample_domain: VolumeSampleDomain,
    pub row_stride_bytes: u64,
    pub slice_stride_bytes: u64,
    pub byte_length: u64,
    pub rescale_slope: f64,
    pub rescale_intercept: f64,
    pub spacing_mm: [f64; 3],
    pub index_to_patient_lps: GeometryAffine,
    pub patient_lps_to_index: GeometryAffine,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VolumeResidencyKey {
    pub store_identity: u64,
    pub generation: u64,
}
impl VolumeResidencyKey {
    pub fn valid(&self) -> bool {
        self.store_identity != 0 && self.generation != 0
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct VolumeStoreIdentity(u64);
impl VolumeStoreIdentity {
    pub fn residency_key(self, generation: u64) -> VolumeResidencyKey {
        VolumeResidencyKey {
            store_identity: self.0,
            generation,
        }
    }
    pub fn ordinal(self) -> u64 {
        self.0
    }
}
enum Payload {
    Bytes(Vec<u8>),
    F32(Vec<f32>),
    Shared(Arc<super::volume::Payload>),
}
#[derive(Clone, Copy)]
enum Packed16<'a> {
    Signed(&'a [i16]),
    Unsigned(&'a [u16]),
}
impl Packed16<'_> {
    fn count(self) -> usize {
        match self {
            Packed16::Signed(v) => v.len(),
            Packed16::Unsigned(v) => v.len(),
        }
    }
    fn raw(self, index: usize) -> u16 {
        match self {
            Packed16::Signed(v) => v[index] as u16,
            Packed16::Unsigned(v) => v[index],
        }
    }
    fn stored(self, index: usize) -> f64 {
        match self {
            Packed16::Signed(v) => f64::from(v[index]),
            Packed16::Unsigned(v) => f64::from(v[index]),
        }
    }
}
fn checked_mul(a: u64, b: u64) -> Option<u64> {
    a.checked_mul(b)
}
fn checked_add(a: u64, b: u64) -> Option<u64> {
    a.checked_add(b)
}
fn write_exact(dst: &mut dyn Write, payload: &[u8]) -> Result<(), RenderError> {
    match dst.write(payload) {
        Err(e) => Err(RenderError::new(ErrorKind::Other, e.to_string())),
        Ok(n) if n != payload.len() => Err(RenderError::new(ErrorKind::Other, "short write")),
        Ok(_) => Ok(()),
    }
}
impl Payload {
    fn f32_values(&self) -> Option<&[f32]> {
        match self {
            Payload::F32(v) => Some(v),
            Payload::Shared(p) => match &**p {
                super::volume::Payload::F32(v) | super::volume::Payload::Rgb(v) => Some(v),
                _ => None,
            },
            Payload::Bytes(_) => None,
        }
    }
    fn packed16(&self) -> Option<Packed16<'_>> {
        match self {
            Payload::Shared(p) => match &**p {
                super::volume::Payload::I16 { data, .. } => Some(Packed16::Signed(data)),
                super::volume::Payload::U16 { data, .. } => Some(Packed16::Unsigned(data)),
                _ => None,
            },
            _ => None,
        }
    }
    fn byte_len(&self) -> u64 {
        if let Payload::Bytes(b) = self {
            return b.len() as u64;
        }
        if let Some(v) = self.f32_values() {
            return v.len() as u64 * 4;
        }
        self.packed16().map_or(0, |p| p.count() as u64 * 2)
    }
    fn le_bytes(&self) -> Option<&[u8]> {
        match self {
            Payload::Bytes(b) => Some(b),
            Payload::F32(v) => le_bytes(v),
            Payload::Shared(p) => match &**p {
                super::volume::Payload::F32(v) | super::volume::Payload::Rgb(v) => le_bytes(v),
                super::volume::Payload::I16 { data, .. } => le_bytes(data),
                super::volume::Payload::U16 { data, .. } => le_bytes(data),
            },
        }
    }
    fn copy_bytes(&self, dst: &mut dyn Write) -> Result<(), RenderError> {
        if let Payload::Bytes(b) = self {
            return write_exact(dst, b);
        }
        if let Some(bytes) = self.le_bytes() {
            if self.f32_values().is_some() {
                for value in bytes.as_chunks::<4>().0 {
                    write_exact(dst, value)?;
                }
                return Ok(());
            }
            let (chunks, rest) = bytes.as_chunks::<{ 64 << 10 }>();
            for chunk in chunks {
                write_exact(dst, chunk)?;
            }
            return write_exact(dst, rest);
        }
        if let Some(v) = self.f32_values() {
            for value in v {
                write_exact(dst, &value.to_bits().to_le_bytes())?;
            }
            return Ok(());
        }
        let Some(p) = self.packed16() else {
            return Ok(());
        };
        let mut buffer = Vec::with_capacity(64 << 10);
        for i in 0..p.count() {
            buffer.extend_from_slice(&p.raw(i).to_le_bytes());
            if buffer.len() == buffer.capacity() {
                write_exact(dst, &buffer)?;
                buffer.clear();
            }
        }
        write_exact(dst, &buffer)
    }
    fn to_le_bytes(&self) -> Vec<u8> {
        if let Some(bytes) = self.le_bytes() {
            return bytes.to_vec();
        }
        let mut out = Vec::with_capacity(usize::try_from(self.byte_len()).unwrap_or(0));
        if let Some(v) = self.f32_values() {
            for value in v {
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        } else if let Some(p) = self.packed16() {
            match p {
                Packed16::Signed(v) => {
                    for value in v {
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                }
                Packed16::Unsigned(v) => {
                    for value in v {
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                }
            }
        }
        out
    }
    fn modality_at(&self, offset: u64, d: &VolumeDescriptor) -> Option<f64> {
        if let Some(p) = self.packed16() {
            if !offset.is_multiple_of(2) {
                return None;
            }
            let index = usize::try_from(offset / 2).ok()?;
            let matches = match p {
                Packed16::Signed(_) => d.scalar_format == VolumeScalarFormat::I16_STORED_LE,
                Packed16::Unsigned(_) => d.scalar_format == VolumeScalarFormat::U16_STORED_LE,
            };
            if !matches || index >= p.count() {
                return None;
            }
            return Some(p.stored(index) * d.rescale_slope + d.rescale_intercept);
        }
        if let Some(v) = self.f32_values() {
            if d.scalar_format != VolumeScalarFormat::F32_MODALITY_LE || !offset.is_multiple_of(4) {
                return None;
            }
            return v
                .get(usize::try_from(offset / 4).ok()?)
                .map(|x| f64::from(*x));
        }
        match self {
            Payload::Bytes(data) => {
                let size = d.scalar_format.scalar_size()?;
                let end = checked_add(offset, size)?;
                if end > data.len() as u64 {
                    return None;
                }
                let s = &data[offset as usize..end as usize];
                match d.scalar_format {
                    VolumeScalarFormat::F32_MODALITY_LE => {
                        Some(f64::from(f32::from_le_bytes([s[0], s[1], s[2], s[3]])))
                    }
                    VolumeScalarFormat::I16_STORED_LE => Some(
                        f64::from(i16::from_le_bytes([s[0], s[1]])) * d.rescale_slope
                            + d.rescale_intercept,
                    ),
                    _ => Some(
                        f64::from(u16::from_le_bytes([s[0], s[1]])) * d.rescale_slope
                            + d.rescale_intercept,
                    ),
                }
            }
            _ => None,
        }
    }
}
struct Record {
    descriptor: VolumeDescriptor,
    payload: Arc<Payload>,
    kind: VolumeMemoryKind,
    leases: u64,
    pins: u64,
    retired: bool,
    on_released: Vec<ReleaseFn>,
}
pub type ReleaseFn = Box<dyn FnOnce() + Send>;
struct Tracked {
    generation: u64,
    kind: VolumeMemoryKind,
    bytes: u64,
    release: Option<ReleaseFn>,
}
#[derive(Default)]
struct State {
    max_live_bytes: u64,
    next_gen: u64,
    current: u64,
    records: BTreeMap<u64, Record>,
    tracked: BTreeMap<u64, Tracked>,
    next_tracked: u64,
    closed: bool,
    live_bytes: u64,
    evictions: u64,
    evicted_bytes: u64,
    replacements: u64,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VolumeStoreOptions {
    pub max_live_bytes: u64,
}
pub struct VolumeStore {
    identity: u64,
    state: Mutex<State>,
}
static NEXT_VOLUME_STORE_IDENTITY: AtomicU64 = AtomicU64::new(0);
fn run_release_callbacks(callbacks: Vec<ReleaseFn>) {
    for callback in callbacks {
        callback();
    }
}
impl State {
    fn collect_retired(&mut self) -> Vec<ReleaseFn> {
        let generations: Vec<u64> = self
            .records
            .iter()
            .filter(|(_, r)| r.retired && r.leases == 0 && r.pins == 0)
            .map(|(g, _)| *g)
            .collect();
        let mut callbacks = Vec::new();
        for generation in generations {
            callbacks.extend(self.remove_generation(generation, false).1);
        }
        callbacks
    }
    fn remove_generation(
        &mut self,
        generation: u64,
        count_eviction: bool,
    ) -> (u64, Vec<ReleaseFn>) {
        let Some(record) = self.records.get_mut(&generation) else {
            return (0, Vec::new());
        };
        if record.leases != 0 || record.pins != 0 {
            return (0, Vec::new());
        }
        let mut bytes = record.payload.byte_len();
        let mut callbacks = std::mem::take(&mut record.on_released);
        let ids: Vec<u64> = self
            .tracked
            .iter()
            .filter(|(_, t)| t.generation == generation)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(t) = self.tracked.remove(&id) {
                bytes += t.bytes;
                callbacks.extend(t.release);
            }
        }
        self.records.remove(&generation);
        if generation == self.current {
            self.current = 0;
        }
        self.live_bytes -= bytes;
        if count_eviction {
            self.evictions += 1;
            self.evicted_bytes += bytes;
        }
        (bytes, callbacks)
    }
}
impl VolumeStore {
    pub fn new(options: VolumeStoreOptions) -> Arc<VolumeStore> {
        let identity = NEXT_VOLUME_STORE_IDENTITY.fetch_add(1, Ordering::SeqCst) + 1;
        Arc::new(VolumeStore {
            identity,
            state: Mutex::new(State {
                max_live_bytes: options.max_live_bytes,
                ..State::default()
            }),
        })
    }
    pub fn identity(&self) -> VolumeStoreIdentity {
        VolumeStoreIdentity(self.identity)
    }
    pub fn replace(
        &self,
        descriptor: VolumeDescriptor,
        payload: &[u8],
    ) -> Result<u64, RenderError> {
        let mut descriptor = descriptor;
        if descriptor.derivation == VolumeDerivation::UNKNOWN {
            descriptor.derivation = VolumeDerivation::NORMALIZED;
        }
        descriptor.volume_generation = 0;
        validate_volume_descriptor(&descriptor, true)?;
        if payload.len() as u64 != descriptor.byte_length {
            return Err(invalid(format!(
                "payload length {}, descriptor byte length {}",
                payload.len(),
                descriptor.byte_length
            )));
        }
        self.replace_owned(descriptor, Payload::Bytes(payload.to_vec()))
    }
    pub fn replace_float32(
        &self,
        descriptor: VolumeDescriptor,
        values: &[f32],
    ) -> Result<u64, RenderError> {
        self.replace_modality_float32(descriptor, values.to_vec(), VolumeDerivation::NORMALIZED, 0)
    }
    #[doc(hidden)]
    pub fn replace_vr_prefiltered_float32(
        &self,
        descriptor: VolumeDescriptor,
        values: Vec<f32>,
    ) -> Result<u64, RenderError> {
        self.replace_modality_float32(descriptor, values, VolumeDerivation::VR_PREFILTERED, 0)
    }
    fn replace_modality_float32(
        &self,
        descriptor: VolumeDescriptor,
        values: Vec<f32>,
        derivation: VolumeDerivation,
        parent_generation: u64,
    ) -> Result<u64, RenderError> {
        let mut descriptor = descriptor;
        descriptor.scalar_format = VolumeScalarFormat::F32_MODALITY_LE;
        descriptor.sample_domain = VolumeSampleDomain::MODALITY;
        descriptor.rescale_slope = 1.0;
        descriptor.rescale_intercept = 0.0;
        descriptor.derivation = derivation;
        descriptor.parent_generation = parent_generation;
        descriptor.volume_generation = 0;
        descriptor.byte_length = values.len() as u64 * 4;
        validate_volume_descriptor(&descriptor, true)?;
        self.replace_owned(descriptor, Payload::F32(values))
    }
    pub fn replace_regularized_float32(
        &self,
        parent_generation: u64,
        descriptor: VolumeDescriptor,
        values: &[f32],
    ) -> Result<u64, RenderError> {
        if parent_generation == 0 {
            return Err(invalid("regularized generation lacks parent"));
        }
        let mut descriptor = descriptor;
        descriptor.scalar_format = VolumeScalarFormat::F32_MODALITY_LE;
        descriptor.sample_domain = VolumeSampleDomain::MODALITY;
        descriptor.rescale_slope = 1.0;
        descriptor.rescale_intercept = 0.0;
        descriptor.derivation = VolumeDerivation::REGULARIZED;
        descriptor.parent_generation = parent_generation;
        descriptor.volume_generation = 0;
        descriptor.byte_length = values.len() as u64 * 4;
        validate_volume_descriptor(&descriptor, true)?;
        if !lock(&self.state).records.contains_key(&parent_generation) {
            return Err(not_found(format!("parent generation {parent_generation}")));
        }
        self.replace_owned(descriptor, Payload::F32(values.to_vec()))
    }
    fn replace_owned(
        &self,
        descriptor: VolumeDescriptor,
        payload: Payload,
    ) -> Result<u64, RenderError> {
        let mut descriptor = descriptor;
        let incoming = payload.byte_len();
        if incoming != descriptor.byte_length {
            return Err(invalid(format!(
                "payload length {incoming}, descriptor byte length {}",
                descriptor.byte_length
            )));
        }
        let mut s = lock(&self.state);
        if s.closed {
            return Err(closed_err());
        }
        if descriptor.parent_generation != 0
            && !s.records.contains_key(&descriptor.parent_generation)
        {
            return Err(not_found(format!(
                "parent generation {}",
                descriptor.parent_generation
            )));
        }
        let mut releases = s.collect_retired();
        let fail = |s: MutexGuard<'_, State>, releases: Vec<ReleaseFn>, e: RenderError| {
            drop(s);
            run_release_callbacks(releases);
            Err(e)
        };
        let reclaimable = match s.records.get(&s.current) {
            Some(p) if p.leases == 0 && p.pins == 0 => p.payload.byte_len(),
            _ => 0,
        };
        let prospective = checked_add(s.live_bytes, incoming);
        let over = match prospective {
            None => true,
            Some(p) => {
                p < reclaimable || (s.max_live_bytes > 0 && p - reclaimable > s.max_live_bytes)
            }
        };
        if over {
            let e = RenderError::new(
                ErrorKind::VolumeBudgetExceeded,
                format!(
                    "{ERR_BUDGET}: live={} incoming={incoming} reclaimable={reclaimable} limit={}",
                    s.live_bytes, s.max_live_bytes
                ),
            );
            return fail(s, releases, e);
        }
        if s.next_gen == u64::MAX {
            return fail(s, releases, invalid("generation overflow"));
        }
        descriptor.volume_generation = s.next_gen + 1;
        if let Err(e) = validate_volume_descriptor(&descriptor, false) {
            return fail(s, releases, e);
        }
        let current = s.current;
        if let Some(previous) = s.records.get_mut(&current) {
            previous.retired = true;
        }
        let kind = if descriptor.derivation == VolumeDerivation::REGULARIZED {
            VolumeMemoryKind::REGULARIZED
        } else if descriptor.derivation == VolumeDerivation::VR_PREFILTERED {
            VolumeMemoryKind::VR_PREPARED
        } else {
            VolumeMemoryKind::NORMALIZED
        };
        if u64::MAX - s.live_bytes < incoming {
            return fail(s, releases, invalid("live-byte accounting overflow"));
        }
        let generation = descriptor.volume_generation;
        s.next_gen = generation;
        s.records.insert(
            generation,
            Record {
                descriptor,
                payload: Arc::new(payload),
                kind,
                leases: 0,
                pins: 0,
                retired: false,
                on_released: Vec::new(),
            },
        );
        s.current = generation;
        s.live_bytes += incoming;
        s.replacements += 1;
        releases.extend(s.collect_retired());
        drop(s);
        run_release_callbacks(releases);
        Ok(generation)
    }
    pub(crate) fn replace_shared(
        &self,
        descriptor: VolumeDescriptor,
        payload: Arc<super::volume::Payload>,
    ) -> Result<u64, RenderError> {
        let packed = matches!(
            &*payload,
            super::volume::Payload::I16 { .. } | super::volume::Payload::U16 { .. }
        );
        if !packed {
            let values = match &*payload {
                super::volume::Payload::F32(v) | super::volume::Payload::Rgb(v) => v.len(),
                _ => 0,
            };
            let mut descriptor = descriptor;
            descriptor.scalar_format = VolumeScalarFormat::F32_MODALITY_LE;
            descriptor.sample_domain = VolumeSampleDomain::MODALITY;
            descriptor.rescale_slope = 1.0;
            descriptor.rescale_intercept = 0.0;
            descriptor.derivation = VolumeDerivation::NORMALIZED;
            descriptor.parent_generation = 0;
            descriptor.volume_generation = 0;
            descriptor.byte_length = values as u64 * 4;
            validate_volume_descriptor(&descriptor, true)?;
            return self.replace_owned(descriptor, Payload::Shared(payload));
        }
        let shared = Payload::Shared(payload);
        let mut descriptor = descriptor;
        descriptor.scalar_format = match shared.packed16() {
            Some(Packed16::Signed(_)) => VolumeScalarFormat::I16_STORED_LE,
            _ => VolumeScalarFormat::U16_STORED_LE,
        };
        descriptor.sample_domain = VolumeSampleDomain::STORED;
        descriptor.derivation = VolumeDerivation::NORMALIZED;
        descriptor.parent_generation = 0;
        descriptor.volume_generation = 0;
        descriptor.row_stride_bytes = u64::from(descriptor.dimensions[0]) * 2;
        descriptor.slice_stride_bytes =
            u64::from(descriptor.dimensions[1]) * descriptor.row_stride_bytes;
        descriptor.byte_length = shared.byte_len();
        validate_volume_descriptor(&descriptor, true)?;
        let mut check = descriptor;
        check.volume_generation = 1;
        validate_modality_f32_conversion(&check)?;
        self.replace_owned(descriptor, shared)
    }
    pub(crate) fn replace_regularized_shared(
        &self,
        parent_generation: u64,
        descriptor: VolumeDescriptor,
        payload: Arc<super::volume::Payload>,
    ) -> Result<u64, RenderError> {
        if parent_generation == 0 {
            return Err(invalid("regularized generation lacks parent"));
        }
        let values = match &*payload {
            super::volume::Payload::F32(v) | super::volume::Payload::Rgb(v) => v.len(),
            _ => 0,
        };
        let mut descriptor = descriptor;
        descriptor.scalar_format = VolumeScalarFormat::F32_MODALITY_LE;
        descriptor.sample_domain = VolumeSampleDomain::MODALITY;
        descriptor.rescale_slope = 1.0;
        descriptor.rescale_intercept = 0.0;
        descriptor.derivation = VolumeDerivation::REGULARIZED;
        descriptor.parent_generation = parent_generation;
        descriptor.volume_generation = 0;
        descriptor.byte_length = values as u64 * 4;
        validate_volume_descriptor(&descriptor, true)?;
        if !lock(&self.state).records.contains_key(&parent_generation) {
            return Err(not_found(format!("parent generation {parent_generation}")));
        }
        self.replace_owned(descriptor, Payload::Shared(payload))
    }
    pub(crate) fn add_release_observer(&self, generation: u64, release: ReleaseFn) -> bool {
        let mut s = lock(&self.state);
        match s.records.get_mut(&generation) {
            Some(record) => {
                record.on_released.push(release);
                true
            }
            None => false,
        }
    }
    pub fn acquire_current(self: &Arc<Self>) -> Result<VolumeLease, RenderError> {
        let generation = lock(&self.state).current;
        if generation == 0 {
            return Err(RenderError::new(ErrorKind::VolumeNotFound, ERR_NOT_FOUND));
        }
        self.acquire(generation)
    }
    pub fn acquire(self: &Arc<Self>, generation: u64) -> Result<VolumeLease, RenderError> {
        let mut s = lock(&self.state);
        if s.closed {
            return Err(closed_err());
        }
        let Some(record) = s.records.get_mut(&generation) else {
            return Err(not_found(generation));
        };
        record.leases += 1;
        Ok(VolumeLease {
            inner: Arc::new(Mutex::new(Handle {
                store: Some(Arc::clone(self)),
                generation,
                released: false,
                descriptor_override: None,
            })),
        })
    }
    pub fn pin(self: &Arc<Self>, generation: u64) -> Result<VolumePin, RenderError> {
        let mut s = lock(&self.state);
        if s.closed {
            return Err(closed_err());
        }
        let Some(record) = s.records.get_mut(&generation) else {
            return Err(not_found(generation));
        };
        record.pins += 1;
        Ok(VolumePin {
            inner: Mutex::new(Handle {
                store: Some(Arc::clone(self)),
                generation,
                released: false,
                descriptor_override: None,
            }),
        })
    }
    pub fn pin_current(self: &Arc<Self>) -> Result<VolumePin, RenderError> {
        let generation = lock(&self.state).current;
        if generation == 0 {
            return Err(RenderError::new(ErrorKind::VolumeNotFound, ERR_NOT_FOUND));
        }
        self.pin(generation)
    }
    pub fn current_generation(&self) -> u64 {
        let s = lock(&self.state);
        if s.closed { 0 } else { s.current }
    }
    pub fn evict(&self, target_bytes: u64, include_current: bool) -> u64 {
        if target_bytes == 0 {
            return 0;
        }
        let mut s = lock(&self.state);
        let current = s.current;
        let mut generations: Vec<u64> = s
            .records
            .iter()
            .filter(|(g, r)| r.leases == 0 && r.pins == 0 && (**g != current || include_current))
            .map(|(g, _)| *g)
            .collect();
        generations.sort_by_key(|g| (*g == current, *g));
        let mut freed = 0u64;
        let mut callbacks = Vec::new();
        for generation in generations {
            if freed >= target_bytes {
                break;
            }
            let (bytes, releases) = s.remove_generation(generation, true);
            freed += bytes;
            callbacks.extend(releases);
        }
        drop(s);
        run_release_callbacks(callbacks);
        freed
    }
    pub fn track_memory(
        self: &Arc<Self>,
        generation: u64,
        kind: VolumeMemoryKind,
        byte_count: u64,
        release: Option<ReleaseFn>,
    ) -> Result<VolumeMemoryLease, RenderError> {
        if kind != VolumeMemoryKind::RAW_FRAMES
            && kind != VolumeMemoryKind::RGBA
            && kind != VolumeMemoryKind::BACKEND_COPY
        {
            return Err(invalid(format!(
                "memory kind {} cannot be externally tracked",
                kind.name()
            )));
        }
        if byte_count == 0 {
            return Err(invalid("zero tracked allocation"));
        }
        let mut s = lock(&self.state);
        if s.closed {
            return Err(closed_err());
        }
        if !s.records.contains_key(&generation) {
            return Err(not_found(generation));
        }
        if u64::MAX - s.live_bytes < byte_count {
            return Err(invalid("live-byte accounting overflow"));
        }
        let callbacks = s.collect_retired();
        if s.max_live_bytes > 0 && s.live_bytes + byte_count > s.max_live_bytes {
            let e = RenderError::new(
                ErrorKind::VolumeBudgetExceeded,
                format!(
                    "{ERR_BUDGET}: live={} incoming={byte_count} limit={}",
                    s.live_bytes, s.max_live_bytes
                ),
            );
            drop(s);
            run_release_callbacks(callbacks);
            return Err(e);
        }
        if s.next_tracked == u64::MAX {
            drop(s);
            run_release_callbacks(callbacks);
            return Err(invalid("tracked allocation id overflow"));
        }
        s.next_tracked += 1;
        let id = s.next_tracked;
        s.tracked.insert(
            id,
            Tracked {
                generation,
                kind,
                bytes: byte_count,
                release,
            },
        );
        if let Some(record) = s.records.get_mut(&generation) {
            record.leases += 1;
        }
        s.live_bytes += byte_count;
        drop(s);
        run_release_callbacks(callbacks);
        Ok(VolumeMemoryLease {
            inner: Mutex::new((Some(Arc::clone(self)), id, false)),
        })
    }
    pub fn close(&self) {
        let mut s = lock(&self.state);
        if s.closed {
            return;
        }
        s.closed = true;
        s.current = 0;
        for record in s.records.values_mut() {
            record.retired = true;
        }
        let callbacks = s.collect_retired();
        drop(s);
        run_release_callbacks(callbacks);
    }
    fn release_reference(&self, generation: u64, pin: bool) {
        let mut s = lock(&self.state);
        let Some(record) = s.records.get_mut(&generation) else {
            return;
        };
        let count = if pin {
            &mut record.pins
        } else {
            &mut record.leases
        };
        if *count == 0 {
            return;
        }
        *count -= 1;
        let callbacks = s.collect_retired();
        drop(s);
        run_release_callbacks(callbacks);
    }
    fn record(&self, generation: u64) -> Result<(VolumeDescriptor, Arc<Payload>), RenderError> {
        let s = lock(&self.state);
        match s.records.get(&generation) {
            Some(r) => Ok((r.descriptor, Arc::clone(&r.payload))),
            None => Err(not_found(generation)),
        }
    }
    pub fn stats(&self) -> VolumeStoreStats {
        let s = lock(&self.state);
        let mut stats = VolumeStoreStats {
            closed: s.closed,
            max_live_bytes: s.max_live_bytes,
            current_generation: s.current,
            live_generations: s.records.len() as u64,
            metadata_records: s.records.len() as u64,
            tracked_records: s.tracked.len() as u64,
            live_bytes: s.live_bytes,
            replacements: s.replacements,
            evictions: s.evictions,
            evicted_bytes: s.evicted_bytes,
            ..VolumeStoreStats::default()
        };
        let mut by_kind: BTreeMap<VolumeMemoryKind, VolumeMemoryStats> = BTreeMap::new();
        for record in s.records.values() {
            let bytes = record.payload.byte_len();
            let entry = by_kind.entry(record.kind).or_insert(VolumeMemoryStats {
                kind: record.kind,
                ..VolumeMemoryStats::default()
            });
            entry.live_bytes += bytes;
            entry.allocations += 1;
            stats.active_leases += record.leases;
            stats.active_pins += record.pins;
            if record.leases > 0 {
                entry.leased_bytes += bytes;
                stats.leased_bytes += bytes;
            }
            if record.pins > 0 {
                entry.pinned_bytes += bytes;
                stats.pinned_bytes += bytes;
            }
        }
        for tracked in s.tracked.values() {
            let entry = by_kind.entry(tracked.kind).or_insert(VolumeMemoryStats {
                kind: tracked.kind,
                ..VolumeMemoryStats::default()
            });
            entry.live_bytes += tracked.bytes;
            entry.leased_bytes += tracked.bytes;
            entry.allocations += 1;
            stats.leased_bytes += tracked.bytes;
        }
        stats.by_kind = by_kind.into_values().collect();
        stats
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeMemoryStats {
    pub kind: VolumeMemoryKind,
    pub live_bytes: u64,
    pub leased_bytes: u64,
    pub pinned_bytes: u64,
    pub allocations: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VolumeStoreStats {
    pub closed: bool,
    pub max_live_bytes: u64,
    pub current_generation: u64,
    pub live_generations: u64,
    pub metadata_records: u64,
    pub tracked_records: u64,
    pub live_bytes: u64,
    pub active_leases: u64,
    pub active_pins: u64,
    pub leased_bytes: u64,
    pub pinned_bytes: u64,
    pub replacements: u64,
    pub evictions: u64,
    pub evicted_bytes: u64,
    pub by_kind: Vec<VolumeMemoryStats>,
}
struct Handle {
    store: Option<Arc<VolumeStore>>,
    generation: u64,
    released: bool,
    descriptor_override: Option<VolumeDescriptor>,
}
pub struct VolumeLease {
    inner: Arc<Mutex<Handle>>,
}
fn lease_record(inner: &Mutex<Handle>) -> Result<(VolumeDescriptor, Arc<Payload>), RenderError> {
    let h = lock(inner);
    match (&h.store, h.released) {
        (Some(store), false) => store.record(h.generation),
        _ => Err(released_err()),
    }
}
fn lease_key(inner: &Mutex<Handle>) -> VolumeResidencyKey {
    let h = lock(inner);
    match &h.store {
        Some(store) if !h.released && h.generation != 0 => VolumeResidencyKey {
            store_identity: store.identity,
            generation: h.generation,
        },
        _ => VolumeResidencyKey::default(),
    }
}
impl VolumeLease {
    pub fn snapshot(&self) -> Result<VolumeSnapshot, RenderError> {
        lease_record(&self.inner)?;
        Ok(VolumeSnapshot {
            lease: Some(Arc::clone(&self.inner)),
        })
    }
    pub fn generation(&self) -> u64 {
        let h = lock(&self.inner);
        if h.released { 0 } else { h.generation }
    }
    pub fn store_identity(&self) -> VolumeStoreIdentity {
        let h = lock(&self.inner);
        match &h.store {
            Some(store) if !h.released => store.identity(),
            _ => VolumeStoreIdentity::default(),
        }
    }
    pub fn residency_key(&self) -> VolumeResidencyKey {
        lease_key(&self.inner)
    }
    pub fn release(&self) {
        let mut h = lock(&self.inner);
        if h.released {
            return;
        }
        h.released = true;
        let store = h.store.take();
        let generation = std::mem::take(&mut h.generation);
        h.descriptor_override = None;
        drop(h);
        if let Some(store) = store {
            store.release_reference(generation, false);
        }
    }
    pub(crate) fn set_descriptor_override(&self, descriptor: VolumeDescriptor) {
        lock(&self.inner).descriptor_override = Some(descriptor);
    }
    pub fn track_memory(
        &self,
        kind: VolumeMemoryKind,
        byte_count: u64,
        release: Option<ReleaseFn>,
    ) -> Result<VolumeMemoryLease, RenderError> {
        let h = lock(&self.inner);
        match &h.store {
            Some(store) if !h.released && h.generation != 0 => {
                store.track_memory(h.generation, kind, byte_count, release)
            }
            _ => Err(released_err()),
        }
    }
}
pub struct VolumePin {
    inner: Mutex<Handle>,
}
impl VolumePin {
    pub fn generation(&self) -> u64 {
        let h = lock(&self.inner);
        if h.released { 0 } else { h.generation }
    }
    pub fn release(&self) {
        let mut h = lock(&self.inner);
        if h.released {
            return;
        }
        h.released = true;
        let store = h.store.take();
        let generation = std::mem::take(&mut h.generation);
        drop(h);
        if let Some(store) = store {
            store.release_reference(generation, true);
        }
    }
}
pub struct VolumeMemoryLease {
    inner: Mutex<(Option<Arc<VolumeStore>>, u64, bool)>,
}
impl VolumeMemoryLease {
    pub fn release(&self) {
        let mut h = lock(&self.inner);
        if h.2 {
            return;
        }
        h.2 = true;
        let store = h.0.take();
        let id = std::mem::take(&mut h.1);
        drop(h);
        let Some(store) = store else {
            return;
        };
        let mut s = lock(&store.state);
        let Some(tracked) = s.tracked.remove(&id) else {
            return;
        };
        s.live_bytes -= tracked.bytes;
        if let Some(record) = s.records.get_mut(&tracked.generation)
            && record.leases > 0
        {
            record.leases -= 1;
        }
        let callbacks = s.collect_retired();
        drop(s);
        if let Some(callback) = tracked.release {
            callback();
        }
        run_release_callbacks(callbacks);
    }
}
#[derive(Clone)]
pub struct VolumePayloadBytes {
    payload: Arc<Payload>,
}
impl AsRef<[u8]> for VolumePayloadBytes {
    fn as_ref(&self) -> &[u8] {
        self.payload.le_bytes().unwrap_or(&[])
    }
}
impl std::ops::Deref for VolumePayloadBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.as_ref()
    }
}
impl std::fmt::Debug for VolumePayloadBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VolumePayloadBytes")
            .field("len", &self.as_ref().len())
            .finish()
    }
}
#[derive(Clone, Default)]
pub struct VolumeSnapshot {
    lease: Option<Arc<Mutex<Handle>>>,
}
impl VolumeSnapshot {
    fn record(&self) -> Result<(VolumeDescriptor, Arc<Payload>), RenderError> {
        match &self.lease {
            Some(l) => lease_record(l),
            None => Err(released_err()),
        }
    }
    pub fn descriptor(&self) -> Result<VolumeDescriptor, RenderError> {
        if let Some(l) = &self.lease {
            let h = lock(l);
            if !h.released
                && let Some(d) = h.descriptor_override
            {
                return Ok(d);
            }
        }
        Ok(self.record()?.0)
    }
    pub fn generation(&self) -> u64 {
        self.record().map(|r| r.0.volume_generation).unwrap_or(0)
    }
    pub fn residency_key(&self) -> VolumeResidencyKey {
        self.lease.as_deref().map(lease_key).unwrap_or_default()
    }
    pub fn modality_at(&self, x: u32, y: u32, z: u32) -> Option<f64> {
        let (d, payload) = self.record().ok()?;
        if d.components != 1 {
            return None;
        }
        component_modality_at(&d, &payload, x, y, z, 0)
    }
    pub fn component_modality_at(&self, x: u32, y: u32, z: u32, component: u32) -> Option<f64> {
        let (d, payload) = self.record().ok()?;
        component_modality_at(&d, &payload, x, y, z, component)
    }
    pub fn write_payload_to(&self, dst: &mut dyn Write) -> Result<(), RenderError> {
        let (_, payload) = self.record()?;
        payload.copy_bytes(dst)
    }
    pub fn payload_bytes(&self) -> Result<Option<VolumePayloadBytes>, RenderError> {
        let (_, payload) = self.record()?;
        Ok(payload
            .le_bytes()
            .is_some()
            .then_some(VolumePayloadBytes { payload }))
    }
    pub fn copy_payload(&self) -> Result<Vec<u8>, RenderError> {
        let (_, payload) = self.record()?;
        Ok(payload.to_le_bytes())
    }
    pub fn write_modality_f32_to(&self, dst: &mut dyn Write) -> Result<(), RenderError> {
        self.write_modality_f32_context(None, dst)
    }
    pub fn write_modality_f32_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        dst: &mut dyn Write,
    ) -> Result<(), RenderError> {
        let (d, payload) = self.record()?;
        validate_modality_f32_conversion(&d)?;
        let scalar_size = d
            .scalar_format
            .scalar_size()
            .ok_or_else(|| invalid("unknown scalar format"))?;
        let row_samples = u64::from(d.dimensions[0]) * u64::from(d.components);
        let row_bytes = match checked_mul(row_samples, 4) {
            Some(n) if n <= i64::MAX as u64 => n,
            _ => return Err(invalid("modality row size overflow")),
        };
        if let Some(values) = payload.f32_values()
            && let Some(result) = write_modality_direct(values, ctx, dst, &d)
        {
            return result;
        }
        let mut row = vec![0u8; row_bytes as usize];
        for z in 0..d.dimensions[2] {
            let z_offset = u64::from(z) * d.slice_stride_bytes;
            for y in 0..d.dimensions[1] {
                if let Some(e) = ctx_err(ctx) {
                    return Err(e);
                }
                let source_offset = z_offset + u64::from(y) * d.row_stride_bytes;
                if let Payload::Bytes(data) = &*payload
                    && d.scalar_format == VolumeScalarFormat::F32_MODALITY_LE
                {
                    let end = source_offset + row_bytes;
                    if end > data.len() as u64 {
                        return Err(invalid(format!("modality row unavailable at {y},{z}")));
                    }
                    write_exact(dst, &data[source_offset as usize..end as usize])?;
                    continue;
                }
                for x in 0..row_samples {
                    if x & 255 == 0
                        && let Some(e) = ctx_err(ctx)
                    {
                        return Err(e);
                    }
                    let offset = source_offset + x * scalar_size;
                    let Some(value) = payload.modality_at(offset, &d) else {
                        return Err(invalid(format!(
                            "modality sample unavailable at {x},{y},{z}"
                        )));
                    };
                    let value32 = value as f32;
                    if d.scalar_format != VolumeScalarFormat::F32_MODALITY_LE
                        && (!value.is_finite() || !value32.is_finite())
                    {
                        return Err(invalid(format!(
                            "modality sample out of finite float32 range at {x},{y},{z}"
                        )));
                    }
                    let at = x as usize * 4;
                    row[at..at + 4].copy_from_slice(&value32.to_bits().to_le_bytes());
                }
                write_exact(dst, &row)?;
            }
        }
        Ok(())
    }
    pub fn read_modality_slice_f32(&self, z: u32, dst: &mut [f32]) -> Result<(), RenderError> {
        self.read_modality_slice_f32_context(None, z, dst)
    }
    pub fn read_modality_slice_f32_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        z: u32,
        dst: &mut [f32],
    ) -> Result<(), RenderError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let (d, payload) = self.record()?;
        validate_modality_f32_conversion(&d)?;
        if z >= d.dimensions[2] {
            return Err(invalid(format!(
                "slice {z} outside {} slices",
                d.dimensions[2]
            )));
        }
        let (cols, rows) = (u64::from(d.dimensions[0]), u64::from(d.dimensions[1]));
        match checked_mul(cols, rows) {
            Some(plane) if plane <= dst.len() as u64 => {}
            _ => {
                return Err(invalid(format!(
                    "slice buffer of {} samples for {cols}x{rows}",
                    dst.len()
                )));
            }
        }
        let scalar_size = d
            .scalar_format
            .scalar_size()
            .ok_or_else(|| invalid("unknown scalar format"))?;
        let z_offset = u64::from(z) * d.slice_stride_bytes;
        for y in 0..rows {
            if let Some(e) = ctx_err(ctx) {
                return Err(e);
            }
            let row = &mut dst[(y * cols) as usize..((y + 1) * cols) as usize];
            let row_offset = z_offset + y * d.row_stride_bytes;
            if let Some(values) = payload.f32_values()
                && row_offset % 4 == 0
            {
                let start = row_offset / 4;
                if start + cols <= values.len() as u64 {
                    row.copy_from_slice(&values[start as usize..(start + cols) as usize]);
                    continue;
                }
            }
            for x in 0..cols {
                if x % 4096 == 0
                    && let Some(e) = ctx_err(ctx)
                {
                    return Err(e);
                }
                let Some(value) = payload.modality_at(row_offset + x * scalar_size, &d) else {
                    return Err(invalid(format!(
                        "modality sample unavailable at {x},{y},{z}"
                    )));
                };
                row[x as usize] = value as f32;
            }
        }
        match ctx_err(ctx) {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    pub fn write_stored16_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        dst: &mut dyn Write,
    ) -> Result<(), RenderError> {
        let (d, payload) = self.record()?;
        if d.scalar_format != VolumeScalarFormat::I16_STORED_LE
            && d.scalar_format != VolumeScalarFormat::U16_STORED_LE
        {
            return Err(invalid(format!(
                "{} is not a 16-bit stored format",
                d.scalar_format.name()
            )));
        }
        let row_bytes = match checked_mul(u64::from(d.dimensions[0]), 2) {
            Some(n) if n <= i64::MAX as u64 => n,
            _ => return Err(invalid("stored row size overflow")),
        };
        for z in 0..d.dimensions[2] {
            let z_offset = u64::from(z) * d.slice_stride_bytes;
            for y in 0..d.dimensions[1] {
                if let Some(e) = ctx_err(ctx) {
                    return Err(e);
                }
                let source_offset = z_offset + u64::from(y) * d.row_stride_bytes;
                if let Some(p) = payload.packed16() {
                    let cols = u64::from(d.dimensions[0]);
                    if source_offset % 2 != 0 || source_offset / 2 + cols > p.count() as u64 {
                        return Err(invalid(format!("stored row unavailable at {y},{z}")));
                    }
                    let base = (source_offset / 2) as usize;
                    if let Some(bytes) = payload.le_bytes() {
                        write_exact(dst, &bytes[base * 2..(base + cols as usize) * 2])?;
                        continue;
                    }
                    let row: Vec<u8> = (base..base + cols as usize)
                        .flat_map(|i| p.raw(i).to_le_bytes())
                        .collect();
                    write_exact(dst, &row)?;
                    continue;
                }
                let Payload::Bytes(data) = &*payload else {
                    return Err(invalid("payload holds no 16-bit stored samples"));
                };
                match checked_add(source_offset, row_bytes) {
                    Some(end) if end <= data.len() as u64 => {
                        write_exact(dst, &data[source_offset as usize..end as usize])?;
                    }
                    _ => return Err(invalid(format!("stored row unavailable at {y},{z}"))),
                }
            }
        }
        Ok(())
    }
    pub fn write_brick_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        dst: Option<&mut dyn Write>,
        brick: super::volume_bricks::VolumeBrick,
        stored16: bool,
    ) -> Result<(), RenderError> {
        let Some(dst) = dst else {
            return Err(invalid("nil brick writer"));
        };
        let (d, payload) = self.record()?;
        for axis in 0..3 {
            let end = u64::from(brick.origin[axis]) + u64::from(brick.size[axis]);
            if brick.size[axis] == 0 || end > u64::from(d.dimensions[axis]) {
                return Err(super::volume_bricks::invalid_bricks(format!(
                    "brick {} outside the {} volume",
                    brick.index,
                    super::volume_bricks::dims_v(d.dimensions)
                )));
            }
        }
        let is_stored16 = d.scalar_format == VolumeScalarFormat::I16_STORED_LE
            || d.scalar_format == VolumeScalarFormat::U16_STORED_LE;
        if stored16 && !is_stored16 {
            return Err(invalid(format!(
                "{} is not a 16-bit stored format",
                d.scalar_format.name()
            )));
        }
        if !stored16 {
            validate_modality_f32_conversion(&d)?;
        }
        let Some(scalar_size) = d.scalar_format.scalar_size() else {
            return Err(invalid("unknown scalar format"));
        };
        let texel_bytes = if stored16 { 2 } else { 4 };
        let width = brick.size[0] as usize;
        let mut row = vec![0u8; width * texel_bytes];
        for z in brick.origin[2]..brick.origin[2] + brick.size[2] {
            for y in brick.origin[1]..brick.origin[1] + brick.size[1] {
                if let Some(e) = ctx_err(ctx) {
                    return Err(e);
                }
                let row_offset = u64::from(z)
                    .wrapping_mul(d.slice_stride_bytes)
                    .wrapping_add(u64::from(y).wrapping_mul(d.row_stride_bytes));
                for i in 0..width {
                    let x = u64::from(brick.origin[0]) + i as u64;
                    let offset = row_offset.wrapping_add(x.wrapping_mul(scalar_size));
                    if stored16 {
                        let Some(value) = payload.stored_sample16(offset) else {
                            return Err(invalid(format!(
                                "stored sample unavailable at {x},{y},{z}"
                            )));
                        };
                        row[2 * i..2 * i + 2].copy_from_slice(&value.to_le_bytes());
                        continue;
                    }
                    let Some(value) = payload.modality_at(offset, &d) else {
                        return Err(invalid(format!(
                            "modality sample unavailable at {x},{y},{z}"
                        )));
                    };
                    let value32 = value as f32;
                    if d.scalar_format != VolumeScalarFormat::F32_MODALITY_LE
                        && (!value.is_finite() || value32.is_infinite())
                    {
                        return Err(invalid(format!(
                            "modality sample out of finite float32 range at {x},{y},{z}"
                        )));
                    }
                    row[4 * i..4 * i + 4].copy_from_slice(&value32.to_bits().to_le_bytes());
                }
                write_exact(dst, &row)?;
            }
        }
        Ok(())
    }
}
impl Payload {
    fn stored_sample16(&self, offset: u64) -> Option<u16> {
        match self {
            Payload::Bytes(data) => {
                let end = checked_add(offset, 2)?;
                if end > data.len() as u64 {
                    return None;
                }
                let o = offset as usize;
                Some(u16::from_le_bytes([data[o], data[o + 1]]))
            }
            _ => {
                let p = self.packed16()?;
                if !offset.is_multiple_of(2) || offset / 2 >= p.count() as u64 {
                    return None;
                }
                Some(p.raw((offset / 2) as usize))
            }
        }
    }
}
fn component_modality_at(
    d: &VolumeDescriptor,
    payload: &Payload,
    x: u32,
    y: u32,
    z: u32,
    component: u32,
) -> Option<f64> {
    if x >= d.dimensions[0]
        || y >= d.dimensions[1]
        || z >= d.dimensions[2]
        || component >= d.components
    {
        return None;
    }
    let scalar_size = d.scalar_format.scalar_size()?;
    let z_offset = checked_mul(u64::from(z), d.slice_stride_bytes)?;
    let y_offset = checked_mul(u64::from(y), d.row_stride_bytes)?;
    let x_offset = checked_mul(
        u64::from(x) * u64::from(d.components) + u64::from(component),
        scalar_size,
    )?;
    let offset = checked_add(checked_add(z_offset, y_offset)?, x_offset)?;
    payload.modality_at(offset, d)
}
fn write_modality_direct(
    values: &[f32],
    ctx: Option<&dyn Cancellation>,
    dst: &mut dyn Write,
    d: &VolumeDescriptor,
) -> Option<Result<(), RenderError>> {
    if d.scalar_format != VolumeScalarFormat::F32_MODALITY_LE {
        return None;
    }
    let (rows, depth) = (u64::from(d.dimensions[1]), u64::from(d.dimensions[2]));
    let (row_stride, slice_stride) = (d.row_stride_bytes, d.slice_stride_bytes);
    let row_bytes = checked_mul(u64::from(d.dimensions[0]) * u64::from(d.components), 4)?;
    if row_bytes == 0 || rows == 0 || depth == 0 || row_stride % 4 != 0 || slice_stride % 4 != 0 {
        return None;
    }
    let last_slice = checked_mul(depth - 1, slice_stride)?;
    let last_row = checked_mul(rows - 1, row_stride)?;
    let end = checked_add(checked_add(last_slice, last_row)?, row_bytes)?;
    if end > values.len() as u64 * 4 || end > i64::MAX as u64 {
        return None;
    }
    let (run, run_stride, runs) = if row_stride == row_bytes {
        (row_bytes * rows, slice_stride, 1)
    } else {
        (row_bytes, row_stride, rows)
    };
    let mut bytes = Vec::with_capacity(run as usize);
    for z in 0..depth {
        for index in 0..runs {
            if let Some(e) = ctx_err(ctx) {
                return Some(Err(e));
            }
            let offset = (z * slice_stride + index * run_stride) / 4;
            bytes.clear();
            for v in &values[offset as usize..(offset + run / 4) as usize] {
                bytes.extend_from_slice(&v.to_bits().to_le_bytes());
            }
            if let Err(e) = write_exact(dst, &bytes) {
                return Some(Err(e));
            }
        }
    }
    Some(Ok(()))
}
pub fn validate_modality_f32_conversion(d: &VolumeDescriptor) -> Result<(), RenderError> {
    validate_volume_descriptor(d, false)?;
    let (minimum, maximum) = match d.scalar_format {
        VolumeScalarFormat::F32_MODALITY_LE => return Ok(()),
        VolumeScalarFormat::I16_STORED_LE => (f64::from(i16::MIN), f64::from(i16::MAX)),
        VolumeScalarFormat::U16_STORED_LE => (0.0, f64::from(u16::MAX)),
        _ => return Err(invalid("unknown scalar format")),
    };
    for stored in [minimum, maximum] {
        let value = stored * d.rescale_slope + d.rescale_intercept;
        if !value.is_finite() || !(value as f32).is_finite() {
            return Err(invalid(
                "stored-value rescale exceeds finite float32 modality range",
            ));
        }
    }
    Ok(())
}
#[doc(hidden)]
pub fn validate_descriptor(d: &VolumeDescriptor) -> Result<(), RenderError> {
    validate_volume_descriptor(d, false)
}
fn validate_volume_descriptor(
    d: &VolumeDescriptor,
    generation_pending: bool,
) -> Result<(), RenderError> {
    if d.contract_version != VOLUME_SNAPSHOT_CONTRACT_VERSION
        && d.contract_version != VOLUME_SNAPSHOT_CONTRACT_VERSION_V2
    {
        return Err(invalid(format!("contract version {}", d.contract_version)));
    }
    if d.header_size < VOLUME_SNAPSHOT_HEADER_SIZE_V1 {
        return Err(invalid(format!(
            "header size {} below V1 minimum {VOLUME_SNAPSHOT_HEADER_SIZE_V1}",
            d.header_size
        )));
    }
    if !generation_pending && d.volume_generation == 0 {
        return Err(invalid("zero volume generation"));
    }
    if d.derivation.0 > VolumeDerivation::VR_PREFILTERED.0 {
        return Err(invalid(format!("unknown derivation {}", d.derivation.0)));
    }
    if d.derivation == VolumeDerivation::REGULARIZED && d.parent_generation == 0 {
        return Err(invalid("regularized generation lacks parent"));
    }
    if d.derivation != VolumeDerivation::REGULARIZED && d.parent_generation != 0 {
        return Err(invalid(format!(
            "root generation has parent {}",
            d.parent_generation
        )));
    }
    if d.contract_version == VOLUME_SNAPSHOT_CONTRACT_VERSION && d.components != 1 {
        return Err(invalid(format!("V1 components {}, want 1", d.components)));
    }
    if d.contract_version == VOLUME_SNAPSHOT_CONTRACT_VERSION_V2
        && (d.components < 2
            || d.components > 4
            || d.scalar_format != VolumeScalarFormat::F32_MODALITY_LE)
    {
        return Err(invalid(
            "V2 requires two to four interleaved F32 modality components",
        ));
    }
    for (axis, dimension) in d.dimensions.iter().enumerate() {
        if *dimension == 0 {
            return Err(invalid(format!("zero dimension at axis {axis}")));
        }
    }
    let Some(scalar_size) = d.scalar_format.scalar_size() else {
        return Err(invalid(format!(
            "unknown scalar format {}",
            d.scalar_format.0
        )));
    };
    if d.scalar_format == VolumeScalarFormat::F32_MODALITY_LE {
        if d.sample_domain != VolumeSampleDomain::MODALITY
            || d.rescale_slope != 1.0
            || d.rescale_intercept != 0.0
        {
            return Err(invalid(
                "F32 modality format requires MODALITY domain and identity rescale",
            ));
        }
    } else if d.sample_domain != VolumeSampleDomain::STORED {
        return Err(invalid(format!(
            "{} requires STORED domain",
            d.scalar_format.name()
        )));
    }
    if !d.rescale_slope.is_finite() || !d.rescale_intercept.is_finite() {
        return Err(invalid("non-finite rescale"));
    }
    let Some(packed_row) = checked_mul(
        u64::from(d.dimensions[0]) * u64::from(d.components),
        scalar_size,
    ) else {
        return Err(invalid("packed row overflow"));
    };
    if d.row_stride_bytes < packed_row {
        return Err(invalid(format!(
            "row stride {} below packed row {packed_row}",
            d.row_stride_bytes
        )));
    }
    let Some(minimum_slice) = checked_mul(u64::from(d.dimensions[1]), d.row_stride_bytes) else {
        return Err(invalid("slice stride overflow"));
    };
    if d.slice_stride_bytes < minimum_slice {
        return Err(invalid(format!(
            "slice stride {} below rows*row_stride {minimum_slice}",
            d.slice_stride_bytes
        )));
    }
    let Some(last_slice) = checked_mul(u64::from(d.dimensions[2] - 1), d.slice_stride_bytes) else {
        return Err(invalid("byte length overflow at final slice"));
    };
    let Some(last_row) = checked_mul(u64::from(d.dimensions[1] - 1), d.row_stride_bytes) else {
        return Err(invalid("byte length overflow at final row"));
    };
    let Some(final_address) = checked_add(last_slice, last_row) else {
        return Err(invalid("byte length overflow"));
    };
    match checked_add(final_address, packed_row) {
        Some(end) if d.byte_length >= end => {}
        other => {
            return Err(invalid(format!(
                "byte length {} below final addressed byte {}",
                d.byte_length,
                other.unwrap_or(0)
            )));
        }
    }
    for (axis, spacing) in d.spacing_mm.iter().enumerate() {
        if !finite_positive_spacing(*spacing) {
            return Err(invalid(format!("invalid spacing at axis {axis}")));
        }
    }
    validate_affine_pair(&d.index_to_patient_lps, &d.patient_lps_to_index).map_err(invalid)?;
    const SPACING_TOLERANCE: f64 = 1e-9;
    let a = &d.index_to_patient_lps.0;
    for axis in 0..3 {
        let column_norm =
            crate::fmath::hypot(crate::fmath::hypot(a[axis], a[4 + axis]), a[8 + axis]);
        if !column_norm.is_finite() {
            return Err(invalid(format!(
                "non-finite affine column norm at axis {axis}"
            )));
        }
        let tolerance = SPACING_TOLERANCE * crate::fmath::max(1.0, d.spacing_mm[axis]);
        let difference = column_norm - d.spacing_mm[axis];
        if !difference.is_finite() || difference.abs() > tolerance {
            return Err(invalid(format!(
                "spacing axis {axis} is {} but affine column norm is {}",
                format_float(d.spacing_mm[axis], 'g', 12),
                format_float(column_norm, 'g', 12)
            )));
        }
    }
    Ok(())
}
fn validate_affine_pair(
    forward: &GeometryAffine,
    inverse: &GeometryAffine,
) -> Result<(), &'static str> {
    if !forward.finite() {
        return Err("index_to_patient_lps is non-finite or has an invalid final row");
    }
    if !inverse.finite() {
        return Err("patient_lps_to_index is non-finite or has an invalid final row");
    }
    const TOLERANCE: f64 = 1e-9;
    let (f, i) = (&forward.0, &inverse.0);
    for row in 0..4 {
        for col in 0..4 {
            let want = if row == col { 1.0 } else { 0.0 };
            let (mut a, mut b) = (0.0f64, 0.0f64);
            for k in 0..4 {
                let product_a = f[row * 4 + k] * i[k * 4 + col];
                let product_b = i[row * 4 + k] * f[k * 4 + col];
                if !product_a.is_finite() || !product_b.is_finite() {
                    return Err("affine product is non-finite");
                }
                a += product_a;
                b += product_b;
                if !a.is_finite() || !b.is_finite() {
                    return Err("affine product accumulator is non-finite");
                }
            }
            let (da, db) = (a - want, b - want);
            if !da.is_finite() || !db.is_finite() || da.abs() > TOLERANCE || db.abs() > TOLERANCE {
                return Err("affines are not mutual inverses within 1e-09");
            }
        }
    }
    Ok(())
}
