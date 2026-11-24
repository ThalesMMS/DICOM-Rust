#![forbid(unsafe_code)]
use super::accessors::trimmed_string;
use super::obj::Object;
use super::types::Tag;
use crate::fmath;
use crate::textfmt::to_upper;
pub const OBLIQUITY_THRESHOLD_COSINE: f64 = 0.8;
pub const DEFAULT_ORIENTATION_MIN_COMPONENT: f64 = 1e-4;
pub const MAX_ORIENTATION_LETTERS: usize = 3;
pub mod image_plane {
    pub const UNKNOWN: &str = "";
    pub const AXIAL: &str = "AXIAL";
    pub const CORONAL: &str = "CORONAL";
    pub const SAGITTAL: &str = "SAGITTAL";
    pub const OBLIQUE: &str = "OBLIQUE";
}
pub mod orientation_source {
    pub const NONE: &str = "";
    pub const IMAGE_ORIENTATION: &str = "image-orientation-patient";
    pub const PATIENT_ORIENTATION: &str = "patient-orientation";
    pub const MAMMOGRAPHY_VIEW: &str = "mammography-view";
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OrientationLabelOptions {
    pub max_letters: i64,
    pub min_component: f64,
    pub superior_inferior: bool,
}
impl OrientationLabelOptions {
    fn max_letters(&self) -> usize {
        if self.max_letters < 1 || self.max_letters > MAX_ORIENTATION_LETTERS as i64 {
            MAX_ORIENTATION_LETTERS
        } else {
            self.max_letters as usize
        }
    }
    fn min_component(&self) -> f64 {
        if self.min_component <= 0.0 || !self.min_component.is_finite() {
            DEFAULT_ORIENTATION_MIN_COMPONENT
        } else {
            self.min_component
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DisplayTransform {
    pub rotation_degrees: f64,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub flip_before_rotation: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrientationEdgeLabels {
    pub top: String,
    pub bottom: String,
    pub left: String,
    pub right: String,
}
impl OrientationEdgeLabels {
    pub fn is_zero(&self) -> bool {
        *self == OrientationEdgeLabels::default()
    }
}
fn normalize_direction(v: [f64; 3]) -> Option<[f64; 3]> {
    if v.iter().any(|c| !c.is_finite()) {
        return None;
    }
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length <= 1e-12 || length.is_infinite() {
        return None;
    }
    Some([v[0] / length, v[1] / length, v[2] / length])
}
fn axis_letter(axis: usize, positive: bool, superior_inferior: bool) -> u8 {
    match axis {
        0 => {
            if positive {
                b'L'
            } else {
                b'R'
            }
        }
        1 => {
            if positive {
                b'P'
            } else {
                b'A'
            }
        }
        _ => match (positive, superior_inferior) {
            (true, true) => b'S',
            (true, false) => b'H',
            (false, true) => b'I',
            (false, false) => b'F',
        },
    }
}
fn opposite_letter(letter: u8) -> Option<u8> {
    Some(match letter {
        b'L' => b'R',
        b'R' => b'L',
        b'A' => b'P',
        b'P' => b'A',
        b'H' => b'F',
        b'F' => b'H',
        b'S' => b'I',
        b'I' => b'S',
        _ => return None,
    })
}
pub fn patient_direction_label(direction: [f64; 3], opts: OrientationLabelOptions) -> String {
    let Some(unit) = normalize_direction(direction) else {
        return String::new();
    };
    let mut axes = [0usize, 1, 2];
    axes.sort_by(|&i, &j| {
        unit[j]
            .abs()
            .partial_cmp(&unit[i].abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let minimum = opts.min_component();
    let mut label = String::new();
    for &axis in &axes[..opts.max_letters()] {
        let value = unit[axis];
        if value.abs() <= minimum {
            break;
        }
        label.push(axis_letter(axis, value > 0.0, opts.superior_inferior) as char);
    }
    label
}
pub fn opposite_orientation_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    for &b in label.as_bytes() {
        match opposite_letter(b) {
            Some(o) => out.push(o as char),
            None => return String::new(),
        }
    }
    out
}
pub fn major_axis_label(
    direction: [f64; 3],
    threshold: f64,
    opts: OrientationLabelOptions,
) -> String {
    let Some(unit) = normalize_direction(direction) else {
        return String::new();
    };
    let threshold = if threshold <= 0.0 || !threshold.is_finite() {
        OBLIQUITY_THRESHOLD_COSINE
    } else {
        threshold
    };
    for (axis, value) in unit.iter().enumerate() {
        if value.abs() > threshold {
            return (axis_letter(axis, *value > 0.0, opts.superior_inferior) as char).to_string();
        }
    }
    String::new()
}
fn split_image_orientation(values: &[f64]) -> Option<([f64; 3], [f64; 3])> {
    if values.len() < 6 {
        return None;
    }
    let row = normalize_direction([values[0], values[1], values[2]]);
    let column = normalize_direction([values[3], values[4], values[5]]);
    Some((row?, column?))
}
fn major_axis(unit: [f64; 3]) -> Option<usize> {
    (0..3).find(|&a| unit[a].abs() > OBLIQUITY_THRESHOLD_COSINE)
}
pub fn classify_image_plane(iop: &[f64]) -> &'static str {
    let Some((row, column)) = split_image_orientation(iop) else {
        return image_plane::UNKNOWN;
    };
    match (major_axis(row), major_axis(column)) {
        (Some(r), Some(c)) if r != c => match r + c {
            1 => image_plane::AXIAL,
            2 => image_plane::CORONAL,
            _ => image_plane::SAGITTAL,
        },
        _ => image_plane::OBLIQUE,
    }
}
fn rotation_cosine_sine(degrees: f64) -> (f64, f64) {
    let mut normalized = degrees % 360.0;
    if normalized < 0.0 {
        normalized += 360.0;
    }
    if normalized == 0.0 {
        return (1.0, 0.0);
    }
    if normalized == 90.0 {
        return (0.0, 1.0);
    }
    if normalized == 180.0 {
        return (-1.0, 0.0);
    }
    if normalized == 270.0 {
        return (0.0, -1.0);
    }
    let radians = normalized * std::f64::consts::PI / 180.0;
    (fmath::cos(radians), fmath::sin(radians))
}
fn multiply(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    [
        [
            a[0][0] * b[0][0] + a[0][1] * b[1][0],
            a[0][0] * b[0][1] + a[0][1] * b[1][1],
        ],
        [
            a[1][0] * b[0][0] + a[1][1] * b[1][0],
            a[1][0] * b[0][1] + a[1][1] * b[1][1],
        ],
    ]
}
impl DisplayTransform {
    fn image_to_screen(&self) -> Option<[[f64; 2]; 2]> {
        if !self.rotation_degrees.is_finite() {
            return None;
        }
        let (cosine, sine) = rotation_cosine_sine(self.rotation_degrees);
        let rotation = [[cosine, -sine], [sine, cosine]];
        let mut flip = [[1.0, 0.0], [0.0, 1.0]];
        if self.flip_horizontal {
            flip[0][0] = -1.0;
        }
        if self.flip_vertical {
            flip[1][1] = -1.0;
        }
        Some(if self.flip_before_rotation {
            multiply(rotation, flip)
        } else {
            multiply(flip, rotation)
        })
    }
}
pub fn edge_labels_for_image_axes(
    row_direction: [f64; 3],
    column_direction: [f64; 3],
    transform: DisplayTransform,
    opts: OrientationLabelOptions,
) -> Option<OrientationEdgeLabels> {
    let row = normalize_direction(row_direction)?;
    let column = normalize_direction(column_direction)?;
    let m = transform.image_to_screen()?;
    let label = |sx: f64, sy: f64| {
        let ix = m[0][0] * sx + m[1][0] * sy;
        let iy = m[0][1] * sx + m[1][1] * sy;
        patient_direction_label(
            [
                ix * row[0] + iy * column[0],
                ix * row[1] + iy * column[1],
                ix * row[2] + iy * column[2],
            ],
            opts,
        )
    };
    Some(OrientationEdgeLabels {
        top: label(0.0, -1.0),
        bottom: label(0.0, 1.0),
        left: label(-1.0, 0.0),
        right: label(1.0, 0.0),
    })
}
pub fn edge_labels_for_image_orientation(
    iop: &[f64],
    transform: DisplayTransform,
    opts: OrientationLabelOptions,
) -> Option<OrientationEdgeLabels> {
    let (row, column) = split_image_orientation(iop)?;
    edge_labels_for_image_axes(row, column, transform, opts)
}
fn normalize_patient_orientation_value(value: &str, opts: OrientationLabelOptions) -> String {
    let value = to_upper(value.trim());
    if value.is_empty() {
        return String::new();
    }
    let mut seen = [false; 3];
    let mut out = Vec::with_capacity(value.len());
    for &b in value.as_bytes() {
        let mut letter = b;
        let axis = match letter {
            b'L' | b'R' => 0,
            b'A' | b'P' => 1,
            b'H' | b'F' => {
                if opts.superior_inferior {
                    letter = axis_letter(2, letter == b'H', true);
                }
                2
            }
            _ => return String::new(),
        };
        if seen[axis] {
            return String::new();
        }
        seen[axis] = true;
        out.push(letter);
    }
    out.truncate(opts.max_letters());
    String::from_utf8(out).unwrap_or_default()
}
pub fn edge_labels_for_patient_orientation(
    patient_orientation: &[String],
    transform: DisplayTransform,
    opts: OrientationLabelOptions,
) -> Option<OrientationEdgeLabels> {
    let mut values = [String::new(), String::new()];
    let mut usable = false;
    for (i, v) in patient_orientation.iter().take(2).enumerate() {
        values[i] = normalize_patient_orientation_value(v, opts);
        usable = usable || !values[i].is_empty();
    }
    if !usable {
        return None;
    }
    let m = transform.image_to_screen()?;
    if transform.rotation_degrees % 90.0 != 0.0 {
        return None;
    }
    let label = |sx: f64, sy: f64| {
        let ix = (m[0][0] * sx + m[1][0] * sy).round();
        let iy = (m[0][1] * sx + m[1][1] * sy).round();
        if ix > 0.0 {
            values[0].clone()
        } else if ix < 0.0 {
            opposite_orientation_label(&values[0])
        } else if iy > 0.0 {
            values[1].clone()
        } else if iy < 0.0 {
            opposite_orientation_label(&values[1])
        } else {
            String::new()
        }
    };
    Some(OrientationEdgeLabels {
        top: label(0.0, -1.0),
        bottom: label(0.0, 1.0),
        left: label(-1.0, 0.0),
        right: label(1.0, 0.0),
    })
}
pub fn mammography_patient_orientation(
    view_position: &str,
    laterality: &str,
) -> Option<Vec<String>> {
    let (row, side) = match to_upper(laterality.trim()).as_str() {
        "R" => ("P", "L"),
        "L" => ("A", "R"),
        _ => return None,
    };
    let s = |v: &str| v.to_string();
    match to_upper(view_position.trim()).as_str() {
        "CC" | "XCC" | "XCCL" | "XCCM" => Some(vec![s(row), s(side)]),
        "MLO" => Some(vec![s(row), format!("F{side}")]),
        "ML" | "LM" => Some(vec![s(row), s("F")]),
        _ => None,
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrientationInfo {
    pub image_orientation_patient: Option<Vec<f64>>,
    pub patient_orientation: Option<Vec<String>>,
    pub modality: String,
    pub view_position: String,
    pub image_laterality: String,
    pub laterality: String,
}
fn trimmed_values(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = values.iter().map(|v| v.trim().to_string()).collect();
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}
impl OrientationInfo {
    pub fn edge_labels(
        &self,
        transform: DisplayTransform,
        opts: OrientationLabelOptions,
    ) -> (OrientationEdgeLabels, &'static str) {
        if let Some(l) = edge_labels_for_image_orientation(
            self.image_orientation_patient.as_deref().unwrap_or(&[]),
            transform,
            opts,
        ) {
            return (l, orientation_source::IMAGE_ORIENTATION);
        }
        if let Some(l) = edge_labels_for_patient_orientation(
            self.patient_orientation.as_deref().unwrap_or(&[]),
            transform,
            opts,
        ) {
            return (l, orientation_source::PATIENT_ORIENTATION);
        }
        if self.modality.trim().eq_ignore_ascii_case("MG") {
            let mut laterality = self.image_laterality.trim();
            if laterality.is_empty()
                || laterality.eq_ignore_ascii_case("B")
                || laterality.eq_ignore_ascii_case("U")
            {
                laterality = &self.laterality;
            }
            if let Some(o) = mammography_patient_orientation(&self.view_position, laterality)
                && let Some(l) = edge_labels_for_patient_orientation(&o, transform, opts)
            {
                return (l, orientation_source::MAMMOGRAPHY_VIEW);
            }
        }
        (OrientationEdgeLabels::default(), orientation_source::NONE)
    }
}
impl Object {
    pub fn orientation_info_at(&self, frame_index: i64) -> OrientationInfo {
        let po = Tag::new(0x0020, 0x0020);
        let mut info = OrientationInfo {
            image_orientation_patient: self
                .frame_geometry_at(frame_index)
                .image_orientation_patient,
            ..OrientationInfo::default()
        };
        if let Some(values) = self.get_strings(po) {
            info.patient_orientation = Some(trimmed_values(&values));
        }
        info.modality = trimmed_string(self, Tag::new(0x0008, 0x0060));
        info.view_position = trimmed_string(self, Tag::new(0x0018, 0x5101));
        info.image_laterality = trimmed_string(self, Tag::new(0x0020, 0x0062));
        info.laterality = trimmed_string(self, Tag::new(0x0020, 0x0060));
        if let (Some(item), _, _) =
            self.functional_group_item(frame_index, Tag::new(0x0020, 0x9450))
            && let Some(values) = item.get_strings(po)
            && !trimmed_values(&values).is_empty()
        {
            info.patient_orientation = Some(trimmed_values(&values));
        }
        if let (Some(item), _, _) =
            self.functional_group_item(frame_index, Tag::new(0x0020, 0x9071))
        {
            let laterality = trimmed_string(&item, Tag::new(0x0020, 0x9072));
            if !laterality.is_empty() {
                info.image_laterality = laterality;
            }
        }
        info
    }
}
