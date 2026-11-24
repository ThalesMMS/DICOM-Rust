use super::bounded::{ReadOptions, read_with_options};
use super::ion::write_ion;
use super::photon::{prepare_photon_write, write_photon};
use super::read::{Reader, item, join};
use super::reflect::semantic_equal;
use super::tags::*;
use super::write::{Pool, WriteBuilder, WriteOptions, begin_write, finish_write, put_sequence};
use super::*;
use crate::object::{File, Object, Tag, Vr};
use std::collections::{HashMap, HashSet};
impl Reader {
    pub(crate) fn float_attribute(&mut self, o: &Object, t: Tag, p: &str) -> Attribute<f64> {
        Attribute {
            present: o.has(t),
            value: self.float_opt(o, t, p),
        }
    }
    pub(crate) fn int_attribute(&mut self, o: &Object, t: Tag, p: &str) -> Attribute<i64> {
        Attribute {
            present: o.has(t),
            value: self.int_opt(o, t, p),
        }
    }
    pub(crate) fn required_float(&mut self, o: &Object, t: Tag, p: &str) -> f64 {
        match self.float_opt(o, t, p) {
            Some(v) => v,
            None => {
                self.fail(join(p, t), "required numeric value is missing");
                0.0
            }
        }
    }
    pub(crate) fn required_text(&mut self, o: &Object, t: Tag, p: &str) -> String {
        let v = self.str(o, t);
        if v.is_empty() {
            self.fail(join(p, t), "required text is missing");
        }
        v
    }
    pub(crate) fn attr_nonnegative(&mut self, a: &Attribute<f64>, t: Tag, p: &str, positive: bool) {
        if let Some(v) = a.value
            && (!finite(v) || v < 0.0 || positive && v == 0.0)
        {
            self.fail(join(p, t), "numeric value is outside the admitted domain");
        }
    }
}
pub(crate) fn read_brachy(r: &mut Reader, obj: &Object, p: &mut Plan) {
    let setups = r.sequence(obj, ft(0x300a0230));
    if setups.is_empty() {
        if obj.has(ft(0x300a0210)) {
            r.fail(
                ft(0x300a0210).to_string(),
                "sources without application setups are outside profile",
            );
        }
        return;
    }
    if p.sop_class_uid == RT_ION_PLAN_STORAGE {
        r.fail(
            ft(0x300a0230).to_string(),
            "brachy is not permitted in Ion profile",
        );
        return;
    }
    let machines = r.sequence(obj, ft(0x300a0206));
    if machines.len() != 1 || !machines[0].has(tagTreatmentMachineName) {
        r.fail(
            ft(0x300a0206).to_string(),
            "one treatment machine with Type 2 name is required",
        );
    }
    if !one_of(
        &p.brachy_treatment_technique,
        &["INTRACAVITARY", "INTERSTITIAL", "INTRALUMENARY", "CONTACT"],
    ) {
        r.fail(
            ft(0x300a0200).to_string(),
            "brachy technique is outside temporary implant profile",
        );
    }
    if !one_of(
        &p.brachy_treatment_type,
        &["HDR", "MDR", "LDR", "PDR", "MANUAL"],
    ) {
        r.fail(
            ft(0x300a0202).to_string(),
            "brachy treatment type is outside profile",
        );
    }
    let mut source_numbers: HashSet<i64> = HashSet::new();
    let mut beta_sources: HashMap<i64, bool> = HashMap::new();
    for (i, o) in r.sequence(obj, ft(0x300a0210)).iter().enumerate() {
        let path = item("", ft(0x300a0210), i);
        let s = BrachySource {
            number: r.required_int(o, ft(0x300a0212), &path, "Source Number"),
            typ: r.required_text(o, ft(0x300a0214), &path),
            serial_number: r.str(o, ft(0x30080105)),
            isotope_name: r.required_text(o, ft(0x300a0226), &path),
            strength_units: r.code(o, ft(0x300a0229)),
            isotope_half_life: r.float_attribute(o, ft(0x300a0228), &path),
            reference_air_kerma_rate: r.float_attribute(o, ft(0x300a022a), &path),
            strength: r.float_attribute(o, ft(0x300a022b), &path),
            strength_reference_date: r.required_text(o, ft(0x300a022c), &path),
            strength_reference_time: r.required_text(o, ft(0x300a022e), &path),
        };
        if !source_numbers.insert(s.number) {
            r.fail(join(&path, ft(0x300a0212)), "source number is duplicated");
        }
        beta_sources.insert(s.number, s.strength_units == "DOSE_RATE_WATER");
        if !one_of(&s.typ, &["POINT", "LINE", "CYLINDER", "SPHERE"]) {
            r.fail(
                join(&path, ft(0x300a0214)),
                "source type is outside profile",
            );
        }
        if s.isotope_half_life.value.is_none() {
            r.fail(join(&path, ft(0x300a0228)), "half life is missing");
        }
        r.attr_nonnegative(&s.isotope_half_life, ft(0x300a0228), &path, true);
        if !s.strength_units.is_empty()
            && !one_of(&s.strength_units, &["AIR_KERMA_RATE", "DOSE_RATE_WATER"])
        {
            r.fail(join(&path, ft(0x300a0229)), "strength unit is unsupported");
        }
        if s.strength_units == "DOSE_RATE_WATER"
            && s.reference_air_kerma_rate.value.is_some_and(|v| v != 0.0)
        {
            r.fail(
                join(&path, ft(0x300a022a)),
                "non-gamma reference air kerma rate must be zero",
            );
        }
        if s.strength_units == "DOSE_RATE_WATER" && s.strength.value.is_none() {
            r.fail(
                join(&path, ft(0x300a022b)),
                "non-gamma source strength is missing",
            );
        }
        if s.reference_air_kerma_rate.value.is_none() {
            r.fail(
                join(&path, ft(0x300a022a)),
                "reference air kerma rate is missing",
            );
        }
        r.attr_nonnegative(&s.reference_air_kerma_rate, ft(0x300a022a), &path, false);
        r.attr_nonnegative(&s.strength, ft(0x300a022b), &path, false);
        p.sources.push(s);
    }
    if p.sources.is_empty() {
        r.fail(ft(0x300a0210).to_string(), "sources are missing");
    }
    for (i, o) in setups.iter().enumerate() {
        let path = item("", ft(0x300a0230), i);
        let mut s = BrachySetup {
            number: r.required_int(o, ft(0x300a0234), &path, "Application Setup Number"),
            typ: r.required_text(o, ft(0x300a0232), &path),
            name: r.str(o, ft(0x300a0236)),
            total_reference_air_kerma: r.float_opt(o, ft(0x300a0250), &path),
            ..BrachySetup::default()
        };
        if s.total_reference_air_kerma.is_some_and(|v| v < 0.0) {
            r.fail(
                join(&path, ft(0x300a0250)),
                "total reference air kerma is negative",
            );
        }
        if s.total_reference_air_kerma.is_none() {
            r.fail(
                join(&path, ft(0x300a0250)),
                "total reference air kerma is missing",
            );
        }
        let channels = r.sequence(o, ft(0x300a0280));
        if channels.is_empty() {
            r.fail(join(&path, ft(0x300a0280)), "channels are missing");
        }
        let mut seen: HashSet<i64> = HashSet::new();
        let mut refs: HashSet<i64> = HashSet::new();
        for (j, c) in channels.iter().enumerate() {
            let cp = item(&path, ft(0x300a0280), j);
            let mut channel = BrachyChannel {
                number: r.required_int(c, ft(0x300a0282), &cp, "Channel Number"),
                source_number: r.required_int(c, ft(0x300c000e), &cp, "Referenced Source Number"),
                movement: r.code(c, ft(0x300a0288)),
                length: r.float_attribute(c, ft(0x300a0284), &cp),
                total_time: r.float_attribute(c, ft(0x300a0286), &cp),
                step_size: r.float_attribute(c, ft(0x300a02a0), &cp),
                pulse_interval: r.float_attribute(c, ft(0x300a028c), &cp),
                number_of_pulses: r.int_attribute(c, ft(0x300a028a), &cp),
                transfer_tube_number: r.int_attribute(c, ft(0x300a02a2), &cp),
                transfer_tube_length: r.float_attribute(c, ft(0x300a02a4), &cp),
                final_cumulative_time_weight: r.float_attribute(c, ft(0x300a02c8), &cp),
                control_points: Vec::new(),
            };
            if !seen.insert(channel.number) {
                r.fail(join(&cp, ft(0x300a0282)), "channel number is duplicated");
            }
            if !source_numbers.contains(&channel.source_number) {
                r.fail(join(&cp, ft(0x300c000e)), "referenced source is missing");
            }
            if refs.insert(channel.source_number) {
                s.referenced_source_numbers.push(channel.source_number);
            }
            if !one_of(&channel.movement, &["STEPWISE", "FIXED"]) {
                r.fail(
                    join(&cp, ft(0x300a0288)),
                    "source movement is outside profile",
                );
            }
            if channel.total_time.value.is_none() {
                r.fail(join(&cp, ft(0x300a0286)), "channel total time is missing");
            }
            r.attr_nonnegative(&channel.total_time, ft(0x300a0286), &cp, false);
            r.attr_nonnegative(&channel.length, ft(0x300a0284), &cp, false);
            r.attr_nonnegative(&channel.transfer_tube_length, ft(0x300a02a4), &cp, false);
            if channel.movement == "STEPWISE" && channel.step_size.value.is_none() {
                r.fail(join(&cp, ft(0x300a02a0)), "step size is missing");
            }
            r.attr_nonnegative(&channel.step_size, ft(0x300a02a0), &cp, true);
            if p.brachy_treatment_type == "PDR" {
                if !channel.number_of_pulses.value.is_some_and(|n| n > 0) {
                    r.fail(join(&cp, ft(0x300a028a)), "positive pulse count is missing");
                }
                if channel.pulse_interval.value.is_none() {
                    r.fail(join(&cp, ft(0x300a028c)), "pulse interval is missing");
                }
                r.attr_nonnegative(&channel.pulse_interval, ft(0x300a028c), &cp, true);
                if let (Some(interval), Some(total)) =
                    (channel.pulse_interval.value, channel.total_time.value)
                    && interval < total
                {
                    r.fail(
                        join(&cp, ft(0x300a028c)),
                        "pulse interval is less than channel time",
                    );
                }
            }
            if channel.transfer_tube_number.value.is_some() && !channel.transfer_tube_length.present
            {
                r.fail(join(&cp, ft(0x300a02a4)), "transfer tube length is missing");
            }
            let points = r.sequence(c, ft(0x300a02d0));
            let n = r.required_int(c, tagNumberOfControlPoints, &cp, "Number of Control Points");
            if n != points.len() as i64 || n < 2 {
                r.fail(
                    join(&cp, ft(0x300a02d0)),
                    "control point count is inconsistent",
                );
            }
            let mut previous: Option<f64> = None;
            for (k, v) in points.iter().enumerate() {
                let pp = item(&cp, ft(0x300a02d0), k);
                let point = BrachyControlPoint {
                    index: r.required_int(v, tagControlPointIndex, &pp, "Control Point Index"),
                    relative_position: r.required_float(v, ft(0x300a02d2), &pp),
                    cumulative_time_weight: r.float_attribute(v, ft(0x300a02d6), &pp),
                    position_3d: r.number_list(v, ft(0x300a02d4), &pp),
                    orientation: r.number_list(v, ft(0x300a0412), &pp),
                    position_3d_present: v.has(ft(0x300a02d4)),
                    orientation_present: v.has(ft(0x300a0412)),
                };
                if point.index != k as i64 {
                    r.fail(
                        join(&pp, tagControlPointIndex),
                        "control point index is inconsistent",
                    );
                }
                if let Some(w) = point.cumulative_time_weight.value {
                    if w < 0.0 || previous.is_some_and(|prev| w < prev) {
                        r.fail(
                            join(&pp, ft(0x300a02d6)),
                            "time weight decreases or is negative",
                        );
                    }
                    previous = Some(w);
                }
                if k == 0 && point.cumulative_time_weight.value != Some(0.0) {
                    r.fail(
                        join(&pp, ft(0x300a02d6)),
                        "first cumulative time weight must be zero",
                    );
                }
                if !point.position_3d.is_empty() && point.position_3d.len() != 3 {
                    r.fail(
                        join(&pp, ft(0x300a02d4)),
                        "patient position needs three components",
                    );
                }
                if !point.orientation.is_empty() {
                    let mut sum = 0.0;
                    for x in &point.orientation {
                        sum += x * x;
                    }
                    if point.orientation.len() != 3 || (sum - 1.0).abs() > 1e-5 {
                        r.fail(
                            join(&pp, ft(0x300a0412)),
                            "orientation must be a unit vector",
                        );
                    }
                }
                channel.control_points.push(point);
            }
            if channel
                .control_points
                .last()
                .is_some_and(|last| last.cumulative_time_weight.value.is_none())
            {
                r.fail(
                    join(&cp, ft(0x300a02d6)),
                    "last time weight is empty in admitted weighted profile",
                );
            }
            if let Some(prev) = previous
                && channel.final_cumulative_time_weight.value != Some(prev)
            {
                r.fail(
                    join(&cp, ft(0x300a02c8)),
                    "final time weight is inconsistent",
                );
            }
            if channel.total_time.value.is_some_and(|t| t > 0.0)
                && !channel
                    .final_cumulative_time_weight
                    .value
                    .is_some_and(|f| f > 0.0)
            {
                r.fail(
                    join(&cp, ft(0x300a02c8)),
                    "positive channel time needs positive final weight",
                );
            }
            if channel.movement == "STEPWISE" {
                if !channel.control_points.len().is_multiple_of(2) {
                    r.fail(
                        join(&cp, ft(0x300a02d0)),
                        "stepwise control points need dwell pairs",
                    );
                }
                let mut k = 0;
                while k + 1 < channel.control_points.len() {
                    if channel.control_points[k].relative_position
                        != channel.control_points[k + 1].relative_position
                    {
                        r.fail(
                            item(&cp, ft(0x300a02d0), k + 1),
                            "dwell pair positions differ",
                        );
                    }
                    k += 2;
                }
            }
            s.channels.push(channel);
        }
        let mut all_beta = !s.channels.is_empty();
        for c in &s.channels {
            all_beta = all_beta && beta_sources.get(&c.source_number).copied().unwrap_or(false);
        }
        if all_beta && s.total_reference_air_kerma.is_some_and(|v| v != 0.0) {
            r.fail(
                join(&path, tagTotalReferenceAirKerma),
                "non-gamma application air kerma must be zero",
            );
        }
        s.number_of_channels = s.channels.len() as i64;
        p.brachy_setups.push(s);
    }
}
pub fn write_plan(p: &Plan, opts: &WriteOptions) -> Result<File, Error> {
    if p.sop_class_uid != RT_PLAN_STORAGE && p.sop_class_uid != RT_ION_PLAN_STORAGE {
        return Err(Error::bare(ErrorKind::UnsupportedSopClass));
    }
    let (mut f, mut w) = begin_write(opts, &p.sop_class_uid, false, p)?;
    let read_options = ReadOptions {
        limits: opts.limits,
        strict_iod: false,
    };
    let Some(template) = &opts.template else {
        return Err(Error::bare(ErrorKind::InvalidObject));
    };
    let before = read_with_options(template, read_options)?;
    let photon = !p.beams.is_empty();
    let changed = !semantic_equal(&before, p);
    if photon == before.beams.is_empty() {
        return Err(path_error(
            ErrorKind::UnsupportedFeature,
            &tagBeamSequence.to_string(),
            "photon authoring requires a conventional RT Plan template",
        ));
    }
    if photon {
        prepare_photon_write(&before, p, &mut f.dataset, opts, changed)?;
    }
    write_plan_common(&mut w, &mut f.dataset, p);
    if photon {
        write_photon(&mut w, &mut f.dataset, p);
    } else if p.sop_class_uid == RT_ION_PLAN_STORAGE {
        write_ion(&mut w, &mut f.dataset, p);
    } else {
        write_brachy(&mut w, &mut f.dataset, p);
    }
    if let Some(e) = w.err.take() {
        return Err(e);
    }
    let actual = read_with_options(&f, read_options)?;
    if !semantic_equal(p, &actual) {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "",
            "model cannot be represented by the selected writer profile",
        ));
    }
    finish_write(f, w, opts, &p.sop_class_uid, changed)
}
fn write_plan_common(w: &mut WriteBuilder, o: &mut Object, p: &Plan) {
    for (t, vr, s) in [
        (tagStudyInstanceUID, Vr::UI, &p.study_instance_uid),
        (tagSeriesInstanceUID, Vr::UI, &p.series_instance_uid),
        (tagFrameOfReferenceUID, Vr::UI, &p.frame_of_reference_uid),
        (tagRTPlanLabel, Vr::SH, &p.label),
        (tagRTPlanName, Vr::LO, &p.name),
        (tagRTPlanDescription, Vr::ST, &p.description),
        (tagRTPlanDate, Vr::DA, &p.date),
        (tagRTPlanTime, Vr::TM, &p.time),
        (tagPlanIntent, Vr::CS, &p.intent),
        (tagRTPlanGeometry, Vr::CS, &p.geometry),
        (tagApprovalStatus, Vr::CS, &p.approval_status),
        (
            tagPrescriptionDescription,
            Vr::ST,
            &p.prescription_description,
        ),
    ] {
        if !s.is_empty() || o.has(t) {
            w.str(o, t, vr, s);
        }
    }
    let refs: Vec<Reference> = p.referenced_structure_set.iter().cloned().collect();
    w.reference(o, tagReferencedStructureSetSequence, &refs);
    w.reference(o, tagReferencedDoseSequence, &p.referenced_doses);
    let pr: Vec<Reference> = p
        .referenced_plans
        .iter()
        .map(|v| v.reference.clone())
        .collect();
    w.reference(o, tagReferencedRTPlanSequence, &pr);
    let mut pis = o
        .get_sequence(tagReferencedRTPlanSequence)
        .unwrap_or_default();
    for (i, it) in pis.iter_mut().enumerate() {
        w.optional_text(
            it,
            tagRTPlanRelationship,
            Vr::CS,
            &p.referenced_plans[i].relationship,
        );
    }
    if !pis.is_empty() {
        put_sequence(o, tagReferencedRTPlanSequence, &pis);
    }
    let mut old = w.keyed(o, tagDoseReferenceSequence, tagDoseReferenceNumber);
    for d in &p.dose_references {
        let i = old.item(&d.number);
        let it = old.get(i);
        w.integer(it, tagDoseReferenceNumber, d.number);
        w.optional_text(it, tagDoseReferenceUID, Vr::UI, &d.uid);
        w.str(it, tagDoseReferenceStructureType, Vr::CS, &d.structure_type);
        w.str(it, tagDoseReferenceType, Vr::CS, &d.typ);
        w.optional_text(it, tagDoseReferenceDescription, Vr::LO, &d.description);
        w.opt_int(it, tagReferencedROINumber, d.referenced_roi_number);
        it.remove(tagDoseReferencePointCoords);
        if !d.point_coordinates.is_empty() {
            w.numbers(it, tagDoseReferencePointCoords, &d.point_coordinates);
        }
        for (t, v) in [
            (tagDeliveryMaximumDose, d.delivery_maximum_dose),
            (tagTargetMinimumDose, d.target_minimum_dose),
            (tagTargetPrescriptionDose, d.target_prescription_dose),
            (tagTargetMaximumDose, d.target_maximum_dose),
            (tagOrganAtRiskMaximumDose, d.organ_at_risk_maximum_dose),
        ] {
            w.opt_float(it, t, v);
        }
    }
    o.remove(tagDoseReferenceSequence);
    if !old.is_empty() {
        put_sequence(o, tagDoseReferenceSequence, &old.objects());
    }
    let mut old = w.keyed(o, tagPatientSetupSequence, tagPatientSetupNumber);
    for s in &p.patient_setups {
        let i = old.item(&s.number);
        let it = old.get(i);
        w.integer(it, tagPatientSetupNumber, s.number);
        w.optional_text(it, tagPatientSetupLabel, Vr::LO, &s.label);
        w.optional_text(it, tagPatientPosition, Vr::CS, &s.patient_position);
        w.optional_text(
            it,
            tagPatientAdditionalPosition,
            Vr::LO,
            &s.patient_additional_position,
        );
        w.optional_text(it, tagSetupTechnique, Vr::CS, &s.setup_technique);
    }
    o.remove(tagPatientSetupSequence);
    if !old.is_empty() {
        put_sequence(o, tagPatientSetupSequence, &old.objects());
    }
    let mut tolerance_sequence = tagToleranceTableSequence;
    if p.sop_class_uid == RT_ION_PLAN_STORAGE {
        tolerance_sequence = ft(0x300a03a0);
        o.remove(tagToleranceTableSequence);
    }
    let mut old = w.keyed(o, tolerance_sequence, tagToleranceTableNumber);
    for s in &p.tolerance_tables {
        let i = old.item(&s.number);
        let it = old.get(i);
        w.integer(it, tagToleranceTableNumber, s.number);
        w.optional_text(it, tagToleranceTableLabel, Vr::SH, &s.label);
        for (t, v) in [
            (tagGantryAngleTolerance, s.gantry_angle),
            (tagBLDAngleTolerance, s.beam_limiting_device_angle),
            (tagPatientSupportAngleTol, s.patient_support_angle),
            (tagTableTopEccentricAngleTol, s.table_top_eccentric_angle),
            (tagTableTopVerticalTol, s.table_top_vertical_position),
            (
                tagTableTopLongitudinalTol,
                s.table_top_longitudinal_position,
            ),
            (tagTableTopLateralTol, s.table_top_lateral_position),
        ] {
            w.opt_float(it, t, v);
        }
        let mut by_type: Pool<String> = Pool::new();
        for v in it.get_sequence(tagBLDToleranceSequence).unwrap_or_default() {
            let key = v
                .get_string(tagRTBeamLimitingDeviceType)
                .unwrap_or_default();
            by_type.insert_old(key, v);
        }
        for v in &s.beam_limiting_device_position_limit {
            let k = by_type.item(&v.device_type);
            let d = by_type.get(k);
            w.str(d, tagRTBeamLimitingDeviceType, Vr::CS, &v.device_type);
            w.numbers(d, tagBLDPositionTolerance, &[v.position]);
        }
        it.remove(tagBLDToleranceSequence);
        if !by_type.is_empty() {
            put_sequence(it, tagBLDToleranceSequence, &by_type.objects());
        }
    }
    o.remove(tolerance_sequence);
    if !old.is_empty() {
        put_sequence(o, tolerance_sequence, &old.objects());
    }
    let mut old = w.keyed(o, tagFractionGroupSequence, tagFractionGroupNumber);
    for s in &p.fraction_groups {
        let i = old.item(&s.number);
        let it = old.get(i);
        w.integer(it, tagFractionGroupNumber, s.number);
        w.optional_text(it, tagFractionGroupDescription, Vr::LT, &s.description);
        w.opt_int(it, tagNumberOfFractionsPlanned, s.fractions_planned);
        w.opt_int(
            it,
            tagFractionPatternDigitsPerDay,
            s.fraction_pattern_digits_day,
        );
        w.opt_int(
            it,
            tagRepeatFractionCycleLength,
            s.repeat_fraction_cycle_length,
        );
        w.optional_text(it, tagFractionPattern, Vr::LT, &s.fraction_pattern);
        w.optional_text(it, tagBeamDoseMeaning, Vr::CS, &s.beam_dose_meaning);
        w.integer(it, tagNumberOfBeams, s.number_of_beams);
        w.integer(it, tagNumberOfBrachySetups, s.number_of_brachy_setups);
        let mut rb = w.keyed(it, tagReferencedBeamSequence, tagReferencedBeamNumber);
        for v in &s.referenced_beams {
            let k = rb.item(&v.beam_number);
            let b = rb.get(k);
            w.integer(b, tagReferencedBeamNumber, v.beam_number);
            w.opt_float(b, tagBeamDose, v.dose);
            w.opt_float(b, tagBeamMeterset, v.meterset);
        }
        it.remove(tagReferencedBeamSequence);
        if !rb.is_empty() {
            put_sequence(it, tagReferencedBeamSequence, &rb.objects());
        }
        let mut rs = w.keyed(
            it,
            tagReferencedBrachySetupSequence,
            tagReferencedBrachySetupNumber,
        );
        for v in &s.referenced_brachy_setups {
            let k = rs.item(&v.setup_number);
            let b = rs.get(k);
            w.integer(b, tagReferencedBrachySetupNumber, v.setup_number);
            w.opt_float(b, tagBrachySetupDose, v.dose);
        }
        it.remove(tagReferencedBrachySetupSequence);
        if !rs.is_empty() {
            put_sequence(it, tagReferencedBrachySetupSequence, &rs.objects());
        }
        let mut rd = w.keyed(
            it,
            tagReferencedDoseReferenceSeq,
            tagReferencedDoseReferenceNumber,
        );
        for v in &s.referenced_dose_reference {
            let k = rd.item(&v.number);
            let b = rd.get(k);
            w.integer(b, tagReferencedDoseReferenceNumber, v.number);
            for (t, x) in [
                (tagConstraintWeight, v.constraint_weight),
                (tagDeliveryMaximumDose, v.delivery_maximum_dose),
                (tagTargetPrescriptionDose, v.target_prescription_dose),
            ] {
                w.opt_float(b, t, x);
            }
        }
        it.remove(tagReferencedDoseReferenceSeq);
        if !rd.is_empty() {
            put_sequence(it, tagReferencedDoseReferenceSeq, &rd.objects());
        }
    }
    o.remove(tagFractionGroupSequence);
    if !old.is_empty() {
        put_sequence(o, tagFractionGroupSequence, &old.objects());
    }
}
fn write_brachy(w: &mut WriteBuilder, o: &mut Object, p: &Plan) {
    o.remove(tagBeamSequence);
    o.remove(ft(0x300a03a2));
    w.str(
        o,
        tagBrachyTreatmentTechnique,
        Vr::CS,
        &p.brachy_treatment_technique,
    );
    w.str(o, tagBrachyTreatmentType, Vr::CS, &p.brachy_treatment_type);
    let mut old = w.keyed(o, ft(0x300a0210), ft(0x300a0212));
    for s in &p.sources {
        let i = old.item(&s.number);
        let it = old.get(i);
        w.integer(it, ft(0x300a0212), s.number);
        w.str(it, ft(0x300a0214), Vr::CS, &s.typ);
        w.optional_text(it, ft(0x30080105), Vr::LO, &s.serial_number);
        w.str(it, ft(0x300a0226), Vr::LO, &s.isotope_name);
        w.optional_text(it, ft(0x300a0229), Vr::CS, &s.strength_units);
        w.float(it, ft(0x300a0228), &s.isotope_half_life);
        w.float(it, ft(0x300a022a), &s.reference_air_kerma_rate);
        w.float(it, ft(0x300a022b), &s.strength);
        w.str(it, ft(0x300a022c), Vr::DA, &s.strength_reference_date);
        w.str(it, ft(0x300a022e), Vr::TM, &s.strength_reference_time);
    }
    put_sequence(o, ft(0x300a0210), &old.objects());
    let mut old = w.keyed(o, tagApplicationSetupSequence, tagApplicationSetupNumber);
    for s in &p.brachy_setups {
        let i = old.item(&s.number);
        let it = old.get(i);
        w.integer(it, tagApplicationSetupNumber, s.number);
        w.str(it, tagApplicationSetupType, Vr::CS, &s.typ);
        w.optional_text(it, tagApplicationSetupName, Vr::LO, &s.name);
        w.opt_float(it, tagTotalReferenceAirKerma, s.total_reference_air_kerma);
        let mut previous = w.keyed(it, tagChannelSequence, ft(0x300a0282));
        for c in &s.channels {
            let k = previous.item(&c.number);
            let ch = previous.get(k);
            w.integer(ch, ft(0x300a0282), c.number);
            w.integer(ch, tagReferencedSourceNumber, c.source_number);
            w.str(ch, ft(0x300a0288), Vr::CS, &c.movement);
            for (t, v) in [
                (ft(0x300a0284), &c.length),
                (ft(0x300a0286), &c.total_time),
                (ft(0x300a02a0), &c.step_size),
                (ft(0x300a028c), &c.pulse_interval),
                (ft(0x300a02a4), &c.transfer_tube_length),
                (ft(0x300a02c8), &c.final_cumulative_time_weight),
            ] {
                w.float(ch, t, v);
            }
            w.int(ch, ft(0x300a028a), &c.number_of_pulses);
            w.int(ch, ft(0x300a02a2), &c.transfer_tube_number);
            w.integer(ch, tagNumberOfControlPoints, c.control_points.len() as i64);
            let mut cp_old = w.keyed(ch, ft(0x300a02d0), tagControlPointIndex);
            for cp in &c.control_points {
                let m = cp_old.item(&cp.index);
                let v = cp_old.get(m);
                w.integer(v, tagControlPointIndex, cp.index);
                w.float(v, ft(0x300a02d6), &cp.cumulative_time_weight);
                w.numbers(v, ft(0x300a02d2), &[cp.relative_position]);
                v.remove(ft(0x300a02d4));
                v.remove(ft(0x300a0412));
                if cp.position_3d_present || !cp.position_3d.is_empty() {
                    w.numbers(v, ft(0x300a02d4), &cp.position_3d);
                }
                if cp.orientation_present || !cp.orientation.is_empty() {
                    w.numbers(v, ft(0x300a0412), &cp.orientation);
                }
            }
            put_sequence(ch, ft(0x300a02d0), &cp_old.objects());
        }
        put_sequence(it, tagChannelSequence, &previous.objects());
    }
    put_sequence(o, tagApplicationSetupSequence, &old.objects());
}
