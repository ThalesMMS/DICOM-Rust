use super::source;
use super::{
    BoundaryReason, BoundaryRecovery, Reader, SeqToken, SeqType, Token, TokenKind,
    required_read_error,
};
use crate::object::error::{Error, Kind, Op, Result};
use crate::object::types::{ElementHeader, Length, Tag, UNDEFINED_LENGTH};
pub(super) enum Control {
    Primitive,
    Skipped,
    Token(Token),
}
fn control_tag_error(r: &Reader, header: &ElementHeader) -> Error {
    Error::parse(
        Op::ReadTag,
        r.position(),
        header.tag,
        Some(header.vr),
        header.length,
        Some(Error::msg(format!(
            "dicom: unexpected sequence control tag: {}",
            header.tag
        ))),
    )
}
impl Reader {
    pub(super) fn control_token(&mut self, mut header: ElementHeader) -> Result<Control> {
        if !header.tag.is_sequence_delimiting() {
            return Ok(Control::Primitive);
        }
        let swapped = self.swapped_item_pending;
        self.swapped_item_pending = false;
        if self.seq_recovery.replace_wrong_delimiter
            && let Some(tok) = self.replace_wrong_delimiter(&header)
        {
            return Ok(Control::Token(tok));
        }
        if self.recover_delimiter_anomalies && self.recover_delimiter(&mut header)? {
            return Ok(Control::Skipped);
        }
        if header.tag.is_item() {
            if self.in_pixel_sequence() {
                if header.is_undefined() {
                    return Err(Error::parse(
                        Op::ReadValue,
                        self.position(),
                        header.tag,
                        Some(header.vr),
                        header.length,
                        Some(Error::msg(
                            "dicom: undefined length values are not supported by this parser",
                        )),
                    ));
                }
                return Ok(Control::Token(self.read_pixel_sequence_item_token(header)?));
            }
            self.validate_item_start(&header)?;
            let skip = self.push_container(SeqType::Item, header, false)?;
            if swapped
                && !skip
                && let Some(item) = self.seq_delimiters.last_mut()
            {
                item.explicit_big_endian = true;
            }
            self.delimiter_check_pending = true;
            if skip {
                return match self.update_seq_delimiters()? {
                    Some(tok) => Ok(Control::Token(tok)),
                    None => Err(Error::parse(
                        Op::ReadValue,
                        self.position(),
                        Tag::default(),
                        None,
                        0,
                        Some(Error::kind_suffix(
                            Kind::ContainerOverrun,
                            ": container did not end after an empty item header",
                        )),
                    )),
                };
            }
            return Ok(Control::Token(Token::with_header(
                TokenKind::StartItem,
                header,
            )));
        }
        if header.tag.is_item_delimitation() {
            validate_delimiter_length(&header, self.position())?;
            self.pop_undefined_length_sequence_token(
                &header,
                SeqType::Item,
                "dicom: item delimitation tag outside of item context",
            )?;
            self.delimiter_check_pending = true;
            return Ok(Control::Token(Token::with_header(
                TokenKind::EndItem,
                header,
            )));
        }
        if header.tag.is_sequence_delimitation() {
            validate_delimiter_length(&header, self.position())?;
            let msg = "dicom: sequence delimitation tag outside of sequence context";
            if self.in_pixel_sequence() {
                self.pop_undefined_length_sequence_token(&header, SeqType::PixelSequence, msg)?;
                self.pixel_sequence_offset_table_pending = false;
                self.delimiter_check_pending = true;
                return Ok(Control::Token(Token::with_header(
                    TokenKind::EndSequence,
                    header,
                )));
            }
            self.pop_undefined_length_sequence_token(&header, SeqType::Sequence, msg)?;
            self.delimiter_check_pending = true;
            return Ok(Control::Token(Token::with_header(
                TokenKind::EndSequence,
                header,
            )));
        }
        Err(control_tag_error(self, &header))
    }
    fn read_pixel_sequence_item_token(&mut self, header: ElementHeader) -> Result<Token> {
        self.check_value_fits(&header)?;
        self.reserve_pixel_data_bytes(&header)?;
        self.check_fragment_count_limit(&header)?;
        if self.sinks.encoded_pixel_active {
            return self.stream_encoded_item(header);
        }
        if self.skips_pixel_sequences() {
            let value_offset = self.position();
            self.skip_n(value_offset, &header, header.length as i64)?;
            if self.pixel_sequence_offset_table_pending {
                self.pixel_sequence_offset_table_pending = false;
            } else {
                self.fragment_count = self.fragment_count.saturating_add(1);
            }
            self.element_count = self.element_count.saturating_add(1);
            self.delimiter_check_pending = true;
            return Ok(Token {
                kind: TokenKind::Element,
                header: Some(header),
                element: Some(crate::object::types::Element {
                    header,
                    value: crate::object::types::Value::Nil,
                }),
                offset: 0,
            });
        }
        let saved = self.defer.inline_threshold;
        self.defer.inline_threshold = 0;
        let tok = self.read_defined_value_token(header);
        self.defer.inline_threshold = saved;
        let tok = tok?;
        if self.pixel_sequence_offset_table_pending {
            self.pixel_sequence_offset_table_pending = false;
            return Ok(tok);
        }
        self.fragment_count = self.fragment_count.saturating_add(1);
        Ok(tok)
    }
    pub(super) fn update_seq_delimiters(&mut self) -> Result<Option<Token>> {
        if !self.delimiter_check_pending {
            return Ok(None);
        }
        let Some(last) = self.seq_delimiters.last().copied() else {
            self.delimiter_check_pending = false;
            return Ok(None);
        };
        if last.length == UNDEFINED_LENGTH {
            if let Some(tok) = self.close_at_container_end()? {
                return Ok(Some(tok));
            }
            self.delimiter_check_pending = false;
            return Ok(None);
        }
        let end_offset = last.base_offset.saturating_add(last.length as u64);
        let current = self.position_u64();
        if current == end_offset {
            self.seq_delimiters.pop();
            return Ok(Some(match last.typ {
                SeqType::Sequence => Token::bare(TokenKind::EndSequence),
                SeqType::PixelSequence => {
                    self.pixel_sequence_offset_table_pending = false;
                    Token::bare(TokenKind::EndSequence)
                }
                SeqType::Item => Token::bare(TokenKind::EndItem),
            }));
        }
        if current > end_offset {
            return Err(Error::parse(
                Op::ReadValue,
                current as i64,
                Tag::default(),
                None,
                last.length,
                Some(Error::msg(format!(
                    "dicom: read past {} boundary: expected end at offset {end_offset}, got {current}",
                    last.typ.text()
                ))),
            ));
        }
        self.delimiter_check_pending = false;
        Ok(None)
    }
    pub(super) fn validate_item_start(&self, header: &ElementHeader) -> Result<()> {
        match self.top() {
            Some(parent)
                if parent.typ == SeqType::Sequence || parent.typ == SeqType::PixelSequence =>
            {
                Ok(())
            }
            _ => Err(control_tag_error(self, header)),
        }
    }
    pub(super) fn push_container(
        &mut self,
        typ: SeqType,
        header: ElementHeader,
        force_implicit: bool,
    ) -> Result<bool> {
        let (length, skip) = self.fit_child_length(typ, &header)?;
        if skip {
            return Ok(true);
        }
        if typ == SeqType::Item {
            self.count_item(&header)?;
            if let Some(parent) = self.seq_delimiters.last_mut() {
                parent.items = parent.items.saturating_add(1);
            }
        }
        self.push_sequence_token(typ, length, force_implicit)?;
        if typ == SeqType::Sequence
            && let Some(pushed) = self.seq_delimiters.last_mut()
        {
            pushed.tag = header.tag;
        }
        Ok(false)
    }
    pub(super) fn push_pixel_sequence(&mut self, header: ElementHeader) -> Result<()> {
        self.push_sequence_token(SeqType::PixelSequence, header.length, false)?;
        if let Some(pushed) = self.seq_delimiters.last_mut() {
            pushed.tag = header.tag;
        }
        Ok(())
    }
    pub(super) fn push_sequence_token(
        &mut self,
        typ: SeqType,
        length: Length,
        force: bool,
    ) -> Result<()> {
        self.check_sequence_depth_limit(length)?;
        let implicit = force || self.implicit_vr_little_endian_active();
        let index = self.seq_delimiters.len() as isize;
        let (mut defined_index, mut item_index) = (-1, -1);
        if let Some(parent) = self.seq_delimiters.last() {
            defined_index = parent.defined_index;
            item_index = parent.item_index;
        }
        if length != UNDEFINED_LENGTH {
            defined_index = index;
        }
        if typ == SeqType::Item {
            item_index = index;
        }
        let explicit_big_endian = !implicit && self.explicit_big_endian_active();
        let base_offset = self.position_u64();
        self.seq_delimiters.push(SeqToken {
            typ,
            tag: Tag::default(),
            items: 0,
            length,
            base_offset,
            implicit_vr_little_endian: implicit,
            explicit_big_endian,
            defined_index,
            item_index,
        });
        Ok(())
    }
    pub(super) fn implicit_vr_little_endian_active(&self) -> bool {
        self.top().is_some_and(|t| t.implicit_vr_little_endian)
    }
    pub(super) fn explicit_big_endian_active(&self) -> bool {
        self.top().is_some_and(|t| t.explicit_big_endian)
    }
    fn pop_undefined_length_sequence_token(
        &mut self,
        header: &ElementHeader,
        want: SeqType,
        err: &str,
    ) -> Result<()> {
        let fail = |r: &Reader| {
            Error::parse(
                Op::ReadTag,
                r.position(),
                header.tag,
                Some(header.vr),
                header.length,
                Some(Error::msg(err)),
            )
        };
        let Some(last) = self.top() else {
            return Err(fail(self));
        };
        if last.typ != want || last.length != UNDEFINED_LENGTH {
            return Err(fail(self));
        }
        self.seq_delimiters.pop();
        Ok(())
    }
    fn recover_delimiter(&mut self, header: &mut ElementHeader) -> Result<bool> {
        let offset = self.position().saturating_sub(8);
        let top = self.top().copied();
        if top.is_some_and(|t| t.typ == SeqType::PixelSequence) {
            return Ok(false);
        }
        let declared = if header.is_undefined() {
            -1
        } else {
            header.length as i64
        };
        if header.tag.is_item()
            && let Some(t) = top
            && t.typ == SeqType::Item
            && t.length != UNDEFINED_LENGTH
            && self.position_u64() == t.base_offset.saturating_add(t.length as u64)
        {
            self.count_delimiter_recovery(offset)?;
            self.record_boundary(BoundaryRecovery::new(
                offset,
                declared,
                0,
                BoundaryReason::OrphanDelimiter,
            ));
            self.delimiter_check_pending = true;
            return Ok(true);
        }
        if header.tag.group == 0xFFFE && header.tag.element == 0x0000 {
            let raw = header.length;
            let mut n = raw;
            if raw as u8 == 0x55 && (raw >> 8) as u8 == 0x4C {
                n = raw >> 16;
            }
            if n != 4 || top.is_none() {
                return Ok(false);
            }
            self.count_delimiter_recovery(offset)?;
            if let Some(err) = source::discard_n(&mut self.counter, 4) {
                return Err(Error::parse(
                    Op::ReadValue,
                    self.position(),
                    header.tag,
                    None,
                    0,
                    Some(required_read_error(err)),
                ));
            }
            self.record_boundary(BoundaryRecovery::new(
                offset,
                4,
                4,
                BoundaryReason::ItemGroupLength,
            ));
            self.delimiter_check_pending = true;
            return Ok(true);
        }
        if header.tag.is_item_delimitation() || header.tag.is_sequence_delimitation() {
            let want = if header.tag.is_sequence_delimitation() {
                SeqType::Sequence
            } else {
                SeqType::Item
            };
            if top.is_none_or(|t: SeqToken| t.typ != want || t.length != UNDEFINED_LENGTH) {
                self.count_delimiter_recovery(offset)?;
                self.record_boundary(BoundaryRecovery::new(
                    offset,
                    declared,
                    0,
                    BoundaryReason::OrphanDelimiter,
                ));
                self.delimiter_check_pending = true;
                return Ok(true);
            }
            if header.length != 0 {
                self.count_delimiter_recovery(offset)?;
                self.record_boundary(BoundaryRecovery::new(
                    offset,
                    declared,
                    0,
                    BoundaryReason::DelimiterLengthIgnored,
                ));
                header.length = 0;
            }
        }
        Ok(false)
    }
    fn replace_wrong_delimiter(&mut self, header: &ElementHeader) -> Option<Token> {
        let top = *self.top()?;
        if header.length != 0 {
            return None;
        }
        let offset = self.position().saturating_sub(8);
        if header.tag.is_sequence_delimitation()
            && top.typ == SeqType::Item
            && top.length == UNDEFINED_LENGTH
        {
            self.record_boundary(BoundaryRecovery::new(
                offset,
                0,
                0,
                BoundaryReason::WrongDelimiterReplaced,
            ));
            self.seq_delimiters.pop();
            self.delimiter_check_pending = true;
            return Some(Token::with_header(TokenKind::EndItem, *header));
        }
        if header.tag.is_item_delimitation()
            && top.typ == SeqType::Sequence
            && top.length == UNDEFINED_LENGTH
        {
            self.record_boundary(BoundaryRecovery::new(
                offset,
                0,
                0,
                BoundaryReason::WrongDelimiterReplaced,
            ));
            self.seq_delimiters.pop();
            self.delimiter_check_pending = true;
            return Some(Token::with_header(TokenKind::EndSequence, *header));
        }
        None
    }
}
fn validate_delimiter_length(header: &ElementHeader, offset: i64) -> Result<()> {
    if header.length == 0 {
        return Ok(());
    }
    Err(Error::parse(
        Op::ReadLength,
        offset,
        header.tag,
        Some(header.vr),
        header.length,
        Some(Error::msg(format!(
            "dicom: delimiter length must be zero for {}",
            header.tag
        ))),
    ))
}
