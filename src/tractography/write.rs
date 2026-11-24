use super::budget::{Budget, operation};
use super::codec::*;
use super::validation::{
    canonical_references, check_iod, metadata_valid, model_valid, required_text,
};
use super::{
    Context, ImageReference, Result, Results, STORAGE, SopReference, TrackSet, WriteOptions, check,
    invalid, unsupported,
};
use crate::dcmtime;
use crate::object::inherit::inherit_modules;
use crate::object::{Object, Tag, Vr};
use std::collections::HashMap;
const T_SIGNATURE_MAC: Tag = Tag::new(0x4ffe, 1);
const T_SIGNATURE: Tag = Tag::new(0xfffa, 0xfffa);
const EQUIPMENT_TAGS: [Tag; 4] = [
    Tag::new(8, 0x70),
    Tag::new(8, 0x1090),
    Tag::new(0x18, 0x1000),
    Tag::new(0x18, 0x1020),
];
pub fn write(ctx: &Context, r: Option<&Results>, opts: &WriteOptions) -> Result<Object> {
    let mut b = operation(ctx, opts.limits)?;
    if r.and_then(|r| r.metadata.as_ref())
        .is_some_and(|m| m.has(T_SIGNATURE_MAC) || m.has(T_SIGNATURE))
    {
        return Err(unsupported("signed metadata rewrite"));
    }
    b.model(r)?;
    let Some(r) = r else {
        return Err(invalid("nil Results"));
    };
    model_valid(&b, r, true)?;
    let mut metadata = None;
    if let Some(m) = &r.metadata {
        let facade = b.facade(m)?;
        if b.signatures {
            return Err(unsupported("signed metadata rewrite"));
        }
        metadata_valid(&b, &facade)?;
        metadata = Some(facade);
    }
    let common_source = match &opts.common.source {
        Some(source) => {
            let facade = b.facade(source)?;
            if b.nested_signatures {
                return Err(unsupported("signed common metadata rewrite"));
            }
            for (t, want) in [
                (T_STUDY, &r.study_instance_uid),
                (T_FRAME, &r.frame_of_reference_uid),
            ] {
                if facade.has(t)
                    && string_value(&facade, t, Vr::UI, true).ok().as_ref() != Some(want)
                {
                    return Err(invalid("common source identity"));
                }
            }
            let mut clone = clone_object(b.ctx, &facade, true)?;
            clone.remove(T_SIGNATURE_MAC);
            clone.remove(T_SIGNATURE);
            metadata_valid(&b, &clone)?;
            Some(clone)
        }
        None => metadata.clone(),
    };
    if let Some(now) = &opts.common.now
        && !now.is_zero()
        && !(1..=9999).contains(&now.year())
    {
        return Err(invalid("creation date"));
    }
    if let Some(e) = &opts.common.equipment {
        for s in [
            &e.manufacturer,
            &e.model_name,
            &e.device_serial_number,
            &e.software_versions,
        ] {
            if !required_text(s, 64, false) {
                return Err(invalid("enhanced equipment"));
            }
            b.text_values(&[s])?;
        }
    }
    let mut o = match &metadata {
        Some(m) => clone_object(b.ctx, m, true)?,
        None => Object::new(),
    };
    o.put(text_element(T_CHARSET, Vr::CS, "ISO_IR 192"));
    for (t, vr, s) in [
        (T_SOP_CLASS, Vr::UI, STORAGE),
        (T_SOP_INSTANCE, Vr::UI, r.sop_instance_uid.as_str()),
        (T_STUDY, Vr::UI, r.study_instance_uid.as_str()),
        (T_SERIES, Vr::UI, r.series_instance_uid.as_str()),
        (T_FRAME, Vr::UI, r.frame_of_reference_uid.as_str()),
        (T_MODALITY, Vr::CS, "MR"),
        (T_DATE, Vr::DA, r.content_date.as_str()),
        (T_TIME, Vr::TM, r.content_time.as_str()),
        (T_LABEL, Vr::CS, r.label.as_str()),
        (T_DESCRIPTION, Vr::LO, r.description.as_str()),
    ] {
        o.put(text_element(t, vr, s));
    }
    o.put(text_element(
        T_SERIES_NUMBER,
        Vr::IS,
        &r.series_number.to_string(),
    ));
    o.put(text_element(
        T_INSTANCE_NUMBER,
        Vr::IS,
        &r.instance_number.to_string(),
    ));
    let position_reference = Tag::new(0x20, 0x1040);
    if !o.has(position_reference) {
        o.put(text_element(position_reference, Vr::LO, ""));
    }
    if opts.common.equipment.is_some() {
        for t in EQUIPMENT_TAGS {
            o.remove(t);
        }
    } else {
        let found = EQUIPMENT_TAGS.iter().filter(|t| o.has(**t)).count();
        if found != 0 && found != 4 {
            return Err(invalid("partial retained equipment"));
        }
    }
    let mut common = opts.common.clone();
    common.source = common_source;
    if common.apply(&mut o, inherit_modules::ALL).is_err() {
        return Err(invalid("common modules"));
    }
    match string_value(&o, Tag::new(8, 0x12), Vr::DA, true) {
        Ok(creation) if creation.len() == 8 && dcmtime::parse_da(&creation).is_ok() => {}
        _ => return Err(invalid("creation date")),
    }
    let mut sets = Vec::with_capacity(r.track_sets.len());
    for s in &r.track_sets {
        sets.push(write_set(&b, s)?);
    }
    o.put(sequence_element(T_SETS, sets));
    if let Some(p) = &r.performed_procedure_step {
        o.put(sequence_element(
            T_PPS,
            vec![pair(&p.sop_class_uid, &p.sop_instance_uid)],
        ));
    }
    let rs = canonical_references(b.ctx, r, None)?;
    if !rs.is_empty() {
        write_references(b.ctx, &mut o, &rs, &r.study_instance_uid)?;
    }
    check_iod(&o, &b)?;
    check(b.ctx)?;
    Ok(o)
}
fn put_color(o: &mut Object, c: &Option<[u16; 3]>) {
    if let Some(c) = c {
        let data = c.iter().flat_map(|v| v.to_le_bytes()).collect();
        o.put(raw_element(T_COLOR, Vr::US, data));
    }
}
fn of_bytes(ctx: &Context, values: &[f32]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for (i, f) in values.iter().enumerate() {
        if i % 1024 == 0 {
            check(ctx)?;
        }
        out.extend_from_slice(&f.to_bits().to_le_bytes());
    }
    check(ctx)?;
    Ok(out)
}
fn write_set(b: &Budget<'_>, s: &TrackSet) -> Result<Object> {
    let mut o = Object::new();
    o.put(raw_element(
        T_NUMBER,
        Vr::UL,
        s.number.to_le_bytes().to_vec(),
    ));
    o.put(text_element(T_SET_LABEL, Vr::LO, &s.label));
    if !s.description.is_empty() {
        o.put(text_element(T_SET_DESCRIPTION, Vr::UT, &s.description));
    }
    let mut a = code_item(&s.anatomy);
    let modifiers = s.anatomy_modifiers.as_deref().unwrap_or_default();
    if !modifiers.is_empty() {
        a.put(sequence_element(
            T_MODIFIER,
            modifiers.iter().map(code_item).collect(),
        ));
    }
    o.put(sequence_element(T_ANATOMY, vec![a]));
    put_color(&mut o, &s.color);
    if let Some(thickness) = s.line_thickness {
        o.put(raw_element(
            T_THICKNESS,
            Vr::FL,
            thickness.to_bits().to_le_bytes().to_vec(),
        ));
    }
    put_code(&mut o, T_MODEL, &s.model);
    if let Some(acquisition) = &s.acquisition {
        put_code(&mut o, T_ACQUISITION, acquisition);
    }
    let mut algorithms = Vec::with_capacity(s.algorithms.len());
    for a in &s.algorithms {
        let mut v = Object::new();
        put_code(&mut v, T_FAMILY, &a.family);
        if let Some(name_code) = &a.name_code {
            put_code(&mut v, T_NAME_CODE, name_code);
        }
        v.put(text_element(T_NAME, Vr::LO, &a.name));
        v.put(text_element(T_VERSION, Vr::LO, &a.version));
        if !a.parameters.is_empty() {
            v.put(text_element(T_PARAMETERS, Vr::LT, &a.parameters));
        }
        if !a.source.is_empty() {
            v.put(text_element(T_ALGORITHM_SOURCE, Vr::LO, &a.source));
        }
        algorithms.push(v);
    }
    o.put(sequence_element(T_ALGORITHMS, algorithms));
    let mut tracks = Vec::with_capacity(s.tracks.len());
    for t in &s.tracks {
        check(b.ctx)?;
        let mut v = Object::new();
        let mut points = Vec::with_capacity(t.points.len() * 12);
        for (j, p) in t.points.iter().enumerate() {
            if j % 1024 == 0 {
                check(b.ctx)?;
            }
            for f in p {
                points.extend_from_slice(&f.to_bits().to_le_bytes());
            }
        }
        v.put(raw_element(T_POINTS, Vr::OF, points));
        put_color(&mut v, &t.color);
        if let Some(point_colors) = &t.point_colors {
            let mut colors = Vec::with_capacity(point_colors.len() * 6);
            for (j, c) in point_colors.iter().enumerate() {
                if j % 1024 == 0 {
                    check(b.ctx)?;
                }
                for n in c {
                    colors.extend_from_slice(&n.to_le_bytes());
                }
            }
            v.put(raw_element(T_COLORS, Vr::OW, colors));
        }
        tracks.push(v);
    }
    o.put(sequence_element(T_TRACKS, tracks));
    let measurements = s.measurements.as_deref().unwrap_or_default();
    if !measurements.is_empty() {
        let mut ms = Vec::with_capacity(measurements.len());
        for m in measurements {
            let mut v = Object::new();
            put_code(&mut v, T_QUANTITY, &m.quantity);
            put_code(&mut v, T_UNITS, &m.units);
            let mut vs = Vec::with_capacity(m.tracks.len());
            for value in &m.tracks {
                let mut item = Object::new();
                item.put(raw_element(
                    T_FLOATS,
                    Vr::OF,
                    of_bytes(b.ctx, &value.values)?,
                ));
                if let Some(indices) = &value.point_indices {
                    let mut own = Vec::with_capacity(indices.len() * 4);
                    for (i, n) in indices.iter().enumerate() {
                        if i % 1024 == 0 {
                            check(b.ctx)?;
                        }
                        own.extend_from_slice(&n.to_le_bytes());
                    }
                    check(b.ctx)?;
                    item.put(raw_element(T_INDICES, Vr::OL, own));
                }
                vs.push(item);
            }
            v.put(sequence_element(T_VALUES_SEQ, vs));
            ms.push(v);
        }
        o.put(sequence_element(T_MEASUREMENTS, ms));
    }
    let track_statistics = s.track_statistics.as_deref().unwrap_or_default();
    if !track_statistics.is_empty() {
        let mut ss = Vec::with_capacity(track_statistics.len());
        for s in track_statistics {
            let mut v = Object::new();
            put_code(&mut v, T_QUANTITY, &s.quantity);
            put_code(&mut v, T_MODIFIER, &s.modifier);
            put_code(&mut v, T_UNITS, &s.units);
            v.put(raw_element(T_FLOATS, Vr::OF, of_bytes(b.ctx, &s.values)?));
            ss.push(v);
        }
        o.put(sequence_element(T_TRACK_STATS, ss));
    }
    let statistics = s.statistics.as_deref().unwrap_or_default();
    if !statistics.is_empty() {
        let mut ss = Vec::with_capacity(statistics.len());
        for s in statistics {
            let mut v = Object::new();
            put_code(&mut v, T_QUANTITY, &s.quantity);
            put_code(&mut v, T_MODIFIER, &s.modifier);
            put_code(&mut v, T_UNITS, &s.units);
            v.put(raw_element(
                T_SCALAR,
                Vr::FD,
                s.value.to_bits().to_le_bytes().to_vec(),
            ));
            ss.push(v);
        }
        o.put(sequence_element(T_SET_STATS, ss));
    }
    check(b.ctx)?;
    Ok(o)
}
fn pair(class: &str, instance: &str) -> Object {
    let mut o = Object::new();
    o.put(text_element(T_REF_CLASS, Vr::UI, class));
    o.put(text_element(T_REF_INSTANCE, Vr::UI, instance));
    o
}
fn write_references(
    ctx: &Context,
    o: &mut Object,
    rs: &[ImageReference],
    study: &str,
) -> Result<()> {
    let mut flat = Vec::with_capacity(rs.len());
    let mut series_index: HashMap<&str, usize> = HashMap::new();
    let mut series: Vec<(&str, Vec<SopReference>)> = Vec::new();
    let mut same: Vec<usize> = Vec::new();
    let mut study_index: HashMap<&str, usize> = HashMap::new();
    let mut studies: Vec<(&str, Vec<usize>)> = Vec::new();
    for r in rs {
        check(ctx)?;
        let mut v = pair(&r.sop_class_uid, &r.sop_instance_uid);
        if let Some(frames) = &r.frames {
            let mut s = Vec::with_capacity(frames.len());
            for (i, f) in frames.iter().enumerate() {
                if i % 1024 == 0 {
                    check(ctx)?;
                }
                s.push(f.to_string());
            }
            v.put(strings_element(T_FRAMES, Vr::IS, s));
        }
        if let Some(segments) = &r.segments {
            check(ctx)?;
            let data = segments.iter().flat_map(|n| n.to_le_bytes()).collect();
            v.put(raw_element(T_SEGMENTS, Vr::US, data));
        }
        flat.push(v);
        let at = match series_index.get(r.series_instance_uid.as_str()) {
            Some(&at) => at,
            None => {
                let at = series.len();
                series_index.insert(&r.series_instance_uid, at);
                series.push((&r.series_instance_uid, Vec::new()));
                if r.study_instance_uid == study {
                    same.push(at);
                } else {
                    let s = match study_index.get(r.study_instance_uid.as_str()) {
                        Some(&s) => s,
                        None => {
                            study_index.insert(&r.study_instance_uid, studies.len());
                            studies.push((&r.study_instance_uid, Vec::new()));
                            studies.len() - 1
                        }
                    };
                    studies[s].1.push(at);
                }
                at
            }
        };
        series[at].1.push(SopReference {
            sop_class_uid: r.sop_class_uid.clone(),
            sop_instance_uid: r.sop_instance_uid.clone(),
        });
    }
    let series_item = |at: usize| {
        let (uid, instances) = &series[at];
        let mut s = Object::new();
        s.put(text_element(T_SERIES, Vr::UI, uid));
        s.put(sequence_element(
            T_REFS,
            instances
                .iter()
                .map(|p| pair(&p.sop_class_uid, &p.sop_instance_uid))
                .collect(),
        ));
        s
    };
    o.put(sequence_element(T_REFS, flat));
    if !same.is_empty() {
        o.put(sequence_element(
            T_REF_SERIES,
            same.iter().map(|&at| series_item(at)).collect(),
        ));
    }
    if !studies.is_empty() {
        let items = studies
            .iter()
            .map(|(uid, members)| {
                let mut s = Object::new();
                s.put(text_element(T_STUDY, Vr::UI, uid));
                s.put(sequence_element(
                    T_REF_SERIES,
                    members.iter().map(|&at| series_item(at)).collect(),
                ));
                s
            })
            .collect();
        o.put(sequence_element(T_OTHER_STUDIES, items));
    }
    check(ctx)
}
