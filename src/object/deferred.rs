#![forbid(unsafe_code)]
use super::dict::{self, Context};
use super::error::Error;
use super::file::{self, File, ReadFileOptions, ReadResult};
use super::obj::Object;
use super::parser::source::MemFile;
use super::parser::{
    BulkDataPathStep, MAX_BULK_DATA_PATH_STEPS, Reader, ReaderOptions, Rescanned, ValueReference,
    bulk_data_vr,
};
pub use super::parser::{Deferral as DeferOptions, ValueLocation};
use super::transfer::Syntax;
use super::types::{Element, Tag, UNDEFINED_LENGTH, Value, Vr};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
pub const ERR_DEFERRED_VALUE_REQUIRES_SEEKABLE: &str =
    "dicom: deferred value reading requires a seekable source";
pub const ERR_DEFERRED_SEQUENCE_VALUE_WRITE: &str =
    "dicom: deferred value nested in a sequence cannot be written";
pub(super) fn deflated_deferred_value_error() -> Error {
    Error::msg(format!(
        "dicom: deflated datasets cannot preserve encoded value offsets for deferred value reading: {ERR_DEFERRED_VALUE_REQUIRES_SEEKABLE}"
    ))
}
pub(super) fn requires_seekable(defer: &DeferOptions) -> bool {
    defer.defer_pixel_data || defer.defer_waveform_data || defer.bulk_data_descriptor.is_some()
}
fn needs_value_provider(defer: &DeferOptions) -> bool {
    defer.inline_value_bytes_threshold > 0 || requires_seekable(defer)
}
#[derive(Debug)]
pub(super) struct PrefixRescan {
    pub(super) threshold: Tag,
    pub(super) max_tokens: i64,
    pub(super) state: Mutex<(bool, i64)>,
}
#[derive(Debug)]
pub(crate) struct ValueProvider {
    source: Arc<[u8]>,
    base_offset: i64,
    root_offset: i64,
    source_end: i64,
    prefix: Option<PrefixRescan>,
    recorded: Mutex<Vec<(ValueLocation, ValueReference)>>,
    syntax: Syntax,
    reader_options: ReaderOptions,
    defer: DeferOptions,
    bound_items: HashSet<i64>,
}
fn count_deferred(elements: &[Element]) -> usize {
    elements
        .iter()
        .map(|e| match &e.value {
            Value::Nil => 1,
            Value::Sequence(items) => items.iter().map(|i| count_deferred(&i.elements)).sum(),
            _ => 0,
        })
        .sum()
}
fn bind_items(elements: &[Element], depth: usize, bound: &mut HashSet<i64>) -> bool {
    let mut has_bulk = false;
    for elem in elements {
        match &elem.value {
            Value::Sequence(items) => {
                if depth >= MAX_BULK_DATA_PATH_STEPS - 1 {
                    continue;
                }
                for item in items {
                    if bind_items(&item.elements, depth + 1, bound) {
                        if let Some(offset) = item.item_offset {
                            bound.insert(offset);
                        }
                        has_bulk = true;
                    }
                }
            }
            Value::Nil
                if bulk_data_vr(elem.vr())
                    && (elem.header.length == UNDEFINED_LENGTH || elem.header.length > 0) =>
            {
                has_bulk = true;
            }
            _ => {}
        }
    }
    has_bulk
}
pub(super) fn attach_value_provider(
    obj: &mut Object,
    parsed: &[Tag],
    reader: &Reader,
    source: &[u8],
    syntax: Syntax,
    reader_options: ReaderOptions,
    defer: &DeferOptions,
) {
    attach_value_provider_prefix(
        obj,
        parsed,
        reader,
        source,
        syntax,
        reader_options,
        defer,
        None,
    );
}
#[allow(clippy::too_many_arguments)]
pub(super) fn attach_value_provider_prefix(
    obj: &mut Object,
    parsed: &[Tag],
    reader: &Reader,
    source: &[u8],
    syntax: Syntax,
    reader_options: ReaderOptions,
    defer: &DeferOptions,
    prefix: Option<PrefixRescan>,
) {
    if !needs_value_provider(defer) {
        return;
    }
    let duplicates = duplicate_tags(parsed);
    let counted: usize = obj
        .elements()
        .iter()
        .filter(|e| !duplicates.contains(&e.tag()))
        .map(|e| count_deferred(std::slice::from_ref(e)))
        .sum();
    if counted == 0 {
        return;
    }
    let mut bound_items = HashSet::new();
    bind_items(obj.elements(), 0, &mut bound_items);
    let (base_offset, root_offset) = reader.offsets();
    obj.value_provider = Some(Arc::new(ValueProvider {
        source: Arc::from(source),
        base_offset,
        root_offset,
        source_end: reader.source_end(),
        prefix,
        recorded: Mutex::new(reader.recorded_values()),
        syntax,
        reader_options,
        defer: defer.clone(),
        bound_items,
    }));
}
pub(super) fn duplicate_tags(tags: &[Tag]) -> Vec<Tag> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for tag in tags {
        if !seen.insert(*tag) && !out.contains(tag) {
            out.push(*tag);
        }
    }
    out
}
impl ValueProvider {
    pub fn value_locations(&self, tag: Tag) -> Vec<ValueLocation> {
        let recorded = self.recorded.lock().unwrap_or_else(|e| e.into_inner());
        recorded
            .iter()
            .filter(|(l, _)| l.tag == tag)
            .map(|(l, _)| *l)
            .collect()
    }
    fn copy_at(&self, location: &ValueLocation, w: &mut Vec<u8>) -> (i64, Option<Error>) {
        if location.length < 0 {
            return (
                0,
                Some(Error::msg(format!(
                    "dicom: invalid value location length {}",
                    location.length
                ))),
            );
        }
        if location.value_offset < self.base_offset {
            return (
                0,
                Some(Error::msg(format!(
                    "dicom: invalid value location offset {}",
                    location.value_offset
                ))),
            );
        }
        self.copy_range(
            location.value_offset,
            location.value_offset.saturating_add(location.length),
            w,
        )
    }
    fn copy_range(&self, start: i64, end: i64, w: &mut Vec<u8>) -> (i64, Option<Error>) {
        let len = self.source.len() as i64;
        let start = (self.root_offset + (start - self.base_offset)).clamp(0, len);
        let want = end - (start - self.root_offset + self.base_offset);
        let available = (len - start).min(want).max(0);
        w.extend_from_slice(&self.source[start as usize..(start + available) as usize]);
        if available < want {
            return (available, Some(Error::eof()));
        }
        (available, None)
    }
    pub(super) fn copy_value_to(&self, tag: Tag, w: &mut Vec<u8>) -> (i64, Option<Error>) {
        let all = self.value_locations(tag);
        let root: Vec<&ValueLocation> = all.iter().filter(|l| l.item_offset.is_none()).collect();
        match root.len() {
            1 => self.copy_at(root[0], w),
            0 if all.is_empty() => self.copy_rescanned(tag, w),
            0 => (
                0,
                Some(Error::msg(format!(
                    "dicom: no top-level value location for element {tag}"
                ))),
            ),
            _ => (
                0,
                Some(Error::msg(format!(
                    "dicom: value location for element {tag} is ambiguous due to duplicate tags"
                ))),
            ),
        }
    }
    fn copy_rescanned(&self, tag: Tag, w: &mut Vec<u8>) -> (i64, Option<Error>) {
        let file = MemFile::new(Rc::from(&self.source[..]));
        file.seek(self.root_offset.max(0) as usize);
        let sized = Some((file.len() as i64, file.position() as i64));
        let (rescanned, found) = match &self.prefix {
            None => {
                let mut reader =
                    Reader::new(Box::new(file), sized, self.syntax, self.reader_options);
                reader.set_deferral(&self.defer);
                let rescanned = reader.rescan_element_value(tag);
                (rescanned, reader.recorded_values())
            }
            Some(prefix) => {
                let mut state = prefix.state.lock().unwrap_or_else(|e| e.into_inner());
                let threshold = prefix.threshold;
                let reader = super::parser::SelectiveReader::new(
                    Box::new(file),
                    sized,
                    self.syntax,
                    self.reader_options,
                    Box::new(move |path, header, _| {
                        if path.len() == 1 && header.tag >= threshold {
                            Ok(super::parser::Disposition::Stop)
                        } else {
                            Ok(super::parser::Disposition::Materialize)
                        }
                    }),
                    prefix.max_tokens,
                );
                let mut reader = match reader {
                    Ok(reader) => reader,
                    Err(err) => return (0, Some(err)),
                };
                reader.set_deferral(&self.defer);
                reader.resume_selective_state(state.0, state.1);
                let rescanned = reader.rescan_element_value(tag);
                *state = reader.selective_state();
                (rescanned, reader.reader().recorded_values())
            }
        };
        {
            let mut recorded = self.recorded.lock().unwrap_or_else(|e| e.into_inner());
            for (location, reference) in found {
                if !recorded.iter().any(|(l, _)| *l == location) {
                    recorded.push((location, reference));
                }
            }
        }
        match rescanned {
            Err(err) => (0, Some(err)),
            Ok(Rescanned::Raw(raw)) => {
                w.extend_from_slice(&raw);
                (raw.len() as i64, None)
            }
            Ok(Rescanned::Range(start, end)) => self.copy_range(start, end, w),
        }
    }
    pub(super) fn copy_nested_value_to(
        &self,
        item_offset: i64,
        tag: Tag,
        w: &mut Vec<u8>,
    ) -> (i64, Option<Error>) {
        let found: Vec<ValueLocation> = self
            .value_locations(tag)
            .into_iter()
            .filter(|l| l.item_offset == Some(item_offset))
            .collect();
        match found.len() {
            0 => (
                0,
                Some(Error::msg(format!(
                    "{ERR_DEFERRED_SEQUENCE_VALUE_WRITE}: no value location for element {tag} in the item at offset {item_offset}"
                ))),
            ),
            1 => self.copy_at(&found[0], w),
            _ => (
                0,
                Some(Error::msg(format!(
                    "dicom: value location for element {tag} is ambiguous due to duplicate tags in the item at offset {item_offset}"
                ))),
            ),
        }
    }
    pub(super) fn encoded_value_length(&self, tag: Tag) -> Result<i64, Error> {
        let root: Vec<ValueLocation> = self
            .value_locations(tag)
            .into_iter()
            .filter(|l| l.item_offset.is_none())
            .collect();
        if root.len() != 1 {
            return Err(Error::msg(format!(
                "{ERR_ENCODED_LENGTH_UNAVAILABLE}: root value location is unavailable or ambiguous"
            )));
        }
        Ok(root[0].length)
    }
    fn value_reference_at(&self, location: &ValueLocation) -> Result<ValueReference, Error> {
        let recorded = self.recorded.lock().unwrap_or_else(|e| e.into_inner());
        let Some((_, reference)) = recorded.iter().find(|(l, _)| l == location) else {
            return Err(Error::msg("dicom: value location has no source reference"));
        };
        if location.value_offset < self.base_offset {
            return Err(Error::msg("dicom: invalid logical value offset"));
        }
        if self.source_end > 0 && location.length > self.source_end - location.value_offset {
            return Err(Error::msg("dicom: invalid physical value range"));
        }
        if reference.path.is_empty() || reference.path.len() > MAX_BULK_DATA_PATH_STEPS {
            return Err(Error::msg(format!(
                "dicom: source reference path exceeds {MAX_BULK_DATA_PATH_STEPS} steps"
            )));
        }
        Ok(reference.clone())
    }
    pub(super) fn item_bound(&self, item_offset: i64) -> bool {
        self.bound_items.contains(&item_offset)
    }
    pub(super) fn authenticated_root_primitive(&self, elem: &Element) -> bool {
        let undefined = elem.header.length == UNDEFINED_LENGTH;
        let roots: Vec<ValueLocation> = {
            let recorded = self.recorded.lock().unwrap_or_else(|e| e.into_inner());
            recorded
                .iter()
                .filter(|(l, _)| l.tag == elem.tag() && l.item_offset.is_none())
                .map(|(l, _)| *l)
                .collect()
        };
        let [location] = roots.as_slice() else {
            return false;
        };
        if !undefined && location.length != i64::from(elem.header.length) {
            return false;
        }
        let Ok(reference) = self.value_reference_at(location) else {
            return false;
        };
        if reference.undefined_length != undefined
            || reference.tag != elem.tag()
            || reference.vr != elem.vr()
            || reference.length != location.length
            || reference.offset < 0
            || reference.path.len() != 1
            || reference.path[0].tag != elem.tag()
            || reference.path[0].item_index != -1
        {
            return false;
        }
        if undefined {
            let encapsulated = super::transfer::lookup(reference.transfer_syntax_uid)
                .is_some_and(|s| s.encapsulated);
            return elem.tag() == Tag::PIXEL_DATA
                && matches!(elem.vr(), Vr::OB | Vr::OW)
                && encapsulated;
        }
        true
    }
}
pub(super) const ERR_ENCODED_LENGTH_UNAVAILABLE: &str =
    "dicom: encoded value length unavailable without reading the source";
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BulkDataReference {
    pub reference: ValueReference,
    pub normalization_width: usize,
}
struct BulkLocation {
    item: Object,
    location: ValueLocation,
}
impl Object {
    pub fn copy_value_to(&self, tag: Tag, w: &mut Vec<u8>) -> (i64, Option<Error>) {
        let Some(elem) = self.get(tag) else {
            return (0, Some(Error::msg(format!("dicom: missing element {tag}"))));
        };
        match &elem.value {
            Value::Raw(raw) => {
                w.extend_from_slice(raw);
                return (raw.len() as i64, None);
            }
            Value::Nil if !self.discarded.contains(&tag) => {}
            _ => {
                return (
                    0,
                    Some(Error::msg(format!(
                        "dicom: element {tag} value is not a raw byte payload"
                    ))),
                );
            }
        }
        if self.ambiguous_deferred.contains(&tag) {
            return (
                0,
                Some(Error::msg(format!(
                    "dicom: deferred value for element {tag} is ambiguous due to duplicate tags"
                ))),
            );
        }
        match &self.value_provider {
            None => (
                0,
                Some(Error::msg(format!(
                    "dicom: no value provider available for element {tag}"
                ))),
            ),
            Some(provider) => provider.copy_value_to(tag, w),
        }
    }
    pub fn copy_nested_value_to(
        &self,
        item: &Object,
        tag: Tag,
        w: &mut Vec<u8>,
    ) -> (i64, Option<Error>) {
        let Some(element) = item.get(tag) else {
            return (
                0,
                Some(Error::msg(format!(
                    "dicom: missing element {tag} in sequence item"
                ))),
            );
        };
        self.copy_nested_element(item.item_offset(), element, w)
    }
    pub(super) fn copy_nested_element(
        &self,
        item_offset: Option<i64>,
        element: &Element,
        w: &mut Vec<u8>,
    ) -> (i64, Option<Error>) {
        let tag = element.tag();
        match &element.value {
            Value::Raw(raw) => {
                w.extend_from_slice(raw);
                return (raw.len() as i64, None);
            }
            Value::Nil => {}
            _ => {
                return (
                    0,
                    Some(Error::msg(format!(
                        "dicom: element {tag} value is not a raw byte payload"
                    ))),
                );
            }
        }
        let Some(item_offset) = item_offset else {
            return (
                0,
                Some(Error::msg(format!(
                    "{ERR_DEFERRED_SEQUENCE_VALUE_WRITE}: element {tag} is in an item without a source offset"
                ))),
            );
        };
        match &self.value_provider {
            None => (
                0,
                Some(Error::msg(format!(
                    "dicom: no value provider available for element {tag}"
                ))),
            ),
            Some(provider) => provider.copy_nested_value_to(item_offset, tag, w),
        }
    }
    pub fn value_locations(&self, tag: Tag) -> Vec<ValueLocation> {
        match &self.value_provider {
            Some(provider) => provider.value_locations(tag),
            None => Vec::new(),
        }
    }
    pub fn has_value_provider(&self) -> bool {
        self.value_provider.is_some()
    }
    pub fn has_discarded_values(&self) -> bool {
        !self.discarded.is_empty()
    }
    fn deferred_bulk_location(
        &self,
        path: &[BulkDataPathStep],
    ) -> Result<Option<BulkLocation>, Error> {
        if path.is_empty() || path.len() > MAX_BULK_DATA_PATH_STEPS {
            return Err(Error::msg("dicom: invalid bulk data path"));
        }
        let not_found = || Error::msg("dicom: bulk data sequence path not found");
        let mut item = self.clone();
        let mut ancestors: Vec<Option<i64>> = Vec::new();
        for (i, step) in path.iter().enumerate() {
            if i == path.len() - 1 {
                if step.item_index != -1 {
                    return Err(Error::msg("dicom: bulk data leaf requires ItemIndex -1"));
                }
                break;
            }
            if step.item_index < 0 {
                return Err(Error::msg(
                    "dicom: reference path requires concrete item indexes",
                ));
            }
            let mut items = item.get_sequence(step.tag).ok_or_else(not_found)?;
            let index = step.item_index as usize;
            if index >= items.len() {
                return Err(not_found());
            }
            let next = items.swap_remove(index);
            ancestors.push(next.item_offset());
            item = next;
        }
        let tag = path[path.len() - 1].tag;
        let Some(elem) = item.get(tag).cloned() else {
            return Ok(None);
        };
        if !matches!(elem.value, Value::Nil)
            || !bulk_data_vr(elem.vr())
            || elem.header.length_set && elem.header.length == 0
        {
            return Ok(None);
        }
        let Some(provider) = &self.value_provider else {
            return Err(Error::msg(
                "dicom: deferred value has no value provider retaining its source",
            ));
        };
        if ancestors
            .iter()
            .any(|offset| !offset.is_some_and(|o| provider.bound_items.contains(&o)))
        {
            return Err(Error::msg(
                "dicom: deferred sequence item is not bound to this source",
            ));
        }
        let item_offset = item.item_offset();
        let nested = path.len() > 1;
        let found: Vec<ValueLocation> = provider
            .value_locations(tag)
            .into_iter()
            .filter(|l| {
                l.item_offset.is_some() == nested
                    && (l.item_offset.is_none() || l.item_offset == item_offset)
            })
            .collect();
        if found.len() != 1 {
            return Err(Error::msg(format!(
                "dicom: deferred value {tag} has {} matching source locations",
                found.len()
            )));
        }
        let location = found[0];
        if elem.header.length_set
            && elem.header.length != UNDEFINED_LENGTH
            && i64::from(elem.header.length) != location.length
        {
            return Err(Error::msg(
                "dicom: deferred value length differs from source",
            ));
        }
        let reference = provider.value_reference_at(&location)?;
        if reference.vr != elem.vr() {
            let contextual =
                dict::by_tag(tag).is_some_and(|entry| entry.context == Some(Context::Xs));
            if reference.explicit_vr || reference.vr != Vr::US || elem.vr() != Vr::SS || !contextual
            {
                return Err(Error::msg(
                    "dicom: deferred value encoding differs from source",
                ));
            }
        }
        if reference.path != path {
            return Err(Error::msg("dicom: deferred source path mismatch"));
        }
        Ok(Some(BulkLocation { item, location }))
    }
    pub fn deferred_bulk_data_reference(
        &self,
        path: &[BulkDataPathStep],
    ) -> Result<Option<BulkDataReference>, Error> {
        let Some(found) = self.deferred_bulk_location(path)? else {
            return Ok(None);
        };
        let Some(provider) = &self.value_provider else {
            return Ok(None);
        };
        let mut reference = provider.value_reference_at(&found.location)?;
        if reference.path != path {
            return Err(Error::msg("dicom: deferred source path mismatch"));
        }
        if let Some(elem) = found.item.get(path[path.len() - 1].tag) {
            reference.vr = elem.vr();
        }
        let normalization_width = if reference.big_endian != self.value_big_endian() {
            reference.vr.byte_order_width()
        } else {
            0
        };
        Ok(Some(BulkDataReference {
            reference,
            normalization_width,
        }))
    }
    pub fn copy_bulk_data_value_to(
        &self,
        path: &[BulkDataPathStep],
        w: &mut Vec<u8>,
    ) -> (i64, Option<Error>) {
        self.copy_bulk_data(path, 0, -1, w)
    }
    pub fn copy_bulk_data_range_to(
        &self,
        path: &[BulkDataPathStep],
        offset: i64,
        length: i64,
        w: &mut Vec<u8>,
    ) -> (i64, Option<Error>) {
        if offset < 0 || length < 0 {
            return (
                0,
                Some(Error::msg("dicom: invalid deferred bulk data range")),
            );
        }
        self.copy_bulk_data(path, offset, length, w)
    }
    fn copy_bulk_data(
        &self,
        path: &[BulkDataPathStep],
        offset: i64,
        length: i64,
        w: &mut Vec<u8>,
    ) -> (i64, Option<Error>) {
        let found = match self.deferred_bulk_location(path) {
            Err(err) => return (0, Some(err)),
            Ok(None) => {
                return (
                    0,
                    Some(Error::msg(
                        "dicom: path does not identify a deferred bulk value",
                    )),
                );
            }
            Ok(Some(found)) => found,
        };
        let Some(provider) = &self.value_provider else {
            return (0, None);
        };
        let mut location = found.location;
        if length >= 0 {
            let undefined = found
                .item
                .get(location.tag)
                .is_none_or(|e| !e.header.length_set || e.header.length == UNDEFINED_LENGTH);
            if undefined
                || offset > location.length
                || length > location.length - offset
                || location.value_offset > i64::MAX - offset
            {
                return (
                    0,
                    Some(Error::msg("dicom: invalid deferred bulk data range")),
                );
            }
            location.value_offset += offset;
            location.length = length;
        }
        provider.copy_at(&location, w)
    }
}
impl File {
    pub fn value_locations(&self, tag: Tag) -> Vec<ValueLocation> {
        self.dataset.value_locations(tag)
    }
}
pub fn read_file_deferred(
    data: &[u8],
    seekable: bool,
    opts: ReadFileOptions,
    defer: &DeferOptions,
) -> ReadResult {
    let mut defer = defer.clone();
    if !seekable {
        if requires_seekable(&defer) {
            return Err((None, Error::msg(ERR_DEFERRED_VALUE_REQUIRES_SEEKABLE)));
        }
        defer.inline_value_bytes_threshold = 0;
    }
    file::read_file_with_deferral(data, opts, Some(&defer))
}
#[doc(hidden)]
pub fn read_data_set_deferred(
    data: &[u8],
    seekable: bool,
    syntax: Syntax,
    opts: ReadFileOptions,
    defer: &DeferOptions,
) -> Result<Object, Error> {
    if requires_seekable(defer) {
        if !seekable {
            return Err(Error::msg(ERR_DEFERRED_VALUE_REQUIRES_SEEKABLE));
        }
        if syntax.deflated {
            return Err(deflated_deferred_value_error());
        }
    }
    let source: Rc<[u8]> = Rc::from(data);
    if syntax.deflated || !seekable {
        return file::read_data_set_with_options(Box::new(MemFile::new(source)), syntax, opts);
    }
    file::read_data_set_with_deferral(source, syntax, opts, defer)
}
pub fn open_source_file(path: &str) -> Result<Vec<u8>, Error> {
    file::read_source_file(path)
}
fn has_recorded_location(reader: &Reader, elements: &[Element]) -> bool {
    elements.iter().any(|e| match &e.value {
        Value::Nil => reader.value_locations(e.tag()).len() == 1,
        Value::Sequence(items) => items
            .iter()
            .any(|item| has_recorded_location(reader, &item.elements)),
        _ => false,
    })
}
pub(super) fn attach_value_provider_validated(
    obj: &mut Object,
    parsed: &[Tag],
    reader: &Reader,
    source: &[u8],
    syntax: Syntax,
    reader_options: ReaderOptions,
    defer: &DeferOptions,
) {
    if !needs_value_provider(defer) && !has_recorded_location(reader, obj.elements()) {
        return;
    }
    let duplicates = duplicate_tags(parsed);
    let counted: usize = obj
        .elements()
        .iter()
        .filter(|e| !duplicates.contains(&e.tag()))
        .map(|e| count_deferred(std::slice::from_ref(e)))
        .sum();
    if counted == 0 {
        return;
    }
    let mut bound_items = HashSet::new();
    bind_items(obj.elements(), 0, &mut bound_items);
    let (base_offset, root_offset) = reader.offsets();
    obj.value_provider = Some(Arc::new(ValueProvider {
        source: Arc::from(source),
        base_offset,
        root_offset,
        source_end: reader.source_end(),
        prefix: None,
        recorded: Mutex::new(reader.recorded_values()),
        syntax,
        reader_options,
        defer: defer.clone(),
        bound_items,
    }));
}
