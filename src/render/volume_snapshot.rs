#![forbid(unsafe_code)]
use super::affine::{GeometryAffine, geometry_affine_pair};
use super::error::{ErrorKind, RenderError};
use super::parallel::{Cancellation, ctx_err};
use super::volume::{Payload, Sampler, Volume};
use std::sync::Arc;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VolumeScalarFormat {
    F32Modality,
    I16Stored,
    U16Stored,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeDescriptor {
    pub dimensions: [u32; 3],
    pub components: u32,
    pub scalar_format: VolumeScalarFormat,
    pub rescale_slope: f64,
    pub rescale_intercept: f64,
    pub spacing_mm: [f64; 3],
    pub index_to_patient_lps: GeometryAffine,
    pub patient_lps_to_index: GeometryAffine,
}
pub struct VolumeSnapshot {
    sampler: Sampler,
    descriptor: VolumeDescriptor,
}
fn invalid_snapshot(detail: &str) -> RenderError {
    RenderError::new(
        ErrorKind::InvalidVolumeSnapshot,
        format!("render: invalid volume snapshot: {detail}"),
    )
}
impl Volume {
    fn snapshot_descriptor(&self, payload: &Payload) -> Result<VolumeDescriptor, RenderError> {
        let max = i64::from(u32::MAX);
        if self.cols <= 0
            || self.rows <= 0
            || self.depth <= 0
            || self.cols > max
            || self.rows > max
            || self.depth > max
        {
            return Err(invalid_snapshot("invalid volume dimensions"));
        }
        let (index_to_patient, patient_to_index, ok) = geometry_affine_pair(
            self.origin,
            self.axis_x.scale(self.col_spacing),
            self.axis_y.scale(self.row_spacing),
            self.normal.scale(self.slice_spacing),
        );
        if !ok {
            return Err(invalid_snapshot("invalid patient affine"));
        }
        let (scalar_format, rescale_slope, rescale_intercept) = match payload {
            Payload::F32(_) | Payload::Rgb(_) => (VolumeScalarFormat::F32Modality, 1.0, 0.0),
            Payload::I16 {
                slope, intercept, ..
            } => (VolumeScalarFormat::I16Stored, *slope, *intercept),
            Payload::U16 {
                slope, intercept, ..
            } => (VolumeScalarFormat::U16Stored, *slope, *intercept),
        };
        Ok(VolumeDescriptor {
            dimensions: [self.cols as u32, self.rows as u32, self.depth as u32],
            components: self.components(),
            scalar_format,
            rescale_slope,
            rescale_intercept,
            spacing_mm: [self.col_spacing, self.row_spacing, self.slice_spacing],
            index_to_patient_lps: index_to_patient,
            patient_lps_to_index: patient_to_index,
        })
    }
    pub fn acquire_snapshot(self: &Arc<Self>) -> Result<VolumeSnapshot, RenderError> {
        self.acquire_snapshot_context(None)
    }
    pub fn acquire_snapshot_context(
        self: &Arc<Self>,
        ctx: Option<&dyn Cancellation>,
    ) -> Result<VolumeSnapshot, RenderError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let canonical = self.regular_grid()?;
        let payload = canonical.payload()?;
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let descriptor = canonical.snapshot_descriptor(&payload)?;
        let sampler = Sampler::direct(canonical)?;
        Ok(VolumeSnapshot {
            sampler,
            descriptor,
        })
    }
}
impl VolumeSnapshot {
    pub fn descriptor(&self) -> VolumeDescriptor {
        self.descriptor
    }
    pub fn modality_at(&self, x: u32, y: u32, z: u32) -> Option<f64> {
        let d = &self.descriptor;
        if d.components != 1 || x >= d.dimensions[0] || y >= d.dimensions[1] || z >= d.dimensions[2]
        {
            return None;
        }
        self.sampler
            .value_at(i64::from(x), i64::from(y), i64::from(z))
    }
}
