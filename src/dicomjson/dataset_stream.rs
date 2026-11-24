#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
pub mod decode;
use super::jsonwire::{self, JAny, RawElement};
use super::{
    Budget, Dataset, ERR_INVALID_LIMITS, ERR_PIXEL_DATA_ENCODING_MISMATCH, Limits,
    base64_decode_padded, base64_decoded_length, encode_numeric_values, parse_json_tag,
};
use crate::object::{Tag, Vr};
use crate::valueencode;
pub const ERR_MAX_VALUES: &str = "dicomjson: maximum represented value count exceeded";
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DatasetEncodeError<E> {
    Encode(String),
    Validate(E),
}
impl<E> From<String> for DatasetEncodeError<E> {
    fn from(text: String) -> Self {
        DatasetEncodeError::Encode(text)
    }
}
pub(crate) type ValidateElementFn<'a, E> = dyn FnMut(&str, &RawElement, i64) -> Result<(), E> + 'a;
pub struct DatasetOptions<'a, E> {
    pub max_values: i64,
    pub limits: Limits,
    pub validate_element: Option<Box<ValidateElementFn<'a, E>>>,
}
impl<E> Default for DatasetOptions<'_, E> {
    fn default() -> Self {
        DatasetOptions {
            max_values: 0,
            limits: Limits::default(),
            validate_element: None,
        }
    }
}
type Res<T, E> = Result<T, DatasetEncodeError<E>>;
struct Writer<'o, 'a, E> {
    out: Vec<u8>,
    budget: Budget,
    maximum: i64,
    represented: i64,
    opts: &'o mut DatasetOptions<'a, E>,
}
impl<E> Writer<'_, '_, E> {
    fn literal(&mut self, s: &str) -> Res<(), E> {
        if self.maximum > 0 && s.len() as i64 > self.maximum.saturating_sub(self.out.len() as i64) {
            return Err(DatasetEncodeError::Encode(super::ERR_MAX_JSON_BYTES.into()));
        }
        self.out.extend_from_slice(s.as_bytes());
        Ok(())
    }
}
pub(crate) fn marshal_any(v: &JAny, out: &mut String) {
    match v {
        JAny::Null => out.push_str("null"),
        JAny::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JAny::Number(n) => out.push_str(n),
        JAny::Str(s) => jsonwire::append_string(out, s),
        JAny::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                marshal_any(item, out);
            }
            out.push(']');
        }
        JAny::Obj(entries) => {
            out.push('{');
            for (i, (k, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                jsonwire::append_string(out, k);
                out.push(':');
                marshal_any(item, out);
            }
            out.push('}');
        }
    }
}
fn marshal_values(values: &[JAny]) -> String {
    let mut out = String::new();
    marshal_any(&JAny::Arr(values.to_vec()), &mut out);
    out
}
pub fn marshal_dataset(dataset: &Dataset) -> String {
    let mut keys: Vec<&(String, RawElement)> = dataset.iter().collect();
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::from("{");
    for (i, (key, e)) in keys.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        jsonwire::append_string(&mut out, key);
        out.push_str(":{\"vr\":");
        jsonwire::append_string(&mut out, &e.vr);
        if !e.value.is_empty() {
            out.push_str(",\"Value\":");
            out.push_str(&marshal_values(&e.value));
        }
        if !e.inline_binary.is_empty() {
            out.push_str(",\"InlineBinary\":");
            jsonwire::append_string(&mut out, &e.inline_binary);
        }
        if !e.bulk_data_uri.is_empty() {
            out.push_str(",\"BulkDataURI\":");
            jsonwire::append_string(&mut out, &e.bulk_data_uri);
        }
        out.push('}');
    }
    out.push('}');
    out
}
pub(crate) fn encode_dataset<E>(
    dataset: Option<&Dataset>,
    opts: &mut DatasetOptions<'_, E>,
) -> Res<Vec<u8>, E> {
    if opts.max_values < 0 {
        return Err(DatasetEncodeError::Encode(ERR_INVALID_LIMITS.into()));
    }
    let budget = Budget::new(opts.limits)?;
    let maximum = opts.limits.max_json_bytes;
    let empty = Dataset::new();
    let dataset = dataset.unwrap_or(&empty);
    let mut w = Writer {
        out: Vec::new(),
        budget,
        maximum,
        represented: 0,
        opts,
    };
    encode_typed(&mut w, dataset, 0)?;
    Ok(w.out)
}
struct View<'v> {
    element: RawElement,
    fields: Option<&'v [(String, JAny)]>,
    value_null: bool,
}
fn field<'v>(fields: &'v [(String, JAny)], name: &str) -> Option<&'v JAny> {
    fields.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}
fn represented_attribute<E>(value: &JAny, remaining: i64) -> Res<View<'_>, E> {
    let err = |s: &str| Err(DatasetEncodeError::Encode(s.to_string()));
    let JAny::Obj(fields) = value else {
        return err("dicomjson: invalid data element");
    };
    let mut e = RawElement::default();
    let mut value_null = false;
    for (key, v) in fields {
        match key.as_str() {
            "vr" => match v {
                JAny::Str(s) => e.vr = s.clone(),
                _ => return err("dicomjson: invalid VR field"),
            },
            "Value" => match v {
                JAny::Null => value_null = true,
                JAny::Arr(items) => {
                    let _ = remaining;
                    e.value = items.clone();
                }
                _ => return err("dicomjson: invalid Value array"),
            },
            "InlineBinary" => match v {
                JAny::Null => {}
                JAny::Str(s) => e.inline_binary = s.clone(),
                _ => return err("dicomjson: invalid InlineBinary"),
            },
            "BulkDataURI" => match v {
                JAny::Null => {}
                JAny::Str(s) => e.bulk_data_uri = s.clone(),
                _ => return err("dicomjson: invalid BulkDataURI"),
            },
            _ => return err("dicomjson: unrecognized element field"),
        }
    }
    if field(fields, "vr").is_none() {
        return err("dicomjson: missing VR field");
    }
    Ok(View {
        element: e,
        fields: Some(fields),
        value_null,
    })
}
fn encode_typed<E>(w: &mut Writer<'_, '_, E>, dataset: &Dataset, depth: i64) -> Res<(), E> {
    w.budget.enter_data_set(depth, dataset.len() as i64)?;
    let mut keys: Vec<&(String, RawElement)> = dataset.iter().collect();
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    w.literal("{")?;
    for (index, (key, element)) in keys.into_iter().enumerate() {
        let tag = parse_json_tag(key, key)?;
        if w.opts.max_values > 0 && w.represented >= w.opts.max_values {
            return Err(DatasetEncodeError::Encode(ERR_MAX_VALUES.into()));
        }
        w.represented = w.represented.saturating_add(1);
        let view = View {
            element: element.clone(),
            fields: None,
            value_null: false,
        };
        encode_attribute(w, index, key, tag, view, depth)?;
    }
    w.literal("}")
}
fn encode_generic<E>(
    w: &mut Writer<'_, '_, E>,
    item: Option<&[(String, JAny)]>,
    depth: i64,
) -> Res<(), E> {
    let size = item.map_or(0, <[_]>::len) as i64;
    w.budget.enter_data_set(depth, size)?;
    let Some(entries) = item else {
        return w.literal("null");
    };
    w.literal("{")?;
    for (index, (key, value)) in entries.iter().enumerate() {
        let tag = parse_json_tag(key, key)?;
        if w.opts.max_values > 0 && w.represented >= w.opts.max_values {
            return Err(DatasetEncodeError::Encode(ERR_MAX_VALUES.into()));
        }
        w.represented = w.represented.saturating_add(1);
        let mut remaining = 0;
        if w.opts.max_values > 0 {
            remaining = w.opts.max_values.saturating_sub(w.represented);
            if remaining == 0 {
                remaining = -1;
            }
        }
        let view = represented_attribute(value, remaining)?;
        encode_attribute(w, index, key, tag, view, depth)?;
    }
    w.literal("}")
}
fn encode_attribute<E>(
    w: &mut Writer<'_, '_, E>,
    index: usize,
    key: &str,
    tag: Tag,
    view: View<'_>,
    depth: i64,
) -> Res<(), E> {
    if let Some(validate) = w.opts.validate_element.as_mut() {
        validate(key, &view.element, depth).map_err(DatasetEncodeError::Validate)?;
    }
    if index > 0 {
        w.literal(",")?;
    }
    let mut quoted = String::new();
    jsonwire::append_string(&mut quoted, key);
    w.literal(&quoted)?;
    w.literal(":")?;
    encode_element(w, tag, &view, depth)
}
fn encode_element<E>(
    w: &mut Writer<'_, '_, E>,
    tag: Tag,
    view: &View<'_>,
    depth: i64,
) -> Res<(), E> {
    let e = &view.element;
    let encode_err = |s: &str| Err(DatasetEncodeError::Encode(s.to_string()));
    if w.opts.max_values > 0
        && e.value.len() as i64 > w.opts.max_values.saturating_sub(w.represented)
    {
        return encode_err(ERR_MAX_VALUES);
    }
    w.represented = w.represented.saturating_add(e.value.len() as i64);
    let Some(vr) = Vr::parse(e.vr.as_bytes()) else {
        return Err(DatasetEncodeError::Encode(format!(
            "dicom: invalid VR {}",
            crate::textfmt::quote(&e.vr)
        )));
    };
    let (has_value, has_inline, has_bulk) = match view.fields {
        Some(fields) => (
            field(fields, "Value").is_some(),
            field(fields, "InlineBinary").is_some(),
            field(fields, "BulkDataURI").is_some(),
        ),
        None => (
            !e.value.is_empty(),
            !e.inline_binary.is_empty(),
            !e.bulk_data_uri.is_empty(),
        ),
    };
    if [has_value, has_inline, has_bulk]
        .iter()
        .filter(|b| **b)
        .count()
        > 1
    {
        return encode_err("dicomjson: conflicting element representations");
    }
    if vr == Vr::SQ && (has_inline || has_bulk) {
        return encode_err("dicomjson: binary representation is invalid for SQ");
    }
    if has_inline {
        w.budget
            .add_inline_binary(base64_decoded_length(&e.inline_binary) as i64)?;
        let Some(decoded) = base64_decode_padded(&e.inline_binary) else {
            return encode_err("dicomjson: invalid InlineBinary: illegal base64 data");
        };
        if tag == Tag::PIXEL_DATA
            && matches!(
                decoded.first_chunk::<4>(),
                Some([0xfe, 0xff, 0x00, 0xe0] | [0xff, 0xfe, 0xe0, 0x00])
            )
        {
            return encode_err(ERR_PIXEL_DATA_ENCODING_MISMATCH);
        }
    }
    w.literal("{")?;
    let mut fields: Vec<&str> = vec!["vr"];
    if has_value {
        fields.push("Value");
    }
    if has_inline {
        fields.push("InlineBinary");
    }
    if has_bulk {
        fields.push("BulkDataURI");
    }
    if view.fields.is_some() {
        fields.sort_unstable();
    }
    for (i, name) in fields.into_iter().enumerate() {
        if i > 0 {
            w.literal(",")?;
        }
        let mut quoted = String::new();
        jsonwire::append_string(&mut quoted, name);
        w.literal(&quoted)?;
        w.literal(":")?;
        if name == "Value" {
            if view.fields.is_some() && view.value_null {
                w.literal("null")?;
                continue;
            }
            if vr == Vr::SQ {
                w.budget.add_sequence_items(e.value.len() as i64)?;
                w.literal("[")?;
                for (j, item) in e.value.iter().enumerate() {
                    if j > 0 {
                        w.literal(",")?;
                    }
                    match item {
                        JAny::Obj(entries) => {
                            encode_generic(w, Some(entries), depth.saturating_add(1))?
                        }
                        JAny::Null => encode_generic(w, None, depth.saturating_add(1))?,
                        _ => return encode_err("dicomjson: invalid sequence item"),
                    }
                }
                w.literal("]")?;
            } else {
                validate_represented_values(vr, &e.value)?;
                w.literal(&marshal_values(&e.value))?;
            }
        } else {
            let mut out = String::new();
            match view.fields {
                Some(fields) => marshal_any(field(fields, name).unwrap_or(&JAny::Null), &mut out),
                None => jsonwire::append_string(
                    &mut out,
                    match name {
                        "InlineBinary" => &e.inline_binary,
                        "BulkDataURI" => &e.bulk_data_uri,
                        _ => &e.vr,
                    },
                ),
            }
            w.literal(&out)?;
        }
    }
    w.literal("}")
}
fn validate_represented_values<E>(vr: Vr, values: &[JAny]) -> Res<(), E> {
    let err = |s: &str| Err(DatasetEncodeError::Encode(s.to_string()));
    match vr {
        Vr::SQ => return err("dicomjson: unexpected SQ scalar"),
        Vr::UN => return err("dicomjson: UN requires binary representation"),
        Vr::PN => {
            for value in values {
                let text = match value {
                    JAny::Null => continue,
                    JAny::Obj(entries) => {
                        let mut text = String::new();
                        for (key, component) in entries {
                            if !matches!(key.as_str(), "Alphabetic" | "Ideographic" | "Phonetic") {
                                return err("dicomjson: invalid person name field");
                            }
                            let JAny::Str(s) = component else {
                                return err("dicomjson: invalid person name component");
                            };
                            text.push_str(s);
                        }
                        text
                    }
                    _ => return err("dicomjson: PN requires object values"),
                };
                if text.contains('\\') {
                    return err("dicomjson: PN value delimiter");
                }
            }
        }
        Vr::DS | Vr::IS => {
            for value in values {
                let token = match value {
                    JAny::Null => continue,
                    JAny::Str(s) | JAny::Number(s) => s.clone(),
                    other => {
                        let mut out = String::new();
                        marshal_any(other, &mut out);
                        out
                    }
                };
                valueencode::validate_number_string(vr, &token)?;
            }
        }
        Vr::AT => {
            for value in values {
                let JAny::Str(text) = value else {
                    return err("dicomjson: AT requires tag strings");
                };
                parse_json_tag(text, "")?;
            }
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
        | Vr::OB => {
            encode_numeric_values(vr, marshal_values(values).as_bytes(), "", false)?;
        }
        _ => {
            if !vr.is_string_like() {
                return err("dicomjson: unsupported VR");
            }
            if !vr.uses_text_value_delimiter() && values.len() > 1 {
                return err("dicomjson: single-valued VR");
            }
            for value in values {
                match value {
                    JAny::Null => {}
                    JAny::Str(text) => {
                        if vr.uses_text_value_delimiter() && text.contains('\\') {
                            return err("dicomjson: text value delimiter");
                        }
                    }
                    _ => return err("dicomjson: text VR requires strings"),
                }
            }
        }
    }
    Ok(())
}
