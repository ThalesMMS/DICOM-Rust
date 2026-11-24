use super::matrix::{Jacobian, Matrix3, finite, finite_slice, finite_vec};
use crate::render::{GeometryAffine, Vec3};
use std::any::Any;
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Kind(pub &'static str);
impl Kind {
    pub const TRANSLATION: Kind = Kind("TranslationTransform");
    pub const SCALE_VERSOR_3D: Kind = Kind("ScaleVersor3DTransform");
    pub const SCALE_SKEW_VERSOR_3D: Kind = Kind("ScaleSkewVersor3DTransform");
    pub const EULER_2D: Kind = Kind("Euler2DTransform");
    pub const SIMILARITY_2D: Kind = Kind("Similarity2DTransform");
    pub const EULER_3D: Kind = Kind("Euler3DTransform");
    pub const VERSOR_RIGID_3D: Kind = Kind("VersorRigid3DTransform");
    pub const SIMILARITY_3D: Kind = Kind("Similarity3DTransform");
    pub const AFFINE: Kind = Kind("AffineTransform");
    pub const COMPOSITE: Kind = Kind("CompositeTransform");
}
impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidParameters,
    InvalidMatrix,
    SingularMatrix,
    UnsupportedTransform,
}
impl ErrorKind {
    pub fn text(self) -> &'static str {
        match self {
            ErrorKind::InvalidParameters => "dicom/registration: invalid parameters",
            ErrorKind::InvalidMatrix => "dicom/registration: invalid matrix",
            ErrorKind::SingularMatrix => "dicom/registration: singular matrix",
            ErrorKind::UnsupportedTransform => "dicom/registration: unsupported transform",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    text: String,
}
impl Error {
    pub(crate) fn bare(kind: ErrorKind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
        }
    }
    pub(crate) fn new(kind: ErrorKind, detail: impl AsRef<str>) -> Error {
        Error {
            kind,
            text: format!("{}: {}", kind.text(), detail.as_ref()),
        }
    }
    pub(crate) fn parameters(detail: impl AsRef<str>) -> Error {
        Error::new(ErrorKind::InvalidParameters, detail)
    }
    pub(crate) fn matrix(detail: impl AsRef<str>) -> Error {
        Error::new(ErrorKind::InvalidMatrix, detail)
    }
    pub(crate) fn prefixed(self, prefix: impl AsRef<str>) -> Error {
        Error {
            kind: self.kind,
            text: format!("{}: {}", prefix.as_ref(), self.text),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub const DEFAULT_ORTHOGONALITY_TOLERANCE: f64 = 1e-10;
pub trait Transform: Any {
    fn kind(&self) -> Kind;
    fn transform_point(&self, point: Vec3) -> Vec3;
    fn number_of_parameters(&self) -> usize;
    fn parameters(&self) -> Vec<f64>;
    fn set_parameters(&mut self, parameters: &[f64]) -> Result<(), Error>;
    fn fixed_parameters(&self) -> Vec<f64>;
    fn set_fixed_parameters(&mut self, fixed: &[f64]) -> Result<(), Error>;
    fn update_parameters(&mut self, update: &[f64], factor: f64) -> Result<(), Error>;
    fn jacobian_with_respect_to_parameters(&self, point: Vec3, out: &mut Jacobian);
    fn jacobian_with_respect_to_position(&self, point: Vec3) -> Matrix3;
    fn clone_box(&self) -> Box<dyn Transform>;
    fn as_matrix_transform(&self) -> Option<&dyn MatrixTransform> {
        None
    }
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
pub trait MatrixTransform: Transform {
    fn matrix(&self) -> Matrix3;
    fn offset(&self) -> Vec3;
    fn center(&self) -> Vec3;
    fn translation(&self) -> Vec3;
    fn geometry_affine(&self) -> GeometryAffine {
        geometry_affine(&self.matrix(), self.offset())
    }
    fn transform_vector(&self, vector: Vec3) -> Vec3 {
        self.matrix().mul_vec(vector)
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MatrixOffset {
    pub matrix: Matrix3,
    pub center: Vec3,
    pub translation: Vec3,
    pub offset: Vec3,
}
impl MatrixOffset {
    pub fn identity() -> MatrixOffset {
        MatrixOffset {
            matrix: Matrix3::identity(),
            center: Vec3::ZERO,
            translation: Vec3::ZERO,
            offset: Vec3::ZERO,
        }
    }
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        self.matrix.mul_vec(point).add(self.offset)
    }
    pub fn set_center(&mut self, center: Vec3) -> Result<(), Error> {
        if !finite_vec(center) {
            return Err(Error::parameters("non-finite centre"));
        }
        self.center = center;
        self.compute_offset();
        Ok(())
    }
    pub fn set_translation(&mut self, translation: Vec3) -> Result<(), Error> {
        if !finite_vec(translation) {
            return Err(Error::parameters("non-finite translation"));
        }
        self.translation = translation;
        self.compute_offset();
        Ok(())
    }
    pub fn set_offset(&mut self, offset: Vec3) -> Result<(), Error> {
        if !finite_vec(offset) {
            return Err(Error::parameters("non-finite offset"));
        }
        self.offset = offset;
        self.compute_translation();
        Ok(())
    }
    pub(crate) fn compute_offset(&mut self) {
        self.offset = self
            .translation
            .add(self.center)
            .sub(self.matrix.mul_vec(self.center));
    }
    pub(crate) fn compute_translation(&mut self) {
        self.translation = self
            .offset
            .sub(self.center)
            .add(self.matrix.mul_vec(self.center));
    }
    pub(crate) fn center_fixed_parameters(&self) -> Vec<f64> {
        vec![self.center.x, self.center.y, self.center.z]
    }
    pub fn inverse(&self) -> Result<(Matrix3, Vec3), Error> {
        let inv = self.matrix.inverse()?;
        Ok((inv, inv.mul_vec(self.offset).scale(-1.0)))
    }
}
macro_rules! matrix_offset_methods {
    ($name:ident) => {
        impl $crate::registration::MatrixTransform for $name {
            fn matrix(&self) -> $crate::registration::Matrix3 {
                self.base.matrix
            }
            fn offset(&self) -> $crate::render::Vec3 {
                self.base.offset
            }
            fn center(&self) -> $crate::render::Vec3 {
                self.base.center
            }
            fn translation(&self) -> $crate::render::Vec3 {
                self.base.translation
            }
        }
        impl $name {
            pub fn set_center(
                &mut self,
                center: $crate::render::Vec3,
            ) -> Result<(), $crate::registration::Error> {
                self.base.set_center(center)
            }
            pub fn set_translation(
                &mut self,
                translation: $crate::render::Vec3,
            ) -> Result<(), $crate::registration::Error> {
                self.base.set_translation(translation)
            }
            pub fn set_offset(
                &mut self,
                offset: $crate::render::Vec3,
            ) -> Result<(), $crate::registration::Error> {
                self.base.set_offset(offset)
            }
        }
    };
}
pub(crate) use matrix_offset_methods;
pub(crate) fn rotation_translation_jacobian(
    out: &mut Jacobian,
    derivatives: &[Matrix3],
    relative: Vec3,
    translation_column: usize,
) {
    for (k, derivative) in derivatives.iter().enumerate() {
        out.set_column(k, derivative.mul_vec(relative));
    }
    out.set(0, translation_column, 1.0);
    out.set(1, translation_column + 1, 1.0);
    out.set(2, translation_column + 2, 1.0);
}
pub(crate) fn check_length(values: &[f64], want: usize, what: &str) -> Result<(), Error> {
    if values.len() != want {
        return Err(Error::parameters(format!(
            "{what} has {} values, want {want}",
            values.len()
        )));
    }
    if !finite_slice(values) {
        return Err(Error::parameters(format!(
            "{what} contains a non-finite value"
        )));
    }
    Ok(())
}
pub(crate) fn vec_from(values: &[f64]) -> Vec3 {
    Vec3::new(values[0], values[1], values[2])
}
pub(crate) fn geometry_affine(m: &Matrix3, offset: Vec3) -> GeometryAffine {
    let m = &m.0;
    GeometryAffine([
        m[0][0], m[0][1], m[0][2], offset.x, m[1][0], m[1][1], m[1][2], offset.y, m[2][0], m[2][1],
        m[2][2], offset.z, 0.0, 0.0, 0.0, 1.0,
    ])
}
pub(crate) fn add_update<T: Transform + ?Sized>(
    t: &mut T,
    update: &[f64],
    factor: f64,
) -> Result<(), Error> {
    let mut parameters = t.parameters();
    check_update(update, parameters.len(), factor)?;
    for (p, u) in parameters.iter_mut().zip(update) {
        *p += factor * u;
    }
    t.set_parameters(&parameters)
}
pub(crate) fn check_update(update: &[f64], want: usize, factor: f64) -> Result<(), Error> {
    if update.len() != want {
        return Err(Error::parameters(format!(
            "update has {} values, want {want}",
            update.len()
        )));
    }
    if !finite(factor) || !finite_slice(update) {
        return Err(Error::parameters("non-finite update"));
    }
    Ok(())
}
