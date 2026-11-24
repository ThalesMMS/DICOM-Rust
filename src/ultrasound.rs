#![forbid(unsafe_code)]
use crate::derivedio;
use crate::fmath;
use crate::object::{Object, Tag, Vr};
use crate::sr;
use crate::textfmt::quote;
use std::fmt;
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_SHARED_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9229);
const TAG_PER_FRAME_FUNCTIONAL_GROUPS: Tag = Tag::new(0x5200, 0x9230);
const TAG_US_REGIONS: Tag = Tag::new(0x0018, 0x6011);
const TAG_SPATIAL_FORMAT: Tag = Tag::new(0x0018, 0x6012);
const TAG_DATA_TYPE: Tag = Tag::new(0x0018, 0x6014);
const TAG_FLAGS: Tag = Tag::new(0x0018, 0x6016);
const TAG_MIN_X: Tag = Tag::new(0x0018, 0x6018);
const TAG_MIN_Y: Tag = Tag::new(0x0018, 0x601A);
const TAG_MAX_X: Tag = Tag::new(0x0018, 0x601C);
const TAG_MAX_Y: Tag = Tag::new(0x0018, 0x601E);
const TAG_REFERENCE_X: Tag = Tag::new(0x0018, 0x6020);
const TAG_REFERENCE_Y: Tag = Tag::new(0x0018, 0x6022);
const TAG_UNITS_X: Tag = Tag::new(0x0018, 0x6024);
const TAG_UNITS_Y: Tag = Tag::new(0x0018, 0x6026);
const TAG_REFERENCE_VALUE_X: Tag = Tag::new(0x0018, 0x6028);
const TAG_REFERENCE_VALUE_Y: Tag = Tag::new(0x0018, 0x602A);
const TAG_DELTA_X: Tag = Tag::new(0x0018, 0x602C);
const TAG_DELTA_Y: Tag = Tag::new(0x0018, 0x602E);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Uncalibrated,
    CrossRegion,
    AmbiguousRegion,
    UnsupportedUnits,
    InvalidCalibration,
    Other,
}
impl ErrorKind {
    fn text(self) -> &'static str {
        match self {
            ErrorKind::Uncalibrated => "dicom/ultrasound: no compatible calibration",
            ErrorKind::CrossRegion => "dicom/ultrasound: measurement crosses incompatible regions",
            ErrorKind::AmbiguousRegion => {
                "dicom/ultrasound: overlapping calibrations are ambiguous"
            }
            ErrorKind::UnsupportedUnits => "dicom/ultrasound: unsupported physical units",
            ErrorKind::InvalidCalibration => "dicom/ultrasound: invalid region calibration",
            ErrorKind::Other => "",
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
    fn other(message: impl Into<String>) -> Error {
        Error {
            kind: ErrorKind::Other,
            message: message.into(),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i64,
    pub y: i64,
}
impl Point {
    pub const fn new(x: i64, y: i64) -> Point {
        Point { x, y }
    }
}
impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({},{})", self.x, self.y)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rectangle {
    pub min: Point,
    pub max: Point,
}
pub type SpatialFormat = u16;
pub const SPATIAL_NONE: SpatialFormat = 0;
pub const SPATIAL_2D: SpatialFormat = 1;
pub const SPATIAL_M_MODE: SpatialFormat = 2;
pub const SPATIAL_SPECTRAL: SpatialFormat = 3;
pub const SPATIAL_WAVEFORM: SpatialFormat = 4;
pub const SPATIAL_GRAPHICS: SpatialFormat = 5;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhysicalUnit(pub u16);
impl PhysicalUnit {
    pub const NONE: PhysicalUnit = PhysicalUnit(0);
    pub const PERCENT: PhysicalUnit = PhysicalUnit(1);
    pub const DECIBEL: PhysicalUnit = PhysicalUnit(2);
    pub const CENTIMETER: PhysicalUnit = PhysicalUnit(3);
    pub const SECOND: PhysicalUnit = PhysicalUnit(4);
    #[doc(hidden)]
    pub const HERTZ: PhysicalUnit = PhysicalUnit(5);
    #[doc(hidden)]
    pub const DECIBEL_PER_SEC: PhysicalUnit = PhysicalUnit(6);
    pub const CENTIMETER_PER_SEC: PhysicalUnit = PhysicalUnit(7);
    pub const SQUARE_CENTIMETER: PhysicalUnit = PhysicalUnit(8);
    #[doc(hidden)]
    pub const SQUARE_CM_PER_SEC: PhysicalUnit = PhysicalUnit(9);
    pub const CUBIC_CENTIMETER: PhysicalUnit = PhysicalUnit(10);
    #[doc(hidden)]
    pub const CUBIC_CM_PER_SEC: PhysicalUnit = PhysicalUnit(11);
    pub const DEGREE: PhysicalUnit = PhysicalUnit(12);
    pub fn as_str(self) -> &'static str {
        match self.0 {
            1 => "%",
            2 => "dB",
            3 => "cm",
            4 => "s",
            5 => "Hz",
            6 => "dB/s",
            7 => "cm/s",
            8 => "cm2",
            9 => "cm2/s",
            10 => "cm3",
            11 => "cm3/s",
            12 => "deg",
            _ => "none",
        }
    }
}
impl fmt::Display for PhysicalUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Region {
    pub index: i64,
    pub bounds: Rectangle,
    pub spatial_format: SpatialFormat,
    pub data_type: u16,
    pub flags: u32,
    pub units_x: PhysicalUnit,
    pub units_y: PhysicalUnit,
    pub delta_x: f64,
    pub delta_y: f64,
    pub reference_pixel_x: i64,
    pub reference_pixel_y: i64,
    pub reference_value_x: f64,
    pub reference_value_y: f64,
}
impl Region {
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.bounds.min.x
            && point.x <= self.bounds.max.x
            && point.y >= self.bounds.min.y
            && point.y <= self.bounds.max.y
    }
    pub fn physical(&self, point: Point) -> (f64, f64) {
        let reference_x = self.bounds.min.x.wrapping_add(self.reference_pixel_x);
        let reference_y = self.bounds.min.y.wrapping_add(self.reference_pixel_y);
        (
            self.reference_value_x + point.x.wrapping_sub(reference_x) as f64 * self.delta_x,
            self.reference_value_y + point.y.wrapping_sub(reference_y) as f64 * self.delta_y,
        )
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameCalibration {
    pub frame_index: i64,
    pub regions: Vec<Region>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Calibration {
    pub frames: Vec<FrameCalibration>,
}
impl Calibration {
    pub fn frame(&self, index: i64) -> Option<&FrameCalibration> {
        usize::try_from(index).ok().and_then(|i| self.frames.get(i))
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegionReference {
    pub frame_number: i64,
    pub region_index: i64,
}
pub fn read(root: Option<&Object>) -> Result<Calibration, Error> {
    let Some(root) = root else {
        return Err(Error::other("dicom/ultrasound: nil dataset"));
    };
    let mut frame_count = derivedio::int(root, TAG_NUMBER_OF_FRAMES);
    let per_frame = derivedio::sequence(root, TAG_PER_FRAME_FUNCTIONAL_GROUPS);
    if frame_count <= 0 {
        frame_count = 1;
        if per_frame.len() > 1 {
            frame_count = per_frame.len() as i64;
        }
    }
    if frame_count > 1_000_000 {
        return Err(Error::other(format!(
            "dicom/ultrasound: unreasonable frame count {frame_count}"
        )));
    }
    if !per_frame.is_empty() && per_frame.len() as i64 != frame_count {
        return Err(Error::other(format!(
            "dicom/ultrasound: {} per-frame groups for {frame_count} frames",
            per_frame.len()
        )));
    }
    let legacy = read_region_items(&derivedio::sequence(root, TAG_US_REGIONS))
        .map_err(|e| Error::other(format!("dicom/ultrasound: top-level calibration: {e}")))?;
    let mut shared = Vec::new();
    if let Some(first) = derivedio::sequence(root, TAG_SHARED_FUNCTIONAL_GROUPS).first() {
        shared = read_region_items(&recursive_region_items(first, 0))
            .map_err(|e| Error::other(format!("dicom/ultrasound: shared calibration: {e}")))?;
    }
    let mut out = Calibration {
        frames: Vec::with_capacity(frame_count as usize),
    };
    for frame_index in 0..frame_count {
        let mut regions = if shared.is_empty() {
            legacy.clone()
        } else {
            shared.clone()
        };
        if let Some(group) = per_frame.get(frame_index as usize) {
            let items = recursive_region_items(group, 0);
            if !items.is_empty() {
                regions = read_region_items(&items).map_err(|e| {
                    Error::other(format!(
                        "dicom/ultrasound: frame {} calibration: {e}",
                        frame_index + 1
                    ))
                })?;
            }
        }
        out.frames.push(FrameCalibration {
            frame_index,
            regions,
        });
    }
    Ok(out)
}
fn recursive_region_items(obj: &Object, depth: i64) -> Vec<Object> {
    if depth > 4 {
        return Vec::new();
    }
    let items = derivedio::sequence(obj, TAG_US_REGIONS);
    if !items.is_empty() {
        return items;
    }
    for element in obj.elements() {
        if element.vr() != Vr::SQ {
            continue;
        }
        let Some(items) = obj.get_sequence(element.tag()) else {
            continue;
        };
        for item in &items {
            let found = recursive_region_items(item, depth + 1);
            if !found.is_empty() {
                return found;
            }
        }
    }
    Vec::new()
}
fn read_region_items(items: &[Object]) -> Result<Vec<Region>, String> {
    let mut out = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let required = (
            required_int(item, TAG_MIN_X),
            required_int(item, TAG_MIN_Y),
            required_int(item, TAG_MAX_X),
            required_int(item, TAG_MAX_Y),
            required_int(item, TAG_SPATIAL_FORMAT),
            required_ranged(item, TAG_DATA_TYPE, i64::from(u16::MAX)),
            required_ranged(item, TAG_FLAGS, i64::from(u32::MAX)),
            required_int(item, TAG_UNITS_X),
            required_int(item, TAG_UNITS_Y),
            required_float(item, TAG_DELTA_X),
            required_float(item, TAG_DELTA_Y),
        );
        let (
            Some(min_x),
            Some(min_y),
            Some(max_x),
            Some(max_y),
            Some(spatial),
            Some(data_type),
            Some(flags),
            Some(units_x),
            Some(units_y),
            Some(delta_x),
            Some(delta_y),
        ) = required
        else {
            return Err(format!(
                "region {index} is missing required calibration attributes"
            ));
        };
        if min_x < 0 || min_y < 0 || max_x < min_x || max_y < min_y {
            return Err(format!(
                "region {index} has invalid bounds ({min_x},{min_y})-({max_x},{max_y})"
            ));
        }
        if delta_x == 0.0 || delta_y == 0.0 {
            return Err(format!("region {index} has invalid physical deltas"));
        }
        let reference_x = required_int(item, TAG_REFERENCE_X).unwrap_or(0);
        let reference_y = required_int(item, TAG_REFERENCE_Y).unwrap_or(0);
        let reference_value_x = required_float(item, TAG_REFERENCE_VALUE_X).unwrap_or(0.0);
        let reference_value_y = required_float(item, TAG_REFERENCE_VALUE_Y).unwrap_or(0.0);
        out.push(Region {
            index: index as i64,
            bounds: Rectangle {
                min: Point::new(min_x, min_y),
                max: Point::new(max_x, max_y),
            },
            spatial_format: spatial as u16,
            data_type: data_type as u16,
            flags: flags as u32,
            units_x: PhysicalUnit(units_x as u16),
            units_y: PhysicalUnit(units_y as u16),
            delta_x,
            delta_y,
            reference_pixel_x: reference_x,
            reference_pixel_y: reference_y,
            reference_value_x,
            reference_value_y,
        });
    }
    Ok(out)
}
fn required_int(obj: &Object, tag: Tag) -> Option<i64> {
    match derivedio::lookup_ints(Some(obj), tag) {
        Ok(values) if values.len() == 1 => Some(values[0]),
        _ => None,
    }
}
fn required_ranged(obj: &Object, tag: Tag, max: i64) -> Option<i64> {
    required_int(obj, tag).filter(|v| (0..=max).contains(v))
}
fn required_float(obj: &Object, tag: Tag) -> Option<f64> {
    match derivedio::lookup_floats(Some(obj), tag) {
        Ok(values) if values.len() == 1 && values[0].is_finite() => Some(values[0]),
        _ => None,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityIssueCode {
    NoCalibration,
    UnsupportedSpatialFormat,
    UnsupportedUnits,
    FrequencyDoppler,
    AmbiguousCalibration,
    InvalidCalibration,
}
impl CapabilityIssueCode {
    pub fn as_str(self) -> &'static str {
        match self {
            CapabilityIssueCode::NoCalibration => "no-calibration",
            CapabilityIssueCode::UnsupportedSpatialFormat => "unsupported-spatial-format",
            CapabilityIssueCode::UnsupportedUnits => "unsupported-units",
            CapabilityIssueCode::FrequencyDoppler => "frequency-doppler",
            CapabilityIssueCode::AmbiguousCalibration => "ambiguous-calibration",
            CapabilityIssueCode::InvalidCalibration => "invalid-calibration",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityIssue {
    pub code: CapabilityIssueCode,
    pub count: i64,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameCapabilities {
    pub regions: i64,
    pub biometry_regions: i64,
    pub doppler_regions: i64,
    pub unsupported_regions: i64,
    pub issues: Vec<CapabilityIssue>,
}
pub fn inspect_frame_capabilities(frame: &FrameCalibration) -> FrameCapabilities {
    let mut out = FrameCapabilities {
        regions: frame.regions.len() as i64,
        ..FrameCapabilities::default()
    };
    if frame.regions.is_empty() {
        out.issues.push(CapabilityIssue {
            code: CapabilityIssueCode::NoCalibration,
            count: 1,
        });
        return out;
    }
    let (mut invalid, mut frequency, mut units, mut spatial) = (0i64, 0i64, 0i64, 0i64);
    let mut biometry = Vec::new();
    let mut doppler = Vec::new();
    for region in &frame.regions {
        if !valid_region_calibration(region) {
            invalid += 1;
            continue;
        }
        match region.spatial_format {
            SPATIAL_2D => {
                if region.units_x == PhysicalUnit::CENTIMETER
                    && region.units_y == PhysicalUnit::CENTIMETER
                {
                    biometry.push(*region);
                } else {
                    units += 1;
                }
            }
            SPATIAL_SPECTRAL | SPATIAL_WAVEFORM => {
                if region.flags & (1 << 2) != 0 {
                    frequency += 1;
                } else if region.units_x == PhysicalUnit::SECOND
                    && region.units_y == PhysicalUnit::CENTIMETER_PER_SEC
                {
                    doppler.push(*region);
                } else {
                    units += 1;
                }
            }
            _ => spatial += 1,
        }
    }
    out.biometry_regions = biometry.len() as i64;
    out.doppler_regions = doppler.len() as i64;
    out.unsupported_regions = invalid + frequency + units + spatial;
    let ambiguous =
        ambiguous_region_pairs(&biometry, false) + ambiguous_region_pairs(&doppler, true);
    for (code, count) in [
        (CapabilityIssueCode::InvalidCalibration, invalid),
        (CapabilityIssueCode::FrequencyDoppler, frequency),
        (CapabilityIssueCode::UnsupportedUnits, units),
        (CapabilityIssueCode::UnsupportedSpatialFormat, spatial),
        (CapabilityIssueCode::AmbiguousCalibration, ambiguous),
    ] {
        if count > 0 {
            out.issues.push(CapabilityIssue { code, count });
        }
    }
    out
}
fn valid_region_calibration(region: &Region) -> bool {
    region.index >= 0
        && region.bounds.min.x >= 0
        && region.bounds.min.y >= 0
        && region.bounds.max.x >= region.bounds.min.x
        && region.bounds.max.y >= region.bounds.min.y
        && region.delta_x.is_finite()
        && region.delta_y.is_finite()
        && region.delta_x != 0.0
        && region.delta_y != 0.0
        && region.reference_value_x.is_finite()
        && region.reference_value_y.is_finite()
}
fn ambiguous_region_pairs(regions: &[Region], absolute_axes: bool) -> i64 {
    let mut count = 0;
    for (i, a) in regions.iter().enumerate() {
        for b in &regions[i + 1..] {
            if inclusive_bounds_overlap(a, b) && !same_scaling(a, b, absolute_axes) {
                count += 1;
            }
        }
    }
    count
}
fn inclusive_bounds_overlap(a: &Region, b: &Region) -> bool {
    a.bounds.min.x <= b.bounds.max.x
        && b.bounds.min.x <= a.bounds.max.x
        && a.bounds.min.y <= b.bounds.max.y
        && b.bounds.min.y <= a.bounds.max.y
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Measurement {
    pub value: f64,
    pub unit: PhysicalUnit,
    pub reference: RegionReference,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AxisMeasurement {
    pub x: f64,
    pub x_unit: PhysicalUnit,
    pub y: f64,
    pub y_unit: PhysicalUnit,
    pub reference: RegionReference,
}
fn result(
    frame: &FrameCalibration,
    region: &Region,
    value: f64,
    unit: PhysicalUnit,
) -> Measurement {
    Measurement {
        value,
        unit,
        reference: RegionReference {
            frame_number: frame.frame_index.wrapping_add(1),
            region_index: region.index,
        },
    }
}
const CM: PhysicalUnit = PhysicalUnit::CENTIMETER;
pub fn distance(frame: &FrameCalibration, a: Point, b: Point) -> Result<Measurement, Error> {
    let region = compatible_region(frame, &[a, b], CM, CM, false)?;
    let (ax, ay) = region.physical(a);
    let (bx, by) = region.physical(b);
    Ok(result(frame, &region, fmath::hypot(bx - ax, by - ay), CM))
}
pub fn polyline_length(frame: &FrameCalibration, points: &[Point]) -> Result<Measurement, Error> {
    if points.len() < 2 {
        return Err(Error::other(
            "dicom/ultrasound: polyline requires at least two points",
        ));
    }
    let region = compatible_region(frame, points, CM, CM, false)?;
    let mut total = 0.0f64;
    let (mut px, mut py) = region.physical(points[0]);
    for &point in &points[1..] {
        let (x, y) = region.physical(point);
        total += fmath::hypot(x - px, y - py);
        (px, py) = (x, y);
    }
    Ok(result(frame, &region, total, CM))
}
pub fn circumference(frame: &FrameCalibration, points: &[Point]) -> Result<Measurement, Error> {
    if points.len() < 3 {
        return Err(Error::other(
            "dicom/ultrasound: circumference requires at least three points",
        ));
    }
    let mut closed = points.to_vec();
    closed.push(points[0]);
    polyline_length(frame, &closed)
}
fn corners(a: Point, b: Point) -> [Point; 4] {
    [a, b, Point::new(a.x, b.y), Point::new(b.x, a.y)]
}
pub fn rectangle_area(frame: &FrameCalibration, a: Point, b: Point) -> Result<Measurement, Error> {
    let region = compatible_region(frame, &corners(a, b), CM, CM, false)?;
    let (ax, ay) = region.physical(a);
    let (bx, by) = region.physical(b);
    Ok(result(
        frame,
        &region,
        ((bx - ax) * (by - ay)).abs(),
        PhysicalUnit::SQUARE_CENTIMETER,
    ))
}
pub fn ellipse_area(frame: &FrameCalibration, bounds: Rectangle) -> Result<Measurement, Error> {
    let (a, b) = (bounds.min, bounds.max);
    let region = compatible_region(frame, &corners(a, b), CM, CM, false)?;
    let (ax, ay) = region.physical(a);
    let (bx, by) = region.physical(b);
    Ok(result(
        frame,
        &region,
        std::f64::consts::PI * (bx - ax).abs() * (by - ay).abs() / 4.0,
        PhysicalUnit::SQUARE_CENTIMETER,
    ))
}
pub fn polygon_area(frame: &FrameCalibration, points: &[Point]) -> Result<Measurement, Error> {
    if points.len() < 3 {
        return Err(Error::other(
            "dicom/ultrasound: polygon requires at least three points",
        ));
    }
    let region = compatible_region(frame, points, CM, CM, false)?;
    let mut twice_area = 0.0f64;
    for (index, &point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        let (x1, y1) = region.physical(point);
        let (x2, y2) = region.physical(next);
        twice_area += x1 * y2 - x2 * y1;
    }
    Ok(result(
        frame,
        &region,
        twice_area.abs() / 2.0,
        PhysicalUnit::SQUARE_CENTIMETER,
    ))
}
pub fn doppler(frame: &FrameCalibration, point: Point) -> Result<AxisMeasurement, Error> {
    let region = compatible_region(
        frame,
        &[point],
        PhysicalUnit::SECOND,
        PhysicalUnit::CENTIMETER_PER_SEC,
        true,
    )?;
    if region.spatial_format != SPATIAL_SPECTRAL && region.spatial_format != SPATIAL_WAVEFORM {
        return Err(Error::detail(
            ErrorKind::UnsupportedUnits,
            format_args!("region {} is not spectral or waveform data", region.index),
        ));
    }
    if region.flags & (1 << 2) != 0 {
        return Err(Error::detail(
            ErrorKind::UnsupportedUnits,
            format_args!("region {} uses a frequency Doppler scale", region.index),
        ));
    }
    let (x, y) = region.physical(point);
    Ok(AxisMeasurement {
        x,
        x_unit: region.units_x,
        y,
        y_unit: region.units_y,
        reference: RegionReference {
            frame_number: frame.frame_index.wrapping_add(1),
            region_index: region.index,
        },
    })
}
fn compatible_region(
    frame: &FrameCalibration,
    points: &[Point],
    units_x: PhysicalUnit,
    units_y: PhysicalUnit,
    absolute_axes: bool,
) -> Result<Region, Error> {
    if points.is_empty() {
        return Err(Error::of(ErrorKind::Uncalibrated));
    }
    let mut candidate: Option<Region> = None;
    for region in &frame.regions {
        if region.units_x != units_x || region.units_y != units_y {
            continue;
        }
        if !points.iter().all(|&point| region.contains(point)) {
            continue;
        }
        if !valid_region_calibration(region) {
            return Err(Error::detail(
                ErrorKind::InvalidCalibration,
                format_args!("region {}", region.index),
            ));
        }
        match &candidate {
            None => candidate = Some(*region),
            Some(first) => {
                if !same_scaling(first, region, absolute_axes) {
                    return Err(Error::of(ErrorKind::AmbiguousRegion));
                }
            }
        }
    }
    if let Some(candidate) = candidate {
        return Ok(candidate);
    }
    for &point in points {
        for region in &frame.regions {
            if region.contains(point) && (region.units_x != units_x || region.units_y != units_y) {
                return Err(Error::detail(
                    ErrorKind::UnsupportedUnits,
                    format_args!("point {point} uses {}/{}", region.units_x, region.units_y),
                ));
            }
        }
    }
    if points_occupy_different_regions(&frame.regions, points) {
        return Err(Error::of(ErrorKind::CrossRegion));
    }
    Err(Error::of(ErrorKind::Uncalibrated))
}
fn same_scaling(a: &Region, b: &Region, absolute_axes: bool) -> bool {
    if a.units_x != b.units_x
        || a.units_y != b.units_y
        || a.delta_x != b.delta_x
        || a.delta_y != b.delta_y
    {
        return false;
    }
    if !absolute_axes {
        return true;
    }
    a.reference_value_x == b.reference_value_x
        && a.reference_value_y == b.reference_value_y
        && a.bounds.min.x.wrapping_add(a.reference_pixel_x)
            == b.bounds.min.x.wrapping_add(b.reference_pixel_x)
        && a.bounds.min.y.wrapping_add(a.reference_pixel_y)
            == b.bounds.min.y.wrapping_add(b.reference_pixel_y)
}
fn points_occupy_different_regions(regions: &[Region], points: &[Point]) -> bool {
    let mut first: i64 = -1;
    let mut inside = 0;
    for &point in points {
        let current = regions
            .iter()
            .find(|region| region.contains(point))
            .map_or(-1, |region| region.index);
        if current < 0 {
            continue;
        }
        inside += 1;
        if first < 0 {
            first = current;
        } else if first != current {
            return true;
        }
    }
    inside > 0 && inside < points.len()
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiometryKind {
    BiparietalDiameter,
    HeadCircumference,
    AbdominalCircumference,
    FemurLength,
}
impl BiometryKind {
    pub const ALL: [BiometryKind; 4] = [
        BiometryKind::BiparietalDiameter,
        BiometryKind::HeadCircumference,
        BiometryKind::AbdominalCircumference,
        BiometryKind::FemurLength,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            BiometryKind::BiparietalDiameter => "biparietal-diameter",
            BiometryKind::HeadCircumference => "head-circumference",
            BiometryKind::AbdominalCircumference => "abdominal-circumference",
            BiometryKind::FemurLength => "femur-length",
        }
    }
    pub fn parse(kind: &str) -> Option<BiometryKind> {
        Self::ALL.into_iter().find(|k| k.as_str() == kind)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodedConcept {
    pub code_value: &'static str,
    pub coding_scheme_designator: &'static str,
    pub code_meaning: &'static str,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiometryTemplate {
    pub kind: BiometryKind,
    pub concept: CodedConcept,
    pub circumference: bool,
}
pub fn template(kind: BiometryKind) -> BiometryTemplate {
    let (value, meaning, circumference) = match kind {
        BiometryKind::BiparietalDiameter => ("11820-8", "Biparietal Diameter", false),
        BiometryKind::HeadCircumference => ("11984-2", "Head Circumference", true),
        BiometryKind::AbdominalCircumference => ("11979-2", "Abdominal Circumference", true),
        BiometryKind::FemurLength => ("11963-6", "Femur Length", false),
    };
    BiometryTemplate {
        kind,
        concept: CodedConcept {
            code_value: value,
            coding_scheme_designator: "LN",
            code_meaning: meaning,
        },
        circumference,
    }
}
pub fn unsupported_biometry_template(kind: &str) -> Error {
    Error::other(format!(
        "dicom/ultrasound: unsupported biometry template {}",
        quote(kind)
    ))
}
pub fn measure_biometry(
    frame: &FrameCalibration,
    kind: BiometryKind,
    points: &[Point],
) -> (BiometryTemplate, Result<Measurement, Error>) {
    let template = template(kind);
    let measurement = if template.circumference {
        circumference(frame, points)
    } else if points.len() >= 2 {
        distance(frame, points[0], points[1])
    } else {
        Err(Error::other(format!(
            "dicom/ultrasound: {} requires two points",
            kind.as_str()
        )))
    };
    (template, measurement)
}
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub template: BiometryTemplate,
    pub value: f64,
    pub units: sr::CodedEntry,
    pub source_image: sr::ImageReference,
    pub spatial: sr::measurement_report::SpatialReference,
    pub tracking: sr::measurement_report::TrackingIdentifier,
    pub region: Option<RegionReference>,
}
pub fn annotations_from_measurement_report(
    report: Option<&sr::measurement_report::MeasurementReport>,
) -> Vec<Annotation> {
    let Some(report) = report else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for group in &report.groups {
        for measurement in &group.measurements {
            let name = &measurement.concept_name;
            let Some(template) = BiometryKind::ALL.into_iter().map(template).find(|t| {
                t.concept.coding_scheme_designator == name.coding_scheme_designator
                    && t.concept.code_value == name.code_value
            }) else {
                continue;
            };
            out.push(Annotation {
                template,
                value: measurement.value,
                units: measurement.units.clone(),
                source_image: measurement.image.clone(),
                spatial: measurement.spatial.clone(),
                tracking: group.tracking.clone(),
                region: region_reference_from_tracking(&group.tracking.identifier),
            });
        }
    }
    out
}
fn atoi(s: &str) -> Option<i64> {
    crate::numtext::parse_int(s).ok()
}
fn region_reference_from_tracking(identifier: &str) -> Option<RegionReference> {
    const MARKER: &str = "[US frame ";
    let start = identifier.rfind(MARKER)?;
    let rest = identifier[start + MARKER.len()..].strip_suffix(']')?;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    if fields.len() != 3 || fields[1] != "region" {
        return None;
    }
    let frame = atoi(fields[0])?;
    let region = atoi(fields[2])?;
    if frame <= 0 || region <= 0 {
        return None;
    }
    Some(RegionReference {
        frame_number: frame,
        region_index: region - 1,
    })
}
impl Annotation {
    pub fn validate_evidence(&self) -> Result<(), Error> {
        if self.source_image.sop_class_uid.is_empty()
            || self.source_image.sop_instance_uid.is_empty()
        {
            return Err(Error::other(
                "dicom/ultrasound: annotation has no source image evidence",
            ));
        }
        if let Some(region) = &self.region
            && !self.source_image.frames.is_empty()
            && !self.source_image.frames.contains(&region.frame_number)
        {
            return Err(Error::other(
                "dicom/ultrasound: annotation region frame is not referenced by its source image",
            ));
        }
        Ok(())
    }
}
