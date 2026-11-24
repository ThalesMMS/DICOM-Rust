#![forbid(unsafe_code)]
mod preview;
mod transform;
use crate::derivedio as d;
use crate::numtext::format_float;
use crate::object::inherit::{apply_derived_common, inherit_modules};
use crate::object::{File, Object, Tag, Vr};
use crate::render::{GeometryTolerances, SliceGeometry, Vec3, VolumeGeometry};
pub use crate::roi::{
    ImagePoint, IsoContour, MaskRun, Point2D, RasterMask, RoiShape, Segmentation3D,
    SubpixelContourOptions, VectorRoi, closed_polygon_area, rasterize_polygon,
};
pub use preview::{SlicePreview, SlicePreviewOptions, preview_slice};
use std::collections::{BTreeMap, HashMap, HashSet};
pub const RT_STRUCTURE_SET_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.3";
pub const CONTOUR_POINT: &str = "POINT";
pub const CONTOUR_OPEN_PLANAR: &str = "OPEN_PLANAR";
pub const CONTOUR_OPEN_NON_PLANAR: &str = "OPEN_NONPLANAR";
pub const CONTOUR_CLOSED_PLANAR: &str = "CLOSED_PLANAR";
pub const CONTOUR_CLOSED_PLANAR_XOR: &str = "CLOSEDPLANAR_XOR";
const DETACHED_STUDY_MANAGEMENT_SOP_CLASS: &str = "1.2.840.10008.3.1.2.3.2";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    UnsupportedSopClass,
    InvalidObject,
    MissingReference,
    GeometryMismatch,
    UnsupportedContourType,
    Transform,
    Object,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::UnsupportedSopClass => "dicom/rtstruct: unsupported SOP class",
            Kind::InvalidObject => "dicom/rtstruct: invalid object",
            Kind::MissingReference => "dicom/rtstruct: missing reference",
            Kind::GeometryMismatch => "dicom/rtstruct: geometry mismatch",
            Kind::UnsupportedContourType => "dicom/rtstruct: unsupported contour geometric type",
            Kind::Transform => "rtstruct: contour points cannot be mapped",
            Kind::Object => "",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    msg: String,
}
impl Error {
    pub(crate) fn new(kind: Kind, detail: impl AsRef<str>) -> Error {
        Error {
            kind,
            msg: format!("{}: {}", kind.text(), detail.as_ref()),
        }
    }
    fn object(err: crate::object::Error) -> Error {
        Error {
            kind: Kind::Object,
            msg: err.to_string(),
        }
    }
    fn suffixed(self, suffix: &str) -> Error {
        Error {
            kind: self.kind,
            msg: format!("{}: {suffix}", self.msg),
        }
    }
    fn prefixed(self, prefix: &str) -> Error {
        Error {
            kind: self.kind,
            msg: format!("{prefix}: {}", self.msg),
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}
impl std::error::Error for Error {}
const TAG_STRUCTURE_SET_LABEL: Tag = Tag::new(0x3006, 0x0002);
const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
const TAG_REFERENCED_FRAME_OF_REFERENCE_SEQ: Tag = Tag::new(0x3006, 0x0010);
const TAG_RT_REFERENCED_STUDY_SEQUENCE: Tag = Tag::new(0x3006, 0x0012);
const TAG_RT_REFERENCED_SERIES_SEQUENCE: Tag = Tag::new(0x3006, 0x0014);
const TAG_REFERENCED_FRAME_OF_REFERENCE_UID: Tag = Tag::new(0x3006, 0x0024);
const TAG_STRUCTURE_SET_ROI_SEQUENCE: Tag = Tag::new(0x3006, 0x0020);
const TAG_ROI_NUMBER: Tag = Tag::new(0x3006, 0x0022);
const TAG_ROI_NAME: Tag = Tag::new(0x3006, 0x0026);
const TAG_ROI_CONTOUR_SEQUENCE: Tag = Tag::new(0x3006, 0x0039);
const TAG_ROI_DISPLAY_COLOR: Tag = Tag::new(0x3006, 0x002A);
const TAG_REFERENCED_ROI_NUMBER: Tag = Tag::new(0x3006, 0x0084);
const TAG_CONTOUR_SEQUENCE: Tag = Tag::new(0x3006, 0x0040);
const TAG_CONTOUR_GEOMETRIC_TYPE: Tag = Tag::new(0x3006, 0x0042);
const TAG_NUMBER_OF_CONTOUR_POINTS: Tag = Tag::new(0x3006, 0x0046);
const TAG_CONTOUR_DATA: Tag = Tag::new(0x3006, 0x0050);
const TAG_CONTOUR_IMAGE_SEQUENCE: Tag = Tag::new(0x3006, 0x0016);
const TAG_RT_ROI_OBSERVATIONS_SEQUENCE: Tag = Tag::new(0x3006, 0x0080);
const TAG_OBSERVATION_NUMBER: Tag = Tag::new(0x3006, 0x0082);
const TAG_RT_ROI_INTERPRETED_TYPE: Tag = Tag::new(0x3006, 0x00A4);
const TAG_ROI_INTERPRETER: Tag = Tag::new(0x3006, 0x00A6);
const TAG_ROI_GENERATION_ALGORITHM: Tag = Tag::new(0x3006, 0x0036);
const TAG_STRUCTURE_SET_DATE: Tag = Tag::new(0x3006, 0x0008);
const TAG_STRUCTURE_SET_TIME: Tag = Tag::new(0x3006, 0x0009);
const TAG_OPERATORS_NAME: Tag = Tag::new(0x0008, 0x1070);
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl Point3D {
    pub const fn new(x: f64, y: f64, z: f64) -> Point3D {
        Point3D { x, y, z }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ReferencedImage {
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contour {
    pub geometric_type: String,
    pub points: Vec<Point3D>,
    pub referenced_images: Vec<ReferencedImage>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Roi {
    pub number: i64,
    pub name: String,
    pub color_rgb: [i64; 3],
    pub generation_algorithm: String,
    pub contours: Vec<Contour>,
}
pub use crate::object::inherit::DerivedCommon;
#[derive(Debug, Clone, Default)]
pub struct StructureSet {
    pub common: DerivedCommon,
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub label: String,
    pub series_number: i64,
    pub date: String,
    pub time: String,
    pub rois: Vec<Roi>,
}
#[derive(Debug, Clone, Default)]
pub struct RasterizeOptions {
    pub roi_number: i64,
    pub geometry: VolumeGeometry,
    pub columns: i64,
    pub rows: i64,
}
#[derive(Debug, Clone, Default)]
pub struct VectorRoiContour {
    pub roi: VectorRoi,
    pub geometry: SliceGeometry,
    pub referenced_images: Vec<ReferencedImage>,
}
#[derive(Debug, Clone, Default)]
pub struct RasterRoiContour {
    pub mask: Option<RasterMask>,
    pub geometry: SliceGeometry,
    pub referenced_images: Vec<ReferencedImage>,
    pub smooth_sigma: f64,
}
#[derive(Debug, Clone, Default)]
pub struct VectorRoiInput {
    pub number: i64,
    pub name: String,
    pub color_rgb: [i64; 3],
    pub contours: Vec<VectorRoiContour>,
    pub masks: Vec<RasterRoiContour>,
}
#[derive(Debug, Clone, Default)]
pub struct FromVectorRoisOptions {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub label: String,
    pub rois: Vec<VectorRoiInput>,
}
fn g6(v: f64) -> String {
    format_float(v, 'g', 6)
}
pub fn write(set: &StructureSet) -> Result<File, Error> {
    let sop_class_uid = if set.sop_class_uid.is_empty() {
        RT_STRUCTURE_SET_STORAGE
    } else {
        set.sop_class_uid.as_str()
    };
    if sop_class_uid != RT_STRUCTURE_SET_STORAGE {
        return Err(Error::new(Kind::UnsupportedSopClass, sop_class_uid));
    }
    validate_structure_set(set)?;
    let label = if set.label.is_empty() {
        "DCRS_RTSTRUCT"
    } else {
        set.label.as_str()
    };
    let (mut date, mut clock) = (set.date.clone(), set.time.clone());
    if date.is_empty() || clock.is_empty() {
        let now = chrono::Local::now();
        if date.is_empty() {
            date = now.format("%Y%m%d").to_string();
        }
        if clock.is_empty() {
            clock = now.format("%H%M%S").to_string();
        }
    }
    let series_number = if set.series_number > 0 {
        d::is(d::TAG_SERIES_NUMBER, &[set.series_number])
    } else {
        d::is(d::TAG_SERIES_NUMBER, &[])
    };
    let mut dataset = d::object(vec![
        d::cs(TAG_SPECIFIC_CHARACTER_SET, "ISO_IR 192"),
        d::ui(d::TAG_SOP_CLASS_UID, sop_class_uid),
        d::ui(d::TAG_SOP_INSTANCE_UID, &set.sop_instance_uid),
        d::cs(d::TAG_MODALITY, "RTSTRUCT"),
        d::ui(d::TAG_STUDY_INSTANCE_UID, &set.study_instance_uid),
        d::ui(d::TAG_SERIES_INSTANCE_UID, &set.series_instance_uid),
        d::sh(TAG_STRUCTURE_SET_LABEL, label),
        d::str_elem(TAG_STRUCTURE_SET_DATE, Vr::DA, &date),
        d::str_elem(TAG_STRUCTURE_SET_TIME, Vr::TM, &clock),
        series_number,
        d::str_elem(TAG_OPERATORS_NAME, Vr::PN, ""),
        referenced_frame_of_reference_sequence(set),
        structure_set_roi_sequence(set),
        roi_contour_sequence(&set.rois),
        rt_roi_observations_sequence(&set.rois),
    ]);
    let uid = set.frame_of_reference_uid.trim();
    if !uid.is_empty() {
        dataset.put(d::ui(d::TAG_FRAME_OF_REFERENCE_UID, uid));
    }
    apply_derived_common(
        &mut dataset,
        set.common.source.as_ref(),
        set.common.equipment.clone(),
        set.common.now,
        inherit_modules::ALL,
    )
    .map_err(Error::object)?;
    d::file(sop_class_uid, &set.sop_instance_uid, Some(dataset)).map_err(Error::object)
}
pub fn read(obj: &Object) -> Result<StructureSet, Error> {
    let sop_class_uid = d::clean_uid(obj, d::TAG_SOP_CLASS_UID);
    if sop_class_uid != RT_STRUCTURE_SET_STORAGE {
        return Err(Error::new(Kind::UnsupportedSopClass, &sop_class_uid));
    }
    let mut set = StructureSet {
        sop_class_uid,
        sop_instance_uid: d::clean_uid(obj, d::TAG_SOP_INSTANCE_UID),
        study_instance_uid: d::clean_uid(obj, d::TAG_STUDY_INSTANCE_UID),
        series_instance_uid: d::clean_uid(obj, d::TAG_SERIES_INSTANCE_UID),
        label: d::clean_string(obj, TAG_STRUCTURE_SET_LABEL),
        series_number: d::int(obj, d::TAG_SERIES_NUMBER),
        date: d::clean_string(obj, TAG_STRUCTURE_SET_DATE),
        time: d::clean_string(obj, TAG_STRUCTURE_SET_TIME),
        ..Default::default()
    };
    let refs = d::sequence(obj, TAG_REFERENCED_FRAME_OF_REFERENCE_SEQ);
    if let Some(first) = refs.first() {
        set.frame_of_reference_uid = read_frame_of_reference_uid(first);
    }
    let referenced_images = read_referenced_image_hierarchy(obj);
    let mut rois = read_rois(obj);
    attach_contours(obj, &mut rois, &referenced_images);
    set.rois = rois.into_values().collect();
    Ok(set)
}
pub fn from_vector_rois(opts: &FromVectorRoisOptions) -> Result<StructureSet, Error> {
    if opts.frame_of_reference_uid.is_empty() {
        return Err(Error::new(
            Kind::MissingReference,
            "missing frame of reference UID",
        ));
    }
    let mut out = StructureSet {
        sop_class_uid: opts.sop_class_uid.clone(),
        sop_instance_uid: opts.sop_instance_uid.clone(),
        study_instance_uid: opts.study_instance_uid.clone(),
        series_instance_uid: opts.series_instance_uid.clone(),
        frame_of_reference_uid: opts.frame_of_reference_uid.clone(),
        label: opts.label.clone(),
        ..Default::default()
    };
    for item in &opts.rois {
        let mut roi_item = Roi {
            number: item.number,
            name: item.name.clone(),
            color_rgb: item.color_rgb,
            ..Default::default()
        };
        if !item.contours.is_empty() && !item.masks.is_empty() {
            return Err(Error::new(
                Kind::GeometryMismatch,
                format!("ROI {} mixes vector contours and raster masks", item.number),
            ));
        }
        for mask in &item.masks {
            roi_item.contours.extend(raster_mask_contours(mask)?);
        }
        for contour in &item.contours {
            validate_slice_geometry(&contour.geometry)?;
            let points = vector_contour_points(&contour.roi)?;
            validate_image_point_bounds(&points, &contour.geometry)?;
            validate_referenced_images(&contour.referenced_images, true)?;
            roi_item.contours.push(Contour {
                geometric_type: CONTOUR_CLOSED_PLANAR.into(),
                points: image_points_to_patient(&points, &contour.geometry),
                referenced_images: contour.referenced_images.clone(),
            });
        }
        out.rois.push(roi_item);
    }
    validate_structure_set(&out)?;
    Ok(out)
}
pub fn rasterize(set: &StructureSet, opts: &RasterizeOptions) -> Result<Segmentation3D, Error> {
    let mut out = Segmentation3D::new(opts.geometry.clone(), opts.columns, opts.rows);
    for item in &set.rois {
        if item.number != opts.roi_number {
            continue;
        }
        let fill = preview::strict_contour_fill(&item.contours)?;
        for contour in &item.contours {
            validate_contour_point_count(contour)?;
            let (slice_index, points) = contour_to_image(contour, &opts.geometry)?;
            fill.combine(out.mask(slice_index), &points);
        }
        return Ok(out);
    }
    Err(Error::new(
        Kind::MissingReference,
        format!("ROI {}", opts.roi_number),
    ))
}
fn vector_contour_points(item: &VectorRoi) -> Result<Vec<Point2D>, Error> {
    let points = item.control_points();
    for p in &points {
        if !p.x.is_finite() || !p.y.is_finite() {
            return Err(Error::new(
                Kind::GeometryMismatch,
                "contour point is not finite",
            ));
        }
    }
    match item.shape {
        RoiShape::Ellipse => ellipse_contour_points(&points),
        RoiShape::Polygon => {
            if points.len() < 3 {
                return Err(Error::new(
                    Kind::GeometryMismatch,
                    "polygon contour needs at least 3 points",
                ));
            }
            if closed_polygon_area(&points) == 0.0 {
                return Err(Error::new(
                    Kind::GeometryMismatch,
                    "polygon contour has zero area",
                ));
            }
            Ok(points)
        }
        _ => rectangle_contour_points(&points),
    }
}
fn corners(points: &[Point2D]) -> (f64, f64, f64, f64) {
    let (a, b) = (points[0], points[1]);
    (
        crate::fmath::min(a.x, b.x),
        crate::fmath::max(a.x, b.x),
        crate::fmath::min(a.y, b.y),
        crate::fmath::max(a.y, b.y),
    )
}
fn rectangle_contour_points(points: &[Point2D]) -> Result<Vec<Point2D>, Error> {
    if points.len() < 2 {
        return Err(Error::new(
            Kind::GeometryMismatch,
            "rectangle contour needs two corners",
        ));
    }
    let (min_x, max_x, min_y, max_y) = corners(points);
    if min_x == max_x || min_y == max_y {
        return Err(Error::new(
            Kind::GeometryMismatch,
            "rectangle contour has zero area",
        ));
    }
    Ok(vec![
        Point2D::new(min_x, min_y),
        Point2D::new(max_x, min_y),
        Point2D::new(max_x, max_y),
        Point2D::new(min_x, max_y),
    ])
}
fn ellipse_contour_points(points: &[Point2D]) -> Result<Vec<Point2D>, Error> {
    if points.len() < 2 {
        return Err(Error::new(
            Kind::GeometryMismatch,
            "ellipse contour needs two bounding corners",
        ));
    }
    let (min_x, max_x, min_y, max_y) = corners(points);
    let rx = (max_x - min_x) / 2.0;
    let ry = (max_y - min_y) / 2.0;
    if rx <= 0.0 || ry <= 0.0 {
        return Err(Error::new(
            Kind::GeometryMismatch,
            "ellipse contour has zero area",
        ));
    }
    let (cx, cy) = (min_x + rx, min_y + ry);
    const STEPS: i64 = 36;
    const TWO_PI: f64 = std::f64::consts::TAU;
    Ok((0..STEPS)
        .map(|i| {
            let t = TWO_PI * i as f64 / STEPS as f64;
            Point2D::new(
                cx + rx * crate::fmath::cos(t),
                cy + ry * crate::fmath::sin(t),
            )
        })
        .collect())
}
fn raster_mask_contours(item: &RasterRoiContour) -> Result<Vec<Contour>, Error> {
    validate_slice_geometry(&item.geometry)?;
    let mask = match &item.mask {
        Some(m) if m.columns == item.geometry.columns && m.rows == item.geometry.rows => m,
        _ => {
            return Err(Error::new(
                Kind::GeometryMismatch,
                format!(
                    "raster mask does not match the {}×{} source image",
                    item.geometry.columns, item.geometry.rows
                ),
            ));
        }
    };
    validate_referenced_images(&item.referenced_images, true)?;
    let iso = mask
        .subpixel_contours(SubpixelContourOptions {
            smooth_sigma: item.smooth_sigma,
        })
        .map_err(|e| Error::new(Kind::GeometryMismatch, e.to_string()))?;
    if iso.is_empty() {
        return Err(Error::new(Kind::GeometryMismatch, "raster mask is empty"));
    }
    Ok(iso
        .into_iter()
        .map(|c| Contour {
            geometric_type: CONTOUR_CLOSED_PLANAR_XOR.into(),
            points: image_points_to_patient(&c.points, &item.geometry),
            referenced_images: item.referenced_images.clone(),
        })
        .collect())
}
fn image_points_to_patient(points: &[Point2D], g: &SliceGeometry) -> Vec<Point3D> {
    points
        .iter()
        .map(|p| {
            let patient = g
                .origin
                .add(g.row_dir.scale(p.x * g.col_spacing))
                .add(g.col_dir.scale(p.y * g.row_spacing));
            Point3D::new(patient.x, patient.y, patient.z)
        })
        .collect()
}
fn validate_slice_geometry(g: &SliceGeometry) -> Result<(), Error> {
    if !preview::valid_projection_geometry(g) || g.rows <= 0 || g.columns <= 0 {
        return Err(Error::new(Kind::GeometryMismatch, "missing slice geometry"));
    }
    Ok(())
}
fn validate_image_point_bounds(points: &[Point2D], g: &SliceGeometry) -> Result<(), Error> {
    for p in points {
        if p.x < -0.5 || p.y < -0.5 || p.x > g.columns as f64 - 0.5 || p.y > g.rows as f64 - 0.5 {
            return Err(Error::new(
                Kind::GeometryMismatch,
                "contour point lies outside the source image",
            ));
        }
    }
    Ok(())
}
pub(crate) fn finite_positive(v: f64) -> bool {
    v > 0.0 && v.is_finite()
}
pub(crate) fn finite_vector(v: Vec3) -> bool {
    let length = v.length();
    finite_vec_components(v) && length.is_finite() && length > 0.0
}
pub(crate) fn finite_vec_components(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}
fn validate_referenced_images(
    refs: &[ReferencedImage],
    require_hierarchy: bool,
) -> Result<(), Error> {
    if refs.is_empty() {
        return Err(Error::new(
            Kind::MissingReference,
            "missing contour image reference",
        ));
    }
    for r in refs {
        if r.sop_class_uid.is_empty() || r.sop_instance_uid.is_empty() {
            return Err(Error::new(
                Kind::MissingReference,
                "missing contour image SOP reference",
            ));
        }
        if require_hierarchy
            && (r.study_instance_uid.is_empty() || r.series_instance_uid.is_empty())
        {
            return Err(Error::new(
                Kind::MissingReference,
                "missing contour image study/series reference",
            ));
        }
    }
    Ok(())
}
fn validate_structure_set(set: &StructureSet) -> Result<(), Error> {
    if !set.rois.is_empty() && set.frame_of_reference_uid.is_empty() {
        return Err(Error::new(
            Kind::MissingReference,
            "missing frame of reference UID",
        ));
    }
    validate_dicom_text("structure set label", &set.label, 16, false)?;
    let mut seen = HashSet::new();
    for roi_item in &set.rois {
        if roi_item.number == 0 {
            return Err(Error::new(Kind::InvalidObject, "missing ROI number"));
        }
        if !seen.insert(roi_item.number) {
            return Err(Error::new(
                Kind::InvalidObject,
                format!("duplicate ROI number {}", roi_item.number),
            ));
        }
        for contour in &roi_item.contours {
            validate_contour_point_count(contour)?;
            for p in &contour.points {
                if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() {
                    return Err(Error::new(
                        Kind::GeometryMismatch,
                        "contour contains a non-finite coordinate",
                    ));
                }
            }
            validate_closed_planar_geometry(contour)?;
            for r in &contour.referenced_images {
                if r.sop_class_uid.is_empty() || r.sop_instance_uid.is_empty() {
                    return Err(Error::new(
                        Kind::MissingReference,
                        "missing contour image SOP reference",
                    ));
                }
            }
        }
        validate_dicom_text("ROI name", &roi_item.name, 64, true)?;
    }
    Ok(())
}
fn sub(a: Point3D, b: Point3D) -> Point3D {
    Point3D::new(a.x - b.x, a.y - b.y, a.z - b.z)
}
fn cross(a: Point3D, b: Point3D) -> Point3D {
    Point3D::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}
fn dot(a: Point3D, b: Point3D) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}
fn point_length(p: Point3D) -> f64 {
    dot(p, p).sqrt()
}
fn validate_closed_planar_geometry(contour: &Contour) -> Result<(), Error> {
    let t = contour.geometric_type.as_str();
    if !t.is_empty() && t != CONTOUR_CLOSED_PLANAR && t != CONTOUR_CLOSED_PLANAR_XOR {
        return Ok(());
    }
    let pts = &contour.points;
    if pts.len() < 3 {
        return Ok(());
    }
    let base = pts[0];
    let mut normal = Point3D::default();
    let mut found_plane = false;
    let mut i = 1;
    while i < pts.len() - 1 && !found_plane {
        for j in i + 1..pts.len() {
            normal = cross(sub(pts[i], base), sub(pts[j], base));
            if point_length(normal) > 1e-12 {
                found_plane = true;
                break;
            }
        }
        i += 1;
    }
    if !found_plane {
        return Err(Error::new(
            Kind::GeometryMismatch,
            "closed planar contour has zero area",
        ));
    }
    let length = point_length(normal);
    if !finite_positive(length) {
        return Err(Error::new(
            Kind::GeometryMismatch,
            "closed planar contour area overflow",
        ));
    }
    for p in pts {
        let distance = dot(sub(*p, base), normal).abs() / length;
        if !distance.is_finite() || distance > 1e-4 {
            return Err(Error::new(
                Kind::GeometryMismatch,
                "closed planar contour points are not coplanar",
            ));
        }
    }
    Ok(())
}
fn validate_dicom_text(
    field: &str,
    value: &str,
    max_bytes: usize,
    required: bool,
) -> Result<(), Error> {
    if required && value.trim().is_empty() {
        return Err(Error::new(
            Kind::InvalidObject,
            format!("{field} is required"),
        ));
    }
    if value.len() > max_bytes {
        return Err(Error::new(
            Kind::InvalidObject,
            format!("{field} exceeds {max_bytes} bytes"),
        ));
    }
    if value.contains('\\') {
        return Err(Error::new(
            Kind::InvalidObject,
            format!("{field} contains an invalid DICOM text value"),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::new(
            Kind::InvalidObject,
            format!("{field} contains a control character"),
        ));
    }
    Ok(())
}
pub(crate) fn validate_contour_point_count(contour: &Contour) -> Result<(), Error> {
    let count = contour.points.len();
    let t = contour.geometric_type.as_str();
    match t {
        CONTOUR_POINT => {
            if count != 1 {
                return Err(Error::new(
                    Kind::GeometryMismatch,
                    format!("POINT contour needs exactly 1 point, got {count}"),
                ));
            }
        }
        CONTOUR_OPEN_PLANAR | CONTOUR_OPEN_NON_PLANAR => {
            if count < 2 {
                return Err(Error::new(
                    Kind::GeometryMismatch,
                    format!("{t} contour needs at least 2 points, got {count}"),
                ));
            }
        }
        "" | CONTOUR_CLOSED_PLANAR | CONTOUR_CLOSED_PLANAR_XOR => {
            if count < 3 {
                return Err(Error::new(
                    Kind::GeometryMismatch,
                    format!("{t} contour needs at least 3 points, got {count}"),
                ));
            }
        }
        _ => {
            if count > 0 && count < 3 {
                return Err(Error::new(
                    Kind::GeometryMismatch,
                    "contour needs at least 3 points",
                ));
            }
        }
    }
    Ok(())
}
fn referenced_frame_of_reference_sequence(set: &StructureSet) -> crate::object::Element {
    let mut elements = vec![d::ui(
        d::TAG_FRAME_OF_REFERENCE_UID,
        &set.frame_of_reference_uid,
    )];
    let study_seq = rt_referenced_study_sequence(set);
    if !study_seq.is_empty() {
        elements.push(d::seq(TAG_RT_REFERENCED_STUDY_SEQUENCE, study_seq));
    }
    d::seq(
        TAG_REFERENCED_FRAME_OF_REFERENCE_SEQ,
        vec![d::data_set(elements)],
    )
}
fn rt_referenced_study_sequence(set: &StructureSet) -> Vec<crate::object::DataSet> {
    let mut studies: BTreeMap<String, BTreeMap<String, Vec<ReferencedImage>>> = BTreeMap::new();
    for r in all_referenced_images(&set.rois) {
        if r.study_instance_uid.is_empty() || r.series_instance_uid.is_empty() {
            continue;
        }
        studies
            .entry(r.study_instance_uid.clone())
            .or_default()
            .entry(r.series_instance_uid.clone())
            .or_default()
            .push(r);
    }
    studies
        .into_iter()
        .map(|(study_uid, series)| {
            let series_items = series
                .into_iter()
                .map(|(series_uid, mut refs)| {
                    refs.sort_by(|a, b| {
                        a.sop_instance_uid
                            .cmp(&b.sop_instance_uid)
                            .then_with(|| a.sop_class_uid.cmp(&b.sop_class_uid))
                    });
                    d::data_set(vec![
                        d::ui(d::TAG_SERIES_INSTANCE_UID, &series_uid),
                        contour_image_sequence(&refs),
                    ])
                })
                .collect();
            d::data_set(vec![
                d::ui(
                    d::TAG_REF_SOP_CLASS_UID,
                    DETACHED_STUDY_MANAGEMENT_SOP_CLASS,
                ),
                d::ui(d::TAG_REF_SOP_INSTANCE_UID, &study_uid),
                d::seq(TAG_RT_REFERENCED_SERIES_SEQUENCE, series_items),
            ])
        })
        .collect()
}
fn all_referenced_images(items: &[Roi]) -> Vec<ReferencedImage> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for roi_item in items {
        for contour in &roi_item.contours {
            for r in &contour.referenced_images {
                if seen.insert(r.clone()) {
                    out.push(r.clone());
                }
            }
        }
    }
    out
}
fn structure_set_roi_sequence(set: &StructureSet) -> crate::object::Element {
    let items = set
        .rois
        .iter()
        .map(|item| {
            d::data_set(vec![
                d::is(TAG_ROI_NUMBER, &[item.number]),
                d::ui(
                    TAG_REFERENCED_FRAME_OF_REFERENCE_UID,
                    &set.frame_of_reference_uid,
                ),
                d::lo(TAG_ROI_NAME, &item.name),
                d::cs(TAG_ROI_GENERATION_ALGORITHM, &item.generation_algorithm),
            ])
        })
        .collect();
    d::seq(TAG_STRUCTURE_SET_ROI_SEQUENCE, items)
}
fn roi_contour_sequence(items: &[Roi]) -> crate::object::Element {
    let sets = items
        .iter()
        .map(|item| {
            d::data_set(vec![
                d::is(TAG_REFERENCED_ROI_NUMBER, &[item.number]),
                d::is(TAG_ROI_DISPLAY_COLOR, &item.color_rgb),
                contour_sequence(&item.contours),
            ])
        })
        .collect();
    d::seq(TAG_ROI_CONTOUR_SEQUENCE, sets)
}
fn contour_sequence(contours: &[Contour]) -> crate::object::Element {
    let sets = contours
        .iter()
        .map(|c| {
            let flat: Vec<f64> = c.points.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
            d::data_set(vec![
                d::cs(TAG_CONTOUR_GEOMETRIC_TYPE, &c.geometric_type),
                d::is(TAG_NUMBER_OF_CONTOUR_POINTS, &[c.points.len() as i64]),
                d::ds(TAG_CONTOUR_DATA, &flat),
                contour_image_sequence(&c.referenced_images),
            ])
        })
        .collect();
    d::seq(TAG_CONTOUR_SEQUENCE, sets)
}
fn contour_image_sequence(refs: &[ReferencedImage]) -> crate::object::Element {
    let sets = refs
        .iter()
        .map(|r| {
            d::data_set(vec![
                d::ui(d::TAG_REF_SOP_CLASS_UID, &r.sop_class_uid),
                d::ui(d::TAG_REF_SOP_INSTANCE_UID, &r.sop_instance_uid),
            ])
        })
        .collect();
    d::seq(TAG_CONTOUR_IMAGE_SEQUENCE, sets)
}
fn rt_roi_observations_sequence(items: &[Roi]) -> crate::object::Element {
    let sets = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            d::data_set(vec![
                d::is(TAG_OBSERVATION_NUMBER, &[i as i64 + 1]),
                d::is(TAG_REFERENCED_ROI_NUMBER, &[item.number]),
                d::cs(TAG_RT_ROI_INTERPRETED_TYPE, "ORGAN"),
                d::str_elem(TAG_ROI_INTERPRETER, Vr::PN, ""),
            ])
        })
        .collect();
    d::seq(TAG_RT_ROI_OBSERVATIONS_SEQUENCE, sets)
}
fn read_rois(obj: &Object) -> BTreeMap<i64, Roi> {
    let mut out = BTreeMap::new();
    for item in d::sequence(obj, TAG_STRUCTURE_SET_ROI_SEQUENCE) {
        let number = d::int(&item, TAG_ROI_NUMBER);
        out.insert(
            number,
            Roi {
                number,
                name: d::clean_string(&item, TAG_ROI_NAME),
                generation_algorithm: d::clean_string(&item, TAG_ROI_GENERATION_ALGORITHM),
                ..Default::default()
            },
        );
    }
    out
}
fn attach_contours(
    obj: &Object,
    out: &mut BTreeMap<i64, Roi>,
    referenced_images: &HashMap<String, ReferencedImage>,
) {
    for item in d::sequence(obj, TAG_ROI_CONTOUR_SEQUENCE) {
        let number = d::int(&item, TAG_REFERENCED_ROI_NUMBER);
        let roi_item = out.entry(number).or_insert_with(|| Roi {
            number,
            ..Default::default()
        });
        if let Ok(values) = item.get_ints(TAG_ROI_DISPLAY_COLOR)
            && values.len() >= 3
        {
            roi_item.color_rgb = [values[0], values[1], values[2]];
        }
        for contour_obj in d::sequence(&item, TAG_CONTOUR_SEQUENCE) {
            roi_item
                .contours
                .push(read_contour(&contour_obj, referenced_images));
        }
    }
}
fn read_contour(obj: &Object, referenced_images: &HashMap<String, ReferencedImage>) -> Contour {
    let mut contour = Contour {
        geometric_type: d::clean_string(obj, TAG_CONTOUR_GEOMETRIC_TYPE),
        ..Default::default()
    };
    let coords = d::floats(Some(obj), TAG_CONTOUR_DATA).unwrap_or_default();
    let mut i = 0;
    while i + 2 < coords.len() {
        contour
            .points
            .push(Point3D::new(coords[i], coords[i + 1], coords[i + 2]));
        i += 3;
    }
    for ref_obj in d::sequence(obj, TAG_CONTOUR_IMAGE_SEQUENCE) {
        let mut r = ReferencedImage {
            sop_class_uid: d::clean_uid(&ref_obj, d::TAG_REF_SOP_CLASS_UID),
            sop_instance_uid: d::clean_uid(&ref_obj, d::TAG_REF_SOP_INSTANCE_UID),
            ..Default::default()
        };
        if let Some(h) = referenced_images.get(&r.sop_instance_uid) {
            r.study_instance_uid = h.study_instance_uid.clone();
            r.series_instance_uid = h.series_instance_uid.clone();
            if r.sop_class_uid.is_empty() {
                r.sop_class_uid = h.sop_class_uid.clone();
            }
        }
        contour.referenced_images.push(r);
    }
    contour
}
fn read_frame_of_reference_uid(obj: &Object) -> String {
    let value = d::clean_uid(obj, d::TAG_FRAME_OF_REFERENCE_UID);
    if !value.is_empty() {
        return value;
    }
    d::clean_uid(obj, TAG_REFERENCED_FRAME_OF_REFERENCE_UID)
}
fn read_referenced_image_hierarchy(obj: &Object) -> HashMap<String, ReferencedImage> {
    let mut out = HashMap::new();
    for frame in d::sequence(obj, TAG_REFERENCED_FRAME_OF_REFERENCE_SEQ) {
        for study_item in d::sequence(&frame, TAG_RT_REFERENCED_STUDY_SEQUENCE) {
            let mut study_uid = d::clean_uid(&study_item, d::TAG_REF_SOP_INSTANCE_UID);
            if study_uid.is_empty() {
                study_uid = d::clean_uid(&study_item, d::TAG_STUDY_INSTANCE_UID);
            }
            for series_item in d::sequence(&study_item, TAG_RT_REFERENCED_SERIES_SEQUENCE) {
                let series_uid = d::clean_uid(&series_item, d::TAG_SERIES_INSTANCE_UID);
                for ref_obj in d::sequence(&series_item, TAG_CONTOUR_IMAGE_SEQUENCE) {
                    let r = ReferencedImage {
                        study_instance_uid: study_uid.clone(),
                        series_instance_uid: series_uid.clone(),
                        sop_class_uid: d::clean_uid(&ref_obj, d::TAG_REF_SOP_CLASS_UID),
                        sop_instance_uid: d::clean_uid(&ref_obj, d::TAG_REF_SOP_INSTANCE_UID),
                    };
                    if !r.sop_instance_uid.is_empty() {
                        out.insert(r.sop_instance_uid.clone(), r);
                    }
                }
            }
        }
    }
    out
}
fn slices(g: &VolumeGeometry) -> &[SliceGeometry] {
    g.slices.as_deref().unwrap_or(&[])
}
fn contour_to_image(
    contour: &Contour,
    geometry: &VolumeGeometry,
) -> Result<(i64, Vec<Point2D>), Error> {
    let list = slices(geometry);
    if list.is_empty() {
        return Err(Error::new(Kind::GeometryMismatch, "empty volume geometry"));
    }
    if contour.points.is_empty() {
        return Err(Error::new(Kind::GeometryMismatch, "empty contour points"));
    }
    for (i, s) in list.iter().enumerate() {
        validate_slice_geometry(s).map_err(|e| e.suffixed(&format!("slice {i}")))?;
    }
    let (slice_index, distance) = nearest_slice(contour.points[0], list);
    let tolerance = contour_plane_tolerance(geometry, slice_index);
    if distance > tolerance {
        return Err(Error::new(
            Kind::GeometryMismatch,
            format!(
                "contour plane is {} mm from nearest slice {slice_index} (tolerance {} mm)",
                g6(distance),
                g6(tolerance)
            ),
        ));
    }
    let points = preview::project_contour(&contour.points, &list[slice_index], tolerance)
        .map_err(|e| e.prefixed(&format!("slice {slice_index}")))?;
    Ok((slice_index as i64, points))
}
fn nearest_slice(point: Point3D, list: &[SliceGeometry]) -> (usize, f64) {
    let p = Vec3::new(point.x, point.y, point.z);
    let mut best = 0;
    let mut best_distance = f64::INFINITY;
    for (i, s) in list.iter().enumerate() {
        let distance = p.sub(s.origin).dot(s.normal.normalize()).abs();
        if distance < best_distance {
            best = i;
            best_distance = distance;
        }
    }
    (best, best_distance)
}
fn contour_plane_tolerance(geometry: &VolumeGeometry, slice_index: usize) -> f64 {
    let tolerances = GeometryTolerances::default_tolerances();
    let mut spacing = local_slice_spacing(geometry, slice_index);
    if !finite_positive(spacing) {
        spacing = geometry.mean_spacing;
    }
    if !finite_positive(spacing) {
        return tolerances.spacing_abs;
    }
    crate::fmath::max(tolerances.spacing_abs, tolerances.spacing_rel * spacing)
}
fn local_slice_spacing(geometry: &VolumeGeometry, slice_index: usize) -> f64 {
    let list = slices(geometry);
    if slice_index >= list.len() {
        return 0.0;
    }
    let normal = list[slice_index].normal.normalize();
    let mut best = f64::INFINITY;
    for neighbor in [slice_index as i64 - 1, slice_index as i64 + 1] {
        if neighbor < 0 || neighbor as usize >= list.len() {
            continue;
        }
        let distance = list[neighbor as usize]
            .origin
            .sub(list[slice_index].origin)
            .dot(normal)
            .abs();
        if finite_positive(distance) && distance < best {
            best = distance;
        }
    }
    if best == f64::INFINITY { 0.0 } else { best }
}
