use super::import::{
    Format, ImageOptions, ImportLimits, ImportReport, RawOptions, SourceFrame, admit_image,
    check_imported, detect_image, from_images, read_image,
};
use super::{
    Error, ImageLayout, JpegOptions, Kind, SECONDARY_CAPTURE_IMAGE_STORAGE, TAG_PHOTOMETRIC,
    string_element,
};
use crate::object::File;
use crate::object::types::Vr;
use std::collections::{HashMap, HashSet};
use std::io::Write;
pub const RASTER_ALL: &str = "all";
pub const RASTER_RED: &str = "red";
pub const RASTER_GREEN: &str = "green";
pub const RASTER_BLUE: &str = "blue";
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RasterPageInfo {
    pub number: i64,
    pub rows: i64,
    pub columns: i64,
    pub samples: i64,
    pub bits_stored: i64,
    pub orientation: i64,
    pub planar_configuration: i64,
    pub photometric_interpretation: &'static str,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RasterInfo {
    pub format: Format,
    pub pages: Vec<RasterPageInfo>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RasterPage {
    pub info: RasterPageInfo,
    pub data: Vec<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rgba8Image {
    pub width: usize,
    pub height: usize,
    pub pix: Vec<u8>,
}
fn bare(kind: Kind) -> Error {
    Error::bare(kind)
}
fn raster_admission(data: &[u8], limits: ImportLimits) -> Result<ImportLimits, Error> {
    let limits = limits.normalized()?;
    if data.len() as i64 > limits.max_input_bytes {
        return Err(bare(Kind::ImportLimit));
    }
    Ok(limits)
}
pub fn inspect_raster(data: &[u8], limits: ImportLimits) -> Result<RasterInfo, Error> {
    let limits = raster_admission(data, limits)?;
    if is_tiff(data) {
        let (pages, _) = inspect_tiff(data, limits)?;
        return Ok(RasterInfo {
            format: Format::Tiff,
            pages: pages.into_iter().map(|p| p.info).collect(),
        });
    }
    let f = read_desktop_raster(data, limits)?;
    Ok(RasterInfo {
        format: f.format,
        pages: vec![raster_info(&f)],
    })
}
fn read_desktop_raster(data: &[u8], limits: ImportLimits) -> Result<SourceFrame, Error> {
    let format = detect_image(data);
    if !matches!(
        format,
        Format::Png | Format::Bmp | Format::Pnm | Format::Pgx
    ) {
        return Err(Error::new(Kind::UnsupportedImage, "desktop raster format"));
    }
    if format == Format::Png {
        let mut pos = 8usize;
        while pos + 12 <= data.len() {
            let n = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                as usize;
            if n > data.len() - pos - 12 {
                return Err(bare(Kind::MalformedImage));
            }
            if &data[pos + 4..pos + 8] == b"iCCP" {
                return Err(Error::new(Kind::UnsupportedImage, "PNG ICC profile"));
            }
            pos += n + 12;
        }
    }
    let f = read_image(data, format, &JpegOptions::default(), false, limits)?;
    if f.layout.signed != 0 {
        return Err(Error::new(Kind::UnsupportedImage, "signed desktop raster"));
    }
    Ok(f)
}
fn raster_info(f: &SourceFrame) -> RasterPageInfo {
    RasterPageInfo {
        number: 1,
        rows: f.layout.rows,
        columns: f.layout.columns,
        samples: f.layout.samples,
        bits_stored: f.layout.bits,
        orientation: 1,
        planar_configuration: 1,
        photometric_interpretation: f.layout.photometric,
    }
}
fn selected_component(channel: &str) -> Result<Option<usize>, Error> {
    match channel {
        "" | RASTER_ALL => Ok(None),
        RASTER_RED => Ok(Some(0)),
        RASTER_GREEN => Ok(Some(1)),
        RASTER_BLUE => Ok(Some(2)),
        _ => Err(bare(Kind::InvalidOption)),
    }
}
pub fn decode_raster_page(
    data: &[u8],
    page: i64,
    channel: &str,
    limits: ImportLimits,
) -> Result<RasterPage, Error> {
    let limits = raster_admission(data, limits)?;
    let selected = selected_component(channel)?;
    if is_tiff(data) {
        let (pages, big_endian) = inspect_tiff(data, limits)?;
        if page < 1 || page > pages.len() as i64 {
            return Err(bare(Kind::InvalidOption));
        }
        return decode_tiff_page(data, &pages[page as usize - 1], big_endian, selected);
    }
    if page != 1 {
        return Err(bare(Kind::InvalidOption));
    }
    let f = read_desktop_raster(data, limits)?;
    let mut result = RasterPage {
        info: raster_info(&f),
        data: f.data,
    };
    if let Some(component) = selected {
        if result.info.samples != 3 {
            return Err(Error::new(
                Kind::InvalidOption,
                "component selection requires RGB",
            ));
        }
        let width = (result.info.bits_stored as usize).div_ceil(8);
        result.data = result
            .data
            .chunks_exact(3 * width)
            .flat_map(|pixel| {
                pixel[component * width..(component + 1) * width]
                    .iter()
                    .copied()
            })
            .collect();
        result.info.samples = 1;
        result.info.photometric_interpretation = "MONOCHROME2";
    }
    Ok(result)
}
impl RasterPage {
    pub fn preview(&self, edge: i64) -> Result<Rgba8Image, Error> {
        let p = &self.info;
        if !(1..=1024).contains(&edge)
            || !(1..=65535).contains(&p.rows)
            || !(1..=65535).contains(&p.columns)
            || (p.samples != 1 && p.samples != 3)
            || !(1..=16).contains(&p.bits_stored)
        {
            return Err(bare(Kind::InvalidOption));
        }
        let photometric = p.photometric_interpretation;
        if !matches!(photometric, "MONOCHROME1" | "MONOCHROME2" | "RGB")
            || (photometric == "RGB") != (p.samples == 3)
        {
            return Err(bare(Kind::UnsupportedImage));
        }
        let width = (p.bits_stored as usize).div_ceil(8);
        let (rows, columns, samples) = (p.rows as usize, p.columns as usize, p.samples as usize);
        if rows * columns * samples * width != self.data.len() {
            return Err(bare(Kind::MalformedImage));
        }
        let (mut w, mut h) = (columns, rows);
        let longest = columns.max(rows);
        if longest > edge as usize {
            w = (w * edge as usize / longest).max(1);
            h = (h * edge as usize / longest).max(1);
        }
        let top = (1u32 << p.bits_stored) - 1;
        let sample = |at: usize| {
            let v = if width == 2 {
                u32::from(u16::from_le_bytes([self.data[at], self.data[at + 1]]))
            } else {
                u32::from(self.data[at])
            };
            let v = v * 255 / top;
            (if photometric == "MONOCHROME1" {
                255u32.wrapping_sub(v)
            } else {
                v
            }) as u8
        };
        let mut pix = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                let at = ((y * rows / h) * columns + x * columns / w) * samples * width;
                let r = sample(at);
                if samples == 3 {
                    pix.extend_from_slice(&[r, sample(at + width), sample(at + 2 * width), 255]);
                } else {
                    pix.extend_from_slice(&[r, r, r, 255]);
                }
            }
        }
        Ok(Rgba8Image {
            width: w,
            height: h,
            pix,
        })
    }
}
pub fn build_raster_page(
    page: &RasterPage,
    opts: &ImageOptions,
) -> Result<(File, ImportReport), Error> {
    let p = &page.info;
    let photometric = p.photometric_interpretation;
    if !matches!(photometric, "MONOCHROME1" | "MONOCHROME2" | "RGB")
        || (photometric == "RGB") != (p.samples == 3)
    {
        return Err(bare(Kind::UnsupportedImage));
    }
    let limits = opts.limits.normalized()?;
    admit_image(
        &ImageLayout {
            rows: p.rows,
            columns: p.columns,
            samples: p.samples,
            bits: p.bits_stored,
            ..ImageLayout::default()
        },
        limits,
    )?;
    if !opts.jpeg.sop_class_uid.is_empty()
        && opts.jpeg.sop_class_uid != SECONDARY_CAPTURE_IMAGE_STORAGE
    {
        return Err(bare(Kind::UnsupportedSopClass));
    }
    let mut opts = opts.clone();
    opts.format = Format::Raw;
    opts.raw = RawOptions {
        rows: p.rows,
        columns: p.columns,
        frames: 1,
        samples: p.samples,
        bits_allocated: if p.bits_stored > 8 { 16 } else { 8 },
        bits_stored: p.bits_stored,
        ..RawOptions::default()
    };
    opts.jpeg.image_type = "DERIVED\\SECONDARY".to_string();
    opts.jpeg.sop_class_uid = SECONDARY_CAPTURE_IMAGE_STORAGE.to_string();
    let (mut f, mut report) = from_images(&[&page.data], &opts)?;
    if photometric == "MONOCHROME1" {
        f.dataset
            .put(string_element(TAG_PHOTOMETRIC, Vr::CS, "MONOCHROME1"));
    }
    check_imported(&f)?;
    report.report.photometric_interpretation = photometric;
    Ok((f, report))
}
struct TiffRasterPage {
    info: RasterPageInfo,
    source_rows: usize,
    source_columns: usize,
    rows_per_strip: usize,
    offsets: Vec<u32>,
    planar: u32,
}
fn is_tiff(b: &[u8]) -> bool {
    b.len() >= 4 && (&b[..2] == b"II" || &b[..2] == b"MM")
}
fn tiff16(b: &[u8], big_endian: bool) -> u16 {
    if big_endian {
        u16::from_be_bytes([b[0], b[1]])
    } else {
        u16::from_le_bytes([b[0], b[1]])
    }
}
fn tiff32(b: &[u8], big_endian: bool) -> u32 {
    if big_endian {
        u32::from_be_bytes([b[0], b[1], b[2], b[3]])
    } else {
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }
}
fn inspect_tiff(data: &[u8], limits: ImportLimits) -> Result<(Vec<TiffRasterPage>, bool), Error> {
    let malformed = |detail: String| Error::new(Kind::MalformedImage, detail);
    if data.len() < 8 {
        return Err(bare(Kind::MalformedImage));
    }
    let big = &data[..2] == b"MM";
    if tiff16(&data[2..], big) != 42 {
        return Err(Error::new(
            Kind::UnsupportedImage,
            "TIFF version (BigTIFF is not supported)",
        ));
    }
    let mut offset = tiff32(&data[4..], big);
    let mut seen: HashSet<u32> = HashSet::new();
    let mut pages: Vec<TiffRasterPage> = Vec::new();
    let (mut total_values, mut total_entries) = (0usize, 0usize);
    while offset != 0 {
        if pages.len() as i64 >= limits.max_frames || pages.len() >= 4096 {
            return Err(bare(Kind::ImportLimit));
        }
        if seen.contains(&offset) || offset < 8 || offset as usize + 2 > data.len() {
            return Err(malformed("TIFF IFD offset/cycle".to_string()));
        }
        seen.insert(offset);
        let directory = offset as usize;
        let n = usize::from(tiff16(&data[directory..], big));
        total_entries += n;
        if n == 0 || n > 256 || total_entries > 65536 {
            return Err(bare(Kind::ImportLimit));
        }
        let end = directory + 2 + n * 12;
        if end + 4 > data.len() {
            return Err(bare(Kind::MalformedImage));
        }
        let mut tags: HashMap<u16, Vec<u32>> = HashMap::new();
        let mut last = 0u16;
        for i in 0..n {
            let entry_at = directory + 2 + i * 12;
            let entry = &data[entry_at..];
            let tag = tiff16(entry, big);
            if i > 0 && tag <= last {
                return Err(malformed("TIFF duplicate/unsorted tags".to_string()));
            }
            last = tag;
            if matches!(
                tag,
                320 | 322 | 323 | 324 | 325 | 330 | 338 | 34675 | 529 | 530 | 531 | 532
            ) {
                return Err(Error::new(
                    Kind::UnsupportedImage,
                    format!("TIFF tag {tag} (palette/tile/sub-IFD/alpha/ICC/YCbCr)"),
                ));
            }
            if !tiff_structural_tag(tag) {
                continue;
            }
            let (typ, count) = (tiff16(&entry[2..], big), tiff32(&entry[4..], big));
            if (typ != 3 && typ != 4) || count == 0 || count > 65536 {
                return Err(malformed(format!("TIFF tag {tag} type/count")));
            }
            let short_or_long = matches!(tag, 256 | 257 | 273 | 278 | 279);
            if (tag == 254 && typ != 4) || (tag != 254 && !short_or_long && typ != 3) {
                return Err(malformed(format!(
                    "TIFF tag {tag} has an invalid field type"
                )));
            }
            let count = count as usize;
            if count > 262_144 - total_values {
                return Err(bare(Kind::ImportLimit));
            }
            total_values += count;
            let width = if typ == 4 { 4usize } else { 2 };
            let size = count * width;
            let start = if size > 4 {
                tiff32(&entry[8..], big) as usize
            } else {
                entry_at + 8
            };
            if start < 8 || start > data.len() || size > data.len() - start {
                return Err(bare(Kind::MalformedImage));
            }
            let values = data[start..start + size]
                .chunks_exact(width)
                .map(|v| {
                    if typ == 3 {
                        u32::from(tiff16(v, big))
                    } else {
                        tiff32(v, big)
                    }
                })
                .collect();
            tags.insert(tag, values);
        }
        let page = admit_tiff_page(&tags, pages.len() as i64 + 1, data.len(), limits)?;
        pages.push(page);
        offset = tiff32(&data[end..], big);
    }
    if pages.is_empty() {
        return Err(bare(Kind::MalformedImage));
    }
    Ok((pages, big))
}
fn tiff_structural_tag(tag: u16) -> bool {
    matches!(
        tag,
        254 | 256 | 257 | 258 | 259 | 262 | 266 | 273 | 274 | 277 | 278 | 279 | 284 | 317 | 339
    )
}
fn admit_tiff_page(
    tags: &HashMap<u16, Vec<u32>>,
    number: i64,
    bytes: usize,
    limits: ImportLimits,
) -> Result<TiffRasterPage, Error> {
    let refused = |detail: &str| Error::new(Kind::UnsupportedImage, detail);
    let values = |tag: u16| tags.get(&tag).map_or(&[][..], Vec::as_slice);
    let multiple = std::cell::Cell::new(false);
    let scalar = |tag: u16, default: u32| match values(tag) {
        [] => default,
        [v] => *v,
        _ => {
            multiple.set(true);
            default
        }
    };
    let (w, h) = (scalar(256, 0), scalar(257, 0));
    let (samples, photo) = (scalar(277, 1), scalar(262, 99));
    let bits = values(258);
    if w == 0
        || h == 0
        || w > 65535
        || h > 65535
        || (samples != 1 && samples != 3)
        || bits.len() != samples as usize
    {
        return Err(refused("TIFF dimensions/components/bits"));
    }
    if bits
        .iter()
        .any(|&bit| bit != bits[0] || (bit != 8 && bit != 16))
    {
        return Err(refused("TIFF bit depth"));
    }
    if (samples == 1 && photo != 0 && photo != 1) || (samples == 3 && photo != 2) {
        return Err(refused("TIFF photometric interpretation"));
    }
    if scalar(259, 1) != 1 || scalar(266, 1) != 1 || scalar(317, 1) != 1 || scalar(254, 0) & !2 != 0
    {
        return Err(refused("TIFF compression/fill/predictor/subfile"));
    }
    let formats = values(339);
    if !formats.is_empty() {
        if formats.len() != samples as usize {
            return Err(bare(Kind::MalformedImage));
        }
        if formats.iter().any(|&f| f != 1) {
            return Err(refused("TIFF signed/floating samples"));
        }
    }
    let (orientation, planar, mut strip_rows) = (scalar(274, 1), scalar(284, 1), scalar(278, h));
    if multiple.get()
        || !(1..=8).contains(&orientation)
        || !(1..=2).contains(&planar)
        || strip_rows == 0
    {
        return Err(bare(Kind::MalformedImage));
    }
    strip_rows = strip_rows.min(h);
    let mut info = RasterPageInfo {
        number,
        rows: i64::from(h),
        columns: i64::from(w),
        samples: i64::from(samples),
        bits_stored: i64::from(bits[0]),
        orientation: i64::from(orientation),
        planar_configuration: i64::from(planar),
        photometric_interpretation: match photo {
            0 => "MONOCHROME1",
            2 => "RGB",
            _ => "MONOCHROME2",
        },
    };
    if orientation >= 5 {
        (info.rows, info.columns) = (info.columns, info.rows);
    }
    admit_image(
        &ImageLayout {
            rows: info.rows,
            columns: info.columns,
            samples: info.samples,
            bits: info.bits_stored,
            ..ImageLayout::default()
        },
        limits,
    )?;
    let (source_rows, source_columns) = (h as usize, w as usize);
    let rows_per_strip = strip_rows as usize;
    let strips_per_plane = source_rows.div_ceil(rows_per_strip);
    let count = if planar == 2 {
        strips_per_plane * samples as usize
    } else {
        strips_per_plane
    };
    let (offsets, counts) = (values(273), values(279));
    if offsets.len() != count || counts.len() != count || count > 65536 {
        return Err(Error::new(Kind::MalformedImage, "TIFF strip count"));
    }
    let components = if planar == 2 { 1 } else { samples as u64 };
    for i in 0..count {
        let rows = rows_per_strip.min(source_rows - (i % strips_per_plane) * rows_per_strip);
        let want = rows as u64 * source_columns as u64 * components * u64::from(bits[0] / 8);
        let (offset, size) = (u64::from(offsets[i]), u64::from(counts[i]));
        if size != want || offset < 8 || offset > bytes as u64 || size > bytes as u64 - offset {
            return Err(Error::new(Kind::MalformedImage, "TIFF strip span"));
        }
    }
    Ok(TiffRasterPage {
        info,
        source_rows,
        source_columns,
        rows_per_strip,
        offsets: offsets.to_vec(),
        planar,
    })
}
fn decode_tiff_page(
    data: &[u8],
    p: &TiffRasterPage,
    big_endian: bool,
    selected: Option<usize>,
) -> Result<RasterPage, Error> {
    let source_samples = p.info.samples as usize;
    if selected.is_some() && source_samples != 3 {
        return Err(Error::new(
            Kind::InvalidOption,
            "component selection requires RGB",
        ));
    }
    let mut info = p.info.clone();
    if selected.is_some() {
        info.samples = 1;
        info.photometric_interpretation = "MONOCHROME2";
    }
    let width = (info.bits_stored / 8) as usize;
    let (out_columns, out_samples) = (info.columns as usize, info.samples as usize);
    let mut out = vec![0u8; info.rows as usize * out_columns * out_samples * width];
    let (rows, columns) = (p.source_rows, p.source_columns);
    let strips_per_plane = rows.div_ceil(p.rows_per_strip);
    for y in 0..rows {
        for x in 0..columns {
            let (dx, dy) = match info.orientation {
                2 => (columns - 1 - x, y),
                3 => (columns - 1 - x, rows - 1 - y),
                4 => (x, rows - 1 - y),
                5 => (y, x),
                6 => (rows - 1 - y, x),
                7 => (rows - 1 - y, columns - 1 - x),
                8 => (y, columns - 1 - x),
                _ => (x, y),
            };
            for c in 0..out_samples {
                let source_c = selected.unwrap_or(c);
                let in_strip = (y % p.rows_per_strip) * columns + x;
                let (strip, sample) = if p.planar == 2 {
                    (y / p.rows_per_strip + source_c * strips_per_plane, in_strip)
                } else {
                    (y / p.rows_per_strip, in_strip * source_samples + source_c)
                };
                let at = p.offsets[strip] as usize + sample * width;
                let dest = ((dy * out_columns + dx) * out_samples + c) * width;
                if width == 1 {
                    out[dest] = data[at];
                } else {
                    let v = tiff16(&data[at..], big_endian);
                    out[dest..dest + 2].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
    Ok(RasterPage { info, data: out })
}
pub fn encode_bmp24(w: &mut dyn Write, img: &Rgba8Image) -> Result<(), Error> {
    const HEADER_SIZE: u64 = 14 + 40;
    if img.width == 0 || img.height == 0 || img.pix.len() != img.width * img.height * 4 {
        return Err(Error::new(
            Kind::MalformedImage,
            "BMP requires an image and destination",
        ));
    }
    let (width, height) = (img.width as u64, img.height as u64);
    if width > i32::MAX as u64 || height > i32::MAX as u64 {
        return Err(Error::new(
            Kind::UnsupportedImage,
            "BMP dimensions exceed the format limit",
        ));
    }
    let stride = (width * 3 + 3) & !3;
    let pixel_bytes = stride * height;
    if pixel_bytes > u64::from(u32::MAX) - HEADER_SIZE {
        return Err(Error::new(
            Kind::UnsupportedImage,
            "BMP file size exceeds the format limit",
        ));
    }
    let mut header = [0u8; HEADER_SIZE as usize];
    header[..2].copy_from_slice(b"BM");
    header[2..6].copy_from_slice(&((HEADER_SIZE + pixel_bytes) as u32).to_le_bytes());
    header[10..14].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
    header[14..18].copy_from_slice(&40u32.to_le_bytes());
    header[18..22].copy_from_slice(&(width as u32).to_le_bytes());
    header[22..26].copy_from_slice(&(height as u32).to_le_bytes());
    header[26..28].copy_from_slice(&1u16.to_le_bytes());
    header[28..30].copy_from_slice(&24u16.to_le_bytes());
    header[34..38].copy_from_slice(&(pixel_bytes as u32).to_le_bytes());
    write_bmp_bytes(w, &header)?;
    let mut row = vec![0u8; stride as usize];
    for y in (0..img.height).rev() {
        let source = &img.pix[y * img.width * 4..(y + 1) * img.width * 4];
        for (bgr, rgba) in row
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .zip(source.as_chunks::<4>().0)
        {
            bgr.copy_from_slice(&[rgba[2], rgba[1], rgba[0]]);
        }
        write_bmp_bytes(w, &row)?;
    }
    Ok(())
}
fn write_bmp_bytes(w: &mut dyn Write, bytes: &[u8]) -> Result<(), Error> {
    match w.write(bytes) {
        Ok(n) if n == bytes.len() => Ok(()),
        Ok(_) => Err(Error::other("short write")),
        Err(err) => Err(Error::other(err)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bmp_rows_are_bottom_up_and_padded() {
        let img = Rgba8Image {
            width: 1,
            height: 2,
            pix: vec![1, 2, 3, 255, 4, 5, 6, 255],
        };
        let mut out = Vec::new();
        assert!(encode_bmp24(&mut out, &img).is_ok());
        assert_eq!(out.len(), 54 + 8);
        assert_eq!(&out[54..], &[6, 5, 4, 0, 3, 2, 1, 0]);
        assert!(encode_bmp24(&mut out, &Rgba8Image::default()).is_err());
    }
    #[test]
    fn preview_scales_and_inverts() {
        let page = RasterPage {
            info: RasterPageInfo {
                number: 1,
                rows: 2,
                columns: 4,
                samples: 1,
                bits_stored: 8,
                orientation: 1,
                planar_configuration: 1,
                photometric_interpretation: "MONOCHROME1",
            },
            data: vec![0, 10, 20, 30, 40, 50, 60, 255],
        };
        let small = page.preview(2).ok();
        assert_eq!(
            small.map(|i| (i.width, i.height, i.pix)),
            Some((2, 1, vec![255, 255, 255, 255, 235, 235, 235, 255]))
        );
        assert!(page.preview(0).is_err());
    }
}
