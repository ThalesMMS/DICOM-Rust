#![forbid(unsafe_code)]
use crate::derivation::DerivedCommon;
use crate::object::element_setters::format_ds;
use crate::object::inherit::{inherit_modules, new_uid};
use crate::object::types::{Element, ElementHeader, Tag, Value, Vr};
use crate::object::{File, Object, UNDEFINED_LENGTH, transfer};
use crate::textfmt::{quote, to_upper};
use std::fmt;
mod import;
mod jpeg_stream;
mod png;
mod raster;
pub use import::{
    AnatomicRegion, Format, ImageOptions, ImportLimits, ImportReport,
    MULTI_FRAME_GRAYSCALE_BYTE_SC, MULTI_FRAME_GRAYSCALE_WORD_SC, MULTI_FRAME_TRUE_COLOR_SC,
    RawOptions, VIDEO_PHOTOGRAPHIC_IMAGE_STORAGE, from_images,
};
use jpeg_stream::{
    FrameComponent, JpegStream, MARKER_APP0, MARKER_APP2, MARKER_APPE, MARKER_COM, MARKER_SOF0,
    MARKER_SOF1, MARKER_SOF2, MARKER_SOF3, parse_jpeg_stream,
};
pub use raster::{
    RASTER_ALL, RASTER_BLUE, RASTER_GREEN, RASTER_RED, RasterInfo, RasterPage, RasterPageInfo,
    Rgba8Image, build_raster_page, decode_raster_page, encode_bmp24, inspect_raster,
};
pub const SECONDARY_CAPTURE_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.7";
pub const VL_PHOTOGRAPHIC_IMAGE_STORAGE: &str = "1.2.840.10008.5.1.4.1.1.77.1.4";
pub const JPEG_PROGRESSIVE_RETIRED: &str = "1.2.840.10008.1.2.4.55";
const JPEG_BASELINE: &str = "1.2.840.10008.1.2.4.50";
const JPEG_EXTENDED: &str = "1.2.840.10008.1.2.4.51";
const JPEG_LOSSLESS_NON_HIERARCHICAL: &str = "1.2.840.10008.1.2.4.57";
const JPEG_LOSSLESS_SV1: &str = "1.2.840.10008.1.2.4.70";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    NotJpeg,
    MalformedJpeg,
    TruncatedJpeg,
    UnsupportedJpegProcess,
    ProgressiveJpeg,
    UnsupportedComponents,
    UnsupportedSopClass,
    InvalidOption,
    ImportLimit,
    UnsupportedImage,
    MalformedImage,
    Other,
}
impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::NotJpeg => "encapimage: input is not a JPEG interchange-format stream",
            Kind::MalformedJpeg => "encapimage: malformed JPEG stream",
            Kind::TruncatedJpeg => "encapimage: truncated JPEG stream",
            Kind::UnsupportedJpegProcess => "encapimage: JPEG process has no DICOM transfer syntax",
            Kind::ProgressiveJpeg => {
                "encapimage: progressive JPEG maps only to a retired transfer syntax"
            }
            Kind::UnsupportedComponents => "encapimage: JPEG component layout is not supported",
            Kind::UnsupportedSopClass => "encapimage: SOP class is not supported",
            Kind::InvalidOption => "encapimage: invalid option",
            Kind::ImportLimit => "encapimage: image import resource limit",
            Kind::UnsupportedImage => "encapimage: unsupported image input or layout",
            Kind::MalformedImage => "encapimage: malformed image input",
            Kind::Other => "",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: Kind,
    text: String,
}
impl Error {
    pub(crate) fn bare(kind: Kind) -> Error {
        Error {
            kind,
            text: kind.text().to_string(),
        }
    }
    pub(crate) fn new(kind: Kind, detail: impl fmt::Display) -> Error {
        Error {
            kind,
            text: format!("{}: {detail}", kind.text()),
        }
    }
    pub(crate) fn other(text: impl fmt::Display) -> Error {
        Error {
            kind: Kind::Other,
            text: text.to_string(),
        }
    }
    pub fn is(&self, kind: Kind) -> bool {
        self.kind == kind
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for Error {}
#[derive(Clone, Debug, Default)]
pub struct JpegOptions {
    pub sop_class_uid: String,
    pub sop_instance_uid: String,
    pub study_instance_uid: String,
    pub series_instance_uid: String,
    pub patient_name: String,
    pub patient_id: String,
    pub patient_birth_date: String,
    pub patient_sex: String,
    pub study_date: String,
    pub study_time: String,
    pub study_id: String,
    pub study_description: String,
    pub accession_number: String,
    pub referring_physician_name: String,
    pub modality: String,
    pub series_number: String,
    pub series_description: String,
    pub instance_number: String,
    pub content_date: String,
    pub content_time: String,
    pub manufacturer: String,
    pub laterality: String,
    pub image_type: String,
    pub conversion_type: String,
    pub burned_in_annotation: String,
    pub keep_appn: bool,
    pub keep_comments: bool,
    pub discard_icc_profile: bool,
    pub allow_retired_progressive: bool,
    pub common: DerivedCommon,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemovedSegment {
    pub marker: String,
    pub identifier: &'static str,
    pub count: i64,
    pub bytes: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub transfer_syntax_uid: String,
    pub process: &'static str,
    pub photometric_interpretation: &'static str,
    pub subsampling: &'static str,
    pub removed_segments: Vec<RemovedSegment>,
    pub trailing_bytes_removed: i64,
    pub icc_profile_copied: bool,
    pub generated_uids: Vec<&'static str>,
    pub padded: bool,
    pub pixel_data_bytes: i64,
}
const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
const TAG_SPECIFIC_CHARACTER_SET: Tag = Tag::new(0x0008, 0x0005);
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const TAG_STUDY_DATE: Tag = Tag::new(0x0008, 0x0020);
const TAG_CONTENT_DATE: Tag = Tag::new(0x0008, 0x0023);
const TAG_STUDY_TIME: Tag = Tag::new(0x0008, 0x0030);
const TAG_CONTENT_TIME: Tag = Tag::new(0x0008, 0x0033);
const TAG_ACCESSION_NUMBER: Tag = Tag::new(0x0008, 0x0050);
const TAG_MODALITY: Tag = Tag::new(0x0008, 0x0060);
const TAG_CONVERSION_TYPE: Tag = Tag::new(0x0008, 0x0064);
const TAG_MANUFACTURER: Tag = Tag::new(0x0008, 0x0070);
const TAG_REFERRING_PHYSICIAN_NAME: Tag = Tag::new(0x0008, 0x0090);
const TAG_STUDY_DESCRIPTION: Tag = Tag::new(0x0008, 0x1030);
const TAG_SERIES_DESCRIPTION: Tag = Tag::new(0x0008, 0x103E);
const TAG_PATIENT_NAME: Tag = Tag::new(0x0010, 0x0010);
const TAG_PATIENT_ID: Tag = Tag::new(0x0010, 0x0020);
const TAG_PATIENT_BIRTH_DATE: Tag = Tag::new(0x0010, 0x0030);
const TAG_PATIENT_SEX: Tag = Tag::new(0x0010, 0x0040);
const TAG_STUDY_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000D);
const TAG_SERIES_INSTANCE_UID: Tag = Tag::new(0x0020, 0x000E);
const TAG_STUDY_ID: Tag = Tag::new(0x0020, 0x0010);
const TAG_SERIES_NUMBER: Tag = Tag::new(0x0020, 0x0011);
const TAG_INSTANCE_NUMBER: Tag = Tag::new(0x0020, 0x0013);
const TAG_PATIENT_ORIENTATION: Tag = Tag::new(0x0020, 0x0020);
const TAG_LATERALITY: Tag = Tag::new(0x0020, 0x0060);
const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
const TAG_PHOTOMETRIC: Tag = Tag::new(0x0028, 0x0004);
const TAG_PLANAR_CONFIGURATION: Tag = Tag::new(0x0028, 0x0006);
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_BURNED_IN_ANNOTATION: Tag = Tag::new(0x0028, 0x0301);
const TAG_LOSSY_IMAGE_COMPRESSION: Tag = Tag::new(0x0028, 0x2110);
const TAG_LOSSY_COMPRESSION_RATIO: Tag = Tag::new(0x0028, 0x2112);
const TAG_LOSSY_COMPRESSION_METHOD: Tag = Tag::new(0x0028, 0x2114);
const TAG_ICC_PROFILE: Tag = Tag::new(0x0028, 0x2000);
const TAG_ACQUISITION_CONTEXT_SEQ: Tag = Tag::new(0x0040, 0x0555);
pub fn from_jpeg(data: &[u8], opts: &JpegOptions) -> (Report, Result<File, Error>) {
    let mut report = Report::default();
    match from_jpeg_into(data, opts, &mut report) {
        Ok(file) => (report, Ok(file)),
        Err(err) => (report, Err(err)),
    }
}
fn from_jpeg_into(data: &[u8], opts: &JpegOptions, report: &mut Report) -> Result<File, Error> {
    let sop_class = sop_class_of(opts);
    if sop_class != SECONDARY_CAPTURE_IMAGE_STORAGE && sop_class != VL_PHOTOGRAPHIC_IMAGE_STORAGE {
        return Err(Error::new(Kind::UnsupportedSopClass, sop_class));
    }
    let burned = to_upper(opts.burned_in_annotation.trim());
    if !burned.is_empty() && burned != "YES" && burned != "NO" {
        return Err(Error::new(
            Kind::InvalidOption,
            format!("Burned In Annotation {}", quote(&opts.burned_in_annotation)),
        ));
    }
    let laterality = to_upper(opts.laterality.trim());
    if !laterality.is_empty() && laterality != "R" && laterality != "L" {
        return Err(Error::new(
            Kind::InvalidOption,
            format!("Laterality {}", quote(&opts.laterality)),
        ));
    }
    let stream = parse_jpeg_stream(data)?;
    let (syntax, process) = jpeg_transfer_syntax(&stream, opts.allow_retired_progressive)?;
    report.transfer_syntax_uid = syntax.uid.to_string();
    report.process = process;
    let lossless = process == "lossless";
    let (photometric, subsampling) = jpeg_photometric(&stream, lossless)?;
    report.photometric_interpretation = photometric;
    report.subsampling = subsampling;
    if sop_class == VL_PHOTOGRAPHIC_IMAGE_STORAGE {
        vl_image_pixels(&stream, process, photometric)?;
    }
    if stream.rows == 0 || stream.columns == 0 {
        return Err(Error::new(
            Kind::UnsupportedComponents,
            format!(
                "{}x{} frame (DNL-defined heights are not supported)",
                stream.columns, stream.rows
            ),
        ));
    }
    let (mut pixels, icc) = stripped_stream(data, &stream, opts, report);
    report.trailing_bytes_removed = (data.len() - stream.eoi) as i64;
    if pixels.len() % 2 != 0 {
        pixels.push(0);
        report.padded = true;
    }
    report.pixel_data_bytes = pixels.len() as i64;
    let layout = ImageLayout {
        rows: stream.rows,
        columns: stream.columns,
        samples: stream.components.len() as i64,
        bits: stream.precision,
        photometric,
        lossless,
        payload_bytes: pixels.len() as i64,
        ..ImageLayout::default()
    };
    let value = Value::Fragments {
        offset_table: Vec::new(),
        fragments: vec![pixels],
    };
    build_image(opts, &layout, value, syntax, report, icc)
}
fn sop_class_of(opts: &JpegOptions) -> &str {
    match opts.sop_class_uid.trim() {
        "" => SECONDARY_CAPTURE_IMAGE_STORAGE,
        uid => uid,
    }
}
fn vl_image_pixels(stream: &JpegStream, process: &str, photometric: &str) -> Result<(), Error> {
    if stream.precision != 8 {
        return Err(Error::new(
            Kind::UnsupportedComponents,
            format!(
                "VL Photographic Image needs 8-bit samples, the frame has {} (use Secondary Capture)",
                stream.precision
            ),
        ));
    }
    if process != "lossless" && photometric == "RGB" {
        return Err(Error::new(
            Kind::UnsupportedComponents,
            "VL Photographic Image needs YBR_FULL_422 for lossy JPEG, the frame is RGB (use Secondary Capture)",
        ));
    }
    Ok(())
}
fn registered(uid: &str) -> Result<transfer::Syntax, Error> {
    transfer::lookup(uid).ok_or_else(|| {
        Error::new(
            Kind::UnsupportedJpegProcess,
            format!("{uid} is not registered"),
        )
    })
}
fn jpeg_transfer_syntax(
    stream: &JpegStream,
    allow_progressive: bool,
) -> Result<(transfer::Syntax, &'static str), Error> {
    let precision = stream.precision;
    let bad = |what: &str| {
        Err(Error::new(
            Kind::MalformedJpeg,
            format!("{what} frame with {precision}-bit precision"),
        ))
    };
    match stream.sof_marker {
        MARKER_SOF0 => {
            if precision != 8 {
                return bad("baseline");
            }
            Ok((registered(JPEG_BASELINE)?, "baseline"))
        }
        MARKER_SOF1 => {
            if precision != 8 && precision != 12 {
                return bad("extended");
            }
            Ok((registered(JPEG_EXTENDED)?, "extended"))
        }
        MARKER_SOF2 => {
            if !allow_progressive {
                return Err(Error::bare(Kind::ProgressiveJpeg));
            }
            if precision != 8 && precision != 12 {
                return bad("progressive");
            }
            Ok((registered(JPEG_PROGRESSIVE_RETIRED)?, "progressive"))
        }
        MARKER_SOF3 => {
            if !(2..=16).contains(&precision) {
                return bad("lossless");
            }
            if stream.predictors.iter().any(|&p| p != 1) {
                return Ok((registered(JPEG_LOSSLESS_NON_HIERARCHICAL)?, "lossless"));
            }
            Ok((registered(JPEG_LOSSLESS_SV1)?, "lossless"))
        }
        marker => Err(Error::new(
            Kind::UnsupportedJpegProcess,
            format!("frame marker FF{marker:02X} ({})", sof_name(marker)),
        )),
    }
}
fn sof_name(marker: u8) -> &'static str {
    match marker {
        0xc5..=0xc7 => "hierarchical",
        0xc9..=0xcb => "arithmetic coding",
        0xcd.. => "hierarchical arithmetic coding",
        _ => "unknown",
    }
}
fn jpeg_photometric(
    stream: &JpegStream,
    lossless: bool,
) -> Result<(&'static str, &'static str), Error> {
    let c = &stream.components;
    match c.len() {
        1 => return Ok(("MONOCHROME2", "")),
        3 => {}
        n => {
            return Err(Error::new(
                Kind::UnsupportedComponents,
                format!("{n} components"),
            ));
        }
    }
    let subsampling = chroma_subsampling(c);
    let ycc = match stream.adobe_transform {
        Some(0) => false,
        Some(1) => true,
        _ if stream.jfif => true,
        _ if c[0].id == b'R' && c[1].id == b'G' && c[2].id == b'B' => false,
        _ => !lossless,
    };
    if lossless {
        if ycc {
            return Err(Error::new(
                Kind::UnsupportedComponents,
                "lossless stream with a YCbCr color transform",
            ));
        }
        return Ok(("RGB", subsampling));
    }
    Ok((if ycc { "YBR_FULL_422" } else { "RGB" }, subsampling))
}
fn chroma_subsampling(components: &[FrameComponent]) -> &'static str {
    let (y, cb, cr) = (components[0], components[1], components[2]);
    if cb.horizontal != cr.horizontal || cb.vertical != cr.vertical {
        return "other";
    }
    if y.horizontal == cb.horizontal && y.vertical == cb.vertical {
        "4:4:4"
    } else if y.horizontal == 2 * cb.horizontal && y.vertical == cb.vertical {
        "4:2:2"
    } else if y.horizontal == 2 * cb.horizontal && y.vertical == 2 * cb.vertical {
        "4:2:0"
    } else {
        "other"
    }
}
#[derive(Default)]
struct IccChunks {
    chunks: Vec<Option<Vec<u8>>>,
    count: usize,
    invalid: bool,
}
impl IccChunks {
    fn add(&mut self, payload: &[u8]) {
        const HEADER: usize = "ICC_PROFILE\x00".len() + 2;
        if self.invalid || payload.len() < HEADER {
            self.invalid = true;
            return;
        }
        let (sequence, total) = (
            usize::from(payload[HEADER - 2]),
            usize::from(payload[HEADER - 1]),
        );
        if total == 0
            || sequence == 0
            || sequence > total
            || (self.count != 0 && self.count != total)
        {
            self.invalid = true;
            return;
        }
        if self.chunks.is_empty() {
            self.chunks = vec![None; total];
        }
        if self.chunks[sequence - 1].is_some() {
            self.invalid = true;
            return;
        }
        self.chunks[sequence - 1] = Some(payload[HEADER..].to_vec());
        self.count = total;
    }
    fn profile(self) -> Option<Vec<u8>> {
        if self.invalid || self.count == 0 || self.chunks.len() != self.count {
            return None;
        }
        let mut icc = Vec::new();
        for chunk in self.chunks {
            icc.extend_from_slice(&chunk?);
        }
        (!icc.is_empty()).then_some(icc)
    }
}
fn stripped_stream(
    data: &[u8],
    stream: &JpegStream,
    opts: &JpegOptions,
    report: &mut Report,
) -> (Vec<u8>, Option<Vec<u8>>) {
    let mut out = Vec::with_capacity(stream.eoi);
    let mut icc = IccChunks::default();
    for segment in &stream.segments {
        let is_app = (MARKER_APP0..=0xef).contains(&segment.marker);
        let identifier = if is_app {
            application_identifier(segment.marker, segment.payload(data))
        } else {
            ""
        };
        if segment.marker == MARKER_APP2 && identifier == "ICC_PROFILE" {
            icc.add(segment.payload(data));
        }
        let keep = if is_app {
            opts.keep_appn
                || (segment.marker == MARKER_APP0 && identifier == "JFIF")
                || (segment.marker == MARKER_APPE && identifier == "Adobe")
        } else if segment.marker == MARKER_COM {
            opts.keep_comments
        } else {
            true
        };
        if keep {
            out.extend_from_slice(&data[segment.start..segment.end]);
            continue;
        }
        let name = if is_app {
            format!("APP{}", segment.marker - MARKER_APP0)
        } else {
            "COM".to_string()
        };
        let removed = &mut report.removed_segments;
        let found = removed
            .iter()
            .position(|r| r.marker == name && r.identifier == identifier);
        let index = match found {
            Some(index) => index,
            None => {
                removed.push(RemovedSegment {
                    marker: name,
                    identifier,
                    ..RemovedSegment::default()
                });
                removed.len() - 1
            }
        };
        removed[index].count += 1;
        removed[index].bytes += (segment.end - segment.start) as i64;
    }
    (out, icc.profile())
}
fn application_identifier(marker: u8, payload: &[u8]) -> &'static str {
    const KNOWN: &[(u8, &[u8], &str)] = &[
        (MARKER_APP0, b"JFIF\x00", "JFIF"),
        (MARKER_APP0, b"JFXX\x00", "JFXX"),
        (0xe1, b"Exif\x00", "Exif"),
        (0xe1, b"http://ns.adobe.com/xap/1.0/\x00", "XMP"),
        (0xe1, b"http://ns.adobe.com/xmp/extension/\x00", "XMP"),
        (MARKER_APP2, b"ICC_PROFILE\x00", "ICC_PROFILE"),
        (MARKER_APP2, b"MPF\x00", "MPF"),
        (MARKER_APP2, b"FPXR\x00", "FPXR"),
        (0xec, b"Ducky", "Ducky"),
        (0xed, b"Photoshop 3.0\x00", "Photoshop"),
        (MARKER_APPE, b"Adobe", "Adobe"),
    ];
    KNOWN
        .iter()
        .find(|(m, prefix, _)| *m == marker && payload.starts_with(prefix))
        .map_or("", |(_, _, name)| name)
}
fn format_ratio(ratio: f64) -> String {
    if !ratio.is_finite() {
        return "1".to_string();
    }
    format_ds((ratio * 1000.0).round() / 1000.0).unwrap_or_else(|_| "1".to_string())
}
fn default_string(value: &str, fallback: &str) -> String {
    match value.trim() {
        "" => fallback.to_string(),
        value => value.to_string(),
    }
}
fn string_element(tag: Tag, vr: Vr, value: &str) -> Element {
    strings_element(tag, vr, vec![value.trim().to_string()])
}
fn strings_element(tag: Tag, vr: Vr, values: Vec<String>) -> Element {
    Element {
        header: ElementHeader {
            tag,
            vr,
            length: 0,
            length_set: false,
        },
        value: Value::Strings(values),
    }
}
fn raw_element(tag: Tag, vr: Vr, raw: Vec<u8>) -> Element {
    Element {
        header: ElementHeader::new(tag, vr, raw.len() as u32),
        value: Value::Raw(raw),
    }
}
fn us_element(tag: Tag, value: i64) -> Element {
    raw_element(tag, Vr::US, (value as u16).to_le_bytes().to_vec())
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ImageLayout {
    rows: i64,
    columns: i64,
    samples: i64,
    bits: i64,
    signed: i64,
    allocated: i64,
    frame_count: i64,
    method: &'static str,
    process: &'static str,
    photometric: &'static str,
    lossless: bool,
    lossy_unknown: bool,
    payload_bytes: i64,
}
impl ImageLayout {
    fn effective_allocated(&self) -> i64 {
        if self.allocated > 0 {
            self.allocated
        } else if self.bits > 8 {
            16
        } else {
            8
        }
    }
}
fn build_image(
    opts: &JpegOptions,
    layout: &ImageLayout,
    value: Value,
    syntax: transfer::Syntax,
    report: &mut Report,
    icc: Option<Vec<u8>>,
) -> Result<File, Error> {
    let sop_class = sop_class_of(opts);
    let burned = to_upper(opts.burned_in_annotation.trim());
    let laterality = to_upper(opts.laterality.trim());
    let mut uid = |value: &str, keyword: &'static str| match value.trim() {
        "" => {
            report.generated_uids.push(keyword);
            new_uid()
        }
        value => value.to_string(),
    };
    let study = uid(&opts.study_instance_uid, "StudyInstanceUID");
    let series = uid(&opts.series_instance_uid, "SeriesInstanceUID");
    let instance = uid(&opts.sop_instance_uid, "SOPInstanceUID");
    let samples = layout.samples;
    let allocated = layout.effective_allocated();
    let native = matches!(value, Value::Raw(_));
    let vlp =
        sop_class == VL_PHOTOGRAPHIC_IMAGE_STORAGE || sop_class == VIDEO_PHOTOGRAPHIC_IMAGE_STORAGE;
    let (modality, image_type) = if vlp {
        (
            default_string(&opts.modality, "XC"),
            default_string(&opts.image_type, "DERIVED\\SECONDARY"),
        )
    } else {
        (
            default_string(&opts.modality, "OT"),
            opts.image_type.clone(),
        )
    };
    let mut elements = vec![
        string_element(TAG_SOP_CLASS_UID, Vr::UI, sop_class),
        string_element(TAG_SOP_INSTANCE_UID, Vr::UI, &instance),
        string_element(TAG_STUDY_DATE, Vr::DA, &opts.study_date),
        string_element(TAG_STUDY_TIME, Vr::TM, &opts.study_time),
        string_element(TAG_ACCESSION_NUMBER, Vr::SH, &opts.accession_number),
        string_element(TAG_MODALITY, Vr::CS, &modality),
        string_element(
            TAG_REFERRING_PHYSICIAN_NAME,
            Vr::PN,
            &opts.referring_physician_name,
        ),
        string_element(TAG_PATIENT_NAME, Vr::PN, &opts.patient_name),
        string_element(TAG_PATIENT_ID, Vr::LO, &opts.patient_id),
        string_element(TAG_PATIENT_BIRTH_DATE, Vr::DA, &opts.patient_birth_date),
        string_element(TAG_PATIENT_SEX, Vr::CS, &opts.patient_sex),
        string_element(TAG_STUDY_INSTANCE_UID, Vr::UI, &study),
        string_element(TAG_SERIES_INSTANCE_UID, Vr::UI, &series),
        string_element(TAG_STUDY_ID, Vr::SH, &opts.study_id),
        string_element(
            TAG_SERIES_NUMBER,
            Vr::IS,
            &default_string(&opts.series_number, "1"),
        ),
        string_element(TAG_LATERALITY, Vr::CS, &laterality),
        string_element(
            TAG_INSTANCE_NUMBER,
            Vr::IS,
            &default_string(&opts.instance_number, "1"),
        ),
        string_element(TAG_PATIENT_ORIENTATION, Vr::CS, ""),
        us_element(TAG_SAMPLES_PER_PIXEL, samples),
        string_element(TAG_PHOTOMETRIC, Vr::CS, layout.photometric),
        us_element(TAG_ROWS, layout.rows),
        us_element(TAG_COLUMNS, layout.columns),
        us_element(TAG_BITS_ALLOCATED, allocated),
        us_element(TAG_BITS_STORED, layout.bits),
        us_element(TAG_HIGH_BIT, layout.bits - 1),
        us_element(TAG_PIXEL_REPRESENTATION, layout.signed),
        Element {
            header: ElementHeader {
                tag: Tag::PIXEL_DATA,
                vr: if native && allocated > 8 {
                    Vr::OW
                } else {
                    Vr::OB
                },
                length: if native { 0 } else { UNDEFINED_LENGTH },
                length_set: false,
            },
            value,
        },
    ];
    let optional = |elements: &mut Vec<Element>, tag: Tag, vr: Vr, value: &str| {
        if !value.trim().is_empty() {
            elements.push(string_element(tag, vr, value));
        }
    };
    let image_type = image_type.trim();
    if !image_type.is_empty() {
        elements.push(strings_element(
            TAG_IMAGE_TYPE,
            Vr::CS,
            image_type.split('\\').map(str::to_string).collect(),
        ));
    }
    optional(&mut elements, TAG_CONTENT_DATE, Vr::DA, &opts.content_date);
    optional(&mut elements, TAG_CONTENT_TIME, Vr::TM, &opts.content_time);
    optional(
        &mut elements,
        TAG_STUDY_DESCRIPTION,
        Vr::LO,
        &opts.study_description,
    );
    optional(
        &mut elements,
        TAG_SERIES_DESCRIPTION,
        Vr::LO,
        &opts.series_description,
    );
    optional(&mut elements, TAG_BURNED_IN_ANNOTATION, Vr::CS, &burned);
    if vlp {
        elements.push(string_element(TAG_MANUFACTURER, Vr::LO, &opts.manufacturer));
        elements.push(Element {
            header: ElementHeader {
                tag: TAG_ACQUISITION_CONTEXT_SEQ,
                vr: Vr::SQ,
                length: UNDEFINED_LENGTH,
                length_set: true,
            },
            value: Value::Sequence(Vec::new()),
        });
    } else {
        elements.push(string_element(
            TAG_CONVERSION_TYPE,
            Vr::CS,
            &default_string(&opts.conversion_type, "WSD"),
        ));
        optional(&mut elements, TAG_MANUFACTURER, Vr::LO, &opts.manufacturer);
    }
    if samples > 1 {
        elements.push(us_element(TAG_PLANAR_CONFIGURATION, 0));
    }
    if layout.lossy_unknown {
        elements.push(string_element(TAG_LOSSY_IMAGE_COMPRESSION, Vr::CS, ""));
    } else if layout.lossless {
        elements.push(string_element(TAG_LOSSY_IMAGE_COMPRESSION, Vr::CS, "00"));
    } else {
        let uncompressed = layout.rows as f64
            * layout.columns as f64
            * samples as f64
            * (allocated / 8) as f64
            * layout.frame_count.max(1) as f64;
        elements.push(string_element(TAG_LOSSY_IMAGE_COMPRESSION, Vr::CS, "01"));
        elements.push(string_element(
            TAG_LOSSY_COMPRESSION_RATIO,
            Vr::DS,
            &format_ratio(uncompressed / layout.payload_bytes as f64),
        ));
        elements.push(string_element(
            TAG_LOSSY_COMPRESSION_METHOD,
            Vr::CS,
            &default_string(layout.method, "ISO_10918_1"),
        ));
    }
    if let Some(mut icc) = icc
        && !opts.discard_icc_profile
    {
        if icc.len() % 2 != 0 {
            icc.push(0);
        }
        elements.push(raw_element(TAG_ICC_PROFILE, Vr::OB, icc));
        report.icc_profile_copied = true;
    }
    let text = [
        &opts.patient_name,
        &opts.patient_id,
        &opts.study_id,
        &opts.study_description,
        &opts.accession_number,
        &opts.referring_physician_name,
        &opts.series_description,
        &opts.manufacturer,
    ];
    if text.iter().any(|value| !value.is_ascii()) {
        elements.push(string_element(
            TAG_SPECIFIC_CHARACTER_SET,
            Vr::CS,
            "ISO_IR 192",
        ));
    }
    let mut dataset = Object::from_elements(elements);
    if let Some(source) = &opts.common.source {
        for (tag, value) in [
            (TAG_PATIENT_NAME, &opts.patient_name),
            (TAG_PATIENT_ID, &opts.patient_id),
            (TAG_PATIENT_BIRTH_DATE, &opts.patient_birth_date),
            (TAG_PATIENT_SEX, &opts.patient_sex),
            (TAG_STUDY_DATE, &opts.study_date),
            (TAG_STUDY_TIME, &opts.study_time),
            (TAG_ACCESSION_NUMBER, &opts.accession_number),
            (TAG_REFERRING_PHYSICIAN_NAME, &opts.referring_physician_name),
            (TAG_STUDY_ID, &opts.study_id),
            (TAG_STUDY_INSTANCE_UID, &opts.study_instance_uid),
        ] {
            if value.trim().is_empty() {
                dataset.remove(tag);
            }
        }
        let source_uid = source
            .get_string(TAG_STUDY_INSTANCE_UID)
            .unwrap_or_default();
        if !source_uid.trim_matches([' ', '\0']).is_empty() {
            report
                .generated_uids
                .retain(|keyword| *keyword != "StudyInstanceUID");
        }
    }
    if let Err(err) = opts
        .common
        .apply(&mut dataset, inherit_modules::WITHOUT_FRAME_OF_REFERENCE)
    {
        *report = Report::default();
        return Err(Error {
            kind: Kind::Other,
            text: err.to_string(),
        });
    }
    Ok(File {
        preamble: Vec::new(),
        meta: Object::new(),
        dataset,
        transfer_syntax: syntax,
        partial_read: None,
        incomplete_pixel_data: None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icc_chunks() {
        let chunk = |seq: u8, total: u8, data: &[u8]| {
            let mut p = b"ICC_PROFILE\x00".to_vec();
            p.extend_from_slice(&[seq, total]);
            p.extend_from_slice(data);
            p
        };
        let mut icc = IccChunks::default();
        icc.add(&chunk(2, 2, b"B"));
        icc.add(&chunk(1, 2, b"A"));
        assert_eq!(icc.profile(), Some(b"AB".to_vec()));
        let mut missing = IccChunks::default();
        missing.add(&chunk(1, 2, b"A"));
        assert_eq!(missing.profile(), None);
        let mut empty = IccChunks::default();
        empty.add(&chunk(1, 1, b""));
        assert_eq!(empty.profile(), None);
    }
    #[test]
    fn identifiers_and_ratio() {
        assert_eq!(application_identifier(0xe1, b"Exif\x00II"), "Exif");
        assert_eq!(application_identifier(0xe2, b"Exif\x00II"), "");
        assert_eq!(format_ratio(f64::INFINITY), "1");
        assert_eq!(format_ratio(2.34567), "2.346");
        assert_eq!(sof_name(0xc9), "arithmetic coding");
    }
}
