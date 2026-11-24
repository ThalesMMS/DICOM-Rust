#![forbid(unsafe_code)]
use super::dict;
use super::error::Error;
use super::parser::BoundaryRecovery;
use super::text::{
    PersonName, TextOptions, clean_text_value, normalize_uid, split_text_multiplicity,
    trim_text_value_bytes,
};
use super::types::{DataSet, Element, Length, Tag, UNDEFINED_LENGTH, Value, Vr};
use crate::charset::SpecificCharacterSet;
use crate::dcmtime;
use crate::numtext;
pub const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
#[derive(Clone, Debug, PartialEq)]
struct Inherited {
    charset: SpecificCharacterSet,
    err: Option<Error>,
    declared: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Object {
    elements: Vec<Element>,
    pub(super) boundary_recoveries: Vec<BoundaryRecovery>,
    pub(super) boundary_recoveries_truncated: bool,
    text: TextOptions,
    big_endian: bool,
    item_offset: Option<i64>,
    inherited: Option<Inherited>,
    pub(super) parsed_pixel_data: bool,
    private_duplicate_slots: Vec<Tag>,
    pub(super) value_provider: Option<std::sync::Arc<super::deferred::ValueProvider>>,
    pub(super) ambiguous_deferred: Vec<Tag>,
    pub(super) pixel_data_in_destination_order: bool,
    pub(super) discarded: Vec<Tag>,
    pub(super) private_diagnostics: Vec<super::parser::PrivateDiagnostic>,
    pub(super) private_diagnostics_truncated: bool,
}
#[doc(hidden)]
pub fn element_length(e: &Element) -> Length {
    if e.header.length_set {
        return e.header.length;
    }
    match &e.value {
        Value::Raw(raw) => raw.len() as Length,
        Value::Strings(values) => strings_encoded_length(values),
        _ => UNDEFINED_LENGTH,
    }
}
pub(crate) fn strings_encoded_length(values: &[String]) -> Length {
    let mut total: usize = values.iter().map(String::len).sum();
    if !values.is_empty() {
        total += values.len() - 1;
    }
    if total % 2 == 1 {
        total += 1;
    }
    total as Length
}
fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Nil => "<nil>",
        Value::Raw(_) => "core.RawValue",
        Value::Strings(_) => "core.StringValue",
        Value::Sequence(_) => "core.SequenceValue",
        Value::Fragments { .. } => "core.FragmentSequence",
        Value::BulkData(_) => "core.BulkDataValue",
    }
}
impl Object {
    pub fn new() -> Object {
        Object::default()
    }
    pub fn from_elements(elements: Vec<Element>) -> Object {
        let mut obj = Object::default();
        let mut creators = std::collections::HashSet::new();
        let mut duplicates = Vec::new();
        for element in elements {
            let tag = element.tag();
            if dict::is_private_creator(tag) && !creators.insert(tag) && !duplicates.contains(&tag)
            {
                duplicates.push(tag);
            }
            obj.put(element);
        }
        obj.private_duplicate_slots = duplicates;
        obj
    }
    pub(crate) fn private_duplicate_slots(&self) -> &[Tag] {
        &self.private_duplicate_slots
    }
    pub fn from_data_set(ds: DataSet) -> Object {
        let mut obj = Object::from_elements(ds.elements);
        obj.item_offset = ds.item_offset;
        obj
    }
    pub fn from_elements_with_text_options(elements: Vec<Element>, text: TextOptions) -> Object {
        let mut obj = Object::from_elements(elements);
        obj.text = text;
        obj
    }
    pub fn to_data_set(&self) -> DataSet {
        DataSet {
            elements: self.elements.clone(),
            item_offset: self.item_offset,
        }
    }
    pub fn elements(&self) -> &[Element] {
        &self.elements
    }
    pub fn sorted_elements(&self) -> Vec<Element> {
        let mut out = self.elements.clone();
        out.sort_by_key(|e| e.tag());
        out
    }
    pub fn len(&self) -> usize {
        self.elements.len()
    }
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
    pub fn get(&self, tag: Tag) -> Option<&Element> {
        self.elements.iter().find(|e| e.tag() == tag)
    }
    pub fn has(&self, tag: Tag) -> bool {
        self.get(tag).is_some()
    }
    pub fn has_duplicate_parsed_tag(&self, tag: Tag) -> bool {
        self.ambiguous_deferred.contains(&tag)
    }
    pub fn must_get(&self, tag: Tag) -> Result<&Element, Error> {
        self.get(tag)
            .ok_or_else(|| Error::msg(format!("dicom: missing element {tag}")))
    }
    pub fn put(&mut self, element: Element) {
        if element.tag() == Tag::PIXEL_DATA {
            self.parsed_pixel_data = false;
        }
        if !self.private_duplicate_slots.is_empty() {
            self.private_duplicate_slots.retain(|t| *t != element.tag());
        }
        if !self.ambiguous_deferred.is_empty() {
            self.ambiguous_deferred.retain(|t| *t != element.tag());
        }
        if !self.discarded.is_empty() {
            self.discarded.retain(|t| *t != element.tag());
        }
        if let Some(i) = self.elements.iter().position(|e| e.tag() == element.tag()) {
            self.elements.remove(i);
        }
        self.elements.push(element);
    }
    pub(super) fn set_element_in_place(&mut self, element: Element) -> bool {
        match self.elements.iter_mut().find(|e| e.tag() == element.tag()) {
            Some(slot) => {
                *slot = element;
                true
            }
            None => false,
        }
    }
    pub(super) fn replace_elements_retaining_state(&mut self, elements: Vec<Element>) {
        let kept = |t: &Tag| elements.iter().any(|e| e.tag() == *t);
        self.private_duplicate_slots.retain(kept);
        self.ambiguous_deferred.retain(kept);
        self.discarded.retain(kept);
        self.elements = elements;
    }
    pub fn remove(&mut self, tag: Tag) -> bool {
        if tag == Tag::PIXEL_DATA {
            self.parsed_pixel_data = false;
        }
        match self.elements.iter().position(|e| e.tag() == tag) {
            Some(i) => {
                self.elements.remove(i);
                self.ambiguous_deferred.retain(|t| *t != tag);
                self.discarded.retain(|t| *t != tag);
                true
            }
            None => false,
        }
    }
    pub fn boundary_recoveries(&self) -> (&[BoundaryRecovery], bool) {
        (
            &self.boundary_recoveries,
            self.boundary_recoveries_truncated,
        )
    }
    pub fn value_big_endian(&self) -> bool {
        self.big_endian
    }
    pub fn set_value_big_endian(&mut self, big_endian: bool) {
        self.big_endian = big_endian;
    }
    pub fn item_offset(&self) -> Option<i64> {
        self.item_offset
    }
    pub fn text_options(&self) -> &TextOptions {
        &self.text
    }
    pub fn set_text_options(&mut self, opts: TextOptions) {
        self.text = opts;
    }
    pub fn get_raw(&self, tag: Tag) -> Option<Vec<u8>> {
        self.get(tag)?.raw().map(<[u8]>::to_vec)
    }
    pub fn get_string(&self, tag: Tag) -> Option<String> {
        self.lookup_string(tag).ok()
    }
    pub fn get_strings(&self, tag: Tag) -> Option<Vec<String>> {
        self.lookup_strings(tag).ok()
    }
    pub fn lookup_string(&self, tag: Tag) -> Result<String, Error> {
        let elem = self.must_get(tag)?;
        let values = self.decode_text_values(elem)?;
        Ok(values.into_iter().next().unwrap_or_default())
    }
    pub fn lookup_strings(&self, tag: Tag) -> Result<Vec<String>, Error> {
        let elem = self.must_get(tag)?;
        self.decode_text_values(elem)
    }
    pub(super) fn inherited_charset(&self) -> Option<(SpecificCharacterSet, Option<Error>, bool)> {
        self.inherited
            .as_ref()
            .map(|i| (i.charset.clone(), i.err.clone(), i.declared))
    }
    #[doc(hidden)]
    pub fn character_set_state(&self) -> (SpecificCharacterSet, Option<Error>) {
        let (mut charset, mut err) = match &self.inherited {
            Some(i) => (i.charset.clone(), i.err.clone()),
            None => (SpecificCharacterSet::default(), None),
        };
        if let Some(elem) = self.get(TAG_SPECIFIC_CHARACTER_SET) {
            match decode_text_values_with_default(elem) {
                Err(e) => err = Some(e),
                Ok(values) => match SpecificCharacterSet::parse(&values) {
                    Ok(cs) => {
                        charset = cs;
                        err = None;
                    }
                    Err(e) => {
                        charset = SpecificCharacterSet::default();
                        err = Some(Error::msg(e.to_string()));
                    }
                },
            }
        }
        (charset, err)
    }
    pub fn character_set(&self) -> Result<SpecificCharacterSet, Error> {
        match self.character_set_state() {
            (cs, None) => Ok(cs),
            (_, Some(err)) => Err(err),
        }
    }
    fn character_set_declared(&self) -> bool {
        if let Some(elem) = self.get(TAG_SPECIFIC_CHARACTER_SET) {
            return match decode_text_values_with_default(elem) {
                Err(_) => true,
                Ok(values) => values.iter().any(|v| !v.trim().is_empty()),
            };
        }
        self.inherited.as_ref().is_some_and(|i| i.declared)
    }
    fn charset_fallback_authorized(&self) -> bool {
        use super::text::CharsetFallbackCases as C;
        if self.character_set_declared() {
            self.text.charset_fallback.contains(C::UNDECODABLE)
        } else {
            self.text.charset_fallback.contains(C::UNDECLARED)
        }
    }
    fn character_set_for_vr(&self, vr: Vr) -> Result<SpecificCharacterSet, Error> {
        if !vr.uses_specific_character_set() {
            return Ok(SpecificCharacterSet::default());
        }
        match self.character_set_state() {
            (cs, None) => Ok(cs),
            (_, Some(err)) => {
                if self.text.allow_unsupported_charset_fallback {
                    Ok(self.text.fallback_character_set.clone())
                } else {
                    Err(err)
                }
            }
        }
    }
    pub fn decode_text_values(&self, elem: &Element) -> Result<Vec<String>, Error> {
        let vr = elem.vr();
        if !vr.is_string_like() {
            return Err(not_textual(elem));
        }
        match &elem.value {
            Value::Raw(raw) => {
                if dict::is_private_creator(elem.tag()) {
                    return decode_raw_with_charset(vr, raw, &SpecificCharacterSet::default());
                }
                let charset = self.character_set_for_vr(vr)?;
                let values = decode_raw_with_charset(vr, raw, &charset);
                if values.is_err()
                    && vr.uses_specific_character_set()
                    && self.charset_fallback_authorized()
                    && let Ok(fallback) =
                        decode_raw_with_charset(vr, raw, &self.text.fallback_character_set)
                {
                    return Ok(fallback);
                }
                values
            }
            Value::Strings(values) => Ok(trimmed_strings(vr, values)),
            other => Err(unsupported_text(elem.tag(), other)),
        }
    }
    pub fn get_person_names(&self, tag: Tag) -> Option<Vec<PersonName>> {
        let elem = self.get(tag)?;
        if elem.vr() != Vr::PN {
            return None;
        }
        let values = self.get_strings(tag)?;
        Some(values.iter().map(|v| PersonName::parse(v)).collect())
    }
    pub fn get_person_name(&self, tag: Tag) -> Option<PersonName> {
        self.get_person_names(tag)?.into_iter().next()
    }
    pub fn get_uids(&self, tag: Tag) -> Option<Vec<String>> {
        let elem = self.get(tag)?;
        if elem.vr() != Vr::UI {
            return None;
        }
        let values = self.decode_text_values(elem).ok()?;
        let uids: Vec<String> = values
            .iter()
            .map(|v| normalize_uid(v))
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect();
        if uids.is_empty() { None } else { Some(uids) }
    }
    pub fn get_uid(&self, tag: Tag) -> Option<String> {
        self.get_uids(tag)?.into_iter().next()
    }
    fn numeric_strings(&self, tag: Tag, vr: Vr) -> Result<Vec<String>, Error> {
        let elem = self.must_get(tag)?;
        if elem.vr() != vr {
            return Err(Error::msg(format!(
                "dicom: element {tag} has VR {}, want {vr}",
                elem.vr()
            )));
        }
        let values = self.lookup_strings(tag)?;
        Ok(values.iter().map(|v| clean_text_value(vr, v)).collect())
    }
    pub fn get_ints(&self, tag: Tag) -> Result<Vec<i64>, Error> {
        if let Some(elem) = self.get(tag)
            && elem.vr() != Vr::IS
            && is_binary_integer_vr(elem.vr())
        {
            return binary_ints(elem, self.big_endian);
        }
        let values = self.numeric_strings(tag, Vr::IS)?;
        let mut out = Vec::with_capacity(values.len());
        for (i, value) in values.iter().enumerate() {
            match numtext::parse_int(value) {
                Ok(n) => out.push(n),
                Err(e) => {
                    return Err(Error::msg(format!(
                        "dicom: parse IS value {i} for {tag}: {e}"
                    )));
                }
            }
        }
        Ok(out)
    }
    pub fn get_int(&self, tag: Tag) -> Result<i64, Error> {
        let values = self.get_ints(tag)?;
        values
            .first()
            .copied()
            .ok_or_else(|| Error::msg(format!("dicom: element {tag} has no values")))
    }
    pub fn get_floats(&self, tag: Tag) -> Result<Vec<f64>, Error> {
        if let Some(elem) = self.get(tag)
            && matches!(elem.vr(), Vr::FL | Vr::FD)
        {
            return binary_floats(elem, self.big_endian);
        }
        let values = self.numeric_strings(tag, Vr::DS)?;
        let mut out = Vec::with_capacity(values.len());
        for (i, value) in values.iter().enumerate() {
            match numtext::parse_float(value) {
                Ok(n) => out.push(n),
                Err(e) => {
                    return Err(Error::msg(format!(
                        "dicom: parse DS value {i} for {tag}: {e}"
                    )));
                }
            }
        }
        Ok(out)
    }
    pub fn get_float(&self, tag: Tag) -> Result<f64, Error> {
        let values = self.get_floats(tag)?;
        values
            .first()
            .copied()
            .ok_or_else(|| Error::msg(format!("dicom: element {tag} has no values")))
    }
    fn lookup_temporal_string(&self, tag: Tag, want: Vr) -> Result<String, Error> {
        let elem = self.must_get(tag)?;
        if elem.vr() != want {
            return Err(Error::msg(format!(
                "dicom: element {tag} has VR {}, want {want}",
                elem.vr()
            )));
        }
        self.lookup_string(tag)
    }
    pub fn lookup_date(&self, tag: Tag) -> Result<dcmtime::Date, Error> {
        let value = self.lookup_temporal_string(tag, Vr::DA)?;
        dcmtime::parse_date(&value).map_err(|e| Error::msg(e.to_string()))
    }
    pub fn lookup_time(&self, tag: Tag) -> Result<dcmtime::Time, Error> {
        let value = self.lookup_temporal_string(tag, Vr::TM)?;
        dcmtime::parse_time(&value).map_err(|e| Error::msg(e.to_string()))
    }
    pub fn lookup_date_time(&self, tag: Tag) -> Result<dcmtime::Datetime, Error> {
        let value = self.lookup_temporal_string(tag, Vr::DT)?;
        dcmtime::parse_datetime(&value).map_err(|e| Error::msg(e.to_string()))
    }
    pub fn get_sequence(&self, tag: Tag) -> Option<Vec<Object>> {
        let Value::Sequence(items) = &self.get(tag)?.value else {
            return None;
        };
        let parent = self.inherited_state();
        Some(
            items
                .iter()
                .map(|item| self.new_sequence_item(item, &parent))
                .collect(),
        )
    }
    pub(crate) fn sequence_item_at(&self, tag: Tag, index: i64) -> Option<Object> {
        if index < 0 {
            return None;
        }
        let Value::Sequence(items) = &self.get(tag)?.value else {
            return None;
        };
        let item = items.get(index as usize)?;
        Some(self.new_sequence_item(item, &self.inherited_state()))
    }
    fn inherited_state(&self) -> Inherited {
        let (charset, err) = self.character_set_state();
        Inherited {
            charset,
            err,
            declared: self.character_set_declared(),
        }
    }
    fn new_sequence_item(&self, data: &DataSet, parent: &Inherited) -> Object {
        let mut item = Object::from_data_set(data.clone());
        item.text = self.text.clone();
        item.big_endian = self.big_endian;
        item.inherited = Some(parent.clone());
        item
    }
    pub(super) fn get_default_string(&self, tag: Tag) -> Option<String> {
        self.get_string(tag)
    }
}
fn trimmed_strings(vr: Vr, values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| super::text::trim_text_value(vr, v).to_string())
        .collect()
}
fn not_textual(elem: &Element) -> Error {
    Error::msg(format!(
        "dicom: element {} with VR {} is not textual",
        elem.tag(),
        elem.vr()
    ))
}
fn unsupported_text(tag: Tag, value: &Value) -> Error {
    Error::msg(format!(
        "dicom: unsupported textual value type {} for {tag}",
        value_type_name(value)
    ))
}
pub(super) fn decode_text_values_with_default(elem: &Element) -> Result<Vec<String>, Error> {
    let vr = elem.vr();
    if !vr.is_string_like() {
        return Err(not_textual(elem));
    }
    match &elem.value {
        Value::Raw(raw) => {
            let text = SpecificCharacterSet::default()
                .decode(trim_text_value_bytes(vr, raw))
                .map_err(|e| Error::msg(format!("dicom: decode {vr} value: {e}")))?;
            Ok(split_text_multiplicity(vr, &text))
        }
        Value::Strings(values) => Ok(trimmed_strings(vr, values)),
        other => Err(unsupported_text(elem.tag(), other)),
    }
}
#[doc(hidden)]
pub fn decode_raw_with_charset(
    vr: Vr,
    raw: &[u8],
    charset: &SpecificCharacterSet,
) -> Result<Vec<String>, Error> {
    let bytes = trim_text_value_bytes(vr, raw);
    let text = if vr == Vr::PN {
        charset.decode_person_name(bytes)
    } else if !vr.uses_text_value_delimiter() {
        charset.decode_single_value(bytes)
    } else {
        charset.decode(bytes)
    }
    .map_err(|e| Error::msg(format!("dicom: decode {vr} value: {e}")))?;
    Ok(split_text_multiplicity(vr, &text))
}
#[doc(hidden)]
pub fn is_binary_integer_vr(vr: Vr) -> bool {
    matches!(vr, Vr::US | Vr::SS | Vr::UL | Vr::SL | Vr::UV | Vr::SV)
}
fn read_u16(b: &[u8], big: bool) -> u16 {
    let v = [b[0], b[1]];
    if big {
        u16::from_be_bytes(v)
    } else {
        u16::from_le_bytes(v)
    }
}
fn read_u32(b: &[u8], big: bool) -> u32 {
    let v = [b[0], b[1], b[2], b[3]];
    if big {
        u32::from_be_bytes(v)
    } else {
        u32::from_le_bytes(v)
    }
}
fn read_u64(b: &[u8], big: bool) -> u64 {
    let v = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
    if big {
        u64::from_be_bytes(v)
    } else {
        u64::from_le_bytes(v)
    }
}
#[doc(hidden)]
pub fn binary_ints(elem: &Element, big_endian: bool) -> Result<Vec<i64>, Error> {
    let (tag, vr) = (elem.tag(), elem.vr());
    let Some(raw) = elem.raw() else {
        return Err(Error::msg(format!(
            "dicom: element {tag} with VR {vr} is not raw integer data"
        )));
    };
    let width = match vr {
        Vr::UL | Vr::SL => 4,
        Vr::UV | Vr::SV => 8,
        _ => 2,
    };
    if raw.len() % width != 0 {
        return Err(Error::msg(format!(
            "dicom: element {tag} has {} bytes, want a multiple of {width} for VR {vr}",
            raw.len()
        )));
    }
    let mut out = Vec::with_capacity(raw.len() / width);
    for chunk in raw.chunks_exact(width) {
        out.push(match vr {
            Vr::US => i64::from(read_u16(chunk, big_endian)),
            Vr::SS => i64::from(read_u16(chunk, big_endian) as i16),
            Vr::UL => i64::from(read_u32(chunk, big_endian)),
            Vr::SL => i64::from(read_u32(chunk, big_endian) as i32),
            Vr::UV => {
                let value = read_u64(chunk, big_endian);
                if value > i64::MAX as u64 {
                    return Err(Error::msg(format!(
                        "dicom: element {tag} unsigned value {value} overflows int64"
                    )));
                }
                value as i64
            }
            _ => read_u64(chunk, big_endian) as i64,
        });
    }
    Ok(out)
}
#[doc(hidden)]
pub fn binary_floats(elem: &Element, big_endian: bool) -> Result<Vec<f64>, Error> {
    let (tag, vr) = (elem.tag(), elem.vr());
    let Some(raw) = elem.raw() else {
        return Err(Error::msg(format!(
            "dicom: element {tag} with VR {vr} is not raw floating-point data"
        )));
    };
    let width = if vr == Vr::FD { 8 } else { 4 };
    if raw.len() % width != 0 {
        return Err(Error::msg(format!(
            "dicom: element {tag} has {} bytes, want a multiple of {width} for VR {vr}",
            raw.len()
        )));
    }
    Ok(raw
        .chunks_exact(width)
        .map(|c| {
            if width == 4 {
                f64::from(f32::from_bits(read_u32(c, big_endian)))
            } else {
                f64::from_bits(read_u64(c, big_endian))
            }
        })
        .collect())
}
