use super::import_mp4::inspect_mp4;
use super::import_mpeg::inspect_mpeg;
use super::{Error, Kind, import_media, too_large};
use crate::object::transfer::Syntax;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportLimits {
    pub max_bytes: i64,
    pub max_pixels: i64,
    pub max_decoded_bytes: i64,
    pub max_frames: i64,
}
impl ImportLimits {
    pub(crate) fn normalize(mut self) -> Result<ImportLimits, Error> {
        if self.max_bytes < 0
            || self.max_bytes > 1 << 30
            || self.max_pixels < 0
            || self.max_decoded_bytes < 0
            || self.max_frames < 0
            || self.max_frames > 100_000
        {
            return Err(import_media());
        }
        if self.max_bytes == 0 {
            self.max_bytes = 256 << 20;
        }
        if self.max_pixels == 0 {
            self.max_pixels = 16 << 20;
        }
        if self.max_decoded_bytes == 0 {
            self.max_decoded_bytes = 512 << 20;
        }
        if self.max_frames == 0 {
            self.max_frames = 1024;
        }
        Ok(self)
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceInfo {
    pub rows: i64,
    pub columns: i64,
    pub frames: i64,
    pub frame_rate: f64,
    pub transfer_syntax: Syntax,
    pub container: &'static str,
    pub compression_method: &'static str,
}
pub fn inspect_source(data: &[u8], limits: ImportLimits) -> Result<SourceInfo, Error> {
    let limits = limits.normalize()?;
    if data.len() as i64 > limits.max_bytes {
        return Err(too_large());
    }
    let info = if data.len() >= 12 && &data[4..8] == b"ftyp" {
        inspect_mp4(data, limits)?
    } else if data.len() >= 4 && data[..4] == [0, 0, 1, 0xba] {
        inspect_mpeg(data, limits)?
    } else {
        return Err(Error::bare(Kind::UnsupportedContainer));
    };
    if info.rows < 1
        || info.columns < 1
        || info.rows > 65535
        || info.columns > 65535
        || info.frames < 1
        || info.frames > limits.max_frames
        || info.frame_rate.is_nan()
        || info.frame_rate <= 0.0
        || info.frame_rate.is_infinite()
        || info.frame_rate > 1000.0
    {
        return Err(import_media());
    }
    let pixels = info.rows * info.columns;
    if pixels > limits.max_pixels || pixels > limits.max_decoded_bytes / 3 / info.frames {
        return Err(too_large());
    }
    Ok(info)
}
