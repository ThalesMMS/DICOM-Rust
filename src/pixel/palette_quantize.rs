use crate::derivedio;
use crate::display::encode_lut_data;
use crate::fmath;
use crate::net::ul::{Ctx, Kind as CtxKind};
use crate::object::{Object, Tag, Vr};
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{NativeFrames, PixelError, extract_metadata, extract_native_frames};
use std::collections::HashSet;
use std::fmt;
const ERR_PALETTE_QUANTIZE: &str = "dicom: palette quantization";
const ERR_PALETTE_QUANTIZE_QUALITY: &str = "dicom: palette quantization error exceeds limit";
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaletteQuantizeOptions {
    pub max_colors: i64,
    pub dither: bool,
    pub max_pixels: i64,
    pub max_rmse: f64,
}
#[derive(Clone, Debug)]
pub struct PaletteQuantization {
    pub image_pixel: Object,
    pub colors: i64,
    pub frames: i64,
    pub rmse: f64,
    pub max_absolute_error: u8,
}
#[derive(Clone, Debug, PartialEq)]
pub enum QuantizeError {
    Invalid(&'static str),
    Quality { rmse: f64, max_rmse: f64 },
    Pixel(PixelError),
    Context(CtxKind),
}
impl fmt::Display for QuantizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuantizeError::Invalid(detail) => write!(f, "{ERR_PALETTE_QUANTIZE}: {detail}"),
            QuantizeError::Quality { rmse, max_rmse } => {
                write!(
                    f,
                    "{ERR_PALETTE_QUANTIZE_QUALITY}: RMSE {rmse:.6} > {max_rmse:.6}"
                )
            }
            QuantizeError::Pixel(e) => e.fmt(f),
            QuantizeError::Context(CtxKind::Canceled) => f.write_str("context canceled"),
            QuantizeError::Context(_) => f.write_str("context deadline exceeded"),
        }
    }
}
impl std::error::Error for QuantizeError {}
fn context(ctx: &Ctx) -> Result<(), QuantizeError> {
    ctx.err().map_or(Ok(()), |k| Err(QuantizeError::Context(k)))
}
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const PALETTE_DESCRIPTORS: [Tag; 3] = [
    Tag::new(0x0028, 0x1101),
    Tag::new(0x0028, 0x1102),
    Tag::new(0x0028, 0x1103),
];
const PALETTE_DATA: [Tag; 3] = [
    Tag::new(0x0028, 0x1201),
    Tag::new(0x0028, 0x1202),
    Tag::new(0x0028, 0x1203),
];
pub fn quantize_palette_color(
    ctx: &Ctx,
    source: &Object,
    opts: &PaletteQuantizeOptions,
) -> Result<PaletteQuantization, QuantizeError> {
    context(ctx)?;
    let mut opts = *opts;
    if opts.max_colors == 0 {
        opts.max_colors = 256;
    }
    if opts.max_pixels == 0 {
        opts.max_pixels = 16_000_000;
    }
    if !(1..=256).contains(&opts.max_colors)
        || opts.max_pixels < 1
        || opts.max_rmse < 0.0
        || !opts.max_rmse.is_finite()
    {
        return Err(QuantizeError::Invalid("invalid options"));
    }
    let m = extract_metadata(source).map_err(QuantizeError::Pixel)?;
    if m.photometric_interpretation != "RGB"
        || m.samples_per_pixel != 3
        || m.bits_allocated != 8
        || m.bits_stored != 8
        || m.high_bit != 7
        || m.pixel_representation != 0
        || !m.planar_configuration_present
        || m.planar_configuration > 1
        || m.rows == 0
        || m.columns == 0
        || m.number_of_frames < 1
    {
        return Err(QuantizeError::Invalid(
            "expected native unsigned RGB8 with planar configuration 0 or 1",
        ));
    }
    let pixels_per_frame = i64::from(m.rows).saturating_mul(i64::from(m.columns));
    if m.number_of_frames > opts.max_pixels.checked_div(pixels_per_frame).unwrap_or(0)
        || pixels_per_frame > i64::MAX / 3
        || m.number_of_frames > (i64::MAX - 1).checked_div(pixels_per_frame).unwrap_or(0)
    {
        return Err(QuantizeError::Invalid("pixel count exceeds limit"));
    }
    if source
        .get(TAG_PIXEL_DATA)
        .is_some_and(|e| e.vr() == Vr::OW && source.value_big_endian())
    {
        return Err(QuantizeError::Invalid(
            "big-endian OW RGB8 requires byte-order normalization",
        ));
    }
    let native = extract_native_frames(source).map_err(QuantizeError::Pixel)?;
    let frame_bytes = (pixels_per_frame as usize).saturating_mul(3);
    if native.metadata.rows != m.rows
        || native.metadata.columns != m.columns
        || native.metadata.planar_configuration != m.planar_configuration
        || native.data.iter().any(|f| f.len() < frame_bytes)
    {
        return Err(QuantizeError::Invalid("pixel data shorter than its frames"));
    }
    let palette = build(ctx, &native, opts.max_colors as usize)?;
    let (mut indices, squared_error, max_error) = map(ctx, &native, &palette, opts.dither)?;
    let rmse = (squared_error / (pixels_per_frame as f64 * m.number_of_frames as f64 * 3.0)).sqrt();
    if opts.max_rmse > 0.0 && rmse > opts.max_rmse {
        return Err(QuantizeError::Quality {
            rmse,
            max_rmse: opts.max_rmse,
        });
    }
    context(ctx)?;
    if indices.len() % 2 != 0 {
        indices.push(0);
    }
    let mut out = derivedio::object(vec![
        derivedio::us(TAG_ROWS, &[m.rows]),
        derivedio::us(TAG_COLUMNS, &[m.columns]),
        derivedio::us(TAG_SAMPLES_PER_PIXEL, &[1]),
        derivedio::cs(TAG_PHOTOMETRIC, "PALETTE COLOR"),
        derivedio::us(TAG_BITS_ALLOCATED, &[8]),
        derivedio::us(TAG_BITS_STORED, &[8]),
        derivedio::us(TAG_HIGH_BIT, &[7]),
        derivedio::us(TAG_PIXEL_REPRESENTATION, &[0]),
        derivedio::raw(TAG_PIXEL_DATA, Vr::OB, indices),
    ]);
    if m.number_of_frames > 1 {
        out.put(derivedio::is(TAG_NUMBER_OF_FRAMES, &[m.number_of_frames]));
    }
    for (channel, (descriptor, data)) in PALETTE_DESCRIPTORS
        .into_iter()
        .zip(PALETTE_DATA)
        .enumerate()
    {
        let entries: Vec<u16> = palette
            .iter()
            .map(|rgb| {
                rgb.get(channel)
                    .map_or(0, |&v| u16::from(v).saturating_mul(257))
            })
            .collect();
        out.put(derivedio::us(descriptor, &[palette.len() as u16, 0, 16]));
        out.put(derivedio::raw(data, Vr::OW, encode_lut_data(&entries, 16)));
    }
    Ok(PaletteQuantization {
        image_pixel: out,
        colors: palette.len() as i64,
        frames: m.number_of_frames,
        rmse,
        max_absolute_error: max_error,
    })
}
#[derive(Clone, Copy, Default)]
struct Bin {
    count: u64,
    sum: [u64; 3],
}
#[derive(Clone, Copy)]
struct Color {
    bin: Bin,
    rgb: [u8; 3],
    key: usize,
}
struct BoxOf {
    colors: Vec<Color>,
    axis: usize,
    score: f64,
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn sample(frame: &[u8], m: &Metadata, pixel: usize) -> [u8; 3] {
    if m.planar_configuration == 1 {
        let n = usize::from(m.rows) * usize::from(m.columns);
        return [frame[pixel], frame[n + pixel], frame[2 * n + pixel]];
    }
    let p = 3 * pixel;
    [frame[p], frame[p + 1], frame[p + 2]]
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn build(
    ctx: &Ctx,
    native: &NativeFrames,
    max_colors: usize,
) -> Result<Vec<[u8; 3]>, QuantizeError> {
    let mut bins = vec![Bin::default(); 1 << 15];
    let mut exact: Option<HashSet<[u8; 3]>> = Some(HashSet::with_capacity(max_colors + 1));
    let m = &native.metadata;
    let (rows, columns) = (usize::from(m.rows), usize::from(m.columns));
    for frame in &native.data {
        for y in 0..rows {
            context(ctx)?;
            for x in 0..columns {
                let rgb = sample(frame, m, y * columns + x);
                if let Some(set) = &mut exact {
                    set.insert(rgb);
                    if set.len() > max_colors {
                        exact = None;
                    }
                }
                let bin = &mut bins[(usize::from(rgb[0] >> 3) << 10)
                    | (usize::from(rgb[1] >> 3) << 5)
                    | usize::from(rgb[2] >> 3)];
                bin.count = bin.count.saturating_add(1);
                for c in 0..3 {
                    bin.sum[c] = bin.sum[c].saturating_add(u64::from(rgb[c]));
                }
            }
        }
    }
    if let Some(set) = exact {
        let mut palette: Vec<[u8; 3]> = set.into_iter().collect();
        palette.sort_unstable();
        return Ok(palette);
    }
    let colors: Vec<Color> = bins
        .iter()
        .enumerate()
        .filter(|(_, bin)| bin.count > 0)
        .map(|(key, bin)| Color {
            bin: *bin,
            key,
            rgb: std::array::from_fn(|c| ((bin.sum[c] + bin.count / 2) / bin.count) as u8),
        })
        .collect();
    let mut boxes = vec![measure(colors)];
    while boxes.len() < max_colors {
        context(ctx)?;
        let mut chosen: Option<usize> = None;
        for (i, b) in boxes.iter().enumerate() {
            if b.colors.len() > 1 && chosen.is_none_or(|c| b.score > boxes[c].score) {
                chosen = Some(i);
            }
        }
        let Some(chosen) = chosen else {
            break;
        };
        let mut colors = std::mem::take(&mut boxes[chosen].colors);
        let axis = boxes[chosen].axis;
        colors.sort_by(|a, b| a.rgb[axis].cmp(&b.rgb[axis]).then(a.key.cmp(&b.key)));
        let total: u64 = colors.iter().map(|c| c.bin.count).sum();
        let mut accumulated = 0u64;
        let mut cut = 1;
        while cut < colors.len().saturating_sub(1) {
            accumulated = accumulated.saturating_add(colors[cut - 1].bin.count);
            if accumulated >= total.div_ceil(2) {
                break;
            }
            cut = cut.saturating_add(1);
        }
        let upper = colors.split_off(cut);
        boxes[chosen] = measure(colors);
        boxes.push(measure(upper));
    }
    let mut palette: Vec<[u8; 3]> = boxes
        .iter()
        .map(|b| {
            let count: u64 = b.colors.iter().map(|c| c.bin.count).sum();
            std::array::from_fn(|c| {
                let sum: u64 = b.colors.iter().map(|color| color.bin.sum[c]).sum();
                ((sum + count / 2) / count) as u8
            })
        })
        .collect();
    palette.sort_unstable();
    palette.dedup();
    Ok(palette)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn measure(colors: Vec<Color>) -> BoxOf {
    let mut weight = 0.0;
    let mut sums = [0.0f64; 3];
    let mut squares = [0.0f64; 3];
    for color in &colors {
        let n = color.bin.count as f64;
        weight += n;
        for c in 0..3 {
            let v = f64::from(color.rgb[c]);
            sums[c] += n * v;
            squares[c] += n * v * v;
        }
    }
    let mut out = BoxOf {
        colors,
        axis: 0,
        score: 0.0,
    };
    let mut largest = 0.0;
    for c in 0..3 {
        let variance = fmath::max(0.0, squares[c] - sums[c] * sums[c] / weight);
        out.score += variance;
        if variance > largest {
            largest = variance;
            out.axis = c;
        }
    }
    out
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn nearest(rgb: [f64; 3], palette: &[[u8; 3]]) -> usize {
    let mut best = 0;
    let mut distance = f64::INFINITY;
    for (i, candidate) in palette.iter().enumerate() {
        let mut squared = 0.0;
        for c in 0..3 {
            let d = rgb[c] - f64::from(candidate[c]);
            squared += d * d;
        }
        if squared < distance {
            best = i;
            distance = squared;
        }
    }
    best
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn map(
    ctx: &Ctx,
    native: &NativeFrames,
    palette: &[[u8; 3]],
    dither: bool,
) -> Result<(Vec<u8>, f64, u8), QuantizeError> {
    let m = &native.metadata;
    let (width, height) = (usize::from(m.columns), usize::from(m.rows));
    let mut indices = vec![0u8; width * height * native.data.len()];
    let mut squared_error = 0.0;
    let mut max_error = 0u8;
    for (f, frame) in native.data.iter().enumerate() {
        let (mut current, mut next) = if dither {
            (vec![[0.0f64; 3]; width + 2], vec![[0.0f64; 3]; width + 2])
        } else {
            (Vec::new(), Vec::new())
        };
        for y in 0..height {
            context(ctx)?;
            let reverse = dither && y % 2 == 1;
            for step in 0..width {
                let x = if reverse { width - 1 - step } else { step };
                let original = sample(frame, m, y * width + x);
                let mut corrected = [0.0f64; 3];
                for c in 0..3 {
                    corrected[c] = f64::from(original[c]);
                    if dither {
                        corrected[c] =
                            fmath::max(0.0, fmath::min(255.0, corrected[c] + current[x + 1][c]));
                    }
                }
                let index = nearest(corrected, palette);
                indices[f * width * height + y * width + x] = index as u8;
                for c in 0..3 {
                    let d = i64::from(original[c]) - i64::from(palette[index][c]);
                    squared_error += d.saturating_mul(d) as f64;
                    let absolute = d.unsigned_abs() as u8;
                    max_error = max_error.max(absolute);
                    if dither {
                        let error = corrected[c] - f64::from(palette[index][c]);
                        let (ahead, behind) = if reverse { (x, x + 2) } else { (x + 2, x) };
                        current[ahead][c] += error * 7.0 / 16.0;
                        next[behind][c] += error * 3.0 / 16.0;
                        next[x + 1][c] += error * 5.0 / 16.0;
                        next[ahead][c] += error / 16.0;
                    }
                }
            }
            if dither {
                std::mem::swap(&mut current, &mut next);
                next.iter_mut().for_each(|v| *v = [0.0; 3]);
            }
        }
    }
    Ok((indices, squared_error, max_error))
}
