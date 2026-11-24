#![forbid(unsafe_code)]
use super::error::RenderError;
use super::vec3::Vec3;
use super::volume::{Sampler, Volume};
use std::sync::Arc;
pub struct VolumeReader {
    pub(crate) source: Arc<Volume>,
    pub(crate) sampler: Sampler,
}
impl Volume {
    pub fn acquire_reader(self: &Arc<Self>) -> Result<VolumeReader, RenderError> {
        let sampler = Sampler::new(self)?;
        Ok(VolumeReader {
            source: self.clone(),
            sampler,
        })
    }
}
impl VolumeReader {
    pub(crate) fn to_sampled(&self, p: Vec3) -> Vec3 {
        if Arc::ptr_eq(&self.sampler.vol, &self.source) {
            return p;
        }
        self.sampler
            .vol
            .patient_to_voxel(self.source.voxel_to_patient(p))
    }
    pub fn trilinear_at(&self, p: Vec3) -> Option<f64> {
        self.sampler.trilinear_at(self.to_sampled(p))
    }
    pub fn sample_patient(&self, patient: Vec3) -> Option<f64> {
        self.sampler
            .trilinear_at(self.sampler.vol.patient_to_voxel(patient))
    }
    pub fn close(self) {}
}
