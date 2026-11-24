#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::frame::{
    Frame, FrameIdentity, RenderImage, blank_image, decode_color_rgba, effective_frame_metadata,
    grayscale_display_frame, load_frame, normalized_photometric, sanitized_rescale,
};
use super::parallel::parallel_rows;
use super::png_encode::{PngSource, encode_png};
use super::volume::Stack;
use super::window::{DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH, WindowLevel};
use crate::display::{self, VoiByteMapper, VoiLut};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
const DEFAULT_RENDER_CACHE_BYTES: i64 = 128 << 20;
const LEVEL_INDEX_WINDOW_MISSES: i64 = 2;
const MAX_DECODED_LEVELS: f64 = 65536.0;
pub const ERR_NOT_GRAYSCALE_FRAME: &str = "render: frame is not grayscale";
pub const ERR_MODALITY_PRECISION_REQUIRED: &str =
    "render: float32 modality filter cannot preserve this frame's precision";
fn frame_error(kind: &str, detail: String) -> RenderError {
    RenderError::other(format!("dicom: {kind}: {detail}"))
}
fn display_err(e: impl std::fmt::Display) -> RenderError {
    RenderError::other(e.to_string())
}
fn missing_grayscale() -> RenderError {
    frame_error("invalid frame metadata", "missing grayscale pixels".into())
}
fn not_grayscale(photometric: &str) -> RenderError {
    RenderError::new(
        ErrorKind::NotGrayscaleFrame,
        format!("{ERR_NOT_GRAYSCALE_FRAME}: {photometric}"),
    )
}
#[derive(Debug, Clone, Default)]
pub(crate) struct DecodedFrame {
    pub(crate) rows: i64,
    pub(crate) cols: i64,
    photometric: String,
    pub(crate) grayscale: Option<Arc<Vec<f32>>>,
    grayscale64: Option<Arc<Vec<f64>>>,
    rgba: Option<Arc<Vec<u8>>>,
    level_index: Option<Arc<Vec<u16>>>,
    levels: Option<Arc<Vec<f32>>>,
}
impl DecodedFrame {
    fn has_level_index(&self) -> bool {
        match (&self.levels, &self.level_index) {
            (Some(_), Some(index)) => {
                index.len() as i64 == self.rows * self.cols
                    && self
                        .grayscale
                        .as_ref()
                        .is_none_or(|g| g.len() == index.len())
            }
            _ => false,
        }
    }
    fn has_grayscale(&self) -> bool {
        self.grayscale.is_some() || self.grayscale64.is_some() || self.has_level_index()
    }
    fn cache_bytes(&self) -> i64 {
        let len = |n: Option<usize>| n.unwrap_or(0) as i64;
        len(self.grayscale64.as_ref().map(|v| v.len())) * 8
            + len(self.grayscale.as_ref().map(|v| v.len())) * 4
            + len(self.rgba.as_ref().map(|v| v.len()))
            + len(self.level_index.as_ref().map(|v| v.len())) * 2
            + len(self.levels.as_ref().map(|v| v.len())) * 4
    }
    fn map_gray(&self, to_gray: impl Fn(f32) -> u8 + Sync) -> RenderImage {
        let (w, h) = (self.cols as usize, self.rows as usize);
        if self.has_level_index()
            && let (Some(levels), Some(index)) = (&self.levels, &self.level_index)
        {
            let table: Vec<u8> = levels.iter().map(|v| to_gray(*v)).collect();
            let pixels = index
                .iter()
                .map(|l| table.get(*l as usize).copied().unwrap_or(0))
                .collect();
            return RenderImage::gray(w, h, pixels);
        }
        let values = self.grayscale.clone().unwrap_or_default();
        RenderImage::gray(
            w,
            h,
            rows_of(w, h, |y, out| {
                for (o, v) in out.iter_mut().zip(&values[y * w..(y + 1) * w]) {
                    *o = to_gray(*v);
                }
            }),
        )
    }
    fn map_gray16(&self, to_gray16: impl Fn(f32) -> u16 + Sync) -> Gray16 {
        let (w, h) = (self.cols as usize, self.rows as usize);
        if self.has_level_index()
            && let (Some(levels), Some(index)) = (&self.levels, &self.level_index)
        {
            let table: Vec<u16> = levels.iter().map(|v| to_gray16(*v)).collect();
            let pixels = index
                .iter()
                .map(|l| table.get(*l as usize).copied().unwrap_or(0))
                .collect();
            return Gray16 {
                width: w,
                height: h,
                pixels,
            };
        }
        let values = self.grayscale.clone().unwrap_or_default();
        Gray16 {
            width: w,
            height: h,
            pixels: rows_of(w, h, |y, out| {
                for (o, v) in out.iter_mut().zip(&values[y * w..(y + 1) * w]) {
                    *o = to_gray16(*v);
                }
            }),
        }
    }
}
fn rows_of<T: Clone + Default + Send + Sync>(
    w: usize,
    h: usize,
    row: impl Fn(usize, &mut [T]) + Sync,
) -> Vec<T> {
    let slots: Vec<OnceLock<Vec<T>>> = (0..h).map(|_| OnceLock::new()).collect();
    parallel_rows(h as i64, |y| {
        let mut out = vec![T::default(); w];
        row(y as usize, &mut out);
        let _ = slots[y as usize].set(out);
    });
    slots
        .into_iter()
        .flat_map(|s| s.into_inner().unwrap_or_else(|| vec![T::default(); w]))
        .collect()
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gray16 {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u16>,
}
fn decode_cache_frame(slice: &Frame) -> Result<DecodedFrame, RenderError> {
    let slice = load_frame(Some(slice))?;
    if !slice.preserve_modality_precision && slice.metadata.samples_per_pixel == 1 {
        return decode_compact_grayscale_frame(&slice);
    }
    if !slice.preserve_modality_precision || slice.metadata.samples_per_pixel != 1 {
        return decode_slice_frame(&slice);
    }
    let metadata = effective_frame_metadata(&slice);
    let (rows, cols) = (i64::from(metadata.rows), i64::from(metadata.columns));
    let photometric = normalized_photometric(&metadata.photometric_interpretation);
    let frame = grayscale_display_frame(&slice, &metadata, &photometric, rows, cols)?;
    let values = display::decode_modality(&frame).map_err(display_err)?;
    Ok(DecodedFrame {
        rows,
        cols,
        photometric,
        grayscale64: Some(Arc::new(values)),
        ..Default::default()
    })
}
fn decode_compact_grayscale_frame(slice: &Frame) -> Result<DecodedFrame, RenderError> {
    let metadata = effective_frame_metadata(slice);
    let (rows, cols) = (i64::from(metadata.rows), i64::from(metadata.columns));
    if rows <= 0 || cols <= 0 || (metadata.bits_allocated != 8 && metadata.bits_allocated != 16) {
        return decode_slice_frame(slice);
    }
    let photometric = normalized_photometric(&metadata.photometric_interpretation);
    let frame = grayscale_display_frame(slice, &metadata, &photometric, rows, cols)?;
    match display::decode_modality_levels(&frame).map_err(display_err)? {
        None => decode_slice_frame(slice),
        Some(levels) => Ok(DecodedFrame {
            rows,
            cols,
            photometric,
            level_index: Some(Arc::new(levels.index)),
            levels: Some(Arc::new(levels.levels)),
            ..Default::default()
        }),
    }
}
pub(crate) fn decode_slice_frame(slice: &Frame) -> Result<DecodedFrame, RenderError> {
    let slice = load_frame(Some(slice))?;
    let metadata = effective_frame_metadata(&slice);
    let (rows, cols) = (i64::from(metadata.rows), i64::from(metadata.columns));
    if rows <= 0 || cols <= 0 {
        return Err(frame_error(
            "invalid frame metadata",
            format!("Rows={} Columns={}", metadata.rows, metadata.columns),
        ));
    }
    let photometric = normalized_photometric(&metadata.photometric_interpretation);
    match metadata.samples_per_pixel {
        1 => {
            let frame = grayscale_display_frame(&slice, &metadata, &photometric, rows, cols)?;
            let values = display::decode_modality_f32(&frame).map_err(display_err)?;
            Ok(DecodedFrame {
                rows,
                cols,
                photometric,
                grayscale: Some(Arc::new(values)),
                ..Default::default()
            })
        }
        3 => Ok(DecodedFrame {
            rows,
            cols,
            photometric,
            rgba: Some(Arc::new(decode_color_rgba(&slice)?)),
            ..Default::default()
        }),
        spp => Err(frame_error(
            "unsupported samples per pixel",
            format!("SamplesPerPixel={spp}"),
        )),
    }
}
fn index_decoded_levels(decoded: &mut DecodedFrame, slice: &Frame) {
    let Some(gray) = decoded.grayscale.clone() else {
        return;
    };
    if gray.is_empty() || gray.len() as i64 != decoded.rows * decoded.cols {
        return;
    }
    let (slope, intercept) = sanitized_rescale(slice.rescale);
    let inverse = 1.0 / slope;
    let level = |v: f32| ((f64::from(v) - intercept) * inverse).round();
    let (mut lowest, mut highest) = (gray[0], gray[0]);
    for v in &gray[1..] {
        if *v < lowest {
            lowest = *v;
        } else if *v > highest {
            highest = *v;
        }
    }
    let (mut first, mut last) = (level(lowest), level(highest));
    if first > last {
        std::mem::swap(&mut first, &mut last);
    }
    let span = last - first + 1.0;
    if span.is_nan() || span > MAX_DECODED_LEVELS || span > gray.len() as f64 {
        return;
    }
    let mut levels = vec![0f32; span as usize];
    let mut seen = vec![false; levels.len()];
    let mut index = vec![0u16; gray.len()];
    for (i, v) in gray.iter().enumerate() {
        let offset = level(*v) - first;
        if !(offset >= 0.0 && offset < span) {
            return;
        }
        let k = offset as usize;
        if seen[k] {
            if levels[k].to_bits() != v.to_bits() {
                return;
            }
        } else {
            seen[k] = true;
            levels[k] = *v;
        }
        index[i] = k as u16;
    }
    decoded.level_index = Some(Arc::new(index));
    decoded.levels = Some(Arc::new(levels));
}
fn voi_mapper(window: &WindowLevel) -> VoiByteMapper {
    VoiByteMapper::new(VoiLut {
        center: window.center,
        width: window.width,
        function: window.function,
        lut: window.lut.clone(),
        explanation: String::new(),
    })
}
fn render_decoded_frame(
    decoded: &DecodedFrame,
    window: &WindowLevel,
) -> Result<RenderImage, RenderError> {
    if let Some(rgba) = &decoded.rgba {
        return Ok(RenderImage::Rgba {
            width: decoded.cols as usize,
            height: decoded.rows as usize,
            pixels: rgba.as_ref().clone(),
        });
    }
    if !decoded.has_grayscale() {
        return Err(missing_grayscale());
    }
    let invert = decoded.photometric == "MONOCHROME1";
    let mapper = voi_mapper(window);
    let gray = |value: f64| {
        let g = mapper.byte(value);
        if invert { 255 - g } else { g }
    };
    if let Some(values) = &decoded.grayscale64 {
        return Ok(map_gray64(decoded, values, gray));
    }
    Ok(decoded.map_gray(|v| gray(f64::from(v))))
}
fn map_gray64(
    decoded: &DecodedFrame,
    values: &[f64],
    to_gray: impl Fn(f64) -> u8 + Sync,
) -> RenderImage {
    let (w, h) = (decoded.cols as usize, decoded.rows as usize);
    RenderImage::gray(
        w,
        h,
        rows_of(w, h, |y, out| {
            for (o, v) in out.iter_mut().zip(&values[y * w..(y + 1) * w]) {
                *o = to_gray(*v);
            }
        }),
    )
}
pub(crate) fn normalized_render_window(slice: Option<&Frame>, window: &WindowLevel) -> WindowLevel {
    if window.lut.is_some() {
        return window.clone();
    }
    let mut window = window.clone();
    if (window.width <= 0.0 || window.width.is_nan() || window.width.is_infinite())
        && let Some(slice) = slice
    {
        window = slice.default_window.clone();
    }
    if window.width <= 0.0 {
        window = WindowLevel::new(DEFAULT_WINDOW_CENTER, DEFAULT_WINDOW_WIDTH);
    }
    window
}
fn cache_float_bits(v: f64) -> u64 {
    if v == 0.0 { 0 } else { v.to_bits() }
}
fn image_cache_bytes(img: &RenderImage) -> i64 {
    match img {
        RenderImage::Gray { pixels, .. } | RenderImage::Rgba { pixels, .. } => pixels.len() as i64,
        RenderImage::Nil => 0,
    }
}
#[derive(Clone)]
#[allow(dead_code)]
enum Keeper {
    Frame(Arc<Frame>),
    Identity(Arc<FrameIdentity>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FrameKey {
    Frame(usize),
    Identity(usize),
}
fn frame_key(frame: &Arc<Frame>) -> (FrameKey, Keeper) {
    match &frame.cache_identity {
        Some(identity) => (
            FrameKey::Identity(Arc::as_ptr(identity) as usize),
            Keeper::Identity(identity.clone()),
        ),
        None => (
            FrameKey::Frame(Arc::as_ptr(frame) as usize),
            Keeper::Frame(frame.clone()),
        ),
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WindowKey {
    slice: FrameKey,
    center_bits: u64,
    width_bits: u64,
    function: u32,
    lut: Option<(i64, i64, Vec<u16>)>,
}
fn window_key(slice: FrameKey, window: &WindowLevel) -> WindowKey {
    WindowKey {
        slice,
        center_bits: cache_float_bits(window.center),
        width_bits: cache_float_bits(window.width),
        function: window.function as u32,
        lut: window
            .lut
            .as_ref()
            .map(|l| (l.first_mapped, l.bits_per_entry, l.entries.clone())),
    }
}
struct DecodedEntry {
    frame: Arc<DecodedFrame>,
    bytes: i64,
    window_misses: i64,
    levels_tried: bool,
    tick: u64,
    _keep: Keeper,
}
struct WindowEntry {
    image: RenderImage,
    err: Option<RenderError>,
    bytes: i64,
    tick: u64,
    _keep: Keeper,
}
#[derive(Default)]
struct CacheState {
    generation: u64,
    tick: u64,
    max_decoded_bytes: i64,
    decoded_bytes: i64,
    decoded: HashMap<FrameKey, DecodedEntry>,
    max_window_bytes: i64,
    window_bytes: i64,
    windows: HashMap<WindowKey, WindowEntry>,
    pinned_decoded: HashMap<FrameKey, Keeper>,
    pinned_window: HashMap<WindowKey, Keeper>,
    hits: u64,
    misses: u64,
    evictions: u64,
}
impl CacheState {
    fn next_tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }
    fn evict_decoded_to_limit(&mut self) {
        while self.decoded_bytes > self.max_decoded_bytes {
            let victim = self
                .decoded
                .iter()
                .filter(|(k, _)| !self.pinned_decoded.contains_key(k))
                .min_by_key(|(_, e)| e.tick)
                .map(|(k, _)| *k);
            let Some(key) = victim else { return };
            self.remove_decoded(&key);
        }
    }
    fn evict_window_to_limit(&mut self) {
        while self.window_bytes > self.max_window_bytes {
            let victim = self
                .windows
                .iter()
                .filter(|(k, _)| !self.pinned_window.contains_key(k))
                .min_by_key(|(_, e)| e.tick)
                .map(|(k, _)| k.clone());
            let Some(key) = victim else { return };
            self.remove_window(&key);
        }
    }
    fn remove_decoded(&mut self, key: &FrameKey) {
        if let Some(entry) = self.decoded.remove(key) {
            self.decoded_bytes -= entry.bytes;
            self.evictions += 1;
        }
    }
    fn remove_window(&mut self, key: &WindowKey) {
        if let Some(entry) = self.windows.remove(key) {
            self.window_bytes -= entry.bytes;
            self.evictions += 1;
        }
    }
}
#[derive(Clone)]
pub struct PinnedFrame {
    pub frame: Option<Arc<Frame>>,
    pub window: WindowLevel,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub decoded_bytes: i64,
    pub window_bytes: i64,
    pub pinned_bytes: i64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}
pub struct RenderCache {
    state: Mutex<CacheState>,
}
impl RenderCache {
    pub fn new(max_bytes: i64) -> RenderCache {
        let max_bytes = if max_bytes <= 0 {
            DEFAULT_RENDER_CACHE_BYTES
        } else {
            max_bytes
        };
        RenderCache {
            state: Mutex::new(CacheState {
                max_decoded_bytes: max_bytes,
                max_window_bytes: max_bytes,
                ..Default::default()
            }),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, CacheState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn cached_window_image(&self, key: &WindowKey) -> Option<(RenderImage, Option<RenderError>)> {
        let mut s = self.lock();
        let tick = s.next_tick();
        let entry = s.windows.get_mut(key)?;
        entry.tick = tick;
        Some((entry.image.clone(), entry.err.clone()))
    }
    fn record_result(&self, hit: bool) {
        let mut s = self.lock();
        if hit {
            s.hits += 1;
        } else {
            s.misses += 1;
        }
    }
    fn generation(&self) -> u64 {
        self.lock().generation
    }
    fn decoded_at(
        &self,
        slice: &Arc<Frame>,
        generation: u64,
    ) -> Result<Arc<DecodedFrame>, RenderError> {
        let (key, keep) = frame_key(slice);
        {
            let mut s = self.lock();
            let tick = s.next_tick();
            if let Some(entry) = s.decoded.get_mut(&key) {
                entry.tick = tick;
                return Ok(entry.frame.clone());
            }
        }
        let decoded = Arc::new(decode_cache_frame(slice)?);
        self.store_decoded(key, keep, decoded.clone(), generation);
        Ok(decoded)
    }
    fn store_decoded(
        &self,
        key: FrameKey,
        keep: Keeper,
        decoded: Arc<DecodedFrame>,
        generation: u64,
    ) {
        let bytes = decoded.cache_bytes();
        let mut s = self.lock();
        if generation != s.generation {
            return;
        }
        if bytes > s.max_decoded_bytes && !s.pinned_decoded.contains_key(&key) {
            return;
        }
        let tick = s.next_tick();
        if let Some(entry) = s.decoded.get_mut(&key) {
            let old = entry.bytes;
            entry.frame = decoded;
            entry.bytes = bytes;
            entry.window_misses = 0;
            entry.levels_tried = false;
            entry.tick = tick;
            s.decoded_bytes += bytes - old;
        } else {
            s.decoded.insert(
                key,
                DecodedEntry {
                    frame: decoded,
                    bytes,
                    window_misses: 0,
                    levels_tried: false,
                    tick,
                    _keep: keep,
                },
            );
            s.decoded_bytes += bytes;
        }
        s.evict_decoded_to_limit();
    }
    fn level_indexed_frame(
        &self,
        slice: &Arc<Frame>,
        decoded: Arc<DecodedFrame>,
    ) -> Arc<DecodedFrame> {
        if decoded.grayscale.is_none() || decoded.levels.is_some() {
            return decoded;
        }
        let (key, _) = frame_key(slice);
        {
            let mut s = self.lock();
            let Some(entry) = s.decoded.get_mut(&key) else {
                return decoded;
            };
            if !Arc::ptr_eq(&entry.frame, &decoded) || entry.levels_tried {
                let current = entry.frame.clone();
                return if current.levels.is_some() {
                    current
                } else {
                    decoded
                };
            }
            entry.window_misses += 1;
            if entry.window_misses < LEVEL_INDEX_WINDOW_MISSES {
                return decoded;
            }
            entry.levels_tried = true;
        }
        let mut indexed = decoded.as_ref().clone();
        index_decoded_levels(&mut indexed, slice);
        if indexed.levels.is_none() {
            return decoded;
        }
        let indexed = Arc::new(indexed);
        let mut s = self.lock();
        let mut delta = None;
        if let Some(entry) = s.decoded.get_mut(&key)
            && Arc::ptr_eq(&entry.frame, &decoded)
        {
            let bytes = indexed.cache_bytes();
            delta = Some(bytes - entry.bytes);
            entry.frame = indexed.clone();
            entry.bytes = bytes;
        }
        if let Some(delta) = delta {
            s.decoded_bytes += delta;
            s.evict_decoded_to_limit();
        }
        indexed
    }
    fn store_window_image(
        &self,
        key: WindowKey,
        keep: Keeper,
        img: &RenderImage,
        err: &Option<RenderError>,
        generation: u64,
    ) {
        let bytes = image_cache_bytes(img);
        let mut s = self.lock();
        if generation != s.generation {
            return;
        }
        if bytes > s.max_window_bytes && !s.pinned_window.contains_key(&key) {
            return;
        }
        let tick = s.next_tick();
        if let Some(entry) = s.windows.get_mut(&key) {
            let old = entry.bytes;
            entry.image = img.clone();
            entry.err = err.clone();
            entry.bytes = bytes;
            entry.tick = tick;
            s.window_bytes += bytes - old;
        } else {
            s.windows.insert(
                key,
                WindowEntry {
                    image: img.clone(),
                    err: err.clone(),
                    bytes,
                    tick,
                    _keep: keep,
                },
            );
            s.window_bytes += bytes;
        }
        s.evict_window_to_limit();
    }
    pub fn render_frame(
        &self,
        slice: Option<&Arc<Frame>>,
        window: &WindowLevel,
    ) -> (RenderImage, Option<RenderError>) {
        let Some(slice) = slice else {
            return (
                blank_image(512, 512),
                Some(RenderError::other("render: nil slice")),
            );
        };
        let blank = || {
            blank_image(
                i64::from(slice.metadata.columns),
                i64::from(slice.metadata.rows),
            )
        };
        if let Some(e) = &slice.decode_err {
            return (blank(), Some(e.clone()));
        }
        let generation = self.generation();
        let window = normalized_render_window(Some(slice), window);
        let (fkey, keep) = frame_key(slice);
        let key = window_key(fkey, &window);
        if let Some((img, err)) = self.cached_window_image(&key) {
            self.record_result(true);
            return (img, err);
        }
        self.record_result(false);
        let decoded = match self.decoded_at(slice, generation) {
            Ok(d) => d,
            Err(e) => return (blank(), Some(e)),
        };
        let decoded = self.level_indexed_frame(slice, decoded);
        let (img, err) = match render_decoded_frame(&decoded, &window) {
            Ok(img) => (img, None),
            Err(e) => (blank(), Some(e)),
        };
        self.store_window_image(key, keep, &img, &err, generation);
        (img, err)
    }
    pub fn contains_frame(&self, slice: Option<&Arc<Frame>>, window: &WindowLevel) -> bool {
        let Some(slice) = slice else { return false };
        let window = normalized_render_window(Some(slice), window);
        let key = window_key(frame_key(slice).0, &window);
        self.cached_window_image(&key).is_some()
    }
    pub fn lookup_frame(
        &self,
        slice: Option<&Arc<Frame>>,
        window: &WindowLevel,
    ) -> Option<(RenderImage, Option<RenderError>)> {
        let slice = slice?;
        let window = normalized_render_window(Some(slice), window);
        let key = window_key(frame_key(slice).0, &window);
        let found = self.cached_window_image(&key);
        if found.is_some() {
            self.record_result(true);
        }
        found
    }
    pub fn set_pinned_frames(&self, frames: &[PinnedFrame]) {
        let mut decoded = HashMap::new();
        let mut windowed = HashMap::new();
        for pinned in frames {
            let Some(frame) = &pinned.frame else { continue };
            let (key, keep) = frame_key(frame);
            decoded.insert(key, keep.clone());
            let window = normalized_render_window(Some(frame), &pinned.window);
            windowed.insert(window_key(key, &window), keep);
        }
        let mut s = self.lock();
        s.pinned_decoded = decoded;
        s.pinned_window = windowed;
        s.evict_decoded_to_limit();
        s.evict_window_to_limit();
    }
    pub fn evict_non_pinned(&self) -> i64 {
        let mut s = self.lock();
        let before = s.decoded_bytes + s.window_bytes;
        let decoded: Vec<FrameKey> = s
            .decoded
            .keys()
            .filter(|k| !s.pinned_decoded.contains_key(k))
            .copied()
            .collect();
        for k in decoded {
            s.remove_decoded(&k);
        }
        let windows: Vec<WindowKey> = s
            .windows
            .keys()
            .filter(|k| !s.pinned_window.contains_key(k))
            .cloned()
            .collect();
        for k in windows {
            s.remove_window(&k);
        }
        before - s.decoded_bytes - s.window_bytes
    }
    pub fn stats(&self) -> CacheStats {
        let s = self.lock();
        let mut stats = CacheStats {
            decoded_bytes: s.decoded_bytes,
            window_bytes: s.window_bytes,
            hits: s.hits,
            misses: s.misses,
            evictions: s.evictions,
            pinned_bytes: 0,
        };
        for (k, e) in &s.decoded {
            if s.pinned_decoded.contains_key(k) {
                stats.pinned_bytes += e.bytes;
            }
        }
        for (k, e) in &s.windows {
            if s.pinned_window.contains_key(k) {
                stats.pinned_bytes += e.bytes;
            }
        }
        stats
    }
    pub fn clear(&self) {
        let mut s = self.lock();
        s.generation += 1;
        s.decoded_bytes = 0;
        s.decoded.clear();
        s.window_bytes = 0;
        s.windows.clear();
        s.pinned_decoded.clear();
        s.pinned_window.clear();
    }
    pub fn render_frame_png(
        &self,
        slice: Option<&Arc<Frame>>,
        window: &WindowLevel,
    ) -> Result<Vec<u8>, RenderError> {
        let (img, err) = self.render_frame(slice, window);
        if let Some(e) = err {
            return Err(e);
        }
        png_of(&img)
    }
    pub fn render_thumbnail(&self, series: Option<&Stack>) -> RenderImage {
        let Some(series) = series else {
            return blank_image(128, 128);
        };
        match self.render_frame(series.first_display_frame(), &series.default_window) {
            (img, None) if img != RenderImage::Nil => img,
            _ => blank_image(128, 128),
        }
    }
}
pub(crate) fn png_of(img: &RenderImage) -> Result<Vec<u8>, RenderError> {
    let (w, h) = img.size();
    let source = match img {
        RenderImage::Gray { pixels, .. } => PngSource::Gray(pixels),
        RenderImage::Rgba { pixels, .. } => PngSource::Rgba(pixels),
        RenderImage::Nil => return Err(RenderError::other("png: nil image")),
    };
    encode_png(source, w, h).map_err(|e| RenderError::other(e.to_string()))
}
fn decoded_of(
    cache: Option<&RenderCache>,
    slice: &Arc<Frame>,
) -> Result<Arc<DecodedFrame>, RenderError> {
    match cache {
        Some(c) => c.decoded_at(slice, c.generation()),
        None => decode_cache_frame(slice).map(Arc::new),
    }
}
pub fn render_frame_gray16(
    cache: Option<&RenderCache>,
    slice: Option<&Arc<Frame>>,
    window: &WindowLevel,
) -> Result<Gray16, RenderError> {
    let Some(slice) = slice else {
        return Err(RenderError::other("render: nil slice"));
    };
    if let Some(e) = &slice.decode_err {
        return Err(e.clone());
    }
    let window = normalized_render_window(Some(slice), window);
    let decoded = decoded_of(cache, slice)?;
    if decoded.rgba.is_some() {
        return Err(not_grayscale(&decoded.photometric));
    }
    if !decoded.has_grayscale() {
        return Err(missing_grayscale());
    }
    let invert = decoded.photometric == "MONOCHROME1";
    let invalid = window.center.is_nan() || window.width.is_nan();
    let mapper = voi_mapper(&window);
    let p = |value: f64| {
        let mut p = 0u16;
        if !invalid && !value.is_nan() {
            p = (mapper.unit(value) * 65535.0 + 0.5) as u16;
        }
        if invert { 65535 - p } else { p }
    };
    if let Some(values) = &decoded.grayscale64 {
        let (w, h) = (decoded.cols as usize, decoded.rows as usize);
        return Ok(Gray16 {
            width: w,
            height: h,
            pixels: rows_of(w, h, |y, out| {
                for (o, v) in out.iter_mut().zip(&values[y * w..(y + 1) * w]) {
                    *o = p(*v);
                }
            }),
        });
    }
    Ok(decoded.map_gray16(|v| p(f64::from(v))))
}
pub fn render_frame_remapped(
    cache: Option<&RenderCache>,
    slice: Option<&Arc<Frame>>,
    window: &WindowLevel,
    remap: Option<&(dyn Fn(f64) -> f64 + Sync)>,
) -> Result<RenderImage, RenderError> {
    let Some(slice) = slice else {
        return Err(RenderError::other("render: nil slice"));
    };
    if let Some(e) = &slice.decode_err {
        return Err(e.clone());
    }
    let window = normalized_render_window(Some(slice), window);
    let decoded = decoded_of(cache, slice)?;
    if decoded.rgba.is_some() {
        return Err(not_grayscale(&decoded.photometric));
    }
    if !decoded.has_grayscale() {
        return Err(missing_grayscale());
    }
    let invert = decoded.photometric == "MONOCHROME1";
    let mapper = voi_mapper(&window);
    let gray = |mut value: f64| {
        if let Some(remap) = remap {
            value = remap(value);
        }
        let mut g = 0;
        if !value.is_nan() {
            g = mapper.byte(value);
        }
        if invert { 255 - g } else { g }
    };
    if let Some(values) = &decoded.grayscale64 {
        return Ok(map_gray64(&decoded, values, gray));
    }
    Ok(decoded.map_gray(|v| gray(f64::from(v))))
}
pub fn render_frame_values(
    cache: Option<&RenderCache>,
    slice: Option<&Arc<Frame>>,
    window: &WindowLevel,
    values: &[f32],
) -> Result<RenderImage, RenderError> {
    let Some(slice) = slice else {
        return Err(RenderError::other("render: nil slice"));
    };
    if let Some(e) = &slice.decode_err {
        return Err(e.clone());
    }
    if slice.preserve_modality_precision {
        return Err(RenderError::new(
            ErrorKind::ModalityPrecisionRequired,
            ERR_MODALITY_PRECISION_REQUIRED,
        ));
    }
    let window = normalized_render_window(Some(slice), window);
    let decoded = decoded_of(cache, slice)?;
    if decoded.rgba.is_some() {
        return Err(not_grayscale(&decoded.photometric));
    }
    if decoded.grayscale.is_none() && !decoded.has_level_index() {
        return Err(missing_grayscale());
    }
    let (w, h) = (decoded.cols as usize, decoded.rows as usize);
    if values.len() != w * h {
        return Err(RenderError::other(format!(
            "render: {} values for a {}\u{d7}{} frame",
            values.len(),
            decoded.cols,
            decoded.rows
        )));
    }
    let invert = decoded.photometric == "MONOCHROME1";
    let mapper = voi_mapper(&window);
    Ok(RenderImage::gray(
        w,
        h,
        rows_of(w, h, |y, out| {
            for (o, v) in out.iter_mut().zip(&values[y * w..(y + 1) * w]) {
                let mut g = 0;
                if !v.is_nan() {
                    g = mapper.byte(f64::from(*v));
                }
                *o = if invert { 255 - g } else { g };
            }
        }),
    ))
}
pub fn frame_modality_values(slice: Option<&Frame>) -> Result<Vec<f32>, RenderError> {
    let Some(slice) = slice else {
        return Err(RenderError::other("render: nil slice"));
    };
    if let Some(e) = &slice.decode_err {
        return Err(e.clone());
    }
    if slice.preserve_modality_precision {
        return Err(RenderError::new(
            ErrorKind::ModalityPrecisionRequired,
            ERR_MODALITY_PRECISION_REQUIRED,
        ));
    }
    let decoded = decode_slice_frame(slice)?;
    if decoded.rgba.is_some() {
        return Err(not_grayscale(&decoded.photometric));
    }
    match decoded.grayscale {
        Some(g) => Ok(g.as_ref().clone()),
        None => Err(missing_grayscale()),
    }
}
