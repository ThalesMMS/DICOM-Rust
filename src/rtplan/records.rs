use super::bounded::{
    ReadOptions, default_limits, file_read_options, fresh_facade, preflight, strict_object,
};
use super::ion::{read_fl, species_inconsistent};
use super::read::{Reader, clean, item, join};
use super::reflect::{Pair, model_admission_pair, semantic_floats};
use super::tags::*;
use super::*;
use crate::fmath;
use crate::object::{File, Object, Tag, Value};
use std::collections::HashSet;
model! {
    TreatmentRecord "TreatmentRecord" {
        sop_class_uid: String = "SOPClassUID",
        sop_instance_uid: String = "SOPInstanceUID",
        study_instance_uid: String = "StudyInstanceUID",
        series_instance_uid: String = "SeriesInstanceUID",
        instance_number: i64 = "InstanceNumber",
        date: Attribute<String> = "Date",
        time: Attribute<String> = "Time",
        timezone_offset: Attribute<String> = "TimezoneOffset",
        content_origin: Attribute<String> = "ContentOrigin",
        referenced_plan: Option<Reference> = "ReferencedPlan",
        related_records: Vec<Reference> = "RelatedRecords",
        machine: Option<RecordMachine> = "Machine",
        fraction_group_number: Attribute<i64> = "FractionGroupNumber",
        fractions_planned: Attribute<i64> = "FractionsPlanned",
        primary_dosimeter_unit: String = "PrimaryDosimeterUnit",
        patient_setups: Vec<PatientSetup> = "PatientSetups",
        measured_dose_references: Vec<RecordDoseReference> = "MeasuredDoseReferences",
        calculated_dose_references: Vec<RecordDoseReference> = "CalculatedDoseReferences",
        beam_sessions: Vec<RecordBeamSession> = "BeamSessions",
        brachy_technique: String = "BrachyTechnique",
        brachy_type: String = "BrachyType",
        brachy_sources: Vec<RecordedBrachySource> = "BrachySources",
        brachy_sessions: Vec<RecordBrachySession> = "BrachySessions",
        summary: Option<RecordSummary> = "Summary",
        warnings: Vec<Problem> = "Warnings",
    }
}
model! {
    RecordMachine "RecordMachine" {
        name: Attribute<String> = "Name",
        manufacturer: Attribute<String> = "Manufacturer",
        institution: Attribute<String> = "Institution",
        model: Attribute<String> = "Model",
        serial_number: Attribute<String> = "SerialNumber",
        table_top_position_alignment_uid: String = "TableTopPositionAlignmentUID",
    }
}
model! {
    RecordDoseReference "RecordDoseReference" {
        number: Attribute<i64> = "Number",
        plan_number: Attribute<i64> = "PlanNumber",
        units: String = "Units",
        value: Attribute<f64> = "Value",
        typ: Attribute<String> = "Type",
        description: String = "Description",
    }
}
model! {
    RecordDoseValue "RecordDoseValue" {
        plan_number: Attribute<i64> = "PlanNumber",
        local_number: Attribute<i64> = "LocalNumber",
        value: f64 = "Value",
    }
}
model! {
    RecordBeamSession "RecordBeamSession" {
        referenced_beam_number: Attribute<i64> = "ReferencedBeamNumber",
        referenced_patient_setup_number: Attribute<i64> = "ReferencedPatientSetupNumber",
        referenced_tolerance_table_number: Attribute<i64> = "ReferencedToleranceTableNumber",
        name: String = "Name",
        description: String = "Description",
        typ: String = "Type",
        radiation_type: String = "RadiationType",
        fraction_number: Attribute<i64> = "FractionNumber",
        clinical_fraction_number: Attribute<i64> = "ClinicalFractionNumber",
        delivery_type: Attribute<String> = "DeliveryType",
        verification_status: Attribute<String> = "VerificationStatus",
        termination_status: String = "TerminationStatus",
        specified_primary_meterset: Attribute<f64> = "SpecifiedPrimaryMeterset",
        specified_secondary_meterset: Attribute<f64> = "SpecifiedSecondaryMeterset",
        delivered_primary_meterset: Attribute<f64> = "DeliveredPrimaryMeterset",
        delivered_secondary_meterset: Attribute<f64> = "DeliveredSecondaryMeterset",
        specified_treatment_time: Attribute<f64> = "SpecifiedTreatmentTime",
        delivered_treatment_time: Attribute<f64> = "DeliveredTreatmentTime",
        source_axis_distance: Attribute<f64> = "SourceAxisDistance",
        devices: Vec<BeamLimitingDevice> = "Devices",
        measured_doses: Vec<RecordDoseValue> = "MeasuredDoses",
        calculated_doses: Vec<RecordDoseValue> = "CalculatedDoses",
        verification_images: Vec<Reference> = "VerificationImages",
        atomic_number: Attribute<i64> = "AtomicNumber",
        mass_number: Attribute<i64> = "MassNumber",
        charge_state: Attribute<i64> = "ChargeState",
        scan_mode: String = "ScanMode",
        snout_id: String = "SnoutID",
        patient_support_type: String = "PatientSupportType",
        range_shifters: Vec<RecordedRangeShifter> = "RangeShifters",
        control_points: Vec<RecordControlPoint> = "ControlPoints",
    }
}
model! {
    RecordedRangeShifter "RecordedRangeShifter" {
        number: i64 = "Number",
        id: String = "ID",
    }
}
model! {
    RecordMachineState "RecordMachineState" {
        energy: Attribute<f64> = "Energy",
        gantry_angle: Attribute<f64> = "GantryAngle",
        beam_limiting_device_angle: Attribute<f64> = "BeamLimitingDeviceAngle",
        patient_support_angle: Attribute<f64> = "PatientSupportAngle",
        table_top_eccentric_angle: Attribute<f64> = "TableTopEccentricAngle",
        table_top_eccentric_axis_distance: Attribute<f64> = "TableTopEccentricAxisDistance",
        table_top_pitch_angle: Attribute<f64> = "TableTopPitchAngle",
        table_top_roll_angle: Attribute<f64> = "TableTopRollAngle",
        gantry_pitch_angle: Attribute<f64> = "GantryPitchAngle",
        table_top_vertical_position: Attribute<f64> = "TableTopVerticalPosition",
        table_top_longitudinal_position: Attribute<f64> = "TableTopLongitudinalPosition",
        table_top_lateral_position: Attribute<f64> = "TableTopLateralPosition",
        snout_position: Attribute<f64> = "SnoutPosition",
        gantry_direction: Attribute<String> = "GantryDirection",
        beam_limiting_device_direction: Attribute<String> = "BeamLimitingDeviceDirection",
        patient_support_direction: Attribute<String> = "PatientSupportDirection",
        table_top_eccentric_direction: Attribute<String> = "TableTopEccentricDirection",
        table_top_pitch_direction: Attribute<String> = "TableTopPitchDirection",
        table_top_roll_direction: Attribute<String> = "TableTopRollDirection",
        gantry_pitch_direction: Attribute<String> = "GantryPitchDirection",
        device_positions: Vec<DevicePosition> = "DevicePositions",
        range_shifter_settings: Vec<RangeShifterSetting> = "RangeShifterSettings",
    }
}
model! {
    RecordControlPoint "RecordControlPoint" {
        referenced_index: Attribute<i64> = "ReferencedIndex",
        date: String = "Date",
        time: String = "Time",
        specified_meterset: Attribute<f64> = "SpecifiedMeterset",
        delivered_meterset: f64 = "DeliveredMeterset",
        rate_set: Attribute<f64> = "RateSet",
        rate_delivered: Attribute<f64> = "RateDelivered",
        energy_unit: String = "EnergyUnit",
        state: RecordMachineState = "State",
        encoded: Vec<Tag> = "Encoded",
        scan_spot_tune_id: String = "ScanSpotTuneID",
        spot_positions: Vec<f32> = "SpotPositions",
        spot_metersets: Vec<f32> = "SpotMetersets",
        number_of_paintings: Attribute<i64> = "NumberOfPaintings",
        spot_reordered: Attribute<String> = "SpotReordered",
        spot_prescribed_indices: Vec<i64> = "SpotPrescribedIndices",
    }
}
model! {
    RecordedBrachySource "RecordedBrachySource" {
        brachy_source: BrachySource = "BrachySource",
        manufacturer: Attribute<String> = "Manufacturer",
        recorded_serial_number: Attribute<String> = "RecordedSerialNumber",
        model_id: String = "ModelID",
    }
}
model! {
    RecordBrachySession "RecordBrachySession" {
        referenced_setup_number: Attribute<i64> = "ReferencedSetupNumber",
        typ: String = "Type",
        name: String = "Name",
        check: String = "Check",
        fraction_number: Attribute<i64> = "FractionNumber",
        delivery_type: Attribute<String> = "DeliveryType",
        verification_status: Attribute<String> = "VerificationStatus",
        termination_status: String = "TerminationStatus",
        total_reference_air_kerma: f64 = "TotalReferenceAirKerma",
        verification_images: Vec<Reference> = "VerificationImages",
        measured_doses: Vec<RecordDoseValue> = "MeasuredDoses",
        calculated_doses: Vec<RecordDoseValue> = "CalculatedDoses",
        channels: Vec<RecordBrachyChannel> = "Channels",
    }
}
model! {
    RecordSafePosition "RecordSafePosition" {
        exit_date: String = "ExitDate",
        exit_time: String = "ExitTime",
        return_date: String = "ReturnDate",
        return_time: String = "ReturnTime",
    }
}
model! {
    RecordBrachyChannel "RecordBrachyChannel" {
        number: i64 = "Number",
        source_number: i64 = "SourceNumber",
        referenced_channel_number: Attribute<i64> = "ReferencedChannelNumber",
        movement: String = "Movement",
        length: Attribute<f64> = "Length",
        effective_length: Attribute<f64> = "EffectiveLength",
        inner_length: Attribute<f64> = "InnerLength",
        afterloader_channel_id: String = "AfterloaderChannelID",
        specified_time: f64 = "SpecifiedTime",
        delivered_time: f64 = "DeliveredTime",
        specified_pulses: Attribute<i64> = "SpecifiedPulses",
        delivered_pulses: Attribute<i64> = "DeliveredPulses",
        specified_pulse_interval: Attribute<f64> = "SpecifiedPulseInterval",
        delivered_pulse_interval: Attribute<f64> = "DeliveredPulseInterval",
        transfer_tube_number: Attribute<i64> = "TransferTubeNumber",
        transfer_tube_length: Attribute<f64> = "TransferTubeLength",
        safe_position: RecordSafePosition = "SafePosition",
        applicators: Vec<RecordedSourceApplicator> = "Applicators",
        measured_doses: Vec<RecordDoseValue> = "MeasuredDoses",
        calculated_doses: Vec<RecordDoseValue> = "CalculatedDoses",
        control_points: Vec<RecordBrachyControlPoint> = "ControlPoints",
        pulses: Vec<RecordBrachyPulse> = "Pulses",
    }
}
model! {
    RecordedSourceApplicator "RecordedSourceApplicator" {
        number: Attribute<i64> = "Number",
        id: Attribute<String> = "ID",
        typ: String = "Type",
        name: String = "Name",
        length: f64 = "Length",
        tip_length: Attribute<f64> = "TipLength",
        step_size: Attribute<f64> = "StepSize",
    }
}
model! {
    RecordBrachyControlPoint "RecordBrachyControlPoint" {
        referenced_index: Attribute<i64> = "ReferencedIndex",
        date: String = "Date",
        time: String = "Time",
        relative_position: f64 = "RelativePosition",
    }
}
model! {
    RecordBrachyPulse "RecordBrachyPulse" {
        number: i64 = "Number",
        safe_position: RecordSafePosition = "SafePosition",
        control_points: Vec<RecordBrachyControlPoint> = "ControlPoints",
    }
}
model! {
    RecordSummary "RecordSummary" {
        status: String = "Status",
        comment: String = "Comment",
        first_date: Attribute<String> = "FirstDate",
        most_recent_date: Attribute<String> = "MostRecentDate",
        fraction_groups: Vec<RecordFractionGroupSummary> = "FractionGroups",
        measured_doses: Vec<RecordSummaryDose> = "MeasuredDoses",
        calculated_doses: Vec<RecordSummaryDose> = "CalculatedDoses",
    }
}
model! {
    RecordFractionGroupSummary "RecordFractionGroupSummary" {
        referenced_number: Attribute<i64> = "ReferencedNumber",
        typ: Attribute<String> = "Type",
        planned: Attribute<i64> = "Planned",
        delivered: Attribute<i64> = "Delivered",
        fractions: Vec<RecordFractionStatus> = "Fractions",
    }
}
model! {
    RecordFractionStatus "RecordFractionStatus" {
        number: i64 = "Number",
        clinical_number: Attribute<i64> = "ClinicalNumber",
        date: Attribute<String> = "Date",
        time: Attribute<String> = "Time",
        termination_status: Attribute<String> = "TerminationStatus",
    }
}
model! {
    RecordSummaryDose "RecordSummaryDose" {
        plan_number: Attribute<i64> = "PlanNumber",
        description: String = "Description",
        cumulative_dose: f64 = "CumulativeDose",
    }
}
const RECORD_CLASSES: [&str; 4] = [
    RT_BEAMS_TREATMENT_RECORD_STORAGE,
    RT_ION_BEAMS_TREATMENT_RECORD_STORAGE,
    RT_BRACHY_TREATMENT_RECORD_STORAGE,
    RT_TREATMENT_SUMMARY_RECORD_STORAGE,
];
pub(crate) fn is_record_class(sop: &str) -> bool {
    RECORD_CLASSES.contains(&sop)
}
pub fn read_treatment_record(f: &File, opts: ReadOptions) -> Result<TreatmentRecord, Error> {
    let opts = file_read_options(f, opts)?;
    read_treatment_record_dataset(&f.dataset, opts)
}
pub fn read_treatment_record_dataset(
    obj: &Object,
    opts: ReadOptions,
) -> Result<TreatmentRecord, Error> {
    let b = preflight(obj, &opts.limits, false)?;
    let obj = &fresh_facade(obj, false);
    let sop = obj.get_uid(tagSOPClassUID).unwrap_or_default();
    if !is_record_class(&sop) {
        return Err(Error::bare(ErrorKind::UnsupportedSopClass));
    }
    rs_profile(obj, &sop, "")?;
    if opts.strict_iod {
        strict_object(obj, &b.limits)?;
    }
    let r = &mut Reader::new(b.limits.max_problems);
    let mut rec = TreatmentRecord {
        sop_class_uid: sop.clone(),
        sop_instance_uid: r.uid(obj, tagSOPInstanceUID),
        study_instance_uid: r.uid(obj, tagStudyInstanceUID),
        series_instance_uid: r.uid(obj, tagSeriesInstanceUID),
        instance_number: r.required_int(obj, ft(0x00200013), "", "Instance Number"),
        date: rs_text(r, obj, ft(0x30080250)),
        time: rs_text(r, obj, ft(0x30080251)),
        timezone_offset: rs_text(r, obj, ft(0x00080201)),
        content_origin: rs_text(r, obj, ft(0x300a0709)),
        fraction_group_number: r.int_attribute(obj, ft(0x300c0022), ""),
        fractions_planned: r.int_attribute(obj, tagNumberOfFractionsPlanned, ""),
        primary_dosimeter_unit: r.code(obj, tagPrimaryDosimeterUnit),
        brachy_technique: r.code(obj, tagBrachyTreatmentTechnique),
        brachy_type: r.code(obj, tagBrachyTreatmentType),
        ..TreatmentRecord::default()
    };
    if r.code(obj, tagModality) != "RTRECORD" {
        r.fail(tagModality.to_string(), "Modality is not RTRECORD");
    }
    for (t, s) in [
        (tagSOPInstanceUID, &rec.sop_instance_uid),
        (tagStudyInstanceUID, &rec.study_instance_uid),
        (tagSeriesInstanceUID, &rec.series_instance_uid),
    ] {
        if !rs_uid(s) {
            r.fail(t.to_string(), "UID is missing or invalid");
        }
    }
    rs_required_presence(r, obj, ft(0x30080250), "");
    rs_required_presence(r, obj, ft(0x30080251), "");
    rs_required_presence(r, obj, tagReferencedRTPlanSequence, "");
    let refs = rs_references(r, obj, tagReferencedRTPlanSequence, "");
    if refs.len() > 1 {
        r.fail(
            tagReferencedRTPlanSequence.to_string(),
            "plan reference item count",
        );
    }
    if refs.len() == 1 {
        rec.referenced_plan = Some(refs[0].clone());
        if !one_of(
            &refs[0].sop_class_uid,
            &[RT_PLAN_STORAGE, RT_ION_PLAN_STORAGE],
        ) {
            r.fail(
                tagReferencedRTPlanSequence.to_string(),
                "referenced SOP is not an admitted plan",
            );
        }
    }
    rec.related_records = rs_references(r, obj, ft(0x30080030), "");
    rs_check_date_time(
        r,
        rec.date.value.as_deref(),
        rec.time.value.as_deref(),
        ft(0x30080250),
        ft(0x30080251),
        "",
    );
    if rec
        .timezone_offset
        .value
        .as_deref()
        .is_some_and(|v| !rs_timezone(v))
    {
        r.fail(ft(0x00080201).to_string(), "timezone offset is invalid");
    }
    let machines = r.sequence(obj, ft(0x300a0206));
    if sop != RT_TREATMENT_SUMMARY_RECORD_STORAGE && machines.len() != 1 {
        r.fail(
            ft(0x300a0206).to_string(),
            "one treatment machine is required",
        );
    }
    if machines.len() > 1 {
        r.fail(ft(0x300a0206).to_string(), "machine item count");
    }
    if machines.len() == 1 {
        let m = &machines[0];
        let p = item("", ft(0x300a0206), 0);
        let machine = RecordMachine {
            name: rs_text(r, m, tagTreatmentMachineName),
            manufacturer: rs_text(r, m, ft(0x00080070)),
            institution: rs_text(r, m, ft(0x00080080)),
            model: rs_text(r, m, ft(0x00081090)),
            serial_number: rs_text(r, m, ft(0x00181000)),
            table_top_position_alignment_uid: r.uid(m, ft(0x300a0054)),
        };
        for t in [
            tagTreatmentMachineName,
            ft(0x00080070),
            ft(0x00080080),
            ft(0x00081090),
            ft(0x00181000),
        ] {
            rs_required_presence(r, m, t, &p);
        }
        if !machine.table_top_position_alignment_uid.is_empty()
            && !rs_uid(&machine.table_top_position_alignment_uid)
        {
            r.fail(join(&p, ft(0x300a0054)), "alignment UID is invalid");
        }
        rec.machine = Some(machine);
    }
    for (i, o) in r.sequence(obj, tagPatientSetupSequence).iter().enumerate() {
        let p = item("", tagPatientSetupSequence, i);
        let s = PatientSetup {
            number: r.required_int(o, tagPatientSetupNumber, &p, "Patient Setup Number"),
            label: r.str(o, tagPatientSetupLabel),
            patient_position: r.code(o, tagPatientPosition),
            patient_additional_position: r.str(o, tagPatientAdditionalPosition),
            setup_technique: r.code(o, tagSetupTechnique),
        };
        for old in &rec.patient_setups {
            if old.number == s.number {
                r.fail(
                    join(&p, tagPatientSetupNumber),
                    "patient setup number is duplicated",
                );
            }
        }
        rec.patient_setups.push(s);
    }
    rec.measured_dose_references = rs_dose_references(r, obj, false);
    rec.calculated_dose_references = rs_dose_references(r, obj, true);
    match sop.as_str() {
        RT_BEAMS_TREATMENT_RECORD_STORAGE | RT_ION_BEAMS_TREATMENT_RECORD_STORAGE => {
            rs_read_beams(r, obj, &mut rec)
        }
        RT_BRACHY_TREATMENT_RECORD_STORAGE => rs_read_brachy(r, obj, &mut rec),
        _ => rec.summary = Some(rs_read_summary(r, obj)),
    }
    if rec.summary.is_none() && obj.has(ft(0x30080200)) {
        rec.summary = Some(rs_read_summary(r, obj));
    }
    rs_validate_doses(r, &rec);
    if !r.errors.is_empty() {
        return Err(Error::validation(std::mem::take(&mut r.errors)));
    }
    rec.warnings = std::mem::take(&mut r.warnings);
    Ok(rec)
}
pub(crate) fn rs_text(r: &Reader, o: &Object, t: Tag) -> Attribute<String> {
    if !o.has(t) {
        return Attribute::default();
    }
    let s = r.str(o, t);
    if s.is_empty() {
        return Attribute {
            present: true,
            value: None,
        };
    }
    Attribute::of(s)
}
fn rs_required_presence(r: &mut Reader, o: &Object, t: Tag, p: &str) {
    if !o.has(t) {
        r.fail(join(p, t), "required attribute is absent");
    }
}
fn rs_uid(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 || !s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return false;
    }
    s.split('.')
        .all(|part| !part.is_empty() && !(part.len() > 1 && part.starts_with('0')))
}
fn rs_references(r: &mut Reader, o: &Object, t: Tag, p: &str) -> Vec<Reference> {
    let out = r.references(o, t);
    let mut seen: HashSet<&str> = HashSet::new();
    for (i, reference) in out.iter().enumerate() {
        let ip = item(p, t, i);
        if !rs_uid(&reference.sop_class_uid) || !rs_uid(&reference.sop_instance_uid) {
            r.fail(ip.clone(), "reference identity is missing or invalid");
        }
        if !seen.insert(&reference.sop_instance_uid) {
            r.fail(ip, "reference identity is duplicated");
        }
    }
    out
}
fn digits(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_digit())
}
fn rs_date(s: &str) -> bool {
    if s.len() != 8 || !digits(s) {
        return false;
    }
    let year: i64 = s[..4].parse().unwrap_or(0);
    let month: i64 = s[4..6].parse().unwrap_or(0);
    let day: i64 = s[6..].parse().unwrap_or(0);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    (1..=days).contains(&day)
}
fn rs_time(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 2 {
        return false;
    }
    let base = parts[0];
    if base.len() != 2 && base.len() != 4 && base.len() != 6 {
        return false;
    }
    if parts.len() == 2
        && (base.len() != 6 || parts[1].is_empty() || parts[1].len() > 6 || !digits(parts[1]))
    {
        return false;
    }
    if !digits(base) {
        return false;
    }
    for i in (0..base.len()).step_by(2) {
        let n: i64 = base[i..i + 2].parse().unwrap_or(0);
        let max = match i {
            0 => 23,
            4 => 60,
            _ => 59,
        };
        if n > max {
            return false;
        }
    }
    true
}
fn rs_timezone(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 5 || !(b[0] == b'+' || b[0] == b'-') || !b[1..].iter().all(u8::is_ascii_digit) {
        return false;
    }
    let h: i64 = s[1..3].parse().unwrap_or(0);
    let m: i64 = s[3..].parse().unwrap_or(0);
    m < 60
        && (b[0] == b'+' && h <= 14 && (h < 14 || m == 0)
            || b[0] == b'-' && h <= 12 && (h < 12 || m == 0))
}
fn rs_check_date_time(r: &mut Reader, d: Option<&str>, t: Option<&str>, dt: Tag, tt: Tag, p: &str) {
    if d.is_some_and(|d| !rs_date(d)) {
        r.fail(join(p, dt), "date is invalid");
    }
    if t.is_some_and(|t| !rs_time(t)) {
        r.fail(join(p, tt), "time is invalid");
    }
}
fn rs_positive_int(r: &mut Reader, a: &Attribute<i64>, t: Tag, p: &str, required: bool) {
    if required && a.value.is_none() || a.value.is_some_and(|v| v <= 0) {
        r.fail(join(p, t), "positive integer is missing or invalid");
    }
}
fn rs_profile(o: &Object, sop: &str, p: &str) -> Result<(), Error> {
    let cleaned = |t: Tag| o.get_string(t).map(|v| clean(&v).to_string());
    if p.is_empty() {
        let origin = cleaned(ft(0x300a0709)).unwrap_or_default();
        if !origin.is_empty() && !one_of(&origin, &["DEVICE", "SIMULATION"]) {
            return Err(path_error(
                ErrorKind::UnsupportedFeature,
                &ft(0x300a0709).to_string(),
                "content origin is outside DEVICE/SIMULATION profile",
            ));
        }
        for seq in [ft(0x30080020), ft(0x30080021), ft(0x30080110)] {
            if o.has(seq)
                && (sop == RT_BEAMS_TREATMENT_RECORD_STORAGE && seq != ft(0x30080020)
                    || sop == RT_ION_BEAMS_TREATMENT_RECORD_STORAGE && seq != ft(0x30080021)
                    || sop == RT_BRACHY_TREATMENT_RECORD_STORAGE && seq != ft(0x30080110)
                    || sop == RT_TREATMENT_SUMMARY_RECORD_STORAGE)
            {
                return Err(path_error(
                    ErrorKind::InvalidObject,
                    &seq.to_string(),
                    "session does not match SOP class",
                ));
            }
        }
    }
    let brachy: [(Tag, &[&str]); 2] = [
        (
            tagBrachyTreatmentTechnique,
            &[
                "INTRACAVITARY",
                "INTERSTITIAL",
                "INTRALUMENARY",
                "INTRAVASCULAR",
                "CONTACT",
            ],
        ),
        (ft(0x300a0288), &["STEPWISE", "FIXED"]),
    ];
    for (t, allow) in brachy {
        if let Some(v) = cleaned(t)
            && !v.is_empty()
            && !one_of(&v, allow)
        {
            return Err(path_error(
                ErrorKind::UnsupportedFeature,
                &join(p, t),
                "brachy feature is outside profile",
            ));
        }
    }
    if sop == RT_ION_BEAMS_TREATMENT_RECORD_STORAGE {
        let ion: [(Tag, &[&str]); 2] = [
            (tagRadiationType, &["PROTON", "ION"]),
            (ft(0x300a0308), &["NONE", "MODULATED"]),
        ];
        for (t, allow) in ion {
            if let Some(v) = cleaned(t)
                && !v.is_empty()
                && !one_of(&v, allow)
            {
                return Err(path_error(
                    ErrorKind::UnsupportedFeature,
                    &join(p, t),
                    "ion feature is outside profile",
                ));
            }
        }
    }
    for t in [
        tagNumberOfWedges,
        tagNumberOfCompensators,
        tagNumberOfBoli,
        tagNumberOfBlocks,
        ft(0x300a0330),
        ft(0x300a0340),
    ] {
        let mut r = Reader::default();
        if r.int_opt(o, t, p).is_some_and(|n| n != 0) {
            return Err(path_error(
                ErrorKind::UnsupportedFeature,
                &join(p, t),
                "nonzero accessory count is outside profile",
            ));
        }
    }
    for t in [
        0x30080060, 0x30080068, 0x300800a1, 0x300800a2, 0x300800b0, 0x300800c0, 0x300800d0,
        0x300800d1, 0x300c00b0, 0x300a0107, 0x300a0420, 0x300a0116, 0x300a03ac, 0x300800f4,
        0x300800f6, 0x300a0370, 0x300a0380, 0x300a0506, 0x300c0120, 0x300a065a, 0x300c0125,
        0x30080120, 0x30080150,
    ] {
        if o.has(ft(t)) {
            return Err(path_error(
                ErrorKind::UnsupportedFeature,
                &join(p, ft(t)),
                "accessory, correction, or advanced delivery structure is outside profile",
            ));
        }
    }
    let flag = o.get_string(ft(0x300800a3)).unwrap_or_default();
    if !flag.is_empty() && flag != "NO" {
        return Err(path_error(
            ErrorKind::UnsupportedFeature,
            &join(p, ft(0x300800a3)),
            "enhanced devices are outside profile",
        ));
    }
    for e in o.elements() {
        if let Value::Sequence(items) = &e.value {
            for (i, it) in items.iter().enumerate() {
                let mut child = Object::from_elements(it.elements.clone());
                child.set_value_big_endian(o.value_big_endian());
                rs_profile(&child, sop, &item(p, e.tag(), i))?;
            }
        }
    }
    Ok(())
}
fn rs_dose_references(r: &mut Reader, o: &Object, calc: bool) -> Vec<RecordDoseReference> {
    let (seq, num, val, desc) = if calc {
        (
            ft(0x30080070),
            ft(0x30080072),
            ft(0x30080076),
            ft(0x30080074),
        )
    } else {
        (
            ft(0x30080010),
            ft(0x30080064),
            ft(0x30080016),
            ft(0x30080012),
        )
    };
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (i, d) in r.sequence(o, seq).iter().enumerate() {
        let p = item("", seq, i);
        let v = RecordDoseReference {
            number: r.int_attribute(d, num, &p),
            plan_number: r.int_attribute(d, tagReferencedDoseReferenceNumber, &p),
            value: r.float_attribute(d, val, &p),
            units: r.code(d, ft(0x30040002)),
            typ: rs_text(r, d, ft(0x30080014)),
            description: r.str(d, desc),
        };
        rs_dose_choice(r, &v.number, &v.plan_number, num, &p);
        if !seen.insert(rs_dose_key(&v.number, &v.plan_number)) {
            r.fail(p.clone(), "dose reference is duplicated");
        }
        rs_required_presence(r, d, val, &p);
        r.attr_nonnegative(&v.value, val, &p, false);
        if !calc {
            if !one_of(&v.units, &["GY", "RELATIVE"]) {
                r.fail(join(&p, ft(0x30040002)), "measured dose unit is invalid");
            }
            rs_required_presence(r, d, ft(0x30080014), &p);
        }
        out.push(v);
    }
    out
}
pub(crate) fn rs_dose_key(local: &Attribute<i64>, plan: &Attribute<i64>) -> String {
    if let Some(n) = local.value {
        return format!("L{n}");
    }
    if let Some(n) = plan.value {
        return format!("P{n}");
    }
    String::new()
}
fn rs_dose_choice(r: &mut Reader, local: &Attribute<i64>, plan: &Attribute<i64>, t: Tag, p: &str) {
    if local.value.is_none() == plan.value.is_none() {
        r.fail(
            join(p, t),
            "exactly one local or plan dose reference is required",
        );
    }
    rs_positive_int(r, local, t, p, false);
    rs_positive_int(r, plan, tagReferencedDoseReferenceNumber, p, false);
}
fn rs_dose_values(r: &mut Reader, o: &Object, p: &str, calc: bool) -> Vec<RecordDoseValue> {
    let (seq, num, val) = if calc {
        (ft(0x30080090), ft(0x30080092), ft(0x30080076))
    } else {
        (ft(0x30080080), ft(0x30080082), ft(0x30080016))
    };
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (i, d) in r.sequence(o, seq).iter().enumerate() {
        let dp = item(p, seq, i);
        let v = RecordDoseValue {
            plan_number: r.int_attribute(d, tagReferencedDoseReferenceNumber, &dp),
            local_number: r.int_attribute(d, num, &dp),
            value: r.required_float(d, val, &dp),
        };
        rs_dose_choice(r, &v.local_number, &v.plan_number, num, &dp);
        if v.value < 0.0 {
            r.fail(join(&dp, val), "dose value is negative");
        }
        if !seen.insert(rs_dose_key(&v.local_number, &v.plan_number)) {
            r.fail(dp, "dose reference is duplicated");
        }
        out.push(v);
    }
    out
}
fn rs_read_beams(r: &mut Reader, o: &Object, rec: &mut TreatmentRecord) {
    let ion = rec.sop_class_uid == RT_ION_BEAMS_TREATMENT_RECORD_STORAGE;
    let (seq, cpseq) = if ion {
        (ft(0x30080021), ft(0x30080041))
    } else {
        (ft(0x30080020), ft(0x30080040))
    };
    rs_required_presence(r, o, tagNumberOfFractionsPlanned, "");
    if !one_of(&rec.primary_dosimeter_unit, &["MU", "MINUTE"]) && !ion
        || ion && !one_of(&rec.primary_dosimeter_unit, &["MU", "NP"])
    {
        r.fail(
            tagPrimaryDosimeterUnit.to_string(),
            "primary dosimeter unit is outside profile",
        );
    }
    let items = r.sequence(o, seq);
    if items.is_empty() {
        r.fail(seq.to_string(), "beam sessions are missing");
    }
    for (i, b) in items.iter().enumerate() {
        let p = &item("", seq, i);
        let mut v = RecordBeamSession {
            referenced_beam_number: r.int_attribute(b, tagReferencedBeamNumber, p),
            referenced_patient_setup_number: r.int_attribute(b, tagReferencedPatientSetupNumber, p),
            referenced_tolerance_table_number: r.int_attribute(
                b,
                tagReferencedToleranceTableNumber,
                p,
            ),
            name: r.str(b, tagBeamName),
            description: r.str(b, tagBeamDescription),
            typ: r.required_text(b, tagBeamType, p),
            radiation_type: r.required_text(b, tagRadiationType, p),
            fraction_number: r.int_attribute(b, ft(0x30080022), p),
            clinical_fraction_number: r.int_attribute(b, ft(0x300a0705), p),
            delivery_type: rs_text(r, b, tagTreatmentDeliveryType),
            verification_status: rs_text(r, b, ft(0x3008002c)),
            termination_status: r.required_text(b, ft(0x3008002a), p),
            specified_primary_meterset: r.float_attribute(b, ft(0x30080032), p),
            specified_secondary_meterset: r.float_attribute(b, ft(0x30080033), p),
            delivered_primary_meterset: r.float_attribute(b, ft(0x30080036), p),
            delivered_secondary_meterset: r.float_attribute(b, ft(0x30080037), p),
            specified_treatment_time: r.float_attribute(b, ft(0x3008003a), p),
            delivered_treatment_time: r.float_attribute(b, ft(0x3008003b), p),
            source_axis_distance: r.float_attribute(b, tagSourceAxisDistance, p),
            atomic_number: r.int_attribute(b, ft(0x300a0304), p),
            mass_number: r.int_attribute(b, ft(0x300a0302), p),
            charge_state: r.int_attribute(b, ft(0x300a0306), p),
            scan_mode: r.code(b, ft(0x300a0308)),
            ..RecordBeamSession::default()
        };
        rs_validate_session(
            r,
            b,
            p,
            &v.fraction_number,
            &v.delivery_type,
            &v.verification_status,
            &v.termination_status,
        );
        if !one_of(&v.typ, &["STATIC", "DYNAMIC"]) {
            r.fail(join(p, tagBeamType), "beam type is invalid");
        }
        if ion {
            if !one_of(&v.radiation_type, &["PROTON", "ION"]) {
                r.fail(
                    join(p, tagRadiationType),
                    "ion radiation type is outside profile",
                );
            }
            v.patient_support_type = r.required_text(b, ft(0x300a0350), p);
            if !one_of(&v.scan_mode, &["NONE", "MODULATED"]) {
                r.fail(join(p, ft(0x300a0308)), "scan mode is outside profile");
            }
            rs_positive_int(
                r,
                &v.referenced_beam_number,
                tagReferencedBeamNumber,
                p,
                true,
            );
            if v.name.is_empty() {
                r.fail(join(p, tagBeamName), "ion beam name is missing");
            }
            if v.radiation_type == "ION" {
                match (
                    v.atomic_number.value,
                    v.mass_number.value,
                    v.charge_state.value,
                ) {
                    (Some(atomic), Some(mass), Some(charge)) => {
                        if species_inconsistent(atomic, mass, charge) {
                            r.fail(join(p, ft(0x300a0306)), "ion species is inconsistent");
                        }
                    }
                    _ => r.fail(join(p, ft(0x300a0302)), "ion species is incomplete"),
                }
            }
        } else if !one_of(
            &v.radiation_type,
            &["PHOTON", "ELECTRON", "NEUTRON", "PROTON"],
        ) {
            r.fail(
                join(p, tagRadiationType),
                "radiation type is outside profile",
            );
        }
        for t in [
            tagNumberOfWedges,
            tagNumberOfCompensators,
            tagNumberOfBoli,
            tagNumberOfBlocks,
        ] {
            let a = r.int_attribute(b, t, p);
            if t == tagNumberOfWedges || ion {
                rs_required_presence(r, b, t, p);
                if a.value.is_none() {
                    r.fail(join(p, t), "accessory count is missing");
                }
            }
            if a.value.is_some_and(|n| n != 0) {
                r.fail(join(p, t), "nonzero accessory count is outside profile");
            }
        }
        if ion {
            for t in [ft(0x300a0330), ft(0x300a0340)] {
                let n = r.required_int(b, t, p, "Accessory Count");
                if n != 0 {
                    r.fail(join(p, t), "nonzero accessory count is outside profile");
                }
            }
        }
        for (j, d) in r.sequence(b, ft(0x300800a0)).iter().enumerate() {
            let dp = item(p, ft(0x300800a0), j);
            let device = BeamLimitingDevice {
                typ: r.required_text(d, tagRTBeamLimitingDeviceType, &dp),
                pairs: r.required_int(d, tagNumberOfLeafJawPairs, &dp, "Number of Leaf/Jaw Pairs"),
                ..BeamLimitingDevice::default()
            };
            if !one_of(&device.typ, &["X", "Y", "ASYMX", "ASYMY", "MLCX", "MLCY"])
                || device.pairs <= 0
                || !device.is_mlc() && device.pairs != 1
            {
                r.fail(dp.clone(), "beam limiting device is invalid");
            }
            for old in &v.devices {
                if old.typ == device.typ {
                    r.fail(dp.clone(), "beam limiting device is duplicated");
                }
            }
            v.devices.push(device);
        }
        if !ion && v.devices.is_empty() {
            r.fail(
                join(p, ft(0x300800a0)),
                "legacy beam limiting devices are missing",
            );
        }
        if ion {
            let snouts = r.sequence(b, ft(0x300800f0));
            if snouts.len() > 1 {
                r.fail(join(p, ft(0x300800f0)), "snout item count");
            }
            if snouts.len() == 1 {
                v.snout_id =
                    r.required_text(&snouts[0], ft(0x300a030f), &item(p, ft(0x300800f0), 0));
            }
            for (j, s) in r.sequence(b, ft(0x300800f2)).iter().enumerate() {
                let sp = item(p, ft(0x300800f2), j);
                let sh = RecordedRangeShifter {
                    number: r.required_int(
                        s,
                        ft(0x300c0100),
                        &sp,
                        "Referenced Range Shifter Number",
                    ),
                    id: r.required_text(s, ft(0x300a0318), &sp),
                };
                for old in &v.range_shifters {
                    if old.number == sh.number {
                        r.fail(sp.clone(), "range shifter reference is duplicated");
                    }
                }
                v.range_shifters.push(sh);
            }
            let n = r.required_int(b, ft(0x300a0312), p, "Number of Range Shifters");
            if n != v.range_shifters.len() as i64 {
                r.fail(
                    join(p, ft(0x300800f2)),
                    "range shifter count is inconsistent",
                );
            }
        }
        for (a, t) in [
            (&v.specified_primary_meterset, ft(0x30080032)),
            (&v.specified_secondary_meterset, ft(0x30080033)),
            (&v.delivered_primary_meterset, ft(0x30080036)),
            (&v.delivered_secondary_meterset, ft(0x30080037)),
            (&v.specified_treatment_time, ft(0x3008003a)),
            (&v.delivered_treatment_time, ft(0x3008003b)),
            (&v.source_axis_distance, tagSourceAxisDistance),
        ] {
            r.attr_nonnegative(a, t, p, false);
        }
        v.measured_doses = rs_dose_values(r, b, p, false);
        v.calculated_doses = rs_dose_values(r, b, p, true);
        v.verification_images = rs_references(r, b, ft(0x300c0040), p);
        let points = r.sequence(b, cpseq);
        let n = r.required_int(b, tagNumberOfControlPoints, p, "Number of Control Points");
        if n != points.len() as i64 || n < 2 {
            r.fail(
                join(p, tagNumberOfControlPoints),
                "control point count is inconsistent",
            );
        }
        let mut state = RecordMachineState::default();
        for (j, c) in points.iter().enumerate() {
            let pp = &item(p, cpseq, j);
            let mut cp = RecordControlPoint {
                referenced_index: r.int_attribute(c, ft(0x300c00f0), pp),
                date: r.required_text(c, ft(0x30080024), pp),
                time: r.required_text(c, ft(0x30080025), pp),
                specified_meterset: r.float_attribute(c, ft(0x30080042), pp),
                delivered_meterset: r.required_float(c, ft(0x30080044), pp),
                energy_unit: r.code(c, ft(0x300a0015)),
                ..RecordControlPoint::default()
            };
            rs_check_date_time(
                r,
                Some(&cp.date),
                Some(&cp.time),
                ft(0x30080024),
                ft(0x30080025),
                pp,
            );
            rs_required_presence(r, c, ft(0x30080042), pp);
            if ion {
                for (t, beam_value) in [
                    (ft(0x300a0302), v.mass_number.value),
                    (ft(0x300a0304), v.atomic_number.value),
                    (ft(0x300a0306), v.charge_state.value),
                ] {
                    if c.has(t) {
                        let a = r.int_attribute(c, t, pp);
                        if a.value.is_none() || beam_value.is_none() || a.value != beam_value {
                            r.fail(
                                join(pp, t),
                                "control point species differs from admitted beam",
                            );
                        }
                    }
                }
            }
            if cp.referenced_index.value.is_some_and(|n| n < 0)
                || ion && cp.referenced_index.value.is_none()
            {
                r.fail(
                    join(pp, ft(0x300c00f0)),
                    "control point reference is missing or invalid",
                );
            }
            if cp.delivered_meterset < 0.0
                || j > 0 && cp.delivered_meterset < v.control_points[j - 1].delivered_meterset
            {
                r.fail(
                    join(pp, ft(0x30080044)),
                    "delivered meterset decreases or is negative",
                );
            }
            r.attr_nonnegative(&cp.specified_meterset, ft(0x30080042), pp, false);
            let (rate_set, rate_delivered) = if ion {
                (ft(0x30080045), ft(0x30080046))
            } else {
                (tagDoseRateSet, ft(0x30080048))
            };
            cp.rate_set = r.float_attribute(c, rate_set, pp);
            cp.rate_delivered = r.float_attribute(c, rate_delivered, pp);
            if !ion {
                rs_required_presence(r, c, rate_set, pp);
                rs_required_presence(r, c, rate_delivered, pp);
            }
            r.attr_nonnegative(&cp.rate_set, rate_set, pp, false);
            r.attr_nonnegative(&cp.rate_delivered, rate_delivered, pp, false);
            rs_read_state(r, c, &mut state, &mut cp, pp, j == 0, ion, &v);
            cp.state = state.clone();
            if !ion && cp.state.energy.present {
                if cp.energy_unit.is_empty() && j > 0 && !c.has(tagNominalBeamEnergy) {
                    cp.energy_unit = v.control_points[j - 1].energy_unit.clone();
                }
                if !one_of(&cp.energy_unit, &["MV", "MEV"])
                    || v.radiation_type == "PHOTON" && cp.energy_unit != "MV"
                    || v.radiation_type == "ELECTRON" && cp.energy_unit != "MEV"
                {
                    r.fail(
                        join(pp, ft(0x300a0015)),
                        "nominal energy unit is missing or inconsistent",
                    );
                }
            }
            if ion && v.scan_mode == "MODULATED" {
                cp.scan_spot_tune_id = r.required_text(c, ft(0x300a0390), pp);
                let count = r.required_int(c, ft(0x300a0392), pp, "Number of Scan Spot Positions");
                cp.spot_positions = read_fl(r, c, ft(0x300a0394), pp);
                cp.spot_metersets = read_fl(r, c, ft(0x30080047), pp);
                cp.number_of_paintings = r.int_attribute(c, ft(0x300a039a), pp);
                cp.spot_reordered = rs_text(r, c, ft(0x300a0393));
                if count <= 0
                    || cp.spot_positions.len() as i64 != 2 * count
                    || cp.spot_metersets.len() as i64 != count
                {
                    r.fail(
                        join(pp, ft(0x300a0394)),
                        "delivered spot count is inconsistent",
                    );
                }
                rs_positive_int(r, &cp.number_of_paintings, ft(0x300a039a), pp, true);
                for m in &cp.spot_metersets {
                    if *m < 0.0 {
                        r.fail(
                            join(pp, ft(0x30080047)),
                            "delivered spot meterset is negative",
                        );
                    }
                }
                let reordered = cp.spot_reordered.value.as_deref();
                if reordered.is_some_and(|v| !one_of(v, &["YES", "NO"])) {
                    r.fail(join(pp, ft(0x300a0393)), "spot reordered flag is invalid");
                }
                cp.spot_prescribed_indices = rs_ints(r, c, ft(0x300a0391), pp);
                let indices = cp.spot_prescribed_indices.len() as i64;
                if reordered == Some("YES") && indices != count || indices != 0 && indices != count
                {
                    r.fail(
                        join(pp, ft(0x300a0391)),
                        "prescribed spot index count is inconsistent",
                    );
                }
                for n in &cp.spot_prescribed_indices {
                    if *n <= 0 {
                        r.fail(
                            join(pp, ft(0x300a0391)),
                            "prescribed spot index must be positive",
                        );
                    }
                }
            } else if ion && (c.has(ft(0x300a0394)) || c.has(ft(0x30080047))) {
                r.fail(pp.clone(), "spots are outside passive scanning profile");
            }
            v.control_points.push(cp);
        }
        if ion && v.scan_mode == "MODULATED" {
            for j in 0..v.control_points.len().saturating_sub(1) {
                let (cp, next) = (&v.control_points[j], &v.control_points[j + 1]);
                let mut sum = 0.0f64;
                for m in &cp.spot_metersets {
                    sum += f64::from(*m);
                }
                let tol = fmath::ldexp(sum, -23)
                    + fmath::ldexp(
                        cp.delivered_meterset.abs() + next.delivered_meterset.abs(),
                        -40,
                    );
                let differs = !finite(sum)
                    || (sum - (next.delivered_meterset - cp.delivered_meterset)).abs() > tol;
                if differs {
                    r.fail(
                        format!("{}/{}", item(p, cpseq, j), ft(0x30080047)),
                        "spot sum differs from delivered segment meterset",
                    );
                }
            }
        }
        rs_validate_metersets(r, &v, p, cpseq);
        rec.beam_sessions.push(v);
    }
}
fn rs_ints(r: &mut Reader, o: &Object, t: Tag, p: &str) -> Vec<i64> {
    if !o.has(t) {
        return Vec::new();
    }
    let Ok(values) = o.get_ints(t) else {
        r.fail(join(p, t), "integer list cannot be decoded");
        return Vec::new();
    };
    for n in &values {
        if *n < i64::from(i32::MIN) || *n > i64::from(i32::MAX) {
            r.fail(join(p, t), "integer is outside signed 32-bit range");
            return Vec::new();
        }
    }
    values
}
fn rs_validate_session(
    r: &mut Reader,
    o: &Object,
    p: &str,
    fraction: &Attribute<i64>,
    delivery: &Attribute<String>,
    verification: &Attribute<String>,
    termination: &str,
) {
    for t in [ft(0x30080022), tagTreatmentDeliveryType, ft(0x3008002c)] {
        rs_required_presence(r, o, t, p);
    }
    rs_positive_int(r, fraction, ft(0x30080022), p, false);
    if !one_of(termination, &["NORMAL", "OPERATOR", "MACHINE", "UNKNOWN"]) {
        r.fail(join(p, ft(0x3008002a)), "termination status is invalid");
    }
    if verification
        .value
        .as_deref()
        .is_some_and(|v| !one_of(v, &["VERIFIED", "VERIFIED_OVR", "NOT_VERIFIED"]))
    {
        r.fail(join(p, ft(0x3008002c)), "verification status is invalid");
    }
    if delivery.value.as_deref().is_some_and(|v| {
        !one_of(
            v,
            &[
                "TREATMENT",
                "OPEN_PORTFILM",
                "TRMT_PORTFILM",
                "CONTINUATION",
                "SETUP",
                "VERIFICATION",
            ],
        )
    }) {
        r.fail(
            join(p, tagTreatmentDeliveryType),
            "delivery type is outside profile",
        );
    }
}
pub(crate) fn rs_state_numbers(s: &mut RecordMachineState) -> [(Tag, &mut Attribute<f64>); 13] {
    [
        (tagNominalBeamEnergy, &mut s.energy),
        (tagGantryAngle, &mut s.gantry_angle),
        (tagBLDAngle, &mut s.beam_limiting_device_angle),
        (tagPatientSupportAngle, &mut s.patient_support_angle),
        (tagTableTopEccentricAngle, &mut s.table_top_eccentric_angle),
        (ft(0x300a0124), &mut s.table_top_eccentric_axis_distance),
        (tagTableTopPitchAngle, &mut s.table_top_pitch_angle),
        (tagTableTopRollAngle, &mut s.table_top_roll_angle),
        (ft(0x300a014a), &mut s.gantry_pitch_angle),
        (
            tagTableTopVerticalPosition,
            &mut s.table_top_vertical_position,
        ),
        (
            tagTableTopLongitudinalPosition,
            &mut s.table_top_longitudinal_position,
        ),
        (
            tagTableTopLateralPosition,
            &mut s.table_top_lateral_position,
        ),
        (ft(0x300a030d), &mut s.snout_position),
    ]
}
pub(crate) fn rs_state_texts(s: &mut RecordMachineState) -> [(Tag, &mut Attribute<String>); 7] {
    [
        (tagGantryRotationDirection, &mut s.gantry_direction),
        (
            tagBLDRotationDirection,
            &mut s.beam_limiting_device_direction,
        ),
        (
            tagPatientSupportRotationDir,
            &mut s.patient_support_direction,
        ),
        (
            tagTableTopEccentricRotationDir,
            &mut s.table_top_eccentric_direction,
        ),
        (ft(0x300a0142), &mut s.table_top_pitch_direction),
        (ft(0x300a0146), &mut s.table_top_roll_direction),
        (ft(0x300a014c), &mut s.gantry_pitch_direction),
    ]
}
#[allow(clippy::too_many_arguments)]
fn rs_read_state(
    r: &mut Reader,
    o: &Object,
    s: &mut RecordMachineState,
    cp: &mut RecordControlPoint,
    p: &str,
    first: bool,
    ion: bool,
    b: &RecordBeamSession,
) {
    for (t, slot) in rs_state_numbers(s) {
        let a = r.float_attribute(o, t, p);
        if a.present {
            *slot = a;
            cp.encoded.push(t);
        }
    }
    for (t, slot) in rs_state_texts(s) {
        let a = rs_text(r, o, t);
        if a.present {
            *slot = a;
            cp.encoded.push(t);
        }
        if slot
            .value
            .as_deref()
            .is_some_and(|v| !one_of(v, &["CW", "CC", "NONE"]))
        {
            r.fail(join(p, t), "rotation direction is invalid");
        }
    }
    if first {
        for t in [
            tagGantryAngle,
            tagGantryRotationDirection,
            tagBLDAngle,
            tagBLDRotationDirection,
            tagPatientSupportAngle,
            tagPatientSupportRotationDir,
        ] {
            r.required_text(o, t, p);
        }
        for t in [
            tagTableTopVerticalPosition,
            tagTableTopLongitudinalPosition,
            tagTableTopLateralPosition,
        ] {
            rs_required_presence(r, o, t, p);
        }
        if ion {
            r.required_float(o, tagNominalBeamEnergy, p);
            for t in [
                tagTableTopPitchAngle,
                ft(0x300a0142),
                tagTableTopRollAngle,
                ft(0x300a0146),
                ft(0x300a014a),
                ft(0x300a014c),
                ft(0x300a030d),
            ] {
                rs_required_presence(r, o, t, p);
            }
        } else {
            r.required_float(o, tagTableTopEccentricAngle, p);
            r.required_text(o, tagTableTopEccentricRotationDir, p);
        }
    }
    if s.energy.value.is_some_and(|v| v < 0.0) {
        r.fail(join(p, tagNominalBeamEnergy), "nominal energy is negative");
    }
    if o.has(tagBLDPositionSequence) {
        cp.encoded.push(tagBLDPositionSequence);
        let mut seen: HashSet<String> = HashSet::new();
        for (j, d) in r.sequence(o, tagBLDPositionSequence).iter().enumerate() {
            let dp = item(p, tagBLDPositionSequence, j);
            let v = DevicePosition {
                device_type: r.required_text(d, tagRTBeamLimitingDeviceType, &dp),
                positions: r.number_list(d, tagLeafJawPositions, &dp),
            };
            let mut pairs = 0;
            for def in &b.devices {
                if def.typ == v.device_type {
                    pairs = def.pairs;
                }
            }
            if pairs <= 0 || v.positions.len() as i64 != 2 * pairs || seen.contains(&v.device_type)
            {
                r.fail(dp, "device position reference or count is inconsistent");
            }
            seen.insert(v.device_type.clone());
            let mut replaced = false;
            for old in s.device_positions.iter_mut() {
                if old.device_type == v.device_type {
                    *old = v.clone();
                    replaced = true;
                }
            }
            if !replaced {
                s.device_positions.push(v);
            }
        }
    }
    if first && s.device_positions.len() != b.devices.len() {
        r.fail(
            join(p, tagBLDPositionSequence),
            "first device positions are incomplete",
        );
    }
    if o.has(ft(0x300a0360)) {
        cp.encoded.push(ft(0x300a0360));
        let mut seen: HashSet<i64> = HashSet::new();
        for (j, d) in r.sequence(o, ft(0x300a0360)).iter().enumerate() {
            let dp = item(p, ft(0x300a0360), j);
            let v = RangeShifterSetting {
                number: r.required_int(d, ft(0x300c0100), &dp, "Referenced Range Shifter Number"),
                setting: r.required_text(d, ft(0x300a0362), &dp),
                distance: r.float_attribute(d, ft(0x300a0364), &dp),
                water_equivalent_thickness: r.float_attribute(d, ft(0x300a0366), &dp),
            };
            let found = b.range_shifters.iter().any(|def| def.number == v.number);
            if !found || seen.contains(&v.number) {
                r.fail(dp, "range shifter reference is missing or duplicated");
            }
            seen.insert(v.number);
            let mut replaced = false;
            for old in s.range_shifter_settings.iter_mut() {
                if old.number == v.number {
                    *old = v.clone();
                    replaced = true;
                }
            }
            if !replaced {
                s.range_shifter_settings.push(v);
            }
        }
    }
    if first && s.range_shifter_settings.len() != b.range_shifters.len() {
        r.fail(
            join(p, ft(0x300a0360)),
            "first range shifter settings are incomplete",
        );
    }
}
fn rs_read_brachy(r: &mut Reader, o: &Object, rec: &mut TreatmentRecord) {
    rs_required_presence(r, o, tagNumberOfFractionsPlanned, "");
    if rec.brachy_technique == "PERMANENT" || rec.brachy_technique.is_empty() {
        r.fail(
            tagBrachyTreatmentTechnique.to_string(),
            "brachy technique is outside profile",
        );
    }
    if !one_of(&rec.brachy_type, &["HDR", "MDR", "LDR", "PDR", "MANUAL"]) {
        r.fail(
            tagBrachyTreatmentType.to_string(),
            "brachy type is outside profile",
        );
    }
    let mut sources: HashSet<i64> = HashSet::new();
    for (i, s) in r.sequence(o, ft(0x30080100)).iter().enumerate() {
        let p = &item("", ft(0x30080100), i);
        let number = r.required_int(s, ft(0x300a0212), p, "Source Number");
        let typ = r.required_text(s, ft(0x300a0214), p);
        let isotope_name = r.required_text(s, ft(0x300a0226), p);
        let isotope_half_life = r.float_attribute(s, ft(0x300a0228), p);
        let strength_units = r.code(s, ft(0x300a0229));
        let reference_air_kerma_rate = r.float_attribute(s, ft(0x300a022a), p);
        let strength = r.float_attribute(s, ft(0x300a022b), p);
        let strength_reference_date = r.required_text(s, ft(0x300a022c), p);
        let strength_reference_time = r.required_text(s, ft(0x300a022e), p);
        let serial_number = r.str(s, ft(0x30080105));
        let v = RecordedBrachySource {
            brachy_source: BrachySource {
                number,
                typ,
                serial_number,
                isotope_name,
                strength_units,
                isotope_half_life,
                reference_air_kerma_rate,
                strength,
                strength_reference_date,
                strength_reference_time,
            },
            manufacturer: rs_text(r, s, ft(0x300a0216)),
            recorded_serial_number: rs_text(r, s, ft(0x30080105)),
            model_id: r.str(s, ft(0x300a021b)),
        };
        let src = &v.brachy_source;
        if !sources.insert(src.number) {
            r.fail(join(p, ft(0x300a0212)), "source number is duplicated");
        }
        if !one_of(&src.typ, &["POINT", "LINE", "CYLINDER", "SPHERE"]) {
            r.fail(join(p, ft(0x300a0214)), "source type is outside profile");
        }
        for t in [ft(0x300a0216), ft(0x30080105)] {
            rs_required_presence(r, s, t, p);
        }
        if src.isotope_half_life.value.is_none() || src.reference_air_kerma_rate.value.is_none() {
            r.fail(p.clone(), "source half life or air kerma is missing");
        }
        r.attr_nonnegative(&src.isotope_half_life, ft(0x300a0228), p, true);
        r.attr_nonnegative(&src.reference_air_kerma_rate, ft(0x300a022a), p, false);
        r.attr_nonnegative(&src.strength, ft(0x300a022b), p, false);
        if !src.strength_units.is_empty()
            && !one_of(&src.strength_units, &["AIR_KERMA_RATE", "DOSE_RATE_WATER"])
        {
            r.fail(
                join(p, ft(0x300a0229)),
                "source strength unit is outside profile",
            );
        }
        if src.strength_units == "DOSE_RATE_WATER"
            && (src.strength.value.is_none()
                || src.reference_air_kerma_rate.value.is_some_and(|v| v != 0.0))
        {
            r.fail(
                join(p, ft(0x300a022b)),
                "non-gamma source strength is inconsistent",
            );
        }
        rs_check_date_time(
            r,
            Some(&src.strength_reference_date),
            Some(&src.strength_reference_time),
            ft(0x300a022c),
            ft(0x300a022e),
            p,
        );
        rec.brachy_sources.push(v);
    }
    if rec.brachy_sources.is_empty() {
        r.fail(ft(0x30080100).to_string(), "recorded sources are missing");
    }
    for (i, s) in r.sequence(o, ft(0x30080110)).iter().enumerate() {
        let p = &item("", ft(0x30080110), i);
        let mut v = RecordBrachySession {
            referenced_setup_number: r.int_attribute(s, tagReferencedBrachySetupNumber, p),
            typ: r.required_text(s, tagApplicationSetupType, p),
            name: r.str(s, tagApplicationSetupName),
            check: r.code(s, ft(0x30080116)),
            fraction_number: r.int_attribute(s, ft(0x30080022), p),
            delivery_type: rs_text(r, s, tagTreatmentDeliveryType),
            verification_status: rs_text(r, s, ft(0x3008002c)),
            termination_status: r.required_text(s, ft(0x3008002a), p),
            total_reference_air_kerma: r.required_float(s, tagTotalReferenceAirKerma, p),
            verification_images: rs_references(r, s, ft(0x300c0040), p),
            measured_doses: rs_dose_values(r, s, p, false),
            calculated_doses: rs_dose_values(r, s, p, true),
            channels: Vec::new(),
        };
        rs_validate_session(
            r,
            s,
            p,
            &v.fraction_number,
            &v.delivery_type,
            &v.verification_status,
            &v.termination_status,
        );
        if v.total_reference_air_kerma < 0.0 {
            r.fail(
                join(p, tagTotalReferenceAirKerma),
                "reference air kerma is negative",
            );
        }
        if !v.check.is_empty() && !one_of(&v.check, &["PASSED", "FAILED", "UNKNOWN"]) {
            r.fail(
                join(p, ft(0x30080116)),
                "application setup check is invalid",
            );
        }
        let mut channels: HashSet<i64> = HashSet::new();
        for (j, c) in r.sequence(s, ft(0x30080130)).iter().enumerate() {
            let cp = &item(p, ft(0x30080130), j);
            let mut ch = RecordBrachyChannel {
                number: r.required_int(c, ft(0x300a0282), cp, "Channel Number"),
                source_number: r.required_int(
                    c,
                    tagReferencedSourceNumber,
                    cp,
                    "Referenced Source Number",
                ),
                referenced_channel_number: r.int_attribute(c, ft(0x00741406), cp),
                movement: r.required_text(c, ft(0x300a0288), cp),
                length: r.float_attribute(c, ft(0x300a0284), cp),
                effective_length: r.float_attribute(c, ft(0x300a0271), cp),
                inner_length: r.float_attribute(c, ft(0x300a0272), cp),
                afterloader_channel_id: r.str(c, ft(0x300a0273)),
                specified_time: r.required_float(c, ft(0x30080132), cp),
                delivered_time: r.required_float(c, ft(0x30080134), cp),
                specified_pulses: r.int_attribute(c, ft(0x30080136), cp),
                delivered_pulses: r.int_attribute(c, ft(0x30080138), cp),
                specified_pulse_interval: r.float_attribute(c, ft(0x3008013a), cp),
                delivered_pulse_interval: r.float_attribute(c, ft(0x3008013c), cp),
                transfer_tube_number: r.int_attribute(c, ft(0x300a02a2), cp),
                transfer_tube_length: r.float_attribute(c, ft(0x300a02a4), cp),
                safe_position: rs_safe(r, c),
                measured_doses: rs_dose_values(r, c, cp, false),
                calculated_doses: rs_dose_values(r, c, cp, true),
                ..RecordBrachyChannel::default()
            };
            if !channels.insert(ch.number) {
                r.fail(join(cp, ft(0x300a0282)), "channel number is duplicated");
            }
            if !sources.contains(&ch.source_number) {
                r.fail(
                    join(cp, tagReferencedSourceNumber),
                    "source reference is missing",
                );
            }
            if !one_of(&ch.movement, &["STEPWISE", "FIXED"]) {
                r.fail(
                    join(cp, ft(0x300a0288)),
                    "source movement is outside profile",
                );
            }
            if ch.specified_time < 0.0 || ch.delivered_time < 0.0 {
                r.fail(cp.clone(), "channel time is negative");
            }
            rs_required_presence(r, c, ft(0x300a0284), cp);
            rs_required_presence(r, c, ft(0x300a02a2), cp);
            if ch.transfer_tube_number.value.is_some() {
                rs_required_presence(r, c, ft(0x300a02a4), cp);
            }
            if ch.effective_length.present {
                rs_required_presence(r, c, ft(0x300a0272), cp);
            }
            for (a, t) in [
                (&ch.length, ft(0x300a0284)),
                (&ch.effective_length, ft(0x300a0271)),
                (&ch.inner_length, ft(0x300a0272)),
                (&ch.transfer_tube_length, ft(0x300a02a4)),
                (&ch.specified_pulse_interval, ft(0x3008013a)),
                (&ch.delivered_pulse_interval, ft(0x3008013c)),
            ] {
                r.attr_nonnegative(a, t, cp, false);
            }
            for (k, a) in r.sequence(c, ft(0x30080140)).iter().enumerate() {
                let ap = &item(cp, ft(0x30080140), k);
                let app = RecordedSourceApplicator {
                    number: r.int_attribute(a, ft(0x30080142), ap),
                    id: rs_text(r, a, ft(0x300a0291)),
                    typ: r.required_text(a, ft(0x300a0292), ap),
                    name: r.str(a, ft(0x300a0294)),
                    length: r.required_float(a, ft(0x300a0296), ap),
                    tip_length: r.float_attribute(a, ft(0x300a0274), ap),
                    step_size: r.float_attribute(a, ft(0x300a02a0), ap),
                };
                rs_required_presence(r, a, ft(0x30080142), ap);
                rs_required_presence(r, a, ft(0x300a0291), ap);
                let bad_length = app.length <= 0.0;
                if !one_of(&app.typ, &["FLEXIBLE", "RIGID"]) || bad_length {
                    r.fail(ap.clone(), "source applicator is invalid");
                }
                if ch.movement == "STEPWISE" && !app.step_size.value.is_some_and(|v| v > 0.0) {
                    r.fail(
                        join(ap, ft(0x300a02a0)),
                        "positive applicator step size is required",
                    );
                }
                if ch.effective_length.present {
                    rs_required_presence(r, a, ft(0x300a0274), ap);
                }
                ch.applicators.push(app);
            }
            ch.control_points = rs_read_brachy_cp(r, c, ft(0x30080160), cp);
            let n = r.required_int(c, tagNumberOfControlPoints, cp, "Number of Control Points");
            if n != ch.control_points.len() as i64 || n < 2 {
                r.fail(
                    join(cp, ft(0x30080160)),
                    "control point count is inconsistent",
                );
            }
            if rec.brachy_type != "MANUAL" && rec.brachy_type != "PDR" {
                rs_validate_safe(r, &ch.safe_position, cp);
            }
            for (k, po) in r.sequence(c, ft(0x30080171)).iter().enumerate() {
                let pp = &item(cp, ft(0x30080171), k);
                let pulse = RecordBrachyPulse {
                    number: r.required_int(po, ft(0x30080172), pp, "Pulse Number"),
                    safe_position: rs_safe(r, po),
                    control_points: rs_read_brachy_cp(r, po, ft(0x30080173), pp),
                };
                if pulse.number <= 0 || k > 0 && pulse.number != ch.pulses[k - 1].number + 1 {
                    r.fail(
                        join(pp, ft(0x30080172)),
                        "pulse number is invalid or not consecutive",
                    );
                }
                rs_validate_safe(r, &pulse.safe_position, pp);
                if pulse.control_points.len() < 2 {
                    r.fail(join(pp, ft(0x30080173)), "pulse control points are missing");
                }
                ch.pulses.push(pulse);
            }
            if rec.brachy_type == "PDR" {
                rs_positive_int(r, &ch.specified_pulses, ft(0x30080136), cp, true);
                if !ch.delivered_pulses.value.is_some_and(|n| n >= 0) {
                    r.fail(
                        join(cp, ft(0x30080138)),
                        "delivered pulse count is missing or negative",
                    );
                }
                if ch.specified_pulse_interval.value.is_none()
                    || ch.delivered_pulse_interval.value.is_none()
                {
                    r.fail(cp.clone(), "PDR pulse intervals are missing");
                }
                if let Some(delivered) = ch.delivered_pulses.value {
                    if ch.control_points.len() as i64 != 2 * delivered {
                        r.fail(
                            join(cp, ft(0x30080160)),
                            "PDR endpoint count differs from delivered pulses",
                        );
                    }
                    if c.has(ft(0x30080171)) && ch.pulses.len() as i64 != delivered {
                        r.fail(
                            join(cp, ft(0x30080171)),
                            "pulse sequence count differs from delivered pulses",
                        );
                    }
                }
            } else if !ch.pulses.is_empty() {
                r.fail(
                    join(cp, ft(0x30080171)),
                    "pulse records are outside non-PDR profile",
                );
            }
            if rec.brachy_type == "PDR" && ch.pulses.len() * 2 == ch.control_points.len() {
                for (k, pulse) in ch.pulses.iter().enumerate() {
                    if pulse.control_points.len() >= 2 {
                        let a = &pulse.control_points[0];
                        let b = &pulse.control_points[pulse.control_points.len() - 1];
                        for (m, x) in [a, b].into_iter().enumerate() {
                            let y = &ch.control_points[2 * k + m];
                            if x.date != y.date
                                || x.time != y.time
                                || !semantic_floats(x.relative_position, y.relative_position)
                                || matches!(
                                    (x.referenced_index.value, y.referenced_index.value),
                                    (Some(p), Some(q)) if p != q
                                )
                            {
                                r.fail(
                                    item(cp, ft(0x30080171), k),
                                    "pulse endpoints differ from channel endpoints",
                                );
                            }
                        }
                    }
                }
            }
            if rec.brachy_type != "PDR" {
                rs_check_dwell_pairs(
                    r,
                    &ch.control_points,
                    &ch.movement,
                    &join(cp, ft(0x30080160)),
                );
            }
            for (k, pulse) in ch.pulses.iter().enumerate() {
                rs_check_dwell_pairs(
                    r,
                    &pulse.control_points,
                    &ch.movement,
                    &format!("{}/{}", item(cp, ft(0x30080171), k), ft(0x30080173)),
                );
            }
            v.channels.push(ch);
        }
        if v.channels.is_empty() {
            r.fail(join(p, ft(0x30080130)), "recorded channels are missing");
        }
        rec.brachy_sessions.push(v);
    }
    if rec.brachy_sessions.is_empty() {
        r.fail(ft(0x30080110).to_string(), "brachy sessions are missing");
    }
}
fn rs_safe(r: &Reader, o: &Object) -> RecordSafePosition {
    RecordSafePosition {
        exit_date: r.str(o, ft(0x30080162)),
        exit_time: r.str(o, ft(0x30080164)),
        return_date: r.str(o, ft(0x30080166)),
        return_time: r.str(o, ft(0x30080168)),
    }
}
fn rs_validate_safe(r: &mut Reader, s: &RecordSafePosition, p: &str) {
    rs_check_date_time(
        r,
        Some(&s.exit_date),
        Some(&s.exit_time),
        ft(0x30080162),
        ft(0x30080164),
        p,
    );
    rs_check_date_time(
        r,
        Some(&s.return_date),
        Some(&s.return_time),
        ft(0x30080166),
        ft(0x30080168),
        p,
    );
}
fn rs_read_brachy_cp(
    r: &mut Reader,
    o: &Object,
    seq: Tag,
    p: &str,
) -> Vec<RecordBrachyControlPoint> {
    let mut out = Vec::new();
    for (i, c) in r.sequence(o, seq).iter().enumerate() {
        let pp = &item(p, seq, i);
        let v = RecordBrachyControlPoint {
            referenced_index: r.int_attribute(c, ft(0x300c00f0), pp),
            date: r.required_text(c, ft(0x30080024), pp),
            time: r.required_text(c, ft(0x30080025), pp),
            relative_position: r.required_float(c, ft(0x300a02d2), pp),
        };
        if v.referenced_index.value.is_some_and(|n| n < 0) {
            r.fail(
                join(pp, ft(0x300c00f0)),
                "control point reference is negative",
            );
        }
        rs_check_date_time(
            r,
            Some(&v.date),
            Some(&v.time),
            ft(0x30080024),
            ft(0x30080025),
            pp,
        );
        out.push(v);
    }
    out
}
fn rs_read_summary(r: &mut Reader, o: &Object) -> RecordSummary {
    let mut s = RecordSummary {
        status: r.required_text(o, ft(0x30080200), ""),
        comment: r.str(o, ft(0x30080202)),
        first_date: rs_text(r, o, ft(0x30080054)),
        most_recent_date: rs_text(r, o, ft(0x30080056)),
        ..RecordSummary::default()
    };
    if !one_of(
        &s.status,
        &[
            "NOT_STARTED",
            "ON_TREATMENT",
            "ON_BREAK",
            "SUSPENDED",
            "STOPPED",
            "COMPLETED",
        ],
    ) {
        r.fail(ft(0x30080200).to_string(), "treatment status is invalid");
    }
    for t in [ft(0x30080054), ft(0x30080056)] {
        rs_required_presence(r, o, t, "");
    }
    rs_check_date_time(
        r,
        s.first_date.value.as_deref(),
        None,
        ft(0x30080054),
        ft(0x30080251),
        "",
    );
    rs_check_date_time(
        r,
        s.most_recent_date.value.as_deref(),
        None,
        ft(0x30080056),
        ft(0x30080251),
        "",
    );
    let mut seen: HashSet<i64> = HashSet::new();
    for (i, group) in r.sequence(o, ft(0x30080220)).iter().enumerate() {
        let p = &item("", ft(0x30080220), i);
        let mut v = RecordFractionGroupSummary {
            referenced_number: r.int_attribute(group, ft(0x300c0022), p),
            typ: rs_text(r, group, ft(0x30080224)),
            planned: r.int_attribute(group, tagNumberOfFractionsPlanned, p),
            delivered: r.int_attribute(group, ft(0x3008005a), p),
            fractions: Vec::new(),
        };
        for t in [ft(0x30080224), tagNumberOfFractionsPlanned, ft(0x3008005a)] {
            rs_required_presence(r, group, t, p);
        }
        if v.typ
            .value
            .as_deref()
            .is_some_and(|t| !one_of(t, &["EXTERNAL_BEAM", "BRACHY"]))
        {
            r.fail(join(p, ft(0x30080224)), "fraction group type is invalid");
        }
        if let Some(n) = v.referenced_number.value {
            if n <= 0 || seen.contains(&n) {
                r.fail(
                    join(p, ft(0x300c0022)),
                    "fraction group reference is invalid or duplicated",
                );
            }
            seen.insert(n);
        }
        for n in [&v.planned, &v.delivered] {
            if n.value.is_some_and(|n| n < 0) {
                r.fail(p.clone(), "fraction count is negative");
            }
        }
        let mut fractions: HashSet<i64> = HashSet::new();
        for (j, f) in r.sequence(group, ft(0x30080240)).iter().enumerate() {
            let fp = &item(p, ft(0x30080240), j);
            let fs = RecordFractionStatus {
                number: r.required_int(f, ft(0x30080223), fp, "Referenced Fraction Number"),
                clinical_number: r.int_attribute(f, ft(0x300a0705), fp),
                date: rs_text(r, f, ft(0x30080250)),
                time: rs_text(r, f, ft(0x30080251)),
                termination_status: rs_text(r, f, ft(0x3008002a)),
            };
            if fs.number <= 0 || fractions.contains(&fs.number) {
                r.fail(
                    join(fp, ft(0x30080223)),
                    "fraction number is invalid or duplicated",
                );
            }
            fractions.insert(fs.number);
            for t in [ft(0x30080250), ft(0x30080251), ft(0x3008002a)] {
                rs_required_presence(r, f, t, fp);
            }
            rs_check_date_time(
                r,
                fs.date.value.as_deref(),
                fs.time.value.as_deref(),
                ft(0x30080250),
                ft(0x30080251),
                fp,
            );
            if fs
                .termination_status
                .value
                .as_deref()
                .is_some_and(|t| !one_of(t, &["NORMAL", "OPERATOR", "MACHINE", "UNKNOWN"]))
            {
                r.fail(join(fp, ft(0x3008002a)), "termination status is invalid");
            }
            v.fractions.push(fs);
        }
        s.fraction_groups.push(v);
    }
    s.measured_doses = rs_summary_doses(r, o, ft(0x300800e0));
    s.calculated_doses = rs_summary_doses(r, o, ft(0x30080050));
    s
}
fn rs_summary_doses(r: &mut Reader, o: &Object, seq: Tag) -> Vec<RecordSummaryDose> {
    let mut out = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();
    for (i, d) in r.sequence(o, seq).iter().enumerate() {
        let p = &item("", seq, i);
        let v = RecordSummaryDose {
            plan_number: r.int_attribute(d, tagReferencedDoseReferenceNumber, p),
            description: r.str(d, tagDoseReferenceDescription),
            cumulative_dose: r.required_float(d, ft(0x30080052), p),
        };
        if v.cumulative_dose < 0.0 {
            r.fail(join(p, ft(0x30080052)), "cumulative dose is negative");
        }
        if let Some(n) = v.plan_number.value {
            if n <= 0 || seen.contains(&n) {
                r.fail(
                    join(p, tagReferencedDoseReferenceNumber),
                    "dose reference is invalid or duplicated",
                );
            }
            seen.insert(n);
        }
        out.push(v);
    }
    out
}
fn rs_validate_doses(r: &mut Reader, rec: &TreatmentRecord) {
    let measured: HashSet<i64> = rec
        .measured_dose_references
        .iter()
        .filter_map(|v| v.number.value)
        .collect();
    let calculated: HashSet<i64> = rec
        .calculated_dose_references
        .iter()
        .filter_map(|v| v.number.value)
        .collect();
    fn check(r: &mut Reader, values: &[RecordDoseValue], local: &HashSet<i64>, p: &str) {
        for (i, v) in values.iter().enumerate() {
            if v.local_number.value.is_some_and(|n| !local.contains(&n)) {
                r.fail(format!("{p}[{i}]"), "local dose reference is missing");
            }
        }
    }
    for (i, v) in rec.beam_sessions.iter().enumerate() {
        let p = if rec.sop_class_uid == RT_ION_BEAMS_TREATMENT_RECORD_STORAGE {
            item("", ft(0x30080021), i)
        } else {
            item("", ft(0x30080020), i)
        };
        check(r, &v.measured_doses, &measured, &join(&p, ft(0x30080080)));
        check(
            r,
            &v.calculated_doses,
            &calculated,
            &join(&p, ft(0x30080090)),
        );
        if let Some(n) = v.referenced_patient_setup_number.value
            && !rec.patient_setups.iter().any(|s| s.number == n)
        {
            r.fail(
                join(&p, tagReferencedPatientSetupNumber),
                "local patient setup reference is missing",
            );
        }
    }
    for (i, v) in rec.brachy_sessions.iter().enumerate() {
        let p = item("", ft(0x30080110), i);
        check(r, &v.measured_doses, &measured, &join(&p, ft(0x30080080)));
        check(
            r,
            &v.calculated_doses,
            &calculated,
            &join(&p, ft(0x30080090)),
        );
        for (j, c) in v.channels.iter().enumerate() {
            let p = item(&p, ft(0x30080130), j);
            check(r, &c.measured_doses, &measured, &join(&p, ft(0x30080080)));
            check(
                r,
                &c.calculated_doses,
                &calculated,
                &join(&p, ft(0x30080090)),
            );
        }
    }
}
impl TreatmentRecord {
    pub fn validate_referenced_plan(&self, p: &Plan) -> Result<(), Error> {
        let rec = self;
        let plan_path = tagReferencedRTPlanSequence.to_string();
        let Some(referenced) = &rec.referenced_plan else {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &plan_path,
                "an explicit referenced plan is required",
            ));
        };
        if referenced.sop_class_uid != p.sop_class_uid
            || referenced.sop_instance_uid != p.sop_instance_uid
        {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &plan_path,
                "supplied plan identity does not match",
            ));
        }
        model_admission_pair(&Pair("Record", rec, "Plan", p), &default_limits())?;
        let r = &mut Reader::new(default_limits().max_problems);
        let group: HashSet<i64> = p.fraction_groups.iter().map(|g| g.number).collect();
        let dose: HashSet<i64> = p.dose_references.iter().map(|d| d.number).collect();
        if rec
            .fraction_group_number
            .value
            .is_some_and(|n| !group.contains(&n))
        {
            r.fail(
                ft(0x300c0022).to_string(),
                "fraction group reference is missing in supplied plan",
            );
        }
        for (i, b) in rec.beam_sessions.iter().enumerate() {
            let mut seq = ft(0x30080020);
            let mut limit: i64 = -1;
            let mut shifters: HashSet<i64> = HashSet::new();
            if rec.sop_class_uid == RT_ION_BEAMS_TREATMENT_RECORD_STORAGE {
                seq = ft(0x30080021);
                for pb in &p.ion_beams {
                    if b.referenced_beam_number.value == Some(pb.number) {
                        limit = pb.control_points.len() as i64;
                        for s in &pb.range_shifters {
                            shifters.insert(s.number);
                        }
                    }
                }
            } else {
                for pb in &p.beams {
                    if b.referenced_beam_number.value == Some(pb.number) {
                        limit = pb.control_points.len() as i64;
                    }
                }
            }
            let path = item("", seq, i);
            if b.referenced_beam_number.value.is_some() && limit < 0 {
                r.fail(
                    join(&path, tagReferencedBeamNumber),
                    "beam reference is missing in supplied plan",
                );
            }
            for (j, c) in b.control_points.iter().enumerate() {
                if limit >= 0 && c.referenced_index.value.is_some_and(|n| n >= limit) {
                    r.fail(
                        format!("{}/{}", item(&path, ft(0x30080040), j), ft(0x300c00f0)),
                        "control point reference is missing in supplied plan",
                    );
                }
            }
            for s in &b.range_shifters {
                if !shifters.contains(&s.number) {
                    r.fail(
                        join(&path, ft(0x300800f2)),
                        "range shifter reference is missing in supplied plan",
                    );
                }
            }
        }
        for (i, s) in rec.brachy_sessions.iter().enumerate() {
            let path = item("", ft(0x30080110), i);
            let mut setup: Option<&BrachySetup> = None;
            for candidate in &p.brachy_setups {
                if s.referenced_setup_number.value == Some(candidate.number) {
                    setup = Some(candidate);
                }
            }
            if s.referenced_setup_number.value.is_some() && setup.is_none() {
                r.fail(
                    join(&path, tagReferencedBrachySetupNumber),
                    "setup reference is missing in supplied plan",
                );
            }
            for (j, c) in s.channels.iter().enumerate() {
                let (Some(setup), Some(number)) = (setup, c.referenced_channel_number.value) else {
                    continue;
                };
                let mut channel: Option<&BrachyChannel> = None;
                for candidate in &setup.channels {
                    if candidate.number == number {
                        channel = Some(candidate);
                    }
                }
                let cp = item(&path, ft(0x30080130), j);
                let Some(channel) = channel else {
                    r.fail(
                        join(&cp, ft(0x00741406)),
                        "channel reference is missing in supplied plan",
                    );
                    continue;
                };
                let limit = channel.control_points.len() as i64;
                let mut check = |points: &[RecordBrachyControlPoint]| {
                    for v in points {
                        if v.referenced_index.value.is_some_and(|n| n >= limit) {
                            r.fail(
                                join(&cp, ft(0x300c00f0)),
                                "control point reference is missing in supplied plan",
                            );
                        }
                    }
                };
                check(&c.control_points);
                for pulse in &c.pulses {
                    check(&pulse.control_points);
                }
            }
        }
        let mut check = |a: &Attribute<i64>, path: String| {
            if a.value.is_some_and(|n| !dose.contains(&n)) {
                r.fail(path, "dose reference is missing in supplied plan");
            }
        };
        let number_path = tagReferencedDoseReferenceNumber.to_string();
        for v in &rec.measured_dose_references {
            check(&v.plan_number, ft(0x30080010).to_string());
        }
        for v in &rec.calculated_dose_references {
            check(&v.plan_number, ft(0x30080070).to_string());
        }
        for b in &rec.beam_sessions {
            for v in b.measured_doses.iter().chain(&b.calculated_doses) {
                check(&v.plan_number, number_path.clone());
            }
        }
        for s in &rec.brachy_sessions {
            for v in s.measured_doses.iter().chain(&s.calculated_doses) {
                check(&v.plan_number, number_path.clone());
            }
            for c in &s.channels {
                for v in c.measured_doses.iter().chain(&c.calculated_doses) {
                    check(&v.plan_number, number_path.clone());
                }
            }
        }
        if let Some(summary) = &rec.summary {
            for g in &summary.fraction_groups {
                if g.referenced_number
                    .value
                    .is_some_and(|n| !group.contains(&n))
                {
                    r.fail(
                        ft(0x30080220).to_string(),
                        "fraction group reference is missing in supplied plan",
                    );
                }
            }
            let mut check = |a: &Attribute<i64>| {
                if a.value.is_some_and(|n| !dose.contains(&n)) {
                    r.fail(
                        number_path.clone(),
                        "dose reference is missing in supplied plan",
                    );
                }
            };
            for v in summary
                .measured_doses
                .iter()
                .chain(&summary.calculated_doses)
            {
                check(&v.plan_number);
            }
        }
        if !r.errors.is_empty() {
            return Err(Error::validation(std::mem::take(&mut r.errors)));
        }
        Ok(())
    }
}
fn rs_validate_metersets(r: &mut Reader, b: &RecordBeamSession, path: &str, seq: Tag) {
    if b.control_points.len() < 2 {
        return;
    }
    let start = b.control_points[0].delivered_meterset;
    let end = b.control_points[b.control_points.len() - 1].delivered_meterset;
    let close =
        |a: f64, b: f64, scale: f64| (a - b).abs() <= fmath::ldexp(a.abs() + b.abs() + scale, -48);
    if let Some(v) = b.delivered_primary_meterset.value
        && !close(v, end - start, start.abs() + end.abs())
    {
        r.fail(
            join(path, ft(0x30080036)),
            "beam meterset differs from recorded control point increment",
        );
    }
    let mut prev: Option<f64> = None;
    for (i, cp) in b.control_points.iter().enumerate() {
        let Some(v) = cp.specified_meterset.value else {
            continue;
        };
        let p = item(path, seq, i);
        if prev.is_some_and(|prev| v < prev) {
            r.fail(join(&p, ft(0x30080042)), "specified meterset decreases");
        }
        prev = Some(v);
        let expected = fmath::max(start, fmath::min(v, end));
        if !close(
            cp.delivered_meterset,
            expected,
            start.abs() + end.abs() + v.abs(),
        ) {
            r.fail(
                join(&p, ft(0x30080044)),
                "delivered meterset is inconsistent with recorded partial session",
            );
        }
    }
}
fn rs_check_dwell_pairs(
    r: &mut Reader,
    points: &[RecordBrachyControlPoint],
    movement: &str,
    path: &str,
) {
    if movement == "STEPWISE" {
        if !points.len().is_multiple_of(2) {
            r.fail(path, "stepwise delivery requires dwell pairs");
        }
        let mut k = 0;
        while k + 1 < points.len() {
            if points[k].relative_position != points[k + 1].relative_position {
                r.fail(
                    format!("{path}[{}]", k + 1),
                    "delivered dwell pair positions differ",
                );
            }
            k += 2;
        }
    }
}
