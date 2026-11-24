use super::affine::geometry_affine_pair;
use super::error::{ErrorKind, RenderError};
use super::parallel::{Cancellation, ctx_err, default_workers, run_background};
use super::plane_buffer::{PlaneBuffer, Samples};
use super::volume::{Payload, Stack, Volume, build_volume, checked_voxel_count};
use super::volume_plans::{VolumeRegularizationPlan, plan_volume_regularization};
use super::volume_store::{
    ReleaseFn, VOLUME_SNAPSHOT_CONTRACT_VERSION, VOLUME_SNAPSHOT_CONTRACT_VERSION_V2,
    VOLUME_SNAPSHOT_HEADER_SIZE_V1, VolumeDerivation, VolumeDescriptor, VolumeLease,
    VolumeMemoryKind, VolumeMemoryLease, VolumeResidencyKey, VolumeSampleDomain,
    VolumeScalarFormat, VolumeStore, VolumeStoreIdentity, VolumeStoreOptions, VolumeStoreStats,
    validate_descriptor,
};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
fn closed_err() -> RenderError {
    RenderError::new(ErrorKind::VolumeStoreClosed, "render: volume store closed")
}
fn not_found_err() -> RenderError {
    RenderError::new(
        ErrorKind::VolumeNotFound,
        "render: volume generation not found",
    )
}
fn invalid(detail: impl std::fmt::Display) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVolumeSnapshot,
        format!("render: invalid volume snapshot: {detail}"),
    )
}
fn wrap(prefix: &str, e: RenderError) -> RenderError {
    RenderError::new(e.kind(), format!("{prefix}: {e}"))
}
fn is_cancel(e: &RenderError) -> bool {
    e.is(ErrorKind::Canceled) || e.is(ErrorKind::DeadlineExceeded)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumePreparationStats {
    pub normalization_duration: Duration,
    pub canonicalization_duration: Duration,
    pub total_duration: Duration,
    pub voxel_bytes: u64,
    pub transient_bytes: u64,
    pub frames: i64,
    pub reused: bool,
}
#[derive(Default)]
struct LinkState {
    store: Option<Arc<VolumeStore>>,
    generation: u64,
    err: Option<RenderError>,
    stats: VolumePreparationStats,
    building: bool,
}
const CANCEL_POLL: Duration = Duration::from_millis(2);
#[derive(Default)]
pub(crate) struct VolumeLink {
    state: Mutex<LinkState>,
    built: Condvar,
    closed: AtomicBool,
    regular: OnceLock<Result<Arc<Volume>, RenderError>>,
}
impl std::fmt::Debug for VolumeLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.lock();
        f.debug_struct("VolumeLink")
            .field("generation", &s.generation)
            .field("closed", &self.closed())
            .finish()
    }
}
impl VolumeLink {
    fn lock(&self) -> MutexGuard<'_, LinkState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    fn store(&self) -> Arc<VolumeStore> {
        let mut s = self.lock();
        s.store
            .get_or_insert_with(|| VolumeStore::new(VolumeStoreOptions::default()))
            .clone()
    }
    pub(crate) fn wait_while_building(&self) {
        let mut s = self.lock();
        while s.building {
            s = self.built.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }
    fn wait_build<'a>(
        &'a self,
        s: MutexGuard<'a, LinkState>,
        ctx: Option<&dyn Cancellation>,
    ) -> MutexGuard<'a, LinkState> {
        match ctx {
            None => self.built.wait(s).unwrap_or_else(|e| e.into_inner()),
            Some(_) => match self.built.wait_timeout(s, CANCEL_POLL) {
                Ok((s, _)) => s,
                Err(e) => e.into_inner().0,
            },
        }
    }
}
struct BuildSlot<'a> {
    link: &'a VolumeLink,
    open: bool,
}
impl BuildSlot<'_> {
    fn end(mut self, s: &mut LinkState) {
        s.building = false;
        self.open = false;
        self.link.built.notify_all();
    }
}
impl Drop for BuildSlot<'_> {
    fn drop(&mut self) {
        if self.open {
            self.link.lock().building = false;
            self.link.built.notify_all();
        }
    }
}
pub fn build_volume_with_store(
    series: &Stack,
    store: Arc<VolumeStore>,
) -> Result<Arc<Volume>, RenderError> {
    let volume = build_volume(series)?;
    volume.link.lock().store = Some(store);
    Ok(volume)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumePreparationEstimate {
    pub scalar_format: VolumeScalarFormat,
    pub payload_bytes: u64,
    pub transient_bytes: u64,
    pub decode_workers: i64,
    pub requires_resampling: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AcquiredVolumeDescriptor {
    pub residency_key: VolumeResidencyKey,
    pub dimensions: [u32; 3],
    pub scalar_format: VolumeScalarFormat,
    pub sample_domain: VolumeSampleDomain,
    pub row_stride_bytes: u64,
    pub slice_stride_bytes: u64,
    pub byte_length: u64,
    pub rescale_slope: f64,
    pub rescale_intercept: f64,
}
struct Acquired {
    lease: VolumeLease,
    plan: VolumeRegularizationPlan,
}
pub struct AcquiredVolumeLease {
    inner: Arc<Acquired>,
}
#[derive(Clone)]
pub struct AcquiredVolumeSnapshot {
    inner: Arc<Acquired>,
}
impl AcquiredVolumeLease {
    pub fn release(&self) {
        self.inner.lease.release();
    }
    pub fn snapshot(&self) -> Result<AcquiredVolumeSnapshot, RenderError> {
        self.inner.lease.snapshot()?;
        Ok(AcquiredVolumeSnapshot {
            inner: Arc::clone(&self.inner),
        })
    }
}
impl AcquiredVolumeSnapshot {
    fn source(&self) -> Result<super::volume_store::VolumeSnapshot, RenderError> {
        self.inner.lease.snapshot()
    }
    pub fn descriptor(&self) -> Result<AcquiredVolumeDescriptor, RenderError> {
        let source = self.source()?;
        let d = source.descriptor()?;
        let key = source.residency_key();
        if !key.valid() {
            return Err(RenderError::new(
                ErrorKind::VolumeLeaseReleased,
                "render: volume lease released",
            ));
        }
        Ok(AcquiredVolumeDescriptor {
            residency_key: key,
            dimensions: d.dimensions,
            scalar_format: d.scalar_format,
            sample_domain: d.sample_domain,
            row_stride_bytes: d.row_stride_bytes,
            slice_stride_bytes: d.slice_stride_bytes,
            byte_length: d.byte_length,
            rescale_slope: d.rescale_slope,
            rescale_intercept: d.rescale_intercept,
        })
    }
    pub fn regularization_plan(&self) -> Result<VolumeRegularizationPlan, RenderError> {
        self.source()?;
        self.inner.plan.freeze()
    }
    pub fn residency_key(&self) -> VolumeResidencyKey {
        self.inner.lease.residency_key()
    }
    pub fn write_payload_to(&self, dst: &mut dyn Write) -> Result<(), RenderError> {
        self.source()?.write_payload_to(dst)
    }
    pub fn payload_bytes(
        &self,
    ) -> Result<Option<super::volume_store::VolumePayloadBytes>, RenderError> {
        self.source()?.payload_bytes()
    }
    pub fn write_modality_f32_context(
        &self,
        ctx: Option<&dyn Cancellation>,
        dst: &mut dyn Write,
    ) -> Result<(), RenderError> {
        self.source()?.write_modality_f32_context(ctx, dst)
    }
}
impl Volume {
    pub(crate) fn store_descriptor(
        &self,
        derivation: VolumeDerivation,
        parent: u64,
        source_geometry: bool,
    ) -> Result<VolumeDescriptor, RenderError> {
        let max = i64::from(u32::MAX);
        if self.cols <= 0
            || self.rows <= 0
            || self.depth <= 0
            || self.cols > max
            || self.rows > max
            || self.depth > max
        {
            return Err(invalid("invalid volume dimensions"));
        }
        let mut origin = self.origin;
        let mut z_step = self.normal.scale(self.slice_spacing);
        let mut slice_spacing = self.slice_spacing;
        let origins = self.slice_origins();
        if source_geometry && self.geometry().source_affine && origins.len() > 1 {
            origin = origins[0];
            z_step = origins[origins.len() - 1]
                .sub(origin)
                .scale(1.0 / (origins.len() - 1) as f64);
            slice_spacing = z_step.length();
        }
        let (index_to_patient, patient_to_index, ok) = geometry_affine_pair(
            origin,
            self.axis_x.scale(self.col_spacing),
            self.axis_y.scale(self.row_spacing),
            z_step,
        );
        if !ok {
            return Err(invalid("invalid patient affine"));
        }
        let components = self.components();
        let version = if components > 1 {
            VOLUME_SNAPSHOT_CONTRACT_VERSION_V2
        } else {
            VOLUME_SNAPSHOT_CONTRACT_VERSION
        };
        let row_stride = self.cols as u64 * u64::from(components) * 4;
        let slice_stride = self.rows as u64 * row_stride;
        Ok(VolumeDescriptor {
            contract_version: version,
            header_size: VOLUME_SNAPSHOT_HEADER_SIZE_V1,
            volume_generation: 0,
            parent_generation: parent,
            derivation,
            dimensions: [self.cols as u32, self.rows as u32, self.depth as u32],
            components,
            scalar_format: VolumeScalarFormat::F32_MODALITY_LE,
            sample_domain: VolumeSampleDomain::MODALITY,
            row_stride_bytes: row_stride,
            slice_stride_bytes: slice_stride,
            byte_length: self.depth as u64 * slice_stride,
            rescale_slope: 1.0,
            rescale_intercept: 0.0,
            spacing_mm: [self.col_spacing, self.row_spacing, slice_spacing],
            index_to_patient_lps: index_to_patient,
            patient_lps_to_index: patient_to_index,
        })
    }
    fn payload_descriptor(&self, payload: &Payload) -> Result<VolumeDescriptor, RenderError> {
        let mut d = self.store_descriptor(VolumeDerivation::NORMALIZED, 0, false)?;
        if let Payload::I16 {
            slope, intercept, ..
        }
        | Payload::U16 {
            slope, intercept, ..
        } = payload
        {
            d.scalar_format = if matches!(payload, Payload::I16 { .. }) {
                VolumeScalarFormat::I16_STORED_LE
            } else {
                VolumeScalarFormat::U16_STORED_LE
            };
            d.sample_domain = VolumeSampleDomain::STORED;
            d.rescale_slope = *slope;
            d.rescale_intercept = *intercept;
            d.row_stride_bytes = self.cols as u64 * 2;
            d.slice_stride_bytes = self.rows as u64 * d.row_stride_bytes;
            d.byte_length = payload_bytes(payload);
        }
        Ok(d)
    }
    fn link_regular_grid(self: &Arc<Self>) -> Result<Arc<Volume>, RenderError> {
        if self.link.closed() {
            return Err(closed_err());
        }
        if !self.geometry().requires_resampling {
            return Ok(Arc::clone(self));
        }
        self.link
            .regular
            .get_or_init(|| {
                if self.link.closed() {
                    return Err(closed_err());
                }
                let grid = self.regular_grid()?;
                self.install_regular(&grid)?;
                Ok(grid)
            })
            .clone()
    }
    fn install_regular(&self, grid: &Arc<Volume>) -> Result<(), RenderError> {
        let store = self.link.store();
        grid.link.lock().store = Some(Arc::clone(&store));
        let source_descriptor = grid
            .store_descriptor(VolumeDerivation::NORMALIZED, 0, true)
            .map_err(|e| wrap("render: describe source generation", e))?;
        let payload = grid.payload()?;
        let bytes = payload_bytes(&payload);
        let source_generation = store
            .replace_shared(source_descriptor, Arc::clone(&payload))
            .map_err(|e| wrap("render: install source generation", e))?;
        {
            let mut s = self.link.lock();
            s.generation = source_generation;
            s.stats = VolumePreparationStats {
                voxel_bytes: bytes,
                frames: grid.depth,
                ..VolumePreparationStats::default()
            };
        }
        let parent_lease = store
            .acquire(source_generation)
            .map_err(|e| wrap("render: lease source generation", e))?;
        let descriptor =
            match grid.store_descriptor(VolumeDerivation::REGULARIZED, source_generation, false) {
                Ok(d) => d,
                Err(e) => {
                    parent_lease.release();
                    return Err(e);
                }
            };
        let installed = store.replace_regularized_shared(source_generation, descriptor, payload);
        parent_lease.release();
        let target = installed?;
        let mut s = grid.link.lock();
        s.generation = target;
        s.stats = VolumePreparationStats {
            voxel_bytes: bytes,
            frames: grid.depth,
            ..VolumePreparationStats::default()
        };
        Ok(())
    }
    fn prepare_direct(
        &self,
        ctx: Option<&dyn Cancellation>,
        observe: Option<&mut dyn FnMut(&VolumeDecodeProgress<'_>)>,
        options: DecodePlan,
    ) -> (VolumePreparationStats, Option<RenderError>) {
        let slot = {
            let mut s = self.link.lock();
            loop {
                if let Some(e) = ctx_err(ctx) {
                    return (VolumePreparationStats::default(), Some(e));
                }
                if self.link.closed() {
                    return (VolumePreparationStats::default(), Some(closed_err()));
                }
                if s.generation != 0 && s.err.is_none() {
                    let stats = VolumePreparationStats {
                        voxel_bytes: s.stats.voxel_bytes,
                        transient_bytes: s.stats.transient_bytes,
                        frames: s.stats.frames,
                        reused: true,
                        ..VolumePreparationStats::default()
                    };
                    return (stats, None);
                }
                if let Some(e) = &s.err {
                    return (VolumePreparationStats::default(), Some(e.clone()));
                }
                if !s.building {
                    break;
                }
                s = self.link.wait_build(s, ctx);
            }
            s.building = true;
            BuildSlot {
                link: &self.link,
                open: true,
            }
        };
        let (stats, result) = self.build_direct(ctx, observe, options);
        let mut s = self.link.lock();
        let result = if self.link.closed() {
            Err(closed_err())
        } else {
            result
        };
        let out = match result {
            Ok(generation) => {
                s.generation = generation;
                s.stats = stats;
                (stats, None)
            }
            Err(e) => {
                if !is_cancel(&e) && ctx_err(ctx).is_none() {
                    s.err = Some(e.clone());
                }
                (stats, Some(e))
            }
        };
        slot.end(&mut s);
        out
    }
    fn build_direct(
        &self,
        ctx: Option<&dyn Cancellation>,
        observe: Option<&mut dyn FnMut(&VolumeDecodeProgress<'_>)>,
        options: DecodePlan,
    ) -> (VolumePreparationStats, Result<u64, RenderError>) {
        let started = Instant::now();
        let mut stats = VolumePreparationStats {
            frames: self.depth,
            ..VolumePreparationStats::default()
        };
        let layout = self.packed16_layout();
        let mut progressive = false;
        let decoded = match (self.cached_payload(), observe) {
            (Some(cached), _) => cached,
            (None, Some(observe)) if layout.is_some() || self.components() == 1 => {
                progressive = true;
                self.decode_progressive(ctx, observe, options, layout)
            }
            _ => self.payload_now(),
        };
        stats.normalization_duration = started.elapsed();
        let payload = match decoded {
            Ok(p) => p,
            Err(e) => {
                stats.total_duration = started.elapsed();
                if layout.is_some()
                    && e.is(ErrorKind::InvalidVolumeSnapshot)
                    && e.message().contains("stored-value rescale")
                {
                    stats.voxel_bytes = checked_voxel_count(self.cols, self.rows, self.depth)
                        .map_or(0, |n| n as u64 * 2);
                }
                return (stats, Err(e));
            }
        };
        stats.voxel_bytes = payload_bytes(&payload);
        if let Some(e) = ctx_err(ctx) {
            stats.total_duration = started.elapsed();
            return (stats, Err(e));
        }
        let canonical_started = Instant::now();
        let descriptor = match self.payload_descriptor(&payload) {
            Ok(d) => d,
            Err(e) => {
                stats.canonicalization_duration = canonical_started.elapsed();
                stats.total_duration = started.elapsed();
                return (stats, Err(e));
            }
        };
        let store = self.link.store();
        let installed = store.replace_shared(descriptor, Arc::clone(&payload));
        stats.canonicalization_duration = canonical_started.elapsed();
        stats.total_duration = started.elapsed();
        if progressive && installed.is_ok() {
            self.set_payload(payload);
        }
        (stats, installed)
    }
    fn acquire_direct(&self, ctx: Option<&dyn Cancellation>) -> Result<VolumeLease, RenderError> {
        if let (_, Some(e)) = self.prepare_direct(ctx, None, DecodePlan::default()) {
            return Err(e);
        }
        let (store, generation) = {
            let s = self.link.lock();
            (s.store.clone(), s.generation)
        };
        match store {
            Some(store) if generation != 0 => store.acquire(generation),
            _ => Err(not_found_err()),
        }
    }
    pub fn acquire_store_snapshot_context(
        self: &Arc<Self>,
        ctx: Option<&dyn Cancellation>,
    ) -> Result<VolumeLease, RenderError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        if self.geometry().requires_resampling {
            return self.link_regular_grid()?.acquire_direct(ctx);
        }
        self.acquire_direct(ctx)
    }
    pub fn acquire_source_snapshot_context(
        &self,
        ctx: Option<&dyn Cancellation>,
    ) -> Result<VolumeLease, RenderError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        if !self.geometry().source_affine {
            return Err(invalid(
                "source geometry is not representable by one affine",
            ));
        }
        let lease = self.acquire_direct(ctx)?;
        let mut descriptor = match self.store_descriptor(VolumeDerivation::NORMALIZED, 0, true) {
            Ok(d) => d,
            Err(e) => {
                lease.release();
                return Err(e);
            }
        };
        descriptor.volume_generation = lease.generation();
        lease.set_descriptor_override(descriptor);
        Ok(lease)
    }
    pub fn prepare_snapshot_context(
        self: &Arc<Self>,
        ctx: Option<&dyn Cancellation>,
    ) -> (VolumePreparationStats, Option<RenderError>) {
        let started = Instant::now();
        let target = if self.geometry().requires_resampling {
            if let Some(e) = ctx_err(ctx) {
                return (VolumePreparationStats::default(), Some(e));
            }
            match self.link_regular_grid() {
                Ok(grid) => grid,
                Err(e) => return (VolumePreparationStats::default(), Some(e)),
            }
        } else {
            Arc::clone(self)
        };
        let (mut stats, err) = target.prepare_direct(ctx, None, DecodePlan::default());
        stats.total_duration = started.elapsed();
        (stats, err)
    }
    pub fn volume_generation(self: &Arc<Self>) -> u64 {
        let target = match self.link_regular_grid() {
            Ok(t) => t,
            Err(_) => return 0,
        };
        match target.acquire_direct(None) {
            Ok(lease) => {
                let generation = lease.generation();
                lease.release();
                generation
            }
            Err(_) => 0,
        }
    }
    pub fn volume_store_identity(self: &Arc<Self>) -> VolumeStoreIdentity {
        if self.geometry().requires_resampling {
            return match self.link_regular_grid() {
                Ok(grid) => grid.link.store().identity(),
                Err(_) => VolumeStoreIdentity::default(),
            };
        }
        self.link.store().identity()
    }
    pub fn volume_store_stats(&self) -> VolumeStoreStats {
        self.link.store().stats()
    }
    pub fn track_memory(
        self: &Arc<Self>,
        kind: VolumeMemoryKind,
        byte_count: u64,
        release: Option<ReleaseFn>,
    ) -> Result<VolumeMemoryLease, RenderError> {
        let lease = self.acquire_store_snapshot_context(None)?;
        let generation = lease.generation();
        lease.release();
        let grid = self.link_regular_grid()?;
        grid.link
            .store()
            .track_memory(generation, kind, byte_count, release)
    }
    pub fn close(&self) {
        if self.link.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.link.lock().err = Some(closed_err());
        if let Some(Ok(grid)) = self.link.regular.get()
            && !std::ptr::eq(Arc::as_ptr(grid), self)
        {
            grid.close();
        }
        self.link.store().close();
    }
    pub fn on_snapshot_release(&self, release: ReleaseFn) -> Result<(), RenderError> {
        let (store, generation) = {
            let s = self.link.lock();
            (s.store.clone(), s.generation)
        };
        match store {
            Some(store) if generation != 0 && store.add_release_observer(generation, release) => {
                Ok(())
            }
            _ => Err(closed_err()),
        }
    }
    pub fn estimate_direct_preparation(&self) -> Result<VolumePreparationEstimate, RenderError> {
        if self.link.closed() {
            return Err(closed_err());
        }
        let Some(count) = checked_voxel_count(self.cols, self.rows, self.depth) else {
            return Err(RenderError::other("render: volume dimensions overflow"));
        };
        let (scalar_size, format) = match self.packed16_layout() {
            Some((_, _, true)) => (2u64, VolumeScalarFormat::I16_STORED_LE),
            Some((_, _, false)) => (2u64, VolumeScalarFormat::U16_STORED_LE),
            None => (4u64, VolumeScalarFormat::F32_MODALITY_LE),
        };
        let Some(payload) = (count as u64).checked_mul(scalar_size) else {
            return Err(RenderError::other("render: payload estimate overflow"));
        };
        let mut workers = self.depth.min(default_workers());
        if count < 64 << 10 {
            workers = 1;
        }
        let Some(plane) = (self.cols as u64).checked_mul(self.rows as u64) else {
            return Err(RenderError::other("render: plane estimate overflow"));
        };
        let transient = plane.checked_mul(workers.max(1) as u64 * 8);
        let Some(transient) =
            transient.filter(|t| payload <= i64::MAX as u64 && *t <= i64::MAX as u64 - payload)
        else {
            return Err(RenderError::other("render: preparation estimate overflow"));
        };
        Ok(VolumePreparationEstimate {
            scalar_format: format,
            payload_bytes: payload,
            transient_bytes: transient,
            decode_workers: workers,
            requires_resampling: self.geometry().requires_resampling,
        })
    }
    pub fn acquire_acquired_snapshot_context(
        &self,
        ctx: Option<&dyn Cancellation>,
    ) -> Result<AcquiredVolumeLease, RenderError> {
        if self.link.closed() {
            return Err(closed_err());
        }
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let plan = plan_volume_regularization(self.geometry())?;
        let lease = self.acquire_direct(ctx)?;
        if let Some(e) = ctx_err(ctx) {
            lease.release();
            return Err(e);
        }
        Ok(AcquiredVolumeLease {
            inner: Arc::new(Acquired { lease, plan }),
        })
    }
    pub fn acquired_residency_key(&self) -> VolumeResidencyKey {
        if self.link.closed() {
            return VolumeResidencyKey::default();
        }
        let s = self.link.lock();
        match &s.store {
            Some(store) if s.generation != 0 => store.identity().residency_key(s.generation),
            _ => VolumeResidencyKey::default(),
        }
    }
    pub fn plan_acquired_regularization(&self) -> Result<VolumeRegularizationPlan, RenderError> {
        if self.link.closed() {
            return Err(closed_err());
        }
        plan_volume_regularization(self.geometry())
    }
    pub fn snapshot_ready(&self) -> bool {
        if self.link.closed() {
            return false;
        }
        if self.geometry().requires_resampling {
            return match self.link.regular.get() {
                Some(Ok(grid)) => {
                    let s = grid.link.lock();
                    s.generation != 0 && s.err.is_none()
                }
                _ => false,
            };
        }
        let s = self.link.lock();
        s.generation != 0 && s.err.is_none()
    }
    pub fn snapshot_building(&self) -> bool {
        !self.link.closed() && self.link.lock().building
    }
    pub fn recommended_window_if_ready(self: &Arc<Self>) -> Option<super::window::WindowLevel> {
        if let Some(window) = self.first_slice_default_window()
            && window.width > 0.0
        {
            return Some(window);
        }
        if self.hu_range.get().is_some() || self.snapshot_ready() {
            return Some(self.recommended_window());
        }
        None
    }
    pub fn prepare_snapshot_progressive_context(
        self: &Arc<Self>,
        ctx: Option<&dyn Cancellation>,
        observe: Option<&mut dyn FnMut(&VolumeDecodeProgress<'_>)>,
    ) -> (VolumePreparationStats, Option<RenderError>) {
        self.prepare_snapshot_progressive_with_options_context(
            ctx,
            VolumeDecodeOptions::default(),
            observe,
        )
    }
    pub fn prepare_snapshot_progressive_with_options_context(
        self: &Arc<Self>,
        ctx: Option<&dyn Cancellation>,
        options: VolumeDecodeOptions,
        observe: Option<&mut dyn FnMut(&VolumeDecodeProgress<'_>)>,
    ) -> (VolumePreparationStats, Option<RenderError>) {
        let Some(observe) = observe else {
            return self.prepare_snapshot_context(ctx);
        };
        let (stats, err) = if self.geometry().requires_resampling {
            self.prepare_snapshot_context(ctx)
        } else {
            self.prepare_direct(ctx, Some(&mut *observe), options.into())
        };
        if err.is_some() {
            return (stats, err);
        }
        (stats, self.report_complete(observe).err())
    }
    pub fn prepare_snapshot_bands_in_order_context(
        self: &Arc<Self>,
        ctx: Option<&dyn Cancellation>,
        observe: &mut dyn FnMut(&VolumeDecodeProgress<'_>),
    ) -> (VolumePreparationStats, Option<RenderError>) {
        let (stats, err) = if self.geometry().requires_resampling {
            self.prepare_snapshot_context(ctx)
        } else {
            let plan = DecodePlan {
                skip_preview: true,
                report_even_slices: false,
                sequential: true,
                stream_evens: false,
            };
            self.prepare_direct(ctx, Some(&mut *observe), plan)
        };
        if err.is_some() {
            return (stats, err);
        }
        (stats, self.report_complete(observe).err())
    }
    pub fn prepare_snapshot_streaming_evens_context(
        self: &Arc<Self>,
        ctx: Option<&dyn Cancellation>,
        observe: &mut dyn FnMut(&VolumeDecodeProgress<'_>),
    ) -> (VolumePreparationStats, Option<RenderError>) {
        let (stats, err) = if self.geometry().requires_resampling {
            self.prepare_snapshot_context(ctx)
        } else {
            let plan = DecodePlan {
                skip_preview: true,
                report_even_slices: true,
                sequential: false,
                stream_evens: true,
            };
            self.prepare_direct(ctx, Some(&mut *observe), plan)
        };
        if err.is_some() {
            return (stats, err);
        }
        (stats, self.report_complete(observe).err())
    }
    fn report_complete(
        self: &Arc<Self>,
        observe: &mut dyn FnMut(&VolumeDecodeProgress<'_>),
    ) -> Result<(), RenderError> {
        let lease = self.acquire_store_snapshot_context(None)?;
        let descriptor = lease.snapshot().and_then(|s| s.descriptor());
        lease.release();
        let descriptor = descriptor?;
        let depth = i64::from(descriptor.dimensions[2]);
        observe(&VolumeDecodeProgress {
            phase: VolumeDecodePhase::COMPLETE,
            decoded_slices: depth,
            total_slices: depth,
            descriptor,
            ..VolumeDecodeProgress::default()
        });
        Ok(())
    }
    #[allow(unsafe_code)]
    fn decode_progressive(
        &self,
        ctx: Option<&dyn Cancellation>,
        observe: &mut dyn FnMut(&VolumeDecodeProgress<'_>),
        options: DecodePlan,
        layout: Option<(f64, f64, bool)>,
    ) -> Result<Arc<Payload>, RenderError> {
        let Some(count) = checked_voxel_count(self.cols, self.rows, self.depth) else {
            return Err(invalid("volume dimensions overflow"));
        };
        let mut descriptor = self.store_descriptor(VolumeDerivation::NORMALIZED, 0, false)?;
        if let Some((slope, intercept, signed)) = layout {
            descriptor.scalar_format = if signed {
                VolumeScalarFormat::I16_STORED_LE
            } else {
                VolumeScalarFormat::U16_STORED_LE
            };
            descriptor.sample_domain = VolumeSampleDomain::STORED;
            descriptor.rescale_slope = slope;
            descriptor.rescale_intercept = intercept;
            descriptor.row_stride_bytes = self.cols as u64 * 2;
            descriptor.slice_stride_bytes = self.rows as u64 * descriptor.row_stride_bytes;
            descriptor.byte_length = count as u64 * 2;
        }
        let depth = self.depth as usize;
        let plane = (self.cols * self.rows) as usize;
        let buffer = match layout {
            Some((_, _, true)) => BandData::SharedI16(PlaneBuffer::new(vec![0i16; count])),
            Some((_, _, false)) => BandData::SharedU16(PlaneBuffer::new(vec![0u16; count])),
            None => BandData::SharedF32(PlaneBuffer::new(vec![0f32; count])),
        };
        let mut state = ProgressState {
            ready: vec![false; depth],
            decoded: 0,
            prefix: 0,
            reported: 0,
        };
        let order: Vec<usize> = if options.sequential {
            (0..depth).collect()
        } else {
            (0..depth).step_by(2).chain((1..depth).step_by(2)).collect()
        };
        let evens = depth.div_ceil(2);
        let batch = (2 * default_workers()).max(1) as usize;
        let mut batch_end = |start: usize| {
            let mut end = order.len().min(start + batch);
            if options.sequential && start == 0 {
                end = order.len().min((batch / 2).max(PROGRESSIVE_BAND_SLICES));
            } else if !options.sequential && start < evens {
                end = end.min(evens);
            }
            end
        };
        let mut decoded = |start: usize, end: usize| -> Result<(), RenderError> {
            let part = &order[start..end];
            if options.stream_evens
                && !options.sequential
                && let (Some(&first), Some(&last)) = (part.first(), part.last())
                && first % 2 == 0
                && last % 2 == 0
                && part.len() == (last - first) / 2 + 1
            {
                observe(&VolumeDecodeProgress {
                    phase: VolumeDecodePhase::EVEN_BAND,
                    decoded_slices: (state.decoded + part.len()) as i64,
                    total_slices: depth as i64,
                    descriptor,
                    band_start: first as i64,
                    band_end: last as i64 + 1,
                    even_slices: Some(VolumeBandReader {
                        cols: self.cols,
                        rows: self.rows,
                        start: first as i64,
                        end: last as i64 + 1,
                        step: 2,
                        descriptor,
                        data: buffer.view(),
                        compact: false,
                        owner: Some(&buffer),
                    }),
                    ..VolumeDecodeProgress::default()
                });
            }
            for &index in part {
                if let Some(e) = ctx_err(ctx) {
                    return Err(e);
                }
                self.slice_decoded(index, &mut state, &buffer, &descriptor, options, observe)?;
            }
            Ok(())
        };
        match &buffer {
            BandData::SharedI16(v) => unsafe {
                self.decode_batches_shared(
                    ctx,
                    v,
                    plane,
                    &order,
                    &|index, dst| self.decode_stored_plane(index, dst),
                    &mut batch_end,
                    &mut decoded,
                )?
            },
            BandData::SharedU16(v) => unsafe {
                self.decode_batches_shared(
                    ctx,
                    v,
                    plane,
                    &order,
                    &|index, dst| self.decode_stored_plane(index, dst),
                    &mut batch_end,
                    &mut decoded,
                )?
            },
            BandData::SharedF32(v) => unsafe {
                self.decode_batches_shared(
                    ctx,
                    v,
                    plane,
                    &order,
                    &|index, dst| self.decode_f32_plane(index, dst),
                    &mut batch_end,
                    &mut decoded,
                )?
            },
            _ => return Err(invalid("decode buffer is not shared")),
        }
        if let Some(layout) = layout {
            Volume::validate_packed16_layout(layout)?;
        }
        let (slope, intercept, _) = layout.unwrap_or((1.0, 0.0, false));
        Ok(Arc::new(match buffer {
            BandData::F32(v) => Payload::F32(v.into()),
            BandData::SharedF32(data) => Payload::F32(Samples::finished(data)),
            BandData::I16(data) => Payload::I16 {
                data: data.into(),
                slope,
                intercept,
            },
            BandData::U16(data) => Payload::U16 {
                data: data.into(),
                slope,
                intercept,
            },
            BandData::SharedI16(data) => Payload::I16 {
                data: Samples::finished(data),
                slope,
                intercept,
            },
            BandData::SharedU16(data) => Payload::U16 {
                data: Samples::finished(data),
                slope,
                intercept,
            },
        }))
    }
    fn slice_decoded(
        &self,
        z: usize,
        state: &mut ProgressState,
        buffer: &BandData,
        descriptor: &VolumeDescriptor,
        options: DecodePlan,
        observe: &mut dyn FnMut(&VolumeDecodeProgress<'_>),
    ) -> Result<(), RenderError> {
        state.ready[z] = true;
        state.decoded += 1;
        let depth = self.depth as usize;
        let reader = |start: usize, end: usize, step: usize| VolumeBandReader {
            cols: self.cols,
            rows: self.rows,
            start: start as i64,
            end: end as i64,
            step: step as i64,
            descriptor: *descriptor,
            data: buffer.view(),
            compact: false,
            owner: Some(buffer),
        };
        let halfway = !options.sequential && state.decoded == depth.div_ceil(2);
        if options.report_even_slices && halfway {
            observe(&VolumeDecodeProgress {
                phase: VolumeDecodePhase::EVEN_SLICES,
                decoded_slices: state.decoded as i64,
                total_slices: depth as i64,
                descriptor: *descriptor,
                even_slices: Some(reader(0, depth, 2)),
                ..VolumeDecodeProgress::default()
            });
        }
        if !options.skip_preview && halfway {
            let preview = self.progressive_preview(buffer, descriptor)?;
            observe(&VolumeDecodeProgress {
                phase: VolumeDecodePhase::PREVIEW,
                decoded_slices: state.decoded as i64,
                total_slices: depth as i64,
                descriptor: *descriptor,
                preview: Some(preview),
                ..VolumeDecodeProgress::default()
            });
        }
        while state.prefix < depth && state.ready[state.prefix] {
            state.prefix += 1;
        }
        if state.prefix - state.reported >= PROGRESSIVE_BAND_SLICES
            || (state.prefix == depth && state.reported < depth)
        {
            observe(&VolumeDecodeProgress {
                phase: VolumeDecodePhase::BAND,
                decoded_slices: state.decoded as i64,
                total_slices: depth as i64,
                descriptor: *descriptor,
                band_start: state.reported as i64,
                band_end: state.prefix as i64,
                band: Some(reader(state.reported, state.prefix, 0)),
                ..VolumeDecodeProgress::default()
            });
            state.reported = state.prefix;
        }
        Ok(())
    }
    fn progressive_preview(
        &self,
        buffer: &BandData,
        descriptor: &VolumeDescriptor,
    ) -> Result<Arc<Volume>, RenderError> {
        let (fx, fy) = (self.cols.min(2), self.rows.min(2));
        let (cols, rows) = ((self.cols / fx).max(1), (self.rows / fy).max(1));
        let depth = (self.depth + 1) / 2;
        let Some(count) = checked_voxel_count(cols, rows, depth) else {
            return Err(RenderError::other("render: preview dimensions overflow"));
        };
        let view = buffer.view();
        let mut values = vec![0f32; count];
        let block = (fx * fy) as f64;
        let (scols, srows) = (self.cols as usize, self.rows as usize);
        for z in 0..depth as usize {
            let source_slice = 2 * z * srows * scols;
            for y in 0..rows as usize {
                for x in 0..cols as usize {
                    let mut sum = 0.0;
                    for dy in 0..fy as usize {
                        for dx in 0..fx as usize {
                            let index = source_slice
                                + (y * fy as usize + dy) * scols
                                + x * fx as usize
                                + dx;
                            sum += view.modality(index, descriptor);
                        }
                    }
                    values[(z * rows as usize + y) * cols as usize + x] = (sum / block) as f32;
                }
            }
        }
        let slice_spacing = if depth == 1 {
            self.slice_spacing
        } else {
            self.slice_spacing * 2.0
        };
        let origin = self
            .origin
            .add(self.axis_x.scale((fx - 1) as f64 / 2.0 * self.col_spacing))
            .add(self.axis_y.scale((fy - 1) as f64 / 2.0 * self.row_spacing));
        let bytes = values.len() as u64 * 4;
        let preview = Arc::new(Volume::derived_f32(
            self,
            cols,
            rows,
            depth,
            origin,
            self.col_spacing * fx as f64,
            self.row_spacing * fy as f64,
            slice_spacing,
            values,
        ));
        let store = VolumeStore::new(VolumeStoreOptions::default());
        preview.link.lock().store = Some(Arc::clone(&store));
        let installed = preview
            .store_descriptor(VolumeDerivation::NORMALIZED, 0, false)
            .and_then(|d| store.replace_shared(d, preview.payload()?));
        match installed {
            Ok(generation) => {
                let mut s = preview.link.lock();
                s.generation = generation;
                s.stats = VolumePreparationStats {
                    voxel_bytes: bytes,
                    frames: depth,
                    ..VolumePreparationStats::default()
                };
            }
            Err(e) => {
                preview.close();
                return Err(e);
            }
        }
        Ok(preview)
    }
}
fn payload_bytes(payload: &Payload) -> u64 {
    match payload {
        Payload::F32(v) | Payload::Rgb(v) => v.len() as u64 * 4,
        Payload::I16 { data, .. } => data.len() as u64 * 2,
        Payload::U16 { data, .. } => data.len() as u64 * 2,
    }
}
const PROGRESSIVE_BAND_SLICES: usize = 8;
const PARALLEL_RETAIN_SAMPLES: usize = 8 << 20;
struct ProgressState {
    ready: Vec<bool>,
    decoded: usize,
    prefix: usize,
    reported: usize,
}
enum BandData {
    F32(Vec<f32>),
    I16(Vec<i16>),
    U16(Vec<u16>),
    SharedI16(Arc<PlaneBuffer<i16>>),
    SharedU16(Arc<PlaneBuffer<u16>>),
    SharedF32(Arc<PlaneBuffer<f32>>),
}
#[allow(unsafe_code)]
impl BandData {
    fn view(&self) -> BandView<'_> {
        match self {
            BandData::F32(v) => BandView::F32(v),
            BandData::I16(v) => BandView::I16(v),
            BandData::U16(v) => BandView::U16(v),
            BandData::SharedI16(v) => BandView::I16(unsafe { v.range(0, v.len()) }),
            BandData::SharedU16(v) => BandView::U16(unsafe { v.range(0, v.len()) }),
            BandData::SharedF32(v) => BandView::F32(unsafe { v.range(0, v.len()) }),
        }
    }
    fn plane_view(&self, at: usize, count: usize) -> BandView<'_> {
        match self {
            BandData::SharedI16(v) => BandView::I16(unsafe { v.range(at, count) }),
            BandData::SharedU16(v) => BandView::U16(unsafe { v.range(at, count) }),
            BandData::SharedF32(v) => BandView::F32(unsafe { v.range(at, count) }),
            other => other.view().range(at, at + count),
        }
    }
    fn shared_clone(&self) -> Option<BandData> {
        match self {
            BandData::SharedI16(v) => Some(BandData::SharedI16(v.clone())),
            BandData::SharedU16(v) => Some(BandData::SharedU16(v.clone())),
            BandData::SharedF32(v) => Some(BandData::SharedF32(v.clone())),
            _ => None,
        }
    }
}
#[derive(Clone, Copy)]
enum BandView<'a> {
    F32(&'a [f32]),
    I16(&'a [i16]),
    U16(&'a [u16]),
}
impl<'a> BandView<'a> {
    fn len(&self) -> usize {
        match self {
            BandView::F32(v) => v.len(),
            BandView::I16(v) => v.len(),
            BandView::U16(v) => v.len(),
        }
    }
    fn stored(&self) -> bool {
        !matches!(self, BandView::F32(_))
    }
    fn modality(&self, index: usize, d: &VolumeDescriptor) -> f64 {
        match self {
            BandView::F32(v) => f64::from(v[index]),
            BandView::I16(v) => f64::from(v[index]) * d.rescale_slope + d.rescale_intercept,
            BandView::U16(v) => f64::from(v[index]) * d.rescale_slope + d.rescale_intercept,
        }
    }
    fn range(&self, from: usize, to: usize) -> BandView<'a> {
        match self {
            BandView::F32(v) => BandView::F32(&v[from..to]),
            BandView::I16(v) => BandView::I16(&v[from..to]),
            BandView::U16(v) => BandView::U16(&v[from..to]),
        }
    }
    fn append_modality_f32_le(&self, d: &VolumeDescriptor, out: &mut Vec<u8>) {
        let at = out.len();
        out.resize(at + self.len() * 4, 0);
        let bytes = out[at..].as_chunks_mut::<4>().0.iter_mut();
        match self {
            BandView::F32(v) => {
                for (o, value) in bytes.zip(v.iter()) {
                    o.copy_from_slice(&value.to_bits().to_le_bytes());
                }
            }
            _ => {
                for (index, o) in bytes.enumerate() {
                    let value = self.modality(index, d) as f32;
                    o.copy_from_slice(&value.to_bits().to_le_bytes());
                }
            }
        }
    }
    fn append_stored16_le(&self, out: &mut Vec<u8>) {
        let at = out.len();
        match self {
            BandView::F32(_) => {}
            BandView::I16(v) => {
                out.resize(at + v.len() * 2, 0);
                for (o, value) in out[at..].as_chunks_mut::<2>().0.iter_mut().zip(v.iter()) {
                    o.copy_from_slice(&value.to_le_bytes());
                }
            }
            BandView::U16(v) => {
                out.resize(at + v.len() * 2, 0);
                for (o, value) in out[at..].as_chunks_mut::<2>().0.iter_mut().zip(v.iter()) {
                    o.copy_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeDecodePhase(pub u8);
impl VolumeDecodePhase {
    pub const PREVIEW: VolumeDecodePhase = VolumeDecodePhase(1);
    pub const BAND: VolumeDecodePhase = VolumeDecodePhase(2);
    pub const COMPLETE: VolumeDecodePhase = VolumeDecodePhase(3);
    pub const EVEN_SLICES: VolumeDecodePhase = VolumeDecodePhase(4);
    pub const EVEN_BAND: VolumeDecodePhase = VolumeDecodePhase(5);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VolumeDecodeOptions {
    pub skip_preview: bool,
    pub report_even_slices: bool,
}
#[derive(Debug, Clone, Copy, Default)]
struct DecodePlan {
    skip_preview: bool,
    report_even_slices: bool,
    sequential: bool,
    stream_evens: bool,
}
impl From<VolumeDecodeOptions> for DecodePlan {
    fn from(options: VolumeDecodeOptions) -> DecodePlan {
        DecodePlan {
            skip_preview: options.skip_preview,
            report_even_slices: options.report_even_slices,
            sequential: false,
            stream_evens: false,
        }
    }
}
#[derive(Default)]
pub struct VolumeDecodeProgress<'a> {
    pub phase: VolumeDecodePhase,
    pub decoded_slices: i64,
    pub total_slices: i64,
    pub descriptor: VolumeDescriptor,
    pub preview: Option<Arc<Volume>>,
    pub band_start: i64,
    pub band_end: i64,
    pub band: Option<VolumeBandReader<'a>>,
    pub even_slices: Option<VolumeBandReader<'a>>,
}
#[derive(Clone, Copy)]
pub struct VolumeBandReader<'a> {
    cols: i64,
    rows: i64,
    start: i64,
    end: i64,
    step: i64,
    descriptor: VolumeDescriptor,
    data: BandView<'a>,
    compact: bool,
    owner: Option<&'a BandData>,
}
#[derive(Clone)]
pub struct RetainedVolumeBandReader {
    cols: i64,
    rows: i64,
    start: i64,
    end: i64,
    step: i64,
    descriptor: VolumeDescriptor,
    data: Arc<BandData>,
    compact: bool,
}
fn is_stored(d: &VolumeDescriptor, data: &BandView<'_>) -> bool {
    data.stored()
        && (d.scalar_format == VolumeScalarFormat::I16_STORED_LE
            || d.scalar_format == VolumeScalarFormat::U16_STORED_LE)
}
impl<'a> VolumeBandReader<'a> {
    fn slices(&self) -> impl Iterator<Item = i64> + use<> {
        let (end, step) = (self.end, self.step.max(1));
        let mut z = self.start;
        std::iter::from_fn(move || {
            (z < end).then(|| {
                let current = z;
                z += step;
                current
            })
        })
    }
    fn plane(&self, ordinal: usize, z: i64) -> BandView<'a> {
        let plane = (self.cols * self.rows) as usize;
        let at = if self.compact { ordinal } else { z as usize };
        self.data.range(at * plane, (at + 1) * plane)
    }
    pub fn retain(&self) -> Result<RetainedVolumeBandReader, RenderError> {
        let valid = checked_voxel_count(self.cols, self.rows, self.end);
        let Some(count) = valid.filter(|_| self.start >= 0 && self.start < self.end) else {
            return Err(invalid("invalid decode band"));
        };
        if self.step < 0 {
            return Err(invalid("invalid decode band stride"));
        }
        if !self.compact && self.data.len() < count {
            return Err(invalid(if self.data.stored() {
                "incomplete stored decode band"
            } else {
                "incomplete modality decode band"
            }));
        }
        if !self.compact
            && let Some(shared) = self.owner.and_then(BandData::shared_clone)
        {
            return Ok(RetainedVolumeBandReader {
                cols: self.cols,
                rows: self.rows,
                start: self.start,
                end: self.end,
                step: self.step,
                descriptor: self.descriptor,
                data: Arc::new(shared),
                compact: false,
            });
        }
        let plane = (self.cols * self.rows) as usize;
        let slices: Vec<i64> = self.slices().collect();
        let parallel = slices.len() * plane >= PARALLEL_RETAIN_SAMPLES;
        let data = match self.data {
            BandView::F32(_) => {
                BandData::F32(self.gather(&slices, plane, parallel, |v| match v {
                    BandView::F32(v) => v,
                    _ => &[],
                })?)
            }
            BandView::I16(_) => {
                BandData::I16(self.gather(&slices, plane, parallel, |v| match v {
                    BandView::I16(v) => v,
                    _ => &[],
                })?)
            }
            BandView::U16(_) => {
                BandData::U16(self.gather(&slices, plane, parallel, |v| match v {
                    BandView::U16(v) => v,
                    _ => &[],
                })?)
            }
        };
        Ok(RetainedVolumeBandReader {
            cols: self.cols,
            rows: self.rows,
            start: self.start,
            end: self.end,
            step: self.step,
            descriptor: self.descriptor,
            data: Arc::new(data),
            compact: true,
        })
    }
    fn gather<T: Copy + Default + Send + Sync + 'a>(
        &self,
        slices: &[i64],
        plane: usize,
        parallel: bool,
        samples: impl Fn(BandView<'a>) -> &'a [T] + Sync,
    ) -> Result<Vec<T>, RenderError> {
        let mut out = vec![T::default(); slices.len() * plane];
        if !parallel {
            for ((ordinal, &z), dst) in slices.iter().enumerate().zip(out.chunks_mut(plane)) {
                dst.copy_from_slice(samples(self.plane(ordinal, z)));
            }
            return Ok(out);
        }
        let planes: Vec<Mutex<&mut [T]>> = out.chunks_mut(plane).map(Mutex::new).collect();
        let copy = |item: i64| {
            let ordinal = item as usize;
            let mut dst = planes[ordinal].lock().unwrap_or_else(|e| e.into_inner());
            dst.copy_from_slice(samples(self.plane(ordinal, slices[ordinal])));
        };
        run_background(None, slices.len() as i64, 0, 1, || &copy)?;
        drop(planes);
        Ok(out)
    }
    pub fn write_modality_f32_to(&self, dst: Option<&mut dyn Write>) -> Result<(), RenderError> {
        let Some(dst) = dst.filter(|_| self.cols > 0 && self.rows > 0) else {
            return Err(invalid("no decode band"));
        };
        let mut bytes = Vec::new();
        for (ordinal, z) in self.slices().enumerate() {
            bytes.clear();
            self.plane(ordinal, z)
                .append_modality_f32_le(&self.descriptor, &mut bytes);
            write_exact(dst, &bytes)?;
        }
        Ok(())
    }
    pub fn write_stored16_to(&self, dst: Option<&mut dyn Write>) -> Result<(), RenderError> {
        let ok = self.cols > 0 && self.rows > 0 && is_stored(&self.descriptor, &self.data);
        let (Some(dst), true) = (dst, ok) else {
            return Err(invalid("band holds no 16-bit stored samples"));
        };
        let mut bytes = Vec::new();
        for (ordinal, z) in self.slices().enumerate() {
            bytes.clear();
            self.plane(ordinal, z).append_stored16_le(&mut bytes);
            write_exact(dst, &bytes)?;
        }
        Ok(())
    }
}
impl RetainedVolumeBandReader {
    fn plane_samples(&self) -> usize {
        (self.cols * self.rows).max(0) as usize
    }
    fn plane_view(&self, ordinal: usize) -> BandView<'_> {
        let plane = self.plane_samples();
        let at = if self.compact {
            ordinal
        } else {
            (self.start + ordinal as i64 * self.step.max(1)) as usize
        };
        self.data.plane_view(at * plane, plane)
    }
    pub fn write_modality_f32_to(&self, dst: Option<&mut dyn Write>) -> Result<(), RenderError> {
        let Some(dst) = dst.filter(|_| self.cols > 0 && self.rows > 0) else {
            return Err(invalid("no decode band"));
        };
        let mut bytes = Vec::new();
        for ordinal in 0..self.slice_count() {
            bytes.clear();
            self.plane_view(ordinal)
                .append_modality_f32_le(&self.descriptor, &mut bytes);
            write_exact(dst, &bytes)?;
        }
        Ok(())
    }
    pub fn write_stored16_to(&self, dst: Option<&mut dyn Write>) -> Result<(), RenderError> {
        let ok = self.cols > 0
            && self.rows > 0
            && self.slice_count() > 0
            && is_stored(&self.descriptor, &self.plane_view(0));
        let (Some(dst), true) = (dst, ok) else {
            return Err(invalid("band holds no 16-bit stored samples"));
        };
        let mut bytes = Vec::new();
        for ordinal in 0..self.slice_count() {
            bytes.clear();
            self.plane_view(ordinal).append_stored16_le(&mut bytes);
            write_exact(dst, &bytes)?;
        }
        Ok(())
    }
    pub fn modality_range(&self) -> Option<(f64, f64)> {
        let plane = self.plane_samples();
        if plane == 0 {
            return None;
        }
        let total = plane * self.slice_count();
        let stride = (total / 1_000_000).max(1);
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut current: Option<(usize, BandView<'_>)> = None;
        for index in (0..total).step_by(stride) {
            let ordinal = index / plane;
            let view = match current {
                Some((at, view)) if at == ordinal => view,
                _ => {
                    let view = self.plane_view(ordinal);
                    current = Some((ordinal, view));
                    view
                }
            };
            let value = view.modality(index % plane, &self.descriptor);
            if value.is_finite() {
                low = low.min(value);
                high = high.max(value);
            }
        }
        (low <= high).then_some((low, high))
    }
    #[doc(hidden)]
    pub fn append_native_le(&self, out: &mut Vec<u8>) {
        for ordinal in 0..self.slice_count() {
            let view = self.plane_view(ordinal);
            if view.stored() {
                view.append_stored16_le(out);
            } else {
                view.append_modality_f32_le(&self.descriptor, out);
            }
        }
    }
    pub fn native_le_bytes(&self) -> Option<&[u8]> {
        let plane = self.plane_samples();
        let count = self.slice_count();
        let view = if self.compact {
            self.data.plane_view(0, count * plane)
        } else if count <= 1 || self.step <= 1 {
            self.data
                .plane_view(self.start.max(0) as usize * plane, count * plane)
        } else {
            return None;
        };
        le_bytes(view)
    }
    pub fn slice_native_le_bytes(&self, ordinal: usize) -> Option<&[u8]> {
        if ordinal >= self.slice_count() {
            return None;
        }
        le_bytes(self.plane_view(ordinal))
    }
    pub fn slice_count(&self) -> usize {
        let plane = self.plane_samples().max(1);
        if self.compact {
            return self.data.view().len() / plane;
        }
        let step = self.step.max(1);
        (((self.end - self.start).max(0) + step - 1) / step) as usize
    }
    pub fn write_slice_native_le(
        &self,
        lower: usize,
        upper: usize,
        out: &mut [u8],
    ) -> Result<(), RenderError> {
        let plane = self.plane_samples();
        let count = self.slice_count();
        if plane == 0 || lower >= count || upper >= count {
            return Err(invalid("slice outside the decode band"));
        }
        let (a, b) = (self.plane_view(lower), self.plane_view(upper));
        let width = if a.stored() { 2 } else { 4 };
        if out.len() != plane * width {
            return Err(invalid("output is not one slice of the decode band"));
        }
        if lower == upper
            && let Some(bytes) = le_bytes(a)
        {
            out.copy_from_slice(bytes);
            return Ok(());
        }
        match (a, b) {
            (BandView::I16(a), BandView::I16(b)) => {
                for ((o, x), y) in out.as_chunks_mut::<2>().0.iter_mut().zip(a).zip(b) {
                    let mean = ((i32::from(*x) + i32::from(*y)) >> 1) as i16;
                    o.copy_from_slice(&mean.to_le_bytes());
                }
            }
            (BandView::U16(a), BandView::U16(b)) => {
                for ((o, x), y) in out.as_chunks_mut::<2>().0.iter_mut().zip(a).zip(b) {
                    let mean = ((u32::from(*x) + u32::from(*y)) >> 1) as u16;
                    o.copy_from_slice(&mean.to_le_bytes());
                }
            }
            (BandView::F32(a), BandView::F32(b)) => {
                for ((o, x), y) in out.as_chunks_mut::<4>().0.iter_mut().zip(a).zip(b) {
                    let mean = if lower == upper { *x } else { (*x + *y) * 0.5 };
                    o.copy_from_slice(&mean.to_bits().to_le_bytes());
                }
            }
            _ => return Err(invalid("mixed decode band")),
        }
        Ok(())
    }
}
fn le_bytes(view: BandView<'_>) -> Option<&[u8]> {
    match view {
        BandView::I16(v) => super::plane_buffer::le_bytes(v),
        BandView::U16(v) => super::plane_buffer::le_bytes(v),
        BandView::F32(v) => super::plane_buffer::le_bytes(v),
    }
}
fn write_exact(dst: &mut dyn Write, payload: &[u8]) -> Result<(), RenderError> {
    match dst.write(payload) {
        Err(e) => Err(RenderError::new(ErrorKind::Other, e.to_string())),
        Ok(n) if n != payload.len() => Err(RenderError::new(ErrorKind::Other, "short write")),
        Ok(_) => Ok(()),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VolumeProgressivePreviewPlan {
    pub original: VolumeDescriptor,
    pub even_slices: VolumeDescriptor,
    pub preview: VolumeDescriptor,
    pub in_plane_factors: [u32; 2],
}
pub fn plan_volume_progressive_preview(
    d: &VolumeDescriptor,
) -> Result<VolumeProgressivePreviewPlan, RenderError> {
    let mut check = *d;
    if check.volume_generation == 0 {
        check.volume_generation = 1;
    }
    validate_descriptor(&check)?;
    if d.components != 1 {
        return Err(invalid("progressive preview requires scalar input"));
    }
    let mut p = VolumeProgressivePreviewPlan {
        original: *d,
        even_slices: *d,
        preview: VolumeDescriptor::default(),
        in_plane_factors: [d.dimensions[0].min(2), d.dimensions[1].min(2)],
    };
    p.even_slices.dimensions[2] = d.dimensions[2] / 2 + d.dimensions[2] % 2;
    let z_factor: u32 = if p.even_slices.dimensions[2] > 1 {
        2
    } else {
        1
    };
    p.even_slices.spacing_mm[2] *= f64::from(z_factor);
    for row in 0..3 {
        p.even_slices.index_to_patient_lps.0[row * 4 + 2] *= f64::from(z_factor);
    }
    let (inverse, ok) = p.even_slices.index_to_patient_lps.inverse();
    if !ok {
        return Err(invalid("even-slice affine"));
    }
    p.even_slices.patient_lps_to_index = inverse;
    let bytes_per_sample: u64 = if d.scalar_format == VolumeScalarFormat::I16_STORED_LE
        || d.scalar_format == VolumeScalarFormat::U16_STORED_LE
    {
        2
    } else {
        4
    };
    p.even_slices.row_stride_bytes = u64::from(d.dimensions[0]) * bytes_per_sample;
    p.even_slices.slice_stride_bytes = p.even_slices.row_stride_bytes * u64::from(d.dimensions[1]);
    p.even_slices.byte_length =
        p.even_slices.slice_stride_bytes * u64::from(p.even_slices.dimensions[2]);
    p.preview = p.even_slices;
    p.preview.parent_generation = 0;
    p.preview.derivation = VolumeDerivation::NORMALIZED;
    p.preview.scalar_format = VolumeScalarFormat::F32_MODALITY_LE;
    p.preview.sample_domain = VolumeSampleDomain::MODALITY;
    p.preview.rescale_slope = 1.0;
    p.preview.rescale_intercept = 0.0;
    for (axis, &factor) in p.in_plane_factors.iter().enumerate() {
        p.preview.dimensions[axis] /= factor;
        p.preview.spacing_mm[axis] *= f64::from(factor);
        for row in 0..3 {
            p.preview.index_to_patient_lps.0[row * 4 + 3] +=
                d.index_to_patient_lps.0[row * 4 + axis] * f64::from(factor - 1) / 2.0;
            p.preview.index_to_patient_lps.0[row * 4 + axis] *= f64::from(factor);
        }
    }
    let (inverse, ok) = p.preview.index_to_patient_lps.inverse();
    if !ok {
        return Err(invalid("preview affine"));
    }
    p.preview.patient_lps_to_index = inverse;
    p.preview.row_stride_bytes = u64::from(p.preview.dimensions[0]) * 4;
    if p.preview.row_stride_bytes > u64::MAX / u64::from(p.preview.dimensions[1]) {
        return Err(invalid("preview slice byte count overflows"));
    }
    p.preview.slice_stride_bytes = p.preview.row_stride_bytes * u64::from(p.preview.dimensions[1]);
    if p.preview.slice_stride_bytes > u64::MAX / u64::from(p.preview.dimensions[2]) {
        return Err(invalid("preview byte count overflows"));
    }
    p.preview.byte_length = p.preview.slice_stride_bytes * u64::from(p.preview.dimensions[2]);
    for target in [p.even_slices, p.preview] {
        let mut target = target;
        if target.volume_generation == 0 {
            target.volume_generation = 1;
        }
        validate_descriptor(&target)?;
    }
    Ok(p)
}
