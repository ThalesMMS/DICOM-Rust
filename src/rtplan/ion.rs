use super::read::{Reader, item, join};
use super::tags::*;
use super::validate::validate_family_devices;
use super::write::{Pool, WriteBuilder, put_sequence};
use super::*;
use crate::fmath;
use crate::object::{Object, Tag, Vr};
use std::collections::HashSet;
pub(crate) fn read_fl(r: &mut Reader, o: &Object, t: Tag, p: &str) -> Vec<f32> {
    let Some(e) = o.get(t) else {
        return Vec::new();
    };
    if e.vr() != Vr::FL {
        r.fail(join(p, t), "FL representation is required");
        return Vec::new();
    }
    r.number_list(o, t, p).iter().map(|&v| v as f32).collect()
}
pub(crate) fn species_inconsistent(atomic: i64, mass: i64, charge: i64) -> bool {
    atomic <= 0 || mass < atomic || charge == 0 || (charge as f64).abs() > atomic as f64
}
pub(crate) fn read_ion(r: &mut Reader, obj: &Object, p: &mut Plan) {
    let seq = ft(0x300a03a2);
    let cp_seq = ft(0x300a03a8);
    let items = r.sequence(obj, seq);
    if items.is_empty() {
        r.fail(seq.to_string(), "ion beams are missing");
    }
    if !p.beams.is_empty() {
        r.fail(
            tagBeamSequence.to_string(),
            "conventional beams are outside Ion profile",
        );
    }
    for (i, o) in items.iter().enumerate() {
        let path = item("", seq, i);
        let mut b = IonBeam {
            number: r.required_int(o, tagBeamNumber, &path, "Beam Number"),
            name: r.required_text(o, tagBeamName, &path),
            description: r.str(o, tagBeamDescription),
            typ: r.code(o, tagBeamType),
            radiation_type: r.code(o, tagRadiationType),
            scan_mode: r.code(o, ft(0x300a0308)),
            treatment_machine_name: r.str(o, tagTreatmentMachineName),
            primary_dosimeter_unit: r.code(o, tagPrimaryDosimeterUnit),
            treatment_delivery_type: r.code(o, tagTreatmentDeliveryType),
            patient_support_type: r.code(o, ft(0x300a0350)),
            atomic_number: r.int_attribute(o, ft(0x300a0304), &path),
            mass_number: r.int_attribute(o, ft(0x300a0302), &path),
            charge_state: r.int_attribute(o, ft(0x300a0306), &path),
            referenced_patient_setup_number: r.int_opt(o, tagReferencedPatientSetupNumber, &path),
            referenced_tolerance_table_number: r.int_opt(
                o,
                tagReferencedToleranceTableNumber,
                &path,
            ),
            final_cumulative_meterset_weight: r.float_attribute(
                o,
                tagFinalCumulativeMetersetWeight,
                &path,
            ),
            virtual_source_axis_distances: r.number_list(o, ft(0x300a030a), &path),
            ..IonBeam::default()
        };
        let vsad = &b.virtual_source_axis_distances;
        if vsad.len() != 2 || vsad[0] <= 0.0 || vsad[1] <= 0.0 {
            r.fail(
                join(&path, ft(0x300a030a)),
                "virtual source-axis distances require two positive values",
            );
        }
        if !one_of(&b.primary_dosimeter_unit, &["MU", "NP"]) {
            r.fail(
                join(&path, tagPrimaryDosimeterUnit),
                "dosimeter unit is outside profile",
            );
        }
        if !one_of(&b.typ, &["STATIC", "DYNAMIC"]) {
            r.fail(join(&path, tagBeamType), "beam type is outside profile");
        }
        if !one_of(&b.radiation_type, &["PROTON", "ION"]) {
            r.fail(
                join(&path, tagRadiationType),
                "radiation type is outside profile",
            );
        }
        if b.patient_support_type.is_empty() {
            r.fail(
                join(&path, ft(0x300a0350)),
                "patient support type is missing",
            );
        }
        if !one_of(&b.scan_mode, &["NONE", "MODULATED"]) {
            r.fail(join(&path, ft(0x300a0308)), "scan mode is outside profile");
        }
        if b.radiation_type == "ION" {
            match (
                b.atomic_number.value,
                b.mass_number.value,
                b.charge_state.value,
            ) {
                (Some(atomic), Some(mass), Some(charge)) => {
                    if species_inconsistent(atomic, mass, charge) {
                        r.fail(join(&path, ft(0x300a0306)), "ion species is inconsistent");
                    }
                }
                _ => r.fail(join(&path, ft(0x300a0302)), "ion species is incomplete"),
            }
        }
        for tag in [
            tagNumberOfWedges,
            tagNumberOfCompensators,
            tagNumberOfBoli,
            tagNumberOfBlocks,
            ft(0x300a0330),
            ft(0x300a0340),
        ] {
            if r.int_opt(o, tag, &path).is_some_and(|v| v != 0) {
                r.fail(join(&path, tag), "accessory is outside profile");
            }
        }
        for tag in [
            ft(0x300800a1),
            ft(0x300a03aa),
            ft(0x300a02ea),
            ft(0x300a03a6),
            ft(0x300c00b0),
            ft(0x300a0332),
            ft(0x300a0342),
            ft(0x300a0505),
        ] {
            if o.has(tag) {
                r.fail(join(&path, tag), "advanced accessory is outside profile");
            }
        }
        let flag = r.code(o, ft(0x300800a3));
        if !flag.is_empty() && flag != "NO" {
            r.fail(
                join(&path, ft(0x300800a3)),
                "enhanced devices are outside profile",
            );
        }
        let snouts = r.sequence(o, ft(0x300a030c));
        if snouts.len() > 1 {
            r.fail(join(&path, ft(0x300a030c)), "snout item count");
        }
        if snouts.len() == 1 {
            b.snout_id =
                r.required_text(&snouts[0], ft(0x300a030f), &item(&path, ft(0x300a030c), 0));
        }
        for (j, d) in r.sequence(o, ft(0x300a03a4)).iter().enumerate() {
            let dp = item(&path, ft(0x300a03a4), j);
            let typ = r.code(d, tagRTBeamLimitingDeviceType);
            let pairs = r.required_int(d, tagNumberOfLeafJawPairs, &dp, "Number of Leaf/Jaw Pairs");
            let leaf_boundaries = NilFloats(r.numbers(d, tagLeafPositionBoundaries, &dp));
            let source_distance = r.float_opt(d, ft(0x300a00bb), &dp);
            b.devices.push(BeamLimitingDevice {
                typ,
                source_distance,
                pairs,
                leaf_boundaries,
            });
        }
        validate_family_devices(r, &b.devices, &path, ft(0x300a03a4));
        if !r.errors.is_empty() {
            return;
        }
        let mut shifters: HashSet<i64> = HashSet::new();
        for (j, s) in r.sequence(o, ft(0x300a0314)).iter().enumerate() {
            let sp = item(&path, ft(0x300a0314), j);
            let rs = RangeShifter {
                number: r.required_int(s, ft(0x300a0316), &sp, "Range Shifter Number"),
                id: r.required_text(s, ft(0x300a0318), &sp),
                typ: r.code(s, ft(0x300a0320)),
                description: r.str(s, ft(0x300a0322)),
            };
            if !shifters.insert(rs.number) {
                r.fail(
                    join(&sp, ft(0x300a0316)),
                    "range shifter number is duplicated",
                );
            }
            if !one_of(&rs.typ, &["ANALOG", "BINARY"]) {
                r.fail(
                    join(&sp, ft(0x300a0320)),
                    "range shifter type is outside profile",
                );
            }
            b.range_shifters.push(rs);
        }
        let declared = r.required_int(o, ft(0x300a0312), &path, "Number of Range Shifters");
        r.check_count(
            o,
            ft(0x300a0314),
            &path,
            "Number of Range Shifters",
            declared,
        );
        let cp_items = r.sequence(o, cp_seq);
        let n = r.required_int(
            o,
            tagNumberOfControlPoints,
            &path,
            "Number of Control Points",
        );
        if n != cp_items.len() as i64 || n < 2 {
            r.fail(join(&path, cp_seq), "control point count is inconsistent");
        }
        let generic = r.read_control_points_at(&b.devices, &cp_items, &path, cp_seq);
        let mut snout: Attribute<f64> = Attribute::default();
        let mut pitch: Attribute<f64> = Attribute::default();
        let mut settings: Vec<RangeShifterSetting> = Vec::new();
        let mut pitch_direction = String::new();
        let mut table_pitch_direction = String::new();
        let mut table_roll_direction = String::new();
        for (j, c) in cp_items.iter().enumerate() {
            let cp_path = item(&path, cp_seq, j);
            let mut cp = IonControlPoint {
                control_point: generic[j].clone(),
                scan_spot_tune_id: r.str(c, ft(0x300a0390)),
                number_of_paintings: r.int_attribute(c, ft(0x300a039a), &cp_path),
                meterset_rate: r.float_attribute(c, ft(0x300a035a), &cp_path),
                ..IonControlPoint::default()
            };
            let v = r.float_attribute(c, ft(0x300a030d), &cp_path);
            if v.present {
                snout = v;
                cp.encoded_snout_position = true;
            }
            cp.snout_position = snout.clone();
            let v = r.float_attribute(c, ft(0x300a014a), &cp_path);
            if v.present {
                pitch = v;
                cp.encoded_gantry_pitch_angle = true;
            }
            cp.gantry_pitch_angle = pitch.clone();
            if c.has(ft(0x300a014c)) {
                pitch_direction = r.code(c, ft(0x300a014c));
            }
            cp.gantry_pitch_rotation_direction = pitch_direction.clone();
            if c.has(ft(0x300a0142)) {
                table_pitch_direction = r.code(c, ft(0x300a0142));
            }
            cp.table_top_pitch_rotation_direction = table_pitch_direction.clone();
            if c.has(ft(0x300a0146)) {
                table_roll_direction = r.code(c, ft(0x300a0146));
            }
            cp.table_top_roll_rotation_direction = table_roll_direction.clone();
            if c.has(ft(0x300a0360)) {
                cp.encoded_range_shifter_settings = true;
                let mut seen_settings: HashSet<i64> = HashSet::new();
                for (k, s) in r.sequence(c, ft(0x300a0360)).iter().enumerate() {
                    let sp = item(&cp_path, ft(0x300a0360), k);
                    let rs = RangeShifterSetting {
                        number: r.required_int(
                            s,
                            ft(0x300c0100),
                            &sp,
                            "Referenced Range Shifter Number",
                        ),
                        setting: r.required_text(s, ft(0x300a0362), &sp),
                        distance: r.float_attribute(s, ft(0x300a0364), &sp),
                        water_equivalent_thickness: r.float_attribute(s, ft(0x300a0366), &sp),
                    };
                    if !seen_settings.insert(rs.number) {
                        r.fail(
                            join(&sp, ft(0x300c0100)),
                            "range shifter setting is duplicated",
                        );
                    }
                    r.attr_nonnegative(&rs.water_equivalent_thickness, ft(0x300a0366), &sp, false);
                    if !shifters.contains(&rs.number) {
                        r.fail(
                            join(&sp, ft(0x300c0100)),
                            "range shifter reference is missing",
                        );
                    }
                    match settings.iter_mut().find(|m| m.number == rs.number) {
                        Some(m) => *m = rs,
                        None => settings.push(rs),
                    }
                }
            }
            cp.range_shifter_settings = settings.clone();
            if j == 0 && settings.len() != b.range_shifters.len() {
                r.fail(
                    join(&cp_path, ft(0x300a0360)),
                    "first range shifter settings are incomplete",
                );
            }
            if cp.control_point.index != j as i64 {
                r.fail(
                    join(&cp_path, tagControlPointIndex),
                    "control point index is inconsistent",
                );
            }
            if j == 0 {
                for field in [
                    FIELD_NOMINAL_BEAM_ENERGY,
                    FIELD_GANTRY_ANGLE,
                    FIELD_GANTRY_ROTATION_DIRECTION,
                    FIELD_BEAM_LIMITING_DEVICE_ANGLE,
                    FIELD_BEAM_LIMITING_DEVICE_ROTATION_DIRECTION,
                    FIELD_PATIENT_SUPPORT_ANGLE,
                    FIELD_PATIENT_SUPPORT_ROTATION_DIRECTION,
                ] {
                    if !cp.control_point.encodes(field) {
                        r.fail(cp_path.clone(), "first machine state is incomplete");
                    }
                }
                for t in [
                    ft(0x300a014a),
                    ft(0x300a014c),
                    tagTableTopPitchAngle,
                    ft(0x300a0142),
                    tagTableTopRollAngle,
                    ft(0x300a0146),
                    tagTableTopVerticalPosition,
                    tagTableTopLongitudinalPosition,
                    tagTableTopLateralPosition,
                    ft(0x300a030d),
                    tagIsocenterPosition,
                ] {
                    if !c.has(t) {
                        r.fail(join(&cp_path, t), "first Type 2 machine state is missing");
                    }
                }
                if cp.control_point.device_positions.len() != b.devices.len() {
                    r.fail(
                        join(&cp_path, tagBLDPositionSequence),
                        "first device positions are incomplete",
                    );
                }
            }
            for dir in [
                &cp.control_point.gantry_rotation_direction,
                &cp.control_point.beam_limiting_device_rotation_direction,
                &cp.control_point.patient_support_rotation_direction,
            ] {
                if !one_of(dir, &["CW", "CC", "NONE"]) {
                    r.fail(cp_path.clone(), "rotation direction is outside profile");
                }
            }
            for dir in [
                &pitch_direction,
                &table_pitch_direction,
                &table_roll_direction,
            ] {
                if !dir.is_empty() && !one_of(dir, &["CW", "CC", "NONE"]) {
                    r.fail(
                        cp_path.clone(),
                        "pitch or roll direction is outside profile",
                    );
                }
            }
            if !cp
                .control_point
                .nominal_beam_energy
                .is_some_and(|e| e > 0.0)
            {
                r.fail(
                    join(&cp_path, tagNominalBeamEnergy),
                    "energy must be positive",
                );
            }
            r.attr_nonnegative(&cp.meterset_rate, ft(0x300a035a), &cp_path, false);
            for t in [
                ft(0x300a03ac),
                ft(0x300a0370),
                ft(0x300a0380),
                ft(0x300800a2),
                ft(0x300a0302),
                ft(0x300a0304),
                ft(0x300a0306),
            ] {
                if c.has(t) {
                    r.fail(
                        join(&cp_path, t),
                        "control-point accessory or varying species is outside profile",
                    );
                }
            }
            if b.scan_mode == "NONE" {
                for t in [
                    ft(0x300a0390),
                    ft(0x300a0392),
                    ft(0x300a0394),
                    ft(0x300a0396),
                    ft(0x300a039a),
                ] {
                    if c.has(t) {
                        r.fail(
                            join(&cp_path, t),
                            "spot attributes are outside non-scanning profile",
                        );
                    }
                }
            }
            if b.scan_mode == "MODULATED" {
                let count =
                    r.required_int(c, ft(0x300a0392), &cp_path, "Number of Scan Spot Positions");
                cp.spot_positions = read_fl(r, c, ft(0x300a0394), &cp_path);
                cp.spot_weights = read_fl(r, c, ft(0x300a0396), &cp_path);
                if count <= 0
                    || cp.spot_positions.len() as i64 != 2 * count
                    || cp.spot_weights.len() as i64 != count
                {
                    r.fail(join(&cp_path, ft(0x300a0394)), "spot count is inconsistent");
                }
                if cp.scan_spot_tune_id.is_empty()
                    || !cp.number_of_paintings.value.is_some_and(|n| n > 0)
                {
                    r.fail(
                        join(&cp_path, ft(0x300a039a)),
                        "spot tune or positive paintings are missing",
                    );
                }
                for v in &cp.spot_weights {
                    if *v < 0.0 {
                        r.fail(join(&cp_path, ft(0x300a0396)), "spot weight is negative");
                    }
                }
            }
            b.control_points.push(cp);
        }
        for (j, cp) in b.control_points.iter().enumerate() {
            let cp_path = item(&path, cp_seq, j);
            let w = cp.control_point.cumulative_meterset_weight;
            if !w.is_some_and(|w| !(w < 0.0 || j == 0 && w != 0.0)) {
                r.fail(
                    join(&cp_path, tagCumulativeMetersetWeight),
                    "cumulative weight is missing or inconsistent",
                );
            }
            if j + 1 < b.control_points.len()
                && let Some(w) = w
                && let Some(next) = b.control_points[j + 1]
                    .control_point
                    .cumulative_meterset_weight
            {
                if next < w {
                    r.fail(
                        join(&cp_path, tagCumulativeMetersetWeight),
                        "cumulative weight decreases",
                    );
                }
                if b.scan_mode == "MODULATED" {
                    let mut sum = 0.0f64;
                    let mut abs = 0.0f64;
                    for v in &cp.spot_weights {
                        sum += f64::from(*v);
                        abs += f64::from(*v).abs();
                    }
                    let tolerance = fmath::ldexp(abs, -23)
                        + (fmath::next_up(next) - next).abs()
                        + (fmath::next_up(w) - w).abs();
                    if (sum - (next - w)).abs() > tolerance {
                        r.fail(
                            join(&cp_path, ft(0x300a0396)),
                            "spot sum differs from segment weight",
                        );
                    }
                }
            }
        }
        if let Some(last_cp) = b.control_points.last() {
            let last = last_cp.control_point.cumulative_meterset_weight;
            let consistent = match (last, b.final_cumulative_meterset_weight.value) {
                (Some(last), Some(f)) => last == f && last > 0.0,
                _ => false,
            };
            if !consistent {
                r.fail(
                    join(&path, tagFinalCumulativeMetersetWeight),
                    "final weight is inconsistent",
                );
            }
        }
        p.ion_beams.push(b);
    }
}
pub(crate) fn write_ion(w: &mut WriteBuilder, o: &mut Object, p: &Plan) {
    o.remove(tagBeamSequence);
    o.remove(tagApplicationSetupSequence);
    o.remove(ft(0x300a0210));
    o.remove(tagBrachyTreatmentTechnique);
    o.remove(tagBrachyTreatmentType);
    let mut previous = w.keyed(o, ft(0x300a03a2), tagBeamNumber);
    for b in &p.ion_beams {
        let i = previous.item(&b.number);
        let it = previous.get(i);
        w.integer(it, tagBeamNumber, b.number);
        for (t, vr, v) in [
            (tagBeamName, Vr::LO, &b.name),
            (tagBeamDescription, Vr::ST, &b.description),
            (tagBeamType, Vr::CS, &b.typ),
            (tagRadiationType, Vr::CS, &b.radiation_type),
            (ft(0x300a0308), Vr::CS, &b.scan_mode),
            (tagTreatmentMachineName, Vr::SH, &b.treatment_machine_name),
            (tagPrimaryDosimeterUnit, Vr::CS, &b.primary_dosimeter_unit),
            (tagTreatmentDeliveryType, Vr::CS, &b.treatment_delivery_type),
            (ft(0x300a0350), Vr::CS, &b.patient_support_type),
        ] {
            if !v.is_empty() || it.has(t) {
                w.str(it, t, vr, v);
            }
        }
        w.numbers(it, ft(0x300a030a), &b.virtual_source_axis_distances);
        w.int(it, ft(0x300a0302), &b.mass_number);
        w.int(it, ft(0x300a0304), &b.atomic_number);
        w.int(it, ft(0x300a0306), &b.charge_state);
        w.opt_int(
            it,
            tagReferencedPatientSetupNumber,
            b.referenced_patient_setup_number,
        );
        w.opt_int(
            it,
            tagReferencedToleranceTableNumber,
            b.referenced_tolerance_table_number,
        );
        w.float(
            it,
            tagFinalCumulativeMetersetWeight,
            &b.final_cumulative_meterset_weight,
        );
        for t in [
            tagNumberOfWedges,
            tagNumberOfCompensators,
            tagNumberOfBoli,
            tagNumberOfBlocks,
            ft(0x300a0330),
            ft(0x300a0340),
        ] {
            w.integer(it, t, 0);
        }
        let old_snouts = it.get_sequence(ft(0x300a030c)).unwrap_or_default();
        it.remove(ft(0x300a030c));
        if !b.snout_id.is_empty() {
            let mut snout = Object::new();
            if old_snouts.len() == 1
                && old_snouts[0].get_string(ft(0x300a030f)).unwrap_or_default() == b.snout_id
            {
                snout = old_snouts[0].clone();
            }
            w.str(&mut snout, ft(0x300a030f), Vr::SH, &b.snout_id);
            put_sequence(it, ft(0x300a030c), &[snout]);
        }
        let mut by_type: Pool<String> = Pool::new();
        for d in it.get_sequence(ft(0x300a03a4)).unwrap_or_default() {
            let key = d
                .get_string(tagRTBeamLimitingDeviceType)
                .unwrap_or_default();
            if by_type.insert_old(key, d) {
                w.err = Some(Error::bare(ErrorKind::InvalidObject));
            }
        }
        for d in &b.devices {
            let k = by_type.item(&d.typ);
            let v = by_type.get(k);
            w.str(v, tagRTBeamLimitingDeviceType, Vr::CS, &d.typ);
            w.integer(v, tagNumberOfLeafJawPairs, d.pairs);
            w.opt_float(v, ft(0x300a00bb), d.source_distance);
            v.remove(tagLeafPositionBoundaries);
            if !d.leaf_boundaries.is_empty() {
                w.numbers(v, tagLeafPositionBoundaries, &d.leaf_boundaries);
            }
        }
        it.remove(ft(0x300a03a4));
        if !by_type.is_empty() {
            put_sequence(it, ft(0x300a03a4), &by_type.objects());
        }
        let mut rs_old = w.keyed(it, ft(0x300a0314), ft(0x300a0316));
        for s in &b.range_shifters {
            let k = rs_old.item(&s.number);
            let v = rs_old.get(k);
            w.integer(v, ft(0x300a0316), s.number);
            w.str(v, ft(0x300a0318), Vr::SH, &s.id);
            w.str(v, ft(0x300a0320), Vr::CS, &s.typ);
            w.optional_text(v, ft(0x300a0322), Vr::LO, &s.description);
        }
        w.integer(it, ft(0x300a0312), rs_old.len() as i64);
        it.remove(ft(0x300a0314));
        if !rs_old.is_empty() {
            put_sequence(it, ft(0x300a0314), &rs_old.objects());
        }
        let mut cp_old = w.keyed(it, ft(0x300a03a8), tagControlPointIndex);
        for (j, cp) in b.control_points.iter().enumerate() {
            let k = cp_old.item(&cp.control_point.index);
            let v = cp_old.get(k);
            write_control_state(w, v, &cp.control_point);
            if j == 0 {
                for t in [
                    tagIsocenterPosition,
                    tagTableTopVerticalPosition,
                    tagTableTopLongitudinalPosition,
                    tagTableTopLateralPosition,
                    tagTableTopPitchAngle,
                    tagTableTopRollAngle,
                ] {
                    if !v.has(t) {
                        w.numbers(v, t, &[]);
                    }
                }
            }
            w.float(v, ft(0x300a035a), &cp.meterset_rate);
            w.float(v, ft(0x300a030d), &cp.snout_position);
            w.float(v, ft(0x300a014a), &cp.gantry_pitch_angle);
            for (t, s) in [
                (ft(0x300a014c), &cp.gantry_pitch_rotation_direction),
                (ft(0x300a0142), &cp.table_top_pitch_rotation_direction),
                (ft(0x300a0146), &cp.table_top_roll_rotation_direction),
            ] {
                if j == 0 || !s.is_empty() || v.has(t) {
                    w.str(v, t, Vr::CS, s);
                }
            }
            let mut settings_old = w.keyed(v, ft(0x300a0360), ft(0x300c0100));
            for s in &cp.range_shifter_settings {
                let m = settings_old.item(&s.number);
                let rs = settings_old.get(m);
                w.integer(rs, ft(0x300c0100), s.number);
                w.str(rs, ft(0x300a0362), Vr::LO, &s.setting);
                w.float(rs, ft(0x300a0364), &s.distance);
                w.float(rs, ft(0x300a0366), &s.water_equivalent_thickness);
            }
            v.remove(ft(0x300a0360));
            if !settings_old.is_empty() {
                put_sequence(v, ft(0x300a0360), &settings_old.objects());
            }
            for t in [
                ft(0x300a0390),
                ft(0x300a0392),
                ft(0x300a0394),
                ft(0x300a0396),
                ft(0x300a039a),
            ] {
                v.remove(t);
            }
            if b.scan_mode == "MODULATED" {
                w.str(v, ft(0x300a0390), Vr::SH, &cp.scan_spot_tune_id);
                w.integer(v, ft(0x300a0392), cp.spot_weights.len() as i64);
                w.fl(v, ft(0x300a0394), &cp.spot_positions);
                w.fl(v, ft(0x300a0396), &cp.spot_weights);
                w.int(v, ft(0x300a039a), &cp.number_of_paintings);
            }
        }
        w.integer(it, tagNumberOfControlPoints, cp_old.len() as i64);
        put_sequence(it, ft(0x300a03a8), &cp_old.objects());
    }
    put_sequence(o, ft(0x300a03a2), &previous.objects());
}
pub(crate) fn write_control_state(w: &mut WriteBuilder, o: &mut Object, cp: &ControlPoint) {
    w.integer(o, tagControlPointIndex, cp.index);
    w.opt_float(
        o,
        tagCumulativeMetersetWeight,
        cp.cumulative_meterset_weight,
    );
    for (t, v) in [
        (tagNominalBeamEnergy, cp.nominal_beam_energy),
        (tagDoseRateSet, cp.dose_rate_set),
        (tagTableTopVerticalPosition, cp.table_top_vertical_position),
        (
            tagTableTopLongitudinalPosition,
            cp.table_top_longitudinal_position,
        ),
        (tagTableTopLateralPosition, cp.table_top_lateral_position),
        (tagTableTopPitchAngle, cp.table_top_pitch_angle),
        (tagTableTopRollAngle, cp.table_top_roll_angle),
        (tagSourceToSurfaceDistance, cp.source_to_surface_distance),
    ] {
        w.opt_float(o, t, v);
    }
    for (t, v, field) in [
        (tagGantryAngle, cp.gantry_angle, FIELD_GANTRY_ANGLE),
        (
            tagBLDAngle,
            cp.beam_limiting_device_angle,
            FIELD_BEAM_LIMITING_DEVICE_ANGLE,
        ),
        (
            tagPatientSupportAngle,
            cp.patient_support_angle,
            FIELD_PATIENT_SUPPORT_ANGLE,
        ),
        (
            tagTableTopEccentricAngle,
            cp.table_top_eccentric_angle,
            FIELD_TABLE_TOP_ECCENTRIC_ANGLE,
        ),
    ] {
        if cp.encodes(field) || o.has(t) || field != FIELD_TABLE_TOP_ECCENTRIC_ANGLE {
            w.numbers(o, t, &[v]);
        } else {
            o.remove(t);
        }
    }
    for (t, v) in [
        (tagGantryRotationDirection, &cp.gantry_rotation_direction),
        (
            tagBLDRotationDirection,
            &cp.beam_limiting_device_rotation_direction,
        ),
        (
            tagPatientSupportRotationDir,
            &cp.patient_support_rotation_direction,
        ),
        (
            tagTableTopEccentricRotationDir,
            &cp.table_top_eccentric_rotation_direction,
        ),
    ] {
        w.optional_text(o, t, Vr::CS, v);
    }
    let isocenter_present = o.has(tagIsocenterPosition) || cp.encodes(FIELD_ISOCENTER_POSITION);
    o.remove(tagIsocenterPosition);
    if !cp.isocenter_position.is_empty() || isocenter_present {
        w.numbers(o, tagIsocenterPosition, &cp.isocenter_position);
    }
    let mut by_type: Pool<String> = Pool::new();
    for d in o.get_sequence(tagBLDPositionSequence).unwrap_or_default() {
        let key = d
            .get_string(tagRTBeamLimitingDeviceType)
            .unwrap_or_default();
        by_type.insert_old(key, d);
    }
    for d in &cp.device_positions {
        let k = by_type.item(&d.device_type);
        let v = by_type.get(k);
        w.str(v, tagRTBeamLimitingDeviceType, Vr::CS, &d.device_type);
        w.numbers(v, tagLeafJawPositions, &d.positions);
    }
    o.remove(tagBLDPositionSequence);
    if !by_type.is_empty() {
        put_sequence(o, tagBLDPositionSequence, &by_type.objects());
    }
    let mut refs_old = w.keyed(
        o,
        tagReferencedDoseReferenceSeq,
        tagReferencedDoseReferenceNumber,
    );
    for r in &cp.dose_references {
        let k = refs_old.item(&r.number);
        let v = refs_old.get(k);
        w.integer(v, tagReferencedDoseReferenceNumber, r.number);
        w.opt_float(v, tagCumulativeDoseRefCoefficient, r.coefficient);
    }
    o.remove(tagReferencedDoseReferenceSeq);
    if !refs_old.is_empty() {
        put_sequence(o, tagReferencedDoseReferenceSeq, &refs_old.objects());
    }
}
