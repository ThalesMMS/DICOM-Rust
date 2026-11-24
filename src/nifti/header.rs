use super::{Error, ReadCode, Sentinel, read_error};
use crate::fmath;
use crate::render::GeometryAffine;
pub(crate) const NIFTI1_HEADER_SIZE: usize = 348;
pub(crate) const NIFTI2_HEADER_SIZE: usize = 540;
const MAX_HEADER_DIMENSION: i64 = i32::MAX as i64;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct XformCode(pub i32);
impl XformCode {
    pub const UNKNOWN: XformCode = XformCode(0);
    pub const SCANNER_ANAT: XformCode = XformCode(1);
    #[doc(hidden)]
    pub const ALIGNED_ANAT: XformCode = XformCode(2);
    #[doc(hidden)]
    pub const TALAIRACH: XformCode = XformCode(3);
    #[doc(hidden)]
    pub const MNI152: XformCode = XformCode(4);
    #[doc(hidden)]
    pub const TEMPLATE_OTHER: XformCode = XformCode(5);
    pub fn as_str(self) -> &'static str {
        match self.0 {
            0 => "unknown",
            1 => "scanner-anat",
            2 => "aligned-anat",
            3 => "talairach",
            4 => "mni-152",
            5 => "template-other",
            _ => "invalid",
        }
    }
    pub(crate) fn valid(self) -> bool {
        (0..=5).contains(&self.0)
    }
}
pub const SPATIAL_UNIT_UNKNOWN: u8 = 0;
pub const SPATIAL_UNIT_METER: u8 = 1;
pub const SPATIAL_UNIT_MILLIMETER: u8 = 2;
pub const SPATIAL_UNIT_MICROMETER: u8 = 3;
const SPATIAL_UNIT_MASK: u8 = 0x07;
const TEMPORAL_UNIT_MASK: u8 = 0x38;
pub(crate) fn unit_millimeters(unit: u8) -> f64 {
    match unit {
        SPATIAL_UNIT_METER => 1000.0,
        SPATIAL_UNIT_MICROMETER => 0.001,
        _ => 1.0,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Header {
    pub version: i64,
    pub paired: bool,
    pub big_endian: bool,
    pub dim: [i64; 8],
    pub pix_dim: [f64; 8],
    pub datatype: i16,
    pub bit_pix: i16,
    pub voxel_offset: f64,
    pub scl_slope: f64,
    pub scl_inter: f64,
    pub cal_max: f64,
    pub cal_min: f64,
    pub slice_duration: f64,
    pub t_offset: f64,
    pub slice_start: i64,
    pub slice_end: i64,
    pub slice_code: u8,
    pub dim_info: u8,
    pub xyzt_units: u8,
    pub intent_code: i32,
    pub intent_params: [f64; 3],
    pub qform_code: XformCode,
    pub sform_code: XformCode,
    pub quatern_b: f64,
    pub quatern_c: f64,
    pub quatern_d: f64,
    pub q_offset: [f64; 3],
    pub s_row: [[f64; 4]; 3],
    pub extended: bool,
}
impl Header {
    pub fn size(&self) -> usize {
        if self.version == 2 {
            NIFTI2_HEADER_SIZE
        } else {
            NIFTI1_HEADER_SIZE
        }
    }
    pub fn spatial_unit(&self) -> u8 {
        self.xyzt_units & SPATIAL_UNIT_MASK
    }
    pub fn temporal_unit(&self) -> u8 {
        self.xyzt_units & TEMPORAL_UNIT_MASK
    }
    pub fn q_fac(&self) -> f64 {
        if self.pix_dim[0] < 0.0 { -1.0 } else { 1.0 }
    }
}
struct Fields<'a> {
    data: &'a [u8],
    big: bool,
}
impl Fields<'_> {
    fn u16(&self, offset: usize) -> u16 {
        let b = [self.data[offset], self.data[offset + 1]];
        if self.big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        }
    }
    fn u32(&self, offset: usize) -> u32 {
        let b: [u8; 4] = self.data[offset..offset + 4].try_into().expect("4 bytes");
        if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        }
    }
    fn u64(&self, offset: usize) -> u64 {
        let b: [u8; 8] = self.data[offset..offset + 8].try_into().expect("8 bytes");
        if self.big {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        }
    }
    fn i16(&self, offset: usize) -> i16 {
        self.u16(offset) as i16
    }
    fn i32(&self, offset: usize) -> i32 {
        self.u32(offset) as i32
    }
    fn i64(&self, offset: usize) -> i64 {
        self.u64(offset) as i64
    }
    fn f32(&self, offset: usize) -> f64 {
        f32::from_bits(self.u32(offset)) as f64
    }
    fn f64(&self, offset: usize) -> f64 {
        f64::from_bits(self.u64(offset))
    }
}
pub fn parse_header(data: &[u8]) -> Result<Header, Error> {
    if data.len() < 4 {
        return Err(read_error(
            ReadCode::Format,
            "sizeof_hdr",
            Sentinel::NotNifti,
            None,
        ));
    }
    let le = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let be = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let (big, version) = if le == NIFTI1_HEADER_SIZE {
        (false, 1)
    } else if be == NIFTI1_HEADER_SIZE {
        (true, 1)
    } else if le == NIFTI2_HEADER_SIZE {
        (false, 2)
    } else if be == NIFTI2_HEADER_SIZE {
        (true, 2)
    } else {
        return Err(read_error(
            ReadCode::Format,
            "sizeof_hdr",
            Sentinel::NotNifti,
            None,
        ));
    };
    let size = if version == 2 {
        NIFTI2_HEADER_SIZE
    } else {
        NIFTI1_HEADER_SIZE
    };
    if data.len() < size {
        return Err(read_error(
            ReadCode::Truncated,
            "header",
            Sentinel::TruncatedData,
            None,
        ));
    }
    let fields = Fields {
        data: &data[..size],
        big,
    };
    let mut header = if version == 1 {
        parse_nifti1(&fields)?
    } else {
        parse_nifti2(&fields)?
    };
    header.big_endian = big;
    normalize_header_dimensions(&mut header)?;
    Ok(header)
}
fn parse_nifti1(f: &Fields) -> Result<Header, Error> {
    let mut header = Header {
        version: 1,
        ..Header::default()
    };
    match &f.data[344..348] {
        b"n+1\0" => {}
        b"ni1\0" => header.paired = true,
        _ => {
            return Err(read_error(
                ReadCode::Format,
                "magic",
                Sentinel::NotNifti,
                None,
            ));
        }
    }
    header.dim_info = f.data[39];
    for index in 0..8 {
        header.dim[index] = f.i16(40 + 2 * index) as i64;
    }
    for index in 0..3 {
        header.intent_params[index] = f.f32(56 + 4 * index);
    }
    header.intent_code = f.i16(68) as i32;
    header.datatype = f.i16(70);
    header.bit_pix = f.i16(72);
    header.slice_start = f.i16(74) as i64;
    for index in 0..8 {
        header.pix_dim[index] = f.f32(76 + 4 * index);
    }
    header.voxel_offset = f.f32(108);
    header.scl_slope = f.f32(112);
    header.scl_inter = f.f32(116);
    header.slice_end = f.i16(120) as i64;
    header.slice_code = f.data[122];
    header.xyzt_units = f.data[123];
    header.cal_max = f.f32(124);
    header.cal_min = f.f32(128);
    header.slice_duration = f.f32(132);
    header.t_offset = f.f32(136);
    header.qform_code = XformCode(f.i16(252) as i32);
    header.sform_code = XformCode(f.i16(254) as i32);
    header.quatern_b = f.f32(256);
    header.quatern_c = f.f32(260);
    header.quatern_d = f.f32(264);
    for index in 0..3 {
        header.q_offset[index] = f.f32(268 + 4 * index);
    }
    for row in 0..3 {
        for column in 0..4 {
            header.s_row[row][column] = f.f32(280 + 16 * row + 4 * column);
        }
    }
    Ok(header)
}
fn parse_nifti2(f: &Fields) -> Result<Header, Error> {
    let mut header = Header {
        version: 2,
        ..Header::default()
    };
    let magic = &f.data[4..12];
    if magic == b"n+2\0\r\n\x1a\n" {
    } else if magic == b"ni2\0\r\n\x1a\n" {
        header.paired = true;
    } else if &magic[..4] == b"n+2\0" || &magic[..4] == b"ni2\0" {
        return Err(read_error(
            ReadCode::Header,
            "magic",
            Sentinel::MalformedHeader,
            None,
        ));
    } else {
        return Err(read_error(
            ReadCode::Format,
            "magic",
            Sentinel::NotNifti,
            None,
        ));
    }
    header.datatype = f.i16(12);
    header.bit_pix = f.i16(14);
    for index in 0..8 {
        header.dim[index] = f.i64(16 + 8 * index);
    }
    for index in 0..3 {
        header.intent_params[index] = f.f64(80 + 8 * index);
    }
    for index in 0..8 {
        header.pix_dim[index] = f.f64(104 + 8 * index);
    }
    let vox_offset = f.i64(168);
    header.voxel_offset = vox_offset as f64;
    if fmath::f64_to_i64(header.voxel_offset) != vox_offset {
        return Err(read_error(
            ReadCode::Header,
            "vox_offset",
            Sentinel::MalformedHeader,
            None,
        ));
    }
    header.scl_slope = f.f64(176);
    header.scl_inter = f.f64(184);
    header.cal_max = f.f64(192);
    header.cal_min = f.f64(200);
    header.slice_duration = f.f64(208);
    header.t_offset = f.f64(216);
    header.slice_start = f.i64(224);
    header.slice_end = f.i64(232);
    header.qform_code = XformCode(f.i32(344));
    header.sform_code = XformCode(f.i32(348));
    header.quatern_b = f.f64(352);
    header.quatern_c = f.f64(360);
    header.quatern_d = f.f64(368);
    for index in 0..3 {
        header.q_offset[index] = f.f64(376 + 8 * index);
    }
    for row in 0..3 {
        for column in 0..4 {
            header.s_row[row][column] = f.f64(400 + 32 * row + 8 * column);
        }
    }
    header.slice_code = f.i32(496) as u8;
    header.xyzt_units = f.i32(500) as u8;
    header.intent_code = f.i32(504);
    header.dim_info = f.data[524];
    Ok(header)
}
fn normalize_header_dimensions(header: &mut Header) -> Result<(), Error> {
    let count = header.dim[0];
    if !(1..=7).contains(&count) {
        return Err(read_error(
            ReadCode::Dimensions,
            "dim[0]",
            Sentinel::MalformedHeader,
            None,
        ));
    }
    for index in 1..8 {
        if index as i64 > count {
            header.dim[index] = 1;
            continue;
        }
        if header.dim[index] < 1 || header.dim[index] > MAX_HEADER_DIMENSION {
            return Err(read_error(
                ReadCode::Dimensions,
                "dim",
                Sentinel::MalformedHeader,
                None,
            ));
        }
    }
    Ok(())
}
const HEADER_DESCRIPTION: &[u8] = b"dcm-rust NIfTI-1 export";
const TEMPORAL_UNIT_SECOND: u8 = 8;
#[derive(Debug, Clone, Copy)]
pub(crate) struct HeaderSpec {
    pub dimensions: [i64; 4],
    pub datatype: i16,
    pub bit_pix: i16,
    pub spacing: [f64; 4],
    pub voxel_offset: i64,
    pub slope: f64,
    pub intercept: f64,
    pub time_offset: f64,
    pub temporal_unit: u8,
    pub affine_ras: GeometryAffine,
    pub no_spatial_transform: bool,
}
pub(crate) fn build_header(spec: &HeaderSpec) -> Result<Vec<u8>, Error> {
    validate_header_spec(spec)?;
    let mut header = vec![0u8; NIFTI1_HEADER_SIZE];
    fn put_i16(header: &mut [u8], offset: usize, value: i16) {
        header[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_f32(header: &mut [u8], offset: usize, value: f64) {
        header[offset..offset + 4].copy_from_slice(&(value as f32).to_bits().to_le_bytes());
    }
    header[0..4].copy_from_slice(&(NIFTI1_HEADER_SIZE as i32).to_le_bytes());
    let dimension_count: i16 = if spec.dimensions[3] > 1 { 4 } else { 3 };
    put_i16(&mut header, 40, dimension_count);
    for (index, &dimension) in spec.dimensions.iter().enumerate() {
        put_i16(&mut header, 42 + 2 * index, dimension as i16);
    }
    for index in 5..8 {
        put_i16(&mut header, 40 + 2 * index, 1);
    }
    put_i16(&mut header, 70, spec.datatype);
    put_i16(&mut header, 72, spec.bit_pix);
    let (mut quaternion, mut qfac) = affine_quaternion(&spec.affine_ras, &spec.spacing);
    if spec.no_spatial_transform {
        (quaternion, qfac) = ([0.0; 3], 1.0);
    }
    put_f32(&mut header, 76, qfac);
    for (index, &spacing) in spec.spacing.iter().enumerate() {
        put_f32(&mut header, 80 + 4 * index, spacing);
    }
    put_f32(&mut header, 108, spec.voxel_offset as f64);
    put_f32(&mut header, 112, spec.slope);
    put_f32(&mut header, 116, spec.intercept);
    header[123] = SPATIAL_UNIT_MILLIMETER;
    if spec.dimensions[3] > 1 {
        let unit = if spec.temporal_unit == 0 {
            TEMPORAL_UNIT_SECOND
        } else {
            spec.temporal_unit
        };
        header[123] |= unit;
        put_f32(&mut header, 136, spec.time_offset);
    }
    header[148..148 + HEADER_DESCRIPTION.len()].copy_from_slice(HEADER_DESCRIPTION);
    header[344..348].copy_from_slice(b"n+1\0");
    if spec.no_spatial_transform {
        return Ok(header);
    }
    let a = &spec.affine_ras.0;
    put_i16(&mut header, 252, 1);
    put_i16(&mut header, 254, 1);
    put_f32(&mut header, 256, quaternion[0]);
    put_f32(&mut header, 260, quaternion[1]);
    put_f32(&mut header, 264, quaternion[2]);
    put_f32(&mut header, 268, a[3]);
    put_f32(&mut header, 272, a[7]);
    put_f32(&mut header, 276, a[11]);
    for (row, offset) in [280usize, 296, 312].into_iter().enumerate() {
        for column in 0..4 {
            put_f32(&mut header, offset + 4 * column, a[4 * row + column]);
        }
    }
    Ok(header)
}
fn unsupported_header(reason: String) -> Error {
    Error::Header(reason)
}
fn validate_header_spec(spec: &HeaderSpec) -> Result<(), Error> {
    for (index, &dimension) in spec.dimensions.iter().enumerate() {
        if !(1..=i16::MAX as i64).contains(&dimension) {
            return Err(unsupported_header(format!(
                "dimension {} is {}, want 1..32767",
                index + 1,
                dimension
            )));
        }
    }
    let Some(want_bit_pix) = supported_datatype_bit_pix(spec.datatype) else {
        return Err(unsupported_header(format!(
            "unsupported datatype {}",
            spec.datatype
        )));
    };
    if spec.bit_pix != want_bit_pix {
        return Err(unsupported_header(format!(
            "datatype {} requires bitpix {}, got {}",
            spec.datatype, want_bit_pix, spec.bit_pix
        )));
    }
    for (index, &spacing) in spec.spacing.iter().enumerate() {
        if spacing <= 0.0 || (spacing as f32) <= 0.0 || !fits_finite_float32(spacing) {
            return Err(unsupported_header(format!(
                "spacing {} is not a positive finite float32",
                index + 1
            )));
        }
    }
    if spec.voxel_offset < 352 || spec.voxel_offset % 16 != 0 {
        return Err(unsupported_header(format!(
            "voxel offset {} is not a 16-byte-aligned offset at or after 352",
            spec.voxel_offset
        )));
    }
    let float_offset = spec.voxel_offset as f32;
    if float_offset.is_infinite() || fmath::f64_to_i64(float_offset as f64) != spec.voxel_offset {
        return Err(unsupported_header(format!(
            "voxel offset {} is not exactly representable as float32",
            spec.voxel_offset
        )));
    }
    if spec.slope == 0.0 || (spec.slope as f32) == 0.0 || !fits_finite_float32(spec.slope) {
        return Err(unsupported_header(
            "scaling slope is zero, non-finite, or outside float32".to_string(),
        ));
    }
    if !fits_finite_float32(spec.intercept) {
        return Err(unsupported_header(
            "scaling intercept is non-finite or outside float32".to_string(),
        ));
    }
    if !fits_finite_float32(spec.time_offset)
        || (spec.time_offset != 0.0 && (spec.time_offset as f32) == 0.0)
    {
        return Err(unsupported_header(
            "time offset is non-finite or outside float32".to_string(),
        ));
    }
    if !spec.affine_ras.finite() {
        return Err(unsupported_header(
            "affine is non-finite or has an invalid homogeneous row".to_string(),
        ));
    }
    let a = &spec.affine_ras.0;
    if a.iter().any(|&value| !fits_finite_float32(value)) {
        return Err(unsupported_header(
            "affine coefficient is outside float32".to_string(),
        ));
    }
    let columns = [[a[0], a[4], a[8]], [a[1], a[5], a[9]], [a[2], a[6], a[10]]];
    let mut norms = [0.0f64; 3];
    for (index, column) in columns.iter().enumerate() {
        norms[index] = dot3(column, column).sqrt();
        if !nearly_equal(norms[index], spec.spacing[index], 1e-5) {
            return Err(unsupported_header(format!(
                "affine column {} norm does not match spacing",
                index + 1
            )));
        }
    }
    for first in 0..3 {
        for second in first + 1..3 {
            let cosine = dot3(&columns[first], &columns[second]) / (norms[first] * norms[second]);
            if cosine.abs() > 1e-5 {
                return Err(unsupported_header(format!(
                    "affine columns {} and {} are not orthogonal",
                    first + 1,
                    second + 1
                )));
            }
        }
    }
    Ok(())
}
fn supported_datatype_bit_pix(datatype: i16) -> Option<i16> {
    match datatype {
        2 | 256 => Some(8),
        4 | 512 => Some(16),
        8 | 16 | 768 => Some(32),
        64 => Some(64),
        _ => None,
    }
}
fn fits_finite_float32(value: f64) -> bool {
    let converted = value as f32;
    !value.is_nan() && !value.is_infinite() && !converted.is_nan() && !converted.is_infinite()
}
fn dot3(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn nearly_equal(a: f64, b: f64, relative_tolerance: f64) -> bool {
    let scale = fmath::max(a.abs(), b.abs());
    if scale == 0.0 {
        return true;
    }
    (a - b).abs() <= relative_tolerance * scale
}
fn affine_quaternion(affine: &GeometryAffine, spacing: &[f64; 4]) -> ([f64; 3], f64) {
    let m = &affine.0;
    let mut r = [
        m[0] / spacing[0],
        m[1] / spacing[1],
        m[2] / spacing[2],
        m[4] / spacing[0],
        m[5] / spacing[1],
        m[6] / spacing[2],
        m[8] / spacing[0],
        m[9] / spacing[1],
        m[10] / spacing[2],
    ];
    let determinant = r[0] * (r[4] * r[8] - r[5] * r[7]) - r[1] * (r[3] * r[8] - r[5] * r[6])
        + r[2] * (r[3] * r[7] - r[4] * r[6]);
    let mut qfac = 1.0;
    if determinant < 0.0 {
        qfac = -1.0;
        r[2] = -r[2];
        r[5] = -r[5];
        r[8] = -r[8];
    }
    let (mut a, mut b, mut c, mut d);
    let trace = r[0] + r[4] + r[8];
    if trace > 0.0 {
        let s = 2.0 * (trace + 1.0).sqrt();
        a = s / 4.0;
        b = (r[7] - r[5]) / s;
        c = (r[2] - r[6]) / s;
        d = (r[3] - r[1]) / s;
    } else if r[0] > r[4] && r[0] > r[8] {
        let s = 2.0 * (1.0 + r[0] - r[4] - r[8]).sqrt();
        a = (r[7] - r[5]) / s;
        b = s / 4.0;
        c = (r[1] + r[3]) / s;
        d = (r[2] + r[6]) / s;
    } else if r[4] > r[8] {
        let s = 2.0 * (1.0 + r[4] - r[0] - r[8]).sqrt();
        a = (r[2] - r[6]) / s;
        b = (r[1] + r[3]) / s;
        c = s / 4.0;
        d = (r[5] + r[7]) / s;
    } else {
        let s = 2.0 * (1.0 + r[8] - r[0] - r[4]).sqrt();
        a = (r[3] - r[1]) / s;
        b = (r[2] + r[6]) / s;
        c = (r[5] + r[7]) / s;
        d = s / 4.0;
    }
    let length = (a * a + b * b + c * c + d * d).sqrt();
    if length != 0.0 {
        (a, b, c, d) = (a / length, b / length, c / length, d / length);
    }
    if a < 0.0 {
        (b, c, d) = (-b, -c, -d);
    }
    let _ = a;
    ([b, c, d], qfac)
}
pub(crate) fn affine_lps2ras(mut affine: GeometryAffine) -> GeometryAffine {
    for column in 0..4 {
        affine.0[column] = -affine.0[column];
        affine.0[4 + column] = -affine.0[4 + column];
    }
    affine
}
