use super::{Reader, Token, TokenKind};
use crate::object::error::{Error, Kind, Op, PartialContainer, PathStep, Result};
use crate::object::types::{DataSet, Element, ElementHeader, Tag, Value};
pub(super) const TOKEN_INVARIANT: &str = "dicom: reader token without its header or element";
pub(super) fn unexpected_collector_token(tok: &Token, offset: i64, context: &str) -> Error {
    let (tag, vr, length) = match tok.header {
        Some(h) => (h.tag, Some(h.vr), h.length),
        None => (Tag::default(), None, 0),
    };
    Error::parse(
        Op::ReadValue,
        offset,
        tag,
        vr,
        length,
        Some(Error::msg(format!(
            "dicom: unexpected {} while collecting {context}",
            tok.kind.text()
        ))),
    )
}
pub(super) fn eof_parse_error(offset: i64, header: Option<&ElementHeader>) -> Error {
    let (tag, vr, length) = match header {
        Some(h) => (h.tag, Some(h.vr), h.length),
        None => (Tag::default(), None, 0),
    };
    Error::parse(
        Op::ReadValue,
        offset,
        tag,
        vr,
        length,
        Some(Error::unexpected_eof()),
    )
}
impl Reader {
    pub fn read_data_set(&mut self) -> (DataSet, Option<Error>) {
        let (dataset, err) = self.read_data_set_open();
        (dataset, self.close_sinks(err))
    }
    fn read_data_set_open(&mut self) -> (DataSet, Option<Error>) {
        let (elements, err) = self.collect_elements(false);
        if let Some(err) = err {
            return (DataSet::default(), Some(err));
        }
        (
            DataSet {
                elements,
                item_offset: None,
            },
            self.incomplete_collection_error(),
        )
    }
    fn collect_elements(&mut self, in_item: bool) -> (Vec<Element>, Option<Error>) {
        let saved = self.suspend_inline_threshold(in_item);
        let result = self.collect_elements_inner(in_item);
        self.restore_inline_threshold(saved);
        result
    }
    fn collect_elements_inner(&mut self, in_item: bool) -> (Vec<Element>, Option<Error>) {
        let mut elements = Vec::new();
        loop {
            let res = self.next();
            if let Some(failure) = self.incomplete_source_failure(res.as_ref().err().cloned()) {
                return (Vec::new(), Some(failure));
            }
            let tok = match res {
                Ok(tok) => tok,
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
                    let Some(element) = tok.element else {
                        return (elements, Some(Error::msg(TOKEN_INVARIANT)));
                    };
                    self.append_projected(&mut elements, element, in_item);
                }
                TokenKind::StartSequence => {
                    let Some(header) = tok.header else {
                        return (elements, Some(Error::msg(TOKEN_INVARIANT)));
                    };
                    let discard = self.projection_begin_sequence(header.tag, in_item);
                    let res = self.collect_sequence(&header);
                    self.projection_end_sequence(discard);
                    match res {
                        Ok(items) => {
                            let element = Element {
                                header,
                                value: Value::Sequence(items),
                            };
                            self.append_projected(&mut elements, element, in_item);
                        }
                        Err(err) => return (elements, Some(err)),
                    }
                }
                TokenKind::StartPixelSequence => {
                    let Some(header) = tok.header else {
                        return (elements, Some(Error::msg(TOKEN_INVARIANT)));
                    };
                    if self.skips_pixel_sequences() {
                        let value_start = self.position();
                        if let Err(err) = self.discard_fragment_sequence(&header) {
                            return (elements, Some(err));
                        }
                        let length = self.position().saturating_sub(value_start);
                        self.record_value_location(&header, value_start, length);
                        let element = Element {
                            header,
                            value: Value::Nil,
                        };
                        self.append_projected(&mut elements, element, in_item);
                        continue;
                    }
                    match self.collect_fragment_sequence(&header) {
                        Ok(value) => {
                            self.append_projected(&mut elements, Element { header, value }, in_item)
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
    fn collect_sequence(&mut self, header: &ElementHeader) -> Result<Vec<DataSet>> {
        let mut items = Vec::new();
        loop {
            let tok = match self.next() {
                Ok(tok) => tok,
                Err(err) if err.is(Kind::Eof) => {
                    return Err(eof_parse_error(self.position(), Some(header)));
                }
                Err(err) => return Err(err),
            };
            match tok.kind {
                TokenKind::StartItem => {
                    let (elements, err) = self.collect_elements(true);
                    if let Some(err) = err {
                        return Err(err);
                    }
                    if !self.projection_discarding() {
                        items.push(DataSet {
                            elements,
                            item_offset: Some(tok.offset),
                        });
                    }
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
    fn collect_fragment_sequence(&mut self, header: &ElementHeader) -> Result<Value> {
        let mut offset_table = Vec::new();
        let mut fragments: Vec<Vec<u8>> = Vec::new();
        let mut saw_offset_table = false;
        let value_offset = self.position();
        loop {
            let start = self.position();
            let res = self.next();
            if let Some(failure) = self.incomplete_source_failure(res.as_ref().err().cloned()) {
                return Err(failure);
            }
            let tok = match res {
                Ok(tok) => tok,
                Err(err) => {
                    if self.finish_incomplete_fragments(
                        start,
                        value_offset,
                        &offset_table,
                        &fragments,
                        saw_offset_table,
                        &err,
                    ) {
                        return Ok(Value::Fragments {
                            offset_table,
                            fragments,
                        });
                    }
                    if let Some(failure) = self.incomplete_source_failure(Some(err.clone())) {
                        return Err(failure);
                    }
                    if err.is(Kind::Eof) {
                        return Err(eof_parse_error(self.position(), Some(header)));
                    }
                    return Err(err);
                }
            };
            match tok.kind {
                TokenKind::Element => {
                    let raw = match tok.element.as_ref().map(|e| &e.value) {
                        Some(Value::Raw(raw)) => raw.clone(),
                        _ => {
                            return Err(unexpected_collector_token(
                                &tok,
                                self.position(),
                                &format!("pixel sequence {}", header.tag),
                            ));
                        }
                    };
                    if !saw_offset_table {
                        offset_table = raw;
                        saw_offset_table = true;
                        continue;
                    }
                    fragments.push(raw);
                }
                TokenKind::EndSequence => {
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
                        &format!("pixel sequence {}", header.tag),
                    ));
                }
            }
        }
    }
    pub(super) fn discard_fragment_sequence(&mut self, header: &ElementHeader) -> Result<()> {
        loop {
            let tok = match self.next() {
                Ok(tok) => tok,
                Err(err) if err.is(Kind::Eof) => {
                    return Err(eof_parse_error(self.position(), Some(header)));
                }
                Err(err) => return Err(err),
            };
            match tok.kind {
                TokenKind::Element => continue,
                TokenKind::EndSequence => return Ok(()),
                _ => {
                    return Err(unexpected_collector_token(
                        &tok,
                        self.position(),
                        &format!("pixel sequence {}", header.tag),
                    ));
                }
            }
        }
    }
    pub fn read_data_set_partial(&mut self) -> (DataSet, Option<Error>) {
        if self.allow_incomplete_pixel_data {
            return (
                DataSet::default(),
                Some(Error::sentinel(Kind::IncompletePixelDataOptions)),
            );
        }
        let mut collector = PartialCollector { frames: Vec::new() };
        let (elements, err) = collector.elements(self, false);
        (
            DataSet {
                elements,
                item_offset: None,
            },
            self.close_sinks(err),
        )
    }
}
struct PartialCollector {
    frames: Vec<PartialContainer>,
}
impl PartialCollector {
    fn fail(&self, err: Error) -> Error {
        self.fail_at(err, Tag::default())
    }
    fn fail_at(&self, err: Error, leaf: Tag) -> Error {
        if err.partial_info().is_some() {
            return err;
        }
        let mut path: Vec<PathStep> = Vec::new();
        for frame in &self.frames {
            if frame.item_index < 0 {
                path.push(PathStep {
                    tag: frame.tag,
                    item_index: -1,
                });
            } else if let Some(last) = path.last_mut() {
                last.item_index = frame.item_index;
            }
        }
        let mut leaf = leaf;
        if leaf.is_zero()
            && let Some(pe) = err.parse_info()
        {
            leaf = pe.tag;
        }
        if !leaf.is_zero()
            && path
                .last()
                .is_none_or(|last| last.tag != leaf || last.item_index >= 0)
        {
            path.push(PathStep {
                tag: leaf,
                item_index: -1,
            });
        }
        Error::partial_data_set(path, self.frames.clone(), err)
    }
    fn retain(err: &mut Error, index: usize) {
        if let Some(container) = err
            .partial_info_mut()
            .and_then(|partial| partial.containers.get_mut(index))
        {
            container.retained = true;
        }
    }
    fn elements(&mut self, r: &mut Reader, in_item: bool) -> (Vec<Element>, Option<Error>) {
        let saved = r.suspend_inline_threshold(in_item);
        let result = self.elements_inner(r, in_item);
        r.restore_inline_threshold(saved);
        result
    }
    fn elements_inner(&mut self, r: &mut Reader, in_item: bool) -> (Vec<Element>, Option<Error>) {
        let mut elements = Vec::new();
        loop {
            let tok = match r.next() {
                Ok(tok) => tok,
                Err(err) => {
                    let err = if err.is(Kind::Eof) {
                        if !in_item {
                            return (elements, None);
                        }
                        eof_parse_error(r.position(), None)
                    } else {
                        err
                    };
                    return (elements, Some(self.fail(err)));
                }
            };
            match tok.kind {
                TokenKind::Element => match tok.element {
                    Some(element) => elements.push(element),
                    None => return (elements, Some(Error::msg(TOKEN_INVARIANT))),
                },
                TokenKind::StartSequence => {
                    let Some(header) = tok.header else {
                        return (elements, Some(Error::msg(TOKEN_INVARIANT)));
                    };
                    let index = self.frames.len();
                    self.frames.push(PartialContainer {
                        tag: header.tag,
                        item_index: -1,
                        offset: tok.offset,
                        retained: false,
                    });
                    let (items, mut err) = self.sequence(r, &header);
                    if err.is_none() || !items.is_empty() {
                        elements.push(Element {
                            header,
                            value: Value::Sequence(items),
                        });
                        if let Some(err) = err.as_mut() {
                            Self::retain(err, index);
                        }
                    }
                    self.frames.truncate(index);
                    if err.is_some() {
                        return (elements, err);
                    }
                }
                TokenKind::StartPixelSequence => {
                    let Some(header) = tok.header else {
                        return (elements, Some(Error::msg(TOKEN_INVARIANT)));
                    };
                    if r.skips_pixel_sequences() {
                        let value_start = r.position();
                        if let Err(err) = r.discard_fragment_sequence(&header) {
                            return (elements, Some(self.fail_at(err, header.tag)));
                        }
                        let length = r.position().saturating_sub(value_start);
                        r.record_value_location(&header, value_start, length);
                        elements.push(Element {
                            header,
                            value: Value::Nil,
                        });
                    } else {
                        match r.collect_fragment_sequence(&header) {
                            Ok(value) => elements.push(Element { header, value }),
                            Err(err) => return (elements, Some(self.fail_at(err, header.tag))),
                        }
                    }
                }
                TokenKind::EndItem => {
                    if in_item {
                        return (elements, None);
                    }
                    let err = unexpected_collector_token(&tok, r.position(), "top-level dataset");
                    return (elements, Some(self.fail(err)));
                }
                _ => {
                    let err = unexpected_collector_token(&tok, r.position(), "dataset");
                    return (elements, Some(self.fail(err)));
                }
            }
        }
    }
    fn sequence(
        &mut self,
        r: &mut Reader,
        header: &ElementHeader,
    ) -> (Vec<DataSet>, Option<Error>) {
        let mut items: Vec<DataSet> = Vec::new();
        loop {
            let tok = match r.next() {
                Ok(tok) => tok,
                Err(err) => {
                    let err = if err.is(Kind::Eof) {
                        eof_parse_error(r.position(), Some(header))
                    } else {
                        err
                    };
                    return (items, Some(self.fail(err)));
                }
            };
            match tok.kind {
                TokenKind::StartItem => {
                    let index = self.frames.len();
                    self.frames.push(PartialContainer {
                        tag: header.tag,
                        item_index: items.len() as i64,
                        offset: tok.offset,
                        retained: false,
                    });
                    let (elements, mut err) = self.elements(r, true);
                    if err.is_none() || !elements.is_empty() {
                        items.push(DataSet {
                            elements,
                            item_offset: Some(tok.offset),
                        });
                        if let Some(err) = err.as_mut() {
                            Self::retain(err, index);
                        }
                    }
                    self.frames.truncate(index);
                    if err.is_some() {
                        return (items, err);
                    }
                }
                TokenKind::EndSequence => return (items, None),
                _ => {
                    let err = unexpected_collector_token(
                        &tok,
                        r.position(),
                        &format!("sequence {}", header.tag),
                    );
                    return (items, Some(self.fail(err)));
                }
            }
        }
    }
}
