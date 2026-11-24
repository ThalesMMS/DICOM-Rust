#![forbid(unsafe_code)]
use super::error::RenderError;
use super::window::{Rescale, WindowLevel, normalize_window};
use crate::display::{self, ModalityLut, PixelFormat, StoredSampleReader, VoiLut};
use crate::dynamic::FrameMetadata;
use crate::pixel::Metadata;
use crate::pixel::frame::{self as pframe, FrameImage, FrameOptions, Window};
pub use crate::pixel::samples::PixelPadding;
use std::sync::Arc;
pub(crate) type PixelLoader = Arc<dyn Fn() -> Result<Frame, RenderError> + Send + Sync>;
#[derive(Clone, Default)]
pub struct Frame {
    pub sop_instance_uid: String,
    pub frame_index: i64,
    pub metadata: Metadata,
    pub transfer_syntax_uid: String,
    pub transfer_syntax_name: String,
    pub big_endian: bool,
    pub pixel_bytes: Arc<Vec<u8>>,
    pub load_pixels: Option<PixelLoader>,
    pub default_window: WindowLevel,
    pub rescale: Rescale,
    pub preserve_modality_precision: bool,
    pub pixel_padding: Option<PixelPadding>,
    pub image_position: Vec<f64>,
    pub image_orientation: Vec<f64>,
    pub pixel_spacing: Vec<f64>,
    pub slice_thickness: f64,
    pub spacing_between_slices: f64,
    pub slice_location: f64,
    pub slice_location_ok: bool,
    pub instance_number: i64,
    pub sort: f64,
    pub temporal: FrameMetadata,
    pub echo_number: i64,
    pub has_echo_number: bool,
    pub acquisition_number: i64,
    pub has_acquisition_number: bool,
    pub decode_err: Option<RenderError>,
    pub encapsulated: bool,
    pub cache_identity: Option<Arc<FrameIdentity>>,
}
#[derive(Debug, Default)]
pub struct FrameIdentity {
    _marker: u8,
}
impl FrameIdentity {
    pub fn new() -> Arc<FrameIdentity> {
        Arc::new(FrameIdentity::default())
    }
}
impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("sop_instance_uid", &self.sop_instance_uid)
            .field("frame_index", &self.frame_index)
            .field("metadata", &self.metadata)
            .field("pixel_bytes", &self.pixel_bytes.len())
            .finish()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderImage {
    Nil,
    Gray {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    },
    Rgba {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    },
}
impl RenderImage {
    pub fn gray(width: usize, height: usize, pixels: Vec<u8>) -> RenderImage {
        RenderImage::Gray {
            width,
            height,
            pixels,
        }
    }
    pub fn size(&self) -> (usize, usize) {
        match self {
            RenderImage::Nil => (0, 0),
            RenderImage::Gray { width, height, .. } | RenderImage::Rgba { width, height, .. } => {
                (*width, *height)
            }
        }
    }
    pub fn pixels(&self) -> &[u8] {
        match self {
            RenderImage::Nil => &[],
            RenderImage::Gray { pixels, .. } | RenderImage::Rgba { pixels, .. } => pixels,
        }
    }
}
pub fn blank_image(width: i64, height: i64) -> RenderImage {
    let w = if width <= 0 { 512 } else { width } as usize;
    let h = if height <= 0 { 512 } else { height } as usize;
    RenderImage::gray(w, h, vec![24; w * h])
}
pub fn load_frame(frame: Option<&Frame>) -> Result<Frame, RenderError> {
    let Some(frame) = frame else {
        return Err(RenderError::other("render: nil frame"));
    };
    let Some(loader) = &frame.load_pixels else {
        return match &frame.decode_err {
            Some(e) => Err(e.clone()),
            None => Ok(frame.clone()),
        };
    };
    let loaded = loader()?;
    if loaded.load_pixels.is_some() {
        return Err(RenderError::other("render: invalid deferred pixel frame"));
    }
    if loaded.metadata != frame.metadata || loaded.rescale != frame.rescale {
        return Err(RenderError::other(
            "render: deferred pixels changed planned frame metadata",
        ));
    }
    match &loaded.decode_err {
        Some(e) => Err(e.clone()),
        None => Ok(loaded),
    }
}
pub(crate) fn normalized_photometric(value: &str) -> String {
    crate::textfmt::to_upper(value)
        .trim_matches(|c| c == ' ' || c == '\0')
        .to_string()
}
fn trim_uid(uid: &str) -> &str {
    uid.trim_end_matches(['\0', ' '])
}
const RLE_LOSSLESS: &str = "1.2.840.10008.1.2.5";
const JPEG_BASELINE: &str = "1.2.840.10008.1.2.4.50";
const JPEG_EXTENDED: &str = "1.2.840.10008.1.2.4.51";
const JPEG2000_UIDS: [&str; 7] = [
    "1.2.840.10008.1.2.4.90",
    "1.2.840.10008.1.2.4.91",
    "1.2.840.10008.1.2.4.92",
    "1.2.840.10008.1.2.4.93",
    "1.2.840.10008.1.2.4.201",
    "1.2.840.10008.1.2.4.202",
    "1.2.840.10008.1.2.4.203",
];
const JPEGXL_UIDS: [&str; 3] = [
    "1.2.840.10008.1.2.4.110",
    "1.2.840.10008.1.2.4.111",
    "1.2.840.10008.1.2.4.112",
];
pub(crate) fn effective_frame_metadata(slice: &Frame) -> Metadata {
    let mut metadata = slice.metadata.clone();
    if !slice.encapsulated || metadata.samples_per_pixel != 3 {
        return metadata;
    }
    let uid = trim_uid(&slice.transfer_syntax_uid);
    let photometric = normalized_photometric(&metadata.photometric_interpretation);
    let to_rgb = match uid {
        RLE_LOSSLESS => {
            metadata.planar_configuration = 0;
            metadata.planar_configuration_present = true;
            false
        }
        JPEG_BASELINE | JPEG_EXTENDED => matches!(
            photometric.as_str(),
            "YBR_FULL" | "YBR_FULL_422" | "YBR_PARTIAL_422" | "YBR_PARTIAL_420"
        ),
        _ if JPEG2000_UIDS.contains(&uid) || JPEGXL_UIDS.contains(&uid) => matches!(
            photometric.as_str(),
            "YBR_FULL" | "YBR_FULL_422" | "YBR_RCT" | "YBR_ICT"
        ),
        _ => false,
    };
    if to_rgb {
        metadata.photometric_interpretation = "RGB".into();
        metadata.planar_configuration = 0;
        metadata.planar_configuration_present = true;
    }
    metadata
}
pub(crate) fn sanitized_rescale(rescale: Rescale) -> (f64, f64) {
    let mut slope = rescale.slope;
    if slope == 0.0 || !slope.is_finite() {
        slope = 1.0;
    }
    let intercept = if rescale.intercept.is_finite() {
        rescale.intercept
    } else {
        0.0
    };
    (slope, intercept)
}
fn frame_error(kind: &str, detail: String) -> RenderError {
    RenderError::other(format!("dicom: {kind}: {detail}"))
}
pub(crate) fn grayscale_display_frame<'a>(
    slice: &'a Frame,
    metadata: &Metadata,
    photometric: &str,
    rows: i64,
    cols: i64,
) -> Result<display::Frame<'a>, RenderError> {
    let ba = metadata.bits_allocated;
    if ba != 8 && ba != 16 && ba != 32 {
        return Err(frame_error(
            "unsupported BitsAllocated",
            format!("BitsAllocated={ba}"),
        ));
    }
    if photometric != "MONOCHROME1" && photometric != "MONOCHROME2" {
        return Err(frame_error(
            "unsupported PhotometricInterpretation",
            crate::textfmt::quote(&metadata.photometric_interpretation),
        ));
    }
    let bytes_per_sample = i64::from(ba / 8);
    let expected = rows * cols * bytes_per_sample;
    if (slice.pixel_bytes.len() as i64) < expected {
        return Err(frame_error(
            "pixel data too short for frame",
            format!("got {} bytes, want {expected}", slice.pixel_bytes.len()),
        ));
    }
    let (slope, intercept) = sanitized_rescale(slice.rescale);
    Ok(display::Frame {
        rows,
        columns: cols,
        pixels: &slice.pixel_bytes,
        format: PixelFormat {
            bits_allocated: i64::from(ba),
            bits_stored: i64::from(metadata.bits_stored),
            high_bit: i64::from(metadata.high_bit),
            signed: metadata.pixel_representation != 0,
            big_endian: slice.big_endian,
        },
        modality: ModalityLut {
            slope,
            intercept,
            ..Default::default()
        },
        voi: VoiLut::default(),
    })
}
pub(crate) fn decode_grayscale_f32(slice: &Frame) -> Result<Vec<f32>, RenderError> {
    let metadata = effective_frame_metadata(slice);
    let rows = i64::from(metadata.rows);
    let cols = i64::from(metadata.columns);
    if rows <= 0 || cols <= 0 {
        return Err(frame_error(
            "invalid frame metadata",
            format!("Rows={} Columns={}", metadata.rows, metadata.columns),
        ));
    }
    let photometric = normalized_photometric(&metadata.photometric_interpretation);
    let frame = grayscale_display_frame(slice, &metadata, &photometric, rows, cols)?;
    display::decode_modality_f32(&frame).map_err(|e| RenderError::other(e.to_string()))
}
pub(crate) fn decode_color_rgba(slice: &Frame) -> Result<Vec<u8>, RenderError> {
    let metadata = effective_frame_metadata(slice);
    if metadata.rows == 0 || metadata.columns == 0 {
        return Err(frame_error(
            "invalid frame metadata",
            format!("Rows={} Columns={}", metadata.rows, metadata.columns),
        ));
    }
    if metadata.samples_per_pixel != 3 {
        return Err(frame_error(
            "unsupported samples per pixel",
            format!("SamplesPerPixel={}", metadata.samples_per_pixel),
        ));
    }
    if !slice.encapsulated {
        display::validate_color_photometric(
            &metadata.photometric_interpretation,
            &slice.transfer_syntax_uid,
        )
        .map_err(|e| RenderError::other(e.to_string()))?;
    }
    pframe::render_color(&slice.pixel_bytes, &metadata, slice.big_endian)
        .map(|img| img.pixels)
        .map_err(|e| RenderError::other(e.to_string()))
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PixelValue {
    pub x: i64,
    pub y: i64,
    pub stored: i32,
    pub rescaled: f64,
}
pub fn pixel_value_at(slice: Option<&Frame>, x: i64, y: i64) -> Option<PixelValue> {
    let slice = load_frame(slice).ok()?;
    sample_pixel_value(
        &slice.metadata,
        slice.big_endian,
        &slice.pixel_bytes,
        slice.rescale,
        x,
        y,
    )
}
pub fn sample_pixel_value(
    metadata: &Metadata,
    big_endian: bool,
    pixel_bytes: &[u8],
    rescale: Rescale,
    x: i64,
    y: i64,
) -> Option<PixelValue> {
    if x < 0 || y < 0 || metadata.samples_per_pixel != 1 {
        return None;
    }
    let cols = i64::from(metadata.columns);
    let rows = i64::from(metadata.rows);
    if cols <= 0 || rows <= 0 || x >= cols || y >= rows {
        return None;
    }
    let bytes_per_sample = i64::from(metadata.bits_allocated / 8);
    if bytes_per_sample != 1 && bytes_per_sample != 2 {
        return None;
    }
    let offset = (y * cols + x) * bytes_per_sample;
    if offset < 0 || offset + bytes_per_sample > pixel_bytes.len() as i64 {
        return None;
    }
    let stored = stored_pixel_value(&pixel_bytes[offset as usize..], metadata, big_endian);
    let (slope, intercept) = sanitized_rescale(rescale);
    Some(PixelValue {
        x,
        y,
        stored,
        rescaled: f64::from(stored) * slope + intercept,
    })
}
fn stored_pixel_value(data: &[u8], metadata: &Metadata, big_endian: bool) -> i32 {
    let ba = metadata.bits_allocated;
    if ba != 8 && ba != 16 {
        return 0;
    }
    let reader = StoredSampleReader::legacy(
        i64::from(ba),
        i64::from(metadata.bits_stored),
        i64::from(metadata.high_bit),
        metadata.pixel_representation != 0,
        big_endian,
    );
    reader.value(data) as i32
}
pub fn render_frame(
    slice: Option<&Frame>,
    window: &WindowLevel,
) -> (RenderImage, Option<RenderError>) {
    let slice = match load_frame(slice) {
        Ok(s) => s,
        Err(e) => return (blank_image(512, 512), Some(e)),
    };
    let metadata = effective_frame_metadata(&slice);
    let window = normalize_window(window, &slice.default_window);
    let opts = FrameOptions {
        big_endian: slice.big_endian,
        window: Window {
            center: window.center,
            width: window.width,
            function: window.function,
            lut: window.lut.clone(),
        },
        rescale: pframe::Rescale {
            slope: slice.rescale.slope,
            intercept: slice.rescale.intercept,
        },
        palette: None,
    };
    let data = slice.pixel_bytes.as_ref().clone();
    let frame = if slice.encapsulated {
        pframe::new_encapsulated_frame(slice.frame_index, data, metadata, opts)
    } else {
        pframe::new_native_frame(slice.frame_index, data, metadata, opts)
    };
    match frame.image() {
        Ok(FrameImage::Gray(g)) => (RenderImage::gray(g.width, g.height, g.pixels), None),
        Ok(FrameImage::Rgba(r)) => (
            RenderImage::Rgba {
                width: r.width,
                height: r.height,
                pixels: r.pixels,
            },
            None,
        ),
        Err(e) => {
            let md = &slice.metadata;
            let untyped =
                md.rows == 0 || md.columns == 0 || !matches!(md.samples_per_pixel, 1 | 3 | 4);
            let image = if untyped {
                blank_image(i64::from(md.columns), i64::from(md.rows))
            } else {
                RenderImage::Nil
            };
            (image, Some(RenderError::other(e.to_string())))
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AutoWindowStrategy {
    #[default]
    MinMax,
    MinMaxIgnoreExtremes,
    Histogram,
}
impl AutoWindowStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            AutoWindowStrategy::MinMaxIgnoreExtremes => "min-max-ignore-extremes",
            AutoWindowStrategy::Histogram => "histogram",
            AutoWindowStrategy::MinMax => "min-max",
        }
    }
}
#[derive(Clone, Default)]
pub struct AutoWindowOptions {
    pub strategy: AutoWindowStrategy,
    pub histogram_ignore: f64,
    pub region: [i64; 4],
    pub include: Option<Arc<dyn Fn(i64, i64) -> bool + Send + Sync>>,
}
struct StoredHistogram {
    base: i64,
    counts: Vec<i64>,
    lo: i64,
    hi: i64,
    total: i64,
}
impl StoredHistogram {
    fn count(&self, value: i64) -> i64 {
        let index = value.wrapping_sub(self.base);
        if index < 0 || index >= self.counts.len() as i64 {
            return 0;
        }
        self.counts[index as usize]
    }
}
fn stored_histogram(slice: &Frame, opts: &AutoWindowOptions) -> Option<StoredHistogram> {
    let md = &slice.metadata;
    if md.samples_per_pixel != 1 {
        return None;
    }
    let (columns, rows) = (i64::from(md.columns), i64::from(md.rows));
    let bytes_per_sample = i64::from(md.bits_allocated / 8);
    if columns <= 0 || rows <= 0 || (bytes_per_sample != 1 && bytes_per_sample != 2) {
        return None;
    }
    let signed = md.pixel_representation != 0;
    let reader = StoredSampleReader::legacy(
        i64::from(md.bits_allocated),
        i64::from(md.bits_stored),
        i64::from(md.high_bit),
        signed,
        slice.big_endian,
    );
    let base = if signed {
        -1i64 << (8 * bytes_per_sample - 1)
    } else {
        0
    };
    let mut out = StoredHistogram {
        base,
        counts: vec![0; 1 << (8 * bytes_per_sample)],
        lo: i64::MAX,
        hi: i64::MIN,
        total: 0,
    };
    let (mut x0, mut y0, mut x1, mut y1) = (0, 0, columns, rows);
    let r = opts.region;
    if r[0] < r[2] && r[1] < r[3] {
        x0 = x0.max(r[0]);
        y0 = y0.max(r[1]);
        x1 = x1.min(r[2]);
        y1 = y1.min(r[3]);
        if x0 >= x1 || y0 >= y1 {
            (x0, y0, x1, y1) = (0, 0, 0, 0);
        }
    }
    let pixels = &slice.pixel_bytes;
    for y in y0..y1 {
        for x in x0..x1 {
            let offset = (y * columns + x) * bytes_per_sample;
            if offset + bytes_per_sample > pixels.len() as i64 {
                continue;
            }
            if let Some(include) = &opts.include
                && !include(x, y)
            {
                continue;
            }
            let stored = reader.value(&pixels[offset as usize..]);
            if slice.pixel_padding.is_some_and(|p| p.contains(stored)) {
                continue;
            }
            let index = stored - out.base;
            if index < 0 || index >= out.counts.len() as i64 {
                continue;
            }
            out.counts[index as usize] += 1;
            out.total += 1;
            out.lo = out.lo.min(stored);
            out.hi = out.hi.max(stored);
        }
    }
    (out.total > 0).then_some(out)
}
pub fn auto_window(slice: Option<&Frame>) -> Option<WindowLevel> {
    auto_window_with(slice, &AutoWindowOptions::default())
}
pub fn auto_window_with(slice: Option<&Frame>, opts: &AutoWindowOptions) -> Option<WindowLevel> {
    let slice = load_frame(slice).ok()?;
    let h = stored_histogram(&slice, opts)?;
    let (lo, hi) = match opts.strategy {
        AutoWindowStrategy::MinMaxIgnoreExtremes => {
            let (mut first, mut last) = (h.lo + 1, h.hi - 1);
            while first <= last && h.count(first) == 0 {
                first += 1;
            }
            while last >= first && h.count(last) == 0 {
                last -= 1;
            }
            if first > last {
                return None;
            }
            (first, last)
        }
        AutoWindowStrategy::Histogram => {
            let mut ignore = opts.histogram_ignore;
            if ignore.is_nan() || ignore < 0.0 {
                ignore = 0.0;
            }
            if ignore > 0.5 {
                ignore = 0.5;
            }
            let threshold = crate::fmath::f64_to_i64(ignore * h.total as f64);
            let mut lo = h.lo;
            if threshold > 0 {
                let mut sum = 0;
                let mut value = h.lo;
                while value <= h.hi {
                    sum += h.count(value);
                    if sum >= threshold {
                        lo = value + 1;
                        break;
                    }
                    value += 1;
                }
            }
            let mut hi = h.hi + 1;
            if threshold > 0 {
                let mut sum = 0;
                let mut value = h.hi;
                while value >= h.lo {
                    sum += h.count(value);
                    if sum >= threshold {
                        hi = value;
                        break;
                    }
                    value -= 1;
                }
            }
            (lo, hi)
        }
        AutoWindowStrategy::MinMax => (h.lo, h.hi),
    };
    if lo >= hi {
        return None;
    }
    Some(rescaled_window(lo, hi, slice.rescale))
}
fn rescaled_window(lo: i64, hi: i64, rescale: Rescale) -> WindowLevel {
    let (slope, intercept) = sanitized_rescale(rescale);
    let (mut low, mut high) = (lo as f64 * slope + intercept, hi as f64 * slope + intercept);
    if low > high {
        std::mem::swap(&mut low, &mut high);
    }
    let unit = slope.abs();
    WindowLevel::new((low + high + unit) / 2.0, high - low + unit)
}
pub fn pixel_value_range(slice: Option<&Frame>) -> Option<(f64, f64)> {
    let slice = load_frame(slice).ok()?;
    let h = stored_histogram(&slice, &AutoWindowOptions::default())?;
    let (slope, intercept) = sanitized_rescale(slice.rescale);
    let (mut lo, mut hi) = (
        h.lo as f64 * slope + intercept,
        h.hi as f64 * slope + intercept,
    );
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    Some((lo, hi))
}
pub(crate) fn spatial_sort_value(frame: &Frame) -> f64 {
    if frame.slice_location_ok {
        return frame.slice_location;
    }
    if frame.image_position.len() >= 3 {
        return frame.image_position[2];
    }
    if frame.sort != 0.0 {
        return frame.sort;
    }
    frame.instance_number as f64
}
pub(crate) fn display_gray_mapped(
    value: f64,
    mapper: &super::window::PreparedVoi,
    photometric: &str,
) -> u8 {
    let gray = mapper.gray(value);
    if normalized_photometric(photometric) == "MONOCHROME1" {
        255 - gray
    } else {
        gray
    }
}
