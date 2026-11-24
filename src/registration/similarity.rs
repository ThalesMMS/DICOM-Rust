use super::matrix::{Jacobian, Matrix3, finite, finite_vec};
use super::transform::{
    DEFAULT_ORTHOGONALITY_TOLERANCE, Error, Kind, MatrixOffset, MatrixTransform, Transform,
    check_length, check_update, matrix_offset_methods, rotation_translation_jacobian, vec_from,
};
use super::versor::{Versor, compose_versor_update, versor_from_parameters};
use crate::fmath;
use crate::render::Vec3;
use crate::textfmt::float_v;
use std::any::Any;
#[derive(Debug, Clone, PartialEq)]
pub struct Similarity3D {
    pub(crate) base: MatrixOffset,
    versor: Versor,
    scale: f64,
    rotation: Matrix3,
    derivatives: [Matrix3; 3],
}
impl Default for Similarity3D {
    fn default() -> Self {
        Similarity3D::new()
    }
}
matrix_offset_methods!(Similarity3D);
impl Similarity3D {
    pub fn new() -> Similarity3D {
        let mut s = Similarity3D {
            base: MatrixOffset::identity(),
            versor: Versor::identity(),
            scale: 1.0,
            rotation: Matrix3::default(),
            derivatives: [Matrix3::default(); 3],
        };
        s.compute_matrix();
        s
    }
    pub fn versor(&self) -> Versor {
        self.versor
    }
    pub fn scale(&self) -> f64 {
        self.scale
    }
    pub fn set_versor(&mut self, q: Versor) -> Result<(), Error> {
        let Some(n) = q.normalized() else {
            return Err(Error::parameters("zero or non-finite versor"));
        };
        self.versor = n.canonical();
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    pub fn set_rotation(&mut self, axis: Vec3, angle: f64) -> Result<(), Error> {
        let q = Versor::from_axis_angle(axis, angle)?;
        self.set_versor(q)
    }
    pub fn set_scale(&mut self, scale: f64) -> Result<(), Error> {
        if !finite(scale) || scale <= 0.0 {
            return Err(Error::parameters(format!(
                "similarity scale {} must be finite and positive",
                float_v(scale)
            )));
        }
        self.scale = scale;
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    pub fn set_identity(&mut self) {
        self.versor = Versor::identity();
        self.scale = 1.0;
        self.base.translation = Vec3::ZERO;
        self.compute_matrix();
        self.base.compute_offset();
    }
    pub fn set_matrix(&mut self, m: &Matrix3) -> Result<(), Error> {
        self.set_matrix_with_tolerance(m, DEFAULT_ORTHOGONALITY_TOLERANCE)
    }
    pub fn set_matrix_with_tolerance(&mut self, m: &Matrix3, tolerance: f64) -> Result<(), Error> {
        let (q, scale) = similarity_from_matrix(m, tolerance)?;
        self.versor = q;
        self.scale = scale;
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    pub fn inverse(&self) -> Result<Similarity3D, Error> {
        let (_, offset) = self.base.inverse()?;
        let mut out = Similarity3D::new();
        out.base.center = self.base.center;
        out.versor = self.versor.conjugate();
        out.scale = 1.0 / self.scale;
        out.compute_matrix();
        out.set_offset(offset)?;
        Ok(out)
    }
    fn compute_matrix(&mut self) {
        self.rotation = self.versor.matrix();
        self.base.matrix = self.rotation.scale(self.scale);
        self.derivatives = self
            .versor
            .right_part_derivatives()
            .map(|d| d.scale(self.scale));
    }
}
fn similarity_from_matrix(m: &Matrix3, tolerance: f64) -> Result<(Versor, f64), Error> {
    if !m.finite() {
        return Err(Error::matrix("non-finite similarity matrix"));
    }
    let det = m.determinant();
    if !(det > 0.0) || !finite(det) {
        return Err(Error::matrix(format!(
            "similarity matrix determinant {} must be positive",
            float_v(det)
        )));
    }
    let scale = fmath::cbrt(det);
    let q = Versor::from_matrix(&m.scale(1.0 / scale), tolerance)?;
    Ok((q, scale))
}
impl Transform for Similarity3D {
    fn kind(&self) -> Kind {
        Kind::SIMILARITY_3D
    }
    fn transform_point(&self, point: Vec3) -> Vec3 {
        self.base.transform_point(point)
    }
    fn number_of_parameters(&self) -> usize {
        7
    }
    fn parameters(&self) -> Vec<f64> {
        let t = self.base.translation;
        vec![
            self.versor.x,
            self.versor.y,
            self.versor.z,
            t.x,
            t.y,
            t.z,
            self.scale,
        ]
    }
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error> {
        check_length(parameters, 7, "Similarity3D parameters")?;
        if parameters[6] <= 0.0 {
            return Err(Error::parameters(format!(
                "similarity scale {} must be positive",
                float_v(parameters[6])
            )));
        }
        self.versor = versor_from_parameters(parameters)?;
        self.base.translation = vec_from(&parameters[3..]);
        self.scale = parameters[6];
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        self.base.center_fixed_parameters()
    }
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error> {
        check_length(fixed, 3, "Similarity3D fixed parameters")?;
        self.set_center(vec_from(fixed))
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        check_update(update, 7, factor)?;
        let scale = self.scale + factor * update[6];
        if !finite(scale) || scale <= 0.0 {
            return Err(Error::parameters(format!(
                "similarity scale step to {}",
                float_v(scale)
            )));
        }
        let translation = self
            .base
            .translation
            .add(vec_from(&update[3..]).scale(factor));
        if !finite_vec(translation) {
            return Err(Error::parameters("translation update overflowed"));
        }
        let q = compose_versor_update(&self.versor, update, factor)?;
        self.versor = q;
        self.base.translation = translation;
        self.scale = scale;
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    fn jacobian_with_respect_to_parameters(&self, point: Vec3, out: &mut Jacobian) {
        out.reset(7);
        let relative = point.sub(self.base.center);
        rotation_translation_jacobian(out, &self.derivatives, relative, 3);
        out.set_column(6, self.rotation.mul_vec(relative));
    }
    fn jacobian_with_respect_to_position(&self, _point: Vec3) -> Matrix3 {
        self.base.matrix
    }
    fn clone_box(&self) -> Box<dyn Transform> {
        Box::new(self.clone())
    }
    fn as_matrix_transform(&self) -> Option<&dyn MatrixTransform> {
        Some(self)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
