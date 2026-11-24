use super::region::{FrameRegion, crop_frame, is_subsampled_422};
use super::{FrameReader, Stored, layout_bare};
#[cfg(feature = "jpeg2000")]
use crate::codec::jpeg2000;
#[cfg(feature = "jpeg-ls")]
use crate::codec::jpegls;
use crate::codec::rle;
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{PixelErrorKind, frame_size};
use crate::pixel::transcode::decompress::{limit_error, normalized};
use crate::pixel::transcode::{Error, ErrorKind};
use crate::transfer;
pub const ERR_BUFFER_LAYOUT_UNAVAILABLE: &str =
    "dicom: decoded buffer layout unavailable without decoding";
pub const ERR_BUFFER_TOO_SMALL: &str = "dicom: destination buffer too small";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BufferMode {
    #[default]
    None,
    NativeDirect,
    CodecDirect,
    FrameCopy,
    RegionCopy,
    FrameRegionCopy,
}
impl BufferMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BufferMode::None => "",
            BufferMode::NativeDirect => "native-direct",
            BufferMode::CodecDirect => "codec-direct",
            BufferMode::FrameCopy => "frame-copy",
            BufferMode::RegionCopy => "region-copy",
            BufferMode::FrameRegionCopy => "frame-region-copy",
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BufferLayout {
    pub metadata: Metadata,
    pub bytes: i64,
    pub mode: BufferMode,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BufferResult {
    pub layout: BufferLayout,
    pub written: i64,
    pub diagnostics: Vec<String>,
}
fn unavailable() -> Error {
    Error::new(
        ErrorKind::BufferLayoutUnavailable,
        ERR_BUFFER_LAYOUT_UNAVAILABLE,
    )
}
fn too_small(required: i64, provided: usize) -> Error {
    Error::new(
        ErrorKind::BufferTooSmall,
        format!("{ERR_BUFFER_TOO_SMALL}: need {required} bytes, have {provided}"),
    )
}
fn invalid_region() -> Error {
    Error::new(ErrorKind::Other, super::ERR_INVALID_REGION)
}
#[cfg_attr(not(all(feature = "jpeg-ls", feature = "jpeg2000")), allow(dead_code))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Codec {
    None,
    Rle,
    JpegLs(bool),
    Jpeg2000,
    Other,
}
fn codec_for(uid: &str) -> Codec {
    match transfer::normalize_uid(uid) {
        rle::UID => Codec::Rle,
        #[cfg(feature = "jpeg-ls")]
        jpegls::UID => Codec::JpegLs(false),
        #[cfg(feature = "jpeg-ls")]
        jpegls::UID_NEAR_LOSSLESS => Codec::JpegLs(true),
        #[cfg(feature = "jpeg2000")]
        "1.2.840.10008.1.2.4.92" | "1.2.840.10008.1.2.4.93" => Codec::Other,
        #[cfg(feature = "jpeg2000")]
        u if jpeg2000::transfer_syntax_uids().contains(&u) => Codec::Jpeg2000,
        #[cfg(feature = "jpeg")]
        "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51" => Codec::Other,
        #[cfg(feature = "jpeg-lossless")]
        "1.2.840.10008.1.2.4.57" | "1.2.840.10008.1.2.4.70" => Codec::Other,
        "1.2.840.10008.1.2.8.1" => Codec::Other,
        _ => Codec::None,
    }
}
#[cfg(feature = "jpeg-ls")]
fn codec_metadata(m: &Metadata) -> jpegls::Metadata {
    jpegls::Metadata {
        rows: m.rows,
        columns: m.columns,
        samples_per_pixel: m.samples_per_pixel,
        bits_allocated: m.bits_allocated,
        bits_stored: m.bits_stored,
        high_bit: m.high_bit,
        pixel_representation: m.pixel_representation,
        planar_configuration: m.planar_configuration,
        planar_configuration_present: m.planar_configuration_present,
        number_of_frames: m.number_of_frames,
        photometric_interpretation: m.photometric_interpretation.clone(),
    }
}
fn codec_error(uid: &str) -> Error {
    let mut text = String::from("dicom: pixel data codec decode failed");
    if !uid.is_empty() {
        text.push_str(&format!(
            " for transfer syntax {}",
            crate::textfmt::quote(uid)
        ));
        if let Some(name) = crate::object::transfer::lookup(uid)
            .map(|s| s.name)
            .filter(|n| !n.is_empty())
        {
            text.push_str(&format!(" ({name})"));
        }
    }
    Error::new(ErrorKind::Pixel(PixelErrorKind::CodecDecodeFailed), text)
}
fn checked_layout(
    m: Metadata,
    region: FrameRegion,
    mode: BufferMode,
) -> Result<BufferLayout, Error> {
    if i64::from(m.rows) != region.dy()
        || i64::from(m.columns) != region.dx()
        || m.number_of_frames != 1
        || m.samples_per_pixel == 0
        || m.bits_allocated < 8 && (mode != BufferMode::NativeDirect || m.bits_allocated != 1)
        || frame_size(&m) <= 0
    {
        return Err(layout_bare());
    }
    let bytes = frame_size(&m);
    Ok(BufferLayout {
        metadata: m,
        bytes,
        mode,
    })
}
type LayoutFailure = (Option<BufferLayout>, Error);
impl FrameReader<'_> {
    fn bounds(&self) -> FrameRegion {
        FrameRegion::new(
            0,
            0,
            i64::from(self.metadata.columns),
            i64::from(self.metadata.rows),
        )
    }
    #[cfg_attr(not(feature = "jpeg-ls"), allow(unused_variables))]
    fn codec_buffer_metadata(
        &self,
        codec: Codec,
        mut m: Metadata,
        region: FrameRegion,
        regional: bool,
    ) -> Result<Option<Metadata>, Error> {
        match codec {
            Codec::Rle => {
                rle::validate_decode_metadata(&m)
                    .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
                m.rows = region.dy() as u16;
                m.columns = region.dx() as u16;
                m.number_of_frames = 1;
                if m.samples_per_pixel == 3 {
                    m.planar_configuration = 0;
                    m.planar_configuration_present = true;
                }
                Ok(Some(m))
            }
            #[cfg(feature = "jpeg-ls")]
            Codec::JpegLs(near) => {
                let jls = if near {
                    jpegls::Codec::near_lossless()
                } else {
                    jpegls::Codec::lossless()
                };
                if regional {
                    let photometric = crate::textfmt::to_upper(m.photometric_interpretation.trim());
                    if m.samples_per_pixel != 1
                        || photometric != "MONOCHROME1" && photometric != "MONOCHROME2"
                    {
                        return Ok(None);
                    }
                    m.number_of_frames = 1;
                    jls.check_decode_metadata(&codec_metadata(&m))
                        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
                    m.rows = region.dy() as u16;
                    m.columns = region.dx() as u16;
                    return Ok(Some(m));
                }
                m.number_of_frames = 1;
                jls.check_decode_metadata(&codec_metadata(&m))
                    .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
                Ok(Some(m))
            }
            _ => Ok(None),
        }
    }
    #[allow(clippy::result_large_err)]
    fn buffer_layout(
        &self,
        i: i64,
        region: FrameRegion,
        regional: bool,
    ) -> Result<Option<BufferLayout>, LayoutFailure> {
        self.check_index(i).map_err(|e| (None, e))?;
        if self.collection().is_some() {
            return Ok(None);
        }
        let bounds = self.bounds();
        let region = if regional {
            let inside = region.min_x >= 0
                && region.min_y >= 0
                && region.max_x <= bounds.max_x
                && region.max_y <= bounds.max_y;
            if region.is_empty() || !inside {
                return Err((None, invalid_region()));
            }
            region
        } else {
            bounds
        };
        let mut m = self.frame_metadata();
        let Stored::Native(value) = &self.stored else {
            let codec = codec_for(&self.syntax.uid);
            let Some(m) = self
                .codec_buffer_metadata(codec, m, region, regional)
                .map_err(|e| (None, e))?
            else {
                return Ok(None);
            };
            return checked_layout(m, region, BufferMode::CodecDirect)
                .map(Some)
                .map_err(|e| (None, e));
        };
        if regional && is_subsampled_422(&m.photometric_interpretation) {
            return Err((None, layout_bare()));
        }
        m.rows = region.dy() as u16;
        m.columns = region.dx() as u16;
        let layout = checked_layout(m, region, BufferMode::NativeDirect).map_err(|e| (None, e))?;
        let checked = if regional {
            self.native_region(value, i, region).map(|_| ())
        } else {
            self.native_frame(value, i).map(|_| ())
        };
        checked.map_err(|e| (Some(layout.clone()), e))?;
        Ok(Some(layout))
    }
    pub fn frame_buffer_layout(&self, i: i64) -> Result<BufferLayout, Error> {
        self.layout_query(i, FrameRegion::default(), false)
    }
    pub fn region_buffer_layout(&self, i: i64, region: FrameRegion) -> Result<BufferLayout, Error> {
        self.layout_query(i, region, true)
    }
    fn layout_query(
        &self,
        i: i64,
        region: FrameRegion,
        regional: bool,
    ) -> Result<BufferLayout, Error> {
        match self.buffer_layout(i, region, regional) {
            Ok(Some(layout)) => Ok(layout),
            Ok(None) => Err(unavailable()),
            Err((_, e)) => Err(e),
        }
    }
    pub fn frame_into(&self, i: i64, dst: &mut [u8]) -> (BufferResult, Result<(), Error>) {
        self.into(i, FrameRegion::default(), false, dst)
    }
    pub fn region_into(
        &self,
        i: i64,
        region: FrameRegion,
        dst: &mut [u8],
    ) -> (BufferResult, Result<(), Error>) {
        self.into(i, region, true, dst)
    }
    fn into(
        &self,
        i: i64,
        region: FrameRegion,
        regional: bool,
        dst: &mut [u8],
    ) -> (BufferResult, Result<(), Error>) {
        let layout = match self.buffer_layout(i, region, regional) {
            Ok(Some(layout)) => layout,
            Ok(None) => return self.copy_into(i, region, regional, dst),
            Err((layout, e)) => {
                let result = BufferResult {
                    layout: layout.unwrap_or_default(),
                    ..BufferResult::default()
                };
                return (result, Err(e));
            }
        };
        let mut result = BufferResult {
            layout: layout.clone(),
            ..BufferResult::default()
        };
        if (dst.len() as i64) < layout.bytes {
            return (result, Err(too_small(layout.bytes, dst.len())));
        }
        let region = if regional { region } else { self.bounds() };
        match self.direct(i, region, regional, &layout) {
            Ok((data, diagnostics)) => {
                let Some(to) = dst.get_mut(..data.len()) else {
                    return (result, Err(too_small(data.len() as i64, dst.len())));
                };
                to.copy_from_slice(&data);
                result.written = data.len() as i64;
                result.diagnostics = diagnostics;
                (result, Ok(()))
            }
            Err(e) => (result, Err(e)),
        }
    }
    fn direct(
        &self,
        i: i64,
        region: FrameRegion,
        regional: bool,
        layout: &BufferLayout,
    ) -> Result<(Vec<u8>, Vec<String>), Error> {
        if let Stored::Native(value) = &self.stored {
            let (data, _) = if regional {
                self.native_region(value, i, region)?
            } else {
                self.native_frame(value, i)?
            };
            return Ok((data, Vec::new()));
        }
        let input = self.admitted_frame_input_bytes(i)?;
        let mut limits = self.options.limits;
        limits.max_frames = 1;
        let limits = normalized(limits)?;
        if layout.bytes > limits.max_native_bytes {
            return Err(limit_error(
                "MaxNativeBytes",
                layout.bytes,
                limits.max_native_bytes,
            ));
        }
        let ratio = if input <= 0 {
            i64::MAX
        } else {
            layout
                .bytes
                .checked_div(input)
                .unwrap_or(i64::MAX)
                .saturating_add(i64::from(layout.bytes.checked_rem(input).unwrap_or(0) != 0))
        };
        if ratio > limits.max_expansion_ratio {
            return Err(limit_error(
                "MaxExpansionRatio",
                ratio,
                limits.max_expansion_ratio,
            ));
        }
        let uid = transfer::normalize_uid(&self.syntax.uid).to_string();
        let (decoded, frame, diagnostics) =
            self.decode_frame_object(i).map_err(|_| codec_error(&uid))?;
        let mut m =
            crate::pixel::pixeldata::extract_metadata(&decoded).map_err(|_| codec_error(&uid))?;
        m.number_of_frames = 1;
        let mut frame = frame;
        if m.samples_per_pixel > 1 && layout.metadata.planar_configuration != m.planar_configuration
        {
            frame = reorder(&frame, &m, layout.metadata.planar_configuration);
            m.planar_configuration = layout.metadata.planar_configuration;
        }
        let data = if regional {
            crop_frame(&frame, m, region)
                .map_err(|_| codec_error(&uid))?
                .0
        } else {
            frame
        };
        if data.len() as i64 != layout.bytes {
            return Err(codec_error(&uid));
        }
        Ok((data, diagnostics))
    }
    fn copy_into(
        &self,
        i: i64,
        region: FrameRegion,
        regional: bool,
        dst: &mut [u8],
    ) -> (BufferResult, Result<(), Error>) {
        let (mode, decoded) = if !regional {
            (BufferMode::FrameCopy, self.frame(i))
        } else if codec_for(&self.syntax.uid) == Codec::Jpeg2000 {
            (BufferMode::RegionCopy, self.region(i, region))
        } else if let (Codec::JpegLs(_), Err(e)) = (
            codec_for(&self.syntax.uid),
            self.admitted_frame_input_bytes(i),
        ) {
            (BufferMode::RegionCopy, Err(e))
        } else {
            let decoded = self
                .frame(i)
                .and_then(|(frame, m)| crop_frame(&frame, m, region));
            (BufferMode::FrameRegionCopy, decoded)
        };
        let mut result = BufferResult {
            layout: BufferLayout {
                mode,
                ..BufferLayout::default()
            },
            ..BufferResult::default()
        };
        let (data, m) = match decoded {
            Ok(d) => d,
            Err(e) => return (result, Err(e)),
        };
        result.layout = BufferLayout {
            bytes: data.len() as i64,
            metadata: m.clone(),
            mode,
        };
        if data.len() as i64 != frame_size(&m) {
            return (
                result,
                Err(Error::pixel_bare(PixelErrorKind::PixelDataSizeMismatch)),
            );
        }
        let Some(to) = dst.get_mut(..data.len()) else {
            return (result, Err(too_small(data.len() as i64, dst.len())));
        };
        to.copy_from_slice(&data);
        result.written = data.len() as i64;
        (result, Ok(()))
    }
}
fn reorder(frame: &[u8], m: &Metadata, target: u16) -> Vec<u8> {
    let samples = usize::from(m.samples_per_pixel);
    let width = usize::from(m.bits_allocated).div_ceil(8);
    let pixels = usize::from(m.rows).saturating_mul(usize::from(m.columns));
    let mut out = vec![0u8; frame.len()];
    for p in 0..pixels {
        for s in 0..samples {
            let planar = s
                .saturating_mul(pixels)
                .saturating_add(p)
                .saturating_mul(width);
            let interleaved = p
                .saturating_mul(samples)
                .saturating_add(s)
                .saturating_mul(width);
            let (from, to) = if target == 0 {
                (planar, interleaved)
            } else {
                (interleaved, planar)
            };
            if let (Some(src), Some(dst)) = (
                frame.get(from..from.saturating_add(width)),
                out.get_mut(to..to.saturating_add(width)),
            ) {
                dst.copy_from_slice(src);
            }
        }
    }
    out
}
