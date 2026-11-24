#![forbid(unsafe_code)]
use super::grid::{clamp_continuous_index, nearest_index};
use super::vec3::Vec3;
use super::view_state::Interpolation;
use super::volume::Sampler;
use super::volume_sampler::VolumeReader;
use crate::fmath;
const VOXEL_BORDER_TOLERANCE: f64 = 1e-3;
fn clamp_to_grid(s: &Sampler, p: Vec3) -> Option<Vec3> {
    Some(Vec3::new(
        clamp_continuous_index(p.x, s.cols, VOXEL_BORDER_TOLERANCE)?,
        clamp_continuous_index(p.y, s.rows, VOXEL_BORDER_TOLERANCE)?,
        clamp_continuous_index(p.z, s.depth, VOXEL_BORDER_TOLERANCE)?,
    ))
}
pub(crate) fn sample_at(s: &Sampler, p: Vec3, mode: Interpolation) -> Option<f64> {
    match mode {
        Interpolation::NEAREST => nearest_at(s, p),
        Interpolation::CUBIC => cubic_at(s, p),
        _ => s.trilinear_at(p),
    }
}
fn nearest_at(s: &Sampler, p: Vec3) -> Option<f64> {
    let p = clamp_to_grid(s, p)?;
    let nearest = |v: f64, n: i64| nearest_index(v).min(n - 1);
    s.value_at(
        nearest(p.x, s.cols),
        nearest(p.y, s.rows),
        nearest(p.z, s.depth),
    )
}
pub(crate) fn cubic_taps(v: f64, n: i64) -> ([i64; 4], [f64; 4], usize) {
    let base = fmath::f64_to_i64(v.floor());
    let f = v - base as f64;
    let mut index = [0i64; 4];
    let mut weight = [0f64; 4];
    if f == 0.0 {
        index[0] = base;
        weight[0] = 1.0;
        return (index, weight, 1);
    }
    let (f2, f3) = (f * f, f * f * f);
    weight = [
        (-f3 + 2.0 * f2 - f) / 2.0,
        (3.0 * f3 - 5.0 * f2 + 2.0) / 2.0,
        (-3.0 * f3 + 4.0 * f2 + f) / 2.0,
        (f3 - f2) / 2.0,
    ];
    for (i, slot) in index.iter_mut().enumerate() {
        *slot = (base - 1 + i as i64).min(n - 1).max(0);
    }
    (index, weight, 4)
}
fn cubic_at(s: &Sampler, p: Vec3) -> Option<f64> {
    let p = clamp_to_grid(s, p)?;
    let (xi, xw, xn) = cubic_taps(p.x, s.cols);
    let (yi, yw, yn) = cubic_taps(p.y, s.rows);
    let (zi, zw, zn) = cubic_taps(p.z, s.depth);
    let mut sum = 0.0;
    for k in 0..zn {
        let mut plane = 0.0;
        for j in 0..yn {
            let mut row = 0.0;
            for i in 0..xn {
                row += xw[i] * s.value_at(xi[i], yi[j], zi[k]).unwrap_or(0.0);
            }
            plane += yw[j] * row;
        }
        sum += zw[k] * plane;
    }
    Some(sum)
}
impl VolumeReader {
    pub fn sample_at(&self, p: Vec3, mode: Interpolation) -> Option<f64> {
        sample_at(&self.sampler, self.to_sampled(p), mode)
    }
    pub fn sample_patient_at(&self, patient: Vec3, mode: Interpolation) -> Option<f64> {
        sample_at(
            &self.sampler,
            self.sampler.vol.patient_to_voxel(patient),
            mode,
        )
    }
}
