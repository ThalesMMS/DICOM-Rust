#![forbid(unsafe_code)]
use super::vec3::Vec3;
use super::volume::{Sampler, Volume};
use super::vr_transfer::VrLightingMaterial;
use super::window::WindowLevel;
use crate::fmath;
use std::sync::Arc;
pub(crate) const VR_OPACITY_THRESHOLD: f64 = 0.996_078_431_372_549;
pub(crate) fn headlight_shading(
    normal: Vec3,
    eye: Vec3,
    material: &VrLightingMaterial,
) -> (f64, f64) {
    let mut n_dot_l = normal.dot(eye);
    let r_dot_v = 2.0 * n_dot_l * n_dot_l - 1.0;
    n_dot_l = n_dot_l.abs();
    if !(n_dot_l > 0.0) {
        return (material.ambient, 0.0);
    }
    let lit = material.ambient + material.diffuse * n_dot_l;
    let mut specular = 0.0;
    if r_dot_v > 0.0 {
        specular = material.specular * fmath::pow(r_dot_v, material.specular_power);
    } else if material.specular_power == 0.0 {
        specular = material.specular;
    }
    (lit, specular)
}
impl Volume {
    pub fn sample_hu(self: &Arc<Self>, p: Vec3) -> Option<f64> {
        self.trilinear_at(p)
    }
    pub fn hu_range(self: &Arc<Self>) -> Option<(f64, f64)> {
        self.hu_range_from_sampler(None)
    }
    pub(crate) fn hu_range_from_sampler(
        self: &Arc<Self>,
        sampler: Option<&Sampler>,
    ) -> Option<(f64, f64)> {
        if self.depth == 0 {
            return None;
        }
        if let Some(range) = self.hu_range.get() {
            return Some(*range);
        }
        let owned;
        let sampler = match sampler {
            Some(s) if Arc::ptr_eq(&s.vol, self) => s,
            _ => {
                owned = Sampler::new(self).ok()?;
                &owned
            }
        };
        const TARGET: i64 = 2_000_000;
        let total = sampler.cols * sampler.rows * sampler.depth;
        let stride = if total > TARGET { total / TARGET } else { 1 };
        let (mut hu_min, mut hu_max) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut found = false;
        let mut i = 0i64;
        while i < total {
            let x = i % sampler.cols;
            let plane = i / sampler.cols;
            let (y, z) = (plane % sampler.rows, plane / sampler.rows);
            if let Some(val) = sampler.value_at(x, y, z) {
                if val < hu_min {
                    hu_min = val;
                }
                if val > hu_max {
                    hu_max = val;
                }
                found = true;
            }
            if stride >= total - i {
                break;
            }
            i += stride;
        }
        if !found {
            return None;
        }
        if hu_max <= hu_min {
            hu_min -= 1.0;
            hu_max += 1.0;
        }
        Some(*self.hu_range.get_or_init(|| (hu_min, hu_max)))
    }
    pub fn recommended_window(self: &Arc<Self>) -> WindowLevel {
        if let Some(dw) = self.first_slice_default_window()
            && dw.width > 0.0
        {
            return dw;
        }
        if let Some((lo, hi)) = self.hu_range() {
            return WindowLevel::new((lo + hi) / 2.0, hi - lo);
        }
        WindowLevel::new(40.0, 400.0)
    }
    pub fn physical_size(&self) -> Vec3 {
        Vec3::new(
            (self.cols - 1) as f64 * self.col_spacing,
            (self.rows - 1) as f64 * self.row_spacing,
            (self.depth - 1) as f64 * self.slice_spacing,
        )
    }
    pub fn bounding_radius_mm(&self) -> f64 {
        0.5 * self.physical_size().length()
    }
    pub fn frame_radius_mm(&self) -> f64 {
        let s = self.physical_size();
        let mut m = s.x;
        if s.y > m {
            m = s.y;
        }
        if s.z > m {
            m = s.z;
        }
        0.5 * m
    }
    pub fn texture_to_voxel(&self, tex: Vec3) -> Vec3 {
        Vec3::new(
            tex.x * (self.cols - 1) as f64,
            tex.y * (self.rows - 1) as f64,
            tex.z * (self.depth - 1) as f64,
        )
    }
    pub fn sample_hu_texture(self: &Arc<Self>, tex: Vec3) -> Option<f64> {
        self.trilinear_at(self.texture_to_voxel(tex))
    }
    pub(crate) fn shade_vr_with_gradient(
        &self,
        index_gradient: Vec3,
        view_dir: Vec3,
        material: &VrLightingMaterial,
    ) -> (f64, f64) {
        if !material.valid() {
            return (0.0, 0.0);
        }
        let physical = self.physical_gradient(index_gradient);
        let magnitude = physical.length();
        if magnitude <= 1e-6 {
            return (material.ambient, 0.0);
        }
        let normal = physical.scale(-1.0 / magnitude);
        let patient_view = self
            .axis_x
            .scale(view_dir.x)
            .add(self.axis_y.scale(view_dir.y))
            .add(self.normal.scale(view_dir.z));
        headlight_shading(normal, patient_view.scale(-1.0).normalize(), material)
    }
}
