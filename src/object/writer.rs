#![forbid(unsafe_code)]
use super::accessors::is_valid_uid;
use super::bulk::BulkDataWriteOptions;
use super::deferred::{ERR_ENCODED_LENGTH_UNAVAILABLE, ValueProvider};
use super::dict;
use super::error::{Error, Kind};
use super::file::File;
use super::obj::{Object, element_length, strings_encoded_length};
use super::text::{raw_string_values, trim_text_value};
use super::transfer::{self, IMPLICIT_VR_LITTLE_ENDIAN, Syntax};
use super::types::{
    DataSet, Element, ElementHeader, Length, Tag, UNDEFINED_LENGTH, Value, Vr, length_string,
};
use crate::charset::SpecificCharacterSet;
pub const IMPLEMENTATION_CLASS_UID: &str = "1.2.826.0.1.3680043.10.543.1";
pub const IMPLEMENTATION_VERSION_NAME: &str = "DCMRUST";
const TAG_FILE_META_GROUP_LENGTH: Tag = Tag::new(0x0002, 0x0000);
const TAG_FILE_META_VERSION: Tag = Tag::new(0x0002, 0x0001);
const TAG_MEDIA_STORAGE_SOP_CLASS_UID: Tag = Tag::new(0x0002, 0x0002);
const TAG_MEDIA_STORAGE_SOP_INSTANCE_UID: Tag = Tag::new(0x0002, 0x0003);
const TAG_TRANSFER_SYNTAX_UID: Tag = Tag::new(0x0002, 0x0010);
const TAG_IMPLEMENTATION_CLASS_UID: Tag = Tag::new(0x0002, 0x0012);
const TAG_IMPLEMENTATION_VERSION_NAME: Tag = Tag::new(0x0002, 0x0013);
const TAG_SOURCE_AE_TITLE: Tag = Tag::new(0x0002, 0x0016);
const TAG_SENDING_AE_TITLE: Tag = Tag::new(0x0002, 0x0017);
const TAG_RECEIVING_AE_TITLE: Tag = Tag::new(0x0002, 0x0018);
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const MEDIA_STORAGE_DIRECTORY_STORAGE_UID: &str = "1.2.840.10008.1.3.10";
pub mod sequence_length {
    pub const UNDEFINED: u8 = 0;
    #[doc(hidden)]
    pub const PRESERVE: u8 = 1;
    pub const DEFINED: u8 = 2;
}
pub mod group_length {
    pub const PRESERVE: u8 = 0;
    pub const RECALCULATE: u8 = 1;
    pub const CREATE: u8 = 2;
    pub const REMOVE: u8 = 3;
}
#[derive(Default)]
pub(crate) struct Out {
    pub(crate) data: Vec<u8>,
    ends: Vec<usize>,
}
impl Out {
    fn extend_from_slice(&mut self, b: &[u8]) {
        if !b.is_empty() {
            self.data.extend_from_slice(b);
            self.ends.push(self.data.len());
        }
    }
    fn push(&mut self, b: u8) {
        self.extend_from_slice(&[b]);
    }
    fn append(&mut self, other: Out) {
        let base = self.data.len();
        self.data.extend_from_slice(&other.data);
        self.ends.extend(other.ends.into_iter().map(|e| e + base));
    }
}
pub mod padding_policy {
    pub const PRESERVE: u8 = 0;
    pub const REMOVE: u8 = 1;
    pub const CREATE: u8 = 2;
}
pub const DEFAULT_MAX_PADDING_BYTES: i64 = 16 << 20;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaddingOptions {
    pub policy: u8,
    pub alignment: u32,
    pub item_alignment: u32,
    pub max_added_bytes: i64,
}
pub fn validate_padding_options(opts: &PaddingOptions) -> Result<(), Error> {
    if opts.policy > padding_policy::CREATE {
        return Err(Error::msg(format!(
            "dicom: invalid padding policy {}",
            opts.policy
        )));
    }
    if opts.max_added_bytes < 0 {
        return Err(Error::msg("dicom: padding budget must not be negative"));
    }
    for alignment in [opts.alignment, opts.item_alignment] {
        if alignment % 2 != 0 {
            return Err(Error::msg(
                "dicom: padding alignment must be zero or an even number",
            ));
        }
        if alignment != 0 && opts.policy != padding_policy::CREATE {
            return Err(Error::msg(
                "dicom: padding alignment requires PaddingCreate",
            ));
        }
    }
    Ok(())
}
const TAG_DATA_SET_PADDING: Tag = Tag::new(0xFFFC, 0xFFFC);
const TAG_PADDING_GROUP_LENGTH: Tag = Tag::new(0xFFFC, 0x0000);
const ERR_PADDING_BUDGET: &str = "dicom: generated padding exceeds MaxAddedBytes";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SequenceWriteLimits {
    pub max_depth: i64,
    pub max_plan_nodes: i64,
    pub max_plan_bytes: i64,
    pub max_retained_value_bytes: i64,
}
pub const DEFAULT_MAX_WRITE_DEPTH: i64 = 128;
pub const DEFAULT_MAX_WRITE_PLAN_NODES: i64 = 1_000_000;
pub const DEFAULT_MAX_WRITE_PLAN_BYTES: i64 = 64 << 20;
pub const DEFAULT_MAX_RETAINED_VALUE_BYTES: i64 = 32 << 20;
const DEFAULT_SEQUENCE_LIMITS: SequenceWriteLimits = SequenceWriteLimits {
    max_depth: DEFAULT_MAX_WRITE_DEPTH,
    max_plan_nodes: DEFAULT_MAX_WRITE_PLAN_NODES,
    max_plan_bytes: DEFAULT_MAX_WRITE_PLAN_BYTES,
    max_retained_value_bytes: DEFAULT_MAX_RETAINED_VALUE_BYTES,
};
const PLANNED_ELEMENT_BYTES: i64 = 232;
const PLANNED_ITEM_BYTES: i64 = 136;
const ELEMENT_BYTES: i64 = 48;
const DATA_SET_BYTES: i64 = 40;
const STRING_BYTES: i64 = 16;
const SLICE_BYTES: i64 = 24;
const GROUP_SCRATCH_BYTES: i64 = 3 * ELEMENT_BYTES + 96;
fn write_plan_limit() -> Error {
    Error::sentinel(Kind::WritePlanLimit)
}
fn normalize_sequence_limits(l: SequenceWriteLimits) -> Result<SequenceWriteLimits, Error> {
    if l.max_depth < 0
        || l.max_plan_nodes < 0
        || l.max_plan_bytes < 0
        || l.max_retained_value_bytes < 0
    {
        return Err(Error::kind_suffix(
            Kind::WritePlanLimit,
            ": limits must not be negative",
        ));
    }
    let or = |v: i64, default: i64| if v == 0 { default } else { v };
    Ok(SequenceWriteLimits {
        max_depth: or(l.max_depth, DEFAULT_MAX_WRITE_DEPTH),
        max_plan_nodes: or(l.max_plan_nodes, DEFAULT_MAX_WRITE_PLAN_NODES),
        max_plan_bytes: or(l.max_plan_bytes, DEFAULT_MAX_WRITE_PLAN_BYTES),
        max_retained_value_bytes: or(l.max_retained_value_bytes, DEFAULT_MAX_RETAINED_VALUE_BYTES),
    })
}
pub fn validate_sequence_write_limits(limits: SequenceWriteLimits) -> Result<(), Error> {
    normalize_sequence_limits(limits).map(|_| ())
}
fn checked_write_product(n: i64, width: i64) -> Result<i64, Error> {
    if n < 0 {
        return Err(write_plan_limit());
    }
    n.checked_mul(width).ok_or_else(write_plan_limit)
}
fn checked_write_add(a: i64, b: i64) -> Result<i64, Error> {
    a.checked_add(b)
        .filter(|_| a >= 0 && b >= 0)
        .ok_or_else(|| Error::msg("dicom: encoded length overflow"))
}
#[derive(Clone, Copy)]
struct PlanBudget {
    limits: SequenceWriteLimits,
    nodes: i64,
    bytes: i64,
    retained: i64,
    padding: i64,
}
impl PlanBudget {
    fn new(limits: SequenceWriteLimits) -> PlanBudget {
        PlanBudget {
            limits,
            nodes: 0,
            bytes: 0,
            retained: 0,
            padding: 0,
        }
    }
    fn reserve(&mut self, n: i64, width: i64) -> Result<(), Error> {
        if n < 0 || n > self.limits.max_plan_nodes - self.nodes {
            return Err(write_plan_limit());
        }
        let bytes = checked_write_product(n, width)?;
        if bytes > self.limits.max_plan_bytes - self.bytes {
            return Err(write_plan_limit());
        }
        self.nodes += n;
        self.bytes += bytes;
        Ok(())
    }
    fn reserve_bytes(&mut self, n: i64) -> Result<(), Error> {
        if n < 0 || n > self.limits.max_plan_bytes - self.bytes {
            return Err(write_plan_limit());
        }
        self.bytes += n;
        Ok(())
    }
    fn reserve_normalized_value(&mut self, payload: i64, structure: i64) -> Result<(), Error> {
        if payload < 0
            || structure < 0
            || payload > self.limits.max_retained_value_bytes - self.retained
            || structure > self.limits.max_plan_bytes - self.bytes
        {
            return Err(write_plan_limit());
        }
        self.retained += payload;
        self.bytes += structure;
        Ok(())
    }
}
fn admit_clone_structure(
    n: i64,
    width: i64,
    total: &mut i64,
    limits: &SequenceWriteLimits,
) -> Result<(), Error> {
    let bytes = checked_write_product(n, width)?;
    if bytes > limits.max_plan_bytes - *total {
        return Err(write_plan_limit());
    }
    *total += bytes;
    Ok(())
}
fn retained_value_size(
    el: &Element,
    limits: &SequenceWriteLimits,
    depth: i64,
    visited: &mut i64,
    structure: &mut i64,
) -> Result<i64, Error> {
    if depth > limits.max_depth || *visited >= limits.max_plan_nodes {
        return Err(write_plan_limit());
    }
    *visited += 1;
    admit_clone_structure(1, ELEMENT_BYTES, structure, limits)?;
    let n = match &el.value {
        Value::Raw(raw) => raw.len() as i64,
        Value::Strings(values) => {
            admit_clone_structure(values.len() as i64, STRING_BYTES, structure, limits)?;
            let mut n = 0;
            for s in values {
                n = checked_write_add(n, s.len() as i64)?;
            }
            n
        }
        Value::Sequence(items) => retained_items_size(items, limits, depth, visited, structure)?,
        Value::Fragments {
            offset_table,
            fragments,
        } => {
            let count = fragments.len() as i64;
            if count > limits.max_plan_nodes - *visited {
                return Err(write_plan_limit());
            }
            *visited += count;
            admit_clone_structure(count, SLICE_BYTES, structure, limits)?;
            let mut n = offset_table.len() as i64;
            for fragment in fragments {
                n = checked_write_add(n, fragment.len() as i64)?;
            }
            n
        }
        _ => 0,
    };
    if n > limits.max_retained_value_bytes {
        return Err(write_plan_limit());
    }
    Ok(n)
}
fn retained_items_size(
    items: &[DataSet],
    limits: &SequenceWriteLimits,
    depth: i64,
    visited: &mut i64,
    structure: &mut i64,
) -> Result<i64, Error> {
    let count = items.len() as i64;
    if count > limits.max_plan_nodes - *visited {
        return Err(write_plan_limit());
    }
    *visited += count;
    admit_clone_structure(count, DATA_SET_BYTES, structure, limits)?;
    let mut n = 0;
    for item in items {
        if depth + 1 > limits.max_depth {
            return Err(write_plan_limit());
        }
        for child in &item.elements {
            let size = retained_value_size(child, limits, depth + 2, visited, structure)?;
            n = checked_write_add(n, size)?;
        }
    }
    Ok(n)
}
#[derive(Clone, Copy, Default)]
struct PaddingLayout {
    value: i64,
    added: i64,
    group_length: bool,
}
pub mod file_meta_mode {
    pub const UPDATE: i64 = 0;
    pub const CREATE: i64 = 1;
    pub const PRESERVE: i64 = 2;
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMetaAeTitles {
    pub source: String,
    pub sending: String,
    pub receiving: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteFileOptions {
    pub sequence_length: u8,
    pub group_length: u8,
    pub meta_mode: i64,
    pub ae_titles: FileMetaAeTitles,
    pub keep_partial_byte_order_units: bool,
    pub preamble: Option<Vec<u8>>,
    pub default_missing_transfer_syntax: bool,
    pub override_missing_transfer_syntax: String,
    pub omit_reconciled_data_set_sop_uids: bool,
    pub allow_partial: bool,
    pub deflate_level: Option<i64>,
    pub padding: PaddingOptions,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteDataSetOptions {
    pub sequence_length: u8,
    pub group_length: u8,
    pub deflate_level: Option<i64>,
    pub padding: PaddingOptions,
}
pub fn validate_serialization_options(
    uid: &str,
    level: Option<i64>,
    padding: &PaddingOptions,
) -> Result<(), Error> {
    let syntax = writable_syntax(uid)?;
    validate_padding_options(padding)?;
    if let Some(level) = level {
        if !(0..=9).contains(&level) {
            return Err(Error::msg("dicom: DeflateLevel must be between 0 and 9"));
        }
        if !syntax.deflated {
            return Err(Error::msg(
                "dicom: DeflateLevel requires Deflated Explicit VR Little Endian",
            ));
        }
    }
    Ok(())
}
fn serialization_level(
    syntax: &Syntax,
    level: Option<i64>,
    padding: &PaddingOptions,
) -> Result<i32, Error> {
    validate_serialization_options(syntax.uid, level, padding)?;
    Ok(level.map_or(crate::deflater::DEFAULT_COMPRESSION, |l| l as i32))
}
fn padded_deflate(data: &Out, level: i32) -> Result<Vec<u8>, Error> {
    let mut out =
        crate::deflater::compress_chunks(&data.data, &data.ends, level).map_err(Error::msg)?;
    if out.len() % 2 == 1 {
        out.push(0);
    }
    Ok(out)
}
#[derive(Clone, Copy)]
enum Op {
    Length,
    Value,
}
impl Op {
    fn text(self) -> &'static str {
        match self {
            Op::Length => "write length",
            Op::Value => "write value",
        }
    }
}
fn write_error(op: Op, tag: Tag, vr: Vr, length: Length, err: Error) -> Error {
    let mut msg = format!("dicom: {}", op.text());
    if !tag.is_zero() {
        msg.push_str(&format!(" for tag {tag}"));
    }
    msg.push_str(&format!(" with VR {vr}"));
    if length != 0 || length == UNDEFINED_LENGTH || matches!(op, Op::Length | Op::Value) {
        msg.push_str(&format!(" with length {}", length_string(length)));
    }
    msg.push_str(&format!(": {err}"));
    Error::relabel(&msg, err)
}
const ERR_VALUE_NOT_REPRESENTABLE: &str = "dicom: value is not representable in its VR";
const ERR_RESOLVE_BULK_DATA_SOURCE: &str = "dicom: resolve Bulk Data source";
fn bulk_data_encoded_length(size: i64) -> Result<Length, Error> {
    if size < 0 {
        return Err(Error::msg(format!(
            "dicom: Bulk Data source size {size} is negative"
        )));
    }
    if size >= i64::from(UNDEFINED_LENGTH) {
        return Err(Error::msg(format!(
            "dicom: Bulk Data source size {size} exceeds defined length"
        )));
    }
    let encoded = size + size % 2;
    if encoded >= i64::from(UNDEFINED_LENGTH) {
        return Err(Error::msg(format!(
            "dicom: Bulk Data encoded length {encoded} uses or exceeds the undefined-length sentinel"
        )));
    }
    Ok(encoded as Length)
}
#[derive(Clone)]
struct Writer {
    explicit: bool,
    big: bool,
    sequence_length: u8,
    group_length: u8,
    charset: SpecificCharacterSet,
    padding: PaddingOptions,
    padding_budget: std::rc::Rc<std::cell::Cell<i64>>,
    measuring: bool,
    in_item: bool,
    planned: bool,
    bulk: BulkDataWriteOptions,
    encapsulated: bool,
    deferred: Option<std::rc::Rc<DeferredWrite>>,
    item_offset: Option<i64>,
    item_sample_width: usize,
    limits: SequenceWriteLimits,
    sequence_depth: i64,
    shared_budget: Option<std::rc::Rc<std::cell::RefCell<PlanBudget>>>,
}
struct DeferredWrite {
    provider: Option<std::sync::Arc<ValueProvider>>,
    ambiguous: Vec<Tag>,
    reverse: bool,
    target_big: bool,
    keep_partial: bool,
}
struct DeferredCopy {
    data: Vec<u8>,
    copied: i64,
    err: Option<Error>,
    width: usize,
}
impl Writer {
    fn new(
        explicit: bool,
        big: bool,
        sequence_length: u8,
        group_length: u8,
        charset: SpecificCharacterSet,
        padding: PaddingOptions,
    ) -> Writer {
        let limit = if padding.max_added_bytes == 0 {
            DEFAULT_MAX_PADDING_BYTES
        } else {
            padding.max_added_bytes
        };
        Writer {
            explicit,
            big,
            sequence_length,
            group_length,
            charset,
            padding,
            padding_budget: std::rc::Rc::new(std::cell::Cell::new(limit)),
            measuring: false,
            in_item: false,
            planned: false,
            bulk: BulkDataWriteOptions::default(),
            encapsulated: false,
            deferred: None,
            item_offset: None,
            item_sample_width: 0,
            limits: DEFAULT_SEQUENCE_LIMITS,
            sequence_depth: 0,
            shared_budget: None,
        }
    }
    fn check_write_depth(&self, depth: i64) -> Result<(), Error> {
        if depth > self.limits.max_depth {
            return Err(Error::kind_suffix(
                Kind::WritePlanLimit,
                &format!(
                    ": combined sequence/item depth exceeds {}",
                    self.limits.max_depth
                ),
            ));
        }
        Ok(())
    }
    fn admit_group_scratch(&self, n: usize) -> Result<(), Error> {
        let bytes = checked_write_product(n as i64, GROUP_SCRATCH_BYTES)?;
        if n as i64 > self.limits.max_plan_nodes || bytes > self.limits.max_plan_bytes {
            return Err(write_plan_limit());
        }
        Ok(())
    }
    fn item_footprint(&self, item: &DataSet) -> Result<(i64, i64), Error> {
        let limits = &self.limits;
        if self.sequence_depth > limits.max_depth || limits.max_plan_nodes <= 0 {
            return Err(write_plan_limit());
        }
        let (mut visited, mut structure) = (1, 0);
        admit_clone_structure(1, ELEMENT_BYTES, &mut structure, limits)?;
        let payload = retained_items_size(
            std::slice::from_ref(item),
            limits,
            self.sequence_depth,
            &mut visited,
            &mut structure,
        )?;
        if payload > limits.max_retained_value_bytes {
            return Err(write_plan_limit());
        }
        Ok((payload, structure))
    }
    fn nested_deferred(&self, el: &Element) -> bool {
        self.in_item
            && matches!(el.value, Value::Nil)
            && el.vr() != Vr::SQ
            && el.header.length_set
            && el.header.length != UNDEFINED_LENGTH
            && el.header.length > 0
            && self.deferred.as_ref().is_some_and(|d| d.provider.is_some())
    }
    fn deferred_copy(
        &self,
        el: &Element,
        sample_width: usize,
        copy: impl FnOnce(&mut Vec<u8>) -> (i64, Option<Error>),
    ) -> DeferredCopy {
        let mut width = 1;
        let reverse = self.deferred.as_ref().is_some_and(|d| d.reverse);
        let keep_partial = self.deferred.as_ref().is_some_and(|d| d.keep_partial);
        if reverse && el.header.length_set && el.header.length != UNDEFINED_LENGTH {
            let mut unit = byte_order_width(el.vr());
            if el.tag() == Tag::PIXEL_DATA && el.vr() == Vr::OW && sample_width > unit {
                unit = sample_width;
            }
            if unit > 1 {
                if !(el.header.length as usize).is_multiple_of(unit) && !keep_partial {
                    return DeferredCopy {
                        data: Vec::new(),
                        copied: 0,
                        err: Some(Error::msg(format!(
                            "dicom: cannot change the byte order of {} with VR {}: value length is not a multiple of {unit}",
                            el.tag(),
                            el.vr()
                        ))),
                        width: 1,
                    };
                }
                width = unit;
            }
        }
        let mut data = Vec::new();
        let (copied, err) = copy(&mut data);
        if width > 1 {
            for chunk in data.chunks_exact_mut(width) {
                chunk.reverse();
            }
        }
        DeferredCopy {
            data,
            copied,
            err,
            width,
        }
    }
    fn nested_deferred_copy(&self, el: &Element) -> DeferredCopy {
        let (item_offset, sample_width) = (self.item_offset, self.item_sample_width);
        let provider = self.deferred.as_ref().and_then(|d| d.provider.clone());
        self.deferred_copy(el, sample_width, |w| {
            let tag = el.tag();
            let Some(item_offset) = item_offset else {
                return (
                    0,
                    Some(Error::msg(format!(
                        "{}: element {tag} is in an item without a source offset",
                        super::deferred::ERR_DEFERRED_SEQUENCE_VALUE_WRITE
                    ))),
                );
            };
            match &provider {
                Some(provider) => provider.copy_nested_value_to(item_offset, tag, w),
                None => (0, None),
            }
        })
    }
    fn write_copied(out: &mut Out, copy: &DeferredCopy) {
        for chunk in copy.data.chunks(32 * 1024) {
            let whole = chunk.len() - chunk.len() % copy.width;
            out.extend_from_slice(&chunk[..whole]);
            out.extend_from_slice(&chunk[whole..]);
        }
    }
    fn write_deferred_element(
        &self,
        out: &mut Out,
        el: &Element,
        copy: impl FnOnce() -> DeferredCopy,
    ) -> Result<(), Error> {
        let (tag, vr) = (el.tag(), el.vr());
        if self.padding.policy != padding_policy::PRESERVE && tag == TAG_DATA_SET_PADDING {
            return Ok(());
        }
        if tag == Tag::PIXEL_DATA
            && (vr == Vr::OB || vr == Vr::OW)
            && el.header.length == UNDEFINED_LENGTH
        {
            self.header(out, tag, vr, UNDEFINED_LENGTH)?;
            let copied = copy();
            Self::write_copied(out, &copied);
            return match copied.err {
                Some(err) => Err(write_error(Op::Value, tag, vr, UNDEFINED_LENGTH, err)),
                None => Ok(()),
            };
        }
        if !el.header.length_set || el.header.length == UNDEFINED_LENGTH {
            return Err(write_error(
                Op::Value,
                tag,
                vr,
                element_length(el),
                Error::msg(
                    "dicom: deferred element requires a defined length or encapsulated Pixel Data",
                ),
            ));
        }
        let length = el.header.length;
        self.header(out, tag, vr, length)?;
        let copied = copy();
        Self::write_copied(out, &copied);
        if let Some(err) = copied.err {
            return Err(write_error(Op::Value, tag, vr, length, err));
        }
        if copied.copied != i64::from(length) {
            return Err(write_error(
                Op::Value,
                tag,
                vr,
                length,
                Error::msg(format!(
                    "dicom: deferred element copied {} bytes, want {length}",
                    copied.copied
                )),
            ));
        }
        Ok(())
    }
    fn write_top_level_deferred(
        &self,
        out: &mut Out,
        el: &Element,
        sample_width: usize,
    ) -> Result<(), Error> {
        let tag = el.tag();
        let Some(deferred) = self.deferred.as_ref().filter(|d| d.provider.is_some()) else {
            return Err(Error::msg(format!(
                "dicom: element {tag} has deferred value but no value provider"
            )));
        };
        self.write_deferred_element(out, el, || {
            self.deferred_copy(el, sample_width, |w| {
                if deferred.ambiguous.contains(&tag) {
                    return (
                        0,
                        Some(Error::msg(format!(
                            "dicom: deferred value for element {tag} is ambiguous due to duplicate tags"
                        ))),
                    );
                }
                match &deferred.provider {
                    Some(provider) => provider.copy_value_to(tag, w),
                    None => (0, None),
                }
            })
        })
    }
    fn deferred_encoded_size(&self, el: &Element) -> Result<u64, Error> {
        let n = if el.header.length_set && el.header.length != UNDEFINED_LENGTH {
            i64::from(el.header.length)
        } else {
            match self.deferred.as_ref().and_then(|d| d.provider.as_ref()) {
                Some(provider) => provider.encoded_value_length(el.tag())?,
                None => return Err(Error::msg(ERR_ENCODED_LENGTH_UNAVAILABLE)),
            }
        };
        Ok((self.encoded_header_length(el.vr()) + n) as u64)
    }
    fn encoded_header_length(&self, vr: Vr) -> i64 {
        if self.explicit && vr.uses_long_explicit_length() {
            12
        } else {
            8
        }
    }
    fn padding_layout(
        &self,
        value_bytes: i64,
        prefix: i64,
        original: &[Element],
    ) -> Result<PaddingLayout, Error> {
        let opts = self.padding;
        if opts.policy != padding_policy::CREATE {
            return Ok(PaddingLayout::default());
        }
        let (alignment, prefix) = if self.in_item {
            (opts.item_alignment, 0)
        } else {
            (opts.alignment, prefix)
        };
        if alignment == 0 {
            return Ok(PaddingLayout::default());
        }
        if value_bytes < 0 || prefix < 0 || value_bytes > i64::MAX - prefix {
            return Err(Error::msg("dicom: padding length overflow"));
        }
        let n = value_bytes + prefix;
        let alignment = i64::from(alignment);
        if n % alignment == 0 {
            return Ok(PaddingLayout::default());
        }
        let group_length = self.group_length == group_length::CREATE
            || self.group_length == group_length::RECALCULATE
                && original.iter().any(|e| e.tag() == TAG_PADDING_GROUP_LENGTH);
        let header = self.encoded_header_length(Vr::OB);
        let mut extra = header;
        if group_length {
            extra += self.encoded_header_length(Vr::UL) + 4;
        }
        if n % 2 != 0 {
            return Err(Error::msg(
                "dicom: padding requires an even data set length",
            ));
        }
        let value = (alignment - ((n % alignment + extra) % alignment)) % alignment;
        if value > i64::from(u32::MAX) - 1 || header + value > i64::from(u32::MAX) {
            return Err(Error::msg("dicom: padding exceeds a defined 32-bit length"));
        }
        let added = extra + value;
        let limit = if opts.max_added_bytes == 0 {
            DEFAULT_MAX_PADDING_BYTES
        } else {
            opts.max_added_bytes
        };
        if added > limit || !self.measuring && added > self.padding_budget.get() {
            return Err(Error::msg(ERR_PADDING_BUDGET));
        }
        Ok(PaddingLayout {
            value,
            added,
            group_length,
        })
    }
    fn emit_padding(&mut self, out: &mut Out, plan: PaddingLayout) -> Result<(), Error> {
        if plan.added == 0 {
            return Ok(());
        }
        if !self.measuring {
            if plan.added > self.padding_budget.get() {
                return Err(Error::msg(ERR_PADDING_BUDGET));
            }
            self.padding_budget
                .set(self.padding_budget.get() - plan.added);
        }
        if plan.group_length {
            let n = (self.encoded_header_length(Vr::OB) + plan.value) as u32;
            let raw = if self.big {
                n.to_be_bytes()
            } else {
                n.to_le_bytes()
            };
            let el = Element {
                header: ElementHeader {
                    tag: TAG_PADDING_GROUP_LENGTH,
                    vr: Vr::UL,
                    length: 0,
                    length_set: false,
                },
                value: Value::Raw(raw.to_vec()),
            };
            self.write_element(out, &el)?;
        }
        self.header(out, TAG_DATA_SET_PADDING, Vr::OB, plan.value as Length)?;
        let zeros = [0u8; 4096];
        let mut value = plan.value as usize;
        while value > 0 {
            let n = value.min(zeros.len());
            out.extend_from_slice(&zeros[..n]);
            value -= n;
        }
        Ok(())
    }
    fn write_data_set_padding(
        &mut self,
        out: &mut Out,
        value_bytes: i64,
        prefix: i64,
        original: &[Element],
    ) -> Result<(), Error> {
        let plan = self.padding_layout(value_bytes, prefix, original)?;
        self.emit_padding(out, plan)
    }
    fn u16(&self, out: &mut Out, v: u16) {
        out.extend_from_slice(&if self.big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        });
    }
    fn u32(&self, out: &mut Out, v: u32) {
        out.extend_from_slice(&if self.big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        });
    }
    fn tag(&self, out: &mut Out, tag: Tag) {
        self.u16(out, tag.group);
        self.u16(out, tag.element);
    }
    fn header(&self, out: &mut Out, tag: Tag, vr: Vr, length: Length) -> Result<(), Error> {
        if !self.explicit {
            self.tag(out, tag);
            self.u32(out, length);
            return Ok(());
        }
        if !vr.uses_long_explicit_length() && length > 0xFFFF {
            return Err(write_error(
                Op::Length,
                tag,
                vr,
                length,
                Error::msg(format!(
                    "dicom: explicit VR {vr} length {length} exceeds uint16"
                )),
            ));
        }
        self.tag(out, tag);
        out.extend_from_slice(vr.as_str().as_bytes());
        if vr.uses_long_explicit_length() {
            out.extend_from_slice(&[0, 0]);
            self.u32(out, length);
        } else {
            self.u16(out, length as u16);
        }
        Ok(())
    }
    fn item_header(&self, out: &mut Out, length: Length) {
        self.tag(out, Tag::ITEM);
        self.u32(out, length);
    }
    fn delimiter(&self, out: &mut Out, tag: Tag) {
        self.tag(out, tag);
        self.u32(out, 0);
    }
    fn use_declared_character_set(&mut self, el: &Element) -> Result<(), Error> {
        let values: Vec<String> = match &el.value {
            Value::Raw(raw) => raw_string_values(el.vr(), raw)
                .iter()
                .map(|v| String::from_utf8_lossy(v).into_owned())
                .collect(),
            Value::Strings(values) => values
                .iter()
                .map(|v| trim_text_value(el.vr(), v).to_string())
                .collect(),
            _ => Vec::new(),
        };
        self.charset =
            SpecificCharacterSet::parse(&values).map_err(|e| Error::msg(e.to_string()))?;
        Ok(())
    }
    fn child_for(&self, elements: &[Element]) -> Result<Writer, Error> {
        let mut child = self.clone();
        if let Some(el) = elements
            .iter()
            .find(|e| e.tag() == TAG_SPECIFIC_CHARACTER_SET)
        {
            child.use_declared_character_set(el)?;
        }
        Ok(child)
    }
    fn validate(&self, el: &Element) -> Result<(), Error> {
        let (tag, vr) = (el.tag(), el.vr());
        if tag.is_sequence_delimiting() {
            return Err(write_error(
                Op::Value,
                tag,
                vr,
                UNDEFINED_LENGTH,
                Error::msg("dicom: items and delimiters cannot be written as standalone elements"),
            ));
        }
        match &el.value {
            Value::Strings(values) => validate_string(vr, values)
                .map_err(|e| write_error(Op::Value, tag, vr, element_length(el), e)),
            Value::BulkData(_) => {
                let fail = |op, length, text: &str| {
                    Err(write_error(op, tag, vr, length, Error::msg(text)))
                };
                if self.bulk.resolver.is_none() {
                    return fail(
                        Op::Value,
                        element_length(el),
                        "dicom: core.BulkDataValue requires a BulkDataResolver",
                    );
                }
                if vr == Vr::SQ {
                    return fail(
                        Op::Value,
                        element_length(el),
                        "dicom: core.BulkDataValue cannot encode an SQ value",
                    );
                }
                if el.header.length_set && el.header.length == UNDEFINED_LENGTH {
                    return fail(
                        Op::Length,
                        el.header.length,
                        "dicom: core.BulkDataValue requires a defined length",
                    );
                }
                if tag == Tag::PIXEL_DATA && self.encapsulated {
                    return fail(
                        Op::Value,
                        element_length(el),
                        "dicom: core.BulkDataValue cannot synthesize encapsulated Pixel Data",
                    );
                }
                Ok(())
            }
            Value::Sequence(_) if vr != Vr::SQ && vr != Vr::UN => Err(write_error(
                Op::Value,
                tag,
                vr,
                element_length(el),
                Error::msg("dicom: core.SequenceValue requires SQ or UN VR"),
            )),
            Value::Fragments { .. } if tag != Tag::PIXEL_DATA => Err(write_error(
                Op::Value,
                tag,
                vr,
                element_length(el),
                Error::msg("dicom: core.FragmentSequence is only supported for Pixel Data"),
            )),
            Value::Fragments { .. } if vr != Vr::OB && vr != Vr::OW => Err(write_error(
                Op::Value,
                tag,
                vr,
                element_length(el),
                Error::msg("dicom: encapsulated Pixel Data requires OB or OW VR"),
            )),
            _ => Ok(()),
        }
    }
    fn write_element(&mut self, out: &mut Out, el: &Element) -> Result<(), Error> {
        let (tag, vr) = (el.tag(), el.vr());
        if self.padding.policy != padding_policy::PRESERVE && tag == TAG_DATA_SET_PADDING {
            return Ok(());
        }
        if self.nested_deferred(el) {
            return self.write_deferred_element(out, el, || self.nested_deferred_copy(el));
        }
        if tag == TAG_SPECIFIC_CHARACTER_SET {
            self.use_declared_character_set(el)
                .map_err(|e| write_error(Op::Value, tag, vr, element_length(el), e))?;
        }
        if vr == Vr::SQ || matches!(el.value, Value::Sequence(_)) {
            self.validate(el)?;
            return self.write_sequence(out, el);
        }
        if let Value::Fragments {
            offset_table,
            fragments,
        } = &el.value
        {
            self.validate(el)?;
            return self.write_fragments(out, offset_table, fragments);
        }
        if let Value::BulkData(uri) = &el.value {
            self.validate(el)?;
            return self.write_bulk_data_value(out, el, uri);
        }
        self.validate(el)?;
        let (value, length) = self.encode_value(el)?;
        self.header(out, tag, vr, length)?;
        out.extend_from_slice(&value);
        Ok(())
    }
    fn bulk_data_planned_length(&self, el: &Element, uri: &str) -> Result<i64, Error> {
        if el.header.length_set {
            return Ok(i64::from(el.header.length));
        }
        let Some(size) = &self.bulk.size else {
            return Err(Error::msg(ERR_ENCODED_LENGTH_UNAVAILABLE));
        };
        let n = size(uri).map_err(|e| Error::relabel(ERR_RESOLVE_BULK_DATA_SOURCE, e))?;
        bulk_data_encoded_length(n).map(i64::from)
    }
    fn write_bulk_data_value(&self, out: &mut Out, el: &Element, uri: &str) -> Result<(), Error> {
        let (tag, vr) = (el.tag(), el.vr());
        let declared = if self.planned {
            Some(self.bulk_data_planned_length(el, uri)? as Length)
        } else if el.header.length_set {
            Some(el.header.length)
        } else {
            None
        };
        let length = declared.unwrap_or(UNDEFINED_LENGTH);
        let Some(resolver) = &self.bulk.resolver else {
            return Err(Error::msg(
                "dicom: core.BulkDataValue requires a BulkDataResolver",
            ));
        };
        let source = resolver(uri).map_err(|e| {
            write_error(
                Op::Value,
                tag,
                vr,
                length,
                Error::relabel(ERR_RESOLVE_BULK_DATA_SOURCE, e),
            )
        })?;
        let Some(data) = source.reader else {
            return Err(write_error(
                Op::Value,
                tag,
                vr,
                length,
                Error::msg("dicom: Bulk Data resolver returned a nil Reader"),
            ));
        };
        let encoded = bulk_data_encoded_length(source.size)
            .map_err(|e| write_error(Op::Length, tag, vr, length, e))?;
        if let Some(declared) = declared
            && declared != encoded
        {
            return Err(write_error(
                Op::Length,
                tag,
                vr,
                declared,
                Error::msg(format!(
                    "dicom: Bulk Data source encoded length {encoded} does not match element header length {declared}"
                )),
            ));
        }
        self.header(out, tag, vr, encoded)?;
        let size = source.size as usize;
        let copied = data.len().min(size);
        for chunk in data[..copied].chunks(32 * 1024) {
            out.extend_from_slice(chunk);
        }
        let fail = |text: String| Err(write_error(Op::Value, tag, vr, encoded, Error::msg(text)));
        if copied < size {
            return fail(format!(
                "dicom: Bulk Data source copied {copied} bytes, want {}: EOF",
                source.size
            ));
        }
        if data.len() > size {
            return fail(format!(
                "dicom: Bulk Data source exceeds declared size {}",
                source.size
            ));
        }
        if !size.is_multiple_of(2) {
            out.push(vr.padding_byte());
        }
        Ok(())
    }
    fn check_planned_header(&self, vr: Vr, n: i64, undefined: bool) -> Result<(), Error> {
        if self.explicit && !vr.uses_long_explicit_length() && (undefined || n > 0xFFFF) {
            return Err(Error::msg(format!(
                "dicom: explicit VR {vr} length {n} exceeds uint16"
            )));
        }
        if !undefined && (n < 0 || n % 2 != 0 || n >= i64::from(UNDEFINED_LENGTH)) {
            return Err(Error::msg(format!(
                "dicom: invalid defined value length {n}: dicom: value length exceeds uint32"
            )));
        }
        Ok(())
    }
    fn encode_value(&self, el: &Element) -> Result<(Vec<u8>, Length), Error> {
        let (tag, vr) = (el.tag(), el.vr());
        let undefined = el.header.length_set && el.header.length == UNDEFINED_LENGTH;
        let undefined_error = || {
            write_error(
                Op::Length,
                tag,
                vr,
                el.header.length,
                Error::msg(
                    "dicom: undefined length is only supported for sequence and fragment values",
                ),
            )
        };
        match &el.value {
            Value::Nil => Ok((Vec::new(), 0)),
            Value::Raw(raw) => {
                if undefined {
                    return Err(undefined_error());
                }
                let mut padded = raw.clone();
                if padded.len() % 2 == 1 {
                    padded.push(vr.padding_byte());
                }
                let n = padded.len() as Length;
                Ok((padded, n))
            }
            Value::Strings(values) => {
                if undefined {
                    return Err(undefined_error());
                }
                let charset = if dict::is_private_creator(tag) {
                    SpecificCharacterSet::default()
                } else {
                    self.charset.clone()
                };
                encode_strings(vr, values, &charset)
                    .map_err(|e| write_error(Op::Length, tag, vr, 0, e))
            }
            _ => Err(write_error(
                Op::Value,
                tag,
                vr,
                element_length(el),
                Error::msg("dicom: unsupported value type"),
            )),
        }
    }
    fn sequence_undefined(&self, el: &Element) -> bool {
        match self.sequence_length {
            sequence_length::UNDEFINED => true,
            sequence_length::DEFINED => false,
            _ => el.header.length_set && el.header.length == UNDEFINED_LENGTH,
        }
    }
    fn write_sequence(&self, out: &mut Out, el: &Element) -> Result<(), Error> {
        self.check_write_depth(self.sequence_depth + 1)?;
        let (tag, vr) = (el.tag(), el.vr());
        let Value::Sequence(items) = &el.value else {
            return Err(write_error(
                Op::Value,
                tag,
                vr,
                element_length(el),
                Error::msg("dicom: sequence VR requires core.SequenceValue"),
            ));
        };
        let un = vr == Vr::UN && self.explicit;
        let mut inner = self.clone();
        inner.sequence_depth += 1;
        let converted: Vec<DataSet>;
        let items: &[DataSet] = if un {
            inner.explicit = false;
            inner.big = false;
            inner.encapsulated = false;
            if self.big {
                if !self.planned {
                    let budget = inner
                        .shared_budget
                        .get_or_insert_with(|| {
                            std::rc::Rc::new(std::cell::RefCell::new(PlanBudget::new(self.limits)))
                        })
                        .clone();
                    for item in items {
                        let (payload, structure) = inner.item_footprint(item)?;
                        budget
                            .borrow_mut()
                            .reserve_normalized_value(payload, structure)?;
                    }
                }
                converted = items.iter().map(little_endian_data_set).collect();
                &converted
            } else {
                items
            }
        } else {
            items
        };
        let undefined = un || self.sequence_undefined(el);
        if undefined {
            self.header(out, tag, if un { Vr::UN } else { vr }, UNDEFINED_LENGTH)?;
            for item in items {
                inner.write_item(out, item, true)?;
            }
            inner.delimiter(out, Tag::SEQUENCE_DELIMITATION);
            return Ok(());
        }
        if !self.planned {
            let mut local = PlanBudget::new(self.limits);
            let mut shared;
            let budget: &mut PlanBudget = match &self.shared_budget {
                Some(budget) => {
                    shared = budget.borrow_mut();
                    &mut shared
                }
                None => &mut local,
            };
            budget.reserve(1, PLANNED_ELEMENT_BYTES)?;
            inner
                .plan_sequence(el, budget)
                .map_err(|e| write_error(Op::Length, tag, vr, element_length(el), e))?;
            if budget.padding > self.padding_budget.get() && !self.measuring {
                return Err(Error::msg(ERR_PADDING_BUDGET));
            }
        }
        inner.planned = true;
        let mut body = Out::default();
        for item in items {
            inner.write_item(&mut body, item, false)?;
        }
        if body.data.len() as u64 >= u64::from(UNDEFINED_LENGTH) {
            return Err(write_error(
                Op::Length,
                tag,
                vr,
                element_length(el),
                Error::msg("dicom: sequence value too long"),
            ));
        }
        self.header(out, tag, vr, body.data.len() as Length)?;
        out.append(body);
        Ok(())
    }
    fn write_item(&self, out: &mut Out, ds: &DataSet, undefined: bool) -> Result<(), Error> {
        let planned = self.planned;
        let item_error = |e| {
            if planned {
                e
            } else {
                write_error(Op::Value, Tag::ITEM, Vr::UN, UNDEFINED_LENGTH, e)
            }
        };
        self.check_write_depth(self.sequence_depth + 1)?;
        let mut child = self.child_for(&ds.elements).map_err(item_error)?;
        child.in_item = true;
        child.sequence_depth = self.sequence_depth + 1;
        child.item_offset = ds.item_offset;
        child.item_sample_width = match self.deferred.as_deref() {
            Some(d) if d.reverse && d.provider.is_some() => {
                pixel_data_sample_width(&ds.elements, d.target_big)
            }
            _ => 0,
        };
        let elements = child.prepare_elements(&ds.elements).map_err(item_error)?;
        let mut body = Out::default();
        for el in &elements {
            child.write_element(&mut body, el).map_err(item_error)?;
        }
        let value_bytes = body.data.len() as i64;
        child.write_data_set_padding(&mut body, value_bytes, 0, &ds.elements)?;
        if undefined {
            self.item_header(out, UNDEFINED_LENGTH);
            out.append(body);
            self.delimiter(out, Tag::ITEM_DELIMITATION);
        } else {
            self.item_header(out, body.data.len() as Length);
            out.append(body);
        }
        Ok(())
    }
    fn write_fragments(
        &self,
        out: &mut Out,
        offset_table: &[u8],
        fragments: &[Vec<u8>],
    ) -> Result<(), Error> {
        self.header(out, Tag::PIXEL_DATA, Vr::OB, UNDEFINED_LENGTH)?;
        if !offset_table.len().is_multiple_of(4) {
            let inner = write_error(
                Op::Value,
                Tag::ITEM,
                Vr::UN,
                0,
                Error::msg(format!(
                    "dicom: Basic Offset Table length {} is not a multiple of 4",
                    offset_table.len()
                )),
            );
            return Err(write_error(
                Op::Value,
                Tag::PIXEL_DATA,
                Vr::OB,
                UNDEFINED_LENGTH,
                inner,
            ));
        }
        self.item_header(out, offset_table.len() as Length);
        out.extend_from_slice(offset_table);
        for fragment in fragments {
            let padded = fragment.len() + fragment.len() % 2;
            self.item_header(out, padded as Length);
            out.extend_from_slice(fragment);
            if fragment.len() % 2 == 1 {
                out.push(0);
            }
        }
        self.delimiter(out, Tag::SEQUENCE_DELIMITATION);
        Ok(())
    }
    fn header_size(&self, vr: Vr) -> u64 {
        if self.explicit && vr.uses_long_explicit_length() {
            12
        } else {
            8
        }
    }
    fn plan_element(&mut self, el: &Element, budget: &mut PlanBudget) -> Result<u64, Error> {
        if self.padding.policy != padding_policy::PRESERVE && el.tag() == TAG_DATA_SET_PADDING {
            return Ok(0);
        }
        self.validate(el)?;
        if el.tag() == TAG_SPECIFIC_CHARACTER_SET {
            self.use_declared_character_set(el)?;
        }
        if let Value::Sequence(_) = &el.value {
            self.sequence_depth += 1;
            let planned = self
                .check_write_depth(self.sequence_depth)
                .and_then(|()| self.plan_sequence(el, budget));
            self.sequence_depth -= 1;
            return planned;
        }
        if el.vr() == Vr::SQ {
            return Err(Error::msg("dicom: sequence VR requires core.SequenceValue"));
        }
        let mut undefined = false;
        let value = match &el.value {
            Value::Nil if self.nested_deferred(el) => u64::from(el.header.length),
            Value::Nil => 0,
            Value::BulkData(uri) => self.bulk_data_planned_length(el, uri)? as u64,
            Value::Raw(raw) => {
                if el.header.length_set && el.header.length == UNDEFINED_LENGTH {
                    return Err(Error::msg(
                        "dicom: undefined length is only supported for sequence and fragment values",
                    ));
                }
                let n = raw.len() as u64;
                n + n % 2
            }
            Value::Strings(_) => u64::from(self.encode_value(el)?.1),
            Value::Fragments {
                offset_table,
                fragments,
            } => {
                if offset_table.len() % 4 != 0 {
                    return Err(Error::msg(
                        "dicom: Basic Offset Table length is not a multiple of 4",
                    ));
                }
                let mut n = 8 + offset_table.len() as u64;
                for f in fragments {
                    n += 8 + (f.len() + f.len() % 2) as u64;
                }
                undefined = true;
                n + 8
            }
            Value::Sequence(_) => unreachable!("handled above"),
        };
        self.check_planned_header(el.vr(), value as i64, undefined)?;
        Ok(self.header_size(el.vr()) + value)
    }
    fn plan_sequence(&self, el: &Element, budget: &mut PlanBudget) -> Result<u64, Error> {
        let Value::Sequence(items) = &el.value else {
            return Err(Error::msg("dicom: sequence VR requires core.SequenceValue"));
        };
        let un = el.vr() == Vr::UN && self.explicit;
        let undefined = un || self.sequence_undefined(el);
        self.check_write_depth(self.sequence_depth)?;
        budget.reserve(items.len() as i64, PLANNED_ITEM_BYTES)?;
        let mut inner = self.clone();
        if un {
            inner.explicit = false;
            inner.big = false;
            inner.encapsulated = false;
        }
        let mut value = 0;
        for item in items {
            let item = if un && self.big {
                let (payload, structure) = self.item_footprint(item)?;
                budget.reserve_normalized_value(payload, structure)?;
                little_endian_data_set(item)
            } else {
                item.clone()
            };
            value += inner.plan_item(&item, undefined, budget)?;
        }
        if undefined {
            value += 8;
        }
        Ok(self.header_size(el.vr()) + value)
    }
    fn plan_item(
        &self,
        ds: &DataSet,
        undefined: bool,
        budget: &mut PlanBudget,
    ) -> Result<u64, Error> {
        self.check_write_depth(self.sequence_depth + 1)?;
        let count = ds.elements.len() as i64;
        if count > budget.limits.max_plan_nodes - budget.nodes {
            return Err(write_plan_limit());
        }
        let mut child = self.child_for(&ds.elements)?;
        child.in_item = true;
        child.sequence_depth = self.sequence_depth + 1;
        for needed in [
            self.padding.policy != padding_policy::PRESERVE,
            self.group_length != group_length::PRESERVE,
        ] {
            if needed {
                budget.reserve_bytes(checked_write_product(count, GROUP_SCRATCH_BYTES)?)?;
            }
        }
        child.limits.max_plan_bytes = budget.limits.max_plan_bytes - budget.bytes;
        let elements = child.prepare_elements(&ds.elements)?;
        budget.reserve(elements.len() as i64, PLANNED_ELEMENT_BYTES)?;
        let mut value = 0;
        for el in &elements {
            value += child.plan_element(el, budget)?;
        }
        let layout = child.padding_layout(value as i64, 0, &ds.elements)?;
        if layout.added > 0 {
            let nodes = if layout.group_length { 2 } else { 1 };
            budget.reserve(nodes, ELEMENT_BYTES)?;
        }
        budget.padding = checked_write_add(budget.padding, layout.added)?;
        value += layout.added as u64;
        Ok(8 + value + if undefined { 8 } else { 0 })
    }
    fn prepare_elements(&self, elements: &[Element]) -> Result<Vec<Element>, Error> {
        let filtered: Vec<Element>;
        let mut elements = elements;
        if self.padding.policy != padding_policy::PRESERVE {
            self.admit_group_scratch(elements.len())?;
            let create = self.padding.policy == padding_policy::CREATE
                && self.group_length != group_length::PRESERVE;
            let mut kept = Vec::with_capacity(elements.len());
            for el in elements {
                let tag = el.tag();
                if tag == TAG_DATA_SET_PADDING {
                    continue;
                }
                if create && tag.group == 0xFFFC && tag != TAG_PADDING_GROUP_LENGTH {
                    return Err(Error::msg(
                        "dicom: padding with recalculated Group Length does not support other FFFC attributes",
                    ));
                }
                if create && tag == TAG_PADDING_GROUP_LENGTH {
                    continue;
                }
                kept.push(el.clone());
            }
            filtered = kept;
            elements = &filtered;
        }
        let policy = self.group_length;
        if policy == group_length::PRESERVE {
            return Ok(elements.to_vec());
        }
        self.admit_group_scratch(elements.len())?;
        let exempt = |g: u16| g == 0x0000 || g == 0x0002 || g == 0xFFFE;
        let mut present = std::collections::HashSet::new();
        let mut kept = Vec::with_capacity(elements.len());
        for el in elements {
            let tag = el.tag();
            if tag.element == 0 && !exempt(tag.group) {
                present.insert(tag.group);
                continue;
            }
            kept.push(el.clone());
        }
        if policy == group_length::REMOVE {
            return Ok(kept);
        }
        let mut measure = self.child_for(&kept)?;
        measure.measuring = true;
        let limits = self.limits;
        let size = |measure: &mut Writer, el: &Element| {
            let mut budget = PlanBudget::new(limits);
            budget.reserve(1, PLANNED_ELEMENT_BYTES)?;
            measure.plan_element(el, &mut budget)
        };
        let mut lengths: std::collections::HashMap<u16, u64> = std::collections::HashMap::new();
        for el in &kept {
            let n = if matches!(el.value, Value::Nil) {
                let n = if self.in_item {
                    size(&mut measure, el)
                } else {
                    self.deferred_encoded_size(el)
                };
                if el.tag() == TAG_SPECIFIC_CHARACTER_SET {
                    return Err(Error::msg(
                        "dicom: deferred Specific Character Set cannot be measured",
                    ));
                }
                n?
            } else {
                size(&mut measure, el)?
            };
            *lengths.entry(el.tag().group).or_default() += n;
        }
        let mut out = Vec::with_capacity(kept.len() + lengths.len());
        let mut written = std::collections::HashSet::new();
        for el in kept {
            let group = el.tag().group;
            if !written.contains(&group)
                && !exempt(group)
                && (policy == group_length::CREATE || present.contains(&group))
            {
                written.insert(group);
                let n = lengths[&group];
                if n > u64::from(u32::MAX) {
                    return Err(Error::msg(format!(
                        "dicom: group {group:04X} is longer than a Group Length can hold"
                    )));
                }
                let n = n as u32;
                let raw = if self.big {
                    n.to_be_bytes()
                } else {
                    n.to_le_bytes()
                };
                out.push(Element {
                    header: ElementHeader {
                        tag: Tag::new(group, 0),
                        vr: Vr::UL,
                        length: 0,
                        length_set: false,
                    },
                    value: Value::Raw(raw.to_vec()),
                });
            }
            out.push(el);
        }
        Ok(out)
    }
}
fn encode_strings(
    vr: Vr,
    values: &[String],
    charset: &SpecificCharacterSet,
) -> Result<(Vec<u8>, Length), Error> {
    let mut out = Vec::new();
    for (i, component) in values.iter().enumerate() {
        let encoded = if !vr.uses_specific_character_set() {
            Ok(component.as_bytes().to_vec())
        } else if vr == Vr::PN {
            charset.encode_person_name(component)
        } else if !vr.uses_text_value_delimiter() {
            charset.encode_single_value(component)
        } else {
            charset.encode(component)
        }
        .map_err(|e| Error::msg(format!("dicom: encode {vr} string component {i}: {e}")))?;
        if i > 0 {
            out.push(b'\\');
        }
        out.extend_from_slice(&encoded);
    }
    if out.len() % 2 == 1 {
        out.push(vr.padding_byte());
    }
    let n = out.len() as Length;
    Ok((out, n))
}
fn number_matches_ds(s: &[u8]) -> bool {
    let mut i = 0;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < s.len() && s[*i].is_ascii_digit() {
            *i += 1;
        }
        *i - start
    };
    let int = digits(&mut i);
    if int > 0 {
        if i < s.len() && s[i] == b'.' {
            i += 1;
            digits(&mut i);
        }
    } else {
        if i >= s.len() || s[i] != b'.' {
            return false;
        }
        i += 1;
        if digits(&mut i) == 0 {
            return false;
        }
    }
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        i += 1;
        if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return false;
        }
    }
    i == s.len()
}
fn number_matches_is(s: &[u8]) -> bool {
    let s = s
        .strip_prefix(b"+")
        .or_else(|| s.strip_prefix(b"-"))
        .unwrap_or(s);
    !s.is_empty() && s.iter().all(u8::is_ascii_digit)
}
fn validate_number_string(vr: Vr, value: &str) -> Result<(), String> {
    let max = if vr == Vr::IS { 12 } else { 16 };
    if value.len() > max {
        return Err(format!(
            "{vr} value is {} bytes, want at most {max}",
            value.len()
        ));
    }
    let trimmed = value.trim_matches(' ');
    let ok = if vr == Vr::IS {
        number_matches_is(trimmed.as_bytes())
    } else {
        number_matches_ds(trimmed.as_bytes())
    };
    if !ok {
        return Err(format!(
            "{}-byte value does not match {vr} syntax",
            value.len()
        ));
    }
    if vr == Vr::IS
        && !matches!(trimmed.parse::<i64>(), Ok(n) if i64::from(i32::MIN) <= n && n <= i64::from(i32::MAX))
    {
        return Err("IS value is outside the signed 32-bit range".to_string());
    }
    Ok(())
}
fn validate_string(vr: Vr, values: &[String]) -> Result<(), Error> {
    let fail = |m: String| Err(Error::msg(format!("{ERR_VALUE_NOT_REPRESENTABLE}: {m}")));
    if !vr.is_string_like() {
        return fail(format!("text value is incompatible with VR {vr}"));
    }
    if !vr.uses_text_value_delimiter() && values.len() > 1 {
        return fail(format!("VR {vr} holds one value, got {}", values.len()));
    }
    if vr == Vr::UI {
        let mut index = 0;
        for component in values {
            for uid in component.split('\\') {
                let uid = uid.trim_end_matches(['\0', ' ']);
                if !uid.is_empty() && !is_valid_uid(uid.as_bytes()) {
                    return fail(format!(
                        "UI value {index} ({} bytes) is not a valid UID",
                        uid.len()
                    ));
                }
                index += 1;
            }
        }
        return Ok(());
    }
    if vr != Vr::IS && vr != Vr::DS {
        return Ok(());
    }
    let mut index = 0;
    for component in values {
        for number in component.split('\\') {
            if !number.trim_matches(' ').is_empty()
                && let Err(e) = validate_number_string(vr, number)
            {
                return fail(format!("{vr} value {index}: {e}"));
            }
            index += 1;
        }
    }
    Ok(())
}
fn byte_order_width(vr: Vr) -> usize {
    vr.byte_order_width()
}
fn reverse_fixed_width(data: &[u8], width: usize) -> Vec<u8> {
    let mut out = data.to_vec();
    if width > 1 {
        for chunk in out.chunks_exact_mut(width) {
            chunk.reverse();
        }
    }
    out
}
fn little_endian_data_set(ds: &DataSet) -> DataSet {
    let elements = ds
        .elements
        .iter()
        .map(|e| {
            let mut e = e.clone();
            match &e.value {
                Value::Raw(raw) => {
                    let width = byte_order_width(e.vr());
                    if width > 1 && raw.len() % width == 0 {
                        e.value = Value::Raw(reverse_fixed_width(raw, width));
                    }
                }
                Value::Sequence(items) => {
                    e.value = Value::Sequence(items.iter().map(little_endian_data_set).collect());
                }
                _ => {}
            }
            e
        })
        .collect();
    DataSet {
        elements,
        item_offset: ds.item_offset,
    }
}
fn pixel_data_sample_width(elements: &[Element], big: bool) -> usize {
    let Some(element) = elements.iter().find(|e| e.tag() == TAG_BITS_ALLOCATED) else {
        return 0;
    };
    let Value::Raw(raw) = &element.value else {
        return 0;
    };
    if raw.len() != 2 {
        return 0;
    }
    let bits = if big {
        u16::from_be_bytes([raw[0], raw[1]])
    } else {
        u16::from_le_bytes([raw[0], raw[1]])
    };
    match bits {
        32 => 4,
        64 => 8,
        _ => 0,
    }
}
fn reverse_elements_byte_order(
    elements: &[Element],
    big: bool,
    keep_partial: bool,
) -> Result<Vec<Element>, Error> {
    let sample_width = pixel_data_sample_width(elements, big);
    let mut out = Vec::with_capacity(elements.len());
    for element in elements {
        let mut element = element.clone();
        match &element.value {
            Value::Raw(raw) => {
                let mut width = byte_order_width(element.vr());
                if element.tag() == Tag::PIXEL_DATA
                    && element.vr() == Vr::OW
                    && sample_width > width
                {
                    width = sample_width;
                }
                if width > 1 {
                    if raw.len() % width != 0 && !keep_partial {
                        return Err(Error::msg(format!(
                            "dicom: cannot change the byte order of {} with VR {}: value length is not a multiple of {width}",
                            element.tag(),
                            element.vr()
                        )));
                    }
                    element.value = Value::Raw(reverse_fixed_width(raw, width));
                }
            }
            Value::Sequence(items) => {
                let mut converted = Vec::with_capacity(items.len());
                for item in items {
                    converted.push(DataSet {
                        elements: reverse_elements_byte_order(&item.elements, big, keep_partial)?,
                        item_offset: item.item_offset,
                    });
                }
                element.value = Value::Sequence(converted);
            }
            _ => {}
        }
        out.push(element);
    }
    Ok(out)
}
fn unsupported(syntax: &Syntax) -> Error {
    let name = if syntax.name.is_empty() {
        String::new()
    } else {
        format!(" ({})", syntax.name)
    };
    Error::kind_suffix(
        Kind::UnsupportedTransferSyntax,
        &format!(
            ": transfer syntax {}{name} is registered but not supported by this writer",
            crate::textfmt::quote(syntax.uid)
        ),
    )
}
pub fn writable_syntax(uid: &str) -> Result<Syntax, Error> {
    let uid = transfer::normalize_uid(uid);
    if uid.is_empty() {
        return Err(Error::sentinel(Kind::MissingTransferSyntax));
    }
    let Some(syntax) = transfer::lookup(uid) else {
        return Err(Error::kind_suffix(
            Kind::UnknownTransferSyntax,
            &format!(": {}", crate::textfmt::quote(uid)),
        ));
    };
    if !syntax.supported {
        return Err(unsupported(&syntax));
    }
    Ok(syntax)
}
fn pixel_data_encapsulated(element: &Element, parsed: bool) -> (bool, bool) {
    if parsed && element.header.length_set {
        let length = element.header.length;
        return (length == UNDEFINED_LENGTH, length != 0);
    }
    match &element.value {
        Value::Fragments { .. } => (true, true),
        Value::Raw(raw) => (
            raw.starts_with(&[0xfe, 0xff, 0x00, 0xe0])
                || raw.starts_with(&[0xff, 0xfe, 0xe0, 0x00]),
            !raw.is_empty(),
        ),
        Value::BulkData(_) => (false, true),
        Value::Nil => {
            if !element.header.length_set || element.header.length == 0 {
                (false, false)
            } else {
                (element.header.length == UNDEFINED_LENGTH, true)
            }
        }
        _ => (false, false),
    }
}
impl Object {
    pub fn check_pixel_data_encoding(&self, syntax: &Syntax) -> Result<(), Error> {
        let Some(element) = self.get(Tag::PIXEL_DATA) else {
            return Ok(());
        };
        let (encapsulated, known) = pixel_data_encapsulated(element, self.parsed_pixel_data);
        if !known || encapsulated == syntax.encapsulated {
            return Ok(());
        }
        let what = if encapsulated {
            "encapsulated Pixel Data under native"
        } else {
            "native Pixel Data under encapsulated"
        };
        Err(Error::msg(format!(
            "dicom: Pixel Data encoding does not match the transfer syntax: {what} transfer syntax {}",
            syntax.uid
        )))
    }
}
#[allow(clippy::too_many_arguments)]
fn write_data_set_inner(
    obj: &Object,
    syntax: &Syntax,
    sequence: u8,
    group: u8,
    keep_partial: bool,
    level: i32,
    padding: PaddingOptions,
    prefix: i64,
    bulk: &BulkDataWriteOptions,
    limits: SequenceWriteLimits,
) -> Result<Vec<u8>, Error> {
    if obj.has_discarded_values() {
        return Err(Error::msg(super::read_ext::ERR_DISCARDED_VALUE));
    }
    let mut out = Out::default();
    write_data_set_elements(
        &mut out,
        obj,
        syntax,
        (sequence, group),
        keep_partial,
        padding,
        prefix,
        bulk,
        limits,
    )?;
    if syntax.deflated {
        return padded_deflate(&out, level);
    }
    Ok(out.data)
}
#[allow(clippy::too_many_arguments)]
fn write_data_set_elements(
    out: &mut Out,
    obj: &Object,
    syntax: &Syntax,
    (sequence, group): (u8, u8),
    keep_partial: bool,
    padding: PaddingOptions,
    prefix: i64,
    bulk: &BulkDataWriteOptions,
    limits: SequenceWriteLimits,
) -> Result<(), Error> {
    let charset = match obj.character_set_state() {
        (cs, None) => cs,
        (_, Some(err)) => {
            if obj.text_options().allow_unsupported_charset_fallback {
                obj.text_options().fallback_character_set.clone()
            } else {
                return Err(Error::wrap(
                    "dicom: resolve dataset Specific Character Set for writing",
                    err,
                ));
            }
        }
    };
    obj.check_pixel_data_encoding(syntax)?;
    let mut original = obj.sorted_elements();
    let reverse = obj.value_big_endian() != syntax.big_endian;
    let mut sample_width = 0;
    if reverse {
        sample_width = pixel_data_sample_width(&original, obj.value_big_endian());
        let kept = if obj.pixel_data_in_destination_order {
            original
                .iter()
                .position(|e| e.tag() == Tag::PIXEL_DATA)
                .map(|i| (i, original.remove(i)))
        } else {
            None
        };
        original = reverse_elements_byte_order(&original, obj.value_big_endian(), keep_partial)?;
        if let Some((i, element)) = kept {
            original.insert(i, element);
        }
    }
    let mut writer = Writer::new(
        syntax.explicit_vr,
        syntax.big_endian,
        sequence,
        group,
        charset,
        padding,
    );
    writer.bulk = bulk.clone();
    writer.encapsulated = syntax.encapsulated;
    writer.limits = limits;
    writer.deferred = Some(std::rc::Rc::new(DeferredWrite {
        provider: obj.value_provider.clone(),
        ambiguous: obj.ambiguous_deferred.clone(),
        reverse,
        target_big: syntax.big_endian,
        keep_partial,
    }));
    let elements = writer.prepare_elements(&original)?;
    for el in &elements {
        if matches!(el.value, Value::Nil) {
            writer.write_top_level_deferred(out, el, sample_width)?;
            continue;
        }
        writer.write_element(out, el)?;
    }
    let value_bytes = out.data.len() as i64;
    writer.write_data_set_padding(out, value_bytes, prefix, &original)
}
pub fn write_data_set_with_options(
    obj: &Object,
    uid: &str,
    opts: WriteDataSetOptions,
) -> Result<Vec<u8>, Error> {
    write_data_set_with_bulk_data(obj, uid, opts, &BulkDataWriteOptions::default())
}
#[doc(hidden)]
pub fn write_data_set_with_bulk_data(
    obj: &Object,
    uid: &str,
    opts: WriteDataSetOptions,
    bulk: &BulkDataWriteOptions,
) -> Result<Vec<u8>, Error> {
    write_data_set_with_sequence_limits(obj, uid, opts, bulk, SequenceWriteLimits::default())
}
#[doc(hidden)]
pub fn write_data_set_with_sequence_limits(
    obj: &Object,
    uid: &str,
    opts: WriteDataSetOptions,
    bulk: &BulkDataWriteOptions,
    limits: SequenceWriteLimits,
) -> Result<Vec<u8>, Error> {
    let syntax = writable_syntax(uid)?;
    let limits = normalize_sequence_limits(limits)?;
    let level = serialization_level(&syntax, opts.deflate_level, &opts.padding)?;
    write_data_set_inner(
        obj,
        &syntax,
        opts.sequence_length,
        opts.group_length,
        false,
        level,
        opts.padding,
        0,
        bulk,
        limits,
    )
}
pub fn write_data_set(obj: &Object, uid: &str) -> Result<Vec<u8>, Error> {
    write_data_set_with_options(obj, uid, WriteDataSetOptions::default())
}
fn valid_ae_title(title: &str) -> bool {
    if title.len() > 16 || title.trim().is_empty() {
        return false;
    }
    title
        .bytes()
        .all(|b| (0x20..=0x7e).contains(&b) && b != b'\\')
}
fn validate_meta_options(opts: &WriteFileOptions) -> Result<(), Error> {
    let invalid = "dicom: invalid File Meta options";
    if !(file_meta_mode::UPDATE..=file_meta_mode::PRESERVE).contains(&opts.meta_mode) {
        return Err(Error::msg(format!("{invalid}: mode {}", opts.meta_mode)));
    }
    let t = &opts.ae_titles;
    for title in [&t.source, &t.sending, &t.receiving] {
        if !title.is_empty() && !valid_ae_title(title) {
            return Err(Error::msg(format!(
                "{invalid}: AE title {}",
                crate::textfmt::quote(title)
            )));
        }
    }
    Ok(())
}
fn strings_element(tag: Tag, vr: Vr, value: &str) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(vec![value.to_string()]),
    }
}
fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, raw.len() as Length),
        value: Value::Raw(raw),
    }
}
fn element_string_value(el: Option<&Element>) -> String {
    let Some(el) = el else {
        return String::new();
    };
    match &el.value {
        Value::Raw(raw) => trim_text_value(el.vr(), &String::from_utf8_lossy(raw)).to_string(),
        Value::Strings(values) => values
            .first()
            .map(|v| trim_text_value(el.vr(), v).to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}
fn is_authored(el: Option<&Element>) -> bool {
    matches!(el.map(|e| &e.value), Some(Value::Strings(_)))
}
fn derived_uid_element(tag: Tag, uid: &str, read: bool) -> Element {
    if !read {
        return strings_element(tag, Vr::UI, uid);
    }
    let mut raw = uid.as_bytes().to_vec();
    if !raw.len().is_multiple_of(2) {
        raw.push(0);
    }
    raw_element(tag, Vr::UI, raw)
}
fn resolve_file_meta_uid(
    file: &File,
    meta: &std::collections::BTreeMap<Tag, Element>,
    dataset_tag: Tag,
    meta_tag: Tag,
    missing: Kind,
) -> Result<(String, bool), Error> {
    if let Some(value) = file.dataset.get_string(dataset_tag) {
        let uid = transfer::normalize_uid(&value);
        if !uid.is_empty() {
            return Ok((uid.to_string(), !is_authored(file.dataset.get(dataset_tag))));
        }
    }
    let value = element_string_value(meta.get(&meta_tag));
    let value = transfer::normalize_uid(&value);
    if !value.is_empty() {
        return Ok((value.to_string(), !is_authored(meta.get(&meta_tag))));
    }
    Err(Error::sentinel(missing))
}
fn build_file_meta_elements(
    file: &File,
    syntax: &Syntax,
    opts: &WriteFileOptions,
) -> Result<Vec<Element>, Error> {
    validate_meta_options(opts)?;
    let mut meta: std::collections::BTreeMap<Tag, Element> = file
        .meta
        .elements()
        .iter()
        .filter(|e| e.tag().group == 0x0002)
        .map(|e| (e.tag(), e.clone()))
        .collect();
    let (class_uid, class_read) = resolve_file_meta_uid(
        file,
        &meta,
        TAG_SOP_CLASS_UID,
        TAG_MEDIA_STORAGE_SOP_CLASS_UID,
        Kind::MissingSopClassUid,
    )?;
    let (instance_uid, instance_read) = resolve_file_meta_uid(
        file,
        &meta,
        TAG_SOP_INSTANCE_UID,
        TAG_MEDIA_STORAGE_SOP_INSTANCE_UID,
        Kind::MissingSopInstanceUid,
    )?;
    if opts.meta_mode == file_meta_mode::CREATE {
        meta.clear();
    }
    meta.insert(
        TAG_MEDIA_STORAGE_SOP_CLASS_UID,
        derived_uid_element(TAG_MEDIA_STORAGE_SOP_CLASS_UID, &class_uid, class_read),
    );
    meta.insert(
        TAG_MEDIA_STORAGE_SOP_INSTANCE_UID,
        derived_uid_element(
            TAG_MEDIA_STORAGE_SOP_INSTANCE_UID,
            &instance_uid,
            instance_read,
        ),
    );
    meta.insert(
        TAG_TRANSFER_SYNTAX_UID,
        strings_element(TAG_TRANSFER_SYNTAX_UID, Vr::UI, syntax.uid),
    );
    let existing = element_string_value(meta.get(&TAG_IMPLEMENTATION_CLASS_UID));
    let existing = transfer::normalize_uid(&existing).to_string();
    if opts.meta_mode == file_meta_mode::PRESERVE && !existing.is_empty() {
        let read = !is_authored(meta.get(&TAG_IMPLEMENTATION_CLASS_UID));
        meta.insert(
            TAG_IMPLEMENTATION_CLASS_UID,
            derived_uid_element(TAG_IMPLEMENTATION_CLASS_UID, &existing, read),
        );
    } else {
        meta.insert(
            TAG_IMPLEMENTATION_CLASS_UID,
            strings_element(
                TAG_IMPLEMENTATION_CLASS_UID,
                Vr::UI,
                IMPLEMENTATION_CLASS_UID,
            ),
        );
        meta.insert(
            TAG_IMPLEMENTATION_VERSION_NAME,
            strings_element(
                TAG_IMPLEMENTATION_VERSION_NAME,
                Vr::SH,
                IMPLEMENTATION_VERSION_NAME,
            ),
        );
    }
    let t = &opts.ae_titles;
    for (tag, value) in [
        (TAG_SOURCE_AE_TITLE, &t.source),
        (TAG_SENDING_AE_TITLE, &t.sending),
        (TAG_RECEIVING_AE_TITLE, &t.receiving),
    ] {
        if !value.is_empty() {
            meta.insert(tag, strings_element(tag, Vr::AE, value.trim()));
        }
    }
    meta.entry(TAG_FILE_META_VERSION)
        .or_insert_with(|| raw_element(TAG_FILE_META_VERSION, Vr::OB, vec![0x00, 0x01]));
    meta.remove(&TAG_FILE_META_GROUP_LENGTH);
    let mut group_length: u64 = 0;
    for el in meta.values() {
        let value_length = match &el.value {
            Value::Raw(raw) => raw.len() as u64,
            Value::Strings(values) => u64::from(strings_encoded_length(values)),
            _ => {
                return Err(Error::msg(format!(
                    "dicom: file meta element {} has no defined length",
                    el.tag()
                )));
            }
        };
        let header = if el.vr().uses_long_explicit_length() {
            12
        } else if value_length > 0xFFFF {
            return Err(Error::msg(format!(
                "dicom: file meta element {} length {value_length} exceeds uint16",
                el.tag()
            )));
        } else {
            8
        };
        group_length += header + value_length;
    }
    if group_length > u64::from(u32::MAX) {
        return Err(Error::msg(format!(
            "dicom: file meta group length {group_length} exceeds uint32"
        )));
    }
    let mut elements = vec![raw_element(
        TAG_FILE_META_GROUP_LENGTH,
        Vr::UL,
        (group_length as u32).to_le_bytes().to_vec(),
    )];
    elements.extend(meta.into_values());
    Ok(elements)
}
fn resolve_write_transfer_syntax(file: &File, opts: &WriteFileOptions) -> Result<Syntax, Error> {
    let uid = transfer::normalize_uid(file.transfer_syntax.uid);
    if !uid.is_empty() {
        return writable_syntax(uid);
    }
    let meta_uid = file
        .meta
        .get_string(TAG_TRANSFER_SYNTAX_UID)
        .map(|v| transfer::normalize_uid(&v).to_string())
        .unwrap_or_default();
    if meta_uid.is_empty() {
        let over = transfer::normalize_uid(&opts.override_missing_transfer_syntax);
        if !over.is_empty() {
            return writable_syntax(over);
        }
        if !opts.override_missing_transfer_syntax.is_empty() {
            return Err(Error::kind_suffix(
                Kind::MissingTransferSyntax,
                ": empty OverrideMissingTransferSyntax",
            ));
        }
        if opts.default_missing_transfer_syntax {
            return Ok(IMPLICIT_VR_LITTLE_ENDIAN);
        }
        return Err(Error::sentinel(Kind::MissingTransferSyntax));
    }
    writable_syntax(&meta_uid)
}
fn prepare_writable_data_set(file: &File, meta: &[Element], omit: bool) -> Result<Object, Error> {
    let mut dataset = Object::from_elements_with_text_options(
        file.dataset.elements().to_vec(),
        file.dataset.text_options().clone(),
    );
    dataset.set_value_big_endian(file.dataset.value_big_endian());
    dataset.value_provider = file.dataset.value_provider.clone();
    dataset.pixel_data_in_destination_order = file.dataset.pixel_data_in_destination_order;
    let meta_object = Object::from_elements(meta.to_vec());
    if omit {
        let class = meta_object.get_uid(TAG_MEDIA_STORAGE_SOP_CLASS_UID);
        if class.as_deref().map(transfer::normalize_uid)
            != Some(MEDIA_STORAGE_DIRECTORY_STORAGE_UID)
        {
            return Err(Error::msg(
                "dicom: omission of reconciled dataset SOP UIDs requires Media Storage Directory Storage",
            ));
        }
        return Ok(dataset);
    }
    for (dataset_tag, meta_tag) in [
        (TAG_SOP_CLASS_UID, TAG_MEDIA_STORAGE_SOP_CLASS_UID),
        (TAG_SOP_INSTANCE_UID, TAG_MEDIA_STORAGE_SOP_INSTANCE_UID),
    ] {
        if let Some(value) = dataset.get_string(dataset_tag)
            && !transfer::normalize_uid(&value).is_empty()
        {
            continue;
        }
        let value = meta_object.get_string(meta_tag).unwrap_or_default();
        let value = transfer::normalize_uid(&value);
        if value.is_empty() {
            return Err(Error::msg(format!(
                "dicom: missing reconciled dataset value for {dataset_tag}"
            )));
        }
        dataset.put(strings_element(dataset_tag, Vr::UI, value));
    }
    Ok(dataset)
}
impl File {
    pub fn rebuild_file_meta_with_options(&mut self, opts: &WriteFileOptions) -> Result<(), Error> {
        let syntax = resolve_write_transfer_syntax(self, &WriteFileOptions::default())?;
        let meta = build_file_meta_elements(self, &syntax, opts)?;
        self.meta = Object::from_elements(meta);
        self.transfer_syntax = syntax;
        Ok(())
    }
    pub fn rebuild_file_meta(&mut self) -> Result<(), Error> {
        self.rebuild_file_meta_with_options(&WriteFileOptions::default())
    }
}
pub fn write_file(file: &File) -> Result<Vec<u8>, Error> {
    write_file_with_options(file, &WriteFileOptions::default())
}
pub fn write_file_with_options(file: &File, opts: &WriteFileOptions) -> Result<Vec<u8>, Error> {
    write_file_with_bulk_data(file, opts, &BulkDataWriteOptions::default())
}
#[doc(hidden)]
pub fn write_file_with_bulk_data(
    file: &File,
    opts: &WriteFileOptions,
    bulk: &BulkDataWriteOptions,
) -> Result<Vec<u8>, Error> {
    write_file_with_sequence_limits(file, opts, bulk, SequenceWriteLimits::default())
}
#[doc(hidden)]
pub fn write_file_with_sequence_limits(
    file: &File,
    opts: &WriteFileOptions,
    bulk: &BulkDataWriteOptions,
    limits: SequenceWriteLimits,
) -> Result<Vec<u8>, Error> {
    let limits = normalize_sequence_limits(limits)?;
    if file.dataset.has_discarded_values() {
        return Err(Error::msg(super::read_ext::ERR_DISCARDED_VALUE));
    }
    if file.incomplete_pixel_data.is_some() || file.partial_read.is_some() && !opts.allow_partial {
        return Err(Error::msg("dicom: file is the result of a partial read"));
    }
    let invalid_preamble = || Error::msg("dicom: invalid Part 10 preamble length");
    if opts.preamble.as_ref().is_some_and(|p| p.len() != 128) {
        return Err(invalid_preamble());
    }
    let syntax = resolve_write_transfer_syntax(file, opts)?;
    let level = serialization_level(&syntax, opts.deflate_level, &opts.padding)?;
    let meta = build_file_meta_elements(file, &syntax, opts)?;
    let mut encoded_meta = Out::default();
    let mut meta_writer = Writer::new(
        true,
        false,
        sequence_length::UNDEFINED,
        group_length::PRESERVE,
        SpecificCharacterSet::default(),
        PaddingOptions::default(),
    );
    for el in &meta {
        meta_writer.write_element(&mut encoded_meta, el)?;
    }
    let dataset = prepare_writable_data_set(file, &meta, opts.omit_reconciled_data_set_sop_uids)?;
    let preamble = match &opts.preamble {
        Some(p) => p.clone(),
        None => match file.preamble.len() {
            0 => vec![0; 128],
            128 => file.preamble.clone(),
            _ => return Err(invalid_preamble()),
        },
    };
    let mut out = preamble;
    out.extend_from_slice(b"DICM");
    out.extend_from_slice(&encoded_meta.data);
    let prefix = out.len() as i64;
    out.extend(write_data_set_inner(
        &dataset,
        &syntax,
        opts.sequence_length,
        opts.group_length,
        opts.keep_partial_byte_order_units,
        level,
        opts.padding,
        prefix,
        bulk,
        limits,
    )?);
    Ok(out)
}
pub(crate) fn encode_element_explicit_le(el: &Element) -> Option<Vec<u8>> {
    try_encode_element_explicit_le(el).ok()
}
pub(crate) fn try_encode_element_explicit_le(el: &Element) -> Result<Vec<u8>, Error> {
    let mut writer = Writer::new(
        true,
        false,
        sequence_length::UNDEFINED,
        group_length::PRESERVE,
        SpecificCharacterSet::default(),
        PaddingOptions::default(),
    );
    let mut out = Out::default();
    writer.write_element(&mut out, el)?;
    Ok(out.data)
}
pub(super) fn frame_writer_syntax(file: &File, opts: &WriteFileOptions) -> Result<Syntax, Error> {
    resolve_write_transfer_syntax(file, opts)
}
pub(super) fn frame_value_error(vr: Vr, length: Length, err: Error) -> Error {
    write_error(Op::Value, Tag::PIXEL_DATA, vr, length, err)
}
pub(crate) struct ElementStream {
    stack: Vec<Writer>,
    pub(crate) out: Out,
    deflated: bool,
    provider: Option<std::sync::Arc<ValueProvider>>,
    reverse: bool,
    target_big: bool,
    sample_width: usize,
}
impl ElementStream {
    pub(crate) fn new(obj: &Object, uid: &str) -> Result<(ElementStream, Vec<Element>), Error> {
        let charset = match obj.character_set_state() {
            (cs, None) => cs,
            (_, Some(err)) => {
                if obj.text_options().allow_unsupported_charset_fallback {
                    obj.text_options().fallback_character_set.clone()
                } else {
                    return Err(Error::wrap(
                        "dicom: resolve dataset Specific Character Set for writing",
                        err,
                    ));
                }
            }
        };
        let syntax = writable_syntax(uid)?;
        let mut elements = obj.sorted_elements();
        let reverse = obj.value_big_endian() != syntax.big_endian;
        let mut sample_width = 0;
        if reverse {
            sample_width = pixel_data_sample_width(&elements, obj.value_big_endian());
            elements = reverse_elements_byte_order(&elements, obj.value_big_endian(), false)?;
        }
        let mut writer = Writer::new(
            syntax.explicit_vr,
            syntax.big_endian,
            sequence_length::UNDEFINED,
            group_length::PRESERVE,
            charset,
            PaddingOptions::default(),
        );
        writer.encapsulated = syntax.encapsulated;
        Ok((
            ElementStream {
                stack: vec![writer],
                out: Out::default(),
                deflated: syntax.deflated,
                provider: obj.value_provider.clone(),
                reverse,
                target_big: syntax.big_endian,
                sample_width,
            },
            elements,
        ))
    }
    pub(crate) fn for_syntax(syntax: &Syntax) -> ElementStream {
        let mut writer = Writer::new(
            syntax.explicit_vr,
            syntax.big_endian,
            sequence_length::UNDEFINED,
            group_length::PRESERVE,
            SpecificCharacterSet::default(),
            PaddingOptions::default(),
        );
        writer.encapsulated = syntax.encapsulated;
        ElementStream {
            stack: vec![writer],
            out: Out::default(),
            deflated: false,
            provider: None,
            reverse: false,
            target_big: syntax.big_endian,
            sample_width: 0,
        }
    }
    #[allow(clippy::type_complexity)]
    pub(crate) fn write_deferred_leaf(
        &mut self,
        el: &Element,
        copy: Option<&mut dyn FnMut(&mut Vec<u8>) -> (i64, Option<Error>)>,
    ) -> Result<(), Error> {
        let mut out = std::mem::take(&mut self.out);
        let top = self.top();
        let r =
            if top.padding.policy != padding_policy::PRESERVE && el.tag() == TAG_DATA_SET_PADDING {
                Ok(())
            } else {
                match copy {
                    None => Err(write_error(
                        Op::Value,
                        el.tag(),
                        el.vr(),
                        element_length(el),
                        Error::msg("dicom: deferred element requires value provider"),
                    )),
                    Some(copy) => top.write_deferred_element(&mut out, el, || {
                        let mut data = Vec::new();
                        let (copied, err) = copy(&mut data);
                        DeferredCopy {
                            data,
                            copied,
                            err,
                            width: 1,
                        }
                    }),
                }
            };
        self.out = out;
        r
    }
    fn top(&self) -> &Writer {
        self.stack.last().expect("writer stack")
    }
    pub(crate) fn charset(&self) -> SpecificCharacterSet {
        self.top().charset.clone()
    }
    pub(crate) fn position(&self) -> i64 {
        self.out.data.len() as i64
    }
    pub(crate) fn write_leaf(&mut self, el: &Element, top_level: bool) -> Result<(), Error> {
        let mut out = std::mem::take(&mut self.out);
        let r = if matches!(el.value, Value::Nil) && top_level {
            let tag = el.tag();
            match self.provider.clone() {
                None => self
                    .top()
                    .write_deferred_element(&mut out, el, || DeferredCopy {
                        data: Vec::new(),
                        copied: 0,
                        err: Some(Error::msg(format!(
                            "dicom: deferred element {tag} requires value provider"
                        ))),
                        width: 1,
                    }),
                Some(provider) => {
                    let mut writer = self.top().clone();
                    writer.deferred = Some(std::rc::Rc::new(DeferredWrite {
                        provider: None,
                        ambiguous: Vec::new(),
                        reverse: self.reverse,
                        target_big: self.target_big,
                        keep_partial: false,
                    }));
                    let sample_width = self.sample_width;
                    writer.write_deferred_element(&mut out, el, || {
                        writer.deferred_copy(el, sample_width, |w| provider.copy_value_to(tag, w))
                    })
                }
            }
        } else {
            let mut writer = self.stack.pop().expect("writer stack");
            let r = writer.write_element(&mut out, el);
            self.stack.push(writer);
            r
        };
        self.out = out;
        r
    }
    pub(crate) fn is_sequence(el: &Element) -> bool {
        el.vr() == Vr::SQ || matches!(el.value, Value::Sequence(_))
    }
    pub(crate) fn begin_sequence(&mut self, el: &Element) -> Result<Vec<DataSet>, Error> {
        let top = self.top().clone();
        top.validate(el)?;
        let (tag, vr) = (el.tag(), el.vr());
        let Value::Sequence(items) = &el.value else {
            return Err(write_error(
                Op::Value,
                tag,
                vr,
                element_length(el),
                Error::msg("dicom: sequence VR requires core.SequenceValue"),
            ));
        };
        let un = vr == Vr::UN && top.explicit;
        let mut inner = top.clone();
        let items: Vec<DataSet> = if un {
            inner.explicit = false;
            inner.big = false;
            inner.encapsulated = false;
            if top.big {
                items.iter().map(little_endian_data_set).collect()
            } else {
                items.clone()
            }
        } else {
            items.clone()
        };
        top.header(
            &mut self.out,
            tag,
            if un { Vr::UN } else { vr },
            UNDEFINED_LENGTH,
        )?;
        self.stack.push(inner);
        Ok(items)
    }
    pub(crate) fn begin_item(&mut self, item: &DataSet) -> Result<(), Error> {
        let mut child = self
            .top()
            .child_for(&item.elements)
            .map_err(Self::item_error)?;
        child.in_item = true;
        child.item_offset = item.item_offset;
        self.stack.push(child);
        Ok(())
    }
    pub(crate) fn item_header(&mut self) {
        let n = self.stack.len();
        self.stack[n - 2].item_header(&mut self.out, UNDEFINED_LENGTH);
    }
    pub(crate) fn end_item(&mut self) {
        self.stack.pop();
        let top = self.top().clone();
        top.delimiter(&mut self.out, Tag::ITEM_DELIMITATION);
    }
    pub(crate) fn abandon(&mut self) {
        self.stack.pop();
    }
    pub(crate) fn end_sequence(&mut self) {
        let top = self.top().clone();
        top.delimiter(&mut self.out, Tag::SEQUENCE_DELIMITATION);
        self.stack.pop();
    }
    pub(crate) fn item_error(e: Error) -> Error {
        write_error(Op::Value, Tag::ITEM, Vr::UN, UNDEFINED_LENGTH, e)
    }
    pub(crate) fn finish(&self) -> Result<Vec<u8>, Error> {
        if self.deflated {
            return padded_deflate(&self.out, crate::deflater::DEFAULT_COMPRESSION);
        }
        Ok(self.out.data.clone())
    }
    pub(crate) fn deflated(&self) -> bool {
        self.deflated
    }
}
