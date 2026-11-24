use super::collect::{eof_parse_error, unexpected_collector_token};
use super::deferred::Deferral;
use super::sequence::Control;
use super::source::ByteRead;
use super::{
    BoundaryRecovery, Reader, ReaderOptions, SeqType, Token, TokenKind, ValueLocation,
    ValueReference,
};
use crate::object::error::{Error, Kind, Op};
use crate::object::transfer::Syntax;
use crate::object::types::{DataSet, Element, ElementHeader, Tag, Value, Vr};
use crate::validation::{
    self, CODE_RESERVED_BYTES, Finding, HookEvent, HookPoint, HookResult, NO_ITEM, Operation, Path,
    PathStep, Report, Severity, ValidationOptions,
};
use std::fmt;
pub const ERR_HOOK_ACTION: &str =
    "dicom validation hook action is invalid for this lifecycle point";
#[derive(Clone, Debug)]
pub enum ValidatedReadError {
    Parse(Error),
    Validation(validation::Error),
    HookAction(&'static str),
}
impl fmt::Display for ValidatedReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidatedReadError::Parse(e) => e.fmt(f),
            ValidatedReadError::Validation(e) => e.fmt(f),
            ValidatedReadError::HookAction(detail) => write!(f, "{ERR_HOOK_ACTION}: {detail}"),
        }
    }
}
impl std::error::Error for ValidatedReadError {}
impl From<Error> for ValidatedReadError {
    fn from(e: Error) -> Self {
        ValidatedReadError::Parse(e)
    }
}
impl From<validation::Error> for ValidatedReadError {
    fn from(e: validation::Error) -> Self {
        ValidatedReadError::Validation(e)
    }
}
impl ValidatedReadError {
    pub fn is_validation_failed(&self) -> bool {
        matches!(self, ValidatedReadError::Validation(e) if e.is_validation_failed())
    }
    pub fn is_hook_action(&self) -> bool {
        match self {
            ValidatedReadError::HookAction(_) => true,
            ValidatedReadError::Validation(validation::Error::Hook { action, .. }) => *action,
            _ => false,
        }
    }
    fn is_parse_error(&self) -> bool {
        matches!(self, ValidatedReadError::Parse(e) if e.parse_info().is_some())
    }
    fn is_eof(&self) -> bool {
        matches!(self, ValidatedReadError::Parse(e) if e.is(Kind::Eof))
    }
}
type Result<T> = std::result::Result<T, ValidatedReadError>;
#[derive(Clone, Copy, Debug)]
struct Frame {
    tag: Tag,
    item_index: i64,
    pixel: bool,
}
enum Step {
    Token(Token),
    Filtered,
}
pub struct ValidatedReader {
    r: Reader,
    operation: Operation,
    frames: Vec<Frame>,
    nodes: i64,
    max_nodes: i64,
}
impl ValidatedReader {
    pub fn new(
        source: Box<dyn ByteRead>,
        sized: Option<(i64, i64)>,
        syntax: Syntax,
        mut reader_opts: ReaderOptions,
        mut validation_opts: ValidationOptions,
    ) -> std::result::Result<ValidatedReader, validation::Error> {
        let mut max_elements = validation_opts.max_elements;
        if max_elements == 0 {
            max_elements = validation::DEFAULT_MAX_ELEMENTS;
        }
        let mut max_depth = validation_opts.max_depth;
        if max_depth == 0 {
            max_depth = validation::DEFAULT_MAX_DEPTH;
        }
        let parser_max_depth = max_depth.saturating_mul(2).saturating_add(1);
        if reader_opts.max_sequence_depth <= 0 || reader_opts.max_sequence_depth > parser_max_depth
        {
            reader_opts.max_sequence_depth = parser_max_depth;
        }
        validation_opts.dictionary = true;
        validation_opts.big_endian = syntax.big_endian;
        validation_opts.transfer_syntax_uid = syntax.uid.to_string();
        let operation = Operation::new(validation_opts)?;
        Ok(ValidatedReader {
            r: Reader::new(source, sized, syntax, reader_opts),
            operation,
            frames: Vec::new(),
            nodes: 0,
            max_nodes: max_elements,
        })
    }
    pub fn set_deferral(&mut self, d: &Deferral) {
        self.r.set_deferral(d);
    }
    pub fn validation_report(&self) -> Report {
        self.operation.report()
    }
    pub fn position(&self) -> i64 {
        self.r.position()
    }
    pub fn boundary_recoveries(&self) -> (Vec<BoundaryRecovery>, bool) {
        self.r.boundary_recoveries()
    }
    #[doc(hidden)]
    pub fn recorded_values(&self) -> Vec<(ValueLocation, ValueReference)> {
        self.r.recorded_values()
    }
    pub fn reader(&self) -> &Reader {
        &self.r
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Token> {
        if let Some(failure) = self.r.incomplete_source_failure(None) {
            return Err(failure.into());
        }
        if self.r.allow_incomplete_pixel_data {
            return Err(Error::kind_suffix(
                Kind::IncompletePixelDataOptions,
                ": selective or validated collector",
            )
            .into());
        }
        if self.r.incomplete_terminal {
            return Err(Error::eof().into());
        }
        if let Some(err) = &self.r.init_error {
            return Err(err.clone().into());
        }
        loop {
            if let Step::Token(tok) = self.next_token()? {
                return Ok(tok);
            }
        }
    }
    fn next_token(&mut self) -> Result<Step> {
        loop {
            if let Some(tok) = self.r.update_seq_delimiters()? {
                return self.finish(tok);
            }
            self.r.check_total_bytes()?;
            let header_offset = self.r.position();
            let header = match self.r.read_header() {
                Ok(h) => h,
                Err(err) => {
                    if err.is(Kind::Eof) && !self.r.seq_delimiters.is_empty() {
                        return Err(Error::parse(
                            Op::ReadTag,
                            self.r.position(),
                            Tag::default(),
                            None,
                            0,
                            Some(Error::unexpected_eof()),
                        )
                        .into());
                    }
                    return Err(err.into());
                }
            };
            match self.r.control_token(header)? {
                Control::Skipped => continue,
                Control::Token(mut tok) => {
                    tok.offset = header_offset;
                    return self.finish(tok);
                }
                Control::Primitive => {}
            }
            if self.r.in_pixel_sequence() {
                return Err(parse_err(
                    Op::ReadValue,
                    self.r.position(),
                    &header,
                    Error::msg(format!(
                        "dicom: unexpected tag {} inside encapsulated Pixel Data",
                        header.tag
                    )),
                )
                .into());
            }
            let header_result = self.handle_header(&header, header_offset)?;
            if header_result.skip_value || header_result.defer_value {
                if header.vr == Vr::SQ || header.is_undefined() {
                    return Err(ValidatedReadError::HookAction(
                        "header hook can only skip or defer a defined-length primitive",
                    ));
                }
                let mut tok = self.read_skipped_value(header, header_result.defer_value)?;
                tok.offset = header_offset;
                return self.finish(tok);
            }
            if header.vr == Vr::SQ || (header.vr == Vr::UN && header.is_undefined()) {
                self.r
                    .push_container(SeqType::Sequence, header, header.vr == Vr::UN)?;
                self.r.delimiter_check_pending = true;
                let mut tok = Token::with_header(TokenKind::StartSequence, header);
                tok.offset = header_offset;
                return self.finish(tok);
            }
            if (header.vr == Vr::OB || header.vr == Vr::OW)
                && header.tag == Tag::PIXEL_DATA
                && header.is_undefined()
            {
                self.r.push_pixel_sequence(header)?;
                self.r.pixel_sequence_offset_table_pending = true;
                self.r.pixel_data_bytes = 0;
                self.r.delimiter_check_pending = true;
                let mut tok = Token::with_header(TokenKind::StartPixelSequence, header);
                tok.offset = header_offset;
                return self.finish(tok);
            }
            if header.is_undefined() {
                return Err(parse_err(
                    Op::ReadValue,
                    self.r.position(),
                    &header,
                    Error::msg("dicom: undefined length values are not supported by this parser"),
                )
                .into());
            }
            let mut tok = self.r.read_defined_value_token(header)?;
            tok.offset = header_offset;
            return self.finish(tok);
        }
    }
    fn handle_header(&mut self, header: &ElementHeader, offset: i64) -> Result<HookResult> {
        let path = self.element_path(header.tag);
        if self.r.last_reserved_nonzero {
            let mut finding = Finding::new(
                path.clone(),
                header.tag,
                Some(header.vr),
                CODE_RESERVED_BYTES,
                Severity::Error,
                "explicit VR reserved bytes are non-zero",
            );
            finding.offset = self.r.last_reserved_offset;
            finding.offset_set = true;
            self.operation.add_finding(finding);
            self.r.last_reserved_nonzero = false;
        }
        let (result, err) = self.operation.handle(&HookEvent {
            point: HookPoint::ElementHeaderRead,
            path,
            header: Some(*header),
            offset,
            offset_set: true,
            ..HookEvent::default()
        });
        match err {
            Some(e) => Err(e.into()),
            None => Ok(result),
        }
    }
    fn finish(&mut self, mut tok: Token) -> Result<Step> {
        if self.consumes_node(&tok) {
            self.nodes = self.nodes.saturating_add(1);
            if self.max_nodes > 0 && self.nodes > self.max_nodes {
                let (tag, vr, length) = match tok.header {
                    Some(h) => (h.tag, Some(h.vr), h.length),
                    None => (Tag::default(), None, 0),
                };
                return Err(Error::parse(
                    Op::CheckElementCount,
                    tok.offset,
                    tag,
                    vr,
                    length,
                    Some(Error::msg(format!(
                        "dicom: maximum element count exceeded: got {} element/item nodes, limit {}",
                        self.nodes, self.max_nodes
                    ))),
                )
                .into());
            }
        }
        if tok.kind == TokenKind::Element
            && !tok.tag().is_sequence_delimiting()
            && let Some(element) = tok.element.clone()
        {
            let path = self.element_path(element.tag());
            let (result, err) = self.operation.handle(&HookEvent {
                point: HookPoint::AfterElement,
                path,
                header: tok.header,
                element: Some(element.clone()),
                offset: tok.offset,
                offset_set: true,
                ..HookEvent::default()
            });
            if let Some(e) = err {
                return Err(e.into());
            }
            if result.filter {
                return Ok(Step::Filtered);
            }
            if let Some(replacement) = result.element {
                validate_deferred_replacement(&element, &replacement)?;
                tok.header = Some(replacement.header);
                tok.element = Some(replacement);
            }
        }
        self.advance_path(&tok);
        Ok(Step::Token(tok))
    }
    fn consumes_node(&self, tok: &Token) -> bool {
        match tok.kind {
            TokenKind::StartSequence | TokenKind::StartPixelSequence => true,
            TokenKind::Element | TokenKind::StartItem => !self.inside_pixel_sequence(),
            _ => false,
        }
    }
    fn inside_pixel_sequence(&self) -> bool {
        self.frames.last().is_some_and(|f| f.pixel)
    }
    fn element_path(&self, tag: Tag) -> Path {
        let mut path: Path = self
            .frames
            .iter()
            .filter(|f| !f.pixel && f.item_index >= 0)
            .map(|f| PathStep {
                tag: f.tag,
                item_index: f.item_index,
            })
            .collect();
        path.push(PathStep {
            tag,
            item_index: NO_ITEM,
        });
        path
    }
    fn advance_path(&mut self, tok: &Token) {
        match tok.kind {
            TokenKind::StartSequence => self.frames.push(Frame {
                tag: tok.tag(),
                item_index: -1,
                pixel: false,
            }),
            TokenKind::StartPixelSequence => self.frames.push(Frame {
                tag: tok.tag(),
                item_index: -1,
                pixel: true,
            }),
            TokenKind::StartItem => {
                if let Some(last) = self.frames.last_mut()
                    && !last.pixel
                {
                    last.item_index = last.item_index.saturating_add(1);
                }
            }
            TokenKind::EndSequence => {
                self.frames.pop();
            }
            _ => {}
        }
    }
    fn read_skipped_value(&mut self, header: ElementHeader, defer_value: bool) -> Result<Token> {
        self.r.validate_defined_value_length(&header)?;
        self.r.check_element_count_limit(&header)?;
        self.r.check_element_byte_limit(&header)?;
        if defer_value {
            if self.frames.iter().any(|f| !f.pixel && f.item_index >= 0) {
                return Err(ValidatedReadError::HookAction(
                    "defer hook does not support values nested in sequence items",
                ));
            }
            if self.r.counter.r.seek_remaining().is_none() {
                return Err(ValidatedReadError::HookAction(
                    "defer hook requires a seekable source",
                ));
            }
        }
        let value_offset = self.r.position();
        self.r.skip_n(value_offset, &header, header.length as i64)?;
        if defer_value {
            self.r
                .record_value_location(&header, value_offset, header.length as i64);
        }
        self.r.element_count = self.r.element_count.saturating_add(1);
        self.r.delimiter_check_pending = true;
        Ok(Token {
            kind: TokenKind::Element,
            header: Some(header),
            element: Some(Element {
                header,
                value: Value::Nil,
            }),
            offset: 0,
        })
    }
    fn handle_completed(
        &mut self,
        mut path: Path,
        offset: i64,
        completion: Option<HookPoint>,
        mut element: Element,
    ) -> Result<(Element, bool)> {
        if let Some(point) = completion {
            let (result, err) = self.operation.handle(&HookEvent {
                point,
                path: path.clone(),
                header: Some(element.header),
                element: Some(element.clone()),
                offset,
                offset_set: true,
                ..HookEvent::default()
            });
            if let Some(e) = err {
                return Err(e.into());
            }
            if result.filter {
                return Ok((element, true));
            }
            if let Some(replacement) = result.element {
                validate_deferred_replacement(&element, &replacement)?;
                element = replacement;
                if let Some(last) = path.last_mut() {
                    last.tag = element.tag();
                }
            }
        }
        let (result, err) = self.operation.handle(&HookEvent {
            point: HookPoint::AfterElement,
            path,
            header: Some(element.header),
            element: Some(element.clone()),
            offset,
            offset_set: true,
            ..HookEvent::default()
        });
        if let Some(e) = err {
            return Err(e.into());
        }
        if result.filter {
            return Ok((element, true));
        }
        if let Some(replacement) = result.element {
            validate_deferred_replacement(&element, &replacement)?;
            element = replacement;
        }
        Ok((element, false))
    }
    pub fn read_data_set(&mut self) -> (DataSet, Option<ValidatedReadError>) {
        let (elements, err) = self.collect_elements(false);
        if let Some(err) = err {
            if err.is_parse_error() {
                self.operation.diagnose_parsed_prefix(&DataSet {
                    elements,
                    item_offset: None,
                });
            }
            return (DataSet::default(), Some(err));
        }
        let dataset = DataSet {
            elements,
            item_offset: None,
        };
        let (result, err) = self.operation.validate_parsed_data_set(dataset);
        (result.data_set, err.map(ValidatedReadError::Validation))
    }
    fn collect_elements(&mut self, in_item: bool) -> (Vec<Element>, Option<ValidatedReadError>) {
        let saved = self.r.suspend_inline_threshold(in_item);
        let result = self.collect_elements_inner(in_item);
        self.r.restore_inline_threshold(saved);
        result
    }
    fn collect_elements_inner(
        &mut self,
        in_item: bool,
    ) -> (Vec<Element>, Option<ValidatedReadError>) {
        let mut elements = Vec::new();
        loop {
            let tok = match self.next() {
                Ok(tok) => tok,
                Err(err) if err.is_eof() => {
                    if in_item {
                        let eof = eof_parse_error(self.r.position(), None);
                        return (elements, Some(eof.into()));
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
                    let path = self.element_path(header.tag);
                    let (items, err) = self.collect_sequence(&header, &path);
                    let has_items = !items.is_empty();
                    let element = Element {
                        header,
                        value: Value::Sequence(items),
                    };
                    if let Some(err) = err {
                        if has_items {
                            elements.push(element);
                        }
                        return (elements, Some(err));
                    }
                    match self.handle_completed(
                        path,
                        tok.offset,
                        Some(HookPoint::SequenceComplete),
                        element,
                    ) {
                        Ok((element, false)) => elements.push(element),
                        Ok((_, true)) => {}
                        Err(err) => return (elements, Some(err)),
                    }
                }
                TokenKind::StartPixelSequence => {
                    let Some(header) = tok.header else { continue };
                    let path = self.element_path(header.tag);
                    let value = if self.r.skips_pixel_sequences() {
                        let value_start = self.r.position();
                        if let Err(err) = self.discard_fragments(&header) {
                            return (elements, Some(err));
                        }
                        let length = self.r.position().saturating_sub(value_start);
                        self.r.record_value_location(&header, value_start, length);
                        Value::Nil
                    } else {
                        match self.collect_fragments(&header) {
                            Ok(value) => value,
                            Err(err) => return (elements, Some(err)),
                        }
                    };
                    match self.handle_completed(path, tok.offset, None, Element { header, value }) {
                        Ok((element, false)) => elements.push(element),
                        Ok((_, true)) => {}
                        Err(err) => return (elements, Some(err)),
                    }
                }
                TokenKind::EndItem => {
                    if !in_item {
                        let err = unexpected_collector_token(
                            &tok,
                            self.r.position(),
                            "top-level dataset",
                        );
                        return (elements, Some(err.into()));
                    }
                    return (elements, None);
                }
                TokenKind::StartItem | TokenKind::EndSequence => {
                    let err = unexpected_collector_token(&tok, self.r.position(), "dataset");
                    return (elements, Some(err.into()));
                }
            }
        }
    }
    fn collect_sequence(
        &mut self,
        header: &ElementHeader,
        sequence_path: &Path,
    ) -> (Vec<DataSet>, Option<ValidatedReadError>) {
        let mut items: Vec<DataSet> = Vec::new();
        loop {
            let tok = match self.next() {
                Ok(tok) => tok,
                Err(err) if err.is_eof() => {
                    let eof = eof_parse_error(self.r.position(), Some(header));
                    return (items, Some(eof.into()));
                }
                Err(err) => return (items, Some(err)),
            };
            match tok.kind {
                TokenKind::StartItem => {
                    let (elements, err) = self.collect_elements(true);
                    let item = DataSet {
                        elements,
                        item_offset: Some(tok.offset),
                    };
                    if let Some(err) = err {
                        if !item.elements.is_empty() {
                            items.push(item);
                        }
                        return (items, Some(err));
                    }
                    let mut item_path = sequence_path.clone();
                    if let Some(last) = item_path.last_mut() {
                        last.item_index = items.len() as i64;
                    }
                    let (_, err) = self.operation.handle(&HookEvent {
                        point: HookPoint::ItemComplete,
                        path: item_path,
                        data_set: Some(item.clone()),
                        offset: tok.offset,
                        offset_set: true,
                        ..HookEvent::default()
                    });
                    if let Some(e) = err {
                        return (items, Some(e.into()));
                    }
                    items.push(item);
                }
                TokenKind::EndSequence => return (items, None),
                _ => {
                    let err = unexpected_collector_token(
                        &tok,
                        self.r.position(),
                        &format!("sequence {}", header.tag),
                    );
                    return (items, Some(err.into()));
                }
            }
        }
    }
    fn discard_fragments(&mut self, header: &ElementHeader) -> Result<()> {
        loop {
            let tok = match self.next() {
                Ok(tok) => tok,
                Err(err) if err.is_eof() => {
                    return Err(eof_parse_error(self.r.position(), Some(header)).into());
                }
                Err(err) => return Err(err),
            };
            match tok.kind {
                TokenKind::Element => continue,
                TokenKind::EndSequence => return Ok(()),
                _ => {
                    return Err(unexpected_collector_token(
                        &tok,
                        self.r.position(),
                        &format!("pixel sequence {}", header.tag),
                    )
                    .into());
                }
            }
        }
    }
    fn collect_fragments(&mut self, header: &ElementHeader) -> Result<Value> {
        let mut offset_table = Vec::new();
        let mut fragments: Vec<Vec<u8>> = Vec::new();
        let mut saw_offset_table = false;
        loop {
            let tok = match self.next() {
                Ok(tok) => tok,
                Err(err) if err.is_eof() => {
                    return Err(eof_parse_error(self.r.position(), Some(header)).into());
                }
                Err(err) => return Err(err),
            };
            match tok.kind {
                TokenKind::Element => {
                    let Some(Value::Raw(raw)) = tok.element.as_ref().map(|e| &e.value) else {
                        return Err(unexpected_collector_token(
                            &tok,
                            self.r.position(),
                            &format!("pixel sequence {}", header.tag),
                        )
                        .into());
                    };
                    if !saw_offset_table {
                        offset_table = raw.clone();
                        saw_offset_table = true;
                        continue;
                    }
                    fragments.push(raw.clone());
                }
                TokenKind::EndSequence => {
                    if !saw_offset_table {
                        return Err(parse_err(
                            Op::ReadValue,
                            self.r.position(),
                            header,
                            Error::msg(
                                "dicom: encapsulated Pixel Data is missing the Basic Offset Table item",
                            ),
                        )
                        .into());
                    }
                    return Ok(Value::Fragments {
                        offset_table,
                        fragments,
                    });
                }
                _ => {
                    return Err(unexpected_collector_token(
                        &tok,
                        self.r.position(),
                        &format!("pixel sequence {}", header.tag),
                    )
                    .into());
                }
            }
        }
    }
}
fn parse_err(op: Op, offset: i64, h: &ElementHeader, err: Error) -> Error {
    Error::parse(op, offset, h.tag, Some(h.vr), h.length, Some(err))
}
fn validate_deferred_replacement(original: &Element, replacement: &Element) -> Result<()> {
    if original.value != Value::Nil
        || replacement.value != Value::Nil
        || original.header == replacement.header
    {
        return Ok(());
    }
    Err(ValidatedReadError::HookAction(
        "a skipped or deferred element replacement without a value must preserve its header",
    ))
}
