use super::jpeg_stream::parse_jpeg_stream_limited;
use super::png;
use super::{
    Error, ImageLayout, JpegOptions, Kind, Report, SECONDARY_CAPTURE_IMAGE_STORAGE,
    TAG_BURNED_IN_ANNOTATION, TAG_ICC_PROFILE, TAG_LOSSY_COMPRESSION_METHOD, TAG_MODALITY,
    TAG_PHOTOMETRIC, TAG_SERIES_DESCRIPTION, TAG_SERIES_INSTANCE_UID, TAG_SERIES_NUMBER,
    TAG_SOP_CLASS_UID, TAG_SOP_INSTANCE_UID, TAG_SPECIFIC_CHARACTER_SET, TAG_STUDY_INSTANCE_UID,
    VL_PHOTOGRAPHIC_IMAGE_STORAGE, build_image, jpeg_photometric, jpeg_transfer_syntax,
    raw_element, registered, string_element, strings_element, stripped_stream,
};
#[cfg(feature = "jpeg-ls")]
use crate::codec::jpegls;
use crate::numtext::format_float;
use crate::object::charset_conversion::{CharsetConversionOptions, convert_text_to_utf8};
use crate::object::types::{DataSet, Element, ElementHeader, Tag, Value, Vr};
use crate::object::{File, Object, is_valid_uid, transfer};
use crate::pixel::encapsulated;
use crate::validation;
use crate::video;
use std::collections::HashSet;
pub const MULTI_FRAME_GRAYSCALE_BYTE_SC: &str = "1.2.840.10008.5.1.4.1.1.7.2";
pub const MULTI_FRAME_GRAYSCALE_WORD_SC: &str = "1.2.840.10008.5.1.4.1.1.7.3";
pub const MULTI_FRAME_TRUE_COLOR_SC: &str = "1.2.840.10008.5.1.4.1.1.7.4";
pub const VIDEO_PHOTOGRAPHIC_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.77.1.4.1";
const EXPLICIT_VR_LITTLE_ENDIAN: &str = "1.2.840.10008.1.2.1";
#[cfg(feature = "jpeg-ls")]
const JPEG_LS_LOSSLESS: &str = "1.2.840.10008.1.2.4.80";
#[cfg(feature = "jpeg-ls")]
const JPEG_LS_NEAR_LOSSLESS: &str = "1.2.840.10008.1.2.4.81";
const JPEG_2000_LOSSLESS_ONLY: &str = "1.2.840.10008.1.2.4.90";
const JPEG_2000: &str = "1.2.840.10008.1.2.4.91";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Auto,
    Png,
    Bmp,
    Pnm,
    Pgx,
    Raw,
    Jpeg,
    J2k,
    Jls,
    Video,
    Tiff,
}
impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Auto => "auto",
            Format::Png => "png",
            Format::Bmp => "bmp",
            Format::Pnm => "pnm",
            Format::Pgx => "pgx",
            Format::Raw => "raw",
            Format::Jpeg => "jpeg",
            Format::J2k => "j2k",
            Format::Jls => "jls",
            Format::Video => "video",
            Format::Tiff => "tiff",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportLimits {
    pub max_input_bytes: i64,
    pub max_decoded_bytes: i64,
    pub max_pixels: i64,
    pub max_frames: i64,
}
impl ImportLimits {
    pub(super) fn normalized(mut self) -> Result<ImportLimits, Error> {
        if self.max_input_bytes < 0
            || self.max_input_bytes > 1 << 30
            || self.max_decoded_bytes < 0
            || self.max_pixels < 0
            || self.max_frames < 0
            || self.max_frames > 100_000
        {
            return Err(Error::bare(Kind::InvalidOption));
        }
        if self.max_input_bytes == 0 {
            self.max_input_bytes = 256 << 20;
        }
        if self.max_decoded_bytes == 0 {
            self.max_decoded_bytes = 512 << 20;
        }
        if self.max_pixels == 0 {
            self.max_pixels = 16 << 20;
        }
        if self.max_frames == 0 {
            self.max_frames = 1024;
        }
        Ok(self)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnatomicRegion {
    pub code_value: String,
    pub coding_scheme_designator: String,
    pub code_meaning: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawOptions {
    pub rows: i64,
    pub columns: i64,
    pub frames: i64,
    pub samples: i64,
    pub bits_allocated: i64,
    pub bits_stored: i64,
    pub signed: bool,
    pub byte_order: String,
    pub offset: i64,
}
#[derive(Clone, Debug, Default)]
pub struct ImageOptions {
    pub anatomic_region: Option<AnatomicRegion>,
    pub jpeg: JpegOptions,
    pub format: Format,
    pub limits: ImportLimits,
    pub raw: RawOptions,
    pub series_source: Option<Object>,
    pub j2k_lossless: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub report: Report,
    pub format: Format,
    pub rows: i64,
    pub columns: i64,
    pub frames: i64,
    pub bits_stored: i64,
    pub encapsulated: bool,
    pub decoded: bool,
    pub lossless_declared: bool,
    pub video_container: &'static str,
}
pub(super) struct SourceFrame {
    pub(super) layout: ImageLayout,
    pub(super) data: Vec<u8>,
    syntax: transfer::Syntax,
    icc: Option<Vec<u8>>,
    report: Report,
    pub(super) format: Format,
}
fn bare(kind: Kind) -> Error {
    Error::bare(kind)
}
fn unsupported(detail: &str) -> Error {
    Error::new(Kind::UnsupportedImage, detail)
}
fn malformed(detail: &str) -> Error {
    Error::new(Kind::MalformedImage, detail)
}
fn native_frame(layout: ImageLayout, data: Vec<u8>, format: Format) -> Result<SourceFrame, Error> {
    Ok(SourceFrame {
        layout,
        data,
        syntax: registered(EXPLICIT_VR_LITTLE_ENDIAN)?,
        icc: None,
        report: Report::default(),
        format,
    })
}
pub fn from_images(inputs: &[&[u8]], opts: &ImageOptions) -> Result<(File, ImportReport), Error> {
    let limits = opts.limits.normalized()?;
    if inputs.is_empty() {
        return Err(bare(Kind::InvalidOption));
    }
    if inputs.len() as i64 > limits.max_frames {
        return Err(bare(Kind::ImportLimit));
    }
    if inputs.len() > 1 && page_vector_bytes(inputs.len() as i64) > 65534 {
        return Err(bare(Kind::ImportLimit));
    }
    let mut total = 0i64;
    for b in inputs {
        if b.len() as i64 > limits.max_input_bytes - total {
            return Err(bare(Kind::ImportLimit));
        }
        total += b.len() as i64;
    }
    let mut jpeg = opts.jpeg.clone();
    if !valid_import_text(&jpeg) {
        return Err(bare(Kind::InvalidOption));
    }
    if let Some(region) = &opts.anatomic_region
        && !valid_region(region)
    {
        return Err(bare(Kind::InvalidOption));
    }
    inherit_series(opts, &mut jpeg)?;
    for uid in [
        &jpeg.sop_class_uid,
        &jpeg.sop_instance_uid,
        &jpeg.study_instance_uid,
        &jpeg.series_instance_uid,
    ] {
        if !uid.is_empty() && !is_valid_uid(uid.as_bytes()) {
            return Err(bare(Kind::InvalidOption));
        }
    }
    if !matches!(jpeg.burned_in_annotation.as_str(), "" | "YES" | "NO")
        || !matches!(jpeg.laterality.as_str(), "" | "R" | "L")
    {
        return Err(bare(Kind::InvalidOption));
    }
    if jpeg.burned_in_annotation.is_empty() {
        jpeg.burned_in_annotation = "YES".to_string();
    }
    if let Some(source) = &jpeg.common.source
        && !jpeg.sop_instance_uid.is_empty()
        && source.get_string(TAG_SOP_INSTANCE_UID).unwrap_or_default() == jpeg.sop_instance_uid
    {
        return Err(bare(Kind::InvalidOption));
    }
    let format = opts.format;
    if format != Format::Raw && opts.raw != RawOptions::default() {
        return Err(bare(Kind::InvalidOption));
    }
    let jpeg_only = jpeg.keep_appn
        || jpeg.keep_comments
        || jpeg.discard_icc_profile
        || jpeg.allow_retired_progressive;
    if format == Format::Raw && (opts.j2k_lossless || jpeg_only) {
        return Err(bare(Kind::InvalidOption));
    }
    if format == Format::Video
        || (format == Format::Auto && inputs.len() == 1 && is_video(inputs[0]))
    {
        return import_video(inputs, opts, jpeg, limits);
    }
    let mut frames: Vec<SourceFrame> = Vec::new();
    let mut decoded = 0i64;
    for b in inputs {
        let mut remaining = limits;
        remaining.max_decoded_bytes -= decoded;
        remaining.max_frames -= frames.len() as i64;
        if remaining.max_frames <= 0 {
            return Err(bare(Kind::ImportLimit));
        }
        let next = if format == Format::Raw {
            read_raw(b, &opts.raw, remaining)?
        } else {
            vec![read_image(b, format, &jpeg, opts.j2k_lossless, remaining)?]
        };
        if next.len() as i64 > limits.max_frames - frames.len() as i64 {
            return Err(bare(Kind::ImportLimit));
        }
        for f in next {
            let n = admit_image(&f.layout, limits)?;
            if n > limits.max_decoded_bytes - decoded {
                return Err(bare(Kind::ImportLimit));
            }
            if !f.syntax.encapsulated {
                native_pixel_data_length(decoded + n)?;
            }
            decoded += n;
            if let Some(first) = frames.first()
                && !same_layout(first, &f)
            {
                return Err(unsupported("frames differ"));
            }
            frames.push(f);
        }
    }
    if frames.is_empty() {
        return Err(bare(Kind::MalformedImage));
    }
    let count = frames.len() as i64;
    let class = if jpeg.sop_class_uid.is_empty() {
        if count > 1 {
            multi_class(&frames[0].layout)?
        } else {
            SECONDARY_CAPTURE_IMAGE_STORAGE
        }
        .to_string()
    } else {
        jpeg.sop_class_uid.clone()
    };
    check_image_class(&class, &frames[0], count)?;
    let multi = class != SECONDARY_CAPTURE_IMAGE_STORAGE && class != VL_PHOTOGRAPHIC_IMAGE_STORAGE;
    if multi && page_vector_bytes(count) > 65534 {
        return Err(bare(Kind::ImportLimit));
    }
    jpeg.sop_class_uid = class;
    let encapsulated = frames[0].syntax.encapsulated;
    let mut rep = frames[0].report.clone();
    rep.removed_segments.clear();
    rep.trailing_bytes_removed = 0;
    for f in &frames {
        rep.trailing_bytes_removed += f.report.trailing_bytes_removed;
        for removed in &f.report.removed_segments {
            match rep
                .removed_segments
                .iter_mut()
                .find(|p| p.marker == removed.marker && p.identifier == removed.identifier)
            {
                Some(p) => {
                    p.count += removed.count;
                    p.bytes += removed.bytes;
                }
                None => rep.removed_segments.push(removed.clone()),
            }
        }
    }
    rep.transfer_syntax_uid = frames[0].syntax.uid.to_string();
    rep.photometric_interpretation = frames[0].layout.photometric;
    rep.process = frames[0].layout.process;
    if encapsulated {
        rep.pixel_data_bytes = frames
            .iter()
            .map(|f| (f.data.len() + (f.data.len() & 1)) as i64)
            .sum();
        rep.padded = frames.iter().any(|f| f.data.len() & 1 != 0);
    } else {
        rep.pixel_data_bytes = decoded + (decoded & 1);
        rep.padded = decoded & 1 != 0;
    }
    let mut layout = frames[0].layout.clone();
    layout.frame_count = count;
    layout.payload_bytes = frames.iter().map(|f| f.data.len() as i64).sum();
    let syntax = frames[0].syntax;
    let source_format = frames[0].format;
    let icc = frames[0].icc.take();
    let value = if encapsulated {
        Value::Fragments {
            offset_table: Vec::new(),
            fragments: frames
                .into_iter()
                .map(|f| {
                    let mut payload = f.data;
                    if payload.len() & 1 != 0 {
                        payload.push(0);
                    }
                    payload
                })
                .collect(),
        }
    } else {
        let mut data = Vec::with_capacity(native_pixel_data_length(decoded)?);
        for f in &frames {
            data.extend_from_slice(&f.data);
        }
        data.resize(native_pixel_data_length(decoded)?, 0);
        Value::Raw(data)
    };
    let mut file = build_image(&jpeg, &layout, value, syntax, &mut rep, icc)?;
    if let Some(region) = &opts.anatomic_region {
        add_region(&mut file.dataset, region);
    }
    if multi {
        add_multiframe(&mut file.dataset, count);
    }
    check_imported(&file)?;
    let report = ImportReport {
        report: rep,
        format: source_format,
        rows: layout.rows,
        columns: layout.columns,
        frames: count,
        bits_stored: layout.bits,
        encapsulated,
        decoded: !encapsulated,
        lossless_declared: source_format == Format::J2k && opts.j2k_lossless,
        video_container: "",
    };
    Ok((file, report))
}
pub(super) fn admit_image(l: &ImageLayout, limits: ImportLimits) -> Result<i64, Error> {
    if l.rows <= 0
        || l.columns <= 0
        || l.rows > 65535
        || l.columns > 65535
        || l.bits < 1
        || l.bits > 16
        || (l.samples != 1 && l.samples != 3)
        || l.signed < 0
        || l.signed > 1
        || (l.samples == 3 && l.signed != 0)
    {
        return Err(bare(Kind::UnsupportedImage));
    }
    let pixels = l.rows * l.columns;
    if pixels > limits.max_pixels {
        return Err(bare(Kind::ImportLimit));
    }
    let allocated = l.effective_allocated();
    if (allocated != 8 && allocated != 16) || l.bits > allocated {
        return Err(bare(Kind::UnsupportedImage));
    }
    let n = pixels * l.samples * (allocated / 8);
    if n > limits.max_decoded_bytes || n > 4_294_967_294 {
        return Err(bare(Kind::ImportLimit));
    }
    Ok(n)
}
fn same_layout(a: &SourceFrame, b: &SourceFrame) -> bool {
    let (x, y) = (&a.layout, &b.layout);
    x.rows == y.rows
        && x.columns == y.columns
        && x.samples == y.samples
        && x.bits == y.bits
        && x.signed == y.signed
        && x.effective_allocated() == y.effective_allocated()
        && x.photometric == y.photometric
        && x.lossless == y.lossless
        && x.lossy_unknown == y.lossy_unknown
        && x.method == y.method
        && a.icc.as_deref().unwrap_or_default() == b.icc.as_deref().unwrap_or_default()
        && a.syntax.uid == b.syntax.uid
}
fn multi_class(l: &ImageLayout) -> Result<&'static str, Error> {
    if l.signed == 0 {
        let allocated = l.effective_allocated();
        if l.samples == 1 && l.bits == 8 && allocated == 8 {
            return Ok(MULTI_FRAME_GRAYSCALE_BYTE_SC);
        }
        if l.samples == 1 && l.bits > 8 && allocated == 16 {
            return Ok(MULTI_FRAME_GRAYSCALE_WORD_SC);
        }
        if l.samples == 3
            && l.bits == 8
            && allocated == 8
            && matches!(
                l.photometric,
                "RGB" | "YBR_FULL_422" | "YBR_RCT" | "YBR_ICT"
            )
        {
            return Ok(MULTI_FRAME_TRUE_COLOR_SC);
        }
    }
    Err(bare(Kind::UnsupportedSopClass))
}
fn check_image_class(class: &str, f: &SourceFrame, n: i64) -> Result<(), Error> {
    let l = &f.layout;
    match class {
        SECONDARY_CAPTURE_IMAGE_STORAGE => {
            if n != 1 {
                return Err(bare(Kind::UnsupportedSopClass));
            }
        }
        VL_PHOTOGRAPHIC_IMAGE_STORAGE => {
            if n != 1 || l.bits != 8 || l.effective_allocated() != 8 || l.signed != 0 {
                return Err(bare(Kind::UnsupportedSopClass));
            }
            if l.samples == 3 && !l.lossless && l.photometric == "RGB" {
                return Err(bare(Kind::UnsupportedComponents));
            }
        }
        MULTI_FRAME_GRAYSCALE_BYTE_SC
        | MULTI_FRAME_GRAYSCALE_WORD_SC
        | MULTI_FRAME_TRUE_COLOR_SC => {
            if multi_class(l).ok() != Some(class) {
                return Err(bare(Kind::UnsupportedSopClass));
            }
        }
        _ => return Err(bare(Kind::UnsupportedSopClass)),
    }
    Ok(())
}
fn tag_value(tag: Tag) -> Vec<u8> {
    let mut raw = tag.group.to_le_bytes().to_vec();
    raw.extend_from_slice(&tag.element.to_le_bytes());
    raw
}
fn add_multiframe(ds: &mut Object, n: i64) {
    ds.put(string_element(
        Tag::new(0x0028, 0x0008),
        Vr::IS,
        &n.to_string(),
    ));
    ds.put(raw_element(
        Tag::new(0x0028, 0x0009),
        Vr::AT,
        tag_value(Tag::new(0x0018, 0x2001)),
    ));
    ds.put(strings_element(
        Tag::new(0x0018, 0x2001),
        Vr::IS,
        (1..=n).map(|page| page.to_string()).collect(),
    ));
    if !ds.has(TAG_BURNED_IN_ANNOTATION) {
        ds.put(string_element(TAG_BURNED_IN_ANNOTATION, Vr::CS, "YES"));
    }
    if ds.get_string(TAG_SOP_CLASS_UID).as_deref() == Some(MULTI_FRAME_GRAYSCALE_WORD_SC) {
        ds.put(string_element(Tag::new(0x0028, 0x1052), Vr::DS, "0"));
        ds.put(string_element(Tag::new(0x0028, 0x1053), Vr::DS, "1"));
        ds.put(string_element(Tag::new(0x0028, 0x1054), Vr::LO, "US"));
    }
    if ds.get_string(TAG_PHOTOMETRIC).as_deref() == Some("MONOCHROME2") {
        ds.put(string_element(Tag::new(0x2050, 0x0020), Vr::CS, "IDENTITY"));
    }
}
fn check_iod(ds: &Object) -> Result<(), Error> {
    let (report, err) = validation::iod::validate(
        ds,
        &validation::iod::Options {
            check_values: true,
            ..validation::iod::Options::default()
        },
    );
    if let Some(err) = err {
        return Err(Error::other(err));
    }
    match report.errors().first() {
        Some(finding) => Err(Error::new(
            Kind::InvalidOption,
            format!("output IOD {finding}"),
        )),
        None => Ok(()),
    }
}
fn inherit_series(opts: &ImageOptions, jpeg: &mut JpegOptions) -> Result<(), Error> {
    let Some(source) = &opts.series_source else {
        return Ok(());
    };
    let (ds, _) = convert_text_to_utf8(source, CharsetConversionOptions::default())
        .map_err(|failure| Error::other(&failure.error))?;
    let study = ds.get_string(TAG_STUDY_INSTANCE_UID).unwrap_or_default();
    let series = ds.get_string(TAG_SERIES_INSTANCE_UID).unwrap_or_default();
    if !is_valid_uid(study.as_bytes()) || !is_valid_uid(series.as_bytes()) {
        return Err(bare(Kind::InvalidOption));
    }
    if !jpeg.study_instance_uid.is_empty()
        && jpeg.study_instance_uid != study
        && jpeg.series_instance_uid.is_empty()
    {
        return Err(bare(Kind::InvalidOption));
    }
    if jpeg.common.source.is_some() {
        return Err(bare(Kind::InvalidOption));
    }
    for (tag, dst) in [
        (TAG_SERIES_INSTANCE_UID, &mut jpeg.series_instance_uid),
        (TAG_STUDY_INSTANCE_UID, &mut jpeg.study_instance_uid),
        (TAG_SERIES_NUMBER, &mut jpeg.series_number),
        (TAG_SERIES_DESCRIPTION, &mut jpeg.series_description),
        (TAG_MODALITY, &mut jpeg.modality),
    ] {
        if dst.is_empty() {
            *dst = ds.get_string(tag).unwrap_or_default();
        }
    }
    jpeg.common.source = Some(ds);
    Ok(())
}
pub(super) fn detect_image(b: &[u8]) -> Format {
    if b.starts_with(&[137, b'P', b'N', b'G', 13, 10, 26, 10]) {
        Format::Png
    } else if b.starts_with(b"BM") {
        Format::Bmp
    } else if b.starts_with(b"PG ") {
        Format::Pgx
    } else if b.len() >= 2 && b[0] == b'P' && matches!(b[1], b'2' | b'3' | b'5' | b'6') {
        Format::Pnm
    } else if b.len() >= 2 && b[0] == 255 && b[1] == 0x4f {
        Format::J2k
    } else if b.len() >= 2 && b[0] == 255 && b[1] == 0xd8 {
        detect_jpeg_family(b)
    } else {
        Format::Auto
    }
}
pub(super) fn read_image(
    b: &[u8],
    format: Format,
    jpeg: &JpegOptions,
    j2k_lossless: bool,
    limits: ImportLimits,
) -> Result<SourceFrame, Error> {
    let format = if format == Format::Auto {
        detect_image(b)
    } else {
        format
    };
    let jpeg_only = jpeg.keep_appn
        || jpeg.keep_comments
        || jpeg.discard_icc_profile
        || jpeg.allow_retired_progressive;
    if (format != Format::Jpeg && jpeg_only) || (j2k_lossless && format != Format::J2k) {
        return Err(bare(Kind::InvalidOption));
    }
    match format {
        Format::Bmp => read_bmp(b, limits),
        Format::Png => read_png(b, limits),
        Format::Pnm => read_portable(b, limits),
        Format::Pgx => read_pgx(b, limits),
        Format::Jpeg | Format::J2k | Format::Jls => {
            read_compressed(b, format, jpeg, j2k_lossless, limits)
        }
        _ => Err(unsupported("input format")),
    }
}
fn is_video(b: &[u8]) -> bool {
    (b.len() >= 12 && &b[4..8] == b"ftyp") || b.starts_with(&[0, 0, 1, 0xba])
}
fn import_video(
    inputs: &[&[u8]],
    opts: &ImageOptions,
    mut jpeg: JpegOptions,
    limits: ImportLimits,
) -> Result<(File, ImportReport), Error> {
    let jpeg_only = jpeg.keep_appn
        || jpeg.keep_comments
        || jpeg.discard_icc_profile
        || jpeg.allow_retired_progressive;
    if inputs.len() != 1
        || opts.j2k_lossless
        || jpeg_only
        || (!jpeg.sop_class_uid.is_empty()
            && jpeg.sop_class_uid != VIDEO_PHOTOGRAPHIC_IMAGE_STORAGE)
    {
        return Err(bare(Kind::InvalidOption));
    }
    let Some(region) = &opts.anatomic_region else {
        return Err(Error::new(
            Kind::InvalidOption,
            "video requires anatomic region code",
        ));
    };
    let input = inputs[0];
    let info = video::inspect_source(
        input,
        video::ImportLimits {
            max_bytes: limits.max_input_bytes,
            max_frames: limits.max_frames,
            max_pixels: limits.max_pixels,
            max_decoded_bytes: limits.max_decoded_bytes,
        },
    )
    .map_err(Error::other)?;
    let layout = ImageLayout {
        rows: info.rows,
        columns: info.columns,
        samples: 3,
        bits: 8,
        photometric: "YBR_PARTIAL_420",
        process: "video",
        payload_bytes: input.len() as i64,
        frame_count: info.frames,
        method: info.compression_method,
        ..ImageLayout::default()
    };
    jpeg.sop_class_uid = VIDEO_PHOTOGRAPHIC_IMAGE_STORAGE.to_string();
    let mut data = input.to_vec();
    if data.len() & 1 != 0 {
        data.push(0);
    }
    let mut rep = Report {
        process: "video",
        transfer_syntax_uid: info.transfer_syntax.uid.to_string(),
        photometric_interpretation: layout.photometric,
        pixel_data_bytes: data.len() as i64,
        padded: input.len() & 1 != 0,
        ..Report::default()
    };
    let value = Value::Fragments {
        offset_table: Vec::new(),
        fragments: vec![data],
    };
    let mut file = build_image(&jpeg, &layout, value, info.transfer_syntax, &mut rep, None)?;
    let ds = &mut file.dataset;
    add_region(ds, region);
    ds.put(string_element(
        Tag::new(0x0028, 0x0008),
        Vr::IS,
        &info.frames.to_string(),
    ));
    ds.put(raw_element(
        Tag::new(0x0028, 0x0009),
        Vr::AT,
        tag_value(Tag::new(0x0018, 0x1063)),
    ));
    ds.put(string_element(
        Tag::new(0x0018, 0x1063),
        Vr::DS,
        &format_float(1000.0 / info.frame_rate, 'g', 10),
    ));
    ds.put(string_element(
        TAG_LOSSY_COMPRESSION_METHOD,
        Vr::CS,
        info.compression_method,
    ));
    check_imported(&file)?;
    let report = ImportReport {
        report: rep,
        format: Format::Video,
        rows: layout.rows,
        columns: layout.columns,
        frames: info.frames,
        bits_stored: 8,
        encapsulated: true,
        decoded: false,
        lossless_declared: false,
        video_container: info.container,
    };
    Ok((file, report))
}
fn detect_jpeg_family(b: &[u8]) -> Format {
    let mut at = 2;
    while at + 4 <= b.len() && at < 65536 {
        if b[at] != 255 {
            return Format::Jpeg;
        }
        let code = b[at + 1];
        if code == 0xf7 {
            return Format::Jls;
        }
        if code == 0xda || (0xc0..=0xcf).contains(&code) {
            return Format::Jpeg;
        }
        let n = usize::from(b[at + 2]) << 8 | usize::from(b[at + 3]);
        if n < 2 || n > b.len() - at - 2 {
            return Format::Jpeg;
        }
        at += 2 + n;
    }
    Format::Jpeg
}
fn valid_region(c: &AnatomicRegion) -> bool {
    [
        (&c.code_value, 16),
        (&c.coding_scheme_designator, 16),
        (&c.code_meaning, 64),
    ]
    .iter()
    .all(|(v, cap)| {
        !v.trim().is_empty() && v.len() <= *cap && !v.contains(['\\', '\r', '\n', '\0'])
    })
}
fn add_region(ds: &mut Object, c: &AnatomicRegion) {
    let values = [&c.code_value, &c.coding_scheme_designator, &c.code_meaning];
    if values.iter().any(|v| !v.is_ascii()) {
        ds.put(string_element(
            TAG_SPECIFIC_CHARACTER_SET,
            Vr::CS,
            "ISO_IR 192",
        ));
    }
    let elements = vec![
        string_element(Tag::new(0x0008, 0x0100), Vr::SH, &c.code_value),
        string_element(
            Tag::new(0x0008, 0x0102),
            Vr::SH,
            &c.coding_scheme_designator,
        ),
        string_element(Tag::new(0x0008, 0x0104), Vr::LO, &c.code_meaning),
    ];
    ds.put(Element {
        header: ElementHeader {
            tag: Tag::new(0x0008, 0x2218),
            vr: Vr::SQ,
            length: 0,
            length_set: false,
        },
        value: Value::Sequence(vec![DataSet {
            elements,
            item_offset: None,
        }]),
    });
}
pub(super) fn check_imported(f: &File) -> Result<(), Error> {
    let mut ds = f.dataset.to_data_set();
    ds.elements
        .retain(|e| e.header.tag != Tag::PIXEL_DATA && e.header.tag != TAG_ICC_PROFILE);
    let report = validation::validate_data_set(&ds, validation::Options::default());
    if stops_on_error(&report) {
        return Err(Error::new(Kind::InvalidOption, "output metadata syntax"));
    }
    check_iod(&f.dataset)
}
fn stops_on_error(report: &validation::Report) -> bool {
    let Some(first) = report.findings.first() else {
        return report.has_errors();
    };
    let structural = |f: &validation::Finding| {
        f.code == validation::CODE_DUPLICATE_ELEMENT || f.code == validation::CODE_ELEMENT_ORDER
    };
    report
        .findings
        .iter()
        .take_while(|f| f.path == first.path && (!structural(first) || structural(f)))
        .any(|f| f.severity == validation::Severity::Error)
}
fn page_vector_bytes(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut total = n - 1;
    let (mut first, mut digits) = (1i64, 1i64);
    while first <= n {
        let last = (first * 10 - 1).min(n);
        total += (last - first + 1) * digits;
        first *= 10;
        digits += 1;
    }
    total + (total & 1)
}
fn valid_import_text(o: &JpegOptions) -> bool {
    let single = [
        &o.sop_class_uid,
        &o.sop_instance_uid,
        &o.study_instance_uid,
        &o.series_instance_uid,
        &o.patient_name,
        &o.patient_id,
        &o.patient_birth_date,
        &o.patient_sex,
        &o.study_date,
        &o.study_time,
        &o.study_id,
        &o.study_description,
        &o.accession_number,
        &o.referring_physician_name,
        &o.modality,
        &o.series_number,
        &o.series_description,
        &o.instance_number,
        &o.content_date,
        &o.content_time,
        &o.manufacturer,
        &o.laterality,
        &o.conversion_type,
        &o.burned_in_annotation,
    ];
    single.iter().all(|v| !v.contains(['\\', '\r', '\n', '\0']))
        && !o.image_type.contains(['\r', '\n', '\0'])
}
fn native_pixel_data_length(decoded: i64) -> Result<usize, Error> {
    if !(0..=4_294_967_294).contains(&decoded) {
        return Err(bare(Kind::ImportLimit));
    }
    Ok((decoded + (decoded & 1)) as usize)
}
fn read_png(b: &[u8], limits: ImportLimits) -> Result<SourceFrame, Error> {
    let config = png::decode_config(b).map_err(|_| malformed("PNG header"))?;
    let grey = matches!(config.model, png::Model::Gray | png::Model::Gray16);
    let wide = matches!(config.model, png::Model::Gray16 | png::Model::Colour16);
    let layout = ImageLayout {
        rows: config.height as i64,
        columns: config.width as i64,
        samples: if grey { 1 } else { 3 },
        bits: if wide { 16 } else { 8 },
        lossless: true,
        process: "native",
        photometric: if grey { "MONOCHROME2" } else { "RGB" },
        ..ImageLayout::default()
    };
    let n = admit_image(&layout, limits)? as usize;
    if layout.rows * layout.columns > limits.max_decoded_bytes / 8 {
        return Err(bare(Kind::ImportLimit));
    }
    let image = png::decode(b).map_err(|_| malformed("PNG decode"))?;
    if image.width != config.width || image.height != config.height {
        return Err(bare(Kind::MalformedImage));
    }
    if !image.opaque {
        return Err(unsupported("transparent alpha"));
    }
    let mut data = Vec::with_capacity(n);
    for pixel in &image.pixels {
        for &v in &pixel[..layout.samples as usize] {
            if wide {
                data.extend_from_slice(&v.to_le_bytes());
            } else {
                data.push((v >> 8) as u8);
            }
        }
    }
    native_frame(layout, data, Format::Png)
}
struct PortableTokens<'a> {
    data: &'a [u8],
    at: usize,
    limit: usize,
}
fn portable_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)
}
impl<'a> PortableTokens<'a> {
    fn next(&mut self) -> Result<&'a [u8], Error> {
        let over = |at: usize, limit: usize| limit > 0 && at >= limit;
        while self.at < self.data.len() {
            if over(self.at, self.limit) {
                return Err(bare(Kind::ImportLimit));
            }
            if portable_space(self.data[self.at]) {
                self.at += 1;
                continue;
            }
            if self.data[self.at] != b'#' {
                break;
            }
            while self.at < self.data.len() && self.data[self.at] != b'\n' {
                if over(self.at, self.limit) {
                    return Err(bare(Kind::ImportLimit));
                }
                self.at += 1;
            }
        }
        let start = self.at;
        while self.at < self.data.len()
            && !portable_space(self.data[self.at])
            && self.data[self.at] != b'#'
        {
            self.at += 1;
            if self.at - start > 64 {
                return Err(bare(Kind::MalformedImage));
            }
        }
        if start == self.at {
            return Err(bare(Kind::MalformedImage));
        }
        Ok(&self.data[start..self.at])
    }
    fn number(&mut self) -> Result<i64, Error> {
        let token = self.next()?;
        let mut n = 0i64;
        for &b in token {
            if !b.is_ascii_digit() || n > u32::MAX as i64 {
                return Err(bare(Kind::MalformedImage));
            }
            n = n * 10 + i64::from(b - b'0');
        }
        if n > 65535 {
            return Err(bare(Kind::MalformedImage));
        }
        Ok(n)
    }
}
fn binary_separator(b: &[u8], at: usize) -> Option<usize> {
    if at >= b.len() || !portable_space(b[at]) {
        return None;
    }
    if b[at] == b'\r' && b.get(at + 1) == Some(&b'\n') {
        return Some(at + 2);
    }
    Some(at + 1)
}
fn swap_words(data: &mut [u8]) {
    for pair in data.as_chunks_mut::<2>().0 {
        pair.swap(0, 1);
    }
}
fn read_portable(b: &[u8], limits: ImportLimits) -> Result<SourceFrame, Error> {
    let mut p = PortableTokens {
        data: b,
        at: 0,
        limit: 65536,
    };
    let magic = p.next()?;
    if !matches!(magic, b"P2" | b"P3" | b"P5" | b"P6") {
        return Err(bare(Kind::UnsupportedImage));
    }
    let columns = p.number()?;
    let rows = p.number()?;
    let maximum = p.number()?;
    if maximum != 255 && maximum != 65535 {
        return Err(unsupported("PNM maxval must be 255 or 65535"));
    }
    if p.at > 65536 {
        return Err(bare(Kind::ImportLimit));
    }
    p.limit = 0;
    let colour = matches!(magic, b"P3" | b"P6");
    let layout = ImageLayout {
        rows,
        columns,
        samples: if colour { 3 } else { 1 },
        bits: if maximum == 65535 { 16 } else { 8 },
        photometric: if colour { "RGB" } else { "MONOCHROME2" },
        process: "native",
        lossless: true,
        ..ImageLayout::default()
    };
    let n = admit_image(&layout, limits)? as usize;
    let wide = layout.bits > 8;
    let mut data;
    if matches!(magic, b"P5" | b"P6") {
        let at = binary_separator(b, p.at).filter(|at| b.len() - at == n);
        let Some(at) = at else {
            return Err(bare(Kind::MalformedImage));
        };
        data = b[at..].to_vec();
        if wide {
            swap_words(&mut data);
        }
    } else {
        data = Vec::with_capacity(n);
        while data.len() < n {
            let v = match p.number() {
                Ok(v) if v <= maximum => v,
                _ => return Err(bare(Kind::MalformedImage)),
            };
            if wide {
                data.extend_from_slice(&(v as u16).to_le_bytes());
            } else {
                data.push(v as u8);
            }
        }
        if p.next().is_ok() || p.at != p.data.len() {
            return Err(bare(Kind::MalformedImage));
        }
    }
    native_frame(layout, data, Format::Pnm)
}
fn read_pgx(b: &[u8], limits: ImportLimits) -> Result<SourceFrame, Error> {
    let end = match b.iter().position(|&c| c == b'\n') {
        Some(end) if end <= 256 => end,
        _ => return Err(bare(Kind::MalformedImage)),
    };
    let mut p = PortableTokens {
        data: &b[..end],
        at: 0,
        limit: 0,
    };
    if !matches!(p.next(), Ok(b"PG")) {
        return Err(bare(Kind::MalformedImage));
    }
    let order = match p.next() {
        Ok(order @ (b"ML" | b"LM")) => order,
        _ => return Err(bare(Kind::UnsupportedImage)),
    };
    let sign = match p.next() {
        Ok(sign @ (b"+" | b"-")) => sign,
        _ => return Err(bare(Kind::MalformedImage)),
    };
    let bits = match p.number() {
        Ok(bits) if (1..=16).contains(&bits) => bits,
        _ => return Err(bare(Kind::UnsupportedImage)),
    };
    let columns = p.number()?;
    let rows = p.number()?;
    if p.next().is_ok() || p.at != p.data.len() {
        return Err(bare(Kind::MalformedImage));
    }
    let layout = ImageLayout {
        rows,
        columns,
        samples: 1,
        bits,
        signed: i64::from(sign == b"-"),
        photometric: "MONOCHROME2",
        process: "native",
        lossless: true,
        ..ImageLayout::default()
    };
    let n = admit_image(&layout, limits)?;
    if (b.len() - end - 1) as i64 != n {
        return Err(bare(Kind::MalformedImage));
    }
    let mut data = b[end + 1..].to_vec();
    if bits > 8 && order == b"ML" {
        swap_words(&mut data);
    }
    check_codes(&data, &layout)?;
    native_frame(layout, data, Format::Pgx)
}
fn read_raw(b: &[u8], raw: &RawOptions, limits: ImportLimits) -> Result<Vec<SourceFrame>, Error> {
    let frames = if raw.frames == 0 { 1 } else { raw.frames };
    let samples = if raw.samples == 0 { 1 } else { raw.samples };
    let allocated = raw.bits_allocated;
    let stored = if raw.bits_stored == 0 {
        allocated
    } else {
        raw.bits_stored
    };
    if frames < 1
        || frames > limits.max_frames
        || raw.offset < 0
        || raw.offset > b.len() as i64
        || stored < 1
        || stored > allocated
        || (allocated != 8 && allocated != 16)
        || !matches!(raw.byte_order.as_str(), "" | "little" | "big")
    {
        return Err(bare(Kind::InvalidOption));
    }
    if frames > 1 && page_vector_bytes(frames) > 65534 {
        return Err(bare(Kind::ImportLimit));
    }
    let layout = ImageLayout {
        rows: raw.rows,
        columns: raw.columns,
        samples,
        bits: stored,
        allocated,
        signed: i64::from(raw.signed),
        lossless: true,
        process: "native",
        photometric: if samples == 3 { "RGB" } else { "MONOCHROME2" },
        ..ImageLayout::default()
    };
    let n = admit_image(&layout, limits)?;
    let stride = raw.rows * raw.columns * samples * (allocated / 8);
    if stride > limits.max_decoded_bytes / frames || stride * frames != b.len() as i64 - raw.offset
    {
        return Err(malformed("raw length"));
    }
    let (offset, stride, n) = (raw.offset as usize, stride as usize, n as usize);
    let mut out = Vec::with_capacity(frames as usize);
    for f in 0..frames as usize {
        let start = offset + f * stride;
        let mut data = b[start..start + stride].to_vec();
        data.resize(n, 0);
        if allocated == 16 && raw.byte_order == "big" {
            swap_words(&mut data);
        }
        check_codes(&data, &layout)?;
        out.push(native_frame(layout.clone(), data, Format::Raw)?);
    }
    Ok(out)
}
fn check_codes(data: &[u8], l: &ImageLayout) -> Result<(), Error> {
    let width = (l.effective_allocated() / 8) as usize;
    let mask = ((1u32 << l.bits) - 1) as u16;
    let full_mask: u16 = if width == 1 { 255 } else { 65535 };
    for sample in data.chunks_exact(width) {
        let value = if width == 2 {
            u16::from_le_bytes([sample[0], sample[1]])
        } else {
            u16::from(sample[0])
        };
        let unused = value & !mask;
        if l.signed == 0 {
            if unused != 0 {
                return Err(malformed("samples exceed precision"));
            }
            continue;
        }
        let negative = value & (1u16 << (l.bits - 1)) != 0;
        let want = if negative { full_mask & !mask } else { 0 };
        if unused != want {
            return Err(malformed("invalid sign extension"));
        }
    }
    Ok(())
}
fn le16(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn read_bmp(b: &[u8], limits: ImportLimits) -> Result<SourceFrame, Error> {
    if b.len() < 54 || &b[..2] != b"BM" {
        return Err(bare(Kind::MalformedImage));
    }
    let offset = u64::from(le32(&b[10..]));
    if le32(&b[14..]) != 40 {
        return Err(unsupported("BMP DIB"));
    }
    if u64::from(le32(&b[2..])) != b.len() as u64 & 0xffff_ffff
        || offset < 54
        || offset > b.len() as u64
        || le16(&b[26..]) != 1
    {
        return Err(bare(Kind::MalformedImage));
    }
    let width = i64::from(le32(&b[18..]) as i32);
    let mut height = i64::from(le32(&b[22..]) as i32);
    let top = height < 0;
    if top {
        height = -height;
    }
    let bits = u64::from(le16(&b[28..]));
    if !matches!(bits, 8 | 24 | 32) || le32(&b[30..]) != 0 {
        return Err(unsupported("BMP compression/bit depth"));
    }
    let layout = ImageLayout {
        rows: height,
        columns: width,
        samples: 3,
        bits: 8,
        lossless: true,
        process: "native",
        photometric: "RGB",
        ..ImageLayout::default()
    };
    let n = admit_image(&layout, limits)? as usize;
    let row = (width as u64 * bits).div_ceil(32) * 4;
    let span = row * height as u64;
    if span > b.len() as u64 - offset || offset + span != b.len() as u64 {
        return Err(bare(Kind::MalformedImage));
    }
    let mut palette: &[u8] = &[];
    if bits == 8 {
        let mut count = u64::from(le32(&b[46..]));
        if count == 0 {
            count = 256;
        }
        if count > 256 || 54 + count * 4 > offset {
            return Err(bare(Kind::MalformedImage));
        }
        palette = &b[54..54 + count as usize * 4];
    }
    let (rows, columns, offset, row) = (
        height as usize,
        width as usize,
        offset as usize,
        row as usize,
    );
    let step = (bits / 8) as usize;
    let mut data = Vec::with_capacity(n);
    for y in 0..rows {
        let source_y = if top { y } else { rows - 1 - y };
        let line = &b[offset + source_y * row..];
        for x in 0..columns {
            let pix = if bits == 8 {
                let index = usize::from(line[x]) * 4;
                match palette.get(index..index + 4) {
                    Some(pix) => pix,
                    None => return Err(bare(Kind::MalformedImage)),
                }
            } else {
                &line[x * step..x * step + step]
            };
            data.extend_from_slice(&[pix[2], pix[1], pix[0]]);
        }
    }
    native_frame(layout, data, Format::Bmp)
}
fn read_compressed(
    b: &[u8],
    format: Format,
    jpeg: &JpegOptions,
    j2k_lossless: bool,
    limits: ImportLimits,
) -> Result<SourceFrame, Error> {
    if format == Format::Jpeg {
        let stream = parse_jpeg_stream_limited(b, 4096)?;
        let (syntax, process) = jpeg_transfer_syntax(&stream, jpeg.allow_retired_progressive)?;
        let lossless = process == "lossless";
        let (photometric, subsampling) = jpeg_photometric(&stream, lossless)?;
        let mut layout = ImageLayout {
            rows: stream.rows,
            columns: stream.columns,
            bits: stream.precision,
            samples: stream.components.len() as i64,
            photometric,
            process,
            lossless,
            method: "ISO_10918_1",
            ..ImageLayout::default()
        };
        admit_image(&layout, limits)?;
        let mut report = Report {
            process,
            transfer_syntax_uid: syntax.uid.to_string(),
            photometric_interpretation: photometric,
            subsampling,
            ..Report::default()
        };
        let (data, icc) = stripped_stream(b, &stream, jpeg, &mut report);
        report.trailing_bytes_removed = (b.len() - stream.eoi) as i64;
        layout.payload_bytes = data.len() as i64;
        return Ok(SourceFrame {
            layout,
            data,
            syntax,
            icc,
            report,
            format,
        });
    }
    #[cfg(not(feature = "jpeg-ls"))]
    if format == Format::Jls {
        return Err(unsupported("JPEG-LS (feature jpeg-ls not built)"));
    }
    #[cfg(feature = "jpeg-ls")]
    if format == Format::Jls {
        let h =
            jpegls::inspect_codestream(b, limits.max_input_bytes, jpegls::DecodeOptions::default())
                .map_err(Error::other)?;
        if h.max_value != (1i64 << h.precision) - 1 {
            return Err(unsupported("custom JPEG-LS MAXVAL"));
        }
        let lossless = h.near == 0;
        let layout = ImageLayout {
            rows: h.rows,
            columns: h.columns,
            bits: h.precision,
            samples: h.components,
            photometric: if h.components == 3 {
                "RGB"
            } else {
                "MONOCHROME2"
            },
            process: "jpeg-ls",
            lossless,
            method: "ISO_14495_1",
            payload_bytes: b.len() as i64,
            ..ImageLayout::default()
        };
        admit_image(&layout, limits)?;
        return Ok(SourceFrame {
            layout,
            data: b.to_vec(),
            syntax: registered(if lossless {
                JPEG_LS_LOSSLESS
            } else {
                JPEG_LS_NEAR_LOSSLESS
            })?,
            icc: None,
            report: Report {
                process: "jpeg-ls",
                ..Report::default()
            },
            format,
        });
    }
    encapsulated::validate_frame(b, encapsulated::Format::JPEG2000).map_err(Error::other)?;
    let layout = j2k_layout(b, j2k_lossless)?;
    admit_image(&layout, limits)?;
    Ok(SourceFrame {
        layout,
        data: b.to_vec(),
        syntax: registered(if j2k_lossless {
            JPEG_2000_LOSSLESS_ONLY
        } else {
            JPEG_2000
        })?,
        icc: None,
        report: Report {
            process: "jpeg2000",
            ..Report::default()
        },
        format: Format::J2k,
    })
}
fn be16(b: &[u8]) -> usize {
    usize::from(b[0]) << 8 | usize::from(b[1])
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
fn j2k_layout(b: &[u8], declared: bool) -> Result<ImageLayout, Error> {
    let mut l = ImageLayout {
        process: "jpeg2000",
        lossless: declared,
        method: "ISO_15444_1",
        payload_bytes: b.len() as i64,
        ..ImageLayout::default()
    };
    let malformed = || bare(Kind::MalformedImage);
    let refused = || bare(Kind::UnsupportedImage);
    let (mut siz, mut cod, mut qcd) = (false, false, false);
    let (mut transform, mut mct) = (0u8, 0u8);
    let mut tile_count = 0u64;
    let mut seen: HashSet<u16> = HashSet::new();
    let mut at = 2usize;
    while at + 2 <= b.len() {
        if b[at] != 255 {
            return Err(malformed());
        }
        let code = b[at + 1];
        if code == 0x90 {
            if !siz || !cod || !qcd {
                return Err(malformed());
            }
            while at + 12 <= b.len() && b[at] == 255 && b[at + 1] == 0x90 {
                if be16(&b[at + 2..]) != 10 {
                    return Err(malformed());
                }
                let psot = u64::from(be32(&b[at + 6..]));
                if psot < 14 || psot > (b.len() - at) as u64 {
                    return Err(refused());
                }
                let tile = be16(&b[at + 4..]) as u16;
                if u64::from(tile) >= tile_count
                    || seen.contains(&tile)
                    || b[at + 10] != 0
                    || b[at + 11] != 1
                {
                    return Err(unsupported("JPEG2000 tile parts"));
                }
                seen.insert(tile);
                let end = at + psot as usize;
                let mut h = at + 12;
                while h + 2 <= end && !(b[h] == 255 && b[h + 1] == 0x93) {
                    if b[h] != 255 || h + 4 > end {
                        return Err(malformed());
                    }
                    if b[h + 1] != 0x64 {
                        return Err(unsupported("JPEG2000 tile header override"));
                    }
                    let n = be16(&b[h + 2..]);
                    if n < 2 || n > end - h - 2 {
                        return Err(malformed());
                    }
                    h += n + 2;
                }
                if h + 2 > end {
                    return Err(malformed());
                }
                at = end;
            }
            if seen.len() as u64 != tile_count
                || at + 2 > b.len()
                || b[at] != 255
                || b[at + 1] != 0xd9
            {
                return Err(malformed());
            }
            break;
        }
        if at + 4 > b.len() {
            return Err(malformed());
        }
        let n = be16(&b[at + 2..]);
        if n < 2 || n > b.len() - at - 2 {
            return Err(malformed());
        }
        let v = &b[at + 4..at + n + 2];
        match code {
            0x51 => {
                if siz || v.len() < 36 || be16(v) != 0 {
                    return Err(refused());
                }
                siz = true;
                let (xs, ys, xo, yo) =
                    (be32(&v[2..]), be32(&v[6..]), be32(&v[10..]), be32(&v[14..]));
                if xs <= xo || ys <= yo || xo != 0 || yo != 0 {
                    return Err(refused());
                }
                (l.columns, l.rows) = (i64::from(xs), i64::from(ys));
                let (xt, yt) = (u64::from(be32(&v[18..])), u64::from(be32(&v[22..])));
                if xt == 0 || yt == 0 || be32(&v[26..]) != 0 || be32(&v[30..]) != 0 {
                    return Err(refused());
                }
                tile_count = u64::from(xs).div_ceil(xt) * u64::from(ys).div_ceil(yt);
                if tile_count == 0 || tile_count > 4096 {
                    return Err(bare(Kind::ImportLimit));
                }
                let samples = be16(&v[34..]);
                if (samples != 1 && samples != 3) || v.len() != 36 + 3 * samples {
                    return Err(refused());
                }
                l.samples = samples as i64;
                l.bits = i64::from(v[36] & 127) + 1;
                l.signed = i64::from(v[36] >> 7);
                for component in v[36..].as_chunks::<3>().0 {
                    if component[0] != v[36] || component[1] != 1 || component[2] != 1 {
                        return Err(refused());
                    }
                }
            }
            0x52 => {
                if cod || v.len() != 10 || v[0] != 0 || v[1] > 4 {
                    return Err(refused());
                }
                cod = true;
                let layers = be16(&v[2..]);
                (mct, transform) = (v[4], v[9]);
                if !(1..=32).contains(&layers) || mct > 1 || transform > 1 {
                    return Err(refused());
                }
                if declared && transform != 1 {
                    return Err(bare(Kind::InvalidOption));
                }
            }
            0x5c => {
                if qcd || v.len() < 2 {
                    return Err(malformed());
                }
                qcd = true;
                if declared && v[0] & 31 != 0 {
                    return Err(bare(Kind::InvalidOption));
                }
            }
            0x64 => {}
            _ => return Err(unsupported("JPEG2000 coding marker")),
        }
        at += n + 2;
    }
    if !siz || !cod || !qcd {
        return Err(malformed());
    }
    l.lossy_unknown = !declared && transform == 1;
    l.photometric = match (l.samples, mct, transform) {
        (3, 1, 1) => "YBR_RCT",
        (3, 1, _) => "YBR_ICT",
        (3, _, _) => "RGB",
        _ => "MONOCHROME2",
    };
    Ok(l)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_vectors() {
        assert_eq!(page_vector_bytes(0), 0);
        assert_eq!(page_vector_bytes(1), 2);
        assert_eq!(page_vector_bytes(10), 20);
        assert_eq!(page_vector_bytes(100), 9 + 180 + 3 + 99 + 1);
    }
    #[test]
    fn portable_tokens_skip_comments() {
        let mut p = PortableTokens {
            data: b"P5 # size\n 4\t3\n255\n",
            at: 0,
            limit: 0,
        };
        assert_eq!(p.next().ok(), Some(&b"P5"[..]));
        assert_eq!(p.number().ok(), Some(4));
        assert_eq!(p.number().ok(), Some(3));
        assert_eq!(p.number().ok(), Some(255));
        assert!(p.next().is_err());
    }
    #[test]
    fn sign_extension() {
        let layout = ImageLayout {
            bits: 12,
            allocated: 16,
            signed: 1,
            ..ImageLayout::default()
        };
        assert!(check_codes(&[0xff, 0xff, 0x00, 0x08, 0xff, 0x07], &layout).is_err());
        assert!(check_codes(&[0xff, 0xff, 0x00, 0xf8, 0xff, 0x07], &layout).is_ok());
    }
}
