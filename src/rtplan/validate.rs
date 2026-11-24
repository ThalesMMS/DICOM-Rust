use super::read::{POSITION_TOLERANCE, Reader, g, item, join};
use super::tags::*;
use super::*;
use crate::fmath;
use crate::object::{Tag, is_valid_uid};
use std::collections::HashSet;
#[allow(clippy::manual_range_contains)]
pub(crate) fn validate_plan(r: &mut Reader, plan: &Plan) {
    if plan.sop_instance_uid.is_empty()
        || plan.study_instance_uid.is_empty()
        || plan.series_instance_uid.is_empty()
    {
        r.fail(
            "",
            "SOP Instance, Study Instance or Series Instance UID is missing",
        );
    }
    for (t, s) in [
        (tagSOPInstanceUID, &plan.sop_instance_uid),
        (tagStudyInstanceUID, &plan.study_instance_uid),
        (tagSeriesInstanceUID, &plan.series_instance_uid),
    ] {
        if !s.is_empty() && !is_valid_uid(s.as_bytes()) {
            r.fail(t.to_string(), "identity UID syntax is invalid");
        }
    }
    if !plan.frame_of_reference_uid.is_empty()
        && !is_valid_uid(plan.frame_of_reference_uid.as_bytes())
    {
        r.fail(
            tagFrameOfReferenceUID.to_string(),
            "frame UID syntax is invalid",
        );
    }
    if plan.label.is_empty() {
        r.fail(tagRTPlanLabel.to_string(), "RT Plan Label is missing");
    }
    match plan.geometry.as_str() {
        GEOMETRY_PATIENT => {
            if plan.referenced_structure_set.is_none() {
                r.fail(
                    tagReferencedStructureSetSequence.to_string(),
                    "is required when RT Plan Geometry is PATIENT",
                );
            }
        }
        GEOMETRY_TREATMENT_DEVICE => {}
        "" => r.fail(tagRTPlanGeometry.to_string(), "RT Plan Geometry is missing"),
        _ => r.fail(
            tagRTPlanGeometry.to_string(),
            "RT Plan Geometry is neither PATIENT nor TREATMENT_DEVICE",
        ),
    }
    if let Some(reference) = &plan.referenced_structure_set
        && (reference.sop_class_uid.is_empty() || reference.sop_instance_uid.is_empty())
    {
        r.fail(
            format!("{tagReferencedStructureSetSequence}[0]"),
            "referenced SOP Class or Instance UID is missing",
        );
    }
    if !plan.approval_status.is_empty()
        && !one_of(
            &plan.approval_status,
            &["APPROVED", "UNAPPROVED", "REJECTED"],
        )
    {
        r.warn(
            tagApprovalStatus.to_string(),
            "Approval Status is not an enumerated value",
        );
    }
    if !plan.intent.is_empty()
        && !one_of(
            &plan.intent,
            &[
                "CURATIVE",
                "PALLIATIVE",
                "PROPHYLACTIC",
                "VERIFICATION",
                "MACHINE_QA",
                "RESEARCH",
                "SERVICE",
            ],
        )
    {
        r.warn(
            tagPlanIntent.to_string(),
            "Plan Intent is not a defined term",
        );
    }
    let mut dose_refs: HashSet<i64> = HashSet::new();
    for (i, reference) in plan.dose_references.iter().enumerate() {
        let path = item("", tagDoseReferenceSequence, i);
        if dose_refs.contains(&reference.number) {
            r.fail(
                join(&path, tagDoseReferenceNumber),
                format!("Dose Reference Number {} is not unique", reference.number),
            );
        }
        dose_refs.insert(reference.number);
        match reference.structure_type.as_str() {
            "POINT" | "VOLUME" => {
                if reference.referenced_roi_number.is_none() {
                    r.fail(
                        join(&path, tagReferencedROINumber),
                        format!(
                            "is required for a {} dose reference",
                            reference.structure_type
                        ),
                    );
                }
            }
            "COORDINATES" => {
                if reference.point_coordinates.len() != 3 {
                    r.fail(
                        join(&path, tagDoseReferencePointCoords),
                        "needs 3 values for a COORDINATES dose reference",
                    );
                }
            }
            "SITE" => {}
            "" => r.fail(
                join(&path, tagDoseReferenceStructureType),
                "Dose Reference Structure Type is missing",
            ),
            _ => r.warn(
                join(&path, tagDoseReferenceStructureType),
                "Dose Reference Structure Type is not a defined term",
            ),
        }
        match reference.typ.as_str() {
            "TARGET" | "ORGAN_AT_RISK" => {}
            "" => r.fail(
                join(&path, tagDoseReferenceType),
                "Dose Reference Type is missing",
            ),
            _ => r.fail(
                join(&path, tagDoseReferenceType),
                "Dose Reference Type is neither TARGET nor ORGAN_AT_RISK",
            ),
        }
    }
    let mut tolerances: HashSet<i64> = HashSet::new();
    for (i, table) in plan.tolerance_tables.iter().enumerate() {
        if tolerances.contains(&table.number) {
            r.fail(
                join(
                    &item("", tagToleranceTableSequence, i),
                    tagToleranceTableNumber,
                ),
                format!("Tolerance Table Number {} is not unique", table.number),
            );
        }
        tolerances.insert(table.number);
    }
    let mut setups: HashSet<i64> = HashSet::new();
    for (i, setup) in plan.patient_setups.iter().enumerate() {
        let path = item("", tagPatientSetupSequence, i);
        if setups.contains(&setup.number) {
            r.fail(
                join(&path, tagPatientSetupNumber),
                format!("Patient Setup Number {} is not unique", setup.number),
            );
        }
        setups.insert(setup.number);
        if setup.patient_position.is_empty() && setup.patient_additional_position.is_empty() {
            r.warn(
                join(&path, tagPatientPosition),
                "Patient Position is required when Patient Additional Position is absent",
            );
        }
    }
    let mut beams: HashSet<i64> = HashSet::new();
    for (i, beam) in plan.beams.iter().enumerate() {
        let path = item("", tagBeamSequence, i);
        if beams.contains(&beam.number) {
            r.fail(
                join(&path, tagBeamNumber),
                format!("Beam Number {} is not unique", beam.number),
            );
        }
        beams.insert(beam.number);
        validate_beam(r, plan, beam, &path, &setups, &tolerances, &dose_refs);
    }
    for (i, b) in plan.ion_beams.iter().enumerate() {
        let path = item("", ft(0x300a03a2), i);
        if beams.contains(&b.number) {
            r.fail(join(&path, tagBeamNumber), "beam number is duplicated");
        }
        beams.insert(b.number);
        if let Some(n) = b.referenced_patient_setup_number
            && !setups.contains(&n)
        {
            r.fail(
                join(&path, tagReferencedPatientSetupNumber),
                "patient setup reference is missing",
            );
        }
        if let Some(n) = b.referenced_tolerance_table_number
            && !tolerances.contains(&n)
        {
            r.fail(
                join(&path, tagReferencedToleranceTableNumber),
                "tolerance reference is missing",
            );
        }
        for (j, icp) in b.control_points.iter().enumerate() {
            let cp = &icp.control_point;
            let cp_path = item(&path, ft(0x300a03a8), j);
            for (tag, value) in [
                (tagGantryAngle, cp.gantry_angle),
                (tagBLDAngle, cp.beam_limiting_device_angle),
                (tagPatientSupportAngle, cp.patient_support_angle),
            ] {
                if value < 0.0 || value >= 360.0 {
                    r.fail(join(&cp_path, tag), "angle is outside [0, 360)");
                }
            }
            let mut seen: HashSet<i64> = HashSet::new();
            for (k, reference) in cp.dose_references.iter().enumerate() {
                if seen.contains(&reference.number) || !dose_refs.contains(&reference.number) {
                    r.fail(
                        join(
                            &item(&cp_path, tagReferencedDoseReferenceSeq, k),
                            tagReferencedDoseReferenceNumber,
                        ),
                        "dose reference is missing or duplicated",
                    );
                }
                seen.insert(reference.number);
            }
        }
    }
    let mut brachy: HashSet<i64> = HashSet::new();
    for (i, setup) in plan.brachy_setups.iter().enumerate() {
        if brachy.contains(&setup.number) {
            r.fail(
                join(
                    &item("", tagApplicationSetupSequence, i),
                    tagApplicationSetupNumber,
                ),
                format!("Application Setup Number {} is not unique", setup.number),
            );
        }
        brachy.insert(setup.number);
    }
    if (!plan.beams.is_empty() || !plan.ion_beams.is_empty()) && !plan.brachy_setups.is_empty() {
        r.fail(
            "",
            "RT Beams and RT Brachy Application Setups shall not both be present",
        );
    }
    let mut groups: HashSet<i64> = HashSet::new();
    for (i, group) in plan.fraction_groups.iter().enumerate() {
        let path = item("", tagFractionGroupSequence, i);
        if groups.contains(&group.number) {
            r.fail(
                join(&path, tagFractionGroupNumber),
                format!("Fraction Group Number {} is not unique", group.number),
            );
        }
        groups.insert(group.number);
        if group.fractions_planned.is_some_and(|n| n < 0) {
            r.fail(join(&path, tagNumberOfFractionsPlanned), "is negative");
        }
        if group.number_of_beams < 0 || group.number_of_brachy_setups < 0 {
            r.fail(
                path.clone(),
                "Number of Beams or Number of Brachy Application Setups is negative",
            );
        }
        if group.number_of_beams > 0 && group.number_of_brachy_setups > 0 {
            r.fail(
                path.clone(),
                "fraction group has both beams and brachy application setups",
            );
        }
        if group.referenced_beams.len() as i64 != group.number_of_beams.max(0) {
            r.fail(
                join(&path, tagReferencedBeamSequence),
                format!(
                    "has {} items, Number of Beams is {}",
                    group.referenced_beams.len(),
                    group.number_of_beams
                ),
            );
        }
        let mut seen: HashSet<i64> = HashSet::new();
        for (j, reference) in group.referenced_beams.iter().enumerate() {
            let ref_path = item(&path, tagReferencedBeamSequence, j);
            if seen.contains(&reference.beam_number) {
                r.fail(
                    join(&ref_path, tagReferencedBeamNumber),
                    format!("beam {} is referenced twice", reference.beam_number),
                );
            }
            seen.insert(reference.beam_number);
            if !beams.contains(&reference.beam_number) {
                r.fail(
                    join(&ref_path, tagReferencedBeamNumber),
                    format!("beam {} is not in the Beam Sequence", reference.beam_number),
                );
            }
            if reference.meterset.is_some_and(|v| v < 0.0) {
                r.fail(join(&ref_path, tagBeamMeterset), "is negative");
            }
            if reference.dose.is_some_and(|v| v < 0.0) {
                r.fail(join(&ref_path, tagBeamDose), "is negative");
            }
        }
        if group.referenced_brachy_setups.len() as i64 != group.number_of_brachy_setups.max(0) {
            r.fail(
                join(&path, tagReferencedBrachySetupSequence),
                format!(
                    "has {} items, Number of Brachy Application Setups is {}",
                    group.referenced_brachy_setups.len(),
                    group.number_of_brachy_setups
                ),
            );
        }
        for (j, reference) in group.referenced_brachy_setups.iter().enumerate() {
            if !brachy.contains(&reference.setup_number) {
                r.fail(
                    join(
                        &item(&path, tagReferencedBrachySetupSequence, j),
                        tagReferencedBrachySetupNumber,
                    ),
                    format!(
                        "application setup {} is not in the Application Setup Sequence",
                        reference.setup_number
                    ),
                );
            }
        }
        for (j, reference) in group.referenced_dose_reference.iter().enumerate() {
            if !dose_refs.contains(&reference.number) {
                r.fail(
                    join(
                        &item(&path, tagReferencedDoseReferenceSeq, j),
                        tagReferencedDoseReferenceNumber,
                    ),
                    format!(
                        "dose reference {} is not in the Dose Reference Sequence",
                        reference.number
                    ),
                );
            }
        }
    }
}
#[allow(clippy::too_many_arguments, clippy::manual_range_contains)]
fn validate_beam(
    r: &mut Reader,
    plan: &Plan,
    beam: &Beam,
    path: &str,
    setups: &HashSet<i64>,
    tolerances: &HashSet<i64>,
    dose_refs: &HashSet<i64>,
) {
    match beam.typ.as_str() {
        BEAM_TYPE_STATIC | BEAM_TYPE_DYNAMIC => {}
        "" => r.fail(join(path, tagBeamType), "Beam Type is missing"),
        _ => r.fail(
            join(path, tagBeamType),
            "Beam Type is neither STATIC nor DYNAMIC",
        ),
    }
    if !beam.radiation_type.is_empty()
        && !one_of(
            &beam.radiation_type,
            &["PHOTON", "ELECTRON", "NEUTRON", "PROTON"],
        )
    {
        r.warn(
            join(path, tagRadiationType),
            "Radiation Type is not a defined term",
        );
    }
    if beam.source_axis_distance.is_some_and(|v| v <= 0.0) {
        r.fail(join(path, tagSourceAxisDistance), "is not positive");
    }
    if let Some(n) = beam.referenced_patient_setup_number
        && !setups.contains(&n)
    {
        if !plan.patient_setups.is_empty() {
            r.fail(
                join(path, tagReferencedPatientSetupNumber),
                format!("patient setup {n} is not in the Patient Setup Sequence"),
            );
        } else {
            r.warn(
                join(path, tagReferencedPatientSetupNumber),
                format!(
                    "patient setup {n} is referenced but the plan has no Patient Setup Sequence"
                ),
            );
        }
    }
    if let Some(n) = beam.referenced_tolerance_table_number
        && !tolerances.contains(&n)
    {
        if !plan.tolerance_tables.is_empty() {
            r.fail(
                join(path, tagReferencedToleranceTableNumber),
                format!("tolerance table {n} is not in the Tolerance Table Sequence"),
            );
        } else {
            r.warn(
                join(path, tagReferencedToleranceTableNumber),
                format!(
                    "tolerance table {n} is referenced but the plan has no Tolerance Table Sequence"
                ),
            );
        }
    }
    if beam.devices.is_empty() {
        r.fail(
            join(path, tagBLDSequence),
            "Beam Limiting Device Sequence is missing or empty",
        );
    }
    validate_family_devices(r, &beam.devices, path, tagBLDSequence);
    validate_blocks(r, beam, path);
    let mut wedges: HashSet<i64> = HashSet::new();
    for (j, wedge) in beam.wedges.iter().enumerate() {
        if wedges.contains(&wedge.number) {
            r.fail(
                join(&item(path, tagWedgeSequence, j), tagWedgeNumber),
                format!("Wedge Number {} is not unique", wedge.number),
            );
        }
        wedges.insert(wedge.number);
    }
    let cps = &beam.control_points;
    if cps.len() < 2 {
        r.fail(
            join(path, tagControlPointSequence),
            format!("a beam needs at least 2 control points, has {}", cps.len()),
        );
    }
    let mut weighted = 0usize;
    for (i, cp) in cps.iter().enumerate() {
        let cp_path = item(path, tagControlPointSequence, i);
        if cp.index != i as i64 {
            r.fail(
                join(&cp_path, tagControlPointIndex),
                format!("Control Point Index is {}, want {i}", cp.index),
            );
        }
        if cp.cumulative_meterset_weight.is_some() {
            weighted += 1;
        }
        if i == 0 {
            for (field, tag, name) in [
                (FIELD_GANTRY_ANGLE, tagGantryAngle, "Gantry Angle"),
                (
                    FIELD_GANTRY_ROTATION_DIRECTION,
                    tagGantryRotationDirection,
                    "Gantry Rotation Direction",
                ),
                (
                    FIELD_BEAM_LIMITING_DEVICE_ANGLE,
                    tagBLDAngle,
                    "Beam Limiting Device Angle",
                ),
                (
                    FIELD_BEAM_LIMITING_DEVICE_ROTATION_DIRECTION,
                    tagBLDRotationDirection,
                    "Beam Limiting Device Rotation Direction",
                ),
                (
                    FIELD_PATIENT_SUPPORT_ANGLE,
                    tagPatientSupportAngle,
                    "Patient Support Angle",
                ),
                (
                    FIELD_PATIENT_SUPPORT_ROTATION_DIRECTION,
                    tagPatientSupportRotationDir,
                    "Patient Support Rotation Direction",
                ),
                (
                    FIELD_TABLE_TOP_ECCENTRIC_ANGLE,
                    tagTableTopEccentricAngle,
                    "Table Top Eccentric Angle",
                ),
                (
                    FIELD_TABLE_TOP_ECCENTRIC_ROTATION_DIRECTION,
                    tagTableTopEccentricRotationDir,
                    "Table Top Eccentric Rotation Direction",
                ),
            ] {
                if !cp.encodes(field) {
                    r.fail(
                        format!("{cp_path}/{tag}"),
                        format!("{name} is required in the first control point"),
                    );
                }
            }
            for device in &beam.devices {
                if cp.device(&device.typ).is_none() && !device.typ.is_empty() {
                    r.fail(
                        join(&cp_path, tagBLDPositionSequence),
                        format!(
                            "the first control point does not position device {}",
                            device.typ
                        ),
                    );
                }
            }
        }
        for (field, value, tag) in [
            (FIELD_GANTRY_ANGLE, cp.gantry_angle, tagGantryAngle),
            (
                FIELD_BEAM_LIMITING_DEVICE_ANGLE,
                cp.beam_limiting_device_angle,
                tagBLDAngle,
            ),
            (
                FIELD_PATIENT_SUPPORT_ANGLE,
                cp.patient_support_angle,
                tagPatientSupportAngle,
            ),
            (
                FIELD_TABLE_TOP_ECCENTRIC_ANGLE,
                cp.table_top_eccentric_angle,
                tagTableTopEccentricAngle,
            ),
        ] {
            if cp.encodes(field) && (value < 0.0 || value >= 360.0) {
                r.fail(
                    format!("{cp_path}/{tag}"),
                    format!("angle {} is outside [0, 360)", g(value)),
                );
            }
        }
        for (field, value, tag) in [
            (
                FIELD_GANTRY_ROTATION_DIRECTION,
                &cp.gantry_rotation_direction,
                tagGantryRotationDirection,
            ),
            (
                FIELD_BEAM_LIMITING_DEVICE_ROTATION_DIRECTION,
                &cp.beam_limiting_device_rotation_direction,
                tagBLDRotationDirection,
            ),
            (
                FIELD_PATIENT_SUPPORT_ROTATION_DIRECTION,
                &cp.patient_support_rotation_direction,
                tagPatientSupportRotationDir,
            ),
            (
                FIELD_TABLE_TOP_ECCENTRIC_ROTATION_DIRECTION,
                &cp.table_top_eccentric_rotation_direction,
                tagTableTopEccentricRotationDir,
            ),
        ] {
            if cp.encodes(field)
                && !one_of(
                    value,
                    &[
                        ROTATION_CLOCKWISE,
                        ROTATION_COUNTER_CLOCKWISE,
                        ROTATION_NONE,
                    ],
                )
            {
                r.fail(
                    format!("{cp_path}/{tag}"),
                    "rotation direction is not CW, CC or NONE",
                );
            }
        }
        if let Some(energy) = cp.nominal_beam_energy
            && cp.encodes(FIELD_NOMINAL_BEAM_ENERGY)
            && energy <= 0.0
        {
            r.fail(join(&cp_path, tagNominalBeamEnergy), "is not positive");
        }
        for (j, wedge) in cp.wedge_positions.iter().enumerate() {
            let wedge_path = item(&cp_path, tagWedgePositionSequence, j);
            if !wedges.contains(&wedge.wedge_number) {
                r.fail(
                    join(&wedge_path, tagReferencedWedgeNumber),
                    format!("wedge {} is not in the Wedge Sequence", wedge.wedge_number),
                );
            }
            if !one_of(&wedge.position, &["IN", "OUT"]) {
                r.warn(
                    join(&wedge_path, tagWedgePosition),
                    "Wedge Position is neither IN nor OUT",
                );
            }
        }
        for (j, reference) in cp.dose_references.iter().enumerate() {
            if !dose_refs.contains(&reference.number) {
                r.fail(
                    join(
                        &item(&cp_path, tagReferencedDoseReferenceSeq, j),
                        tagReferencedDoseReferenceNumber,
                    ),
                    format!(
                        "dose reference {} is not in the Dose Reference Sequence",
                        reference.number
                    ),
                );
            }
        }
        if beam.typ == BEAM_TYPE_STATIC && i > 0 && static_change(&cps[0], cp) {
            r.warn(
                cp_path,
                "a STATIC beam changes its machine parameters between control points",
            );
        }
    }
    if weighted == 0 {
        if beam.final_cumulative_meterset_weight.is_some() {
            r.warn(
                join(path, tagFinalCumulativeMetersetWeight),
                "is present although no control point has a Cumulative Meterset Weight",
            );
        }
    } else if weighted != cps.len() {
        r.fail(
            join(path, tagControlPointSequence),
            format!(
                "Cumulative Meterset Weight is empty in {} of {} control points",
                cps.len() - weighted,
                cps.len()
            ),
        );
    } else {
        let weight = |i: usize| cps[i].cumulative_meterset_weight.unwrap_or(0.0);
        if weight(0) != 0.0 {
            r.warn(
                join(
                    &item(path, tagControlPointSequence, 0),
                    tagCumulativeMetersetWeight,
                ),
                "the first control point does not start at weight 0",
            );
        }
        for i in 1..cps.len() {
            if weight(i) < weight(i - 1) {
                r.fail(
                    join(
                        &item(path, tagControlPointSequence, i),
                        tagCumulativeMetersetWeight,
                    ),
                    format!("decreases from {} to {}", g(weight(i - 1)), g(weight(i))),
                );
                break;
            }
        }
        let last = weight(cps.len() - 1);
        match beam.final_cumulative_meterset_weight {
            None => r.fail(
                join(path, tagFinalCumulativeMetersetWeight),
                "is required when control points carry Cumulative Meterset Weights",
            ),
            Some(f) if f <= 0.0 => r.fail(
                join(path, tagFinalCumulativeMetersetWeight),
                "is not positive",
            ),
            Some(f) if (last - f).abs() > 1e-6 * fmath::max(1.0, f.abs()) => r.fail(
                join(path, tagFinalCumulativeMetersetWeight),
                format!("is {} but the last control point is at {}", g(f), g(last)),
            ),
            Some(_) => {}
        }
    }
}
#[allow(clippy::manual_range_contains)]
fn validate_blocks(r: &mut Reader, beam: &Beam, path: &str) {
    let mut seen: HashSet<i64> = HashSet::new();
    let mut aperture: Option<(Option<f64>, Option<f64>)> = None;
    for (j, block) in beam.blocks.iter().enumerate() {
        let block_path = item(path, tagBlockSequence, j);
        if seen.contains(&block.number) {
            r.fail(
                join(&block_path, tagBlockNumber),
                "Block Number must be unique within the beam",
            );
        }
        seen.insert(block.number);
        if !one_of(&block.typ, &["APERTURE", "SHIELDING"]) {
            r.fail(
                join(&block_path, tagBlockType),
                "Block Type is not APERTURE or SHIELDING",
            );
        }
        if !block.divergence.is_empty() && !one_of(&block.divergence, &["PRESENT", "ABSENT"]) {
            r.fail(
                join(&block_path, tagBlockDivergence),
                "Block Divergence is not PRESENT or ABSENT",
            );
        }
        if block.number_of_points < 0
            || !block.contour.is_empty()
                && (block.contour.len() as i64 != block.number_of_points || block.contour.len() < 3)
        {
            r.fail(
                join(&block_path, tagBlockNumberOfPoints),
                "declared outline count does not describe a closed polygon",
            );
        }
        for point in &block.contour {
            if !finite(point[0]) || !finite(point[1]) {
                r.fail(
                    join(&block_path, tagBlockData),
                    "block coordinate is not finite",
                );
                break;
            }
        }
        if block.contour.is_empty() {
            r.warn(
                join(&block_path, tagBlockData),
                "block outline unavailable; no complete composite aperture can be inferred",
            );
        }
        if block.thickness.is_some_and(|v| v < 0.0) {
            r.fail(
                join(&block_path, tagBlockThickness),
                "Block Thickness is negative",
            );
        }
        if block.transmission.is_some_and(|v| v < 0.0 || v > 1.0) {
            r.fail(
                join(&block_path, tagBlockTransmission),
                "Block Transmission is outside [0,1]",
            );
        }
        if block.typ == "APERTURE" {
            match &mut aperture {
                Some((thickness, transmission)) => {
                    if let (Some(a), Some(b)) = (block.thickness, *thickness)
                        && a != b
                    {
                        r.fail(
                            join(&block_path, tagBlockThickness),
                            "APERTURE blocks must have equal specified thickness",
                        );
                    }
                    if let (Some(a), Some(b)) = (block.transmission, *transmission)
                        && a != b
                    {
                        r.fail(
                            join(&block_path, tagBlockTransmission),
                            "APERTURE blocks must have equal specified transmission",
                        );
                    }
                    if thickness.is_none() {
                        *thickness = block.thickness;
                    }
                    if transmission.is_none() {
                        *transmission = block.transmission;
                    }
                }
                None => aperture = Some((block.thickness, block.transmission)),
            }
        }
    }
}
fn static_change(first: &ControlPoint, cp: &ControlPoint) -> bool {
    if cp.gantry_angle != first.gantry_angle
        || cp.beam_limiting_device_angle != first.beam_limiting_device_angle
        || cp.patient_support_angle != first.patient_support_angle
        || cp.table_top_eccentric_angle != first.table_top_eccentric_angle
    {
        return true;
    }
    if cp.device_positions.len() != first.device_positions.len() {
        return true;
    }
    for (device, other) in cp.device_positions.iter().zip(&first.device_positions) {
        if device.device_type != other.device_type
            || device.positions.len() != other.positions.len()
        {
            return true;
        }
        for (a, b) in device.positions.iter().zip(&other.positions) {
            if (a - b).abs() > POSITION_TOLERANCE {
                return true;
            }
        }
    }
    false
}
pub(crate) fn validate_family_devices(
    r: &mut Reader,
    list: &[BeamLimitingDevice],
    path: &str,
    sequence: Tag,
) {
    let mut devices: HashSet<&str> = HashSet::new();
    for (j, device) in list.iter().enumerate() {
        let device_path = item(path, sequence, j);
        if devices.contains(device.typ.as_str()) {
            r.fail(
                join(&device_path, tagRTBeamLimitingDeviceType),
                format!("device {} is declared twice", device.typ),
            );
        }
        devices.insert(&device.typ);
        match device.typ.as_str() {
            DEVICE_X | DEVICE_Y | DEVICE_ASYM_X | DEVICE_ASYM_Y => {
                if device.pairs != 1 {
                    r.fail(
                        join(&device_path, tagNumberOfLeafJawPairs),
                        format!("a jaw has {} pairs, want 1", device.pairs),
                    );
                }
                if !device.leaf_boundaries.is_nil() {
                    r.warn(
                        join(&device_path, tagLeafPositionBoundaries),
                        "jaws have no leaf position boundaries",
                    );
                }
            }
            DEVICE_MLC_X | DEVICE_MLC_Y => {
                if device.pairs < 1 {
                    r.fail(
                        join(&device_path, tagNumberOfLeafJawPairs),
                        "an MLC needs at least one leaf pair",
                    );
                    continue;
                }
                if device.leaf_boundaries.len() as i64 != device.pairs + 1 {
                    r.fail(
                        join(&device_path, tagLeafPositionBoundaries),
                        format!(
                            "has {} values, want {} (pairs + 1)",
                            device.leaf_boundaries.len(),
                            device.pairs + 1
                        ),
                    );
                    continue;
                }
                for k in 1..device.leaf_boundaries.len() {
                    if device.leaf_boundaries[k] <= device.leaf_boundaries[k - 1] {
                        r.fail(
                            join(&device_path, tagLeafPositionBoundaries),
                            format!("boundaries are not strictly increasing at value {}", k + 1),
                        );
                        break;
                    }
                }
            }
            "" => r.fail(
                join(&device_path, tagRTBeamLimitingDeviceType),
                "RT Beam Limiting Device Type is missing",
            ),
            _ => r.fail(
                join(&device_path, tagRTBeamLimitingDeviceType),
                "device type is not X, Y, ASYMX, ASYMY, MLCX or MLCY",
            ),
        }
    }
}
