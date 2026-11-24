use super::affine::Affine;
use super::matrix::{Jacobian, Matrix3, finite, finite_vec};
use super::transform::{
    Error, ErrorKind, Kind, MatrixOffset, MatrixTransform, Transform, check_length, check_update,
    rotation_translation_jacobian, vec_from,
};
use super::versor::{Versor, compose_versor_update, versor_from_parameters};
use crate::render::Vec3;
use std::any::Any;
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleVersor3D {
    base: MatrixOffset,
    versor: Versor,
    scale: Vec3,
    skew: [f64; 6],
    derivatives: [Matrix3; 3],
    with_skew: bool,
}
const SKEW_POSITIONS: [[usize; 2]; 6] = [[0, 1], [0, 2], [1, 0], [1, 2], [2, 0], [2, 1]];
impl Default for ScaleVersor3D {
    fn default() -> Self {
        ScaleVersor3D::new()
    }
}
impl ScaleVersor3D {
    pub fn new() -> ScaleVersor3D {
        ScaleVersor3D::build(false)
    }
    pub fn new_skew() -> ScaleVersor3D {
        ScaleVersor3D::build(true)
    }
    fn build(with_skew: bool) -> ScaleVersor3D {
        let mut b = ScaleVersor3D {
            base: MatrixOffset::identity(),
            versor: Versor::identity(),
            scale: Vec3::new(1.0, 1.0, 1.0),
            skew: [0.0; 6],
            derivatives: [Matrix3::default(); 3],
            with_skew,
        };
        let mut p = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        if with_skew {
            p.extend_from_slice(&[0.0; 6]);
        }
        let _ = b.commit(&p, Vec3::ZERO, Versor::identity());
        b
    }
    pub fn with_skew(&self) -> bool {
        self.with_skew
    }
    fn set(&mut self, p: &[f64], c: Vec3) -> Result<(), Error> {
        check_length(
            p,
            self.number_of_parameters(),
            &format!("{} parameters", self.kind()),
        )?;
        if !finite_vec(c) || !finite(vec_from(p).length()) {
            return Err(Error::parameters("centre/versor norm overflow"));
        }
        let q = versor_from_parameters(p)?;
        self.commit(p, c, q)
    }
    fn commit(&mut self, p: &[f64], c: Vec3, q: Versor) -> Result<(), Error> {
        check_length(p, self.number_of_parameters(), "scale versor parameters")?;
        if !finite_vec(c) {
            return Err(Error::parameters("non-finite centre"));
        }
        let mut next = self.clone();
        next.versor = q;
        next.base.center = c;
        next.base.translation = vec_from(&p[3..]);
        next.scale = vec_from(&p[6..]);
        next.skew = [0.0; 6];
        if self.with_skew {
            next.skew.copy_from_slice(&p[9..15]);
        }
        next.base.matrix = q.matrix();
        let scales = [next.scale.x, next.scale.y, next.scale.z];
        for (i, s) in scales.iter().enumerate() {
            next.base.matrix.0[i][i] += s - 1.0;
        }
        for (i, pos) in SKEW_POSITIONS.iter().enumerate() {
            next.base.matrix.0[pos[0]][pos[1]] += next.skew[i];
        }
        next.base.compute_offset();
        if !next.base.matrix.finite() || !finite_vec(next.base.offset) {
            return Err(Error::parameters(
                "derived scale/skew matrix or offset overflow",
            ));
        }
        next.derivatives = q.right_part_derivatives();
        *self = next;
        Ok(())
    }
    pub fn set_center(&mut self, c: Vec3) -> Result<(), Error> {
        let p = self.parameters();
        self.commit(&p, c, self.versor)
    }
    pub fn set_translation(&mut self, t: Vec3) -> Result<(), Error> {
        let mut p = self.parameters();
        p[3] = t.x;
        p[4] = t.y;
        p[5] = t.z;
        self.commit(&p, self.base.center, self.versor)
    }
    pub fn set_offset(&mut self, o: Vec3) -> Result<(), Error> {
        if !finite_vec(o) {
            return Err(Error::parameters("non-finite offset"));
        }
        let c = self.base.center;
        self.set_translation(o.sub(c).add(self.base.matrix.mul_vec(c)))
    }
    pub fn inverse(&self) -> Result<Affine, Error> {
        let (m, offset) = self.base.inverse()?;
        let mut out = Affine::new();
        out.base.center = self.base.center;
        out.base.matrix = m;
        let t = offset.sub(out.base.center).add(m.mul_vec(out.base.center));
        if !finite_vec(offset) || !finite_vec(t) {
            return Err(Error::bare(ErrorKind::SingularMatrix));
        }
        out.base.translation = t;
        out.base.offset = offset;
        Ok(out)
    }
}
impl MatrixTransform for ScaleVersor3D {
    fn matrix(&self) -> Matrix3 {
        self.base.matrix
    }
    fn offset(&self) -> Vec3 {
        self.base.offset
    }
    fn center(&self) -> Vec3 {
        self.base.center
    }
    fn translation(&self) -> Vec3 {
        self.base.translation
    }
}
impl Transform for ScaleVersor3D {
    fn kind(&self) -> Kind {
        if self.with_skew {
            Kind::SCALE_SKEW_VERSOR_3D
        } else {
            Kind::SCALE_VERSOR_3D
        }
    }
    fn transform_point(&self, point: Vec3) -> Vec3 {
        self.base.transform_point(point)
    }
    fn number_of_parameters(&self) -> usize {
        if self.with_skew { 15 } else { 9 }
    }
    fn parameters(&self) -> Vec<f64> {
        let t = self.base.translation;
        let mut p = vec![
            self.versor.x,
            self.versor.y,
            self.versor.z,
            t.x,
            t.y,
            t.z,
            self.scale.x,
            self.scale.y,
            self.scale.z,
        ];
        if self.with_skew {
            p.extend_from_slice(&self.skew);
        }
        p
    }
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error> {
        self.set(parameters, self.base.center)
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        self.base.center_fixed_parameters()
    }
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error> {
        check_length(fixed, 3, "scale versor fixed parameters")?;
        self.set_center(vec_from(fixed))
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        check_update(update, self.number_of_parameters(), factor)?;
        let q = compose_versor_update(&self.versor, update, factor)?;
        let mut p = self.parameters();
        p[0] = q.x;
        p[1] = q.y;
        p[2] = q.z;
        for i in 3..p.len() {
            p[i] += factor * update[i];
        }
        self.commit(&p, self.base.center, q)
    }
    fn jacobian_with_respect_to_parameters(&self, point: Vec3, out: &mut Jacobian) {
        out.reset(self.number_of_parameters());
        let r = point.sub(self.base.center);
        rotation_translation_jacobian(out, &self.derivatives, r, 3);
        let rv = [r.x, r.y, r.z];
        for (i, v) in rv.iter().enumerate() {
            out.set(i, 6 + i, *v);
        }
        if self.with_skew {
            for (i, pos) in SKEW_POSITIONS.iter().enumerate() {
                out.set(pos[0], 9 + i, rv[pos[1]]);
            }
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
