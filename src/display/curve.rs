use super::GrayImage;
use super::object::{ObjectError, ObjectErrorKind, trim_space, u16_at};
use crate::object::{Object, Tag};
use crate::textfmt;
const INVALID_CURVE: &str = "dicom: invalid curve";
const CURVE_GROUP_FIRST: u16 = 0x5000;
const CURVE_GROUP_LAST: u16 = 0x501E;
const CURVE_DIMENSIONS: u16 = 0x0005;
const CURVE_NUMBER_OF_POINTS: u16 = 0x0010;
const CURVE_TYPE_OF_DATA: u16 = 0x0020;
const CURVE_DESCRIPTION: u16 = 0x0022;
const CURVE_AXIS_UNITS: u16 = 0x0030;
const CURVE_AXIS_LABELS: u16 = 0x0040;
const CURVE_DATA_VALUE_REPRESENTATION: u16 = 0x0103;
const CURVE_DATA_DESCRIPTOR: u16 = 0x0110;
const CURVE_COORDINATE_START: u16 = 0x0112;
const CURVE_COORDINATE_STEP: u16 = 0x0114;
const CURVE_LABEL: u16 = 0x2500;
const CURVE_DATA: u16 = 0x3000;
const MAX_CURVE_DIMENSIONS: u16 = 16;
pub const CURVE_VALUES_US: i64 = 0;
pub const CURVE_VALUES_SS: i64 = 1;
pub const CURVE_VALUES_FL: i64 = 2;
pub const CURVE_VALUES_FD: i64 = 3;
pub const CURVE_VALUES_SL: i64 = 4;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Curve {
    pub group: u16,
    pub dimensions: i64,
    pub type_of_data: String,
    pub description: String,
    pub label: String,
    pub axis_units: Option<Vec<String>>,
    pub axis_labels: Option<Vec<String>>,
    pub value_representation: i64,
    pub points: Vec<Vec<f64>>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CurvePoint {
    pub x: f64,
    pub y: f64,
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
impl Curve {
    pub fn is_image_graphic(&self) -> bool {
        if self.type_of_data != "POLY" && self.type_of_data != "ROI"
            || self.dimensions < 2
            || self.points.is_empty()
        {
            return false;
        }
        if let Some(units) = &self.axis_units {
            for unit in units.iter().take(2) {
                let unit = textfmt::to_upper(trim_space(unit));
                if !unit.is_empty() && unit != "PIXL" {
                    return false;
                }
            }
        }
        true
    }
    pub fn closed(&self) -> bool {
        self.type_of_data == "ROI"
    }
    pub fn image_points(&self) -> Option<Vec<CurvePoint>> {
        if !self.is_image_graphic() {
            return None;
        }
        let out: Vec<CurvePoint> = self
            .points
            .iter()
            .filter(|t| t.len() >= 2 && finite(t[0]) && finite(t[1]))
            .map(|t| CurvePoint {
                x: t[0] - 1.0,
                y: t[1] - 1.0,
            })
            .collect();
        (!out.is_empty()).then_some(out)
    }
    pub fn composite(&self, img: &mut GrayImage, value: u8) {
        let Some(mut points) = self.image_points() else {
            return;
        };
        let (w, h) = (img.width as i64, img.height as i64);
        if w <= 0 || h <= 0 {
            return;
        }
        let pixels = &mut img.pixels;
        let mut set = |x: i64, y: i64| {
            if x >= 0 && y >= 0 && x < w && y < h {
                pixels[(y * w + x) as usize] = value;
            }
        };
        if points.len() == 1 {
            set(round_int(points[0].x), round_int(points[0].y));
            return;
        }
        let mut last = points.len() - 1;
        if self.closed() && points[0] != points[last] {
            points.push(points[0]);
            last += 1;
        }
        let (max_x, max_y) = (w as f64 - 0.5, h as f64 - 0.5);
        for i in 0..last {
            if let Some((a, b)) = clip_segment(points[i], points[i + 1], -0.5, -0.5, max_x, max_y) {
                draw_line(
                    round_int(a.x),
                    round_int(a.y),
                    round_int(b.x),
                    round_int(b.y),
                    &mut set,
                );
            }
        }
    }
}
fn round_int(v: f64) -> i64 {
    v.round() as i64
}
fn draw_line(mut x0: i64, mut y0: i64, x1: i64, y1: i64, set: &mut impl FnMut(i64, i64)) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 > x1 { -1 } else { 1 };
    let sy = if y0 > y1 { -1 } else { 1 };
    let mut err = dx + dy;
    loop {
        set(x0, y0);
        if x0 == x1 && y0 == y1 {
            return;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}
fn clip_segment(
    a: CurvePoint,
    b: CurvePoint,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> Option<(CurvePoint, CurvePoint)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    for (p, q) in [
        (-dx, a.x - min_x),
        (dx, max_x - a.x),
        (-dy, a.y - min_y),
        (dy, max_y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return None;
            }
            if r > t0 {
                t0 = r;
            }
        } else {
            if r < t0 {
                return None;
            }
            if r < t1 {
                t1 = r;
            }
        }
    }
    Some((
        CurvePoint {
            x: a.x + t0 * dx,
            y: a.y + t0 * dy,
        },
        CurvePoint {
            x: a.x + t1 * dx,
            y: a.y + t1 * dy,
        },
    ))
}
pub fn curves_from_object(obj: &Object) -> (Vec<Curve>, Option<ObjectError>) {
    let mut curves = Vec::new();
    let mut errs = Vec::new();
    for group in (CURVE_GROUP_FIRST..=CURVE_GROUP_LAST).step_by(2) {
        let Some(data) = obj.get_raw(Tag::new(group, CURVE_DATA)) else {
            continue;
        };
        match decode_curve(obj, group, &data) {
            Ok(c) => curves.push(c),
            Err(e) => errs.push(Some(e)),
        }
    }
    (curves, ObjectError::join(errs))
}
fn group_string(obj: &Object, group: u16, element: u16) -> String {
    obj.get_string(Tag::new(group, element))
        .map(|v| trim_space(&v).to_owned())
        .unwrap_or_default()
}
fn group_uint16(obj: &Object, group: u16, element: u16) -> Option<u16> {
    let raw = obj.get_raw(Tag::new(group, element))?;
    (raw.len() >= 2).then(|| u16_at(obj, &raw))
}
fn curve_uint16s(obj: &Object, group: u16, element: u16) -> Vec<u16> {
    let Some(raw) = obj.get_raw(Tag::new(group, element)) else {
        return Vec::new();
    };
    raw.as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16_at(obj, p))
        .collect()
}
fn curve_strings(obj: &Object, group: u16, element: u16) -> Option<Vec<String>> {
    obj.get_strings(Tag::new(group, element))
        .map(|v| v.iter().map(|s| trim_space(s).to_owned()).collect())
}
fn nth_or_first(values: &[u16], n: usize) -> Option<u16> {
    values.get(n).or(values.first()).copied()
}
fn curve_value_size(rep: i64) -> usize {
    match rep {
        CURVE_VALUES_US | CURVE_VALUES_SS => 2,
        CURVE_VALUES_FL | CURVE_VALUES_SL => 4,
        CURVE_VALUES_FD => 8,
        _ => 0,
    }
}
fn curve_value(raw: &[u8], rep: i64, big_endian: bool) -> f64 {
    macro_rules! read {
        ($t:ty, $n:expr) => {{
            let b: [u8; $n] = raw[..$n].try_into().expect("sized");
            if big_endian {
                <$t>::from_be_bytes(b)
            } else {
                <$t>::from_le_bytes(b)
            }
        }};
    }
    match rep {
        CURVE_VALUES_US => f64::from(read!(u16, 2)),
        CURVE_VALUES_SS => f64::from(read!(i16, 2)),
        CURVE_VALUES_FL => f64::from(read!(f32, 4)),
        CURVE_VALUES_FD => read!(f64, 8),
        CURVE_VALUES_SL => f64::from(read!(i32, 4)),
        _ => 0.0,
    }
}
fn decode_curve(obj: &Object, group: u16, data: &[u8]) -> Result<Curve, ObjectError> {
    let invalid = |detail: String| {
        ObjectError::new(
            ObjectErrorKind::InvalidCurve,
            format!(
                "{INVALID_CURVE}: group {}: {detail}",
                super::overlay_object::hex_0x(group)
            ),
        )
    };
    let mut curve = Curve {
        group,
        type_of_data: textfmt::to_upper(&group_string(obj, group, CURVE_TYPE_OF_DATA)),
        description: group_string(obj, group, CURVE_DESCRIPTION),
        label: group_string(obj, group, CURVE_LABEL),
        axis_units: curve_strings(obj, group, CURVE_AXIS_UNITS),
        axis_labels: curve_strings(obj, group, CURVE_AXIS_LABELS),
        ..Curve::default()
    };
    let dims = group_uint16(obj, group, CURVE_DIMENSIONS);
    let dims = match dims {
        Some(d) if (1..=MAX_CURVE_DIMENSIONS).contains(&d) => d,
        _ => {
            return Err(invalid(format!(
                "Curve Dimensions {} missing or outside 1-{MAX_CURVE_DIMENSIONS}",
                dims.unwrap_or(0)
            )));
        }
    };
    curve.dimensions = i64::from(dims);
    let points = match group_uint16(obj, group, CURVE_NUMBER_OF_POINTS) {
        Some(p) if p >= 1 => p,
        _ => return Err(invalid("Number of Points missing or zero".into())),
    };
    let rep = group_uint16(obj, group, CURVE_DATA_VALUE_REPRESENTATION)
        .ok_or_else(|| invalid("Data Value Representation missing".into()))?;
    let size = curve_value_size(i64::from(rep));
    if size == 0 {
        return Err(invalid(format!(
            "Data Value Representation {rep} is not 0-4"
        )));
    }
    curve.value_representation = i64::from(rep);
    let dimensions = usize::from(dims);
    let descriptor = curve_uint16s(obj, group, CURVE_DATA_DESCRIPTOR);
    if !descriptor.is_empty() && descriptor.len() != dimensions {
        return Err(invalid(format!(
            "Curve Data Descriptor has {} values for {dimensions} dimensions",
            descriptor.len()
        )));
    }
    let starts = curve_uint16s(obj, group, CURVE_COORDINATE_START);
    let steps = curve_uint16s(obj, group, CURVE_COORDINATE_STEP);
    let mut stored = dimensions;
    let mut interval = vec![None; dimensions];
    for (axis, &value) in descriptor.iter().enumerate() {
        if value != 0 {
            continue;
        }
        let k = dimensions - stored;
        let (Some(start), Some(step)) = (nth_or_first(&starts, k), nth_or_first(&steps, k)) else {
            return Err(invalid(format!(
                "dimension {} uses interval spacing without Coordinate Start/Step Value",
                axis + 1
            )));
        };
        interval[axis] = Some((f64::from(start), f64::from(step)));
        stored -= 1;
    }
    let need = u64::from(points) * stored as u64 * size as u64;
    if (data.len() as u64) < need {
        return Err(invalid(format!(
            "Curve Data has {} bytes, too few for {points} point(s) of {stored} stored value(s) of {size} bytes",
            data.len()
        )));
    }
    let big_endian = obj.value_big_endian();
    let mut offset = 0;
    curve.points = (0..usize::from(points))
        .map(|i| {
            (0..dimensions)
                .map(|axis| match interval[axis] {
                    Some((start, step)) => start + step * i as f64,
                    None => {
                        let v =
                            curve_value(&data[offset..offset + size], i64::from(rep), big_endian);
                        offset += size;
                        v
                    }
                })
                .collect()
        })
        .collect();
    Ok(curve)
}
