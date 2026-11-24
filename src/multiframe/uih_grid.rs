use super::{
    Error, Kind, Limits, MR_IMAGE_STORAGE_UID, TAG_COLUMNS, TAG_IMAGE_ORIENTATION_PATIENT,
    TAG_IMAGE_POSITION_PATIENT, TAG_IMAGE_TYPE, TAG_NUMBER_OF_FRAMES,
    TAG_PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE, TAG_PIXEL_SPACING, TAG_ROWS,
    TAG_SHARED_FUNCTIONAL_GROUPS_SEQUENCE, TAG_SLICE_THICKNESS, TAG_SOP_CLASS_UID,
    TAG_SPACING_BETWEEN_SLICES, cross, dot, normalize_limits, uint16_value, unit,
};
use crate::dictionary::reservations::{PrivateError, private_tag};
use crate::fmath;
use crate::numtext;
use crate::object::{Object, Tag, Value, Vr};
use crate::siemens::trim_space;
const UIH_CREATOR: &str = "Image Private Header";
const UIH_GEOMETRY_TOLERANCE: f64 = 1e-4;
#[derive(Clone, Copy, Debug, Default)]
pub struct UihGridOptions {
    pub limits: Limits,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UihGridGeometry {
    pub images: i64,
    pub grid_rows: i64,
    pub grid_columns: i64,
    pub rows: i64,
    pub columns: i64,
    pub normal: [f64; 3],
    pub private_normal: [f64; 3],
    pub pixel_spacing: [f64; 2],
    pub positions: Vec<[f64; 3]>,
    pub orientations: Vec<[f64; 6]>,
    pub(crate) source_rows: i64,
    pub(crate) source_columns: i64,
}
pub fn uih_grid_geometry_of(
    dataset: Option<&Object>,
    opts: UihGridOptions,
) -> Result<UihGridGeometry, Error> {
    let limits = normalize_limits(opts.limits)?;
    uih_grid_geometry(dataset, &limits)
}
fn fail(reason: &str) -> Error {
    Error::wrap(Kind::Nonconformant, format!("UIH {reason}"))
}
struct Bad;
pub(crate) fn uih_grid_geometry(
    dataset: Option<&Object>,
    limits: &Limits,
) -> Result<UihGridGeometry, Error> {
    let Some(dataset) = dataset else {
        return Err(Error::bare(Kind::NotUihGrid));
    };
    if dataset.len() as i64 > limits.max_elements {
        return Err(Error::bare(Kind::ResourceLimit));
    }
    match uih_text(dataset, TAG_IMAGE_TYPE, Vr::CS, 128) {
        Ok(image_type)
            if image_type.len() >= 2
                && trim_space(&image_type[image_type.len() - 1]) == "VFRAME" => {}
        _ => return Err(Error::bare(Kind::NotUihGrid)),
    }
    let count_tag = match private_tag(dataset, 0x0065, UIH_CREATOR, 0x50) {
        Ok(tag) => tag,
        Err(PrivateError::Missing) => return Err(Error::bare(Kind::NotUihGrid)),
        Err(_) => return Err(fail("private reservation is invalid or ambiguous")),
    };
    match uih_text(dataset, TAG_SOP_CLASS_UID, Vr::UI, 64) {
        Ok(classes) if classes.len() == 1 && trim_space(&classes[0]) == MR_IMAGE_STORAGE_UID => {}
        _ => return Err(Error::bare(Kind::UnsupportedSopClass)),
    }
    let count = match uih_private_floats(dataset, count_tag, 1) {
        Ok(counts) if counts[0] >= 1.0 && counts[0] == counts[0].trunc() => counts[0],
        _ => return Err(fail("slice count must be a positive integer DS")),
    };
    if count > limits.max_frames as f64 {
        return Err(Error::bare(Kind::ResourceLimit));
    }
    let (rows, columns) = match (
        uih_dimension(dataset, TAG_ROWS),
        uih_dimension(dataset, TAG_COLUMNS),
    ) {
        (Some(rows), Some(columns)) if rows != 0 && columns != 0 => (rows, columns),
        _ => return Err(fail("Rows and Columns are required")),
    };
    let source_pixels = rows as u64 * columns as u64;
    if count > source_pixels as f64 {
        return Err(fail("slice count exceeds pixel count"));
    }
    let images = count as i64;
    if source_pixels > limits.max_pixels {
        return Err(Error::bare(Kind::ResourceLimit));
    }
    if dataset.has(TAG_SHARED_FUNCTIONAL_GROUPS_SEQUENCE)
        || dataset.has(TAG_PER_FRAME_FUNCTIONAL_GROUPS_SEQUENCE)
    {
        return Err(fail(
            "functional groups are unsupported in this classic grid layout",
        ));
    }
    if dataset.has(TAG_NUMBER_OF_FRAMES) {
        let one = match uih_text(dataset, TAG_NUMBER_OF_FRAMES, Vr::IS, 12) {
            Ok(values) if values.len() == 1 => {
                numtext::parse_int(trim_space(&values[0])).ok() == Some(1)
            }
            _ => false,
        };
        if !one {
            return Err(fail("source must contain one grid frame"));
        }
    }
    for e in dataset.elements() {
        if (0x6000..=0x60ff).contains(&e.tag().group) {
            return Err(fail("grid-relative overlays are unsupported"));
        }
    }
    let rows = rows as i64;
    let columns = columns as i64;
    let mut g = UihGridGeometry {
        images,
        source_rows: rows,
        source_columns: columns,
        ..Default::default()
    };
    g.grid_columns = 1;
    while g.grid_columns < (images - 1) / g.grid_columns + 1 {
        g.grid_columns += 1;
    }
    g.grid_rows = (images - 1) / g.grid_columns + 1;
    if rows % g.grid_rows != 0 || columns % g.grid_columns != 0 {
        return Err(fail("dimensions do not divide the tile grid"));
    }
    g.rows = rows / g.grid_rows;
    g.columns = columns / g.grid_columns;
    let Ok(iop) = uih_orientation(dataset) else {
        return Err(fail("Image Orientation (Patient) must be orthonormal"));
    };
    let (row, _) = unit([iop[0], iop[1], iop[2]]);
    let (column, _) = unit([iop[3], iop[4], iop[5]]);
    g.normal = unit(cross(row, column)).0;
    let spacing = match uih_floats(dataset, TAG_PIXEL_SPACING, 2) {
        Ok(v) if v[0] > 0.0 && v[1] > 0.0 => v,
        _ => return Err(fail("Pixel Spacing must be a positive pair")),
    };
    g.pixel_spacing = [spacing[0], spacing[1]];
    let normal_tag = private_tag(dataset, 0x0065, UIH_CREATOR, 0x14).unwrap_or_default();
    let Ok(normal) = uih_private_floats(dataset, normal_tag, 3) else {
        return Err(fail("private slice normal is required"));
    };
    g.private_normal = [normal[0], normal[1], normal[2]];
    if (dot(g.private_normal, g.private_normal) - 1.0).abs() > UIH_GEOMETRY_TOLERANCE
        || (dot(g.private_normal, g.normal).abs() - 1.0).abs() > UIH_GEOMETRY_TOLERANCE
    {
        return Err(fail("private slice normal disagrees with orientation"));
    }
    let frame_tag = private_tag(dataset, 0x0065, UIH_CREATOR, 0x51).unwrap_or_default();
    let items = match dataset.get(frame_tag) {
        Some(element) if element.vr() == Vr::SQ || element.vr() == Vr::UN => match &element.value {
            Value::Sequence(items) if items.len() as i64 == images => items,
            _ => return Err(fail("frame sequence must contain one item per tile")),
        },
        _ => return Err(fail("frame sequence must contain one item per tile")),
    };
    if limits.max_depth < 2 {
        return Err(Error::bare(Kind::ResourceLimit));
    }
    let mut visits = dataset.len() as i64 + images;
    if visits > limits.max_elements {
        return Err(Error::bare(Kind::ResourceLimit));
    }
    for item in items {
        if item.elements.len() as i64 > limits.max_elements - visits {
            return Err(Error::bare(Kind::ResourceLimit));
        }
        visits += item.elements.len() as i64;
    }
    let mut advertised = 0f64;
    for tag in [TAG_SPACING_BETWEEN_SLICES, TAG_SLICE_THICKNESS] {
        if dataset.has(tag) {
            match uih_floats(dataset, tag, 1) {
                Ok(v) if v[0] > 0.0 => {
                    if tag == TAG_SPACING_BETWEEN_SLICES {
                        advertised = v[0];
                    }
                }
                _ => {
                    return Err(fail(
                        "slice thickness/spacing must be positive when present",
                    ));
                }
            }
        }
    }
    if images > limits.max_output_bytes / 72 {
        return Err(Error::bare(Kind::ResourceLimit));
    }
    g.positions = vec![[0f64; 3]; images as usize];
    g.orientations = vec![[0f64; 6]; images as usize];
    let mut previous = 0f64;
    let mut direction = 0f64;
    for (k, item) in items.iter().enumerate() {
        let mut seen: Vec<Tag> = Vec::new();
        for e in &item.elements {
            let tag = e.tag();
            if tag == TAG_IMAGE_POSITION_PATIENT
                || tag == TAG_IMAGE_ORIENTATION_PATIENT
                || tag == TAG_PIXEL_SPACING
            {
                if seen.contains(&tag) {
                    return Err(fail("duplicate frame geometry attribute"));
                }
                seen.push(tag);
            }
        }
        let frame = Object::from_elements(item.elements.clone());
        let Ok(position) = uih_floats(&frame, TAG_IMAGE_POSITION_PATIENT, 3) else {
            return Err(fail(
                "every frame must have a finite Image Position (Patient)",
            ));
        };
        g.positions[k] = [position[0], position[1], position[2]];
        g.orientations[k] = iop;
        if frame.has(TAG_IMAGE_ORIENTATION_PATIENT) {
            let Ok(orientation) = uih_orientation(&frame) else {
                return Err(fail("frame orientation is invalid"));
            };
            for i in 0..6 {
                if (orientation[i] - iop[i]).abs() > UIH_GEOMETRY_TOLERANCE {
                    return Err(fail("frame orientation disagrees with common orientation"));
                }
            }
            g.orientations[k] = orientation;
        }
        if frame.has(TAG_PIXEL_SPACING) {
            match uih_floats(&frame, TAG_PIXEL_SPACING, 2) {
                Ok(v)
                    if v[0] > 0.0
                        && v[1] > 0.0
                        && uih_close(v[0], spacing[0])
                        && uih_close(v[1], spacing[1]) => {}
                _ => {
                    return Err(fail("frame Pixel Spacing disagrees with common spacing"));
                }
            }
        }
        let projection = dot(g.positions[k], g.normal);
        if projection.is_nan() || projection.is_infinite() {
            return Err(fail("frame projection is not finite"));
        }
        if k > 0 {
            let delta = projection - previous;
            if delta.is_nan()
                || delta.is_infinite()
                || delta.abs() <= UIH_GEOMETRY_TOLERANCE
                || (k > 1 && delta * direction <= 0.0)
            {
                return Err(fail(
                    "frame positions must be distinct and monotonic along the normal",
                ));
            }
            if advertised > 0.0 && !uih_close(delta.abs(), advertised) {
                return Err(fail("frame positions disagree with Spacing Between Slices"));
            }
            direction = delta;
        }
        previous = projection;
    }
    Ok(g)
}
fn uih_close(a: f64, b: f64) -> bool {
    (a - b).abs() <= UIH_GEOMETRY_TOLERANCE * fmath::max(1.0, fmath::max(a.abs(), b.abs()))
}
fn uih_dimension(dataset: &Object, tag: Tag) -> Option<u16> {
    let e = dataset.get(tag)?;
    if e.vr() != Vr::US {
        return None;
    }
    uint16_value(dataset, tag)
}
fn uih_orientation(dataset: &Object) -> Result<[f64; 6], Bad> {
    let v = uih_floats(dataset, TAG_IMAGE_ORIENTATION_PATIENT, 6)?;
    let r = [v[0], v[1], v[2]];
    let c = [v[3], v[4], v[5]];
    if (dot(r, r) - 1.0).abs() > UIH_GEOMETRY_TOLERANCE
        || (dot(c, c) - 1.0).abs() > UIH_GEOMETRY_TOLERANCE
        || dot(r, c).abs() > UIH_GEOMETRY_TOLERANCE
    {
        return Err(Bad);
    }
    Ok([v[0], v[1], v[2], v[3], v[4], v[5]])
}
fn uih_text(dataset: &Object, tag: Tag, vr: Vr, max_bytes: usize) -> Result<Vec<String>, Bad> {
    let e = dataset.get(tag).ok_or(Bad)?;
    if e.vr() != vr {
        return Err(Bad);
    }
    match &e.value {
        Value::Raw(raw) => {
            if raw.len() > max_bytes {
                return Err(Bad);
            }
        }
        Value::Strings(values) => {
            if values.len() > max_bytes {
                return Err(Bad);
            }
            let mut n = 0usize;
            for s in values {
                if s.len() > max_bytes - n {
                    return Err(Bad);
                }
                n += s.len();
            }
        }
        _ => return Err(Bad),
    }
    dataset.get_strings(tag).ok_or(Bad)
}
fn uih_floats(dataset: &Object, tag: Tag, count: usize) -> Result<Vec<f64>, Bad> {
    let v = uih_text(dataset, tag, Vr::DS, count * 17)?;
    if v.len() != count {
        return Err(Bad);
    }
    for text in &v {
        let text = text.trim_matches(' ');
        if text.is_empty() || text.len() > 16 {
            return Err(Bad);
        }
        if text
            .chars()
            .any(|c| !c.is_ascii_digit() && !matches!(c, '+' | '-' | '.' | 'E' | 'e'))
        {
            return Err(Bad);
        }
    }
    let floats = dataset.get_floats(tag).map_err(|_| Bad)?;
    if floats.len() != count || floats.iter().any(|v| v.is_nan() || v.is_infinite()) {
        return Err(Bad);
    }
    Ok(floats)
}
fn uih_private_floats(dataset: &Object, tag: Tag, count: usize) -> Result<Vec<f64>, Bad> {
    let e = dataset.get(tag).ok_or(Bad)?;
    if e.vr() == Vr::UN {
        if !matches!(e.value, Value::Raw(_)) {
            return Err(Bad);
        }
        let mut e = e.clone();
        e.header.vr = Vr::DS;
        return uih_floats(&Object::from_elements(vec![e]), tag, count);
    }
    uih_floats(dataset, tag, count)
}
pub(crate) fn uih_required_uid(dataset: &Object, tag: Tag) -> Result<String, Error> {
    match uih_text(dataset, tag, Vr::UI, 64) {
        Ok(v) if v.len() == 1 && crate::object::is_valid_uid(trim_space(&v[0]).as_bytes()) => {
            Ok(trim_space(&v[0]).to_string())
        }
        _ => Err(Error::bare(Kind::Nonconformant)),
    }
}
