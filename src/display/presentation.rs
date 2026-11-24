use super::transform::Image;
use super::voi::{clamp, nearest};
use super::{DisplayError, Frame, FrameFault, Lut, VoiByteMapper};
use crate::fmath;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gray16Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
#[derive(Debug)]
pub struct GrayView<'a> {
    pub pixels: &'a mut [u8],
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}
#[derive(Debug)]
pub struct Gray16View<'a> {
    pub pixels: &'a mut [u8],
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}
impl VoiByteMapper {
    pub fn unit(&self, mut value: f64) -> f64 {
        if let Some(lut) = &self.voi.lut {
            value = f64::from(lut.lookup(nearest(value)));
        }
        if self.range_span > 0.0 {
            return clamp((value - self.range_min) / self.range_span, 0.0, 1.0);
        }
        self.voi.window_unit(value)
    }
}
pub fn render_gray16(frame: &Frame<'_>) -> Result<Gray16Image, DisplayError> {
    if frame.format.bits_allocated != 8 && frame.format.bits_allocated != 16 {
        return Err(DisplayError::frame(
            "BitsAllocated",
            FrameFault::UnsupportedBitsAllocated,
        ));
    }
    let values = super::decode_modality(frame)?;
    let reader = frame.format.reader();
    let mapper = super::frame_voi_mapper(&frame.voi, &frame.modality, &reader);
    let pixels = values
        .iter()
        .flat_map(|&v| {
            let p = fmath::f64_to_i32((mapper.unit(v) * 65535.0).round()) as u16;
            p.to_be_bytes()
        })
        .collect();
    Ok(Gray16Image {
        width: frame.columns as usize,
        height: frame.rows as usize,
        pixels,
    })
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Presentation {
    pub lut: Option<Lut>,
    pub lin_od: bool,
    pub invert: bool,
    pub shutter: Option<Shutter>,
    pub overlays: Vec<Overlay>,
}
const OVERLAY_BURN_VALUE: u8 = 255;
impl Presentation {
    pub fn apply(&self, image: &mut GrayView<'_>) {
        if image.width == 0 || image.height == 0 {
            return;
        }
        if self.lut.is_some() || self.invert {
            for y in 0..image.height {
                let start = y * image.stride;
                let row = &mut image.pixels[start..start + image.width];
                if let Some(lut) = &self.lut {
                    for p in row.iter_mut() {
                        *p = lut.entry_byte(lut.lookup(i64::from(*p)));
                    }
                }
                if self.invert {
                    for p in row.iter_mut() {
                        *p = 255 - *p;
                    }
                }
            }
        }
        if let Some(shutter) = &self.shutter {
            shutter.apply(image);
        }
        for overlay in &self.overlays {
            overlay.composite(image, OVERLAY_BURN_VALUE);
        }
    }
    pub fn apply_gray16(&self, image: &mut Gray16View<'_>) {
        let (width, height, stride) = (image.width, image.height, image.stride);
        let offset = |x: usize, y: usize| y * stride + 2 * x;
        for y in 0..height {
            for x in 0..width {
                let i = offset(x, y);
                let mut value = u16::from_be_bytes([image.pixels[i], image.pixels[i + 1]]);
                if let Some(lut) = self.lut.as_ref().filter(|l| !l.entries.is_empty()) {
                    let index = nearest(f64::from(value) * (lut.entries.len() - 1) as f64 / 65535.0)
                        as usize;
                    value = lut.entry16(lut.entries[index]);
                }
                if self.invert {
                    value = 65535 - value;
                }
                image.pixels[i..i + 2].copy_from_slice(&value.to_be_bytes());
            }
        }
        if let Some(s) = self.shutter.as_ref().filter(|s| s.has_shapes()) {
            let masked = if s.presentation_p_value_defined {
                s.presentation_p_value
            } else {
                u16::from(s.presentation_value) * 257
            };
            for row in 0..height {
                for col in 0..width {
                    if !s.visible(row as i64, col as i64) {
                        let i = offset(col, row);
                        image.pixels[i..i + 2].copy_from_slice(&masked.to_be_bytes());
                    }
                }
            }
        }
        for overlay in &self.overlays {
            let (row_lo, row_hi) =
                overlay_span(overlay.origin_row, overlay.data_rows(), height as i64);
            let (col_lo, col_hi) = overlay_span(overlay.origin_col, overlay.columns, width as i64);
            for r in row_lo..row_hi {
                for c in col_lo..col_hi {
                    if !overlay.is_set(r, c) {
                        continue;
                    }
                    let x = overlay.origin_col.wrapping_sub(1).wrapping_add(c) as usize;
                    let y = overlay.origin_row.wrapping_sub(1).wrapping_add(r) as usize;
                    let i = offset(x, y);
                    image.pixels[i] = 0xFF;
                    image.pixels[i + 1] = 0xFF;
                }
            }
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shutter {
    pub presentation_value: u8,
    pub presentation_p_value: u16,
    pub presentation_p_value_defined: bool,
    pub presentation_color_cielab: [u16; 3],
    pub presentation_color_cielab_defined: bool,
    pub rectangle: Option<RectShutter>,
    pub circle: Option<CircleShutter>,
    pub polygon: Option<PolygonShutter>,
    pub bitmap: Option<BitmapShutter>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RectShutter {
    pub left: i64,
    pub right: i64,
    pub upper: i64,
    pub lower: i64,
}
impl RectShutter {
    fn inside(&self, row: i64, col: i64) -> bool {
        let (c, r) = (col.wrapping_add(1), row.wrapping_add(1));
        c >= self.left && c <= self.right && r >= self.upper && r <= self.lower
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CircleShutter {
    pub center_row: i64,
    pub center_col: i64,
    pub radius: i64,
}
impl CircleShutter {
    fn inside(&self, row: i64, col: i64) -> bool {
        let dr = abs_diff(row.wrapping_add(1), self.center_row);
        let dc = abs_diff(col.wrapping_add(1), self.center_col);
        let radius = abs_diff(self.radius, 0);
        if dr > radius || dc > radius {
            return false;
        }
        let (dr, dc, radius) = (u128::from(dr), u128::from(dc), u128::from(radius));
        dr * dr + dc * dc <= radius * radius
    }
}
fn abs_diff(a: i64, b: i64) -> u64 {
    if a >= b {
        (a as u64).wrapping_sub(b as u64)
    } else {
        (b as u64).wrapping_sub(a as u64)
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PolygonShutter {
    pub vertices: Vec<ShutterPoint>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShutterPoint {
    pub row: i64,
    pub col: i64,
}
impl PolygonShutter {
    fn inside(&self, row: i64, col: i64) -> bool {
        let v = &self.vertices;
        if v.len() < 3 {
            return false;
        }
        let r = row.wrapping_add(1) as f64;
        let c = col.wrapping_add(1) as f64;
        let mut inside = false;
        let mut j = v.len() - 1;
        for i in 0..v.len() {
            let (ri, ci) = (v[i].row as f64, v[i].col as f64);
            let (rj, cj) = (v[j].row as f64, v[j].col as f64);
            if (ri > r) != (rj > r) {
                let cross = (cj - ci) * (r - ri) / (rj - ri) + ci;
                if c < cross {
                    inside = !inside;
                }
            }
            j = i;
        }
        inside
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BitmapShutter {
    pub group: u16,
    pub overlay: Overlay,
}
impl BitmapShutter {
    fn masks(&self, row: i64, col: i64) -> bool {
        let o = &self.overlay;
        let overlay_row = row.wrapping_sub(o.origin_row.wrapping_sub(1));
        let overlay_col = col.wrapping_sub(o.origin_col.wrapping_sub(1));
        if overlay_row < 0 || overlay_row >= o.rows || overlay_col < 0 || overlay_col >= o.columns {
            return false;
        }
        o.is_set(overlay_row, overlay_col)
    }
}
impl Shutter {
    fn has_shapes(&self) -> bool {
        self.rectangle.is_some()
            || self.circle.is_some()
            || self.polygon.is_some()
            || self.bitmap.is_some()
    }
    fn visible(&self, row: i64, col: i64) -> bool {
        if self.rectangle.is_some_and(|r| !r.inside(row, col)) {
            return false;
        }
        if self.circle.is_some_and(|c| !c.inside(row, col)) {
            return false;
        }
        if self.polygon.as_ref().is_some_and(|p| !p.inside(row, col)) {
            return false;
        }
        if self.bitmap.as_ref().is_some_and(|b| b.masks(row, col)) {
            return false;
        }
        true
    }
    pub fn visible_source_pixel(&self, x: i64, y: i64) -> bool {
        self.visible(y, x)
    }
    pub fn apply(&self, image: &mut GrayView<'_>) {
        if !self.has_shapes() {
            return;
        }
        for row in 0..image.height {
            for col in 0..image.width {
                if !self.visible(row as i64, col as i64) {
                    image.pixels[row * image.stride + col] = self.presentation_value;
                }
            }
        }
    }
    pub fn apply_to<'a>(&self, src: &'a Image) -> std::borrow::Cow<'a, Image> {
        use std::borrow::Cow;
        if !self.has_shapes() {
            return Cow::Borrowed(src);
        }
        if let Image::Gray(gray) = src {
            let mut out = gray.clone();
            self.apply(&mut GrayView {
                stride: out.width,
                width: out.width,
                height: out.height,
                pixels: &mut out.pixels,
            });
            return Cow::Owned(Image::Gray(out));
        }
        let mut out = src.to_rgba();
        let v = self.presentation_value;
        for row in 0..out.height {
            for col in 0..out.width {
                if !self.visible(row as i64, col as i64) {
                    let i = 4 * (row * out.width + col);
                    out.pixels[i..i + 4].copy_from_slice(&[v, v, v, 255]);
                }
            }
        }
        Cow::Owned(Image::Rgba(out))
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overlay {
    pub rows: i64,
    pub columns: i64,
    pub origin_row: i64,
    pub origin_col: i64,
    pub data: Vec<u8>,
    pub group: u16,
    pub overlay_type: String,
    pub subtype: String,
    pub label: String,
    pub description: String,
    pub frames: i64,
    pub frame_origin: i64,
    pub embedded: bool,
    pub bit_position: i64,
    pub has_roi_statistics: bool,
    pub roi_area: i64,
    pub roi_mean: f64,
    pub roi_standard_deviation: f64,
}
impl Overlay {
    fn is_set(&self, row: i64, col: i64) -> bool {
        if row < 0 || col < 0 || col >= self.columns || row >= self.data_rows() {
            return false;
        }
        let idx = row.wrapping_mul(self.columns).wrapping_add(col);
        if idx >> 3 >= self.data.len() as i64 {
            return false;
        }
        self.data[(idx >> 3) as usize] & (1 << (idx & 7)) != 0
    }
    fn data_rows(&self) -> i64 {
        if self.rows <= 0 || self.columns <= 0 {
            return 0;
        }
        let columns = self.columns as u64;
        let rows = ((self.data.len() as u64)
            .wrapping_mul(8)
            .wrapping_add(columns)
            - 1)
            / columns;
        if rows < self.rows as u64 {
            rows as i64
        } else {
            self.rows
        }
    }
    pub fn composite(&self, image: &mut GrayView<'_>, value: u8) {
        let (width, height, stride) = (image.width, image.height, image.stride);
        if width == 0
            || height == 0
            || stride < width
            || image.pixels.len() < (height - 1) * stride + width
        {
            return;
        }
        let (row_lo, row_hi) = overlay_span(self.origin_row, self.data_rows(), height as i64);
        let (col_lo, col_hi) = overlay_span(self.origin_col, self.columns, width as i64);
        if row_lo >= row_hi || col_lo >= col_hi {
            return;
        }
        let data_bits = self.data.len() as i64 * 8;
        for r in row_lo..row_hi {
            let start = self.origin_row.wrapping_sub(1).wrapping_add(r) as usize * stride;
            let row_bit = r.wrapping_mul(self.columns);
            for c in col_lo..col_hi {
                let idx = row_bit.wrapping_add(c);
                if idx >= data_bits {
                    break;
                }
                if self.data[(idx >> 3) as usize] & (1 << (idx & 7)) != 0 {
                    let x = self.origin_col.wrapping_sub(1).wrapping_add(c) as usize;
                    image.pixels[start + x] = value;
                }
            }
        }
    }
}
fn overlay_span(origin: i64, extent: i64, size: i64) -> (i64, i64) {
    if extent <= 0 || size <= 0 || origin > size {
        return (0, 0);
    }
    if origin >= 1 {
        return (0, extent.min(size - (origin - 1)));
    }
    if origin <= 1 - extent {
        return (0, 0);
    }
    let lo = 1 - origin;
    if size >= extent - lo {
        return (lo, extent);
    }
    (lo, lo + size)
}
