use super::{Error, Kind, Matrix, finite, finite_vec};
use crate::fmath;
use crate::numtext::format_float;
use crate::render::affine::GeometryAffine;
use crate::render::mpr::Plane;
use crate::render::vec3::Vec3;
use crate::render::volume::Volume;
use crate::render::volume_sampler::VolumeReader;
use std::sync::Arc;
pub const SPATIAL_REGISTRATION_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.1";
pub type TransformType = String;
pub const TRANSFORM_RIGID: &str = "RIGID";
pub const TRANSFORM_RIGID_SCALE: &str = "RIGID_SCALE";
pub const TRANSFORM_AFFINE: &str = "AFFINE";
#[derive(Clone, Debug, PartialEq)]
pub struct MatrixError {
    pub index: usize,
    pub r#type: TransformType,
    pub condition: f64,
    pub kind: Kind,
}
impl std::fmt::Display for MatrixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "matrix {} ({}): {}",
            self.index,
            self.r#type,
            self.kind.text()
        )
    }
}
impl std::error::Error for MatrixError {}
impl MatrixError {
    pub(crate) fn into_error(self) -> Error {
        let text = if self.condition > 0.0 {
            format!(
                "matrix {} ({}): {} (condition {})",
                self.index,
                self.r#type,
                self.kind.text(),
                format_float(self.condition, 'g', 6)
            )
        } else {
            format!(
                "matrix {} ({}): {}",
                self.index,
                self.r#type,
                self.kind.text()
            )
        };
        Error::plain(text).with_kind(self.kind).with_matrix(self)
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AffineRegistration {
    pub source_frame_of_reference_uid: String,
    pub referenced_sop_instance_uids: Vec<String>,
    pub matrices: Vec<Matrix>,
    pub r#type: TransformType,
    pub source_to_registered: GeometryAffine,
    pub registered_to_source: GeometryAffine,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AffineObject {
    pub sop_instance_uid: String,
    pub registered_frame_of_reference_uid: String,
    pub registrations: Vec<AffineRegistration>,
}
fn checked_transform_point(matrix: &GeometryAffine, point: Vec3) -> Result<Vec3, Error> {
    if !matrix.finite() {
        return Err(Error::new(Kind::InvalidMatrix, "transform is invalid"));
    }
    let out = matrix.transform_point(point);
    if !finite_vec(out) {
        return Err(Error::new(
            Kind::InvalidMatrix,
            "transform produced a non-finite point",
        ));
    }
    Ok(out)
}
pub(crate) fn multiply_affine(left: &GeometryAffine, right: &GeometryAffine) -> GeometryAffine {
    let mut out = [0f64; 16];
    for row in 0..4 {
        for column in 0..4 {
            for k in 0..4 {
                out[row * 4 + column] += left.0[row * 4 + k] * right.0[k * 4 + column];
            }
        }
    }
    GeometryAffine(out)
}
fn matrix3_infinity_norm(m: &GeometryAffine) -> f64 {
    let mut maximum = 0.0;
    for row in 0..3 {
        let sum = m.0[row * 4].abs() + m.0[row * 4 + 1].abs() + m.0[row * 4 + 2].abs();
        maximum = fmath::max(maximum, sum);
    }
    maximum
}
pub(crate) fn inverse_affine(
    matrix: &GeometryAffine,
) -> Result<(GeometryAffine, f64), (f64, Kind)> {
    if !matrix.finite() {
        return Err((0.0, Kind::InvalidMatrix));
    }
    let m = &matrix.0;
    let (a, b, c) = (m[0], m[1], m[2]);
    let (d, e, f) = (m[4], m[5], m[6]);
    let (g, h, i) = (m[8], m[9], m[10]);
    let determinant = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    let norm = matrix3_infinity_norm(matrix);
    if !finite(norm) || norm == 0.0 || determinant.abs() <= 1e-15 * norm * norm * norm {
        return Err((f64::INFINITY, Kind::SingularMatrix));
    }
    let inv_det = 1.0 / determinant;
    let mut inverse = [
        (e * i - f * h) * inv_det,
        (c * h - b * i) * inv_det,
        (b * f - c * e) * inv_det,
        0.0,
        (f * g - d * i) * inv_det,
        (a * i - c * g) * inv_det,
        (c * d - a * f) * inv_det,
        0.0,
        (d * h - e * g) * inv_det,
        (b * g - a * h) * inv_det,
        (a * e - b * d) * inv_det,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ];
    inverse[3] = -(inverse[0] * m[3] + inverse[1] * m[7] + inverse[2] * m[11]);
    inverse[7] = -(inverse[4] * m[3] + inverse[5] * m[7] + inverse[6] * m[11]);
    inverse[11] = -(inverse[8] * m[3] + inverse[9] * m[7] + inverse[10] * m[11]);
    let inverse = GeometryAffine(inverse);
    let condition = norm * matrix3_infinity_norm(&inverse);
    if !finite(condition) || condition > 1e10 {
        return Err((condition, Kind::IllConditioned));
    }
    Ok((inverse, condition))
}
pub(crate) fn inverse_affine_err(matrix: &GeometryAffine) -> Result<GeometryAffine, Error> {
    inverse_affine(matrix)
        .map(|(inverse, _)| inverse)
        .map_err(|(_, kind)| Error::sentinel(kind))
}
pub fn validate_matrix(matrix: &GeometryAffine, kind: &str) -> Result<(), Error> {
    validate_matrix_type(matrix, kind).map_err(Error::sentinel)
}
pub(crate) fn validate_matrix_type(matrix: &GeometryAffine, kind: &str) -> Result<(), Kind> {
    if !matrix.finite() {
        return Err(Kind::InvalidMatrix);
    }
    let m = &matrix.0;
    let columns = [
        Vec3::new(m[0], m[4], m[8]),
        Vec3::new(m[1], m[5], m[9]),
        Vec3::new(m[2], m[6], m[10]),
    ];
    const TOLERANCE: f64 = 1e-6;
    let scales = [
        columns[0].length(),
        columns[1].length(),
        columns[2].length(),
    ];
    for scale in scales {
        if !finite(scale) || scale <= 1e-12 {
            return Err(Kind::SingularMatrix);
        }
    }
    let determinant = columns[0].dot(columns[1].cross(columns[2]));
    match kind {
        TRANSFORM_RIGID => {
            for scale in scales {
                if (scale - 1.0).abs() > TOLERANCE {
                    return Err(Kind::InvalidMatrix);
                }
            }
            if (determinant - 1.0).abs() > TOLERANCE {
                return Err(Kind::InvalidMatrix);
            }
        }
        TRANSFORM_RIGID_SCALE => {
            for left in 0..3 {
                for right in left + 1..3 {
                    if columns[left].dot(columns[right]).abs()
                        > TOLERANCE * scales[left] * scales[right]
                    {
                        return Err(Kind::InvalidMatrix);
                    }
                }
            }
            if determinant <= 0.0 {
                return Err(Kind::InvalidMatrix);
            }
        }
        TRANSFORM_AFFINE => {}
        _ => return Err(Kind::UnsupportedMatrix),
    }
    inverse_affine(matrix).map(|_| ()).map_err(|(_, kind)| kind)
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MappedVolumeGeometry {
    pub dimensions: [i64; 3],
    pub corners: [Vec3; 8],
}
fn map_plane(plane: &Plane, mapper: impl Fn(Vec3) -> Result<Vec3, Error>) -> Result<Plane, Error> {
    if !finite_vec(plane.origin) || !finite_vec(plane.u) || !finite_vec(plane.v) {
        return Err(Error::new(
            Kind::InvalidObject,
            "plane contains non-finite geometry",
        ));
    }
    let origin = mapper(plane.origin)?;
    let u_end = mapper(plane.origin.add(plane.u))?;
    let v_end = mapper(plane.origin.add(plane.v))?;
    Ok(Plane {
        origin,
        u: u_end.sub(origin),
        v: v_end.sub(origin),
    })
}
fn map_volume_geometry(
    volume: &Volume,
    mapper: impl Fn(Vec3) -> Result<Vec3, Error>,
) -> Result<MappedVolumeGeometry, Error> {
    if volume.cols <= 0 || volume.rows <= 0 || volume.depth <= 0 {
        return Err(Error::new(
            Kind::InvalidObject,
            "volume geometry is unavailable",
        ));
    }
    let mut out = MappedVolumeGeometry {
        dimensions: [volume.cols, volume.rows, volume.depth],
        corners: [Vec3::ZERO; 8],
    };
    let maximum = [
        (volume.cols - 1) as f64,
        (volume.rows - 1) as f64,
        (volume.depth - 1) as f64,
    ];
    for z in 0..2usize {
        for y in 0..2usize {
            for x in 0..2usize {
                let point = volume.voxel_to_patient(Vec3::new(
                    maximum[0] * x as f64,
                    maximum[1] * y as f64,
                    maximum[2] * z as f64,
                ));
                out.corners[x + 2 * y + 4 * z] = mapper(point)?;
            }
        }
    }
    Ok(out)
}
impl AffineRegistration {
    pub fn map_source_to_registered(&self, point: Vec3) -> Result<Vec3, Error> {
        if !finite_vec(point) {
            return Err(Error::new(Kind::InvalidObject, "non-finite source point"));
        }
        checked_transform_point(&self.source_to_registered, point)
    }
    pub fn map_registered_to_source(&self, point: Vec3) -> Result<Vec3, Error> {
        if !finite_vec(point) {
            return Err(Error::new(
                Kind::InvalidObject,
                "non-finite registered point",
            ));
        }
        checked_transform_point(&self.registered_to_source, point)
    }
    pub fn map_source_plane_to_registered(&self, plane: &Plane) -> Result<Plane, Error> {
        map_plane(plane, |p| self.map_source_to_registered(p))
    }
    pub fn map_registered_plane_to_source(&self, plane: &Plane) -> Result<Plane, Error> {
        map_plane(plane, |p| self.map_registered_to_source(p))
    }
    pub fn map_source_volume_to_registered(
        &self,
        volume: &Volume,
    ) -> Result<MappedVolumeGeometry, Error> {
        map_volume_geometry(volume, |p| self.map_source_to_registered(p))
    }
    pub fn map_registered_volume_to_source(
        &self,
        volume: &Volume,
    ) -> Result<MappedVolumeGeometry, Error> {
        map_volume_geometry(volume, |p| self.map_registered_to_source(p))
    }
    pub fn acquire_registered_volume_sampler(
        &self,
        source: Option<&Arc<Volume>>,
    ) -> Result<RegisteredVolumeSampler, Error> {
        if !self.source_to_registered.finite() || !self.registered_to_source.finite() {
            return Err(Error::new(
                Kind::InvalidMatrix,
                "affine directions are unavailable",
            ));
        }
        let Some(source) = source else {
            return Err(Error::new(Kind::InvalidObject, "source volume is nil"));
        };
        let reader = source
            .acquire_reader()
            .map_err(|e| Error::plain(e.to_string()))?;
        Ok(RegisteredVolumeSampler {
            inner: Some((self.clone(), reader)),
        })
    }
}
pub struct RegisteredVolumeSampler {
    inner: Option<(AffineRegistration, VolumeReader)>,
}
impl RegisteredVolumeSampler {
    pub fn sample_patient(&self, registered: Vec3) -> Option<f64> {
        let (registration, reader) = self.inner.as_ref()?;
        let source = registration.map_registered_to_source(registered).ok()?;
        reader.sample_patient(source)
    }
    pub fn close(&mut self) {
        if let Some((_, reader)) = self.inner.take() {
            reader.close();
        }
    }
}
