use super::error::{RegError, Sentinel};
use super::matrix::{Matrix3, finite_vec};
use super::transform::{Transform, geometry_affine};
use super::{Affine, Composite, Euler3D, Similarity3D, Translation3D, VersorRigid3D};
use crate::render::{GeometryAffine, Vec3};
use crate::spatialreg::{self, AffineRegistration, validate_matrix};
use crate::textfmt::quote;
pub const DICOM_RIGID_TOLERANCE: f64 = 1e-6;
pub(crate) const TRANSFORM_RIGID: &str = "RIGID";
pub(crate) const TRANSFORM_RIGID_SCALE: &str = "RIGID_SCALE";
pub(crate) const TRANSFORM_AFFINE: &str = "AFFINE";
pub(crate) fn transform_from_linear(
    m: &Matrix3,
    offset: Vec3,
    kind: &str,
    center: Vec3,
) -> Result<Box<dyn Transform>, RegError> {
    if !finite_vec(center) {
        return Err(RegError::new(
            Sentinel::InvalidParameters,
            "non-finite centre",
        ));
    }
    match kind {
        TRANSFORM_RIGID => {
            let mut v = VersorRigid3D::new();
            v.set_center(center)?;
            v.set_matrix_with_tolerance(m, DICOM_RIGID_TOLERANCE)?;
            v.set_offset(offset)?;
            return Ok(Box::new(v));
        }
        TRANSFORM_RIGID_SCALE => {
            let mut s = Similarity3D::new();
            s.set_center(center)?;
            if s.set_matrix_with_tolerance(m, DICOM_RIGID_TOLERANCE)
                .is_ok()
            {
                s.set_offset(offset)?;
                return Ok(Box::new(s));
            }
        }
        TRANSFORM_AFFINE => {}
        _ => {
            return Err(RegError::new(
                Sentinel::UnsupportedTransform,
                format!("unsupported DICOM matrix type {}", quote(kind)),
            ));
        }
    }
    let mut a = Affine::new();
    a.set_center(center)?;
    a.set_matrix(*m)?;
    a.set_offset(offset)?;
    Ok(Box::new(a))
}
pub fn dicom_matrix(t: Option<&dyn Transform>) -> Result<spatialreg::Matrix, RegError> {
    let (m, offset, kind) = linear_form(t)?;
    let inv = m
        .inverse()
        .map_err(|e| RegError::from(e).prefixed("DICOM matrix"))?;
    let values = geometry_affine(&inv, inv.mul_vec(offset).scale(-1.0));
    validate_matrix(&values, kind)
        .map_err(|e| RegError::from(e).prefixed(format!("DICOM matrix ({kind})")))?;
    Ok(spatialreg::Matrix {
        r#type: kind.to_string(),
        values,
    })
}
pub fn dicom_forward_matrix(t: Option<&dyn Transform>) -> Result<spatialreg::Matrix, RegError> {
    let (m, offset, kind) = linear_form(t)?;
    let values = geometry_affine(&m, offset);
    validate_matrix(&values, kind)
        .map_err(|e| RegError::from(e).prefixed(format!("DICOM matrix ({kind})")))?;
    Ok(spatialreg::Matrix {
        r#type: kind.to_string(),
        values,
    })
}
pub fn from_dicom_matrix(
    matrix: &spatialreg::Matrix,
    center: Vec3,
) -> Result<Box<dyn Transform>, RegError> {
    let kind = matrix.r#type.as_str();
    validate_matrix(&matrix.values, kind)
        .map_err(|e| RegError::from(e).prefixed(format!("DICOM matrix ({kind})")))?;
    let (m, offset) = split_geometry_affine(&matrix.values);
    let inv = m
        .inverse()
        .map_err(|e| RegError::from(e).prefixed("DICOM matrix"))?;
    transform_from_linear(&inv, inv.mul_vec(offset).scale(-1.0), kind, center)
}
pub fn from_affine_registration(
    r: Option<&AffineRegistration>,
    center: Vec3,
) -> Result<Box<dyn Transform>, RegError> {
    let Some(r) = r else {
        return Err(RegError::new(
            Sentinel::UnsupportedTransform,
            "nil registration",
        ));
    };
    if !r.registered_to_source.finite() {
        return Err(RegError::new(
            Sentinel::InvalidMatrix,
            "registered-to-source matrix is not finite",
        ));
    }
    let (m, offset) = split_geometry_affine(&r.registered_to_source);
    transform_from_linear(&m, offset, &r.r#type, center)
}
fn linear_form(t: Option<&dyn Transform>) -> Result<(Matrix3, Vec3, &'static str), RegError> {
    let Some(t) = t else {
        return Err(RegError::new(
            Sentinel::UnsupportedTransform,
            "nil transform",
        ));
    };
    if let Some(c) = t.as_any().downcast_ref::<Composite>() {
        let (m, offset) = c.linear()?;
        return Ok((m, offset, composite_type(c)));
    }
    match t.as_matrix_transform() {
        Some(m) => Ok((m.matrix(), m.offset(), transform_type(t))),
        None => Err(RegError::new(
            Sentinel::UnsupportedTransform,
            format!(
                "{} has no Frame of Reference Transformation Matrix",
                t.kind()
            ),
        )),
    }
}
fn transform_type(t: &dyn Transform) -> &'static str {
    let any = t.as_any();
    if any.is::<Euler3D>() || any.is::<VersorRigid3D>() || any.is::<Translation3D>() {
        return TRANSFORM_RIGID;
    }
    if any.is::<Similarity3D>() {
        return TRANSFORM_RIGID_SCALE;
    }
    if let Some(c) = any.downcast_ref::<Composite>() {
        return composite_type(c);
    }
    TRANSFORM_AFFINE
}
fn composite_type(c: &Composite) -> &'static str {
    let rank = |k: &str| match k {
        TRANSFORM_RIGID => 0,
        TRANSFORM_RIGID_SCALE => 1,
        _ => 2,
    };
    let mut out = TRANSFORM_RIGID;
    for i in 0..c.len() {
        let kind = transform_type(c.transform(i));
        if rank(kind) > rank(out) {
            out = kind;
        }
    }
    out
}
fn split_geometry_affine(a: &GeometryAffine) -> (Matrix3, Vec3) {
    let a = &a.0;
    (
        Matrix3([[a[0], a[1], a[2]], [a[4], a[5], a[6]], [a[8], a[9], a[10]]]),
        Vec3::new(a[3], a[7], a[11]),
    )
}
