#![forbid(unsafe_code)]
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop)]
pub mod bzip2;
pub mod dump;
pub mod export;
pub mod geometry;
pub mod gzip;
pub mod header;
pub mod label_table;
pub mod segconv;
pub mod sidecar;
pub mod view;
use crate::render::{
    GeometryAffine, GeometryTolerances, SliceGeometry, VolumeGeometry, build_volume_geometry,
};
pub use geometry::{Geometry, XformPolicy, XformSource};
pub use gzip::{ByteStream, Terminal};
pub use header::{Header, XformCode, parse_header};
use header::{NIFTI1_HEADER_SIZE, NIFTI2_HEADER_SIZE};
use std::fmt;
pub const DATATYPE_UINT8: i16 = 2;
pub const DATATYPE_INT16: i16 = 4;
pub const DATATYPE_INT32: i16 = 8;
pub const DATATYPE_FLOAT32: i16 = 16;
pub const DATATYPE_FLOAT64: i16 = 64;
pub const DATATYPE_INT8: i16 = 256;
pub const DATATYPE_UINT16: i16 = 512;
pub const DATATYPE_UINT32: i16 = 768;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentinel {
    InvalidSource,
    InvalidGeometry,
    UnsupportedPixels,
    UnsupportedScaling,
    LimitExceeded,
    NotNifti,
    MalformedHeader,
    UnsupportedDatatype,
    UnsupportedDimensions,
    NoSpatialTransform,
    InvalidTransform,
    NonOrthogonalSForm,
    TruncatedData,
    PairedFile,
    GpuScalarProfile,
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        match self {
            Sentinel::InvalidSource => "dicom/nifti: invalid source",
            Sentinel::InvalidGeometry => "dicom/nifti: invalid or unsupported geometry",
            Sentinel::UnsupportedPixels => "dicom/nifti: unsupported pixel representation",
            Sentinel::UnsupportedScaling => "dicom/nifti: unsupported scaling",
            Sentinel::LimitExceeded => "dicom/nifti: resource limit exceeded",
            Sentinel::NotNifti => "dicom/nifti: not a NIfTI-1 or NIfTI-2 stream",
            Sentinel::MalformedHeader => "dicom/nifti: malformed NIfTI header",
            Sentinel::UnsupportedDatatype => "dicom/nifti: unsupported NIfTI datatype",
            Sentinel::UnsupportedDimensions => "dicom/nifti: unsupported NIfTI dimensions",
            Sentinel::NoSpatialTransform => "dicom/nifti: no usable spatial transform",
            Sentinel::InvalidTransform => "dicom/nifti: invalid spatial transform",
            Sentinel::NonOrthogonalSForm => "dicom/nifti: non-orthogonal sform",
            Sentinel::TruncatedData => "dicom/nifti: truncated NIfTI data",
            Sentinel::PairedFile => "dicom/nifti: header and image are separate files",
            Sentinel::GpuScalarProfile => {
                "dicom/nifti: GPU float32 display requires finite representable samples"
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCode {
    Format,
    Header,
    Datatype,
    Dimensions,
    Geometry,
    Limit,
    Truncated,
    Io,
    Canceled,
}
impl ReadCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ReadCode::Format => "format",
            ReadCode::Header => "header",
            ReadCode::Datatype => "datatype",
            ReadCode::Dimensions => "dimensions",
            ReadCode::Geometry => "geometry",
            ReadCode::Limit => "limit",
            ReadCode::Truncated => "truncated",
            ReadCode::Io => "io",
            ReadCode::Canceled => "canceled",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadError {
    pub code: ReadCode,
    pub field: &'static str,
    pub datatype: i16,
    pub sentinel: Sentinel,
    pub cause: Option<String>,
}
impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dicom/nifti: read {}", self.code.as_str())?;
        if !self.field.is_empty() {
            write!(f, " ({})", self.field)?;
        }
        if self.code == ReadCode::Datatype {
            write!(f, " datatype {}", self.datatype)?;
        }
        Ok(())
    }
}
impl std::error::Error for ReadError {}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Read(ReadError),
    Sentinel(Sentinel),
    Header(String),
}
impl Error {
    pub fn is(&self, sentinel: Sentinel) -> bool {
        match self {
            Error::Read(read) => read.sentinel == sentinel,
            Error::Sentinel(own) => *own == sentinel,
            Error::Header(_) => false,
        }
    }
    #[doc(hidden)]
    pub fn read_code(&self) -> Option<ReadCode> {
        match self {
            Error::Read(read) => Some(read.code),
            _ => None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Read(read) => read.fmt(f),
            Error::Sentinel(sentinel) => f.write_str(sentinel.text()),
            Error::Header(reason) => write!(f, "nifti: invalid header: {reason}"),
        }
    }
}
impl std::error::Error for Error {}
pub(crate) fn read_error(
    code: ReadCode,
    field: &'static str,
    sentinel: Sentinel,
    cause: Option<String>,
) -> Error {
    Error::Read(ReadError {
        code,
        field,
        datatype: 0,
        sentinel,
        cause,
    })
}
fn datatype_error(datatype: i16) -> Error {
    Error::Read(ReadError {
        code: ReadCode::Datatype,
        field: "datatype",
        datatype,
        sentinel: Sentinel::UnsupportedDatatype,
        cause: None,
    })
}
pub(crate) fn finite(value: f64) -> bool {
    !value.is_nan() && !value.is_infinite()
}
pub(crate) fn finite_positive(value: f64) -> bool {
    finite(value) && value > 0.0
}
pub fn strict_geometry_tolerances() -> GeometryTolerances {
    const TENTH_DEGREE: f64 = 0.0017453292519943296;
    GeometryTolerances {
        spacing_rel: 0.001,
        spacing_abs: 0.001,
        orientation_cos: crate::fmath::cos(TENTH_DEGREE),
        tilt_cos: crate::fmath::cos(TENTH_DEGREE),
        position_abs: 1e-6,
        shear_abs: crate::fmath::tan(TENTH_DEGREE),
        affine_round_trip: 1e-9,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReadLimits {
    pub max_voxels: u64,
    pub max_payload_bytes: u64,
    pub max_header_bytes: u64,
}
pub fn default_read_limits() -> ReadLimits {
    ReadLimits {
        max_voxels: 1 << 32,
        max_payload_bytes: 16 << 30,
        max_header_bytes: 64 << 20,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReadOptions {
    pub limits: ReadLimits,
    pub xform: XformPolicy,
    pub sform_permissive: bool,
    pub permissive_max_deviation_degrees: f64,
    pub allow_missing_transform: bool,
}
fn fill_limits(mut limits: ReadLimits) -> ReadLimits {
    let defaults = default_read_limits();
    if limits.max_voxels == 0 {
        limits.max_voxels = defaults.max_voxels;
    }
    if limits.max_payload_bytes == 0 {
        limits.max_payload_bytes = defaults.max_payload_bytes;
    }
    if limits.max_header_bytes == 0 {
        limits.max_header_bytes = defaults.max_header_bytes;
    }
    limits
}
fn normalized_read_options(mut options: ReadOptions) -> Result<ReadOptions, Error> {
    options.limits = fill_limits(options.limits);
    if options.xform.0 > XformPolicy::SFORM.0 {
        return Err(read_error(
            ReadCode::Geometry,
            "xform",
            Sentinel::InvalidTransform,
            None,
        ));
    }
    let deviation = options.permissive_max_deviation_degrees;
    if deviation < 0.0 || deviation.is_nan() || deviation > 90.0 {
        return Err(read_error(
            ReadCode::Geometry,
            "sform",
            Sentinel::InvalidTransform,
            None,
        ));
    }
    Ok(options)
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Scaling {
    pub slope: f64,
    pub intercept: f64,
    pub applied: bool,
}
fn header_scaling(header: &Header) -> Scaling {
    if !finite(header.scl_slope) || header.scl_slope == 0.0 {
        return Scaling {
            slope: 1.0,
            intercept: 0.0,
            applied: false,
        };
    }
    let intercept = if finite(header.scl_inter) {
        header.scl_inter
    } else {
        0.0
    };
    Scaling {
        slope: header.scl_slope,
        intercept,
        applied: header.scl_slope != 1.0 || intercept != 0.0,
    }
}
pub(crate) const PAYLOAD_CHUNK_BYTES: usize = 8 << 20;
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Volume {
    pub header: Header,
    pub geometry: Geometry,
    pub dimensions: [i64; 4],
    pub scaling: Scaling,
    pub warnings: Vec<String>,
    bytes_per_voxel: usize,
    expected: usize,
    data: Vec<u8>,
}
pub(crate) fn datatype_bytes(datatype: i16) -> Option<usize> {
    match datatype {
        DATATYPE_UINT8 | DATATYPE_INT8 => Some(1),
        DATATYPE_INT16 | DATATYPE_UINT16 => Some(2),
        DATATYPE_INT32 | DATATYPE_UINT32 | DATATYPE_FLOAT32 => Some(4),
        DATATYPE_FLOAT64 => Some(8),
        _ => None,
    }
}
impl Volume {
    pub fn len(&self) -> usize {
        self.expected / self.bytes_per_voxel
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn index(&self, i: i64, j: i64, k: i64, t: i64) -> i64 {
        let d = &self.dimensions;
        i + d[0] * (j + d[1] * (k + d[2] * t))
    }
    pub fn bytes_per_voxel(&self) -> usize {
        self.bytes_per_voxel
    }
    pub fn payload(&self) -> &[u8] {
        &self.data
    }
    pub fn stored(&self, index: usize) -> f64 {
        let offset = index * self.bytes_per_voxel;
        let raw = &self.data[offset..offset + self.bytes_per_voxel];
        match self.header.datatype {
            DATATYPE_UINT8 => raw[0] as f64,
            DATATYPE_INT8 => raw[0] as i8 as f64,
            DATATYPE_INT16 => i16::from_le_bytes([raw[0], raw[1]]) as f64,
            DATATYPE_UINT16 => u16::from_le_bytes([raw[0], raw[1]]) as f64,
            DATATYPE_INT32 => i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64,
            DATATYPE_UINT32 => u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64,
            DATATYPE_FLOAT32 => f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64,
            _ => f64::from_le_bytes(raw.try_into().expect("8 bytes")),
        }
    }
    pub fn value(&self, index: usize) -> f64 {
        let stored = self.stored(index);
        if !self.scaling.applied {
            return stored;
        }
        stored * self.scaling.slope + self.scaling.intercept
    }
    pub fn slice_values(&self, k: i64, t: i64) -> Vec<f64> {
        let start = self.index(0, 0, k, t);
        let count = self.dimensions[0] * self.dimensions[1];
        (start..start + count)
            .map(|index| self.value(index as usize))
            .collect()
    }
    pub fn slice_geometries(&self) -> Vec<SliceGeometry> {
        (0..self.dimensions[2])
            .map(|k| {
                self.geometry
                    .slice_geometry(k, self.dimensions[0], self.dimensions[1])
            })
            .collect()
    }
    pub fn volume_geometry(&self, tolerances: GeometryTolerances) -> VolumeGeometry {
        build_volume_geometry(&self.slice_geometries(), tolerances)
    }
    pub fn info(&self) -> Info {
        Info {
            header: self.header,
            geometry: self.geometry.clone(),
            dimensions: self.dimensions,
            scaling: self.scaling,
            payload_bytes: self.expected as u64,
            warnings: self.warnings.clone(),
        }
    }
    fn read_payload(&mut self, stream: &mut ByteStream, probe: bool) -> Result<(), Error> {
        let mut data = Vec::new();
        let mut remaining = self.expected;
        while remaining > 0 {
            let chunk = remaining.min(PAYLOAD_CHUNK_BYTES);
            match stream.read_full(chunk) {
                Ok(bytes) if data.is_empty() => data = bytes,
                Ok(bytes) => data.extend_from_slice(&bytes),
                Err(terminal) => return Err(io_read_error("data", &terminal)),
            }
            remaining -= chunk;
        }
        self.data = data;
        if self.header.big_endian {
            swap_to_little_endian(&mut self.data, self.bytes_per_voxel);
        }
        if !probe {
            return Ok(());
        }
        match stream.read_full(1) {
            Ok(_) => self
                .warnings
                .push(geometry::WARNING_TRAILING_DATA.to_string()),
            Err(Terminal::Eof) => {}
            Err(terminal) => {
                self.data = Vec::new();
                return Err(io_read_error("data", &terminal));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub header: Header,
    pub geometry: Geometry,
    pub dimensions: [i64; 4],
    pub scaling: Scaling,
    pub payload_bytes: u64,
    pub warnings: Vec<String>,
}
fn io_read_error(field: &'static str, terminal: &Terminal) -> Error {
    if terminal.is_eof_kind() {
        return read_error(ReadCode::Truncated, field, Sentinel::TruncatedData, None);
    }
    read_error(
        ReadCode::Io,
        field,
        Sentinel::InvalidSource,
        Some(terminal.text().to_string()),
    )
}
fn swap_to_little_endian(data: &mut [u8], width: usize) {
    if width == 1 {
        return;
    }
    for voxel in data.chunks_exact_mut(width) {
        voxel.reverse();
    }
}
fn open_maybe_gzip(data: &[u8]) -> Result<ByteStream, Error> {
    if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        return gzip::gunzip(data).map_err(|terminal| {
            read_error(
                ReadCode::Format,
                "gzip",
                Sentinel::NotNifti,
                Some(terminal.text().to_string()),
            )
        });
    }
    Ok(ByteStream::plain(data))
}
fn read_header_bytes(stream: &mut ByteStream) -> Result<(Header, usize), Error> {
    fn failed(terminal: &Terminal) -> Error {
        if terminal.is_eof_kind() {
            return read_error(ReadCode::Truncated, "header", Sentinel::TruncatedData, None);
        }
        read_error(
            ReadCode::Io,
            "header",
            Sentinel::InvalidSource,
            Some(terminal.text().to_string()),
        )
    }
    let mut raw = match stream.read_full(NIFTI1_HEADER_SIZE) {
        Ok(bytes) => bytes,
        Err(terminal) => return Err(failed(&terminal)),
    };
    let size_le = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    let size_be = u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
    if size_le == NIFTI2_HEADER_SIZE || size_be == NIFTI2_HEADER_SIZE {
        match stream.read_full(NIFTI2_HEADER_SIZE - NIFTI1_HEADER_SIZE) {
            Ok(bytes) => raw.extend_from_slice(&bytes),
            Err(terminal) => return Err(failed(&terminal)),
        }
    }
    let header = parse_header(&raw)?;
    Ok((header, raw.len()))
}
fn prepare_volume(
    header: Header,
    options: &ReadOptions,
    paired: bool,
) -> Result<(Volume, i64), Error> {
    let mut volume = Volume {
        header,
        scaling: header_scaling(&header),
        ..Volume::default()
    };
    let Some(bytes_per_voxel) = datatype_bytes(header.datatype) else {
        return Err(datatype_error(header.datatype));
    };
    volume.bytes_per_voxel = bytes_per_voxel;
    if header.bit_pix != (8 * bytes_per_voxel) as i16 {
        volume
            .warnings
            .push(geometry::WARNING_BITPIX_MISMATCH.to_string());
    }
    if header.dim[5..].iter().any(|&dimension| dimension != 1) {
        return Err(read_error(
            ReadCode::Dimensions,
            "dim",
            Sentinel::UnsupportedDimensions,
            None,
        ));
    }
    let mut voxels = 1u64;
    for index in 0..4 {
        let dimension = header.dim[index + 1];
        volume.dimensions[index] = dimension;
        voxels = match voxels.checked_mul(dimension as u64) {
            Some(product) if product <= options.limits.max_voxels => product,
            _ => {
                return Err(read_error(
                    ReadCode::Limit,
                    "dim",
                    Sentinel::LimitExceeded,
                    None,
                ));
            }
        };
    }
    let payload = match voxels.checked_mul(bytes_per_voxel as u64) {
        Some(payload)
            if payload <= options.limits.max_payload_bytes && payload <= i64::MAX as u64 =>
        {
            payload
        }
        _ => {
            return Err(read_error(
                ReadCode::Limit,
                "payload",
                Sentinel::LimitExceeded,
                None,
            ));
        }
    };
    let offset = header.voxel_offset;
    let max_header = options.limits.max_header_bytes as f64;
    if !finite(offset) || offset < 0.0 || offset != offset.trunc() || offset > max_header {
        if finite(offset) && offset > max_header {
            return Err(read_error(
                ReadCode::Limit,
                "vox_offset",
                Sentinel::LimitExceeded,
                None,
            ));
        }
        return Err(read_error(
            ReadCode::Header,
            "vox_offset",
            Sentinel::MalformedHeader,
            None,
        ));
    }
    let mut data_offset = offset as i64;
    if !paired && data_offset < header.size() as i64 {
        data_offset = header.size() as i64 + 4;
        volume
            .warnings
            .push(geometry::WARNING_VOXEL_OFFSET_ADJUSTED.to_string());
    }
    let resolved = geometry::resolve_geometry(&header, options)?;
    volume.warnings.extend(resolved.warnings.iter().cloned());
    volume.geometry = resolved;
    volume.expected = payload as usize;
    Ok((volume, data_offset))
}
pub fn read(data: &[u8], options: &ReadOptions) -> Result<Volume, Error> {
    let options = normalized_read_options(*options)?;
    let mut stream = open_maybe_gzip(data)?;
    let (header, raw_len) = read_header_bytes(&mut stream)?;
    if header.paired {
        return Err(read_error(
            ReadCode::Format,
            "magic",
            Sentinel::PairedFile,
            None,
        ));
    }
    let (mut volume, data_offset) = prepare_volume(header, &options, false)?;
    let mut consumed = raw_len as i64;
    if data_offset >= consumed + 4 {
        match stream.read_full(4) {
            Ok(extender) => volume.header.extended = extender[0] != 0,
            Err(terminal) => return Err(io_read_error("extension", &terminal)),
        }
        consumed += 4;
    }
    stream
        .discard((data_offset - consumed) as u64)
        .map_err(|terminal| io_read_error("vox_offset", &terminal))?;
    volume.read_payload(&mut stream, true)?;
    Ok(volume)
}
pub fn read_pair(
    header_data: &[u8],
    image_data: &[u8],
    options: &ReadOptions,
) -> Result<Volume, Error> {
    let options = normalized_read_options(*options)?;
    let mut header_stream = open_maybe_gzip(header_data)?;
    let (header, _) = read_header_bytes(&mut header_stream)?;
    if !header.paired {
        return Err(read_error(
            ReadCode::Format,
            "magic",
            Sentinel::NotNifti,
            None,
        ));
    }
    let (mut volume, data_offset) = prepare_volume(header, &options, true)?;
    let mut image_stream = open_maybe_gzip(image_data)?;
    image_stream
        .discard(data_offset as u64)
        .map_err(|terminal| io_read_error("vox_offset", &terminal))?;
    volume.read_payload(&mut image_stream, true)?;
    Ok(volume)
}
fn open_error(cause: &std::io::Error) -> Error {
    read_error(
        ReadCode::Io,
        "open",
        Sentinel::InvalidSource,
        Some(cause.to_string()),
    )
}
fn open_companion(base: &str, extension: &str) -> Result<Vec<u8>, Error> {
    let upper = extension.to_ascii_uppercase();
    for candidate in [
        extension.to_string(),
        format!("{extension}.gz"),
        upper.clone(),
        format!("{upper}.GZ"),
    ] {
        match std::fs::read(format!("{base}{candidate}")) {
            Ok(data) => return Ok(data),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(open_error(&err)),
        }
    }
    Err(read_error(
        ReadCode::Io,
        "open",
        Sentinel::InvalidSource,
        Some("file does not exist".to_string()),
    ))
}
pub fn read_file(
    path: impl AsRef<std::path::Path>,
    options: &ReadOptions,
) -> Result<Volume, Error> {
    let path = path.as_ref().to_string_lossy();
    let path: &str = &path;
    let lower = path.to_lowercase();
    for suffix in [".hdr.gz", ".img.gz", ".hdr", ".img"] {
        if lower.ends_with(suffix) && path.len() >= suffix.len() {
            let base = &path[..path.len() - suffix.len()];
            let header = open_companion(base, ".hdr")?;
            let image = open_companion(base, ".img")?;
            return read_pair(&header, &image, options);
        }
    }
    let data = std::fs::read(path).map_err(|err| open_error(&err))?;
    read(&data, options)
}
pub fn inspect(data: &[u8], options: &ReadOptions) -> Result<Info, Error> {
    let options = normalized_read_options(*options)?;
    let mut stream = open_maybe_gzip(data)?;
    let (header, _) = read_header_bytes(&mut stream)?;
    if header.paired {
        return Err(read_error(
            ReadCode::Format,
            "magic",
            Sentinel::PairedFile,
            None,
        ));
    }
    let (volume, _) = prepare_volume(header, &options, false)?;
    Ok(volume.info())
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NewVolumeOptions {
    pub datatype: i16,
    pub dimensions: [i64; 4],
    pub index_to_lps: GeometryAffine,
    pub big_endian: bool,
    pub limits: ReadLimits,
}
pub fn new_volume(stream: &mut ByteStream, options: &NewVolumeOptions) -> Result<Volume, Error> {
    new_volume_from(stream, options, true)
}
pub fn new_volume_exact(
    stream: &mut ByteStream,
    options: &NewVolumeOptions,
) -> Result<Volume, Error> {
    new_volume_from(stream, options, false)
}
fn new_volume_from(
    stream: &mut ByteStream,
    options: &NewVolumeOptions,
    probe: bool,
) -> Result<Volume, Error> {
    let limits = fill_limits(options.limits);
    let Some(bytes_per_voxel) = datatype_bytes(options.datatype) else {
        return Err(datatype_error(options.datatype));
    };
    let mut header = Header {
        version: 1,
        big_endian: options.big_endian,
        datatype: options.datatype,
        bit_pix: (8 * bytes_per_voxel) as i16,
        scl_slope: 1.0,
        sform_code: XformCode::SCANNER_ANAT,
        xyzt_units: header::SPATIAL_UNIT_MILLIMETER,
        voxel_offset: (NIFTI1_HEADER_SIZE + 4) as f64,
        ..Header::default()
    };
    let mut dimensions = 0;
    for (index, &size) in options.dimensions.iter().enumerate() {
        let size = if size == 0 { 1 } else { size };
        if !(0..=i32::MAX as i64).contains(&size) {
            return Err(read_error(
                ReadCode::Dimensions,
                "dim",
                Sentinel::UnsupportedDimensions,
                None,
            ));
        }
        header.dim[index + 1] = size;
        if size > 1 || index < 3 {
            dimensions = index + 1;
        }
    }
    for index in 5..8 {
        header.dim[index] = 1;
    }
    header.dim[0] = dimensions as i64;
    let ras = header::affine_lps2ras(options.index_to_lps).0;
    header.pix_dim[0] = 1.0;
    for column in 0..3 {
        header.pix_dim[column + 1] = (ras[column] * ras[column]
            + ras[4 + column] * ras[4 + column]
            + ras[8 + column] * ras[8 + column])
            .sqrt();
    }
    for row in 0..3 {
        for column in 0..4 {
            header.s_row[row][column] = ras[4 * row + column];
        }
    }
    let read_options = ReadOptions {
        limits,
        ..ReadOptions::default()
    };
    let (mut volume, _) = prepare_volume(header, &read_options, true)?;
    volume.read_payload(stream, probe)?;
    Ok(volume)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    #[default]
    None,
    Gzip,
}
pub fn write_volume(v: &Volume, compression: Compression) -> Result<Vec<u8>, Error> {
    if v.bytes_per_voxel == 0 {
        return Err(Error::Sentinel(Sentinel::InvalidSource));
    }
    let mut spacing = [
        v.geometry.spacing[0],
        v.geometry.spacing[1],
        v.geometry.spacing[2],
        1.0,
    ];
    let (mut time_offset, mut temporal_unit) = (0.0, 0u8);
    if v.dimensions[3] > 1 {
        (time_offset, temporal_unit) = (v.header.t_offset, v.header.temporal_unit());
        if v.header.pix_dim[4] > 0.0 {
            spacing[3] = v.header.pix_dim[4];
        }
    }
    let (mut slope, mut intercept) = (1.0, 0.0);
    if v.scaling.applied {
        (slope, intercept) = (v.scaling.slope, v.scaling.intercept);
    }
    let mut affine = v.geometry.index_to_ras;
    let no_transform = v.geometry.source == XformSource::PixDim;
    if no_transform {
        affine = GeometryAffine([
            spacing[0], 0.0, 0.0, 0.0, 0.0, spacing[1], 0.0, 0.0, 0.0, 0.0, spacing[2], 0.0, 0.0,
            0.0, 0.0, 1.0,
        ]);
    }
    let header = header::build_header(&header::HeaderSpec {
        dimensions: v.dimensions,
        datatype: v.header.datatype,
        bit_pix: (8 * v.bytes_per_voxel) as i16,
        spacing,
        voxel_offset: 352,
        slope,
        intercept,
        time_offset,
        temporal_unit,
        affine_ras: affine,
        no_spatial_transform: no_transform,
    })?;
    if compression == Compression::Gzip {
        let mut zipper = gzip::DeflateWriter::new(gzip::Container::Gzip);
        zipper.write(&header);
        zipper.write(&[0, 0, 0, 0]);
        for chunk in v.data.chunks(PAYLOAD_CHUNK_BYTES) {
            zipper.write(chunk);
        }
        return Ok(zipper.finish());
    }
    let mut out = Vec::with_capacity(header.len() + 4 + v.data.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&v.data);
    Ok(out)
}
