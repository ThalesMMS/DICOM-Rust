use super::frames::{DecompressLimits, FrameReader, limit_error, sample_format_error};
use super::plan9::PLAN9;
use super::{Error, ErrorKind, Image, TAG_ICON_IMAGE_SEQUENCE, representative_frame_index};
use crate::display::{
    self, ColorFrame, GrayImage, LutOptions, PixelFormat, StoredSampleReader, VoiByteMapper,
    VoiFunction, VoiLut,
};
use crate::object::{DataSet, Element, ElementHeader, File, Object, Tag, Value, Vr};
use crate::pixel::Metadata;
use crate::pixel::pixeldata::{PixelData, extract_metadata, extract_view, frame_size, total_size};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WindowMode {
    #[default]
    FromSource,
    Automatic,
    MinMax,
    Invalid(u8),
}
impl WindowMode {
    pub fn from_index(v: i64) -> WindowMode {
        match v {
            0 => WindowMode::FromSource,
            1 => WindowMode::Automatic,
            2 => WindowMode::MinMax,
            _ => WindowMode::Invalid(v as u8),
        }
    }
}
pub use crate::pixel::samples::PixelPadding;
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GenerateOptions {
    pub width: i64,
    pub height: i64,
    pub frame_index: Option<i64>,
    pub window: WindowMode,
    pub min: f64,
    pub max: f64,
    pub ignore_padding: bool,
    pub padding: Option<PixelPadding>,
    pub monochrome: bool,
    pub max_fragments: i64,
    pub limits: DecompressLimits,
    pub big_endian: Option<bool>,
}
pub fn default_generate_limits() -> DecompressLimits {
    DecompressLimits {
        max_frames: 1024,
        max_pixels: 16 << 20,
        max_native_bytes: 64 << 20,
        max_input_bytes: 64 << 20,
        max_expansion_ratio: 1024,
    }
}
const TAG_PIXEL_DATA: Tag = Tag::new(0x7FE0, 0x0010);
const TAG_PIXEL_ASPECT_RATIO: Tag = Tag::new(0x0028, 0x0034);
const TAG_PIXEL_PADDING_VALUE: Tag = Tag::new(0x0028, 0x0120);
const TAG_PIXEL_PADDING_RANGE_LIMIT: Tag = Tag::new(0x0028, 0x0121);
fn sample_reader(m: &Metadata, big_endian: bool) -> Result<StoredSampleReader, Error> {
    StoredSampleReader::new(
        i64::from(m.bits_allocated),
        i64::from(m.bits_stored),
        i64::from(m.high_bit),
        m.pixel_representation != 0,
        big_endian,
    )
    .map_err(sample_format_error)
}
pub fn generate(file: &File, options: &GenerateOptions) -> Result<Element, Error> {
    if file.partial_read.is_some() {
        return Err(Error::bare(ErrorKind::PartialFile));
    }
    let opts = normalize(options)?;
    let big_endian = opts
        .big_endian
        .unwrap_or_else(|| file.dataset.value_big_endian());
    let dataset = &file.dataset;
    let metadata = extract_metadata(dataset).map_err(Error::other)?;
    preflight(dataset, &metadata, &opts.limits)?;
    let photo = crate::textfmt::to_upper(metadata.photometric_interpretation.trim());
    if photo != "MONOCHROME1" && photo != "MONOCHROME2" && opts.window != WindowMode::FromSource {
        return Err(Error::detail(
            ErrorKind::InvalidOptions,
            "window requires monochrome source",
        ));
    }
    preflight_input(dataset, &metadata, &opts)?;
    let mut index = representative_frame_index(dataset, metadata.number_of_frames)
        .unwrap_or(metadata.number_of_frames / 3);
    if let Some(i) = opts.frame_index {
        index = i;
    }
    if index < 0 || index >= metadata.number_of_frames {
        return Err(Error::detail(ErrorKind::InvalidOptions, "FrameIndex"));
    }
    let reader = FrameReader::new(
        file,
        opts.limits,
        opts.limits.max_frames,
        opts.max_fragments,
    )?;
    let (data, metadata) = reader.frame(index)?;
    preflight(dataset, &metadata, &opts.limits)?;
    let photo = crate::textfmt::to_upper(metadata.photometric_interpretation.trim());
    let mono = photo == "MONOCHROME1" || photo == "MONOCHROME2";
    if !mono && opts.window != WindowMode::FromSource {
        return Err(Error::detail(
            ErrorKind::InvalidOptions,
            "window requires monochrome source",
        ));
    }
    let img = if mono {
        Image::Gray(generate_gray(dataset, &metadata, &data, index, &opts)?)
    } else {
        let palette = if photo == "PALETTE COLOR" {
            display::palette_from_object(dataset, LutOptions::default()).map_err(Error::other)?
        } else {
            None
        };
        let rgba = display::render_color(&ColorFrame {
            rows: i64::from(metadata.rows),
            columns: i64::from(metadata.columns),
            pixels: &data,
            photometric: photo.clone(),
            samples_per_pixel: i64::from(metadata.samples_per_pixel),
            planar_configuration: i64::from(metadata.planar_configuration),
            format: PixelFormat {
                bits_allocated: i64::from(metadata.bits_allocated),
                bits_stored: i64::from(metadata.bits_stored),
                high_bit: i64::from(metadata.high_bit),
                signed: metadata.pixel_representation != 0,
                big_endian: false,
            },
            palette,
        })
        .map_err(Error::other)?;
        Image::Rgba(rgba)
    };
    let aspect = generate_aspect(dataset, index, &metadata)?;
    let gray = mono || opts.monochrome;
    let (width, height) = (opts.width as usize, opts.height as usize);
    let scaled = fit_icon(&img, width, height, aspect, gray);
    let (pixels, colors) = if gray {
        (scaled, false)
    } else {
        (to_plan9(&scaled), true)
    };
    Ok(generated_sequence(
        pixels,
        opts.width as u16,
        opts.height as u16,
        colors,
        big_endian,
    ))
}
fn normalize(options: &GenerateOptions) -> Result<GenerateOptions, Error> {
    let mut opts = options.clone();
    if opts.width == 0 {
        opts.width = 64;
    }
    if opts.height == 0 {
        opts.height = 64;
    }
    if opts.width < 1
        || opts.width > 1024
        || opts.height < 1
        || opts.height > 1024
        || matches!(opts.window, WindowMode::Invalid(_))
        || opts.max_fragments < 0
    {
        return Err(Error::bare(ErrorKind::InvalidOptions));
    }
    if opts.window == WindowMode::MinMax {
        min_max_voi(opts.min, opts.max)?;
    }
    if opts.max_fragments == 0 {
        opts.max_fragments = 100_000;
    }
    let l = &mut opts.limits;
    if l.max_frames < 0
        || l.max_pixels < 0
        || l.max_native_bytes < 0
        || l.max_input_bytes < 0
        || l.max_expansion_ratio < 0
    {
        return Err(Error::bare(ErrorKind::InvalidOptions));
    }
    let d = default_generate_limits();
    if l.max_frames == 0 {
        l.max_frames = d.max_frames;
    }
    if l.max_pixels == 0 {
        l.max_pixels = d.max_pixels;
    }
    if l.max_native_bytes == 0 {
        l.max_native_bytes = d.max_native_bytes;
    }
    if l.max_input_bytes == 0 {
        l.max_input_bytes = d.max_input_bytes;
    }
    if l.max_expansion_ratio == 0 {
        l.max_expansion_ratio = d.max_expansion_ratio;
    }
    Ok(opts)
}
fn preflight(obj: &Object, m: &Metadata, limits: &DecompressLimits) -> Result<(), Error> {
    let photo = crate::textfmt::to_upper(m.photometric_interpretation.trim());
    match photo.as_str() {
        "MONOCHROME1" | "MONOCHROME2" | "PALETTE COLOR" => {
            if m.samples_per_pixel != 1 {
                return Err(Error::bare(ErrorKind::UnsupportedSource));
            }
        }
        "RGB" | "YBR_FULL" | "YBR_FULL_422" | "YBR_PARTIAL_422" | "YBR_PARTIAL_420" => {
            if m.samples_per_pixel != 3
                || m.pixel_representation != 0
                || m.planar_configuration > 1
                || !m.planar_configuration_present
            {
                return Err(Error::bare(ErrorKind::UnsupportedSource));
            }
        }
        _ => {
            return Err(Error::detail(
                ErrorKind::UnsupportedSource,
                "photometric interpretation",
            ));
        }
    }
    if m.rows == 0
        || m.columns == 0
        || m.pixel_representation > 1
        || (m.bits_allocated != 8 && m.bits_allocated != 16)
    {
        return Err(Error::bare(ErrorKind::UnsupportedSource));
    }
    sample_reader(m, obj.value_big_endian())?;
    if m.number_of_frames > limits.max_frames {
        return Err(limit_error(
            "MaxFrames",
            m.number_of_frames,
            limits.max_frames,
        ));
    }
    let pixels = i64::from(m.rows) * i64::from(m.columns);
    if m.number_of_frames > limits.max_pixels / pixels {
        let actual = if m.number_of_frames <= i64::MAX / pixels {
            pixels * m.number_of_frames
        } else {
            i64::MAX
        };
        return Err(limit_error("MaxPixels", actual, limits.max_pixels));
    }
    let bytes = total_size(m);
    if bytes <= 0 {
        return Err(Error::bare(ErrorKind::UnsupportedSource));
    }
    if bytes > limits.max_native_bytes {
        return Err(limit_error(
            "MaxNativeBytes",
            bytes,
            limits.max_native_bytes,
        ));
    }
    Ok(())
}
fn preflight_input(obj: &Object, m: &Metadata, opts: &GenerateOptions) -> Result<(), Error> {
    if obj.get(TAG_PIXEL_DATA).is_none() {
        return Err(Error::other("dicom: pixel data element not found"));
    }
    let pixel = extract_view(obj).map_err(Error::other)?;
    let (input, encapsulated) = match pixel {
        PixelData::Native(raw) => (raw.len() as i64, false),
        PixelData::Encapsulated {
            offset_table,
            fragments,
        } => {
            if fragments.len() as i64 > opts.max_fragments {
                return Err(limit_error(
                    "MaxFragments",
                    fragments.len() as i64,
                    opts.max_fragments,
                ));
            }
            let mut input = offset_table.len() as i64;
            for f in fragments {
                if f.len() as i64 > opts.limits.max_input_bytes - input {
                    return Err(limit_error(
                        "MaxInputBytes",
                        i64::MAX,
                        opts.limits.max_input_bytes,
                    ));
                }
                input += f.len() as i64;
            }
            (input, true)
        }
    };
    if input > opts.limits.max_input_bytes {
        return Err(limit_error(
            "MaxInputBytes",
            input,
            opts.limits.max_input_bytes,
        ));
    }
    if !encapsulated {
        let total = total_size(m);
        let excess = input - total;
        if excess < 0 || (excess >= frame_size(m) && !(total % 2 == 1 && excess == 1)) {
            return Err(Error::other(
                "dicom: pixel data size does not match metadata",
            ));
        }
    }
    Ok(())
}
fn min_max_voi(low: f64, high: f64) -> Result<VoiLut, Error> {
    let center = low / 2.0 + high / 2.0;
    let width = high - low;
    if !low.is_finite()
        || !high.is_finite()
        || high <= low
        || high.is_nan()
        || !center.is_finite()
        || !width.is_finite()
    {
        return Err(Error::detail(
            ErrorKind::InvalidOptions,
            "finite increasing min/max required",
        ));
    }
    Ok(VoiLut {
        center,
        width,
        function: VoiFunction::LinearExact,
        ..VoiLut::default()
    })
}
const INVALID_PADDING: &str = "dicom: invalid pixel padding";
pub fn pixel_padding_from_object(
    obj: &Object,
    reader: &StoredSampleReader,
    bits_stored: i64,
) -> Result<Option<PixelPadding>, Error> {
    let (lo, hi) = reader.range();
    let signed = reader.signed();
    let Some(value) = padding_value(obj, TAG_PIXEL_PADDING_VALUE, signed)? else {
        return Ok(None);
    };
    let mut padding = PixelPadding {
        value,
        ..Default::default()
    };
    if let Some(limit) = padding_value(obj, TAG_PIXEL_PADDING_RANGE_LIMIT, signed)? {
        padding.range_limit = limit;
        padding.has_range = true;
    }
    for v in [padding.value, padding.range_limit] {
        if v < lo || v > hi {
            return Err(Error::other(format!(
                "{INVALID_PADDING}: value outside the {bits_stored}-bit {} range",
                if signed { "signed" } else { "unsigned" }
            )));
        }
    }
    Ok(Some(padding))
}
fn padding_value(obj: &Object, tag: Tag, signed: bool) -> Result<Option<i64>, Error> {
    let Some(elem) = obj.get(tag) else {
        return Ok(None);
    };
    match elem.vr() {
        Vr::US if signed => {
            return Err(Error::other(format!(
                "{INVALID_PADDING}: {tag} is US for signed samples"
            )));
        }
        Vr::SS if !signed => {
            return Err(Error::other(format!(
                "{INVALID_PADDING}: {tag} is SS for unsigned samples"
            )));
        }
        Vr::US | Vr::SS | Vr::UN => {}
        vr => {
            return Err(Error::other(format!(
                "{INVALID_PADDING}: {tag} has VR {}",
                vr.as_str()
            )));
        }
    }
    if let Some(raw) = elem.raw()
        && raw.len() == 2
    {
        let pair = [raw[0], raw[1]];
        let v = if obj.value_big_endian() {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        };
        return Ok(Some(if signed {
            i64::from(v as i16)
        } else {
            i64::from(v)
        }));
    }
    Err(Error::other(format!(
        "{INVALID_PADDING}: {tag} must hold one 16-bit value"
    )))
}
fn object_error(e: Option<display::ObjectError>) -> Result<(), Error> {
    match e {
        Some(e) => Err(Error::other(e)),
        None => Ok(()),
    }
}
fn generate_gray(
    obj: &Object,
    m: &Metadata,
    data: &[u8],
    index: i64,
    opts: &GenerateOptions,
) -> Result<GrayImage, Error> {
    let reader = sample_reader(m, false)?;
    let (modality, err) = display::modality_lut_at(obj, index, LutOptions::default());
    object_error(err)?;
    if !modality.slope.is_finite() || !modality.intercept.is_finite() {
        return Err(Error::detail(
            ErrorKind::UnsupportedSource,
            "non-finite modality",
        ));
    }
    let padding = if let Some(p) = opts.padding {
        let (lo, hi) = reader.range();
        if p.value < lo
            || p.value > hi
            || (p.has_range && (p.range_limit < lo || p.range_limit > hi))
        {
            return Err(Error::other(INVALID_PADDING));
        }
        Some(p)
    } else if opts.ignore_padding {
        pixel_padding_from_object(obj, &reader, i64::from(m.bits_stored))?
    } else {
        None
    };
    let mut voi = VoiLut::default();
    match opts.window {
        WindowMode::MinMax => voi = min_max_voi(opts.min, opts.max)?,
        WindowMode::FromSource => {
            let (v, err) = display::voi_at(obj, index, LutOptions::default());
            object_error(err)?;
            if !v.center.is_finite() || !v.width.is_finite() {
                return Err(Error::detail(
                    ErrorKind::UnsupportedSource,
                    "non-finite VOI",
                ));
            }
            voi = v;
        }
        _ => {}
    }
    let width = reader.width();
    let count = usize::from(m.rows) * usize::from(m.columns);
    let automatic = voi.lut.is_none() && voi.width <= 0.0;
    let mut constant = false;
    if automatic {
        let (mut low, mut high, mut usable) = (f64::INFINITY, f64::NEG_INFINITY, false);
        for i in 0..count {
            let stored = reader.value(&data[i * width..]);
            if padding.is_some_and(|p| p.contains(stored)) {
                continue;
            }
            let value = modality.apply(stored);
            if !value.is_finite() {
                return Err(Error::detail(
                    ErrorKind::UnsupportedSource,
                    "non-finite modality value",
                ));
            }
            low = crate::fmath::min(low, value);
            high = crate::fmath::max(high, value);
            usable = true;
        }
        if !usable {
            return Err(Error::bare(ErrorKind::NoUsablePixels));
        }
        constant = low == high;
        if !constant {
            voi = min_max_voi(low, high)?;
        }
    }
    let mapper = VoiByteMapper::new(voi);
    let mut out = vec![0u8; count];
    let mut usable = false;
    let mono1 = m
        .photometric_interpretation
        .eq_ignore_ascii_case("MONOCHROME1");
    for (i, px) in out.iter_mut().enumerate() {
        let stored = reader.value(&data[i * width..]);
        if padding.is_some_and(|p| p.contains(stored)) {
            continue;
        }
        usable = true;
        let value = modality.apply(stored);
        if !value.is_finite() {
            return Err(Error::detail(
                ErrorKind::UnsupportedSource,
                "non-finite modality value",
            ));
        }
        *px = if constant { 128 } else { mapper.byte(value) };
        if mono1 {
            *px = 255 - *px;
        }
    }
    if !usable {
        return Err(Error::bare(ErrorKind::NoUsablePixels));
    }
    Ok(GrayImage {
        width: usize::from(m.columns),
        height: usize::from(m.rows),
        pixels: out,
    })
}
fn generate_aspect(obj: &Object, index: i64, m: &Metadata) -> Result<f64, Error> {
    let (mut row, mut column) = (1.0, 1.0);
    if let Some(e) = obj.frame_geometry_at(index).pixel_spacing_err {
        return Err(Error::other(e));
    }
    let spacing = obj.pixel_spacing_at(index);
    if spacing.valid() {
        row = spacing.row_spacing;
        column = spacing.column_spacing;
    } else if obj.has(TAG_PIXEL_ASPECT_RATIO) {
        match obj.get_ints(TAG_PIXEL_ASPECT_RATIO) {
            Ok(r) if r.len() == 2 && r[0] > 0 && r[1] > 0 => {
                row = r[0] as f64;
                column = r[1] as f64;
            }
            _ => {
                return Err(Error::detail(
                    ErrorKind::UnsupportedSource,
                    "PixelAspectRatio",
                ));
            }
        }
    }
    let aspect = f64::from(m.columns) / f64::from(m.rows) * (column / row);
    if !aspect.is_finite() || aspect <= 0.0 {
        return Err(Error::detail(ErrorKind::UnsupportedSource, "aspect ratio"));
    }
    Ok(aspect)
}
fn gray_y(r: u32, g: u32, b: u32) -> u32 {
    (19595 * r + 38470 * g + 7471 * b + (1 << 15)) >> 24
}
fn fit_icon(img: &Image, width: usize, height: usize, aspect: f64, gray: bool) -> Vec<u8> {
    let (mut tw, mut th) = (width, height);
    if aspect > width as f64 / height as f64 {
        th = ((width as f64 / aspect).round() as i64).max(1) as usize;
    } else {
        tw = ((height as f64 * aspect).round() as i64).max(1) as usize;
    }
    let ox = (width as i64 - tw as i64) / 2;
    let oy = (height as i64 - th as i64) / 2;
    let (bw, bh) = img.size();
    let channels = if gray { 1 } else { 4 };
    let mut out = vec![0u8; width * height * channels];
    if !gray {
        for p in out.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
    }
    for ty in 0..th {
        let y0 = ty * bh / th;
        let y1 = (y0 + 1).max((ty + 1) * bh / th);
        for tx in 0..tw {
            let x0 = tx * bw / tw;
            let x1 = (x0 + 1).max((tx + 1) * bw / tw);
            let (mut r, mut g, mut b, mut count) = (0u64, 0u64, 0u64, 0u64);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (mut cr, mut cg, mut cb, _) = img.rgba64(x, y);
                    if gray {
                        let v = gray_y(cr, cg, cb) * 0x101;
                        (cr, cg, cb) = (v, v, v);
                    }
                    r += u64::from(cr);
                    g += u64::from(cg);
                    b += u64::from(cb);
                    count += 1;
                }
            }
            let (px, py) = (ox + tx as i64, oy + ty as i64);
            if px < 0 || py < 0 || px as usize >= width || py as usize >= height {
                continue;
            }
            let at = (py as usize * width + px as usize) * channels;
            if gray {
                out[at] = ((r / 257 + count / 2) / count) as u8;
            } else {
                let c = |v: u64| (((v + count / 2) / count) >> 8) as u8;
                out[at] = c(r);
                out[at + 1] = c(g);
                out[at + 2] = c(b);
                out[at + 3] = 255;
            }
        }
    }
    out
}
fn to_plan9(rgba: &[u8]) -> Vec<u8> {
    let sq = |x: i32, y: i32| {
        let d = (x - y) as u32;
        d.wrapping_mul(d) >> 2
    };
    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let (er, eg, eb, ea) = (
                i32::from(p[0]) * 0x101,
                i32::from(p[1]) * 0x101,
                i32::from(p[2]) * 0x101,
                i32::from(p[3]) * 0x101,
            );
            let mut best = (0usize, u32::MAX);
            for (index, c) in PLAN9.iter().enumerate() {
                let sum = sq(er, i32::from(c[0]) * 0x101)
                    .wrapping_add(sq(eg, i32::from(c[1]) * 0x101))
                    .wrapping_add(sq(eb, i32::from(c[2]) * 0x101))
                    .wrapping_add(sq(ea, 0xFFFF));
                if sum < best.1 {
                    best = (index, sum);
                    if sum == 0 {
                        break;
                    }
                }
            }
            best.0 as u8
        })
        .collect()
}
fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, raw.len() as u32),
        value: Value::Raw(raw),
    }
}
fn text_element(tag: Tag, vr: Vr, values: &[&str]) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(values.iter().map(|s| s.to_string()).collect()),
    }
}
fn generated_sequence(
    pixels: Vec<u8>,
    width: u16,
    height: u16,
    colors: bool,
    big_endian: bool,
) -> Element {
    let put = |v: u16| {
        if big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let us = |tag: Tag, values: &[u16]| {
        raw_element(tag, Vr::US, values.iter().flat_map(|&v| put(v)).collect())
    };
    let photo = if colors {
        "PALETTE COLOR"
    } else {
        "MONOCHROME2"
    };
    let mut elements = vec![
        us(Tag::new(0x0028, 0x0002), &[1]),
        text_element(Tag::new(0x0028, 0x0004), Vr::CS, &[photo]),
        us(Tag::new(0x0028, 0x0010), &[height]),
        us(Tag::new(0x0028, 0x0011), &[width]),
        text_element(Tag::new(0x0028, 0x0034), Vr::IS, &["1", "1"]),
        us(Tag::new(0x0028, 0x0100), &[8]),
        us(Tag::new(0x0028, 0x0101), &[8]),
        us(Tag::new(0x0028, 0x0102), &[7]),
        us(Tag::new(0x0028, 0x0103), &[0]),
    ];
    if colors {
        for ch in 0..3u16 {
            elements.push(us(Tag::new(0x0028, 0x1101 + ch), &[256, 0, 8]));
        }
        for ch in 0..3usize {
            let entries: Vec<u16> = PLAN9.iter().map(|c| u16::from(c[ch])).collect();
            let mut data = display::encode_lut_data(&entries, 8);
            if big_endian {
                for pair in data.as_chunks_mut::<2>().0 {
                    pair.swap(0, 1);
                }
            }
            elements.push(raw_element(
                Tag::new(0x0028, 0x1201 + ch as u16),
                Vr::OW,
                data,
            ));
        }
    }
    elements.push(raw_element(TAG_PIXEL_DATA, Vr::OB, pixels));
    Element {
        header: ElementHeader {
            tag: TAG_ICON_IMAGE_SEQUENCE,
            vr: Vr::SQ,
            length: 0,
            length_set: false,
        },
        value: Value::Sequence(vec![DataSet {
            elements,
            item_offset: None,
        }]),
    }
}
