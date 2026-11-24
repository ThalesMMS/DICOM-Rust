use super::frames::MAX_REDUCTION_LEVEL;
use super::render::{self, WindowLevel};
use super::{Error, ErrorKind, Image, representative_frame_index};
use crate::display::{self, ColorFrame, GrayImage, LutOptions, PixelFormat, RgbaImage};
use crate::object::File;
use crate::pixel::extract::{rescale_at, window_at};
use crate::pixel::frame_reader::{FrameReader, FrameReaderOptions, PartialDecodeRequest};
use crate::pixel::transcode;
fn pixel_error(e: transcode::Error) -> Error {
    let kind = if e.is(transcode::ErrorKind::PartialFile) {
        ErrorKind::PartialFile
    } else if e.is(transcode::ErrorKind::PartialDecodeUnsupported) {
        ErrorKind::PartialDecodeUnsupported
    } else if e.is(transcode::ErrorKind::DecompressResourceLimit) {
        ErrorKind::ResourceLimit
    } else {
        ErrorKind::Other
    };
    Error::new(kind, e.to_string())
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThumbnailFrameOptions {
    pub target_side: i64,
    pub fallback_frame: i64,
    pub frame_index: Option<i64>,
    pub codec_reduction_only: bool,
}
fn reduction_level(longest: i64, size: i64) -> i64 {
    if size == 0 {
        return 0;
    }
    let mut level = 0;
    while level < MAX_REDUCTION_LEVEL && longest >> (level + 1) >= size {
        level += 1;
    }
    level
}
pub fn render_thumbnail_frame(file: &File, opts: &ThumbnailFrameOptions) -> Result<Image, Error> {
    if opts.target_side < 0 {
        return Err(Error::detail(
            ErrorKind::InvalidOptions,
            "negative thumbnail size",
        ));
    }
    let reader = FrameReader::new(file, FrameReaderOptions::default()).map_err(pixel_error)?;
    let metadata = reader.metadata().clone();
    let mut index = opts.fallback_frame;
    if let Some(i) = opts.frame_index {
        index = i;
    } else if let Some(r) = representative_frame_index(&file.dataset, metadata.number_of_frames) {
        index = r;
    }
    let mut level = reduction_level(
        i64::from(metadata.rows.max(metadata.columns)),
        opts.target_side,
    );
    if opts.codec_reduction_only {
        if level > 0 {
            let capabilities = reader
                .partial_decode_capabilities(index)
                .map_err(pixel_error)?;
            level = level.min(capabilities.reduction_levels);
        }
        if level == 0 {
            return Err(Error::new(
                ErrorKind::PartialDecodeUnsupported,
                format!(
                    "{}: no lower resolution available for thumbnail",
                    super::frames::ERR_PARTIAL_DECODE_UNSUPPORTED
                ),
            ));
        }
    }
    let result = reader
        .decode_partial(PartialDecodeRequest {
            frame: index,
            reduction_level: level,
            no_fallback: opts.codec_reduction_only,
            ..PartialDecodeRequest::default()
        })
        .map_err(pixel_error)?;
    let (data, m) = (result.data, result.metadata);
    let dataset = &file.dataset;
    if m.photometric_interpretation
        .trim()
        .eq_ignore_ascii_case("PALETTE COLOR")
    {
        let palette = match display::palette_from_object(dataset, LutOptions::default()) {
            Ok(Some(p)) => p,
            Ok(None) => {
                return Err(Error::other(
                    "iconimage: PALETTE COLOR without palette LUTs: <nil>",
                ));
            }
            Err(e) => {
                return Err(Error::other(format!(
                    "iconimage: PALETTE COLOR without palette LUTs: {e}"
                )));
            }
        };
        return display::render_color(&ColorFrame {
            rows: i64::from(m.rows),
            columns: i64::from(m.columns),
            pixels: &data,
            photometric: "PALETTE COLOR".into(),
            samples_per_pixel: 1,
            planar_configuration: 0,
            format: PixelFormat {
                bits_allocated: i64::from(m.bits_allocated),
                bits_stored: i64::from(m.bits_stored),
                high_bit: i64::from(m.high_bit),
                signed: false,
                big_endian: false,
            },
            palette: Some(palette),
        })
        .map(Image::Rgba)
        .map_err(Error::other);
    }
    let rescale = rescale_at(dataset, index);
    let mut frame = render::Frame {
        index,
        metadata: m,
        big_endian: false,
        pixels: data,
        rescale,
        default_window: WindowLevel::default(),
    };
    let mut window = WindowLevel::default();
    let mut found = false;
    if let Some(w) = window_at(dataset, index) {
        window = WindowLevel {
            center: w.center,
            width: w.width,
            function: w.function,
        };
        found = true;
    }
    if !found && frame.metadata.samples_per_pixel == 1 {
        window = render::auto_window(&frame).unwrap_or_default();
    }
    frame.default_window = window.clone();
    render::render_frame(&frame, &window)
}
pub fn fit_thumbnail(img: &Image, size: i64) -> Option<Image> {
    let (width, height) = img.size();
    if width == 0 || height == 0 || size <= 0 {
        return None;
    }
    let size = size as usize;
    let (mut target_w, mut target_h) = (width, height);
    let longest = width.max(height);
    if longest > size {
        target_w = ((width * size + longest / 2) / longest).max(1);
        target_h = ((height * size + longest / 2) / longest).max(1);
    }
    let cell = |t: usize, target: usize, extent: usize| {
        let from = t * extent / target;
        (from, (from + 1).max((t + 1) * extent / target))
    };
    if let Image::Gray(gray) = img {
        if target_w == width && target_h == height {
            return Some(img.clone());
        }
        let mut out = vec![0u8; target_w * target_h];
        for ty in 0..target_h {
            let (y0, y1) = cell(ty, target_h, height);
            for tx in 0..target_w {
                let (x0, x1) = cell(tx, target_w, width);
                let (mut sum, mut n) = (0usize, 0usize);
                for y in y0..y1 {
                    for x in x0..x1 {
                        sum += usize::from(gray.pixels[y * gray.width + x]);
                        n += 1;
                    }
                }
                out[ty * target_w + tx] = ((sum + n / 2) / n) as u8;
            }
        }
        return Some(Image::Gray(GrayImage {
            width: target_w,
            height: target_h,
            pixels: out,
        }));
    }
    let mut is_gray = true;
    let mut out = vec![0u8; target_w * target_h * 4];
    for ty in 0..target_h {
        let (y0, y1) = cell(ty, target_h, height);
        for tx in 0..target_w {
            let (x0, x1) = cell(tx, target_w, width);
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (pr, pg, pb, pa) = img.rgba64(x, y);
                    r += u64::from(pr);
                    g += u64::from(pg);
                    b += u64::from(pb);
                    a += u64::from(pa);
                    n += 1;
                }
            }
            let (r, g, b, a) = (
                (r / n) as u32,
                (g / n) as u32,
                (b / n) as u32,
                (a / n) as u32,
            );
            let pre = |c: u32| c * a / 0xFFFF;
            let (r, g, b) = (pre(r), pre(g), pre(b));
            let c = if a == 0xFFFF {
                [(r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8, 0xFF]
            } else if a == 0 {
                [0, 0, 0, 0]
            } else {
                let un = |c: u32| ((c * 0xFFFF) / a) >> 8;
                [un(r) as u8, un(g) as u8, un(b) as u8, (a >> 8) as u8]
            };
            if c[0] != c[1] || c[1] != c[2] {
                is_gray = false;
            }
            out[(ty * target_w + tx) * 4..][..4].copy_from_slice(&c);
        }
    }
    let rgba = RgbaImage {
        width: target_w,
        height: target_h,
        pixels: out,
    };
    if !is_gray {
        return Some(Image::Nrgba(rgba));
    }
    Some(Image::Gray(GrayImage {
        width: target_w,
        height: target_h,
        pixels: rgba
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0])
            .collect(),
    }))
}
