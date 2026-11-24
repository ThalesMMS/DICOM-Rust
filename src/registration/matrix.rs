use super::transform::{Error, ErrorKind};
use crate::fmath;
use crate::render::Vec3;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Matrix3(pub [[f64; 3]; 3]);
pub(crate) const MAX_CONDITION: f64 = 1e10;
pub(crate) fn finite(v: f64) -> bool {
    v.is_finite()
}
pub(crate) fn finite_vec(v: Vec3) -> bool {
    finite(v.x) && finite(v.y) && finite(v.z)
}
pub(crate) fn finite_slice(values: &[f64]) -> bool {
    values.iter().all(|v| finite(*v))
}
impl Matrix3 {
    pub const fn identity() -> Matrix3 {
        Matrix3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
    }
    pub fn mul(&self, n: &Matrix3) -> Matrix3 {
        let m = &self.0;
        let mut out = [[0.0; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = m[i][0] * n.0[0][j] + m[i][1] * n.0[1][j] + m[i][2] * n.0[2][j];
            }
        }
        Matrix3(out)
    }
    pub fn mul_vec(&self, v: Vec3) -> Vec3 {
        let m = &self.0;
        Vec3::new(
            m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
            m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
            m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
        )
    }
    pub fn scale(&self, s: f64) -> Matrix3 {
        Matrix3(self.0.map(|row| row.map(|v| v * s)))
    }
    pub fn transpose(&self) -> Matrix3 {
        let m = &self.0;
        Matrix3([
            [m[0][0], m[1][0], m[2][0]],
            [m[0][1], m[1][1], m[2][1]],
            [m[0][2], m[1][2], m[2][2]],
        ])
    }
    pub fn determinant(&self) -> f64 {
        let m = &self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
    pub fn finite(&self) -> bool {
        self.0.iter().all(|row| finite_slice(row))
    }
    pub fn inverse(&self) -> Result<Matrix3, Error> {
        if !self.finite() {
            return Err(Error::bare(ErrorKind::InvalidMatrix));
        }
        let m = &self.0;
        let det = self.determinant();
        let norm = self.infinity_norm();
        if norm == 0.0 || !finite(det) || det.abs() <= 1e-15 * norm * norm * norm {
            return Err(Error::bare(ErrorKind::SingularMatrix));
        }
        let inv = 1.0 / det;
        let out = Matrix3([
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
            ],
        ]);
        let condition = norm * out.infinity_norm();
        if !out.finite() || !finite(condition) || condition > MAX_CONDITION {
            return Err(Error::bare(ErrorKind::SingularMatrix));
        }
        Ok(out)
    }
    pub(crate) fn infinity_norm(&self) -> f64 {
        let mut maximum = 0.0;
        for row in &self.0 {
            maximum = fmath::max(maximum, row[0].abs() + row[1].abs() + row[2].abs());
        }
        maximum
    }
    pub(crate) fn orthogonal(&self, tolerance: f64) -> bool {
        let product = self.mul(&self.transpose());
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                if !((product.0[i][j] - want).abs() <= tolerance) {
                    return false;
                }
            }
        }
        true
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Jacobian {
    pub columns: usize,
    pub values: Vec<f64>,
}
impl Jacobian {
    pub fn new(columns: usize) -> Jacobian {
        Jacobian {
            columns,
            values: vec![0.0; 3 * columns],
        }
    }
    pub fn at(&self, row: usize, column: usize) -> f64 {
        self.values[row * self.columns + column]
    }
    pub(crate) fn set(&mut self, row: usize, column: usize, v: f64) {
        self.values[row * self.columns + column] = v;
    }
    pub(crate) fn reset(&mut self, columns: usize) {
        self.values.clear();
        self.values.resize(3 * columns, 0.0);
        self.columns = columns;
    }
    pub(crate) fn set_column(&mut self, column: usize, v: Vec3) {
        self.set(0, column, v.x);
        self.set(1, column, v.y);
        self.set(2, column, v.z);
    }
    pub(crate) fn column(&self, column: usize) -> Vec3 {
        Vec3::new(self.at(0, column), self.at(1, column), self.at(2, column))
    }
}
