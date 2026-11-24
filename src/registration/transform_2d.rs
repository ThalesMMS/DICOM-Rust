use super::matrix::{MAX_CONDITION, finite, finite_slice};
use super::transform::{Error, ErrorKind, Kind, check_length, check_update};
use crate::fmath;
pub type Point2 = [f64; 2];
fn add2(p: Point2, q: Point2) -> Point2 {
    [p[0] + q[0], p[1] + q[1]]
}
fn sub2(p: Point2, q: Point2) -> Point2 {
    [p[0] - q[0], p[1] - q[1]]
}
fn scale2(p: Point2, s: f64) -> Point2 {
    [p[0] * s, p[1] * s]
}
fn finite_point2(p: Point2) -> bool {
    finite(p[0]) && finite(p[1])
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Matrix2(pub [[f64; 2]; 2]);
impl Matrix2 {
    pub const fn identity() -> Matrix2 {
        Matrix2([[1.0, 0.0], [0.0, 1.0]])
    }
    pub fn mul(&self, n: &Matrix2) -> Matrix2 {
        let m = &self.0;
        let mut o = [[0.0; 2]; 2];
        for (i, row) in o.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = m[i][0] * n.0[0][j] + m[i][1] * n.0[1][j];
            }
        }
        Matrix2(o)
    }
    pub fn mul_vec(&self, p: Point2) -> Point2 {
        let m = &self.0;
        [
            m[0][0] * p[0] + m[0][1] * p[1],
            m[1][0] * p[0] + m[1][1] * p[1],
        ]
    }
    pub fn finite(&self) -> bool {
        finite_slice(&self.0[0]) && finite_slice(&self.0[1])
    }
    pub fn determinant(&self) -> f64 {
        let m = &self.0;
        m[0][0] * m[1][1] - m[0][1] * m[1][0]
    }
    fn infinity_norm(&self) -> f64 {
        let m = &self.0;
        fmath::max(m[0][0].abs() + m[0][1].abs(), m[1][0].abs() + m[1][1].abs())
    }
    pub fn inverse(&self) -> Result<Matrix2, Error> {
        if !self.finite() {
            return Err(Error::bare(ErrorKind::InvalidMatrix));
        }
        let singular = || Error::bare(ErrorKind::SingularMatrix);
        let mut s = 0.0;
        for row in &self.0 {
            for v in row {
                s = fmath::max(s, v.abs());
            }
        }
        if s == 0.0 {
            return Err(singular());
        }
        let a = Matrix2(self.0.map(|row| row.map(|v| v / s)));
        let d = a.determinant();
        let norm = a.infinity_norm();
        if !finite(d) || d.abs() <= 1e-15 * norm * norm {
            return Err(singular());
        }
        let a = &a.0;
        let b = Matrix2([[a[1][1] / d, -a[0][1] / d], [-a[1][0] / d, a[0][0] / d]]);
        let condition = norm * b.infinity_norm();
        if !finite(condition) || condition > MAX_CONDITION {
            return Err(singular());
        }
        let b = Matrix2(b.0.map(|row| row.map(|v| v / s)));
        if !b.finite() {
            return Err(singular());
        }
        Ok(b)
    }
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Jacobian2D {
    pub columns: usize,
    pub values: Vec<f64>,
}
impl Jacobian2D {
    pub fn at(&self, r: usize, c: usize) -> f64 {
        self.values[r * self.columns + c]
    }
    fn set(&mut self, r: usize, c: usize, v: f64) {
        self.values[r * self.columns + c] = v;
    }
    fn reset(&mut self, n: usize) {
        self.values.clear();
        self.values.resize(2 * n, 0.0);
        self.columns = n;
    }
}
pub trait Transform2D {
    fn kind(&self) -> Kind;
    fn transform_point(&self, p: Point2) -> Point2;
    fn number_of_parameters(&self) -> usize;
    fn parameters(&self) -> Vec<f64>;
    fn set_parameters(&mut self, p: &[f64]) -> Result<(), Error>;
    fn fixed_parameters(&self) -> Vec<f64>;
    fn set_fixed_parameters(&mut self, p: &[f64]) -> Result<(), Error>;
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error>;
    fn jacobian_with_respect_to_parameters(&self, p: Point2, out: &mut Jacobian2D);
    fn jacobian_with_respect_to_position(&self, p: Point2) -> Matrix2;
}
pub trait MatrixTransform2D: Transform2D {
    fn matrix(&self) -> Matrix2;
    fn offset(&self) -> Point2;
    fn center(&self) -> Point2;
    fn translation(&self) -> Point2;
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Linear2D {
    kind: Kind,
    matrix: Matrix2,
    center: Point2,
    translation: Point2,
    offset: Point2,
    angle: f64,
    scale: f64,
}
impl Linear2D {
    fn of(kind: Kind, scale: f64) -> Linear2D {
        Linear2D {
            kind,
            matrix: Matrix2::identity(),
            center: [0.0; 2],
            translation: [0.0; 2],
            offset: [0.0; 2],
            angle: 0.0,
            scale,
        }
    }
    pub fn translation_2d() -> Linear2D {
        Linear2D::of(Kind::TRANSLATION, 0.0)
    }
    pub fn euler_2d() -> Linear2D {
        Linear2D::of(Kind::EULER_2D, 0.0)
    }
    pub fn similarity_2d() -> Linear2D {
        Linear2D::of(Kind::SIMILARITY_2D, 1.0)
    }
    pub fn affine_2d() -> Linear2D {
        Linear2D::of(Kind::AFFINE, 0.0)
    }
    pub fn transform_vector(&self, p: Point2) -> Point2 {
        self.matrix.mul_vec(p)
    }
    fn set(&mut self, p: &[f64], c: Point2) -> Result<(), Error> {
        check_length(
            p,
            self.number_of_parameters(),
            &format!("{} parameters", self.kind),
        )?;
        if !finite_point2(c) {
            return Err(Error::parameters("non-finite 2D centre"));
        }
        let mut n = *self;
        n.center = c;
        if self.kind == Kind::TRANSLATION {
            n.matrix = Matrix2::identity();
            n.translation = [p[0], p[1]];
            n.center = [0.0; 2];
        } else if self.kind == Kind::EULER_2D || self.kind == Kind::SIMILARITY_2D {
            n.scale = 1.0;
            n.angle = p[0];
            let mut start = 1;
            if self.kind == Kind::SIMILARITY_2D {
                n.scale = p[0];
                n.angle = p[1];
                start = 2;
            }
            let (s, co) = fmath::sincos(n.angle);
            n.matrix = Matrix2([[n.scale * co, -n.scale * s], [n.scale * s, n.scale * co]]);
            n.translation = [p[start], p[start + 1]];
        } else if self.kind == Kind::AFFINE {
            n.matrix = Matrix2([[p[0], p[1]], [p[2], p[3]]]);
            n.translation = [p[4], p[5]];
        } else {
            return Err(Error::bare(ErrorKind::UnsupportedTransform));
        }
        n.offset = sub2(add2(n.translation, n.center), n.matrix.mul_vec(n.center));
        if !n.matrix.finite() || !finite_point2(n.offset) {
            return Err(Error::parameters("derived 2D matrix/offset overflow"));
        }
        *self = n;
        Ok(())
    }
    pub fn set_center(&mut self, c: Point2) -> Result<(), Error> {
        if self.kind == Kind::TRANSLATION && c != [0.0; 2] {
            return Err(Error::parameters("translation has no centre"));
        }
        let p = self.parameters();
        self.set(&p, c)
    }
    pub fn set_translation(&mut self, t: Point2) -> Result<(), Error> {
        let mut p = self.parameters();
        let n = p.len();
        p[n - 2] = t[0];
        p[n - 1] = t[1];
        self.set_parameters(&p)
    }
    pub fn set_offset(&mut self, o: Point2) -> Result<(), Error> {
        if !finite_point2(o) {
            return Err(Error::bare(ErrorKind::InvalidParameters));
        }
        self.set_translation(add2(sub2(o, self.center), self.matrix.mul_vec(self.center)))
    }
    pub fn set_matrix(&mut self, m: &Matrix2) -> Result<(), Error> {
        if self.kind != Kind::AFFINE {
            return Err(Error::bare(ErrorKind::UnsupportedTransform));
        }
        let m = &m.0;
        self.set_parameters(&[
            m[0][0],
            m[0][1],
            m[1][0],
            m[1][1],
            self.translation[0],
            self.translation[1],
        ])
    }
    pub fn inverse(&self) -> Result<Linear2D, Error> {
        let m = self.matrix.inverse()?;
        let o = scale2(m.mul_vec(self.offset), -1.0);
        let mut t = Linear2D::affine_2d();
        t.center = self.center;
        t.set_matrix(&m)?;
        t.set_offset(o)?;
        Ok(t)
    }
}
impl MatrixTransform2D for Linear2D {
    fn matrix(&self) -> Matrix2 {
        self.matrix
    }
    fn offset(&self) -> Point2 {
        self.offset
    }
    fn center(&self) -> Point2 {
        self.center
    }
    fn translation(&self) -> Point2 {
        self.translation
    }
}
impl Transform2D for Linear2D {
    fn kind(&self) -> Kind {
        self.kind
    }
    fn transform_point(&self, p: Point2) -> Point2 {
        add2(self.matrix.mul_vec(p), self.offset)
    }
    fn number_of_parameters(&self) -> usize {
        if self.kind == Kind::TRANSLATION {
            2
        } else if self.kind == Kind::EULER_2D {
            3
        } else if self.kind == Kind::SIMILARITY_2D {
            4
        } else {
            6
        }
    }
    fn parameters(&self) -> Vec<f64> {
        let t = self.translation;
        let m = &self.matrix.0;
        if self.kind == Kind::TRANSLATION {
            vec![t[0], t[1]]
        } else if self.kind == Kind::EULER_2D {
            vec![self.angle, t[0], t[1]]
        } else if self.kind == Kind::SIMILARITY_2D {
            vec![self.scale, self.angle, t[0], t[1]]
        } else {
            vec![m[0][0], m[0][1], m[1][0], m[1][1], t[0], t[1]]
        }
    }
    fn set_parameters(&mut self, p: &[f64]) -> Result<(), Error> {
        self.set(p, self.center)
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        if self.kind == Kind::TRANSLATION {
            return Vec::new();
        }
        vec![self.center[0], self.center[1]]
    }
    fn set_fixed_parameters(&mut self, p: &[f64]) -> Result<(), Error> {
        if self.kind == Kind::TRANSLATION {
            return check_length(p, 0, "Translation2D fixed parameters");
        }
        check_length(p, 2, "2D fixed parameters")?;
        self.set_center([p[0], p[1]])
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        check_update(update, self.number_of_parameters(), factor)?;
        let mut p = self.parameters();
        for (v, u) in p.iter_mut().zip(update) {
            *v += factor * u;
        }
        self.set_parameters(&p)
    }
    fn jacobian_with_respect_to_parameters(&self, p: Point2, j: &mut Jacobian2D) {
        j.reset(self.number_of_parameters());
        let r = sub2(p, self.center);
        if self.kind == Kind::TRANSLATION {
            j.set(0, 0, 1.0);
            j.set(1, 1, 1.0);
        } else if self.kind == Kind::AFFINE {
            for i in 0..2 {
                for (k, v) in r.iter().enumerate() {
                    j.set(i, 2 * i + k, *v);
                }
                j.set(i, 4 + i, 1.0);
            }
        } else {
            let (s, c) = fmath::sincos(self.angle);
            let rot = [c * r[0] - s * r[1], s * r[0] + c * r[1]];
            let mut d = [-s * r[0] - c * r[1], c * r[0] - s * r[1]];
            let (mut angle_col, mut trans_col) = (0, 1);
            if self.kind == Kind::SIMILARITY_2D {
                j.set(0, 0, rot[0]);
                j.set(1, 0, rot[1]);
                d = scale2(d, self.scale);
                angle_col = 1;
                trans_col = 2;
            }
            j.set(0, angle_col, d[0]);
            j.set(1, angle_col, d[1]);
            j.set(0, trans_col, 1.0);
            j.set(1, trans_col + 1, 1.0);
        }
    }
    fn jacobian_with_respect_to_position(&self, _p: Point2) -> Matrix2 {
        self.matrix
    }
}
type Apply2D<'a> = dyn Fn(&mut Linear2D, &[f64]) -> Result<(), Error> + 'a;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Composite2D {
    transforms: Vec<Linear2D>,
    optimize: Vec<bool>,
}
impl Composite2D {
    pub fn new(transforms: &[Linear2D]) -> Result<Composite2D, Error> {
        let mut c = Composite2D::default();
        for t in transforms {
            c.append(*t)?;
        }
        Ok(c)
    }
    pub fn len(&self) -> usize {
        self.transforms.len()
    }
    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }
    pub fn transform(&self, i: usize) -> &Linear2D {
        &self.transforms[i]
    }
    #[doc(hidden)]
    pub fn transform_mut(&mut self, i: usize) -> &mut Linear2D {
        &mut self.transforms[i]
    }
    pub fn append(&mut self, t: Linear2D) -> Result<(), Error> {
        if self.transforms.len() >= 1024 {
            return Err(Error::parameters("2D composite component limit"));
        }
        self.transforms.push(t);
        self.optimize.push(true);
        Ok(())
    }
    pub fn optimized(&self, i: usize) -> bool {
        self.optimize[i]
    }
    pub fn set_optimized(&mut self, i: i64, on: bool) -> Result<(), Error> {
        if i < 0 || i as usize >= self.len() {
            return Err(Error::bare(ErrorKind::InvalidParameters));
        }
        self.optimize[i as usize] = on;
        Ok(())
    }
    fn apply(&mut self, p: &[f64], fixed: bool, f: &Apply2D<'_>) -> Result<(), Error> {
        let n = if fixed {
            self.fixed_parameters().len()
        } else {
            self.number_of_parameters()
        };
        check_length(p, n, "2D composite parameters")?;
        let count = |t: &Linear2D| {
            if fixed {
                t.fixed_parameters().len()
            } else {
                t.number_of_parameters()
            }
        };
        let mut at = 0;
        for (i, t) in self.transforms.iter().enumerate() {
            if !fixed && !self.optimize[i] {
                continue;
            }
            let n = count(t);
            let mut clone = *t;
            f(&mut clone, &p[at..at + n]).map_err(|e| e.prefixed(format!("component{i}")))?;
            at += n;
        }
        at = 0;
        for (i, t) in self.transforms.iter_mut().enumerate() {
            if !fixed && !self.optimize[i] {
                continue;
            }
            let n = count(t);
            f(t, &p[at..at + n])?;
            at += n;
        }
        Ok(())
    }
}
impl Transform2D for Composite2D {
    fn kind(&self) -> Kind {
        Kind::COMPOSITE
    }
    fn transform_point(&self, mut p: Point2) -> Point2 {
        for t in &self.transforms {
            p = t.transform_point(p);
        }
        p
    }
    fn number_of_parameters(&self) -> usize {
        self.transforms
            .iter()
            .zip(&self.optimize)
            .filter(|(_, on)| **on)
            .map(|(t, _)| t.number_of_parameters())
            .sum()
    }
    fn parameters(&self) -> Vec<f64> {
        let mut p = Vec::with_capacity(self.number_of_parameters());
        for (t, on) in self.transforms.iter().zip(&self.optimize) {
            if *on {
                p.extend(t.parameters());
            }
        }
        p
    }
    fn set_parameters(&mut self, p: &[f64]) -> Result<(), Error> {
        self.apply(p, false, &|t, p| t.set_parameters(p))
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        let mut p = Vec::new();
        for t in &self.transforms {
            p.extend(t.fixed_parameters());
        }
        p
    }
    fn set_fixed_parameters(&mut self, p: &[f64]) -> Result<(), Error> {
        self.apply(p, true, &|t, p| t.set_fixed_parameters(p))
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        check_update(update, self.number_of_parameters(), factor)?;
        self.apply(update, false, &|t, p| t.update_parameters(p, factor))
    }
    fn jacobian_with_respect_to_parameters(&self, mut p: Point2, j: &mut Jacobian2D) {
        j.reset(self.number_of_parameters());
        let mut points = vec![[0.0; 2]; self.len()];
        let mut cols = vec![0usize; self.len()];
        let mut at = 0;
        for (i, t) in self.transforms.iter().enumerate() {
            points[i] = p;
            cols[i] = at;
            if self.optimize[i] {
                at += t.number_of_parameters();
            }
            p = t.transform_point(p);
        }
        let mut suffix = Matrix2::identity();
        let mut part = Jacobian2D::default();
        for i in (0..self.len()).rev() {
            let t = &self.transforms[i];
            if self.optimize[i] {
                t.jacobian_with_respect_to_parameters(points[i], &mut part);
                for k in 0..part.columns {
                    let v = suffix.mul_vec([part.at(0, k), part.at(1, k)]);
                    j.set(0, cols[i] + k, v[0]);
                    j.set(1, cols[i] + k, v[1]);
                }
            }
            suffix = suffix.mul(&t.jacobian_with_respect_to_position(points[i]));
        }
    }
    fn jacobian_with_respect_to_position(&self, mut p: Point2) -> Matrix2 {
        let mut m = Matrix2::identity();
        for t in &self.transforms {
            m = t.jacobian_with_respect_to_position(p).mul(&m);
            p = t.transform_point(p);
        }
        m
    }
}
