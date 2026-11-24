#![forbid(unsafe_code)]
use super::error::RenderError;
use super::frame::{Frame, RenderImage, render_frame};
use super::parallel::{Cancellation, ctx_err};
use super::png_encode::{PngSource, encode_png};
use super::window::WindowLevel;
use crate::codec::jpeg::stdjpeg;
use crate::textfmt::quote;
pub const MEDIA_TYPE_PNG: &str = "image/png";
pub const MEDIA_TYPE_JPEG: &str = "image/jpeg";
pub const ERR_INVALID_ENCODED_REQUEST: &str = "render: invalid encoded image request";
pub const ERR_UNSUPPORTED_ENCODED_MEDIA_TYPE: &str = "render: unsupported encoded image media type";
pub const ERR_ENCODED_IMAGE_RESOURCE_LIMIT: &str = "render: encoded image resource limit exceeded";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodedErrorKind {
    InvalidRequest,
    UnsupportedMediaType,
    ResourceLimit,
    Other,
}
#[derive(Debug, Clone, PartialEq)]
pub struct EncodedError {
    pub kind: EncodedErrorKind,
    pub render: Option<RenderError>,
    message: String,
}
impl EncodedError {
    fn new(kind: EncodedErrorKind, sentinel: &str, detail: String) -> EncodedError {
        EncodedError {
            kind,
            render: None,
            message: format!("{sentinel}: {detail}"),
        }
    }
    fn invalid(detail: impl Into<String>) -> EncodedError {
        EncodedError::new(
            EncodedErrorKind::InvalidRequest,
            ERR_INVALID_ENCODED_REQUEST,
            detail.into(),
        )
    }
    fn limit(detail: String) -> EncodedError {
        EncodedError::new(
            EncodedErrorKind::ResourceLimit,
            ERR_ENCODED_IMAGE_RESOURCE_LIMIT,
            detail,
        )
    }
}
impl From<RenderError> for EncodedError {
    fn from(e: RenderError) -> EncodedError {
        EncodedError {
            kind: EncodedErrorKind::Other,
            message: e.to_string(),
            render: Some(e),
        }
    }
}
impl std::fmt::Display for EncodedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for EncodedError {}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EncodedImageLimits {
    pub max_width: i64,
    pub max_height: i64,
    pub max_pixels: i64,
    pub max_output_bytes: i64,
}
pub fn default_encoded_image_limits() -> EncodedImageLimits {
    EncodedImageLimits {
        max_width: 4096,
        max_height: 4096,
        max_pixels: 16 * 1024 * 1024,
        max_output_bytes: 32 << 20,
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}
impl Rect {
    fn is_empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    fn value_text(&self) -> String {
        format!(
            "({},{})-({},{})",
            self.min_x, self.min_y, self.max_x, self.max_y
        )
    }
}
#[derive(Clone, Default)]
pub struct EncodedRequest<'a> {
    pub frame: Option<&'a Frame>,
    pub window: WindowLevel,
    pub source_rect: Rect,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub width: i64,
    pub height: i64,
    pub media_type: String,
    pub jpeg_quality: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedImage {
    pub data: Vec<u8>,
    pub media_type: String,
    pub width: i64,
    pub height: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardEncodedRenderer {
    limits: EncodedImageLimits,
}
fn normalize_limits(mut l: EncodedImageLimits) -> Result<EncodedImageLimits, EncodedError> {
    if l.max_width < 0 || l.max_height < 0 || l.max_pixels < 0 || l.max_output_bytes < 0 {
        return Err(EncodedError::invalid("negative limit"));
    }
    let d = default_encoded_image_limits();
    if l.max_width == 0 {
        l.max_width = d.max_width;
    }
    if l.max_height == 0 {
        l.max_height = d.max_height;
    }
    if l.max_pixels == 0 {
        l.max_pixels = d.max_pixels;
    }
    if l.max_output_bytes == 0 {
        l.max_output_bytes = d.max_output_bytes;
    }
    Ok(l)
}
fn normalize_media_type(value: &str) -> Result<String, EncodedError> {
    let value = value.trim_matches(|c: char| c.is_whitespace());
    if value.is_empty() {
        return Ok(MEDIA_TYPE_PNG.into());
    }
    let unsupported = || {
        EncodedError::new(
            EncodedErrorKind::UnsupportedMediaType,
            ERR_UNSUPPORTED_ENCODED_MEDIA_TYPE,
            quote(value),
        )
    };
    let parsed =
        crate::net::dicomweb::mimeheader::parse_media_type(value).map_err(|_| unsupported())?;
    let media_type = parsed.media_type;
    if media_type == MEDIA_TYPE_PNG || media_type == MEDIA_TYPE_JPEG {
        Ok(media_type)
    } else {
        Err(unsupported())
    }
}
fn output_dimensions(
    source_width: f64,
    source_height: f64,
    width: i64,
    height: i64,
) -> Result<(i64, i64), EncodedError> {
    if source_width <= 0.0
        || source_height <= 0.0
        || width < 0
        || height < 0
        || source_width.is_nan()
        || source_height.is_nan()
    {
        return Err(EncodedError::invalid("invalid dimensions"));
    }
    let round = |v: f64| crate::fmath::f64_to_i64(v.round());
    let (mut width, mut height) = (width, height);
    if width == 0 && height == 0 {
        width = round(source_width).max(1);
        height = round(source_height).max(1);
    } else if height == 0
        || (width != 0 && source_width * height as f64 > width as f64 * source_height)
    {
        height = round(source_height * width as f64 / source_width).max(1);
    } else {
        width = round(source_width * height as f64 / source_height).max(1);
    }
    Ok((width, height))
}
#[derive(Debug, Clone, Copy, Default)]
struct Region {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    flip_x: bool,
    flip_y: bool,
    full: bool,
}
fn normalize_region(
    bounds: Rect,
    requested: Rect,
    flip_x: bool,
    flip_y: bool,
) -> Result<Region, EncodedError> {
    let (width, height) = (
        (bounds.max_x - bounds.min_x) as f64,
        (bounds.max_y - bounds.min_y) as f64,
    );
    if requested == Rect::default() {
        return Ok(Region {
            width,
            height,
            flip_x,
            flip_y,
            full: !flip_x && !flip_y,
            ..Default::default()
        });
    }
    if requested.is_empty() {
        return Err(EncodedError::invalid(format!(
            "source rectangle {} is empty",
            requested.value_text()
        )));
    }
    if requested.min_x < bounds.min_x
        || requested.min_y < bounds.min_y
        || requested.max_x > bounds.max_x
        || requested.max_y > bounds.max_y
    {
        return Err(EncodedError::invalid(format!(
            "source rectangle {} exceeds image bounds {}",
            requested.value_text(),
            bounds.value_text()
        )));
    }
    Ok(Region {
        x: (requested.min_x - bounds.min_x) as f64,
        y: (requested.min_y - bounds.min_y) as f64,
        width: (requested.max_x - requested.min_x) as f64,
        height: (requested.max_y - requested.min_y) as f64,
        flip_x,
        flip_y,
        full: false,
    })
}
fn interpolation_coordinates(value: f64, size: i64) -> (i64, i64, f64) {
    if value <= 0.0 || size <= 1 {
        return (0, 0, 0.0);
    }
    if value >= (size - 1) as f64 {
        return (size - 1, size - 1, 0.0);
    }
    let first = crate::fmath::f64_to_i64(value.floor());
    (first, first + 1, value - first as f64)
}
fn nrgba_at(img: &RenderImage, x: usize, y: usize) -> [u8; 4] {
    match img {
        RenderImage::Gray { width, pixels, .. } => {
            let v = pixels[y * width + x];
            [v, v, v, 255]
        }
        RenderImage::Rgba { width, pixels, .. } => {
            let o = (y * width + x) * 4;
            let (r, g, b, a) = (pixels[o], pixels[o + 1], pixels[o + 2], pixels[o + 3]);
            if a == 255 {
                return [r, g, b, 255];
            }
            if a == 0 {
                return [0, 0, 0, 0];
            }
            let a16 = u32::from(a) * 0x101;
            let c = |v: u8| (((u32::from(v) * 0x101) * 0xffff / a16) >> 8) as u8;
            [c(r), c(g), c(b), a]
        }
        RenderImage::Nil => [0, 0, 0, 0],
    }
}
enum Output {
    Source(RenderImage),
    Nrgba {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    },
}
fn render_viewport(
    ctx: Option<&dyn Cancellation>,
    source: RenderImage,
    region: Region,
    width: i64,
    height: i64,
) -> Result<Output, RenderError> {
    let (sw, sh) = source.size();
    if region.full && sw as i64 == width && sh as i64 == height {
        return Ok(Output::Source(source));
    }
    let (w, h) = (width as usize, height as usize);
    let mut pixels = vec![0u8; w * h * 4];
    let clamp = |v: f64, lo: f64, hi: f64| crate::fmath::max(lo, crate::fmath::min(hi, v));
    for y in 0..h {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let mut sy = (y as f64 + 0.5) * region.height / height as f64 - 0.5;
        if region.flip_y {
            sy = region.y + region.height - 1.0 - sy;
        } else {
            sy += region.y;
        }
        sy = clamp(sy, region.y, region.y + region.height - 1.0);
        let (y0, y1, fy) = interpolation_coordinates(sy, sh as i64);
        for x in 0..w {
            let mut sx = (x as f64 + 0.5) * region.width / width as f64 - 0.5;
            if region.flip_x {
                sx = region.x + region.width - 1.0 - sx;
            } else {
                sx += region.x;
            }
            sx = clamp(sx, region.x, region.x + region.width - 1.0);
            let (x0, x1, fx) = interpolation_coordinates(sx, sw as i64);
            let tl = nrgba_at(&source, x0 as usize, y0 as usize);
            let tr = nrgba_at(&source, x1 as usize, y0 as usize);
            let bl = nrgba_at(&source, x0 as usize, y1 as usize);
            let br = nrgba_at(&source, x1 as usize, y1 as usize);
            for c in 0..4 {
                let (a, b, cc, d) = (
                    f64::from(tl[c]),
                    f64::from(tr[c]),
                    f64::from(bl[c]),
                    f64::from(br[c]),
                );
                let top = a + (b - a) * fx;
                let bottom = cc + (d - cc) * fx;
                let value = top + (bottom - top) * fy;
                pixels[(y * w + x) * 4 + c] = crate::fmath::f64_to_u8(value.round());
            }
        }
    }
    Ok(Output::Nrgba {
        width: w,
        height: h,
        pixels,
    })
}
impl StandardEncodedRenderer {
    pub fn new(limits: EncodedImageLimits) -> Result<StandardEncodedRenderer, EncodedError> {
        Ok(StandardEncodedRenderer {
            limits: normalize_limits(limits)?,
        })
    }
    pub fn limits(&self) -> EncodedImageLimits {
        self.limits
    }
    fn validate_dimensions(&self, width: i64, height: i64) -> Result<(), EncodedError> {
        if width <= 0 || height <= 0 {
            return Err(EncodedError::invalid("dimensions must be positive"));
        }
        let l = &self.limits;
        if width > l.max_width || height > l.max_height {
            return Err(EncodedError::limit(format!(
                "dimensions {width}x{height} exceed {}x{}",
                l.max_width, l.max_height
            )));
        }
        let pixels = width * height;
        if pixels <= 0 || pixels > l.max_pixels {
            return Err(EncodedError::limit(format!(
                "{pixels} pixels exceed {}",
                l.max_pixels
            )));
        }
        Ok(())
    }
    pub fn render_encoded(
        &self,
        ctx: Option<&dyn Cancellation>,
        request: &EncodedRequest<'_>,
    ) -> Result<EncodedImage, EncodedError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e.into());
        }
        let Some(frame) = request.frame else {
            return Err(EncodedError::invalid("frame is required"));
        };
        if request.width < 0 || request.height < 0 {
            return Err(EncodedError::invalid(
                "requested dimensions must not be negative",
            ));
        }
        let l = &self.limits;
        if request.width > l.max_width || request.height > l.max_height {
            return Err(EncodedError::limit(format!(
                "requested bounds {}x{} exceed {}x{}",
                request.width, request.height, l.max_width, l.max_height
            )));
        }
        let media_type = normalize_media_type(&request.media_type)?;
        let quality = if request.jpeg_quality == 0 {
            90
        } else {
            request.jpeg_quality
        };
        if !(1..=100).contains(&quality) {
            return Err(EncodedError::invalid("JPEG quality must be in 1..100"));
        }
        self.validate_dimensions(
            i64::from(frame.metadata.columns),
            i64::from(frame.metadata.rows),
        )?;
        let (img, err) = render_frame(Some(frame), &request.window);
        if let Some(e) = err {
            return Err(e.into());
        }
        let (sw, sh) = img.size();
        self.validate_dimensions(sw as i64, sh as i64)?;
        let bounds = Rect {
            max_x: sw as i64,
            max_y: sh as i64,
            ..Default::default()
        };
        let region = normalize_region(
            bounds,
            request.source_rect,
            request.flip_horizontal,
            request.flip_vertical,
        )?;
        let (width, height) =
            output_dimensions(region.width, region.height, request.width, request.height)?;
        self.validate_dimensions(width, height)?;
        let output = render_viewport(ctx, img, region, width, height)?;
        let data = if media_type == MEDIA_TYPE_PNG {
            let encoded =
                match &output {
                    Output::Source(RenderImage::Gray {
                        width,
                        height,
                        pixels,
                    }) => encode_png(PngSource::Gray(pixels), *width, *height)
                        .map_err(|e| e.to_string()),
                    Output::Source(RenderImage::Rgba {
                        width,
                        height,
                        pixels,
                    }) => encode_png(PngSource::Rgba(pixels), *width, *height)
                        .map_err(|e| e.to_string()),
                    Output::Source(RenderImage::Nil) => Err("png: nil image".to_string()),
                    Output::Nrgba {
                        width,
                        height,
                        pixels,
                    } => encode_png(PngSource::Nrgba(pixels), *width, *height)
                        .map_err(|e| e.to_string()),
                };
            encoded.map_err(|e| EncodedError {
                kind: EncodedErrorKind::Other,
                render: None,
                message: e,
            })?
        } else {
            let (w, h, source, rgb);
            match &output {
                Output::Source(RenderImage::Gray {
                    width,
                    height,
                    pixels,
                }) => {
                    (w, h) = (*width, *height);
                    source = stdjpeg::Source::Gray(pixels);
                }
                Output::Source(RenderImage::Rgba {
                    width,
                    height,
                    pixels,
                }) => {
                    (w, h) = (*width, *height);
                    rgb = pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| [p[0], p[1], p[2]])
                        .collect::<Vec<u8>>();
                    source = stdjpeg::Source::Rgb(&rgb);
                }
                Output::Nrgba {
                    width,
                    height,
                    pixels,
                } => {
                    (w, h) = (*width, *height);
                    rgb = pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| {
                            let a = u32::from(p[3]);
                            let c = |v: u8| {
                                let v = u32::from(v);
                                ((((v | v << 8) * a) / 0xff) >> 8) as u8
                            };
                            [c(p[0]), c(p[1]), c(p[2])]
                        })
                        .collect::<Vec<u8>>();
                    source = stdjpeg::Source::Rgb(&rgb);
                }
                Output::Source(RenderImage::Nil) => {
                    return Err(EncodedError {
                        kind: EncodedErrorKind::Other,
                        render: None,
                        message: "jpeg: nil image".into(),
                    });
                }
            }
            stdjpeg::encode(source, w, h, quality)
        };
        if data.len() as i64 > self.limits.max_output_bytes {
            return Err(EncodedError::limit(format!(
                "output exceeds {} bytes",
                self.limits.max_output_bytes
            )));
        }
        if let Some(e) = ctx_err(ctx) {
            return Err(e.into());
        }
        Ok(EncodedImage {
            data,
            media_type,
            width,
            height,
        })
    }
}
