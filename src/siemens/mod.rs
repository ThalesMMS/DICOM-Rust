#![forbid(unsafe_code)]
mod ascconv;
mod csa;
mod embedded;
mod medcom;
use crate::dictionary::reservations::private_tag;
use crate::object::{Object, Tag, Value};
pub use ascconv::{MAX_PROTOCOL_SLICES, Protocol, ProtocolEntry, ProtocolSlice, parse_ascconv};
pub use csa::{
    CsaElement, CsaFormat, CsaHeader, CsaLimits, CsaScrubReport, encode_csa, parse_csa,
    parse_csa_with_limits, scrub_csa,
};
pub use embedded::{
    EmbeddedDataset, EmbeddedDatasetLimits, EmbeddedError, EmbeddedKind, embedded_dataset,
    embedded_datasets, embedded_datasets_of, is_embedded_dataset_field,
};
pub use medcom::{
    MedComHistory, MedComLimits, MedComSection, medcom_history_of, parse_medcom_history,
};
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    InvalidCsa,
    CsaLimit,
    NoCsaHeader,
    NoAscconv,
    Other,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::InvalidCsa => "dicom/siemens: invalid CSA header",
            Kind::CsaLimit => "dicom/siemens: CSA header exceeds limits",
            Kind::NoCsaHeader => "dicom/siemens: no CSA header",
            Kind::NoAscconv => "dicom/siemens: no ASCCONV protocol",
            Kind::Other => "",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
}
impl Error {
    pub(crate) fn new(kind: Kind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            text: format!("{}: {detail}", kind.text()),
        }
    }
    pub(crate) fn bare(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
        }
    }
    pub(crate) fn other(text: impl Into<String>) -> Error {
        Error {
            kind: Kind::Other,
            text: text.into(),
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
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub(crate) fn trim_space(s: &str) -> &str {
    s.trim_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}'
        ) || (c as u32 > 0xff && c.is_whitespace())
    })
}
const CSA_GROUP: u16 = 0x0029;
const CSA_CREATOR: &str = "SIEMENS CSA HEADER";
const CSA_IMAGE_OFFSET: u8 = 0x10;
const CSA_SERIES_OFFSET: u8 = 0x20;
const MR_HEADER_CREATOR: &str = "SIEMENS MR HEADER";
#[derive(Clone, Copy, Debug, Default)]
pub struct LookupOptions {
    pub allow_missing_creator: bool,
    pub limits: CsaLimits,
}
#[derive(Clone, Debug)]
pub struct LocatedHeader {
    pub header: CsaHeader,
    pub tag: Tag,
    pub creator_missing: bool,
}
pub fn image_header(dataset: &Object, opts: LookupOptions) -> Result<LocatedHeader, Error> {
    locate_header(dataset, CSA_IMAGE_OFFSET, opts)
}
pub fn series_header(dataset: &Object, opts: LookupOptions) -> Result<LocatedHeader, Error> {
    locate_header(dataset, CSA_SERIES_OFFSET, opts)
}
fn locate_header(
    dataset: &Object,
    offset: u8,
    opts: LookupOptions,
) -> Result<LocatedHeader, Error> {
    let mut creator_missing = false;
    let tag = match private_tag(dataset, CSA_GROUP, CSA_CREATOR, offset) {
        Ok(tag) => tag,
        Err(err) => {
            if !opts.allow_missing_creator || !looks_siemens(dataset) {
                return Err(Error::new(Kind::NoCsaHeader, err));
            }
            creator_missing = true;
            Tag::new(CSA_GROUP, 0x1000 | offset as u16)
        }
    };
    let raw = match dataset.get_raw(tag) {
        Some(raw) if !raw.is_empty() => raw,
        _ => return Err(Error::bare(Kind::NoCsaHeader)),
    };
    let header = parse_csa_with_limits(&raw, opts.limits)?;
    Ok(LocatedHeader {
        header,
        tag,
        creator_missing,
    })
}
fn looks_siemens(dataset: &Object) -> bool {
    if let Some(manufacturer) = dataset.get_string(Tag::new(0x0008, 0x0070))
        && crate::textfmt::to_upper(&manufacturer).contains("SIEMENS")
    {
        return true;
    }
    is_mosaic(dataset)
}
pub fn is_mosaic(dataset: &Object) -> bool {
    let Some(values) = dataset.get_strings(Tag::new(0x0008, 0x0008)) else {
        return false;
    };
    values
        .iter()
        .any(|v| trim_space(v).eq_ignore_ascii_case("MOSAIC"))
}
pub fn series_protocol(dataset: &Object, opts: LookupOptions) -> Result<Protocol, Error> {
    let header = series_header(dataset, opts)?;
    for name in ["MrPhoenixProtocol", "MrProtocol"] {
        let Some(element) = header.header.element(name) else {
            continue;
        };
        for value in &element.values {
            if let Ok(protocol) = parse_ascconv(&String::from_utf8_lossy(value)) {
                return Ok(protocol);
            }
        }
    }
    Err(Error::bare(Kind::NoAscconv))
}
pub fn number_of_images_in_mosaic(
    dataset: &Object,
    opts: LookupOptions,
) -> Result<(i64, &'static str), Error> {
    if let Ok(header) = image_header(dataset, opts)
        && let Some(element) = header.header.element("NumberOfImagesInMosaic")
        && !element.values.is_empty()
    {
        let count = element.int()?;
        if count > 0 {
            return Ok((count, "csa"));
        }
    }
    if let Ok(tag) = private_tag(dataset, 0x0019, MR_HEADER_CREATOR, 0x0a)
        && let Some(values) = uint16s(dataset, tag)
        && values.first().is_some_and(|&v| v > 0)
    {
        return Ok((values[0] as i64, "mr-header"));
    }
    Err(Error::new(Kind::NoCsaHeader, "NumberOfImagesInMosaic"))
}
pub fn slice_normal_vector(dataset: &Object, opts: LookupOptions) -> Result<[f64; 3], Error> {
    let header = image_header(dataset, opts)?;
    let element = match header.header.element("SliceNormalVector") {
        Some(e) if e.values.len() >= 3 => e,
        _ => return Err(Error::new(Kind::NoCsaHeader, "SliceNormalVector")),
    };
    let values = element.float64s()?;
    Ok([values[0], values[1], values[2]])
}
pub fn uint16s(dataset: &Object, tag: Tag) -> Option<Vec<u16>> {
    let element = dataset.get(tag)?;
    let Value::Raw(raw) = &element.value else {
        return None;
    };
    if raw.len() < 2 || raw.len() % 2 != 0 {
        return None;
    }
    let big = dataset.value_big_endian();
    Some(
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if big {
                    u16::from_be_bytes([c[0], c[1]])
                } else {
                    u16::from_le_bytes([c[0], c[1]])
                }
            })
            .collect(),
    )
}
