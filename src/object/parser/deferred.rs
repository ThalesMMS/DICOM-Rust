use super::{Reader, SeqType};
use crate::object::dict;
use crate::object::error::Error;
use crate::object::types::{ElementHeader, Tag, UNDEFINED_LENGTH, Vr};
use std::collections::HashSet;
pub const MAX_BULK_DATA_RULES: usize = 1024;
pub const MAX_BULK_DATA_PATH_STEPS: usize = 65;
pub const ERR_INVALID_BULK_DATA_DESCRIPTOR: &str = "dicom: invalid bulk data descriptor";
const TAG_WAVEFORM_DATA: Tag = Tag::new(0x5400, 0x1010);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BulkDataPathStep {
    pub tag: Tag,
    pub item_index: i64,
}
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BulkDataRule {
    pub vr: String,
    pub threshold: i64,
    pub path: Vec<BulkDataPathStep>,
}
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BulkDataDescriptor {
    pub rules: Vec<BulkDataRule>,
}
pub fn bulk_data_vr(vr: Vr) -> bool {
    matches!(
        vr,
        Vr::DS
            | Vr::FL
            | Vr::FD
            | Vr::IS
            | Vr::LT
            | Vr::OB
            | Vr::OD
            | Vr::OF
            | Vr::OL
            | Vr::OV
            | Vr::OW
            | Vr::SL
            | Vr::SS
            | Vr::ST
            | Vr::SV
            | Vr::UC
            | Vr::UL
            | Vr::UN
            | Vr::US
            | Vr::UT
            | Vr::UV
    )
}
fn compile_bulk_data_descriptor(d: &BulkDataDescriptor) -> Result<Vec<BulkDataRule>, Error> {
    let invalid =
        |detail: String| Error::msg(format!("{ERR_INVALID_BULK_DATA_DESCRIPTOR}: {detail}"));
    if d.rules.len() > MAX_BULK_DATA_RULES {
        return Err(invalid(format!("more than {MAX_BULK_DATA_RULES} rules")));
    }
    for (i, rule) in d.rules.iter().enumerate() {
        let vr_ok = rule.vr.is_empty() || Vr::parse(rule.vr.as_bytes()).is_some_and(bulk_data_vr);
        if rule.threshold < 0 || !vr_ok || rule.path.len() > MAX_BULK_DATA_PATH_STEPS {
            return Err(invalid(format!("rule {i}")));
        }
        for (j, step) in rule.path.iter().enumerate() {
            if step.item_index < -1
                || j.saturating_add(1) == rule.path.len() && step.item_index != -1
                || step.tag.group == 0xfffe
            {
                return Err(invalid(format!("rule {i} path step {j}")));
            }
        }
    }
    Ok(d.rules.clone())
}
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Deferral {
    pub defer_pixel_data: bool,
    pub defer_waveform_data: bool,
    pub inline_value_bytes_threshold: i64,
    pub defer_nested_values: bool,
    pub defer_value_tags: Vec<Tag>,
    pub bulk_data_descriptor: Option<BulkDataDescriptor>,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ValueLocation {
    pub tag: Tag,
    pub value_offset: i64,
    pub length: i64,
    pub item_offset: Option<i64>,
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ValueReference {
    pub undefined_length: bool,
    pub tag: Tag,
    pub vr: Vr,
    pub offset: i64,
    pub length: i64,
    pub path: Vec<BulkDataPathStep>,
    pub big_endian: bool,
    pub transfer_syntax_uid: &'static str,
    pub explicit_vr: bool,
}
#[derive(Default)]
pub(crate) struct DeferState {
    pub(super) defer_pixel_data: bool,
    defer_waveform_data: bool,
    pub(super) inline_threshold: i64,
    pub(super) defer_nested_values: bool,
    defer_value_tags: Option<HashSet<Tag>>,
    bulk_rules: Vec<BulkDataRule>,
    recorded: Vec<(ValueLocation, ValueReference)>,
}
impl Reader {
    pub fn set_deferral(&mut self, d: &Deferral) {
        let state = &mut self.defer;
        state.defer_pixel_data = d.defer_pixel_data;
        state.defer_waveform_data = d.defer_waveform_data;
        state.inline_threshold = d.inline_value_bytes_threshold;
        state.defer_nested_values = d.defer_nested_values;
        state.defer_value_tags = if d.defer_value_tags.is_empty() {
            None
        } else {
            Some(d.defer_value_tags.iter().copied().collect())
        };
        if let Some(descriptor) = &d.bulk_data_descriptor {
            let compiled = compile_bulk_data_descriptor(descriptor).and_then(|rules| {
                if self.syntax.deflated {
                    return Err(Error::msg(format!(
                        "{ERR_INVALID_BULK_DATA_DESCRIPTOR}: source must be seekable, standard non-deflated encoding and without frame sinks"
                    )));
                }
                Ok(rules)
            });
            match compiled {
                Ok(rules) => state.bulk_rules = rules,
                Err(err) => {
                    self.init_error = Error::join(Some(err), self.init_error.take());
                }
            }
        }
    }
    pub fn value_locations(&self, tag: Tag) -> Vec<ValueLocation> {
        self.defer
            .recorded
            .iter()
            .filter(|(l, _)| l.tag == tag)
            .map(|(l, _)| *l)
            .collect()
    }
    pub(crate) fn recorded_values(&self) -> Vec<(ValueLocation, ValueReference)> {
        self.defer
            .recorded
            .iter()
            .map(|(location, reference)| {
                let mut reference = reference.clone();
                reference.offset = self
                    .root_offset
                    .saturating_add(location.value_offset.saturating_sub(self.base_offset));
                (*location, reference)
            })
            .collect()
    }
    pub fn offsets(&self) -> (i64, i64) {
        (self.base_offset, self.root_offset)
    }
    pub(crate) fn source_end(&self) -> i64 {
        self.source_end
    }
    pub(super) fn item_offset(&self) -> Option<i64> {
        let mut i = self.seq_delimiters.last()?.item_index;
        while i >= 0 {
            let frame = self.seq_delimiters.get(i as usize)?;
            if frame.base_offset >= 8 {
                return Some(frame.base_offset.saturating_sub(8) as i64);
            }
            if i == 0 {
                break;
            }
            i = self
                .seq_delimiters
                .get((i as usize).saturating_sub(1))?
                .item_index;
        }
        None
    }
    fn bulk_path(&self, tag: Tag) -> Vec<BulkDataPathStep> {
        if (self.seq_delimiters.len() / 2).saturating_add(1) > MAX_BULK_DATA_PATH_STEPS {
            return Vec::new();
        }
        let mut path: Vec<BulkDataPathStep> = self
            .seq_delimiters
            .iter()
            .filter(|frame| frame.typ == SeqType::Sequence)
            .map(|frame| BulkDataPathStep {
                tag: frame.tag,
                item_index: frame.items.saturating_sub(1),
            })
            .collect();
        path.push(BulkDataPathStep {
            tag,
            item_index: -1,
        });
        path
    }
    pub(super) fn record_value_location(
        &mut self,
        header: &ElementHeader,
        value_offset: i64,
        length: i64,
    ) {
        if length < 0 {
            return;
        }
        let location = ValueLocation {
            tag: header.tag,
            value_offset,
            length,
            item_offset: self.item_offset(),
        };
        let implicit_le = self.implicit_vr_little_endian_active();
        let explicit_be = self.explicit_big_endian_active();
        let big_endian = if implicit_le {
            false
        } else if explicit_be {
            true
        } else {
            self.syntax.big_endian
        };
        let reference = ValueReference {
            undefined_length: header.length == UNDEFINED_LENGTH,
            tag: header.tag,
            vr: header.vr,
            offset: 0,
            length,
            path: self.bulk_path(header.tag),
            big_endian,
            transfer_syntax_uid: self.syntax.uid,
            explicit_vr: self.syntax.explicit_vr && !implicit_le || explicit_be,
        };
        self.defer.recorded.push((location, reference));
    }
    pub(super) fn defer_waveform(&self, header: &ElementHeader) -> bool {
        self.defer.defer_waveform_data && header.tag == TAG_WAVEFORM_DATA
    }
    pub(super) fn defer_bulk_value(&self, header: &ElementHeader) -> bool {
        let tag = header.tag;
        if tag == Tag::new(0x0028, 0x0103)
            || tag == Tag::new(0x0028, 0x0100)
            || tag == Tag::new(0x0028, 0x3002)
        {
            return false;
        }
        if self.defer.bulk_rules.is_empty()
            || !bulk_data_vr(header.vr)
            || header.length == UNDEFINED_LENGTH
            || dict::is_private_creator(tag)
        {
            return false;
        }
        if self
            .seq_delimiters
            .iter()
            .any(|frame| frame.typ == SeqType::PixelSequence)
        {
            return false;
        }
        let mut path: Option<Vec<BulkDataPathStep>> = None;
        for rule in &self.defer.bulk_rules {
            if i64::from(header.length) <= rule.threshold
                || !rule.vr.is_empty() && rule.vr != header.vr.as_str()
            {
                continue;
            }
            if rule.path.is_empty() {
                return true;
            }
            let path = path.get_or_insert_with(|| self.bulk_path(tag));
            if rule.path.len() != path.len() {
                continue;
            }
            if rule.path.iter().zip(path.iter()).all(|(step, actual)| {
                step.tag == actual.tag
                    && (step.item_index == -1 || step.item_index == actual.item_index)
            }) {
                return true;
            }
        }
        false
    }
    pub(super) fn defer_large_value(&self, header: &ElementHeader) -> bool {
        let threshold = self.defer.inline_threshold;
        if threshold <= 0 || i64::from(header.length) <= threshold {
            return false;
        }
        if !matches!(header.vr, Vr::OB | Vr::OW | Vr::OF | Vr::OD | Vr::UN) {
            return false;
        }
        match &self.defer.defer_value_tags {
            Some(tags) => tags.contains(&header.tag),
            None => true,
        }
    }
    pub(super) fn suspend_inline_threshold(&mut self, in_item: bool) -> Option<i64> {
        if in_item && self.defer.inline_threshold > 0 && !self.defer.defer_nested_values {
            let saved = self.defer.inline_threshold;
            self.defer.inline_threshold = 0;
            return Some(saved);
        }
        None
    }
    pub(super) fn restore_inline_threshold(&mut self, saved: Option<i64>) {
        if let Some(saved) = saved {
            self.defer.inline_threshold = saved;
        }
    }
    pub(super) fn deferral_active(&self) -> bool {
        self.defer.defer_pixel_data
            || self.defer.defer_waveform_data
            || self.defer.inline_threshold != 0
            || !self.defer.bulk_rules.is_empty()
    }
    pub(super) fn skips_pixel_sequences(&self) -> bool {
        self.skip_pixel_data || self.defer.defer_pixel_data
    }
}
pub enum Rescanned {
    Raw(Vec<u8>),
    Range(i64, i64),
}
impl Reader {
    pub(crate) fn rescan_element_value(&mut self, tag: Tag) -> Result<Rescanned, Error> {
        if tag == Tag::PIXEL_DATA {
            self.skip_pixel_data = true;
        }
        loop {
            let tok = match self.next() {
                Ok(tok) => tok,
                Err(err) if err.is(crate::object::error::Kind::Eof) => {
                    return Err(Error::msg(format!("dicom: element {tag} not found")));
                }
                Err(err) => return Err(err),
            };
            match tok.kind {
                super::TokenKind::StartPixelSequence => {
                    let Some(header) = tok.header else {
                        return Err(Error::msg(super::collect::TOKEN_INVARIANT));
                    };
                    let start = self.position();
                    let skip = self.skip_pixel_data;
                    self.skip_pixel_data = true;
                    let discarded = self.discard_fragment_sequence(&header);
                    self.skip_pixel_data = skip;
                    discarded?;
                    if header.tag == tag {
                        return Ok(Rescanned::Range(start, self.position()));
                    }
                }
                super::TokenKind::Element => {
                    let Some(element) = tok.element else { continue };
                    if element.tag() != tag {
                        continue;
                    }
                    if element.header.length == UNDEFINED_LENGTH {
                        return Err(Error::msg(format!(
                            "dicom: element {tag} has undefined length"
                        )));
                    }
                    if let crate::object::types::Value::Raw(raw) = element.value {
                        return Ok(Rescanned::Raw(raw));
                    }
                    let end = self.position();
                    let start = end.saturating_sub(i64::from(element.header.length));
                    if start < self.base_offset {
                        return Err(Error::msg(format!(
                            "dicom: invalid element {tag} value position"
                        )));
                    }
                    return Ok(Rescanned::Range(start, end));
                }
                _ => {}
            }
        }
    }
}
