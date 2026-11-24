use super::ion::write_control_state;
use super::read::{POSITION_TOLERANCE, item, join};
use super::tags::*;
use super::write::{Pool, WriteBuilder, WriteOptions, put_sequence};
use super::*;
use crate::object::{Object, Vr, is_valid_uid};
use std::collections::{HashMap, HashSet};
pub(crate) fn prepare_photon_write(
    before: &Plan,
    plan: &Plan,
    dataset: &mut Object,
    opts: &WriteOptions,
    changed: bool,
) -> Result<(), Error> {
    if plan.sop_class_uid != RT_PLAN_STORAGE
        || !plan.ion_beams.is_empty()
        || !plan.sources.is_empty()
        || !plan.brachy_setups.is_empty()
        || !plan.brachy_treatment_technique.is_empty()
        || !plan.brachy_treatment_type.is_empty()
    {
        return Err(path_error(
            ErrorKind::UnsupportedFeature,
            &tagBeamSequence.to_string(),
            "mixed beam families are outside the photon profile",
        ));
    }
    check_photon_template(dataset)?;
    let uid = if opts.sop_instance_uid.is_empty() {
        before.sop_instance_uid.as_str()
    } else {
        opts.sop_instance_uid.as_str()
    };
    if !plan.approval_status.is_empty()
        && !one_of(
            &plan.approval_status,
            &["UNAPPROVED", "APPROVED", "REJECTED"],
        )
    {
        return Err(path_error(
            ErrorKind::InvalidObject,
            &tagApprovalStatus.to_string(),
            "invalid approval status",
        ));
    }
    if changed || uid != before.sop_instance_uid {
        if !plan.approval_status.is_empty() && plan.approval_status != "UNAPPROVED" {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &tagApprovalStatus.to_string(),
                "a new photon instance cannot carry a prior review",
            ));
        }
        for tag in [ft(0x300e0004), ft(0x300e0005), ft(0x300e0008)] {
            dataset.remove(tag);
        }
    }
    check_photon_references(plan, uid)?;
    if plan.fraction_groups.is_empty() {
        return Err(path_error(
            ErrorKind::InvalidObject,
            &tagFractionGroupSequence.to_string(),
            "photon plan requires a fraction group",
        ));
    }
    for (i, group) in plan.fraction_groups.iter().enumerate() {
        let path = item("", tagFractionGroupSequence, i);
        if group.number_of_beams <= 0 || group.number_of_brachy_setups != 0 {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &path,
                "expected beam references in a photon fraction group",
            ));
        }
        if !group.beam_dose_meaning.is_empty()
            && !one_of(&group.beam_dose_meaning, &["BEAM_LEVEL", "FRACTION_LEVEL"])
        {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &join(&path, tagBeamDoseMeaning),
                "invalid Beam Dose Meaning",
            ));
        }
    }
    let setups: HashSet<i64> = plan.patient_setups.iter().map(|s| s.number).collect();
    let tolerances: HashSet<i64> = plan.tolerance_tables.iter().map(|t| t.number).collect();
    for (i, beam) in plan.beams.iter().enumerate() {
        let path = item("", tagBeamSequence, i);
        if let Some(n) = beam.referenced_patient_setup_number
            && !setups.contains(&n)
        {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &join(&path, tagReferencedPatientSetupNumber),
                "referenced patient setup is absent",
            ));
        }
        if let Some(n) = beam.referenced_tolerance_table_number
            && !tolerances.contains(&n)
        {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &join(&path, tagReferencedToleranceTableNumber),
                "referenced tolerance table is absent",
            ));
        }
        check_photon_beam(beam, &path)?;
    }
    Ok(())
}
fn check_photon_references(plan: &Plan, uid: &str) -> Result<(), Error> {
    const DOSE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.2";
    const STRUCTURE_SET_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.481.3";
    let valid = |reference: &Reference| {
        let id = reference.sop_instance_uid.as_str();
        is_valid_uid(id.as_bytes())
            && id != uid
            && id != plan.study_instance_uid
            && id != plan.series_instance_uid
            && id != plan.frame_of_reference_uid
    };
    if let Some(reference) = &plan.referenced_structure_set
        && (plan.geometry != GEOMETRY_PATIENT
            || reference.sop_class_uid != STRUCTURE_SET_STORAGE
            || !valid(reference))
    {
        return Err(path_error(
            ErrorKind::InvalidObject,
            &tagReferencedStructureSetSequence.to_string(),
            "invalid structure set reference",
        ));
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for reference in &plan.referenced_doses {
        if reference.sop_class_uid != DOSE_STORAGE
            || !valid(reference)
            || seen.contains(reference.sop_instance_uid.as_str())
        {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &tagReferencedDoseSequence.to_string(),
                "invalid or duplicate dose reference",
            ));
        }
        seen.insert(&reference.sop_instance_uid);
    }
    for r in &plan.referenced_plans {
        let reference = &r.reference;
        if !one_of(
            &reference.sop_class_uid,
            &[RT_PLAN_STORAGE, RT_ION_PLAN_STORAGE],
        ) || !valid(reference)
            || seen.contains(reference.sop_instance_uid.as_str())
            || !one_of(
                &r.relationship,
                &[
                    "PRIOR",
                    "ALTERNATIVE",
                    "PREDECESSOR",
                    "VERIFIED_PLAN",
                    "CONCURRENT",
                ],
            )
            || r.relationship == "VERIFIED_PLAN" && plan.intent != "VERIFICATION"
        {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &tagReferencedRTPlanSequence.to_string(),
                "invalid or duplicate plan reference",
            ));
        }
        seen.insert(&reference.sop_instance_uid);
    }
    Ok(())
}
fn check_photon_template(dataset: &Object) -> Result<(), Error> {
    for (i, beam) in dataset
        .get_sequence(tagBeamSequence)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let path = item("", tagBeamSequence, i);
        let enhanced = ft(0x300800a3);
        if beam.has(enhanced) && beam.get_string(enhanced).as_deref() != Some("NO") {
            return Err(path_error(
                ErrorKind::UnsupportedFeature,
                &join(&path, enhanced),
                "enhanced beam limiting device definitions are outside the photon profile",
            ));
        }
        for tag in [
            ft(0x300800a1),
            ft(0x30020050),
            ft(0x300a0420),
            tagWedgeSequence,
            tagCompensatorSequence,
            tagReferencedBolusSequence,
            tagBlockSequence,
            tagApplicatorSequence,
        ] {
            if beam.has(tag) {
                return Err(path_error(
                    ErrorKind::UnsupportedFeature,
                    &join(&path, tag),
                    "beam modifier is outside the photon profile",
                ));
            }
        }
        for (j, point) in beam
            .get_sequence(tagControlPointSequence)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            for tag in [
                ft(0x300800a2),
                ft(0x300a014a),
                ft(0x300a014c),
                ft(0x300a0142),
                ft(0x300a0146),
                tagWedgePositionSequence,
            ] {
                if point.has(tag) {
                    return Err(path_error(
                        ErrorKind::UnsupportedFeature,
                        &join(&item(&path, tagControlPointSequence, j), tag),
                        "control-point modifier is outside the photon profile",
                    ));
                }
            }
        }
    }
    Ok(())
}
fn same_photon_position(a: &DevicePosition, b: &DevicePosition) -> bool {
    a.device_type == b.device_type && a.positions == b.positions
}
fn same_positions(a: &[DevicePosition], b: &[DevicePosition]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same_photon_position(x, y))
}
fn check_photon_beam(beam: &Beam, path: &str) -> Result<(), Error> {
    if beam.radiation_type != "PHOTON"
        || !beam.high_dose_technique_type.is_empty()
        || !one_of(&beam.treatment_delivery_type, &["", "TREATMENT"])
        || !one_of(&beam.primary_dosimeter_unit, &["", "MU", "MINUTE"])
        || beam.number_of_wedges != 0
        || beam.number_of_compensators != 0
        || beam.number_of_boli != 0
        || beam.number_of_blocks != 0
        || !beam.wedges.is_empty()
        || !beam.blocks.is_empty()
        || !beam.applicators.is_empty()
    {
        return Err(path_error(
            ErrorKind::UnsupportedFeature,
            path,
            "expected a conventional photon treatment beam without accessory modifiers",
        ));
    }
    if beam.control_points.len() < 2 || beam.final_cumulative_meterset_weight.is_none() {
        return Err(path_error(
            ErrorKind::InvalidObject,
            &join(path, tagControlPointSequence),
            "expected at least two weighted control points and final weight",
        ));
    }
    let first = &beam.control_points[0];
    let mut rotation_during_irradiation = false;
    for (i, point) in beam.control_points.iter().enumerate() {
        let cp_path = item(path, tagControlPointSequence, i);
        let Some(weight) = point.cumulative_meterset_weight else {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &cp_path,
                "invalid control point index or cumulative weight",
            ));
        };
        if point.index != i as i64 || i == 0 && weight != 0.0 {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &cp_path,
                "invalid control point index or cumulative weight",
            ));
        }
        if !one_of(
            &point.gantry_rotation_direction,
            &[
                ROTATION_NONE,
                ROTATION_CLOCKWISE,
                ROTATION_COUNTER_CLOCKWISE,
            ],
        ) {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &join(&cp_path, tagGantryRotationDirection),
                "invalid rotation direction",
            ));
        }
        if !point.wedge_positions.is_empty()
            || point.table_top_pitch_angle.is_some()
            || point.table_top_roll_angle.is_some()
            || point.beam_limiting_device_rotation_direction != ROTATION_NONE
            || point.patient_support_rotation_direction != ROTATION_NONE
            || point.table_top_eccentric_rotation_direction != ROTATION_NONE
            || point.beam_limiting_device_angle != first.beam_limiting_device_angle
            || point.patient_support_angle != first.patient_support_angle
            || point.table_top_eccentric_angle != first.table_top_eccentric_angle
            || point.nominal_beam_energy != first.nominal_beam_energy
            || point.table_top_lateral_position != first.table_top_lateral_position
            || point.table_top_longitudinal_position != first.table_top_longitudinal_position
            || point.table_top_vertical_position != first.table_top_vertical_position
            || point.isocenter_position != first.isocenter_position
        {
            return Err(path_error(
                ErrorKind::UnsupportedFeature,
                &cp_path,
                "moving or tilted couch, rotating collimator or variable energy is outside the photon profile",
            ));
        }
        if beam.typ == BEAM_TYPE_STATIC
            && (point.gantry_rotation_direction != ROTATION_NONE
                || point.gantry_angle != first.gantry_angle
                || point.dose_rate_set != first.dose_rate_set
                || !same_positions(&point.device_positions, &first.device_positions))
        {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &cp_path,
                "STATIC beam changes its treatment parameters",
            ));
        }
        if i > 0 {
            let previous = &beam.control_points[i - 1];
            if previous.gantry_rotation_direction == ROTATION_NONE
                && previous.gantry_angle != point.gantry_angle
            {
                return Err(path_error(
                    ErrorKind::InvalidObject,
                    &join(&cp_path, tagGantryAngle),
                    "gantry angle changed without a preceding rotation",
                ));
            }
            if previous.gantry_rotation_direction != ROTATION_NONE
                && previous.cumulative_meterset_weight.unwrap_or(0.0) < weight
            {
                rotation_during_irradiation = true;
            }
        }
    }
    if beam.typ == BEAM_TYPE_DYNAMIC && !rotation_during_irradiation {
        return Err(path_error(
            ErrorKind::UnsupportedFeature,
            path,
            "DYNAMIC photon beam requires gantry rotation during irradiation",
        ));
    }
    Ok(())
}
pub(crate) fn write_photon(w: &mut WriteBuilder, dataset: &mut Object, plan: &Plan) {
    if plan.approval_status.is_empty() {
        dataset.remove(tagApprovalStatus);
    }
    let mut old = w.keyed(dataset, tagBeamSequence, tagBeamNumber);
    for beam in &plan.beams {
        if !w.tick() {
            return;
        }
        let i = old.item(&beam.number);
        let it = old.get(i);
        w.integer(it, tagBeamNumber, beam.number);
        w.str(it, tagBeamType, Vr::CS, &beam.typ);
        w.str(it, tagRadiationType, Vr::CS, &beam.radiation_type);
        w.str(
            it,
            tagTreatmentMachineName,
            Vr::SH,
            &beam.treatment_machine_name,
        );
        w.optional_text(it, tagBeamName, Vr::LO, &beam.name);
        w.optional_text(it, tagBeamDescription, Vr::ST, &beam.description);
        w.optional_text(it, tagManufacturer, Vr::LO, &beam.manufacturer);
        w.optional_text(
            it,
            tagPrimaryDosimeterUnit,
            Vr::CS,
            &beam.primary_dosimeter_unit,
        );
        w.optional_text(
            it,
            tagTreatmentDeliveryType,
            Vr::CS,
            &beam.treatment_delivery_type,
        );
        w.opt_float(it, tagSourceAxisDistance, beam.source_axis_distance);
        w.opt_int(
            it,
            tagReferencedPatientSetupNumber,
            beam.referenced_patient_setup_number,
        );
        w.opt_int(
            it,
            tagReferencedToleranceTableNumber,
            beam.referenced_tolerance_table_number,
        );
        w.opt_float(
            it,
            tagFinalCumulativeMetersetWeight,
            beam.final_cumulative_meterset_weight,
        );
        for tag in [
            tagNumberOfWedges,
            tagNumberOfCompensators,
            tagNumberOfBoli,
            tagNumberOfBlocks,
        ] {
            w.integer(it, tag, 0);
        }
        write_photon_devices(w, it, &beam.devices);
        if !write_photon_control_points(w, it, &beam.control_points) {
            return;
        }
    }
    put_sequence(dataset, tagBeamSequence, &old.objects());
}
fn write_photon_devices(w: &mut WriteBuilder, beam: &mut Object, devices: &[BeamLimitingDevice]) {
    let mut old = w.old_items(beam, tagBLDSequence, |o| {
        o.get_string(tagRTBeamLimitingDeviceType)
            .unwrap_or_default()
    });
    for device in devices {
        let i = old.item(&device.typ);
        let it = old.get(i);
        w.str(it, tagRTBeamLimitingDeviceType, Vr::CS, &device.typ);
        w.integer(it, tagNumberOfLeafJawPairs, device.pairs);
        w.opt_float(it, tagSourceToBLDDistance, device.source_distance);
        if device.leaf_boundaries.is_nil() && !it.has(tagLeafPositionBoundaries) {
            it.remove(tagLeafPositionBoundaries);
        } else {
            w.numbers(it, tagLeafPositionBoundaries, &device.leaf_boundaries);
        }
    }
    put_sequence(beam, tagBLDSequence, &old.objects());
}
fn write_photon_control_points(
    w: &mut WriteBuilder,
    beam: &mut Object,
    points: &[ControlPoint],
) -> bool {
    let first = &points[0];
    let positions: HashMap<&str, &[f64]> = first
        .device_positions
        .iter()
        .map(|d| (d.device_type.as_str(), d.positions.as_slice()))
        .collect();
    let (mut angle_changes, mut direction_changes, mut rate_changes) = (false, false, false);
    let mut moving: HashSet<&str> = HashSet::new();
    for point in &points[1..] {
        angle_changes = angle_changes || point.gantry_angle != first.gantry_angle;
        direction_changes =
            direction_changes || point.gantry_rotation_direction != first.gantry_rotation_direction;
        rate_changes = rate_changes || point.dose_rate_set != first.dose_rate_set;
        for d in &point.device_positions {
            let base = positions
                .get(d.device_type.as_str())
                .copied()
                .unwrap_or(&[]);
            if base != d.positions.as_slice() {
                moving.insert(&d.device_type);
            }
        }
    }
    let mut old = w.keyed(beam, tagControlPointSequence, tagControlPointIndex);
    for (i, source) in points.iter().enumerate() {
        if !w.tick() {
            return false;
        }
        let idx = old.item(&source.index);
        let it = old.get(idx);
        for d in &source.device_positions {
            if one_of(&d.device_type, &[DEVICE_X, DEVICE_Y])
                && d.positions.len() == 2
                && (d.positions[0] + d.positions[1]).abs() > POSITION_TOLERANCE
            {
                w.err = Some(path_error(
                    ErrorKind::InvalidObject,
                    &tagLeafJawPositions.to_string(),
                    "symmetric jaw positions must oppose each other",
                ));
                return false;
            }
        }
        let mut point = source.clone();
        if i != 0 {
            point.device_positions = source
                .device_positions
                .iter()
                .filter(|d| moving.contains(d.device_type.as_str()))
                .cloned()
                .collect();
        } else {
            point.encoded |= FIELD_TABLE_TOP_ECCENTRIC_ANGLE;
        }
        write_control_state(w, it, &point);
        let mut references = it
            .get_sequence(tagReferencedDoseReferenceSeq)
            .unwrap_or_default();
        for reference in &mut references {
            if !reference.has(tagCumulativeDoseRefCoefficient) {
                w.numbers(reference, tagCumulativeDoseRefCoefficient, &[]);
            }
        }
        if !references.is_empty() {
            put_sequence(it, tagReferencedDoseReferenceSeq, &references);
        }
        if i == 0 {
            for tag in [
                tagTableTopVerticalPosition,
                tagTableTopLongitudinalPosition,
                tagTableTopLateralPosition,
                tagIsocenterPosition,
            ] {
                if !it.has(tag) {
                    w.numbers(it, tag, &[]);
                }
            }
        } else {
            for tag in [
                tagNominalBeamEnergy,
                tagBLDAngle,
                tagBLDRotationDirection,
                tagPatientSupportAngle,
                tagPatientSupportRotationDir,
                tagTableTopEccentricAngle,
                tagTableTopEccentricRotationDir,
                tagTableTopVerticalPosition,
                tagTableTopLongitudinalPosition,
                tagTableTopLateralPosition,
                tagIsocenterPosition,
            ] {
                it.remove(tag);
            }
            if !angle_changes {
                it.remove(tagGantryAngle);
            }
            if !direction_changes {
                it.remove(tagGantryRotationDirection);
            }
            if !rate_changes {
                it.remove(tagDoseRateSet);
            }
        }
    }
    let count = old.len() as i64;
    w.integer(beam, tagNumberOfControlPoints, count);
    put_sequence(beam, tagControlPointSequence, &old.objects());
    true
}
impl WriteBuilder {
    pub(crate) fn old_items(
        &mut self,
        o: &Object,
        seq: crate::object::Tag,
        key: impl Fn(&Object) -> String,
    ) -> Pool<String> {
        let mut out = Pool::new();
        for it in o.get_sequence(seq).unwrap_or_default() {
            if !self.tick() {
                break;
            }
            let k = key(&it);
            if out.contains(&k) {
                self.err = Some(path_error(
                    ErrorKind::InvalidObject,
                    &seq.to_string(),
                    "template item identity is ambiguous",
                ));
                return out;
            }
            out.insert_old(k, it);
        }
        out
    }
    pub(crate) fn new_item(
        &mut self,
        old: &mut Pool<String>,
        seen: &mut HashSet<String>,
        key: &str,
        seq: crate::object::Tag,
    ) -> usize {
        if !seen.insert(key.to_string()) {
            self.err = Some(path_error(
                ErrorKind::InvalidObject,
                &seq.to_string(),
                "model item identity is ambiguous",
            ));
        }
        old.item(&key.to_string())
    }
}
