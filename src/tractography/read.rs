use super::budget::{Budget, operation};
use super::codec::*;
use super::validation::{check_iod, metadata_valid, model_valid};
use super::{
    Algorithm, Context, DICOM_IMAGES, ImageReference, Measurement, MeasurementValues, ReadOptions,
    Result, Results, STORAGE, SopReference, Track, TrackSet, TrackSetStatistic, TrackStatistic,
    UNKNOWN_ORIGIN, check, invalid, invalid_reference, limited,
};
use crate::object::{Object, Tag, Vr, element_length};
use std::collections::{HashMap, HashSet};
pub fn read(ctx: &Context, src: Option<&Object>, opts: ReadOptions) -> Result<Results> {
    let mut b = operation(ctx, opts.limits)?;
    let Some(src) = src else {
        return Err(invalid("nil Object"));
    };
    let o = b.facade(src)?;
    let mut r = Results {
        origin: UNKNOWN_ORIGIN,
        ..Results::default()
    };
    if string_value(&o, T_SOP_CLASS, Vr::UI, true).ok().as_deref() != Some(STORAGE) {
        return Err(invalid("SOP class"));
    }
    if string_value(&o, T_MODALITY, Vr::CS, true).ok().as_deref() != Some("MR") {
        return Err(invalid("Modality"));
    }
    r.sop_instance_uid = string_value(&o, T_SOP_INSTANCE, Vr::UI, true)?;
    r.study_instance_uid = string_value(&o, T_STUDY, Vr::UI, true)?;
    r.series_instance_uid = string_value(&o, T_SERIES, Vr::UI, true)?;
    r.frame_of_reference_uid = string_value(&o, T_FRAME, Vr::UI, true)?;
    r.content_date = string_value(&o, T_DATE, Vr::DA, true)?;
    r.content_time = string_value(&o, T_TIME, Vr::TM, true)?;
    r.label = string_value(&o, T_LABEL, Vr::CS, true)?;
    r.description = string_value(&o, T_DESCRIPTION, Vr::LO, false)?;
    r.series_number = scalar_is(&o, T_SERIES_NUMBER)?;
    r.instance_number = scalar_is(&o, T_INSTANCE_NUMBER)?;
    for s in &required_sequence(&o, T_SETS)? {
        r.track_sets.push(read_set(&b, s)?);
    }
    r.references = read_references(&b, &o, &r.study_instance_uid)?;
    if !r.references.is_empty() {
        r.origin = DICOM_IMAGES;
    }
    if opts.require_dicom_references && r.origin == UNKNOWN_ORIGIN {
        return Err(invalid_reference());
    }
    if o.has(T_PPS) {
        match sequence(&o, T_PPS, true) {
            Ok(Some(v)) if v.len() == 1 => {
                r.performed_procedure_step = Some(read_pair(&v[0])?);
            }
            _ => return Err(invalid("PPS cardinality")),
        }
    }
    model_valid(&b, &r, false)?;
    check_iod(&o, &b)?;
    let metadata = clone_object(b.ctx, &o, true)?;
    metadata_valid(&b, &metadata)?;
    r.metadata = Some(metadata);
    check(b.ctx)?;
    Ok(r)
}
fn scalar_is(o: &Object, t: Tag) -> Result<i32> {
    match ints(o, t, Vr::IS) {
        Ok(v) if v.len() == 1 && i32::try_from(v[0]).is_ok() => Ok(v[0] as i32),
        _ => Err(invalid(format!("{t} IS"))),
    }
}
fn read_set(b: &Budget<'_>, o: &Object) -> Result<TrackSet> {
    let mut s = TrackSet::default();
    match ints(o, T_NUMBER, Vr::UL) {
        Ok(n) if n.len() == 1 && n[0] >= 1 => s.number = n[0] as u32,
        _ => return Err(invalid("set number")),
    }
    s.label = string_value(o, T_SET_LABEL, Vr::LO, true)?;
    s.description = string_value(o, T_SET_DESCRIPTION, Vr::UT, false)?;
    s.anatomy = read_code(o, T_ANATOMY)?;
    let anatomy = required_sequence(o, T_ANATOMY)?;
    if anatomy[0].has(T_MODIFIER) {
        let mut modifiers = Vec::new();
        for m in &required_sequence(&anatomy[0], T_MODIFIER)? {
            modifiers.push(read_code_item(m)?);
        }
        s.anatomy_modifiers = Some(modifiers);
    }
    s.color = color(o)?;
    if o.has(T_THICKNESS) {
        s.line_thickness = Some(scalar_fl(o, T_THICKNESS)?);
    }
    s.model = read_code(o, T_MODEL)?;
    if o.has(T_ACQUISITION) {
        s.acquisition = Some(read_code(o, T_ACQUISITION)?);
    }
    for o in &required_sequence(o, T_ALGORITHMS)? {
        let mut a = Algorithm {
            family: read_code(o, T_FAMILY)?,
            ..Algorithm::default()
        };
        if o.has(T_NAME_CODE) {
            a.name_code = Some(read_code(o, T_NAME_CODE)?);
        }
        a.name = string_value(o, T_NAME, Vr::LO, true)?;
        a.version = string_value(o, T_VERSION, Vr::LO, true)?;
        a.parameters = string_value(o, T_PARAMETERS, Vr::LT, false)?;
        a.source = string_value(o, T_ALGORITHM_SOURCE, Vr::LO, false)?;
        s.algorithms.push(a);
    }
    for o in &required_sequence(o, T_TRACKS)? {
        check(b.ctx)?;
        let v = f32s(b.ctx, o, T_POINTS)?;
        if v.len() < 6 || v.len() % 3 != 0 {
            return Err(invalid("xyz count"));
        }
        let mut t = Track {
            points: Vec::with_capacity(v.len() / 3),
            ..Track::default()
        };
        for (i, p) in v.as_chunks::<3>().0.iter().enumerate() {
            if i % 1024 == 0 {
                check(b.ctx)?;
            }
            t.points.push([p[0], p[1], p[2]]);
        }
        t.color = color(o)?;
        if let Some(e) = o.get(T_COLORS)
            && u64::from(element_length(e)) != t.points.len() as u64 * 6
        {
            return Err(invalid("point color count"));
        }
        t.point_colors = point_colors(b.ctx, o)?;
        s.tracks.push(t);
    }
    if let Some(ms) = sequence(o, T_MEASUREMENTS, false)? {
        let mut measurements = Vec::with_capacity(ms.len());
        for o in &ms {
            let mut m = Measurement {
                quantity: read_code(o, T_QUANTITY)?,
                units: read_code(o, T_UNITS)?,
                tracks: Vec::new(),
            };
            let vs = required_sequence(o, T_VALUES_SEQ)?;
            if vs.len() != s.tracks.len() {
                return Err(invalid("measurement track count"));
            }
            for o in &vs {
                let mut v = MeasurementValues {
                    values: f32s(b.ctx, o, T_FLOATS)?,
                    point_indices: None,
                };
                if o.has(T_INDICES) {
                    v.point_indices = Some(u32s(b.ctx, o, T_INDICES)?);
                }
                m.tracks.push(v);
            }
            measurements.push(m);
        }
        s.measurements = Some(measurements);
    }
    if let Some(ss) = sequence(o, T_TRACK_STATS, false)? {
        let mut statistics = Vec::with_capacity(ss.len());
        for o in &ss {
            statistics.push(TrackStatistic {
                quantity: read_code(o, T_QUANTITY)?,
                modifier: read_code(o, T_MODIFIER)?,
                units: read_code(o, T_UNITS)?,
                values: f32s(b.ctx, o, T_FLOATS)?,
            });
        }
        s.track_statistics = Some(statistics);
    }
    if let Some(ss) = sequence(o, T_SET_STATS, false)? {
        let mut statistics = Vec::with_capacity(ss.len());
        for o in &ss {
            statistics.push(TrackSetStatistic {
                quantity: read_code(o, T_QUANTITY)?,
                modifier: read_code(o, T_MODIFIER)?,
                units: read_code(o, T_UNITS)?,
                value: scalar_float(o, T_SCALAR, Vr::FD)?,
            });
        }
        s.statistics = Some(statistics);
    }
    Ok(s)
}
fn read_pair(o: &Object) -> Result<SopReference> {
    Ok(SopReference {
        sop_class_uid: string_value(o, T_REF_CLASS, Vr::UI, true)?,
        sop_instance_uid: string_value(o, T_REF_INSTANCE, Vr::UI, true)?,
    })
}
fn add_series(
    b: &Budget<'_>,
    h: &mut HashMap<String, ImageReference>,
    parent: &Object,
    study: &str,
) -> Result<()> {
    for s in &required_sequence(parent, T_REF_SERIES)? {
        let series = string_value(s, T_SERIES, Vr::UI, true)?;
        for item in &required_sequence(s, T_REFS)? {
            let p = read_pair(item)?;
            if h.contains_key(&p.sop_instance_uid) {
                return Err(invalid_reference());
            }
            h.insert(
                p.sop_instance_uid.clone(),
                ImageReference {
                    sop_class_uid: p.sop_class_uid,
                    sop_instance_uid: p.sop_instance_uid,
                    study_instance_uid: study.to_string(),
                    series_instance_uid: series.clone(),
                    frames: None,
                    segments: None,
                },
            );
            if h.len() as i64 > b.l.max_references {
                return Err(limited("hierarchical references"));
            }
            if item.has(T_FRAMES) || item.has(T_SEGMENTS) {
                return Err(invalid("selectors in hierarchy"));
            }
        }
    }
    Ok(())
}
fn read_references(b: &Budget<'_>, o: &Object, study: &str) -> Result<Vec<ImageReference>> {
    let mut h: HashMap<String, ImageReference> = HashMap::new();
    if o.has(T_REF_SERIES) {
        add_series(b, &mut h, o, study)?;
    }
    if o.has(T_OTHER_STUDIES) {
        let mut seen = HashSet::new();
        for s in &required_sequence(o, T_OTHER_STUDIES)? {
            let id = string_value(s, T_STUDY, Vr::UI, true)?;
            if id == study || !seen.insert(id.clone()) {
                return Err(invalid_reference());
            }
            add_series(b, &mut h, s, &id)?;
        }
    }
    let rs = sequence(o, T_REFS, false)?.unwrap_or_default();
    if rs.len() as i64 > b.l.max_references {
        return Err(limited("references"));
    }
    let mut out: Vec<ImageReference> = Vec::with_capacity(rs.len());
    let mut seen: HashMap<String, usize> = HashMap::new();
    for o in &rs {
        let p = read_pair(o)?;
        let mut r = match h.get(&p.sop_instance_uid) {
            Some(r) if r.sop_class_uid == p.sop_class_uid => r.clone(),
            _ => return Err(invalid_reference()),
        };
        if o.has(T_FRAMES) {
            let v = ints(o, T_FRAMES, Vr::IS)?;
            let mut frames = Vec::with_capacity(v.len());
            for f in v {
                if f <= 0 || f > 2_147_483_647 {
                    return Err(invalid_reference());
                }
                frames.push(f as i32);
            }
            r.frames = Some(frames);
        }
        if o.has(T_SEGMENTS) {
            let v = ints(o, T_SEGMENTS, Vr::US)?;
            let mut segments = Vec::with_capacity(v.len());
            for s in v {
                if s == 0 {
                    return Err(invalid_reference());
                }
                segments.push(s as u16);
            }
            r.segments = Some(segments);
        }
        if let Some(&old) = seen.get(&p.sop_instance_uid) {
            if out[old] != r {
                return Err(invalid_reference());
            }
            continue;
        }
        seen.insert(p.sop_instance_uid, out.len());
        out.push(r);
    }
    if seen.len() != h.len() {
        return Err(invalid_reference());
    }
    Ok(out)
}
