use super::source::{self, ByteRead};
use super::{
    BoundaryReason, BoundaryRecovery, DEFAULT_MAX_DELIMITER_RECOVERIES,
    DEFAULT_MAX_IMPLICIT_VR_ELEMENTS, MAX_PRIVATE_SEQUENCE_CHECK_BYTES, Reader, ReaderOptions,
    SeqType, normalize_read_error, required_read_error,
};
use crate::object::dict;
use crate::object::error::{Error, Kind, Op, Result, quote_bytes};
use crate::object::types::{ElementHeader, Tag, UNDEFINED_LENGTH, Vr};
pub(super) fn read_u16(r: &mut dyn ByteRead, big: bool) -> Result<u16> {
    let mut b = [0u8; 2];
    if let (_, Some(err)) = source::read_full(r, &mut b) {
        return Err(Error::wrap("dicom: read uint16", err));
    }
    Ok(if big {
        u16::from_be_bytes(b)
    } else {
        u16::from_le_bytes(b)
    })
}
pub(super) fn read_u32(r: &mut dyn ByteRead, big: bool) -> Result<u32> {
    let mut b = [0u8; 4];
    if let (_, Some(err)) = source::read_full(r, &mut b) {
        return Err(Error::wrap("dicom: read uint32", err));
    }
    Ok(if big {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    })
}
fn read_tag(r: &mut dyn ByteRead, big: bool) -> Result<Tag> {
    let group = read_u16(r, big).map_err(|e| Error::wrap("dicom: read tag group", e))?;
    let element = read_u16(r, big).map_err(|e| Error::wrap("dicom: read tag element", e))?;
    Ok(Tag::new(group, element))
}
impl Reader {
    pub(super) fn current_big_endian(&self) -> bool {
        if self.implicit_vr_little_endian_active() {
            return false;
        }
        if self.explicit_big_endian_active() {
            return true;
        }
        self.syntax.big_endian
    }
    pub(super) fn read_header(&mut self) -> Result<ElementHeader> {
        let header_offset = self.position();
        let header = self.read_header_fenced()?;
        if self.seq_recovery == Default::default() || header.tag.is_sequence_delimiting() {
            return Ok(header);
        }
        self.apply_sequence_recovery(header, header_offset)
    }
    fn read_header_fenced(&mut self) -> Result<ElementHeader> {
        self.last_reserved_nonzero = false;
        let header_offset = self.position();
        let fenced = if let Some((_, end)) = self.defined_container() {
            self.counter.fence = end as i64;
            self.counter.fence_hit = false;
            true
        } else {
            false
        };
        let result = self.read_header_bytes(header_offset);
        if fenced {
            self.counter.fence = 0;
        }
        match result {
            Err(_) if self.counter.fence_hit => Err(Error::parse(
                Op::ReadTag,
                header_offset,
                Tag::default(),
                None,
                0,
                Some(Error::kind_suffix(
                    Kind::ContainerOverrun,
                    ": element header crosses the container end",
                )),
            )),
            other => other,
        }
    }
    fn read_header_bytes(&mut self, header_offset: i64) -> Result<ElementHeader> {
        let mut big = self.current_big_endian();
        let mut tag = match read_tag(&mut self.counter, big) {
            Ok(tag) => tag,
            Err(err) => {
                let err = normalize_read_error(header_offset, self.position(), err);
                if self.position() == header_offset && err.is(Kind::Eof) {
                    return Err(Error::eof());
                }
                return Err(Error::parse(
                    Op::ReadTag,
                    header_offset,
                    Tag::default(),
                    None,
                    0,
                    Some(err),
                ));
            }
        };
        if self.recover_delimiter_anomalies
            && tag.group == 0xFEFF
            && !self.seq_delimiters.is_empty()
            && let Some(swapped) = swapped_control_tag(tag)
        {
            self.count_delimiter_recovery(header_offset)?;
            self.record_boundary(BoundaryRecovery::new(
                header_offset,
                -1,
                -1,
                BoundaryReason::SwappedItemTag,
            ));
            tag = swapped;
            big = true;
            self.swapped_item_pending = tag.is_item();
        }
        if tag.is_sequence_delimiting() {
            let length = self.read_long_length_with(tag, Vr::UN, big)?;
            return Ok(ElementHeader::new(tag, Vr::UN, length));
        }
        let header = if self.syntax.explicit_vr && !self.implicit_vr_little_endian_active() {
            self.read_explicit_header(tag)
        } else {
            self.read_implicit_header(tag)
        }?;
        if header.vr != Vr::LO {
            self.capture_private_reservation(&header, None)?;
        }
        Ok(header)
    }
    fn read_explicit_header(&mut self, tag: Tag) -> Result<ElementHeader> {
        let mut vr_bytes = [0u8; 2];
        let vr_offset = self.position();
        if let (_, Some(err)) = source::read_full(&mut self.counter, &mut vr_bytes) {
            return Err(Error::parse(
                Op::ReadVr,
                vr_offset,
                tag,
                None,
                0,
                Some(required_read_error(err)),
            ));
        }
        let mut parsed = Vr::parse(&vr_bytes);
        if parsed.is_none()
            && self.vr_recovery.map_ox_to_ow
            && tag == Tag::PIXEL_DATA
            && vr_bytes == *b"OX"
        {
            parsed = Some(Vr::OW);
            self.record_vr_repair(
                tag,
                vr_offset.saturating_sub(4),
                "OX",
                "OW",
                BoundaryReason::OxPixelData,
            );
        }
        if parsed.is_none()
            && self.recover_implicit_vr_elements
            && let Some(header) = self.read_implicit_after_invalid_vr(tag, vr_bytes, vr_offset)?
        {
            return Ok(header);
        }
        let Some(mut vr) = parsed else {
            return Err(Error::parse(
                Op::ReadVr,
                vr_offset,
                tag,
                None,
                0,
                Some(Error::msg(format!(
                    "dicom: invalid VR {}",
                    quote_bytes(&vr_bytes)
                ))),
            ));
        };
        let mut length_vr = vr;
        if (self.vr_recovery.prefer_dictionary_vr
            || self.vr_recovery.prefer_dictionary_length_field_size)
            && !tag.is_private()
            && let Some(dict_vr) = self.single_dictionary_vr(tag)
            && dict_vr != vr
        {
            if self.vr_recovery.prefer_dictionary_length_field_size
                && dict_vr.uses_long_explicit_length() != vr.uses_long_explicit_length()
            {
                length_vr = dict_vr;
                self.record_vr_repair(
                    tag,
                    vr_offset.saturating_sub(4),
                    vr.as_str(),
                    dict_vr.as_str(),
                    BoundaryReason::DictionaryLengthField,
                );
            }
            if self.vr_recovery.prefer_dictionary_vr {
                self.record_vr_repair(
                    tag,
                    vr_offset.saturating_sub(4),
                    vr.as_str(),
                    dict_vr.as_str(),
                    BoundaryReason::DictionaryVr,
                );
                vr = dict_vr;
            }
        }
        let length = if length_vr.uses_long_explicit_length() {
            let mut reserved = [0u8; 2];
            let reserved_offset = self.position();
            if let (_, Some(err)) = source::read_full(&mut self.counter, &mut reserved) {
                return Err(Error::parse(
                    Op::ReadReserved,
                    reserved_offset,
                    tag,
                    Some(vr),
                    0,
                    Some(required_read_error(err)),
                ));
            }
            if self.strict_reserved_bytes && reserved != [0, 0] {
                return Err(Error::parse(
                    Op::ValidateReserved,
                    reserved_offset,
                    tag,
                    Some(vr),
                    0,
                    Some(Error::msg(format!(
                        "dicom: explicit VR long-form reserved bytes must be zero: got {:02X}{:02X}",
                        reserved[0], reserved[1]
                    ))),
                ));
            }
            if self.vr_recovery.short_length_un
                && vr == Vr::UN
                && let Some(header) = self.read_short_length_un(tag, vr_offset, reserved)?
            {
                return Ok(header);
            }
            if reserved != [0, 0] {
                self.last_reserved_nonzero = true;
                self.last_reserved_offset = reserved_offset;
            }
            self.read_long_length(tag, vr)?
        } else {
            let length_offset = self.position();
            let big = self.current_big_endian();
            let u16v = read_u16(&mut self.counter, big).map_err(|err| {
                Error::parse(
                    Op::ReadLength,
                    length_offset,
                    tag,
                    Some(vr),
                    0,
                    Some(required_read_error(err)),
                )
            })?;
            let mut l = u16v as u32;
            if l == 6
                && vr == Vr::UL
                && tag.is_private()
                && self.vr_recovery.correct_private_ul_length
            {
                self.record_vr_repair(
                    tag,
                    vr_offset.saturating_sub(4),
                    "UL",
                    "UL",
                    BoundaryReason::PrivateUlLength,
                );
                l = 4;
            }
            l
        };
        Ok(ElementHeader::new(tag, vr, length))
    }
    fn implicit_vr(&mut self, tag: Tag) -> Vr {
        if self.private_active()
            && tag != Tag::PIXEL_DATA
            && (tag.group % 2 == 1 || dict::is_private_creator(tag))
        {
            return self.lookup_private_vr(tag);
        }
        if !self.no_dictionary {
            if let Some(chained) = self.chained_dictionary() {
                let overlay = tag.element == 0x3000 && tag.group >> 8 == 0x60 && tag.group & 1 == 0;
                if tag == Tag::PIXEL_DATA || overlay {
                    return Vr::OW;
                }
                if dict::is_private_creator(tag) {
                    return super::private::creator_vr(tag, Some(chained));
                }
                return super::private::chained_vr(tag, chained);
            }
            return lookup_implicit_vr(tag);
        }
        let overlay = tag.element == 0x3000 && tag.group >> 8 == 0x60 && tag.group & 1 == 0;
        if tag == Tag::PIXEL_DATA || overlay {
            return Vr::OW;
        }
        if dict::is_private_creator(tag) {
            Vr::LO
        } else {
            Vr::UN
        }
    }
    fn read_implicit_header(&mut self, tag: Tag) -> Result<ElementHeader> {
        let vr = self.implicit_vr(tag);
        let mut length = self.read_long_length(tag, vr)?;
        if length == 13
            && self.vr_recovery.correct_ge_text_length
            && self.correct_ge_text_length(tag, vr)?
        {
            length = 10;
        }
        Ok(ElementHeader::new(
            tag,
            implicit_length_vr(tag, vr, length),
            length,
        ))
    }
    pub(super) fn read_long_length(&mut self, tag: Tag, vr: Vr) -> Result<u32> {
        let big = self.current_big_endian();
        self.read_long_length_with(tag, vr, big)
    }
    fn read_long_length_with(&mut self, tag: Tag, vr: Vr, big: bool) -> Result<u32> {
        let length_offset = self.position();
        read_u32(&mut self.counter, big).map_err(|err| {
            Error::parse(
                Op::ReadLength,
                length_offset,
                tag,
                Some(vr),
                0,
                Some(required_read_error(err)),
            )
        })
    }
    fn read_implicit_after_invalid_vr(
        &mut self,
        tag: Tag,
        vr_bytes: [u8; 2],
        vr_offset: i64,
    ) -> Result<Option<ElementHeader>> {
        if tag == Tag::PIXEL_DATA || !self.syntax.is_little_endian() {
            return Ok(None);
        }
        let mut rest = [0u8; 2];
        if source::read_full(&mut self.counter, &mut rest).1.is_some() {
            return Ok(None);
        }
        let length = u32::from_le_bytes([vr_bytes[0], vr_bytes[1], rest[0], rest[1]]);
        if length != UNDEFINED_LENGTH {
            if let Some(remaining) = self.remaining_in_container()
                && length as u64 > remaining
            {
                return Ok(None);
            }
            if self.source_end > 0
                && length as i64 > self.source_end.saturating_sub(self.position())
            {
                return Ok(None);
            }
        }
        let limit = if self.max_implicit_vr_elements <= 0 {
            DEFAULT_MAX_IMPLICIT_VR_ELEMENTS
        } else {
            self.max_implicit_vr_elements
        };
        if self.implicit_vr_elements >= limit {
            return Err(Error::parse(
                Op::ReadVr,
                vr_offset,
                tag,
                None,
                0,
                Some(Error::msg(
                    "dicom: maximum implicit VR element switches exceeded",
                )),
            ));
        }
        self.implicit_vr_elements = self.implicit_vr_elements.saturating_add(1);
        let vr = implicit_length_vr(tag, self.implicit_vr(tag), length);
        let declared = if length == UNDEFINED_LENGTH {
            -1
        } else {
            length as i64
        };
        let path = self.child_path(tag);
        self.record_boundary_path(
            BoundaryRecovery::new(
                vr_offset.saturating_sub(4),
                declared,
                declared,
                BoundaryReason::ImplicitVrElement,
            ),
            path,
        );
        Ok(Some(ElementHeader::new(tag, vr, length)))
    }
    pub(super) fn count_delimiter_recovery(&mut self, offset: i64) -> Result<()> {
        let limit = if self.max_delimiter_recoveries <= 0 {
            DEFAULT_MAX_DELIMITER_RECOVERIES
        } else {
            self.max_delimiter_recoveries
        };
        if self.delimiter_recoveries >= limit {
            return Err(Error::parse(
                Op::ReadTag,
                offset,
                Tag::default(),
                None,
                0,
                Some(Error::msg("dicom: maximum delimiter recoveries exceeded")),
            ));
        }
        self.delimiter_recoveries = self.delimiter_recoveries.saturating_add(1);
        Ok(())
    }
    fn short_length(&self, b: [u8; 2]) -> u16 {
        if !self.syntax.is_little_endian() || self.explicit_big_endian_active() {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        }
    }
    fn record_vr_repair(
        &mut self,
        tag: Tag,
        offset: i64,
        declared: &str,
        used: &str,
        reason: BoundaryReason,
    ) {
        let path = self.child_path(tag);
        let mut recovery = BoundaryRecovery::new(offset, -1, -1, reason);
        recovery.declared_vr = declared.to_string();
        recovery.used_vr = used.to_string();
        self.record_boundary_path(recovery, path);
    }
    fn read_short_length_un(
        &mut self,
        tag: Tag,
        vr_offset: i64,
        reserved: [u8; 2],
    ) -> Result<Option<ElementHeader>> {
        if reserved != [0, 0] {
            let short = self.short_length(reserved);
            if let Some(left) = self.remaining_in_container()
                && short as u64 > left
            {
                return Ok(None);
            }
            if self.source_end > 0 && short as i64 > self.source_end.saturating_sub(self.position())
            {
                return Ok(None);
            }
            self.record_vr_repair(
                tag,
                vr_offset.saturating_sub(4),
                "UN",
                "UN",
                BoundaryReason::ShortLengthUn,
            );
            return Ok(Some(ElementHeader::new(tag, Vr::UN, short as u32)));
        }
        let mut ahead = [0u8; 4];
        if let (_, Some(err)) = source::read_full(&mut self.counter, &mut ahead) {
            return Err(Error::parse(
                Op::ReadLength,
                self.position(),
                tag,
                Some(Vr::UN),
                0,
                Some(required_read_error(err)),
            ));
        }
        let mut long = u32::from_le_bytes(ahead) as u64;
        let short = self.short_length(reserved) as u64;
        if !self.syntax.is_little_endian() || self.explicit_big_endian_active() {
            long = u32::from_be_bytes(ahead) as u64;
        }
        let mut remaining: i64 = -1;
        if self.source_end > 0 {
            remaining = self.source_end.saturating_sub(self.position());
        }
        if let Some(left) = self.remaining_in_container()
            && (remaining < 0 || (left as i64) < remaining)
        {
            remaining = left as i64;
        }
        let fits_short = remaining >= 0 && short as i64 <= remaining.saturating_add(4);
        let implausible_long =
            long != UNDEFINED_LENGTH as u64 && remaining >= 0 && long as i64 > remaining;
        if !implausible_long || !fits_short {
            return Ok(Some(ElementHeader::new(tag, Vr::UN, long as u32)));
        }
        self.counter.unread(&ahead);
        self.record_vr_repair(
            tag,
            vr_offset.saturating_sub(4),
            "UN",
            "UN",
            BoundaryReason::ShortLengthUn,
        );
        Ok(Some(ElementHeader::new(tag, Vr::UN, short as u32)))
    }
    fn apply_sequence_recovery(
        &mut self,
        mut header: ElementHeader,
        header_offset: i64,
    ) -> Result<ElementHeader> {
        let tag = header.tag;
        let declared = if header.is_undefined() {
            -1
        } else {
            header.length as i64
        };
        let record = |r: &mut Reader, reason: BoundaryReason, used: Vr, header: &ElementHeader| {
            let path = r.child_path(tag);
            let mut recovery = BoundaryRecovery::new(header_offset, declared, -1, reason);
            recovery.declared_vr = header.vr.as_str().to_string();
            recovery.used_vr = used.as_str().to_string();
            r.record_boundary_path(recovery, path);
        };
        if self.seq_recovery.voi_lut_sequence_ow_as_sq
            && header.tag == Tag::new(0x0028, 0x3010)
            && header.vr != Vr::SQ
        {
            record(self, BoundaryReason::VoiLutAsSequence, Vr::SQ, &header);
            header.vr = Vr::SQ;
        } else if self.seq_recovery.undefined_length_obow_as_sq
            && header.is_undefined()
            && header.tag != Tag::PIXEL_DATA
            && (header.vr == Vr::OB || header.vr == Vr::OW)
        {
            if self.next_is_item() {
                record(self, BoundaryReason::ObowAsSequence, Vr::SQ, &header);
                header.vr = Vr::SQ;
            }
        } else if self.seq_recovery.private_sequence_as_bytes
            && header.vr == Vr::SQ
            && header.tag.is_private()
            && !header.is_undefined()
            && header.length as u64 <= MAX_PRIVATE_SEQUENCE_CHECK_BYTES
            && !self.private_sequence_parses(&header)?
        {
            record(self, BoundaryReason::PrivateSequenceBytes, Vr::UN, &header);
            header.vr = Vr::UN;
        }
        Ok(header)
    }
    fn next_is_item(&mut self) -> bool {
        let mut ahead = [0u8; 4];
        let (n, err) = source::read_full(&mut self.counter, &mut ahead);
        self.counter.unread(ahead.get(..n).unwrap_or_default());
        if err.is_some() {
            return false;
        }
        let (group, element) =
            if !self.syntax.is_little_endian() || self.explicit_big_endian_active() {
                (
                    u16::from_be_bytes([ahead[0], ahead[1]]),
                    u16::from_be_bytes([ahead[2], ahead[3]]),
                )
            } else {
                (
                    u16::from_le_bytes([ahead[0], ahead[1]]),
                    u16::from_le_bytes([ahead[2], ahead[3]]),
                )
            };
        group == 0xFFFE && element == 0xE000
    }
    fn private_sequence_parses(&mut self, header: &ElementHeader) -> Result<bool> {
        let value_offset = self.position();
        let mut value = vec![0u8; header.length as usize];
        if let (_, Some(err)) = source::read_full(&mut self.counter, &mut value) {
            return Err(Error::parse(
                Op::ReadValue,
                value_offset,
                header.tag,
                Some(header.vr),
                header.length,
                Some(required_read_error(err)),
            ));
        }
        let size = value.len() as i64;
        let file = source::MemFile::new(value.clone().into());
        let mut sub = Reader::new(
            Box::new(file),
            Some((size, 0)),
            self.syntax,
            ReaderOptions {
                base_offset: value_offset,
                max_sequence_depth: self.max_sequence_depth,
                max_elements: self.max_elements,
                recover_delimiter_anomalies: self.recover_delimiter_anomalies,
                recover_implicit_vr_elements: self.recover_implicit_vr_elements,
                vr_recovery: self.vr_recovery,
                ..Default::default()
            },
        );
        let result = (|| {
            if sub
                .push_sequence_token(SeqType::Sequence, header.length, false)
                .is_err()
            {
                return false;
            }
            sub.delimiter_check_pending = true;
            loop {
                match sub.next() {
                    Err(_) => return false,
                    Ok(tok) => {
                        if tok.kind == super::TokenKind::EndSequence
                            && sub.seq_delimiters.is_empty()
                        {
                            return sub.position()
                                == value_offset.saturating_add(header.length as i64);
                        }
                    }
                }
            }
        })();
        self.counter.unread(&value);
        Ok(result)
    }
}
pub(super) fn lookup_implicit_vr(tag: Tag) -> Vr {
    if tag == Tag::PIXEL_DATA {
        return Vr::OW;
    }
    if tag.element == 0x3000 && tag.group >> 8 == 0x60 && tag.group & 1 == 0 {
        return Vr::OW;
    }
    if dict::is_private_creator(tag) {
        return dict::by_tag(tag).map_or(Vr::LO, |e| e.vr);
    }
    dict::lookup_vr(tag)
}
pub(super) fn implicit_length_vr(tag: Tag, vr: Vr, length: u32) -> Vr {
    if length == UNDEFINED_LENGTH && vr == Vr::LO && dict::is_private_creator(tag) {
        return Vr::UN;
    }
    vr
}
fn swapped_control_tag(tag: Tag) -> Option<Tag> {
    match tag.element {
        0x00E0 => Some(Tag::new(0xFFFE, 0xE000)),
        0x0DE0 => Some(Tag::new(0xFFFE, 0xE00D)),
        0xDDE0 => Some(Tag::new(0xFFFE, 0xE0DD)),
        _ => None,
    }
}
