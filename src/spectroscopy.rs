#![forbid(unsafe_code)]
use crate::object::{self, Element, Object, Tag, UNDEFINED_LENGTH, Value, Vr};
use std::fmt;
const TAG_DATA: Tag = Tag::new(0x5600, 0x0020);
const TAG_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_VOXEL_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_VOXEL_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_POINT_ROWS: Tag = Tag::new(0x0028, 0x9001);
const TAG_POINT_COLUMNS: Tag = Tag::new(0x0028, 0x9002);
const TAG_REPRESENTATION: Tag = Tag::new(0x0028, 0x9108);
const TAG_DOMAIN_COLUMNS: Tag = Tag::new(0x0028, 0x9003);
const TAG_DOMAIN_ROWS: Tag = Tag::new(0x0028, 0x9235);
const TAG_SPECTRAL_WIDTH: Tag = Tag::new(0x0018, 0x9052);
const TAG_TRANSMITTER: Tag = Tag::new(0x0018, 0x9098);
const TAG_CHEMICAL_SHIFT: Tag = Tag::new(0x0018, 0x9053);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidMetadata,
    InvalidData,
    DataLength,
    LimitExceeded,
    InvalidOptions,
    Index,
    AxisUnavailable,
    Copy,
}
impl ErrorKind {
    fn text(self) -> &'static str {
        match self {
            ErrorKind::InvalidMetadata => "dicom/spectroscopy: invalid metadata",
            ErrorKind::InvalidData => "dicom/spectroscopy: invalid spectroscopy data",
            ErrorKind::DataLength => "dicom/spectroscopy: spectroscopy data length mismatch",
            ErrorKind::LimitExceeded => "dicom/spectroscopy: resource limit exceeded",
            ErrorKind::InvalidOptions => "dicom/spectroscopy: invalid options",
            ErrorKind::Index => "dicom/spectroscopy: index out of range",
            ErrorKind::AxisUnavailable => "dicom/spectroscopy: axis calibration unavailable",
            ErrorKind::Copy => "dicom/spectroscopy: copy OF",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    message: String,
}
impl Error {
    fn of(kind: ErrorKind) -> Error {
        Error {
            kind,
            message: kind.text().to_string(),
        }
    }
    fn detail(kind: ErrorKind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            message: format!("{}: {detail}", kind.text()),
        }
    }
    fn metadata(tag: Tag) -> Error {
        Error::detail(ErrorKind::InvalidMetadata, format_args!("attribute {tag}"))
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Representation(pub String);
impl Representation {
    pub const COMPLEX: &'static str = "COMPLEX";
    pub const REAL: &'static str = "REAL";
    pub const IMAGINARY: &'static str = "IMAGINARY";
    pub const MAGNITUDE: &'static str = "MAGNITUDE";
    fn components(&self) -> Result<i64, Error> {
        match self.0.as_str() {
            Self::COMPLEX => Ok(2),
            Self::REAL | Self::IMAGINARY | Self::MAGNITUDE => Ok(1),
            _ => Err(Error::detail(
                ErrorKind::InvalidMetadata,
                "unsupported data representation",
            )),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Domain(pub String);
impl Domain {
    pub const TIME: &'static str = "TIME";
    pub const FREQUENCY: &'static str = "FREQUENCY";
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Dimensions {
    pub frames: i64,
    pub voxel_rows: i64,
    pub voxel_columns: i64,
    pub point_rows: i64,
    pub point_columns: i64,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Axis {
    pub points: i64,
    pub domain: Domain,
    pub spectral_width_hz: Option<f64>,
    pub transmitter_frequency_mhz: Option<f64>,
    pub chemical_shift_reference_ppm: Option<f64>,
}
fn positive_finite(value: f64) -> bool {
    value > 0.0 && value.is_finite()
}
fn finite_coordinate(value: f64) -> Result<f64, Error> {
    if !value.is_finite() {
        return Err(Error::of(ErrorKind::AxisUnavailable));
    }
    Ok(value)
}
impl Axis {
    pub fn frequency_offset_hz(&self, index: i64, reference_index: f64) -> Result<f64, Error> {
        if self.domain.0 != Domain::FREQUENCY {
            return Err(Error::of(ErrorKind::AxisUnavailable));
        }
        let step = self.step(index, reference_index)?;
        finite_coordinate((reference_index - index as f64) / self.points as f64 * step)
    }
    pub fn ppm(&self, index: i64, reference_index: f64) -> Result<f64, Error> {
        let offset = self.frequency_offset_hz(index, reference_index)?;
        match (
            self.transmitter_frequency_mhz,
            self.chemical_shift_reference_ppm,
        ) {
            (Some(transmitter), Some(reference))
                if positive_finite(transmitter) && reference.is_finite() =>
            {
                finite_coordinate(reference + offset / transmitter)
            }
            _ => Err(Error::of(ErrorKind::AxisUnavailable)),
        }
    }
    pub fn time_offset_seconds(&self, index: i64, reference_index: f64) -> Result<f64, Error> {
        if self.domain.0 != Domain::TIME {
            return Err(Error::of(ErrorKind::AxisUnavailable));
        }
        let width = self.step(index, reference_index)?;
        finite_coordinate((index as f64 - reference_index) / width)
    }
    fn step(&self, index: i64, reference_index: f64) -> Result<f64, Error> {
        if self.points <= 0 || index < 0 || index >= self.points {
            return Err(Error::of(ErrorKind::Index));
        }
        match self.spectral_width_hz {
            Some(width) if reference_index.is_finite() && positive_finite(width) => Ok(width),
            _ => Err(Error::of(ErrorKind::AxisUnavailable)),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Data {
    pub dimensions: Dimensions,
    pub representation: Representation,
    pub column_axis: Axis,
    pub row_axis: Option<Axis>,
    pub values: Vec<f32>,
}
impl Data {
    fn spectrum_range(
        &self,
        frame: i64,
        voxel_row: i64,
        voxel_column: i64,
    ) -> Result<(usize, usize, i64), Error> {
        let components = self.valid_layout()?;
        let shape = self.dimensions;
        if frame < 0
            || frame >= shape.frames
            || voxel_row < 0
            || voxel_row >= shape.voxel_rows
            || voxel_column < 0
            || voxel_column >= shape.voxel_columns
        {
            return Err(Error::of(ErrorKind::Index));
        }
        let count = shape.point_rows * shape.point_columns * components;
        let first =
            ((frame * shape.voxel_rows + voxel_row) * shape.voxel_columns + voxel_column) * count;
        Ok((first as usize, count as usize, components))
    }
    pub fn spectrum(&self, frame: i64, voxel_row: i64, voxel_column: i64) -> Result<&[f32], Error> {
        let (first, count, _) = self.spectrum_range(frame, voxel_row, voxel_column)?;
        Ok(&self.values[first..first + count])
    }
    #[doc(hidden)]
    pub fn spectrum_mut(
        &mut self,
        frame: i64,
        voxel_row: i64,
        voxel_column: i64,
    ) -> Result<&mut [f32], Error> {
        let (first, count, _) = self.spectrum_range(frame, voxel_row, voxel_column)?;
        Ok(&mut self.values[first..first + count])
    }
    pub fn point(
        &self,
        frame: i64,
        voxel_row: i64,
        voxel_column: i64,
        point_row: i64,
        point_column: i64,
    ) -> Result<&[f32], Error> {
        let (first, _, components) = self.spectrum_range(frame, voxel_row, voxel_column)?;
        let shape = self.dimensions;
        if point_row < 0
            || point_row >= shape.point_rows
            || point_column < 0
            || point_column >= shape.point_columns
        {
            return Err(Error::of(ErrorKind::Index));
        }
        let at = first + ((point_row * shape.point_columns + point_column) * components) as usize;
        Ok(&self.values[at..at + components as usize])
    }
    fn valid_layout(&self) -> Result<i64, Error> {
        let components = self.representation.components()?;
        let count = value_count(self.dimensions, components)?;
        if count != self.values.len() as i64 {
            return Err(Error::of(ErrorKind::DataLength));
        }
        Ok(components)
    }
}
pub const DEFAULT_MAX_BYTES: i64 = 64 << 20;
pub const DEFAULT_MAX_VALUES: i64 = DEFAULT_MAX_BYTES / 4;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    pub max_bytes: i64,
    pub max_values: i64,
}
pub fn read(obj: Option<&Object>, opts: Options) -> Result<Data, Error> {
    let mut opts = opts;
    if opts.max_bytes < 0 || opts.max_values < 0 {
        return Err(Error::of(ErrorKind::InvalidOptions));
    }
    if opts.max_bytes == 0 {
        opts.max_bytes = DEFAULT_MAX_BYTES;
    }
    if opts.max_values == 0 {
        opts.max_values = DEFAULT_MAX_VALUES;
    }
    let Some(obj) = obj else {
        return Err(Error::of(ErrorKind::InvalidMetadata));
    };
    let mut data = read_metadata(obj)?;
    let components = data.representation.components()?;
    let count = value_count(data.dimensions, components)?;
    let bytes = count * 4;
    if count > opts.max_values || bytes > opts.max_bytes {
        return Err(Error::of(ErrorKind::LimitExceeded));
    }
    let element = match obj.get(TAG_DATA) {
        Some(element) if element.vr() == Vr::OF => element,
        _ => {
            return Err(Error::detail(
                ErrorKind::InvalidData,
                "(5600,0020) must be OF",
            ));
        }
    };
    match &element.value {
        Value::Nil => {}
        Value::Raw(raw) => {
            if raw.len() as i64 != bytes {
                return Err(Error::of(ErrorKind::DataLength));
            }
        }
        _ => {
            return Err(Error::detail(
                ErrorKind::InvalidData,
                "unsupported OF value kind",
            ));
        }
    }
    let length = object::element_length(element);
    if length == UNDEFINED_LENGTH || i64::from(length) != bytes {
        return Err(Error::of(ErrorKind::DataLength));
    }
    let mut raw = Vec::new();
    let (copied, err) = obj.copy_value_to(TAG_DATA, &mut raw);
    if let Some(err) = err {
        return Err(Error::detail(ErrorKind::Copy, err));
    }
    if copied != bytes || raw.len() as i64 != bytes {
        return Err(Error::of(ErrorKind::DataLength));
    }
    let big = obj.value_big_endian();
    data.values = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| {
            let word = [b[0], b[1], b[2], b[3]];
            f32::from_bits(if big {
                u32::from_be_bytes(word)
            } else {
                u32::from_le_bytes(word)
            })
        })
        .collect();
    Ok(data)
}
fn read_metadata(obj: &Object) -> Result<Data, Error> {
    let fields = [
        (TAG_FRAMES, Vr::IS, 12),
        (TAG_VOXEL_ROWS, Vr::US, 2),
        (TAG_VOXEL_COLUMNS, Vr::US, 2),
        (TAG_POINT_ROWS, Vr::UL, 4),
        (TAG_POINT_COLUMNS, Vr::UL, 4),
    ];
    let mut dims = [0i64; 5];
    for (index, (tag, vr, limit)) in fields.into_iter().enumerate() {
        check_metadata_element(obj, tag, vr, limit)?;
        match obj.get_ints(tag) {
            Ok(values) if values.len() == 1 && values[0] > 0 => dims[index] = values[0],
            _ => return Err(Error::metadata(tag)),
        }
    }
    let mut data = Data {
        dimensions: Dimensions {
            frames: dims[0],
            voxel_rows: dims[1],
            voxel_columns: dims[2],
            point_rows: dims[3],
            point_columns: dims[4],
        },
        ..Data::default()
    };
    data.representation = Representation(code_value(obj, TAG_REPRESENTATION)?);
    data.representation.components()?;
    let column_domain = signal_domain(obj, TAG_DOMAIN_COLUMNS)?;
    data.column_axis = Axis {
        points: data.dimensions.point_columns,
        domain: column_domain,
        ..Axis::default()
    };
    if data.dimensions.point_rows > 1 || obj.has(TAG_DOMAIN_ROWS) {
        let row_domain = signal_domain(obj, TAG_DOMAIN_ROWS)?;
        data.row_axis = Some(Axis {
            points: data.dimensions.point_rows,
            domain: row_domain,
            ..Axis::default()
        });
    }
    let rank: usize = if data.dimensions.point_rows > 1 { 2 } else { 1 };
    for tag in [TAG_SPECTRAL_WIDTH, TAG_TRANSMITTER, TAG_CHEMICAL_SHIFT] {
        if !obj.has(tag) {
            continue;
        }
        check_metadata_element(obj, tag, Vr::FD, rank as u64 * 8)?;
        let values = match obj.get_floats(tag) {
            Ok(values) if !values.is_empty() && values.len() <= rank => values,
            _ => return Err(Error::metadata(tag)),
        };
        for (index, value) in values.into_iter().enumerate() {
            if !value.is_finite() || tag != TAG_CHEMICAL_SHIFT && value <= 0.0 {
                return Err(Error::metadata(tag));
            }
            let axis = if index == 1 {
                match data.row_axis.as_mut() {
                    Some(axis) => axis,
                    None => return Err(Error::metadata(tag)),
                }
            } else {
                &mut data.column_axis
            };
            if tag == TAG_SPECTRAL_WIDTH {
                axis.spectral_width_hz = Some(value);
            } else if tag == TAG_TRANSMITTER {
                axis.transmitter_frequency_mhz = Some(value);
            } else {
                axis.chemical_shift_reference_ppm = Some(value);
            }
        }
    }
    Ok(data)
}
fn check_metadata_element(obj: &Object, tag: Tag, vr: Vr, maximum_bytes: u64) -> Result<(), Error> {
    let element = match obj.get(tag) {
        Some(element) if element.vr() == vr => element,
        _ => return Err(Error::metadata(tag)),
    };
    if !metadata_value_within_bounds(element, vr, maximum_bytes) {
        return Err(Error::metadata(tag));
    }
    let length = object::element_length(element);
    if length == UNDEFINED_LENGTH || length == 0 || u64::from(length) > maximum_bytes {
        return Err(Error::metadata(tag));
    }
    let calculated = match &element.value {
        Value::Nil => None,
        Value::Raw(raw) => Some(Some(raw.len() as u64)),
        Value::Strings(values) => Some(Some(strings_encoded_length(values))),
        _ => Some(None),
    };
    if let Some(calculated) = calculated
        && calculated != Some(u64::from(length))
    {
        return Err(Error::metadata(tag));
    }
    let width = match vr {
        Vr::US => 2,
        Vr::UL => 4,
        Vr::FD => 8,
        _ => 1,
    };
    if u64::from(length) % width != 0 {
        return Err(Error::metadata(tag));
    }
    Ok(())
}
fn strings_encoded_length(values: &[String]) -> u64 {
    let mut total: usize = values.iter().map(String::len).sum();
    if !values.is_empty() {
        total += values.len() - 1;
    }
    (total + total % 2) as u64
}
fn metadata_value_within_bounds(element: &Element, vr: Vr, maximum: u64) -> bool {
    match &element.value {
        Value::Nil => true,
        Value::Raw(raw) => raw.len() as u64 <= maximum,
        Value::Strings(values) => {
            matches!(vr, Vr::IS | Vr::CS) && values.len() == 1 && values[0].len() as u64 <= maximum
        }
        _ => false,
    }
}
fn code_value(obj: &Object, tag: Tag) -> Result<String, Error> {
    check_metadata_element(obj, tag, Vr::CS, 16)?;
    match obj.lookup_strings(tag) {
        Ok(mut values) if values.len() == 1 && !values[0].is_empty() => Ok(values.remove(0)),
        _ => Err(Error::metadata(tag)),
    }
}
fn signal_domain(obj: &Object, tag: Tag) -> Result<Domain, Error> {
    let value = code_value(obj, tag)?;
    if value != Domain::TIME && value != Domain::FREQUENCY {
        return Err(Error::metadata(tag));
    }
    Ok(Domain(value))
}
fn value_count(shape: Dimensions, components: i64) -> Result<i64, Error> {
    let mut count: i64 = 1;
    for dim in [
        shape.frames,
        shape.voxel_rows,
        shape.voxel_columns,
        shape.point_rows,
        shape.point_columns,
        components,
        4,
    ] {
        if dim <= 0 {
            return Err(Error::of(ErrorKind::InvalidMetadata));
        }
        if count > i64::MAX / dim {
            return Err(Error::of(ErrorKind::LimitExceeded));
        }
        count *= dim;
    }
    Ok(count / 4)
}
