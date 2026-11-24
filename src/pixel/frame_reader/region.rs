use super::{FrameReader, Stored, layout, layout_bare, packed_bytes, swap_words};
#[cfg(feature = "jpeg2000")]
use crate::codec::jpeg2000;
#[cfg(feature = "jpeg-ls")]
use crate::codec::jpegls;
use crate::codec::rle;
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{self as pd, PixelErrorKind, frame_size, normalized_photometric};
use crate::pixel::transcode::decompress::normalized;
use crate::pixel::transcode::{Error, ErrorKind};
use crate::transfer;
pub const ERR_INVALID_REGION: &str = "dicom: invalid frame region";
pub const ERR_REGION_UNSUPPORTED: &str = "dicom: codec cannot decode a frame region";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameRegion {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}
impl FrameRegion {
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> FrameRegion {
        FrameRegion {
            min_x: x0.min(x1),
            min_y: y0.min(y1),
            max_x: x0.max(x1),
            max_y: y0.max(y1),
        }
    }
    pub(crate) fn dx(&self) -> i64 {
        self.max_x.saturating_sub(self.min_x)
    }
    pub(crate) fn dy(&self) -> i64 {
        self.max_y.saturating_sub(self.min_y)
    }
    pub(super) fn is_empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    pub fn text(&self) -> String {
        format!(
            "({},{})-({},{})",
            self.min_x, self.min_y, self.max_x, self.max_y
        )
    }
}
pub(super) fn is_subsampled_422(photometric: &str) -> bool {
    matches!(
        normalized_photometric(photometric).as_str(),
        "YBR_FULL_422" | "YBR_PARTIAL_422"
    )
}
pub(super) fn subsampled_error() -> Error {
    layout("regions of subsampled 4:2:2 Pixel Data are unsupported")
}
pub(super) fn crop_frame(
    frame: &[u8],
    mut metadata: Metadata,
    region: FrameRegion,
) -> Result<(Vec<u8>, Metadata), Error> {
    if is_subsampled_422(&metadata.photometric_interpretation) {
        return Err(subsampled_error());
    }
    let size = frame_size(&metadata);
    if (frame.len() as i64) < size {
        return Err(Error::pixel(
            PixelErrorKind::PixelDataSizeMismatch,
            format_args!("frame of {} bytes, want {size}", frame.len()),
        ));
    }
    let sample_bytes = usize::from(metadata.bits_allocated).div_ceil(8);
    let (columns, rows) = (usize::from(metadata.columns), usize::from(metadata.rows));
    let samples = usize::from(metadata.samples_per_pixel);
    let (planes, per_pixel) = if samples > 1 && metadata.planar_configuration == 1 {
        (samples, 1)
    } else {
        (1, samples)
    };
    let width = (region.dx() as usize)
        .saturating_mul(per_pixel)
        .saturating_mul(sample_bytes);
    let mut out = Vec::with_capacity(
        width
            .saturating_mul(region.dy() as usize)
            .saturating_mul(planes),
    );
    for plane in 0..planes {
        for y in region.min_y as usize..region.max_y as usize {
            let start = plane
                .saturating_mul(rows)
                .saturating_add(y)
                .saturating_mul(columns)
                .saturating_add(region.min_x as usize)
                .saturating_mul(per_pixel)
                .saturating_mul(sample_bytes);
            let row = frame
                .get(start..start.saturating_add(width))
                .ok_or_else(layout_bare)?;
            out.extend_from_slice(row);
        }
    }
    metadata.rows = region.dy() as u16;
    metadata.columns = region.dx() as u16;
    Ok((out, metadata))
}
impl FrameReader<'_> {
    pub fn region(&self, i: i64, region: FrameRegion) -> Result<(Vec<u8>, Metadata), Error> {
        self.check_index(i)?;
        let bounds = FrameRegion::new(
            0,
            0,
            i64::from(self.metadata.columns),
            i64::from(self.metadata.rows),
        );
        let inside = region.min_x >= bounds.min_x
            && region.max_x <= bounds.max_x
            && region.min_y >= bounds.min_y
            && region.max_y <= bounds.max_y;
        if region.is_empty() || !inside {
            return Err(Error::new(
                ErrorKind::Other,
                format!(
                    "{ERR_INVALID_REGION}: {} in {}",
                    region.text(),
                    bounds.text()
                ),
            ));
        }
        match &self.stored {
            Stored::Native(value) => {
                if is_subsampled_422(&self.metadata.photometric_interpretation) {
                    return Err(subsampled_error());
                }
                self.native_region(value, i, region)
            }
            Stored::Encapsulated(_) if self.custom_codec().is_some() => {
                if let Some(codec) = self.custom_codec()
                    && let Some(decoder) = codec.region_decoder()
                {
                    let codestream = self.read_admitted_frame(i)?;
                    let cx = crate::pixel::codec_router::DecodeContext::default();
                    match decoder.decode_frame_region(
                        &cx,
                        &codestream,
                        &self.frame_metadata(),
                        region,
                    ) {
                        Ok(frames) => return self.check_decoded_part(frames, region),
                        Err(e) if !e.is(ErrorKind::RegionUnsupported) => return Err(e),
                        Err(_) => {}
                    }
                }
                let (frame, metadata) = self.frame(i)?;
                crop_frame(&frame, metadata, region)
            }
            Stored::Encapsulated(_) => match self.frame(i) {
                Ok((frame, metadata)) => crop_frame(&frame, metadata, region),
                Err(e) if e.is(ErrorKind::Pixel(PixelErrorKind::CodecDecodeFailed)) => {
                    Err(self.region_codec_error(i, region).unwrap_or(e))
                }
                Err(e) => Err(e),
            },
        }
    }
    fn region_codec_error(&self, i: i64, region: FrameRegion) -> Option<Error> {
        let uid = transfer::normalize_uid(&self.syntax.uid);
        let codestream = self.raw_frame(i).ok()?;
        let m = self.frame_metadata();
        let text = if uid == rle::UID {
            let r = rle::Rect::new(region.min_x, region.min_y, region.max_x, region.max_y);
            rle::decode_region(&codestream, &m, r, rle::Recovery::default())
                .err()?
                .to_string()
        } else {
            let cm = crate::codec::common::Metadata {
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
            };
            #[cfg(feature = "jpeg-ls")]
            if uid == jpegls::UID || uid == jpegls::UID_NEAR_LOSSLESS {
                let codec = if uid == jpegls::UID {
                    jpegls::Codec::lossless()
                } else {
                    jpegls::Codec::near_lossless()
                };
                let r = jpegls::Region {
                    x0: region.min_x,
                    y0: region.min_y,
                    x1: region.max_x,
                    y1: region.max_y,
                };
                let text = codec
                    .decode_frame_region(&codestream, &cm, r, jpegls::DecodeOptions::default())
                    .err()?
                    .to_string();
                return Some(Error::new(ErrorKind::Other, text));
            }
            #[cfg(feature = "jpeg2000")]
            if jpeg2000::transfer_syntax_uids().contains(&uid)
                && !matches!(uid, "1.2.840.10008.1.2.4.92" | "1.2.840.10008.1.2.4.93")
            {
                let request = jpeg2000::PartialRequest {
                    region: jpeg2000::j2kcore::Rect::new(
                        region.min_x,
                        region.min_y,
                        region.max_x,
                        region.max_y,
                    ),
                    reduction_level: 0,
                    quality_layers: 0,
                };
                let text = jpeg2000::decode_partial(
                    &codestream,
                    &cm,
                    request,
                    jpeg2000::CoreOptions::default(),
                )
                .err()?
                .to_string();
                return Some(Error::new(ErrorKind::Other, text));
            }
            let _ = (&cm, &codestream);
            return None;
        };
        Some(Error::new(ErrorKind::Other, text))
    }
    fn native_runs(&self, i: i64, region: FrameRegion) -> Vec<(i64, i64)> {
        let m = &self.metadata;
        let (mut frame_units, mut sample_units) =
            (frame_size(m), i64::from(m.bits_allocated.div_ceil(8)));
        if m.bits_allocated == 1 {
            frame_units = pd::packed_frame_bits(m);
            sample_units = 1;
        }
        let start = i.saturating_mul(frame_units);
        let (mut planes, mut per_pixel) = (1i64, i64::from(m.samples_per_pixel));
        if per_pixel > 1 && m.planar_configuration == 1 {
            planes = per_pixel;
            per_pixel = 1;
        }
        let (columns, rows) = (i64::from(m.columns), i64::from(m.rows));
        let row_units = region
            .dx()
            .saturating_mul(per_pixel)
            .saturating_mul(sample_units);
        let mut runs = Vec::new();
        for p in 0..planes {
            for y in region.min_y..region.max_y {
                let offset = p
                    .saturating_mul(rows)
                    .saturating_add(y)
                    .saturating_mul(columns)
                    .saturating_add(region.min_x)
                    .saturating_mul(per_pixel)
                    .saturating_mul(sample_units);
                runs.push((start.saturating_add(offset), row_units));
            }
        }
        runs
    }
    pub(super) fn native_region(
        &self,
        value: &[u8],
        i: i64,
        region: FrameRegion,
    ) -> Result<(Vec<u8>, Metadata), Error> {
        let mut m = self.frame_metadata();
        m.rows = region.dy() as u16;
        m.columns = region.dx() as u16;
        let bytes = frame_size(&m);
        if m.samples_per_pixel == 0 || (m.bits_allocated < 8 && m.bits_allocated != 1) || bytes <= 0
        {
            return Err(layout_bare());
        }
        let mut limits = self.options.limits;
        limits.max_frames = 1;
        let limits = normalized(limits)?;
        let runs = self.native_runs(i, region);
        let input = if self.shared_units() {
            let mut total = 0i64;
            for &(start, length) in &runs {
                let (first, end) = self.native_wire_range(start, length);
                if first < 0 || end < first || end > value.len() as i64 {
                    return Err(layout_bare());
                }
                total = total.saturating_add(end.saturating_sub(first));
            }
            total
        } else {
            bytes
        };
        super::check_frame_read_limits(&limits, &m, input, bytes)?;
        let mut dst = vec![0u8; bytes as usize];
        if !self.shared_units() {
            let width = usize::from(self.metadata.bits_allocated).div_ceil(8);
            let mut touched = 0usize;
            for (start, length) in runs {
                let (start, length) = (start as usize, length as usize);
                let part = dst
                    .get_mut(touched..touched.saturating_add(length))
                    .ok_or_else(layout_bare)?;
                let from = value
                    .get(start..start.saturating_add(length))
                    .ok_or_else(layout_bare)?;
                part.copy_from_slice(from);
                if self.syntax.big_endian && width > 1 {
                    for sample in part.chunks_exact_mut(width) {
                        sample.reverse();
                    }
                }
                touched = touched.saturating_add(length);
            }
            return Ok((dst, m));
        }
        let bit_packed = self.metadata.bits_allocated == 1;
        let mut next = 0i64;
        let mut touched = 0usize;
        for (start, length) in runs {
            let (first, end) = self.native_wire_range(start, length);
            let mut chunk = value
                .get(first as usize..end as usize)
                .ok_or_else(layout_bare)?
                .to_vec();
            if self.ow_lanes() {
                swap_words(&mut chunk);
            }
            if bit_packed {
                let at = start.saturating_sub(first.saturating_mul(8));
                pd::copy_packed_bits(&mut dst, next, &chunk, at, length);
                touched = packed_bytes(next.saturating_add(length)) as usize;
            } else {
                let at = start.saturating_sub(first) as usize;
                let target = next as usize;
                let n = length as usize;
                let to = dst
                    .get_mut(target..target.saturating_add(n))
                    .ok_or_else(layout_bare)?;
                let from = chunk
                    .get(at..at.saturating_add(n))
                    .ok_or_else(layout_bare)?;
                to.copy_from_slice(from);
                touched = target.saturating_add(n);
            }
            next = next.saturating_add(length);
        }
        if bit_packed
            && next % 8 != 0
            && let Some(last) = dst.get_mut(touched.wrapping_sub(1))
        {
            *last &= ((1u16 << (next % 8)).wrapping_sub(1)) as u8;
        }
        Ok((dst, m))
    }
}
