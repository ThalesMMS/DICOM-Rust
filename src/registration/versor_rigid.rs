use super::matrix::{Jacobian, Matrix3, finite_vec};
use super::transform::{
    DEFAULT_ORTHOGONALITY_TOLERANCE, Error, Kind, MatrixOffset, MatrixTransform, Transform,
    check_length, check_update, matrix_offset_methods, rotation_translation_jacobian, vec_from,
};
use super::versor::{Versor, compose_versor_update, versor_from_parameters};
use crate::render::Vec3;
use std::any::Any;
#[derive(Debug, Clone, PartialEq)]
pub struct VersorRigid3D {
    pub(crate) base: MatrixOffset,
    versor: Versor,
    derivatives: [Matrix3; 3],
}
impl Default for VersorRigid3D {
    fn default() -> Self {
        VersorRigid3D::new()
    }
}
matrix_offset_methods!(VersorRigid3D);
impl VersorRigid3D {
    pub fn new() -> VersorRigid3D {
        let mut v = VersorRigid3D {
            base: MatrixOffset::identity(),
            versor: Versor::identity(),
            derivatives: [Matrix3::default(); 3],
        };
        v.compute_matrix();
        v
    }
    pub fn versor(&self) -> Versor {
        self.versor
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
    pub fn set_identity(&mut self) {
        self.versor = Versor::identity();
        self.base.translation = Vec3::ZERO;
        self.compute_matrix();
        self.base.compute_offset();
    }
    pub fn set_matrix(&mut self, m: &Matrix3) -> Result<(), Error> {
        self.set_matrix_with_tolerance(m, DEFAULT_ORTHOGONALITY_TOLERANCE)
    }
    pub fn set_matrix_with_tolerance(&mut self, m: &Matrix3, tolerance: f64) -> Result<(), Error> {
        self.versor = Versor::from_matrix(m, tolerance)?;
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    pub fn inverse(&self) -> Result<VersorRigid3D, Error> {
        let (_, offset) = self.base.inverse()?;
        let mut out = VersorRigid3D::new();
        out.base.center = self.base.center;
        out.set_versor(self.versor.conjugate())?;
        out.set_offset(offset)?;
        Ok(out)
    }
    fn compute_matrix(&mut self) {
        self.base.matrix = self.versor.matrix();
        self.derivatives = self.versor.right_part_derivatives();
    }
}
impl Transform for VersorRigid3D {
    fn kind(&self) -> Kind {
        Kind::VERSOR_RIGID_3D
    }
    fn transform_point(&self, point: Vec3) -> Vec3 {
        self.base.transform_point(point)
    }
    fn number_of_parameters(&self) -> usize {
        6
    }
    fn parameters(&self) -> Vec<f64> {
        let t = self.base.translation;
        vec![self.versor.x, self.versor.y, self.versor.z, t.x, t.y, t.z]
    }
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error> {
        check_length(parameters, 6, "VersorRigid3D parameters")?;
        self.versor = versor_from_parameters(parameters)?;
        self.base.translation = vec_from(&parameters[3..]);
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        self.base.center_fixed_parameters()
    }
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error> {
        check_length(fixed, 3, "VersorRigid3D fixed parameters")?;
        self.set_center(vec_from(fixed))
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        check_update(update, 6, factor)?;
        let q = compose_versor_update(&self.versor, update, factor)?;
        let translation = self
            .base
            .translation
            .add(vec_from(&update[3..]).scale(factor));
        if !finite_vec(translation) {
            return Err(Error::parameters("translation update overflowed"));
        }
        self.versor = q;
        self.base.translation = translation;
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    fn jacobian_with_respect_to_parameters(&self, point: Vec3, out: &mut Jacobian) {
        out.reset(6);
        rotation_translation_jacobian(out, &self.derivatives, point.sub(self.base.center), 3);
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
