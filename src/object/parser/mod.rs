#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
mod boundary;
mod collect;
mod deferred;
mod frame;
mod ge_text;
mod header;
mod incomplete;
mod lifecycle;
mod private;
mod projection;
mod selective;
mod sequence;
pub mod source;
mod value;
use crate::object::error::{Error, Kind, Op, Result};
use crate::object::transfer::Syntax;
use crate::object::types::{Element, ElementHeader, Length, Tag, Vr};
pub use boundary::{BoundaryReason, BoundaryRecovery};
pub use deferred::{
    BulkDataDescriptor, BulkDataPathStep, BulkDataRule, Deferral, ERR_INVALID_BULK_DATA_DESCRIPTOR,
    MAX_BULK_DATA_PATH_STEPS, MAX_BULK_DATA_RULES, Rescanned, ValueLocation, ValueReference,
    bulk_data_vr,
};
pub use frame::{EncapsulatedSink, Frame, FrameMetadata, FrameSink, FrameSinkFn};
pub use incomplete::IncompletePixelData;
pub use incomplete::incomplete_pixel_data_error;
pub use lifecycle::{ERR_HOOK_ACTION, ValidatedReadError, ValidatedReader};
pub use private::{
    DEFAULT_MAX_PRIVATE_ISSUES, PrivateDiagnostic, PrivateDictionaryOptions, PrivateIssue,
};
pub use projection::{
    ERR_DATA_SET_PROJECTION_OPTIONS, ProjectedDataSet, ReadStop, prefix_requires_selective_reader,
};
pub use selective::{
    Disposition, ERR_MAX_SELECTIVE_TOKENS, ERR_SELECTIVE_DISPOSITION,
    ERR_SELECTIVE_READER_CALLBACK, ERR_SELECTIVE_READER_OPTIONS, SelectFn, SelectiveReader,
    SelectiveToken,
};
use source::{ByteRead, Counter};
pub const DEFAULT_MAX_SEQUENCE_DEPTH: i64 = 128;
pub const DEFAULT_MAX_DELIMITER_RECOVERIES: i64 = 4096;
pub const DEFAULT_MAX_IMPLICIT_VR_ELEMENTS: i64 = 4096;
const MAX_PRIVATE_SEQUENCE_CHECK_BYTES: u64 = 16 << 20;
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SequenceRecoveryOptions {
    pub undefined_length_obow_as_sq: bool,
    pub voi_lut_sequence_ow_as_sq: bool,
    pub replace_wrong_delimiter: bool,
    pub private_sequence_as_bytes: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct VrRecoveryOptions {
    pub map_ox_to_ow: bool,
    pub prefer_dictionary_vr: bool,
    pub prefer_dictionary_length_field_size: bool,
    pub correct_private_ul_length: bool,
    pub short_length_un: bool,
    pub correct_ge_text_length: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ReaderOptions {
    pub allow_incomplete_pixel_data: bool,
    pub max_element_bytes: i64,
    pub max_pixel_data_bytes: i64,
    pub max_total_bytes: i64,
    pub base_offset: i64,
    pub max_sequence_depth: i64,
    pub max_elements: i64,
    pub max_items: i64,
    pub max_fragments: i64,
    pub strict_reserved_bytes: bool,
    pub accept_odd_length: bool,
    pub skip_pixel_data: bool,
    pub recover_container_overruns: bool,
    pub max_boundary_recoveries: i64,
    pub recover_implicit_vr_elements: bool,
    pub sequence_recovery: SequenceRecoveryOptions,
    pub vr_recovery: VrRecoveryOptions,
    pub recover_delimiter_anomalies: bool,
    pub max_delimiter_recoveries: i64,
    pub max_implicit_vr_elements: i64,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    Element,
    StartSequence,
    StartPixelSequence,
    EndSequence,
    StartItem,
    EndItem,
}
impl TokenKind {
    pub fn text(self) -> &'static str {
        match self {
            TokenKind::Element => "element",
            TokenKind::StartSequence => "sequence start",
            TokenKind::StartPixelSequence => "pixel sequence start",
            TokenKind::EndSequence => "sequence end",
            TokenKind::StartItem => "item start",
            TokenKind::EndItem => "item end",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub header: Option<ElementHeader>,
    pub element: Option<Element>,
    pub offset: i64,
}
impl Token {
    fn bare(kind: TokenKind) -> Token {
        Token {
            kind,
            header: None,
            element: None,
            offset: 0,
        }
    }
    fn with_header(kind: TokenKind, header: ElementHeader) -> Token {
        Token {
            kind,
            header: Some(header),
            element: None,
            offset: 0,
        }
    }
    pub fn tag(&self) -> Tag {
        self.header.map_or(Tag::default(), |h| h.tag)
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SeqType {
    Sequence,
    PixelSequence,
    Item,
}
impl SeqType {
    fn text(self) -> &'static str {
        match self {
            SeqType::Sequence => "sequence",
            SeqType::PixelSequence => "pixel sequence",
            SeqType::Item => "item",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct SeqToken {
    typ: SeqType,
    tag: Tag,
    items: i64,
    length: Length,
    base_offset: u64,
    implicit_vr_little_endian: bool,
    explicit_big_endian: bool,
    defined_index: isize,
    item_index: isize,
}
pub struct Reader {
    allow_incomplete_pixel_data: bool,
    incomplete_pixel_data: Option<IncompletePixelData>,
    incomplete_terminal: bool,
    init_error: Option<Error>,
    recover_container_overruns: bool,
    recover_implicit_vr_elements: bool,
    recover_delimiter_anomalies: bool,
    swapped_item_pending: bool,
    vr_recovery: VrRecoveryOptions,
    seq_recovery: SequenceRecoveryOptions,
    max_delimiter_recoveries: i64,
    delimiter_recoveries: i64,
    max_implicit_vr_elements: i64,
    implicit_vr_elements: i64,
    max_boundary_recoveries: i64,
    boundary_recoveries: Vec<BoundaryRecovery>,
    boundary_recoveries_truncated: bool,
    pub(crate) counter: Counter,
    syntax: Syntax,
    max_element_bytes: i64,
    max_pixel_data_bytes: i64,
    max_total_bytes: i64,
    deflate_limit_has_tail: bool,
    max_sequence_depth: i64,
    max_elements: i64,
    max_items: i64,
    item_count: i64,
    max_fragments: i64,
    element_count: i64,
    fragment_count: i64,
    pixel_data_bytes: i64,
    strict_reserved_bytes: bool,
    accept_odd_length: bool,
    skip_pixel_data: bool,
    seq_delimiters: Vec<SeqToken>,
    delimiter_check_pending: bool,
    pixel_sequence_offset_table_pending: bool,
    base_offset: i64,
    root_offset: i64,
    source_end: i64,
    stop_before_pixel_data: bool,
    no_dictionary: bool,
    defer: deferred::DeferState,
    projection: Option<projection::Projection>,
    last_reserved_nonzero: bool,
    last_reserved_offset: i64,
    sinks: frame::SinkState,
    private: private::PrivateState,
}
impl Reader {
    pub fn new(
        r: Box<dyn ByteRead>,
        sized: Option<(i64, i64)>,
        syntax: Syntax,
        opts: ReaderOptions,
    ) -> Reader {
        let (opts, incomplete_init_error) = incomplete::incomplete_read_options(opts, &syntax);
        let mut opts = opts;
        if opts.max_sequence_depth == 0 {
            opts.max_sequence_depth = DEFAULT_MAX_SEQUENCE_DEPTH;
        }
        let counter = Counter::new(
            r,
            opts.base_offset,
            opts.max_total_bytes,
            opts.allow_incomplete_pixel_data,
        );
        let mut root_offset = 0;
        let mut source_end = 0;
        if let Some((size, pos)) = sized {
            root_offset = pos;
            if size >= pos {
                source_end = opts.base_offset.saturating_add(size.saturating_sub(pos));
            }
        }
        Reader {
            allow_incomplete_pixel_data: opts.allow_incomplete_pixel_data,
            incomplete_pixel_data: None,
            incomplete_terminal: false,
            init_error: incomplete_init_error,
            recover_container_overruns: opts.recover_container_overruns,
            recover_implicit_vr_elements: opts.recover_implicit_vr_elements,
            recover_delimiter_anomalies: opts.recover_delimiter_anomalies,
            swapped_item_pending: false,
            vr_recovery: opts.vr_recovery,
            seq_recovery: opts.sequence_recovery,
            max_delimiter_recoveries: opts.max_delimiter_recoveries,
            delimiter_recoveries: 0,
            max_implicit_vr_elements: opts.max_implicit_vr_elements,
            implicit_vr_elements: 0,
            max_boundary_recoveries: opts.max_boundary_recoveries,
            boundary_recoveries: Vec::new(),
            boundary_recoveries_truncated: false,
            counter,
            syntax,
            max_element_bytes: opts.max_element_bytes,
            max_pixel_data_bytes: opts.max_pixel_data_bytes,
            max_total_bytes: opts.max_total_bytes,
            deflate_limit_has_tail: false,
            max_sequence_depth: opts.max_sequence_depth,
            max_elements: opts.max_elements,
            max_items: opts.max_items,
            item_count: 0,
            max_fragments: opts.max_fragments,
            element_count: 0,
            fragment_count: 0,
            pixel_data_bytes: 0,
            strict_reserved_bytes: opts.strict_reserved_bytes,
            accept_odd_length: opts.accept_odd_length,
            skip_pixel_data: opts.skip_pixel_data,
            seq_delimiters: Vec::new(),
            delimiter_check_pending: false,
            pixel_sequence_offset_table_pending: false,
            base_offset: opts.base_offset,
            root_offset,
            source_end,
            stop_before_pixel_data: false,
            no_dictionary: false,
            defer: deferred::DeferState::default(),
            projection: None,
            last_reserved_nonzero: false,
            last_reserved_offset: 0,
            sinks: frame::SinkState::default(),
            private: private::PrivateState::default(),
        }
    }
    pub fn set_stop_before_pixel_data(&mut self, stop: bool) {
        self.stop_before_pixel_data = stop;
    }
    pub fn set_no_dictionary(&mut self, none: bool) {
        self.no_dictionary = none;
    }
    pub fn position(&self) -> i64 {
        self.counter.pos
    }
    fn position_u64(&self) -> u64 {
        self.position().max(0) as u64
    }
    fn top(&self) -> Option<&SeqToken> {
        self.seq_delimiters.last()
    }
    fn in_pixel_sequence(&self) -> bool {
        self.top().is_some_and(|t| t.typ == SeqType::PixelSequence)
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Token> {
        if let Some(failure) = self.incomplete_source_failure(None) {
            return Err(failure);
        }
        if self.incomplete_terminal {
            return Err(Error::eof());
        }
        if let Some(err) = &self.init_error {
            return Err(err.clone());
        }
        self.encoded_context_error()?;
        if let Some(tok) = self.update_seq_delimiters()? {
            return Ok(tok);
        }
        self.check_total_bytes()?;
        let header_offset = self.position();
        let header = match self.read_header() {
            Ok(h) => h,
            Err(err) => {
                if let Some(failure) = self.incomplete_source_failure(Some(err.clone())) {
                    return Err(failure);
                }
                if err.is(Kind::Eof) && !self.seq_delimiters.is_empty() {
                    return Err(Error::parse(
                        Op::ReadTag,
                        self.position(),
                        Tag::default(),
                        None,
                        0,
                        Some(Error::unexpected_eof()),
                    ));
                }
                return Err(err);
            }
        };
        match self.control_token(header)? {
            sequence::Control::Primitive => {}
            sequence::Control::Skipped => return self.next(),
            sequence::Control::Token(mut tok) => {
                tok.offset = header_offset;
                return Ok(tok);
            }
        }
        if self.in_pixel_sequence() {
            return Err(Error::parse(
                Op::ReadValue,
                self.position(),
                header.tag,
                Some(header.vr),
                header.length,
                Some(Error::msg(format!(
                    "dicom: unexpected tag {} inside encapsulated Pixel Data",
                    header.tag
                ))),
            ));
        }
        if let Some(tok) = self.seek_unselected_sequence(&header, header_offset) {
            return Ok(tok);
        }
        if header.vr == Vr::SQ || (header.vr == Vr::UN && header.is_undefined()) {
            self.push_container(SeqType::Sequence, header, header.vr == Vr::UN)?;
            self.delimiter_check_pending = true;
            let mut tok = Token::with_header(TokenKind::StartSequence, header);
            tok.offset = header_offset;
            return Ok(tok);
        }
        if (header.vr == Vr::OB || header.vr == Vr::OW)
            && header.tag == Tag::PIXEL_DATA
            && header.is_undefined()
        {
            self.push_pixel_sequence(header)?;
            self.pixel_sequence_offset_table_pending = true;
            self.pixel_data_bytes = 0;
            self.delimiter_check_pending = true;
            if self.has_encapsulated_sink() {
                return self.stream_encoded_pixel_data(header, header_offset);
            }
            let mut tok = Token::with_header(TokenKind::StartPixelSequence, header);
            tok.offset = header_offset;
            return Ok(tok);
        }
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
        match self.read_defined_value_token(header) {
            Ok(mut tok) => {
                tok.offset = header_offset;
                Ok(tok)
            }
            Err(err) => {
                if let Some(failure) = self.incomplete_source_failure(Some(err.clone())) {
                    return Err(failure);
                }
                Err(err)
            }
        }
    }
    pub fn boundary_recoveries(&self) -> (Vec<BoundaryRecovery>, bool) {
        (
            self.boundary_recoveries.clone(),
            self.boundary_recoveries_truncated,
        )
    }
    pub fn incomplete_pixel_data(&self) -> Option<IncompletePixelData> {
        self.incomplete_pixel_data.clone()
    }
    fn check_element_count_limit(&self, header: &ElementHeader) -> Result<()> {
        if self.max_elements <= 0 || self.element_count < self.max_elements {
            return Ok(());
        }
        Err(Error::parse(
            Op::CheckElementCount,
            self.position(),
            header.tag,
            Some(header.vr),
            header.length,
            Some(Error::msg(format!(
                "dicom: maximum element count exceeded: got {}, limit {}",
                self.element_count.saturating_add(1),
                self.max_elements
            ))),
        ))
    }
    fn check_fragment_count_limit(&self, header: &ElementHeader) -> Result<()> {
        if self.pixel_sequence_offset_table_pending
            || self.max_fragments <= 0
            || self.fragment_count < self.max_fragments
        {
            return Ok(());
        }
        Err(Error::parse(
            Op::CheckFragmentCount,
            self.position(),
            header.tag,
            Some(header.vr),
            header.length,
            Some(Error::msg(format!(
                "dicom: maximum fragment count exceeded: got {}, limit {}",
                self.fragment_count.saturating_add(1),
                self.max_fragments
            ))),
        ))
    }
    fn count_item(&mut self, header: &ElementHeader) -> Result<()> {
        if self.max_items > 0 && self.item_count >= self.max_items {
            return Err(Error::parse(
                Op::CheckElementCount,
                self.position(),
                header.tag,
                Some(header.vr),
                header.length,
                Some(Error::msg(format!(
                    "dicom: maximum sequence item count exceeded: got {}, limit {}",
                    self.item_count.saturating_add(1),
                    self.max_items
                ))),
            ));
        }
        self.item_count = self.item_count.saturating_add(1);
        Ok(())
    }
    fn check_sequence_depth_limit(&self, length: Length) -> Result<()> {
        let depth = (self.seq_delimiters.len() as i64).saturating_add(1);
        if self.max_sequence_depth <= 0 || depth <= self.max_sequence_depth {
            return Ok(());
        }
        Err(Error::parse(
            Op::CheckDepth,
            self.position(),
            Tag::default(),
            None,
            length,
            Some(Error::msg(format!(
                "dicom: maximum sequence nesting depth exceeded: got {depth}, limit {}",
                self.max_sequence_depth
            ))),
        ))
    }
    fn check_total_bytes(&mut self) -> Result<()> {
        if self.max_total_bytes <= 0 || self.position() < self.max_total_bytes {
            return Ok(());
        }
        if self.syntax.deflated
            && self.position() == self.max_total_bytes
            && self.seq_delimiters.is_empty()
            && self.counter.pending.is_empty()
            && !self.deflate_limit_has_tail
        {
            let mut probe = [0u8; 1];
            let (_, err) = source::read_full(self.counter.r.as_mut(), &mut probe);
            match err {
                None => self.deflate_limit_has_tail = true,
                Some(e) if e.is(Kind::Eof) => return Err(Error::eof()),
                Some(e) => {
                    return Err(Error::parse(
                        Op::ReadTag,
                        self.position(),
                        Tag::default(),
                        None,
                        0,
                        Some(e),
                    ));
                }
            }
        }
        Err(Error::parse(
            Op::CheckTotalBytes,
            self.position(),
            Tag::default(),
            None,
            0,
            Some(Error::kind_suffix(
                Kind::MaxTotalBytes,
                &format!(
                    ": read {} bytes, limit {}",
                    self.position(),
                    self.max_total_bytes
                ),
            )),
        ))
    }
    fn validate_defined_value_length(&self, header: &ElementHeader) -> Result<()> {
        if header.length & 1 == 0 || self.accept_odd_length {
            return Ok(());
        }
        Err(Error::parse(
            Op::ReadValue,
            self.position(),
            header.tag,
            Some(header.vr),
            header.length,
            Some(Error::msg(format!(
                "dicom: element length must be even: got {}",
                header.length
            ))),
        ))
    }
    fn skip_n(&mut self, offset: i64, header: &ElementHeader, n: i64) -> Result<()> {
        if n == 0 {
            return Ok(());
        }
        if let Some(err) = source::discard_n(&mut self.counter, n) {
            return Err(Error::parse(
                Op::ReadValue,
                offset,
                header.tag,
                Some(header.vr),
                header.length,
                Some(required_read_error(err)),
            ));
        }
        Ok(())
    }
    fn check_element_byte_limit(&self, header: &ElementHeader) -> Result<()> {
        let mut limit = self.max_element_bytes;
        let mut text = "dicom: maximum element value size exceeded";
        if is_pixel_data_value_tag(header.tag) && self.max_pixel_data_bytes > 0 {
            limit = self.max_pixel_data_bytes;
            text = "dicom: maximum Pixel Data size exceeded";
        }
        if limit <= 0 || header.length as i64 <= limit {
            return Ok(());
        }
        Err(Error::parse(
            Op::ReadValue,
            self.position(),
            header.tag,
            Some(header.vr),
            header.length,
            Some(Error::msg(format!(
                "{text}: value length exceeds configured limit"
            ))),
        ))
    }
    fn reserve_pixel_data_bytes(&mut self, header: &ElementHeader) -> Result<()> {
        if self.max_pixel_data_bytes <= 0 {
            return Ok(());
        }
        let length = header.length as i64;
        if self.pixel_data_bytes > self.max_pixel_data_bytes
            || length
                > self
                    .max_pixel_data_bytes
                    .saturating_sub(self.pixel_data_bytes)
        {
            return Err(Error::parse(
                Op::ReadValue,
                self.position(),
                Tag::PIXEL_DATA,
                Some(header.vr),
                header.length,
                Some(Error::msg(
                    "dicom: maximum Pixel Data size exceeded: encapsulated value exceeds configured limit",
                )),
            ));
        }
        self.pixel_data_bytes = self.pixel_data_bytes.saturating_add(length);
        Ok(())
    }
}
pub(crate) fn is_pixel_data_value_tag(tag: Tag) -> bool {
    tag == Tag::PIXEL_DATA || tag == Tag::new(0x7FE0, 0x0008) || tag == Tag::new(0x7FE0, 0x0009)
}
pub(crate) fn normalize_read_error(start: i64, end: i64, err: Error) -> Error {
    if end > start && err.is(Kind::Eof) && !err.is(Kind::UnexpectedEof) {
        return Error::unexpected_eof();
    }
    err
}
pub(crate) fn required_read_error(err: Error) -> Error {
    if err.is(Kind::Eof) {
        return Error::unexpected_eof();
    }
    err
}
