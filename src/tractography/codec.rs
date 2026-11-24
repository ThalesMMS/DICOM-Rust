use super::budget::bulk_tag;
use super::{
    Code, CodeContext, CodeKind, Context, LONG_CODE, Result, SHORT_CODE, URN_CODE, check, invalid,
    unsupported,
};
use crate::object::{DataSet, Element, ElementHeader, Object, Tag, Value, Vr, element_length};
use crate::validation::iod::modules;
use std::collections::HashSet;
use std::sync::OnceLock;
pub(crate) const T_SOP_CLASS: Tag = Tag::new(8, 0x16);
pub(crate) const T_SOP_INSTANCE: Tag = Tag::new(8, 0x18);
pub(crate) const T_STUDY: Tag = Tag::new(0x20, 0xd);
pub(crate) const T_SERIES: Tag = Tag::new(0x20, 0xe);
pub(crate) const T_FRAME: Tag = Tag::new(0x20, 0x52);
pub(crate) const T_SERIES_NUMBER: Tag = Tag::new(0x20, 0x11);
pub(crate) const T_INSTANCE_NUMBER: Tag = Tag::new(0x20, 0x13);
pub(crate) const T_DATE: Tag = Tag::new(8, 0x23);
pub(crate) const T_TIME: Tag = Tag::new(8, 0x33);
pub(crate) const T_LABEL: Tag = Tag::new(0x70, 0x80);
pub(crate) const T_DESCRIPTION: Tag = Tag::new(0x70, 0x81);
pub(crate) const T_MODALITY: Tag = Tag::new(8, 0x60);
pub(crate) const T_CHARSET: Tag = Tag::new(8, 5);
pub(crate) const T_SETS: Tag = Tag::new(0x66, 0x101);
pub(crate) const T_NUMBER: Tag = Tag::new(0x66, 0x105);
pub(crate) const T_SET_LABEL: Tag = Tag::new(0x66, 0x106);
pub(crate) const T_SET_DESCRIPTION: Tag = Tag::new(0x66, 0x107);
pub(crate) const T_ANATOMY: Tag = Tag::new(0x66, 0x108);
pub(crate) const T_MODIFIER: Tag = Tag::new(0x40, 0xa195);
pub(crate) const T_TRACKS: Tag = Tag::new(0x66, 0x102);
pub(crate) const T_POINTS: Tag = Tag::new(0x66, 0x16);
pub(crate) const T_COLOR: Tag = Tag::new(0x62, 0xd);
pub(crate) const T_COLORS: Tag = Tag::new(0x66, 0x103);
pub(crate) const T_THICKNESS: Tag = Tag::new(0x66, 0x38);
pub(crate) const T_MEASUREMENTS: Tag = Tag::new(0x66, 0x121);
pub(crate) const T_QUANTITY: Tag = Tag::new(0x40, 0xa043);
pub(crate) const T_UNITS: Tag = Tag::new(0x40, 0x8ea);
pub(crate) const T_VALUES_SEQ: Tag = Tag::new(0x66, 0x132);
pub(crate) const T_FLOATS: Tag = Tag::new(0x66, 0x125);
pub(crate) const T_INDICES: Tag = Tag::new(0x66, 0x129);
pub(crate) const T_TRACK_STATS: Tag = Tag::new(0x66, 0x130);
pub(crate) const T_SET_STATS: Tag = Tag::new(0x66, 0x124);
pub(crate) const T_SCALAR: Tag = Tag::new(0x40, 0xa161);
pub(crate) const T_ACQUISITION: Tag = Tag::new(0x66, 0x133);
pub(crate) const T_MODEL: Tag = Tag::new(0x66, 0x134);
pub(crate) const T_ALGORITHMS: Tag = Tag::new(0x66, 0x104);
pub(crate) const T_FAMILY: Tag = Tag::new(0x66, 0x2f);
pub(crate) const T_NAME_CODE: Tag = Tag::new(0x66, 0x30);
pub(crate) const T_NAME: Tag = Tag::new(0x66, 0x36);
pub(crate) const T_VERSION: Tag = Tag::new(0x66, 0x31);
pub(crate) const T_PARAMETERS: Tag = Tag::new(0x66, 0x32);
pub(crate) const T_ALGORITHM_SOURCE: Tag = Tag::new(0x24, 0x202);
pub(crate) const T_REFS: Tag = Tag::new(8, 0x114a);
pub(crate) const T_REF_CLASS: Tag = Tag::new(8, 0x1150);
pub(crate) const T_REF_INSTANCE: Tag = Tag::new(8, 0x1155);
pub(crate) const T_FRAMES: Tag = Tag::new(8, 0x1160);
pub(crate) const T_SEGMENTS: Tag = Tag::new(0x62, 0xb);
pub(crate) const T_REF_SERIES: Tag = Tag::new(8, 0x1115);
pub(crate) const T_OTHER_STUDIES: Tag = Tag::new(8, 0x1200);
pub(crate) const T_PPS: Tag = Tag::new(8, 0x1111);
const T_CODE_VALUE: Tag = Tag::new(8, 0x100);
const T_CODE_SCHEME: Tag = Tag::new(8, 0x102);
const T_CODE_VERSION: Tag = Tag::new(8, 0x103);
const T_CODE_MEANING: Tag = Tag::new(8, 0x104);
const T_LONG_CODE: Tag = Tag::new(8, 0x119);
const T_URN: Tag = Tag::new(8, 0x120);
const T_EQUIVALENT: Tag = Tag::new(8, 0x121);
pub(crate) const MANAGED: [Tag; 17] = [
    T_SOP_CLASS,
    T_SOP_INSTANCE,
    T_STUDY,
    T_SERIES,
    T_FRAME,
    T_SERIES_NUMBER,
    T_INSTANCE_NUMBER,
    T_DATE,
    T_TIME,
    T_LABEL,
    T_DESCRIPTION,
    T_MODALITY,
    T_SETS,
    T_REFS,
    T_REF_SERIES,
    T_OTHER_STUDIES,
    T_PPS,
];
pub(crate) fn metadata_tag(tag: Tag) -> bool {
    static TAGS: OnceLock<HashSet<u32>> = OnceLock::new();
    let tags = TAGS.get_or_init(|| {
        let mut out = HashSet::new();
        for usage in modules::modules("Tractography Results").unwrap_or_default() {
            for a in modules::attributes(usage.module).unwrap_or_default() {
                out.insert(a.tag);
            }
        }
        out.insert(T_CHARSET.key());
        out
    });
    tags.contains(&tag.key())
}
fn element(tag: Tag, vr: Vr, value: Value) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value,
    }
}
pub(crate) fn text_element(t: Tag, vr: Vr, s: &str) -> Element {
    element(t, vr, Value::Strings(vec![s.to_string()]))
}
pub(crate) fn strings_element(t: Tag, vr: Vr, values: Vec<String>) -> Element {
    element(t, vr, Value::Strings(values))
}
pub(crate) fn raw_element(t: Tag, vr: Vr, data: Vec<u8>) -> Element {
    element(t, vr, Value::Raw(data))
}
pub(crate) fn sequence_element(t: Tag, items: Vec<Object>) -> Element {
    let items = items
        .into_iter()
        .map(|o| {
            let mut ds: DataSet = o.to_data_set();
            ds.elements.sort_by_key(Element::tag);
            ds
        })
        .collect();
    element(t, Vr::SQ, Value::Sequence(items))
}
pub(crate) fn string_value(o: &Object, t: Tag, vr: Vr, required: bool) -> Result<String> {
    let Some(e) = o.get(t) else {
        if required {
            return Err(invalid(t.to_string()));
        }
        return Ok(String::new());
    };
    if e.vr() != vr {
        return Err(invalid(format!("{t} VR")));
    }
    let v = match o.lookup_strings(t) {
        Ok(v) if v.len() <= 1 && !(required && (v.len() != 1 || v[0].is_empty())) => v,
        _ => return Err(invalid(format!("{t} text/VM"))),
    };
    Ok(v.into_iter().next().unwrap_or_default())
}
pub(crate) fn sequence(o: &Object, t: Tag, required: bool) -> Result<Option<Vec<Object>>> {
    let Some(e) = o.get(t) else {
        if required {
            return Err(invalid(t.to_string()));
        }
        return Ok(None);
    };
    if e.vr() != Vr::SQ {
        return Err(invalid(format!("{t} VR")));
    }
    match &e.value {
        Value::Sequence(items) if !items.is_empty() => {}
        _ => return Err(invalid(format!("{t} items"))),
    }
    match o.get_sequence(t) {
        Some(v) => Ok(Some(v)),
        None => Err(invalid(t.to_string())),
    }
}
pub(crate) fn required_sequence(o: &Object, t: Tag) -> Result<Vec<Object>> {
    Ok(sequence(o, t, true)?.unwrap_or_default())
}
pub(crate) fn ints(o: &Object, t: Tag, vr: Vr) -> Result<Vec<i64>> {
    if o.get(t).is_none_or(|e| e.vr() != vr) {
        return Err(invalid(format!("{t} VR")));
    }
    match o.get_ints(t) {
        Ok(v) if !v.is_empty() => Ok(v),
        _ => Err(invalid(format!("{t} values"))),
    }
}
pub(crate) fn scalar_float(o: &Object, t: Tag, vr: Vr) -> Result<f64> {
    if o.get(t).is_none_or(|e| e.vr() != vr) {
        return Err(invalid(format!("{t} VR")));
    }
    match o.get_floats(t) {
        Ok(v) if v.len() == 1 => Ok(v[0]),
        _ => Err(invalid(format!("{t} VM"))),
    }
}
fn word32(o: &Object, bytes: &[u8]) -> u32 {
    let b = [bytes[0], bytes[1], bytes[2], bytes[3]];
    if o.value_big_endian() {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    }
}
fn words32(ctx: &Context, o: &Object, t: Tag, vr: Vr, name: &str) -> Result<Vec<u32>> {
    let e = match o.get(t) {
        Some(e)
            if e.vr() == vr && element_length(e) != 0 && element_length(e).is_multiple_of(4) =>
        {
            e
        }
        _ => return Err(invalid(format!("{t} {name}"))),
    };
    let n = (element_length(e) / 4) as usize;
    let Value::Raw(src) = &e.value else {
        return Err(invalid(format!("{t} {name} kind")));
    };
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        if i % 1024 == 0 {
            check(ctx)?;
        }
        v.push(word32(o, &src[i * 4..]));
    }
    check(ctx)?;
    Ok(v)
}
pub(crate) fn f32s(ctx: &Context, o: &Object, t: Tag) -> Result<Vec<f32>> {
    Ok(words32(ctx, o, t, Vr::OF, "OF")?
        .into_iter()
        .map(f32::from_bits)
        .collect())
}
pub(crate) fn u32s(ctx: &Context, o: &Object, t: Tag) -> Result<Vec<u32>> {
    words32(ctx, o, t, Vr::OL, "OL")
}
pub(crate) fn color(o: &Object) -> Result<Option<[u16; 3]>> {
    if !o.has(T_COLOR) {
        return Ok(None);
    }
    match ints(o, T_COLOR, Vr::US) {
        Ok(v) if v.len() == 3 => Ok(Some([v[0] as u16, v[1] as u16, v[2] as u16])),
        _ => Err(invalid("color VM")),
    }
}
pub(crate) fn point_colors(ctx: &Context, o: &Object) -> Result<Option<Vec<[u16; 3]>>> {
    let Some(e) = o.get(T_COLORS) else {
        return Ok(None);
    };
    if e.vr() != Vr::OW || element_length(e) == 0 || !element_length(e).is_multiple_of(6) {
        return Err(invalid("point colors OW"));
    }
    let n = (element_length(e) / 6) as usize;
    let Value::Raw(src) = &e.value else {
        return Err(invalid("point colors OW kind"));
    };
    let big = o.value_big_endian();
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        check(ctx)?;
        let mut c = [0u16; 3];
        for (j, slot) in c.iter_mut().enumerate() {
            let at = (i * 3 + j) * 2;
            let b = [src[at], src[at + 1]];
            *slot = if big {
                u16::from_be_bytes(b)
            } else {
                u16::from_le_bytes(b)
            };
        }
        v.push(c);
    }
    Ok(Some(v))
}
fn canonical_raw(ctx: &Context, v: &mut [u8], vr: Vr) -> Result<()> {
    let width = match vr {
        Vr::US | Vr::SS | Vr::OW | Vr::AT => 2,
        Vr::UL | Vr::SL | Vr::FL | Vr::OF | Vr::OL => 4,
        Vr::UV | Vr::SV | Vr::FD | Vr::OD | Vr::OV => 8,
        _ => 1,
    };
    if !v.len().is_multiple_of(width) {
        return Err(invalid("metadata word length"));
    }
    for (i, word) in v.chunks_exact_mut(width).enumerate() {
        if (i * width) % 4096 == 0 {
            check(ctx)?;
        }
        word.reverse();
    }
    check(ctx)
}
pub(crate) fn clone_object(ctx: &Context, o: &Object, filter: bool) -> Result<Object> {
    let mut out = Object::new();
    out.put(text_element(T_CHARSET, Vr::CS, "ISO_IR 192"));
    for e in o.elements() {
        check(ctx)?;
        if e.tag() == T_CHARSET || bulk_tag(e.tag()) {
            continue;
        }
        if filter && (!metadata_tag(e.tag()) || MANAGED.contains(&e.tag())) {
            continue;
        }
        if matches!(e.value, Value::Sequence(_)) {
            let Some(children) = o.get_sequence(e.tag()) else {
                return Err(invalid("metadata sequence"));
            };
            let mut own = Vec::with_capacity(children.len());
            for c in &children {
                own.push(clone_object(ctx, c, false)?);
            }
            out.put(sequence_element(e.tag(), own));
            continue;
        }
        if e.vr().is_string_like() {
            let Ok(v) = o.lookup_strings(e.tag()) else {
                return Err(invalid("metadata text"));
            };
            out.put(strings_element(e.tag(), e.vr(), v));
            continue;
        }
        check(ctx)?;
        let mut value = match &e.value {
            Value::Nil => Vec::new(),
            Value::Raw(v) => v.clone(),
            _ => return Err(unsupported("metadata value")),
        };
        check(ctx)?;
        if o.value_big_endian() {
            canonical_raw(ctx, &mut value, e.vr())?;
        }
        out.put(raw_element(e.tag(), e.vr(), value));
    }
    Ok(out)
}
pub(crate) fn read_code(o: &Object, t: Tag) -> Result<Code> {
    match sequence(o, t, true) {
        Ok(Some(seq)) if seq.len() == 1 => read_code_item(&seq[0]),
        _ => Err(invalid(format!("{t} code cardinality"))),
    }
}
const CONTEXT_FIELDS: [(Tag, Vr); 9] = [
    (Tag::new(8, 0x10f), Vr::CS),
    (Tag::new(8, 0x117), Vr::UI),
    (Tag::new(8, 0x105), Vr::CS),
    (Tag::new(8, 0x118), Vr::UI),
    (Tag::new(8, 0x122), Vr::LO),
    (Tag::new(8, 0x106), Vr::DT),
    (Tag::new(8, 0x10b), Vr::CS),
    (Tag::new(8, 0x107), Vr::DT),
    (Tag::new(8, 0x10d), Vr::UI),
];
fn context_values(x: &CodeContext) -> [&String; 9] {
    [
        &x.identifier,
        &x.uid,
        &x.mapping_resource,
        &x.mapping_resource_uid,
        &x.mapping_resource_name,
        &x.version,
        &x.extension_flag,
        &x.local_version,
        &x.extension_creator_uid,
    ]
}
fn context_slots(x: &mut CodeContext) -> [&mut String; 9] {
    [
        &mut x.identifier,
        &mut x.uid,
        &mut x.mapping_resource,
        &mut x.mapping_resource_uid,
        &mut x.mapping_resource_name,
        &mut x.version,
        &mut x.extension_flag,
        &mut x.local_version,
        &mut x.extension_creator_uid,
    ]
}
pub(crate) fn read_code_item(o: &Object) -> Result<Code> {
    let mut c = Code::default();
    let mut count = 0;
    let forms: [(Tag, Vr, CodeKind); 3] = [
        (T_CODE_VALUE, Vr::SH, SHORT_CODE),
        (T_LONG_CODE, Vr::UC, LONG_CODE),
        (T_URN, Vr::UR, URN_CODE),
    ];
    for (t, vr, kind) in forms {
        if o.has(t) {
            c.value = string_value(o, t, vr, true)?;
            c.kind = kind;
            count += 1;
        }
    }
    if count != 1 {
        return Err(invalid("code value form"));
    }
    c.scheme = string_value(o, T_CODE_SCHEME, Vr::SH, c.kind != URN_CODE)?;
    c.scheme_version = string_value(o, T_CODE_VERSION, Vr::SH, false)?;
    c.meaning = string_value(o, T_CODE_MEANING, Vr::LO, true)?;
    let mut x = CodeContext::default();
    let mut present = false;
    for ((t, vr), slot) in CONTEXT_FIELDS.into_iter().zip(context_slots(&mut x)) {
        if o.has(t) {
            present = true;
        }
        *slot = string_value(o, t, vr, false)?;
    }
    if present {
        c.context = Some(x);
    }
    if o.has(T_EQUIVALENT) {
        let items = required_sequence(o, T_EQUIVALENT)?;
        let mut equivalent = Vec::with_capacity(items.len());
        for item in &items {
            let code = read_code_item(item)?;
            if code.equivalent.as_ref().is_some_and(|e| !e.is_empty()) {
                return Err(invalid("nested equivalent code"));
            }
            equivalent.push(code);
        }
        c.equivalent = Some(equivalent);
    }
    Ok(c)
}
pub(crate) fn code_item(c: &Code) -> Object {
    let mut o = Object::new();
    let (t, vr) = match c.kind {
        LONG_CODE => (T_LONG_CODE, Vr::UC),
        URN_CODE => (T_URN, Vr::UR),
        _ => (T_CODE_VALUE, Vr::SH),
    };
    o.put(text_element(t, vr, &c.value));
    if !c.scheme.is_empty() {
        o.put(text_element(T_CODE_SCHEME, Vr::SH, &c.scheme));
    }
    if !c.scheme_version.is_empty() {
        o.put(text_element(T_CODE_VERSION, Vr::SH, &c.scheme_version));
    }
    o.put(text_element(T_CODE_MEANING, Vr::LO, &c.meaning));
    if let Some(x) = &c.context {
        for ((t, vr), value) in CONTEXT_FIELDS.into_iter().zip(context_values(x)) {
            if !value.is_empty() {
                o.put(text_element(t, vr, value));
            }
        }
    }
    let equivalent = c.equivalent.as_deref().unwrap_or_default();
    if !equivalent.is_empty() {
        o.put(sequence_element(
            T_EQUIVALENT,
            equivalent.iter().map(code_item).collect(),
        ));
    }
    o
}
pub(crate) fn put_code(o: &mut Object, t: Tag, c: &Code) {
    o.put(sequence_element(t, vec![code_item(c)]));
}
pub(crate) fn scalar_fl(o: &Object, t: Tag) -> Result<f32> {
    let v = scalar_float(o, t, Vr::FL)?;
    if v.is_nan()
        && let Some(Value::Raw(x)) = o.get(t).map(|e| &e.value)
        && x.len() >= 4
    {
        return Ok(f32::from_bits(word32(o, x)));
    }
    Ok(v as f32)
}
