use super::matrix::{Jacobian, Matrix3};
use super::transform::{
    Error, Kind, MatrixTransform, Transform, add_update, check_length, vec_from,
};
use crate::render::Vec3;
use std::any::Any;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Translation3D {
    translation: Vec3,
}
impl Translation3D {
    pub fn new() -> Translation3D {
        Translation3D::default()
    }
    pub fn set_translation(&mut self, v: Vec3) -> Result<(), Error> {
        self.set_parameters(&[v.x, v.y, v.z])
    }
    pub fn inverse(&self) -> Result<Translation3D, Error> {
        Ok(Translation3D {
            translation: self.translation.scale(-1.0),
        })
    }
}
impl MatrixTransform for Translation3D {
    fn matrix(&self) -> Matrix3 {
        Matrix3::identity()
    }
    fn offset(&self) -> Vec3 {
        self.translation
    }
    fn center(&self) -> Vec3 {
        Vec3::ZERO
    }
    fn translation(&self) -> Vec3 {
        self.translation
    }
    fn transform_vector(&self, vector: Vec3) -> Vec3 {
        vector
    }
}
impl Transform for Translation3D {
    fn kind(&self) -> Kind {
        Kind::TRANSLATION
    }
    fn transform_point(&self, point: Vec3) -> Vec3 {
        point.add(self.translation)
    }
    fn number_of_parameters(&self) -> usize {
        3
    }
    fn parameters(&self) -> Vec<f64> {
        vec![self.translation.x, self.translation.y, self.translation.z]
    }
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error> {
        check_length(parameters, 3, "Translation3D parameters")?;
        self.translation = vec_from(parameters);
        Ok(())
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        Vec::new()
    }
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error> {
        check_length(fixed, 0, "Translation3D fixed parameters")
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        add_update(self, update, factor)
    }
    fn jacobian_with_respect_to_parameters(&self, _point: Vec3, out: &mut Jacobian) {
        out.reset(3);
        for i in 0..3 {
            out.set(i, i, 1.0);
        }
    }
    fn jacobian_with_respect_to_position(&self, _point: Vec3) -> Matrix3 {
        Matrix3::identity()
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
