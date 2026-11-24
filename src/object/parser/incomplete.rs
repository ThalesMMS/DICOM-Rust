use super::source::{self, ByteRead, terminal_eof_cause};
use super::{Reader, ReaderOptions, SeqType, Token, value::element_token};
use crate::object::error::{Error, Kind, Op, Result};
use crate::object::transfer::{self, Syntax};
use crate::object::types::{ElementHeader, Value, Vr};
pub const MAX_INCOMPLETE_PIXEL_DATA_BYTES: i64 = 128 << 20;
pub const MAX_INCOMPLETE_PIXEL_DATA_FRAGMENTS: i64 = 100000;
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IncompletePixelData {
    pub kind: &'static str,
    pub value_offset: i64,
    pub failure_offset: i64,
    pub declared_bytes: i64,
    pub available_bytes: i64,
    pub retained_fragments: i64,
    pub discarded_fragments: i64,
    pub discarded_bytes: i64,
    pub cause: Error,
}
pub(super) fn incomplete_read_options(
    mut o: ReaderOptions,
    syntax: &Syntax,
) -> (ReaderOptions, Option<Error>) {
    if !o.allow_incomplete_pixel_data {
        return (o, None);
    }
    let options_error = Some(Error::sentinel(Kind::IncompletePixelDataOptions));
    if syntax.deflated
        || transfer::is_legacy(syntax.uid)
        || o.recover_container_overruns
        || o.recover_implicit_vr_elements
        || o.recover_delimiter_anomalies
        || o.sequence_recovery != Default::default()
        || o.vr_recovery != Default::default()
        || o.skip_pixel_data
    {
        return (o, options_error);
    }
    if o.max_element_bytes < 0
        || o.max_pixel_data_bytes < 0
        || o.max_total_bytes < 0
        || o.max_elements < 0
        || o.max_fragments < 0
        || o.max_sequence_depth < 0
    {
        return (o, options_error);
    }
    if o.max_pixel_data_bytes == 0 || o.max_pixel_data_bytes > MAX_INCOMPLETE_PIXEL_DATA_BYTES {
        o.max_pixel_data_bytes = MAX_INCOMPLETE_PIXEL_DATA_BYTES;
    }
    if o.max_fragments == 0 || o.max_fragments > MAX_INCOMPLETE_PIXEL_DATA_FRAGMENTS {
        o.max_fragments = MAX_INCOMPLETE_PIXEL_DATA_FRAGMENTS;
    }
    if o.max_element_bytes == 0 {
        o.max_element_bytes = 64 << 20;
    }
    if o.max_total_bytes == 0 {
        o.max_total_bytes = 512 << 20;
    }
    if o.max_elements == 0 {
        o.max_elements = 1000000;
    }
    (o, None)
}
impl Reader {
    pub(super) fn incomplete_source_failure(&self, err: Option<Error>) -> Option<Error> {
        if self.allow_incomplete_pixel_data
            && let Some(first) = &self.counter.first_failure
        {
            return Error::join(err, Some(first.clone()));
        }
        None
    }
    pub(super) fn read_incomplete_native_token(
        &mut self,
        header: ElementHeader,
        offset: i64,
    ) -> Result<Token> {
        let mut out = Vec::new();
        let mut left = header.length as i64;
        let mut buffer = vec![0u8; left.min(32 << 10) as usize];
        while left > 0 {
            let want = left.min(buffer.len() as i64) as usize;
            let (n, err) = source::read_full(
                &mut self.counter,
                buffer.get_mut(..want).unwrap_or_default(),
            );
            if let Some(failure) = self.incomplete_source_failure(err.clone()) {
                return Err(Error::parse(
                    Op::ReadValue,
                    offset,
                    header.tag,
                    Some(header.vr),
                    header.length,
                    Some(failure),
                ));
            }
            out.extend_from_slice(buffer.get(..n).unwrap_or_default());
            left = left.saturating_sub(n as i64);
            if let Some(err) = err {
                if !terminal_eof_cause(&err) {
                    return Err(Error::parse(
                        Op::ReadValue,
                        offset,
                        header.tag,
                        Some(header.vr),
                        header.length,
                        Some(err),
                    ));
                }
                let cause = self.value_read_error(offset, &header, err);
                if !terminal_eof_cause(&cause)
                    || self.counter.fence_hit
                    || (self.source_end > 0 && self.position() != self.source_end)
                {
                    return Err(cause);
                }
                self.incomplete_pixel_data = Some(IncompletePixelData {
                    kind: "native-value-eof",
                    value_offset: offset,
                    failure_offset: self.position(),
                    declared_bytes: header.length as i64,
                    available_bytes: out.len() as i64,
                    retained_fragments: 0,
                    discarded_fragments: 0,
                    discarded_bytes: 0,
                    cause,
                });
                self.incomplete_terminal = true;
                break;
            }
        }
        let data = self.to_stream_byte_order(header.vr, out);
        self.element_count = self.element_count.saturating_add(1);
        self.delimiter_check_pending = true;
        Ok(element_token(header, Value::Raw(data)))
    }
    pub(super) fn read_incomplete_item_bytes(
        &mut self,
        header: &ElementHeader,
        offset: i64,
    ) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut left = header.length as i64;
        let mut scratch = vec![0u8; left.min(32 << 10) as usize];
        while left > 0 {
            let want = left.min(scratch.len() as i64) as usize;
            let (n, err) = source::read_full(
                &mut self.counter,
                scratch.get_mut(..want).unwrap_or_default(),
            );
            if let Some(failure) = self.incomplete_source_failure(err.clone()) {
                return Err(Error::parse(
                    Op::ReadValue,
                    offset,
                    header.tag,
                    Some(header.vr),
                    header.length,
                    Some(failure),
                ));
            }
            out.extend_from_slice(scratch.get(..n).unwrap_or_default());
            left = left.saturating_sub(n as i64);
            if let Some(err) = err {
                return Err(self.value_read_error(offset, header, err));
            }
        }
        Ok(out)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish_incomplete_fragments(
        &mut self,
        start: i64,
        value_offset: i64,
        offset_table: &[u8],
        fragments: &[Vec<u8>],
        saw_bot: bool,
        err: &Error,
    ) -> bool {
        if !self.allow_incomplete_pixel_data
            || !saw_bot
            || fragments.is_empty()
            || self.seq_delimiters.len() != 1
            || self.seq_delimiters.first().map(|t| t.typ) != Some(SeqType::PixelSequence)
            || self.counter.fence_hit
        {
            return false;
        }
        let kind;
        let discarded: i64;
        let mut dropped = 0;
        let pe = err.parse_info();
        if terminal_eof_cause(err)
            && self
                .counter
                .last_error
                .as_ref()
                .is_some_and(terminal_eof_cause)
        {
            if self.source_end > 0 && self.position() != self.source_end {
                return false;
            }
            if let Some(pe) = pe.filter(|pe| pe.tag.is_item() && pe.op == Op::ReadValue) {
                if pe.length == crate::object::types::UNDEFINED_LENGTH {
                    return false;
                }
                kind = "encapsulated-fragment-eof";
                dropped = 1;
                discarded = self.position().saturating_sub(start).saturating_sub(8);
                if discarded < 0 {
                    return false;
                }
            } else {
                discarded = self.position().saturating_sub(start);
                if !(0..=7).contains(&discarded) {
                    return false;
                }
                if let Some(pe) = pe
                    && !pe.tag.is_zero()
                    && !pe.tag.is_item()
                    && !pe.tag.is_sequence_delimitation()
                {
                    return false;
                }
                kind = "encapsulated-delimiter-eof";
            }
        } else if let Some(pe) = pe.filter(|pe| {
            pe.op == Op::ReadVr
                && self.source_end.saturating_sub(start) == 8
                && self.position().saturating_sub(start) == 6
                && !pe.tag.is_private()
        }) {
            if self.dictionary_knows(pe.tag) {
                return false;
            }
            let mut tail = [0u8; 8];
            let off = start
                .saturating_sub(self.base_offset)
                .saturating_add(self.root_offset);
            let Some((n, re)) = self.counter.r.read_at(&mut tail, off) else {
                return false;
            };
            if n != 8 || re.as_ref().is_some_and(|e| !e.is_exactly(Kind::Eof)) {
                if let Some(re) = re
                    && self.counter.first_failure.is_none()
                {
                    self.counter.first_failure = Some(re);
                }
                return false;
            }
            if Vr::parse(&tail[4..6]).is_some() {
                return false;
            }
            let mut rest = [0u8; 2];
            if source::read_full(&mut self.counter, &mut rest).1.is_some() {
                return false;
            }
            let mut probe = [0u8; 1];
            let (n, re) = self.counter.read(&mut probe);
            if n != 0 || !re.is_some_and(|e| e.is_exactly(Kind::Eof)) {
                return false;
            }
            kind = "encapsulated-garbage-terminator";
            discarded = 8;
        } else {
            return false;
        }
        if self.incomplete_source_failure(Some(err.clone())).is_some() {
            return false;
        }
        let mut available = offset_table.len() as i64;
        for f in fragments {
            available = available.saturating_add(f.len() as i64);
        }
        self.incomplete_pixel_data = Some(IncompletePixelData {
            kind,
            value_offset,
            failure_offset: start,
            declared_bytes: -1,
            available_bytes: available,
            retained_fragments: fragments.len() as i64,
            discarded_fragments: dropped,
            discarded_bytes: discarded,
            cause: err.clone(),
        });
        self.seq_delimiters.clear();
        self.pixel_sequence_offset_table_pending = false;
        self.delimiter_check_pending = false;
        self.incomplete_terminal = true;
        true
    }
    pub(super) fn incomplete_collection_error(&self) -> Option<Error> {
        self.incomplete_pixel_data
            .as_ref()
            .map(|info| incomplete_pixel_data_error(info.cause.clone()))
    }
}
pub fn incomplete_pixel_data_error(cause: Error) -> Error {
    let joined = Error::join(
        Some(Error::sentinel(Kind::IncompletePixelData)),
        Some(cause),
    )
    .expect("joined");
    Error::relabel(Kind::IncompletePixelData.text(), joined)
}
