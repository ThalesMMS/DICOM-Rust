use super::bounded::{
    EXPLICIT_VR_BIG_ENDIAN_UID, fresh_facade, native_syntax, preflight, preflight_prepared,
    strict_object, validate_file_meta_groups,
};
use super::read::Reader;
use super::reflect::model_admission;
use super::tags::*;
use super::*;
use crate::object::clone::{CloneFileOptions, clone_file_with_options};
use crate::object::element_setters::{format_ds, format_is};
use crate::object::transfer::{self, Syntax};
use crate::object::{
    DataSet, Element, ElementHeader, File, Object, Tag, Value, Vr, dict, is_valid_uid,
};
use crate::valueencode::reverse_fixed_width;
use std::collections::HashMap;
use std::hash::Hash;
#[derive(Clone, Debug, Default)]
pub struct WriteOptions {
    pub template: Option<File>,
    pub limits: Limits,
    pub transfer_syntax_uid: String,
    pub sop_instance_uid: String,
}
fn header(tag: Tag, vr: Vr) -> ElementHeader {
    ElementHeader {
        tag,
        vr,
        length: 0,
        length_set: false,
    }
}
fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: header(tag, vr),
        value: Value::Raw(raw),
    }
}
pub(crate) struct WriteBuilder {
    pub limits: Limits,
    pub err: Option<Error>,
}
impl WriteBuilder {
    pub fn str(&mut self, o: &mut Object, t: Tag, vr: Vr, s: &str) {
        self.strings(o, t, vr, vec![s.to_string()]);
    }
    pub fn strings(&mut self, o: &mut Object, t: Tag, vr: Vr, values: Vec<String>) {
        o.put(Element {
            header: header(t, vr),
            value: Value::Strings(values),
        });
    }
    pub fn integer(&mut self, o: &mut Object, t: Tag, n: i64) {
        if let Some(e) = dict::by_tag(t) {
            match e.vr {
                Vr::SS => {
                    let Ok(v) = i16::try_from(n) else {
                        self.err = Some(Error::bare(ErrorKind::InvalidObject));
                        return;
                    };
                    o.put(raw_element(t, Vr::SS, v.to_le_bytes().to_vec()));
                    return;
                }
                Vr::US => {
                    let Ok(v) = u16::try_from(n) else {
                        self.err = Some(Error::bare(ErrorKind::InvalidObject));
                        return;
                    };
                    o.put(raw_element(t, Vr::US, v.to_le_bytes().to_vec()));
                    return;
                }
                _ => {}
            }
        }
        match format_is(n) {
            Ok(s) => self.str(o, t, Vr::IS, &s),
            Err(_) => {
                self.err = Some(path_error(
                    ErrorKind::InvalidObject,
                    &t.to_string(),
                    "integer cannot be encoded",
                ));
            }
        }
    }
    pub fn numbers(&mut self, o: &mut Object, t: Tag, values: &[f64]) {
        match dict::by_tag(t).map(|e| e.vr) {
            Some(Vr::FD) => {
                let mut raw = Vec::with_capacity(values.len() * 8);
                for v in values {
                    raw.extend_from_slice(&v.to_le_bytes());
                }
                o.put(raw_element(t, Vr::FD, raw));
            }
            Some(Vr::FL) => {
                let mut raw = Vec::with_capacity(values.len() * 4);
                for &v in values {
                    if !finite(v) || f64::from(v as f32) != v {
                        self.err = Some(path_error(
                            ErrorKind::InvalidObject,
                            &t.to_string(),
                            "value is not exactly representable as FL",
                        ));
                        return;
                    }
                    raw.extend_from_slice(&(v as f32).to_le_bytes());
                }
                o.put(raw_element(t, Vr::FL, raw));
            }
            _ => {
                let mut out = Vec::with_capacity(values.len());
                for &v in values {
                    match format_ds(v) {
                        Ok(s) => out.push(s),
                        Err(_) => {
                            self.err = Some(path_error(
                                ErrorKind::InvalidObject,
                                &t.to_string(),
                                "decimal cannot be encoded",
                            ));
                            return;
                        }
                    }
                }
                self.strings(o, t, Vr::DS, out);
            }
        }
    }
    pub fn float(&mut self, o: &mut Object, t: Tag, a: &Attribute<f64>) {
        o.remove(t);
        if a.present {
            match a.value {
                None => self.numbers(o, t, &[]),
                Some(v) => self.numbers(o, t, &[v]),
            }
        }
    }
    pub fn int(&mut self, o: &mut Object, t: Tag, a: &Attribute<i64>) {
        o.remove(t);
        if a.present {
            match a.value {
                None => match dict::by_tag(t).map(|e| e.vr) {
                    Some(Vr::SS) => o.put(raw_element(t, Vr::SS, Vec::new())),
                    Some(Vr::US) => o.put(raw_element(t, Vr::US, Vec::new())),
                    _ => self.str(o, t, Vr::IS, ""),
                },
                Some(n) => self.integer(o, t, n),
            }
        }
    }
    #[doc(hidden)]
    pub fn fl(&mut self, o: &mut Object, t: Tag, v: &[f32]) {
        let mut raw = Vec::with_capacity(v.len() * 4);
        for x in v {
            raw.extend_from_slice(&x.to_le_bytes());
        }
        o.put(raw_element(t, Vr::FL, raw));
    }
    pub(crate) fn optional_text(&mut self, o: &mut Object, t: Tag, vr: Vr, s: &str) {
        o.remove(t);
        if !s.is_empty() {
            self.str(o, t, vr, s);
        }
    }
    #[doc(hidden)]
    pub fn opt_float(&mut self, o: &mut Object, t: Tag, v: Option<f64>) {
        let present = o.has(t);
        self.float(
            o,
            t,
            &Attribute {
                present: v.is_some() || present,
                value: v,
            },
        );
    }
    pub(crate) fn opt_int(&mut self, o: &mut Object, t: Tag, v: Option<i64>) {
        let present = o.has(t);
        self.int(
            o,
            t,
            &Attribute {
                present: v.is_some() || present,
                value: v,
            },
        );
    }
    pub fn keyed(&mut self, o: &Object, seq: Tag, key: Tag) -> Pool<i64> {
        let mut pool = Pool::new();
        for it in o.get_sequence(seq).unwrap_or_default() {
            let n = match it.get_ints(key) {
                Ok(v) if v.len() == 1 => v[0],
                _ => {
                    self.err = Some(path_error(
                        ErrorKind::InvalidObject,
                        &key.to_string(),
                        "template item key is invalid",
                    ));
                    continue;
                }
            };
            if pool.insert_old(n, it) {
                self.err = Some(path_error(
                    ErrorKind::InvalidObject,
                    &key.to_string(),
                    "template item key is duplicated",
                ));
            }
        }
        pool
    }
    pub fn reference(&mut self, o: &mut Object, seq: Tag, refs: &[Reference]) {
        let mut pool: Pool<String> = Pool::new();
        for it in o.get_sequence(seq).unwrap_or_default() {
            let uid = it.get_uid(tagRefSOPInstanceUID).unwrap_or_default();
            if pool.insert_old(uid, it) {
                self.err = Some(path_error(
                    ErrorKind::InvalidObject,
                    &seq.to_string(),
                    "template reference is duplicated",
                ));
            }
        }
        for reference in refs {
            let i = pool.item(&reference.sop_instance_uid);
            let it = pool.get(i);
            self.str(it, tagRefSOPClassUID, Vr::UI, &reference.sop_class_uid);
            self.str(
                it,
                tagRefSOPInstanceUID,
                Vr::UI,
                &reference.sop_instance_uid,
            );
        }
        o.remove(seq);
        if !pool.is_empty() {
            put_sequence(o, seq, &pool.objects());
        }
    }
    pub fn tick(&self) -> bool {
        self.err.is_none()
    }
}
pub(crate) struct Pool<K> {
    arena: Vec<Object>,
    map: HashMap<K, usize>,
    order: Vec<usize>,
}
impl<K: Hash + Eq + Clone> Pool<K> {
    pub fn new() -> Pool<K> {
        Pool {
            arena: Vec::new(),
            map: HashMap::new(),
            order: Vec::new(),
        }
    }
    pub(crate) fn insert_old(&mut self, key: K, item: Object) -> bool {
        self.arena.push(item);
        self.map.insert(key, self.arena.len() - 1).is_some()
    }
    pub fn contains(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }
    pub fn item(&mut self, key: &K) -> usize {
        let i = match self.map.get(key) {
            Some(&i) => i,
            None => {
                self.arena.push(Object::new());
                self.arena.len() - 1
            }
        };
        self.order.push(i);
        i
    }
    pub fn get(&mut self, i: usize) -> &mut Object {
        &mut self.arena[i]
    }
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
    pub fn len(&self) -> usize {
        self.order.len()
    }
    pub fn objects(&self) -> Vec<Object> {
        self.order.iter().map(|&i| self.arena[i].clone()).collect()
    }
}
pub(crate) fn put_sequence(o: &mut Object, t: Tag, items: &[Object]) {
    let sets = items
        .iter()
        .map(|it| DataSet {
            elements: it.sorted_elements(),
            item_offset: None,
        })
        .collect();
    o.put(Element {
        header: header(t, Vr::SQ),
        value: Value::Sequence(sets),
    });
}
pub(crate) fn rs_sequence(o: &mut Object, t: Tag, items: &[Object], required: bool) {
    o.remove(t);
    if required || !items.is_empty() {
        put_sequence(o, t, items);
    }
}
fn explicit_le_file(meta: Object, dataset: Object) -> File {
    File {
        preamble: Vec::new(),
        meta,
        dataset,
        transfer_syntax: transfer::EXPLICIT_VR_LITTLE_ENDIAN,
        partial_read: None,
        incomplete_pixel_data: None,
    }
}
pub(crate) fn begin_write(
    opts: &WriteOptions,
    sop: &str,
    pixels: bool,
    model: &dyn Reflect,
) -> Result<(File, WriteBuilder), Error> {
    let limits = opts.limits.normalized()?;
    let model_bytes = model_admission(model, &limits)?;
    let Some(src) = &opts.template else {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "",
            "explicit template is required",
        ));
    };
    let mut remaining = limits;
    remaining.max_model_bytes -= model_bytes;
    let dataset_admission = preflight_prepared(&src.dataset, remaining, pixels)?;
    remaining.max_metadata_bytes -= dataset_admission.bytes;
    remaining.max_model_bytes -= dataset_admission.model;
    remaining.max_elements -= dataset_admission.elements;
    let dataset = fresh_facade(&src.dataset, pixels);
    let class = dataset.get_uid(tagSOPClassUID).unwrap_or_default();
    let instance = dataset.get_uid(tagSOPInstanceUID).unwrap_or_default();
    if class != sop || instance.is_empty() {
        return Err(path_error(
            ErrorKind::InvalidObject,
            &tagSOPClassUID.to_string(),
            "template SOP identity",
        ));
    }
    preflight_prepared(&src.meta, remaining, false)?;
    validate_file_meta_groups(&src.meta)?;
    let mut meta = fresh_facade(&src.meta, false);
    let mc = meta.get_uid(ft(0x00020002)).unwrap_or_default();
    let mi = meta.get_uid(ft(0x00020003)).unwrap_or_default();
    if mc != class || mi != instance {
        return Err(path_error(
            ErrorKind::InvalidObject,
            "(0002,0002)",
            "template meta identity",
        ));
    }
    if !src.transfer_syntax.uid.is_empty() {
        if !native_syntax(&src.transfer_syntax) {
            return Err(Error::bare(ErrorKind::UnsupportedFeature));
        }
        if dataset.value_big_endian() != src.transfer_syntax.big_endian {
            return Err(path_error(
                ErrorKind::InvalidObject,
                "(0002,0010)",
                "template syntax and byte order differ",
            ));
        }
        let ts = meta.get_uid(ft(0x00020010)).unwrap_or_default();
        if ts != src.transfer_syntax.uid {
            return Err(path_error(
                ErrorKind::InvalidObject,
                "(0002,0010)",
                "template transfer syntax identity differs",
            ));
        }
    }
    refuse_retained_signatures(dataset.elements())?;
    refuse_retained_signatures(meta.elements())?;
    let mut w = WriteBuilder { limits, err: None };
    w.str(&mut meta, ft(0x00020002), Vr::UI, &class);
    w.str(&mut meta, ft(0x00020003), Vr::UI, &instance);
    w.str(
        &mut meta,
        ft(0x00020010),
        Vr::UI,
        transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid,
    );
    let dataset = native_order_facade(dataset, false)?;
    let f = clone_file_with_options(
        &explicit_le_file(meta, dataset),
        CloneFileOptions {
            max_element_bytes: limits.max_value_bytes,
            max_pixel_data_bytes: limits.max_pixel_bytes,
            max_total_bytes: limits.max_metadata_bytes + limits.max_pixel_bytes + (1 << 20),
            max_sequence_depth: limits.max_depth,
            max_elements: limits.max_elements,
            max_items: limits.max_elements,
            max_fragments: 0,
        },
    )
    .map_err(|_| {
        path_error(
            ErrorKind::InvalidObject,
            "",
            "bounded template clone failed",
        )
    })?;
    Ok((f, w))
}
pub(crate) fn finish_write(
    mut f: File,
    mut w: WriteBuilder,
    opts: &WriteOptions,
    sop: &str,
    changed: bool,
) -> Result<File, Error> {
    if let Some(e) = w.err.take() {
        return Err(e);
    }
    let original = f.dataset.get_uid(tagSOPInstanceUID).unwrap_or_default();
    let uid = if opts.sop_instance_uid.is_empty() {
        original.clone()
    } else {
        opts.sop_instance_uid.clone()
    };
    let instance_path = tagSOPInstanceUID.to_string();
    if !is_valid_uid(uid.as_bytes()) {
        return Err(path_error(
            ErrorKind::InvalidObject,
            &instance_path,
            "instance UID is invalid",
        ));
    }
    for t in [
        tagStudyInstanceUID,
        tagSeriesInstanceUID,
        tagFrameOfReferenceUID,
    ] {
        if f.dataset.get_uid(t).unwrap_or_default() == uid {
            return Err(path_error(
                ErrorKind::InvalidObject,
                &instance_path,
                "instance UID collides with another identity role",
            ));
        }
    }
    if changed && uid == original {
        return Err(path_error(
            ErrorKind::InvalidObject,
            &instance_path,
            "semantic change requires a new instance UID",
        ));
    }
    w.str(&mut f.dataset, tagSOPClassUID, Vr::UI, sop);
    w.str(&mut f.dataset, tagSOPInstanceUID, Vr::UI, &uid);
    w.str(&mut f.meta, ft(0x00020002), Vr::UI, sop);
    w.str(&mut f.meta, ft(0x00020003), Vr::UI, &uid);
    let syntax: Syntax = if opts.transfer_syntax_uid.is_empty() {
        transfer::EXPLICIT_VR_LITTLE_ENDIAN
    } else {
        let uid = opts.transfer_syntax_uid.as_str();
        if uid != transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid
            && uid != transfer::IMPLICIT_VR_LITTLE_ENDIAN.uid
            && uid != EXPLICIT_VR_BIG_ENDIAN_UID
        {
            return Err(Error::bare(ErrorKind::UnsupportedFeature));
        }
        match transfer::lookup(uid) {
            Some(s) => s,
            None => return Err(Error::bare(ErrorKind::UnsupportedFeature)),
        }
    };
    f.dataset = native_order_facade(f.dataset, syntax.big_endian)?;
    f.transfer_syntax = syntax;
    w.str(&mut f.meta, ft(0x00020010), Vr::UI, syntax.uid);
    let admitted = preflight(&f.dataset, &w.limits, sop == RT_IMAGE_STORAGE)?;
    let mut remaining = w.limits;
    remaining.max_metadata_bytes -= admitted.bytes;
    remaining.max_model_bytes -= admitted.model;
    remaining.max_elements -= admitted.elements;
    preflight_prepared(&f.meta, remaining, false)?;
    strict_object(&f.dataset, &w.limits)?;
    Ok(f)
}
pub(crate) fn native_order_facade(source: Object, big_endian: bool) -> Result<Object, Error> {
    if source.value_big_endian() == big_endian {
        return Ok(source);
    }
    fn visit(input: &[Element]) -> Result<Vec<Element>, Error> {
        let mut output = Vec::with_capacity(input.len());
        for e in input {
            let mut e = e.clone();
            match &e.value {
                Value::Raw(v) => {
                    let width = e.vr().byte_order_width();
                    if width > 1 {
                        if v.len() % width != 0 {
                            return Err(path_error(
                                ErrorKind::InvalidObject,
                                &e.tag().to_string(),
                                "raw fixed-width value extent is invalid",
                            ));
                        }
                        e.value = Value::Raw(reverse_fixed_width(v, width));
                    }
                }
                Value::Sequence(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for it in items {
                        out.push(DataSet {
                            elements: visit(&it.elements)?,
                            item_offset: None,
                        });
                    }
                    e.value = Value::Sequence(out);
                }
                _ => {}
            }
            output.push(e);
        }
        Ok(output)
    }
    let mut out = Object::from_elements(visit(source.elements())?);
    out.set_value_big_endian(big_endian);
    Ok(out)
}
fn refuse_retained_signatures(es: &[Element]) -> Result<(), Error> {
    for e in es {
        if e.tag() == ft(0x4ffe0001) || e.tag() == ft(0xfffafffa) {
            return Err(path_error(
                ErrorKind::UnsupportedFeature,
                &e.tag().to_string(),
                "retained digital signature metadata",
            ));
        }
        if let Value::Sequence(items) = &e.value {
            for it in items {
                refuse_retained_signatures(&it.elements)?;
            }
        }
    }
    Ok(())
}
pub(crate) fn object_int(o: &Object, t: Tag) -> Attribute<i64> {
    let mut r = Reader::default();
    Attribute {
        present: o.has(t),
        value: r.int_opt(o, t, ""),
    }
}
