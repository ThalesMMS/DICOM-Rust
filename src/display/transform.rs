use super::{Gray16Image, GrayImage, Rgba64Image, RgbaImage};
use crate::fmath;
use std::borrow::Cow;
use std::fmt;
pub const MAX_EXPORT_PIXELS: i64 = 1 << 26;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    Gray(GrayImage),
    Gray16(Gray16Image),
    Rgba(RgbaImage),
    Rgba64(Rgba64Image),
}
impl Image {
    pub fn size(&self) -> (usize, usize) {
        match self {
            Image::Gray(i) => (i.width, i.height),
            Image::Gray16(i) => (i.width, i.height),
            Image::Rgba(i) => (i.width, i.height),
            Image::Rgba64(i) => (i.width, i.height),
        }
    }
    fn rgba_at(&self, index: usize) -> [u8; 4] {
        match self {
            Image::Gray(i) => {
                let y = i.pixels[index];
                [y, y, y, 255]
            }
            Image::Gray16(i) => {
                let y = i.pixels[2 * index];
                [y, y, y, 255]
            }
            Image::Rgba(i) => {
                let p = &i.pixels[4 * index..4 * index + 4];
                [p[0], p[1], p[2], p[3]]
            }
            Image::Rgba64(i) => {
                let p = &i.pixels[8 * index..8 * index + 8];
                [p[0], p[2], p[4], p[6]]
            }
        }
    }
    pub fn to_rgba(&self) -> RgbaImage {
        let (width, height) = self.size();
        RgbaImage {
            width,
            height,
            pixels: (0..width * height).flat_map(|i| self.rgba_at(i)).collect(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageTransformError(pub String);
impl fmt::Display for ImageTransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dicom/display: invalid image transform: {}", self.0)
    }
}
impl std::error::Error for ImageTransformError {}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ImageTransform {
    pub rotation_degrees: i64,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub scale: f64,
    pub width: i64,
    pub height: i64,
}
fn checked_export_size(width: i64, height: i64) -> bool {
    width > 0
        && height > 0
        && width <= MAX_EXPORT_PIXELS
        && height <= MAX_EXPORT_PIXELS
        && width <= MAX_EXPORT_PIXELS / height
}
pub fn transform_image<'a>(
    src: &'a Image,
    opts: &ImageTransform,
) -> Result<Cow<'a, Image>, ImageTransformError> {
    let err = |detail: &str| Err(ImageTransformError(detail.to_owned()));
    let (w, h) = src.size();
    let (w, h) = (w as i64, h as i64);
    if !checked_export_size(w, h) {
        return err(&format!("input size exceeds {MAX_EXPORT_PIXELS} pixels"));
    }
    let (rw, rh) = match opts.rotation_degrees {
        0 | 180 => (w, h),
        90 | 270 => (h, w),
        _ => return err("rotation must be 0, 90, 180 or 270"),
    };
    let scale = if opts.scale == 0.0 { 1.0 } else { opts.scale };
    if !scale.is_finite() || !(1.0 / 64.0..=64.0).contains(&scale) {
        return err("scale must be finite and in 1/64..64");
    }
    let (mut ow, mut oh) = (opts.width, opts.height);
    if ow == 0 && oh == 0 {
        ow = fmath::f64_to_i64((rw as f64 * scale).round());
        oh = fmath::f64_to_i64((rh as f64 * scale).round());
    } else if ow <= 0 || oh <= 0 || opts.scale != 0.0 {
        return err("size needs two positive dimensions and cannot accompany scale");
    }
    if !checked_export_size(ow, oh) {
        return err(&format!(
            "output size exceeds {MAX_EXPORT_PIXELS} pixels or rounds to zero"
        ));
    }
    if opts.rotation_degrees == 0
        && !opts.flip_horizontal
        && !opts.flip_vertical
        && ow == w
        && oh == h
    {
        return Ok(Cow::Borrowed(src));
    }
    let source = |x: i64, y: i64| -> usize {
        let mut u = x * rw / ow;
        let mut v = y * rh / oh;
        if opts.flip_horizontal {
            u = rw - 1 - u;
        }
        if opts.flip_vertical {
            v = rh - 1 - v;
        }
        let (sx, sy) = match opts.rotation_degrees {
            90 => (v, h - 1 - u),
            180 => (w - 1 - u, h - 1 - v),
            270 => (w - 1 - v, u),
            _ => (u, v),
        };
        (sy * w + sx) as usize
    };
    let positions = (0..oh).flat_map(|y| (0..ow).map(move |x| (x, y)));
    let (width, height) = (ow as usize, oh as usize);
    Ok(Cow::Owned(match src {
        Image::Gray(g) => Image::Gray(GrayImage {
            width,
            height,
            pixels: positions.map(|(x, y)| g.pixels[source(x, y)]).collect(),
        }),
        Image::Gray16(g) => Image::Gray16(Gray16Image {
            width,
            height,
            pixels: positions
                .flat_map(|(x, y)| {
                    let i = 2 * source(x, y);
                    [g.pixels[i], g.pixels[i + 1]]
                })
                .collect(),
        }),
        _ => Image::Rgba(RgbaImage {
            width,
            height,
            pixels: positions
                .flat_map(|(x, y)| src.rgba_at(source(x, y)))
                .collect(),
        }),
    }))
}
