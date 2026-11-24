use super::header::{Header, XformCode, affine_lps2ras, nearly_equal, unit_millimeters};
use super::{Error, ReadCode, ReadOptions, Sentinel, finite, finite_positive, read_error};
use crate::fmath;
use crate::render::{GeometryAffine, SliceGeometry, Vec3};
use std::f64::consts::PI;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct XformPolicy(pub u8);
impl XformPolicy {
    pub const AUTO: XformPolicy = XformPolicy(0);
    pub const QFORM: XformPolicy = XformPolicy(1);
    pub const SFORM: XformPolicy = XformPolicy(2);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum XformSource {
    #[default]
    None,
    QForm,
    SForm,
    SFormOrthogonalized,
    PixDim,
}
impl XformSource {
    pub fn as_str(self) -> &'static str {
        match self {
            XformSource::None => "",
            XformSource::QForm => "qform",
            XformSource::SForm => "sform",
            XformSource::SFormOrthogonalized => "sform-orthogonalized",
            XformSource::PixDim => "pixdim",
        }
    }
}
pub const WARNING_QFORM_INVALID: &str = "qform-invalid-ignored";
pub const WARNING_SFORM_IGNORED: &str = "sform-disagrees-with-qform-ignored";
pub const WARNING_SFORM_NON_ORTHOGONAL: &str = "sform-non-orthogonal-ignored";
pub const WARNING_SFORM_ORTHOGONALIZED: &str = "sform-non-orthogonal-orthogonalized";
pub const WARNING_SFORM_PIXDIM_MISMATCH: &str = "sform-pixdim-mismatch";
pub const WARNING_NO_SPATIAL_TRANSFORM: &str = "no-spatial-transform-pixdim-used";
pub const WARNING_VOXEL_OFFSET_ADJUSTED: &str = "vox-offset-below-header-adjusted";
pub const WARNING_BITPIX_MISMATCH: &str = "bitpix-datatype-mismatch";
pub const WARNING_TRAILING_DATA: &str = "trailing-data-ignored";
pub const WARNING_NON_SCANNER_COORDINATES: &str = "non-scanner-coordinate-system";
const ORTHONORMAL_TOLERANCE: f64 = 1e-4;
const SINGULAR_TOLERANCE: f64 = 1e-6;
const QUATERNION_SNAP: f64 = 1.0 / (1u64 << 22) as f64;
pub const DEFAULT_PERMISSIVE_MAX_DEVIATION_DEGREES: f64 = 10.0;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    pub source: XformSource,
    pub code: XformCode,
    pub qform_code: XformCode,
    pub sform_code: XformCode,
    pub index_to_ras: GeometryAffine,
    pub index_to_lps: GeometryAffine,
    pub spacing: [f64; 3],
    pub left_handed: bool,
    pub qform_sform_agree: bool,
    pub sform_corrected: bool,
    pub sform_deviation_degrees: f64,
    pub warnings: Vec<String>,
}
impl Geometry {
    pub fn slice_geometry(&self, k: i64, columns: i64, rows: i64) -> SliceGeometry {
        let a = &self.index_to_lps;
        let origin = a.transform_point(Vec3::new(0.0, 0.0, k as f64));
        let x = Vec3::new(a.0[0], a.0[4], a.0[8]);
        let y = Vec3::new(a.0[1], a.0[5], a.0[9]);
        let row = x.normalize();
        let column = y.sub(row.scale(y.dot(row))).normalize();
        SliceGeometry {
            origin,
            row_dir: row,
            col_dir: column,
            normal: row.cross(column).normalize(),
            row_spacing: self.spacing[1],
            col_spacing: self.spacing[0],
            rows,
            columns,
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
struct Xform {
    columns: [Vec3; 3],
    norms: [f64; 3],
    offset: Vec3,
}
impl Xform {
    fn affine(&self) -> GeometryAffine {
        let (c, o) = (&self.columns, &self.offset);
        GeometryAffine([
            c[0].x, c[1].x, c[2].x, o.x, c[0].y, c[1].y, c[2].y, o.y, c[0].z, c[1].z, c[2].z, o.z,
            0.0, 0.0, 0.0, 1.0,
        ])
    }
    fn unit(&self, index: usize) -> Vec3 {
        self.columns[index].scale(1.0 / self.norms[index])
    }
    fn determinant(&self) -> f64 {
        self.columns[0].dot(self.columns[1].cross(self.columns[2]))
    }
    fn orthonormal(&self) -> bool {
        for first in 0..3 {
            for second in first + 1..3 {
                if self.unit(first).dot(self.unit(second)).abs() > ORTHONORMAL_TOLERANCE {
                    return false;
                }
            }
        }
        true
    }
}
fn invalid(field: &'static str) -> Error {
    read_error(ReadCode::Geometry, field, Sentinel::InvalidTransform, None)
}
fn new_xform(columns: [Vec3; 3], offset: Vec3, field: &'static str) -> Result<Xform, Error> {
    let mut out = Xform {
        columns,
        norms: [0.0; 3],
        offset,
    };
    if !offset.finite() {
        return Err(invalid(field));
    }
    for (index, column) in columns.iter().enumerate() {
        out.norms[index] = column.length();
        if !column.finite() || !finite_positive(out.norms[index]) {
            return Err(invalid(field));
        }
    }
    let normalized = out.unit(0).dot(out.unit(1).cross(out.unit(2)));
    if !(normalized.abs() > SINGULAR_TOLERANCE) {
        return Err(invalid(field));
    }
    Ok(out)
}
fn qform_transform(header: &Header) -> Result<Xform, Error> {
    let (mut b, mut c, mut d) = (header.quatern_b, header.quatern_c, header.quatern_d);
    if !finite(b) || !finite(c) || !finite(d) {
        return Err(invalid("quatern"));
    }
    for index in 1..=3 {
        if !finite_positive(header.pix_dim[index]) {
            return Err(invalid("pixdim"));
        }
    }
    let sum = b * b + c * c + d * d;
    if sum > 1.0 + 1e-4 {
        return Err(invalid("quatern"));
    }
    let mut a = 0.0;
    let residual = 1.0 - sum;
    if residual < 1e-7 {
        let length = sum.sqrt();
        (b, c, d) = (b / length, c / length, d / length);
    } else {
        a = residual.sqrt();
    }
    let r = [
        [
            a * a + b * b - c * c - d * d,
            2.0 * (b * c - a * d),
            2.0 * (b * d + a * c),
        ],
        [
            2.0 * (b * c + a * d),
            a * a + c * c - b * b - d * d,
            2.0 * (c * d - a * b),
        ],
        [
            2.0 * (b * d - a * c),
            2.0 * (c * d + a * b),
            a * a + d * d - c * c - b * b,
        ],
    ];
    let scales = [
        header.pix_dim[1],
        header.pix_dim[2],
        header.q_fac() * header.pix_dim[3],
    ];
    let mut columns = [Vec3::ZERO; 3];
    for column in 0..3 {
        let direction = Vec3::new(r[0][column], r[1][column], r[2][column]);
        let direction = snap_small(direction).normalize();
        columns[column] = direction.scale(scales[column]);
    }
    let offset = Vec3::new(header.q_offset[0], header.q_offset[1], header.q_offset[2]);
    new_xform(columns, offset, "qform")
}
fn snap_small(mut v: Vec3) -> Vec3 {
    if v.x.abs() < QUATERNION_SNAP {
        v.x = 0.0;
    }
    if v.y.abs() < QUATERNION_SNAP {
        v.y = 0.0;
    }
    if v.z.abs() < QUATERNION_SNAP {
        v.z = 0.0;
    }
    v
}
fn sform_transform(header: &Header) -> Result<Xform, Error> {
    let s = &header.s_row;
    let mut columns = [Vec3::ZERO; 3];
    for (column, slot) in columns.iter_mut().enumerate() {
        *slot = Vec3::new(s[0][column], s[1][column], s[2][column]);
    }
    let offset = Vec3::new(s[0][3], s[1][3], s[2][3]);
    new_xform(columns, offset, "sform")
}
fn pixdim_transform(header: &Header) -> Result<Xform, Error> {
    for index in 1..=3 {
        if !finite_positive(header.pix_dim[index]) {
            return Err(invalid("pixdim"));
        }
    }
    let columns = [
        Vec3::new(header.pix_dim[1], 0.0, 0.0),
        Vec3::new(0.0, header.pix_dim[2], 0.0),
        Vec3::new(0.0, 0.0, header.pix_dim[3]),
    ];
    new_xform(columns, Vec3::ZERO, "pixdim")
}
fn same_transform(a: &Xform, b: &Xform) -> bool {
    for index in 0..3 {
        let scale = fmath::max(a.norms[index], b.norms[index]);
        if a.columns[index].sub(b.columns[index]).length() > 1e-4 * scale {
            return false;
        }
    }
    let smallest = fmath::min(a.norms[0], fmath::min(a.norms[1], a.norms[2]));
    let tolerance = 1e-3 * smallest + 1e-6 * fmath::max(a.offset.length(), b.offset.length());
    a.offset.sub(b.offset).length() <= tolerance
}
fn non_orthogonal() -> Error {
    read_error(
        ReadCode::Geometry,
        "sform",
        Sentinel::NonOrthogonalSForm,
        None,
    )
}
fn orthogonalize_sform(
    x: &Xform,
    header: &Header,
    max_deviation_degrees: f64,
) -> Result<(Xform, f64), Error> {
    let mut matrix = [[0.0f64; 3]; 3];
    for column in 0..3 {
        let unit = x.unit(column);
        matrix[0][column] = unit.x;
        matrix[1][column] = unit.y;
        matrix[2][column] = unit.z;
    }
    let Some(rotation) = polar_factor(matrix) else {
        return Err(non_orthogonal());
    };
    let mut columns = [Vec3::ZERO; 3];
    let mut deviation = 0.0f64;
    for column in 0..3 {
        let direction = Vec3::new(
            rotation[0][column],
            rotation[1][column],
            rotation[2][column],
        )
        .normalize();
        let cosine = fmath::max(-1.0, fmath::min(1.0, direction.dot(x.unit(column))));
        deviation = fmath::max(deviation, fmath::acos(cosine) * 180.0 / PI);
        let mut spacing = x.norms[column];
        let pixdim = header.pix_dim[column + 1];
        if finite_positive(pixdim) {
            spacing = pixdim;
        }
        columns[column] = direction.scale(spacing);
    }
    if !(deviation <= max_deviation_degrees) {
        return Err(non_orthogonal());
    }
    let out = new_xform(columns, x.offset, "sform")?;
    Ok((out, deviation))
}
fn polar_factor(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let mut q = m;
    for _ in 0..64 {
        let inverse = invert3(&q)?;
        let mut next = [[0.0f64; 3]; 3];
        let mut change = 0.0f64;
        for r in 0..3 {
            for c in 0..3 {
                next[r][c] = 0.5 * (q[r][c] + inverse[c][r]);
                change = fmath::max(change, (next[r][c] - q[r][c]).abs());
            }
        }
        q = next;
        if change < 1e-15 {
            break;
        }
    }
    if q.iter().flatten().any(|&value| !finite(value)) {
        return None;
    }
    Some(q)
}
pub(crate) fn invert3(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let (a, b, c) = (m[0][0], m[0][1], m[0][2]);
    let (d, e, f) = (m[1][0], m[1][1], m[1][2]);
    let (g, h, i) = (m[2][0], m[2][1], m[2][2]);
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if !finite(det) || det.abs() < 1e-300 {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        [
            (e * i - f * h) * inv,
            (c * h - b * i) * inv,
            (b * f - c * e) * inv,
        ],
        [
            (f * g - d * i) * inv,
            (a * i - c * g) * inv,
            (c * d - a * f) * inv,
        ],
        [
            (d * h - e * g) * inv,
            (b * g - a * h) * inv,
            (a * e - b * d) * inv,
        ],
    ])
}
pub(crate) fn resolve_geometry(header: &Header, options: &ReadOptions) -> Result<Geometry, Error> {
    if !header.qform_code.valid() {
        return Err(read_error(
            ReadCode::Header,
            "qform_code",
            Sentinel::MalformedHeader,
            None,
        ));
    }
    if !header.sform_code.valid() {
        return Err(read_error(
            ReadCode::Header,
            "sform_code",
            Sentinel::MalformedHeader,
            None,
        ));
    }
    let mut geometry = Geometry {
        qform_code: header.qform_code,
        sform_code: header.sform_code,
        ..Geometry::default()
    };
    let has_q = header.qform_code != XformCode::UNKNOWN;
    let has_s = header.sform_code != XformCode::UNKNOWN;
    let qform = if has_q {
        Some(qform_transform(header))
    } else {
        None
    };
    let sform = if has_s {
        Some(sform_transform(header))
    } else {
        None
    };
    let q_ok = qform.as_ref().and_then(|r| r.as_ref().ok()).copied();
    let s_ok = sform.as_ref().and_then(|r| r.as_ref().ok()).copied();
    let q_err = qform.and_then(|r| r.err());
    let s_err = sform.and_then(|r| r.err());
    let (q_valid, s_valid) = (q_ok.is_some(), s_ok.is_some());
    let s_orthonormal = s_ok.is_some_and(|s| s.orthonormal());
    geometry.qform_sform_agree = match (q_ok, s_ok) {
        (Some(q), Some(s)) => s_orthonormal && same_transform(&q, &s),
        _ => false,
    };
    let mut max_deviation = options.permissive_max_deviation_degrees;
    if max_deviation == 0.0 {
        max_deviation = DEFAULT_PERMISSIVE_MAX_DEVIATION_DEGREES;
    }
    let use_sform = |geometry: &mut Geometry| -> Result<Xform, Error> {
        let sform = s_ok.unwrap_or_default();
        let chosen;
        if !s_orthonormal {
            if !options.sform_permissive {
                return Err(non_orthogonal());
            }
            let (corrected, deviation) = orthogonalize_sform(&sform, header, max_deviation)?;
            chosen = corrected;
            geometry.source = XformSource::SFormOrthogonalized;
            geometry.sform_corrected = true;
            geometry.sform_deviation_degrees = deviation;
            geometry
                .warnings
                .push(WARNING_SFORM_ORTHOGONALIZED.to_string());
        } else {
            chosen = sform;
            geometry.source = XformSource::SForm;
            for index in 0..3 {
                let pixdim = header.pix_dim[index + 1];
                if finite_positive(pixdim) && !nearly_equal(sform.norms[index], pixdim, 1e-3) {
                    geometry
                        .warnings
                        .push(WARNING_SFORM_PIXDIM_MISMATCH.to_string());
                    break;
                }
            }
        }
        geometry.code = header.sform_code;
        Ok(chosen)
    };
    let use_qform = |geometry: &mut Geometry| -> Xform {
        geometry.source = XformSource::QForm;
        geometry.code = header.qform_code;
        q_ok.unwrap_or_default()
    };
    let mut chosen = match options.xform {
        XformPolicy::QFORM => {
            if !has_q {
                return Err(read_error(
                    ReadCode::Geometry,
                    "qform",
                    Sentinel::NoSpatialTransform,
                    None,
                ));
            }
            if let Some(err) = q_err {
                return Err(err);
            }
            use_qform(&mut geometry)
        }
        XformPolicy::SFORM => {
            if !has_s {
                return Err(read_error(
                    ReadCode::Geometry,
                    "sform",
                    Sentinel::NoSpatialTransform,
                    None,
                ));
            }
            if let Some(err) = s_err {
                return Err(err);
            }
            use_sform(&mut geometry)?
        }
        XformPolicy::AUTO => {
            if !has_q && !has_s {
                if !options.allow_missing_transform {
                    return Err(read_error(
                        ReadCode::Geometry,
                        "xform",
                        Sentinel::NoSpatialTransform,
                        None,
                    ));
                }
                let pixdim = pixdim_transform(header)?;
                geometry.source = XformSource::PixDim;
                geometry
                    .warnings
                    .push(WARNING_NO_SPATIAL_TRANSFORM.to_string());
                pixdim
            } else if s_orthonormal
                && (!q_valid
                    || header.sform_code == XformCode::SCANNER_ANAT
                    || geometry.qform_sform_agree)
            {
                if has_q && !q_valid {
                    geometry.warnings.push(WARNING_QFORM_INVALID.to_string());
                }
                use_sform(&mut geometry)?
            } else if q_valid {
                if s_valid && !s_orthonormal {
                    geometry
                        .warnings
                        .push(WARNING_SFORM_NON_ORTHOGONAL.to_string());
                } else if s_orthonormal {
                    geometry.warnings.push(WARNING_SFORM_IGNORED.to_string());
                }
                use_qform(&mut geometry)
            } else if s_valid {
                if has_q {
                    geometry.warnings.push(WARNING_QFORM_INVALID.to_string());
                }
                use_sform(&mut geometry)?
            } else if let Some(err) = s_err {
                return Err(err);
            } else {
                return Err(q_err.unwrap_or_else(|| invalid("xform")));
            }
        }
        _ => return Err(invalid("xform")),
    };
    let scale = unit_millimeters(header.spatial_unit());
    for index in 0..3 {
        chosen.columns[index] = chosen.columns[index].scale(scale);
        geometry.spacing[index] = chosen.norms[index] * scale;
    }
    chosen.offset = chosen.offset.scale(scale);
    geometry.index_to_ras = chosen.affine();
    geometry.index_to_lps = affine_lps2ras(geometry.index_to_ras);
    if !geometry.index_to_ras.finite() || !geometry.index_to_lps.finite() {
        return Err(invalid("xform"));
    }
    if geometry.spacing.iter().any(|&s| !finite_positive(s)) {
        return Err(invalid("xform"));
    }
    geometry.left_handed = chosen.determinant() < 0.0;
    if geometry.source != XformSource::PixDim && geometry.code != XformCode::SCANNER_ANAT {
        geometry
            .warnings
            .push(WARNING_NON_SCANNER_COORDINATES.to_string());
    }
    Ok(geometry)
}
