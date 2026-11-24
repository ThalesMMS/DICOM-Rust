use super::source;
use super::{Reader, SeqType, Token, TokenKind, is_pixel_data_value_tag, normalize_read_error};
use crate::object::error::{Error, Kind, Op, Result};
use crate::object::types::{Element, ElementHeader, Tag, Value, Vr};
impl Reader {
    pub(super) fn read_defined_value_token(&mut self, header: ElementHeader) -> Result<Token> {
        self.encoded_value_precheck(&header)?;
        self.validate_defined_value_length(&header)?;
        self.check_value_fits(&header)?;
        self.check_element_count_limit(&header)?;
        if self.stop_before_pixel_data && header.tag == Tag::PIXEL_DATA {
            self.element_count = self.element_count.saturating_add(1);
            return Ok(element_token(header, Value::Nil));
        }
        let incomplete_item =
            self.allow_incomplete_pixel_data && header.tag.is_item() && self.in_pixel_sequence();
        if !incomplete_item {
            self.check_element_byte_limit(&header)?;
        }
        let value_offset = self.position();
        if self.allow_incomplete_pixel_data
            && self.seq_delimiters.is_empty()
            && header.tag == Tag::PIXEL_DATA
            && !self.syntax.encapsulated
            && matches!(header.vr, Vr::OW | Vr::OB | Vr::UN)
        {
            return self.read_incomplete_native_token(header, value_offset);
        }
        if self.frame_sink_takes(&header) {
            return self.read_native_pixel_data_frames_token(header, value_offset);
        }
        if self.projection_discards_value(&header)
            && !(self.skip_pixel_data && is_pixel_data_value_tag(header.tag))
        {
            self.skip_n(value_offset, &header, header.length as i64)?;
            self.element_count = self.element_count.saturating_add(1);
            self.delimiter_check_pending = true;
            return Ok(element_token(header, Value::Nil));
        }
        if (self.skip_pixel_data || self.defer.defer_pixel_data && self.seq_delimiters.is_empty())
            && is_pixel_data_value_tag(header.tag)
            || self.defer_waveform(&header)
            || self.defer_bulk_value(&header)
            || self.defer_large_value(&header)
        {
            self.skip_n(value_offset, &header, header.length as i64)?;
            self.record_value_location(&header, value_offset, header.length as i64);
            self.element_count = self.element_count.saturating_add(1);
            self.delimiter_check_pending = true;
            return Ok(element_token(header, Value::Nil));
        }
        let data = self.read_defined_value_bytes(&header, value_offset)?;
        let data = self.to_stream_byte_order(header.vr, data);
        self.encoded_table_read(&header, &data)?;
        if header.vr == Vr::LO {
            self.capture_private_reservation(&header, Some(&data))?;
        }
        self.sink_value_read(&header, &data, value_offset)?;
        self.element_count = self.element_count.saturating_add(1);
        self.delimiter_check_pending = true;
        Ok(element_token(header, Value::Raw(data)))
    }
    fn read_defined_value_bytes(
        &mut self,
        header: &ElementHeader,
        value_offset: i64,
    ) -> Result<Vec<u8>> {
        if self.allow_incomplete_pixel_data
            && header.tag.is_item()
            && self.top().is_some_and(|t| t.typ == SeqType::PixelSequence)
        {
            return self.read_incomplete_item_bytes(header, value_offset);
        }
        let length = header.length as i64;
        if length == 0 {
            return Ok(Vec::new());
        }
        const CHUNK: i64 = 32 << 10;
        const BOUNDED: i64 = 1 << 20;
        if length <= CHUNK
            || (self.max_element_bytes > 0 && length <= BOUNDED)
            || (self.source_end > 0 && length <= self.source_end.saturating_sub(self.position()))
        {
            let mut data = vec![0u8; length as usize];
            if let (_, Some(err)) = source::read_full(&mut self.counter, &mut data) {
                return Err(self.value_read_error(value_offset, header, err));
            }
            return Ok(data);
        }
        let mut out = Vec::with_capacity(CHUNK as usize);
        let mut remaining = length;
        while remaining > 0 {
            let want = remaining.min(CHUNK) as usize;
            let start = out.len();
            out.resize(start.saturating_add(want), 0);
            let (n, err) =
                source::read_full(&mut self.counter, out.get_mut(start..).unwrap_or_default());
            out.truncate(start.saturating_add(n));
            remaining = remaining.saturating_sub(n as i64);
            if let Some(err) = err {
                return Err(self.value_read_error(value_offset, header, err));
            }
        }
        Ok(out)
    }
    pub(super) fn value_read_error(
        &self,
        value_offset: i64,
        header: &ElementHeader,
        err: Error,
    ) -> Error {
        let err = if err.is(Kind::Eof) {
            Error::unexpected_eof()
        } else {
            normalize_read_error(value_offset, self.position(), err)
        };
        Error::parse(
            Op::ReadValue,
            value_offset,
            header.tag,
            Some(header.vr),
            header.length,
            Some(err),
        )
    }
    pub(super) fn to_stream_byte_order(&self, vr: Vr, data: Vec<u8>) -> Vec<u8> {
        let swapped = self.explicit_big_endian_active() && self.syntax.is_little_endian();
        if !swapped && (!self.implicit_vr_little_endian_active() || !self.syntax.big_endian) {
            return data;
        }
        let width = vr.byte_order_width();
        if width <= 1 || !data.len().is_multiple_of(width) {
            return data;
        }
        let mut out = data;
        for unit in out.chunks_exact_mut(width) {
            unit.reverse();
        }
        out
    }
}
pub(super) fn element_token(header: ElementHeader, value: Value) -> Token {
    Token {
        kind: TokenKind::Element,
        header: Some(header),
        element: Some(Element { header, value }),
        offset: 0,
    }
}
