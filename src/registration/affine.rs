use super::matrix::{Jacobian, Matrix3};
use super::transform::{
    Error, Kind, MatrixOffset, MatrixTransform, Transform, add_update, check_length,
    matrix_offset_methods, vec_from,
};
use crate::render::Vec3;
use std::any::Any;
#[derive(Debug, Clone, PartialEq)]
pub struct Affine {
    pub(crate) base: MatrixOffset,
}
impl Default for Affine {
    fn default() -> Self {
        Affine::new()
    }
}
matrix_offset_methods!(Affine);
impl Affine {
    pub fn new() -> Affine {
        Affine {
            base: MatrixOffset::identity(),
        }
    }
    pub fn from_transform(t: Option<&dyn MatrixTransform>) -> Result<Affine, Error> {
        let Some(t) = t else {
            return Err(Error::new(
                super::ErrorKind::UnsupportedTransform,
                "nil transform",
            ));
        };
        let mut a = Affine::new();
        a.set_matrix(t.matrix())?;
        a.set_center(t.center())?;
        a.set_offset(t.offset())?;
        Ok(a)
    }
    pub fn set_matrix(&mut self, m: Matrix3) -> Result<(), Error> {
        if !m.finite() {
            return Err(Error::matrix("non-finite affine matrix"));
        }
        self.base.matrix = m;
        self.base.compute_offset();
        Ok(())
    }
    pub fn set_identity(&mut self) {
        self.base.matrix = Matrix3::identity();
        self.base.translation = Vec3::ZERO;
        self.base.compute_offset();
    }
    pub fn inverse(&self) -> Result<Affine, Error> {
        let (m, offset) = self.base.inverse()?;
        let mut out = Affine::new();
        out.base.center = self.base.center;
        out.base.matrix = m;
        out.set_offset(offset)?;
        Ok(out)
    }
}
impl Transform for Affine {
    fn kind(&self) -> Kind {
        Kind::AFFINE
    }
    fn transform_point(&self, point: Vec3) -> Vec3 {
        self.base.transform_point(point)
    }
    fn number_of_parameters(&self) -> usize {
        12
    }
    fn parameters(&self) -> Vec<f64> {
        let mut out = Vec::with_capacity(12);
        for row in &self.base.matrix.0 {
            out.extend_from_slice(row);
        }
        let t = self.base.translation;
        out.extend_from_slice(&[t.x, t.y, t.z]);
        out
    }
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error> {
        check_length(parameters, 12, "Affine parameters")?;
        for i in 0..3 {
            self.base.matrix.0[i].copy_from_slice(&parameters[3 * i..3 * i + 3]);
        }
        self.base.translation = vec_from(&parameters[9..]);
        self.base.compute_offset();
        Ok(())
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        self.base.center_fixed_parameters()
    }
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error> {
        check_length(fixed, 3, "Affine fixed parameters")?;
        self.set_center(vec_from(fixed))
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        add_update(self, update, factor)
    }
    fn jacobian_with_respect_to_parameters(&self, point: Vec3, out: &mut Jacobian) {
        out.reset(12);
        let c = self.base.center;
        let relative = [point.x - c.x, point.y - c.y, point.z - c.z];
        for i in 0..3 {
            for (j, r) in relative.iter().enumerate() {
                out.set(i, 3 * i + j, *r);
            }
            out.set(i, 9 + i, 1.0);
        }
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
