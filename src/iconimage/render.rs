use super::{Error, Image};
use crate::display::{StoredSampleReader, VoiFunction};
use crate::pixel::Metadata;
use crate::pixel::frame::{FrameImage, FrameOptions, Rescale, Window, new_native_frame};
const DEFAULT_WINDOW_CENTER: f64 = 40.0;
const DEFAULT_WINDOW_WIDTH: f64 = 400.0;
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct WindowLevel {
    pub center: f64,
    pub width: f64,
    pub function: VoiFunction,
}
#[derive(Debug, Clone)]
pub(super) struct Frame {
    pub index: i64,
    pub metadata: Metadata,
    pub big_endian: bool,
    pub pixels: Vec<u8>,
    pub rescale: Rescale,
    pub default_window: WindowLevel,
}
fn valid_window_width(width: f64) -> bool {
    width > 0.0 && width.is_finite()
}
fn normalize_window(window: &WindowLevel, fallback: &WindowLevel) -> WindowLevel {
    let mut window = window.clone();
    if !valid_window_width(window.width) {
        window = fallback.clone();
    }
    if !valid_window_width(window.width) {
        return WindowLevel {
            center: DEFAULT_WINDOW_CENTER,
            width: DEFAULT_WINDOW_WIDTH,
            function: VoiFunction::Linear,
        };
    }
    window
}
pub(super) fn render_frame(frame: &Frame, window: &WindowLevel) -> Result<Image, Error> {
    let window = normalize_window(window, &frame.default_window);
    let opts = FrameOptions {
        big_endian: frame.big_endian,
        window: Window {
            center: window.center,
            width: window.width,
            function: window.function,
            lut: None,
        },
        rescale: frame.rescale,
        palette: None,
    };
    let native = new_native_frame(
        frame.index,
        frame.pixels.clone(),
        frame.metadata.clone(),
        opts,
    );
    match native.image() {
        Ok(FrameImage::Gray(g)) => Ok(Image::Gray(g)),
        Ok(FrameImage::Rgba(c)) => Ok(Image::Rgba(c)),
        Err(e) => Err(Error::other(e)),
    }
}
pub(super) fn auto_window(frame: &Frame) -> Option<WindowLevel> {
    let m = &frame.metadata;
    if m.samples_per_pixel != 1 {
        return None;
    }
    let (columns, rows) = (usize::from(m.columns), usize::from(m.rows));
    let bytes = usize::from(m.bits_allocated / 8);
    if columns == 0 || rows == 0 || (bytes != 1 && bytes != 2) {
        return None;
    }
    let signed = m.pixel_representation != 0;
    let reader = StoredSampleReader::legacy(
        i64::from(m.bits_allocated),
        i64::from(m.bits_stored),
        i64::from(m.high_bit),
        signed,
        frame.big_endian,
    );
    let (mut lo, mut hi, mut total) = (i64::MAX, i64::MIN, 0i64);
    for i in 0..rows * columns {
        let offset = i * bytes;
        if offset + bytes > frame.pixels.len() {
            continue;
        }
        let stored = reader.value(&frame.pixels[offset..]);
        let base = if signed {
            -(1i64 << (8 * bytes - 1))
        } else {
            0
        };
        let index = stored - base;
        if index < 0 || index >= 1i64 << (8 * bytes) {
            continue;
        }
        total += 1;
        lo = lo.min(stored);
        hi = hi.max(stored);
    }
    if total == 0 || lo >= hi {
        return None;
    }
    Some(rescaled_window(lo, hi, frame.rescale))
}
fn rescaled_window(lo: i64, hi: i64, rescale: Rescale) -> WindowLevel {
    let mut slope = rescale.slope;
    let mut intercept = rescale.intercept;
    if slope == 0.0 || !slope.is_finite() {
        slope = 1.0;
    }
    if !intercept.is_finite() {
        intercept = 0.0;
    }
    let (mut low, mut high) = (lo as f64 * slope + intercept, hi as f64 * slope + intercept);
    if low > high {
        std::mem::swap(&mut low, &mut high);
    }
    let unit = slope.abs();
    WindowLevel {
        center: (low + high + unit) / 2.0,
        width: high - low + unit,
        function: VoiFunction::Linear,
    }
}
