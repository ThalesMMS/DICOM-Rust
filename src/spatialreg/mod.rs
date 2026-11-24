#![forbid(unsafe_code)]
#![allow(clippy::neg_cmp_op_on_partial_ord)]
mod affine;
mod chain;
mod inspection;
mod inverse;
mod jacobian;
mod read;
mod resolve;
mod write;
use crate::fmath;
use crate::render::affine::GeometryAffine;
use crate::render::vec3::Vec3;
use crate::textfmt::float_v;
pub use affine::*;
pub use chain::*;
pub use inspection::*;
pub use inverse::*;
pub use jacobian::*;
pub use read::*;
pub use resolve::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
pub use write::*;
pub const DEFORMABLE_SPATIAL_REGISTRATION_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.66.3";
pub const DEFAULT_MAX_GRID_BYTES: u64 = 128 << 20;
pub const DEFAULT_MAX_GRID_VOXELS: u64 = 8 * 1024 * 1024;
pub const DEFAULT_MAX_REGISTRATIONS: i64 = 64;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    UnsupportedSopClass,
    InvalidObject,
    MemoryLimit,
    OutsideDomain,
    UndefinedDomain,
    InvalidMatrix,
    SingularMatrix,
    IllConditioned,
    UnsupportedMatrix,
    InverseNotConverged,
    Folding,
    InverseClosed,
    Chain,
    Write,
    Canceled,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::UnsupportedSopClass => "dicom/spatialreg: unsupported SOP class",
            Kind::InvalidObject => "dicom/spatialreg: invalid object",
            Kind::MemoryLimit => "dicom/spatialreg: memory limit exceeded",
            Kind::OutsideDomain => "dicom/spatialreg: point outside deformation domain",
            Kind::UndefinedDomain => "dicom/spatialreg: deformation undefined at point",
            Kind::InvalidMatrix => "dicom/spatialreg: invalid matrix",
            Kind::SingularMatrix => "dicom/spatialreg: singular matrix",
            Kind::IllConditioned => "dicom/spatialreg: ill-conditioned matrix",
            Kind::UnsupportedMatrix => "dicom/spatialreg: unsupported matrix type",
            Kind::InverseNotConverged => "dicom/spatialreg: inverse deformation did not converge",
            Kind::Folding => "dicom/spatialreg: deformation folds",
            Kind::InverseClosed => "dicom/spatialreg: prepared inverse is closed",
            Kind::Chain => "dicom/spatialreg: chained registration",
            Kind::Write => "spatialreg: invalid Spatial Registration",
            Kind::Canceled => "context canceled",
        }
    }
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LimitError {
    pub limit: &'static str,
    pub value: u64,
    pub maximum: u64,
}
impl std::fmt::Display for LimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} exceeds the maximum {}",
            self.limit, self.value, self.maximum
        )
    }
}
impl std::error::Error for LimitError {}
pub const LIMIT_GRID_BYTES: &str = "grid-bytes";
pub const LIMIT_GRID_VOXELS: &str = "grid-voxels";
pub const LIMIT_REGISTRATIONS: &str = "registrations";
#[derive(Clone, Debug, Default)]
struct Detail {
    limit: Option<LimitError>,
    matrix: Option<MatrixError>,
    resolution: Option<ResolutionError>,
    batch: Option<(usize, InverseResult)>,
}
#[derive(Clone, Debug, Default)]
pub struct Error {
    text: String,
    kinds: Vec<Kind>,
    detail: Option<Box<Detail>>,
}
impl Error {
    pub fn sentinel(kind: Kind) -> Error {
        Error {
            text: kind.text().to_string(),
            kinds: vec![kind],
            ..Error::default()
        }
    }
    pub fn new(kind: Kind, detail: impl AsRef<str>) -> Error {
        Error {
            text: format!("{}: {}", kind.text(), detail.as_ref()),
            kinds: vec![kind],
            ..Error::default()
        }
    }
    pub fn plain(text: impl Into<String>) -> Error {
        Error {
            text: text.into(),
            ..Error::default()
        }
    }
    pub fn wrap(mut self, kind: Kind, prefix: impl AsRef<str>) -> Error {
        self.text = format!("{}: {}: {}", kind.text(), prefix.as_ref(), self.text);
        self.kinds.insert(0, kind);
        self
    }
    pub fn prefixed(mut self, prefix: impl AsRef<str>) -> Error {
        self.text = format!("{}: {}", prefix.as_ref(), self.text);
        self
    }
    pub(crate) fn limit_exceeded(limit: &'static str, value: u64, maximum: u64) -> Error {
        let mut err = Error {
            text: format!(
                "{}: {limit} value {value} exceeds maximum {maximum}",
                Kind::MemoryLimit.text()
            ),
            kinds: vec![Kind::MemoryLimit],
            detail: None,
        };
        err.detail_mut().limit = Some(LimitError {
            limit,
            value,
            maximum,
        });
        err
    }
    fn detail_mut(&mut self) -> &mut Detail {
        self.detail.get_or_insert_with(Box::default)
    }
    pub(crate) fn with_kind(mut self, kind: Kind) -> Error {
        self.kinds.push(kind);
        self
    }
    pub(crate) fn with_matrix(mut self, matrix: MatrixError) -> Error {
        self.detail_mut().matrix = Some(matrix);
        self
    }
    pub(crate) fn with_resolution(mut self, resolution: ResolutionError) -> Error {
        self.detail_mut().resolution = Some(resolution);
        self
    }
    pub(crate) fn with_batch(mut self, index: usize, result: InverseResult) -> Error {
        self.detail_mut().batch = Some((index, result));
        self
    }
    pub fn limit(&self) -> Option<&LimitError> {
        self.detail.as_ref()?.limit.as_ref()
    }
    pub fn matrix(&self) -> Option<&MatrixError> {
        self.detail.as_ref()?.matrix.as_ref()
    }
    pub fn resolution(&self) -> Option<&ResolutionError> {
        self.detail.as_ref()?.resolution.as_ref()
    }
    pub fn batch(&self) -> Option<&(usize, InverseResult)> {
        self.detail.as_ref()?.batch.as_ref()
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kinds.contains(&kind)
    }
    pub fn kinds(&self) -> &[Kind] {
        &self.kinds
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Debug, Default)]
pub struct Context(Arc<AtomicBool>);
impl Context {
    pub fn background() -> Context {
        Context::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_canceled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub(crate) fn err(&self) -> Result<(), Error> {
        if self.is_canceled() {
            Err(Error::sentinel(Kind::Canceled))
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub max_grid_bytes: u64,
    pub max_grid_voxels: u64,
    pub max_registrations: i64,
}
impl Options {
    pub(crate) fn normalized(mut self) -> Options {
        if self.max_grid_bytes == 0 {
            self.max_grid_bytes = DEFAULT_MAX_GRID_BYTES;
        }
        if self.max_grid_voxels == 0 {
            self.max_grid_voxels = DEFAULT_MAX_GRID_VOXELS;
        }
        if self.max_registrations == 0 {
            self.max_registrations = DEFAULT_MAX_REGISTRATIONS;
        }
        self
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Matrix {
    pub r#type: String,
    pub values: GeometryAffine,
}
pub(crate) const IDENTITY: GeometryAffine = GeometryAffine([
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
]);
impl Matrix {
    pub fn identity() -> Matrix {
        Matrix {
            r#type: String::new(),
            values: IDENTITY,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Grid {
    pub origin: Vec3,
    pub row_direction: Vec3,
    pub column_direction: Vec3,
    pub normal_direction: Vec3,
    pub spacing: Vec3,
    pub dimensions: [u32; 3],
    pub(crate) vectors: Arc<[[f32; 3]]>,
}
pub(crate) fn undefined_vector(v: [f32; 3]) -> bool {
    v[0].is_nan() && v[1].is_nan() && v[2].is_nan()
}
pub(crate) fn finite(v: f64) -> bool {
    !v.is_nan() && !v.is_infinite()
}
pub(crate) fn finite_vec(v: Vec3) -> bool {
    finite(v.x) && finite(v.y) && finite(v.z)
}
pub(crate) fn finite_positive(v: f64) -> bool {
    v > 0.0 && finite(v)
}
pub(crate) fn all_finite(values: &[f64]) -> bool {
    values.iter().all(|v| finite(*v))
}
pub(crate) fn checked_product64(values: &[u64]) -> Option<u64> {
    let mut product = 1u64;
    for &value in values {
        if value == 0 || product > u64::MAX / value {
            return None;
        }
        product *= value;
    }
    Some(product)
}
pub(crate) fn valid_basis(row: Vec3, column: Vec3, normal: Vec3) -> bool {
    if !finite_vec(row) || !finite_vec(column) || !finite_vec(normal) {
        return false;
    }
    (row.length() - 1.0).abs() <= 1e-4
        && (column.length() - 1.0).abs() <= 1e-4
        && (normal.length() - 1.0).abs() <= 1e-4
        && row.dot(column).abs() <= 1e-4
        && row.dot(normal).abs() <= 1e-4
        && column.dot(normal).abs() <= 1e-4
}
pub(crate) fn dims_v(d: [u32; 3]) -> String {
    format!("[{} {} {}]", d[0], d[1], d[2])
}
const DOMAIN_TOLERANCE: f64 = 1e-9;
impl Grid {
    pub fn new(
        origin: Vec3,
        row: Vec3,
        column: Vec3,
        spacing: Vec3,
        dimensions: [u32; 3],
        vectors: &[Vec3],
    ) -> Result<Grid, Error> {
        let count = checked_product64(&[
            u64::from(dimensions[0]),
            u64::from(dimensions[1]),
            u64::from(dimensions[2]),
        ]);
        if count != Some(vectors.len() as u64) {
            return Err(Error::new(
                Kind::InvalidObject,
                format!(
                    "{} vectors for grid dimensions {}",
                    vectors.len(),
                    dims_v(dimensions)
                ),
            ));
        }
        if !finite_vec(origin)
            || !finite_positive(spacing.x)
            || !finite_positive(spacing.y)
            || !finite_positive(spacing.z)
        {
            return Err(Error::new(
                Kind::InvalidObject,
                "non-finite origin or non-positive spacing",
            ));
        }
        let normal = row.cross(column);
        if !valid_basis(row, column, normal) {
            return Err(Error::new(
                Kind::InvalidObject,
                "grid directions must be orthonormal",
            ));
        }
        let mut out = Vec::with_capacity(vectors.len());
        for (index, v) in vectors.iter().enumerate() {
            let value = [v.x as f32, v.y as f32, v.z as f32];
            if !undefined_vector(value)
                && !(value[0].is_finite() && value[1].is_finite() && value[2].is_finite())
            {
                return Err(Error::new(
                    Kind::InvalidObject,
                    format!("vector {index} is partially undefined or non-finite"),
                ));
            }
            out.push(value);
        }
        Ok(Grid {
            origin,
            row_direction: row,
            column_direction: column,
            normal_direction: normal.normalize(),
            spacing,
            dimensions,
            vectors: out.into(),
        })
    }
    pub fn vector_at(&self, x: i64, y: i64, z: i64) -> Option<Vec3> {
        let d = self.dimensions.map(i64::from);
        if x < 0 || y < 0 || z < 0 || x >= d[0] || y >= d[1] || z >= d[2] {
            return None;
        }
        let index = (z * d[1] + y) * d[0] + x;
        if index < 0 || index as usize >= self.vectors.len() {
            return None;
        }
        let value = self.vectors[index as usize];
        if undefined_vector(value) {
            return None;
        }
        Some(Vec3::new(
            f64::from(value[0]),
            f64::from(value[1]),
            f64::from(value[2]),
        ))
    }
    pub fn vector_bytes(&self) -> u64 {
        self.vectors.len() as u64 * 12
    }
    pub fn sample(&self, point: Vec3) -> Result<Vec3, Error> {
        if !finite_vec(point) {
            return Err(Error::new(
                Kind::InvalidObject,
                "missing grid or non-finite point",
            ));
        }
        if self.dimensions.contains(&0)
            || !finite_positive(self.spacing.x)
            || !finite_positive(self.spacing.y)
            || !finite_positive(self.spacing.z)
            || !valid_basis(
                self.row_direction,
                self.column_direction,
                self.normal_direction,
            )
        {
            return Err(Error::new(
                Kind::InvalidObject,
                "invalid deformation grid geometry",
            ));
        }
        let delta = point.sub(self.origin);
        let coordinates = [
            delta.dot(self.row_direction) / self.spacing.x,
            delta.dot(self.column_direction) / self.spacing.y,
            delta.dot(self.normal_direction) / self.spacing.z,
        ];
        let mut lower = [0i64; 3];
        let mut upper = [0i64; 3];
        let mut fraction = [0f64; 3];
        for axis in 0..3 {
            let mut coordinate = coordinates[axis];
            let maximum = f64::from(self.dimensions[axis] - 1);
            if !finite(coordinate)
                || coordinate < -DOMAIN_TOLERANCE
                || coordinate > maximum + DOMAIN_TOLERANCE
            {
                return Err(Error::new(
                    Kind::OutsideDomain,
                    format!(
                        "grid coordinate [{} {} {}]",
                        float_v(coordinates[0]),
                        float_v(coordinates[1]),
                        float_v(coordinates[2])
                    ),
                ));
            }
            coordinate = fmath::max(0.0, fmath::min(maximum, coordinate));
            lower[axis] = coordinate.floor() as i64;
            upper[axis] = (lower[axis] + 1).min(i64::from(self.dimensions[axis]) - 1);
            fraction[axis] = coordinate - lower[axis] as f64;
        }
        let choice = |axis: usize, c: usize| -> (i64, f64) {
            if c == 0 {
                (lower[axis], 1.0 - fraction[axis])
            } else {
                (upper[axis], fraction[axis])
            }
        };
        let mut result = Vec3::ZERO;
        for zc in 0..2 {
            let (z, wz) = choice(2, zc);
            for yc in 0..2 {
                let (y, wy) = choice(1, yc);
                for xc in 0..2 {
                    let (x, wx) = choice(0, xc);
                    let weight = wx * wy * wz;
                    if weight == 0.0 {
                        continue;
                    }
                    let Some(value) = self.vector_at(x, y, z) else {
                        return Err(Error::new(
                            Kind::UndefinedDomain,
                            format!("contributing vector ({x},{y},{z})"),
                        ));
                    };
                    result = result.add(value.scale(weight));
                }
            }
        }
        if !finite_vec(result) {
            return Err(Error::new(
                Kind::UndefinedDomain,
                "interpolation produced a non-finite vector",
            ));
        }
        Ok(result)
    }
}
#[derive(Clone, Debug, Default)]
pub struct Registration {
    pub source_frame_of_reference_uid: String,
    pub referenced_sop_instance_uids: Vec<String>,
    pub pre: Matrix,
    pub post: Matrix,
    pub grid: Option<Grid>,
}
#[derive(Clone, Debug, Default)]
pub struct Object {
    pub sop_instance_uid: String,
    pub registered_frame_of_reference_uid: String,
    pub registrations: Vec<Registration>,
}
impl Registration {
    pub fn map_registered_to_source(&self, point: Vec3) -> Result<Vec3, Error> {
        if !finite_vec(point) {
            return Err(Error::new(
                Kind::InvalidObject,
                "non-finite registered point",
            ));
        }
        if !self.pre.values.finite() || !self.post.values.finite() {
            return Err(Error::new(
                Kind::InvalidObject,
                "pre or post matrix is invalid",
            ));
        }
        let mut transformed = self.pre.values.transform_point(point);
        if let Some(grid) = &self.grid {
            transformed = transformed.add(grid.sample(point)?);
        }
        let source = self.post.values.transform_point(transformed);
        if !finite_vec(source) {
            return Err(Error::new(
                Kind::InvalidObject,
                "transform produced a non-finite source point",
            ));
        }
        Ok(source)
    }
}
pub mod nil {
    use super::{Error, Kind};
    #[doc(hidden)]
    pub fn registration_map() -> Error {
        Error::new(Kind::InvalidObject, "non-finite registered point")
    }
    #[doc(hidden)]
    pub fn registration_invert() -> Error {
        Error::new(Kind::InvalidObject, "non-finite source point")
    }
    #[doc(hidden)]
    pub fn registration_prepare() -> Error {
        Error::new(Kind::InvalidObject, "missing registration")
    }
    #[doc(hidden)]
    pub fn registration_volume_change_at() -> Error {
        Error::new(Kind::InvalidObject, "no registration")
    }
    #[doc(hidden)]
    pub fn registration_jacobian() -> Error {
        Error::new(Kind::InvalidObject, "no deformation grid")
    }
    #[doc(hidden)]
    pub fn registration_inspection() -> Error {
        Error::new(Kind::InvalidObject, "deformation grid required")
    }
    #[doc(hidden)]
    pub fn grid_sample() -> Error {
        Error::new(Kind::InvalidObject, "missing grid or non-finite point")
    }
}
