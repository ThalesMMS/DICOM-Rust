#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::frame::{
    Frame, FrameIdentity, RenderImage, effective_frame_metadata, load_frame,
    normalized_photometric, sanitized_rescale,
};
use super::parallel::{Cancellation, ctx_err, parallel_rows_worker_context};
use super::window::{WindowLevel, normalize_window, prepare_window, valid_window_width};
use crate::display::StoredSampleReader;
use crate::fmath;
use std::sync::{Arc, Mutex, OnceLock};
pub const MAX_SOURCE_CONVOLUTION_SAMPLES: i64 = 16 << 20;
const DEFAULT_SOURCE_CONVOLUTION_CACHE_BYTES: i64 = 64 << 20;
const DEFAULT_SOURCE_CONVOLUTION_CACHE_ENTRIES: usize = 16;
fn kernel_error() -> RenderError {
    RenderError::new(
        ErrorKind::SourceConvolutionKernel,
        "render: invalid source convolution kernel",
    )
}
fn frame_error() -> RenderError {
    RenderError::new(
        ErrorKind::SourceConvolutionFrame,
        "render: unsupported source convolution frame",
    )
}
fn frame_error_detail(detail: impl std::fmt::Display) -> RenderError {
    RenderError::new(
        ErrorKind::SourceConvolutionFrame,
        format!("render: unsupported source convolution frame: {detail}"),
    )
}
fn limit_error() -> RenderError {
    RenderError::new(
        ErrorKind::SourceConvolutionLimit,
        "render: source convolution sample limit exceeded",
    )
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SourceKernel3x3 {
    pub weights: [f64; 9],
    pub divisor: f64,
    pub bias: f64,
    pub midrange_bias: bool,
}
impl SourceKernel3x3 {
    fn check(&self) -> Result<(), RenderError> {
        if !self.divisor.is_finite() || self.divisor == 0.0 || !self.bias.is_finite() {
            return Err(kernel_error());
        }
        if self.weights.iter().any(|w| !w.is_finite()) {
            return Err(kernel_error());
        }
        Ok(())
    }
    fn finite_range(&self, lo: i64, hi: i64) -> bool {
        let abs_weight: f64 = self.weights.iter().fold(0.0, |acc, w| acc + w.abs());
        let abs_sum = abs_weight * fmath::max((lo as f64).abs(), (hi as f64).abs());
        abs_sum.is_finite() && (abs_sum / self.divisor.abs() + self.bias.abs()).is_finite()
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ConvolvedFrame {
    width: i64,
    height: i64,
    values: Vec<f64>,
    rgb: Vec<u8>,
    is_rgb: bool,
    invert: bool,
    default_window: WindowLevel,
}
impl ConvolvedFrame {
    pub fn gray_values(&self) -> Option<(&[f64], i64, i64, bool)> {
        (!self.is_rgb).then_some((&self.values, self.width, self.height, self.invert))
    }
    pub fn memory_bytes(&self) -> i64 {
        let mut bytes = self.values.len() as i64 * 8 + self.rgb.len() as i64;
        if let Some(lut) = &self.default_window.lut {
            bytes += lut.entries.len() as i64 * 2;
        }
        bytes
    }
    pub fn render(
        &self,
        ctx: Option<&dyn Cancellation>,
        window: &WindowLevel,
    ) -> Result<RenderImage, RenderError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        if self.width <= 0 || self.height <= 0 {
            return Err(frame_error());
        }
        let (w, h) = (self.width as usize, self.height as usize);
        if self.is_rgb {
            return Ok(RenderImage::Rgba {
                width: w,
                height: h,
                pixels: self.rgb.clone(),
            });
        }
        let mut window = window.clone();
        if window.lut.is_none() && !valid_window_width(window.width) {
            window = self.default_window.clone();
        }
        let window = normalize_window(&window, &self.default_window);
        let mapper = prepare_window(&window);
        let slots: Vec<OnceLock<Vec<u8>>> = (0..h).map(|_| OnceLock::new()).collect();
        parallel_rows_worker_context(ctx, h as i64, || {
            |y: i64| {
                let y = y as usize;
                let mut line = vec![0u8; w];
                for (x, v) in self.values[y * w..(y + 1) * w].iter().enumerate() {
                    if v.is_nan() {
                        continue;
                    }
                    let g = mapper.gray(*v);
                    line[x] = if self.invert { 255 - g } else { g };
                }
                let _ = slots[y].set(line);
            }
        })?;
        Ok(RenderImage::gray(
            w,
            h,
            slots
                .into_iter()
                .flat_map(|s| s.into_inner().unwrap_or_else(|| vec![0; w]))
                .collect(),
        ))
    }
}
fn check_size(frame: &Frame) -> Result<(), RenderError> {
    let m = &frame.metadata;
    let (columns, rows, channels) = (
        i64::from(m.columns),
        i64::from(m.rows),
        i64::from(m.samples_per_pixel),
    );
    if columns <= 0 || rows <= 0 || (channels != 1 && channels != 3) {
        return Err(frame_error());
    }
    if rows > MAX_SOURCE_CONVOLUTION_SAMPLES / (columns * channels) {
        return Err(limit_error());
    }
    Ok(())
}
fn own_window(window: &WindowLevel) -> Result<WindowLevel, RenderError> {
    let mut window = window.clone();
    if let Some(lut) = &mut window.lut {
        if lut.entries.len() > 65536 {
            return Err(limit_error());
        }
        lut.adjustments.clear();
    }
    Ok(window)
}
fn read_row(
    dest: &mut [i32],
    source: &[u8],
    reader: &StoredSampleReader,
    y: usize,
    columns: usize,
    rows: usize,
    planar: bool,
) {
    let width = reader.width();
    if planar {
        for c in 0..3 {
            let start = (c * rows + y) * columns * width;
            let values = &source[start..start + columns * width];
            for x in 0..columns {
                dest[3 * x + c] = reader.value(&values[x * width..]) as i32;
            }
        }
        return;
    }
    let offset = y * dest.len() * width;
    let values = &source[offset..offset + dest.len() * width];
    for (i, d) in dest.iter_mut().enumerate() {
        *d = reader.value(&values[i * width..]) as i32;
    }
}
#[allow(clippy::too_many_arguments)]
fn correlate(
    w: &[f64; 9],
    top: &[i32],
    center: &[i32],
    bottom: &[i32],
    left: usize,
    middle: usize,
    right: usize,
) -> f64 {
    f64::from(top[left]) * w[0]
        + f64::from(top[middle]) * w[1]
        + f64::from(top[right]) * w[2]
        + f64::from(center[left]) * w[3]
        + f64::from(center[middle]) * w[4]
        + f64::from(center[right]) * w[5]
        + f64::from(bottom[left]) * w[6]
        + f64::from(bottom[middle]) * w[7]
        + f64::from(bottom[right]) * w[8]
}
fn clamp_float(v: f64, lo: f64, hi: f64) -> f64 {
    fmath::max(lo, fmath::min(hi, v))
}
fn gray_domain(signed: bool) -> (f64, f64) {
    if signed {
        (-32768.0, 32767.0)
    } else {
        (0.0, 65535.0)
    }
}
pub fn prepare_frame_convolution_3x3(
    ctx: Option<&dyn Cancellation>,
    frame: Option<&Frame>,
    kernel: SourceKernel3x3,
) -> Result<ConvolvedFrame, RenderError> {
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    kernel.check()?;
    let Some(frame) = frame else {
        return Err(frame_error());
    };
    check_size(frame)?;
    let frame = load_frame(Some(frame))?;
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    let meta = effective_frame_metadata(&frame);
    let photo = normalized_photometric(&meta.photometric_interpretation);
    let rgb = meta.samples_per_pixel == 3;
    if (meta.bits_allocated != 8 && meta.bits_allocated != 16) || meta.pixel_representation > 1 {
        return Err(frame_error());
    }
    if rgb {
        if photo != "RGB" || meta.planar_configuration > 1 || meta.pixel_representation != 0 {
            return Err(frame_error());
        }
    } else if photo != "MONOCHROME1" && photo != "MONOCHROME2" {
        return Err(frame_error());
    }
    let signed = meta.pixel_representation != 0;
    let reader = StoredSampleReader::new(
        i64::from(meta.bits_allocated),
        i64::from(meta.bits_stored),
        i64::from(meta.high_bit),
        signed,
        frame.big_endian,
    )
    .map_err(|e| {
        let text = e.to_string();
        let field = text.split(' ').next().unwrap_or_default();
        frame_error_detail(format!(
            "dicom: invalid stored sample format: {field}: {text}"
        ))
    })?;
    let (stored_min, stored_max) = reader.range();
    let mut kernel = kernel;
    if kernel.midrange_bias && !signed {
        kernel.bias += ((stored_max + 1) / 2) as f64;
    }
    if !kernel.finite_range(stored_min, stored_max) {
        return Err(kernel_error());
    }
    let (columns, rows, channels) = (
        usize::from(meta.columns),
        usize::from(meta.rows),
        usize::from(meta.samples_per_pixel),
    );
    let input_bytes = columns * rows * channels * reader.width();
    let len = frame.pixel_bytes.len();
    if len != input_bytes && (input_bytes % 2 == 0 || len != input_bytes + 1) {
        return Err(frame_error_detail(format!(
            "expected {input_bytes} source bytes, got {len}"
        )));
    }
    let mut result = ConvolvedFrame {
        width: columns as i64,
        height: rows as i64,
        values: Vec::new(),
        rgb: Vec::new(),
        is_rgb: rgb,
        invert: false,
        default_window: WindowLevel::default(),
    };
    let (slope, intercept) = sanitized_rescale(frame.rescale);
    let (lo, hi) = gray_domain(signed);
    if !rgb {
        if !(lo * slope + intercept).is_finite() || !(hi * slope + intercept).is_finite() {
            return Err(frame_error());
        }
        result.default_window = own_window(&frame.default_window)?;
        result.invert = photo == "MONOCHROME1";
    }
    let container_max = ((1u32 << meta.bits_allocated) - 1) as f64;
    let rgb_max = stored_max as f64;
    let source = &frame.pixel_bytes[..input_bytes];
    let planar = rgb && meta.planar_configuration == 1;
    let padding = frame.pixel_padding;
    let line_length = columns * channels;
    let out_width = if rgb { columns * 4 } else { columns };
    type Row = (Vec<f64>, Vec<u8>);
    let slots: Vec<OnceLock<Row>> = (0..rows).map(|_| OnceLock::new()).collect();
    parallel_rows_worker_context(ctx, rows as i64, || {
        let mut buffer = vec![0i32; 3 * line_length];
        let mut row_indices = [usize::MAX; 3];
        let slots = &slots;
        let reader = &reader;
        move |y: i64| {
            let y = y as usize;
            let mut fetch = |yy: usize, buffer: &mut [i32]| {
                let slot = yy % 3;
                if row_indices[slot] != yy {
                    read_row(
                        &mut buffer[slot * line_length..(slot + 1) * line_length],
                        source,
                        reader,
                        yy,
                        columns,
                        rows,
                        planar,
                    );
                    row_indices[slot] = yy;
                }
                slot
            };
            let t = fetch(y.saturating_sub(1), &mut buffer);
            let c = fetch(y, &mut buffer);
            let b = fetch((y + 1).min(rows - 1), &mut buffer);
            let line = |s: usize| &buffer[s * line_length..(s + 1) * line_length];
            let (top, center, bottom) = (line(t), line(c), line(b));
            let mut values = if rgb {
                Vec::new()
            } else {
                vec![0.0; out_width]
            };
            let mut colours = if rgb {
                vec![0u8; out_width]
            } else {
                Vec::new()
            };
            for x in 0..columns {
                let left = x.saturating_sub(1) * channels;
                let middle = x * channels;
                let right = (x + 1).min(columns - 1) * channels;
                if !rgb
                    && let Some(p) = &padding
                    && p.contains(i64::from(center[middle]))
                {
                    values[x] = f64::NAN;
                    continue;
                }
                let mut sums = [0.0; 3];
                for (ch, s) in sums.iter_mut().enumerate().take(channels) {
                    *s = correlate(
                        &kernel.weights,
                        top,
                        center,
                        bottom,
                        left + ch,
                        middle + ch,
                        right + ch,
                    );
                }
                if rgb {
                    for (ch, sum) in sums.iter().enumerate() {
                        let sample =
                            clamp_float(sum / kernel.divisor + kernel.bias, 0.0, container_max);
                        let normalized = fmath::min(1.0, sample / rgb_max);
                        colours[x * 4 + ch] =
                            fmath::f64_to_u8((255.0 * clamp_float(normalized, 0.0, 1.0)).round());
                    }
                    colours[x * 4 + 3] = 255;
                } else {
                    let stored = clamp_float(sums[0] / kernel.divisor + kernel.bias, lo, hi);
                    values[x] = stored * slope + intercept;
                }
            }
            let _ = slots[y].set((values, colours));
        }
    })?;
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    for slot in slots {
        let (values, colours) = slot.into_inner().unwrap_or_default();
        if rgb {
            result.rgb.extend_from_slice(&colours);
        } else {
            result.values.extend_from_slice(&values);
        }
    }
    Ok(result)
}
#[derive(Clone)]
struct ConvolutionKey {
    identity: Arc<FrameIdentity>,
    frame: i64,
    kernel: SourceKernel3x3,
}
impl ConvolutionKey {
    fn matches(&self, other: &ConvolutionKey) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
            && self.frame == other.frame
            && self.kernel == other.kernel
    }
}
#[derive(Default)]
struct ConvolutionState {
    generation: u64,
    bytes: i64,
    computations: u64,
    entries: Vec<(ConvolutionKey, Arc<ConvolvedFrame>)>,
}
impl ConvolutionState {
    fn lookup(&mut self, key: &ConvolutionKey) -> Option<Arc<ConvolvedFrame>> {
        let i = self.entries.iter().position(|(k, _)| k.matches(key))?;
        let entry = self.entries.remove(i);
        let prepared = entry.1.clone();
        self.entries.insert(0, entry);
        Some(prepared)
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceConvolutionCacheStats {
    pub retained_bytes: i64,
    pub entries: i64,
    pub computations: u64,
}
pub struct SourceConvolutionCache {
    max_bytes: i64,
    state: Mutex<ConvolutionState>,
    work: Mutex<()>,
}
impl SourceConvolutionCache {
    pub fn new(max_bytes: i64) -> SourceConvolutionCache {
        SourceConvolutionCache {
            max_bytes: if max_bytes <= 0 {
                DEFAULT_SOURCE_CONVOLUTION_CACHE_BYTES
            } else {
                max_bytes
            },
            state: Mutex::new(ConvolutionState::default()),
            work: Mutex::new(()),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, ConvolutionState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn key(frame: &Frame, kernel: SourceKernel3x3) -> Option<ConvolutionKey> {
        Some(ConvolutionKey {
            identity: frame.cache_identity.clone()?,
            frame: frame.frame_index,
            kernel,
        })
    }
    pub fn lookup(
        &self,
        frame: Option<&Frame>,
        kernel: SourceKernel3x3,
    ) -> Option<Arc<ConvolvedFrame>> {
        let key = Self::key(frame?, kernel)?;
        self.lock().lookup(&key)
    }
    pub fn prepare(
        &self,
        ctx: Option<&dyn Cancellation>,
        frame: Option<&Frame>,
        kernel: SourceKernel3x3,
    ) -> Result<Arc<ConvolvedFrame>, RenderError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let Some(key) = frame.and_then(|f| Self::key(f, kernel)) else {
            return prepare_frame_convolution_3x3(ctx, frame, kernel).map(Arc::new);
        };
        let Some(frame) = frame else {
            return Err(frame_error());
        };
        let generation = {
            let mut s = self.lock();
            if let Some(prepared) = s.lookup(&key) {
                return Ok(prepared);
            }
            s.generation
        };
        check_size(frame)?;
        let mut bytes_per_pixel = 8;
        let mut lut_bytes = 0;
        if frame.metadata.samples_per_pixel == 3 {
            bytes_per_pixel = 4;
        } else if let Some(lut) = &frame.default_window.lut {
            lut_bytes = lut.entries.len() as i64 * 2;
        }
        if i64::from(frame.metadata.columns) * i64::from(frame.metadata.rows) * bytes_per_pixel
            + lut_bytes
            > self.max_bytes
        {
            return Err(limit_error());
        }
        let _work = self.work.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        {
            let mut s = self.lock();
            if let Some(prepared) = s.lookup(&key) {
                return Ok(prepared);
            }
            s.computations += 1;
        }
        let prepared = Arc::new(prepare_frame_convolution_3x3(ctx, Some(frame), kernel)?);
        let bytes = prepared.memory_bytes();
        let mut s = self.lock();
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        if generation == s.generation && bytes <= self.max_bytes {
            while s.entries.len() >= DEFAULT_SOURCE_CONVOLUTION_CACHE_ENTRIES
                || bytes > self.max_bytes - s.bytes
            {
                let Some((_, old)) = s.entries.pop() else {
                    break;
                };
                s.bytes -= old.memory_bytes();
            }
            s.entries.insert(0, (key, prepared.clone()));
            s.bytes += bytes;
        }
        Ok(prepared)
    }
    pub fn clear(&self) -> i64 {
        let mut s = self.lock();
        let freed = s.bytes;
        s.generation += 1;
        s.entries.clear();
        s.bytes = 0;
        freed
    }
    pub fn stats(&self) -> SourceConvolutionCacheStats {
        let s = self.lock();
        SourceConvolutionCacheStats {
            retained_bytes: s.bytes,
            entries: s.entries.len() as i64,
            computations: s.computations,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PyramidImage {
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
    Nrgba {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    },
    Gray16 {
        width: usize,
        height: usize,
        pixels: Vec<u16>,
    },
}
impl PyramidImage {
    pub fn size(&self) -> (usize, usize) {
        match self {
            PyramidImage::Gray { width, height, .. }
            | PyramidImage::Rgba { width, height, .. }
            | PyramidImage::Nrgba { width, height, .. }
            | PyramidImage::Gray16 { width, height, .. } => (*width, *height),
        }
    }
}
pub fn halve_box(img: &PyramidImage) -> Option<PyramidImage> {
    let (sw, sh) = img.size();
    let (w, h) = (sw / 2, sh / 2);
    if w < 1 || h < 1 {
        return None;
    }
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            match img {
                PyramidImage::Rgba { pixels, .. } => {
                    let (i0, i1) = ((2 * y * sw + 2 * x) * 4, ((2 * y + 1) * sw + 2 * x) * 4);
                    for c in 0..4 {
                        let sum = u32::from(pixels[i0 + c])
                            + u32::from(pixels[i0 + 4 + c])
                            + u32::from(pixels[i1 + c])
                            + u32::from(pixels[i1 + 4 + c]);
                        out[o + c] = ((sum + 2) / 4) as u8;
                    }
                }
                PyramidImage::Nrgba { pixels, .. } => {
                    let row0 = 2 * y * sw * 4;
                    let row1 = row0 + sw * 4;
                    let mut sum = [0u32; 4];
                    for i in [
                        row0 + 8 * x,
                        row0 + 8 * x + 4,
                        row1 + 8 * x,
                        row1 + 8 * x + 4,
                    ] {
                        let a = u32::from(pixels[i + 3]);
                        for c in 0..3 {
                            let v = u32::from(pixels[i + c]);
                            sum[c] += if a == 255 { v } else { (v * a + 127) / 255 };
                        }
                        sum[3] += a;
                    }
                    for c in 0..4 {
                        out[o + c] = ((sum[c] + 2) / 4) as u8;
                    }
                }
                PyramidImage::Gray { pixels, .. } => {
                    let (r0, r1) = (2 * y * sw, (2 * y + 1) * sw);
                    let sum = u32::from(pixels[r0 + 2 * x])
                        + u32::from(pixels[r0 + 2 * x + 1])
                        + u32::from(pixels[r1 + 2 * x])
                        + u32::from(pixels[r1 + 2 * x + 1]);
                    let v = ((sum + 2) / 4) as u8;
                    out[o..o + 4].copy_from_slice(&[v, v, v, 255]);
                }
                PyramidImage::Gray16 { pixels, .. } => {
                    let mut sum = [0u32; 4];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let v = u32::from(pixels[(2 * y + dy) * sw + 2 * x + dx]);
                        sum[0] += v;
                        sum[1] += v;
                        sum[2] += v;
                        sum[3] += 0xffff;
                    }
                    for c in 0..4 {
                        out[o + c] = (((sum[c] + 2) / 4 + 128) / 257) as u8;
                    }
                }
            }
        }
    }
    Some(PyramidImage::Rgba {
        width: w,
        height: h,
        pixels: out,
    })
}
pub fn shrunk_pixel_center(index: i64, factor: i64) -> f64 {
    factor.wrapping_mul(index) as f64 + factor.wrapping_sub(1) as f64 / 2.0
}
pub struct ImagePyramid {
    levels: Mutex<Vec<Arc<PyramidImage>>>,
}
impl ImagePyramid {
    pub fn new(img: PyramidImage) -> ImagePyramid {
        ImagePyramid {
            levels: Mutex::new(vec![Arc::new(img)]),
        }
    }
    pub fn source(&self) -> Arc<PyramidImage> {
        self.levels.lock().unwrap_or_else(|e| e.into_inner())[0].clone()
    }
    pub fn for_display(&self, width: i64, height: i64) -> (Arc<PyramidImage>, i64) {
        let mut levels = self.levels.lock().unwrap_or_else(|e| e.into_inner());
        if width <= 0 || height <= 0 {
            return (levels[0].clone(), 1);
        }
        let (mut level, mut factor) = (0usize, 1i64);
        loop {
            let (w, h) = levels[level].size();
            if ((w / 2) as i64) < width || ((h / 2) as i64) < height {
                return (levels[level].clone(), factor);
            }
            if level + 1 == levels.len() {
                let Some(next) = halve_box(&levels[level]) else {
                    return (levels[level].clone(), factor);
                };
                levels.push(Arc::new(next));
            }
            level += 1;
            factor *= 2;
        }
    }
}
