use super::voi::clamp;
use super::{DisplayError, FrameFault, Lut, PixelFormat, StoredSampleReader};
use crate::textfmt;
use std::fmt;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaletteColorLut {
    pub red: Option<Lut>,
    pub green: Option<Lut>,
    pub blue: Option<Lut>,
}
impl PaletteColorLut {
    pub(super) fn tables(&self) -> Option<[&Lut; 3]> {
        Some([
            self.red.as_ref()?,
            self.green.as_ref()?,
            self.blue.as_ref()?,
        ])
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColorFrame<'a> {
    pub rows: i64,
    pub columns: i64,
    pub pixels: &'a [u8],
    pub photometric: String,
    pub samples_per_pixel: i64,
    pub planar_configuration: i64,
    pub format: PixelFormat,
    pub palette: Option<PaletteColorLut>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba64Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFault {
    UnsupportedPhotometric,
    UnsupportedLayout,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorError {
    Display(DisplayError),
    Frame {
        field: &'static str,
        fault: ColorFault,
    },
    MissingPalette,
    Photometric(String),
}
impl From<DisplayError> for ColorError {
    fn from(e: DisplayError) -> Self {
        ColorError::Display(e)
    }
}
impl fmt::Display for ColorFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ColorFault::UnsupportedPhotometric => PHOTOMETRIC,
            ColorFault::UnsupportedLayout => "dicom/display: unsupported color pixel layout",
        })
    }
}
impl std::error::Error for ColorFault {}
const PHOTOMETRIC: &str = "dicom/display: unsupported color photometric interpretation";
impl fmt::Display for ColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColorError::Display(e) => e.fmt(f),
            ColorError::Frame { field, fault } => write!(f, "{fault}: {field}"),
            ColorError::MissingPalette => {
                f.write_str("dicom/display: PALETTE COLOR frame is missing palette LUTs")
            }
            ColorError::Photometric(detail) => write!(f, "{PHOTOMETRIC}: {detail}"),
        }
    }
}
impl std::error::Error for ColorError {}
fn layout_error(field: &'static str) -> ColorError {
    ColorError::Frame {
        field,
        fault: ColorFault::UnsupportedLayout,
    }
}
fn too_short() -> ColorError {
    DisplayError::frame("Pixels", FrameFault::PixelDataTooShort).into()
}
pub(super) fn normalize_photometric(value: &str) -> String {
    textfmt::to_upper(value)
        .trim_matches(|c| c == ' ' || c == '\0')
        .to_owned()
}
pub fn render_color(frame: &ColorFrame<'_>) -> Result<RgbaImage, ColorError> {
    super::validate_dimensions(frame.rows, frame.columns)?;
    let photometric = normalize_photometric(&frame.photometric);
    if photometric == "ARGB" {
        let mut out = Vec::new();
        argb_samples(frame, |i, rgb| {
            if out.is_empty() {
                out = new_image(frame, 4);
            }
            set_rgba(&mut out, i, rgb.map(|v| (v >> 8) as u8));
        })?;
        return Ok(RgbaImage {
            width: frame.columns as usize,
            height: frame.rows as usize,
            pixels: out,
        });
    }
    if photometric == "PALETTE COLOR" {
        return render_palette(frame);
    }
    if let Some(out) = copy_rgb8(frame) {
        return Ok(out);
    }
    let plan = color_samples(frame)?;
    let scale = 255.0 / plan.max_value;
    let mut out = new_image(frame, 4);
    plan.for_each(frame, |i, rgb| {
        set_rgba(&mut out, i, rgb.map(|v| clamp_byte(v * scale)));
    });
    Ok(RgbaImage {
        width: frame.columns as usize,
        height: frame.rows as usize,
        pixels: out,
    })
}
pub fn render_color16(frame: &ColorFrame<'_>) -> Result<Rgba64Image, ColorError> {
    super::validate_dimensions(frame.rows, frame.columns)?;
    let photometric = normalize_photometric(&frame.photometric);
    let image = |pixels| Rgba64Image {
        width: frame.columns as usize,
        height: frame.rows as usize,
        pixels,
    };
    if photometric == "ARGB" {
        let mut out = Vec::new();
        argb_samples(frame, |i, rgb| {
            if out.is_empty() {
                out = new_image(frame, 8);
            }
            set_rgba64(&mut out, i, rgb);
        })?;
        return Ok(image(out));
    }
    if photometric == "PALETTE COLOR" {
        return render_palette16(frame).map(image);
    }
    let plan = color_samples(frame)?;
    let scale = 65535.0 / plan.max_value;
    let mut out = new_image(frame, 8);
    plan.for_each(frame, |i, rgb| {
        set_rgba64(
            &mut out,
            i,
            rgb.map(|v| clamp(v * scale, 0.0, 65535.0).round() as u16),
        );
    });
    Ok(image(out))
}
fn new_image(frame: &ColorFrame<'_>, bytes_per_pixel: usize) -> Vec<u8> {
    vec![0; frame.rows as usize * frame.columns as usize * bytes_per_pixel]
}
fn set_rgba(out: &mut [u8], i: usize, rgb: [u8; 3]) {
    out[4 * i..4 * i + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
}
fn set_rgba64(out: &mut [u8], i: usize, rgb: [u16; 3]) {
    let dst = &mut out[8 * i..8 * i + 8];
    for (c, v) in rgb.iter().enumerate() {
        dst[2 * c..2 * c + 2].copy_from_slice(&v.to_be_bytes());
    }
    dst[6] = 0xFF;
    dst[7] = 0xFF;
}
fn clamp_byte(v: f64) -> u8 {
    clamp(v, 0.0, 255.0).round() as u8
}
fn copy_rgb8(frame: &ColorFrame<'_>) -> Option<RgbaImage> {
    let format = &frame.format;
    if normalize_photometric(&frame.photometric) != "RGB"
        || frame.samples_per_pixel != 3
        || frame.planar_configuration != 0
        || format.bits_allocated != 8
        || format.signed
        || (format.bits_stored != 0 && format.bits_stored != 8)
    {
        return None;
    }
    let pixels = super::checked_layout(frame.rows, frame.columns, 3, 1, 4).ok()? as usize;
    if frame.pixels.len() < pixels * 3 {
        return None;
    }
    let mut out = vec![0u8; pixels * 4];
    for (dst, src) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(frame.pixels.as_chunks::<3>().0)
    {
        dst.copy_from_slice(&[src[0], src[1], src[2], 255]);
    }
    Some(RgbaImage {
        width: frame.columns as usize,
        height: frame.rows as usize,
        pixels: out,
    })
}
fn repaired(format: &PixelFormat) -> PixelFormat {
    let mut format = *format;
    if format.bits_stored <= 0 || format.bits_stored > format.bits_allocated {
        format.bits_stored = format.bits_allocated;
        format.high_bit = format.bits_allocated - 1;
    }
    format
}
fn max_stored(bits_stored: i64) -> f64 {
    (1u64 << bits_stored) as f64 - 1.0
}
fn argb_samples(
    frame: &ColorFrame<'_>,
    mut emit: impl FnMut(usize, [u16; 3]),
) -> Result<(), ColorError> {
    if frame.samples_per_pixel != 4 {
        return Err(layout_error("SamplesPerPixel"));
    }
    if frame.planar_configuration != 0 && frame.planar_configuration != 1 {
        return Err(layout_error("PlanarConfiguration"));
    }
    let format = repaired(&frame.format);
    if format.bits_allocated != 8 && format.bits_allocated != 16 {
        return Err(layout_error("BitsAllocated"));
    }
    if format.signed {
        return Err(layout_error("PixelRepresentation"));
    }
    let width = (format.bits_allocated / 8) as usize;
    let pixels = super::checked_layout(frame.rows, frame.columns, 4, width as i64, 8)? as usize;
    if frame.pixels.len() < pixels * 4 * width {
        return Err(too_short());
    }
    let reader = format.reader();
    let planar = frame.planar_configuration == 1;
    let sample = |i: usize, channel: usize| -> i64 {
        let index = if planar {
            channel * pixels + i
        } else {
            4 * i + channel
        };
        reader.value(&frame.pixels[index * width..])
    };
    let scale = 65535.0 / max_stored(format.bits_stored);
    for i in 0..pixels {
        let alpha = sample(i, 0);
        let mut rgb = [0u16; 3];
        if alpha > 0 {
            let tables = frame
                .palette
                .as_ref()
                .and_then(PaletteColorLut::tables)
                .ok_or(ColorError::MissingPalette)?;
            for (out, lut) in rgb.iter_mut().zip(tables) {
                *out = lut.entry16(lut.lookup(alpha));
            }
        } else {
            for (channel, out) in rgb.iter_mut().enumerate() {
                *out = clamp(sample(i, channel + 1) as f64 * scale, 0.0, 65535.0).round() as u16;
            }
        }
        emit(i, rgb);
    }
    Ok(())
}
#[derive(Clone, Copy)]
enum Conversion {
    Rgb,
    YbrFull,
    YbrPartial,
}
impl Conversion {
    fn apply(self, a: f64, b: f64, c: f64, half: f64) -> [f64; 3] {
        match self {
            Conversion::Rgb => [a, b, c],
            Conversion::YbrFull => {
                let (y, cb, cr) = (a, b - half, c - half);
                [
                    y + 1.402 * cr,
                    y - 0.344136 * cb - 0.714136 * cr,
                    y + 1.772 * cb,
                ]
            }
            Conversion::YbrPartial => {
                let scale = half / 128.0;
                let y = 1.164383 * (a - 16.0 * scale);
                let (cb, cr) = (b - half, c - half);
                [
                    y + 1.596027 * cr,
                    y - 0.391762 * cb - 0.812968 * cr,
                    y + 2.017232 * cb,
                ]
            }
        }
    }
}
struct ColorPlan {
    conversion: Conversion,
    subsampled: bool,
    pixels: usize,
    bytes_per_sample: usize,
    reader: StoredSampleReader,
    max_value: f64,
}
impl ColorPlan {
    fn for_each(&self, frame: &ColorFrame<'_>, mut emit: impl FnMut(usize, [f64; 3])) {
        let sample = |index: usize| {
            self.reader
                .value(&frame.pixels[index * self.bytes_per_sample..]) as f64
        };
        let half = (self.max_value + 1.0) / 2.0;
        if self.subsampled {
            for pair in 0..self.pixels / 2 {
                let (y0, y1, cb, cr) = (
                    sample(4 * pair),
                    sample(4 * pair + 1),
                    sample(4 * pair + 2),
                    sample(4 * pair + 3),
                );
                emit(2 * pair, self.conversion.apply(y0, cb, cr, half));
                emit(2 * pair + 1, self.conversion.apply(y1, cb, cr, half));
            }
            return;
        }
        let planar = frame.planar_configuration == 1;
        let n = self.pixels;
        for i in 0..n {
            let (a, b, c) = if planar {
                (sample(i), sample(n + i), sample(2 * n + i))
            } else {
                (sample(3 * i), sample(3 * i + 1), sample(3 * i + 2))
            };
            emit(i, self.conversion.apply(a, b, c, half));
        }
    }
}
fn color_samples(frame: &ColorFrame<'_>) -> Result<ColorPlan, ColorError> {
    let (conversion, mut subsampled) = match normalize_photometric(&frame.photometric).as_str() {
        "RGB" => (Conversion::Rgb, false),
        "YBR_FULL" => (Conversion::YbrFull, false),
        "YBR_FULL_422" => (Conversion::YbrFull, true),
        "YBR_PARTIAL_422" => (Conversion::YbrPartial, true),
        "YBR_PARTIAL_420" => (Conversion::YbrPartial, false),
        _ => {
            return Err(ColorError::Frame {
                field: "PhotometricInterpretation",
                fault: ColorFault::UnsupportedPhotometric,
            });
        }
    };
    if frame.samples_per_pixel != 3 {
        return Err(layout_error("SamplesPerPixel"));
    }
    let bits = frame.format.bits_allocated;
    if bits != 8 && bits != 16 {
        return Err(layout_error("BitsAllocated"));
    }
    if frame.format.signed {
        return Err(layout_error("PixelRepresentation"));
    }
    if frame.planar_configuration != 0 && frame.planar_configuration != 1 {
        return Err(layout_error("PlanarConfiguration"));
    }
    let bytes_per_sample = (bits / 8) as usize;
    let pixels =
        super::checked_layout(frame.rows, frame.columns, 3, bytes_per_sample as i64, 4)? as usize;
    let full_bytes = pixels * 3 * bytes_per_sample;
    if subsampled && frame.pixels.len() >= full_bytes {
        subsampled = false;
    }
    if subsampled && frame.columns % 2 != 0 {
        return Err(layout_error("Columns"));
    }
    if subsampled && frame.planar_configuration != 0 {
        return Err(layout_error("PlanarConfiguration"));
    }
    let source_bytes = if subsampled {
        pixels * 2 * bytes_per_sample
    } else {
        full_bytes
    };
    if frame.pixels.len() < source_bytes {
        return Err(too_short());
    }
    let format = repaired(&frame.format);
    Ok(ColorPlan {
        conversion,
        subsampled,
        pixels,
        bytes_per_sample,
        reader: format.reader(),
        max_value: max_stored(format.bits_stored),
    })
}
const VIDEO_SYNTAXES: &[&str] = &[
    "1.2.840.10008.1.2.4.100",
    "1.2.840.10008.1.2.4.100.1",
    "1.2.840.10008.1.2.4.101",
    "1.2.840.10008.1.2.4.101.1",
    "1.2.840.10008.1.2.4.102",
    "1.2.840.10008.1.2.4.102.1",
    "1.2.840.10008.1.2.4.103",
    "1.2.840.10008.1.2.4.103.1",
    "1.2.840.10008.1.2.4.104",
    "1.2.840.10008.1.2.4.104.1",
    "1.2.840.10008.1.2.4.105",
    "1.2.840.10008.1.2.4.105.1",
    "1.2.840.10008.1.2.4.106",
    "1.2.840.10008.1.2.4.106.1",
    "1.2.840.10008.1.2.4.107",
    "1.2.840.10008.1.2.4.108",
];
const JPEG2000_SYNTAXES: &[&str] = &[
    "1.2.840.10008.1.2.4.90",
    "1.2.840.10008.1.2.4.91",
    "1.2.840.10008.1.2.4.92",
    "1.2.840.10008.1.2.4.93",
    "1.2.840.10008.1.2.4.201",
    "1.2.840.10008.1.2.4.202",
    "1.2.840.10008.1.2.4.203",
];
fn syntax_in(uid: &str, list: &[&str]) -> bool {
    list.contains(&uid.trim_end_matches([' ', '\0']))
}
pub fn validate_color_photometric(
    photometric: &str,
    transfer_syntax_uid: &str,
) -> Result<(), ColorError> {
    let name = normalize_photometric(photometric);
    match name.as_str() {
        "YBR_PARTIAL_420" if !syntax_in(transfer_syntax_uid, VIDEO_SYNTAXES) => {
            Err(ColorError::Photometric(format!(
                "YBR_PARTIAL_420 is valid only for MPEG or HEVC pixel data, not {transfer_syntax_uid}"
            )))
        }
        "YBR_ICT" | "YBR_RCT" if !syntax_in(transfer_syntax_uid, JPEG2000_SYNTAXES) => {
            Err(ColorError::Photometric(format!(
                "{name} is valid only for JPEG 2000 pixel data, not {transfer_syntax_uid}"
            )))
        }
        _ => Ok(()),
    }
}
fn palette_layout<'f>(
    frame: &'f ColorFrame<'_>,
    output_bytes: i64,
) -> Result<([&'f Lut; 3], usize, usize), ColorError> {
    let tables = frame
        .palette
        .as_ref()
        .and_then(PaletteColorLut::tables)
        .ok_or(ColorError::MissingPalette)?;
    if frame.samples_per_pixel != 1 {
        return Err(layout_error("SamplesPerPixel"));
    }
    let bits = frame.format.bits_allocated;
    if bits != 8 && bits != 16 {
        return Err(
            DisplayError::frame("BitsAllocated", FrameFault::UnsupportedBitsAllocated).into(),
        );
    }
    let bytes_per_sample = (bits / 8) as usize;
    let pixels = super::checked_layout(
        frame.rows,
        frame.columns,
        1,
        bytes_per_sample as i64,
        output_bytes,
    )? as usize;
    if frame.pixels.len() < pixels * bytes_per_sample {
        return Err(too_short());
    }
    Ok((tables, bytes_per_sample, pixels))
}
fn render_palette(frame: &ColorFrame<'_>) -> Result<RgbaImage, ColorError> {
    let (tables, width, pixels) = palette_layout(frame, 4)?;
    let reader = frame.format.reader();
    let mut out = vec![0u8; pixels * 4];
    for (i, sample) in frame.pixels.chunks_exact(width).take(pixels).enumerate() {
        let stored = reader.value(sample);
        set_rgba(
            &mut out,
            i,
            tables.map(|lut| lut.entry_byte(lut.lookup(stored))),
        );
    }
    Ok(RgbaImage {
        width: frame.columns as usize,
        height: frame.rows as usize,
        pixels: out,
    })
}
fn render_palette16(frame: &ColorFrame<'_>) -> Result<Vec<u8>, ColorError> {
    let (tables, width, pixels) = palette_layout(frame, 8)?;
    let reader = frame.format.reader();
    let mut out = vec![0u8; pixels * 8];
    for (i, sample) in frame.pixels.chunks_exact(width).take(pixels).enumerate() {
        let stored = reader.value(sample);
        set_rgba64(
            &mut out,
            i,
            tables.map(|lut| lut.entry16(lut.lookup(stored))),
        );
    }
    Ok(out)
}
