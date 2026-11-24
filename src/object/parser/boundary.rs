use super::{Reader, SeqType, Token, TokenKind};
use crate::object::error::{Error, Kind, Op, Result};
use crate::object::types::{ElementHeader, Length, Tag, UNDEFINED_LENGTH};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoundaryReason {
    ItemOverrun,
    SequenceOverrun,
    ItemHeaderAtEnd,
    MissingDelimiter,
    ImplicitVrElement,
    SwappedItemTag,
    DelimiterLengthIgnored,
    OrphanDelimiter,
    ItemGroupLength,
    OxPixelData,
    DictionaryVr,
    DictionaryLengthField,
    PrivateUlLength,
    ShortLengthUn,
    GeTextLength,
    ObowAsSequence,
    VoiLutAsSequence,
    WrongDelimiterReplaced,
    PrivateSequenceBytes,
    ImplicitVrFileMeta,
    ZlibDeflate,
}
impl BoundaryReason {
    pub fn text(self) -> &'static str {
        match self {
            BoundaryReason::ItemOverrun => "item-overruns-container",
            BoundaryReason::SequenceOverrun => "sequence-overruns-container",
            BoundaryReason::ItemHeaderAtEnd => "item-header-at-container-end",
            BoundaryReason::MissingDelimiter => "missing-delimiter-at-container-end",
            BoundaryReason::ImplicitVrElement => "implicit-vr-element",
            BoundaryReason::SwappedItemTag => "swapped-item-tag",
            BoundaryReason::DelimiterLengthIgnored => "delimiter-length-ignored",
            BoundaryReason::OrphanDelimiter => "orphan-delimiter-skipped",
            BoundaryReason::ItemGroupLength => "item-group-length-skipped",
            BoundaryReason::OxPixelData => "ox-pixel-data-read-as-ow",
            BoundaryReason::DictionaryVr => "dictionary-vr-preferred",
            BoundaryReason::DictionaryLengthField => "dictionary-length-field-size",
            BoundaryReason::PrivateUlLength => "private-ul-length-6-read-as-4",
            BoundaryReason::ShortLengthUn => "short-length-un",
            BoundaryReason::GeTextLength => "ge-text-length-13-read-as-10",
            BoundaryReason::ObowAsSequence => "undefined-length-ob-ow-read-as-sq",
            BoundaryReason::VoiLutAsSequence => "voi-lut-sequence-read-as-sq",
            BoundaryReason::WrongDelimiterReplaced => "wrong-delimiter-replaced",
            BoundaryReason::PrivateSequenceBytes => "private-sequence-kept-as-bytes",
            BoundaryReason::ImplicitVrFileMeta => "implicit-vr-file-meta",
            BoundaryReason::ZlibDeflate => "zlib-deflate-wrapper",
        }
    }
}
pub const DEFAULT_MAX_BOUNDARY_RECOVERIES: i64 = 128;
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BoundaryRecovery {
    pub path: String,
    pub offset: i64,
    pub declared_length: i64,
    pub available_length: i64,
    pub reason: BoundaryReason,
    pub declared_vr: String,
    pub used_vr: String,
}
impl BoundaryRecovery {
    pub fn new(
        offset: i64,
        declared_length: i64,
        available_length: i64,
        reason: BoundaryReason,
    ) -> BoundaryRecovery {
        BoundaryRecovery {
            path: String::new(),
            offset,
            declared_length,
            available_length,
            reason,
            declared_vr: String::new(),
            used_vr: String::new(),
        }
    }
}
impl Reader {
    pub(super) fn record_boundary(&mut self, mut recovery: BoundaryRecovery) {
        if !self.boundary_has_room() {
            return;
        }
        if recovery.path.is_empty() {
            recovery.path = self.structural_path();
        }
        self.boundary_recoveries.push(recovery);
    }
    pub(super) fn record_boundary_path(&mut self, mut recovery: BoundaryRecovery, path: String) {
        if !self.boundary_has_room() {
            return;
        }
        recovery.path = path;
        self.boundary_recoveries.push(recovery);
    }
    fn boundary_has_room(&mut self) -> bool {
        let limit = if self.max_boundary_recoveries <= 0 {
            DEFAULT_MAX_BOUNDARY_RECOVERIES
        } else {
            self.max_boundary_recoveries
        };
        if self.boundary_recoveries.len() as i64 >= limit {
            self.boundary_recoveries_truncated = true;
            return false;
        }
        true
    }
    pub(super) fn child_path(&self, tag: Tag) -> String {
        let mut path = self.structural_path();
        if !path.is_empty() {
            path.push('.');
        }
        path + &tag.to_string()
    }
    pub(super) fn structural_path(&self) -> String {
        let mut b = String::new();
        for (i, token) in self.seq_delimiters.iter().enumerate() {
            if token.typ != SeqType::Sequence && token.typ != SeqType::PixelSequence {
                continue;
            }
            if !b.is_empty() {
                b.push('.');
            }
            b.push_str(&token.tag.to_string());
            if self
                .seq_delimiters
                .get(i.saturating_add(1))
                .is_some_and(|next| next.typ == SeqType::Item)
            {
                b.push_str(&format!("[{}]", token.items.saturating_sub(1)));
            }
        }
        b
    }
    pub(super) fn defined_container(&self) -> Option<(usize, u64)> {
        let top = self.seq_delimiters.last()?;
        if top.defined_index < 0 {
            return None;
        }
        let i = top.defined_index as usize;
        let token = self.seq_delimiters.get(i)?;
        Some((i, token.base_offset.saturating_add(token.length as u64)))
    }
    pub(super) fn remaining_in_container(&self) -> Option<u64> {
        let (_, end) = self.defined_container()?;
        let position = self.position_u64();
        if position >= end {
            return Some(0);
        }
        Some(end.saturating_sub(position))
    }
    fn overrun_error(&self, header: &ElementHeader, available: u64) -> Error {
        Error::parse(
            Op::ReadValue,
            self.position(),
            header.tag,
            Some(header.vr),
            header.length,
            Some(Error::kind_suffix(
                Kind::ContainerOverrun,
                &format!(
                    ": {} bytes declared, {available} left",
                    header.length as u64
                ),
            )),
        )
    }
    pub(super) fn check_value_fits(&self, header: &ElementHeader) -> Result<()> {
        if header.is_undefined() {
            return Ok(());
        }
        if let Some(available) = self.remaining_in_container()
            && header.length as u64 > available
        {
            return Err(self.overrun_error(header, available));
        }
        Ok(())
    }
    pub(super) fn fit_child_length(
        &mut self,
        typ: SeqType,
        header: &ElementHeader,
    ) -> Result<(Length, bool)> {
        let length = header.length;
        if length == UNDEFINED_LENGTH {
            return Ok((length, false));
        }
        let Some(available) = self.remaining_in_container() else {
            return Ok((length, false));
        };
        if length as u64 <= available {
            return Ok((length, false));
        }
        if !self.recover_container_overruns {
            return Err(self.overrun_error(header, available));
        }
        let path = if typ == SeqType::Sequence {
            self.child_path(header.tag)
        } else {
            let mut path = self.structural_path();
            if let Some(top) = self.seq_delimiters.last() {
                path.push_str(&format!("[{}]", top.items));
            }
            path
        };
        let reason = if typ == SeqType::Sequence {
            BoundaryReason::SequenceOverrun
        } else {
            BoundaryReason::ItemOverrun
        };
        let mut recovery =
            BoundaryRecovery::new(self.position(), length as i64, available as i64, reason);
        if available == 0 {
            if typ != SeqType::Item {
                return Err(self.overrun_error(header, available));
            }
            recovery.reason = BoundaryReason::ItemHeaderAtEnd;
            self.record_boundary_path(recovery, path);
            return Ok((0, true));
        }
        self.record_boundary_path(recovery, path);
        Ok((available as Length, false))
    }
    pub(super) fn close_at_container_end(&mut self) -> Result<Option<Token>> {
        let Some((index, end)) = self.defined_container() else {
            return Ok(None);
        };
        let top = self.seq_delimiters.len().saturating_sub(1);
        if index == top {
            return Ok(None);
        }
        let position = self.position_u64();
        if position < end {
            return Ok(None);
        }
        let Some(&last) = self.seq_delimiters.get(top) else {
            return Ok(None);
        };
        if position > end || !self.recover_container_overruns {
            return Err(Error::parse(
                Op::ReadValue,
                position as i64,
                Tag::default(),
                None,
                0,
                Some(Error::kind_suffix(
                    Kind::ContainerOverrun,
                    &format!(
                        ": undefined-length {} open at the end of its container (offset {end})",
                        last.typ.text()
                    ),
                )),
            ));
        }
        self.record_boundary(BoundaryRecovery::new(
            position as i64,
            -1,
            0,
            BoundaryReason::MissingDelimiter,
        ));
        self.seq_delimiters.truncate(top);
        Ok(Some(match last.typ {
            SeqType::Item => Token::bare(TokenKind::EndItem),
            SeqType::PixelSequence => {
                self.pixel_sequence_offset_table_pending = false;
                Token::bare(TokenKind::EndSequence)
            }
            SeqType::Sequence => Token::bare(TokenKind::EndSequence),
        }))
    }
}
