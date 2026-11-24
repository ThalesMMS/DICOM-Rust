use super::{Error, Kind};
use crate::numtext;
use crate::object::{self, Object};
use crate::textfmt::quote;
use std::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsaFormat {
    Sv10,
    Nosv,
    Dataset,
    Interfile,
    Xml,
    Zeroed,
}
impl CsaFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            CsaFormat::Sv10 => "SV10",
            CsaFormat::Nosv => "NOSV",
            CsaFormat::Dataset => "DATASET",
            CsaFormat::Interfile => "INTERFILE",
            CsaFormat::Xml => "XML",
            CsaFormat::Zeroed => "ZEROED",
        }
    }
}
impl fmt::Display for CsaFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CsaLimits {
    pub max_bytes: i64,
    pub max_elements: i64,
    pub max_items: i64,
}
impl CsaLimits {
    fn normalized(self) -> CsaLimits {
        CsaLimits {
            max_bytes: if self.max_bytes <= 0 {
                64 << 20
            } else {
                self.max_bytes
            },
            max_elements: if self.max_elements <= 0 {
                4096
            } else {
                self.max_elements
            },
            max_items: if self.max_items <= 0 {
                65536
            } else {
                self.max_items
            },
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CsaElement {
    pub name: String,
    pub vm: i64,
    pub vr: String,
    pub syngo_dt: i64,
    pub items: i64,
    pub values: Vec<Vec<u8>>,
}
fn trim_unicode_space(b: &[u8]) -> &[u8] {
    let s = String::from_utf8_lossy(b);
    if s.len() == b.len() && std::str::from_utf8(b).is_ok() {
        let t = super::trim_space(std::str::from_utf8(b).unwrap_or(""));
        let start = t.as_ptr() as usize - b.as_ptr() as usize;
        return &b[start..start + t.len()];
    }
    let ws = |c: &u8| matches!(*c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let start = b.iter().position(|c| !ws(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !ws(c)).map_or(start, |e| e + 1);
    &b[start..end]
}
impl CsaElement {
    pub fn strings(&self) -> Vec<Vec<u8>> {
        self.values
            .iter()
            .map(|v| trim_unicode_space(v).to_vec())
            .collect()
    }
    pub fn float64s(&self) -> Result<Vec<f64>, Error> {
        let mut out = Vec::new();
        for (i, value) in self.strings().iter().enumerate() {
            let parsed = std::str::from_utf8(value)
                .ok()
                .and_then(|v| numtext::parse_float(v).ok())
                .filter(|f| f.is_finite());
            match parsed {
                Some(f) => out.push(f),
                None => {
                    return Err(Error::new(
                        Kind::InvalidCsa,
                        format!("element {} value {i} is not a number", quote(&self.name)),
                    ));
                }
            }
        }
        Ok(out)
    }
    pub fn int(&self) -> Result<i64, Error> {
        let values = self.strings();
        let Some(first) = values.first() else {
            return Err(Error::new(
                Kind::InvalidCsa,
                format!("element {} is empty", quote(&self.name)),
            ));
        };
        let text = String::from_utf8_lossy(first).into_owned();
        if let Ok(n) = numtext::parse_int(&text) {
            return Ok(n);
        }
        match numtext::parse_float(&text) {
            Ok(f) if f == f.trunc() && f.abs() <= i32::MAX as f64 => Ok(f as i64),
            _ => Err(Error::new(
                Kind::InvalidCsa,
                format!("element {} is not an integer", quote(&self.name)),
            )),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct CsaHeader {
    pub format: Option<CsaFormat>,
    pub byte_swapped: bool,
    pub elements: Vec<CsaElement>,
    pub dataset: Option<Object>,
    pub text: Vec<u8>,
}
impl CsaHeader {
    pub fn element(&self, name: &str) -> Option<&CsaElement> {
        self.elements.iter().find(|e| e.name == name)
    }
    pub fn format(&self) -> CsaFormat {
        self.format.unwrap_or(CsaFormat::Nosv)
    }
}
const CSA_ELEMENT_HEADER_BYTES: u64 = 64 + 4 + 4 + 4 + 4 + 4;
const CSA_ITEM_HEADER_BYTES: u64 = 16;
const MARKER77: u32 = 77;
const MARKER205: u32 = 205;
pub fn parse_csa(data: &[u8]) -> Result<CsaHeader, Error> {
    parse_csa_with_limits(data, CsaLimits::default())
}
fn trim_right_nul(b: &[u8]) -> &[u8] {
    let end = b.iter().rposition(|&c| c != 0).map_or(0, |e| e + 1);
    &b[..end]
}
pub fn parse_csa_with_limits(data: &[u8], limits: CsaLimits) -> Result<CsaHeader, Error> {
    let limits = limits.normalized();
    if data.len() as i64 > limits.max_bytes {
        return Err(Error::new(Kind::CsaLimit, format!("{} bytes", data.len())));
    }
    if data.len() < 4 {
        return Err(Error::new(Kind::InvalidCsa, "too short"));
    }
    let mut header = CsaHeader::default();
    let swapped;
    let data: &[u8] = if &data[..4] == b"VS01" {
        let mut copy = data[..data.len() & !1].to_vec();
        swap_words(&mut copy);
        header.byte_swapped = true;
        swapped = copy;
        &swapped
    } else {
        data
    };
    if data.starts_with(b"SV10") {
        if data.len() < 16 || data[4..8] != [4, 3, 2, 1] {
            return Err(Error::new(Kind::InvalidCsa, "SV10 signature"));
        }
        header.format = Some(CsaFormat::Sv10);
        header.elements = CsaReader::new(&data[8..], CsaFormat::Sv10, false).elements(limits)?;
    } else if data.iter().all(|&b| b == 0) {
        header.format = Some(CsaFormat::Zeroed);
    } else if data.starts_with(b"!INTERFILE") {
        header.format = Some(CsaFormat::Interfile);
        header.text = trim_right_nul(data).to_vec();
    } else if data[0] == b'<' && std::str::from_utf8(trim_right_nul(data)).is_ok() {
        header.format = Some(CsaFormat::Xml);
        header.text = trim_right_nul(data).to_vec();
    } else if data.starts_with(&[0, 0]) || data.starts_with(&[6, 0]) {
        let syntax = object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
        let opts = object::ReadFileOptions {
            max_element_bytes: data.len() as i64,
            ..Default::default()
        };
        let source = object::parser::source::MemFile::new(std::rc::Rc::from(data));
        match object::read_data_set_with_options(Box::new(source), syntax, opts) {
            Ok(ds) => {
                header.format = Some(CsaFormat::Dataset);
                header.dataset = Some(ds);
            }
            Err(e) => {
                return Err(Error::new(Kind::InvalidCsa, format!("DATASET format: {e}")));
            }
        }
    } else {
        header.format = Some(CsaFormat::Nosv);
        header.elements = CsaReader::new(data, CsaFormat::Nosv, false).elements(limits)?;
    }
    Ok(header)
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CsaScrubReport {
    pub format: Option<CsaFormat>,
    pub byte_swapped: bool,
    pub cleared: usize,
}
pub fn scrub_csa(
    data: &[u8],
    limits: CsaLimits,
) -> (Option<Vec<u8>>, CsaScrubReport, Option<Error>) {
    let header = match parse_csa_with_limits(data, limits) {
        Ok(h) => h,
        Err(e) => return (None, CsaScrubReport::default(), Some(e)),
    };
    let mut report = CsaScrubReport {
        format: header.format,
        byte_swapped: header.byte_swapped,
        cleared: 0,
    };
    match header.format() {
        CsaFormat::Zeroed => return (Some(data.to_vec()), report, None),
        CsaFormat::Sv10 | CsaFormat::Nosv => {}
        other => {
            return (
                None,
                report,
                Some(Error::new(
                    Kind::InvalidCsa,
                    format!("{other} headers are not scrubbed"),
                )),
            );
        }
    }
    let mut work = data.to_vec();
    if header.byte_swapped {
        swap_words(&mut work);
    }
    let start = if header.format() == CsaFormat::Sv10 {
        8
    } else {
        0
    };
    let (scrub, cleared) = {
        let mut r = CsaReader::new(&work[start..], header.format(), true);
        if let Err(e) = r.elements(limits.normalized()) {
            return (None, report, Some(e));
        }
        (r.scrub.unwrap_or_default(), r.cleared)
    };
    work[start..].copy_from_slice(&scrub);
    report.cleared = cleared;
    if header.byte_swapped {
        swap_words(&mut work);
    }
    (Some(work), report, None)
}
fn swap_words(data: &mut [u8]) {
    let mut i = 0;
    while i + 1 < data.len() {
        data.swap(i, i + 1);
        i += 2;
    }
}
struct CsaReader<'a> {
    data: &'a [u8],
    pos: usize,
    format: CsaFormat,
    first_tag_items: u32,
    scrub: Option<Vec<u8>>,
    cleared: usize,
}
impl<'a> CsaReader<'a> {
    fn new(data: &'a [u8], format: CsaFormat, scrub: bool) -> CsaReader<'a> {
        CsaReader {
            data,
            pos: 0,
            format,
            first_tag_items: 0,
            scrub: scrub.then(|| data.to_vec()),
            cleared: 0,
        }
    }
    fn clear(&mut self, from: usize, to: usize) {
        if let Some(scrub) = &mut self.scrub {
            let mut i = from;
            while i < to && i < scrub.len() {
                if scrub[i] != 0 {
                    scrub[i] = 0;
                    self.cleared += 1;
                }
                i += 1;
            }
        }
    }
    fn clear_after_nul(&mut self, from: usize, to: usize) {
        if self.scrub.is_none() {
            return;
        }
        if let Some(i) = self.data[from..to].iter().position(|&b| b == 0) {
            self.clear(from + i + 1, to);
        }
    }
    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if n > self.remaining() {
            return Err(Error::new(Kind::InvalidCsa, "truncated"));
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn elements(&mut self, limits: CsaLimits) -> Result<Vec<CsaElement>, Error> {
        let count = self.u32()?;
        let marker = self.u32()?;
        if marker != MARKER77 {
            return Err(Error::new(
                Kind::InvalidCsa,
                format!("header marker {marker}"),
            ));
        }
        if count as i64 > limits.max_elements {
            return Err(Error::new(Kind::CsaLimit, format!("{count} elements")));
        }
        if count as u64 * CSA_ELEMENT_HEADER_BYTES > self.remaining() as u64 {
            return Err(Error::new(
                Kind::InvalidCsa,
                format!("{count} elements do not fit in {} bytes", self.remaining()),
            ));
        }
        let mut elements = Vec::with_capacity(count as usize);
        for index in 0..count {
            let element = self
                .element(limits, index == 0)
                .map_err(|e| e.prefixed(format!("element {index}")))?;
            elements.push(element);
        }
        Ok(elements)
    }
    fn element(&mut self, limits: CsaLimits, first: bool) -> Result<CsaElement, Error> {
        let name = self.bytes(64)?;
        self.clear_after_nul(self.pos - 64, self.pos);
        let vm = self.u32()?;
        let vr = self.bytes(4)?;
        if vr[2] != 0 {
            return Err(Error::new(Kind::InvalidCsa, "VR is not NUL-terminated"));
        }
        self.clear_after_nul(self.pos - 4, self.pos);
        let syngo_dt = self.u32()?;
        let items = self.u32()?;
        let marker = self.u32()?;
        if marker != MARKER77 && marker != MARKER205 {
            return Err(Error::new(
                Kind::InvalidCsa,
                format!("element marker {marker}"),
            ));
        }
        if items as i64 > limits.max_items {
            return Err(Error::new(Kind::CsaLimit, format!("{items} items")));
        }
        if items as u64 * CSA_ITEM_HEADER_BYTES > self.remaining() as u64 {
            return Err(Error::new(
                Kind::InvalidCsa,
                format!("{items} items do not fit"),
            ));
        }
        if first {
            self.first_tag_items = items;
        }
        let mut element = CsaElement {
            name: String::from_utf8_lossy(c_string(name)).into_owned(),
            vm: vm as i32 as i64,
            vr: String::from_utf8_lossy(c_string(&vr[..3])).into_owned(),
            syngo_dt: syngo_dt as i32 as i64,
            items: items as i64,
            values: Vec::new(),
        };
        for _ in 0..items {
            let mut lengths = [0u32; 4];
            for l in lengths.iter_mut() {
                *l = self.u32()?;
            }
            if lengths[2] != MARKER77 && lengths[2] != MARKER205 {
                return Err(Error::new(
                    Kind::InvalidCsa,
                    format!("item marker {}", lengths[2]),
                ));
            }
            let mut length = lengths[1];
            if self.format == CsaFormat::Nosv {
                if lengths[0] < self.first_tag_items {
                    return Err(Error::new(Kind::InvalidCsa, "negative NOSV item length"));
                }
                length = lengths[0] - self.first_tag_items;
            }
            if length as u64 > self.remaining() as u64 {
                return Err(Error::new(Kind::InvalidCsa, "item exceeds the header"));
            }
            let value = self.bytes(length as usize)?;
            self.clear_after_nul(self.pos - length as usize, self.pos);
            let pad = (((4 - length % 4) % 4) as usize).min(self.remaining());
            self.clear(self.pos, self.pos + pad);
            self.pos += pad;
            if length > 0 {
                element.values.push(c_string(value).to_vec());
            }
        }
        Ok(element)
    }
}
fn c_string(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(i) => &b[..i],
        None => b,
    }
}
pub fn encode_csa(elements: &[CsaElement]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let u32 = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    out.extend_from_slice(b"SV10");
    out.extend_from_slice(&[4, 3, 2, 1]);
    u32(&mut out, elements.len() as u32);
    u32(&mut out, MARKER77);
    for element in elements {
        if element.name.len() >= 64 || element.vr.len() > 3 || element.name.contains('\0') {
            return Err(Error::new(
                Kind::InvalidCsa,
                format!("element {} cannot be encoded", quote(&element.name)),
            ));
        }
        let mut name = [0u8; 64];
        name[..element.name.len()].copy_from_slice(element.name.as_bytes());
        out.extend_from_slice(&name);
        u32(&mut out, element.vm as i32 as u32);
        let mut vr = [0u8; 4];
        vr[..element.vr.len()].copy_from_slice(element.vr.as_bytes());
        out.extend_from_slice(&vr);
        u32(&mut out, element.syngo_dt as i32 as u32);
        let items = element.items.max(element.values.len() as i64).max(0) as usize;
        u32(&mut out, items as u32);
        u32(&mut out, MARKER77);
        for i in 0..items {
            let mut value = Vec::new();
            if i < element.values.len() {
                value.extend_from_slice(&element.values[i]);
                value.push(0);
            }
            let length = value.len() as u32;
            u32(&mut out, length);
            u32(&mut out, length);
            u32(&mut out, MARKER77);
            u32(&mut out, length);
            out.extend_from_slice(&value);
            out.extend(std::iter::repeat_n(0u8, (4 - value.len() % 4) % 4));
        }
    }
    Ok(out)
}
