use super::{Metadata, SampleFormat, sample_format};
use crate::display::{
    self, ColorError, ColorFault, ColorFrame, DisplayError, FrameFault, GrayImage, Lut,
    ModalityLut, PaletteColorLut, PixelFormat, RgbaImage, VoiFunction, VoiLut,
};
use crate::textfmt;
use std::fmt;
pub const ERR_FRAME_TYPE_NOT_PRESENT: &str = "dicom: requested frame type is not present";
const ERR_SAMPLES_PER_PIXEL: &str = "dicom: unsupported samples per pixel";
const ERR_BITS_ALLOCATED: &str = "dicom: unsupported BitsAllocated";
const ERR_PHOTOMETRIC: &str = "dicom: unsupported PhotometricInterpretation";
const ERR_PLANAR: &str = "dicom: unsupported PlanarConfiguration";
const ERR_INVALID_METADATA: &str = "dicom: invalid frame metadata";
const ERR_TOO_SHORT: &str = "dicom: pixel data too short for frame";
const DISPLAY_COLOR_LAYOUT: &str = "dicom/display: unsupported color pixel layout";
const DISPLAY_COLOR_PHOTOMETRIC: &str =
    "dicom/display: unsupported color photometric interpretation";
const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameErrorKind {
    TypeNotPresent,
    UnsupportedSamplesPerPixel,
    UnsupportedBitsAllocated,
    UnsupportedPhotometricInterpretation,
    UnsupportedPlanarConfiguration,
    InvalidFrameMetadata,
    PixelDataTooShort,
    Display,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError {
    pub kind: FrameErrorKind,
    text: String,
}
impl FrameError {
    fn new(kind: FrameErrorKind, detail: Option<String>) -> FrameError {
        let base = match kind {
            FrameErrorKind::TypeNotPresent => ERR_FRAME_TYPE_NOT_PRESENT,
            FrameErrorKind::UnsupportedSamplesPerPixel => ERR_SAMPLES_PER_PIXEL,
            FrameErrorKind::UnsupportedBitsAllocated => ERR_BITS_ALLOCATED,
            FrameErrorKind::UnsupportedPhotometricInterpretation => ERR_PHOTOMETRIC,
            FrameErrorKind::UnsupportedPlanarConfiguration => ERR_PLANAR,
            FrameErrorKind::InvalidFrameMetadata => ERR_INVALID_METADATA,
            FrameErrorKind::PixelDataTooShort => ERR_TOO_SHORT,
            FrameErrorKind::Display => "",
        };
        let text = match detail {
            Some(d) if base.is_empty() => d,
            Some(d) => format!("{base}: {d}"),
            None => base.to_owned(),
        };
        FrameError { kind, text }
    }
    fn display(err: impl fmt::Display) -> FrameError {
        FrameError::new(FrameErrorKind::Display, Some(err.to_string()))
    }
    pub(crate) fn type_not_present(detail: Option<&str>) -> FrameError {
        FrameError::new(FrameErrorKind::TypeNotPresent, detail.map(str::to_owned))
    }
}
impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for FrameError {}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Window {
    pub center: f64,
    pub width: f64,
    pub function: VoiFunction,
    pub lut: Option<Lut>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rescale {
    pub slope: f64,
    pub intercept: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct FrameOptions {
    pub big_endian: bool,
    pub window: Window,
    pub rescale: Rescale,
    pub palette: Option<PaletteColorLut>,
}
impl Default for FrameOptions {
    fn default() -> Self {
        FrameOptions {
            big_endian: false,
            window: Window::default(),
            rescale: Rescale {
                slope: 1.0,
                intercept: 0.0,
            },
            palette: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameImage {
    Gray(GrayImage),
    Rgba(RgbaImage),
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NativeFrame {
    pub index: i64,
    pub data: Vec<u8>,
    pub metadata: Metadata,
    pub big_endian: bool,
    pub window: Window,
    pub rescale: Rescale,
    pub palette: Option<PaletteColorLut>,
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EncapsulatedFrame {
    pub index: i64,
    pub data: Vec<u8>,
    pub metadata: Metadata,
    pub big_endian: bool,
    pub window: Window,
    pub rescale: Rescale,
    pub palette: Option<PaletteColorLut>,
}
impl NativeFrame {
    pub fn sample_format(&self) -> SampleFormat {
        sample_format(&self.metadata, self.big_endian)
    }
    pub fn image(&self) -> Result<FrameImage, FrameError> {
        let md = &self.metadata;
        let interleaved = !md.planar_configuration_present || md.planar_configuration == 0;
        if md.samples_per_pixel == 3 && interleaved {
            display::validate_color_photometric(
                &md.photometric_interpretation,
                EXPLICIT_VR_LITTLE_ENDIAN,
            )
            .map_err(|e| {
                FrameError::new(
                    FrameErrorKind::UnsupportedPhotometricInterpretation,
                    Some(e.to_string()),
                )
            })?;
        }
        render_image(
            &self.data,
            md,
            self.big_endian,
            self.rescale,
            &self.window,
            self.palette.as_ref(),
        )
    }
}
impl EncapsulatedFrame {
    pub fn sample_format(&self) -> SampleFormat {
        sample_format(&self.metadata, self.big_endian)
    }
    pub fn image(&self) -> Result<FrameImage, FrameError> {
        render_image(
            &self.data,
            &self.metadata,
            self.big_endian,
            self.rescale,
            &self.window,
            self.palette.as_ref(),
        )
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub encapsulated: bool,
    pub native_data: Option<NativeFrame>,
    pub encapsulated_data: Option<EncapsulatedFrame>,
}
impl Frame {
    pub fn is_encapsulated(&self) -> bool {
        self.encapsulated
    }
    pub fn native_frame(&self) -> Result<&NativeFrame, FrameError> {
        if self.encapsulated {
            return Err(FrameError::type_not_present(None));
        }
        self.native_data
            .as_ref()
            .ok_or_else(|| FrameError::type_not_present(Some("native frame data is nil")))
    }
    pub fn encapsulated_frame(&self) -> Result<&EncapsulatedFrame, FrameError> {
        if !self.encapsulated {
            return Err(FrameError::type_not_present(None));
        }
        self.encapsulated_data
            .as_ref()
            .ok_or_else(|| FrameError::type_not_present(Some("encapsulated frame data is nil")))
    }
    pub fn image(&self) -> Result<FrameImage, FrameError> {
        if self.encapsulated {
            self.encapsulated_frame()?.image()
        } else {
            self.native_frame()?.image()
        }
    }
    pub fn stored_sample_data(&self) -> Result<(&[u8], SampleFormat), FrameError> {
        match (
            self.encapsulated,
            &self.native_data,
            &self.encapsulated_data,
        ) {
            (true, _, Some(f)) => Ok((&f.data, f.sample_format())),
            (false, Some(f), _) => Ok((&f.data, f.sample_format())),
            _ => Err(FrameError::type_not_present(Some("frame data is nil"))),
        }
    }
}
pub fn new_native_frame(
    index: i64,
    data: Vec<u8>,
    metadata: Metadata,
    opts: FrameOptions,
) -> Frame {
    Frame {
        encapsulated: false,
        native_data: Some(NativeFrame {
            index,
            data,
            metadata,
            big_endian: opts.big_endian,
            window: opts.window,
            rescale: opts.rescale,
            palette: opts.palette,
        }),
        encapsulated_data: None,
    }
}
pub fn new_encapsulated_frame(
    index: i64,
    data: Vec<u8>,
    metadata: Metadata,
    opts: FrameOptions,
) -> Frame {
    Frame {
        encapsulated: true,
        native_data: None,
        encapsulated_data: Some(EncapsulatedFrame {
            index,
            data,
            metadata,
            big_endian: opts.big_endian,
            window: opts.window,
            rescale: opts.rescale,
            palette: opts.palette,
        }),
    }
}
pub fn from_decoded_frames(
    metadata: &Metadata,
    data: Vec<Vec<u8>>,
    encapsulated: bool,
    opts: &FrameOptions,
) -> Vec<Frame> {
    data.into_iter()
        .enumerate()
        .map(|(i, d)| {
            if encapsulated {
                new_encapsulated_frame(i as i64, d, metadata.clone(), opts.clone())
            } else {
                new_native_frame(i as i64, d, metadata.clone(), opts.clone())
            }
        })
        .collect()
}
#[doc(hidden)]
pub fn rescale_from(slope: Option<f64>, intercept: Option<f64>) -> Rescale {
    let mut out = Rescale {
        slope: 1.0,
        intercept: 0.0,
    };
    if let Some(s) = slope
        && s != 0.0
    {
        out.slope = s;
    }
    if let Some(i) = intercept {
        out.intercept = i;
    }
    out
}
fn normalized_photometric(value: &str) -> String {
    textfmt::to_upper(value)
        .trim_matches([' ', '\0'])
        .to_owned()
}
fn render_image(
    data: &[u8],
    md: &Metadata,
    big_endian: bool,
    rescale: Rescale,
    window: &Window,
    palette: Option<&PaletteColorLut>,
) -> Result<FrameImage, FrameError> {
    if md.rows == 0 || md.columns == 0 {
        return Err(FrameError::new(
            FrameErrorKind::InvalidFrameMetadata,
            Some(format!("Rows={} Columns={}", md.rows, md.columns)),
        ));
    }
    let photometric = normalized_photometric(&md.photometric_interpretation);
    match md.samples_per_pixel {
        1 => {
            return render_grayscale(data, md, &photometric, big_endian, rescale, window)
                .map(FrameImage::Gray);
        }
        3 => return render_color(data, md, big_endian).map(FrameImage::Rgba),
        4 if photometric == "ARGB" => {
            return render_color_with_palette(data, md, big_endian, palette).map(FrameImage::Rgba);
        }
        _ => {}
    }
    Err(FrameError::new(
        FrameErrorKind::UnsupportedSamplesPerPixel,
        Some(format!("SamplesPerPixel={}", md.samples_per_pixel)),
    ))
}
fn render_grayscale(
    data: &[u8],
    md: &Metadata,
    photometric: &str,
    big_endian: bool,
    rescale: Rescale,
    window: &Window,
) -> Result<GrayImage, FrameError> {
    if md.bits_allocated != 8 && md.bits_allocated != 16 {
        return Err(FrameError::new(
            FrameErrorKind::UnsupportedBitsAllocated,
            Some(format!("BitsAllocated={}", md.bits_allocated)),
        ));
    }
    if photometric != "MONOCHROME1" && photometric != "MONOCHROME2" {
        return Err(FrameError::new(
            FrameErrorKind::UnsupportedPhotometricInterpretation,
            Some(textfmt::quote(&md.photometric_interpretation)),
        ));
    }
    let expected = usize::from(md.rows)
        .saturating_mul(usize::from(md.columns))
        .saturating_mul(usize::from(md.bits_allocated / 8));
    if data.len() < expected {
        return Err(FrameError::new(
            FrameErrorKind::PixelDataTooShort,
            Some(format!("got {} bytes, want {expected}", data.len())),
        ));
    }
    let frame = display::Frame {
        rows: i64::from(md.rows),
        columns: i64::from(md.columns),
        pixels: data,
        format: pixel_format(md, big_endian),
        modality: ModalityLut {
            slope: rescale.slope,
            intercept: rescale.intercept,
            lut: None,
        },
        voi: VoiLut {
            center: window.center,
            width: window.width,
            function: window.function,
            lut: window.lut.clone(),
            explanation: String::new(),
        },
    };
    let mut out = display::render_gray(&frame).map_err(FrameError::display)?;
    if photometric == "MONOCHROME1" {
        let (width, height) = (out.width, out.height);
        let presentation = display::Presentation {
            invert: true,
            ..display::Presentation::default()
        };
        presentation.apply(&mut display::GrayView {
            pixels: &mut out.pixels,
            stride: width,
            width,
            height,
        });
    }
    Ok(out)
}
fn pixel_format(md: &Metadata, big_endian: bool) -> PixelFormat {
    PixelFormat {
        bits_allocated: i64::from(md.bits_allocated),
        bits_stored: i64::from(md.bits_stored),
        high_bit: i64::from(md.high_bit),
        signed: md.pixel_representation != 0,
        big_endian,
    }
}
pub fn render_color(data: &[u8], md: &Metadata, big_endian: bool) -> Result<RgbaImage, FrameError> {
    render_color_with_palette(data, md, big_endian, None)
}
fn render_color_with_palette(
    data: &[u8],
    md: &Metadata,
    big_endian: bool,
    palette: Option<&PaletteColorLut>,
) -> Result<RgbaImage, FrameError> {
    if md.bits_allocated != 8 && md.bits_allocated != 16 {
        return Err(FrameError::new(
            FrameErrorKind::UnsupportedBitsAllocated,
            Some(format!(
                "{DISPLAY_COLOR_LAYOUT}: BitsAllocated={} for color",
                md.bits_allocated
            )),
        ));
    }
    if md.pixel_representation != 0 {
        return Err(FrameError::new(
            FrameErrorKind::InvalidFrameMetadata,
            Some(format!(
                "PixelRepresentation={} for color",
                md.pixel_representation
            )),
        ));
    }
    let photometric = normalized_photometric(&md.photometric_interpretation);
    let planar = if md.planar_configuration_present {
        md.planar_configuration
    } else {
        0
    };
    if planar != 0 && planar != 1
        || planar == 1 && photometric != "RGB" && photometric != "YBR_FULL" && photometric != "ARGB"
    {
        return Err(FrameError::new(
            FrameErrorKind::UnsupportedPlanarConfiguration,
            Some(format!(
                "{DISPLAY_COLOR_LAYOUT}: PlanarConfiguration={planar} for {photometric}"
            )),
        ));
    }
    match photometric.as_str() {
        "RGB" | "YBR_FULL" | "YBR_FULL_422" | "YBR_PARTIAL_422" | "YBR_PARTIAL_420" | "ARGB" => {}
        "PALETTE COLOR" => {
            return Err(FrameError::new(
                FrameErrorKind::UnsupportedPhotometricInterpretation,
                Some(format!(
                    "{} requires palette LUT metadata",
                    textfmt::quote(&md.photometric_interpretation)
                )),
            ));
        }
        _ => {
            return Err(FrameError::new(
                FrameErrorKind::UnsupportedPhotometricInterpretation,
                Some(format!(
                    "{DISPLAY_COLOR_PHOTOMETRIC}: {} for SamplesPerPixel=3",
                    textfmt::quote(&md.photometric_interpretation)
                )),
            ));
        }
    }
    let frame = ColorFrame {
        rows: i64::from(md.rows),
        columns: i64::from(md.columns),
        pixels: data,
        photometric,
        samples_per_pixel: i64::from(md.samples_per_pixel),
        planar_configuration: i64::from(planar),
        format: pixel_format(md, big_endian),
        palette: palette.cloned(),
    };
    display::render_color(&frame).map_err(translate_color_render_error)
}
fn translate_color_render_error(err: ColorError) -> FrameError {
    use FrameErrorKind as K;
    let by_field = |field: &str| match field {
        "SamplesPerPixel" => Some(K::UnsupportedSamplesPerPixel),
        "BitsAllocated" => Some(K::UnsupportedBitsAllocated),
        "PlanarConfiguration" => Some(K::UnsupportedPlanarConfiguration),
        "PhotometricInterpretation" => Some(K::UnsupportedPhotometricInterpretation),
        "Pixels" => Some(K::PixelDataTooShort),
        _ => None,
    };
    let kind = match &err {
        ColorError::Frame { field, fault } => by_field(field).or(Some(match fault {
            ColorFault::UnsupportedPhotometric => K::UnsupportedPhotometricInterpretation,
            ColorFault::UnsupportedLayout => K::InvalidFrameMetadata,
        })),
        ColorError::Display(DisplayError::Frame { field, fault }) => {
            by_field(field).or(match fault {
                FrameFault::PixelDataTooShort => Some(K::PixelDataTooShort),
                FrameFault::Invalid | FrameFault::SizeOverflow => Some(K::InvalidFrameMetadata),
                _ => None,
            })
        }
        ColorError::Photometric(_) => Some(K::UnsupportedPhotometricInterpretation),
        _ => None,
    };
    match kind {
        Some(kind) => FrameError::new(kind, Some(err.to_string())),
        None => FrameError::display(err),
    }
}
