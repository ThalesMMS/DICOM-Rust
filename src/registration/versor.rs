use super::matrix::{Matrix3, finite, finite_vec};
use super::transform::{Error, vec_from};
use crate::fmath;
use crate::render::Vec3;
use crate::textfmt::float_v;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Versor {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}
impl Versor {
    pub const fn identity() -> Versor {
        Versor {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        }
    }
    pub fn from_axis_angle(axis: Vec3, angle: f64) -> Result<Versor, Error> {
        let norm = axis.length();
        if !finite_vec(axis) || !finite(angle) || norm == 0.0 || !finite(norm) {
            return Err(Error::parameters(
                "rotation axis must be finite and non-zero",
            ));
        }
        let s = fmath::sin(angle / 2.0) / norm;
        Ok(Versor {
            x: axis.x * s,
            y: axis.y * s,
            z: axis.z * s,
            w: fmath::cos(angle / 2.0),
        })
    }
    pub fn from_right_part(right: Vec3) -> Result<Versor, Error> {
        if !finite_vec(right) {
            return Err(Error::parameters("non-finite versor"));
        }
        let n2 = right.dot(right);
        if n2 > 1.0 {
            return Err(Error::parameters(format!(
                "versor right part has norm {} > 1",
                float_v(n2.sqrt())
            )));
        }
        Ok(Versor {
            x: right.x,
            y: right.y,
            z: right.z,
            w: (1.0 - n2).sqrt(),
        })
    }
    pub fn from_matrix(m: &Matrix3, tolerance: f64) -> Result<Versor, Error> {
        check_rotation(m, tolerance)?;
        let m = &m.0;
        let trace = m[0][0] + m[1][1] + m[2][2];
        let q = if trace > m[0][0] && trace > m[1][1] && trace > m[2][2] {
            let s = 2.0 * (1.0 + trace).sqrt();
            Versor {
                w: s / 4.0,
                x: (m[2][1] - m[1][2]) / s,
                y: (m[0][2] - m[2][0]) / s,
                z: (m[1][0] - m[0][1]) / s,
            }
        } else if m[0][0] >= m[1][1] && m[0][0] >= m[2][2] {
            let s = 2.0 * (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt();
            Versor {
                x: s / 4.0,
                y: (m[0][1] + m[1][0]) / s,
                z: (m[0][2] + m[2][0]) / s,
                w: (m[2][1] - m[1][2]) / s,
            }
        } else if m[1][1] >= m[2][2] {
            let s = 2.0 * (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt();
            Versor {
                x: (m[0][1] + m[1][0]) / s,
                y: s / 4.0,
                z: (m[1][2] + m[2][1]) / s,
                w: (m[0][2] - m[2][0]) / s,
            }
        } else {
            let s = 2.0 * (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt();
            Versor {
                x: (m[0][2] + m[2][0]) / s,
                y: (m[1][2] + m[2][1]) / s,
                z: s / 4.0,
                w: (m[1][0] - m[0][1]) / s,
            }
        };
        match q.normalized() {
            Some(q) => Ok(q.canonical()),
            None => Err(Error::matrix("cannot convert matrix to a versor")),
        }
    }
    pub fn mul(&self, r: &Versor) -> Versor {
        let q = self;
        Versor {
            x: q.w * r.x + q.x * r.w + q.y * r.z - q.z * r.y,
            y: q.w * r.y - q.x * r.z + q.y * r.w + q.z * r.x,
            z: q.w * r.z + q.x * r.y - q.y * r.x + q.z * r.w,
            w: q.w * r.w - q.x * r.x - q.y * r.y - q.z * r.z,
        }
    }
    pub fn conjugate(&self) -> Versor {
        Versor {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }
    pub fn right(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
    pub fn angle(&self) -> f64 {
        2.0 * fmath::atan2(self.right().length(), self.w)
    }
    pub fn axis(&self) -> Vec3 {
        self.right().normalize()
    }
    pub fn matrix(&self) -> Matrix3 {
        let q = self;
        let (xx, yy, zz) = (q.x * q.x, q.y * q.y, q.z * q.z);
        let (xy, xz, xw) = (q.x * q.y, q.x * q.z, q.x * q.w);
        let (yz, yw, zw) = (q.y * q.z, q.y * q.w, q.z * q.w);
        Matrix3([
            [1.0 - 2.0 * (yy + zz), 2.0 * (xy - zw), 2.0 * (xz + yw)],
            [2.0 * (xy + zw), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - xw)],
            [2.0 * (xz - yw), 2.0 * (yz + xw), 1.0 - 2.0 * (xx + yy)],
        ])
    }
    pub(crate) fn right_part_derivatives(&self) -> [Matrix3; 3] {
        let (x, y, z, w) = (self.x, self.y, self.z, self.w);
        let dx = [
            [0.0, 2.0 * y, 2.0 * z],
            [2.0 * y, -4.0 * x, -2.0 * w],
            [2.0 * z, 2.0 * w, -4.0 * x],
        ];
        let dy = [
            [-4.0 * y, 2.0 * x, 2.0 * w],
            [2.0 * x, 0.0, 2.0 * z],
            [-2.0 * w, 2.0 * z, -4.0 * y],
        ];
        let dz = [
            [-4.0 * z, -2.0 * w, 2.0 * x],
            [2.0 * w, -4.0 * z, 2.0 * y],
            [2.0 * x, 2.0 * y, 0.0],
        ];
        let dw = [
            [0.0, -2.0 * z, 2.0 * y],
            [2.0 * z, 0.0, -2.0 * x],
            [-2.0 * y, 2.0 * x, 0.0],
        ];
        let mut out = [dx, dy, dz];
        let components = [x, y, z];
        for (k, m) in out.iter_mut().enumerate() {
            let chain = -components[k] / w;
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] += dw[i][j] * chain;
                }
            }
        }
        out.map(Matrix3)
    }
    pub(crate) fn normalized(&self) -> Option<Versor> {
        let q = self;
        let n = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
        if !finite(n) || n < 1e-20 {
            return None;
        }
        Some(Versor {
            x: q.x / n,
            y: q.y / n,
            z: q.z / n,
            w: q.w / n,
        })
    }
    pub(crate) fn canonical(&self) -> Versor {
        if self.w < 0.0 {
            return Versor {
                x: -self.x,
                y: -self.y,
                z: -self.z,
                w: -self.w,
            };
        }
        *self
    }
}
pub(crate) fn versor_from_parameters(parameters: &[f64]) -> Result<Versor, Error> {
    let mut right = vec_from(parameters);
    const EPSILON: f64 = 1e-10;
    let norm = right.length();
    if norm >= 1.0 - EPSILON {
        right = right.scale(1.0 / (norm + EPSILON * norm));
    }
    Versor::from_right_part(right)
}
pub(crate) fn compose_versor_update(
    current: &Versor,
    update: &[f64],
    factor: f64,
) -> Result<Versor, Error> {
    let axis = vec_from(update);
    let norm = axis.length();
    let mut step = Versor::identity();
    if norm != 0.0 {
        step = Versor::from_axis_angle(axis, factor * norm)?;
    }
    match current.mul(&step).normalized() {
        Some(next) => Ok(next.canonical()),
        None => Err(Error::parameters("versor update degenerated")),
    }
}
pub(crate) fn check_rotation(m: &Matrix3, tolerance: f64) -> Result<(), Error> {
    if !m.finite() || !finite(tolerance) || tolerance < 0.0 {
        return Err(Error::matrix("non-finite rotation matrix or tolerance"));
    }
    if !m.orthogonal(tolerance) {
        return Err(Error::matrix(format!(
            "matrix is not orthogonal within {}",
            float_v(tolerance)
        )));
    }
    if m.determinant() <= 0.0 {
        return Err(Error::matrix("matrix is a reflection"));
    }
    Ok(())
}
