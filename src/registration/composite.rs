use super::affine::Affine;
use super::matrix::{Jacobian, Matrix3, finite_vec};
use super::transform::{Error, ErrorKind, Kind, Transform, check_update};
use crate::render::Vec3;
use std::any::Any;
type Apply<'a> = dyn Fn(&mut dyn Transform, &[f64]) -> Result<(), Error> + 'a;
#[derive(Default)]
pub struct Composite {
    transforms: Vec<Box<dyn Transform>>,
    optimize: Vec<bool>,
}
impl Composite {
    pub fn new(transforms: Vec<Box<dyn Transform>>) -> Composite {
        let optimize = vec![true; transforms.len()];
        Composite {
            transforms,
            optimize,
        }
    }
    pub fn append(&mut self, t: Option<Box<dyn Transform>>) -> Result<(), Error> {
        let Some(t) = t else {
            return Err(Error::new(
                ErrorKind::UnsupportedTransform,
                "nil transform in composite",
            ));
        };
        self.transforms.push(t);
        self.optimize.push(true);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.transforms.len()
    }
    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }
    pub fn transform(&self, index: usize) -> &dyn Transform {
        self.transforms[index].as_ref()
    }
    #[doc(hidden)]
    pub fn transform_mut(&mut self, index: usize) -> &mut dyn Transform {
        self.transforms[index].as_mut()
    }
    pub fn optimized(&self, index: usize) -> bool {
        self.optimize[index]
    }
    pub fn set_optimized(&mut self, index: i64, on: bool) -> Result<(), Error> {
        if index < 0 || index as usize >= self.transforms.len() {
            return Err(Error::parameters(format!(
                "composite index {index} out of range"
            )));
        }
        self.optimize[index as usize] = on;
        Ok(())
    }
    fn apply_to_optimized(&mut self, values: &[f64], apply: &Apply<'_>) -> Result<(), Error> {
        let mut offset = 0;
        for (i, t) in self.transforms.iter().enumerate() {
            if !self.optimize[i] {
                continue;
            }
            let n = t.number_of_parameters();
            let mut clone = t.clone_box();
            apply(clone.as_mut(), &values[offset..offset + n])
                .map_err(|e| e.prefixed(format!("composite transform {i}")))?;
            offset += n;
        }
        offset = 0;
        for (i, t) in self.transforms.iter_mut().enumerate() {
            if !self.optimize[i] {
                continue;
            }
            let n = t.number_of_parameters();
            apply(t.as_mut(), &values[offset..offset + n])
                .map_err(|e| e.prefixed(format!("composite transform {i}")))?;
            offset += n;
        }
        Ok(())
    }
    pub fn collapse_to_affine(&self) -> Result<Affine, Error> {
        let (m, offset) = self.linear()?;
        let mut out = Affine::new();
        out.base.matrix = m;
        out.set_offset(offset)?;
        Ok(out)
    }
    pub(crate) fn linear(&self) -> Result<(Matrix3, Vec3), Error> {
        let mut m = Matrix3::identity();
        let mut offset = Vec3::ZERO;
        for (i, t) in self.transforms.iter().enumerate() {
            let (tm, to) = if let Some(v) = t.as_matrix_transform() {
                (v.matrix(), v.offset())
            } else if let Some(v) = t.as_any().downcast_ref::<Composite>() {
                v.linear()?
            } else {
                return Err(Error::new(
                    ErrorKind::UnsupportedTransform,
                    format!("composite transform {i} ({}) is not linear", t.kind()),
                ));
            };
            m = tm.mul(&m);
            offset = tm.mul_vec(offset).add(to);
        }
        if !m.finite() || !finite_vec(offset) {
            return Err(Error::matrix("composite is not finite"));
        }
        Ok((m, offset))
    }
}
impl Transform for Composite {
    fn kind(&self) -> Kind {
        Kind::COMPOSITE
    }
    fn transform_point(&self, mut point: Vec3) -> Vec3 {
        for t in &self.transforms {
            point = t.transform_point(point);
        }
        point
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
        let mut out = Vec::with_capacity(self.number_of_parameters());
        for (t, on) in self.transforms.iter().zip(&self.optimize) {
            if *on {
                out.extend(t.parameters());
            }
        }
        out
    }
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error> {
        let want = self.number_of_parameters();
        if parameters.len() != want {
            return Err(Error::parameters(format!(
                "composite parameters have {} values, want {want}",
                parameters.len()
            )));
        }
        self.apply_to_optimized(parameters, &|t, part| t.set_parameters(part))
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        let mut out = Vec::new();
        for t in &self.transforms {
            out.extend(t.fixed_parameters());
        }
        out
    }
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error> {
        let total: usize = self
            .transforms
            .iter()
            .map(|t| t.fixed_parameters().len())
            .sum();
        if fixed.len() != total {
            return Err(Error::parameters(format!(
                "composite fixed parameters have {} values, want {total}",
                fixed.len()
            )));
        }
        let mut offset = 0;
        for (i, t) in self.transforms.iter().enumerate() {
            let n = t.fixed_parameters().len();
            let mut clone = t.clone_box();
            clone
                .set_fixed_parameters(&fixed[offset..offset + n])
                .map_err(|e| e.prefixed(format!("composite transform {i}")))?;
            offset += n;
        }
        offset = 0;
        for t in &mut self.transforms {
            let n = t.fixed_parameters().len();
            t.set_fixed_parameters(&fixed[offset..offset + n])?;
            offset += n;
        }
        Ok(())
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        check_update(update, self.number_of_parameters(), factor)?;
        self.apply_to_optimized(update, &|t, part| t.update_parameters(part, factor))
    }
    fn jacobian_with_respect_to_parameters(&self, mut point: Vec3, out: &mut Jacobian) {
        out.reset(self.number_of_parameters());
        let mut local = Jacobian::default();
        let mut filled = 0;
        for (i, t) in self.transforms.iter().enumerate() {
            if filled > 0 {
                let position = t.jacobian_with_respect_to_position(point);
                for column in 0..filled {
                    let v = position.mul_vec(out.column(column));
                    out.set_column(column, v);
                }
            }
            if self.optimize[i] {
                t.jacobian_with_respect_to_parameters(point, &mut local);
                for column in 0..local.columns {
                    out.set_column(filled + column, local.column(column));
                }
                filled += local.columns;
            }
            point = t.transform_point(point);
        }
    }
    fn jacobian_with_respect_to_position(&self, mut point: Vec3) -> Matrix3 {
        let mut out = Matrix3::identity();
        for t in &self.transforms {
            out = t.jacobian_with_respect_to_position(point).mul(&out);
            point = t.transform_point(point);
        }
        out
    }
    fn clone_box(&self) -> Box<dyn Transform> {
        Box::new(Composite {
            transforms: self.transforms.iter().map(|t| t.clone_box()).collect(),
            optimize: self.optimize.clone(),
        })
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
