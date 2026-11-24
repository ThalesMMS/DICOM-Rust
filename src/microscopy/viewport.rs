use super::{Error, Kind, Level, Rect, Tile, deduplicate_tiles, finite_positive};
use crate::fmath::{self, f64_to_i64, f64_to_u8};
use crate::numtext::format_float;
use std::collections::HashSet;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Viewport {
    pub center_x: f64,
    pub center_y: f64,
    pub matrix_pixels_per_view: f64,
    pub screen_width: i64,
    pub screen_height: i64,
    pub rotation_degrees: i64,
    pub optical_paths: Vec<Channel>,
    pub focal_plane: i64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Channel {
    pub optical_path: String,
    pub tint: [u8; 4],
    pub opacity: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NrgbaImage {
    pub width: usize,
    pub height: usize,
    pub pix: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub pix: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileLayer {
    pub tile: Tile,
    pub image: Option<NrgbaImage>,
    pub tint: [u8; 4],
    pub opacity: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScaleBar {
    pub pixels: f64,
    pub millimeters: f64,
    pub label: String,
}
impl Viewport {
    fn scale(&self) -> f64 {
        if finite_positive(self.matrix_pixels_per_view) {
            self.matrix_pixels_per_view
        } else {
            1.0
        }
    }
    pub fn bounds(&self, level: &Level) -> Rect {
        if self.screen_width <= 0 || self.screen_height <= 0 {
            return Rect::default();
        }
        let scale = self.scale();
        let width = f64_to_i64((self.screen_width as f64 * scale).ceil());
        let height = f64_to_i64((self.screen_height as f64 * scale).ceil());
        let center = |value: f64, extent: i64| {
            if value.is_finite() {
                value
            } else {
                extent as f64 / 2.0
            }
        };
        let center_x = center(self.center_x, level.matrix_width);
        let center_y = center(self.center_y, level.matrix_height);
        let left = f64_to_i64((center_x - width as f64 / 2.0).floor());
        let top = f64_to_i64((center_y - height as f64 / 2.0).floor());
        Rect::new(
            left,
            top,
            left.wrapping_add(width),
            top.wrapping_add(height),
        )
        .intersect(level.matrix_bounds())
    }
    pub fn visible_tiles(&self, level: &Level) -> Vec<Tile> {
        self.visible_tiles_limit(level, 0)
    }
    pub fn visible_tiles_limit(&self, level: &Level, limit: i64) -> Vec<Tile> {
        let bounds = self.bounds(level);
        if self.optical_paths.is_empty() {
            return level.tiles_for_viewport_limit(bounds, "", self.focal_plane, limit);
        }
        if limit > 0 {
            let paths: HashSet<&str> = self
                .optical_paths
                .iter()
                .map(|c| c.optical_path.as_str())
                .collect();
            let all_paths = paths.contains("");
            let matches = |path: &str| all_paths || paths.contains(path);
            return level.select_tiles(bounds, self.focal_plane, limit, &matches, true);
        }
        let mut out = Vec::new();
        for channel in &self.optical_paths {
            out.extend(level.tiles_for_viewport(bounds, &channel.optical_path, self.focal_plane));
        }
        deduplicate_tiles(out, 0)
    }
    pub fn scale_bar(&self, level: &Level) -> Result<ScaleBar, Error> {
        if !level.calibrated() {
            return Err(Error::bare(Kind::UncalibratedSlide));
        }
        let mm_per_screen_pixel = level.pixel_spacing_x * self.scale();
        let millimeters = nice_scale_value(mm_per_screen_pixel * 100.0);
        let label = if millimeters < 1.0 {
            format!("{} µm", format_float(millimeters * 1000.0, 'g', -1))
        } else {
            format!("{} mm", format_float(millimeters, 'g', -1))
        };
        Ok(ScaleBar {
            pixels: millimeters / mm_per_screen_pixel,
            millimeters,
            label,
        })
    }
    pub fn overview_viewport(
        &self,
        level: &Level,
        overview_width: i64,
        overview_height: i64,
    ) -> Rect {
        let bounds = self.bounds(level);
        if bounds.is_empty() || overview_width <= 0 || overview_height <= 0 {
            return Rect::default();
        }
        let map = |value: i64, size: i64, extent: i64| value as f64 * size as f64 / extent as f64;
        let (w, h) = (level.matrix_width, level.matrix_height);
        Rect::new(
            f64_to_i64(map(bounds.min_x, overview_width, w).floor()),
            f64_to_i64(map(bounds.min_y, overview_height, h).floor()),
            f64_to_i64(map(bounds.max_x, overview_width, w).ceil()),
            f64_to_i64(map(bounds.max_y, overview_height, h).ceil()),
        )
        .intersect(Rect::new(0, 0, overview_width, overview_height))
    }
}
fn nice_scale_value(value: f64) -> f64 {
    if !finite_positive(value) {
        return 0.0;
    }
    let exponent = fmath::log10(value).floor();
    let fraction = value / fmath::pow(10.0, exponent);
    let nice = if fraction < 1.5 {
        1.0
    } else if fraction < 3.5 {
        2.0
    } else if fraction < 7.5 {
        5.0
    } else {
        10.0
    };
    nice * fmath::pow(10.0, exponent)
}
pub fn compose_viewport(
    viewport: Rect,
    layers: &[TileLayer],
    rotation_degrees: i64,
) -> Result<RgbaImage, Error> {
    compose_viewport_to_size(
        viewport,
        viewport.dx(),
        viewport.dy(),
        layers,
        rotation_degrees,
    )
}
pub fn compose_viewport_to_size(
    viewport: Rect,
    output_width: i64,
    output_height: i64,
    layers: &[TileLayer],
    rotation_degrees: i64,
) -> Result<RgbaImage, Error> {
    if viewport.is_empty() {
        return Err(Error::other("dicom/microscopy: empty viewport"));
    }
    if output_width <= 0 || output_height <= 0 {
        return Err(Error::other("dicom/microscopy: empty output"));
    }
    if output_width.saturating_mul(output_height) > 100_000_000 {
        return Err(Error::other(format!(
            "dicom/microscopy: output {output_width}x{output_height} exceeds safety limit"
        )));
    }
    let mut out = RgbaImage {
        width: output_width as usize,
        height: output_height as usize,
        pix: vec![0; (output_width * output_height * 4) as usize],
    };
    let out_bounds = Rect::new(0, 0, output_width, output_height);
    let (dx, dy) = (viewport.dx() as f64, viewport.dy() as f64);
    let (ow, oh) = (output_width as f64, output_height as f64);
    for layer in layers {
        let Some(image) = &layer.image else {
            continue;
        };
        let intersection = layer.tile.bounds().intersect(viewport);
        if intersection.is_empty() {
            continue;
        }
        let mut opacity = layer.opacity;
        if opacity <= 0.0 {
            continue;
        }
        if opacity > 1.0 {
            opacity = 1.0;
        }
        let tint = if layer.tint[3] == 0 {
            [255; 4]
        } else {
            layer.tint
        };
        let (source_w, source_h) = (image.width as i64, image.height as i64);
        let target = Rect::new(
            f64_to_i64(((intersection.min_x - viewport.min_x) as f64 * ow / dx).floor()),
            f64_to_i64(((intersection.min_y - viewport.min_y) as f64 * oh / dy).floor()),
            f64_to_i64(((intersection.max_x - viewport.min_x) as f64 * ow / dx).ceil()),
            f64_to_i64(((intersection.max_y - viewport.min_y) as f64 * oh / dy).ceil()),
        )
        .intersect(out_bounds);
        for target_y in target.min_y..target.max_y {
            let matrix_y = viewport.min_y + f64_to_i64((target_y as f64 + 0.5) * dy / oh);
            let source_y = (matrix_y - layer.tile.row) * source_h / layer.tile.height.max(1);
            if source_y < 0 || source_y >= source_h {
                continue;
            }
            for target_x in target.min_x..target.max_x {
                let matrix_x = viewport.min_x + f64_to_i64((target_x as f64 + 0.5) * dx / ow);
                let source_x = (matrix_x - layer.tile.column) * source_w / layer.tile.width.max(1);
                if source_x < 0 || source_x >= source_w {
                    continue;
                }
                let at = ((source_y * source_w + source_x) * 4) as usize;
                let source = &image.pix[at..at + 4];
                let alpha = opacity * f64::from(source[3]) / 255.0 * f64::from(tint[3]) / 255.0;
                let tinted = |channel: usize| {
                    (u16::from(source[channel]) * u16::from(tint[channel]) / 255) as u8
                };
                let pixel = [
                    tinted(0),
                    tinted(1),
                    tinted(2),
                    f64_to_u8((alpha * 255.0).round()),
                ];
                let at = ((target_y * output_width + target_x) * 4) as usize;
                blend_nrgba(&mut out.pix[at..at + 4], pixel);
            }
        }
    }
    rotate_right_angles(out, rotation_degrees)
}
fn unpremultiply(p: &[u8]) -> [u8; 4] {
    let wide = |v: u8| u32::from(v) * 0x101;
    let a = wide(p[3]);
    if a == 0xffff {
        return [p[0], p[1], p[2], 0xff];
    }
    if a == 0 {
        return [0; 4];
    }
    let channel = |v: u8| ((wide(v) * 0xffff / a) >> 8) as u8;
    [channel(p[0]), channel(p[1]), channel(p[2]), p[3]]
}
fn premultiply(p: [u8; 4]) -> [u8; 4] {
    let channel = |v: u8| ((u32::from(v) * 0x101 * u32::from(p[3]) / 0xff) >> 8) as u8;
    [channel(p[0]), channel(p[1]), channel(p[2]), p[3]]
}
fn blend_nrgba(destination: &mut [u8], source: [u8; 4]) {
    if source[3] == 0 {
        return;
    }
    let current = unpremultiply(destination);
    let source_alpha = f64::from(source[3]) / 255.0;
    let destination_alpha = f64::from(current[3]) / 255.0;
    let out_alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
    if out_alpha == 0.0 {
        return;
    }
    let blend = |src: u8, dst: u8| {
        let value = (f64::from(src) * source_alpha
            + f64::from(dst) * destination_alpha * (1.0 - source_alpha))
            / out_alpha;
        f64_to_u8(value.round())
    };
    destination.copy_from_slice(&premultiply([
        blend(source[0], current[0]),
        blend(source[1], current[1]),
        blend(source[2], current[2]),
        f64_to_u8((out_alpha * 255.0).round()),
    ]));
}
pub fn rotate_viewport(source: RgbaImage, rotation_degrees: i64) -> Result<RgbaImage, Error> {
    rotate_right_angles(source, rotation_degrees)
}
fn rotate_right_angles(source: RgbaImage, degrees: i64) -> Result<RgbaImage, Error> {
    let degrees = degrees.rem_euclid(360);
    if degrees % 90 != 0 {
        return Err(Error::other(
            "dicom/microscopy: rotation must be a multiple of 90 degrees",
        ));
    }
    if degrees == 0 {
        return Ok(source);
    }
    let (width, height) = (source.width, source.height);
    let (out_width, out_height) = if degrees == 180 {
        (width, height)
    } else {
        (height, width)
    };
    let mut out = RgbaImage {
        width: out_width,
        height: out_height,
        pix: vec![0; width * height * 4],
    };
    for y in 0..height {
        for x in 0..width {
            let (to_x, to_y) = match degrees {
                90 => (height - 1 - y, x),
                180 => (width - 1 - x, height - 1 - y),
                _ => (y, width - 1 - x),
            };
            let from = (y * width + x) * 4;
            let to = (to_y * out_width + to_x) * 4;
            out.pix[to..to + 4].copy_from_slice(&source.pix[from..from + 4]);
        }
    }
    Ok(out)
}
