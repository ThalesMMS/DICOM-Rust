use super::matrix::{Jacobian, Matrix3, finite, finite_slice};
use super::transform::{
    DEFAULT_ORTHOGONALITY_TOLERANCE, Error, Kind, MatrixOffset, MatrixTransform, Transform,
    add_update, check_length, matrix_offset_methods, rotation_translation_jacobian, vec_from,
};
use super::versor::check_rotation;
use crate::fmath;
use crate::render::Vec3;
use std::any::Any;
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EulerOrder(pub i64);
impl EulerOrder {
    pub const ZXY: EulerOrder = EulerOrder(0);
    pub const ZYX: EulerOrder = EulerOrder(1);
}
impl fmt::Display for EulerOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            EulerOrder::ZXY => f.write_str("ZXY"),
            EulerOrder::ZYX => f.write_str("ZYX"),
            EulerOrder(n) => write!(f, "EulerOrder({n})"),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Euler3D {
    pub(crate) base: MatrixOffset,
    angle_x: f64,
    angle_y: f64,
    angle_z: f64,
    order: EulerOrder,
    derivatives: [Matrix3; 3],
}
impl Default for Euler3D {
    fn default() -> Self {
        Euler3D::new()
    }
}
matrix_offset_methods!(Euler3D);
impl Euler3D {
    pub fn new() -> Euler3D {
        Euler3D::with_order(EulerOrder::ZXY)
    }
    fn with_order(order: EulerOrder) -> Euler3D {
        let mut e = Euler3D {
            base: MatrixOffset::identity(),
            angle_x: 0.0,
            angle_y: 0.0,
            angle_z: 0.0,
            order,
            derivatives: [Matrix3::default(); 3],
        };
        e.compute_matrix();
        e
    }
    pub fn angles(&self) -> (f64, f64, f64) {
        (self.angle_x, self.angle_y, self.angle_z)
    }
    pub fn order(&self) -> EulerOrder {
        self.order
    }
    pub fn set_rotation(&mut self, x: f64, y: f64, z: f64) -> Result<(), Error> {
        if !finite(x) || !finite(y) || !finite(z) {
            return Err(Error::parameters("non-finite Euler angle"));
        }
        self.angle_x = x;
        self.angle_y = y;
        self.angle_z = z;
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    pub fn set_order(&mut self, order: EulerOrder) -> Result<(), Error> {
        if order != EulerOrder::ZXY && order != EulerOrder::ZYX {
            return Err(Error::parameters(format!(
                "unknown Euler order {}",
                order.0
            )));
        }
        self.order = order;
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    pub fn set_identity(&mut self) {
        self.angle_x = 0.0;
        self.angle_y = 0.0;
        self.angle_z = 0.0;
        self.base.translation = Vec3::ZERO;
        self.compute_matrix();
        self.base.compute_offset();
    }
    pub fn set_matrix(&mut self, m: &Matrix3) -> Result<(), Error> {
        self.set_matrix_with_tolerance(m, DEFAULT_ORTHOGONALITY_TOLERANCE)
    }
    pub fn set_matrix_with_tolerance(&mut self, m: &Matrix3, tolerance: f64) -> Result<(), Error> {
        check_rotation(m, tolerance)?;
        self.angles_from_matrix(m);
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    pub fn inverse(&self) -> Result<Euler3D, Error> {
        let (m, offset) = self.base.inverse()?;
        let mut out = Euler3D {
            base: MatrixOffset::identity(),
            angle_x: 0.0,
            angle_y: 0.0,
            angle_z: 0.0,
            order: self.order,
            derivatives: [Matrix3::default(); 3],
        };
        out.base.center = self.base.center;
        out.set_matrix_with_tolerance(&m, 1e-8)?;
        out.set_offset(offset)?;
        Ok(out)
    }
    fn compute_matrix(&mut self) {
        let (rx, drx) = axis_rotation(0, self.angle_x);
        let (ry, dry) = axis_rotation(1, self.angle_y);
        let (rz, drz) = axis_rotation(2, self.angle_z);
        if self.order == EulerOrder::ZYX {
            self.base.matrix = rz.mul(&ry).mul(&rx);
            self.derivatives = [
                rz.mul(&ry).mul(&drx),
                rz.mul(&dry).mul(&rx),
                drz.mul(&ry).mul(&rx),
            ];
            return;
        }
        self.base.matrix = rz.mul(&rx).mul(&ry);
        self.derivatives = [
            rz.mul(&drx).mul(&ry),
            rz.mul(&rx).mul(&dry),
            drz.mul(&rx).mul(&ry),
        ];
    }
    fn angles_from_matrix(&mut self, m: &Matrix3) {
        const GIMBAL: f64 = 0.00005;
        let m = &m.0;
        if self.order == EulerOrder::ZYX {
            self.angle_y = -fmath::asin(clamp_unit(m[2][0]));
            let c = fmath::cos(self.angle_y);
            if c.abs() > GIMBAL {
                self.angle_x = fmath::atan2(m[2][1] / c, m[2][2] / c);
                self.angle_z = fmath::atan2(m[1][0] / c, m[0][0] / c);
            } else {
                self.angle_x = 0.0;
                self.angle_z = fmath::atan2(-m[0][1], m[1][1]);
            }
            return;
        }
        self.angle_x = fmath::asin(clamp_unit(m[2][1]));
        let a = fmath::cos(self.angle_x);
        if a.abs() > GIMBAL {
            self.angle_y = fmath::atan2(-m[2][0] / a, m[2][2] / a);
            self.angle_z = fmath::atan2(-m[0][1] / a, m[1][1] / a);
        } else {
            self.angle_z = 0.0;
            self.angle_y = fmath::atan2(m[1][0], m[0][0]);
        }
    }
}
fn axis_rotation(axis: usize, angle: f64) -> (Matrix3, Matrix3) {
    let (c, s) = (fmath::cos(angle), fmath::sin(angle));
    match axis {
        0 => (
            Matrix3([[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]),
            Matrix3([[0.0, 0.0, 0.0], [0.0, -s, -c], [0.0, c, -s]]),
        ),
        1 => (
            Matrix3([[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]),
            Matrix3([[-s, 0.0, c], [0.0, 0.0, 0.0], [-c, 0.0, -s]]),
        ),
        _ => (
            Matrix3([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]),
            Matrix3([[-s, -c, 0.0], [c, -s, 0.0], [0.0, 0.0, 0.0]]),
        ),
    }
}
fn clamp_unit(v: f64) -> f64 {
    fmath::max(-1.0, fmath::min(1.0, v))
}
impl Transform for Euler3D {
    fn kind(&self) -> Kind {
        Kind::EULER_3D
    }
    fn transform_point(&self, point: Vec3) -> Vec3 {
        self.base.transform_point(point)
    }
    fn number_of_parameters(&self) -> usize {
        6
    }
    fn parameters(&self) -> Vec<f64> {
        let t = self.base.translation;
        vec![self.angle_x, self.angle_y, self.angle_z, t.x, t.y, t.z]
    }
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error> {
        check_length(parameters, 6, "Euler3D parameters")?;
        self.angle_x = parameters[0];
        self.angle_y = parameters[1];
        self.angle_z = parameters[2];
        self.base.translation = vec_from(&parameters[3..]);
        self.compute_matrix();
        self.base.compute_offset();
        Ok(())
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        let order = if self.order == EulerOrder::ZYX {
            1.0
        } else {
            0.0
        };
        let mut out = self.base.center_fixed_parameters();
        out.push(order);
        out
    }
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error> {
        if fixed.len() != 3 && fixed.len() != 4 {
            return Err(Error::parameters(format!(
                "Euler3D fixed parameters have {} values, want 3 or 4",
                fixed.len()
            )));
        }
        if !finite_slice(fixed) {
            return Err(Error::parameters("non-finite Euler3D fixed parameter"));
        }
        self.base.center = vec_from(fixed);
        if fixed.len() == 4 {
            self.order = if fixed[3] != 0.0 {
                EulerOrder::ZYX
            } else {
                EulerOrder::ZXY
            };
            self.compute_matrix();
        }
        self.base.compute_offset();
        Ok(())
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        add_update(self, update, factor)
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
