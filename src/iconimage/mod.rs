#![forbid(unsafe_code)]
mod frames;
mod generate;
mod plan9;
mod render;
mod thumbnail;
use crate::dictionary::reservations;
use crate::display::{self, ColorFrame, GrayImage, LutOptions, PixelFormat, RgbaImage};
use crate::object::{Object, Tag, Value};
use crate::pixel::pixeldata::extract_metadata;
pub use frames::DecompressLimits;
pub use generate::{
    GenerateOptions, PixelPadding, WindowMode, default_generate_limits, generate,
    pixel_padding_from_object,
};
use std::fmt;
pub use thumbnail::{ThumbnailFrameOptions, fit_thumbnail, render_thumbnail_frame};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    IconImageSequence,
    GeIis,
    GeUltrasound,
}
impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::IconImageSequence => "icon-image-sequence",
            Source::GeIis => "geiis",
            Source::GeUltrasound => "gems-ultrasound",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NoIcon,
    InvalidOptions,
    UnsupportedSource,
    NoUsablePixels,
    PartialFile,
    PartialDecodeUnsupported,
    ResourceLimit,
    Other,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    text: String,
}
pub(crate) const ERR_NO_ICON: &str = "iconimage: no renderable embedded icon";
pub(crate) const ERR_INVALID_OPTIONS: &str = "iconimage: invalid generation options";
pub(crate) const ERR_UNSUPPORTED_SOURCE: &str = "iconimage: unsupported source representation";
pub(crate) const ERR_NO_USABLE_PIXELS: &str = "iconimage: no pixels outside padding";
pub(crate) const ERR_PARTIAL_FILE: &str = "dicom: file is the result of a partial read";
impl Error {
    pub(crate) fn new(kind: ErrorKind, text: impl Into<String>) -> Error {
        Error {
            kind,
            text: text.into(),
        }
    }
    pub(crate) fn other(text: impl fmt::Display) -> Error {
        Error::new(ErrorKind::Other, text.to_string())
    }
    pub(crate) fn bare(kind: ErrorKind) -> Error {
        let text = match kind {
            ErrorKind::NoIcon => ERR_NO_ICON,
            ErrorKind::InvalidOptions => ERR_INVALID_OPTIONS,
            ErrorKind::UnsupportedSource => ERR_UNSUPPORTED_SOURCE,
            ErrorKind::NoUsablePixels => ERR_NO_USABLE_PIXELS,
            ErrorKind::PartialFile => ERR_PARTIAL_FILE,
            ErrorKind::PartialDecodeUnsupported => frames::ERR_PARTIAL_DECODE_UNSUPPORTED,
            ErrorKind::ResourceLimit => frames::ERR_DECOMPRESS_RESOURCE_LIMIT,
            ErrorKind::Other => "",
        };
        Error::new(kind, text)
    }
    pub(crate) fn detail(kind: ErrorKind, detail: impl fmt::Display) -> Error {
        let base = Error::bare(kind).text;
        Error::new(kind, format!("{base}: {detail}"))
    }
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kind == kind
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    Gray(GrayImage),
    Rgba(RgbaImage),
    Nrgba(RgbaImage),
}
impl Image {
    pub fn size(&self) -> (usize, usize) {
        match self {
            Image::Gray(g) => (g.width, g.height),
            Image::Rgba(c) | Image::Nrgba(c) => (c.width, c.height),
        }
    }
    pub fn pixels(&self) -> &[u8] {
        match self {
            Image::Gray(g) => &g.pixels,
            Image::Rgba(c) | Image::Nrgba(c) => &c.pixels,
        }
    }
    pub(crate) fn rgba64(&self, x: usize, y: usize) -> (u32, u32, u32, u32) {
        match self {
            Image::Gray(g) => {
                let v = u32::from(g.pixels[y * g.width + x]) * 0x101;
                (v, v, v, 0xFFFF)
            }
            Image::Rgba(c) => {
                let p = &c.pixels[(y * c.width + x) * 4..];
                (
                    u32::from(p[0]) * 0x101,
                    u32::from(p[1]) * 0x101,
                    u32::from(p[2]) * 0x101,
                    u32::from(p[3]) * 0x101,
                )
            }
            Image::Nrgba(c) => {
                let p = &c.pixels[(y * c.width + x) * 4..];
                let a = u32::from(p[3]);
                let ch = |v: u8| u32::from(v) * 0x101 * a / 0xFF;
                (ch(p[0]), ch(p[1]), ch(p[2]), a * 0x101)
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    pub image: Image,
    pub source: Source,
}
const MAX_ICON_PIXELS: i64 = 1024 * 1024;
pub(crate) const TAG_ICON_IMAGE_SEQUENCE: Tag = Tag::new(0x0088, 0x0200);
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_REPRESENTATIVE_FRAME_NUMBER: Tag = Tag::new(0x0028, 0x6010);
const PRIVATE_ICONS: [(u16, &str, u8, Source); 2] = [
    (0x0009, "GEIIS", 0x10, Source::GeIis),
    (
        0x6003,
        "GEMS_Ultrasound_ImageGroup_001",
        0x10,
        Source::GeUltrasound,
    ),
];
pub fn read(dataset: &Object) -> Result<Icon, Error> {
    let mut last: Option<Error> = None;
    let mut try_tag = |tag: Tag, source: Source| -> Option<Icon> {
        let items = dataset.get_sequence(tag)?;
        for item in &items {
            match render_item(item) {
                Ok(image) => return Some(Icon { image, source }),
                Err(e) => last = Some(e),
            }
        }
        None
    };
    if let Some(icon) = try_tag(TAG_ICON_IMAGE_SEQUENCE, Source::IconImageSequence) {
        return Ok(icon);
    }
    for (group, creator, offset, source) in PRIVATE_ICONS {
        let Ok(tag) = reservations::private_tag(dataset, group, creator, offset) else {
            continue;
        };
        if let Some(icon) = try_tag(tag, source) {
            return Ok(icon);
        }
    }
    match last {
        Some(e) => Err(Error::detail(ErrorKind::NoIcon, e)),
        None => Err(Error::bare(ErrorKind::NoIcon)),
    }
}
pub fn render_item(item: &Object) -> Result<Image, Error> {
    let metadata = extract_metadata(item).map_err(Error::other)?;
    let (rows, columns) = (i64::from(metadata.rows), i64::from(metadata.columns));
    if rows <= 0 || columns <= 0 || rows * columns > MAX_ICON_PIXELS {
        return Err(Error::other(format!(
            "iconimage: icon of {columns}x{rows} pixels"
        )));
    }
    if metadata.bits_allocated != 8 && metadata.bits_allocated != 16 {
        return Err(Error::other(format!(
            "iconimage: icon with BitsAllocated {}",
            metadata.bits_allocated
        )));
    }
    let samples = i64::from(metadata.samples_per_pixel);
    if samples != 1 && samples != 3 {
        return Err(Error::other(format!(
            "iconimage: icon with {samples} samples per pixel"
        )));
    }
    let element = item
        .get(TAG_PIXEL_DATA)
        .ok_or_else(|| Error::other("iconimage: icon without Pixel Data"))?;
    let Value::Raw(raw) = &element.value else {
        return Err(Error::other("iconimage: icon Pixel Data is not native"));
    };
    let size = (rows * columns * samples * i64::from(metadata.bits_allocated / 8)) as usize;
    if raw.len() < size {
        return Err(Error::other(format!(
            "iconimage: icon Pixel Data holds {} bytes, its attributes describe {size}",
            raw.len()
        )));
    }
    let raw = &raw[..size];
    let big_endian = item.value_big_endian();
    let photometric = crate::textfmt::to_upper(metadata.photometric_interpretation.trim());
    if photometric == "PALETTE COLOR" {
        let palette = display::palette_from_object(item, LutOptions::default())
            .map_err(Error::other)?
            .ok_or_else(|| Error::other("iconimage: PALETTE COLOR icon without palette LUTs"))?;
        return display::render_color(&ColorFrame {
            rows,
            columns,
            pixels: raw,
            photometric: "PALETTE COLOR".into(),
            samples_per_pixel: 1,
            planar_configuration: 0,
            format: PixelFormat {
                bits_allocated: i64::from(metadata.bits_allocated),
                bits_stored: i64::from(metadata.bits_stored),
                high_bit: i64::from(metadata.high_bit),
                signed: false,
                big_endian,
            },
            palette: Some(palette),
        })
        .map(Image::Rgba)
        .map_err(Error::other);
    }
    let mut window = render::WindowLevel::default();
    if samples == 1 {
        let mut bits = i64::from(metadata.bits_stored);
        if bits <= 0 || bits > i64::from(metadata.bits_allocated) {
            bits = i64::from(metadata.bits_allocated);
        }
        let span = (1u64 << bits) as f64;
        window = render::WindowLevel {
            center: span / 2.0,
            width: span,
            ..Default::default()
        };
        if metadata.pixel_representation == 1 {
            window.center = 0.0;
        }
    }
    let frame = render::Frame {
        index: 0,
        metadata,
        big_endian,
        pixels: raw.to_vec(),
        rescale: crate::pixel::frame::Rescale {
            slope: 1.0,
            intercept: 0.0,
        },
        default_window: window.clone(),
    };
    render::render_frame(&frame, &window)
}
pub fn representative_frame_index(dataset: &Object, frames: i64) -> Option<i64> {
    let element = dataset.get(TAG_REPRESENTATIVE_FRAME_NUMBER)?;
    if element.vr() != crate::object::Vr::US {
        return None;
    }
    let values = dataset.get_ints(TAG_REPRESENTATIVE_FRAME_NUMBER).ok()?;
    if values.len() != 1 || values[0] < 1 || values[0] > frames {
        return None;
    }
    Some(values[0] - 1)
}
