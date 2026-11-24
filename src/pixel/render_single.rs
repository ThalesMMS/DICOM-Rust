use super::extract::{self, rescale_at, window_at};
use super::frame::{FrameImage, FrameOptions, new_encapsulated_frame, new_native_frame};
use super::frame_reader::decode_file_frame;
use super::pixeldata::{PixelError, PixelErrorKind, normalized_photometric};
use crate::display::{self, LutOptions};
use crate::object::{File, Tag};
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
#[doc(hidden)]
pub fn render_single_frame(file: &File, index: i64) -> Result<FrameImage, PixelError> {
    let ds = &file.dataset;
    let mut opts = FrameOptions {
        big_endian: file.transfer_syntax.big_endian,
        ..FrameOptions::default()
    };
    if normalized_photometric(&ds.get_string(TAG_PHOTOMETRIC).unwrap_or_default()) == "ARGB" {
        opts.palette = display::palette_from_object(ds, LutOptions::default())
            .map_err(|e| PixelError::new(PixelErrorKind::Display, e.to_string()))?;
    }
    if let Some(window) = window_at(ds, index) {
        opts.window.center = window.center;
        opts.window.width = window.width;
        opts.window.function = window.function;
    }
    opts.rescale = rescale_at(ds, index);
    let (metadata, data, encapsulated) = decode_file_frame(file, index, None)
        .map_err(|e| PixelError::new(PixelErrorKind::Other, e.to_string()))?;
    let frame = if encapsulated {
        new_encapsulated_frame(index, data, metadata, opts)
    } else {
        new_native_frame(index, data, metadata, opts)
    };
    frame
        .image()
        .map_err(|e| PixelError::new(PixelErrorKind::Frame, e.to_string()))
}
#[doc(hidden)]
pub fn render_file_frame(file: &File, index: i64) -> Result<FrameImage, PixelError> {
    match render_single_frame(file, index) {
        Ok(image) => Ok(image),
        Err(_) => extract::render_frame(file, index),
    }
}
