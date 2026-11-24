use super::source::{self, ByteRead, Counter};
use super::value::element_token;
use super::{Reader, Token, TokenKind};
use crate::object::error::{Error, Kind, Op, Result, quote_bytes};
use crate::object::text::trim_text_value_bytes;
use crate::object::transfer::Syntax;
use crate::object::types::{Element, ElementHeader, Tag, Value};
use std::io::Read;
const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_PLANAR_CONFIGURATION: Tag = Tag::new(0x0028, 0x0006);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7FE0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7FE0, 0x0002);
const ERR_MISSING_BASIC_OFFSET_TABLE: &str =
    "dicom: encapsulated Pixel Data is missing the Basic Offset Table item";
const ERR_UNEXPECTED_SEQUENCE_CONTROL_TAG: &str = "dicom: unexpected sequence control tag";
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FrameMetadata {
    pub rows: u16,
    pub columns: u16,
    pub samples_per_pixel: u16,
    pub bits_allocated: u16,
    pub bits_stored: u16,
    pub high_bit: u16,
    pub pixel_representation: u16,
    pub planar_configuration: u16,
    pub planar_configuration_present: bool,
    pub number_of_frames: i64,
    pub photometric_interpretation: String,
}
impl FrameMetadata {
    pub fn bytes_per_sample(&self) -> i64 {
        i64::from(self.bits_allocated.wrapping_add(7) / 8)
    }
    pub fn frame_size(&self) -> i64 {
        let photometric =
            crate::textfmt::to_upper(trim_space_str(&self.photometric_interpretation));
        if (photometric == "YBR_FULL_422" || photometric == "YBR_PARTIAL_422")
            && self.bits_allocated >= 8
        {
            return i64::from(self.rows)
                .saturating_mul(i64::from(self.columns))
                .saturating_mul(2)
                .saturating_mul(self.bytes_per_sample());
        }
        let samples = i64::from(self.rows)
            .saturating_mul(i64::from(self.columns))
            .saturating_mul(i64::from(self.samples_per_pixel));
        if self.bits_allocated < 8 {
            return samples
                .saturating_mul(i64::from(self.bits_allocated))
                .saturating_add(7)
                / 8;
        }
        samples.saturating_mul(self.bytes_per_sample())
    }
    pub fn total_size(&self) -> i64 {
        let frame = self.frame_size();
        let frames = self.number_of_frames;
        if frame <= 0 || frames <= 0 {
            return 0;
        }
        frame.checked_mul(frames).unwrap_or(0)
    }
}
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    pub index: usize,
    pub data: Vec<u8>,
    pub metadata: FrameMetadata,
    pub transfer_syntax: Syntax,
}
pub trait FrameSink {
    fn handle_frame(&mut self, frame: Frame) -> Result<()>;
    fn close(&mut self) -> Result<()>;
}
pub struct FrameSinkFn<F: FnMut(Frame) -> Result<()>>(pub F);
impl<F: FnMut(Frame) -> Result<()>> FrameSink for FrameSinkFn<F> {
    fn handle_frame(&mut self, frame: Frame) -> Result<()> {
        (self.0)(frame)
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}
pub trait EncapsulatedSink {
    fn context_error(&self) -> Option<Error> {
        None
    }
    fn check_extended_table(&mut self, header: &ElementHeader) -> Result<()>;
    fn extended_table(&mut self, element: &Element) -> Result<()>;
    fn start_pixel_data(&mut self, metadata: &FrameMetadata, syntax: &Syntax) -> Result<()>;
    fn item(&mut self, length: u32, basic: bool, value: &mut dyn Read) -> Result<()>;
    fn end_pixel_data(&mut self) -> Result<()>;
    fn close(&mut self) -> Result<()>;
}
#[derive(Default)]
struct FrameMetadataState {
    metadata: FrameMetadata,
    rows_seen: bool,
    columns_seen: bool,
    samples_seen: bool,
    bits_allocated_seen: bool,
    bits_stored_seen: bool,
    high_bit_seen: bool,
    pixel_rep_seen: bool,
    number_of_frames_seen: bool,
    photometric_seen: bool,
}
impl FrameMetadataState {
    fn complete(&self) -> Result<FrameMetadata> {
        let mut missing: Vec<String> = Vec::new();
        for (seen, name, tag) in [
            (self.rows_seen, "Rows", TAG_ROWS),
            (self.columns_seen, "Columns", TAG_COLUMNS),
            (self.samples_seen, "SamplesPerPixel", TAG_SAMPLES_PER_PIXEL),
            (
                self.bits_allocated_seen,
                "BitsAllocated",
                TAG_BITS_ALLOCATED,
            ),
            (self.bits_stored_seen, "BitsStored", TAG_BITS_STORED),
            (self.high_bit_seen, "HighBit", TAG_HIGH_BIT),
            (
                self.pixel_rep_seen,
                "PixelRepresentation",
                TAG_PIXEL_REPRESENTATION,
            ),
            (
                self.photometric_seen,
                "PhotometricInterpretation",
                TAG_PHOTOMETRIC,
            ),
        ] {
            if !seen {
                missing.push(format!("{name} ({tag})"));
            }
        }
        if !missing.is_empty() {
            return Err(Error::kind_suffix(
                Kind::MissingFrameMetadata,
                &format!(": {}", missing.join(", ")),
            ));
        }
        let mut m = self.metadata.clone();
        if !self.number_of_frames_seen {
            m.number_of_frames = 1;
        }
        if m.rows == 0
            || m.columns == 0
            || m.samples_per_pixel == 0
            || m.bits_allocated == 0
            || m.number_of_frames <= 0
            || m.frame_size() <= 0
        {
            return Err(invalid_layout(&m));
        }
        Ok(m)
    }
}
fn invalid_layout(m: &FrameMetadata) -> Error {
    Error::kind_suffix(
        Kind::InvalidFrameMetadata,
        &format!(
            ": rows={} columns={} samples_per_pixel={} bits_allocated={} number_of_frames={}",
            m.rows, m.columns, m.samples_per_pixel, m.bits_allocated, m.number_of_frames
        ),
    )
}
#[derive(Default)]
pub(crate) struct SinkState {
    frame_sink: Option<Box<dyn FrameSink>>,
    frame_sink_closed: bool,
    encapsulated_sink: Option<Box<dyn EncapsulatedSink>>,
    encapsulated_sink_closed: bool,
    pub(super) encoded_pixel_active: bool,
    encoded_pixel_seen: bool,
    encoded_discarded: bool,
    metadata: FrameMetadataState,
}
impl SinkState {
    fn any(&self) -> bool {
        self.frame_sink.is_some() || self.encapsulated_sink.is_some()
    }
}
fn trim_unicode_space(mut data: &[u8]) -> &[u8] {
    fn is_space(c: char) -> bool {
        matches!(
            c,
            '\t' | '\n'
                | '\u{b}'
                | '\u{c}'
                | '\r'
                | ' '
                | '\u{85}'
                | '\u{a0}'
                | '\u{1680}'
                | '\u{2000}'
                ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
        )
    }
    fn first_char(data: &[u8]) -> Option<char> {
        let upto = data.len().min(4);
        (1..=upto).find_map(|n| {
            std::str::from_utf8(data.get(..n)?)
                .ok()
                .and_then(|s| s.chars().next())
        })
    }
    while let Some(c) = first_char(data) {
        if !is_space(c) {
            break;
        }
        data = data.get(c.len_utf8()..).unwrap_or_default();
    }
    loop {
        let upto = data.len().min(4);
        let last = (1..=upto).find_map(|n| {
            let s = std::str::from_utf8(data.get(data.len().checked_sub(n)?..)?).ok()?;
            let mut chars = s.chars();
            let c = chars.next()?;
            chars.next().is_none().then_some(c)
        });
        match last {
            Some(c) if is_space(c) => {
                let end = data.len().saturating_sub(c.len_utf8());
                data = data.get(..end).unwrap_or_default();
            }
            _ => break,
        }
    }
    data
}
fn trim_space_str(s: &str) -> &str {
    let trimmed = trim_unicode_space(s.as_bytes());
    std::str::from_utf8(trimmed).unwrap_or(s)
}
fn atoi(value: &[u8]) -> Option<i64> {
    let digits = match value.first() {
        Some(b'+' | b'-') => value.get(1..).unwrap_or_default(),
        _ => value,
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(value).ok()?.parse::<i64>().ok()
}
struct ItemReader<'a> {
    counter: &'a mut Counter,
    remaining: u64,
}
impl Read for ItemReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 || buf.is_empty() {
            return Ok(0);
        }
        let want = (buf.len() as u64).min(self.remaining) as usize;
        let (n, err) = self.counter.read(buf.get_mut(..want).unwrap_or_default());
        self.remaining = self.remaining.saturating_sub(n as u64);
        if n > 0 {
            return Ok(n);
        }
        match err {
            Some(err) if !err.is(Kind::Eof) => Err(std::io::Error::other(err.to_string())),
            _ => Ok(0),
        }
    }
}
impl Reader {
    pub fn set_frame_sink(&mut self, sink: Box<dyn FrameSink>) {
        if self.allow_incomplete_pixel_data {
            self.init_error = Error::join(
                self.init_error.take(),
                Some(Error::sentinel(Kind::IncompletePixelDataOptions)),
            );
        }
        self.sinks.frame_sink = Some(sink);
    }
    pub fn set_encapsulated_sink(&mut self, sink: Box<dyn EncapsulatedSink>) {
        if self.allow_incomplete_pixel_data {
            self.init_error = Error::join(
                self.init_error.take(),
                Some(Error::sentinel(Kind::IncompletePixelDataOptions)),
            );
        }
        self.sinks.encapsulated_sink = Some(sink);
    }
    pub fn has_sinks(&self) -> bool {
        self.sinks.any()
    }
    pub fn has_frame_sink(&self) -> bool {
        self.sinks.frame_sink.is_some()
    }
    pub(crate) fn encoded_pixel_data_discarded(&self) -> bool {
        self.sinks.encoded_discarded
    }
    pub fn close(&mut self) -> Result<()> {
        match self.close_sinks(None) {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    pub(super) fn close_sinks(&mut self, err: Option<Error>) -> Option<Error> {
        let mut err = err;
        if !self.sinks.frame_sink_closed
            && let Some(sink) = self.sinks.frame_sink.as_mut()
        {
            self.sinks.frame_sink_closed = true;
            err = Error::join(err, sink.close().err());
        }
        if !self.sinks.encapsulated_sink_closed
            && let Some(sink) = self.sinks.encapsulated_sink.as_mut()
        {
            self.sinks.encapsulated_sink_closed = true;
            err = Error::join(err, sink.close().err());
        }
        err
    }
    pub(super) fn encoded_context_error(&self) -> Result<()> {
        match self
            .sinks
            .encapsulated_sink
            .as_ref()
            .and_then(|s| s.context_error())
        {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    pub(super) fn encoded_value_precheck(&mut self, header: &ElementHeader) -> Result<()> {
        let top_level = self.seq_delimiters.is_empty();
        let Some(sink) = self.sinks.encapsulated_sink.as_mut() else {
            return Ok(());
        };
        if super::is_pixel_data_value_tag(header.tag) {
            return Err(Error::kind_suffix(
                Kind::FrameSink,
                ": encoded frame streaming requires encapsulated integer Pixel Data",
            ));
        }
        if top_level && is_extended_table(header.tag) {
            sink.check_extended_table(header)?;
        }
        Ok(())
    }
    pub(super) fn frame_sink_takes(&self, header: &ElementHeader) -> bool {
        self.sinks.frame_sink.is_some()
            && !self.sinks.frame_sink_closed
            && self.seq_delimiters.is_empty()
            && header.tag == Tag::PIXEL_DATA
            && !self.syntax.encapsulated
    }
    pub(super) fn encoded_table_read(&mut self, header: &ElementHeader, data: &[u8]) -> Result<()> {
        if self.seq_delimiters.is_empty()
            && is_extended_table(header.tag)
            && let Some(sink) = self.sinks.encapsulated_sink.as_mut()
        {
            sink.extended_table(&Element {
                header: *header,
                value: Value::Raw(data.to_vec()),
            })?;
        }
        Ok(())
    }
    pub(super) fn sink_value_read(
        &mut self,
        header: &ElementHeader,
        data: &[u8],
        value_offset: i64,
    ) -> Result<()> {
        if !self.sinks.any() || !self.seq_delimiters.is_empty() {
            return Ok(());
        }
        self.capture_frame_metadata(header, data).map_err(|err| {
            Error::parse(
                Op::ReadValue,
                value_offset,
                header.tag,
                Some(header.vr),
                header.length,
                Some(err),
            )
        })
    }
    fn capture_frame_metadata(&mut self, header: &ElementHeader, data: &[u8]) -> Result<()> {
        let image_tag = matches!(
            header.tag,
            TAG_ROWS
                | TAG_COLUMNS
                | TAG_SAMPLES_PER_PIXEL
                | TAG_BITS_ALLOCATED
                | TAG_BITS_STORED
                | TAG_HIGH_BIT
                | TAG_PIXEL_REPRESENTATION
                | TAG_PLANAR_CONFIGURATION
                | TAG_NUMBER_OF_FRAMES
                | TAG_PHOTOMETRIC
        );
        if !image_tag {
            return Ok(());
        }
        if self.sinks.encoded_pixel_seen {
            return Err(Error::kind_suffix(
                Kind::InvalidFrameMetadata,
                ": image metadata after streamed Pixel Data",
            ));
        }
        let big_endian = self.syntax.big_endian;
        let uint16 = |data: &[u8]| -> Result<u16> {
            let &[a, b, ..] = data else {
                return Err(Error::kind_suffix(
                    Kind::InvalidFrameMetadata,
                    &format!(": {} has {} byte(s)", header.tag, data.len()),
                ));
            };
            let pair = [a, b];
            Ok(if big_endian {
                u16::from_be_bytes(pair)
            } else {
                u16::from_le_bytes(pair)
            })
        };
        let state = &mut self.sinks.metadata;
        match header.tag {
            TAG_ROWS => {
                state.metadata.rows = uint16(data)?;
                state.rows_seen = true;
            }
            TAG_COLUMNS => {
                state.metadata.columns = uint16(data)?;
                state.columns_seen = true;
            }
            TAG_SAMPLES_PER_PIXEL => {
                state.metadata.samples_per_pixel = uint16(data)?;
                state.samples_seen = true;
            }
            TAG_BITS_ALLOCATED => {
                state.metadata.bits_allocated = uint16(data)?;
                state.bits_allocated_seen = true;
            }
            TAG_BITS_STORED => {
                state.metadata.bits_stored = uint16(data)?;
                state.bits_stored_seen = true;
            }
            TAG_HIGH_BIT => {
                state.metadata.high_bit = uint16(data)?;
                state.high_bit_seen = true;
            }
            TAG_PIXEL_REPRESENTATION => {
                state.metadata.pixel_representation = uint16(data)?;
                state.pixel_rep_seen = true;
            }
            TAG_PLANAR_CONFIGURATION => {
                state.metadata.planar_configuration = uint16(data)?;
                state.metadata.planar_configuration_present = true;
            }
            TAG_NUMBER_OF_FRAMES => {
                let value = trim_unicode_space(trim_text_value_bytes(header.vr, data));
                if value.is_empty() {
                    state.metadata.number_of_frames = 1;
                    state.number_of_frames_seen = true;
                    return Ok(());
                }
                match atoi(value) {
                    Some(n) if n > 0 && n <= i64::from(i32::MAX) => {
                        state.metadata.number_of_frames = n;
                        state.number_of_frames_seen = true;
                    }
                    _ => {
                        return Err(Error::kind_suffix(
                            Kind::InvalidFrameMetadata,
                            &format!(": NumberOfFrames={}", quote_bytes(value)),
                        ));
                    }
                }
            }
            _ => {
                let value = trim_unicode_space(trim_text_value_bytes(header.vr, data));
                if value.is_empty() {
                    return Err(Error::kind_suffix(
                        Kind::InvalidFrameMetadata,
                        ": PhotometricInterpretation is empty",
                    ));
                }
                state.metadata.photometric_interpretation =
                    String::from_utf8_lossy(value).into_owned();
                state.photometric_seen = true;
            }
        }
        Ok(())
    }
    pub(super) fn read_native_pixel_data_frames_token(
        &mut self,
        header: ElementHeader,
        value_offset: i64,
    ) -> Result<Token> {
        let parse = |offset: i64, err: Error| {
            Error::parse(
                Op::ReadValue,
                offset,
                header.tag,
                Some(header.vr),
                header.length,
                Some(err),
            )
        };
        let metadata = self
            .sinks
            .metadata
            .complete()
            .map_err(|e| parse(value_offset, e))?;
        let frame_size = metadata.frame_size();
        let total_size = metadata.total_size();
        if frame_size <= 0 || total_size <= 0 {
            return Err(parse(value_offset, invalid_layout(&metadata)));
        }
        let encoded_size = header.length as i64;
        let has_padding = encoded_size == total_size.saturating_add(1) && total_size % 2 == 1;
        if encoded_size != total_size && !has_padding {
            return Err(parse(
                value_offset,
                Error::kind_suffix(
                    Kind::FrameDataSizeMismatch,
                    &format!(
                        ": expected {total_size} bytes for {} frame(s), got {}",
                        metadata.number_of_frames, header.length
                    ),
                ),
            ));
        }
        for i in 0..metadata.number_of_frames {
            let mut data = vec![0u8; frame_size as usize];
            if let (_, Some(err)) = source::read_full(&mut self.counter, &mut data) {
                let offset = value_offset.saturating_add(i.saturating_mul(frame_size));
                return Err(parse(offset, required(err)));
            }
            let frame = Frame {
                index: i as usize,
                data,
                metadata: metadata.clone(),
                transfer_syntax: self.syntax,
            };
            let delivered = match self.sinks.frame_sink.as_mut() {
                Some(sink) => sink.handle_frame(frame),
                None => Ok(()),
            };
            if let Err(err) = delivered {
                return Err(parse(
                    self.position(),
                    Error::stage(Kind::FrameSink, Error::wrap(&format!("frame {i}"), err)),
                ));
            }
        }
        if has_padding {
            let mut pad = [0u8; 1];
            let pad_offset = value_offset.saturating_add(total_size);
            if let (_, Some(err)) = source::read_full(&mut self.counter, &mut pad) {
                return Err(parse(pad_offset, required(err)));
            }
            if pad[0] != 0 {
                return Err(parse(
                    pad_offset,
                    Error::kind_suffix(
                        Kind::FrameDataSizeMismatch,
                        &format!(
                            ": expected zero padding byte after {total_size} byte payload, got 0x{:02X}",
                            pad[0]
                        ),
                    ),
                ));
            }
        }
        self.record_value_location(&header, value_offset, header.length as i64);
        self.element_count = self.element_count.saturating_add(1);
        self.delimiter_check_pending = true;
        Ok(element_token(header, Value::Nil))
    }
    pub(super) fn has_encapsulated_sink(&self) -> bool {
        self.sinks.encapsulated_sink.is_some()
    }
    pub(super) fn stream_encoded_pixel_data(
        &mut self,
        header: ElementHeader,
        header_offset: i64,
    ) -> Result<Token> {
        if self.seq_delimiters.len() != 1
            || self.skip_pixel_data
            || self.sinks.encapsulated_sink_closed
            || self.sinks.encoded_pixel_seen
        {
            return Err(Error::kind_suffix(
                Kind::FrameSink,
                ": encoded streaming requires a single top-level Pixel Data value and no skip policy",
            ));
        }
        self.sinks.encoded_pixel_seen = true;
        let metadata = self.sinks.metadata.complete()?;
        let syntax = self.syntax;
        if let Some(sink) = self.sinks.encapsulated_sink.as_mut() {
            sink.start_pixel_data(&metadata, &syntax)?;
        }
        let start = self.position();
        self.sinks.encoded_pixel_active = true;
        let result = self.stream_encoded_items(header, header_offset, start);
        self.sinks.encoded_pixel_active = false;
        result
    }
    fn stream_encoded_items(
        &mut self,
        header: ElementHeader,
        header_offset: i64,
        start: i64,
    ) -> Result<Token> {
        loop {
            let tok = self.next()?;
            if tok.kind == TokenKind::EndSequence {
                if self.pixel_sequence_offset_table_pending {
                    return Err(Error::msg(ERR_MISSING_BASIC_OFFSET_TABLE));
                }
                if let Some(sink) = self.sinks.encapsulated_sink.as_mut() {
                    sink.end_pixel_data()?;
                }
                if self.defer.defer_pixel_data {
                    self.record_value_location(
                        &header,
                        start,
                        self.position().saturating_sub(start),
                    );
                } else {
                    self.sinks.encoded_discarded = true;
                }
                let mut tok = element_token(header, Value::Nil);
                tok.offset = header_offset;
                return Ok(tok);
            }
            let is_item =
                tok.kind == TokenKind::Element && tok.header.is_some_and(|h| h.tag.is_item());
            if !is_item {
                return Err(Error::msg(ERR_UNEXPECTED_SEQUENCE_CONTROL_TAG));
            }
        }
    }
    pub(super) fn stream_encoded_item(&mut self, header: ElementHeader) -> Result<Token> {
        self.validate_defined_value_length(&header)?;
        self.check_element_count_limit(&header)?;
        self.check_element_byte_limit(&header)?;
        let basic = self.pixel_sequence_offset_table_pending;
        let mut limited = ItemReader {
            counter: &mut self.counter,
            remaining: u64::from(header.length),
        };
        if let Some(sink) = self.sinks.encapsulated_sink.as_mut() {
            sink.item(header.length, basic, &mut limited)
                .map_err(|err| Error::stage(Kind::FrameSink, err))?;
        }
        if limited.remaining != 0 {
            return Err(Error::kind_suffix(
                Kind::FrameSink,
                ": Item callback did not consume value",
            ));
        }
        if basic {
            self.pixel_sequence_offset_table_pending = false;
        } else {
            self.fragment_count = self.fragment_count.saturating_add(1);
        }
        self.element_count = self.element_count.saturating_add(1);
        self.delimiter_check_pending = true;
        Ok(Token {
            kind: TokenKind::Element,
            header: Some(header),
            element: None,
            offset: 0,
        })
    }
}
fn is_extended_table(tag: Tag) -> bool {
    tag == TAG_EXTENDED_OFFSET_TABLE || tag == TAG_EXTENDED_OFFSET_TABLE_LENGTHS
}
fn required(err: Error) -> Error {
    if err.is(Kind::Eof) {
        Error::unexpected_eof()
    } else {
        err
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trims_like_go() {
        assert_eq!(trim_unicode_space(b"  12 \t"), b"12");
        assert_eq!(trim_unicode_space("\u{a0}7\u{2003}".as_bytes()), b"7");
        assert_eq!(trim_unicode_space(b"\xa07 "), b"\xa07");
        assert_eq!(trim_unicode_space(b" \n"), b"");
    }
    #[test]
    fn atoi_like_go() {
        assert_eq!(atoi(b"+12"), Some(12));
        assert_eq!(atoi(b"-3"), Some(-3));
        assert_eq!(atoi(b"1_0"), None);
        assert_eq!(atoi(b""), None);
        assert_eq!(atoi(b"99999999999999999999"), None);
    }
    #[test]
    fn sizes() {
        let m = FrameMetadata {
            rows: 2,
            columns: 4,
            samples_per_pixel: 3,
            bits_allocated: 8,
            number_of_frames: 2,
            photometric_interpretation: "ybr_full_422 ".into(),
            ..FrameMetadata::default()
        };
        assert_eq!(m.frame_size(), 16);
        assert_eq!(m.total_size(), 32);
    }
}
