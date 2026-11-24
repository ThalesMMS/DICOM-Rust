#![forbid(unsafe_code)]
use crate::dictionary::reservations::private_tag;
use crate::object::{Object, Tag, UNDEFINED_LENGTH};
use crate::textfmt::decode_rune;
use std::fmt;
mod blobs;
mod elscint_pixels;
mod gzip;
pub mod headers;
mod manufacturer;
mod private_headers;
pub use blobs::{
    ElscintEntry, ElscintGroup, ElscintProtocol, GeProtocolDataBlock, GeProtocolEntry, VeproData,
    elscint_protocol_of, ge_protocol_data_block_of, parse_elscint_protocol,
    parse_ge_protocol_data_block, parse_vepro_data, vepro_data_of,
};
pub use elscint_pixels::{
    DIAGNOSTIC_ELSCINT_PRIVATE_PIXELS, DIAGNOSTIC_ELSCINT_STANDARD_PIXEL_DATA,
    DIAGNOSTIC_ELSCINT_STORED, ELSCINT_PMSCT_RGB1, ELSCINT_PMSCT_RLE1, ElscintPixels,
    decode_elscint_pmsct_rgb1, decode_elscint_pmsct_rle1, elscint_pixel_data_of,
};
pub use headers::{
    MecMr3Options, MecMr3Record, PhilipsSdsItem, SdsValue, canon_pas_of, decode_canon_pas,
    decode_mec_mr3, decode_mec_mr3_with_options, decode_philips_sds, decode_pmtf,
    decode_toshiba_ct3, philips_sds_of, pmtf_of, toshiba_ct3_of, toshiba_mec_mr3_of,
};
pub use manufacturer::{
    EVIDENCE_MANUFACTURER, EVIDENCE_PRIVATE_CREATOR, Manufacturer, ManufacturerDetection,
    ManufacturerEvidence, detect_manufacturer, manufacturer_of_creator, manufacturer_of_name,
};
pub use private_headers::{
    PrivateHeader, PrivateHeaderInfo, PrivateHeaderLocation, PrivateHeaderOptions,
    decode_private_headers, detect_private_headers, private_header_info_of, private_header_kinds,
};
pub type ByteString = Vec<u8>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    NotFound,
    Invalid,
    Limit,
    UnsupportedHeader,
    Siemens(crate::siemens::Kind),
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::NotFound => "dicom/vendors: private blob not found",
            Kind::Invalid => "dicom/vendors: invalid private blob",
            Kind::Limit => "dicom/vendors: private blob exceeds limits",
            Kind::UnsupportedHeader => "dicom/vendors: unsupported private header variant",
            Kind::Siemens(kind) => kind.text(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
}
impl Error {
    pub(crate) fn bare(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
        }
    }
    pub(crate) fn new(kind: Kind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            text: format!("{}: {detail}", kind.text()),
        }
    }
    pub(crate) fn prefixed(self, prefix: impl fmt::Display) -> Error {
        Error {
            kind: self.kind,
            text: format!("{prefix}: {}", self.text),
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
    }
}
impl From<crate::siemens::Error> for Error {
    fn from(err: crate::siemens::Error) -> Error {
        Error {
            kind: Kind::Siemens(err.kind),
            text: err.to_string(),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_bytes: i64,
    pub max_decoded_bytes: i64,
    pub max_entries: i64,
}
impl Limits {
    pub(crate) fn normalized(mut self) -> Limits {
        if self.max_bytes <= 0 {
            self.max_bytes = 16 << 20;
        }
        if self.max_decoded_bytes <= 0 {
            self.max_decoded_bytes = 16 << 20;
        }
        if self.max_entries <= 0 {
            self.max_entries = 100_000;
        }
        self
    }
}
pub(crate) fn private_value(
    dataset: &Object,
    group: u16,
    creators: &[&str],
    offset: u8,
    limits: Limits,
) -> Result<(Vec<u8>, Tag), Error> {
    for creator in creators {
        let Ok(tag) = private_tag(dataset, group, creator, offset) else {
            continue;
        };
        let Some(element) = dataset.get(tag) else {
            continue;
        };
        let length = element.header.length;
        if length != UNDEFINED_LENGTH && i64::from(length) > limits.max_bytes {
            return Err(Error::new(Kind::Limit, format!("{length} bytes")));
        }
        let Some(raw) = dataset.get_raw(tag) else {
            return Err(Error::new(
                Kind::Invalid,
                format!("{tag} has no byte value"),
            ));
        };
        if raw.len() as i64 > limits.max_bytes {
            return Err(Error::new(Kind::Limit, format!("{} bytes", raw.len())));
        }
        return Ok((raw, tag));
    }
    Err(Error::bare(Kind::NotFound))
}
pub(crate) struct Cursor<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}
impl Cursor<'_> {
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    pub fn uint32(&mut self) -> Result<u32, Error> {
        if self.remaining() < 4 {
            return Err(Error::new(
                Kind::Invalid,
                format!("truncated at byte {}", self.pos),
            ));
        }
        let b = &self.data[self.pos..self.pos + 4];
        self.pos += 4;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub(crate) fn c_string(&mut self) -> Result<ByteString, Error> {
        match self.data[self.pos..].iter().position(|&b| b == 0) {
            Some(n) => {
                let s = self.data[self.pos..self.pos + n].to_vec();
                self.pos += n + 1;
                Ok(s)
            }
            None => Err(Error::new(
                Kind::Invalid,
                format!("unterminated string at byte {}", self.pos),
            )),
        }
    }
}
fn decode_last_rune(b: &[u8]) -> (Option<char>, usize) {
    let end = b.len();
    if end == 0 {
        return (None, 0);
    }
    let floor = end.saturating_sub(4);
    let mut start = end - 1;
    while start > floor && b[start] & 0xC0 == 0x80 {
        start -= 1;
    }
    let (r, width) = decode_rune(&b[start..end]);
    if start + width != end {
        return (None, 1);
    }
    (r, width)
}
pub(crate) fn trim_unicode_space(mut b: &[u8]) -> &[u8] {
    loop {
        match decode_rune(b) {
            (Some(c), width) if width > 0 && c.is_whitespace() => b = &b[width..],
            _ => break,
        }
    }
    loop {
        match decode_last_rune(b) {
            (Some(c), width) if width > 0 && c.is_whitespace() => b = &b[..b.len() - width],
            _ => break,
        }
    }
    b
}
pub(crate) fn trim_right_bytes<'a>(b: &'a [u8], cutset: &[u8]) -> &'a [u8] {
    let end = b
        .iter()
        .rposition(|c| !cutset.contains(c))
        .map_or(0, |i| i + 1);
    &b[..end]
}
pub(crate) fn text_lines(text: &[u8]) -> Vec<ByteString> {
    let mut unix = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'\r' && text.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        unix.push(text[i]);
        i += 1;
    }
    let trimmed = trim_right_bytes(&unix, b"\n\x00 ");
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trims_like_go() {
        assert_eq!(trim_unicode_space(b" \t a b \xc2\xa0\r\n"), b"a b");
        assert_eq!(trim_unicode_space(b"\xff a \xff"), b"\xff a \xff");
        assert_eq!(trim_unicode_space(b"\xe2\x80\x83"), b"");
        assert_eq!(trim_right_bytes(b"ab \0 ", b" \0"), b"ab");
        assert_eq!(
            text_lines(b"a\r\nb\n\n \0"),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
        assert!(text_lines(b"\n \0").is_empty());
    }
    #[test]
    fn limits_default() {
        let l = Limits::default().normalized();
        assert_eq!(
            (l.max_bytes, l.max_decoded_bytes, l.max_entries),
            (16 << 20, 16 << 20, 100_000)
        );
    }
}
