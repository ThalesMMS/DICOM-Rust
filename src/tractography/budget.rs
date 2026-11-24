use super::codec::{T_FLOATS, T_POINTS, T_REFS, T_SETS, T_TRACKS};
use super::{
    Code, Context, Error, Kind, Limits, Result, Results, check, invalid, limited, unsupported,
};
use crate::object::{Element, Object, Tag, Value, Vr, element_length};
use std::collections::HashSet;
pub(crate) fn normalize(mut l: Limits) -> Result<Limits> {
    let d = Limits::defaults();
    let options = || Error::sentinel(Kind::InvalidOptions);
    for (v, cap) in [
        (&mut l.max_bytes, d.max_bytes),
        (&mut l.max_value_bytes, d.max_value_bytes),
        (&mut l.max_text_bytes, d.max_text_bytes),
        (&mut l.max_track_sets, d.max_track_sets),
        (&mut l.max_tracks, d.max_tracks),
        (&mut l.max_points, d.max_points),
        (&mut l.max_values, d.max_values),
        (&mut l.max_references, d.max_references),
        (&mut l.max_elements, d.max_elements),
        (&mut l.max_items, d.max_items),
        (&mut l.max_sequence_depth, d.max_sequence_depth),
    ] {
        if *v < 0 || *v > cap {
            return Err(options());
        }
        if *v == 0 {
            *v = cap;
        }
    }
    Ok(l)
}
pub(crate) struct Budget<'a> {
    pub ctx: &'a Context,
    pub l: Limits,
    bytes: i64,
    text: i64,
    elements: i64,
    items: i64,
    tracks: i64,
    points: i64,
    values: i64,
    pub signatures: bool,
    pub nested_signatures: bool,
}
pub(crate) fn operation(ctx: &Context, l: Limits) -> Result<Budget<'_>> {
    check(ctx)?;
    let l = normalize(l)?;
    Ok(Budget {
        ctx,
        l,
        bytes: 0,
        text: 0,
        elements: 0,
        items: 0,
        tracks: 0,
        points: 0,
        values: 0,
        signatures: false,
        nested_signatures: false,
    })
}
pub(crate) fn bulk_tag(t: Tag) -> bool {
    t == Tag::PIXEL_DATA
        || t == Tag::new(0x7fe0, 8)
        || t == Tag::new(0x7fe0, 9)
        || t == Tag::new(0x5600, 0x20)
}
fn signature_tag(t: Tag) -> bool {
    t == Tag::new(0x4ffe, 1) || t == Tag::new(0xfffa, 0xfffa)
}
impl Budget<'_> {
    fn err(&self) -> Result<()> {
        check(self.ctx)
    }
    fn add(&mut self, n: i64) -> Result<()> {
        if n < 0 || n > self.l.max_bytes - self.bytes {
            return Err(limited("bytes"));
        }
        self.bytes += n;
        self.err()
    }
    pub fn payload(&mut self, n: i64, text: bool) -> Result<()> {
        if n < 0 || n > self.l.max_value_bytes {
            return Err(limited("value bytes"));
        }
        if text {
            if n > self.l.max_text_bytes - self.text {
                return Err(limited("text bytes"));
            }
            self.text += n;
        }
        let factor = if text { 64 } else { 8 };
        if n > (self.l.max_bytes - self.bytes) / factor {
            return Err(limited("copy bytes"));
        }
        self.add(n * factor)
    }
    fn structure(&mut self, elems: i64, items: i64) -> Result<()> {
        if elems < 0
            || elems > self.l.max_elements - self.elements
            || items < 0
            || items > self.l.max_items - self.items
        {
            return Err(limited("structure"));
        }
        self.elements += elems;
        self.items += items;
        self.add(elems * 512 + items * 1024)
    }
    pub(crate) fn text_values(&mut self, values: &[&str]) -> Result<()> {
        for s in values {
            self.payload(s.len() as i64, true)?;
        }
        Ok(())
    }
    fn source_structure(&mut self, elems: i64, items: i64) -> Result<()> {
        if elems < 0
            || items < 0
            || elems > (self.l.max_elements - self.elements) / 4
            || items > (self.l.max_items - self.items) / 4
        {
            return Err(limited("source structure"));
        }
        self.structure(elems * 4, items * 4)
    }
    fn array(&mut self, n: i64, width: i64) -> Result<()> {
        if n < 0 || width <= 0 || n > self.l.max_value_bytes / width {
            return Err(limited("array bytes"));
        }
        self.payload(n * width, false)
    }
    fn float_values(&mut self, n: i64) -> Result<()> {
        if n > self.l.max_values - self.values {
            return Err(limited("values"));
        }
        self.values += n;
        self.array(n, 4)
    }
    fn primitive_size(&mut self, e: &Element) -> Result<i64> {
        match &e.value {
            Value::Raw(v) => {
                if v.len() % 2 != 0 {
                    return Err(invalid("odd primitive length"));
                }
                if v.len() as i64 > self.l.max_value_bytes {
                    return Err(limited("value bytes"));
                }
                Ok(v.len() as i64)
            }
            Value::Strings(v) => {
                if !e.vr().is_string_like() {
                    return Err(invalid("text kind/VR"));
                }
                if v.len() as i64 > self.l.max_values {
                    return Err(limited("text VM"));
                }
                let mut n = 0i64;
                for (i, x) in v.iter().enumerate() {
                    self.err()?;
                    if x.len() as i64 > self.l.max_value_bytes - n {
                        return Err(limited("text length"));
                    }
                    n += x.len() as i64;
                    if i > 0 {
                        if n == self.l.max_value_bytes {
                            return Err(limited("text length"));
                        }
                        n += 1;
                    }
                }
                if n % 2 != 0 {
                    n += 1;
                }
                if n > self.l.max_value_bytes {
                    return Err(limited("text length"));
                }
                Ok(n)
            }
            _ => Err(unsupported("value kind")),
        }
    }
    fn walk(&mut self, es: &[Element], depth: i64, root: bool) -> Result<()> {
        self.err()?;
        if depth > self.l.max_sequence_depth {
            return Err(limited("sequence depth"));
        }
        self.source_structure(es.len() as i64, 0)?;
        for e in es {
            if signature_tag(e.tag()) {
                self.signatures = true;
                if depth > 0 {
                    self.nested_signatures = true;
                }
            }
            self.err()?;
            if root && bulk_tag(e.tag()) {
                continue;
            }
            if let Value::Sequence(items) = &e.value {
                let count = items.len() as i64;
                if e.tag() == T_SETS && count > self.l.max_track_sets {
                    return Err(limited("sets"));
                }
                if e.tag() == T_TRACKS {
                    if count > self.l.max_tracks - self.tracks {
                        return Err(limited("tracks"));
                    }
                    self.tracks += count;
                }
                if root && e.tag() == T_REFS && count > self.l.max_references {
                    return Err(limited("references"));
                }
                if e.vr() != Vr::SQ {
                    return Err(invalid("sequence VR"));
                }
                if count > 0 && depth == self.l.max_sequence_depth {
                    return Err(limited("sequence depth"));
                }
                self.source_structure(0, count)?;
                for item in items {
                    self.walk(&item.elements, depth + 1, false)?;
                }
                continue;
            }
            if e.vr() == Vr::SQ {
                return Err(unsupported("sequence"));
            }
            if e.value == Value::Nil {
                if !e.header.length_set || element_length(e) != 0 {
                    return Err(unsupported("deferred value"));
                }
                continue;
            }
            let n = self.primitive_size(e)?;
            if e.header.length_set && u64::from(element_length(e)) != n as u64 {
                return Err(invalid("value length"));
            }
            if e.tag() == T_POINTS {
                if e.vr() != Vr::OF || n % 12 != 0 {
                    return Err(invalid("point stream"));
                }
                let count = n / 12;
                if count > self.l.max_points - self.points {
                    return Err(limited("points"));
                }
                self.points += count;
            }
            if e.tag() == T_FLOATS {
                if e.vr() != Vr::OF || n % 4 != 0 {
                    return Err(invalid("measurement stream"));
                }
                let count = n / 4;
                if count > self.l.max_values - self.values {
                    return Err(limited("values"));
                }
                self.values += count;
            }
            if let Value::Strings(v) = &e.value {
                if v.len() as i64 > self.l.max_values {
                    return Err(limited("text VM"));
                }
                self.add(v.len() as i64 * 128)?;
            }
            self.payload(n, e.vr().is_string_like())?;
        }
        Ok(())
    }
    pub fn facade(&mut self, src: &Object) -> Result<Object> {
        self.err()?;
        let len = src.len() as i64;
        if len > (self.l.max_elements - self.elements) / 4 {
            return Err(limited("root elements"));
        }
        self.add(len * 512)?;
        let es = src.elements();
        self.walk(es, 0, true)?;
        duplicates(self.ctx, es, true)?;
        let mut admitted = Vec::with_capacity(es.len());
        for e in es {
            self.err()?;
            if !bulk_tag(e.tag()) {
                admitted.push(e.clone());
            }
        }
        let mut o = Object::from_elements(admitted);
        o.set_value_big_endian(src.value_big_endian());
        if o.character_set().is_err() {
            return Err(invalid("character set"));
        }
        Ok(o)
    }
    fn code(&mut self, c: &Code, depth: i64) -> Result<()> {
        if depth > self.l.max_sequence_depth {
            return Err(limited("code depth"));
        }
        self.structure(20, 1)?;
        self.text_values(&[&c.value, &c.scheme, &c.meaning, &c.scheme_version])?;
        if let Some(x) = &c.context {
            self.text_values(&[
                &x.identifier,
                &x.uid,
                &x.mapping_resource,
                &x.mapping_resource_uid,
                &x.mapping_resource_name,
                &x.version,
                &x.extension_flag,
                &x.local_version,
                &x.extension_creator_uid,
            ])?;
        }
        let equivalent = c.equivalent.as_deref().unwrap_or_default();
        if equivalent.len() as i64 > self.l.max_items - self.items {
            return Err(limited("equivalent codes"));
        }
        for v in equivalent {
            self.code(v, depth + 1)?;
        }
        Ok(())
    }
    pub fn model(&mut self, r: Option<&Results>) -> Result<()> {
        let Some(r) = r else {
            return Err(invalid("nil Results"));
        };
        if r.track_sets.len() as i64 > self.l.max_track_sets
            || r.references.len() as i64 > self.l.max_references
        {
            return Err(limited("sets or references"));
        }
        self.structure(64, 4)?;
        self.text_values(&[
            &r.sop_instance_uid,
            &r.study_instance_uid,
            &r.series_instance_uid,
            &r.frame_of_reference_uid,
            &r.content_date,
            &r.content_time,
            &r.label,
            &r.description,
        ])?;
        for s in &r.track_sets {
            self.structure(20, 1)?;
            self.text_values(&[&s.label, &s.description])?;
            if s.tracks.len() as i64 > self.l.max_tracks - self.tracks {
                return Err(limited("tracks"));
            }
            self.tracks += s.tracks.len() as i64;
            for c in s.anatomy_modifiers.iter().flatten() {
                self.code(c, 3)?;
            }
            for c in [&s.anatomy, &s.model] {
                self.code(c, 2)?;
            }
            if let Some(c) = &s.acquisition {
                self.code(c, 2)?;
            }
            for a in &s.algorithms {
                self.structure(6, 1)?;
                self.text_values(&[&a.name, &a.version, &a.parameters, &a.source])?;
                self.code(&a.family, 3)?;
                if let Some(c) = &a.name_code {
                    self.code(c, 3)?;
                }
            }
            for t in &s.tracks {
                if t.points.len() as i64 > self.l.max_points - self.points {
                    return Err(limited("points"));
                }
                self.points += t.points.len() as i64;
                self.structure(3, 1)?;
                self.array(t.points.len() as i64, 12)?;
                self.array(t.point_colors.as_ref().map_or(0, Vec::len) as i64, 6)?;
            }
            for m in s.measurements.iter().flatten() {
                self.structure(3, 1)?;
                for c in [&m.quantity, &m.units] {
                    self.code(c, 3)?;
                }
                for v in &m.tracks {
                    self.structure(2, 1)?;
                    self.float_values(v.values.len() as i64)?;
                    self.array(v.point_indices.as_ref().map_or(0, Vec::len) as i64, 4)?;
                }
            }
            for v in s.track_statistics.iter().flatten() {
                self.structure(4, 1)?;
                for c in [&v.quantity, &v.modifier, &v.units] {
                    self.code(c, 3)?;
                }
                self.float_values(v.values.len() as i64)?;
            }
            for v in s.statistics.iter().flatten() {
                self.structure(4, 1)?;
                for c in [&v.quantity, &v.modifier, &v.units] {
                    self.code(c, 3)?;
                }
                self.payload(8, false)?;
            }
        }
        for reference in &r.references {
            if self.l.max_sequence_depth < 3 {
                return Err(limited("reference depth"));
            }
            self.structure(10, 4)?;
            self.text_values(&[
                &reference.sop_class_uid,
                &reference.sop_instance_uid,
                &reference.study_instance_uid,
                &reference.series_instance_uid,
            ])?;
            self.array(reference.frames.as_ref().map_or(0, Vec::len) as i64, 16)?;
            self.array(reference.segments.as_ref().map_or(0, Vec::len) as i64, 2)?;
        }
        if let Some(p) = &r.performed_procedure_step {
            self.text_values(&[&p.sop_class_uid, &p.sop_instance_uid])?;
        }
        self.err()
    }
}
fn duplicates(ctx: &Context, es: &[Element], root: bool) -> Result<()> {
    let mut seen = HashSet::with_capacity(es.len());
    for e in es {
        check(ctx)?;
        if !seen.insert(e.tag()) {
            return Err(invalid("duplicate attribute"));
        }
        if root && bulk_tag(e.tag()) {
            continue;
        }
        if let Value::Sequence(items) = &e.value {
            for item in items {
                duplicates(ctx, &item.elements, false)?;
            }
        }
    }
    Ok(())
}
