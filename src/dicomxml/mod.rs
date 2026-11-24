#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
mod error;
#[doc(hidden)]
pub mod xmlwire;
use crate::dicomjson::sizedparse;
use crate::dicomjson::{hex_tag, kind_name, parse_core_tag, read_encapsulated, trim_unicode_space};
use crate::dictionary::private::private_creator_id;
use crate::object::{
    DataSet, Element, ElementHeader, Object, Syntax, Tag, TextOptions, UNDEFINED_LENGTH, Value, Vr,
    dict, standard_names,
};
use crate::valueencode;
pub use error::Error;
use std::fmt;
use xmlwire::{Name, Printer, Tok, XmlError};
pub const NAMESPACE: &str = "http://dicom.nema.org/PS3.19/models/NativeDICOM";
pub const MEDIA_TYPE: &str = "application/dicom+xml";
pub const ERR_INVALID_XML: &str = "dicomxml: invalid Native DICOM Model XML";
pub const ERR_INVALID_LIMITS: &str = "dicomxml: invalid resource limits";
pub const ERR_MAX_XML_BYTES: &str = "dicomxml: maximum XML bytes exceeded";
pub const ERR_MAX_VALUE_BYTES: &str = "dicomxml: maximum value bytes exceeded";
pub const ERR_MAX_INLINE_BYTES: &str = "dicomxml: maximum InlineBinary bytes exceeded";
pub const ERR_MAX_BINARY_BYTES: &str = "dicomxml: maximum total binary bytes exceeded";
pub const ERR_MAX_DEPTH: &str = "dicomxml: maximum sequence depth exceeded";
pub const ERR_MAX_ELEMENTS: &str = "dicomxml: maximum element count exceeded";
pub const ERR_MAX_ITEMS: &str = "dicomxml: maximum sequence item count exceeded";
pub const ERR_BULK_DATA_REJECTED: &str = "dicomxml: BulkData rejected by policy";
pub const ERR_BULK_DATA_RESOLVER: &str = "dicomxml: BulkData resolver failed";
pub const ERR_TRANSFER_SYNTAX_REQUIRED: &str =
    "dicomxml: transfer syntax required for encapsulated Pixel Data";
const XML_HEADER: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
const XSI_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema-instance";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_xml_bytes: i64,
    pub max_value_bytes: i64,
    pub max_inline_binary_bytes: i64,
    pub max_total_binary_bytes: i64,
    pub max_sequence_depth: i64,
    pub max_elements: i64,
    pub max_sequence_items: i64,
}
impl Limits {
    pub fn defaults() -> Limits {
        Limits {
            max_xml_bytes: 64 << 20,
            max_value_bytes: 16 << 20,
            max_inline_binary_bytes: 32 << 20,
            max_total_binary_bytes: 64 << 20,
            max_sequence_depth: 64,
            max_elements: 1_000_000,
            max_sequence_items: 100_000,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRecoveryKind {
    NulPaddingTrimmed,
}
impl fmt::Display for TextRecoveryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("text NUL padding trimmed")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRecovery {
    pub kind: TextRecoveryKind,
    pub path: String,
    pub tag: Tag,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateRecoveryKind {
    CreatorNulTrimmed,
    ElementOmitted,
    ElementMerged,
}
impl fmt::Display for PrivateRecoveryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PrivateRecoveryKind::CreatorNulTrimmed => "private creator NUL padding trimmed",
            PrivateRecoveryKind::ElementOmitted => "private data element omitted",
            PrivateRecoveryKind::ElementMerged => {
                "private data element merged into its creator's first block"
            }
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateRecovery {
    pub kind: PrivateRecoveryKind,
    pub path: String,
    pub tag: Tag,
    pub creator: String,
}
pub type BulkDataUriFunc<'a> = dyn FnMut(Tag, Vr, &[u8]) -> String + 'a;
pub(crate) type PrivateRecoveredFunc<'a> = dyn FnMut(&PrivateRecovery) + 'a;
pub(crate) type TextRecoveredFunc<'a> = dyn FnMut(&TextRecovery) + 'a;
#[derive(Default)]
pub struct Options<'a> {
    pub pretty: bool,
    pub omit_group_length: bool,
    pub byte_order: Option<bool>,
    pub source_byte_order_inline_binary: bool,
    pub bulk_data_uri_func: Option<Box<BulkDataUriFunc<'a>>>,
    pub limits: Limits,
    pub recover_private: bool,
    pub private_recovered: Option<Box<PrivateRecoveredFunc<'a>>>,
    pub recover_text: bool,
    pub text_recovered: Option<Box<TextRecoveredFunc<'a>>>,
}
impl Options<'_> {
    pub fn defaults() -> Options<'static> {
        Options {
            pretty: true,
            omit_group_length: true,
            limits: Limits::defaults(),
            ..Options::default()
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BulkDataPolicy {
    #[default]
    Preserve,
    Reject,
    Resolve,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BulkDataReference {
    pub tag: Tag,
    pub vr: Vr,
    pub uri: String,
    pub uuid: String,
}
pub type BulkDataResolver<'a> =
    dyn FnMut(&BulkDataReference) -> Result<Option<Vec<u8>>, crate::BoxError> + 'a;
#[derive(Default)]
pub struct UnmarshalOptions<'a> {
    pub text_options: TextOptions,
    pub allow_missing_keyword: bool,
    pub allow_foreign_root_attributes: bool,
    pub byte_order: Option<bool>,
    pub source_byte_order_inline_binary: bool,
    pub transfer_syntax: Option<Syntax>,
    pub bulk_data_policy: BulkDataPolicy,
    pub bulk_data_resolver: Option<Box<BulkDataResolver<'a>>>,
    pub limits: Limits,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum XErr {
    Wrapped(String),
    Bare(String),
}
impl XErr {
    fn at(op: &str, path: &str, err: impl fmt::Display) -> XErr {
        XErr::Wrapped(if path.is_empty() {
            format!("dicomxml: {op}: {err}")
        } else {
            format!("dicomxml: {op} at {path}: {err}")
        })
    }
    fn or_at(self, op: &str, path: &str) -> XErr {
        match self {
            XErr::Bare(m) => XErr::at(op, path, m),
            w => w,
        }
    }
    fn text(self) -> String {
        match self {
            XErr::Wrapped(m) | XErr::Bare(m) => m,
        }
    }
}
fn invalid(path: &str, msg: &str) -> XErr {
    XErr::at("decode", path, format!("{ERR_INVALID_XML}: {msg}"))
}
fn token_error(path: &str, err: impl fmt::Display) -> XErr {
    XErr::at("decode", path, format!("{ERR_INVALID_XML}: {err}"))
}
struct Budget {
    limits: Limits,
    elements: i64,
    final_elements: i64,
    items: i64,
    binary: i64,
}
impl Budget {
    fn new(limits: Limits) -> Result<Budget, String> {
        let l = limits;
        if l.max_xml_bytes < 0
            || l.max_value_bytes < 0
            || l.max_inline_binary_bytes < 0
            || l.max_total_binary_bytes < 0
            || l.max_sequence_depth < 0
            || l.max_elements < 0
            || l.max_sequence_items < 0
        {
            return Err(ERR_INVALID_LIMITS.to_string());
        }
        let d = Limits::defaults();
        let pick = |v: i64, d: i64| if v == 0 { d } else { v };
        Ok(Budget {
            limits: Limits {
                max_xml_bytes: pick(l.max_xml_bytes, d.max_xml_bytes),
                max_value_bytes: pick(l.max_value_bytes, d.max_value_bytes),
                max_inline_binary_bytes: pick(l.max_inline_binary_bytes, d.max_inline_binary_bytes),
                max_total_binary_bytes: pick(l.max_total_binary_bytes, d.max_total_binary_bytes),
                max_sequence_depth: pick(l.max_sequence_depth, d.max_sequence_depth),
                max_elements: pick(l.max_elements, d.max_elements),
                max_sequence_items: pick(l.max_sequence_items, d.max_sequence_items),
            },
            elements: 0,
            final_elements: 0,
            items: 0,
            binary: 0,
        })
    }
    fn add_elements(&mut self, n: i64, depth: i64) -> Result<(), String> {
        if depth > self.limits.max_sequence_depth {
            return Err(format!(
                "{ERR_MAX_DEPTH}: got {depth}, limit {}",
                self.limits.max_sequence_depth
            ));
        }
        if n > self.limits.max_elements.saturating_sub(self.elements) {
            return Err(format!(
                "{ERR_MAX_ELEMENTS}: limit {}",
                self.limits.max_elements
            ));
        }
        self.elements = self.elements.saturating_add(n);
        Ok(())
    }
    fn add_item(&mut self, depth: i64) -> Result<(), String> {
        self.add_elements(0, depth)?;
        if self.items >= self.limits.max_sequence_items {
            return Err(format!(
                "{ERR_MAX_ITEMS}: limit {}",
                self.limits.max_sequence_items
            ));
        }
        self.items = self.items.saturating_add(1);
        Ok(())
    }
    fn check_value(&self, n: usize) -> Result<(), String> {
        if n as i64 > self.limits.max_value_bytes {
            return Err(format!(
                "{ERR_MAX_VALUE_BYTES}: got {n}, limit {}",
                self.limits.max_value_bytes
            ));
        }
        Ok(())
    }
    fn add_binary(&mut self, n: i64) -> Result<(), String> {
        if n > self
            .limits
            .max_total_binary_bytes
            .saturating_sub(self.binary)
        {
            return Err(format!(
                "{ERR_MAX_BINARY_BYTES}: limit {}",
                self.limits.max_total_binary_bytes
            ));
        }
        self.binary = self.binary.saturating_add(n);
        Ok(())
    }
}
fn check_xml_text(s: &str) -> Result<(), String> {
    for (i, r) in s.char_indices() {
        if !xmlwire::is_in_character_range(r as u32) {
            return Err(format!(
                "{ERR_INVALID_XML}: character {} at byte {i} cannot be represented in XML 1.0",
                xmlwire::u_plus(r as u32)
            ));
        }
    }
    Ok(())
}
fn check_xml_attrs(attrs: &[(&str, &str, &str)]) -> Result<(), String> {
    for (_, name, value) in attrs {
        check_xml_text(value).map_err(|e| format!("{name} attribute: {e}"))?;
    }
    Ok(())
}
fn attribute_path(parent: &str, tag: Tag) -> String {
    format!("{parent}/DicomAttribute[@tag='{}']", hex_tag(tag))
}
fn is_private_data_tag(tag: Tag) -> bool {
    dict::is_private_group(tag.group) && tag.element >= 0x1000
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
fn put(out: &mut Vec<u8>, v: u64, width: usize, big: bool) {
    let bytes = v.to_be_bytes();
    let s = bytes
        .get(8usize.saturating_sub(width)..)
        .unwrap_or_default();
    if big {
        out.extend_from_slice(s);
    } else {
        out.extend(s.iter().rev());
    }
}
fn float_text(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v == f64::INFINITY {
        "Infinity".into()
    } else if v == f64::NEG_INFINITY {
        "-Infinity".into()
    } else {
        crate::dicomjson::jsonwire::format_float(v)
    }
}
fn numeric_text(vr: Vr, d: &[u8], big: bool) -> String {
    match vr {
        Vr::AT => hex_tag(Tag::new(
            read_u16(d, big),
            read_u16(d.get(2..).unwrap_or_default(), big),
        )),
        Vr::SS => (read_u16(d, big) as i16).to_string(),
        Vr::US => read_u16(d, big).to_string(),
        Vr::SL => (read_u32(d, big) as i32).to_string(),
        Vr::UL => read_u32(d, big).to_string(),
        Vr::SV => (read_u64(d, big) as i64).to_string(),
        Vr::UV => read_u64(d, big).to_string(),
        Vr::FL => float_text(f64::from(f32::from_bits(read_u32(d, big)))),
        _ => float_text(f64::from_bits(read_u64(d, big))),
    }
}
fn numeric_width(vr: Vr) -> usize {
    match vr {
        Vr::SS | Vr::US => 2,
        Vr::AT | Vr::FL | Vr::SL | Vr::UL => 4,
        Vr::FD | Vr::SV | Vr::UV => 8,
        _ => 0,
    }
}
pub fn marshal(obj: Option<&Object>, opts: &mut Options) -> Result<Vec<u8>, Error> {
    marshal_into(obj, opts, None).map_err(Error::new)
}
#[cfg_attr(not(feature = "dicomweb"), allow(dead_code))]
pub(crate) fn marshal_text(obj: Option<&Object>, opts: &mut Options) -> Result<Vec<u8>, String> {
    marshal_into(obj, opts, None)
}
#[doc(hidden)]
pub fn encode_to(
    w: Option<&mut dyn std::io::Write>,
    obj: Option<&Object>,
    opts: &mut Options,
) -> Result<(), String> {
    let Some(w) = w else {
        return Err(XErr::at("encode", "", format!("{ERR_INVALID_XML}: nil writer")).text());
    };
    marshal_into(obj, opts, Some(w)).map(|_| ())
}
fn marshal_into(
    obj: Option<&Object>,
    opts: &mut Options,
    sink: Option<&mut dyn std::io::Write>,
) -> Result<Vec<u8>, String> {
    let budget = Budget::new(opts.limits).map_err(|e| XErr::at("encode", "", e).text())?;
    let big = opts
        .byte_order
        .unwrap_or_else(|| obj.is_some_and(Object::value_big_endian));
    if opts.recover_private && opts.private_recovered.is_none() {
        return Err(XErr::at(
            "encode",
            "",
            format!(
                "{ERR_INVALID_XML}: RecoverPrivate requires PrivateRecovered to report recoveries"
            ),
        )
        .text());
    }
    if opts.recover_text && opts.text_recovered.is_none() {
        return Err(XErr::at(
            "encode",
            "",
            format!("{ERR_INVALID_XML}: RecoverText requires TextRecovered to report recoveries"),
        )
        .text());
    }
    let limit_error = format!("{ERR_MAX_XML_BYTES}: limit reached");
    let mut p = match sink {
        Some(sink) => {
            Printer::with_sink(opts.pretty, budget.limits.max_xml_bytes, limit_error, sink)
        }
        None => Printer::new(opts.pretty, budget.limits.max_xml_bytes, limit_error),
    };
    p.write_direct(XML_HEADER.as_bytes())
        .map_err(|e| XErr::at("encode", "", e).text())?;
    let root = "/NativeDicomModel";
    p.start(
        "NativeDicomModel",
        &[("", "xmlns", NAMESPACE), ("xml", "space", "preserve")],
    )
    .map_err(|e| XErr::at("encode", root, e).text())?;
    let mut enc = Encoder {
        p,
        opts,
        b: budget,
        big,
    };
    let empty = Object::new();
    enc.dataset(obj.unwrap_or(&empty), 0, root)
        .map_err(XErr::text)?;
    enc.p
        .end("NativeDicomModel")
        .map_err(|e| XErr::at("encode", root, e).text())?;
    enc.p.finish().map_err(|e| XErr::at("encode", "", e).text())
}
struct Encoder<'o, 'a, 'w> {
    p: Printer<'w>,
    opts: &'o mut Options<'a>,
    b: Budget,
    big: bool,
}
#[derive(Clone)]
struct Creator {
    id: String,
    value: Option<String>,
    first: u16,
}
impl Encoder<'_, '_, '_> {
    fn private_recovered(&mut self, r: PrivateRecovery) {
        if let Some(f) = self.opts.private_recovered.as_mut() {
            f(&r);
        }
    }
    fn dataset(&mut self, obj: &Object, depth: i64, path: &str) -> Result<(), XErr> {
        self.b
            .add_elements(0, depth)
            .map_err(|e| XErr::at("encode", path, e))?;
        let elements = obj.sorted_elements();
        let count = elements
            .iter()
            .filter(|e| !e.tag().is_group_length())
            .count() as i64;
        self.b
            .add_elements(count, depth)
            .map_err(|e| XErr::at("encode", path, e))?;
        let creators = self.private_creators(obj, &elements, path)?;
        for elem in &elements {
            if elem.tag().is_group_length() {
                continue;
            }
            let mut creator = String::new();
            if is_private_data_tag(elem.tag()) {
                creator = self.data_element_creator(elem.tag(), obj, &creators, path)?;
                if creator.is_empty() {
                    continue;
                }
            }
            let replacement = creators
                .iter()
                .find(|(t, _)| *t == elem.tag())
                .and_then(|(_, c)| c.value.clone());
            self.element(obj, elem, &creator, replacement, depth, path)?;
        }
        Ok(())
    }
    fn private_creators(
        &mut self,
        obj: &Object,
        elements: &[Element],
        path: &str,
    ) -> Result<Vec<(Tag, Creator)>, XErr> {
        let mut out: Vec<(Tag, Creator)> = Vec::new();
        let mut firsts: Vec<((String, u16), u16)> = Vec::new();
        for elem in elements {
            let tag = elem.tag();
            if !dict::is_private_creator(tag) || matches!(elem.value, Value::BulkData(_)) {
                continue;
            }
            let (raw, rewrite) = match elem.vr() {
                Vr::LO => {
                    let values = obj
                        .lookup_strings(tag)
                        .map_err(|e| XErr::at("encode", &attribute_path(path, tag), e))?;
                    if values.len() != 1 {
                        continue;
                    }
                    (values.into_iter().next().unwrap_or_default(), false)
                }
                Vr::UN => {
                    let Some(data) = elem.raw() else {
                        continue;
                    };
                    if data.len() > 96 {
                        continue;
                    }
                    match String::from_utf8(data.to_vec()) {
                        Ok(s) => (s, true),
                        Err(_) => continue,
                    }
                }
                _ => continue,
            };
            let mut id = private_creator_id(&raw).map(str::to_string);
            let mut recovered = false;
            if id.is_err() && self.opts.recover_private {
                let trimmed = raw.trim_end_matches([' ', '\0']);
                if trimmed != raw {
                    id = private_creator_id(trimmed).map(str::to_string);
                    recovered = id.is_ok();
                }
            }
            let Ok(id) = id else {
                continue;
            };
            let mut c = Creator {
                id: id.clone(),
                value: None,
                first: 0,
            };
            if elem.vr() == Vr::LO && !recovered {
                c.id = raw.clone();
            }
            if rewrite || recovered {
                c.value = Some(id.clone());
            }
            let key = (c.id.clone(), tag.group);
            if let Some((_, first)) = firsts.iter().find(|(k, _)| *k == key) {
                c.first = *first;
            } else {
                firsts.push((key, tag.element));
            }
            out.push((tag, c));
            if recovered {
                self.private_recovered(PrivateRecovery {
                    kind: PrivateRecoveryKind::CreatorNulTrimmed,
                    path: attribute_path(path, tag),
                    tag,
                    creator: id,
                });
            }
        }
        Ok(out)
    }
    fn data_element_creator(
        &mut self,
        tag: Tag,
        dataset: &Object,
        creators: &[(Tag, Creator)],
        parent: &str,
    ) -> Result<String, XErr> {
        let path = attribute_path(parent, tag);
        let key = Tag::new(tag.group, tag.element >> 8);
        let Some((_, creator)) = creators.iter().find(|(t, _)| *t == key) else {
            if !self.opts.recover_private {
                return Err(XErr::at(
                    "encode",
                    &path,
                    format!("{ERR_INVALID_XML}: private data element has no valid private creator"),
                ));
            }
            self.private_recovered(PrivateRecovery {
                kind: PrivateRecoveryKind::ElementOmitted,
                path,
                tag,
                creator: String::new(),
            });
            return Ok(String::new());
        };
        let creator = creator.clone();
        if creator.first != 0 {
            if !self.opts.recover_private {
                return Err(XErr::at(
                    "encode",
                    &path,
                    format!(
                        "{ERR_INVALID_XML}: private creator {} reserves blocks {:02X} and {:02X} of group {:04X}, which the Native DICOM Model cannot tell apart",
                        crate::textfmt::quote(&creator.id),
                        creator.first,
                        tag.element >> 8,
                        tag.group
                    ),
                ));
            }
            if dataset.has(Tag::new(tag.group, creator.first << 8 | tag.element & 0xff)) {
                self.private_recovered(PrivateRecovery {
                    kind: PrivateRecoveryKind::ElementOmitted,
                    path,
                    tag,
                    creator: creator.id,
                });
                return Ok(String::new());
            }
            self.private_recovered(PrivateRecovery {
                kind: PrivateRecoveryKind::ElementMerged,
                path,
                tag,
                creator: creator.id.clone(),
            });
        }
        Ok(creator.id)
    }
    fn element(
        &mut self,
        obj: &Object,
        elem: &Element,
        creator: &str,
        replacement: Option<String>,
        depth: i64,
        parent: &str,
    ) -> Result<(), XErr> {
        let tag = elem.tag();
        let mut vr = elem.vr();
        let path = attribute_path(parent, tag);
        let fail = |e: String| XErr::at("encode", &path, e);
        if let Value::Strings(values) = &elem.value {
            valueencode::validate_string(vr, values).map_err(fail)?;
        }
        if replacement.is_some() {
            vr = Vr::LO;
        }
        if matches!(elem.value, Value::Sequence(_)) && vr == Vr::UN {
            vr = Vr::SQ;
        }
        let xml_tag = if creator.is_empty() {
            hex_tag(tag)
        } else {
            format!("{:04X}00{:02X}", tag.group, tag.element & 0xff)
        };
        let keyword = standard_names(tag).map(|(k, _)| k).unwrap_or("");
        let vr_text = vr.to_string();
        let mut attrs: Vec<(&str, &str, &str)> = Vec::new();
        if !creator.is_empty() {
            attrs.push(("", "privateCreator", creator));
        }
        attrs.push(("", "tag", &xml_tag));
        attrs.push(("", "vr", &vr_text));
        if !keyword.is_empty() {
            attrs.push(("", "keyword", keyword));
        }
        check_xml_attrs(&attrs).map_err(fail)?;
        self.p.start("DicomAttribute", &attrs).map_err(fail)?;
        match &elem.value {
            Value::BulkData(uri) => {
                if !uri.is_empty() {
                    self.bulk(uri, &path)?;
                }
            }
            value => {
                if let Some(r) = &replacement {
                    self.text_values(tag, vr, std::slice::from_ref(r), &path)?;
                } else if vr == Vr::SQ {
                    let Value::Sequence(seq) = value else {
                        return Err(fail(format!(
                            "expected SequenceValue, got {}",
                            kind_name(value)
                        )));
                    };
                    if seq.len() as i64
                        > self
                            .b
                            .limits
                            .max_sequence_items
                            .saturating_sub(self.b.items)
                    {
                        return Err(fail(ERR_MAX_ITEMS.to_string()));
                    }
                    let inner = depth.saturating_add(1);
                    if !seq.is_empty() {
                        self.b.add_elements(0, inner).map_err(fail)?;
                    }
                    let items = obj.get_sequence(tag).unwrap_or_default();
                    for (number, item) in (1usize..).zip(items.iter()) {
                        self.b.add_item(inner).map_err(fail)?;
                        let number = number.to_string();
                        self.p
                            .start("Item", &[("", "number", &number)])
                            .map_err(fail)?;
                        self.dataset(item, inner, &format!("{path}/Item[{number}]"))?;
                        self.p.end("Item").map_err(fail)?;
                    }
                } else if vr.is_string_like() {
                    let values = obj.lookup_strings(tag).map_err(|e| fail(e.to_string()))?;
                    self.text_values(tag, vr, &values, &path)?;
                } else {
                    self.binary_or_numeric(obj, elem, &path)
                        .map_err(|e| e.or_at("encode", &path))?;
                }
            }
        }
        self.p.end("DicomAttribute").map_err(fail)?;
        Ok(())
    }
    fn bulk(&mut self, uri: &str, path: &str) -> Result<(), XErr> {
        let lower = uri.to_lowercase();
        let attrs = if lower.starts_with("urn:uuid:") && uri.len() >= 9 && uri.is_char_boundary(9) {
            [("", "uuid", &uri[9..])]
        } else {
            [("", "uri", uri)]
        };
        check_xml_attrs(&attrs).map_err(|e| XErr::at("encode", &format!("{path}/BulkData"), e))?;
        self.p
            .start("BulkData", &attrs)
            .and_then(|()| self.p.end("BulkData"))
            .map_err(|e| XErr::at("encode", path, e))
    }
    fn trim_nul_padding(&mut self, value: &str, tag: Tag, path: &str) -> String {
        if !self.opts.recover_text || check_xml_text(value).is_ok() {
            return value.to_string();
        }
        let trimmed = value.trim_end_matches([' ', '\0']);
        if !value[trimmed.len()..].contains('\0') || check_xml_text(trimmed).is_err() {
            return value.to_string();
        }
        let r = TextRecovery {
            kind: TextRecoveryKind::NulPaddingTrimmed,
            path: path.to_string(),
            tag,
        };
        if let Some(f) = self.opts.text_recovered.as_mut() {
            f(&r);
        }
        trimmed.to_string()
    }
    fn text_values(&mut self, tag: Tag, vr: Vr, values: &[String], path: &str) -> Result<(), XErr> {
        if matches!(values, [only] if only.is_empty()) {
            return Ok(());
        }
        let at = |e: String| XErr::at("encode", path, e);
        for (i, value) in values.iter().enumerate() {
            self.b.check_value(value.len()).map_err(at)?;
            let number = i.saturating_add(1);
            let last = number == values.len();
            if vr == Vr::PN {
                let pn_path = format!("{path}/PersonName[{number}]");
                let number = number.to_string();
                self.p
                    .start("PersonName", &[("", "number", &number)])
                    .map_err(at)?;
                let mut groups: Vec<String> = value.splitn(3, '=').map(str::to_string).collect();
                let names = ["Alphabetic", "Ideographic", "Phonetic"];
                if last
                    && let Some(j) = groups.iter().rposition(|g| !g.is_empty())
                    && let (Some(name), Some(group)) = (names.get(j), groups.get(j).cloned())
                {
                    let p = format!("{pn_path}/{name}");
                    let trimmed = self.trim_nul_padding(&group, tag, &p);
                    if let Some(slot) = groups.get_mut(j) {
                        *slot = trimmed;
                    }
                }
                for (group, name) in groups.iter().zip(names) {
                    if !group.is_empty() {
                        self.person_name_group(name, group, &format!("{pn_path}/{name}"))?;
                    }
                }
                self.p.end("PersonName").map_err(at)?;
            } else {
                let value_path = format!("{path}/Value[{number}]");
                let is_creator = dict::is_private_creator(tag);
                let value = if last && !is_creator {
                    self.trim_nul_padding(value, tag, &value_path)
                } else {
                    value.clone()
                };
                if let Err(e) = check_xml_text(&value) {
                    let e = if is_creator {
                        format!("private creator: {e}")
                    } else {
                        e
                    };
                    return Err(XErr::at("encode", &value_path, e));
                }
                self.numbered_value(&value, number).map_err(at)?;
            }
        }
        Ok(())
    }
    fn numbered_value(&mut self, value: &str, number: usize) -> Result<(), String> {
        self.b.check_value(value.len())?;
        let n = number.to_string();
        self.p.start("Value", &[("", "number", &n)])?;
        if !value.is_empty() {
            self.p.chardata(value.as_bytes())?;
        }
        self.p.end("Value")
    }
    fn person_name_group(&mut self, name: &str, value: &str, path: &str) -> Result<(), XErr> {
        let at = |e: String| XErr::at("encode", path, e);
        self.p.start(name, &[]).map_err(at)?;
        let names = [
            "FamilyName",
            "GivenName",
            "MiddleName",
            "NamePrefix",
            "NameSuffix",
        ];
        for (part, part_name) in value.splitn(5, '^').zip(names) {
            if part.is_empty() {
                continue;
            }
            check_xml_text(part)
                .map_err(|e| XErr::at("encode", &format!("{path}/{part_name}"), e))?;
            self.p
                .start(part_name, &[])
                .and_then(|()| self.p.chardata(part.as_bytes()))
                .and_then(|()| self.p.end(part_name))
                .map_err(at)?;
        }
        self.p.end(name).map_err(at)?;
        Ok(())
    }
    fn little_endian_width(&self, obj: &Object, elem: &Element) -> usize {
        if self.opts.source_byte_order_inline_binary || !self.big {
            return 1;
        }
        if matches!(elem.value, Value::Fragments { .. }) {
            return 1;
        }
        let mut bits = 0;
        if elem.tag() == Tag::PIXEL_DATA
            && elem.vr() == Vr::OW
            && let Some(allocated) = obj.get(valueencode::BITS_ALLOCATED_TAG)
        {
            bits = valueencode::bits_allocated(allocated, self.big);
        }
        valueencode::little_endian_unit_width(elem.tag(), elem.vr(), bits)
    }
    fn value_field(&self, elem: &Element) -> Result<Vec<u8>, XErr> {
        match &elem.value {
            Value::Raw(raw) => Ok(raw.clone()),
            Value::Fragments {
                offset_table,
                fragments,
            } => {
                binary_length(elem)?;
                let big = self.big;
                let mut out = Vec::new();
                let item = |out: &mut Vec<u8>, tag: Tag, len: u64| {
                    put(out, u64::from(tag.group), 2, big);
                    put(out, u64::from(tag.element), 2, big);
                    put(out, len, 4, big);
                };
                item(&mut out, Tag::ITEM, offset_table.len() as u64);
                out.extend_from_slice(offset_table);
                for f in fragments {
                    item(
                        &mut out,
                        Tag::ITEM,
                        f.len().saturating_add(f.len() % 2) as u64,
                    );
                    out.extend_from_slice(f);
                    if f.len() % 2 == 1 {
                        out.push(0);
                    }
                }
                item(&mut out, Tag::SEQUENCE_DELIMITATION, 0);
                Ok(out)
            }
            Value::Nil => Ok(Vec::new()),
            Value::Strings(v) if v.is_empty() => Ok(Vec::new()),
            Value::Sequence(v) if v.is_empty() => Ok(Vec::new()),
            other => Err(XErr::Bare(format!(
                "{ERR_INVALID_XML}: unsupported {} value type {}",
                elem.tag(),
                kind_name(other)
            ))),
        }
    }
    fn binary_or_numeric(&mut self, obj: &Object, elem: &Element, path: &str) -> Result<(), XErr> {
        let vr = elem.vr();
        let width = numeric_width(vr);
        if width != 0 {
            let data = self.value_field(elem)?;
            for (number, chunk) in (1usize..).zip(data.chunks_exact(width)) {
                let text = numeric_text(vr, chunk, self.big);
                self.numbered_value(&text, number).map_err(XErr::Bare)?;
            }
            if !data.len().is_multiple_of(width) {
                return Err(XErr::Bare(format!(
                    "{ERR_INVALID_XML}: {} raw numeric length is not a multiple of {width}",
                    elem.tag()
                )));
            }
            return Ok(());
        }
        let (length, known) = binary_length(elem)?;
        if let Value::Fragments { fragments, .. } = &elem.value {
            if fragments.len() as i64
                > self
                    .b
                    .limits
                    .max_sequence_items
                    .saturating_sub(self.b.items)
            {
                return Err(XErr::Bare(ERR_MAX_ITEMS.to_string()));
            }
            self.b.items = self.b.items.saturating_add(fragments.len() as i64);
        }
        if known && length == 0 {
            return Ok(());
        }
        let width = self.little_endian_width(obj, elem);
        let mut raw = self.value_field(elem)?;
        let mut length = length;
        if let Some(func) = self.opts.bulk_data_uri_func.as_mut() {
            if raw.is_empty() {
                return Ok(());
            }
            valueencode::reverse_units_in_place(&mut raw, width);
            let uri = func(elem.tag(), vr, &raw);
            if !uri.is_empty() {
                return self.bulk(&uri, path);
            }
            length = raw.len() as i64;
        } else {
            valueencode::reverse_units_in_place(&mut raw, width);
        }
        if length > self.b.limits.max_inline_binary_bytes {
            return Err(XErr::Bare(ERR_MAX_INLINE_BYTES.to_string()));
        }
        if length
            > self
                .b
                .limits
                .max_total_binary_bytes
                .saturating_sub(self.b.binary)
        {
            return Err(XErr::Bare(ERR_MAX_BINARY_BYTES.to_string()));
        }
        self.b.add_binary(raw.len() as i64).map_err(XErr::Bare)?;
        let text = sizedparse::base64_encode(&raw);
        if !text.is_empty() {
            self.p.start("InlineBinary", &[]).map_err(XErr::Bare)?;
            for chunk in text.as_bytes().chunks(1024) {
                self.p.chardata(chunk).map_err(XErr::Bare)?;
            }
            self.p.end("InlineBinary").map_err(XErr::Bare)?;
        }
        Ok(())
    }
}
fn binary_length(elem: &Element) -> Result<(i64, bool), XErr> {
    match &elem.value {
        Value::Raw(raw) => Ok((raw.len() as i64, true)),
        Value::Fragments {
            offset_table,
            fragments,
        } => {
            if elem.tag() != Tag::PIXEL_DATA || elem.vr() != Vr::OB && elem.vr() != Vr::OW {
                return Err(XErr::Bare(format!(
                    "{ERR_INVALID_XML}: fragments require OB or OW Pixel Data"
                )));
            }
            if offset_table.len() % 4 != 0 {
                return Err(XErr::Bare(format!(
                    "{ERR_INVALID_XML}: invalid Basic Offset Table length"
                )));
            }
            let mut total = (offset_table.len() as i64).saturating_add(16);
            for f in fragments {
                let padded = f.len().saturating_add(f.len() % 2) as i64;
                total = total.saturating_add(padded).saturating_add(8);
            }
            Ok((total, true))
        }
        Value::Nil if elem.header.length_set && elem.header.length != UNDEFINED_LENGTH => {
            Ok((i64::from(elem.header.length), true))
        }
        Value::Nil => Ok((0, false)),
        Value::Strings(v) if v.is_empty() => Ok((0, true)),
        Value::Sequence(v) if v.is_empty() => Ok((0, true)),
        _ => Ok((0, false)),
    }
}
pub fn charset_reader(label: &str, input: &[u8]) -> Result<Vec<u8>, Error> {
    charset_reader_text(label, input).map_err(Error::new)
}
pub(crate) fn charset_reader_text(label: &str, input: &[u8]) -> Result<Vec<u8>, String> {
    let l = label.to_ascii_lowercase();
    let latin1 = [
        "iso-8859-1",
        "iso_8859-1",
        "iso_8859-1:1987",
        "latin1",
        "l1",
        "ibm819",
        "cp819",
        "csisolatin1",
        "iso-ir-100",
    ];
    let ascii = [
        "us-ascii",
        "ansi_x3.4-1968",
        "iso-ir-6",
        "ansi_x3.4-1986",
        "iso_646.irv:1991",
        "iso646-us",
        "us",
        "ibm367",
        "cp367",
        "csascii",
    ];
    let decode = |f: &dyn Fn(u8) -> char| -> Vec<u8> {
        input.iter().map(|&b| f(b)).collect::<String>().into_bytes()
    };
    if latin1.contains(&l.as_str()) {
        return Ok(decode(&|b| char::from(b)));
    }
    if ascii.contains(&l.as_str()) {
        return Ok(decode(&|b| {
            if b < 0x80 { char::from(b) } else { '\u{FFFD}' }
        }));
    }
    if l == "windows-1252" || l == "cswindows1252" {
        const HIGH: [u32; 32] = [
            0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
            0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022,
            0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
        ];
        return Ok(decode(&|b| {
            if (0x80..0xA0).contains(&b) {
                HIGH.get(usize::from(b.wrapping_sub(0x80)))
                    .and_then(|&c| char::from_u32(c))
                    .unwrap_or('\u{FFFD}')
            } else {
                char::from(b)
            }
        }));
    }
    Err(format!(
        "unsupported XML encoding {}",
        crate::textfmt::quote(label)
    ))
}
struct Attribute {
    tag: String,
    private_creator: String,
    element: Element,
    inline_binary: bool,
}
struct Parser<'o, 'a> {
    dec: xmlwire::Decoder,
    b: Budget,
    opts: &'o mut UnmarshalOptions<'a>,
    big: bool,
}
fn ns_is(name: &Name, local: &str) -> bool {
    name.space == NAMESPACE && name.local == local
}
fn trim_is_empty(data: &[u8]) -> bool {
    trim_unicode_space(&String::from_utf8_lossy(data)).is_empty()
}
pub fn unmarshal(data: &[u8], opts: &mut UnmarshalOptions) -> Result<Object, Error> {
    unmarshal_text(data, opts).map_err(Error::new)
}
pub(crate) fn unmarshal_text(data: &[u8], opts: &mut UnmarshalOptions) -> Result<Object, String> {
    decode(data, opts, None).map(Option::unwrap_or_default)
}
#[derive(Clone, Debug, PartialEq)]
pub struct WalkAttribute {
    pub element: Element,
    pub private_creator: String,
    pub path: String,
}
pub(crate) type Visitor<'v> = dyn FnMut(WalkAttribute) -> Result<(), String> + 'v;
pub fn walk(
    data: &[u8],
    opts: &mut UnmarshalOptions,
    visit: &mut dyn FnMut(WalkAttribute) -> Result<(), crate::BoxError>,
) -> Result<(), Error> {
    let mut visit = |a: WalkAttribute| visit(a).map_err(|e| e.to_string());
    decode(data, opts, Some(&mut visit))
        .map(|_| ())
        .map_err(Error::new)
}
fn decode(
    data: &[u8],
    opts: &mut UnmarshalOptions,
    visit: Option<&mut Visitor>,
) -> Result<Option<Object>, String> {
    let b = Budget::new(opts.limits).map_err(|e| XErr::at("decode", "", e).text())?;
    let big = match opts.byte_order {
        Some(big) => big,
        None => opts.transfer_syntax.is_some_and(|s| s.big_endian),
    };
    if opts.bulk_data_policy == BulkDataPolicy::Resolve && opts.bulk_data_resolver.is_none() {
        return Err(XErr::at(
            "decode",
            "",
            format!("{ERR_INVALID_XML}: ResolveBulkData requires a resolver"),
        )
        .text());
    }
    let limit = b.limits.max_xml_bytes.max(0) as usize;
    let (input, tail) = if data.len() > limit {
        (
            data.get(..limit).unwrap_or(data).to_vec(),
            Some(ERR_MAX_XML_BYTES.to_string()),
        )
    } else {
        (data.to_vec(), None)
    };
    let mut dec = xmlwire::Decoder::new(input, tail, charset_reader_text);
    dec.set_max_decoded_bytes(limit);
    let mut p = Parser { dec, b, opts, big };
    let walking = visit.is_some();
    let elements = p.parse_document(visit).map_err(XErr::text)?;
    if walking {
        return Ok(None);
    }
    let mut obj = Object::from_elements_with_text_options(elements, p.opts.text_options.clone());
    obj.set_value_big_endian(big);
    if p.opts.bulk_data_policy == BulkDataPolicy::Resolve {
        p.resolve_object(&mut obj).map_err(XErr::text)?;
    }
    Ok(Some(obj))
}
impl Parser<'_, '_> {
    fn token(&mut self, path: &str) -> Result<Tok, XErr> {
        self.dec.token().map_err(|e| token_error(path, e))
    }
    fn parse_document(&mut self, mut visit: Option<&mut Visitor>) -> Result<Vec<Element>, XErr> {
        loop {
            let tok = match self.dec.token() {
                Ok(t) => t,
                Err(XmlError::Eof) => return Err(invalid("", "missing root element")),
                Err(e) => return Err(token_error("", e)),
            };
            match tok {
                Tok::ProcInst(target, _) => {
                    if !target.eq_ignore_ascii_case("xml") {
                        return Err(invalid("/", "processing instructions are not allowed"));
                    }
                }
                Tok::Directive(_) => {
                    return Err(invalid("/", "DTD and directives are not allowed"));
                }
                Tok::Comment(_) => {}
                Tok::CharData(d) => {
                    if !trim_is_empty(&d) {
                        return Err(invalid("/", "text is not allowed before the root element"));
                    }
                }
                Tok::Start(name, attrs) => {
                    if !ns_is(&name, "NativeDicomModel") {
                        return Err(invalid(
                            "/",
                            &format!(
                                "root must be NativeDicomModel in namespace {}",
                                crate::textfmt::quote(NAMESPACE)
                            ),
                        ));
                    }
                    self.validate_root_attributes(&attrs)?;
                    let elements =
                        self.parse_data_set(&name, 0, "/NativeDicomModel", visit.take())?;
                    self.ensure_document_eof()?;
                    return Ok(elements);
                }
                Tok::End(_) => {}
            }
        }
    }
    fn validate_root_attributes(&self, attrs: &[xmlwire::Attr]) -> Result<(), XErr> {
        let path = "/NativeDicomModel";
        let foreign = self.opts.allow_foreign_root_attributes;
        let (mut space, mut default_ns) = (false, false);
        for a in attrs {
            if foreign && (a.name.space == "xmlns" || a.name.space == XSI_NAMESPACE) {
                continue;
            }
            if a.name.space.is_empty() && a.name.local == "xmlns" && a.value == NAMESPACE {
                if default_ns {
                    return Err(invalid(path, "duplicate default namespace declaration"));
                }
                default_ns = true;
                continue;
            }
            if a.name.space == xmlwire::XML_URL && a.name.local == "space" {
                if space || a.value != "preserve" {
                    return Err(invalid(
                        path,
                        "xml:space must occur once with value \"preserve\"",
                    ));
                }
                space = true;
                continue;
            }
            return Err(invalid(
                path,
                &format!("unexpected root attribute {}", a.name.local),
            ));
        }
        if !space && !foreign {
            return Err(invalid(path, "missing xml:space=\"preserve\""));
        }
        Ok(())
    }
    fn ensure_document_eof(&mut self) -> Result<(), XErr> {
        loop {
            match self.dec.token() {
                Err(XmlError::Eof) => return Ok(()),
                Err(e) => return Err(token_error("/", e)),
                Ok(Tok::CharData(d)) => {
                    if !trim_is_empty(&d) {
                        return Err(invalid("/", "trailing text is not allowed"));
                    }
                }
                Ok(Tok::Comment(_)) => {}
                Ok(_) => return Err(invalid("/", "trailing XML content is not allowed")),
            }
        }
    }
    fn parse_data_set(
        &mut self,
        parent: &Name,
        depth: i64,
        path: &str,
        mut visit: Option<&mut Visitor>,
    ) -> Result<Vec<Element>, XErr> {
        let mut attrs: Vec<Attribute> = Vec::new();
        let mut bits = 0i64;
        loop {
            match self.token(path)? {
                Tok::Directive(_) => {
                    return Err(invalid(path, "DTD and directives are not allowed"));
                }
                Tok::ProcInst(..) => {
                    return Err(invalid(path, "processing instructions are not allowed"));
                }
                Tok::CharData(d) => {
                    if !trim_is_empty(&d) {
                        return Err(invalid(path, "text is not allowed between attributes"));
                    }
                }
                Tok::Comment(_) => {}
                Tok::Start(name, a) => {
                    if !ns_is(&name, "DicomAttribute") {
                        return Err(invalid(path, &format!("unexpected element {}", name.local)));
                    }
                    self.b
                        .add_elements(1, depth)
                        .map_err(|e| XErr::at("decode", path, e))?;
                    let mut attr = self.parse_attribute(&name, &a, depth, path)?;
                    if attr.element.tag() == valueencode::BITS_ALLOCATED_TAG {
                        bits = valueencode::bits_allocated(&attr.element, self.big);
                    }
                    if let Some(visit) = visit.as_mut() {
                        let attr_path = format!("{path}/DicomAttribute[@tag='{}']", attr.tag);
                        let mut elem = attr.element.clone();
                        if attr.inline_binary {
                            self.to_value_byte_order(&mut elem, bits);
                        }
                        if self.opts.bulk_data_policy == BulkDataPolicy::Resolve {
                            elem = self.resolve_element(elem, &attr_path, bits)?;
                        }
                        let keep = attr.private_creator.is_empty()
                            && dict::is_private_creator(elem.tag())
                            && matches!(elem.value, Value::Strings(_));
                        if !keep {
                            attr.element.value = Value::Nil;
                        }
                        visit(WalkAttribute {
                            element: elem,
                            private_creator: attr.private_creator.clone(),
                            path: attr_path.clone(),
                        })
                        .map_err(|e| XErr::at("decode", &attr_path, e))?;
                    }
                    attrs.push(attr);
                }
                Tok::End(name) => {
                    if name != *parent {
                        return Err(invalid(
                            path,
                            &format!("unexpected closing element {}", name.local),
                        ));
                    }
                    if visit.is_none() {
                        for a in &mut attrs {
                            if a.inline_binary {
                                self.to_value_byte_order(&mut a.element, bits);
                            }
                        }
                    }
                    return self.assemble_data_set(attrs, path);
                }
            }
        }
    }
    fn to_value_byte_order(&self, elem: &mut Element, bits: i64) {
        if self.opts.source_byte_order_inline_binary || !self.big {
            return;
        }
        let width = valueencode::little_endian_unit_width(elem.tag(), elem.vr(), bits);
        if let Value::Raw(raw) = &mut elem.value {
            valueencode::reverse_units_in_place(raw, width);
        }
    }
    fn parse_attribute(
        &mut self,
        start: &Name,
        attrs: &[xmlwire::Attr],
        depth: i64,
        parent: &str,
    ) -> Result<Attribute, XErr> {
        let (mut tag_s, mut vr_s, mut keyword, mut creator) =
            (String::new(), String::new(), String::new(), String::new());
        let mut seen: Vec<&str> = Vec::new();
        for a in attrs {
            if !a.name.space.is_empty() {
                return Err(invalid(
                    parent,
                    &format!(
                        "namespaced DicomAttribute attribute {} is not allowed",
                        a.name.local
                    ),
                ));
            }
            if seen.contains(&a.name.local.as_str()) {
                return Err(invalid(
                    parent,
                    &format!("duplicate attribute {}", a.name.local),
                ));
            }
            seen.push(&a.name.local);
            match a.name.local.as_str() {
                "tag" => tag_s = a.value.clone(),
                "vr" => vr_s = a.value.clone(),
                "keyword" => keyword = a.value.clone(),
                "privateCreator" => creator = a.value.clone(),
                other => {
                    return Err(invalid(
                        parent,
                        &format!("unexpected DicomAttribute attribute {other}"),
                    ));
                }
            }
        }
        if !is_tag_pattern(&tag_s) {
            return Err(invalid(
                parent,
                &format!(
                    "tag {} must be eight uppercase hexadecimal digits",
                    crate::textfmt::quote(&tag_s)
                ),
            ));
        }
        let tag = parse_core_tag(&tag_s).unwrap_or_default();
        if tag.is_group_length() && creator.is_empty() {
            return Err(invalid(parent, "group length attributes are prohibited"));
        }
        let path = format!("{parent}/DicomAttribute[@tag='{tag_s}']");
        if vr_s.is_empty() && !creator.is_empty() {
            return Err(invalid(&path, "VR is required for a private data element"));
        }
        if vr_s.is_empty() {
            match dict::by_tag(tag) {
                Some(e) => vr_s = e.vr.to_string(),
                None => return Err(invalid(&path, "VR is required when the tag is unknown")),
            }
        }
        let Some(vr) = Vr::parse(vr_s.as_bytes()) else {
            return Err(token_error(
                &path,
                format!("dicom: invalid VR {}", crate::textfmt::quote(&vr_s)),
            ));
        };
        let known = standard_names(tag)
            .map(|(k, _)| k)
            .filter(|k| !k.is_empty());
        if !keyword.is_empty() {
            if let Some(k) = known
                && keyword != k
            {
                return Err(invalid(
                    &path,
                    &format!(
                        "keyword {} does not match {}",
                        crate::textfmt::quote(&keyword),
                        crate::textfmt::quote(k)
                    ),
                ));
            }
        } else if let Some(k) = known
            && !self.opts.allow_missing_keyword
        {
            return Err(invalid(
                &path,
                &format!(
                    "keyword {} is required for known tag {tag_s}",
                    crate::textfmt::quote(k)
                ),
            ));
        }
        if !creator.is_empty() {
            if !dict::is_private_group(tag.group) || tag.element > 0x00ff {
                return Err(invalid(
                    &path,
                    "privateCreator requires a normalized private tag gggg00ee",
                ));
            }
        } else if is_private_data_tag(tag) {
            return Err(invalid(
                &path,
                "private data tag must be normalized and include privateCreator",
            ));
        }
        let mut kind = String::new();
        let mut expected = 1usize;
        let mut values: Vec<String> = Vec::new();
        let mut persons: Vec<[Option<[String; 5]>; 3]> = Vec::new();
        let mut items: Vec<DataSet> = Vec::new();
        let mut inline: Option<Vec<u8>> = None;
        let mut bulk: Option<BulkDataReference> = None;
        loop {
            match self.token(&path)? {
                Tok::Directive(_) => {
                    return Err(invalid(&path, "DTD and directives are not allowed"));
                }
                Tok::ProcInst(..) => {
                    return Err(invalid(&path, "processing instructions are not allowed"));
                }
                Tok::CharData(d) => {
                    if !trim_is_empty(&d) {
                        return Err(invalid(&path, "text is not allowed outside a value"));
                    }
                }
                Tok::Comment(_) => {}
                Tok::Start(name, a) => {
                    if name.space != NAMESPACE {
                        return Err(invalid(
                            &path,
                            &format!("foreign namespace on {}", name.local),
                        ));
                    }
                    let child = name.local.clone();
                    if !kind.is_empty() && kind != child {
                        return Err(invalid(
                            &path,
                            &format!("mixed value representations {kind} and {child}"),
                        ));
                    }
                    kind = child.clone();
                    match child.as_str() {
                        "Value" => {
                            if vr == Vr::SQ || vr == Vr::PN {
                                return Err(invalid(
                                    &path,
                                    &format!("Value is prohibited for VR {vr}"),
                                ));
                            }
                            let vp = format!("{path}/Value");
                            validate_number(&a, expected, &vp)?;
                            let value = self.parse_text_element(&name, &vp)?;
                            if vr == Vr::AT && !is_tag_pattern(&value) {
                                return Err(invalid(
                                    &vp,
                                    &format!(
                                        "AT value {} must be eight uppercase hexadecimal digits",
                                        crate::textfmt::quote(&value)
                                    ),
                                ));
                            }
                            values.push(value);
                            expected = expected.saturating_add(1);
                        }
                        "Item" => {
                            if vr != Vr::SQ {
                                return Err(invalid(&path, "Item requires VR SQ"));
                            }
                            validate_number(&a, expected, &format!("{path}/Item"))?;
                            self.b
                                .add_item(depth.saturating_add(1))
                                .map_err(|e| XErr::at("decode", &path, e))?;
                            let item = self.parse_data_set(
                                &name,
                                depth.saturating_add(1),
                                &format!("{path}/Item[{expected}]"),
                                None,
                            )?;
                            items.push(DataSet {
                                elements: item,
                                item_offset: None,
                            });
                            expected = expected.saturating_add(1);
                        }
                        "PersonName" => {
                            if vr != Vr::PN {
                                return Err(invalid(&path, "PersonName requires VR PN"));
                            }
                            let pp = format!("{path}/PersonName");
                            validate_number(&a, expected, &pp)?;
                            persons.push(self.parse_person_name(&name, &pp)?);
                            expected = expected.saturating_add(1);
                        }
                        "InlineBinary" => {
                            if expected != 1 || !inline_vr(vr) {
                                return Err(invalid(
                                    &path,
                                    &format!(
                                        "InlineBinary is not valid for VR {vr} or multiplicity"
                                    ),
                                ));
                            }
                            if !a.is_empty() {
                                return Err(invalid(
                                    &path,
                                    "InlineBinary has unexpected attributes",
                                ));
                            }
                            inline = Some(
                                self.parse_inline_binary(&name, &format!("{path}/InlineBinary"))?,
                            );
                            expected = expected.saturating_add(1);
                        }
                        "BulkData" => {
                            if expected != 1 || vr == Vr::SQ {
                                return Err(invalid(
                                    &path,
                                    &format!("BulkData is not valid for VR {vr} or multiplicity"),
                                ));
                            }
                            bulk = Some(self.parse_bulk_data(
                                &name,
                                &a,
                                tag,
                                vr,
                                &format!("{path}/BulkData"),
                            )?);
                            expected = expected.saturating_add(1);
                        }
                        _ => {
                            return Err(invalid(
                                &path,
                                &format!("unexpected value element {child}"),
                            ));
                        }
                    }
                }
                Tok::End(name) => {
                    if name != *start {
                        return Err(invalid(
                            &path,
                            &format!("unexpected closing element {}", name.local),
                        ));
                    }
                    let inline_binary = inline.is_some();
                    let element = self
                        .attribute_element(tag, vr, values, persons, items, inline, bulk, &path)?;
                    return Ok(Attribute {
                        tag: tag_s,
                        private_creator: creator,
                        element,
                        inline_binary,
                    });
                }
            }
        }
    }
    fn parse_text_element(&mut self, start: &Name, path: &str) -> Result<String, XErr> {
        let limit = self.b.limits.max_value_bytes;
        let mut text: Vec<u8> = Vec::new();
        loop {
            match self.token(path)? {
                Tok::CharData(d) => {
                    if text.len() as i64 > limit.saturating_sub(d.len() as i64) {
                        return Err(XErr::at("decode", path, ERR_MAX_VALUE_BYTES));
                    }
                    text.extend_from_slice(&d);
                }
                Tok::Comment(_) => {}
                Tok::End(name) => {
                    if name != *start {
                        return Err(invalid(path, "unexpected closing element"));
                    }
                    return Ok(String::from_utf8_lossy(&text).into_owned());
                }
                _ => return Err(invalid(path, "markup is not allowed inside text values")),
            }
        }
    }
    fn parse_person_name(
        &mut self,
        start: &Name,
        path: &str,
    ) -> Result<[Option<[String; 5]>; 3], XErr> {
        let mut pn: [Option<[String; 5]>; 3] = [None, None, None];
        let mut last: i64 = -1;
        loop {
            match self.token(path)? {
                Tok::CharData(d) => {
                    if !trim_is_empty(&d) {
                        return Err(invalid(path, "text outside name components is not allowed"));
                    }
                }
                Tok::Comment(_) => {}
                Tok::Start(name, a) => {
                    let idx = ["Alphabetic", "Ideographic", "Phonetic"]
                        .iter()
                        .position(|g| *g == name.local);
                    let Some(idx) =
                        idx.filter(|&i| name.space == NAMESPACE && i as i64 > last && a.is_empty())
                    else {
                        return Err(invalid(
                            path,
                            &format!("invalid or out-of-order person name group {}", name.local),
                        ));
                    };
                    let group = self.parse_name_group(&name, &format!("{path}/{}", name.local))?;
                    if let Some(slot) = pn.get_mut(idx) {
                        *slot = Some(group);
                    }
                    last = idx as i64;
                }
                Tok::End(name) => {
                    if name != *start {
                        return Err(invalid(path, "unexpected closing element"));
                    }
                    return Ok(pn);
                }
                _ => return Err(invalid(path, "unsupported XML token")),
            }
        }
    }
    fn parse_name_group(&mut self, start: &Name, path: &str) -> Result<[String; 5], XErr> {
        let mut values: [String; 5] = Default::default();
        let mut last: i64 = -1;
        let names = [
            "FamilyName",
            "GivenName",
            "MiddleName",
            "NamePrefix",
            "NameSuffix",
        ];
        loop {
            match self.token(path)? {
                Tok::CharData(d) => {
                    if !trim_is_empty(&d) {
                        return Err(invalid(path, "text outside name components is not allowed"));
                    }
                }
                Tok::Comment(_) => {}
                Tok::Start(name, a) => {
                    let Some(idx) = names
                        .iter()
                        .position(|n| *n == name.local)
                        .filter(|&i| name.space == NAMESPACE && i as i64 > last && a.is_empty())
                    else {
                        return Err(invalid(
                            path,
                            &format!("invalid or out-of-order name component {}", name.local),
                        ));
                    };
                    let text = self.parse_text_element(&name, &format!("{path}/{}", name.local))?;
                    if let Some(slot) = values.get_mut(idx) {
                        *slot = text;
                    }
                    last = idx as i64;
                }
                Tok::End(name) => {
                    if name != *start {
                        return Err(invalid(path, "unexpected closing element"));
                    }
                    return Ok(values);
                }
                _ => return Err(invalid(path, "unsupported XML token")),
            }
        }
    }
    fn parse_inline_binary(&mut self, start: &Name, path: &str) -> Result<Vec<u8>, XErr> {
        const CHUNK: usize = 32 << 10;
        let mut encoded: Vec<u8> = Vec::with_capacity(64);
        let mut out: Vec<u8> = Vec::new();
        let mut padded = false;
        loop {
            let tok = self.token(path)?;
            match tok {
                Tok::CharData(d) => {
                    for c in d {
                        if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
                            continue;
                        }
                        if padded {
                            return Err(invalid(path, "invalid base64: data after padding"));
                        }
                        encoded.push(c);
                        if encoded.len() == CHUNK {
                            padded = self.flush_inline(&mut encoded, &mut out, path)?;
                        }
                    }
                }
                Tok::Comment(_) => {}
                Tok::End(name) => {
                    if name != *start {
                        return Err(invalid(path, "unexpected closing element"));
                    }
                    self.flush_inline(&mut encoded, &mut out, path)?;
                    if out.is_empty() {
                        return Err(invalid(
                            path,
                            "InlineBinary must not represent a zero-length Value Field",
                        ));
                    }
                    return Ok(out);
                }
                other => {
                    return Err(invalid(
                        path,
                        &format!(
                            "markup is not allowed inside InlineBinary ({})",
                            other.type_name()
                        ),
                    ));
                }
            }
        }
    }
    fn flush_inline(
        &mut self,
        encoded: &mut Vec<u8>,
        out: &mut Vec<u8>,
        path: &str,
    ) -> Result<bool, XErr> {
        let used = encoded.len();
        if !used.is_multiple_of(4) {
            return Err(invalid(
                path,
                "invalid base64: length is not a multiple of four",
            ));
        }
        if used == 0 {
            return Ok(false);
        }
        let mut n = (used / 4).saturating_mul(3);
        for pad in encoded.iter().rev().take(2) {
            if *pad != b'=' {
                break;
            }
            n = n.saturating_sub(1);
        }
        if n as i64
            > self
                .b
                .limits
                .max_inline_binary_bytes
                .saturating_sub(out.len() as i64)
        {
            return Err(XErr::at("decode", path, ERR_MAX_INLINE_BYTES));
        }
        if n as i64
            > self
                .b
                .limits
                .max_total_binary_bytes
                .saturating_sub(self.b.binary)
        {
            return Err(XErr::at("decode", path, ERR_MAX_BINARY_BYTES));
        }
        let decoded = sizedparse::base64_decode(encoded, true).map_err(|(off, _)| {
            invalid(
                path,
                &format!("invalid base64: illegal base64 data at input byte {off}"),
            )
        })?;
        if decoded.len() != n {
            return Err(invalid(path, "invalid base64: decoded length mismatch"));
        }
        self.b
            .add_binary(n as i64)
            .map_err(|e| XErr::at("decode", path, e))?;
        out.extend_from_slice(&decoded);
        let padded = encoded.contains(&b'=');
        encoded.clear();
        Ok(padded)
    }
    fn parse_bulk_data(
        &mut self,
        start: &Name,
        attrs: &[xmlwire::Attr],
        tag: Tag,
        vr: Vr,
        path: &str,
    ) -> Result<BulkDataReference, XErr> {
        let mut r = BulkDataReference {
            tag,
            vr,
            uri: String::new(),
            uuid: String::new(),
        };
        for a in attrs {
            if !a.name.space.is_empty() {
                return Err(invalid(
                    path,
                    "namespaced BulkData attribute is not allowed",
                ));
            }
            match a.name.local.as_str() {
                "uri" => {
                    if !r.uri.is_empty() {
                        return Err(invalid(path, "duplicate uri"));
                    }
                    r.uri = a.value.clone();
                }
                "uuid" => {
                    if !r.uuid.is_empty() {
                        return Err(invalid(path, "duplicate uuid"));
                    }
                    r.uuid = a.value.clone();
                }
                other => {
                    return Err(invalid(
                        path,
                        &format!("unexpected BulkData attribute {other}"),
                    ));
                }
            }
        }
        if r.uri.is_empty() == r.uuid.is_empty() {
            return Err(invalid(path, "exactly one of uri or uuid is required"));
        }
        if !r.uuid.is_empty() && !is_uuid(&r.uuid) {
            return Err(invalid(path, "invalid UUID"));
        }
        if !r.uri.is_empty() {
            if r.uri.chars().any(char::is_control) {
                return Err(invalid(path, "URI contains control characters"));
            }
            if let Err(e) = urlrules::parse(&r.uri) {
                return Err(invalid(path, &format!("invalid URI: {e}")));
            }
        }
        loop {
            match self.token(path)? {
                Tok::CharData(d) => {
                    if !trim_is_empty(&d) {
                        return Err(invalid(path, "BulkData must be empty"));
                    }
                }
                Tok::Comment(_) => {}
                Tok::End(name) => {
                    if name != *start {
                        return Err(invalid(path, "unexpected closing element"));
                    }
                    return Ok(r);
                }
                _ => return Err(invalid(path, "BulkData must be empty")),
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn attribute_element(
        &mut self,
        tag: Tag,
        vr: Vr,
        values: Vec<String>,
        persons: Vec<[Option<[String; 5]>; 3]>,
        items: Vec<DataSet>,
        inline: Option<Vec<u8>>,
        bulk: Option<BulkDataReference>,
        path: &str,
    ) -> Result<Element, XErr> {
        let mut elem = Element {
            header: ElementHeader {
                tag,
                vr,
                length: 0,
                length_set: false,
            },
            value: Value::Nil,
        };
        if let Some(bulk) = bulk {
            if self.opts.bulk_data_policy == BulkDataPolicy::Reject {
                return Err(XErr::at(
                    "decode",
                    &format!("{path}/BulkData"),
                    ERR_BULK_DATA_REJECTED,
                ));
            }
            let uri = if bulk.uri.is_empty() {
                format!("urn:uuid:{}", bulk.uuid)
            } else {
                bulk.uri
            };
            elem.value = Value::BulkData(uri);
        } else if let Some(inline) = inline {
            let encapsulated = self.opts.transfer_syntax.filter(|s| s.encapsulated);
            if tag == Tag::PIXEL_DATA && encapsulated.is_none() && looks_encapsulated(&inline) {
                return Err(XErr::at(
                    "decode",
                    &format!("{path}/InlineBinary"),
                    ERR_TRANSFER_SYNTAX_REQUIRED,
                ));
            }
            if tag == Tag::PIXEL_DATA
                && let Some(syntax) = encapsulated
            {
                let (offset_table, fragments) = self
                    .decode_fragments(&inline, syntax)
                    .map_err(|e| XErr::at("decode", &format!("{path}/InlineBinary"), e))?;
                elem.value = Value::Fragments {
                    offset_table,
                    fragments,
                };
                elem.header.length = UNDEFINED_LENGTH;
                elem.header.length_set = true;
            } else {
                elem.value = Value::Raw(inline);
            }
        } else if vr == Vr::SQ {
            elem.value = Value::Sequence(items);
        } else if vr == Vr::PN {
            let mut out = Vec::with_capacity(persons.len());
            for pn in persons {
                let mut parts: [String; 3] = Default::default();
                let mut last = 0;
                for ((count, group), part) in (1usize..).zip(pn.iter()).zip(parts.iter_mut()) {
                    if let Some(g) = group {
                        let n = g
                            .iter()
                            .rposition(|c| !c.is_empty())
                            .map_or(0, |i| i.saturating_add(1));
                        *part = g.get(..n).unwrap_or_default().join("^");
                        if !part.is_empty() {
                            last = count;
                        }
                    }
                }
                let value = parts.get(..last).unwrap_or_default().join("=");
                if value.contains('\\') {
                    return Err(invalid(
                        &format!("{path}/PersonName"),
                        "VR PN value contains the backslash value delimiter",
                    ));
                }
                out.push(value);
            }
            elem.value = Value::Strings(out);
        } else if vr.is_string_like() {
            if !vr.uses_text_value_delimiter() && values.len() > 1 {
                return Err(invalid(
                    &format!("{path}/Value"),
                    &format!("VR {vr} holds one value, got {}", values.len()),
                ));
            }
            for (number, value) in (1usize..).zip(values.iter()) {
                let vp = format!("{path}/Value[{number}]");
                if vr.uses_text_value_delimiter() && value.contains('\\') {
                    return Err(invalid(
                        &vp,
                        &format!("VR {vr} value contains the backslash value delimiter"),
                    ));
                }
                if (vr == Vr::DS || vr == Vr::IS)
                    && !value.is_empty()
                    && let Err(e) = valueencode::validate_number_string(vr, value)
                {
                    return Err(invalid(&vp, &e));
                }
            }
            elem.value = Value::Strings(values);
        } else if values.is_empty() {
            elem.value = Value::Raw(Vec::new());
        } else {
            elem.value = Value::Raw(self.decode_numeric_values(vr, &values, path)?);
        }
        Ok(elem)
    }
    fn decode_numeric_values(
        &mut self,
        vr: Vr,
        values: &[String],
        path: &str,
    ) -> Result<Vec<u8>, XErr> {
        let width = match vr {
            Vr::OB => 1,
            Vr::SS | Vr::US | Vr::OW => 2,
            Vr::AT | Vr::FL | Vr::OF | Vr::SL | Vr::UL | Vr::OL => 4,
            Vr::FD | Vr::OD | Vr::SV | Vr::UV | Vr::OV => 8,
            _ => {
                return Err(invalid(
                    &format!("{path}/Value"),
                    &format!("Value is not supported for VR {vr}"),
                ));
            }
        };
        let big = self.big;
        let mut data = Vec::with_capacity(values.len().saturating_mul(width));
        let bits = (width as u32).saturating_mul(8);
        for (number, value) in (1usize..).zip(values.iter()) {
            let parsed: Result<(), String> = match vr {
                Vr::AT => parse_core_tag(value).map(|t| {
                    put(&mut data, u64::from(t.group), 2, big);
                    put(&mut data, u64::from(t.element), 2, big);
                }),
                Vr::FL | Vr::OF | Vr::FD | Vr::OD => {
                    let n = match value.to_lowercase().as_str() {
                        "nan" => Ok(crate::dicomjson::GO_NAN),
                        "inf" | "+inf" | "infinity" | "+infinity" => Ok(f64::INFINITY),
                        "-inf" | "-infinity" => Ok(f64::NEG_INFINITY),
                        _ if width == 4 => sizedparse::parse_float32(value),
                        _ => crate::numtext::parse_float(value).map_err(|e| e.to_string()),
                    };
                    n.map(|n| {
                        if width == 4 {
                            put(&mut data, u64::from((n as f32).to_bits()), 4, big);
                        } else {
                            put(&mut data, n.to_bits(), 8, big);
                        }
                    })
                }
                Vr::SS | Vr::SL | Vr::SV => {
                    sizedparse::parse_int(value, bits).map(|n| put(&mut data, n as u64, width, big))
                }
                _ => sizedparse::parse_uint(value, bits).map(|n| put(&mut data, n, width, big)),
            };
            if let Err(e) = parsed {
                return Err(invalid(
                    &format!("{path}/Value[{number}]"),
                    &format!("invalid {vr} value: {e}"),
                ));
            }
        }
        Ok(data)
    }
    fn decode_fragments(
        &mut self,
        data: &[u8],
        syntax: Syntax,
    ) -> Result<(Vec<u8>, Vec<Vec<u8>>), String> {
        let remaining = self
            .b
            .limits
            .max_sequence_items
            .saturating_sub(self.b.items);
        let limit = if remaining == 0 { 1 } else { remaining };
        let (offset_table, fragments) =
            read_encapsulated(data, syntax, limit).map_err(|(e, frag)| {
                if frag {
                    format!("{ERR_MAX_ITEMS}: {e}")
                } else if e.starts_with("expected exactly one") {
                    format!("{ERR_INVALID_XML}: expected one encapsulated Pixel Data element")
                } else if e.starts_with("got ") {
                    format!("{ERR_INVALID_XML}: expected encapsulated fragments")
                } else {
                    format!("{ERR_INVALID_XML}: encapsulated Pixel Data: {e}")
                }
            })?;
        if fragments.len() as i64 > remaining {
            return Err(ERR_MAX_ITEMS.to_string());
        }
        self.b.items = self.b.items.saturating_add(fragments.len() as i64);
        Ok((offset_table, fragments))
    }
    fn assemble_data_set(
        &mut self,
        attrs: Vec<Attribute>,
        path: &str,
    ) -> Result<Vec<Element>, XErr> {
        let mut alloc = PrivateAllocator::new(&attrs);
        let mut elements: Vec<Element> = Vec::with_capacity(attrs.len());
        for attr in attrs {
            let mut elem = attr.element;
            let mut tag = elem.tag();
            if !attr.private_creator.is_empty() {
                tag = alloc
                    .allocate(tag, &attr.private_creator)
                    .map_err(|e| XErr::at("decode", path, e))?;
                elem.header.tag = tag;
                let creator_tag = Tag::new(tag.group, tag.element >> 8);
                if !elements.iter().any(|e| e.tag() == creator_tag) {
                    elements.push(Element {
                        header: ElementHeader {
                            tag: creator_tag,
                            vr: Vr::LO,
                            length: 0,
                            length_set: false,
                        },
                        value: Value::Strings(vec![attr.private_creator.clone()]),
                    });
                }
            }
            if let Some(existing) = elements.iter().find(|e| e.tag() == tag) {
                let same_creator = attr.private_creator.is_empty()
                    && dict::is_private_creator(tag)
                    && elem.vr() == Vr::LO
                    && existing.vr() == Vr::LO
                    && matches!((&elem.value, &existing.value),
                        (Value::Strings(a), Value::Strings(b)) if matches!((a.as_slice(), b.as_slice()), ([x], [y]) if x == y));
                if same_creator {
                    continue;
                }
                return Err(invalid(
                    path,
                    &format!("duplicate resolved tag {}", hex_tag(tag)),
                ));
            }
            elements.push(elem);
        }
        if elements.len() as i64
            > self
                .b
                .limits
                .max_elements
                .saturating_sub(self.b.final_elements)
        {
            return Err(XErr::at("decode", path, ERR_MAX_ELEMENTS));
        }
        self.b.final_elements = self.b.final_elements.saturating_add(elements.len() as i64);
        elements.sort_by_key(|e| e.tag());
        Ok(elements)
    }
    fn resolve_object(&mut self, obj: &mut Object) -> Result<(), XErr> {
        let elements = obj.sorted_elements();
        let bits = elements_bits_allocated(&elements, self.big);
        for elem in elements {
            let path = format!(
                "/NativeDicomModel/DicomAttribute[@tag='{}']",
                hex_tag(elem.tag())
            );
            let resolved = self.resolve_element(elem, &path, bits)?;
            obj.put(resolved);
        }
        Ok(())
    }
    fn resolve_element(
        &mut self,
        mut elem: Element,
        path: &str,
        bits: i64,
    ) -> Result<Element, XErr> {
        match elem.value {
            Value::BulkData(uri) => {
                let mut r = BulkDataReference {
                    tag: elem.header.tag,
                    vr: elem.header.vr,
                    uri: uri.clone(),
                    uuid: String::new(),
                };
                if uri.to_lowercase().starts_with("urn:uuid:") && uri.is_char_boundary(9) {
                    r.uuid = uri[9..].to_string();
                    r.uri = String::new();
                }
                let Some(resolver) = self.opts.bulk_data_resolver.as_mut() else {
                    return Err(XErr::at(
                        "decode",
                        path,
                        "bulk data reference without a resolver",
                    ));
                };
                let data = match resolver(&r) {
                    Err(e) => {
                        return Err(XErr::at(
                            "decode",
                            path,
                            format!("{ERR_BULK_DATA_RESOLVER}: {e}"),
                        ));
                    }
                    Ok(None) => {
                        return Err(XErr::at(
                            "decode",
                            path,
                            format!("{ERR_BULK_DATA_RESOLVER}: resolver returned nil reader"),
                        ));
                    }
                    Ok(Some(d)) => d,
                };
                let remaining = self
                    .b
                    .limits
                    .max_total_binary_bytes
                    .saturating_sub(self.b.binary)
                    .max(0);
                if data.len() as i64 > remaining {
                    return Err(XErr::at("decode", path, ERR_MAX_BINARY_BYTES));
                }
                self.b
                    .add_binary(data.len() as i64)
                    .map_err(|e| XErr::at("decode", path, e))?;
                if elem.header.tag == Tag::PIXEL_DATA && looks_encapsulated(&data) {
                    let Some(syntax) = self.opts.transfer_syntax.filter(|s| s.encapsulated) else {
                        return Err(XErr::at("decode", path, ERR_TRANSFER_SYNTAX_REQUIRED));
                    };
                    let (offset_table, fragments) = self
                        .decode_fragments(&data, syntax)
                        .map_err(|e| XErr::at("decode", path, e))?;
                    elem.value = Value::Fragments {
                        offset_table,
                        fragments,
                    };
                    elem.header.length = UNDEFINED_LENGTH;
                    elem.header.length_set = true;
                    return Ok(elem);
                }
                let w = match elem.header.vr {
                    Vr::AT | Vr::FL | Vr::OF | Vr::SL | Vr::UL | Vr::OL => 4,
                    Vr::FD | Vr::OD | Vr::SV | Vr::UV | Vr::OV => 8,
                    Vr::SS | Vr::US | Vr::OW => 2,
                    _ => 1,
                };
                if !data.len().is_multiple_of(w) {
                    return Err(XErr::at(
                        "decode",
                        path,
                        format!(
                            "{ERR_BULK_DATA_RESOLVER}: resolved VR {} length {} is not a multiple of {w}",
                            elem.header.vr,
                            data.len()
                        ),
                    ));
                }
                elem.header.length = data.len() as u32;
                elem.header.length_set = true;
                elem.value = Value::Raw(data);
                self.to_value_byte_order(&mut elem, bits);
                Ok(elem)
            }
            Value::Sequence(mut items) => {
                for (number, item) in (1usize..).zip(items.iter_mut()) {
                    let item_bits = elements_bits_allocated(&item.elements, self.big);
                    for slot in item.elements.iter_mut() {
                        let nested = std::mem::replace(
                            slot,
                            Element {
                                header: ElementHeader::new(Tag::default(), Vr::UN, 0),
                                value: Value::Nil,
                            },
                        );
                        let p = format!(
                            "{path}/Item[{number}]/DicomAttribute[@tag='{}']",
                            hex_tag(nested.tag())
                        );
                        *slot = self.resolve_element(nested, &p, item_bits)?;
                    }
                }
                elem.value = Value::Sequence(items);
                Ok(elem)
            }
            other => {
                elem.value = other;
                Ok(elem)
            }
        }
    }
}
fn elements_bits_allocated(elements: &[Element], big: bool) -> i64 {
    elements
        .iter()
        .find(|e| e.tag() == valueencode::BITS_ALLOCATED_TAG)
        .map_or(0, |e| valueencode::bits_allocated(e, big))
}
fn looks_encapsulated(data: &[u8]) -> bool {
    matches!(
        data.first_chunk::<4>(),
        Some([0xfe, 0xff, 0x00, 0xe0] | [0xff, 0xfe, 0xe0, 0x00])
    )
}
fn inline_vr(vr: Vr) -> bool {
    matches!(
        vr,
        Vr::OB | Vr::OD | Vr::OF | Vr::OL | Vr::OV | Vr::OW | Vr::UN
    )
}
fn is_tag_pattern(s: &str) -> bool {
    s.len() == 8
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
}
fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
fn validate_number(attrs: &[xmlwire::Attr], expected: usize, path: &str) -> Result<(), XErr> {
    let [attr] = attrs else {
        return Err(invalid(path, "requires exactly one number attribute"));
    };
    if !attr.name.space.is_empty() || attr.name.local != "number" {
        return Err(invalid(path, "requires exactly one number attribute"));
    }
    let v = &attr.value;
    match sizedparse::parse_int(v, 64) {
        Ok(n) if n == expected as i64 => Ok(()),
        _ => Err(invalid(
            path,
            &format!(
                "number must be monotonically increasing; got {}, want {expected}",
                crate::textfmt::quote(v)
            ),
        )),
    }
}
struct PrivateAllocator {
    by_group: Vec<(u16, Vec<(String, u16)>)>,
    used: Vec<(u16, Vec<u16>)>,
}
impl PrivateAllocator {
    fn new(attrs: &[Attribute]) -> PrivateAllocator {
        let mut a = PrivateAllocator {
            by_group: Vec::new(),
            used: Vec::new(),
        };
        for attr in attrs {
            let Ok(tag) = parse_core_tag(&attr.tag) else {
                continue;
            };
            if !attr.private_creator.is_empty() || !dict::is_private_creator(tag) {
                continue;
            }
            a.group(tag.group);
            a.used_mut(tag.group).push(tag.element);
            if attr.element.vr() == Vr::LO
                && let Value::Strings(values) = &attr.element.value
                && let [creator] = values.as_slice()
            {
                let g = a.by_group_mut(tag.group);
                if !g.iter().any(|(c, _)| c == creator) {
                    g.push((creator.clone(), tag.element));
                }
            }
        }
        a
    }
    fn group(&mut self, group: u16) {
        if !self.by_group.iter().any(|(g, _)| *g == group) {
            self.by_group.push((group, Vec::new()));
            self.used.push((group, Vec::new()));
        }
    }
    #[allow(clippy::indexing_slicing)]
    fn by_group_mut(&mut self, group: u16) -> &mut Vec<(String, u16)> {
        let index = match self.by_group.iter().position(|(g, _)| *g == group) {
            Some(index) => index,
            None => {
                self.by_group.push((group, Vec::new()));
                self.by_group.len().saturating_sub(1)
            }
        };
        &mut self.by_group[index].1
    }
    #[allow(clippy::indexing_slicing)]
    fn used_mut(&mut self, group: u16) -> &mut Vec<u16> {
        let index = match self.used.iter().position(|(g, _)| *g == group) {
            Some(index) => index,
            None => {
                self.used.push((group, Vec::new()));
                self.used.len().saturating_sub(1)
            }
        };
        &mut self.used[index].1
    }
    fn allocate(&mut self, normalized: Tag, creator: &str) -> Result<Tag, String> {
        let group = normalized.group;
        self.group(group);
        let existing = self
            .by_group_mut(group)
            .iter()
            .find(|(c, _)| c == creator)
            .map(|(_, b)| *b);
        let block = match existing {
            Some(b) => b,
            None => {
                let used = self.used_mut(group);
                let Some(b) = (0x10..=0xffu16).find(|c| !used.contains(c)) else {
                    return Err(format!(
                        "{ERR_INVALID_XML}: no private creator block available in group {group:04X}"
                    ));
                };
                used.push(b);
                self.by_group_mut(group).push((creator.to_string(), b));
                b
            }
        };
        Ok(Tag::new(group, block << 8 | normalized.element & 0xff))
    }
}
mod urlrules {
    fn is_hex(c: u8) -> bool {
        c.is_ascii_hexdigit()
    }
    fn escape_error(s: &str, i: usize) -> String {
        let end = i.saturating_add(3).min(s.len());
        let bytes = s.as_bytes().get(i..end).unwrap_or_default();
        format!(
            "invalid URL escape {}",
            crate::object::error::quote_bytes(bytes)
        )
    }
    fn check_escapes(s: &str) -> Result<(), String> {
        let b = s.as_bytes();
        let mut i = 0usize;
        while let Some(&c) = b.get(i) {
            if c == b'%' {
                match b.get(i.saturating_add(1)..i.saturating_add(3)) {
                    Some(&[h, l]) if is_hex(h) && is_hex(l) => {}
                    _ => return Err(escape_error(s, i)),
                }
                i = i.saturating_add(3);
            } else {
                i = i.saturating_add(1);
            }
        }
        Ok(())
    }
    fn host_escape_error(s: &str) -> Result<(), String> {
        let b = s.as_bytes();
        let mut i = 0usize;
        while let Some(&c) = b.get(i) {
            match c {
                b'%' => {
                    match b.get(i.saturating_add(1)..i.saturating_add(3)) {
                        Some(&[h, l]) if is_hex(h) && is_hex(l) => {}
                        _ => return Err(escape_error(s, i)),
                    }
                    i = i.saturating_add(3);
                }
                c if c < 0x80 && should_escape_host(c) => {
                    return Err(format!(
                        "invalid character {} in host name",
                        crate::textfmt::quote(&char::from(c).to_string())
                    ));
                }
                _ => i = i.saturating_add(1),
            }
        }
        Ok(())
    }
    fn should_escape_host(c: u8) -> bool {
        if c.is_ascii_alphanumeric() {
            return false;
        }
        !matches!(
            c,
            b'-' | b'_'
                | b'.'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'['
                | b']'
                | b'<'
                | b'>'
                | b'"'
        )
    }
    fn parse_host(host: &str) -> Result<(), String> {
        if host.starts_with('[') {
            let Some(i) = host.rfind(']') else {
                return Err("missing ']' in host".to_string());
            };
            let port = host.get(i.saturating_add(1)..).unwrap_or_default();
            if !port.is_empty() && !valid_port(port) {
                return Err(format!(
                    "invalid port {} after host",
                    crate::textfmt::quote(port)
                ));
            }
            return Ok(());
        }
        if let Some(i) = host.rfind(':') {
            let port = &host[i..];
            if !valid_port(port) {
                return Err(format!(
                    "invalid port {} after host",
                    crate::textfmt::quote(port)
                ));
            }
        }
        host_escape_error(host)
    }
    fn valid_port(port: &str) -> bool {
        port.strip_prefix(':')
            .is_some_and(|p| p.bytes().all(|c| c.is_ascii_digit()))
    }
    fn parse_authority(authority: &str) -> Result<(), String> {
        let host = match authority.rsplit_once('@') {
            Some((_, host)) => host,
            None => authority,
        };
        parse_host(host)
    }
    fn get_scheme(raw: &str) -> Result<(&str, &str), String> {
        for (i, c) in raw.bytes().enumerate() {
            match c {
                b'a'..=b'z' | b'A'..=b'Z' => {}
                b'0'..=b'9' | b'+' | b'-' | b'.' => {
                    if i == 0 {
                        return Ok(("", raw));
                    }
                }
                b':' => {
                    if i == 0 {
                        return Err("missing protocol scheme".to_string());
                    }
                    let (scheme, rest) = raw.split_at(i);
                    return Ok((scheme, rest.get(1..).unwrap_or_default()));
                }
                _ => return Ok(("", raw)),
            }
        }
        Ok(("", raw))
    }
    fn parse_inner(raw: &str) -> Result<(), String> {
        if raw == "*" {
            return Ok(());
        }
        let (scheme, rest) = get_scheme(raw)?;
        let rest = if rest.ends_with('?') && rest.matches('?').count() == 1 {
            rest.strip_suffix('?').unwrap_or(rest)
        } else {
            rest.split_once('?').map_or(rest, |(a, _)| a)
        };
        if !rest.starts_with('/') {
            if !scheme.is_empty() {
                return Ok(());
            }
            let segment = rest.split_once('/').map_or(rest, |(a, _)| a);
            if segment.contains(':') {
                return Err("first path segment in URL cannot contain colon".to_string());
            }
        }
        let mut path = rest;
        if (!scheme.is_empty() || !rest.starts_with("///")) && rest.starts_with("//") {
            let authority_rest = &rest[2..];
            let (authority, p) = match authority_rest.find('/') {
                Some(i) => (&authority_rest[..i], &authority_rest[i..]),
                None => (authority_rest, ""),
            };
            parse_authority(authority)?;
            path = p;
        }
        check_escapes(path)
    }
    pub fn parse(raw: &str) -> Result<(), String> {
        let (u, frag) = match raw.split_once('#') {
            Some((u, f)) => (u, Some(f)),
            None => (raw, None),
        };
        if let Err(e) = parse_inner(u) {
            return Err(format!("parse {}: {e}", crate::textfmt::quote(u)));
        }
        if let Some(f) = frag
            && let Err(e) = check_escapes(f)
        {
            return Err(format!("parse {}: {e}", crate::textfmt::quote(raw)));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        let doc = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<NativeDicomModel xmlns=\"http://dicom.nema.org/PS3.19/models/NativeDICOM\" xml:space=\"preserve\"><DicomAttribute tag=\"00100020\" vr=\"LO\" keyword=\"PatientID\"><Value number=\"1\">a&amp;b</Value></DicomAttribute></NativeDicomModel>";
        let obj = unmarshal(doc.as_bytes(), &mut UnmarshalOptions::default()).unwrap();
        let out = marshal(Some(&obj), &mut Options::default()).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), doc);
    }
    #[test]
    fn syntax_error_line() {
        let doc = "<NativeDicomModel xmlns=\"http://dicom.nema.org/PS3.19/models/NativeDICOM\" xml:space=\"preserve\">\n<DicomAttribute tag=\"00100020\" vr=\"LO\" keyword=\"PatientID\"></b></NativeDicomModel>";
        let err = unmarshal(doc.as_bytes(), &mut UnmarshalOptions::default()).unwrap_err();
        assert_eq!(
            err.message(),
            "dicomxml: decode at /NativeDicomModel/DicomAttribute[@tag='00100020']: dicomxml: invalid Native DICOM Model XML: XML syntax error on line 2: element <DicomAttribute> closed by </b>"
        );
    }
}
