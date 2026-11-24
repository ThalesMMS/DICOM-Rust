use super::source;
use super::{BoundaryReason, BoundaryRecovery, Reader};
use crate::object::error::{Error, Kind, Op, Result};
use crate::object::types::{Tag, UNDEFINED_LENGTH, Vr};
const MAX_LOOKAHEAD: usize = 10 + 8 + 64;
const MAX_CHAIN: usize = 4;
fn le_tag(b: &[u8]) -> Tag {
    let byte = |i: usize| b.get(i).copied().unwrap_or(0);
    Tag::new(
        u16::from_le_bytes([byte(0), byte(1)]),
        u16::from_le_bytes([byte(2), byte(3)]),
    )
}
impl Reader {
    pub(super) fn correct_ge_text_length(&mut self, tag: Tag, vr: Vr) -> Result<bool> {
        if self.syntax.explicit_vr
            || self.current_big_endian()
            || !self.ge_text_length_eligible(tag, vr)
        {
            return Ok(false);
        }
        let value_offset = self.position();
        let (mut remaining, mut bounded) = self.ge_text_length_remaining(value_offset);
        let mut peek = MAX_LOOKAHEAD;
        if bounded && remaining < peek as i64 {
            peek = remaining.max(0) as usize;
        }
        if peek < 10 + 8 {
            return Ok(false);
        }
        let mut ahead = [0u8; MAX_LOOKAHEAD];
        let (n, err) =
            source::read_full(&mut self.counter, ahead.get_mut(..peek).unwrap_or_default());
        let ahead = ahead.get(..n).unwrap_or_default();
        self.counter.unread(ahead);
        if let Some(err) = err
            && !err.is_exactly(Kind::Eof)
            && !err.is_exactly(Kind::UnexpectedEof)
        {
            return Err(Error::parse(
                Op::ReadValue,
                value_offset,
                tag,
                Some(vr),
                0,
                Some(err),
            ));
        }
        if n < peek {
            remaining = n as i64;
            bounded = true;
        }
        if n < 10 + 8 {
            return Ok(false);
        }
        let (mut current, mut current_vr, mut at) = (tag, vr, 0usize);
        let mut terminal = false;
        for _ in 0..MAX_CHAIN {
            let window = ahead.get(at..).unwrap_or_default();
            let Some((text, next_header)) = window.split_first_chunk::<10>() else {
                return Ok(false);
            };
            let Some(next_header) = next_header.first_chunk::<8>() else {
                return Ok(false);
            };
            if !self.ge_text_length_eligible(current, current_vr) {
                return Ok(false);
            }
            if text.iter().any(|&b| !(0x20..=0x7E).contains(&b)) {
                return Ok(false);
            }
            let Some((next, next_vr)) = self.ge_text_length_public_tag(next_header) else {
                return Ok(false);
            };
            if current >= next || next_vr == Vr::SQ {
                return Ok(false);
            }
            let shifted = window.get(13..21.min(window.len())).unwrap_or_default();
            if self.ge_text_length_ambiguous_tag(shifted) {
                return Ok(false);
            }
            let [_, _, _, _, l0, l1, l2, l3] = *next_header;
            let length = u32::from_le_bytes([l0, l1, l2, l3]);
            if length == 13 {
                current = next;
                current_vr = next_vr;
                at = at.saturating_add(18);
                continue;
            }
            if length == UNDEFINED_LENGTH
                || length & 1 != 0
                || (self.max_element_bytes > 0 && length as i64 > self.max_element_bytes)
            {
                return Ok(false);
            }
            let consumed = (at as i64).saturating_add(18);
            if bounded && length as i64 > remaining.saturating_sub(consumed) {
                return Ok(false);
            }
            if self.source_end <= 0 && length as i64 > (n as i64).saturating_sub(consumed) {
                return Ok(false);
            }
            terminal = true;
            break;
        }
        if !terminal {
            return Ok(false);
        }
        let mut recovery =
            BoundaryRecovery::new(value_offset, 13, 10, BoundaryReason::GeTextLength);
        recovery.declared_vr = vr.as_str().to_string();
        recovery.used_vr = vr.as_str().to_string();
        let path = self.child_path(tag);
        self.record_boundary_path(recovery, path);
        Ok(true)
    }
    fn ge_text_length_ambiguous_tag(&self, b: &[u8]) -> bool {
        let tag = le_tag(b);
        if tag.is_private() || tag.is_sequence_delimiting() {
            return false;
        }
        if self.dictionary_knows(tag) {
            return true;
        }
        if tag.is_group_length() {
            return self.ge_text_length_public_tag(b).is_some();
        }
        false
    }
    fn ge_text_length_eligible(&self, tag: Tag, vr: Vr) -> bool {
        if tag.is_private()
            || tag.is_group_length()
            || !vr.is_string_like()
            || tag == Tag::new(0x0008, 0x0070)
            || tag == Tag::new(0x0008, 0x0080)
            || (self.max_element_bytes > 0 && self.max_element_bytes < 10)
        {
            return false;
        }
        self.single_dictionary_vr(tag) == Some(vr)
    }
    fn ge_text_length_remaining(&self, offset: i64) -> (i64, bool) {
        let mut end = 0i64;
        for bound in [self.source_end, self.counter.fence, self.max_total_bytes] {
            if bound > 0 && (end == 0 || bound < end) {
                end = bound;
            }
        }
        (end.saturating_sub(offset), end != 0)
    }
    fn ge_text_length_public_tag(&self, b: &[u8]) -> Option<(Tag, Vr)> {
        let tag = le_tag(b);
        if tag.group < 0x0008 || tag.is_private() || tag.is_sequence_delimiting() {
            return None;
        }
        if tag.is_group_length() {
            let &[_, _, _, _, l0, l1, l2, l3] = b.first_chunk::<8>()?;
            if u32::from_le_bytes([l0, l1, l2, l3]) != 4 {
                return None;
            }
            self.single_dictionary_vr(Tag::new(tag.group, 0x0010))?;
            return Some((tag, Vr::UL));
        }
        self.single_dictionary_vr(tag).map(|vr| (tag, vr))
    }
}
