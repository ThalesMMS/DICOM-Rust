use super::tags::*;
use super::validate::validate_family_devices;
use super::*;
use crate::numtext::{format_float, parse_float};
use crate::object::{Object, Tag, Vr};
use crate::textfmt::to_upper;
use crate::validation::{ValidationOptions, validate_element_with};
use std::collections::{HashMap, HashSet};
pub(crate) const POSITION_TOLERANCE: f64 = 1e-6;
pub(crate) fn g(v: f64) -> String {
    format_float(v, 'g', -1)
}
pub(crate) fn join(prefix: &str, tag: Tag) -> String {
    if prefix.is_empty() {
        tag.to_string()
    } else {
        format!("{prefix}/{tag}")
    }
}
pub(crate) fn item(prefix: &str, tag: Tag, index: usize) -> String {
    format!("{}[{index}]", join(prefix, tag))
}
pub(crate) fn clean(value: &str) -> &str {
    value.trim_matches(|c: char| c == '\0' || c.is_whitespace())
}
#[derive(Default)]
pub(crate) struct Reader {
    pub max_problems: i64,
    pub errors: Vec<Problem>,
    pub warnings: Vec<Problem>,
}
impl Reader {
    pub fn new(max_problems: i64) -> Reader {
        Reader {
            max_problems,
            ..Reader::default()
        }
    }
    pub fn fail(&mut self, path: impl Into<String>, message: impl Into<String>) {
        if self.max_problems > 0 && self.errors.len() as i64 >= self.max_problems {
            return;
        }
        self.errors.push(Problem {
            path: path.into(),
            message: message.into(),
        });
    }
    pub fn warn(&mut self, path: impl Into<String>, message: impl Into<String>) {
        if self.max_problems > 0 && self.warnings.len() as i64 >= self.max_problems {
            return;
        }
        self.warnings.push(Problem {
            path: path.into(),
            message: message.into(),
        });
    }
    pub fn str(&self, obj: &Object, tag: Tag) -> String {
        match obj.get_string(tag) {
            Some(v) => clean(&v).to_string(),
            None => String::new(),
        }
    }
    pub fn code(&self, obj: &Object, tag: Tag) -> String {
        to_upper(&self.str(obj, tag))
    }
    pub fn uid(&self, obj: &Object, tag: Tag) -> String {
        match obj.get_uid(tag) {
            Some(v) => clean(&v).to_string(),
            None => String::new(),
        }
    }
    pub fn numbers(&mut self, obj: &Object, tag: Tag, prefix: &str) -> Option<Vec<f64>> {
        let element = obj.get(tag)?;
        let path = join(prefix, tag);
        if matches!(element.vr(), Vr::FL | Vr::FD) {
            return match obj.get_floats(tag) {
                Ok(values) => self.finite(values, &path),
                Err(_) => {
                    self.fail(path, "binary value cannot be decoded");
                    None
                }
            };
        }
        let texts = obj.get_strings(tag)?;
        let non_empty = texts.iter().filter(|t| !clean(t).is_empty()).count();
        if non_empty == 0 {
            return None;
        }
        if non_empty != texts.len() {
            self.fail(path, "value list has empty entries");
            return None;
        }
        let mut out = Vec::with_capacity(texts.len());
        for (i, text) in texts.iter().enumerate() {
            match parse_float(clean(text)) {
                Ok(v) => out.push(v),
                Err(_) => {
                    self.fail(path, format!("value {} is not a number", i + 1));
                    return None;
                }
            }
        }
        self.finite(out, &path)
    }
    fn finite(&mut self, values: Vec<f64>, path: &str) -> Option<Vec<f64>> {
        for (i, value) in values.iter().enumerate() {
            if !value.is_finite() {
                self.fail(path, format!("value {} is not finite", i + 1));
                return None;
            }
        }
        Some(values)
    }
    pub(crate) fn number_list(&mut self, obj: &Object, tag: Tag, prefix: &str) -> Vec<f64> {
        self.numbers(obj, tag, prefix).unwrap_or_default()
    }
    pub(crate) fn float_opt(&mut self, obj: &Object, tag: Tag, prefix: &str) -> Option<f64> {
        let values = self.numbers(obj, tag, prefix)?;
        if values.is_empty() {
            return None;
        }
        if values.len() != 1 {
            self.fail(
                join(prefix, tag),
                format!("has {} values, want 1", values.len()),
            );
            return None;
        }
        Some(values[0])
    }
    pub(crate) fn int_opt(&mut self, obj: &Object, tag: Tag, prefix: &str) -> Option<i64> {
        if let Some(e) = obj.get(tag)
            && matches!(e.vr(), Vr::US | Vr::SS | Vr::UL | Vr::SL | Vr::UV | Vr::SV)
        {
            let values = match obj.get_ints(tag) {
                Ok(v) if v.len() <= 1 => v,
                _ => {
                    self.fail(join(prefix, tag), "integer value cannot be decoded");
                    return None;
                }
            };
            let first = *values.first()?;
            if first < i64::from(i32::MIN) || first > i64::from(i32::MAX) {
                self.fail(join(prefix, tag), "integer is outside signed 32-bit range");
                return None;
            }
            return Some(first);
        }
        let value = self.float_opt(obj, tag, prefix)?;
        if value != value.trunc() || value.abs() > f64::from(i32::MAX) {
            self.fail(join(prefix, tag), "is not an integer");
            return None;
        }
        Some(value as i64)
    }
    pub fn required_int(&mut self, obj: &Object, tag: Tag, prefix: &str, name: &str) -> i64 {
        match self.int_opt(obj, tag, prefix) {
            Some(v) => v,
            None => {
                let path = join(prefix, tag);
                if !self.has_problem(&path) {
                    self.fail(path, format!("{name} is missing"));
                }
                0
            }
        }
    }
    pub(crate) fn has_problem(&self, path: &str) -> bool {
        self.errors.iter().any(|p| p.path == path)
    }
    pub fn sequence(&self, obj: &Object, tag: Tag) -> Vec<Object> {
        obj.get_sequence(tag).unwrap_or_default()
    }
    pub fn references(&self, obj: &Object, tag: Tag) -> Vec<Reference> {
        self.sequence(obj, tag)
            .iter()
            .map(|it| Reference {
                sop_class_uid: self.uid(it, tagRefSOPClassUID),
                sop_instance_uid: self.uid(it, tagRefSOPInstanceUID),
            })
            .collect()
    }
    #[doc(hidden)]
    pub fn read_plan(&mut self, obj: &Object) -> Plan {
        let mut plan = Plan {
            sop_instance_uid: self.uid(obj, tagSOPInstanceUID),
            study_instance_uid: self.uid(obj, tagStudyInstanceUID),
            series_instance_uid: self.uid(obj, tagSeriesInstanceUID),
            frame_of_reference_uid: self.uid(obj, tagFrameOfReferenceUID),
            label: self.str(obj, tagRTPlanLabel),
            name: self.str(obj, tagRTPlanName),
            description: self.str(obj, tagRTPlanDescription),
            date: self.str(obj, tagRTPlanDate),
            time: self.str(obj, tagRTPlanTime),
            intent: self.code(obj, tagPlanIntent),
            geometry: self.code(obj, tagRTPlanGeometry),
            approval_status: self.code(obj, tagApprovalStatus),
            prescription_description: self.str(obj, tagPrescriptionDescription),
            brachy_treatment_technique: self.code(obj, tagBrachyTreatmentTechnique),
            brachy_treatment_type: self.code(obj, tagBrachyTreatmentType),
            ..Plan::default()
        };
        if self.code(obj, tagModality) != "RTPLAN" {
            self.fail(tagModality.to_string(), "Modality is not RTPLAN");
        }
        let refs = self.references(obj, tagReferencedStructureSetSequence);
        if !refs.is_empty() {
            if refs.len() > 1 {
                self.fail(
                    tagReferencedStructureSetSequence.to_string(),
                    format!("has {} items, want 1", refs.len()),
                );
            }
            plan.referenced_structure_set = Some(refs[0].clone());
        }
        plan.referenced_doses = self.references(obj, tagReferencedDoseSequence);
        for it in self.sequence(obj, tagReferencedRTPlanSequence) {
            plan.referenced_plans.push(PlanReference {
                reference: Reference {
                    sop_class_uid: self.uid(&it, tagRefSOPClassUID),
                    sop_instance_uid: self.uid(&it, tagRefSOPInstanceUID),
                },
                relationship: self.code(&it, tagRTPlanRelationship),
            });
        }
        for (i, it) in self
            .sequence(obj, tagDoseReferenceSequence)
            .iter()
            .enumerate()
        {
            let path = item("", tagDoseReferenceSequence, i);
            let reference = self.read_dose_reference(it, &path);
            plan.dose_references.push(reference);
        }
        let mut tolerance_sequence = tagToleranceTableSequence;
        if obj.get_uid(tagSOPClassUID).as_deref() == Some(RT_ION_PLAN_STORAGE) {
            tolerance_sequence = ft(0x300a03a0);
        }
        for (i, it) in self.sequence(obj, tolerance_sequence).iter().enumerate() {
            let path = item("", tolerance_sequence, i);
            let table = self.read_tolerance_table(it, &path);
            plan.tolerance_tables.push(table);
        }
        for (i, it) in self
            .sequence(obj, tagPatientSetupSequence)
            .iter()
            .enumerate()
        {
            let path = item("", tagPatientSetupSequence, i);
            plan.patient_setups.push(PatientSetup {
                number: self.required_int(it, tagPatientSetupNumber, &path, "Patient Setup Number"),
                label: self.str(it, tagPatientSetupLabel),
                patient_position: self.code(it, tagPatientPosition),
                patient_additional_position: self.str(it, tagPatientAdditionalPosition),
                setup_technique: self.code(it, tagSetupTechnique),
            });
        }
        for (i, it) in self
            .sequence(obj, tagFractionGroupSequence)
            .iter()
            .enumerate()
        {
            let path = item("", tagFractionGroupSequence, i);
            let group = self.read_fraction_group(it, &path);
            plan.fraction_groups.push(group);
        }
        for (i, it) in self.sequence(obj, tagBeamSequence).iter().enumerate() {
            let path = item("", tagBeamSequence, i);
            let beam = self.read_beam(it, &path);
            plan.beams.push(beam);
        }
        plan
    }
    fn read_dose_reference(&mut self, it: &Object, path: &str) -> DoseReference {
        DoseReference {
            number: self.required_int(it, tagDoseReferenceNumber, path, "Dose Reference Number"),
            uid: self.uid(it, tagDoseReferenceUID),
            structure_type: self.code(it, tagDoseReferenceStructureType),
            description: self.str(it, tagDoseReferenceDescription),
            typ: self.code(it, tagDoseReferenceType),
            referenced_roi_number: self.int_opt(it, tagReferencedROINumber, path),
            point_coordinates: self.number_list(it, tagDoseReferencePointCoords, path),
            delivery_maximum_dose: self.float_opt(it, tagDeliveryMaximumDose, path),
            target_minimum_dose: self.float_opt(it, tagTargetMinimumDose, path),
            target_prescription_dose: self.float_opt(it, tagTargetPrescriptionDose, path),
            target_maximum_dose: self.float_opt(it, tagTargetMaximumDose, path),
            organ_at_risk_maximum_dose: self.float_opt(it, tagOrganAtRiskMaximumDose, path),
        }
    }
    fn read_tolerance_table(&mut self, it: &Object, path: &str) -> ToleranceTable {
        let mut table = ToleranceTable {
            number: self.required_int(it, tagToleranceTableNumber, path, "Tolerance Table Number"),
            label: self.str(it, tagToleranceTableLabel),
            gantry_angle: self.float_opt(it, tagGantryAngleTolerance, path),
            beam_limiting_device_angle: self.float_opt(it, tagBLDAngleTolerance, path),
            patient_support_angle: self.float_opt(it, tagPatientSupportAngleTol, path),
            table_top_eccentric_angle: self.float_opt(it, tagTableTopEccentricAngleTol, path),
            table_top_vertical_position: self.float_opt(it, tagTableTopVerticalTol, path),
            table_top_longitudinal_position: self.float_opt(it, tagTableTopLongitudinalTol, path),
            table_top_lateral_position: self.float_opt(it, tagTableTopLateralTol, path),
            beam_limiting_device_position_limit: Vec::new(),
        };
        for (j, device) in self
            .sequence(it, tagBLDToleranceSequence)
            .iter()
            .enumerate()
        {
            let device_path = item(path, tagBLDToleranceSequence, j);
            let Some(position) = self.float_opt(device, tagBLDPositionTolerance, &device_path)
            else {
                let p = join(&device_path, tagBLDPositionTolerance);
                if !self.has_problem(&p) {
                    self.fail(p, "Beam Limiting Device Position Tolerance is missing");
                }
                continue;
            };
            table
                .beam_limiting_device_position_limit
                .push(DeviceTolerance {
                    device_type: self.code(device, tagRTBeamLimitingDeviceType),
                    position,
                });
        }
        table
    }
    fn read_fraction_group(&mut self, it: &Object, path: &str) -> FractionGroup {
        let mut group = FractionGroup {
            number: self.required_int(it, tagFractionGroupNumber, path, "Fraction Group Number"),
            description: self.str(it, tagFractionGroupDescription),
            fractions_planned: self.int_opt(it, tagNumberOfFractionsPlanned, path),
            fraction_pattern_digits_day: self.int_opt(it, tagFractionPatternDigitsPerDay, path),
            repeat_fraction_cycle_length: self.int_opt(it, tagRepeatFractionCycleLength, path),
            fraction_pattern: self.str(it, tagFractionPattern),
            beam_dose_meaning: self.code(it, tagBeamDoseMeaning),
            number_of_beams: self.required_int(it, tagNumberOfBeams, path, "Number of Beams"),
            number_of_brachy_setups: self.required_int(
                it,
                tagNumberOfBrachySetups,
                path,
                "Number of Brachy Application Setups",
            ),
            ..FractionGroup::default()
        };
        for (j, r) in self
            .sequence(it, tagReferencedBeamSequence)
            .iter()
            .enumerate()
        {
            let ref_path = item(path, tagReferencedBeamSequence, j);
            group.referenced_beams.push(ReferencedBeam {
                beam_number: self.required_int(
                    r,
                    tagReferencedBeamNumber,
                    &ref_path,
                    "Referenced Beam Number",
                ),
                dose: self.float_opt(r, tagBeamDose, &ref_path),
                meterset: self.float_opt(r, tagBeamMeterset, &ref_path),
            });
        }
        for (j, r) in self
            .sequence(it, tagReferencedBrachySetupSequence)
            .iter()
            .enumerate()
        {
            let ref_path = item(path, tagReferencedBrachySetupSequence, j);
            group.referenced_brachy_setups.push(ReferencedBrachySetup {
                setup_number: self.required_int(
                    r,
                    tagReferencedBrachySetupNumber,
                    &ref_path,
                    "Referenced Brachy Application Setup Number",
                ),
                dose: self.float_opt(r, tagBrachySetupDose, &ref_path),
            });
        }
        for (j, r) in self
            .sequence(it, tagReferencedDoseReferenceSeq)
            .iter()
            .enumerate()
        {
            let ref_path = item(path, tagReferencedDoseReferenceSeq, j);
            group
                .referenced_dose_reference
                .push(ReferencedDoseReference {
                    number: self.required_int(
                        r,
                        tagReferencedDoseReferenceNumber,
                        &ref_path,
                        "Referenced Dose Reference Number",
                    ),
                    constraint_weight: self.float_opt(r, tagConstraintWeight, &ref_path),
                    delivery_maximum_dose: self.float_opt(r, tagDeliveryMaximumDose, &ref_path),
                    target_prescription_dose: self.float_opt(
                        r,
                        tagTargetPrescriptionDose,
                        &ref_path,
                    ),
                });
        }
        group
    }
    fn read_beam(&mut self, it: &Object, path: &str) -> Beam {
        let mut beam = Beam {
            number: self.required_int(it, tagBeamNumber, path, "Beam Number"),
            name: self.str(it, tagBeamName),
            description: self.str(it, tagBeamDescription),
            typ: self.code(it, tagBeamType),
            radiation_type: self.code(it, tagRadiationType),
            high_dose_technique_type: self.code(it, tagHighDoseTechniqueType),
            treatment_machine_name: self.str(it, tagTreatmentMachineName),
            manufacturer: self.str(it, tagManufacturer),
            primary_dosimeter_unit: self.code(it, tagPrimaryDosimeterUnit),
            source_axis_distance: self.float_opt(it, tagSourceAxisDistance, path),
            treatment_delivery_type: self.code(it, tagTreatmentDeliveryType),
            referenced_patient_setup_number: self.int_opt(
                it,
                tagReferencedPatientSetupNumber,
                path,
            ),
            referenced_tolerance_table_number: self.int_opt(
                it,
                tagReferencedToleranceTableNumber,
                path,
            ),
            number_of_wedges: self.required_int(it, tagNumberOfWedges, path, "Number of Wedges"),
            number_of_compensators: self.required_int(
                it,
                tagNumberOfCompensators,
                path,
                "Number of Compensators",
            ),
            number_of_boli: self.required_int(it, tagNumberOfBoli, path, "Number of Boli"),
            number_of_blocks: self.required_int(it, tagNumberOfBlocks, path, "Number of Blocks"),
            final_cumulative_meterset_weight: self.float_opt(
                it,
                tagFinalCumulativeMetersetWeight,
                path,
            ),
            ..Beam::default()
        };
        for (j, device) in self.sequence(it, tagBLDSequence).iter().enumerate() {
            let device_path = item(path, tagBLDSequence, j);
            beam.devices.push(BeamLimitingDevice {
                typ: self.code(device, tagRTBeamLimitingDeviceType),
                source_distance: self.float_opt(device, tagSourceToBLDDistance, &device_path),
                pairs: self.required_int(
                    device,
                    tagNumberOfLeafJawPairs,
                    &device_path,
                    "Number of Leaf/Jaw Pairs",
                ),
                leaf_boundaries: NilFloats(self.numbers(
                    device,
                    tagLeafPositionBoundaries,
                    &device_path,
                )),
            });
        }
        let before_devices = self.errors.len();
        validate_family_devices(self, &beam.devices, path, tagBLDSequence);
        if self.errors.len() > before_devices {
            return beam;
        }
        for (j, wedge) in self.sequence(it, tagWedgeSequence).iter().enumerate() {
            let wedge_path = item(path, tagWedgeSequence, j);
            let mut w = Wedge {
                number: self.required_int(wedge, tagWedgeNumber, &wedge_path, "Wedge Number"),
                typ: self.code(wedge, tagWedgeType),
                id: self.str(wedge, tagWedgeID),
                factor: self.float_opt(wedge, tagWedgeFactor, &wedge_path),
                ..Wedge::default()
            };
            if let Some(angle) = self.float_opt(wedge, tagWedgeAngle, &wedge_path) {
                w.angle = angle;
            }
            if let Some(orientation) = self.float_opt(wedge, tagWedgeOrientation, &wedge_path) {
                w.orientation = orientation;
            }
            beam.wedges.push(w);
        }
        for (j, block) in self.sequence(it, tagBlockSequence).iter().enumerate() {
            let block_path = item(path, tagBlockSequence, j);
            let mut b = Block {
                number: self.required_int(block, tagBlockNumber, &block_path, "Block Number"),
                typ: self.code(block, tagBlockType),
                name: self.str(block, tagBlockName),
                divergence: self.code(block, tagBlockDivergence),
                material_id: self.str(block, tagMaterialID),
                thickness: self.float_opt(block, tagBlockThickness, &block_path),
                transmission: self.float_opt(block, tagBlockTransmission, &block_path),
                ..Block::default()
            };
            if let Some(points) = self.int_opt(block, tagBlockNumberOfPoints, &block_path) {
                b.number_of_points = points;
            }
            if let Some(element) = block.get(tagBlockData) {
                let (report, err) = validate_element_with(
                    element,
                    &ValidationOptions {
                        max_findings: self.max_problems,
                        big_endian: block.value_big_endian(),
                        ..ValidationOptions::default()
                    },
                );
                if element.vr() != Vr::DS
                    || err.is_some()
                    || report.has_errors()
                    || report.truncated
                {
                    self.fail(
                        join(&block_path, tagBlockData),
                        "Block Data is not a valid DS value list",
                    );
                } else {
                    let values = self.number_list(block, tagBlockData, &block_path);
                    if !values.is_empty()
                        && (!values.len().is_multiple_of(2)
                            || (values.len() / 2) as i64 != b.number_of_points)
                    {
                        self.fail(
                            join(&block_path, tagBlockData),
                            format!(
                                "has {} coordinates, want twice Block Number of Points ({})",
                                values.len(),
                                b.number_of_points
                            ),
                        );
                    } else {
                        b.contour = values
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|p| [p[0], p[1]])
                            .collect();
                    }
                }
            }
            beam.blocks.push(b);
        }
        for applicator in self.sequence(it, tagApplicatorSequence) {
            beam.applicators.push(Applicator {
                id: self.str(&applicator, tagApplicatorID),
                typ: self.code(&applicator, tagApplicatorType),
            });
        }
        self.check_count(
            it,
            tagWedgeSequence,
            path,
            "Number of Wedges",
            beam.number_of_wedges,
        );
        self.check_count(
            it,
            tagCompensatorSequence,
            path,
            "Number of Compensators",
            beam.number_of_compensators,
        );
        self.check_count(
            it,
            tagReferencedBolusSequence,
            path,
            "Number of Boli",
            beam.number_of_boli,
        );
        self.check_count(
            it,
            tagBlockSequence,
            path,
            "Number of Blocks",
            beam.number_of_blocks,
        );
        let declared = self.required_int(
            it,
            tagNumberOfControlPoints,
            path,
            "Number of Control Points",
        );
        let items = self.sequence(it, tagControlPointSequence);
        if declared != items.len() as i64 {
            self.fail(
                join(path, tagControlPointSequence),
                format!(
                    "has {} items, Number of Control Points is {declared}",
                    items.len()
                ),
            );
        }
        beam.control_points =
            self.read_control_points_at(&beam.devices, &items, path, tagControlPointSequence);
        beam
    }
    pub(crate) fn check_count(
        &mut self,
        obj: &Object,
        tag: Tag,
        prefix: &str,
        name: &str,
        count: i64,
    ) {
        let n = self.sequence(obj, tag).len() as i64;
        if count < 0 {
            self.fail(join(prefix, tag), format!("{name} is negative"));
        } else if !obj.has(tag) && count > 0 {
            self.fail(
                join(prefix, tag),
                format!("is missing although {name} is {count}"),
            );
        } else if obj.has(tag) && n != count {
            self.fail(
                join(prefix, tag),
                format!("has {n} items, {name} is {count}"),
            );
        }
    }
    pub(crate) fn read_control_points_at(
        &mut self,
        devices: &[BeamLimitingDevice],
        items: &[Object],
        beam_path: &str,
        seq_tag: Tag,
    ) -> Vec<ControlPoint> {
        let mut out = Vec::with_capacity(items.len());
        let mut state = ControlPoint::default();
        for (i, it) in items.iter().enumerate() {
            let path = item(beam_path, seq_tag, i);
            let mut cp = state.clone();
            cp.encoded = 0;
            cp.encoded_device_types = Vec::new();
            cp.wedge_positions = Vec::new();
            cp.dose_references = Vec::new();
            cp.index = self.required_int(it, tagControlPointIndex, &path, "Control Point Index");
            cp.cumulative_meterset_weight = self.float_opt(it, tagCumulativeMetersetWeight, &path);
            cp.source_to_surface_distance = self.float_opt(it, tagSourceToSurfaceDistance, &path);
            macro_rules! set_float_ptr {
                ($tag:expr, $field:expr, $dst:ident) => {
                    if let Some(value) = self.float_opt(it, $tag, &path) {
                        cp.$dst = Some(value);
                        cp.encoded |= $field;
                    }
                };
            }
            macro_rules! set_float {
                ($tag:expr, $field:expr, $dst:ident) => {
                    if let Some(value) = self.float_opt(it, $tag, &path) {
                        cp.$dst = value;
                        cp.encoded |= $field;
                    }
                };
            }
            macro_rules! set_code {
                ($tag:expr, $field:expr, $dst:ident) => {
                    let value = self.code(it, $tag);
                    if !value.is_empty() {
                        cp.$dst = value;
                        cp.encoded |= $field;
                    }
                };
            }
            set_float_ptr!(
                tagNominalBeamEnergy,
                FIELD_NOMINAL_BEAM_ENERGY,
                nominal_beam_energy
            );
            set_float_ptr!(tagDoseRateSet, FIELD_DOSE_RATE_SET, dose_rate_set);
            set_float!(tagGantryAngle, FIELD_GANTRY_ANGLE, gantry_angle);
            set_code!(
                tagGantryRotationDirection,
                FIELD_GANTRY_ROTATION_DIRECTION,
                gantry_rotation_direction
            );
            set_float!(
                tagBLDAngle,
                FIELD_BEAM_LIMITING_DEVICE_ANGLE,
                beam_limiting_device_angle
            );
            set_code!(
                tagBLDRotationDirection,
                FIELD_BEAM_LIMITING_DEVICE_ROTATION_DIRECTION,
                beam_limiting_device_rotation_direction
            );
            set_float!(
                tagPatientSupportAngle,
                FIELD_PATIENT_SUPPORT_ANGLE,
                patient_support_angle
            );
            set_code!(
                tagPatientSupportRotationDir,
                FIELD_PATIENT_SUPPORT_ROTATION_DIRECTION,
                patient_support_rotation_direction
            );
            set_float!(
                tagTableTopEccentricAngle,
                FIELD_TABLE_TOP_ECCENTRIC_ANGLE,
                table_top_eccentric_angle
            );
            set_code!(
                tagTableTopEccentricRotationDir,
                FIELD_TABLE_TOP_ECCENTRIC_ROTATION_DIRECTION,
                table_top_eccentric_rotation_direction
            );
            set_float_ptr!(
                tagTableTopPitchAngle,
                FIELD_TABLE_TOP_PITCH_ANGLE,
                table_top_pitch_angle
            );
            set_float_ptr!(
                tagTableTopRollAngle,
                FIELD_TABLE_TOP_ROLL_ANGLE,
                table_top_roll_angle
            );
            set_float_ptr!(
                tagTableTopVerticalPosition,
                FIELD_TABLE_TOP_VERTICAL_POSITION,
                table_top_vertical_position
            );
            set_float_ptr!(
                tagTableTopLongitudinalPosition,
                FIELD_TABLE_TOP_LONGITUDINAL_POSITION,
                table_top_longitudinal_position
            );
            set_float_ptr!(
                tagTableTopLateralPosition,
                FIELD_TABLE_TOP_LATERAL_POSITION,
                table_top_lateral_position
            );
            if let Some(values) = self.numbers(it, tagIsocenterPosition, &path) {
                if values.len() != 3 {
                    self.fail(
                        join(&path, tagIsocenterPosition),
                        format!("has {} values, want 3", values.len()),
                    );
                } else {
                    cp.isocenter_position = values;
                    cp.encoded |= FIELD_ISOCENTER_POSITION;
                }
            }
            if let Some(positions) = it.get_sequence(tagBLDPositionSequence) {
                cp.encoded |= FIELD_DEVICE_POSITIONS;
                cp.device_positions =
                    self.merge_device_positions(devices, &cp.device_positions, &positions, &path);
                for position in &positions {
                    cp.encoded_device_types
                        .push(self.code(position, tagRTBeamLimitingDeviceType));
                }
            }
            for (j, wedge) in self
                .sequence(it, tagWedgePositionSequence)
                .iter()
                .enumerate()
            {
                let wedge_path = item(&path, tagWedgePositionSequence, j);
                cp.wedge_positions.push(WedgePosition {
                    wedge_number: self.required_int(
                        wedge,
                        tagReferencedWedgeNumber,
                        &wedge_path,
                        "Referenced Wedge Number",
                    ),
                    position: self.code(wedge, tagWedgePosition),
                });
            }
            for (j, r) in self
                .sequence(it, tagReferencedDoseReferenceSeq)
                .iter()
                .enumerate()
            {
                let ref_path = item(&path, tagReferencedDoseReferenceSeq, j);
                cp.dose_references.push(ControlPointDoseReference {
                    number: self.required_int(
                        r,
                        tagReferencedDoseReferenceNumber,
                        &ref_path,
                        "Referenced Dose Reference Number",
                    ),
                    coefficient: self.float_opt(r, tagCumulativeDoseRefCoefficient, &ref_path),
                });
            }
            out.push(cp.clone());
            state = cp;
        }
        out
    }
    fn merge_device_positions(
        &mut self,
        devices: &[BeamLimitingDevice],
        inherited: &[DevicePosition],
        items: &[Object],
        path: &str,
    ) -> Vec<DevicePosition> {
        let mut current: HashMap<String, Vec<f64>> = HashMap::new();
        for d in inherited {
            current.insert(d.device_type.clone(), d.positions.clone());
        }
        let mut seen: HashSet<String> = HashSet::new();
        for (j, it) in items.iter().enumerate() {
            let item_path = item(path, tagBLDPositionSequence, j);
            let device_type = self.code(it, tagRTBeamLimitingDeviceType);
            if device_type.is_empty() {
                self.fail(
                    join(&item_path, tagRTBeamLimitingDeviceType),
                    "RT Beam Limiting Device Type is missing",
                );
                continue;
            }
            if seen.contains(&device_type) {
                self.fail(
                    join(&item_path, tagRTBeamLimitingDeviceType),
                    format!("device {device_type} is positioned twice"),
                );
                continue;
            }
            seen.insert(device_type.clone());
            let Some(device) = devices.iter().find(|d| d.typ == device_type) else {
                self.fail(
                    join(&item_path, tagRTBeamLimitingDeviceType),
                    format!("device {device_type} is not in the Beam Limiting Device Sequence"),
                );
                continue;
            };
            let Some(positions) = self.numbers(it, tagLeafJawPositions, &item_path) else {
                let p = join(&item_path, tagLeafJawPositions);
                if !self.has_problem(&p) {
                    self.fail(p, "Leaf/Jaw Positions are missing");
                }
                continue;
            };
            if device.pairs <= 0 || positions.len() as i64 != 2 * device.pairs {
                self.fail(
                    join(&item_path, tagLeafJawPositions),
                    format!(
                        "has {} values, want {} (2 x {} pairs)",
                        positions.len(),
                        2 * device.pairs,
                        device.pairs
                    ),
                );
                continue;
            }
            let pairs = device.pairs as usize;
            for k in 0..pairs {
                if positions[k] > positions[pairs + k] + POSITION_TOLERANCE {
                    self.fail(
                        join(&item_path, tagLeafJawPositions),
                        format!(
                            "pair {}: bank 1 at {} mm is beyond bank 2 at {} mm",
                            k + 1,
                            g(positions[k]),
                            g(positions[pairs + k])
                        ),
                    );
                    break;
                }
            }
            current.insert(device_type, positions);
        }
        let mut out = Vec::with_capacity(devices.len());
        for device in devices {
            if let Some(positions) = current.get(&device.typ) {
                out.push(DevicePosition {
                    device_type: device.typ.clone(),
                    positions: positions.clone(),
                });
            }
        }
        out
    }
}
