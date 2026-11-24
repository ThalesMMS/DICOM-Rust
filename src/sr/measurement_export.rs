use super::code_value::from_context_group;
use super::measurement_report::{
    COMPREHENSIVE_3D_SR_STORAGE, GRAPHIC_TYPE_MULTIPOINT_3D, GRAPHIC_TYPE_POINT_3D,
    GRAPHIC_TYPE_POLYGON_3D, GRAPHIC_TYPE_POLYLINE_3D, MeasurementGroup, MeasurementReport,
    Point3D, ReportMeasurement, SpatialReference, TrackingIdentifier, has_spatial_reference,
};
use super::{CodedEntry, ImageReference, Kind, SrError, from_code};
use crate::codes::{dcm, sct, ucum};
pub const MEASUREMENT_KIND_LENGTH: &str = "length";
pub const MEASUREMENT_KIND_ANGLE: &str = "angle";
pub const MEASUREMENT_KIND_AREA: &str = "area";
pub const MEASUREMENT_KIND_LONG_AXIS: &str = "long_axis";
pub const MEASUREMENT_KIND_SHORT_AXIS: &str = "short_axis";
pub const MEASUREMENT_KIND_REAL_WORLD_VALUE: &str = "real_world_value";
const CID_GENERAL_ROI_MEASUREMENT_MODIFIER: i64 = 7464;
const CID_LINEAR_MEASUREMENT: i64 = 7470;
const CID_AREA_MEASUREMENT: i64 = 7471;
pub fn mean_derivation() -> CodedEntry {
    from_context_group(
        CID_GENERAL_ROI_MEASUREMENT_MODIFIER,
        sct::MeanValueOfPopulation,
    )
}
pub fn minimum_derivation() -> CodedEntry {
    from_context_group(CID_GENERAL_ROI_MEASUREMENT_MODIFIER, sct::Minimum)
}
pub fn maximum_derivation() -> CodedEntry {
    from_context_group(CID_GENERAL_ROI_MEASUREMENT_MODIFIER, sct::Maximum)
}
pub fn standard_deviation_derivation() -> CodedEntry {
    from_context_group(CID_GENERAL_ROI_MEASUREMENT_MODIFIER, sct::StandardDeviation)
}
pub fn long_axis_concept() -> CodedEntry {
    from_context_group(CID_LINEAR_MEASUREMENT, sct::LongAxis)
}
pub fn short_axis_concept() -> CodedEntry {
    from_context_group(CID_LINEAR_MEASUREMENT, sct::ShortAxis)
}
pub fn millimeter_unit() -> CodedEntry {
    from_code(ucum::Mm)
}
pub fn degree_unit() -> CodedEntry {
    from_code(ucum::Degree)
}
pub fn square_millimeter_unit() -> CodedEntry {
    from_code(ucum::SquareMillimeter)
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewerMeasurement {
    pub tracking: TrackingIdentifier,
    pub kind: String,
    pub label: String,
    pub value: f64,
    pub concept_name: CodedEntry,
    pub units: CodedEntry,
    pub derivation: CodedEntry,
    pub source_image: ImageReference,
    pub spatial: SpatialReference,
    pub group_measurements: Vec<ViewerMeasurement>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasurementExportOptions {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub content_date: String,
    pub content_time: String,
    pub title: CodedEntry,
    pub measurements: Vec<ViewerMeasurement>,
}
fn missing(suffix: &str) -> SrError {
    SrError::new(Kind::MissingReference, suffix)
}
fn invalid(suffix: &str) -> SrError {
    SrError::new(Kind::InvalidMeasurementReport, suffix)
}
pub fn from_measurements(opts: &MeasurementExportOptions) -> Result<MeasurementReport, SrError> {
    if opts.sop_instance_uid.trim().is_empty() {
        return Err(invalid(": SOP Instance UID is required"));
    }
    if opts.study_instance_uid.trim().is_empty() || opts.series_instance_uid.trim().is_empty() {
        return Err(missing(": study and series instance UIDs are required"));
    }
    let mut report = MeasurementReport {
        sop_class_uid: first_non_empty(&[&opts.sop_class_uid, COMPREHENSIVE_3D_SR_STORAGE]),
        sop_instance_uid: opts.sop_instance_uid.trim().into(),
        study_instance_uid: opts.study_instance_uid.trim().into(),
        series_instance_uid: opts.series_instance_uid.trim().into(),
        frame_of_reference_uid: opts.frame_of_reference_uid.trim().into(),
        content_date: opts.content_date.trim().into(),
        content_time: opts.content_time.trim().into(),
        title: opts.title.clone(),
        ..Default::default()
    };
    for (index, m) in opts.measurements.iter().enumerate() {
        let group = group_from_viewer(&report.sop_instance_uid, index, m)?;
        report.groups.push(group);
    }
    Ok(report)
}
fn group_from_viewer(
    report_uid: &str,
    index: usize,
    m: &ViewerMeasurement,
) -> Result<MeasurementGroup, SrError> {
    let mut group = MeasurementGroup {
        tracking: tracking_for(report_uid, index, m),
        ..Default::default()
    };
    let parts = std::iter::once(m).chain(m.group_measurements.iter());
    for (part_index, part) in parts.enumerate() {
        if part_index > 0 && !part.group_measurements.is_empty() {
            return Err(invalid(": group measurements cannot nest"));
        }
        validate_source_image(&part.source_image)?;
        validate_spatial(&part.spatial)?;
        let (concept, units) = concept_and_units(part)?;
        group.measurements.push(ReportMeasurement {
            concept_name: concept,
            value: part.value,
            units,
            derivation: part.derivation.clone(),
            image: normalized_image(&part.source_image),
            spatial: normalized_spatial(&part.spatial),
        });
    }
    Ok(group)
}
fn concept_and_units(m: &ViewerMeasurement) -> Result<(CodedEntry, CodedEntry), SrError> {
    let Some((mut concept, mut units)) = default_concept_and_units(&m.kind) else {
        return Err(invalid(&format!(
            ": unsupported measurement kind {}",
            crate::textfmt::quote(&m.kind)
        )));
    };
    if !m.concept_name.empty() {
        concept = m.concept_name.clone();
    }
    if !m.units.empty() {
        units = m.units.clone();
    }
    if units.empty() {
        return Err(invalid(&format!(
            ": measurement kind {} needs units",
            crate::textfmt::quote(&m.kind)
        )));
    }
    Ok((concept, units))
}
fn default_concept_and_units(kind: &str) -> Option<(CodedEntry, CodedEntry)> {
    Some(match kind {
        MEASUREMENT_KIND_LENGTH => (
            from_context_group(CID_LINEAR_MEASUREMENT, dcm::Distance),
            millimeter_unit(),
        ),
        MEASUREMENT_KIND_ANGLE => (from_code(sct::Angle), degree_unit()),
        MEASUREMENT_KIND_AREA => (
            from_context_group(CID_AREA_MEASUREMENT, sct::Area),
            square_millimeter_unit(),
        ),
        MEASUREMENT_KIND_LONG_AXIS => (long_axis_concept(), millimeter_unit()),
        MEASUREMENT_KIND_SHORT_AXIS => (short_axis_concept(), millimeter_unit()),
        MEASUREMENT_KIND_REAL_WORLD_VALUE => (from_code(sct::Quantity), CodedEntry::default()),
        _ => return None,
    })
}
fn validate_source_image(r: &ImageReference) -> Result<(), SrError> {
    if r.sop_class_uid.trim().is_empty() || r.sop_instance_uid.trim().is_empty() {
        return Err(missing(
            ": source image SOP class and instance UIDs are required",
        ));
    }
    if r.frames.iter().any(|&f| f <= 0) {
        return Err(missing(": referenced frame numbers must be one-based"));
    }
    Ok(())
}
fn validate_spatial(r: &SpatialReference) -> Result<(), SrError> {
    if !has_spatial_reference(r) {
        return Ok(());
    }
    if r.frame_of_reference_uid.trim().is_empty() {
        return Err(missing(
            ": SCOORD3D requires referenced frame of reference UID",
        ));
    }
    if r.graphic_type.trim().is_empty() || r.coordinates.is_empty() {
        return Err(missing(": SCOORD3D requires graphic type and coordinates"));
    }
    if !valid_spatial_point_count(r.graphic_type.trim(), &r.coordinates) {
        return Err(invalid(&format!(
            ": SCOORD3D {} cannot have {} points",
            r.graphic_type,
            r.coordinates.len()
        )));
    }
    Ok(())
}
fn valid_spatial_point_count(graphic_type: &str, points: &[Point3D]) -> bool {
    match graphic_type {
        GRAPHIC_TYPE_POINT_3D => points.len() == 1,
        GRAPHIC_TYPE_POLYLINE_3D | GRAPHIC_TYPE_MULTIPOINT_3D => !points.is_empty(),
        GRAPHIC_TYPE_POLYGON_3D => points.len() >= 4 && points[0] == points[points.len() - 1],
        _ => true,
    }
}
pub fn spatial_points(r: &SpatialReference) -> Option<Vec<Point3D>> {
    let mut points = r.coordinates.as_slice();
    if points.is_empty() {
        return None;
    }
    match r.graphic_type.trim() {
        GRAPHIC_TYPE_POINT_3D | GRAPHIC_TYPE_MULTIPOINT_3D | GRAPHIC_TYPE_POLYLINE_3D => {}
        GRAPHIC_TYPE_POLYGON_3D => {
            if points.len() > 1 && points[0] == points[points.len() - 1] {
                points = &points[..points.len() - 1];
            }
        }
        _ => return None,
    }
    Some(points.to_vec())
}
fn tracking_for(report_uid: &str, index: usize, m: &ViewerMeasurement) -> TrackingIdentifier {
    let mut t = m.tracking.clone();
    t.uid = t.uid.trim().into();
    t.identifier = first_non_empty(&[&t.identifier, &m.label, &default_identifier(&m.kind, index)]);
    if t.uid.is_empty() {
        t.uid = derived_tracking_uid(report_uid.trim(), index + 1);
    }
    t
}
fn derived_tracking_uid(report_uid: &str, ordinal: usize) -> String {
    let candidate = format!("{report_uid}.{ordinal}");
    if crate::object::is_valid_uid(candidate.as_bytes()) {
        return candidate;
    }
    crate::deid::uid::name_uid(&["dcm-rust/sr tracking", report_uid, &ordinal.to_string()])
}
fn default_identifier(kind: &str, index: usize) -> String {
    if kind.is_empty() {
        return format!("Measurement {}", index + 1);
    }
    format!("Measurement {} {kind}", index + 1)
}
fn normalized_image(r: &ImageReference) -> ImageReference {
    ImageReference {
        study_instance_uid: r.study_instance_uid.trim().into(),
        series_instance_uid: r.series_instance_uid.trim().into(),
        sop_class_uid: r.sop_class_uid.trim().into(),
        sop_instance_uid: r.sop_instance_uid.trim().into(),
        frames: r.frames.clone(),
    }
}
fn normalized_spatial(r: &SpatialReference) -> SpatialReference {
    SpatialReference {
        frame_of_reference_uid: r.frame_of_reference_uid.trim().into(),
        graphic_type: r.graphic_type.trim().into(),
        fiducial_uid: r.fiducial_uid.trim().into(),
        coordinates: r.coordinates.clone(),
    }
}
fn first_non_empty(values: &[&str]) -> String {
    values
        .iter()
        .map(|v| v.trim())
        .find(|v| !v.is_empty())
        .unwrap_or_default()
        .to_string()
}
