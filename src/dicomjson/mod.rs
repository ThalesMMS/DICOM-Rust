#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
#[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
pub mod dataset_stream;
pub mod encode_stream;
mod error;
#[doc(hidden)]
pub mod jsonwire;
#[doc(hidden)]
pub mod sizedparse;
use crate::dictionary::private::private_creator_id;
use crate::object::dict;
use crate::object::error::Op;
use crate::object::parser::source::MemFile;
use crate::object::parser::{Reader, ReaderOptions};
use crate::object::{
    DataSet, Element, ElementHeader, Object, Syntax, Tag, TextOptions, UNDEFINED_LENGTH, Value, Vr,
};
use crate::valueencode;
pub use error::Error;
pub use jsonwire::{JAny, RawElement};
use jsonwire::{JVal, JsonError, Token};
use std::rc::Rc;
pub const ERR_INVALID_LIMITS: &str = "dicomjson: invalid resource limits";
pub const ERR_MAX_JSON_BYTES: &str = "dicomjson: maximum JSON bytes exceeded";
pub const ERR_MAX_INLINE_BINARY_BYTES: &str = "dicomjson: maximum InlineBinary bytes exceeded";
pub const ERR_MAX_TOTAL_BINARY_BYTES: &str = "dicomjson: maximum total binary bytes exceeded";
pub const ERR_MAX_SEQUENCE_DEPTH: &str = "dicomjson: maximum sequence depth exceeded";
pub const ERR_MAX_ELEMENTS: &str = "dicomjson: maximum element count exceeded";
pub const ERR_MAX_SEQUENCE_ITEMS: &str = "dicomjson: maximum sequence item count exceeded";
pub const ERR_EXPECTED_DATASET_OBJECT: &str = "dicomjson: expected dataset object";
pub const ERR_DUPLICATE_DATASET_TAG: &str = "dicomjson: duplicate dataset tag";
pub const ERR_PIXEL_DATA_ENCODING_MISMATCH: &str =
    "dicom: Pixel Data encoding does not match the transfer syntax";
#[doc(hidden)]
pub const GO_NAN: f64 = f64::from_bits(0x7FF8_0000_0000_0001);
pub const DEFAULT_MAX_SEQUENCE_DEPTH: i64 = 64;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_json_bytes: i64,
    pub max_inline_binary_bytes: i64,
    pub max_total_binary_bytes: i64,
    pub max_sequence_depth: i64,
    pub max_elements: i64,
    pub max_sequence_items: i64,
}
pub type BulkDataUriFunc<'a> = dyn FnMut(Tag, Vr, &[u8]) -> String + 'a;
pub(crate) type PixelDataUriFunc<'a> =
    dyn FnMut(Tag, Vr, &[u8]) -> Result<String, crate::BoxError> + 'a;
#[derive(Default)]
pub struct Options<'a> {
    pub pretty: bool,
    pub omit_group_length: bool,
    pub byte_order: Option<bool>,
    pub source_byte_order_inline_binary: bool,
    pub bulk_data_uri_func: Option<Box<BulkDataUriFunc<'a>>>,
    pub pixel_data_bulk_data_uri_func: Option<Box<PixelDataUriFunc<'a>>>,
    pub max_materialized_value_bytes: i64,
    pub limits: Limits,
}
impl Options<'_> {
    pub fn defaults() -> Options<'static> {
        Options {
            pretty: true,
            omit_group_length: true,
            ..Options::default()
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct UnmarshalOptions {
    pub text_options: TextOptions,
    pub byte_order: Option<bool>,
    pub source_byte_order_inline_binary: bool,
    pub transfer_syntax: Option<Syntax>,
    pub limits: Limits,
}
struct Budget {
    limits: Limits,
    elements: i64,
    sequence_items: i64,
    binary_bytes: i64,
}
impl Budget {
    fn new(limits: Limits) -> Result<Budget, String> {
        let l = limits;
        if l.max_json_bytes < 0
            || l.max_inline_binary_bytes < 0
            || l.max_total_binary_bytes < 0
            || l.max_sequence_depth < 0
            || l.max_elements < 0
            || l.max_sequence_items < 0
        {
            return Err(format!("{ERR_INVALID_LIMITS}: limits must not be negative"));
        }
        let mut limits = limits;
        if limits.max_sequence_depth == 0 {
            limits.max_sequence_depth = DEFAULT_MAX_SEQUENCE_DEPTH;
        }
        Ok(Budget {
            limits,
            elements: 0,
            sequence_items: 0,
            binary_bytes: 0,
        })
    }
    fn check_json_bytes(&self, size: usize) -> Result<(), String> {
        let max = self.limits.max_json_bytes;
        if max > 0 && size as i64 > max {
            return Err(format!("{ERR_MAX_JSON_BYTES}: got {size}, limit {max}"));
        }
        Ok(())
    }
    fn enter_data_set(&mut self, depth: i64, elements: i64) -> Result<(), String> {
        if depth > self.limits.max_sequence_depth {
            return Err(format!(
                "{ERR_MAX_SEQUENCE_DEPTH}: got {depth}, limit {}",
                self.limits.max_sequence_depth
            ));
        }
        let max = self.limits.max_elements;
        if max > 0 && elements > max.saturating_sub(self.elements) {
            return Err(format!("{ERR_MAX_ELEMENTS}: got more than {max}"));
        }
        self.elements = self.elements.saturating_add(elements);
        Ok(())
    }
    fn add_sequence_items(&mut self, items: i64) -> Result<(), String> {
        let max = self.limits.max_sequence_items;
        if max > 0 && items > max.saturating_sub(self.sequence_items) {
            return Err(format!("{ERR_MAX_SEQUENCE_ITEMS}: got more than {max}"));
        }
        self.sequence_items = self.sequence_items.saturating_add(items);
        Ok(())
    }
    fn parser_fragment_limit(&self) -> i64 {
        if self.limits.max_sequence_items == 0 {
            return 0;
        }
        self.limits
            .max_sequence_items
            .saturating_sub(self.sequence_items)
            .max(1)
    }
    fn add_inline_binary(&mut self, size: i64) -> Result<(), String> {
        let l = self.limits;
        if l.max_inline_binary_bytes > 0 && size > l.max_inline_binary_bytes {
            return Err(format!(
                "{ERR_MAX_INLINE_BINARY_BYTES}: got {size}, limit {}",
                l.max_inline_binary_bytes
            ));
        }
        if l.max_total_binary_bytes > 0
            && size > l.max_total_binary_bytes.saturating_sub(self.binary_bytes)
        {
            return Err(format!(
                "{ERR_MAX_TOTAL_BINARY_BYTES}: got more than {}",
                l.max_total_binary_bytes
            ));
        }
        self.binary_bytes = self.binary_bytes.saturating_add(size);
        Ok(())
    }
}
pub(crate) fn kind_name(v: &Value) -> &'static str {
    match v {
        Value::Nil => "<nil>",
        Value::Raw(_) => "core.RawValue",
        Value::Strings(_) => "core.StringValue",
        Value::Sequence(_) => "core.SequenceValue",
        Value::Fragments { .. } => "core.FragmentSequence",
        Value::BulkData(_) => "core.BulkDataValue",
    }
}
pub(crate) fn hex_tag(tag: Tag) -> String {
    format!("{:04X}{:04X}", tag.group, tag.element)
}
pub fn marshal(obj: Option<&Object>, opts: &mut Options) -> Result<Vec<u8>, Error> {
    marshal_text(obj, opts).map_err(Error::new)
}
pub(crate) fn marshal_text(obj: Option<&Object>, opts: &mut Options) -> Result<Vec<u8>, String> {
    let mut budget = Budget::new(opts.limits)?;
    let big = opts
        .byte_order
        .unwrap_or_else(|| obj.is_some_and(Object::value_big_endian));
    let m = marshal_elements(obj, opts, big, &mut budget, 0)?;
    let mut out = String::new();
    if opts.pretty {
        jsonwire::write_pretty(&m, &mut out, 0);
    } else {
        jsonwire::write_compact(&m, &mut out);
    }
    let limit = opts.limits.max_json_bytes;
    if limit != 0 && limit != i64::MAX && out.len() as i64 > limit {
        return Err(format!("{ERR_MAX_JSON_BYTES}: limit {limit}"));
    }
    budget.check_json_bytes(out.len())?;
    Ok(out.into_bytes())
}
pub fn marshal_compact(obj: &Object) -> Result<Vec<u8>, Error> {
    marshal(
        Some(obj),
        &mut Options {
            omit_group_length: true,
            ..Options::default()
        },
    )
}
pub fn marshal_pretty(obj: &Object) -> Result<Vec<u8>, Error> {
    marshal(Some(obj), &mut Options::defaults())
}
fn marshal_elements(
    obj: Option<&Object>,
    opts: &mut Options,
    big: bool,
    budget: &mut Budget,
    depth: i64,
) -> Result<JVal, String> {
    if opts.max_materialized_value_bytes < 0 {
        return Err(format!(
            "{ERR_INVALID_LIMITS}: MaxMaterializedValueBytes must not be negative"
        ));
    }
    let Some(obj) = obj else {
        return Ok(JVal::Obj(Vec::new()));
    };
    let elements = obj.sorted_elements();
    let mut count = elements.len() as i64;
    if opts.omit_group_length {
        count = count.saturating_sub(
            elements
                .iter()
                .filter(|e| e.tag().is_group_length())
                .count() as i64,
        );
    }
    budget.enter_data_set(depth, count)?;
    let mut out = Vec::with_capacity(elements.len());
    for elem in &elements {
        if opts.omit_group_length && elem.tag().is_group_length() {
            continue;
        }
        let entry = marshal_element(obj, elem, opts, big, budget, depth)?;
        out.push((hex_tag(elem.tag()), entry));
    }
    Ok(JVal::Obj(out))
}
#[derive(Default)]
struct Entry {
    vr: String,
    value: Vec<JVal>,
    inline_binary: String,
    bulk_data_uri: String,
}
impl Entry {
    fn into_json(self) -> JVal {
        let mut fields = vec![("vr".to_string(), JVal::Str(self.vr))];
        if !self.value.is_empty() {
            fields.push(("Value".to_string(), JVal::Arr(self.value)));
        }
        if !self.inline_binary.is_empty() {
            fields.push(("InlineBinary".to_string(), JVal::Str(self.inline_binary)));
        }
        if !self.bulk_data_uri.is_empty() {
            fields.push(("BulkDataURI".to_string(), JVal::Str(self.bulk_data_uri)));
        }
        JVal::Obj(fields)
    }
}
fn read_u16(b: &[u8], big: bool) -> u16 {
    let v = b.first_chunk::<2>().copied().unwrap_or_default();
    if big {
        u16::from_be_bytes(v)
    } else {
        u16::from_le_bytes(v)
    }
}
fn read_u32(b: &[u8], big: bool) -> u32 {
    let v = b.first_chunk::<4>().copied().unwrap_or_default();
    if big {
        u32::from_be_bytes(v)
    } else {
        u32::from_le_bytes(v)
    }
}
fn read_u64(b: &[u8], big: bool) -> u64 {
    let v = b.first_chunk::<8>().copied().unwrap_or_default();
    if big {
        u64::from_be_bytes(v)
    } else {
        u64::from_le_bytes(v)
    }
}
fn put_u16(out: &mut Vec<u8>, v: u16, big: bool) {
    out.extend_from_slice(&if big {
        v.to_be_bytes()
    } else {
        v.to_le_bytes()
    });
}
fn put_u32(out: &mut Vec<u8>, v: u32, big: bool) {
    out.extend_from_slice(&if big {
        v.to_be_bytes()
    } else {
        v.to_le_bytes()
    });
}
fn put_u64(out: &mut Vec<u8>, v: u64, big: bool) {
    out.extend_from_slice(&if big {
        v.to_be_bytes()
    } else {
        v.to_le_bytes()
    });
}
fn unknown_private_creator(elem: &Element) -> Option<String> {
    if elem.vr() != Vr::UN || !dict::is_private_creator(elem.tag()) {
        return None;
    }
    let raw = elem.raw()?;
    let text = std::str::from_utf8(raw).ok()?;
    private_creator_id(text).ok().map(str::to_string)
}
fn inline_byte_order_width(obj: &Object, elem: &Element, source_order: bool, big: bool) -> usize {
    if source_order || !big {
        return 1;
    }
    if matches!(elem.value, Value::Fragments { .. }) {
        return 1;
    }
    let mut bits = 0;
    if elem.vr() == Vr::OW
        && elem.tag() == Tag::PIXEL_DATA
        && let Some(allocated) = obj.get(valueencode::BITS_ALLOCATED_TAG)
    {
        bits = valueencode::bits_allocated(allocated, big);
    }
    valueencode::little_endian_unit_width(elem.tag(), elem.vr(), bits)
}
fn marshals_raw_as_json_value(vr: Vr) -> bool {
    matches!(
        vr,
        Vr::AT | Vr::FL | Vr::FD | Vr::SS | Vr::US | Vr::SL | Vr::UL | Vr::SV | Vr::UV
    )
}
fn fragment_value_field(
    offset_table: &[u8],
    fragments: &[Vec<u8>],
    big: bool,
) -> Result<Vec<u8>, String> {
    if !offset_table.len().is_multiple_of(4) {
        return Err(format!(
            "dicomjson: Basic Offset Table length {} is not a multiple of 4",
            offset_table.len()
        ));
    }
    let mut out = Vec::new();
    let item = |out: &mut Vec<u8>, tag: Tag, len: u32| {
        put_u16(out, tag.group, big);
        put_u16(out, tag.element, big);
        put_u32(out, len, big);
    };
    item(&mut out, Tag::ITEM, offset_table.len() as u32);
    out.extend_from_slice(offset_table);
    for f in fragments {
        let len = f.len().saturating_add(f.len() % 2);
        item(&mut out, Tag::ITEM, len as u32);
        out.extend_from_slice(f);
        if f.len() % 2 == 1 {
            out.push(0);
        }
    }
    item(&mut out, Tag::SEQUENCE_DELIMITATION, 0);
    Ok(out)
}
fn inline_value_field_length(elem: &Element) -> Result<Option<i64>, String> {
    match &elem.value {
        Value::Fragments {
            offset_table,
            fragments,
        } => {
            if offset_table.len() % 4 != 0 {
                return Err(format!(
                    "dicomjson: Basic Offset Table length {} is not a multiple of 4",
                    offset_table.len()
                ));
            }
            let mut total = (offset_table.len() as i64).saturating_add(16);
            for f in fragments {
                let padded = f.len().saturating_add(f.len() % 2) as i64;
                total = total.saturating_add(padded).saturating_add(8);
            }
            Ok(Some(total))
        }
        Value::Nil if elem.header.length_set && elem.header.length != UNDEFINED_LENGTH => {
            Ok(Some(i64::from(elem.header.length)))
        }
        _ => Ok(None),
    }
}
fn materialize_raw_value(elem: &Element, big: bool) -> Result<Vec<u8>, String> {
    match &elem.value {
        Value::Fragments {
            offset_table,
            fragments,
        } => {
            if elem.tag() != Tag::PIXEL_DATA {
                return Err(format!(
                    "dicomjson: marshal {}: FragmentSequence is only valid for Pixel Data",
                    elem.tag()
                ));
            }
            if elem.vr() != Vr::OB && elem.vr() != Vr::OW {
                return Err(format!(
                    "dicomjson: marshal {}: FragmentSequence requires OB or OW VR",
                    elem.tag()
                ));
            }
            fragment_value_field(offset_table, fragments, big)
        }
        Value::Nil => Ok(Vec::new()),
        Value::Strings(values) if values.is_empty() => Ok(Vec::new()),
        Value::Sequence(items) if items.is_empty() => Ok(Vec::new()),
        other => Err(format!(
            "dicomjson: marshal {} VR {}: unsupported value type {}",
            elem.tag(),
            elem.vr(),
            kind_name(other)
        )),
    }
}
fn pixel_data_bulk_bytes(
    elem: &Element,
    big: bool,
    width: usize,
) -> Result<Option<Vec<u8>>, String> {
    match &elem.value {
        Value::Raw(raw) => Ok(Some(valueencode::reverse_fixed_width(raw, width))),
        Value::Fragments {
            offset_table,
            fragments,
        } => fragment_value_field(offset_table, fragments, big).map(Some),
        Value::Nil => Ok(Some(Vec::new())),
        _ => Ok(None),
    }
}
fn strings_to_json(values: &[String]) -> Vec<JVal> {
    if matches!(values, [] | [_]) && values.iter().all(String::is_empty) {
        return Vec::new();
    }
    values
        .iter()
        .map(|v| {
            if v.is_empty() {
                JVal::Null
            } else {
                JVal::Str(v.clone())
            }
        })
        .collect()
}
fn person_names_to_json(values: &[String]) -> Vec<JVal> {
    if matches!(values, [] | [_]) && values.iter().all(String::is_empty) {
        return Vec::new();
    }
    values
        .iter()
        .map(|v| {
            if v.is_empty() {
                return JVal::Null;
            }
            let parts: Vec<&str> = v.splitn(3, '=').collect();
            let mut fields = Vec::new();
            for (i, name) in ["Alphabetic", "Ideographic", "Phonetic"].iter().enumerate() {
                if let Some(p) = parts.get(i)
                    && !p.is_empty()
                {
                    fields.push((name.to_string(), JVal::Str(p.to_string())));
                }
            }
            JVal::Obj(fields)
        })
        .collect()
}
fn marshal_element(
    obj: &Object,
    elem: &Element,
    opts: &mut Options,
    big: bool,
    budget: &mut Budget,
    depth: i64,
) -> Result<JVal, String> {
    let tag = elem.tag();
    let vr = elem.vr();
    let mut entry = Entry {
        vr: vr.to_string(),
        ..Entry::default()
    };
    if let Value::BulkData(uri) = &elem.value {
        entry.bulk_data_uri = uri.clone();
        return Ok(entry.into_json());
    }
    if let Value::Strings(values) = &elem.value {
        valueencode::validate_string(vr, values)
            .map_err(|e| format!("dicomjson: marshal {tag}: {e}"))?;
    }
    if let Some(creator) = unknown_private_creator(elem) {
        entry.vr = "LO".to_string();
        entry.value = vec![JVal::Str(creator)];
        return Ok(entry.into_json());
    }
    let is_sequence = matches!(elem.value, Value::Sequence(_));
    if vr == Vr::PN {
        let values = obj.decode_text_values(elem).map_err(|e| e.to_string())?;
        entry.value = person_names_to_json(&values);
    } else if vr == Vr::SQ || vr == Vr::UN && is_sequence {
        entry.vr = "SQ".to_string();
        let Some(items) = obj.get_sequence(tag) else {
            return Err(format!(
                "dicomjson: marshal {tag} VR SQ: expected SequenceValue, got {}",
                kind_name(&elem.value)
            ));
        };
        budget.add_sequence_items(items.len() as i64)?;
        for item in &items {
            entry.value.push(marshal_elements(
                Some(item),
                opts,
                big,
                budget,
                depth.saturating_add(1),
            )?);
        }
    } else if vr.is_string_like() {
        let values = obj.decode_text_values(elem).map_err(|e| e.to_string())?;
        entry.value = strings_to_json(&values);
    } else {
        if let Value::Fragments { fragments, .. } = &elem.value {
            budget.add_sequence_items(fragments.len() as i64)?;
        }
        let source_order = opts.source_byte_order_inline_binary;
        if tag == Tag::PIXEL_DATA
            && let Some(func) = opts.pixel_data_bulk_data_uri_func.as_mut()
        {
            let width = inline_byte_order_width(obj, elem, source_order, big);
            if let Some(data) = pixel_data_bulk_bytes(elem, big, width)? {
                let uri = func(tag, vr, &data)
                    .map_err(|e| format!("dicomjson: offload Pixel Data: {e}"))?;
                if !uri.is_empty() {
                    entry.bulk_data_uri = uri;
                    return Ok(entry.into_json());
                }
            }
        }
        let mut inline_budgeted = false;
        let raw: Vec<u8> = match elem.raw() {
            Some(raw) => raw.to_vec(),
            None => {
                if opts.bulk_data_uri_func.is_none()
                    && !marshals_raw_as_json_value(vr)
                    && let Some(size) = inline_value_field_length(elem)?
                {
                    budget.add_inline_binary(size)?;
                    inline_budgeted = true;
                }
                materialize_raw_value(elem, big)?
            }
        };
        match vr {
            Vr::AT => {
                if !raw.len().is_multiple_of(4) {
                    return Err(invalid_raw_length(tag, vr, raw.len(), 4));
                }
                entry.value = raw
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| JVal::Str(hex_tag(Tag::new(read_u16(c, big), read_u16(&c[2..], big)))))
                    .collect();
                return Ok(entry.into_json());
            }
            Vr::FL | Vr::FD | Vr::SS | Vr::US | Vr::SL | Vr::UL | Vr::SV | Vr::UV => {
                entry.value = numeric_values(tag, vr, &raw, big)?;
                return Ok(entry.into_json());
            }
            _ => {}
        }
        if raw.is_empty() {
            return Ok(entry.into_json());
        }
        if opts.bulk_data_uri_func.is_none() && !inline_budgeted {
            budget.add_inline_binary(raw.len() as i64)?;
            inline_budgeted = true;
        }
        let width = inline_byte_order_width(obj, elem, source_order, big);
        let raw = valueencode::reverse_fixed_width(&raw, width);
        if let Some(func) = opts.bulk_data_uri_func.as_mut() {
            let uri = func(tag, vr, &raw);
            if !uri.is_empty() {
                entry.bulk_data_uri = uri;
                return Ok(entry.into_json());
            }
        }
        if !inline_budgeted {
            budget.add_inline_binary(raw.len() as i64)?;
        }
        entry.inline_binary = sizedparse::base64_encode(&raw);
    }
    Ok(entry.into_json())
}
fn invalid_raw_length(tag: Tag, vr: Vr, got: usize, width: usize) -> String {
    format!(
        "dicomjson: cannot marshal tag {tag} VR {vr}: raw value length {got} is not a multiple of {width}"
    )
}
fn float_value(v: f64) -> JVal {
    if v.is_nan() {
        JVal::Str("NaN".into())
    } else if v == f64::INFINITY {
        JVal::Str("Infinity".into())
    } else if v == f64::NEG_INFINITY {
        JVal::Str("-Infinity".into())
    } else {
        JVal::F64(v)
    }
}
fn numeric_values(tag: Tag, vr: Vr, raw: &[u8], big: bool) -> Result<Vec<JVal>, String> {
    let width = match vr {
        Vr::SS | Vr::US => 2,
        Vr::FL | Vr::SL | Vr::UL => 4,
        _ => 8,
    };
    if !raw.len().is_multiple_of(width) {
        return Err(invalid_raw_length(tag, vr, raw.len(), width));
    }
    Ok(raw
        .chunks_exact(width)
        .map(|c| match vr {
            Vr::FL => float_value(f64::from(f32::from_bits(read_u32(c, big)))),
            Vr::FD => float_value(f64::from_bits(read_u64(c, big))),
            Vr::SS => JVal::I64(i64::from(read_u16(c, big) as i16)),
            Vr::US => JVal::U64(u64::from(read_u16(c, big))),
            Vr::SL => JVal::I64(i64::from(read_u32(c, big) as i32)),
            Vr::UL => JVal::U64(u64::from(read_u32(c, big))),
            Vr::SV => JVal::I64(read_u64(c, big) as i64),
            _ => JVal::U64(read_u64(c, big)),
        })
        .collect())
}
fn path_error(path: &str, msg: &str) -> String {
    format!("dicomjson: {msg} at {path}")
}
fn join_path(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        suffix.to_string()
    } else {
        format!("{prefix}/{suffix}")
    }
}
fn index_path(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}
fn trim_json_space(raw: &[u8]) -> &[u8] {
    let is = |c: &u8| matches!(c, b' ' | b'\t' | b'\r' | b'\n');
    let start = raw.iter().position(|c| !is(c)).unwrap_or(raw.len());
    let end = raw
        .iter()
        .rposition(|c| !is(c))
        .map_or(start, |e| e.saturating_add(1));
    raw.get(start..end).unwrap_or_default()
}
fn value_type(raw: &[u8]) -> &'static str {
    match trim_json_space(raw).first() {
        None => "empty",
        Some(b'{') => "object",
        Some(b'[') => "array",
        Some(b'"') => "string",
        Some(b't' | b'f') => "boolean",
        Some(b'n') => "null",
        Some(b'-' | b'0'..=b'9') => "number",
        _ => "invalid",
    }
}
fn is_null(raw: &[u8]) -> bool {
    trim_json_space(raw) == b"null"
}
fn value_type_error(path: &str, vr: Vr, raw: &[u8], msg: &str) -> String {
    path_error(
        path,
        &format!("VR {vr} Value received {}: {msg}", value_type(raw)),
    )
}
fn parse_json_tag(tag_str: &str, path: &str) -> Result<Tag, String> {
    let trimmed = trim_unicode_space(tag_str);
    if trimmed.len() != 8 || trimmed.contains(',') {
        return Err(path_error(
            path,
            &format!("invalid tag {}", crate::textfmt::quote(tag_str)),
        ));
    }
    parse_core_tag(trimmed).map_err(|e| path_error(path, &e))
}
pub(crate) fn trim_unicode_space(s: &str) -> &str {
    s.trim_matches(char::is_whitespace)
}
pub(crate) fn parse_core_tag(s: &str) -> Result<Tag, String> {
    let s = trim_unicode_space(s);
    if s.len() == 8 && !s.contains(',') {
        return match sizedparse::parse_uint_base(s, 16, 32) {
            Ok(v) => Ok(Tag::new((v >> 16) as u16, v as u16)),
            Err(e) => Err(format!(
                "dicom: invalid tag {}: {e}",
                crate::textfmt::quote(s)
            )),
        };
    }
    Err(format!("dicom: invalid tag {}", crate::textfmt::quote(s)))
}
fn byte_order_or_default(opts: &UnmarshalOptions) -> bool {
    match opts.byte_order {
        Some(big) => big,
        None => opts.transfer_syntax.is_some_and(|s| s.big_endian),
    }
}
pub fn unmarshal(data: &[u8], opts: &UnmarshalOptions) -> Result<Object, Error> {
    unmarshal_text(data, opts).map_err(Error::new)
}
pub(crate) fn unmarshal_text(data: &[u8], opts: &UnmarshalOptions) -> Result<Object, String> {
    let mut budget = Budget::new(opts.limits)?;
    budget.check_json_bytes(data.len())?;
    let big = byte_order_or_default(opts);
    let elements = unmarshal_data_set(data, opts, big, &mut budget)?;
    let mut obj = Object::from_elements_with_text_options(elements, opts.text_options.clone());
    obj.set_value_big_endian(big);
    Ok(obj)
}
fn decode_err(e: JsonError) -> String {
    format!("dicomjson: decode dataset JSON: {e}")
}
fn decode_dataset_object<'a>(
    dec: &mut jsonwire::Decoder<'a>,
    budget: &Budget,
    path: &str,
    check: &mut dyn FnMut(&[u8]) -> Result<(), String>,
) -> Result<Vec<(String, &'a [u8])>, String> {
    let token = dec.token().map_err(|e| e.to_string())?;
    if token != Token::Delim(b'{') {
        return Err(format!("{ERR_EXPECTED_DATASET_OBJECT} at {path}"));
    }
    let mut dataset: Vec<(String, &'a [u8])> = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    while dec.more() {
        let token = dec.token().map_err(|e| e.to_string())?;
        let Token::Str(key) = token else {
            return Err(format!("{ERR_EXPECTED_DATASET_OBJECT} at {path}"));
        };
        let tag = crate::textfmt::to_upper(trim_unicode_space(&key));
        if tags.contains(&tag) {
            let mut path = path.to_string();
            if tag.len() == 8 && u32::from_str_radix(&tag, 16).is_ok() && !tag.starts_with('+') {
                path = join_path(&path, &tag);
            }
            return Err(format!("{ERR_DUPLICATE_DATASET_TAG} at {path}"));
        }
        let max = budget.limits.max_elements;
        if max > 0 && dataset.len() as i64 >= max.saturating_sub(budget.elements) {
            return Err(format!("{ERR_MAX_ELEMENTS}: got more than {max}"));
        }
        let value = dec.decode_raw().map_err(|e| e.to_string())?;
        check(value)?;
        if let Some(slot) = dataset.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            dataset.push((key, value));
        }
        tags.push(tag);
    }
    dec.token().map_err(|e| e.to_string())?;
    Ok(dataset)
}
fn unmarshal_data_set(
    data: &[u8],
    opts: &UnmarshalOptions,
    big: bool,
    budget: &mut Budget,
) -> Result<Vec<Element>, String> {
    let mut dec = jsonwire::Decoder::new(data);
    let raw = decode_dataset_object(&mut dec, budget, "$", &mut |_| Ok(()))
        .map_err(|e| format!("dicomjson: decode dataset JSON: {e}"))?;
    jsonwire::ensure_eof(&mut dec).map_err(decode_err)?;
    unmarshal_data_set_map(raw, "", opts, big, budget, 0)
}
fn unmarshal_data_set_map(
    mut raw: Vec<(String, &[u8])>,
    path: &str,
    opts: &UnmarshalOptions,
    big: bool,
    budget: &mut Budget,
    depth: i64,
) -> Result<Vec<Element>, String> {
    budget.enter_data_set(depth, raw.len() as i64)?;
    raw.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut elements = Vec::with_capacity(raw.len());
    let mut inline = Vec::new();
    for (key, value) in raw {
        let tag_path = join_path(path, &key);
        let tag = parse_json_tag(&key, &tag_path)?;
        let (elem, from_inline) =
            unmarshal_element(tag, value, &tag_path, opts, big, budget, depth)?;
        if from_inline {
            inline.push(elements.len());
        }
        elements.push(elem);
    }
    if !inline.is_empty() && !opts.source_byte_order_inline_binary && big {
        let bits = elements
            .iter()
            .find(|e| e.tag() == valueencode::BITS_ALLOCATED_TAG)
            .map_or(0, |e| valueencode::bits_allocated(e, big));
        for index in inline {
            let Some(elem) = elements.get_mut(index) else {
                continue;
            };
            let width = valueencode::little_endian_unit_width(elem.tag(), elem.vr(), bits);
            if width > 1
                && let Value::Raw(raw) = &mut elem.value
            {
                valueencode::reverse_units_in_place(raw, width);
            }
        }
    }
    Ok(elements)
}
fn new_element(tag: Tag, vr: Vr, value: Value) -> Element {
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
fn base64_decoded_length(encoded: &str) -> usize {
    let b = encoded.as_bytes();
    let len = b.iter().filter(|&&c| c != b'\r' && c != b'\n').count();
    let mut padding = 0usize;
    for &c in b.iter().rev() {
        match c {
            b'\r' | b'\n' => continue,
            b'=' => padding = padding.saturating_add(1),
            _ => break,
        }
    }
    (len / 4).saturating_mul(3).saturating_sub(padding)
}
pub(crate) fn base64_decode_padded(encoded: &str) -> Option<Vec<u8>> {
    sizedparse::base64_decode(encoded.as_bytes(), false).ok()
}
fn unmarshal_element(
    tag: Tag,
    raw: &[u8],
    path: &str,
    opts: &UnmarshalOptions,
    big: bool,
    budget: &mut Budget,
    depth: i64,
) -> Result<(Element, bool), String> {
    let fields = jsonwire::as_raw_map(raw)
        .map_err(|e| path_error(path, &format!("decode element JSON: {e}")))?;
    for (key, _) in &fields {
        if !matches!(
            key.as_str(),
            "vr" | "Value" | "InlineBinary" | "BulkDataURI"
        ) {
            return Err(path_error(
                &join_path(path, key),
                &format!(
                    "unrecognized data element field {}",
                    crate::textfmt::quote(key)
                ),
            ));
        }
    }
    let field = |name: &str| fields.iter().find(|(k, _)| k == name).map(|(_, v)| *v);
    let vr_path = join_path(path, "vr");
    let Some(vr_raw) = field("vr") else {
        return Err(path_error(&vr_path, "missing VR field"));
    };
    let vr_text = jsonwire::as_string(vr_raw)
        .map_err(|e| path_error(&vr_path, &format!("decode VR: {e}")))?;
    let Some(vr) = Vr::parse(vr_text.as_bytes()) else {
        return Err(path_error(
            &vr_path,
            &format!("dicom: invalid VR {}", crate::textfmt::quote(&vr_text)),
        ));
    };
    let value_raw = field("Value");
    let inline_raw = field("InlineBinary");
    let bulk_raw = field("BulkDataURI");
    match (
        value_raw.is_some(),
        inline_raw.is_some(),
        bulk_raw.is_some(),
    ) {
        (true, true, _) => {
            return Err(path_error(
                path,
                "\"Value\" conflicts with \"InlineBinary\"",
            ));
        }
        (true, _, true) => {
            return Err(path_error(path, "\"Value\" conflicts with \"BulkDataURI\""));
        }
        (_, true, true) => {
            return Err(path_error(
                path,
                "\"InlineBinary\" conflicts with \"BulkDataURI\"",
            ));
        }
        _ => {}
    }
    if let Some(bulk) = bulk_raw {
        let bulk_path = join_path(path, "BulkDataURI");
        if vr == Vr::SQ {
            return Err(path_error(&bulk_path, "BulkDataURI is not valid for SQ"));
        }
        let uri = jsonwire::as_string(bulk)
            .map_err(|e| path_error(&bulk_path, &format!("decode BulkDataURI: {e}")))?;
        return Ok((new_element(tag, vr, Value::BulkData(uri)), false));
    }
    if let Some(inline) = inline_raw {
        let inline_path = join_path(path, "InlineBinary");
        if vr == Vr::SQ {
            return Err(path_error(&inline_path, "InlineBinary is not valid for SQ"));
        }
        let encoded = jsonwire::as_string(inline)
            .map_err(|e| path_error(&inline_path, &format!("decode InlineBinary: {e}")))?;
        let decoded_length = base64_decoded_length(&encoded);
        budget
            .add_inline_binary(decoded_length as i64)
            .map_err(|e| format!("dicomjson: {e} at {inline_path}"))?;
        let Some(data) = base64_decode_padded(&encoded) else {
            return Err(path_error(
                &inline_path,
                "inline binary data is not valid base64",
            ));
        };
        if data.len() != decoded_length {
            return Err(path_error(
                &inline_path,
                &format!(
                    "inline binary decoded length {} differs from expected {decoded_length}",
                    data.len()
                ),
            ));
        }
        let encapsulated = opts.transfer_syntax.is_some_and(|s| s.encapsulated);
        if tag == Tag::PIXEL_DATA
            && !encapsulated
            && (data.starts_with(&[0xfe, 0xff, 0x00, 0xe0])
                || data.starts_with(&[0xff, 0xfe, 0xe0, 0x00]))
        {
            return Err(format!(
                "dicomjson: decode Pixel Data at {inline_path}: {ERR_PIXEL_DATA_ENCODING_MISMATCH}: encapsulated InlineBinary requires an encapsulated transfer syntax"
            ));
        }
        if tag == Tag::PIXEL_DATA
            && let Some(syntax) = opts.transfer_syntax.filter(|s| s.encapsulated)
        {
            let (offset_table, fragments) = parse_encapsulated_pixel_data(&data, syntax, budget)
                .map_err(|e| {
                    format!("dicomjson: decode encapsulated Pixel Data at {inline_path}: {e}")
                })?;
            let mut elem = new_element(
                tag,
                vr,
                Value::Fragments {
                    offset_table,
                    fragments,
                },
            );
            elem.header.length = UNDEFINED_LENGTH;
            elem.header.length_set = true;
            return Ok((elem, false));
        }
        return Ok((new_element(tag, vr, Value::Raw(data)), true));
    }
    let Some(value) = value_raw else {
        return Ok((new_element(tag, vr, empty_value_for_vr(vr)), false));
    };
    let value_path = join_path(path, "Value");
    let v = match vr {
        Vr::SQ => Value::Sequence(unmarshal_sequence(
            value,
            path,
            &value_path,
            opts,
            big,
            budget,
            depth,
        )?),
        Vr::PN => Value::Strings(unmarshal_person_names(value, &value_path, vr)?),
        Vr::DS | Vr::IS => Value::Strings(unmarshal_number_strings(value, &value_path, vr)?),
        Vr::AT => Value::Raw(unmarshal_attribute_tags(value, &value_path, vr, big)?),
        Vr::UN => {
            return Err(value_type_error(
                &value_path,
                vr,
                value,
                "can't parse JSON Value in UN; use InlineBinary or BulkDataURI",
            ));
        }
        Vr::FL
        | Vr::OF
        | Vr::FD
        | Vr::OD
        | Vr::SS
        | Vr::US
        | Vr::OW
        | Vr::SL
        | Vr::UL
        | Vr::OL
        | Vr::SV
        | Vr::UV
        | Vr::OV
        | Vr::OB => Value::Raw(encode_numeric_values(vr, value, &value_path, big)?),
        _ => Value::Strings(unmarshal_string_values(value, &value_path, vr)?),
    };
    Ok((new_element(tag, vr, v), false))
}
fn empty_value_for_vr(vr: Vr) -> Value {
    if vr == Vr::SQ {
        Value::Sequence(Vec::new())
    } else if vr.is_string_like() {
        Value::Strings(Vec::new())
    } else {
        Value::Raw(Vec::new())
    }
}
fn decode_value_array<'a>(raw: &'a [u8], path: &str, vr: Vr) -> Result<Vec<&'a [u8]>, String> {
    jsonwire::as_raw_array(raw)
        .map_err(|e| value_type_error(path, vr, raw, &format!("expected JSON array: {e}")))
}
fn decode_required_string_value(raw: &[u8], path: &str, vr: Vr) -> Result<String, String> {
    if value_type(raw) != "string" {
        return Err(value_type_error(path, vr, raw, "expected string"));
    }
    jsonwire::as_string(raw)
        .map_err(|e| value_type_error(path, vr, raw, &format!("expected string: {e}")))
}
fn decode_required_number_or_string(raw: &[u8], path: &str, vr: Vr) -> Result<String, String> {
    let trimmed = trim_json_space(raw);
    if trimmed.is_empty() {
        return Err(value_type_error(path, vr, raw, "empty JSON value"));
    }
    if trimmed.first() == Some(&b'"') {
        return jsonwire::as_string(raw)
            .map_err(|e| path_error(path, &format!("expected string: {e}")));
    }
    if value_type(raw) != "number" {
        return Err(value_type_error(path, vr, raw, "expected number or string"));
    }
    Ok(String::from_utf8_lossy(trimmed).into_owned())
}
fn unmarshal_string_values(raw: &[u8], path: &str, vr: Vr) -> Result<Vec<String>, String> {
    let items = decode_value_array(raw, path, vr)?;
    let delimited = vr.uses_text_value_delimiter();
    if !delimited && items.len() > 1 {
        return Err(path_error(
            path,
            &format!("VR {vr} holds one value, got {}", items.len()),
        ));
    }
    let mut values = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let item_path = index_path(path, i);
        let value = if is_null(item) {
            String::new()
        } else {
            decode_required_string_value(item, &item_path, vr)?
        };
        if delimited && value.contains('\\') {
            return Err(path_error(
                &item_path,
                &format!("VR {vr} value contains the backslash value delimiter"),
            ));
        }
        values.push(value);
    }
    Ok(values)
}
fn unmarshal_number_strings(raw: &[u8], path: &str, vr: Vr) -> Result<Vec<String>, String> {
    let items = decode_value_array(raw, path, vr)?;
    let mut values = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let item_path = index_path(path, i);
        if is_null(item) {
            values.push(String::new());
            continue;
        }
        let token = decode_required_number_or_string(item, &item_path, vr)?;
        if let Err(e) = valueencode::validate_number_string(vr, &token) {
            return Err(value_type_error(&item_path, vr, item, &e));
        }
        values.push(token);
    }
    Ok(values)
}
fn unmarshal_person_names(raw: &[u8], path: &str, vr: Vr) -> Result<Vec<String>, String> {
    let items = decode_value_array(raw, path, vr)?;
    let mut values = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let item_path = index_path(path, i);
        if is_null(item) {
            values.push(String::new());
            continue;
        }
        if value_type(item) != "object" {
            return Err(value_type_error(
                &item_path,
                vr,
                item,
                "expected person name object",
            ));
        }
        let pn = jsonwire::as_string_struct(
            item,
            "PersonNameComponents",
            "PersonNameComponents",
            &["Alphabetic", "Ideographic", "Phonetic"],
        )
        .map_err(|e| value_type_error(&item_path, vr, item, &format!("decode person name: {e}")))?;
        if pn.iter().any(|g| g.contains('\\')) {
            return Err(path_error(
                &item_path,
                "VR PN value contains the backslash value delimiter",
            ));
        }
        let [alphabetic, ideographic, phonetic] = pn.as_slice() else {
            return Err(path_error(
                &item_path,
                "decode person name: expected three groups",
            ));
        };
        values.push(join_person_name(alphabetic, ideographic, phonetic));
    }
    Ok(values)
}
pub(crate) fn join_person_name(a: &str, i: &str, p: &str) -> String {
    match (i.is_empty(), p.is_empty()) {
        (true, true) => a.to_string(),
        (false, true) => format!("{a}={i}"),
        (true, false) => format!("{a}=={p}"),
        (false, false) => format!("{a}={i}={p}"),
    }
}
fn unmarshal_sequence(
    raw: &[u8],
    path: &str,
    value_path: &str,
    opts: &UnmarshalOptions,
    big: bool,
    budget: &mut Budget,
    depth: i64,
) -> Result<Vec<DataSet>, String> {
    let items = decode_value_array(raw, value_path, Vr::SQ)?;
    budget.add_sequence_items(items.len() as i64)?;
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let item_path = index_path(path, i);
        let dataset = jsonwire::as_raw_map(item)
            .map_err(|e| path_error(&item_path, &format!("decode sequence item: {e}")))?;
        let elements = unmarshal_data_set_map(
            dataset,
            &item_path,
            opts,
            big,
            budget,
            depth.saturating_add(1),
        )?;
        out.push(DataSet {
            elements,
            item_offset: None,
        });
    }
    Ok(out)
}
fn unmarshal_attribute_tags(raw: &[u8], path: &str, vr: Vr, big: bool) -> Result<Vec<u8>, String> {
    let items = decode_value_array(raw, path, vr)?;
    let mut data = Vec::with_capacity(items.len().saturating_mul(4));
    for (i, item) in items.iter().enumerate() {
        let item_path = index_path(path, i);
        let value = decode_required_string_value(item, &item_path, vr)?;
        let tag = parse_json_tag(&value, &item_path)?;
        put_u16(&mut data, tag.group, big);
        put_u16(&mut data, tag.element, big);
    }
    Ok(data)
}
fn float_token(raw: &[u8], path: &str, vr: Vr, bits32: bool) -> Result<f64, String> {
    let token = decode_required_number_or_string(raw, path, vr)?;
    match token.to_lowercase().as_str() {
        "nan" => return Ok(GO_NAN),
        "inf" | "+inf" | "infinity" | "+infinity" => return Ok(f64::INFINITY),
        "-inf" | "-infinity" => return Ok(f64::NEG_INFINITY),
        _ => {}
    }
    let parsed = if bits32 {
        sizedparse::parse_float32(&token)
    } else {
        crate::numtext::parse_float(&token).map_err(|e| e.to_string())
    };
    parsed.map_err(|e| value_type_error(path, vr, raw, &format!("parse float: {e}")))
}
fn encode_numeric_values(vr: Vr, raw: &[u8], path: &str, big: bool) -> Result<Vec<u8>, String> {
    use sizedparse::{parse_int, parse_uint};
    let items = decode_value_array(raw, path, vr)?;
    let mut data = Vec::with_capacity(items.len().saturating_mul(8));
    let int = |item: &[u8], p: &str, bits: u32| -> Result<i64, String> {
        let token = decode_required_number_or_string(item, p, vr)?;
        parse_int(&token, bits)
            .map_err(|e| value_type_error(p, vr, item, &format!("parse signed integer: {e}")))
    };
    let uint = |item: &[u8], p: &str, bits: u32| -> Result<u64, String> {
        let token = decode_required_number_or_string(item, p, vr)?;
        parse_uint(&token, bits)
            .map_err(|e| value_type_error(p, vr, item, &format!("parse unsigned integer: {e}")))
    };
    for (i, item) in items.iter().enumerate() {
        let p = index_path(path, i);
        match vr {
            Vr::FL | Vr::OF => {
                let v = float_token(item, &p, vr, true)? as f32;
                put_u32(&mut data, v.to_bits(), big);
            }
            Vr::FD | Vr::OD => put_u64(&mut data, float_token(item, &p, vr, false)?.to_bits(), big),
            Vr::SS => put_u16(&mut data, int(item, &p, 16)? as u16, big),
            Vr::US | Vr::OW => put_u16(&mut data, uint(item, &p, 16)? as u16, big),
            Vr::SL => put_u32(&mut data, int(item, &p, 32)? as u32, big),
            Vr::UL | Vr::OL => put_u32(&mut data, uint(item, &p, 32)? as u32, big),
            Vr::SV => put_u64(&mut data, int(item, &p, 64)? as u64, big),
            Vr::UV | Vr::OV => put_u64(&mut data, uint(item, &p, 64)?, big),
            _ => data.push(uint(item, &p, 8)? as u8),
        }
    }
    Ok(data)
}
fn parse_encapsulated_pixel_data(
    data: &[u8],
    syntax: Syntax,
    budget: &mut Budget,
) -> Result<(Vec<u8>, Vec<Vec<u8>>), String> {
    let (offset_table, fragments) = read_encapsulated(data, syntax, budget.parser_fragment_limit())
        .map_err(|(e, frag)| {
            if frag {
                format!("{ERR_MAX_SEQUENCE_ITEMS}: {e}")
            } else {
                format!("decode encapsulated Pixel Data Value Field: {e}")
            }
        })?;
    budget.add_sequence_items(fragments.len() as i64)?;
    Ok((offset_table, fragments))
}
pub(crate) type Encapsulated = (Vec<u8>, Vec<Vec<u8>>);
pub(crate) fn read_encapsulated(
    data: &[u8],
    syntax: Syntax,
    max_fragments: i64,
) -> Result<Encapsulated, (String, bool)> {
    let big = syntax.big_endian;
    let mut encoded = Vec::with_capacity(data.len().saturating_add(12));
    put_u16(&mut encoded, Tag::PIXEL_DATA.group, big);
    put_u16(&mut encoded, Tag::PIXEL_DATA.element, big);
    if syntax.explicit_vr {
        encoded.extend_from_slice(b"OB\0\0");
    }
    put_u32(&mut encoded, UNDEFINED_LENGTH, big);
    encoded.extend_from_slice(data);
    let len = encoded.len() as i64;
    let file = MemFile::new(Rc::from(encoded.into_boxed_slice()));
    let mut reader = Reader::new(
        Box::new(file),
        Some((len, 0)),
        syntax,
        ReaderOptions {
            max_pixel_data_bytes: data.len() as i64,
            max_fragments,
            strict_reserved_bytes: true,
            accept_odd_length: false,
            ..ReaderOptions::default()
        },
    );
    let (ds, err) = reader.read_data_set();
    if let Some(err) = err {
        let frag = err
            .parse_info()
            .is_some_and(|p| p.op == Op::CheckFragmentCount);
        return Err((err.to_string(), frag));
    }
    if !matches!(ds.elements.as_slice(), [only] if only.tag() == Tag::PIXEL_DATA) {
        return Err(("expected exactly one Pixel Data element".to_string(), false));
    }
    match ds.elements.into_iter().next().map(|e| e.value) {
        Some(Value::Fragments {
            offset_table,
            fragments,
        }) => Ok((offset_table, fragments)),
        Some(other) => Err((
            format!("got {}, want core.FragmentSequence", kind_name(&other)),
            false,
        )),
        None => Err(("expected exactly one Pixel Data element".to_string(), false)),
    }
}
pub const ERR_EXPECTED_DATASET_ARRAY: &str = "dicomjson: expected dataset array";
fn stream_source<'a>(data: &'a [u8], limits: &Limits) -> (&'a [u8], Option<JsonError>) {
    let max = limits.max_json_bytes;
    if max > 0 && data.len() as i64 > max {
        let visible = (max as usize).saturating_add(1).min(data.len());
        (
            data.get(..visible).unwrap_or(data),
            Some(JsonError::Other(ERR_MAX_JSON_BYTES.to_string())),
        )
    } else {
        (data, None)
    }
}
fn check_decoder_offset(dec: &jsonwire::Decoder, budget: &Budget) -> Result<(), String> {
    let max = budget.limits.max_json_bytes;
    if max > 0 && dec.input_offset() as i64 > max {
        return Err(ERR_MAX_JSON_BYTES.to_string());
    }
    Ok(())
}
fn object_from(elements: Vec<Element>, opts: &UnmarshalOptions, big: bool) -> Object {
    let mut obj = Object::from_elements_with_text_options(elements, opts.text_options.clone());
    obj.set_value_big_endian(big);
    obj
}
#[doc(hidden)]
pub fn decode_stream(data: &[u8], opts: &UnmarshalOptions) -> Result<Object, String> {
    let mut budget = Budget::new(opts.limits)?;
    let (buf, tail) = stream_source(data, &opts.limits);
    let mut dec = jsonwire::Decoder::with_tail(buf, tail, true);
    let raw = decode_dataset_object(&mut dec, &budget, "$", &mut |_| Ok(()))?;
    check_decoder_offset(&dec, &budget)?;
    let big = byte_order_or_default(opts);
    let elements = unmarshal_data_set_map(raw, "", opts, big, &mut budget, 0)?;
    jsonwire::ensure_eof(&mut dec).map_err(|e| e.to_string())?;
    Ok(object_from(elements, opts, big))
}
fn decode_array<'a>(
    dec: &mut jsonwire::Decoder<'a>,
    budget: &mut Budget,
    check: &mut dyn FnMut(&[u8]) -> Result<(), String>,
    mut handle: impl FnMut(&mut Budget, Vec<(String, &'a [u8])>) -> Result<(), String>,
) -> Result<(), String> {
    let token = dec.token().map_err(|e| e.to_string())?;
    if token != Token::Delim(b'[') {
        return Err(ERR_EXPECTED_DATASET_ARRAY.to_string());
    }
    let mut index = 0;
    while dec.more() {
        let raw = decode_dataset_object(dec, budget, &index_path("", index), check)?;
        check_decoder_offset(dec, budget)?;
        handle(budget, raw)?;
        index = index.saturating_add(1);
    }
    let token = dec.token().map_err(|e| e.to_string())?;
    if token != Token::Delim(b']') {
        return Err("dicomjson: invalid dataset array".to_string());
    }
    jsonwire::ensure_eof(dec).map_err(|e| e.to_string())
}
#[doc(hidden)]
pub fn decode_array_objects(
    data: &[u8],
    opts: &UnmarshalOptions,
    handle: &mut dyn FnMut(Object) -> Result<(), String>,
) -> Result<(), String> {
    let mut budget = Budget::new(opts.limits)?;
    let (buf, tail) = stream_source(data, &opts.limits);
    let mut dec = jsonwire::Decoder::with_tail(buf, tail, true);
    let big = byte_order_or_default(opts);
    decode_array(&mut dec, &mut budget, &mut |_| Ok(()), |budget, raw| {
        let elements = unmarshal_data_set_map(raw, "", opts, big, budget, 0)?;
        handle(object_from(elements, opts, big))
    })
}
pub type Dataset = Vec<(String, jsonwire::RawElement)>;
pub fn decode_raw_datasets(
    data: &[u8],
    limits: Limits,
    handle: &mut dyn FnMut(Dataset) -> Result<(), crate::BoxError>,
) -> Result<(), Error> {
    let mut handle = |d: Dataset| handle(d).map_err(|e| e.to_string());
    decode_raw_datasets_text(data, limits, &mut handle).map_err(Error::new)
}
pub(crate) fn decode_raw_datasets_text(
    data: &[u8],
    limits: Limits,
    handle: &mut dyn FnMut(Dataset) -> Result<(), String>,
) -> Result<(), String> {
    let mut budget = Budget::new(limits)?;
    let (buf, tail) = stream_source(data, &limits);
    let mut dec = jsonwire::Decoder::with_tail(buf, tail, true);
    decode_array(
        &mut dec,
        &mut budget,
        &mut |value| {
            jsonwire::as_element(value)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
        |budget, raw| {
            let mut dataset: Dataset = Vec::with_capacity(raw.len());
            for (key, value) in raw {
                dataset.push((key, jsonwire::as_element(value).map_err(|e| e.to_string())?));
            }
            budget.enter_data_set(0, dataset.len() as i64)?;
            handle(dataset)
        },
    )
}
pub fn element_strings(dataset: &Dataset, tag: &str) -> Vec<String> {
    let key = crate::textfmt::to_upper(trim_unicode_space(tag));
    let Some((_, elem)) = dataset.iter().find(|(k, _)| *k == key) else {
        return Vec::new();
    };
    elem.value
        .iter()
        .map(value_string)
        .filter(|s| !s.is_empty())
        .collect()
}
pub fn element_string(dataset: &Dataset, tag: &str) -> String {
    element_strings(dataset, tag)
        .into_iter()
        .next()
        .unwrap_or_default()
}
fn value_string(v: &jsonwire::JAny) -> String {
    use jsonwire::JAny;
    match v {
        JAny::Str(s) => trim_unicode_space(s).to_string(),
        JAny::Number(n) => match sizedparse::parse_int(n, 64) {
            Ok(i) => i.to_string(),
            Err(_) => trim_unicode_space(n).to_string(),
        },
        JAny::Obj(entries) => {
            let group = |key: &str| {
                entries
                    .iter()
                    .find(|(k, _)| k == key)
                    .and_then(|(_, v)| match v {
                        JAny::Str(s) => Some(trim_unicode_space(s).to_string()),
                        _ => None,
                    })
                    .unwrap_or_default()
            };
            join_person_name(
                &group("Alphabetic"),
                &group("Ideographic"),
                &group("Phonetic"),
            )
        }
        JAny::Null => String::new(),
        other => trim_unicode_space(&jsonwire::sprint_text(other)).to_string(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_compact() {
        let text = br#"{"00100010":{"vr":"PN","Value":[{"Alphabetic":"Doe^J"}]},"00280010":{"vr":"US","Value":[512]},"00189005":{"vr":"FD","Value":[1e-7,"NaN"]}}"#;
        let obj = unmarshal(text, &UnmarshalOptions::default()).unwrap();
        let out = marshal_compact(&obj).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            r#"{"00100010":{"vr":"PN","Value":[{"Alphabetic":"Doe^J"}]},"00189005":{"vr":"FD","Value":[1e-7,"NaN"]},"00280010":{"vr":"US","Value":[512]}}"#
        );
    }
    #[test]
    fn syntax_error_texts() {
        let err = unmarshal(b"{\"00100020\":}", &UnmarshalOptions::default()).unwrap_err();
        assert_eq!(
            err.message(),
            "dicomjson: decode dataset JSON: invalid character '}' looking for beginning of value"
        );
    }
}
