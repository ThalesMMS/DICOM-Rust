#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::elem::{calculated_length, non_ascii, raw_element, text_element, value_element};
use super::error::{Error, ErrorKind};
use super::fileset::{FileRecord, FileSet, Limits, normalize_file_set_limits};
use super::keys_sequences::{
    directory_key_condition, directory_sequence, normalize_directory_key_sequences,
    title_modifier_directory_key, validate_directory_key_sequences, verification_directory_key,
};
use super::record_types::{RecordType, standard_file_record_type};
use super::{Reference, TAG_DIRECTORY_RECORD_SEQUENCE, TAG_SPECIFIC_CHARACTER_SET};
use crate::dictionary::reservations::is_private_creator_tag;
use crate::object::error::{Error as ObjectError, Kind as ObjectKind, PathStep};
use crate::object::inflate::{self, Inflated};
use crate::object::parser::source::{ByteRead, MemFile};
use crate::object::parser::{
    Disposition, ReaderOptions, SelectiveReader, SelectiveToken, TokenKind,
};
use crate::object::{
    DataSet, Element, ElementHeader, Object, Part10Header, ReadFileOptions, Tag, UNDEFINED_LENGTH,
    Value, Vr, read_part10_header_mem,
};
use crate::validation::{Mode, ValidationOptions, validate_data_set_with};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
pub(crate) const DEFAULT_MAX_DIRECTORY_RECORDS: i64 = 400_000;
pub(crate) const DEFAULT_MAX_RECORD_KEY_BYTES: i64 = 4 << 20;
pub(crate) const DEFAULT_MAX_TOTAL_RECORD_KEY_BYTES: i64 = 64 << 20;
pub(crate) const DEFAULT_MAX_RECORD_KEY_ELEMENTS: i64 = 10_000;
pub(crate) const DEFAULT_MAX_RECORD_KEY_DEPTH: i64 = 32;
pub(crate) const TAG_INSTANCE_NUMBER: Tag = Tag::new(0x0020, 0x0013);
pub(crate) const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
pub(crate) const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
pub(crate) const TAG_MEDIA_STORAGE_SOP_CLASS_UID: Tag = Tag::new(0x0002, 0x0002);
pub(crate) const TAG_MEDIA_STORAGE_SOP_INSTANCE_UID: Tag = Tag::new(0x0002, 0x0003);
pub(crate) const TAG_TRANSFER_SYNTAX_UID: Tag = Tag::new(0x0002, 0x0010);
pub(crate) const fn key_tag(group: u16, element: u16) -> Tag {
    Tag::new(group, element)
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyRule {
    pub tag: Tag,
    pub kind: &'static str,
}
const fn r(group: u16, element: u16, kind: &'static str) -> KeyRule {
    KeyRule {
        tag: Tag::new(group, element),
        kind,
    }
}
pub(crate) fn directory_key_rules(typ: &str) -> &'static [KeyRule] {
    const IMAGE: &[KeyRule] = &[r(0x0020, 0x0013, "1")];
    const RT_DOSE: &[KeyRule] = &[r(0x0020, 0x0013, "1"), r(0x3004, 0x000A, "1")];
    const RT_STRUCTURE_SET: &[KeyRule] = &[
        r(0x0020, 0x0013, "1"),
        r(0x3006, 0x0002, "1"),
        r(0x3006, 0x0008, "2"),
        r(0x3006, 0x0009, "2"),
    ];
    const RT_PLAN: &[KeyRule] = &[
        r(0x0020, 0x0013, "1"),
        r(0x300A, 0x0002, "1"),
        r(0x300A, 0x0006, "2"),
        r(0x300A, 0x0007, "2"),
    ];
    const RT_TREAT: &[KeyRule] = &[
        r(0x0020, 0x0013, "1"),
        r(0x3008, 0x0250, "2"),
        r(0x3008, 0x0251, "2"),
    ];
    const PRESENTATION: &[KeyRule] = &[
        r(0x0070, 0x0082, "1"),
        r(0x0070, 0x0083, "1"),
        r(0x0008, 0x1115, "1C"),
        r(0x0070, 0x0402, "1C"),
        r(0x0020, 0x0013, "1"),
        r(0x0070, 0x0080, "1"),
        r(0x0070, 0x0081, "2"),
    ];
    const WAVEFORM: &[KeyRule] = &[
        r(0x0020, 0x0013, "1"),
        r(0x0008, 0x0023, "1"),
        r(0x0008, 0x0033, "1"),
    ];
    const SR: &[KeyRule] = &[
        r(0x0020, 0x0013, "1"),
        r(0x0040, 0xA491, "1"),
        r(0x0040, 0xA493, "1"),
        r(0x0008, 0x0023, "1"),
        r(0x0008, 0x0033, "1"),
        r(0x0040, 0xA030, "1C"),
        r(0x0040, 0xA043, "1"),
        r(0x0040, 0xA730, "1C"),
    ];
    const KO: &[KeyRule] = &[
        r(0x0020, 0x0013, "1"),
        r(0x0008, 0x0023, "1"),
        r(0x0008, 0x0033, "1"),
        r(0x0040, 0xA043, "1"),
        r(0x0040, 0xA730, "1C"),
    ];
    const SPECTROSCOPY: &[KeyRule] = &[
        r(0x0008, 0x0008, "1"),
        r(0x0008, 0x0023, "1"),
        r(0x0008, 0x0033, "1"),
        r(0x0020, 0x0013, "1"),
        r(0x0008, 0x9092, "1C"),
        r(0x0028, 0x0008, "1"),
        r(0x0028, 0x0010, "1"),
        r(0x0028, 0x0011, "1"),
        r(0x0028, 0x9001, "1"),
        r(0x0028, 0x9002, "1"),
    ];
    const RAW_DATA: &[KeyRule] = &[
        r(0x0008, 0x0023, "1"),
        r(0x0008, 0x0033, "1"),
        r(0x0020, 0x0013, "2"),
    ];
    const CONTENT: &[KeyRule] = &[
        r(0x0008, 0x0023, "1"),
        r(0x0008, 0x0033, "1"),
        r(0x0020, 0x0013, "1"),
        r(0x0070, 0x0080, "1"),
        r(0x0070, 0x0081, "2"),
    ];
    const HANGING: &[KeyRule] = &[
        r(0x0072, 0x0002, "1"),
        r(0x0072, 0x0004, "1"),
        r(0x0072, 0x0006, "1"),
        r(0x0072, 0x0008, "1"),
        r(0x0072, 0x000A, "1"),
        r(0x0072, 0x000C, "1"),
        r(0x0072, 0x0014, "1"),
        r(0x0072, 0x000E, "2"),
    ];
    const ENCAP: &[KeyRule] = &[
        r(0x0008, 0x0023, "2"),
        r(0x0008, 0x0033, "2"),
        r(0x0020, 0x0013, "1"),
        r(0x0042, 0x0010, "2"),
        r(0x0040, 0xE001, "1C"),
        r(0x0040, 0xA043, "2"),
        r(0x0042, 0x0012, "1"),
    ];
    const EMPTY: &[KeyRule] = &[];
    const PALETTE: &[KeyRule] = &[r(0x0070, 0x0080, "1"), r(0x0070, 0x0081, "2")];
    const IMPLANT: &[KeyRule] = &[
        r(0x0008, 0x0070, "1"),
        r(0x0022, 0x1095, "1"),
        r(0x0068, 0x6210, "1C"),
        r(0x0022, 0x1097, "1"),
    ];
    const IMPLANT_ASSY: &[KeyRule] = &[
        r(0x0076, 0x0001, "1"),
        r(0x0008, 0x0070, "1"),
        r(0x0076, 0x0020, "1"),
    ];
    const IMPLANT_GROUP: &[KeyRule] = &[r(0x0078, 0x0001, "1"), r(0x0078, 0x0020, "1")];
    const SURFACE_SCAN: &[KeyRule] = &[r(0x0008, 0x0023, "1"), r(0x0008, 0x0033, "1")];
    const ASSESSMENT: &[KeyRule] = &[
        r(0x0020, 0x0013, "1"),
        r(0x0008, 0x0012, "1"),
        r(0x0008, 0x0013, "2"),
    ];
    const RADIOTHERAPY: &[KeyRule] = &[
        r(0x0020, 0x0013, "1"),
        r(0x3010, 0x0033, "1C"),
        r(0x3010, 0x0034, "1C"),
        r(0x0070, 0x0081, "2"),
        r(0x0070, 0x0084, "2"),
    ];
    const INVENTORY: &[KeyRule] = &[
        r(0x0008, 0x0023, "1"),
        r(0x0008, 0x0033, "1"),
        r(0x0008, 0x0401, "2"),
        r(0x0008, 0x0403, "1"),
        r(0x0008, 0x0426, "1"),
        r(0x0008, 0x0428, "1"),
        r(0x0008, 0x0400, "2"),
    ];
    const WF_PRESENTATION: &[KeyRule] = &[
        r(0x0070, 0x0082, "1"),
        r(0x0070, 0x0083, "1"),
        r(0x0008, 0x1115, "1C"),
        r(0x0020, 0x0013, "1"),
        r(0x0070, 0x0080, "1"),
        r(0x0070, 0x0081, "2"),
    ];
    match typ {
        "IMAGE" => IMAGE,
        "RT DOSE" => RT_DOSE,
        "RT STRUCTURE SET" => RT_STRUCTURE_SET,
        "RT PLAN" => RT_PLAN,
        "RT TREAT RECORD" => RT_TREAT,
        "PRESENTATION" => PRESENTATION,
        "WAVEFORM" => WAVEFORM,
        "SR DOCUMENT" => SR,
        "KEY OBJECT DOC" => KO,
        "SPECTROSCOPY" => SPECTROSCOPY,
        "RAW DATA" => RAW_DATA,
        "REGISTRATION" | "FIDUCIAL" | "VALUE MAP" | "MEASUREMENT" | "SURFACE" | "TRACT"
        | "ANNOTATION" => CONTENT,
        "HANGING PROTOCOL" => HANGING,
        "ENCAP DOC" => ENCAP,
        "STEREOMETRIC" | "PLAN" => EMPTY,
        "PALETTE" => PALETTE,
        "IMPLANT" => IMPLANT,
        "IMPLANT ASSY" => IMPLANT_ASSY,
        "IMPLANT GROUP" => IMPLANT_GROUP,
        "SURFACE SCAN" => SURFACE_SCAN,
        "ASSESSMENT" => ASSESSMENT,
        "RADIOTHERAPY" => RADIOTHERAPY,
        "INVENTORY" => INVENTORY,
        "WF PRESENTATION" => WF_PRESENTATION,
        _ => EMPTY,
    }
}
#[derive(Clone, Debug)]
pub(crate) struct KeyBudget {
    pub limits: Limits,
    pub bytes: i64,
    pub elements: i64,
}
impl KeyBudget {
    pub fn new(limits: Limits) -> KeyBudget {
        KeyBudget {
            limits,
            bytes: 0,
            elements: 0,
        }
    }
    pub fn charge(&mut self, n: i64, depth: i64) -> Result<(), Error> {
        if n < 0
            || depth > self.limits.max_record_key_depth
            || self.elements >= self.limits.max_record_key_elements
            || n > self.limits.max_record_key_bytes.saturating_sub(self.bytes)
        {
            return Err(ErrorKind::ResourceLimit.into());
        }
        self.bytes = self.bytes.saturating_add(n);
        self.elements = self.elements.saturating_add(1);
        Ok(())
    }
}
pub(crate) fn own_directory_element(
    src: &Object,
    tag: Tag,
    b: &mut KeyBudget,
    depth: i64,
) -> Result<Element, Error> {
    let Some(e) = src.get(tag) else {
        return Err(ErrorKind::MissingRequiredKey.into());
    };
    let vr = e.vr();
    if vr == Vr::SQ {
        let Some(items) = src.get_sequence(tag) else {
            return Err(ErrorKind::InvalidRecord.into());
        };
        b.charge(
            (items.len() as i64).saturating_mul(16).saturating_add(16),
            depth,
        )?;
        let mut owned = Vec::with_capacity(items.len());
        for item in &items {
            owned.push(own_directory_data_set(item, b, depth.saturating_add(1))?);
        }
        return Ok(value_element(tag, Vr::SQ, Value::Sequence(owned)));
    }
    let length = match calculated_length(e) {
        Some(l) if l != UNDEFINED_LENGTH => l,
        _ => return Err(ErrorKind::InvalidRecord.into()),
    };
    b.charge(i64::from(length).saturating_add(16), depth)?;
    if vr.is_string_like() {
        let values = src.get_strings(tag);
        if values.is_none() && length != 0 {
            return Err(ErrorKind::InvalidRecord.into());
        }
        let values = values.unwrap_or_default();
        let utf_bytes: i64 = values
            .iter()
            .map(|v| (v.len() as i64).saturating_add(1))
            .fold(0, i64::saturating_add);
        if utf_bytes > i64::from(length) {
            let extra = utf_bytes.saturating_sub(i64::from(length));
            if extra > b.limits.max_record_key_bytes.saturating_sub(b.bytes) {
                return Err(ErrorKind::ResourceLimit.into());
            }
            b.bytes = b.bytes.saturating_add(extra);
        }
        return Ok(value_element(tag, vr, Value::Strings(values)));
    }
    let Some(raw) = e.raw() else {
        return Err(ErrorKind::InvalidRecord.into());
    };
    if raw.len() != length as usize {
        return Err(ErrorKind::InvalidRecord.into());
    }
    let mut raw = raw.to_vec();
    if src.value_big_endian() {
        let width = match vr {
            Vr::US | Vr::SS | Vr::OW | Vr::AT => 2,
            Vr::UL | Vr::SL | Vr::FL | Vr::OL | Vr::OF => 4,
            Vr::FD | Vr::SV | Vr::UV | Vr::OD | Vr::OV => 8,
            _ => 1,
        };
        if !raw.len().is_multiple_of(width) {
            return Err(ErrorKind::InvalidRecord.into());
        }
        for chunk in raw.chunks_mut(width) {
            chunk.reverse();
        }
    }
    Ok(raw_element(tag, vr, raw))
}
pub(crate) fn own_directory_data_set(
    src: &Object,
    b: &mut KeyBudget,
    depth: i64,
) -> Result<DataSet, Error> {
    if depth > b.limits.max_record_key_depth {
        return Err(ErrorKind::ResourceLimit.into());
    }
    let mut out: Vec<Element> = Vec::new();
    for e in src.elements() {
        if e.tag() == TAG_SPECIFIC_CHARACTER_SET {
            continue;
        }
        out.push(own_directory_element(src, e.tag(), b, depth)?);
    }
    let mut needs_utf8 = src.has(TAG_SPECIFIC_CHARACTER_SET);
    for e in &out {
        if let Value::Strings(values) = &e.value
            && non_ascii(values)
        {
            needs_utf8 = true;
        }
    }
    if needs_utf8 {
        b.charge(26, depth)?;
        out.push(text_element(
            TAG_SPECIFIC_CHARACTER_SET,
            Vr::CS,
            "ISO_IR 192",
        ));
    }
    out.sort_by_key(|e| e.tag());
    Ok(DataSet {
        elements: out,
        item_offset: None,
    })
}
impl FileSet {
    pub(crate) fn read_directory_keys(&self, r: &mut FileRecord) -> Result<(), Error> {
        let limits = self.options.limits;
        self.read_directory_keys_with_limits(r, &limits)
    }
    pub(crate) fn read_directory_keys_with_limits(
        &self,
        r: &mut FileRecord,
        key_limits: &Limits,
    ) -> Result<(), Error> {
        if r.record_type == RecordType::IMAGE {
            r.key_bytes = (r.instance_number.len() as i64).saturating_add(16);
            if r.key_bytes > key_limits.max_record_key_bytes {
                return Err(ErrorKind::ResourceLimit.into());
            }
            r.keys = DataSet {
                elements: vec![text_element(
                    TAG_INSTANCE_NUMBER,
                    Vr::IS,
                    &r.instance_number,
                )],
                item_offset: None,
            };
            return Ok(());
        }
        let reference = Reference {
            file_id: r.file_id.0.clone(),
            typ: String::new(),
        };
        let Ok((handle, _)) = super::open_referenced_file(&self.root, &reference) else {
            return Err(ErrorKind::InvalidRecord.into());
        };
        let Ok(data) = read_handle(handle) else {
            return Err(ErrorKind::InvalidRecord.into());
        };
        let mut max_total = self.options.index_options.limits.max_total_bytes;
        if max_total == 0 {
            max_total = 512 << 20;
        }
        let (header, selected) = match read_selected_directory_metadata(
            data,
            r.record_type.as_str(),
            key_limits,
            max_total,
            false,
        ) {
            Ok(v) => v,
            Err(e) => {
                return Err(if e.is_resource_limit() {
                    ErrorKind::ResourceLimit.into()
                } else {
                    ErrorKind::InvalidRecord.into()
                });
            }
        };
        let class = header
            .meta
            .get_uid(TAG_MEDIA_STORAGE_SOP_CLASS_UID)
            .unwrap_or_default();
        let instance = header
            .meta
            .get_uid(TAG_MEDIA_STORAGE_SOP_INSTANCE_UID)
            .unwrap_or_default();
        if header.transfer_syntax.uid != r.transfer_syntax_uid
            || class != r.sop_class_uid
            || instance != r.sop_instance_uid
        {
            return Err(ErrorKind::SourceChanged.into());
        }
        let mut b = KeyBudget::new(*key_limits);
        let keys = extract_directory_keys(&selected, r, &mut b)?;
        r.keys = keys;
        r.key_bytes = b.bytes;
        Ok(())
    }
}
pub(crate) fn read_handle(mut handle: std::fs::File) -> std::io::Result<Rc<[u8]>> {
    use std::io::Read;
    let mut data = Vec::new();
    handle.read_to_end(&mut data)?;
    Ok(Rc::from(data))
}
pub(crate) fn extract_directory_keys(
    src: &Object,
    r: &FileRecord,
    b: &mut KeyBudget,
) -> Result<DataSet, Error> {
    let typ = r.record_type.as_str();
    if typ == "WF PRESENTATION" && src.has(key_tag(0x0040, 0xB030)) {
        let series = directory_sequence(src, key_tag(0x0008, 0x1115), 1, -1)?;
        for item in &series {
            if !item.has(key_tag(0x0008, 0x114A)) {
                return Err(ErrorKind::MissingRequiredKey.into());
            }
        }
    }
    let mut out: Vec<Element> = Vec::new();
    for rule in directory_key_rules(typ) {
        let (required, include) = directory_key_condition(src, r, rule);
        if !include {
            continue;
        }
        let Some(entry) = crate::object::dict::by_tag(rule.tag) else {
            return Err(ErrorKind::InvalidRecord.into());
        };
        let e = if rule.tag == TAG_INSTANCE_NUMBER {
            if required && r.instance_number.is_empty() {
                return Err(ErrorKind::MissingRequiredKey.into());
            }
            b.charge((r.instance_number.len() as i64).saturating_add(16), 0)?;
            text_element(rule.tag, Vr::IS, &r.instance_number)
        } else if typ == "SR DOCUMENT" && rule.tag == key_tag(0x0040, 0xA030) {
            verification_directory_key(src, b)?
        } else if (typ == "SR DOCUMENT" || typ == "KEY OBJECT DOC")
            && rule.tag == key_tag(0x0040, 0xA730)
        {
            title_modifier_directory_key(src, b)?
        } else if !src.has(rule.tag) {
            if required {
                return Err(ErrorKind::MissingRequiredKey.into());
            }
            b.charge(16, 0)?;
            if entry.vr == Vr::SQ {
                value_element(rule.tag, Vr::SQ, Value::Sequence(Vec::new()))
            } else {
                raw_element(rule.tag, entry.vr, Vec::new())
            }
        } else {
            own_directory_element(src, rule.tag, b, 0)?
        };
        if e.vr() != entry.vr {
            return Err(ErrorKind::InvalidRecord.into());
        }
        if required && !non_empty_directory_key(&e) {
            return Err(ErrorKind::MissingRequiredKey.into());
        }
        out.push(e);
    }
    let out = normalize_directory_key_sequences(
        typ,
        DataSet {
            elements: out,
            item_offset: None,
        },
        b,
    )?;
    let limits = b.limits;
    validate_directory_keys(typ, &out, &r.sop_class_uid, Some(&limits))?;
    Ok(out)
}
pub(crate) fn non_empty_directory_key(e: &Element) -> bool {
    match &e.value {
        Value::Sequence(items) => !items.is_empty(),
        Value::Strings(values) => values.iter().any(|v| !v.is_empty()),
        _ => matches!(calculated_length(e), Some(n) if n != 0 && n != UNDEFINED_LENGTH),
    }
}
pub(crate) fn validate_directory_keys(
    typ: &str,
    keys: &DataSet,
    sop: &str,
    admission: Option<&Limits>,
) -> Result<(), Error> {
    let mut limits = admission.copied().unwrap_or_default();
    normalize_file_set_limits(&mut limits)?;
    if !standard_file_record_type(typ) {
        return Err(ErrorKind::UnsupportedSopClass.into());
    }
    let obj = Object::from_data_set(keys.clone());
    for rule in directory_key_rules(typ) {
        if rule.kind == "1C" || rule.kind == "2C" {
            continue;
        }
        match obj.get(rule.tag) {
            Some(e) if !(rule.kind == "1" && !non_empty_directory_key(e)) => {}
            _ => return Err(ErrorKind::MissingRequiredKey.into()),
        }
    }
    if !strict_valid(
        keys,
        limits
            .max_record_key_depth
            .saturating_mul(2)
            .saturating_add(2),
        limits.max_record_key_elements,
    ) {
        return Err(Error::missing_key());
    }
    validate_directory_key_sequences(&obj, typ, sop)
}
pub(crate) fn strict_valid(ds: &DataSet, max_depth: i64, max_elements: i64) -> bool {
    let (_, err) = validate_data_set_with(
        ds,
        &ValidationOptions {
            mode: Mode::Strict,
            dictionary: true,
            stop_first: true,
            max_findings: 1,
            max_depth,
            max_elements,
            ..Default::default()
        },
    );
    err.is_none()
}
pub(crate) fn directory_keys_equal(a: &DataSet, b: &DataSet) -> bool {
    equal_directory_key_objects(
        &Object::from_data_set(a.clone()),
        &Object::from_data_set(b.clone()),
    )
}
fn equal_directory_key_objects(a: &Object, b: &Object) -> bool {
    let aa = a.sorted_elements();
    let bb = b.sorted_elements();
    if aa.len() != bb.len() {
        return false;
    }
    for (e, other) in aa.iter().zip(bb.iter()) {
        if e.tag() != other.tag() || e.vr() != other.vr() {
            return false;
        }
        if e.vr() == Vr::SQ {
            let a_items = a.get_sequence(e.tag()).unwrap_or_default();
            let b_items = b.get_sequence(e.tag()).unwrap_or_default();
            if a_items.len() != b_items.len() {
                return false;
            }
            for (x, y) in a_items.iter().zip(b_items.iter()) {
                if !equal_directory_key_objects(x, y) {
                    return false;
                }
            }
        } else if e.vr().is_string_like() {
            if a.get_strings(e.tag()).unwrap_or_default()
                != b.get_strings(e.tag()).unwrap_or_default()
            {
                return false;
            }
        } else if e.raw().unwrap_or(&[]) != other.raw().unwrap_or(&[]) {
            return false;
        }
    }
    true
}
#[derive(Debug)]
pub(crate) enum MetaError {
    Dir(Error),
    Object(ObjectError),
}
impl MetaError {
    pub(crate) fn is_resource_limit(&self) -> bool {
        match self {
            MetaError::Dir(e) => e.is(ErrorKind::ResourceLimit),
            MetaError::Object(e) => is_parser_resource_limit(e),
        }
    }
}
impl From<Error> for MetaError {
    fn from(e: Error) -> MetaError {
        MetaError::Dir(e)
    }
}
impl From<ErrorKind> for MetaError {
    fn from(k: ErrorKind) -> MetaError {
        MetaError::Dir(k.into())
    }
}
pub(crate) fn is_parser_resource_limit(err: &ObjectError) -> bool {
    if err.is(ObjectKind::MaxTotalBytes) {
        return true;
    }
    let text = err.to_string();
    [
        "dicom: maximum element value size exceeded",
        "dicom: maximum element count exceeded",
        "dicom: maximum sequence nesting depth exceeded",
        "dicom: maximum fragment count exceeded",
    ]
    .iter()
    .any(|s| text.contains(s))
}
#[derive(Default)]
struct TokenFrame {
    tag: Tag,
    item: bool,
    wanted: bool,
    elements: Vec<Element>,
    items: Vec<DataSet>,
    offset: i64,
    seen: HashSet<Tag>,
}
struct SelectState {
    directory: bool,
    wanted: HashSet<Tag>,
    limits: Limits,
    stack: Vec<TokenFrame>,
    root_seen: HashSet<Tag>,
    selection_err: Option<Error>,
    bytes_used: i64,
    record_bytes: i64,
    selected_count: i64,
    record_count: i64,
}
impl SelectState {
    fn charge(&mut self, n: i64) -> Result<(), Error> {
        if n < 0 {
            return Err(ErrorKind::ResourceLimit.into());
        }
        if self.directory {
            if n > self
                .limits
                .max_total_record_key_bytes
                .saturating_sub(self.bytes_used)
                || n > self
                    .limits
                    .max_record_key_bytes
                    .saturating_sub(self.record_bytes)
            {
                return Err(ErrorKind::ResourceLimit.into());
            }
            self.record_bytes = self.record_bytes.saturating_add(n);
            self.record_count = self.record_count.saturating_add(1);
            if self.record_count > self.limits.max_record_key_elements {
                return Err(ErrorKind::ResourceLimit.into());
            }
        } else if n > self
            .limits
            .max_record_key_bytes
            .saturating_sub(self.bytes_used)
            || self.selected_count >= self.limits.max_record_key_elements
        {
            return Err(ErrorKind::ResourceLimit.into());
        }
        self.bytes_used = self.bytes_used.saturating_add(n);
        self.selected_count = self.selected_count.saturating_add(1);
        Ok(())
    }
    fn include(&self, tag: Tag) -> bool {
        match self.stack.last() {
            Some(frame) => frame.wanted,
            None => self.directory || self.wanted.contains(&tag),
        }
    }
    fn select(&mut self, h: &ElementHeader) -> Result<Disposition, ()> {
        let include = self.include(h.tag);
        if include {
            let seen = match self.stack.last_mut() {
                Some(frame) => &mut frame.seen,
                None => &mut self.root_seen,
            };
            if !seen.insert(h.tag) {
                self.selection_err = Some(ErrorKind::InvalidRecord.into());
                return Err(());
            }
        }
        if h.vr == Vr::SQ || h.vr == Vr::UN && h.is_undefined() {
            return Ok(Disposition::Materialize);
        }
        if !include && is_private_creator_tag(h.tag) {
            if h.is_undefined() || h.vr != Vr::LO || h.length > 64 {
                self.selection_err = Some(ErrorKind::InvalidRecord.into());
                return Err(());
            }
            if let Err(e) = self.charge(i64::from(h.length).saturating_add(16)) {
                self.selection_err = Some(e);
                return Err(());
            }
            return Ok(Disposition::Materialize);
        }
        if !include || h.is_undefined() {
            return Ok(Disposition::Skip);
        }
        if let Err(e) = self.charge(i64::from(h.length).saturating_add(16)) {
            self.selection_err = Some(e);
            return Err(());
        }
        Ok(Disposition::Materialize)
    }
}
pub(crate) fn read_selected_directory_metadata(
    data: Rc<[u8]>,
    typ: &str,
    limits: &Limits,
    max_total: i64,
    directory: bool,
) -> Result<(Part10Header, Object), MetaError> {
    let mut limits = *limits;
    normalize_file_set_limits(&mut limits)?;
    let file = MemFile::new(data);
    let header = read_part10_header_mem(
        &file,
        &ReadFileOptions {
            max_element_bytes: 1 << 20,
            max_total_bytes: 16 << 20,
            max_elements: 256,
            max_sequence_depth: 4,
            ..Default::default()
        },
    )
    .map_err(MetaError::Object)?;
    if directory
        && header.transfer_syntax.uid != crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid
    {
        return Err(ErrorKind::InvalidRecord.into());
    }
    let mut wanted: HashSet<Tag> = [
        TAG_SPECIFIC_CHARACTER_SET,
        TAG_SOP_CLASS_UID,
        TAG_SOP_INSTANCE_UID,
    ]
    .into_iter()
    .collect();
    for rule in directory_key_rules(typ) {
        wanted.insert(rule.tag);
    }
    if typ == "SR DOCUMENT" {
        wanted.insert(key_tag(0x0040, 0xA073));
        wanted.insert(key_tag(0x0008, 0x0201));
    }
    if typ == "WF PRESENTATION" {
        wanted.insert(key_tag(0x0040, 0xB030));
    }
    let (source, sized): (Box<dyn ByteRead>, Option<(i64, i64)>) =
        if header.transfer_syntax.deflated {
            let (decoded, err, _) = inflate::inflate_raw(file.remaining());
            (Box::new(Inflated::new(decoded, err)), None)
        } else {
            (
                Box::new(file.clone()),
                Some((file.len() as i64, file.position() as i64)),
            )
        };
    let state = Rc::new(RefCell::new(SelectState {
        directory,
        wanted,
        limits,
        stack: Vec::new(),
        root_seen: HashSet::new(),
        selection_err: None,
        bytes_used: 0,
        record_bytes: 0,
        selected_count: 0,
        record_count: 0,
    }));
    let select_state = state.clone();
    let mut reader = SelectiveReader::new(
        source,
        sized,
        header.transfer_syntax,
        ReaderOptions {
            base_offset: header.data_set_offset,
            max_total_bytes: max_total,
            max_element_bytes: limits.max_record_key_bytes,
            max_sequence_depth: limits.max_record_key_depth.saturating_add(2),
            max_fragments: 200_000,
            ..Default::default()
        },
        Box::new(move |_: &[PathStep], h: &ElementHeader, _: i64| {
            select_state.borrow_mut().select(h)
        }),
        0,
    )
    .map_err(MetaError::Object)?;
    let max_tokens: i64 = if directory { 16_000_000 } else { 1_000_000 };
    let mut tokens: i64 = 0;
    let mut records: i64 = 0;
    let mut elements: Vec<Element> = Vec::new();
    loop {
        let next = reader.next();
        let mut st = state.borrow_mut();
        if let Some(e) = st.selection_err.take() {
            return Err(e.into());
        }
        let tok = match next {
            Ok(SelectiveToken::Token(tok)) => tok,
            Ok(SelectiveToken::Stop { .. }) => return Err(ErrorKind::InvalidRecord.into()),
            Err(e)
                if e.is(ObjectKind::Eof)
                    && !e.is(ObjectKind::UnexpectedEof)
                    && is_plain_eof(&e) =>
            {
                break;
            }
            Err(e) => return Err(MetaError::Object(e)),
        };
        tokens = tokens.saturating_add(1);
        if tokens > max_tokens {
            return Err(ErrorKind::ResourceLimit.into());
        }
        match tok.kind {
            TokenKind::StartSequence => {
                let tag = tok.tag();
                let include = st.include(tag);
                if include {
                    st.charge(16)?;
                }
                st.stack.push(TokenFrame {
                    tag,
                    wanted: include,
                    ..Default::default()
                });
            }
            TokenKind::StartItem => {
                if st.stack.last().is_none_or(|f| f.item) {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                let include = st.stack.last().is_some_and(|f| f.wanted);
                if directory
                    && matches!(st.stack.as_slice(), [frame] if frame.tag == TAG_DIRECTORY_RECORD_SEQUENCE)
                {
                    st.record_bytes = 0;
                    st.record_count = 0;
                    records = records.saturating_add(1);
                    if records > limits.max_directory_records {
                        return Err(ErrorKind::ResourceLimit.into());
                    }
                }
                if include {
                    st.charge(32)?;
                }
                st.stack.push(TokenFrame {
                    item: true,
                    wanted: include,
                    offset: tok.offset,
                    ..Default::default()
                });
            }
            TokenKind::EndItem => {
                if st.stack.len() < 2 || !st.stack.last().is_some_and(|f| f.item) {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                let frame = st.stack.pop().expect("item frame");
                if frame.wanted {
                    let parent = st.stack.last_mut().expect("sequence frame");
                    parent.items.push(DataSet {
                        elements: frame.elements,
                        item_offset: Some(frame.offset),
                    });
                }
            }
            TokenKind::EndSequence => {
                if st.stack.last().is_none_or(|f| f.item) {
                    return Err(ErrorKind::InvalidRecord.into());
                }
                let frame = st.stack.pop().expect("sequence frame");
                if frame.wanted {
                    let e = value_element(frame.tag, Vr::SQ, Value::Sequence(frame.items));
                    match st.stack.last_mut() {
                        Some(parent) => parent.elements.push(e),
                        None => elements.push(e),
                    }
                }
            }
            TokenKind::Element => {
                let element = tok.element.unwrap_or_else(|| Element {
                    header: tok
                        .header
                        .unwrap_or(ElementHeader::new(Tag::default(), Vr::UN, 0)),
                    value: Value::Nil,
                });
                let include = st.include(element.tag());
                if include {
                    match st.stack.last_mut() {
                        Some(parent) => parent.elements.push(element),
                        None => elements.push(element),
                    }
                }
            }
            TokenKind::StartPixelSequence => return Err(ErrorKind::InvalidRecord.into()),
        }
    }
    let st = state.borrow();
    if !st.stack.is_empty() {
        return Err(ErrorKind::InvalidRecord.into());
    }
    drop(st);
    let mut obj = Object::from_data_set(DataSet {
        elements,
        item_offset: None,
    });
    obj.set_value_big_endian(header.transfer_syntax.big_endian);
    Ok((header, obj))
}
fn is_plain_eof(e: &ObjectError) -> bool {
    e.is(ObjectKind::Eof)
}
