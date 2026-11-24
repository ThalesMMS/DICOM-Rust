use super::error::RegError;
use super::image::{Image, ints_text};
use super::matrix::{Jacobian, Matrix3, finite_slice, finite_vec};
use super::transform::{Error, Kind, Transform, check_length, check_update, vec_from};
use crate::fmath;
use crate::numtext::format_float;
use crate::render::Vec3;
use std::any::Any;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DisplacementField {
    pub size: [i64; 3],
    pub origin: Vec3,
    pub spacing: Vec3,
    pub direction: Matrix3,
    pub vectors: Vec<Vec3>,
}
impl DisplacementField {
    pub fn at(&self, x: usize, y: usize, z: usize) -> Vec3 {
        let (nx, ny) = (self.size[0] as usize, self.size[1] as usize);
        self.vectors[(z * ny + y) * nx + x]
    }
    pub(crate) fn grid(&self) -> Result<Image, RegError> {
        Image::geometry(self.size, self.origin, self.spacing, self.direction)
    }
    pub fn displacement_at(&self, p: Vec3) -> Vec3 {
        match self.grid() {
            Ok(g) => sample_vectors(&self.vectors, g.size, g.continuous_index(p)),
            Err(_) => Vec3::ZERO,
        }
    }
}
pub(crate) fn sample_vectors(vectors: &[Vec3], size: [usize; 3], index: Vec3) -> Vec3 {
    let mut base = [0i64; 3];
    let mut weight = [0f64; 3];
    for (axis, value) in [index.x, index.y, index.z].into_iter().enumerate() {
        let last = size[axis] as i64 - 1;
        let value = fmath::min(fmath::max(value, 0.0), last as f64);
        base[axis] = fmath::f64_to_i64(value.floor());
        weight[axis] = value - base[axis] as f64;
        if base[axis] >= last {
            base[axis] = last;
            weight[axis] = 0.0;
        }
    }
    let mut out = Vec3::ZERO;
    for corner in 0..8 {
        let mut w = 1.0;
        let mut at = base;
        for axis in 0..3 {
            if corner >> axis & 1 == 1 {
                if weight[axis] == 0.0 {
                    w = 0.0;
                    break;
                }
                w *= weight[axis];
                at[axis] += 1;
            } else {
                w *= 1.0 - weight[axis];
            }
        }
        if w != 0.0 {
            let i = (at[2] * size[1] as i64 + at[1]) * size[0] as i64 + at[0];
            out = out.add(vectors[i as usize].scale(w));
        }
    }
    out
}
impl Kind {
    pub const DISPLACEMENT_FIELD: Kind = Kind("DisplacementFieldTransform");
}
pub const DISPLACEMENT_FIELD_FIXED_PARAMETERS: usize = 18;
#[derive(Debug, Clone, PartialEq)]
pub struct DisplacementFieldTransform {
    field: DisplacementField,
    grid: Image,
}
impl DisplacementFieldTransform {
    pub fn new(field: Option<&DisplacementField>) -> Result<DisplacementFieldTransform, Error> {
        let Some(field) = field else {
            return Err(Error::parameters("nil displacement field"));
        };
        let mut t = DisplacementFieldTransform {
            field: DisplacementField::default(),
            grid: Image::default(),
        };
        t.set_geometry(field.size, field.origin, field.spacing, field.direction)?;
        if field.vectors.len() != t.field.vectors.len() {
            return Err(Error::parameters(format!(
                "{} vectors for size {}",
                field.vectors.len(),
                ints_text(&field.size)
            )));
        }
        if field.vectors.iter().any(|v| !finite_vec(*v)) {
            return Err(Error::parameters("non-finite displacement"));
        }
        t.field.vectors = field.vectors.clone();
        Ok(t)
    }
    fn set_geometry(
        &mut self,
        size: [i64; 3],
        origin: Vec3,
        spacing: Vec3,
        direction: Matrix3,
    ) -> Result<(), Error> {
        if size.iter().any(|&n| !(1..=1 << 20).contains(&n)) {
            return Err(Error::parameters(format!(
                "displacement field size {}",
                ints_text(&size)
            )));
        }
        let count = size[0] * size[1] * size[2];
        if count / size[0] / size[1] != size[2] || count > 1 << 26 {
            return Err(Error::parameters(format!(
                "displacement field size {} is too large",
                ints_text(&size)
            )));
        }
        if !finite_vec(origin) || !direction.finite() {
            return Err(Error::parameters("non-finite displacement field geometry"));
        }
        let grid = Image::geometry(size, origin, spacing, direction)
            .map_err(|e| Error::parameters(format!("displacement field: {e}")))?;
        let mut vectors = std::mem::take(&mut self.field.vectors);
        if vectors.len() as i64 != count {
            vectors = vec![Vec3::ZERO; count as usize];
        }
        self.field = DisplacementField {
            size,
            origin,
            spacing,
            direction,
            vectors,
        };
        self.grid = grid;
        Ok(())
    }
    pub(crate) fn from_parts(field: DisplacementField, grid: Image) -> DisplacementFieldTransform {
        DisplacementFieldTransform { field, grid }
    }
    pub fn field(&self) -> DisplacementField {
        self.field.clone()
    }
    fn trilinear(&self, index: Vec3) -> ([usize; 8], [f64; 8], [[f64; 3]; 8]) {
        let size = self.field.size;
        let mut base = [0i64; 3];
        let mut frac = [0f64; 3];
        let mut moves = [false; 3];
        for (axis, value) in [index.x, index.y, index.z].into_iter().enumerate() {
            let last = size[axis] - 1;
            let clamped = fmath::min(fmath::max(value, 0.0), last as f64);
            let floor = clamped.floor();
            base[axis] = fmath::f64_to_i64(floor);
            frac[axis] = clamped - floor;
            moves[axis] = value > 0.0 && value < last as f64;
            if base[axis] >= last {
                base[axis] = last;
                frac[axis] = 0.0;
            }
        }
        let mut at = [0usize; 8];
        let mut weight = [0f64; 8];
        let mut slope = [[0f64; 3]; 8];
        for corner in 0..8 {
            let mut c = base;
            let mut w = 1.0;
            let mut factors = [0f64; 3];
            let mut derivatives = [0f64; 3];
            for axis in 0..3 {
                if corner >> axis & 1 == 1 {
                    c[axis] = (c[axis] + 1).min(size[axis] - 1);
                    factors[axis] = frac[axis];
                    derivatives[axis] = 1.0;
                } else {
                    factors[axis] = 1.0 - frac[axis];
                    derivatives[axis] = -1.0;
                }
                if !moves[axis] {
                    derivatives[axis] = 0.0;
                }
                w *= factors[axis];
            }
            at[corner] = ((c[2] * size[1] + c[1]) * size[0] + c[0]) as usize;
            weight[corner] = w;
            for axis in 0..3 {
                let mut s = derivatives[axis];
                for (other, factor) in factors.iter().enumerate() {
                    if other != axis {
                        s *= factor;
                    }
                }
                slope[corner][axis] = s;
            }
        }
        (at, weight, slope)
    }
}
impl Transform for DisplacementFieldTransform {
    fn kind(&self) -> Kind {
        Kind::DISPLACEMENT_FIELD
    }
    fn transform_point(&self, point: Vec3) -> Vec3 {
        let index = self.grid.continuous_index(point);
        if !self.grid.inside(index) {
            return point;
        }
        point.add(sample_vectors(&self.field.vectors, self.grid.size, index))
    }
    fn number_of_parameters(&self) -> usize {
        3 * self.field.vectors.len()
    }
    fn parameters(&self) -> Vec<f64> {
        let mut out = Vec::with_capacity(3 * self.field.vectors.len());
        for v in &self.field.vectors {
            out.extend([v.x, v.y, v.z]);
        }
        out
    }
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error> {
        let want = 3 * self.field.vectors.len();
        if parameters.len() != want {
            return Err(Error::parameters(format!(
                "displacement field parameters have {} values, want {want}",
                parameters.len()
            )));
        }
        if !finite_slice(parameters) {
            return Err(Error::parameters(
                "displacement field parameters contain a non-finite value",
            ));
        }
        self.field.vectors = (0..parameters.len() / 3)
            .map(|i| vec_from(&parameters[3 * i..]))
            .collect();
        Ok(())
    }
    fn fixed_parameters(&self) -> Vec<f64> {
        let f = &self.field;
        let mut out = vec![
            f.size[0] as f64,
            f.size[1] as f64,
            f.size[2] as f64,
            f.origin.x,
            f.origin.y,
            f.origin.z,
            f.spacing.x,
            f.spacing.y,
            f.spacing.z,
        ];
        for row in &f.direction.0 {
            out.extend(row);
        }
        out
    }
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error> {
        check_length(
            fixed,
            DISPLACEMENT_FIELD_FIXED_PARAMETERS,
            "displacement field fixed parameters",
        )?;
        let mut size = [0i64; 3];
        for axis in 0..3 {
            let v = fixed[axis];
            if v != v.trunc() || v < 1.0 || v > (1 << 20) as f64 {
                let parts: Vec<String> = fixed[..3]
                    .iter()
                    .map(|v| format_float(*v, 'g', -1))
                    .collect();
                return Err(Error::parameters(format!(
                    "displacement field size [{}]",
                    parts.join(" ")
                )));
            }
            size[axis] = v as i64;
        }
        let mut direction = Matrix3::default();
        for row in 0..3 {
            direction.0[row] = [fixed[9 + 3 * row], fixed[10 + 3 * row], fixed[11 + 3 * row]];
        }
        if size != self.field.size {
            self.field.vectors = Vec::new();
        }
        self.set_geometry(
            size,
            vec_from(&fixed[3..6]),
            vec_from(&fixed[6..9]),
            direction,
        )
    }
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error> {
        check_update(update, self.number_of_parameters(), factor)?;
        let mut parameters = self.parameters();
        for (p, u) in parameters.iter_mut().zip(update) {
            *p += factor * u;
        }
        self.set_parameters(&parameters)
    }
    fn jacobian_with_respect_to_parameters(&self, point: Vec3, out: &mut Jacobian) {
        out.reset(self.number_of_parameters());
        let index = self.grid.continuous_index(point);
        if !self.grid.inside(index) {
            return;
        }
        let (at, weight, _) = self.trilinear(index);
        for corner in 0..8 {
            for d in 0..3 {
                out.values[d * out.columns + 3 * at[corner] + d] += weight[corner];
            }
        }
    }
    fn jacobian_with_respect_to_position(&self, point: Vec3) -> Matrix3 {
        let mut out = Matrix3::identity();
        let index = self.grid.continuous_index(point);
        if !self.grid.inside(index) {
            return out;
        }
        let (at, _, slope) = self.trilinear(index);
        let mut per_index = [[0f64; 3]; 3];
        for corner in 0..8 {
            let v = self.field.vectors[at[corner]];
            for a in 0..3 {
                per_index[0][a] += slope[corner][a] * v.x;
                per_index[1][a] += slope[corner][a] * v.y;
                per_index[2][a] += slope[corner][a] * v.z;
            }
        }
        let p = &self.grid.physical_to_index.0;
        for d in 0..3 {
            for a in 0..3 {
                out.0[d][a] += per_index[d][0] * p[0][a]
                    + per_index[d][1] * p[1][a]
                    + per_index[d][2] * p[2][a];
            }
        }
        out
    }
    fn clone_box(&self) -> Box<dyn Transform> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
