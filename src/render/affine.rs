#![forbid(unsafe_code)]
use super::vec3::Vec3;
use crate::fmath;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeometryAffine(pub [f64; 16]);
impl Default for GeometryAffine {
    fn default() -> Self {
        GeometryAffine([0.0; 16])
    }
}
impl GeometryAffine {
    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        let a = &self.0;
        Vec3::new(
            a[0] * p.x + a[1] * p.y + a[2] * p.z + a[3],
            a[4] * p.x + a[5] * p.y + a[6] * p.z + a[7],
            a[8] * p.x + a[9] * p.y + a[10] * p.z + a[11],
        )
    }
    pub fn finite(&self) -> bool {
        let a = &self.0;
        if a.iter().any(|v| v.is_nan() || v.is_infinite()) {
            return false;
        }
        a[12].abs() <= 1e-12
            && a[13].abs() <= 1e-12
            && a[14].abs() <= 1e-12
            && (a[15] - 1.0).abs() <= 1e-12
    }
    pub fn inverse(&self) -> (GeometryAffine, bool) {
        if !self.finite() {
            return (GeometryAffine::default(), false);
        }
        let a = &self.0;
        let (_, inverse, ok) = geometry_affine_pair(
            Vec3::new(a[3], a[7], a[11]),
            Vec3::new(a[0], a[4], a[8]),
            Vec3::new(a[1], a[5], a[9]),
            Vec3::new(a[2], a[6], a[10]),
        );
        (inverse, ok)
    }
}
pub(crate) fn finite_positive_spacing(value: f64) -> bool {
    value > 0.0 && !value.is_nan() && !value.is_infinite()
}
pub(crate) fn geometry_affine_pair(
    origin: Vec3,
    x_step: Vec3,
    y_step: Vec3,
    z_step: Vec3,
) -> (GeometryAffine, GeometryAffine, bool) {
    let zero = GeometryAffine::default();
    if !origin.finite() || !x_step.finite() || !y_step.finite() || !z_step.finite() {
        return (zero, zero, false);
    }
    let index_to_patient = GeometryAffine([
        x_step.x, y_step.x, z_step.x, origin.x, x_step.y, y_step.y, z_step.y, origin.y, x_step.z,
        y_step.z, z_step.z, origin.z, 0.0, 0.0, 0.0, 1.0,
    ]);
    let (a, b, c) = (x_step.x, y_step.x, z_step.x);
    let (d, e, f) = (x_step.y, y_step.y, z_step.y);
    let (g, h, i) = (x_step.z, y_step.z, z_step.z);
    let determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    let scale = fmath::max(
        x_step.length(),
        fmath::max(y_step.length(), z_step.length()),
    );
    if !finite_positive_spacing(scale) || determinant.abs() <= 1e-15 * scale * scale * scale {
        return (zero, zero, false);
    }
    let inv_det = 1.0 / determinant;
    let r00 = (e * i - f * h) * inv_det;
    let r01 = (c * h - b * i) * inv_det;
    let r02 = (b * f - c * e) * inv_det;
    let r10 = (f * g - d * i) * inv_det;
    let r11 = (a * i - c * g) * inv_det;
    let r12 = (c * d - a * f) * inv_det;
    let r20 = (d * h - e * g) * inv_det;
    let r21 = (b * g - a * h) * inv_det;
    let r22 = (a * e - b * d) * inv_det;
    let patient_to_index = GeometryAffine([
        r00,
        r01,
        r02,
        -(r00 * origin.x + r01 * origin.y + r02 * origin.z),
        r10,
        r11,
        r12,
        -(r10 * origin.x + r11 * origin.y + r12 * origin.z),
        r20,
        r21,
        r22,
        -(r20 * origin.x + r21 * origin.y + r22 * origin.z),
        0.0,
        0.0,
        0.0,
        1.0,
    ]);
    let ok = index_to_patient.finite() && patient_to_index.finite();
    (index_to_patient, patient_to_index, ok)
}
