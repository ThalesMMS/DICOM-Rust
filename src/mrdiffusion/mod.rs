#![forbid(unsafe_code)]
use crate::dictionary::reservations::private_tag as reserved_tag;
use crate::numtext::parse_float;
use crate::object::geometry::functional_group_source;
use crate::object::{Object, Tag, Vr};
use crate::siemens::{self, LookupOptions, trim_space};
use crate::textfmt::{float_v, quote, to_upper};
use crate::vendors::{Manufacturer, manufacturer_of_name};
use regex::Regex;
use std::fmt;
use std::sync::LazyLock;
mod adc;
mod siemens_embedded;
pub mod tensor;
mod tensor_maps;
pub use adc::{AdcMap, AdcOptions, fit_adc};
pub use siemens_embedded::{
    SOURCE_SIEMENS_CSA_EMBEDDED, SiemensEmbeddedError, SiemensEmbeddedOptions,
    read_siemens_embedded,
};
pub use tensor::{
    Tensor3, TensorEncoding, TensorGeometry, TensorOptions, TensorResult, TensorSource,
    TensorVolume, TensorVoxel, fit_tensor, read_tensor_encoding, read_tensor_source,
};
pub use tensor_maps::{TensorMapOptions, tensor_map, write_tensor_map};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    NotFound,
    Invalid,
    Adc,
    Tensor,
    TensorRank,
    Other,
}
impl Kind {
    fn text(self) -> &'static str {
        match self {
            Kind::NotFound => "dicom/mrdiffusion: no diffusion b-value",
            Kind::Invalid => "dicom/mrdiffusion: invalid diffusion attribute",
            Kind::Adc => "dicom/mrdiffusion: cannot fit ADC",
            Kind::Tensor => "mrdiffusion: invalid tensor reconstruction",
            Kind::TensorRank => "mrdiffusion: tensor design rank insufficient",
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
    pub(crate) fn bare(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
        }
    }
    pub(crate) fn new(kind: Kind, detail: impl AsRef<str>) -> Error {
        Error {
            kind,
            text: format!("{}: {}", kind.text(), detail.as_ref()),
        }
    }
    fn invalid(detail: impl AsRef<str>) -> Error {
        Error::new(Kind::Invalid, detail)
    }
    pub(crate) fn with_text(kind: Kind, text: impl Into<String>) -> Error {
        Error {
            kind,
            text: text.into(),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
pub mod source {
    pub const MR_DIFFUSION_MACRO: &str = "mr-diffusion-macro";
    pub const STANDARD_ATTRIBUTES: &str = "standard-attributes";
    pub const SIEMENS_CSA: &str = "siemens-csa";
    pub const SIEMENS_MR_HEADER: &str = "siemens-mr-header";
    pub const GE: &str = "ge-private";
    pub const PHILIPS: &str = "philips-private";
    pub const CANON_IMAGE_COMMENTS: &str = "canon-image-comments";
}
pub mod direction_frame {
    pub const PATIENT: &str = "patient";
    pub const PHILIPS_RL_AP_FH: &str = "philips-rl-ap-fh";
    pub const GE_GRADIENT: &str = "ge-gradient";
    pub const UNSPECIFIED: &str = "unspecified";
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub frame_index: i64,
    pub b_value: f64,
    pub direction: [f64; 3],
    pub has_direction: bool,
    pub direction_frame: &'static str,
    pub directionality: String,
    pub source: &'static str,
    pub functional_group: &'static str,
    pub tag: Tag,
}
impl Frame {
    pub fn is_diffusion_weighted(&self) -> bool {
        self.b_value > 0.0
    }
    fn new(source: &'static str, tag: Tag, direction_frame: &'static str) -> Frame {
        Frame {
            source,
            tag,
            direction_frame,
            functional_group: functional_group_source::NONE,
            ..Default::default()
        }
    }
    fn set_direction(&mut self, values: &[f64]) -> Result<(), Error> {
        if values.iter().any(|v| v.is_nan() || v.is_infinite()) {
            return Err(Error::invalid(format!(
                "gradient direction [{}]",
                values
                    .iter()
                    .map(|v| float_v(*v))
                    .collect::<Vec<_>>()
                    .join(" ")
            )));
        }
        if values[0] == 0.0 && values[1] == 0.0 && values[2] == 0.0 {
            return Ok(());
        }
        self.direction = [values[0], values[1], values[2]];
        self.has_direction = true;
        Ok(())
    }
}
const TAG_MR_DIFFUSION_SEQUENCE: Tag = Tag::new(0x0018, 0x9117);
const TAG_DIFFUSION_B_VALUE: Tag = Tag::new(0x0018, 0x9087);
const TAG_DIFFUSION_DIRECTIONALITY: Tag = Tag::new(0x0018, 0x9075);
const TAG_DIFFUSION_GRADIENT_DIR_SEQ: Tag = Tag::new(0x0018, 0x9076);
const TAG_DIFFUSION_GRADIENT_ORIENT: Tag = Tag::new(0x0018, 0x9089);
const TAG_MANUFACTURER: Tag = Tag::new(0x0008, 0x0070);
const TAG_IMAGE_COMMENTS: Tag = Tag::new(0x0020, 0x4000);
const SIEMENS_MR_HEADER_CREATOR: &[&str] = &["SIEMENS MR HEADER"];
const GE_PARM_CREATOR: &[&str] = &["GEMS_PARM_01"];
const GE_ACQU_CREATOR: &[&str] = &["GEMS_ACQU_01"];
const PHILIPS_IMAGING_CREATORS: &[&str] = &["Philips Imaging DD 001", "PHILIPS IMAGING DD 001"];
const PHILIPS_MR_IMAGING_CREATORS: &[&str] =
    &["Philips MR Imaging DD 001", "PHILIPS MR IMAGING DD 001"];
static CANON_IMAGE_COMMENT: LazyLock<Regex> = LazyLock::new(|| {
    const S: &str = r"[\t\n\f\r ]*";
    const N: &str = r"([-+0-9.eE]+)";
    Regex::new(&format!(
        r"(?i-u)\bb{S}={S}([0-9]+(?:\.[0-9]*)?){S}(?:\({S}{N}{S},{S}{N}{S},{S}{N}{S}\))?"
    ))
    .expect("constant pattern")
});
type Found = Result<Option<Frame>, Error>;
pub fn read(dataset: Option<&Object>, frame_index: i64) -> Result<Frame, Error> {
    let Some(dataset) = dataset else {
        return Err(Error::bare(Kind::NotFound));
    };
    let readers: [fn(&Object, i64) -> Found; 7] = [
        read_macro,
        read_standard,
        read_siemens_csa,
        read_siemens_mr_header,
        read_ge,
        read_philips,
        read_canon_comments,
    ];
    for reader in readers {
        if let Some(mut frame) = reader(dataset, frame_index)? {
            frame.frame_index = frame_index;
            return Ok(frame);
        }
    }
    Err(Error::bare(Kind::NotFound))
}
pub fn read_frames(dataset: Option<&Object>, frames: i64) -> Result<Vec<Frame>, Error> {
    let mut out = Vec::new();
    for index in 0..frames {
        match read(dataset, index) {
            Ok(frame) => out.push(frame),
            Err(e) if e.kind == Kind::NotFound => {}
            Err(e) => {
                return Err(Error {
                    kind: e.kind,
                    text: format!("frame {index}: {e}"),
                });
            }
        }
    }
    Ok(out)
}
fn read_macro(dataset: &Object, frame_index: i64) -> Found {
    let (item, source, _) = dataset.functional_group_item(frame_index, TAG_MR_DIFFUSION_SEQUENCE);
    let Some(item) = item else {
        return Ok(None);
    };
    if !item.has(TAG_DIFFUSION_B_VALUE) {
        return Ok(None);
    }
    let mut frame = standard_frame(&item, source::MR_DIFFUSION_MACRO)?;
    frame.functional_group = source;
    Ok(Some(frame))
}
fn read_standard(dataset: &Object, _: i64) -> Found {
    if !dataset.has(TAG_DIFFUSION_B_VALUE) {
        return Ok(None);
    }
    if numbers(dataset, TAG_DIFFUSION_B_VALUE, Vr::FD)
        .unwrap_or_default()
        .is_empty()
    {
        return Ok(None);
    }
    standard_frame(dataset, source::STANDARD_ATTRIBUTES).map(Some)
}
fn standard_frame(obj: &Object, source: &'static str) -> Result<Frame, Error> {
    let mut frame = Frame::new(source, TAG_DIFFUSION_B_VALUE, direction_frame::PATIENT);
    let values = numbers(obj, TAG_DIFFUSION_B_VALUE, Vr::FD)?;
    let Some(&b) = values.first() else {
        return Err(Error::invalid(format!("{TAG_DIFFUSION_B_VALUE} is empty")));
    };
    frame.b_value = check_b(b)?;
    frame.directionality = upper_string(obj, TAG_DIFFUSION_DIRECTIONALITY);
    let first = obj
        .get_sequence(TAG_DIFFUSION_GRADIENT_DIR_SEQ)
        .and_then(|items| items.into_iter().next());
    let direction = first.as_ref().unwrap_or(obj);
    let values = numbers(direction, TAG_DIFFUSION_GRADIENT_ORIENT, Vr::FD)?;
    if values.len() >= 3 {
        frame.set_direction(&values[..3])?;
    }
    Ok(frame)
}
fn blank(value: &[u8]) -> bool {
    value.iter().all(|b| matches!(b, b' ' | 0))
}
fn read_siemens_csa(dataset: &Object, _: i64) -> Found {
    let Ok(header) = siemens::image_header(dataset, LookupOptions::default()) else {
        return Ok(None);
    };
    let Some(element) = header.header.element("B_value") else {
        return Ok(None);
    };
    if element.strings().iter().all(|v| blank(v)) {
        return Ok(None);
    }
    let b = match element.float64s() {
        Ok(values) if !values.is_empty() => values[0],
        _ => return Err(Error::invalid("CSA B_value")),
    };
    let mut frame = Frame::new(source::SIEMENS_CSA, header.tag, direction_frame::PATIENT);
    frame.b_value = check_b(b)?;
    if let Some(element) = header.header.element("DiffusionDirectionality")
        && let Some(first) = element.strings().first()
    {
        frame.directionality = to_upper(trim_space(&String::from_utf8_lossy(first)));
    }
    if let Some(element) = header.header.element("DiffusionGradientDirection")
        && element.values.len() >= 3
    {
        let Ok(values) = element.float64s() else {
            return Err(Error::invalid("CSA DiffusionGradientDirection"));
        };
        frame.set_direction(&values[..3])?;
    }
    Ok(Some(frame))
}
fn read_siemens_mr_header(dataset: &Object, _: i64) -> Found {
    let Some(tag) = private_tag(dataset, 0x0019, SIEMENS_MR_HEADER_CREATOR, 0x0C) else {
        return Ok(None);
    };
    let values = numbers(dataset, tag, Vr::IS)?;
    let Some(&b) = values.first() else {
        return Ok(None);
    };
    let mut frame = Frame::new(source::SIEMENS_MR_HEADER, tag, direction_frame::PATIENT);
    frame.b_value = check_b(b)?;
    if let Some(tag) = private_tag(dataset, 0x0019, SIEMENS_MR_HEADER_CREATOR, 0x0D) {
        frame.directionality = upper_string(dataset, tag);
    }
    if let Some(tag) = private_tag(dataset, 0x0019, SIEMENS_MR_HEADER_CREATOR, 0x0E) {
        let values = numbers(dataset, tag, Vr::FD)?;
        if values.len() >= 3 {
            frame.set_direction(&values[..3])?;
        }
    }
    Ok(Some(frame))
}
const GE_FLAG_OFFSET: f64 = 1_000_000_000.0;
fn read_ge(dataset: &Object, _: i64) -> Found {
    let Some(tag) = private_tag(dataset, 0x0043, GE_PARM_CREATOR, 0x39) else {
        return Ok(None);
    };
    let values = numbers(dataset, tag, Vr::IS)?;
    let Some(&first) = values.first() else {
        return Ok(None);
    };
    let mut b = first;
    if b >= GE_FLAG_OFFSET {
        b %= GE_FLAG_OFFSET;
    }
    let mut frame = Frame::new(source::GE, tag, direction_frame::GE_GRADIENT);
    frame.b_value = check_b(b)?;
    if let Some(direction) =
        component_direction(dataset, 0x0019, GE_ACQU_CREATOR, [0xBB, 0xBC, 0xBD], Vr::DS)?
    {
        frame.set_direction(&direction)?;
    }
    Ok(Some(frame))
}
fn read_philips(dataset: &Object, _: i64) -> Found {
    let Some(tag) = private_tag(dataset, 0x2001, PHILIPS_IMAGING_CREATORS, 0x03) else {
        return Ok(None);
    };
    let values = numbers(dataset, tag, Vr::FL)?;
    let Some(&b) = values.first() else {
        return Ok(None);
    };
    let mut frame = Frame::new(source::PHILIPS, tag, direction_frame::PHILIPS_RL_AP_FH);
    frame.b_value = check_b(b)?;
    if let Some(direction) = component_direction(
        dataset,
        0x2005,
        PHILIPS_MR_IMAGING_CREATORS,
        [0xB0, 0xB1, 0xB2],
        Vr::FL,
    )? {
        frame.set_direction(&direction)?;
    }
    Ok(Some(frame))
}
fn read_canon_comments(dataset: &Object, _: i64) -> Found {
    let manufacturer = dataset.get_string(TAG_MANUFACTURER).unwrap_or_default();
    if manufacturer_of_name(&manufacturer) != Manufacturer::Canon {
        return Ok(None);
    }
    let Some(comments) = dataset.get_string(TAG_IMAGE_COMMENTS) else {
        return Ok(None);
    };
    let Some(captures) = CANON_IMAGE_COMMENT.captures(&comments) else {
        return Ok(None);
    };
    let group = |i: usize| captures.get(i).map_or("", |m| m.as_str());
    let Ok(b) = parse_float(group(1)) else {
        return Err(Error::invalid(format!(
            "Image Comments b-value {}",
            quote(group(1))
        )));
    };
    let mut frame = Frame::new(
        source::CANON_IMAGE_COMMENTS,
        TAG_IMAGE_COMMENTS,
        direction_frame::UNSPECIFIED,
    );
    frame.b_value = check_b(b)?;
    if !group(2).is_empty() {
        let mut direction = [0f64; 3];
        for (i, value) in direction.iter_mut().enumerate() {
            let text = group(2 + i);
            *value = parse_float(text)
                .map_err(|_| Error::invalid(format!("Image Comments direction {}", quote(text))))?;
        }
        frame.set_direction(&direction)?;
    }
    Ok(Some(frame))
}
fn check_b(b: f64) -> Result<f64, Error> {
    if b.is_nan() || b.is_infinite() || b < 0.0 {
        return Err(Error::invalid(format!("b-value {}", float_v(b))));
    }
    Ok(b)
}
fn component_direction(
    dataset: &Object,
    group: u16,
    creators: &[&str],
    offsets: [u8; 3],
    vr: Vr,
) -> Result<Option<[f64; 3]>, Error> {
    let mut direction = [0f64; 3];
    for (value, offset) in direction.iter_mut().zip(offsets) {
        let Some(tag) = private_tag(dataset, group, creators, offset) else {
            return Ok(None);
        };
        let values = numbers(dataset, tag, vr)?;
        let Some(&first) = values.first() else {
            return Ok(None);
        };
        *value = first;
    }
    Ok(Some(direction))
}
fn private_tag(dataset: &Object, group: u16, creators: &[&str], offset: u8) -> Option<Tag> {
    creators.iter().find_map(|creator| {
        reserved_tag(dataset, group, creator, offset)
            .ok()
            .filter(|tag| dataset.has(*tag))
    })
}
fn numbers(obj: &Object, tag: Tag, want: Vr) -> Result<Vec<f64>, Error> {
    let Some(element) = obj.get(tag) else {
        return Ok(Vec::new());
    };
    let actual = element.vr();
    let vr = match actual {
        Vr::FL | Vr::FD | Vr::DS | Vr::IS => actual,
        _ if actual.is_string_like() => Vr::DS,
        _ => want,
    };
    if (vr == Vr::FL || vr == Vr::FD) && actual == vr {
        let values = obj
            .get_floats(tag)
            .map_err(|e| Error::invalid(format!("{tag}: {e}")))?;
        return finite(tag, values);
    }
    if vr == Vr::FL || vr == Vr::FD {
        let Some(raw) = obj.get_raw(tag) else {
            return Err(Error::invalid(format!("{tag} has no byte value")));
        };
        let width = if vr == Vr::FD { 8 } else { 4 };
        if raw.len() % width != 0 {
            return Err(Error::invalid(format!(
                "{tag} has {} bytes for VR {vr}",
                raw.len()
            )));
        }
        let big = obj.value_big_endian();
        let values = raw
            .chunks_exact(width)
            .map(|c| {
                if width == 4 {
                    let bits = [c[0], c[1], c[2], c[3]];
                    f32::from_bits(if big {
                        u32::from_be_bytes(bits)
                    } else {
                        u32::from_le_bytes(bits)
                    }) as f64
                } else {
                    let bits = [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]];
                    f64::from_bits(if big {
                        u64::from_be_bytes(bits)
                    } else {
                        u64::from_le_bytes(bits)
                    })
                }
            })
            .collect();
        return finite(tag, values);
    }
    let mut values = Vec::new();
    if actual.is_string_like() {
        let texts = obj
            .lookup_strings(tag)
            .map_err(|e| Error::invalid(format!("{tag}: {e}")))?;
        for text in &texts {
            let text = text.trim_matches([' ', '\0']);
            if text.is_empty() {
                continue;
            }
            match parse_float(text) {
                Ok(value) => values.push(value),
                Err(_) => {
                    return Err(Error::invalid(format!("{tag} value {}", quote(text))));
                }
            }
        }
    } else {
        let Some(raw) = obj.get_raw(tag) else {
            return Err(Error::invalid(format!("{tag} has no byte value")));
        };
        for part in raw.split(|b| *b == b'\\') {
            let padding = |b: &u8| matches!(*b, b' ' | 0);
            let start = part.iter().position(|b| !padding(b)).unwrap_or(part.len());
            let end = part
                .iter()
                .rposition(|b| !padding(b))
                .map_or(start, |e| e + 1);
            let text = &part[start..end];
            if text.is_empty() {
                continue;
            }
            match std::str::from_utf8(text)
                .ok()
                .and_then(|t| parse_float(t).ok())
            {
                Some(value) => values.push(value),
                None => {
                    return Err(Error::invalid(format!(
                        "{tag} value {}",
                        crate::object::error::quote_bytes(text)
                    )));
                }
            }
        }
    }
    finite(tag, values)
}
fn finite(tag: Tag, values: Vec<f64>) -> Result<Vec<f64>, Error> {
    if values.iter().any(|v| v.is_nan() || v.is_infinite()) {
        return Err(Error::invalid(format!("{tag} is not finite")));
    }
    Ok(values)
}
fn upper_string(obj: &Object, tag: Tag) -> String {
    let value = obj.get_string(tag).unwrap_or_default();
    to_upper(value.trim_matches([' ', '\0']))
}
