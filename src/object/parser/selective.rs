use super::collect::{eof_parse_error, unexpected_collector_token};
use super::sequence::Control;
use super::{Reader, ReaderOptions, SeqType, Token, TokenKind, normalize_read_error};
use super::{required_read_error, source::ByteRead};
use crate::dictionary::private::PrivateCreatorError;
use crate::dictionary::reservations::is_private_creator_tag;
use crate::net::ul::{Ctx, Kind as CtxKind};
use crate::object::error::{Error, Kind, Op, PathStep, Result};
use crate::object::transfer::Syntax;
use crate::object::types::{Element, ElementHeader, Tag, Value, Vr};
pub const ERR_SELECTIVE_READER_OPTIONS: &str = "dicom: invalid selective reader options";
pub const ERR_SELECTIVE_READER_CALLBACK: &str = "dicom: selective reader callback failed";
pub const ERR_SELECTIVE_DISPOSITION: &str = "dicom: invalid selective reader disposition";
#[doc(hidden)]
pub const ERR_MAX_SELECTIVE_TOKENS: &str = "dicom: selective reader token limit exceeded";
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    Materialize,
    Skip,
    Stop,
    SkipSequence,
    Other(u8),
}
pub type SelectFn =
    Box<dyn FnMut(&[PathStep], &ElementHeader, i64) -> std::result::Result<Disposition, ()>>;
#[derive(Clone, Debug)]
pub enum SelectiveToken {
    Token(Token),
    Stop { header: ElementHeader, offset: i64 },
}
#[derive(Clone, Copy, Debug)]
struct Frame {
    tag: Tag,
    item_index: i64,
    pixel: bool,
}
pub(crate) fn context_error(ctx: &Ctx) -> Option<Error> {
    match ctx.err()? {
        CtxKind::DeadlineExceeded => Some(Error::msg("context deadline exceeded")),
        _ => Some(Error::sentinel(Kind::ContextCanceled)),
    }
}
struct ContextSource {
    ctx: Ctx,
    inner: Box<dyn ByteRead>,
}
impl ByteRead for ContextSource {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        if let Some(err) = context_error(&self.ctx) {
            return (0, Some(err));
        }
        self.inner.read(p)
    }
    fn read_at(&self, p: &mut [u8], off: i64) -> Option<(usize, Option<Error>)> {
        self.inner.read_at(p, off)
    }
    fn seek_remaining(&self) -> Option<i64> {
        self.inner.seek_remaining()
    }
    fn seek_forward(&mut self, n: i64) -> bool {
        self.inner.seek_forward(n)
    }
}
pub struct SelectiveReader {
    r: Reader,
    ctx: Ctx,
    select: SelectFn,
    frames: Vec<Frame>,
    stopped: bool,
    max_tokens: i64,
    tokens: i64,
}
fn parse_err(op: Op, offset: i64, h: &ElementHeader, err: Error) -> Error {
    Error::parse(op, offset, h.tag, Some(h.vr), h.length, Some(err))
}
fn is_encapsulated_pixel_data_header(h: &ElementHeader) -> bool {
    h.tag == Tag::PIXEL_DATA && (h.vr == Vr::OB || h.vr == Vr::OW) && h.is_undefined()
}
impl SelectiveReader {
    pub fn new(
        source: Box<dyn ByteRead>,
        sized: Option<(i64, i64)>,
        syntax: Syntax,
        opts: ReaderOptions,
        select: SelectFn,
        max_tokens: i64,
    ) -> Result<SelectiveReader> {
        Self::with_context(
            Ctx::background(),
            source,
            sized,
            syntax,
            opts,
            select,
            max_tokens,
        )
    }
    pub fn with_context(
        ctx: Ctx,
        source: Box<dyn ByteRead>,
        sized: Option<(i64, i64)>,
        syntax: Syntax,
        opts: ReaderOptions,
        select: SelectFn,
        max_tokens: i64,
    ) -> Result<SelectiveReader> {
        if max_tokens < 0 {
            return Err(Error::msg(ERR_SELECTIVE_READER_OPTIONS));
        }
        if let Some(err) = context_error(&ctx) {
            return Err(err);
        }
        let source: Box<dyn ByteRead> = if ctx.can_end() {
            Box::new(ContextSource {
                ctx: ctx.clone(),
                inner: source,
            })
        } else {
            source
        };
        Ok(SelectiveReader {
            r: Reader::new(source, sized, syntax, opts),
            ctx,
            select,
            frames: Vec::new(),
            stopped: false,
            max_tokens,
            tokens: 0,
        })
    }
    pub fn position(&self) -> i64 {
        self.r.position()
    }
    pub fn set_deferral(&mut self, d: &super::Deferral) {
        self.r.set_deferral(d);
    }
    pub fn reader(&self) -> &Reader {
        &self.r
    }
    pub fn set_private_dictionary(&mut self, options: &super::PrivateDictionaryOptions) {
        self.r.set_private_dictionary(options);
    }
    pub(super) fn reader_mut(&mut self) -> &mut Reader {
        &mut self.r
    }
    pub(crate) fn selective_state(&self) -> (bool, i64) {
        (self.stopped, self.tokens)
    }
    pub(crate) fn resume_selective_state(&mut self, stopped: bool, tokens: i64) {
        self.stopped = stopped;
        self.tokens = tokens;
    }
    pub(crate) fn rescan_element_value(&mut self, tag: Tag) -> Result<super::Rescanned> {
        use super::Rescanned;
        let not_found = || Error::msg(format!("dicom: element {tag} not found"));
        if tag == Tag::PIXEL_DATA {
            self.r.skip_pixel_data = true;
        }
        loop {
            let tok = match self.next() {
                Ok(SelectiveToken::Token(tok)) => tok,
                Ok(SelectiveToken::Stop { .. }) => continue,
                Err(err) if err.is(Kind::Eof) => return Err(not_found()),
                Err(err) => return Err(err),
            };
            match tok.kind {
                TokenKind::StartPixelSequence => {
                    let Some(header) = tok.header else { continue };
                    let start = self.position();
                    let skip = self.r.skip_pixel_data;
                    self.r.skip_pixel_data = true;
                    let discarded = self.discard_fragment_sequence(&header);
                    self.r.skip_pixel_data = skip;
                    discarded?;
                    if header.tag == tag {
                        return Ok(Rescanned::Range(start, self.position()));
                    }
                }
                TokenKind::Element => {
                    let Some(element) = tok.element else { continue };
                    if element.tag() != tag {
                        continue;
                    }
                    if element.header.is_undefined() {
                        return Err(Error::msg(format!(
                            "dicom: element {tag} has undefined length"
                        )));
                    }
                    if let Value::Raw(raw) = element.value {
                        return Ok(Rescanned::Raw(raw));
                    }
                    let end = self.position();
                    let start = end.saturating_sub(i64::from(element.header.length));
                    if start < self.r.offsets().0 {
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
    pub(super) fn skips_pixel_data(&self) -> bool {
        self.r.skips_pixel_sequences()
    }
    pub fn boundary_recoveries(&self) -> (Vec<super::BoundaryRecovery>, bool) {
        self.r.boundary_recoveries()
    }
    fn check_ctx(&self) -> Result<()> {
        match context_error(&self.ctx) {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    fn reserve(&mut self, header: Option<ElementHeader>, offset: i64) -> Result<()> {
        if self.max_tokens == 0 {
            return Ok(());
        }
        self.check_ctx()?;
        if self.tokens >= self.max_tokens {
            let h = header.unwrap_or_else(|| ElementHeader {
                tag: Tag::default(),
                vr: Vr::UN,
                length: 0,
                length_set: false,
            });
            let vr = header.map(|h| h.vr);
            return Err(Error::parse(
                Op::ReadTag,
                offset,
                h.tag,
                vr,
                h.length,
                Some(Error::msg(format!(
                    "{ERR_MAX_SELECTIVE_TOKENS}: limit {}",
                    self.max_tokens
                ))),
            ));
        }
        self.tokens = self.tokens.saturating_add(1);
        Ok(())
    }
    fn advance(&mut self, tok: &Token) {
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
    fn element_path(&self, tag: Tag) -> Vec<PathStep> {
        let mut path: Vec<PathStep> = self
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
            item_index: -1,
        });
        path
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<SelectiveToken> {
        loop {
            if self.stopped {
                return Err(Error::eof());
            }
            self.check_ctx()?;
            if let Some(tok) = self.r.update_seq_delimiters()? {
                let pos = self.r.position();
                self.reserve(tok.header, pos)?;
                self.advance(&tok);
                return Ok(SelectiveToken::Token(tok));
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
                        ));
                    }
                    return Err(err);
                }
            };
            self.check_ctx()?;
            self.reserve(Some(header), header_offset)?;
            match self.r.control_token(header)? {
                Control::Skipped => continue,
                Control::Token(mut tok) => {
                    tok.offset = header_offset;
                    self.advance(&tok);
                    return Ok(SelectiveToken::Token(tok));
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
                ));
            }
            let path = self.element_path(header.tag);
            let selected = (self.select)(&path, &header, header_offset);
            self.check_ctx()?;
            let disposition = match selected {
                Ok(d) => d,
                Err(()) => {
                    return Err(parse_err(
                        Op::ReadValue,
                        header_offset,
                        &header,
                        Error::msg(ERR_SELECTIVE_READER_CALLBACK),
                    ));
                }
            };
            match disposition {
                Disposition::Stop => {
                    self.stopped = true;
                    return Ok(SelectiveToken::Stop {
                        header,
                        offset: header_offset,
                    });
                }
                Disposition::Skip => {
                    if self.r.private_active() && is_private_creator_tag(header.tag) {
                        return Err(Error::msg(PrivateCreatorError.to_string()));
                    }
                    if is_encapsulated_pixel_data_header(&header) {
                        let mut tok = self.skip_pixel_sequence(header, header_offset)?;
                        tok.offset = header_offset;
                        return Ok(SelectiveToken::Token(tok));
                    }
                    if header.vr == Vr::SQ || header.is_undefined() {
                        return Err(disposition_error(&header, header_offset));
                    }
                    let mut tok = self.skip_value(header)?;
                    tok.offset = header_offset;
                    return Ok(SelectiveToken::Token(tok));
                }
                Disposition::SkipSequence => {
                    return self
                        .skip_sequence(header, header_offset)
                        .map(SelectiveToken::Token);
                }
                Disposition::Materialize => {}
                Disposition::Other(_) => return Err(disposition_error(&header, header_offset)),
            }
            if header.vr == Vr::SQ || (header.vr == Vr::UN && header.is_undefined()) {
                self.r
                    .push_container(SeqType::Sequence, header, header.vr == Vr::UN)?;
                self.r.delimiter_check_pending = true;
                let mut tok = Token::with_header(TokenKind::StartSequence, header);
                tok.offset = header_offset;
                self.advance(&tok);
                return Ok(SelectiveToken::Token(tok));
            }
            if (header.vr == Vr::OB || header.vr == Vr::OW)
                && header.tag == Tag::PIXEL_DATA
                && header.is_undefined()
            {
                self.r.push_pixel_sequence(header)?;
                self.r.pixel_sequence_offset_table_pending = true;
                self.r.delimiter_check_pending = true;
                let mut tok = Token::with_header(TokenKind::StartPixelSequence, header);
                tok.offset = header_offset;
                self.advance(&tok);
                return Ok(SelectiveToken::Token(tok));
            }
            if header.is_undefined() {
                return Err(parse_err(
                    Op::ReadValue,
                    self.r.position(),
                    &header,
                    Error::msg("dicom: undefined length values are not supported by this parser"),
                ));
            }
            let mut tok = self.r.read_defined_value_token(header)?;
            tok.offset = header_offset;
            return Ok(SelectiveToken::Token(tok));
        }
    }
    fn skip_pixel_sequence(&mut self, header: ElementHeader, header_offset: i64) -> Result<Token> {
        self.r.push_pixel_sequence(header)?;
        self.r.pixel_sequence_offset_table_pending = true;
        self.r.delimiter_check_pending = true;
        let mut start = Token::with_header(TokenKind::StartPixelSequence, header);
        start.offset = header_offset;
        self.advance(&start);
        let previous = self.r.skip_pixel_data;
        self.r.skip_pixel_data = true;
        let res = self.discard_fragment_sequence(&header);
        self.r.skip_pixel_data = previous;
        res?;
        self.check_ctx()?;
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
    fn discard_fragment_sequence(&mut self, header: &ElementHeader) -> Result<()> {
        loop {
            let tok = match self.next() {
                Ok(SelectiveToken::Token(tok)) => tok,
                Ok(SelectiveToken::Stop { .. }) => continue,
                Err(err) if err.is(Kind::Eof) => {
                    return Err(eof_parse_error(self.r.position(), Some(header)));
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
                    ));
                }
            }
        }
    }
    fn skip_value(&mut self, header: ElementHeader) -> Result<Token> {
        self.r.validate_defined_value_length(&header)?;
        self.r.check_element_count_limit(&header)?;
        let value_offset = self.r.position();
        self.skip_n(value_offset, &header, header.length as i64)?;
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
    fn seek_available(&self, n: i64) -> Option<(i64, Option<Error>)> {
        let remaining = self.r.counter.r.seek_remaining()?;
        let pos = self.r.position();
        let mut available = n;
        let mut limit_err = None;
        if self.r.max_total_bytes > 0 {
            let total_remaining = self.r.max_total_bytes.saturating_sub(pos).max(0);
            if available > total_remaining {
                available = total_remaining;
                limit_err = Some(Error::sentinel(Kind::MaxTotalBytes));
            }
        }
        let source_remaining = if self.r.source_end > 0 {
            self.r.source_end.saturating_sub(pos)
        } else {
            remaining
        }
        .max(0);
        if available > source_remaining {
            available = source_remaining;
            if limit_err.is_none() || source_remaining < self.r.max_total_bytes.saturating_sub(pos)
            {
                limit_err = Some(Error::unexpected_eof());
            }
        }
        Some((available, limit_err))
    }
    fn skip_n(&mut self, offset: i64, header: &ElementHeader, n: i64) -> Result<()> {
        self.check_ctx()?;
        if self.r.counter.r.seek_remaining().is_none() {
            return self.r.skip_n(offset, header, n);
        }
        if n == 0 {
            return Ok(());
        }
        let Some((available, limit_err)) = self.seek_available(n) else {
            return self.r.skip_n(offset, header, n);
        };
        if available > 0 {
            self.r.counter.r.seek_forward(available);
            self.r.counter.pos = self.r.counter.pos.saturating_add(available);
        }
        if let Some(err) = limit_err {
            return Err(parse_err(
                Op::ReadValue,
                offset,
                header,
                normalize_read_error(offset, self.r.position(), err),
            ));
        }
        Ok(())
    }
    fn skip_sequence(&mut self, header: ElementHeader, offset: i64) -> Result<Token> {
        if header.vr != Vr::SQ && !(header.vr == Vr::UN && header.is_undefined()) {
            return Err(disposition_error(&header, offset));
        }
        if !header.is_undefined() {
            self.skip_opaque_body(&header)?;
        } else {
            self.skip_undefined_sequence(header)?;
        }
        self.r.delimiter_check_pending = true;
        Ok(Token {
            kind: TokenKind::Element,
            header: Some(header),
            element: Some(Element {
                header,
                value: Value::Nil,
            }),
            offset,
        })
    }
    fn skip_opaque_body(&mut self, header: &ElementHeader) -> Result<()> {
        self.check_ctx()?;
        self.r.validate_defined_value_length(header)?;
        self.r.check_value_fits(header)?;
        let n = header.length as i64;
        let offset = self.r.position();
        let total = || {
            parse_err(
                Op::ReadValue,
                offset,
                header,
                Error::sentinel(Kind::MaxTotalBytes),
            )
        };
        if offset < 0 || n > i64::MAX.saturating_sub(offset) {
            return Err(total());
        }
        let max = self.r.max_total_bytes;
        if max > 0 && (offset > max || n > max.saturating_sub(offset)) {
            return Err(total());
        }
        if n == 0 {
            return Ok(());
        }
        if let Some((_, limit_err)) = self.seek_available(n) {
            if let Some(err) = limit_err {
                return Err(parse_err(Op::ReadValue, offset, header, err));
            }
            self.r.counter.r.seek_forward(n);
            self.r.counter.pos = self.r.counter.pos.saturating_add(n);
            return self.check_ctx();
        }
        if let Some(err) = super::source::discard_n(&mut self.r.counter, n) {
            return Err(parse_err(
                Op::ReadValue,
                offset,
                header,
                required_read_error(err),
            ));
        }
        Ok(())
    }
    fn skip_undefined_sequence(&mut self, header: ElementHeader) -> Result<()> {
        let base_depth = self.r.seq_delimiters.len();
        self.r
            .push_container(SeqType::Sequence, header, header.vr == Vr::UN)?;
        self.r.delimiter_check_pending = true;
        let old_skip = self.r.skip_pixel_data;
        self.r.skip_pixel_data = true;
        let res = self.skip_undefined_body(base_depth);
        self.r.skip_pixel_data = old_skip;
        res
    }
    fn skip_undefined_body(&mut self, base_depth: usize) -> Result<()> {
        while self.r.seq_delimiters.len() > base_depth {
            self.check_ctx()?;
            if let Some(tok) = self.r.update_seq_delimiters()? {
                let pos = self.r.position();
                self.reserve(tok.header, pos)?;
                continue;
            }
            self.r.check_total_bytes()?;
            let offset = self.r.position();
            let h = match self.r.read_header() {
                Ok(h) => h,
                Err(err) if err.is(Kind::Eof) => {
                    return Err(Error::parse(
                        Op::ReadTag,
                        offset,
                        Tag::default(),
                        None,
                        0,
                        Some(Error::unexpected_eof()),
                    ));
                }
                Err(err) => return Err(err),
            };
            if self.max_tokens == 0 {
                self.check_ctx()?;
            }
            self.reserve(Some(h), offset)?;
            let top_is_pixel = self
                .r
                .seq_delimiters
                .last()
                .is_some_and(|t| t.typ == SeqType::PixelSequence);
            if h.tag.is_item() && !h.is_undefined() && !top_is_pixel {
                self.r.validate_item_start(&h)?;
                self.r.check_sequence_depth_limit(h.length)?;
                self.skip_opaque_body(&h)?;
                if let Some(top) = self.r.seq_delimiters.last_mut() {
                    top.items = top.items.saturating_add(1);
                }
                self.r.delimiter_check_pending = true;
                continue;
            }
            match self.r.control_token(h)? {
                Control::Skipped | Control::Token(_) => continue,
                Control::Primitive => {}
            }
            if self.r.seq_delimiters.last().map(|parent| parent.typ) != Some(SeqType::Item) {
                return Err(parse_err(
                    Op::ReadTag,
                    offset,
                    &h,
                    Error::msg("dicom: unexpected sequence control tag"),
                ));
            }
            if h.vr == Vr::SQ || (h.vr == Vr::UN && h.is_undefined()) {
                if !h.is_undefined() {
                    self.skip_opaque_body(&h)?;
                } else {
                    self.r
                        .push_container(SeqType::Sequence, h, h.vr == Vr::UN)?;
                }
                self.r.delimiter_check_pending = true;
                continue;
            }
            if is_encapsulated_pixel_data_header(&h) {
                self.r.push_pixel_sequence(h)?;
                self.r.pixel_sequence_offset_table_pending = true;
                self.r.delimiter_check_pending = true;
                self.r.pixel_data_bytes = 0;
                continue;
            }
            if h.is_undefined() {
                return Err(parse_err(
                    Op::ReadValue,
                    offset,
                    &h,
                    Error::msg("dicom: undefined length values are not supported by this parser"),
                ));
            }
            self.r.check_value_fits(&h)?;
            if self.r.private_active() && is_private_creator_tag(h.tag) {
                self.r.read_defined_value_token(h)?;
            } else {
                self.skip_value(h)?;
            }
        }
        Ok(())
    }
}
fn disposition_error(header: &ElementHeader, offset: i64) -> Error {
    parse_err(
        Op::ReadValue,
        offset,
        header,
        Error::msg(ERR_SELECTIVE_DISPOSITION),
    )
}
