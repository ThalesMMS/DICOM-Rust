use super::compound::select_graphic_annotations;
use super::glyphs::BASIC_GLYPHS;
use super::model::{
    has_anchor, normalized_justification, resolved_graphic_layers, validate_presentation_lut,
    validate_text_object,
};
use super::{
    ANNOTATION_UNITS_DISPLAY, CompoundGraphic, CompoundLineStyle, CompoundTextStyle, CompoundTick,
    DisplayedArea, Error, GRAPHIC_TYPE_CIRCLE, GRAPHIC_TYPE_ELLIPSE, GRAPHIC_TYPE_INTERPOLATED,
    GRAPHIC_TYPE_POINT, GraphicAnnotation, GraphicLayer, ImageState, JUSTIFY_CENTER, JUSTIFY_LEFT,
    JUSTIFY_RIGHT, Kind, PRESENTATION_LUT_INVERSE, Point2D, SpatialGeometry, State,
    active_overlays, annotation_point_in_pixel_units, closed_polyline, displayed_area_defined,
    displayed_area_pixel_edges, graphic_layer_name, invalid, normalized_annotation_units,
    normalized_graphic_type_or_polyline, references_apply, select_displayed_area,
    select_softcopy_voi, softcopy_voi_defined, validate_annotation_point,
};
use crate::deid::trim_space;
use crate::dicomcolor;
use crate::display::{Frame, GrayImage, GrayView, Overlay, VoiLut, render_gray, render_gray16};
use crate::fmath::{self, f64_to_i64};
use std::collections::HashMap;
const MAX_RENDER_PIXELS: i64 = 1 << 26;
const TILE_SIDE: i64 = 32;
const TILE_AREA: usize = (TILE_SIDE * TILE_SIDE) as usize;
const COORDINATE_LIMIT: f64 = (1u64 << 30) as f64;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}
impl Rect {
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Rect {
        Rect {
            min_x: x0.min(x1),
            min_y: y0.min(y1),
            max_x: x0.max(x1),
            max_y: y0.max(y1),
        }
    }
    pub fn dx(&self) -> i64 {
        self.max_x.wrapping_sub(self.min_x)
    }
    pub fn dy(&self) -> i64 {
        self.max_y.wrapping_sub(self.min_y)
    }
    pub fn empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    fn contains(&self, x: i64, y: i64) -> bool {
        self.min_x <= x && x < self.max_x && self.min_y <= y && y < self.max_y
    }
    fn add(&self, x: i64, y: i64) -> Rect {
        Rect {
            min_x: self.min_x.wrapping_add(x),
            min_y: self.min_y.wrapping_add(y),
            max_x: self.max_x.wrapping_add(x),
            max_y: self.max_y.wrapping_add(y),
        }
    }
    fn intersect(&self, s: &Rect) -> Rect {
        let r = Rect {
            min_x: self.min_x.max(s.min_x),
            min_y: self.min_y.max(s.min_y),
            max_x: self.max_x.min(s.max_x),
            max_y: self.max_y.min(s.max_y),
        };
        if r.empty() { Rect::default() } else { r }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Raster {
    pub rect: Rect,
    pub channels: usize,
    pub pixels: Vec<u8>,
}
impl Raster {
    pub fn gray(rect: Rect) -> Raster {
        Raster::new(rect, 1)
    }
    pub fn nrgba(rect: Rect) -> Raster {
        Raster::new(rect, 4)
    }
    fn new(rect: Rect, channels: usize) -> Raster {
        let n = if rect.empty() {
            0
        } else {
            (rect.dx() * rect.dy()) as usize * channels
        };
        Raster {
            rect: if rect.empty() { Rect::default() } else { rect },
            channels,
            pixels: vec![0; n],
        }
    }
    fn offset(&self, x: i64, y: i64) -> usize {
        (((y - self.rect.min_y) * self.rect.dx() + (x - self.rect.min_x)) as usize) * self.channels
    }
    fn gray_at(&self, x: i64, y: i64) -> u8 {
        if !self.rect.contains(x, y) {
            return 0;
        }
        self.pixels[self.offset(x, y)]
    }
    fn set_gray(&mut self, x: i64, y: i64, v: u8) {
        if self.rect.contains(x, y) {
            let o = self.offset(x, y);
            self.pixels[o] = v;
        }
    }
    fn nrgba_at(&self, x: i64, y: i64) -> [u8; 4] {
        if !self.rect.contains(x, y) {
            return [0; 4];
        }
        let o = self.offset(x, y);
        [
            self.pixels[o],
            self.pixels[o + 1],
            self.pixels[o + 2],
            self.pixels[o + 3],
        ]
    }
    fn set_nrgba(&mut self, x: i64, y: i64, c: [u8; 4]) {
        if self.rect.contains(x, y) {
            let o = self.offset(x, y);
            self.pixels[o..o + 4].copy_from_slice(&c);
        }
    }
}
pub trait Font {
    fn line_height(&self) -> i64;
    fn width(&self, line: &str) -> i64;
    fn draw(&self, dst: &mut Raster, x: i64, y: i64, line: &str, value: u8);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BasicFont {
    scale: i64,
}
pub fn basic_font(scale: i64) -> BasicFont {
    BasicFont {
        scale: scale.max(1),
    }
}
impl Font for BasicFont {
    fn line_height(&self) -> i64 {
        9 * self.scale
    }
    fn width(&self, line: &str) -> i64 {
        let n = line.chars().count() as i64;
        if n == 0 {
            return 0;
        }
        (6 * n - 1) * self.scale
    }
    fn draw(&self, dst: &mut Raster, x: i64, y: i64, line: &str, value: u8) {
        let mut x = x;
        let s = self.scale;
        for r in line.chars() {
            let r = if (r as u32) < 0x20 || (r as u32) > 0x7E {
                '?'
            } else {
                r
            };
            let glyph = BASIC_GLYPHS[(r as usize) - 0x20];
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..5 {
                    if bits & (0x10 >> col) == 0 {
                        continue;
                    }
                    for dy in 0..s {
                        for dx in 0..s {
                            let px = x.wrapping_add(col * s).wrapping_add(dx);
                            let py = y.wrapping_add(row as i64 * s).wrapping_add(dy);
                            dst.set_gray(px, py, value);
                        }
                    }
                }
            }
            x = x.wrapping_add(6 * s);
        }
    }
}
#[derive(Default)]
pub struct RenderOptions<'a> {
    pub whole_image: bool,
    pub no_annotations: bool,
    pub image_overlays: Vec<Overlay>,
    pub font: Option<&'a dyn Font>,
}
#[derive(Clone, Copy, Debug)]
struct RenderGeometry {
    g: SpatialGeometry,
    crop: Rect,
    area: [Point2D; 2],
}
fn new_render_geometry(
    width: i64,
    height: i64,
    state: &State,
    target: &ImageState,
    whole: bool,
) -> Result<RenderGeometry, Error> {
    match state.spatial_transform.rotation_degrees {
        0 | 90 | 180 | 270 => {}
        r => {
            return Err(invalid(format!(
                "image rotation {r} is not 0, 90, 180 or 270"
            )));
        }
    }
    let g = SpatialGeometry::new(width, height, state.spatial_transform);
    let (mut left, mut top, mut right, mut bottom) = (0.0, 0.0, width as f64, height as f64);
    let area = select_displayed_area(state, target);
    if displayed_area_defined(&area) {
        (left, top, right, bottom) = displayed_area_pixel_edges(&area, target)?;
    }
    let a = g.rotate(Point2D::new(left, top));
    let b = g.rotate(Point2D::new(right, bottom));
    let area_rect = Rect::new(
        f64_to_i64(fmath::min(a.x, b.x)),
        f64_to_i64(fmath::min(a.y, b.y)),
        f64_to_i64(fmath::max(a.x, b.x)),
        f64_to_i64(fmath::max(a.y, b.y)),
    );
    let mut crop = area_rect;
    if whole {
        crop = if g.rotation == 90 || g.rotation == 270 {
            Rect::new(0, 0, height, width)
        } else {
            Rect::new(0, 0, width, height)
        };
    }
    if crop.empty()
        || crop.dx().wrapping_mul(crop.dy()) > MAX_RENDER_PIXELS
        || crop.dx() > MAX_RENDER_PIXELS
        || crop.dy() > MAX_RENDER_PIXELS
    {
        return Err(invalid(format!(
            "rendered size {}x{}",
            crop.dx(),
            crop.dy()
        )));
    }
    let (ox, oy) = (crop.min_x as f64, crop.min_y as f64);
    Ok(RenderGeometry {
        g,
        crop,
        area: [
            Point2D::new(area_rect.min_x as f64 - ox, area_rect.min_y as f64 - oy),
            Point2D::new(area_rect.max_x as f64 - ox, area_rect.max_y as f64 - oy),
        ],
    })
}
impl RenderGeometry {
    fn place(&self, p: Point2D, units: &str) -> Point2D {
        if normalized_annotation_units(units) == ANNOTATION_UNITS_DISPLAY {
            return Point2D::new(
                self.area[0].x + p.x * (self.area[1].x - self.area[0].x),
                self.area[0].y + p.y * (self.area[1].y - self.area[0].y),
            );
        }
        let p = self.g.rotate(p);
        Point2D::new(p.x - self.crop.min_x as f64, p.y - self.crop.min_y as f64)
    }
    fn transform(&self, src: &GrayImage, pad: u8) -> Raster {
        let mut out = Raster::gray(Rect::new(0, 0, self.crop.dx(), self.crop.dy()));
        if pad != 0 {
            out.pixels.fill(pad);
        }
        let (w, h) = (out.rect.dx(), out.rect.dy());
        for row in 0..self.g.height {
            for col in 0..self.g.width {
                let p = self
                    .g
                    .rotate(Point2D::new(col as f64 + 0.5, row as f64 + 0.5));
                let x = f64_to_i64(p.x.floor()) - self.crop.min_x;
                let y = f64_to_i64(p.y.floor()) - self.crop.min_y;
                if x >= 0 && y >= 0 && x < w && y < h {
                    out.pixels[(y * w + x) as usize] =
                        src.pixels[row as usize * src.width + col as usize];
                }
            }
        }
        out
    }
}
pub fn render(
    frame: &Frame<'_>,
    state: Option<&State>,
    target: &ImageState,
    opts: &RenderOptions<'_>,
) -> Result<GrayImage, Error> {
    let Some(state) = state else {
        return Err(invalid("state is nil"));
    };
    let mut target = target.clone();
    if target.rows == 0 && target.columns == 0 {
        target.rows = frame.rows;
        target.columns = frame.columns;
    }
    if target.rows != frame.rows || target.columns != frame.columns {
        return Err(invalid(format!(
            "image is {}x{} but the frame is {}x{}",
            target.columns, target.rows, frame.columns, frame.rows
        )));
    }
    if !target.sop_instance_uid.is_empty()
        && !state.referenced_images.is_empty()
        && !references_apply(&state.referenced_images, &target)
    {
        return Err(Error::new(
            Kind::MissingReference,
            format!("image {}", target.sop_instance_uid),
        ));
    }
    let layers = resolved_graphic_layers(state)?;
    validate_presentation_lut(state.presentation_lut.as_ref())?;
    let geometry =
        new_render_geometry(frame.columns, frame.rows, state, &target, opts.whole_image)?;
    let mut frame = frame.clone();
    if let Some(m) = &state.modality_lut {
        frame.modality = m.transform();
    }
    frame.voi = VoiLut::default();
    let voi = select_softcopy_voi(state, &target);
    if softcopy_voi_defined(&voi) {
        frame.voi = VoiLut {
            center: voi.window_center,
            width: voi.window_width,
            function: voi.function,
            lut: voi.lut,
            explanation: String::new(),
        };
    }
    let mut pvalues = presentation_values(&frame, state)?;
    let mut layer_values: HashMap<String, u8> = HashMap::new();
    for layer in &layers {
        layer_values.insert(
            layer.name.clone(),
            layer.recommended_grayscale.map_or(255, |g| (g >> 8) as u8),
        );
    }
    let frame_number = target.frame_number.max(1);
    let mut active_layer: HashMap<u16, String> = HashMap::new();
    for activation in &state.overlay_activations {
        active_layer.insert(activation.group, graphic_layer_name(&activation.layer));
    }
    {
        let (w, h) = (pvalues.width, pvalues.height);
        let mut view = GrayView {
            pixels: &mut pvalues.pixels,
            stride: w,
            width: w,
            height: h,
        };
        for overlay in active_overlays(Some(state), &opts.image_overlays) {
            if let Some(plane) = overlay.for_image_frame(frame_number - 1) {
                let value = active_layer
                    .get(&overlay.group)
                    .and_then(|name| layer_values.get(name))
                    .copied()
                    .unwrap_or(0);
                plane.composite(&mut view, value);
            }
        }
        if let Some(shutter) = &state.shutter {
            shutter.apply(&mut view);
        }
    }
    let pad = state.shutter.as_ref().map_or(0, |s| s.presentation_value);
    let mut out = geometry.transform(&pvalues, pad);
    if !opts.no_annotations {
        let default_font;
        let font: &dyn Font = match opts.font {
            Some(f) => f,
            None => {
                default_font = basic_font(1 + out.rect.dx().min(out.rect.dy()) / 512);
                &default_font
            }
        };
        let mut canvas = Canvas::new(&mut out, false);
        let place =
            |p: Point2D, units: &str| -> Result<Point2D, Error> { Ok(geometry.place(p, units)) };
        draw_selected_annotations(&mut canvas, state, &target, &layers, &place, font)?;
    }
    Ok(GrayImage {
        width: out.rect.dx() as usize,
        height: out.rect.dy() as usize,
        pixels: out.pixels,
    })
}
fn presentation_values(frame: &Frame<'_>, state: &State) -> Result<GrayImage, Error> {
    let Some(lut) = &state.presentation_lut else {
        let mut out = render_gray(frame).map_err(Error::other)?;
        if trim_space(&state.presentation_lut_shape).eq_ignore_ascii_case(PRESENTATION_LUT_INVERSE)
        {
            for p in &mut out.pixels {
                *p = 255 - *p;
            }
        }
        return Ok(out);
    };
    let wide = render_gray16(frame).map_err(Error::other)?;
    let last = (lut.entries.len() as f64) - 1.0;
    let top = 2f64.powi(lut.bits_per_entry as i32) - 1.0;
    let n = wide.width * wide.height;
    let mut pixels = vec![0u8; n];
    for (i, p) in pixels.iter_mut().enumerate() {
        let value = f64::from(u16::from_be_bytes([
            wide.pixels[2 * i],
            wide.pixels[2 * i + 1],
        ]));
        let entry = f64::from(lut.entries[(value * last / 65535.0).floor() as usize]);
        *p = (fmath::min(entry, top) * 255.0 / top).round() as u8;
    }
    Ok(GrayImage {
        width: wide.width,
        height: wide.height,
        pixels,
    })
}
struct Canvas<'a> {
    dst: &'a mut Raster,
    overlay: bool,
    ink: [u8; 4],
    budget: i64,
    limited: bool,
    exhausted: Option<Error>,
}
impl<'a> Canvas<'a> {
    fn new(dst: &'a mut Raster, overlay: bool) -> Canvas<'a> {
        Canvas {
            dst,
            overlay,
            ink: [0; 4],
            budget: 128 << 20,
            limited: overlay,
            exhausted: None,
        }
    }
    fn bounds(&self) -> Rect {
        self.dst.rect
    }
    fn reserve(&mut self, cost: i64) -> bool {
        if !self.limited {
            return true;
        }
        if self.exhausted.is_none() {
            self.exhausted = spend(&mut self.budget, cost).err();
        }
        self.exhausted.is_none()
    }
    fn set(&mut self, x: i64, y: i64, value: u8) {
        if self.dst.rect.contains(x, y) {
            if self.overlay {
                blend_pixel(self.dst, x, y, self.ink);
            } else {
                self.dst.set_gray(x, y, value);
            }
        }
    }
    fn draw_graphic(&mut self, graphic_type: &str, points: &[Point2D], filled: bool, value: u8) {
        let (outline, closed): (Vec<Point2D>, bool) = match graphic_type {
            GRAPHIC_TYPE_POINT => {
                for p in points {
                    self.cross(*p, 2, value);
                }
                return;
            }
            GRAPHIC_TYPE_CIRCLE => {
                if points.len() < 2 {
                    return;
                }
                let r = fmath::hypot(points[1].x - points[0].x, points[1].y - points[0].y);
                (ellipse_points(points[0], r, r, 0.0), true)
            }
            GRAPHIC_TYPE_ELLIPSE => {
                if points.len() < 4 {
                    return;
                }
                let center = Point2D::new(
                    (points[0].x + points[1].x) / 2.0,
                    (points[0].y + points[1].y) / 2.0,
                );
                let a = fmath::hypot(points[1].x - points[0].x, points[1].y - points[0].y) / 2.0;
                let b = fmath::hypot(points[3].x - points[2].x, points[3].y - points[2].y) / 2.0;
                let angle = fmath::atan2(points[1].y - points[0].y, points[1].x - points[0].x);
                (ellipse_points(center, a, b, angle), true)
            }
            GRAPHIC_TYPE_INTERPOLATED => (catmull_rom(points), closed_polyline(points)),
            _ => (points.to_vec(), closed_polyline(points)),
        };
        if filled && closed {
            self.fill(&outline, value);
        }
        for i in 1..outline.len() {
            self.line(outline[i - 1], outline[i], value);
        }
        if outline.len() == 1 {
            self.set(
                f64_to_i64(outline[0].x.floor()),
                f64_to_i64(outline[0].y.floor()),
                value,
            );
        }
    }
    fn line(&mut self, from: Point2D, to: Point2D, value: u8) {
        let b = self.bounds();
        let Some((a, bb)) = clip_segment(
            from,
            to,
            b.min_x as f64 - 1.0,
            b.min_y as f64 - 1.0,
            b.max_x as f64 + 1.0,
            b.max_y as f64 + 1.0,
        ) else {
            return;
        };
        let (mut x0, mut y0) = (f64_to_i64(a.x.floor()), f64_to_i64(a.y.floor()));
        let (x1, y1) = (f64_to_i64(bb.x.floor()), f64_to_i64(bb.y.floor()));
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        if !self.reserve(dx.max(-dy) + 1) {
            return;
        }
        let (sx, sy) = ((x1 - x0).signum(), (y1 - y0).signum());
        let mut e = dx + dy;
        loop {
            self.set(x0, y0, value);
            if x0 == x1 && y0 == y1 {
                return;
            }
            let e2 = 2 * e;
            if e2 >= dy {
                e += dy;
                x0 += sx;
            } else {
                e += dx;
                y0 += sy;
            }
        }
    }
    fn cross(&mut self, p: Point2D, arm: i64, value: u8) {
        let arm = arm as f64;
        self.line(
            Point2D::new(p.x - arm, p.y),
            Point2D::new(p.x + arm, p.y),
            value,
        );
        self.line(
            Point2D::new(p.x, p.y - arm),
            Point2D::new(p.x, p.y + arm),
            value,
        );
    }
    fn fill(&mut self, polygon: &[Point2D], value: u8) {
        let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in polygon {
            min_y = fmath::min(min_y, p.y);
            max_y = fmath::max(max_y, p.y);
        }
        let b = self.bounds();
        let first = b.min_y.max(f64_to_i64(min_y.floor()));
        let last = (b.max_y - 1).min(f64_to_i64(max_y.ceil()));
        let mut xs: Vec<f64> = Vec::new();
        let mut y = first;
        while y <= last {
            if !self.reserve(polygon.len() as i64) {
                return;
            }
            let cy = y as f64 + 0.5;
            xs.clear();
            for i in 0..polygon.len() {
                let (a, bb) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                if (a.y <= cy) != (bb.y <= cy) {
                    xs.push(a.x + (cy - a.y) * (bb.x - a.x) / (bb.y - a.y));
                }
            }
            sort_float64s(&mut xs);
            let mut i = 0;
            while i + 1 < xs.len() {
                let from = b.min_x.max(f64_to_i64((xs[i] - 0.5).ceil()));
                let to = (b.max_x - 1).min(f64_to_i64((xs[i + 1] - 0.5).floor()));
                if !self.reserve(0.max(to - from + 1)) {
                    return;
                }
                let mut x = from;
                while x <= to {
                    self.set(x, y, value);
                    x += 1;
                }
                i += 2;
            }
            y += 1;
        }
    }
    fn text(&mut self, font: &dyn Font, x: i64, y: i64, line: &str, value: u8) {
        const AROUND: [(i64, i64); 8] = [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ];
        if self.overlay {
            let mask = glyph_mask(font, line, false, false, false);
            let ink = self.ink;
            if !mask.rect.empty() {
                if !self.reserve(9 * mask.rect.dx() * mask.rect.dy()) {
                    return;
                }
                for d in AROUND {
                    blend_glyph(self.dst, &mask, x + d.0, y + d.1, [0, 0, 0, 255]);
                }
                blend_glyph(self.dst, &mask, x, y, ink);
            }
            return;
        }
        let outline = if value < 128 { 255 } else { 0 };
        for d in AROUND {
            font.draw(
                self.dst,
                x.wrapping_add(d.0),
                y.wrapping_add(d.1),
                line,
                outline,
            );
        }
        font.draw(self.dst, x, y, line, value);
    }
}
fn sort_float64s(xs: &mut [f64]) {
    xs.sort_by(|a, b| match (a.is_nan(), b.is_nan()) {
        (true, true) => std::cmp::Ordering::Equal,
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
    });
}
fn ellipse_points(center: Point2D, a: f64, b: f64, angle: f64) -> Vec<Point2D> {
    let mut steps = f64_to_i64(fmath::min(
        1024.0,
        fmath::max(16.0, 2.0 * std::f64::consts::PI * fmath::max(a, b) / 2.0),
    ));
    if a.is_nan() || b.is_nan() {
        steps = 16;
    }
    let (sin, cos) = (fmath::sin(angle), fmath::cos(angle));
    let mut out = Vec::with_capacity(steps.max(0) as usize + 1);
    for i in 0..=steps {
        let t = 2.0 * std::f64::consts::PI * ((i % steps) as f64) / steps as f64;
        let (x, y) = (a * fmath::cos(t), b * fmath::sin(t));
        out.push(Point2D::new(
            center.x + x * cos - y * sin,
            center.y + x * sin + y * cos,
        ));
    }
    out
}
fn catmull_rom(points: &[Point2D]) -> Vec<Point2D> {
    if points.len() < 3 {
        return points.to_vec();
    }
    const STEPS: i64 = 8;
    let n = points.len() as i64;
    let at = |i: i64| points[i.clamp(0, n - 1) as usize];
    let mut out = Vec::with_capacity(((n - 1) * STEPS + 1) as usize);
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for s in 0..STEPS {
            let t = s as f64 / STEPS as f64;
            let (t2, t3) = (t * t, t * t * t);
            let blend = |a: f64, b: f64, c: f64, d: f64| {
                0.5 * (2.0 * b
                    + (c - a) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (3.0 * b - a - 3.0 * c + d) * t3)
            };
            out.push(Point2D::new(
                blend(p0.x, p1.x, p2.x, p3.x),
                blend(p0.y, p1.y, p2.y, p3.y),
            ));
        }
    }
    out.push(points[points.len() - 1]);
    out
}
fn clip_segment(
    a: Point2D,
    b: Point2D,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> Option<(Point2D, Point2D)> {
    let sum = a.x + a.y + b.x + b.y;
    if !sum.is_finite() {
        return None;
    }
    let (mut t0, mut t1) = (0.0, 1.0);
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    for (p, q) in [
        (-dx, a.x - min_x),
        (dx, max_x - a.x),
        (-dy, a.y - min_y),
        (dy, max_y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            t0 = fmath::max(t0, r);
        } else {
            t1 = fmath::min(t1, r);
        }
        if t0 > t1 {
            return None;
        }
    }
    Some((
        Point2D::new(a.x + t0 * dx, a.y + t0 * dy),
        Point2D::new(a.x + t1 * dx, a.y + t1 * dy),
    ))
}
fn printable(line: &str) -> String {
    line.chars()
        .map(|r| {
            let c = r as u32;
            if c < 0x20 || c == 0x7F || (0x80..0xA0).contains(&c) {
                ' '
            } else {
                r
            }
        })
        .collect()
}
type Projector<'p> = dyn Fn(Point2D, &str) -> Result<Point2D, Error> + 'p;
fn draw_simple_annotation(
    c: &mut Canvas<'_>,
    item: &GraphicAnnotation,
    value: u8,
    font: &dyn Font,
    place: &Projector<'_>,
) -> Result<(), Error> {
    if !item.polyline.is_empty() {
        let mut points = Vec::with_capacity(item.polyline.len());
        for p in &item.polyline {
            validate_annotation_point(*p, &item.graphic_units)?;
            points.push(place(*p, &item.graphic_units)?);
        }
        c.draw_graphic(
            &normalized_graphic_type_or_polyline(&item.graphic_type),
            &points,
            item.filled,
            value,
        );
    }
    if item.text.is_empty() {
        return Ok(());
    }
    validate_text_object(item)?;
    let text = item.text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut anchor = None;
    if has_anchor(item) {
        anchor = Some(place(item.anchor, &item.anchor_units)?);
    }
    let (left, top, right);
    let mut justification = JUSTIFY_LEFT.to_string();
    if let Some(bx) = &item.bounding_box {
        let a = place(bx.top_left, &bx.units)?;
        let b = place(bx.bottom_right, &bx.units)?;
        left = fmath::min(a.x, b.x);
        top = fmath::min(a.y, b.y);
        right = fmath::max(a.x, b.x);
        justification = normalized_justification(&bx.justification);
        if let Some(anchor) = anchor
            && item.anchor_visible
        {
            let bottom = fmath::max(a.y, b.y);
            let nearest = Point2D::new(
                fmath::max(left, fmath::min(anchor.x, right)),
                fmath::max(top, fmath::min(anchor.y, bottom)),
            );
            c.line(anchor, nearest, value);
        }
    } else {
        let anchor = anchor.unwrap_or_default();
        left = anchor.x + 2.0;
        top = anchor.y + 2.0;
        right = left;
        if item.anchor_visible {
            c.cross(anchor, 2, value);
        }
    }
    let mut y = f64_to_i64(top.floor());
    for line in lines {
        let line = printable(line);
        let width = font.width(&line);
        let x = match justification.as_str() {
            JUSTIFY_RIGHT => f64_to_i64(right.floor()).wrapping_sub(width),
            JUSTIFY_CENTER => f64_to_i64(((left + right) / 2.0).floor()).wrapping_sub(width / 2),
            _ => f64_to_i64(left.floor()),
        };
        c.text(font, x, y, &line, value);
        y = y.wrapping_add(font.line_height());
    }
    Ok(())
}
fn draw_selected_annotations(
    c: &mut Canvas<'_>,
    state: &State,
    target: &ImageState,
    layers: &[GraphicLayer],
    place: &Projector<'_>,
    font: &dyn Font,
) -> Result<(), Error> {
    let mut by_layer: HashMap<String, Vec<GraphicAnnotation>> = HashMap::new();
    for item in select_graphic_annotations(state, target) {
        by_layer
            .entry(graphic_layer_name(&item.layer_name))
            .or_default()
            .push(item);
    }
    for layer in layers {
        let value = layer.recommended_grayscale.map_or(255, |g| (g >> 8) as u8);
        let mut base = [value, value, value, 255];
        if c.overlay {
            base = compound_color(layer.recommended_cielab, None, base);
        }
        c.ink = base;
        let Some(items) = by_layer.get(&layer.name) else {
            continue;
        };
        for item in items {
            let Some(compound) = &item.compound else {
                if !c.reserve(item.polyline.len() as i64 + item.text.len() as i64) {
                    return Err(c.exhausted.clone().unwrap_or_else(exceeded));
                }
                draw_simple_annotation(c, item, value, font, place)?;
                if let Some(e) = &c.exhausted {
                    return Err(e.clone());
                }
                continue;
            };
            let bounds = c.bounds();
            let graphic = project_compound(compound, place, bounds)?;
            let mut budget = c.budget;
            paint_compound_paths(
                c.dst,
                &graphic.paths,
                graphic.closed,
                compound,
                base,
                graphic.scale,
                &mut budget,
            )?;
            for label in &graphic.labels {
                draw_compound_tick_label(
                    c.dst,
                    label,
                    compound.text_style.as_ref(),
                    font,
                    base,
                    graphic.scale,
                    &mut budget,
                )?;
            }
            c.budget = budget;
        }
    }
    Ok(())
}
fn exceeded() -> Error {
    invalid("Compound Graphic paint work exceeds limit")
}
fn spend(budget: &mut i64, cost: i64) -> Result<(), Error> {
    if cost > *budget {
        return Err(exceeded());
    }
    *budget -= cost;
    Ok(())
}
pub fn render_annotation_overlay(
    state: Option<&State>,
    target: &ImageState,
    bounds: Rect,
    pixel_to_canvas: Option<&dyn Fn(Point2D) -> Point2D>,
) -> Result<Raster, Error> {
    const MAX_OVERLAY_PIXELS: i64 = 1 << 24;
    let (width, height) = (bounds.dx(), bounds.dy());
    let limit = COORDINATE_LIMIT as i64;
    let (Some(state), Some(pixel_to_canvas)) = (state, pixel_to_canvas) else {
        return Err(invalid("invalid annotation viewport"));
    };
    if target.rows <= 0
        || target.columns <= 0
        || width <= 0
        || height <= 0
        || width > MAX_OVERLAY_PIXELS / height
        || bounds.min_x < -limit
        || bounds.min_y < -limit
        || bounds.max_x > limit
        || bounds.max_y > limit
    {
        return Err(invalid("invalid annotation viewport"));
    }
    if !state.referenced_images.is_empty() && !references_apply(&state.referenced_images, target) {
        return Err(Error::new(
            Kind::MissingReference,
            "presentation state does not reference the displayed image",
        ));
    }
    if !matches!(state.spatial_transform.rotation_degrees, 0 | 90 | 180 | 270) {
        return Err(invalid("noncardinal GSPS image rotation"));
    }
    let layers = resolved_graphic_layers(state)?;
    let area: DisplayedArea = select_displayed_area(state, target);
    let transform = state.spatial_transform;
    let place = |p: Point2D, units: &str| -> Result<Point2D, Error> {
        let pixel = annotation_point_in_pixel_units(p, units, &area, target, transform)?;
        let mapped = pixel_to_canvas(pixel);
        if !safe_annotation_point(mapped) {
            return Err(invalid("nonfinite or oversized annotation projection"));
        }
        Ok(mapped)
    };
    let mut out = Raster::nrgba(bounds);
    {
        let mut canvas = Canvas::new(&mut out, true);
        let font = basic_font(8.min(1 + width.min(height) / 512));
        draw_selected_annotations(&mut canvas, state, target, &layers, &place, &font)?;
    }
    Ok(out)
}
fn draw_compound_tick_label(
    dst: &mut Raster,
    label: &TickLabel,
    style: Option<&CompoundTextStyle>,
    font: &dyn Font,
    fallback: [u8; 4],
    scale: f64,
    budget: &mut i64,
) -> Result<(), Error> {
    let mut ink = fallback;
    let mask = match style {
        None => glyph_mask(font, &label.text, false, false, false),
        Some(s) => {
            ink = compound_color(s.color, None, fallback);
            glyph_mask(
                font,
                &label.text,
                s.bold == Some(true),
                s.italic == Some(true),
                s.underlined == Some(true),
            )
        }
    };
    if mask.rect.empty() {
        return Ok(());
    }
    let x = f64_to_i64(label.anchor.x.floor()) - mask.rect.dx() / 2;
    let mut y = f64_to_i64(label.anchor.y.floor());
    if !label.below {
        y -= mask.rect.dy();
    }
    spend(budget, mask.rect.dx() * mask.rect.dy())?;
    if let Some(s) = style
        && (s.shadow == "NORMAL" || s.shadow == "OUTLINED")
    {
        let shadow = compound_color(s.shadow_color, s.shadow_opacity, [0, 0, 0, 255]);
        let dx = s.shadow_offset_x.unwrap_or(0.0) * scale;
        let dy = s.shadow_offset_y.unwrap_or(0.0) * scale;
        if !valid_paint_number(dx) || !valid_paint_number(dy) {
            return Err(invalid("invalid Compound Graphic tick shadow offset"));
        }
        if s.shadow == "NORMAL" {
            spend(budget, mask.rect.dx() * mask.rect.dy())?;
            blend_glyph(
                dst,
                &mask,
                x + f64_to_i64(dx.round()),
                y + f64_to_i64(dy.round()),
                shadow,
            );
        } else {
            blend_outline_glyph(dst, &mask, x, y, fmath::hypot(dx, dy), shadow, budget)?;
        }
    }
    blend_glyph(dst, &mask, x, y, ink);
    Ok(())
}
fn glyph_mask(font: &dyn Font, text: &str, bold: bool, italic: bool, underline: bool) -> Raster {
    const MAX_GLYPH_PIXELS: i64 = 1 << 20;
    let line = printable(&text.replace("\r\n", " "));
    let (width, height) = (font.width(&line), font.line_height());
    if width <= 0 || height <= 0 || width > MAX_GLYPH_PIXELS / height {
        return Raster::gray(Rect::default());
    }
    let mut base = Raster::gray(Rect::new(0, 0, width, height));
    font.draw(&mut base, 0, 0, &line, 255);
    if !bold && !italic && !underline {
        return base;
    }
    let mut extra = 0;
    if italic {
        extra = height / 4;
    }
    if bold {
        extra += 1;
    }
    if width + extra > MAX_GLYPH_PIXELS / height {
        return Raster::gray(Rect::default());
    }
    let stride = width + extra;
    let mut mask = Raster::gray(Rect::new(0, 0, stride, height));
    for row in 0..height {
        let shift = if italic { (height - row - 1) / 4 } else { 0 };
        for col in 0..width {
            if base.pixels[(row * width + col) as usize] == 0 {
                continue;
            }
            let offset = (row * stride + col + shift) as usize;
            mask.pixels[offset] = 255;
            if bold {
                mask.pixels[offset + 1] = 255;
            }
        }
    }
    if underline {
        for col in 0..width {
            mask.pixels[((height - 1) * stride + col) as usize] = 255;
        }
    }
    mask
}
fn blend_glyph(dst: &mut Raster, mask: &Raster, x: i64, y: i64, ink: [u8; 4]) {
    let visible = mask.rect.add(x, y).intersect(&dst.rect);
    for py in visible.min_y..visible.max_y {
        for px in visible.min_x..visible.max_x {
            if mask.gray_at(px - x, py - y) != 0 {
                blend_pixel(dst, px, py, ink);
            }
        }
    }
}
fn blend_outline_glyph(
    dst: &mut Raster,
    mask: &Raster,
    x: i64,
    y: i64,
    radius: f64,
    ink: [u8; 4],
    budget: &mut i64,
) -> Result<(), Error> {
    if radius <= 0.0 || ink[3] == 0 {
        return Ok(());
    }
    const MAX_OUTLINE: f64 = 64.0;
    if radius > MAX_OUTLINE {
        return Err(invalid("Compound Graphic tick outline exceeds limit"));
    }
    let r = f64_to_i64(radius.ceil());
    let expanded = Rect {
        min_x: mask.rect.min_x - r,
        min_y: mask.rect.min_y - r,
        max_x: mask.rect.max_x + r,
        max_y: mask.rect.max_y + r,
    };
    spend(budget, expanded.dx() * expanded.dy())?;
    let mut dilated = Raster::gray(expanded);
    for py in 0..mask.rect.dy() {
        for px in 0..mask.rect.dx() {
            if mask.gray_at(px, py) == 0 {
                continue;
            }
            spend(budget, (2 * r + 1) * (2 * r + 1))?;
            for dy in -r..=r {
                for dx in -r..=r {
                    if ((dx * dx + dy * dy) as f64) <= radius * radius {
                        dilated.set_gray(px + dx, py + dy, 255);
                    }
                }
            }
        }
    }
    blend_glyph(dst, &dilated, x, y, ink);
    Ok(())
}
pub(crate) struct TickLabel {
    anchor: Point2D,
    below: bool,
    text: String,
}
struct ProjectedCompound {
    paths: Vec<Vec<Point2D>>,
    closed: bool,
    scale: f64,
    labels: Vec<TickLabel>,
}
fn sum(p: Point2D, q: Point2D) -> Point2D {
    Point2D::new(p.x + q.x, p.y + q.y)
}
fn delta(p: Point2D, q: Point2D) -> Point2D {
    Point2D::new(p.x - q.x, p.y - q.y)
}
fn scale_point(p: Point2D, s: f64) -> Point2D {
    Point2D::new(p.x * s, p.y * s)
}
fn length(p: Point2D) -> f64 {
    fmath::hypot(p.x, p.y)
}
fn perpendicular(p: Point2D) -> Point2D {
    let l = length(p);
    if l == 0.0 {
        return Point2D::default();
    }
    Point2D::new(-p.y / l, p.x / l)
}
fn safe_annotation_point(p: Point2D) -> bool {
    const MAX: f64 = (1u64 << 29) as f64;
    p.x.abs() <= MAX && p.y.abs() <= MAX
}
fn project_compound(
    graphic: &CompoundGraphic,
    place: &Projector<'_>,
    bounds: Rect,
) -> Result<ProjectedCompound, Error> {
    let base = place(Point2D::default(), &graphic.units)?;
    let right = place(Point2D::new(1.0, 0.0), &graphic.units)?;
    let down = place(Point2D::new(0.0, 1.0), &graphic.units)?;
    let (x, y) = (delta(right, base), delta(down, base));
    let (width, height) = (length(x), length(y));
    let determinant = x.x * y.y - x.y * y.x;
    if !safe_annotation_point(base)
        || !safe_annotation_point(right)
        || !safe_annotation_point(down)
        || width == 0.0
        || height == 0.0
        || determinant == 0.0
    {
        return Err(invalid("invalid annotation projection"));
    }
    let display = normalized_annotation_units(&graphic.units) == ANNOTATION_UNITS_DISPLAY;
    let scale = fmath::min(width, height);
    let mapping = |p: Point2D| -> Point2D {
        let mut p = p;
        if let (Some(angle), Some(center)) = (graphic.rotation_angle, graphic.rotation_point) {
            let d = delta(p, center);
            let (x_scale, y_scale) = if display { (width, height) } else { (1.0, 1.0) };
            let radians = angle * std::f64::consts::PI / 180.0;
            let (sine, cosine) = (fmath::sin(radians), fmath::cos(radians));
            let (dx, dy) = (d.x * x_scale, d.y * y_scale);
            p = sum(
                center,
                Point2D::new(
                    (cosine * dx + sine * dy) / x_scale,
                    (-sine * dx + cosine * dy) / y_scale,
                ),
            );
        }
        sum(base, sum(scale_point(x, p.x), scale_point(y, p.y)))
    };
    let mut projected = ProjectedCompound {
        paths: Vec::new(),
        closed: false,
        scale,
        labels: Vec::new(),
    };
    let points: Vec<Point2D> = graphic.points.iter().map(|&p| mapping(p)).collect();
    let typ = crate::textfmt::to_upper(trim_space(&graphic.compound_type));
    let size = bounds.dx().min(bounds.dy()) as f64;
    let tick_length = fmath::min(12.0, fmath::max(3.0, size * 0.02));
    let handedness = 1f64.copysign(determinant);
    match typ.as_str() {
        "RECTANGLE" => {
            let (a, b) = (graphic.points[0], graphic.points[1]);
            projected.closed = true;
            projected.paths = vec![vec![
                points[0],
                mapping(Point2D::new(b.x, a.y)),
                points[1],
                mapping(Point2D::new(a.x, b.y)),
                points[0],
            ]];
        }
        "ELLIPSE" => {
            let (a, b) = (graphic.points[0], graphic.points[1]);
            const MAX_STEPS: f64 = 1024.0;
            let steps = f64_to_i64(fmath::min(
                MAX_STEPS,
                fmath::max(
                    16.0,
                    std::f64::consts::PI
                        * fmath::max((b.x - a.x).abs() * width, (b.y - a.y).abs() * height)
                        / 2.0,
                ),
            ));
            let center = scale_point(sum(a, b), 0.5);
            let radius = scale_point(delta(b, a), 0.5);
            let mut outline = Vec::with_capacity(steps as usize + 1);
            for i in 0..=steps {
                let t = ((i % steps) as f64) * (2.0 * std::f64::consts::PI) / steps as f64;
                let (sine, cosine) = (fmath::sin(t), fmath::cos(t));
                outline.push(mapping(sum(
                    center,
                    Point2D::new(cosine * radius.x, sine * radius.y),
                )));
            }
            projected.closed = true;
            projected.paths = vec![outline];
        }
        "MULTILINE" => {
            let mut i = 0;
            while i + 1 < points.len() {
                projected.paths.push(points[i..i + 2].to_vec());
                i += 2;
            }
        }
        "ARROW" => projected.paths = compound_arrow(points[1], points[0], tick_length),
        "RANGELINE" | "RULER" | "AXIS" => {
            projected.paths = vec![points.clone()];
            let mut ticks: Vec<CompoundTick> = graphic.major_ticks.clone();
            let mut alignment = graphic.tick_alignment.clone();
            let tick = |position: f64| CompoundTick {
                position,
                label: String::new(),
            };
            if typ == "RANGELINE" {
                ticks = vec![tick(0.0), tick(1.0)];
                alignment = "CENTER".into();
            }
            if typ == "RULER" {
                ticks = vec![tick(0.0), tick(0.25), tick(0.5), tick(0.75), tick(1.0)];
            }
            let axis = delta(points[1], points[0]);
            let n = scale_point(perpendicular(axis), handedness);
            for t in &ticks {
                let p = sum(points[0], scale_point(axis, t.position));
                let (left, right) = compound_tick(p, n, tick_length, &alignment);
                projected.paths.push(vec![left, right]);
                if graphic.show_tick_label == Some(true) && !t.label.is_empty() {
                    let below = graphic.tick_label_alignment.eq_ignore_ascii_case("BOTTOM");
                    let mut distance = tick_length + 2.0;
                    if !below {
                        distance = -distance;
                    }
                    projected.labels.push(TickLabel {
                        anchor: sum(p, scale_point(n, distance)),
                        below,
                        text: t.label.clone(),
                    });
                }
            }
        }
        "CROSSHAIR" => {
            let gap = graphic.gap_length.unwrap_or(0.0) * size / 2.0;
            let radius = graphic.diameter_of_visibility.unwrap_or(0.0) * size / 2.0;
            for direction in [
                Point2D::new(-1.0, 0.0),
                Point2D::new(1.0, 0.0),
                Point2D::new(0.0, -1.0),
                Point2D::new(0.0, 1.0),
            ] {
                let projected_direction =
                    delta(mapping(sum(graphic.points[0], direction)), points[0]);
                let l = length(projected_direction);
                if l == 0.0 {
                    return Err(invalid("collapsed crosshair projection"));
                }
                let unit = scale_point(projected_direction, 1.0 / l);
                projected.paths.push(vec![
                    sum(points[0], scale_point(unit, gap)),
                    sum(points[0], scale_point(unit, radius)),
                ]);
            }
        }
        "CUTLINE" | "INFINITELINE" => {
            let center = mapping(graphic.rotation_point.unwrap_or_default());
            let gap = graphic.gap_length.unwrap_or(0.0) * size / 2.0;
            let lines =
                compound_infinite_line(points[0], points[1], center, gap, bounds, typ == "CUTLINE");
            projected.paths = lines.clone();
            if typ == "CUTLINE" {
                let normal = scale_point(perpendicular(delta(points[1], points[0])), handedness);
                for line in &lines {
                    let head = scale_point(sum(line[0], line[1]), 0.5);
                    let foot = sum(head, scale_point(normal, tick_length * 2.0));
                    projected
                        .paths
                        .extend(compound_arrow(foot, head, tick_length * 0.65));
                }
            }
        }
        _ => {}
    }
    for path in &projected.paths {
        for p in path {
            if !safe_annotation_point(*p) {
                return Err(invalid("nonfinite or oversized compound projection"));
            }
        }
    }
    Ok(projected)
}
fn compound_tick(p: Point2D, normal: Point2D, size: f64, align: &str) -> (Point2D, Point2D) {
    match crate::textfmt::to_upper(trim_space(align)).as_str() {
        "TOP" => (p, sum(p, scale_point(normal, -size))),
        "BOTTOM" => (p, sum(p, scale_point(normal, size))),
        _ => (
            sum(p, scale_point(normal, -size / 2.0)),
            sum(p, scale_point(normal, size / 2.0)),
        ),
    }
}
fn compound_arrow(foot: Point2D, head: Point2D, len: f64) -> Vec<Vec<Point2D>> {
    let axis = delta(head, foot);
    let l = length(axis);
    if l <= 0.0 {
        return Vec::new();
    }
    let len = fmath::min(len, l / 2.0);
    let center = sum(head, scale_point(axis, -len / l));
    let wing = scale_point(perpendicular(axis), len / 2.0);
    vec![
        vec![foot, head],
        vec![delta(center, wing), head, sum(center, wing)],
    ]
}
fn compound_infinite_line(
    p: Point2D,
    q: Point2D,
    center: Point2D,
    radius: f64,
    bounds: Rect,
    split: bool,
) -> Vec<Vec<Point2D>> {
    let axis = delta(q, p);
    let l = length(axis);
    if l == 0.0 {
        return Vec::new();
    }
    let axis = scale_point(axis, 1.0 / l);
    let reach =
        1.0 + length(delta(
            p,
            Point2D::new(bounds.min_x as f64, bounds.min_y as f64),
        )) + fmath::hypot(bounds.dx() as f64, bounds.dy() as f64);
    let Some((lo, hi)) = clip_segment(
        sum(p, scale_point(axis, -reach)),
        sum(p, scale_point(axis, reach)),
        bounds.min_x as f64,
        bounds.min_y as f64,
        bounds.max_x as f64,
        bounds.max_y as f64,
    ) else {
        return Vec::new();
    };
    let offset = delta(center, lo);
    let along = offset.x * axis.x + offset.y * axis.y;
    let height = offset.x * axis.y - offset.y * axis.x;
    let full = length(delta(hi, lo));
    if height.abs() >= radius {
        if split && along > 0.0 && along < full {
            let c = sum(lo, scale_point(axis, along));
            return vec![vec![lo, c], vec![c, hi]];
        }
        return vec![vec![lo, hi]];
    }
    let gap = fmath::max(0.0, radius * radius - height * height).sqrt();
    let (start, end) = (along - gap, along + gap);
    if end <= 0.0 || start >= full {
        return vec![vec![lo, hi]];
    }
    let mut paths = Vec::with_capacity(2);
    if start > 0.0 {
        paths.push(vec![lo, sum(lo, scale_point(axis, start))]);
    }
    if end < full {
        paths.push(vec![sum(lo, scale_point(axis, end)), hi]);
    }
    paths
}
fn valid_paint_number(v: f64) -> bool {
    v.abs() <= COORDINATE_LIMIT
}
fn compound_color(encoded: Option<[u16; 3]>, opacity: Option<f64>, fallback: [u8; 4]) -> [u8; 4] {
    let mut ink = fallback;
    if let Some(e) = encoded {
        let rgb = dicomcolor::cielab_to_rgb(e);
        ink = [rgb[0] as u8, rgb[1] as u8, rgb[2] as u8, 255];
    }
    if let Some(o) = opacity {
        ink[3] = (255.0 * fmath::max(0.0, fmath::min(1.0, o))).round() as u8;
    }
    ink
}
fn gray_of(r: u8, g: u8, b: u8) -> u8 {
    let (r, g, b) = (
        u32::from(r) * 0x101,
        u32::from(g) * 0x101,
        u32::from(b) * 0x101,
    );
    ((19595 * r + 38470 * g + 7471 * b + (1 << 15)) >> 24) as u8
}
fn blend_pixel(dst: &mut Raster, x: i64, y: i64, src: [u8; 4]) {
    if src[3] == 0 {
        return;
    }
    if dst.channels == 1 {
        let old = i64::from(dst.gray_at(x, y));
        let foreground = i64::from(gray_of(src[0], src[1], src[2]));
        let alpha = i64::from(src[3]);
        let result = (foreground * alpha + old * (255 - alpha) + 127) / 255;
        dst.set_gray(x, y, result as u8);
        return;
    }
    if src[3] == 255 {
        dst.set_nrgba(x, y, src);
        return;
    }
    let base = dst.nrgba_at(x, y);
    let src_weight = i64::from(src[3]) * 255;
    let base_weight = i64::from(base[3]) * (255 - i64::from(src[3]));
    let alpha = src_weight + base_weight;
    let channel = |a: u8, b: u8| {
        ((i64::from(a) * src_weight + i64::from(b) * base_weight + alpha / 2) / alpha) as u8
    };
    dst.set_nrgba(
        x,
        y,
        [
            channel(src[0], base[0]),
            channel(src[1], base[1]),
            channel(src[2], base[2]),
            ((alpha + 127) / 255) as u8,
        ],
    );
}
#[derive(Clone, Copy, Default)]
struct Stroke {
    on: [u8; 4],
    off: [u8; 4],
    pattern: Option<u32>,
    radius: f64,
    offset: Point2D,
}
impl Stroke {
    fn shadow(&self, line: &CompoundLineStyle, scale: f64) -> Result<Stroke, Error> {
        let mut offset = Point2D::default();
        if let Some(x) = line.shadow_offset_x {
            offset.x = x * scale;
        }
        if let Some(y) = line.shadow_offset_y {
            offset.y = y * scale;
        }
        if !valid_paint_number(offset.x) || !valid_paint_number(offset.y) {
            return Err(invalid("invalid Compound Graphic shadow offset"));
        }
        let mut out = Stroke {
            on: compound_color(line.shadow_color, line.shadow_opacity, [0, 0, 0, 255]),
            radius: self.radius,
            offset,
            ..Stroke::default()
        };
        if line.shadow == "OUTLINED" {
            out.radius += fmath::hypot(offset.x, offset.y);
            out.offset = Point2D::default();
        }
        Ok(out)
    }
}
fn paint_compound_paths(
    dst: &mut Raster,
    paths: &[Vec<Point2D>],
    closed: bool,
    graphic: &CompoundGraphic,
    fallback: [u8; 4],
    scale: f64,
    budget: &mut i64,
) -> Result<(), Error> {
    if !valid_paint_number(scale) || scale <= 0.0 {
        return Err(invalid("invalid Compound Graphic paint scale or budget"));
    }
    for path in paths {
        spend(budget, path.len() as i64)?;
        for p in path {
            if !valid_paint_number(p.x) || !valid_paint_number(p.y) {
                return Err(invalid("Compound Graphic projection is out of range"));
            }
        }
    }
    let mut stroke = Stroke {
        on: fallback,
        radius: 0.5,
        ..Stroke::default()
    };
    if let Some(line) = &graphic.line_style {
        stroke.on = compound_color(line.on_color, line.on_opacity, fallback);
        stroke.off = compound_color(line.off_color, line.off_opacity, [0; 4]);
        if let Some(thickness) = line.thickness {
            let width = thickness * scale;
            if !valid_paint_number(width) || width <= 0.0 {
                return Err(invalid("invalid Compound Graphic line thickness"));
            }
            stroke.radius = fmath::max(0.5, width / 2.0);
        }
        if line.dashing == "DASHED" {
            let Some(pattern) = line.pattern else {
                return Err(invalid("DASHED Compound Graphic requires Line Pattern"));
            };
            stroke.pattern = Some(pattern);
        }
        if line.shadow == "NORMAL" || line.shadow == "OUTLINED" {
            let shadow = stroke.shadow(line, scale)?;
            paint_stroke(dst, paths, &shadow, budget)?;
        }
    }
    if closed && graphic.filled == Some(true) {
        let Some(fill) = &graphic.fill_style else {
            return Err(invalid("filled Compound Graphic requires Fill Style"));
        };
        for path in paths {
            paint_fill(dst, path, fill, fallback, budget)?;
        }
    }
    paint_stroke(dst, paths, &stroke, budget)
}
type Coverage = HashMap<(i64, i64), Box<[u8; TILE_AREA]>>;
fn cover(
    coverage: &mut Coverage,
    x: i64,
    y: i64,
    value: u8,
    budget: &mut i64,
) -> Result<(), Error> {
    let origin = (x & !(TILE_SIDE - 1), y & !(TILE_SIDE - 1));
    let tile = match coverage.entry(origin) {
        std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
        std::collections::hash_map::Entry::Vacant(e) => {
            spend(budget, 2 * TILE_AREA as i64)?;
            e.insert(Box::new([0; TILE_AREA]))
        }
    };
    let index = ((y - origin.1) * TILE_SIDE + (x - origin.0)) as usize;
    tile[index] = tile[index].max(value);
    Ok(())
}
fn paint_stroke(
    dst: &mut Raster,
    paths: &[Vec<Point2D>],
    stroke: &Stroke,
    budget: &mut i64,
) -> Result<(), Error> {
    if stroke.on[3] == 0 && (stroke.pattern.is_none() || stroke.off[3] == 0) {
        return Ok(());
    }
    let mut coverage: Coverage = HashMap::new();
    let bounds = dst.rect;
    for path in paths {
        let mut phase = 0.0;
        for i in 1..path.len() {
            spend(budget, 1)?;
            let (from, to) = (path[i - 1], path[i]);
            let len = fmath::hypot(to.x - from.x, to.y - from.y);
            cover_segment(&mut coverage, bounds, from, to, len, phase, stroke, budget)?;
            phase = (phase + len) % 32.0;
        }
    }
    let inks = [[0u8; 4], stroke.off, stroke.on];
    for (origin, tile) in &coverage {
        for (index, &value) in tile.iter().enumerate() {
            if value != 0 {
                let index = index as i64;
                blend_pixel(
                    dst,
                    origin.0 + index % TILE_SIDE,
                    origin.1 + index / TILE_SIDE,
                    inks[value as usize],
                );
            }
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn cover_segment(
    coverage: &mut Coverage,
    bounds: Rect,
    from: Point2D,
    to: Point2D,
    len: f64,
    phase: f64,
    stroke: &Stroke,
    budget: &mut i64,
) -> Result<(), Error> {
    let (mut from, mut to) = (from, to);
    let (mut dx, mut dy) = (to.x - from.x, to.y - from.y);
    if stroke.offset != Point2D::default() && len > 0.0 {
        let shift_x = (dx * stroke.offset.x - dy * stroke.offset.y) / len;
        let shift_y = (dy * stroke.offset.x + dx * stroke.offset.y) / len;
        from.x += shift_x;
        from.y += shift_y;
        to.x += shift_x;
        to.y += shift_y;
    }
    let mut bounds = bounds;
    let swap = dy.abs() > dx.abs();
    if swap {
        std::mem::swap(&mut from.x, &mut from.y);
        std::mem::swap(&mut to.x, &mut to.y);
        std::mem::swap(&mut dx, &mut dy);
        std::mem::swap(&mut bounds.min_x, &mut bounds.min_y);
        std::mem::swap(&mut bounds.max_x, &mut bounds.max_y);
    }
    let slope = if dx != 0.0 { dy / dx } else { 0.0 };
    let radius = stroke.radius;
    let span = radius * fmath::hypot(1.0, slope);
    let (a, b) = paint_range(
        fmath::min(from.x, to.x) - radius,
        fmath::max(from.x, to.x) + radius,
        bounds.min_x,
        bounds.max_x,
    );
    let radius_squared = radius * radius;
    for major in a..b {
        let center_x = major as f64 + 0.5;
        let middle_y = from.y + (center_x - from.x) * slope;
        let (c, d) = paint_range(middle_y - span, middle_y + span, bounds.min_y, bounds.max_y);
        spend(budget, d - c + 1)?;
        for minor in c..d {
            let center_y = minor as f64 + 0.5;
            let mut t = 0.0;
            if len > 0.0 {
                t = fmath::min(
                    1.0,
                    fmath::max(
                        0.0,
                        ((center_x - from.x) * dx + (center_y - from.y) * dy) / (len * len),
                    ),
                );
            }
            let (nx, ny) = (center_x - from.x - t * dx, center_y - from.y - t * dy);
            let distance_squared = nx * nx + ny * ny;
            if distance_squared > radius_squared
                || (distance_squared == radius_squared && (ny < 0.0 || (ny == 0.0 && nx < 0.0)))
            {
                continue;
            }
            let mut value = 2u8;
            if let Some(pattern) = stroke.pattern {
                let bit = ((phase + t * len) % 32.0) as u32;
                if pattern & (1u32 << (31 - bit)) == 0 {
                    value = 1;
                }
            }
            let (x, y) = if swap { (minor, major) } else { (major, minor) };
            cover(coverage, x, y, value, budget)?;
        }
    }
    Ok(())
}
fn paint_range(lo: f64, hi: f64, min_pixel: i64, max_pixel: i64) -> (i64, i64) {
    let first = fmath::min(
        max_pixel as f64,
        fmath::max(min_pixel as f64, (lo - 0.5).ceil()),
    );
    let end = fmath::min(
        max_pixel as f64,
        fmath::max(first, (hi - 0.5).floor() + 1.0),
    );
    (f64_to_i64(first), f64_to_i64(end))
}
fn paint_fill(
    dst: &mut Raster,
    polygon: &[Point2D],
    style: &super::CompoundFillStyle,
    fallback: [u8; 4],
    budget: &mut i64,
) -> Result<(), Error> {
    if polygon.len() < 3 {
        return Ok(());
    }
    let on = compound_color(style.on_color, style.on_opacity, fallback);
    let off = compound_color(style.off_color, style.off_opacity, [0; 4]);
    let stipple = style.mode == "STIPPELED";
    if stipple && style.pattern.len() != 128 {
        return Err(invalid(
            "STIPPELED Compound Graphic requires a 32x32 Fill Pattern",
        ));
    }
    if on[3] == 0 && (!stipple || off[3] == 0) {
        return Ok(());
    }
    let (mut ymin, mut ymax) = (polygon[0].y, polygon[0].y);
    for p in &polygon[1..] {
        ymin = fmath::min(ymin, p.y);
        ymax = fmath::max(ymax, p.y);
    }
    let bounds = dst.rect;
    let (first, end) = paint_range(ymin, ymax, bounds.min_y, bounds.max_y);
    for y in first..end {
        spend(budget, polygon.len() as i64)?;
        let cy = y as f64 + 0.5;
        let (mut left, mut right) = (f64::INFINITY, f64::NEG_INFINITY);
        for (i, &a) in polygon.iter().enumerate() {
            let b = polygon[(i + 1) % polygon.len()];
            if (a.y <= cy) != (b.y <= cy) {
                let x = a.x + (cy - a.y) * (b.x - a.x) / (b.y - a.y);
                left = fmath::min(left, x);
                right = fmath::max(right, x);
            }
        }
        let (x0, x1) = paint_range(left, right, bounds.min_x, bounds.max_x);
        spend(budget, x1 - x0)?;
        for x in x0..x1 {
            let mut ink = on;
            if stipple {
                let row = (y - bounds.min_y) & 31;
                let column = (x - bounds.min_x) & 31;
                if style.pattern[(row * 4 + column / 8) as usize] & (0x80 >> (column & 7)) == 0 {
                    ink = off;
                }
            }
            blend_pixel(dst, x, y, ink);
        }
    }
    Ok(())
}
