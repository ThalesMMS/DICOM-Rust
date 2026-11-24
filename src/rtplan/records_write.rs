use super::bounded::ReadOptions;
use super::read::{Reader, clean};
use super::records::{
    is_record_class, read_treatment_record, rs_dose_key, rs_state_numbers, rs_state_texts, rs_text,
};
use super::reflect::semantic_equal;
use super::tags::*;
use super::write::{
    WriteBuilder, WriteOptions, begin_write, finish_write, object_int, put_sequence, rs_sequence,
};
use super::*;
use crate::object::element_setters::format_is;
use crate::object::{File, Object, Tag, Vr};
use std::collections::HashSet;
pub fn write_treatment_record(rec: &TreatmentRecord, opts: &WriteOptions) -> Result<File, Error> {
    if !is_record_class(&rec.sop_class_uid) {
        return Err(Error::bare(ErrorKind::UnsupportedSopClass));
    }
    let (mut f, mut w) = begin_write(opts, &rec.sop_class_uid, false, rec)?;
    let read_options = ReadOptions {
        limits: opts.limits,
        strict_iod: false,
    };
    let Some(template) = &opts.template else {
        return Err(Error::bare(ErrorKind::InvalidObject));
    };
    let before = read_treatment_record(template, read_options)?;
    rs_write_common(&mut w, &mut f.dataset, rec);
    match rec.sop_class_uid.as_str() {
        RT_BEAMS_TREATMENT_RECORD_STORAGE | RT_ION_BEAMS_TREATMENT_RECORD_STORAGE => {
            rs_write_beams(&mut w, &mut f.dataset, rec)
        }
        RT_BRACHY_TREATMENT_RECORD_STORAGE => rs_write_brachy(&mut w, &mut f.dataset, rec),
        _ => {
            let Some(summary) = &rec.summary else {
                return Err(path_error(
                    ErrorKind::InvalidObject,
                    &ft(0x30080200).to_string(),
                    "summary is missing",
                ));
            };
            rs_write_summary(&mut w, &mut f.dataset, summary);
        }
    }
    if rec.sop_class_uid != RT_TREATMENT_SUMMARY_RECORD_STORAGE {
        match &rec.summary {
            Some(summary) => rs_write_summary(&mut w, &mut f.dataset, summary),
            None => {
                for t in [
                    0x30080200, 0x30080202, 0x30080054, 0x30080056, 0x30080220, 0x300800e0,
                    0x30080050,
                ] {
                    f.dataset.remove(ft(t));
                }
            }
        }
    }
    if let Some(e) = w.err.take() {
        return Err(e);
    }
    let actual = read_treatment_record(&f, read_options)?;
    if !semantic_equal(rec, &actual) {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "",
            "model cannot be represented by the selected record profile",
        ));
    }
    let changed = !semantic_equal(&before, rec);
    finish_write(f, w, opts, &rec.sop_class_uid, changed)
}
fn rs_declared_text(w: &mut WriteBuilder, o: &mut Object, t: Tag, vr: Vr, a: &Attribute<String>) {
    o.remove(t);
    if a.present {
        w.str(o, t, vr, a.value.as_deref().unwrap_or(""));
    }
}
fn rs_attr_key(a: &Attribute<i64>) -> String {
    match a.value {
        None => "-".to_string(),
        Some(n) => n.to_string(),
    }
}
fn rs_cp_key(index: &Attribute<i64>, date: &str, time: &str) -> String {
    format!("{}/{date}/{time}", rs_attr_key(index))
}
fn rs_cp_object_key(o: &Object) -> String {
    let r = Reader::default();
    rs_cp_key(
        &object_int(o, ft(0x300c00f0)),
        &r.str(o, ft(0x30080024)),
        &r.str(o, ft(0x30080025)),
    )
}
fn device_type_key(o: &Object) -> String {
    clean(
        &o.get_string(tagRTBeamLimitingDeviceType)
            .unwrap_or_default(),
    )
    .to_string()
}
fn rs_write_common(w: &mut WriteBuilder, o: &mut Object, rec: &TreatmentRecord) {
    w.str(o, tagStudyInstanceUID, Vr::UI, &rec.study_instance_uid);
    w.str(o, tagSeriesInstanceUID, Vr::UI, &rec.series_instance_uid);
    w.integer(o, ft(0x00200013), rec.instance_number);
    w.str(o, tagModality, Vr::CS, "RTRECORD");
    rs_declared_text(w, o, ft(0x30080250), Vr::DA, &rec.date);
    rs_declared_text(w, o, ft(0x30080251), Vr::TM, &rec.time);
    rs_declared_text(w, o, ft(0x00080201), Vr::SH, &rec.timezone_offset);
    rs_declared_text(w, o, ft(0x300a0709), Vr::CS, &rec.content_origin);
    let refs: Vec<Reference> = rec.referenced_plan.iter().cloned().collect();
    w.reference(o, tagReferencedRTPlanSequence, &refs);
    if refs.is_empty() {
        put_sequence(o, tagReferencedRTPlanSequence, &[]);
    }
    w.reference(o, ft(0x30080030), &rec.related_records);
    w.int(o, ft(0x300c0022), &rec.fraction_group_number);
    w.int(o, tagNumberOfFractionsPlanned, &rec.fractions_planned);
    w.optional_text(
        o,
        tagPrimaryDosimeterUnit,
        Vr::CS,
        &rec.primary_dosimeter_unit,
    );
    match &rec.machine {
        None => {
            o.remove(ft(0x300a0206));
        }
        Some(machine) => {
            let mut items = o.get_sequence(ft(0x300a0206)).unwrap_or_default();
            let mut m = if items.len() == 1 {
                items.remove(0)
            } else {
                Object::new()
            };
            for (t, vr, a) in [
                (tagTreatmentMachineName, Vr::SH, &machine.name),
                (ft(0x00080070), Vr::LO, &machine.manufacturer),
                (ft(0x00080080), Vr::LO, &machine.institution),
                (ft(0x00081090), Vr::LO, &machine.model),
                (ft(0x00181000), Vr::LO, &machine.serial_number),
            ] {
                rs_declared_text(w, &mut m, t, vr, a);
            }
            w.optional_text(
                &mut m,
                ft(0x300a0054),
                Vr::UI,
                &machine.table_top_position_alignment_uid,
            );
            put_sequence(o, ft(0x300a0206), &[m]);
        }
    }
    let mut old = w.keyed(o, tagPatientSetupSequence, tagPatientSetupNumber);
    for v in &rec.patient_setups {
        if !w.tick() {
            return;
        }
        let i = old.item(&v.number);
        let s = old.get(i);
        w.integer(s, tagPatientSetupNumber, v.number);
        w.optional_text(s, tagPatientSetupLabel, Vr::LO, &v.label);
        w.optional_text(s, tagPatientPosition, Vr::CS, &v.patient_position);
        w.optional_text(
            s,
            tagPatientAdditionalPosition,
            Vr::LO,
            &v.patient_additional_position,
        );
        w.optional_text(s, tagSetupTechnique, Vr::CS, &v.setup_technique);
    }
    rs_sequence(o, tagPatientSetupSequence, &old.objects(), false);
    rs_write_dose_references(w, o, &rec.measured_dose_references, false);
    rs_write_dose_references(w, o, &rec.calculated_dose_references, true);
}
fn rs_write_dose_references(
    w: &mut WriteBuilder,
    o: &mut Object,
    values: &[RecordDoseReference],
    calc: bool,
) {
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
    let mut old = w.old_items(o, seq, |o| {
        rs_dose_key(
            &object_int(o, num),
            &object_int(o, tagReferencedDoseReferenceNumber),
        )
    });
    let mut seen = HashSet::new();
    for v in values {
        if !w.tick() {
            return;
        }
        let i = w.new_item(
            &mut old,
            &mut seen,
            &rs_dose_key(&v.number, &v.plan_number),
            seq,
        );
        let it = old.get(i);
        w.int(it, num, &v.number);
        w.int(it, tagReferencedDoseReferenceNumber, &v.plan_number);
        w.float(it, val, &v.value);
        w.optional_text(it, desc, Vr::ST, &v.description);
        if !calc {
            w.str(it, ft(0x30040002), Vr::CS, &v.units);
            rs_declared_text(w, it, ft(0x30080014), Vr::CS, &v.typ);
        }
    }
    rs_sequence(o, seq, &old.objects(), false);
}
fn rs_write_dose_values(
    w: &mut WriteBuilder,
    o: &mut Object,
    values: &[RecordDoseValue],
    calc: bool,
) {
    let (seq, num, val) = if calc {
        (ft(0x30080090), ft(0x30080092), ft(0x30080076))
    } else {
        (ft(0x30080080), ft(0x30080082), ft(0x30080016))
    };
    let mut old = w.old_items(o, seq, |o| {
        rs_dose_key(
            &object_int(o, num),
            &object_int(o, tagReferencedDoseReferenceNumber),
        )
    });
    let mut seen = HashSet::new();
    for v in values {
        if !w.tick() {
            return;
        }
        let i = w.new_item(
            &mut old,
            &mut seen,
            &rs_dose_key(&v.local_number, &v.plan_number),
            seq,
        );
        let it = old.get(i);
        w.int(it, num, &v.local_number);
        w.int(it, tagReferencedDoseReferenceNumber, &v.plan_number);
        w.numbers(it, val, &[v.value]);
    }
    rs_sequence(o, seq, &old.objects(), false);
}
#[allow(clippy::too_many_arguments)]
fn rs_beam_key(
    beam: &Attribute<i64>,
    fraction: &Attribute<i64>,
    first_cp: Option<(&str, &str)>,
    name: &str,
    typ: &str,
    radiation: &str,
) -> String {
    let mut key = format!("{}/{}", rs_attr_key(beam), rs_attr_key(fraction));
    if let Some((date, time)) = first_cp {
        key.push_str(&format!("/{date}/{time}"));
    }
    if beam.value.is_none() {
        key.push_str(&format!("/{name}/{typ}/{radiation}"));
    }
    key
}
fn rs_beam_object_key(o: &Object, cpseq: Tag) -> String {
    let r = Reader::default();
    let points = r.sequence(o, cpseq);
    let first = points
        .first()
        .map(|p| (r.str(p, ft(0x30080024)), r.str(p, ft(0x30080025))));
    rs_beam_key(
        &object_int(o, tagReferencedBeamNumber),
        &object_int(o, ft(0x30080022)),
        first.as_ref().map(|(d, t)| (d.as_str(), t.as_str())),
        &r.str(o, tagBeamName),
        &r.str(o, tagBeamType),
        &r.str(o, tagRadiationType),
    )
}
fn rs_write_beams(w: &mut WriteBuilder, o: &mut Object, rec: &TreatmentRecord) {
    let ion = rec.sop_class_uid == RT_ION_BEAMS_TREATMENT_RECORD_STORAGE;
    let (seq, cpseq) = if ion {
        (ft(0x30080021), ft(0x30080041))
    } else {
        (ft(0x30080020), ft(0x30080040))
    };
    let mut old = w.old_items(o, seq, |o| rs_beam_object_key(o, cpseq));
    let mut seen = HashSet::new();
    for b in &rec.beam_sessions {
        if !w.tick() {
            return;
        }
        let key = rs_beam_key(
            &b.referenced_beam_number,
            &b.fraction_number,
            b.control_points
                .first()
                .map(|cp| (cp.date.as_str(), cp.time.as_str())),
            &b.name,
            &b.typ,
            &b.radiation_type,
        );
        let i = w.new_item(&mut old, &mut seen, &key, seq);
        let it = old.get(i);
        w.int(it, tagReferencedBeamNumber, &b.referenced_beam_number);
        w.int(
            it,
            tagReferencedPatientSetupNumber,
            &b.referenced_patient_setup_number,
        );
        w.int(
            it,
            tagReferencedToleranceTableNumber,
            &b.referenced_tolerance_table_number,
        );
        w.optional_text(it, tagBeamName, Vr::LO, &b.name);
        w.optional_text(it, tagBeamDescription, Vr::ST, &b.description);
        w.str(it, tagBeamType, Vr::CS, &b.typ);
        w.str(it, tagRadiationType, Vr::CS, &b.radiation_type);
        rs_write_session(
            w,
            it,
            &b.fraction_number,
            &b.delivery_type,
            &b.verification_status,
            &b.termination_status,
        );
        w.int(it, ft(0x300a0705), &b.clinical_fraction_number);
        for (t, a) in [
            (ft(0x30080032), &b.specified_primary_meterset),
            (ft(0x30080033), &b.specified_secondary_meterset),
            (ft(0x30080036), &b.delivered_primary_meterset),
            (ft(0x30080037), &b.delivered_secondary_meterset),
            (ft(0x3008003a), &b.specified_treatment_time),
            (ft(0x3008003b), &b.delivered_treatment_time),
            (tagSourceAxisDistance, &b.source_axis_distance),
        ] {
            w.float(it, t, a);
        }
        for t in [
            tagNumberOfWedges,
            tagNumberOfCompensators,
            tagNumberOfBoli,
            tagNumberOfBlocks,
        ] {
            if ion || t == tagNumberOfWedges || it.has(t) {
                w.integer(it, t, 0);
            }
        }
        let mut device_old = w.old_items(it, ft(0x300800a0), device_type_key);
        let mut dev_seen = HashSet::new();
        for d in &b.devices {
            let k = w.new_item(&mut device_old, &mut dev_seen, &d.typ, ft(0x300800a0));
            let v = device_old.get(k);
            w.str(v, tagRTBeamLimitingDeviceType, Vr::CS, &d.typ);
            w.integer(v, tagNumberOfLeafJawPairs, d.pairs);
        }
        rs_sequence(it, ft(0x300800a0), &device_old.objects(), !ion);
        if ion {
            w.int(it, ft(0x300a0304), &b.atomic_number);
            w.int(it, ft(0x300a0302), &b.mass_number);
            w.int(it, ft(0x300a0306), &b.charge_state);
            w.str(it, ft(0x300a0308), Vr::CS, &b.scan_mode);
            w.str(it, ft(0x300a0350), Vr::CS, &b.patient_support_type);
            for t in [ft(0x300a0330), ft(0x300a0340)] {
                w.integer(it, t, 0);
            }
            let mut snouts = it.get_sequence(ft(0x300800f0)).unwrap_or_default();
            let mut snout = if snouts.len() == 1 {
                snouts.remove(0)
            } else {
                Object::new()
            };
            if !b.snout_id.is_empty() {
                w.str(&mut snout, ft(0x300a030f), Vr::SH, &b.snout_id);
                put_sequence(it, ft(0x300800f0), &[snout]);
            } else {
                it.remove(ft(0x300800f0));
            }
            let mut sh_old = w.keyed(it, ft(0x300800f2), ft(0x300c0100));
            for s in &b.range_shifters {
                let k = sh_old.item(&s.number);
                let sh = sh_old.get(k);
                w.integer(sh, ft(0x300c0100), s.number);
                w.str(sh, ft(0x300a0318), Vr::SH, &s.id);
            }
            w.integer(it, ft(0x300a0312), sh_old.len() as i64);
            rs_sequence(it, ft(0x300800f2), &sh_old.objects(), false);
        }
        w.reference(it, ft(0x300c0040), &b.verification_images);
        rs_write_dose_values(w, it, &b.measured_doses, false);
        rs_write_dose_values(w, it, &b.calculated_doses, true);
        let mut cp_old = w.old_items(it, cpseq, rs_cp_object_key);
        let mut cp_seen = HashSet::new();
        for cp in &b.control_points {
            if !w.tick() {
                return;
            }
            let k = w.new_item(
                &mut cp_old,
                &mut cp_seen,
                &rs_cp_key(&cp.referenced_index, &cp.date, &cp.time),
                cpseq,
            );
            let p = cp_old.get(k);
            w.int(p, ft(0x300c00f0), &cp.referenced_index);
            w.str(p, ft(0x30080024), Vr::DA, &cp.date);
            w.str(p, ft(0x30080025), Vr::TM, &cp.time);
            w.float(p, ft(0x30080042), &cp.specified_meterset);
            w.numbers(p, ft(0x30080044), &[cp.delivered_meterset]);
            let (rate_set, rate_del) = if ion {
                (ft(0x30080045), ft(0x30080046))
            } else {
                (tagDoseRateSet, ft(0x30080048))
            };
            w.float(p, rate_set, &cp.rate_set);
            w.float(p, rate_del, &cp.rate_delivered);
            w.optional_text(p, ft(0x300a0015), Vr::CS, &cp.energy_unit);
            if !rs_write_state(w, p, &cp.state) {
                return;
            }
            for t in [
                0x300a0390, 0x300a0392, 0x300a0394, 0x30080047, 0x300a039a, 0x300a0393, 0x300a0391,
            ] {
                p.remove(ft(t));
            }
            if ion && b.scan_mode == "MODULATED" {
                w.str(p, ft(0x300a0390), Vr::SH, &cp.scan_spot_tune_id);
                w.integer(p, ft(0x300a0392), cp.spot_metersets.len() as i64);
                w.fl(p, ft(0x300a0394), &cp.spot_positions);
                w.fl(p, ft(0x30080047), &cp.spot_metersets);
                w.int(p, ft(0x300a039a), &cp.number_of_paintings);
                rs_declared_text(w, p, ft(0x300a0393), Vr::CS, &cp.spot_reordered);
                if !cp.spot_prescribed_indices.is_empty() {
                    let mut out = Vec::with_capacity(cp.spot_prescribed_indices.len());
                    for n in &cp.spot_prescribed_indices {
                        match format_is(*n) {
                            Ok(s) => out.push(s),
                            Err(_) => {
                                w.err = Some(path_error(
                                    ErrorKind::InvalidObject,
                                    &ft(0x300a0391).to_string(),
                                    "spot index cannot be encoded",
                                ));
                                out.push(String::new());
                            }
                        }
                    }
                    w.strings(p, ft(0x300a0391), Vr::IS, out);
                }
            }
        }
        w.integer(it, tagNumberOfControlPoints, cp_old.len() as i64);
        put_sequence(it, cpseq, &cp_old.objects());
    }
    put_sequence(o, seq, &old.objects());
}
fn rs_write_session(
    w: &mut WriteBuilder,
    o: &mut Object,
    fraction: &Attribute<i64>,
    delivery: &Attribute<String>,
    verification: &Attribute<String>,
    termination: &str,
) {
    w.int(o, ft(0x30080022), fraction);
    rs_declared_text(w, o, tagTreatmentDeliveryType, Vr::CS, delivery);
    rs_declared_text(w, o, ft(0x3008002c), Vr::CS, verification);
    w.str(o, ft(0x3008002a), Vr::CS, termination);
}
fn rs_write_state(w: &mut WriteBuilder, o: &mut Object, state: &RecordMachineState) -> bool {
    let mut state = state.clone();
    for (t, a) in rs_state_numbers(&mut state) {
        w.float(o, t, a);
    }
    for (t, a) in rs_state_texts(&mut state) {
        rs_declared_text(w, o, t, Vr::CS, a);
    }
    let mut old = w.old_items(o, tagBLDPositionSequence, device_type_key);
    let mut seen = HashSet::new();
    for d in &state.device_positions {
        if !w.tick() {
            return false;
        }
        let k = w.new_item(&mut old, &mut seen, &d.device_type, tagBLDPositionSequence);
        let it = old.get(k);
        w.str(it, tagRTBeamLimitingDeviceType, Vr::CS, &d.device_type);
        w.numbers(it, tagLeafJawPositions, &d.positions);
    }
    rs_sequence(o, tagBLDPositionSequence, &old.objects(), false);
    let mut sh_old = w.keyed(o, ft(0x300a0360), ft(0x300c0100));
    for s in &state.range_shifter_settings {
        if !w.tick() {
            return false;
        }
        let k = sh_old.item(&s.number);
        let it = sh_old.get(k);
        w.integer(it, ft(0x300c0100), s.number);
        w.str(it, ft(0x300a0362), Vr::LO, &s.setting);
        w.float(it, ft(0x300a0364), &s.distance);
        w.float(it, ft(0x300a0366), &s.water_equivalent_thickness);
    }
    rs_sequence(o, ft(0x300a0360), &sh_old.objects(), false);
    true
}
fn rs_setup_key(
    setup: &Attribute<i64>,
    fraction: &Attribute<i64>,
    typ: &str,
    name: &str,
) -> String {
    let mut key = format!("{}/{}", rs_attr_key(setup), rs_attr_key(fraction));
    if setup.value.is_none() {
        key.push_str(&format!("/{typ}/{name}"));
    }
    key
}
fn rs_setup_object_key(o: &Object) -> String {
    let r = Reader::default();
    rs_setup_key(
        &object_int(o, tagReferencedBrachySetupNumber),
        &object_int(o, ft(0x30080022)),
        &r.str(o, tagApplicationSetupType),
        &r.str(o, tagApplicationSetupName),
    )
}
fn rs_write_brachy(w: &mut WriteBuilder, o: &mut Object, rec: &TreatmentRecord) {
    w.str(
        o,
        tagBrachyTreatmentTechnique,
        Vr::CS,
        &rec.brachy_technique,
    );
    w.str(o, tagBrachyTreatmentType, Vr::CS, &rec.brachy_type);
    let mut source_old = w.keyed(o, ft(0x30080100), ft(0x300a0212));
    for source in &rec.brachy_sources {
        if !w.tick() {
            return;
        }
        let s = &source.brachy_source;
        let i = source_old.item(&s.number);
        let it = source_old.get(i);
        w.integer(it, ft(0x300a0212), s.number);
        w.str(it, ft(0x300a0214), Vr::CS, &s.typ);
        w.str(it, ft(0x300a0226), Vr::LO, &s.isotope_name);
        w.optional_text(it, ft(0x300a0229), Vr::CS, &s.strength_units);
        for (t, a) in [
            (ft(0x300a0228), &s.isotope_half_life),
            (ft(0x300a022a), &s.reference_air_kerma_rate),
            (ft(0x300a022b), &s.strength),
        ] {
            w.float(it, t, a);
        }
        w.str(it, ft(0x300a022c), Vr::DA, &s.strength_reference_date);
        w.str(it, ft(0x300a022e), Vr::TM, &s.strength_reference_time);
        rs_declared_text(w, it, ft(0x300a0216), Vr::LO, &source.manufacturer);
        rs_declared_text(
            w,
            it,
            ft(0x30080105),
            Vr::LO,
            &source.recorded_serial_number,
        );
        w.optional_text(it, ft(0x300a021b), Vr::SH, &source.model_id);
    }
    put_sequence(o, ft(0x30080100), &source_old.objects());
    let mut old = w.old_items(o, ft(0x30080110), rs_setup_object_key);
    let mut seen = HashSet::new();
    for s in &rec.brachy_sessions {
        if !w.tick() {
            return;
        }
        let key = rs_setup_key(
            &s.referenced_setup_number,
            &s.fraction_number,
            &s.typ,
            &s.name,
        );
        let i = w.new_item(&mut old, &mut seen, &key, ft(0x30080110));
        let it = old.get(i);
        w.int(
            it,
            tagReferencedBrachySetupNumber,
            &s.referenced_setup_number,
        );
        w.str(it, tagApplicationSetupType, Vr::CS, &s.typ);
        w.optional_text(it, tagApplicationSetupName, Vr::LO, &s.name);
        w.optional_text(it, ft(0x30080116), Vr::CS, &s.check);
        w.numbers(
            it,
            tagTotalReferenceAirKerma,
            &[s.total_reference_air_kerma],
        );
        rs_write_session(
            w,
            it,
            &s.fraction_number,
            &s.delivery_type,
            &s.verification_status,
            &s.termination_status,
        );
        w.reference(it, ft(0x300c0040), &s.verification_images);
        rs_write_dose_values(w, it, &s.measured_doses, false);
        rs_write_dose_values(w, it, &s.calculated_doses, true);
        let mut ch_old = w.keyed(it, ft(0x30080130), ft(0x300a0282));
        for ch in &s.channels {
            if !w.tick() {
                return;
            }
            let k = ch_old.item(&ch.number);
            let c = ch_old.get(k);
            w.integer(c, ft(0x300a0282), ch.number);
            w.integer(c, tagReferencedSourceNumber, ch.source_number);
            w.int(c, ft(0x00741406), &ch.referenced_channel_number);
            w.str(c, ft(0x300a0288), Vr::CS, &ch.movement);
            w.numbers(c, ft(0x30080132), &[ch.specified_time]);
            w.numbers(c, ft(0x30080134), &[ch.delivered_time]);
            for (t, a) in [
                (ft(0x300a0284), &ch.length),
                (ft(0x300a0271), &ch.effective_length),
                (ft(0x300a0272), &ch.inner_length),
                (ft(0x300a02a4), &ch.transfer_tube_length),
                (ft(0x3008013a), &ch.specified_pulse_interval),
                (ft(0x3008013c), &ch.delivered_pulse_interval),
            ] {
                w.float(c, t, a);
            }
            w.optional_text(c, ft(0x300a0273), Vr::SH, &ch.afterloader_channel_id);
            w.int(c, ft(0x300a02a2), &ch.transfer_tube_number);
            w.int(c, ft(0x30080136), &ch.specified_pulses);
            w.int(c, ft(0x30080138), &ch.delivered_pulses);
            rs_write_safe(w, c, &ch.safe_position);
            rs_write_dose_values(w, c, &ch.measured_doses, false);
            rs_write_dose_values(w, c, &ch.calculated_doses, true);
            let mut app_old = w.old_items(c, ft(0x30080140), |o| {
                let r = Reader::default();
                format!(
                    "{}/{}",
                    rs_attr_key(&object_int(o, ft(0x30080142))),
                    r.str(o, ft(0x300a0291))
                )
            });
            let mut app_seen = HashSet::new();
            for a in &ch.applicators {
                let key = format!(
                    "{}/{}",
                    rs_attr_key(&a.number),
                    a.id.value.as_deref().unwrap_or("")
                );
                let m = w.new_item(&mut app_old, &mut app_seen, &key, ft(0x30080140));
                let app = app_old.get(m);
                w.int(app, ft(0x30080142), &a.number);
                rs_declared_text(w, app, ft(0x300a0291), Vr::SH, &a.id);
                w.str(app, ft(0x300a0292), Vr::CS, &a.typ);
                w.optional_text(app, ft(0x300a0294), Vr::LO, &a.name);
                w.numbers(app, ft(0x300a0296), &[a.length]);
                w.float(app, ft(0x300a0274), &a.tip_length);
                w.float(app, ft(0x300a02a0), &a.step_size);
            }
            rs_sequence(c, ft(0x30080140), &app_old.objects(), false);
            if !rs_write_brachy_cp(w, c, ft(0x30080160), &ch.control_points) {
                return;
            }
            w.integer(c, tagNumberOfControlPoints, ch.control_points.len() as i64);
            let mut pulse_old = w.keyed(c, ft(0x30080171), ft(0x30080172));
            for p in &ch.pulses {
                if !w.tick() {
                    return;
                }
                let m = pulse_old.item(&p.number);
                let pulse = pulse_old.get(m);
                w.integer(pulse, ft(0x30080172), p.number);
                rs_write_safe(w, pulse, &p.safe_position);
                if !rs_write_brachy_cp(w, pulse, ft(0x30080173), &p.control_points) {
                    return;
                }
            }
            rs_sequence(c, ft(0x30080171), &pulse_old.objects(), false);
        }
        put_sequence(it, ft(0x30080130), &ch_old.objects());
    }
    put_sequence(o, ft(0x30080110), &old.objects());
}
fn rs_write_safe(w: &mut WriteBuilder, o: &mut Object, s: &RecordSafePosition) {
    for (t, vr, v) in [
        (ft(0x30080162), Vr::DA, &s.exit_date),
        (ft(0x30080164), Vr::TM, &s.exit_time),
        (ft(0x30080166), Vr::DA, &s.return_date),
        (ft(0x30080168), Vr::TM, &s.return_time),
    ] {
        w.optional_text(o, t, vr, v);
    }
}
fn rs_write_brachy_cp(
    w: &mut WriteBuilder,
    o: &mut Object,
    seq: Tag,
    points: &[RecordBrachyControlPoint],
) -> bool {
    let mut old = w.old_items(o, seq, rs_cp_object_key);
    let mut seen = HashSet::new();
    for p in points {
        if !w.tick() {
            return false;
        }
        let k = w.new_item(
            &mut old,
            &mut seen,
            &rs_cp_key(&p.referenced_index, &p.date, &p.time),
            seq,
        );
        let it = old.get(k);
        w.int(it, ft(0x300c00f0), &p.referenced_index);
        w.str(it, ft(0x30080024), Vr::DA, &p.date);
        w.str(it, ft(0x30080025), Vr::TM, &p.time);
        w.numbers(it, ft(0x300a02d2), &[p.relative_position]);
    }
    put_sequence(o, seq, &old.objects());
    true
}
fn rs_summary_group_key(number: &Attribute<i64>, typ: &Attribute<String>) -> String {
    let mut key = rs_attr_key(number);
    if let Some(t) = &typ.value {
        key.push('/');
        key.push_str(t);
    }
    key
}
fn rs_write_summary(w: &mut WriteBuilder, o: &mut Object, s: &RecordSummary) {
    w.str(o, ft(0x30080200), Vr::CS, &s.status);
    w.optional_text(o, ft(0x30080202), Vr::ST, &s.comment);
    rs_declared_text(w, o, ft(0x30080054), Vr::DA, &s.first_date);
    rs_declared_text(w, o, ft(0x30080056), Vr::DA, &s.most_recent_date);
    let mut old = w.old_items(o, ft(0x30080220), |o| {
        let r = Reader::default();
        rs_summary_group_key(
            &object_int(o, ft(0x300c0022)),
            &rs_text(&r, o, ft(0x30080224)),
        )
    });
    let mut seen = HashSet::new();
    for g in &s.fraction_groups {
        if !w.tick() {
            return;
        }
        let key = rs_summary_group_key(&g.referenced_number, &g.typ);
        let i = w.new_item(&mut old, &mut seen, &key, ft(0x30080220));
        let it = old.get(i);
        w.int(it, ft(0x300c0022), &g.referenced_number);
        rs_declared_text(w, it, ft(0x30080224), Vr::CS, &g.typ);
        w.int(it, tagNumberOfFractionsPlanned, &g.planned);
        w.int(it, ft(0x3008005a), &g.delivered);
        let mut fraction_old = w.keyed(it, ft(0x30080240), ft(0x30080223));
        for f in &g.fractions {
            if !w.tick() {
                return;
            }
            let k = fraction_old.item(&f.number);
            let v = fraction_old.get(k);
            w.integer(v, ft(0x30080223), f.number);
            w.int(v, ft(0x300a0705), &f.clinical_number);
            rs_declared_text(w, v, ft(0x30080250), Vr::DA, &f.date);
            rs_declared_text(w, v, ft(0x30080251), Vr::TM, &f.time);
            rs_declared_text(w, v, ft(0x3008002a), Vr::CS, &f.termination_status);
        }
        rs_sequence(it, ft(0x30080240), &fraction_old.objects(), false);
    }
    rs_sequence(o, ft(0x30080220), &old.objects(), false);
    rs_write_summary_doses(w, o, ft(0x300800e0), &s.measured_doses);
    rs_write_summary_doses(w, o, ft(0x30080050), &s.calculated_doses);
}
fn rs_write_summary_doses(
    w: &mut WriteBuilder,
    o: &mut Object,
    seq: Tag,
    doses: &[RecordSummaryDose],
) {
    let mut old = w.old_items(o, seq, |o| {
        let r = Reader::default();
        let a = object_int(o, tagReferencedDoseReferenceNumber);
        if a.value.is_some() {
            return rs_attr_key(&a);
        }
        format!("-/{}", r.str(o, tagDoseReferenceDescription))
    });
    let mut seen = HashSet::new();
    for d in doses {
        if !w.tick() {
            return;
        }
        let key = if d.plan_number.value.is_some() {
            rs_attr_key(&d.plan_number)
        } else {
            format!("-/{}", d.description)
        };
        let i = w.new_item(&mut old, &mut seen, &key, seq);
        let it = old.get(i);
        w.int(it, tagReferencedDoseReferenceNumber, &d.plan_number);
        w.optional_text(it, tagDoseReferenceDescription, Vr::LO, &d.description);
        w.numbers(it, ft(0x30080052), &[d.cumulative_dose]);
    }
    rs_sequence(o, seq, &old.objects(), false);
}
