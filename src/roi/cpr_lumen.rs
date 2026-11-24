#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    clippy::manual_range_contains,
    clippy::needless_range_loop,
    clippy::too_many_arguments,
    clippy::manual_clamp,
    clippy::map_entry
)]
use super::advanced_geometry::Point2D;
use super::marching_squares::iso_contours;
use super::measure_subpixel::closed_polygon_area;
use super::pdqsort::sort_float64s;
use super::polygon_feret::polygon_feret_diameters_context;
use super::{Cancellation, Error, ctx_err};
use crate::codes::{dcm, sct, ucum};
use crate::deid::uid::name_uid;
use crate::fmath;
use crate::render::cpr::{CprPath, CprPathSample};
use crate::render::vec3::Vec3;
use crate::render::volume::Volume;
use crate::sr::code_value::from_code;
use crate::sr::measurement_report::{
    COMPREHENSIVE_3D_SR_STORAGE, GRAPHIC_TYPE_POINT_3D, GRAPHIC_TYPE_POLYGON_3D, MeasurementGroup,
    MeasurementReport, Point3D, ReportMeasurement, SpatialReference, TrackingIdentifier,
};
use crate::sr::{CodedEntry, ImageReference};
use std::sync::Arc;
pub const ERR_CPR_LUMEN_INPUT: &str = "roi: invalid CPR lumen analysis";
pub const ERR_CPR_LUMEN_LIMIT: &str = "roi: CPR lumen analysis resource limit";
pub const ERR_CPR_LUMEN_REFERENCE: &str = "roi: selected lumen reference is not measurable";
pub const MAX_CPR_LUMEN_SECTIONS: i64 = 512;
pub const MAX_CPR_LUMEN_GRID_SIDE: i64 = 513;
pub const MAX_CPR_LUMEN_SAMPLES: i64 = 16_000_000;
pub const MAX_CPR_LUMEN_CONTOUR_POINTS: usize = 1_000_000;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CprLumenOptions {
    pub threshold_min: f64,
    pub threshold_max: f64,
    pub section_step_mm: f64,
    pub pixel_spacing_mm: f64,
    pub radius_mm: f64,
    pub reference_arc_mm: f64,
    pub rotation_degrees: f64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CprLumenSection {
    pub arc_length_mm: f64,
    pub center: Vec3,
    pub normal: Vec3,
    pub binormal: Vec3,
    pub valid: bool,
    pub reason: String,
    pub area_mm2: f64,
    pub min_diameter_mm: f64,
    pub max_diameter_mm: f64,
    pub area_reduction_percent: f64,
    pub diameter_reduction_percent: f64,
    pub contours: Vec<Vec<Point2D>>,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CprLumenProfile {
    pub options: CprLumenOptions,
    pub path_length_mm: f64,
    pub reference_index: i64,
    pub sections: Vec<CprLumenSection>,
}
fn finite(v: f64) -> bool {
    v.is_finite()
}
fn input() -> Error {
    Error::sentinel(ERR_CPR_LUMEN_INPUT)
}
fn limit() -> Error {
    Error::sentinel(ERR_CPR_LUMEN_LIMIT)
}
fn poll(ctx: Option<&dyn Cancellation>) -> Result<(), Error> {
    match ctx_err(ctx) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
fn normalize(
    path: Option<&CprPath>,
    mut o: CprLumenOptions,
) -> Result<(CprLumenOptions, i64, Vec<f64>), Error> {
    let Some(path) = path else {
        return Err(input());
    };
    let length = path.length();
    if !finite(length) || length <= 0.0 {
        return Err(input());
    }
    if o.section_step_mm == 0.0 {
        o.section_step_mm = 2.0;
    }
    if o.pixel_spacing_mm == 0.0 {
        o.pixel_spacing_mm = 0.25;
    }
    if o.radius_mm == 0.0 {
        o.radius_mm = 12.0;
    }
    for v in [
        o.threshold_min,
        o.threshold_max,
        o.section_step_mm,
        o.pixel_spacing_mm,
        o.radius_mm,
        o.reference_arc_mm,
        o.rotation_degrees,
    ] {
        if !finite(v) {
            return Err(input());
        }
    }
    if o.threshold_min >= o.threshold_max
        || o.section_step_mm <= 0.0
        || o.pixel_spacing_mm <= 0.0
        || o.radius_mm <= 0.0
        || o.reference_arc_mm < 0.0
        || o.reference_arc_mm > length
    {
        return Err(input());
    }
    let half = (o.radius_mm / o.pixel_spacing_mm).ceil();
    let count = (length / o.section_step_mm).ceil() + 1.0;
    if half > ((MAX_CPR_LUMEN_GRID_SIDE - 1) / 2) as f64 || count > MAX_CPR_LUMEN_SECTIONS as f64 {
        return Err(limit());
    }
    let side = 2 * half as i64 + 1;
    let count = count as i64;
    let mut positions: Vec<f64> = Vec::with_capacity(count as usize + 1);
    for i in 0..count {
        positions.push(fmath::min(length, i as f64 * o.section_step_mm));
    }
    positions.push(o.reference_arc_mm);
    sort_float64s(&mut positions);
    let mut unique: Vec<f64> = Vec::with_capacity(positions.len());
    for &arc in &positions {
        if unique.last().is_none_or(|&last| (arc - last).abs() > 1e-9) {
            unique.push(arc);
        }
    }
    if unique.len() as i64 > MAX_CPR_LUMEN_SECTIONS
        || side * side * unique.len() as i64 > MAX_CPR_LUMEN_SAMPLES
    {
        return Err(limit());
    }
    Ok((o, side, unique))
}
pub fn analyze_cpr_lumen(
    ctx: Option<&dyn Cancellation>,
    volume: Option<&Arc<Volume>>,
    path: Option<&CprPath>,
    opts: CprLumenOptions,
) -> Result<CprLumenProfile, Error> {
    poll(ctx)?;
    let (opts, side, positions) = normalize(path, opts)?;
    let (Some(volume), Some(path)) = (volume, path) else {
        return Err(input());
    };
    let _lease = volume.acquire_snapshot_context(ctx).map_err(Error::from)?;
    let reader = volume.acquire_reader().map_err(Error::from)?;
    let mut profile = CprLumenProfile {
        options: opts,
        path_length_mm: path.length(),
        reference_index: -1,
        sections: Vec::new(),
    };
    let mut contour_points = 0usize;
    for &arc in &positions {
        poll(ctx)?;
        let section = measure_section(
            ctx,
            &|p| reader.sample_patient(p),
            &path.frame_at(arc),
            &opts,
            side,
        )?;
        for contour in &section.contours {
            contour_points += contour.len();
            if contour_points > MAX_CPR_LUMEN_CONTOUR_POINTS {
                return Err(limit());
            }
        }
        if (arc - opts.reference_arc_mm).abs() <= 1e-9 {
            profile.reference_index = profile.sections.len() as i64;
        }
        profile.sections.push(section);
    }
    if profile.reference_index < 0 || !profile.sections[profile.reference_index as usize].valid {
        return Err(Error::with_message(
            ERR_CPR_LUMEN_REFERENCE,
            format!(
                "{ERR_CPR_LUMEN_REFERENCE} at {:.3} mm",
                opts.reference_arc_mm
            ),
        ));
    }
    let r = profile.sections[profile.reference_index as usize].clone();
    for s in profile.sections.iter_mut() {
        if s.valid {
            s.area_reduction_percent = 100.0 * (1.0 - s.area_mm2 / r.area_mm2);
            s.diameter_reduction_percent = 100.0 * (1.0 - s.min_diameter_mm / r.min_diameter_mm);
            if !finite(s.area_reduction_percent) || !finite(s.diameter_reduction_percent) {
                return Err(input());
            }
        }
    }
    Ok(profile)
}
fn measure_section(
    ctx: Option<&dyn Cancellation>,
    sample: &dyn Fn(Vec3) -> Option<f64>,
    frame: &CprPathSample,
    opts: &CprLumenOptions,
    side: i64,
) -> Result<CprLumenSection, Error> {
    let angle = opts.rotation_degrees * std::f64::consts::PI / 180.0;
    let (cos, sin) = (fmath::cos(angle), fmath::sin(angle));
    let u = frame.normal.scale(cos).add(frame.binormal.scale(sin));
    let v = frame.binormal.scale(cos).sub(frame.normal.scale(sin));
    let mut section = CprLumenSection {
        arc_length_mm: frame.arc_length,
        center: frame.position,
        normal: u,
        binormal: v,
        ..Default::default()
    };
    let n = (side * side) as usize;
    let s = side as usize;
    let mut inside = vec![false; n];
    let mut available = vec![false; n];
    let mut score = vec![0.0; n];
    let half = side / 2;
    for y in 0..side {
        poll(ctx)?;
        for x in 0..side {
            let p = frame
                .position
                .add(u.scale((x - half) as f64 * opts.pixel_spacing_mm))
                .add(v.scale((y - half) as f64 * opts.pixel_spacing_mm));
            let (value, ok) = match sample(p) {
                Some(value) => (value, true),
                None => (0.0, false),
            };
            let index = (y * side + x) as usize;
            available[index] = ok && finite(value);
            inside[index] =
                available[index] && value >= opts.threshold_min && value <= opts.threshold_max;
            score[index] = fmath::min(value - opts.threshold_min, opts.threshold_max - value);
        }
    }
    let seed = (half * side + half) as usize;
    if !inside[seed] {
        section.reason = "centerline seed outside threshold or source".into();
        return Ok(section);
    }
    let mut component = vec![false; n];
    let mut queue: Vec<usize> = Vec::with_capacity(n);
    queue.push(seed);
    component[seed] = true;
    let mut head = 0;
    while head < queue.len() {
        if head & 1023 == 0 {
            poll(ctx)?;
        }
        let index = queue[head];
        let (x, y) = (index % s, index / s);
        if x == 0 || y == 0 || x == s - 1 || y == s - 1 {
            section.reason = "lumen reaches analysis boundary".into();
            return Ok(section);
        }
        for neighbor in [index - 1, index + 1, index - s, index + s] {
            if !available[neighbor] {
                section.reason = "lumen reaches source FOV boundary".into();
                return Ok(section);
            }
            if inside[neighbor] && !component[neighbor] {
                component[neighbor] = true;
                queue.push(neighbor);
            }
        }
        head += 1;
    }
    let contours = iso_contours(
        side,
        side,
        |x, y| {
            let index = (y * side + x) as usize;
            if component[index] {
                return score[index];
            }
            if !available[index] {
                return -1.0;
            }
            -fmath::max(score[index].abs(), 1e-9)
        },
        0.0,
    )?;
    let mut outer: Vec<Point2D> = Vec::new();
    let mut largest_area = 0.0;
    for contour in contours {
        let physical: Vec<Point2D> = contour
            .points
            .iter()
            .map(|p| Point2D {
                x: (p.x - half as f64) * opts.pixel_spacing_mm,
                y: (p.y - half as f64) * opts.pixel_spacing_mm,
            })
            .collect();
        let area = closed_polygon_area(&physical);
        section.area_mm2 += area;
        if area > largest_area {
            largest_area = area;
            outer = physical.clone();
        }
        section.contours.push(physical);
    }
    if !(section.area_mm2 > 0.0) || outer.len() < 3 {
        section.reason = "no closed lumen contour".into();
        return Ok(section);
    }
    let (min, max) = polygon_feret_diameters_context(ctx, &outer)?;
    section.min_diameter_mm = min;
    section.max_diameter_mm = max;
    section.valid = true;
    Ok(section)
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CprLumenReportOptions {
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub frame_of_reference_uid: String,
    pub content_date: String,
    pub content_time: String,
    pub image_library: Option<Vec<ImageReference>>,
    pub threshold_units: CodedEntry,
}
fn private_code(value: &str, meaning: &str) -> CodedEntry {
    CodedEntry::new(value, "99DCMRUST", meaning)
}
fn measurement(
    concept: CodedEntry,
    value: f64,
    units: CodedEntry,
    spatial: Option<&SpatialReference>,
) -> ReportMeasurement {
    ReportMeasurement {
        concept_name: concept,
        value,
        units,
        spatial: spatial.cloned().unwrap_or_default(),
        ..Default::default()
    }
}
impl CprLumenProfile {
    pub fn measurement_report(
        profile: Option<&CprLumenProfile>,
        opts: &CprLumenReportOptions,
    ) -> Result<MeasurementReport, Error> {
        let reference = Error::sentinel(ERR_CPR_LUMEN_REFERENCE);
        let Some(profile) = profile else {
            return Err(reference);
        };
        let ri = profile.reference_index;
        if ri < 0
            || ri >= profile.sections.len() as i64
            || !profile.sections[ri as usize].valid
            || opts.frame_of_reference_uid.is_empty()
        {
            return Err(reference);
        }
        let r = &profile.sections[ri as usize];
        if !(r.area_mm2 > 0.0)
            || !(r.min_diameter_mm > 0.0)
            || !finite(r.area_mm2)
            || !finite(r.min_diameter_mm)
        {
            return Err(reference);
        }
        let mm = from_code(ucum::Mm);
        let mm2 = from_code(ucum::SquareMillimeter);
        let pct = from_code(ucum::Percent);
        let none = from_code(ucum::NoUnits);
        let threshold_units = if opts.threshold_units.code_value.is_empty() {
            none.clone()
        } else {
            opts.threshold_units.clone()
        };
        let finding = || private_code("CPRLumen", "Threshold-connected transverse lumen");
        let mut metadata = MeasurementGroup {
            tracking: TrackingIdentifier {
                uid: name_uid(&[
                    "dcm-rust/roi/cpr-lumen",
                    &opts.sop_instance_uid,
                    "parameters",
                ]),
                identifier: "CPR lumen analysis parameters".into(),
            },
            finding: finding(),
            ..Default::default()
        };
        let o = &profile.options;
        for m in [
            measurement(
                private_code("Lower", "Lower source-modality threshold"),
                o.threshold_min,
                threshold_units.clone(),
                None,
            ),
            measurement(
                private_code("Upper", "Upper source-modality threshold"),
                o.threshold_max,
                threshold_units.clone(),
                None,
            ),
            measurement(
                private_code("ReferenceArc", "Explicit reference arc length"),
                r.arc_length_mm,
                mm.clone(),
                None,
            ),
            measurement(
                private_code("ReferenceArea", "Reference lumen area"),
                r.area_mm2,
                mm2.clone(),
                None,
            ),
            measurement(
                private_code("ReferenceMin", "Reference minimum Feret diameter"),
                r.min_diameter_mm,
                mm.clone(),
                None,
            ),
            measurement(
                private_code("PixelSpacing", "Transverse analysis pixel spacing"),
                o.pixel_spacing_mm,
                mm.clone(),
                None,
            ),
            measurement(
                private_code("SectionStep", "Requested arc sampling interval"),
                o.section_step_mm,
                mm.clone(),
                None,
            ),
            measurement(
                private_code("Radius", "Transverse analysis radius"),
                o.radius_mm,
                mm.clone(),
                None,
            ),
            measurement(
                private_code("Rotation", "Transverse basis rotation"),
                o.rotation_degrees,
                from_code(ucum::Degree),
                None,
            ),
        ] {
            if !finite(m.value) {
                return Err(input());
            }
            metadata.measurements.push(m);
        }
        let mut report = MeasurementReport {
            sop_class_uid: COMPREHENSIVE_3D_SR_STORAGE.into(),
            sop_instance_uid: opts.sop_instance_uid.clone(),
            study_instance_uid: opts.study_instance_uid.clone(),
            series_instance_uid: opts.series_instance_uid.clone(),
            frame_of_reference_uid: opts.frame_of_reference_uid.clone(),
            content_date: opts.content_date.clone(),
            content_time: opts.content_time.clone(),
            procedures_reported: vec![private_code("CPRAnalysis", "CPR threshold lumen analysis")],
            image_library: opts.image_library.clone(),
            groups: vec![metadata],
            ..Default::default()
        };
        for (i, section) in profile.sections.iter().enumerate() {
            if !finite(section.arc_length_mm) {
                return Err(input());
            }
            let point = SpatialReference {
                frame_of_reference_uid: opts.frame_of_reference_uid.clone(),
                graphic_type: GRAPHIC_TYPE_POINT_3D.into(),
                coordinates: vec![Point3D {
                    x: section.center.x,
                    y: section.center.y,
                    z: section.center.z,
                }],
                ..Default::default()
            };
            let valid = if section.valid { 1.0 } else { 0.0 };
            let index = i.to_string();
            let mut group = MeasurementGroup {
                tracking: TrackingIdentifier {
                    uid: name_uid(&["dcm-rust/roi/cpr-lumen", &opts.sop_instance_uid, &index]),
                    identifier: format!("CPR lumen at {:.3} mm", section.arc_length_mm),
                },
                finding: finding(),
                measurements: vec![
                    measurement(
                        private_code("ArcLength", "Arc length from path start"),
                        section.arc_length_mm,
                        mm.clone(),
                        Some(&point),
                    ),
                    measurement(
                        private_code("Valid", "Lumen section measurable"),
                        valid,
                        none.clone(),
                        None,
                    ),
                ],
                ..Default::default()
            };
            if section.valid {
                if !(section.area_mm2 > 0.0)
                    || !(section.min_diameter_mm > 0.0)
                    || section.max_diameter_mm < section.min_diameter_mm
                {
                    return Err(input());
                }
                for v in [
                    section.area_mm2,
                    section.min_diameter_mm,
                    section.max_diameter_mm,
                ] {
                    if !finite(v) {
                        return Err(input());
                    }
                }
                let mut area_geometry = point.clone();
                for contour in &section.contours {
                    if closed_polygon_area(contour) > 0.0 {
                        area_geometry = SpatialReference {
                            frame_of_reference_uid: opts.frame_of_reference_uid.clone(),
                            graphic_type: GRAPHIC_TYPE_POLYGON_3D.into(),
                            ..Default::default()
                        };
                        for p in contour {
                            let patient = section
                                .center
                                .add(section.normal.scale(p.x))
                                .add(section.binormal.scale(p.y));
                            area_geometry.coordinates.push(Point3D {
                                x: patient.x,
                                y: patient.y,
                                z: patient.z,
                            });
                        }
                        if let Some(&first) = area_geometry.coordinates.first() {
                            area_geometry.coordinates.push(first);
                        }
                        break;
                    }
                }
                let with_derivation = |mut m: ReportMeasurement, d: CodedEntry| {
                    m.derivation = d;
                    m
                };
                group.measurements.extend([
                    measurement(
                        from_code(dcm::VesselLumenCrossSectionalArea),
                        section.area_mm2,
                        mm2.clone(),
                        Some(&area_geometry),
                    ),
                    with_derivation(
                        measurement(
                            from_code(sct::VesselLumenDiameter),
                            section.min_diameter_mm,
                            mm.clone(),
                            Some(&point),
                        ),
                        from_code(sct::Minimum),
                    ),
                    with_derivation(
                        measurement(
                            from_code(sct::VesselLumenDiameter),
                            section.max_diameter_mm,
                            mm.clone(),
                            Some(&point),
                        ),
                        from_code(sct::Maximum),
                    ),
                    measurement(
                        from_code(sct::LumenAreaStenosis),
                        100.0 * (1.0 - section.area_mm2 / r.area_mm2),
                        pct.clone(),
                        Some(&point),
                    ),
                    measurement(
                        from_code(sct::LumenDiameterStenosis),
                        100.0 * (1.0 - section.min_diameter_mm / r.min_diameter_mm),
                        pct.clone(),
                        Some(&point),
                    ),
                ]);
                for contour in &section.contours {
                    let area = closed_polygon_area(contour);
                    if area < 0.0 {
                        group.measurements.push(measurement(
                            private_code("ExcludedArea", "Excluded lumen hole area"),
                            area.abs(),
                            mm2.clone(),
                            None,
                        ));
                    }
                }
            }
            report.groups.push(group);
        }
        Ok(report)
    }
}
