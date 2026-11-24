use super::collect::{eof_parse_error, unexpected_collector_token};
use super::selective::{ERR_SELECTIVE_READER_OPTIONS, SelectiveReader, SelectiveToken};
use super::source;
use super::{Reader, Token, TokenKind, is_pixel_data_value_tag};
use crate::object::dict;
use crate::object::error::{Error, Kind, Op, Result};
use crate::object::types::{DataSet, Element, ElementHeader, Tag, Value, Vr};
pub const ERR_DATA_SET_PROJECTION_OPTIONS: &str =
    "dicom: invalid metadata dataset projection options";
#[derive(Clone, Debug, Default)]
pub(crate) struct Projection {
    tags: Vec<Tag>,
    pub(super) discarding: bool,
    seek_sequences: bool,
    discarded: Vec<Tag>,
}
impl Projection {
    pub(super) fn retains(&self, tag: Tag) -> bool {
        dict::is_private_creator(tag)
            || tag == Tag::new(0x0008, 0x0005)
            || tag == Tag::new(0x0028, 0x0103)
            || tag == Tag::new(0x0028, 0x0100)
            || tag == Tag::new(0x0028, 0x3002)
            || self.tags.contains(&tag)
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReadStop {
    pub header: ElementHeader,
    pub offset: i64,
}
#[derive(Clone, Debug, Default)]
pub struct ProjectedDataSet {
    pub data_set: DataSet,
    pub discarded: Vec<Tag>,
}
impl Reader {
    pub fn read_data_set_projected(
        &mut self,
        tags: &[Tag],
        seek: bool,
    ) -> (ProjectedDataSet, Option<Error>) {
        if self.projection.is_some()
            || !self.skip_pixel_data
            || self.deferral_active()
            || self.has_sinks()
            || self.allow_incomplete_pixel_data
            || self.element_count != 0
            || !self.seq_delimiters.is_empty()
            || tags.iter().any(|t| t.is_sequence_delimiting())
        {
            return (
                ProjectedDataSet::default(),
                Some(Error::msg(ERR_DATA_SET_PROJECTION_OPTIONS)),
            );
        }
        self.projection = Some(Projection {
            tags: tags.to_vec(),
            discarding: false,
            seek_sequences: seek,
            discarded: Vec::new(),
        });
        let (data_set, err) = self.read_data_set();
        let projection = self.projection.take().unwrap_or_default();
        if err.is_some() && data_set.elements.is_empty() {
            return (ProjectedDataSet::default(), err);
        }
        (
            ProjectedDataSet {
                data_set,
                discarded: projection.discarded,
            },
            err,
        )
    }
    pub(super) fn projection_discards_value(&self, header: &ElementHeader) -> bool {
        let Some(p) = &self.projection else {
            return false;
        };
        if dict::is_private_creator(header.tag) {
            return false;
        }
        p.discarding || self.seq_delimiters.is_empty() && !p.retains(header.tag)
    }
    pub(super) fn append_projected(
        &mut self,
        elements: &mut Vec<Element>,
        mut element: Element,
        in_item: bool,
    ) {
        if let Some(p) = &mut self.projection {
            if p.discarding {
                return;
            }
            if !in_item
                && !p.retains(element.tag())
                && !(is_pixel_data_value_tag(element.tag()) && element.value == Value::Nil)
            {
                element.value = Value::Nil;
                p.discarded.push(element.tag());
            }
        }
        elements.push(element);
    }
    pub(super) fn projection_begin_sequence(&mut self, tag: Tag, in_item: bool) -> bool {
        match &mut self.projection {
            Some(p) if !in_item && !p.retains(tag) => {
                p.discarding = true;
                true
            }
            _ => false,
        }
    }
    pub(super) fn projection_end_sequence(&mut self, discard: bool) {
        if discard && let Some(p) = &mut self.projection {
            p.discarding = false;
        }
    }
    pub(super) fn projection_discarding(&self) -> bool {
        self.projection.as_ref().is_some_and(|p| p.discarding)
    }
    pub(super) fn seek_unselected_sequence(
        &mut self,
        header: &ElementHeader,
        header_offset: i64,
    ) -> Option<Token> {
        let p = self.projection.as_ref()?;
        if !p.seek_sequences
            || p.discarding
            || !self.seq_delimiters.is_empty()
            || header.vr != Vr::SQ
            || header.is_undefined()
            || p.retains(header.tag)
        {
            return None;
        }
        let n = header.length as i64;
        if n <= 0 || self.source_end <= 0 {
            return None;
        }
        let end = self.counter.pos.saturating_add(n);
        if end > self.source_end
            || (self.counter.fence > 0 && end > self.counter.fence)
            || (self.max_total_bytes > 0 && end >= self.max_total_bytes)
        {
            return None;
        }
        self.counter.r.seek_remaining()?;
        if source::discard_n(&mut self.counter, n).is_some() {
            return None;
        }
        self.delimiter_check_pending = true;
        Some(Token {
            kind: TokenKind::Element,
            header: Some(*header),
            element: Some(Element {
                header: *header,
                value: Value::Nil,
            }),
            offset: header_offset,
        })
    }
}
impl SelectiveReader {
    pub fn read_data_set_prefix(&mut self) -> Result<(DataSet, Option<ReadStop>)> {
        let mut stop = None;
        let (elements, err) = self.collect_prefix(false, &mut stop);
        if let Some(err) = err {
            return Err(err);
        }
        Ok((
            DataSet {
                elements,
                item_offset: None,
            },
            stop,
        ))
    }
    fn collect_prefix(
        &mut self,
        in_item: bool,
        stop: &mut Option<ReadStop>,
    ) -> (Vec<Element>, Option<Error>) {
        let saved = self.reader_mut().suspend_inline_threshold(in_item);
        let result = self.collect_prefix_inner(in_item, stop);
        self.reader_mut().restore_inline_threshold(saved);
        result
    }
    fn collect_prefix_inner(
        &mut self,
        in_item: bool,
        stop: &mut Option<ReadStop>,
    ) -> (Vec<Element>, Option<Error>) {
        let mut elements = Vec::new();
        loop {
            let tok = match self.next() {
                Ok(SelectiveToken::Token(tok)) => tok,
                Ok(SelectiveToken::Stop { header, offset }) => {
                    if !in_item {
                        *stop = Some(ReadStop { header, offset });
                        return (elements, None);
                    }
                    return (
                        elements,
                        Some(Error::parse(
                            Op::ReadValue,
                            self.position(),
                            header.tag,
                            Some(header.vr),
                            header.length,
                            Some(Error::msg(
                                "dicom: unexpected stop while collecting complete dataset",
                            )),
                        )),
                    );
                }
                Err(err) if err.is(Kind::Eof) => {
                    if in_item {
                        return (elements, Some(eof_parse_error(self.position(), None)));
                    }
                    return (elements, None);
                }
                Err(err) => return (elements, Some(err)),
            };
            match tok.kind {
                TokenKind::Element => {
                    if let Some(element) = tok.element {
                        elements.push(element);
                    }
                }
                TokenKind::StartSequence => {
                    let Some(header) = tok.header else { continue };
                    match self.collect_prefix_sequence(&header, stop) {
                        Ok(items) => elements.push(Element {
                            header,
                            value: Value::Sequence(items),
                        }),
                        Err(err) => return (elements, Some(err)),
                    }
                }
                TokenKind::StartPixelSequence => {
                    let Some(header) = tok.header else { continue };
                    let value_start = self.position();
                    match self.collect_prefix_fragments(&header) {
                        Ok(value) => {
                            if self.skips_pixel_data() {
                                let length = self.position().saturating_sub(value_start);
                                self.reader_mut().record_value_location(
                                    &header,
                                    value_start,
                                    length,
                                );
                            }
                            elements.push(Element { header, value });
                        }
                        Err(err) => return (elements, Some(err)),
                    }
                }
                TokenKind::EndItem => {
                    if !in_item {
                        return (
                            elements,
                            Some(unexpected_collector_token(
                                &tok,
                                self.position(),
                                "top-level dataset",
                            )),
                        );
                    }
                    return (elements, None);
                }
                TokenKind::StartItem | TokenKind::EndSequence => {
                    return (
                        elements,
                        Some(unexpected_collector_token(&tok, self.position(), "dataset")),
                    );
                }
            }
        }
    }
    fn collect_prefix_sequence(
        &mut self,
        header: &ElementHeader,
        stop: &mut Option<ReadStop>,
    ) -> Result<Vec<DataSet>> {
        let mut items = Vec::new();
        loop {
            let tok = match self.next() {
                Ok(SelectiveToken::Token(tok)) => tok,
                Ok(SelectiveToken::Stop { .. }) => continue,
                Err(err) if err.is(Kind::Eof) => {
                    return Err(eof_parse_error(self.position(), Some(header)));
                }
                Err(err) => return Err(err),
            };
            match tok.kind {
                TokenKind::StartItem => {
                    let (elements, err) = self.collect_prefix(true, stop);
                    if let Some(err) = err {
                        return Err(err);
                    }
                    items.push(DataSet {
                        elements,
                        item_offset: Some(tok.offset),
                    });
                }
                TokenKind::EndSequence => return Ok(items),
                _ => {
                    return Err(unexpected_collector_token(
                        &tok,
                        self.position(),
                        &format!("sequence {}", header.tag),
                    ));
                }
            }
        }
    }
    fn collect_prefix_fragments(&mut self, header: &ElementHeader) -> Result<Value> {
        let skip = self.skips_pixel_data();
        let mut offset_table = Vec::new();
        let mut fragments: Vec<Vec<u8>> = Vec::new();
        let mut saw_offset_table = false;
        loop {
            let tok = match self.next() {
                Ok(SelectiveToken::Token(tok)) => tok,
                Ok(SelectiveToken::Stop { .. }) => continue,
                Err(err) if err.is(Kind::Eof) => {
                    return Err(eof_parse_error(self.position(), Some(header)));
                }
                Err(err) => return Err(err),
            };
            let context = || format!("pixel sequence {}", header.tag);
            match tok.kind {
                TokenKind::Element if skip => continue,
                TokenKind::Element => {
                    let Some(Value::Raw(raw)) = tok.element.as_ref().map(|e| &e.value) else {
                        return Err(unexpected_collector_token(
                            &tok,
                            self.position(),
                            &context(),
                        ));
                    };
                    if !saw_offset_table {
                        offset_table = raw.clone();
                        saw_offset_table = true;
                        continue;
                    }
                    fragments.push(raw.clone());
                }
                TokenKind::EndSequence => {
                    if skip {
                        return Ok(Value::Nil);
                    }
                    if !saw_offset_table {
                        return Err(Error::parse(
                            Op::ReadValue,
                            self.position(),
                            header.tag,
                            Some(header.vr),
                            header.length,
                            Some(Error::msg(
                                "dicom: encapsulated Pixel Data is missing the Basic Offset Table item",
                            )),
                        ));
                    }
                    return Ok(Value::Fragments {
                        offset_table,
                        fragments,
                    });
                }
                _ => {
                    return Err(unexpected_collector_token(
                        &tok,
                        self.position(),
                        &context(),
                    ));
                }
            }
        }
    }
}
#[doc(hidden)]
pub fn prefix_requires_selective_reader() -> Error {
    Error::msg(ERR_SELECTIVE_READER_OPTIONS)
}
