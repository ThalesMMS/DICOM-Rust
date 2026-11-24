#![forbid(unsafe_code)]
use super::vec3::Vec3;
use super::vr_transfer::finite;
use crate::fmath;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrScattering {
    pub blend: f64,
    pub reach: f64,
    pub anisotropy: f64,
    pub light_azimuth: f64,
    pub light_elevation: f64,
}
impl VrScattering {
    pub fn enabled(&self) -> bool {
        finite(self.blend) && self.blend > 0.0
    }
    pub fn normalized(&self) -> VrScattering {
        let mut s = *self;
        s.blend = fmath::min(2.0, fmath::max(0.0, s.blend));
        if !finite(s.reach) || s.reach <= 0.0 || s.reach > 1.0 {
            s.reach = 1.0;
        }
        if !finite(s.anisotropy) {
            s.anisotropy = 0.0;
        }
        s.anisotropy = fmath::max(-0.99, fmath::min(0.99, s.anisotropy));
        if !finite(s.light_azimuth) {
            s.light_azimuth = 0.0;
        }
        if !finite(s.light_elevation) {
            s.light_elevation = 0.0;
        }
        s
    }
}
pub fn vr_scattering_light(
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    azimuth: f64,
    elevation: f64,
) -> Vec3 {
    let ce = fmath::cos(elevation);
    forward
        .scale(-ce * fmath::cos(azimuth))
        .add(up.scale(fmath::sin(elevation)))
        .add(right.scale(-ce * fmath::sin(azimuth)))
        .normalize()
}
pub fn henyey_greenstein(g: f64, cos_angle: f64) -> f64 {
    if g.abs() < 0.01 {
        return 1.0;
    }
    let g2 = g * g;
    let d = 1.0 + g2 - 2.0 * g * cos_angle;
    (1.0 - g2) / (d * d.sqrt())
}
pub fn vr_scattering_coefficient(b: f64, gradient: f64, alpha: f64) -> f64 {
    let e = fmath::exp(-2.0 * b * gradient * alpha);
    if b < 1.0 {
        return 2.0 * b * e;
    }
    2.0 * (1.0 - b) * e + 2.0 * b - 1.0
}
