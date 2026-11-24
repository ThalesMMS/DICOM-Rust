#![forbid(unsafe_code)]
mod cielab;
mod color;
mod colormap;
mod curve;
mod error;
mod gsdf;
mod lut;
mod object;
mod overlay_object;
mod presentation;
mod sample;
mod tiff;
mod transform;
mod voi;
pub use cielab::{cielab_lightness, cielab_relative_luminance};
pub use color::{
    ColorError, ColorFault, ColorFrame, PaletteColorLut, Rgba64Image, RgbaImage, render_color,
    render_color16, validate_color_photometric,
};
pub use colormap::{cool_warm_lut, hot_iron_lut, hot_metal_blue_lut};
pub use curve::{Curve, CurvePoint, curves_from_object};
pub use error::DisplayError;
pub use gsdf::{
    CalibrationError, CharacteristicCurve, DEFAULT_ILLUMINATION, DEFAULT_PRINT_DENSITY,
    DEFAULT_REFLECTED_AMBIENT_LIGHT, DeviceType, DisplayCalibration, DisplayFunction, DisplayLut,
    GSDF_MAX_JND, GSDF_MIN_JND, PrintDensity, gsdf_jnd_index, gsdf_luminance,
};
pub use lut::{Lut, LutAdjustment, LutOptions, encode_lut_data};
pub use object::{
    ColorMetadata, ObjectError, ObjectErrorKind, color_metadata_from_object, lut_from_item,
    modality_lut_at, modality_lut_from_object, palette_from_object, print_density_from_object,
    voi_at, voi_from_object, vois_at,
};
pub use overlay_object::{
    embedded_overlays, embedded_overlays_for_frame, overlays_from_object, presentation_from_object,
    shutter_from_object, validate_shutter,
};
pub use presentation::{
    BitmapShutter, CircleShutter, Gray16Image, Gray16View, GrayView, Overlay, PolygonShutter,
    Presentation, RectShutter, Shutter, ShutterPoint, render_gray16,
};
pub use sample::StoredSampleReader;
pub use tiff::encode_tiff;
pub use transform::{
    Image, ImageTransform, ImageTransformError, MAX_EXPORT_PIXELS, transform_image,
};
pub use voi::{
    ModalityLut, VoiByteMapper, VoiFunction, VoiLut, WindowDomain, manual_window_minimum,
    valid_window_width,
};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PixelFormat {
    pub bits_allocated: i64,
    pub bits_stored: i64,
    pub high_bit: i64,
    pub signed: bool,
    pub big_endian: bool,
}
impl PixelFormat {
    fn reader(&self) -> StoredSampleReader {
        StoredSampleReader::legacy(
            self.bits_allocated,
            self.bits_stored,
            self.high_bit,
            self.signed,
            self.big_endian,
        )
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame<'a> {
    pub rows: i64,
    pub columns: i64,
    pub pixels: &'a [u8],
    pub format: PixelFormat,
    pub modality: ModalityLut,
    pub voi: VoiLut,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrayImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
pub const MAX_MODALITY_LEVELS: i64 = 1 << 16;
#[derive(Debug, Clone, PartialEq)]
pub struct ModalityLevels {
    pub index: Vec<u16>,
    pub levels: Vec<f32>,
}
pub fn render_gray(frame: &Frame<'_>) -> Result<GrayImage, DisplayError> {
    validate_dimensions(frame.rows, frame.columns)?;
    let bits_allocated = frame.format.bits_allocated;
    if bits_allocated != 8 && bits_allocated != 16 {
        return Err(DisplayError::frame(
            "BitsAllocated",
            FrameFault::UnsupportedBitsAllocated,
        ));
    }
    let bytes_per_sample = (bits_allocated / 8) as usize;
    let pixels = checked_layout(frame.rows, frame.columns, 1, bytes_per_sample as i64, 1)?;
    if (frame.pixels.len() as i64) < pixels * bytes_per_sample as i64 {
        return Err(DisplayError::frame("Pixels", FrameFault::PixelDataTooShort));
    }
    let pixels = pixels as usize;
    let reader = frame.format.reader();
    let mapper = frame_voi_mapper(&frame.voi, &frame.modality, &reader);
    let slope = frame.modality.effective_slope();
    let intercept = frame.modality.intercept;
    let display_byte = |stored: i64| -> u8 {
        match &frame.modality.lut {
            Some(lut) => mapper.byte(f64::from(lut.lookup(stored))),
            None => mapper.byte(stored as f64 * slope + intercept),
        }
    };
    let mut bits_stored = frame.format.bits_stored;
    if bits_stored <= 0 || bits_stored > bits_allocated {
        bits_stored = bits_allocated;
    }
    let values = 1usize << bits_stored;
    let mut out = vec![0u8; pixels];
    let samples = frame.pixels.chunks_exact(bytes_per_sample).take(pixels);
    if values <= pixels {
        let lowest = if frame.format.signed {
            -((values / 2) as i64)
        } else {
            0
        };
        let table: Vec<u8> = (0..values as i64)
            .map(|i| display_byte(lowest + i))
            .collect();
        for (o, sample) in out.iter_mut().zip(samples) {
            *o = table[(reader.value(sample) - lowest) as usize];
        }
    } else {
        for (o, sample) in out.iter_mut().zip(samples) {
            *o = display_byte(reader.value(sample));
        }
    }
    Ok(GrayImage {
        width: frame.columns as usize,
        height: frame.rows as usize,
        pixels: out,
    })
}
pub fn decode_modality(frame: &Frame<'_>) -> Result<Vec<f64>, DisplayError> {
    let count = modality_sample_count(frame, 8)?;
    Ok(modality_values(frame, count, |v| v))
}
#[doc(hidden)]
pub fn decode_modality_f32(frame: &Frame<'_>) -> Result<Vec<f32>, DisplayError> {
    let count = modality_sample_count(frame, 4)?;
    Ok(modality_values(frame, count, |v| v as f32))
}
fn modality_values<T>(frame: &Frame<'_>, count: usize, convert: impl Fn(f64) -> T) -> Vec<T> {
    let width = (frame.format.bits_allocated / 8) as usize;
    let reader = frame.format.reader();
    let slope = frame.modality.effective_slope();
    let intercept = frame.modality.intercept;
    frame
        .pixels
        .chunks_exact(width)
        .take(count)
        .map(|sample| {
            let stored = reader.value(sample);
            match &frame.modality.lut {
                Some(lut) => convert(f64::from(lut.lookup(stored))),
                None => convert(stored as f64 * slope + intercept),
            }
        })
        .collect()
}
pub fn decode_stored(
    frame: &Frame<'_>,
    destination_len: usize,
    signed_target: bool,
) -> Result<Vec<i32>, DisplayError> {
    if frame.format.bits_allocated != 16 {
        return Err(DisplayError::frame(
            "BitsAllocated",
            FrameFault::UnsupportedBitsAllocated,
        ));
    }
    let count = modality_sample_count(frame, 2)?;
    if destination_len < count {
        return Err(DisplayError::frame(
            "Destination",
            FrameFault::DestinationTooShort,
        ));
    }
    let (minimum, maximum) = if signed_target {
        (i64::from(i16::MIN), i64::from(i16::MAX))
    } else {
        (0, i64::from(u16::MAX))
    };
    let reader = frame.format.reader();
    let mut out = vec![0i32; destination_len];
    for (o, sample) in out
        .iter_mut()
        .zip(frame.pixels.as_chunks::<2>().0)
        .take(count)
    {
        let stored = reader.value(sample);
        if stored < minimum || stored > maximum {
            return Err(DisplayError::frame("Pixels", FrameFault::Invalid));
        }
        *o = stored as i32;
    }
    Ok(out)
}
pub trait StoredTarget: Copy + Send + Sync + 'static {
    const MINIMUM: i64;
    const MAXIMUM: i64;
    fn from_stored(value: i64) -> Self;
}
impl StoredTarget for i16 {
    const MINIMUM: i64 = i16::MIN as i64;
    const MAXIMUM: i64 = i16::MAX as i64;
    fn from_stored(value: i64) -> i16 {
        value as i16
    }
}
impl StoredTarget for u16 {
    const MINIMUM: i64 = 0;
    const MAXIMUM: i64 = u16::MAX as i64;
    fn from_stored(value: i64) -> u16 {
        value as u16
    }
}
pub fn decode_stored_into<T: StoredTarget>(
    destination: &mut [T],
    frame: &Frame<'_>,
) -> Result<(), DisplayError> {
    if frame.format.bits_allocated != 16 {
        return Err(DisplayError::frame(
            "BitsAllocated",
            FrameFault::UnsupportedBitsAllocated,
        ));
    }
    let count = modality_sample_count(frame, 2)?;
    if destination.len() < count {
        return Err(DisplayError::frame(
            "Destination",
            FrameFault::DestinationTooShort,
        ));
    }
    let reader = frame.format.reader();
    let samples = frame.pixels.as_chunks::<2>().0;
    let count = count.min(samples.len()).min(destination.len());
    let (samples, destination) = (&samples[..count], &mut destination[..count]);
    const BLOCK: usize = 1024;
    let mut values = [0i32; BLOCK];
    for (out, block) in destination.chunks_mut(BLOCK).zip(samples.chunks(BLOCK)) {
        let values = &mut values[..block.len()];
        reader.values16(block, values);
        let outside = values.iter().fold(false, |outside, &stored| {
            outside | (i64::from(stored) < T::MINIMUM) | (i64::from(stored) > T::MAXIMUM)
        });
        if !outside {
            for (o, &stored) in out.iter_mut().zip(values.iter()) {
                *o = T::from_stored(i64::from(stored));
            }
            continue;
        }
        for (o, &stored) in out.iter_mut().zip(values.iter()) {
            let stored = i64::from(stored);
            if stored < T::MINIMUM || stored > T::MAXIMUM {
                return Err(DisplayError::frame("Pixels", FrameFault::Invalid));
            }
            *o = T::from_stored(stored);
        }
    }
    Ok(())
}
pub fn decode_modality_levels(frame: &Frame<'_>) -> Result<Option<ModalityLevels>, DisplayError> {
    let count = modality_sample_count(frame, 4)?;
    let bits_allocated = frame.format.bits_allocated;
    if bits_allocated != 8 && bits_allocated != 16 {
        return Ok(None);
    }
    let width = (bits_allocated / 8) as usize;
    let reader = frame.format.reader();
    let mut index = Vec::with_capacity(count);
    let (mut lowest, mut highest) = (i64::MAX, i64::MIN);
    for sample in frame.pixels.chunks_exact(width).take(count) {
        let stored = reader.value(sample);
        index.push(stored as u16);
        lowest = lowest.min(stored);
        highest = highest.max(stored);
    }
    let span = highest - lowest + 1;
    if span > MAX_MODALITY_LEVELS || span > count as i64 {
        return Ok(None);
    }
    let levels: Vec<f32> = match &frame.modality.lut {
        Some(lut) => (0..span)
            .map(|k| f32::from(lut.lookup(lowest + k)))
            .collect(),
        None => {
            let slope = frame.modality.effective_slope();
            let intercept = frame.modality.intercept;
            (0..span)
                .map(|k| ((lowest + k) as f64 * slope + intercept) as f32)
                .collect()
        }
    };
    let offset = lowest as u16;
    for i in &mut index {
        *i = i.wrapping_sub(offset);
    }
    Ok(Some(ModalityLevels { index, levels }))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFault {
    Invalid,
    UnsupportedBitsAllocated,
    PixelDataTooShort,
    DestinationTooShort,
    SizeOverflow,
}
impl std::error::Error for FrameFault {}
fn modality_sample_count(frame: &Frame<'_>, output_bytes: i64) -> Result<usize, DisplayError> {
    validate_dimensions(frame.rows, frame.columns)?;
    let bits_allocated = frame.format.bits_allocated;
    if !matches!(bits_allocated, 8 | 16 | 32) {
        return Err(DisplayError::frame(
            "BitsAllocated",
            FrameFault::UnsupportedBitsAllocated,
        ));
    }
    let bytes_per_sample = bits_allocated / 8;
    let pixels = checked_layout(frame.rows, frame.columns, 1, bytes_per_sample, output_bytes)?;
    if (frame.pixels.len() as i64) < pixels * bytes_per_sample {
        return Err(DisplayError::frame("Pixels", FrameFault::PixelDataTooShort));
    }
    Ok(pixels as usize)
}
fn validate_dimensions(rows: i64, columns: i64) -> Result<(), DisplayError> {
    if rows <= 0 {
        return Err(DisplayError::frame("Rows", FrameFault::Invalid));
    }
    if columns <= 0 {
        return Err(DisplayError::frame("Columns", FrameFault::Invalid));
    }
    Ok(())
}
fn checked_layout(
    rows: i64,
    columns: i64,
    samples_per_pixel: i64,
    bytes_per_sample: i64,
    output_bytes_per_pixel: i64,
) -> Result<i64, DisplayError> {
    validate_dimensions(rows, columns)?;
    if samples_per_pixel <= 0 {
        return Err(DisplayError::frame("SamplesPerPixel", FrameFault::Invalid));
    }
    if bytes_per_sample <= 0 {
        return Err(DisplayError::frame("BytesPerSample", FrameFault::Invalid));
    }
    if output_bytes_per_pixel <= 0 {
        return Err(DisplayError::frame(
            "OutputBytesPerPixel",
            FrameFault::Invalid,
        ));
    }
    let overflow = |field| DisplayError::frame(field, FrameFault::SizeOverflow);
    let pixels = rows
        .checked_mul(columns)
        .ok_or_else(|| overflow("PixelCount"))?;
    let samples = pixels
        .checked_mul(samples_per_pixel)
        .ok_or_else(|| overflow("SourceStride"))?;
    samples
        .checked_mul(bytes_per_sample)
        .ok_or_else(|| overflow("SourceBytes"))?;
    pixels
        .checked_mul(output_bytes_per_pixel)
        .ok_or_else(|| overflow("OutputBytes"))?;
    Ok(pixels)
}
fn frame_voi_mapper(
    voi: &VoiLut,
    modality: &ModalityLut,
    reader: &StoredSampleReader,
) -> VoiByteMapper {
    if voi.lut.is_some() || voi.has_window() {
        return VoiByteMapper::new(voi.clone());
    }
    let (min_stored, max_stored) = reader.range();
    let mut min_value = modality.apply(min_stored);
    let mut max_value = modality.apply(max_stored);
    if min_value > max_value {
        std::mem::swap(&mut min_value, &mut max_value);
    }
    let span = max_value - min_value;
    if span > 0.0 && voi::finite(span) {
        return VoiByteMapper::range(min_value, span);
    }
    VoiByteMapper::new(VoiLut {
        center: (min_value + max_value) / 2.0,
        width: span + 1.0,
        ..VoiLut::default()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ct_window_renders() {
        let pixels: Vec<u8> = [0i16, 1000, 1040, 2000]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let frame = Frame {
            rows: 2,
            columns: 2,
            pixels: &pixels,
            format: PixelFormat {
                bits_allocated: 16,
                bits_stored: 16,
                high_bit: 15,
                signed: true,
                big_endian: false,
            },
            modality: ModalityLut {
                slope: 1.0,
                intercept: -1024.0,
                lut: None,
            },
            voi: VoiLut {
                center: 40.0,
                width: 400.0,
                ..VoiLut::default()
            },
        };
        let image = render_gray(&frame).unwrap();
        assert_eq!(image.pixels[0], 0);
        assert_eq!(image.pixels[3], 255);
    }
}
