use super::extract::{Limits, copy, metadata};
use super::import::ImportLimits;
use super::import_mp4::inspect_mp4_profile;
use super::{CONTAINER_MP4, Error, Kind};
use crate::derivation::{self, Derivation, Source};
use crate::object::clone::{CloneFileOptions, clone_file_with_options};
use crate::object::element_setters::format_ds;
use crate::object::inherit::new_uid;
use crate::object::transfer;
use crate::object::writer::write_file;
use crate::object::{
    Element, ElementHeader, File, Object, ReadFileOptions, Tag, UNDEFINED_LENGTH, Value, Vr,
    WalkOptions, read_file_with_options,
};
use std::io::Write;
const MPEG4_HP41: &str = "1.2.840.10008.1.2.4.102";
const AVC_COMPRESSION_METHOD: &str = "ISO_14496_10";
const AVC_METADATA_BYTES: i64 = 16 << 20;
const MAX_DURATION_SECONDS: i64 = 3600;
const TAG_TRANSFER_SYNTAX_UID: Tag = Tag::new(0x0002, 0x0010);
const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
const TAG_SOP_CLASS_UID: Tag = Tag::new(0x0008, 0x0016);
const TAG_SOP_INSTANCE_UID: Tag = Tag::new(0x0008, 0x0018);
const TAG_RECOMMENDED_DISPLAY_FRAME_RATE: Tag = Tag::new(0x0008, 0x2144);
const TAG_CINE_RATE: Tag = Tag::new(0x0018, 0x0040);
const TAG_FRAME_TIME: Tag = Tag::new(0x0018, 0x1063);
const TAG_FRAME_TIME_VECTOR: Tag = Tag::new(0x0018, 0x1065);
const TAG_SAMPLES_PER_PIXEL: Tag = Tag::new(0x0028, 0x0002);
const TAG_PHOTOMETRIC_INTERPRETATION: Tag = Tag::new(0x0028, 0x0004);
const TAG_PLANAR_CONFIGURATION: Tag = Tag::new(0x0028, 0x0006);
const TAG_NUMBER_OF_FRAMES: Tag = Tag::new(0x0028, 0x0008);
const TAG_FRAME_INCREMENT_POINTER: Tag = Tag::new(0x0028, 0x0009);
const TAG_ROWS: Tag = Tag::new(0x0028, 0x0010);
const TAG_COLUMNS: Tag = Tag::new(0x0028, 0x0011);
const TAG_PIXEL_ASPECT_RATIO: Tag = Tag::new(0x0028, 0x0034);
const TAG_BITS_ALLOCATED: Tag = Tag::new(0x0028, 0x0100);
const TAG_BITS_STORED: Tag = Tag::new(0x0028, 0x0101);
const TAG_HIGH_BIT: Tag = Tag::new(0x0028, 0x0102);
const TAG_PIXEL_REPRESENTATION: Tag = Tag::new(0x0028, 0x0103);
const TAG_LOSSY: Tag = Tag::new(0x0028, 0x2110);
const TAG_LOSSY_RATIO: Tag = Tag::new(0x0028, 0x2112);
const TAG_LOSSY_METHOD: Tag = Tag::new(0x0028, 0x2114);
const TAG_EXTENDED_OFFSET_TABLE: Tag = Tag::new(0x7fe0, 0x0001);
const TAG_EXTENDED_OFFSET_TABLE_LENGTHS: Tag = Tag::new(0x7fe0, 0x0002);
const TAG_ENCAPSULATED_VALUE_LENGTH: Tag = Tag::new(0x7fe0, 0x0003);
pub type BackendError = Box<dyn std::error::Error + Send + Sync>;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AvcProfile {
    pub rows: i64,
    pub columns: i64,
    pub frames: i64,
    pub rate_numerator: u32,
    pub rate_denominator: u32,
    pub colour: [u8; 3],
}
impl AvcProfile {
    fn frame_time(&self) -> f64 {
        1000.0 * f64::from(self.rate_denominator) / f64::from(self.rate_numerator)
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.rows <= 0
            || self.columns <= 0
            || self.rows & 1 != 0
            || self.columns & 1 != 0
            || self.rate_numerator == 0
            || self.rate_denominator == 0
            || self.rate_numerator > i32::MAX as u32
            || self.rate_denominator > i32::MAX as u32
            || u64::from(self.rate_numerator) > 1000 * u64::from(self.rate_denominator)
        {
            return Err(Error::bare(Kind::CodecProfile));
        }
        Ok(())
    }
    pub fn frame_bytes(&self) -> i64 {
        let pixels = self.rows * self.columns;
        pixels + pixels / 2
    }
}
pub(crate) fn normalize_avc_rate(numerator: u32, denominator: u32) -> (u32, u32) {
    let (mut first, mut second) = (numerator, denominator);
    while second != 0 {
        (first, second) = (second, first % second);
    }
    (numerator / first, denominator / first)
}
pub trait AvcDecoder {
    fn decode_avc_mp4(
        &mut self,
        mp4: &[u8],
        profile: &AvcProfile,
        dst: &mut dyn Write,
    ) -> Result<(), BackendError>;
}
pub trait AvcEncoder {
    fn encode_avc_mp4(
        &mut self,
        samples: &[u8],
        profile: &AvcProfile,
        options: AvcEncodeOptions,
        dst: &mut dyn Write,
    ) -> Result<(), BackendError>;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AvcEncodeOptions {
    pub crf: i64,
    pub max_bytes: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AvcCodecLimits {
    pub import: ImportLimits,
    pub max_duration_seconds: i64,
}
impl AvcCodecLimits {
    fn normalized(mut self) -> Result<AvcCodecLimits, Error> {
        if self.max_duration_seconds < 0 || self.max_duration_seconds > MAX_DURATION_SECONDS {
            return Err(Error::bare(Kind::CodecProfile));
        }
        if self.max_duration_seconds == 0 {
            self.max_duration_seconds = 120;
        }
        self.import = self.import.normalize()?;
        Ok(self)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecodedAvc {
    pub profile: AvcProfile,
    pub samples: Vec<u8>,
}
pub fn decode_avc_mp4(
    source: &File,
    decoder: &mut dyn AvcDecoder,
    limits: AvcCodecLimits,
) -> Result<DecodedAvc, Error> {
    let limits = limits.normalized()?;
    let (mp4, profile) = read_dicom_avc(source, limits.import)?;
    let mut samples = Vec::new();
    decode_avc(&mp4, &profile, decoder, &mut samples)?;
    Ok(DecodedAvc { profile, samples })
}
fn profile_error(detail: &str) -> Error {
    Error::new(Kind::CodecProfile, detail)
}
fn read_dicom_avc(source: &File, limits: ImportLimits) -> Result<(Vec<u8>, AvcProfile), Error> {
    let info = metadata(source)?;
    if source.partial_read.is_some() || source.incomplete_pixel_data.is_some() {
        return Err(Error::other("dicom: file is the result of a partial read"));
    }
    if source.dataset.has_discarded_values() {
        return Err(Error::other(
            "dicom: payload was discarded during streaming",
        ));
    }
    if info.transfer_syntax_uid != MPEG4_HP41 {
        return Err(Error::bare(Kind::CodecProfile));
    }
    if let Some(declared) = source.meta.get_string(TAG_TRANSFER_SYNTAX_UID)
        && transfer::normalize_uid(&declared) != info.transfer_syntax_uid
    {
        return Err(profile_error("inconsistent File Meta transfer syntax"));
    }
    let mut mp4: Vec<u8> = Vec::new();
    let (info, copied) = copy(
        source,
        &mut mp4,
        Limits {
            max_bytes: limits.max_bytes,
            max_fragments: 1,
        },
    );
    copied?;
    if info.container != CONTAINER_MP4 {
        return Err(Error::bare(Kind::CodecProfile));
    }
    let mut inspected = inspect_avc(&mp4, limits);
    if inspected.is_err() && mp4.len() >= 2 && mp4.len().is_multiple_of(2) && mp4.last() == Some(&0)
    {
        mp4.pop();
        inspected = inspect_avc(&mp4, limits);
    }
    let profile = inspected?;
    verify_dicom_avc_metadata(&source.dataset, &profile)?;
    Ok((mp4, profile))
}
fn inspect_avc(mp4: &[u8], limits: ImportLimits) -> Result<AvcProfile, Error> {
    if mp4.len() as i64 > limits.max_bytes {
        return Err(Error::bare(Kind::MediaTooLarge));
    }
    let mut profile = AvcProfile::default();
    inspect_mp4_profile(mp4, limits, Some(&mut profile))?;
    Ok(profile)
}
fn decode_avc(
    mp4: &[u8],
    profile: &AvcProfile,
    decoder: &mut dyn AvcDecoder,
    dst: &mut dyn Write,
) -> Result<(), Error> {
    let mut w = BoundedAvcWriter {
        dst,
        remaining: profile.frame_bytes() * profile.frames,
        err: None,
    };
    let result = decoder.decode_avc_mp4(mp4, profile, &mut w);
    if let Some(err) = w.err {
        return Err(err);
    }
    if let Err(err) = result {
        return Err(backend_error(&err));
    }
    if w.remaining != 0 {
        return Err(Error::new(
            Kind::CodecBackend,
            "decoded frame span differs from the admitted frame count",
        ));
    }
    Ok(())
}
fn backend_error(err: &BackendError) -> Error {
    Error::new(Kind::CodecBackend, err)
}
struct BoundedAvcWriter<'a> {
    dst: &'a mut dyn Write,
    remaining: i64,
    err: Option<Error>,
}
impl BoundedAvcWriter<'_> {
    fn failed(&self) -> std::io::Error {
        let text = self.err.as_ref().map(Error::to_string).unwrap_or_default();
        std::io::Error::other(text)
    }
}
impl Write for BoundedAvcWriter<'_> {
    fn write(&mut self, p: &[u8]) -> std::io::Result<usize> {
        if self.err.is_some() {
            return Err(self.failed());
        }
        if p.len() as i64 > self.remaining {
            self.err = Some(Error::bare(Kind::MediaTooLarge));
            return Err(self.failed());
        }
        match self.dst.write(p) {
            Ok(n) => {
                let n = n.min(p.len());
                self.remaining -= n as i64;
                if n != p.len() {
                    self.err = Some(Error::other("short write"));
                    return Err(self.failed());
                }
                Ok(n)
            }
            Err(err) => {
                self.err = Some(Error::other(err.to_string()));
                Err(err)
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.dst.flush()
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AvcTranscodeOptions {
    pub limits: AvcCodecLimits,
    pub allow_lossy: bool,
    pub crf: i64,
    pub max_output_bytes: i64,
}
impl AvcTranscodeOptions {
    fn normalized(mut self) -> Result<AvcTranscodeOptions, Error> {
        if !self.allow_lossy || self.crf < 1 || self.crf > 51 {
            return Err(Error::bare(Kind::LossyPolicy));
        }
        if self.max_output_bytes < 0 || self.max_output_bytes > 1 << 30 {
            return Err(Error::bare(Kind::MediaTooLarge));
        }
        self.limits = self.limits.normalized()?;
        if self.max_output_bytes == 0 {
            self.max_output_bytes = self.limits.import.max_bytes;
        }
        Ok(self)
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AvcTranscodeReport {
    pub profile: AvcProfile,
    pub sop_instance_uid: String,
    pub source_sop_instance_uid: String,
    pub container_bytes: i64,
    pub compression_ratio: f64,
}
pub fn transcode_avc_mp4(
    source: &File,
    decoder: &mut dyn AvcDecoder,
    encoder: &mut dyn AvcEncoder,
    options: AvcTranscodeOptions,
) -> Result<(File, AvcTranscodeReport), Error> {
    transcode(source, decoder, encoder, options.normalized()?)
}
fn transcode(
    source: &File,
    decoder: &mut dyn AvcDecoder,
    encoder: &mut dyn AvcEncoder,
    options: AvcTranscodeOptions,
) -> Result<(File, AvcTranscodeReport), Error> {
    let (input, profile) = read_dicom_avc(source, options.limits.import)?;
    let predecessor = derivation::source_of(
        Some(&source.dataset),
        Some(derivation::codes::SourceImageForProcessing()),
    )
    .map_err(|err| Error::other(err.to_string()))?;
    let mut output = detached_avc_source(source, options.limits.import.max_bytes)?;
    let mut decoded: Vec<u8> = Vec::new();
    decode_avc(&input, &profile, decoder, &mut decoded)?;
    let mut encoded: Vec<u8> = Vec::new();
    let mut writer = BoundedAvcWriter {
        dst: &mut encoded,
        remaining: options.max_output_bytes,
        err: None,
    };
    let result = encoder.encode_avc_mp4(
        &decoded,
        &profile,
        AvcEncodeOptions {
            crf: options.crf,
            max_bytes: options.max_output_bytes,
        },
        &mut writer,
    );
    if let Some(err) = writer.err {
        return Err(err);
    }
    if let Err(err) = result {
        return Err(backend_error(&err));
    }
    let mut output_limits = options.limits.import;
    output_limits.max_bytes = options.max_output_bytes;
    let encoded_profile = inspect_avc(&encoded, output_limits)?;
    if encoded_profile != profile {
        return Err(profile_error("encoder changed geometry, timing or colour"));
    }
    decode_avc(&encoded, &profile, decoder, &mut std::io::sink())?;
    let mut report = AvcTranscodeReport {
        profile,
        source_sop_instance_uid: predecessor.sop_instance_uid.clone(),
        container_bytes: encoded.len() as i64,
        compression_ratio: decoded.len() as f64 / encoded.len() as f64,
        ..AvcTranscodeReport::default()
    };
    finish_avc_transcode(&mut output, predecessor, encoded, &mut report)?;
    Ok((output, report))
}
fn verify_dicom_avc_metadata(dataset: &Object, profile: &AvcProfile) -> Result<(), Error> {
    let class = dataset.get_string(TAG_SOP_CLASS_UID).unwrap_or_default();
    if !matches!(
        class.as_str(),
        "1.2.840.10008.5.1.4.1.1.77.1.1.1"
            | "1.2.840.10008.5.1.4.1.1.77.1.2.1"
            | "1.2.840.10008.5.1.4.1.1.77.1.4.1"
    ) {
        return Err(profile_error("expected a Video VL SOP Class"));
    }
    for (tag, value) in [
        (TAG_ROWS, profile.rows),
        (TAG_COLUMNS, profile.columns),
        (TAG_NUMBER_OF_FRAMES, profile.frames),
        (TAG_SAMPLES_PER_PIXEL, 3),
        (TAG_BITS_ALLOCATED, 8),
        (TAG_BITS_STORED, 8),
        (TAG_HIGH_BIT, 7),
        (TAG_PIXEL_REPRESENTATION, 0),
        (TAG_PLANAR_CONFIGURATION, 0),
    ] {
        if dataset.get_int(tag).ok() != Some(value) {
            return Err(Error::new(
                Kind::CodecProfile,
                format!("inconsistent {tag}"),
            ));
        }
    }
    let photometric = dataset
        .get_string(TAG_PHOTOMETRIC_INTERPRETATION)
        .unwrap_or_default();
    if photometric != "YBR_PARTIAL_420"
        || dataset.has(TAG_PIXEL_ASPECT_RATIO)
        || dataset.has(TAG_FRAME_TIME_VECTOR)
    {
        return Err(profile_error(
            "unsupported photometric interpretation, pixel aspect or timing vector",
        ));
    }
    let constant = match dataset.get_floats(TAG_FRAME_TIME).as_deref() {
        Ok(&[time]) => (time - profile.frame_time()).abs() <= 0.01,
        _ => false,
    };
    let pointer = dataset
        .get(TAG_FRAME_INCREMENT_POINTER)
        .is_some_and(|elem| is_frame_time_pointer(&elem.value, dataset));
    if !constant || !pointer {
        return Err(profile_error("inconsistent constant Frame Time"));
    }
    let nominal = f64::from(profile.rate_numerator) / f64::from(profile.rate_denominator);
    for tag in [TAG_CINE_RATE, TAG_RECOMMENDED_DISPLAY_FRAME_RATE] {
        if dataset.has(tag) {
            match dataset.get_float(tag) {
                Ok(rate) if (rate - nominal).abs() <= 0.5 || rate.is_nan() => {}
                _ => return Err(profile_error("inconsistent display rate")),
            }
        }
    }
    Ok(())
}
fn is_frame_time_pointer(value: &Value, dataset: &Object) -> bool {
    let Value::Raw(raw) = value else {
        return false;
    };
    let word = |b: &[u8]| {
        if dataset.value_big_endian() {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        }
    };
    raw.len() == 4
        && word(&raw[..2]) == TAG_FRAME_TIME.group
        && word(&raw[2..]) == TAG_FRAME_TIME.element
}
fn detached_avc_source(source: &File, max_input: i64) -> Result<File, Error> {
    let refused = [
        Tag::new(0xfffa, 0xfffa),
        Tag::new(0x4ffe, 0x0001),
        Tag::new(0x0400, 0x0500),
        Tag::new(0x5200, 0x9229),
        Tag::new(0x5200, 0x9230),
    ];
    let mut refusal: Option<Error> = None;
    let walked = source.dataset.walk_with(
        WalkOptions {
            max_depth: 32,
            max_elements: 100_000,
        },
        |path, element| {
            let tag = element.tag();
            if refused.contains(&tag) {
                refusal = Some(profile_error("signed, encrypted or functional-group video"));
            } else if path.len() > 1 && tag == Tag::PIXEL_DATA {
                refusal = Some(profile_error("nested Pixel Data"));
            }
            match refusal {
                Some(_) => Err(crate::object::Error::msg("stop")),
                None => Ok(()),
            }
        },
    );
    if let Some(err) = refusal {
        return Err(err);
    }
    walked.map_err(|err| Error::other(err.to_string()))?;
    clone_file_with_options(
        source,
        CloneFileOptions {
            max_element_bytes: AVC_METADATA_BYTES,
            max_pixel_data_bytes: max_input,
            max_total_bytes: max_input + AVC_METADATA_BYTES,
            max_sequence_depth: 32,
            max_elements: 100_000,
            max_items: 0,
            max_fragments: 1,
        },
    )
    .map_err(|err| Error::other(err.to_string()))
}
fn finish_avc_transcode(
    output: &mut File,
    predecessor: Source,
    mut encoded: Vec<u8>,
    report: &mut AvcTranscodeReport,
) -> Result<(), Error> {
    let object_error = |err: crate::object::Error| Error::other(err.to_string());
    let result = &mut output.dataset;
    let uid = new_uid();
    report.sop_instance_uid = uid.clone();
    let mut image_type = result.get_strings(TAG_IMAGE_TYPE).unwrap_or_default();
    if image_type.len() < 2 {
        image_type = vec!["DERIVED".to_string(), "SECONDARY".to_string()];
    } else {
        image_type[0] = "DERIVED".to_string();
        image_type[1] = "SECONDARY".to_string();
    }
    for (tag, vr, values) in [
        (TAG_SOP_INSTANCE_UID, Vr::UI, vec![uid]),
        (TAG_IMAGE_TYPE, Vr::CS, image_type),
        (TAG_LOSSY, Vr::CS, vec!["01".to_string()]),
    ] {
        result.set_strings(tag, vr, &values).map_err(object_error)?;
    }
    append_avc_loss_history(result, report.compression_ratio)?;
    derivation::apply_to(
        Some(result),
        &Derivation {
            description: derivation::lossy_description(
                AVC_COMPRESSION_METHOD,
                report.compression_ratio,
            ),
            codes: vec![derivation::codes::LossyCompression()],
            sources: vec![predecessor],
        },
    )
    .map_err(|err| Error::other(err.to_string()))?;
    for tag in [
        TAG_EXTENDED_OFFSET_TABLE,
        TAG_EXTENDED_OFFSET_TABLE_LENGTHS,
        TAG_ENCAPSULATED_VALUE_LENGTH,
    ] {
        result.remove(tag);
    }
    if encoded.len() & 1 != 0 {
        encoded.push(0);
    }
    result.put(Element {
        header: ElementHeader {
            tag: Tag::PIXEL_DATA,
            vr: Vr::OB,
            length: UNDEFINED_LENGTH,
            length_set: true,
        },
        value: Value::Fragments {
            offset_table: Vec::new(),
            fragments: vec![encoded],
        },
    });
    output.transfer_syntax =
        transfer::lookup(MPEG4_HP41).ok_or_else(|| Error::bare(Kind::CodecProfile))?;
    output.rebuild_file_meta().map_err(object_error)
}
fn append_avc_loss_history(result: &mut Object, ratio: f64) -> Result<(), Error> {
    let object_error = |err: crate::object::Error| Error::other(err.to_string());
    let blank = |values: &[String]| values.len() == 1 && values[0].trim().is_empty();
    let mut methods = result.get_strings(TAG_LOSSY_METHOD).unwrap_or_default();
    let mut ratios = result.get_strings(TAG_LOSSY_RATIO).unwrap_or_default();
    if blank(&methods) {
        methods.clear();
    }
    if blank(&ratios) {
        ratios.clear();
    }
    if methods.is_empty() {
        methods.push(AVC_COMPRESSION_METHOD.to_string());
    }
    if methods.iter().any(|method| method.trim().is_empty()) {
        return Err(profile_error("empty lossy compression method"));
    }
    if !ratios.is_empty() {
        if ratios.len() != methods.len() {
            return Err(profile_error("unaligned lossy compression history"));
        }
        match result.get_floats(TAG_LOSSY_RATIO) {
            Ok(values) if values.len() == ratios.len() => {
                if values
                    .iter()
                    .any(|v| v.is_nan() || *v <= 0.0 || v.is_infinite())
                {
                    return Err(profile_error("nonpositive lossy compression ratio"));
                }
            }
            _ => return Err(profile_error("invalid lossy compression ratios")),
        }
        ratios.push(format_ds(ratio).map_err(object_error)?);
        result
            .set_strings(TAG_LOSSY_RATIO, Vr::DS, &ratios)
            .map_err(object_error)?;
    } else {
        result.remove(TAG_LOSSY_RATIO);
    }
    methods.push(AVC_COMPRESSION_METHOD.to_string());
    result
        .set_strings(TAG_LOSSY_METHOD, Vr::CS, &methods)
        .map_err(object_error)
}
pub fn transcode_avc_mp4_path(
    source_path: &str,
    target_path: &str,
    decoder: &mut dyn AvcDecoder,
    encoder: &mut dyn AvcEncoder,
    options: AvcTranscodeOptions,
) -> Result<AvcTranscodeReport, Error> {
    let io_error = |err: std::io::Error| Error::other(err.to_string());
    let options = options.normalized()?;
    let max_bytes = options.limits.import.max_bytes;
    let input = std::fs::File::open(source_path).map_err(io_error)?;
    let info = input.metadata().map_err(io_error)?;
    if !info.is_file() || info.len() > (max_bytes + AVC_METADATA_BYTES) as u64 {
        return Err(Error::bare(Kind::MediaTooLarge));
    }
    separate_avc_target(target_path, source_path)?;
    let data = std::fs::read(source_path).map_err(io_error)?;
    let source = read_file_with_options(
        &data,
        ReadFileOptions {
            max_total_bytes: max_bytes + AVC_METADATA_BYTES,
            max_element_bytes: AVC_METADATA_BYTES,
            max_pixel_data_bytes: max_bytes,
            max_fragments: 1,
            max_sequence_depth: 32,
            max_elements: 100_000,
            ..ReadFileOptions::default()
        },
    )
    .map_err(|(_, err)| Error::other(err.to_string()))?;
    let (output, report) = transcode(&source, decoder, encoder, options)?;
    let bytes = write_file(&output).map_err(|err| Error::other(err.to_string()))?;
    if bytes.len() as i64 > options.max_output_bytes + AVC_METADATA_BYTES {
        return Err(Error::bare(Kind::MediaTooLarge));
    }
    let target = std::path::Path::new(target_path);
    let directory = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let temp = directory.join(format!(".dicom-video-{}", uuid::Uuid::new_v4().simple()));
    let published = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(io_error)?;
        file.write_all(&bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        separate_avc_target(target_path, source_path)?;
        std::fs::rename(&temp, target_path).map_err(io_error)
    })();
    if published.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    published.map(|()| report)
}
fn separate_avc_target(target: &str, source: &str) -> Result<(), Error> {
    let io_error = |err: std::io::Error| Error::other(err.to_string());
    let info = match std::fs::symlink_metadata(target) {
        Ok(info) => info,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(io_error(err)),
    };
    let same = info.is_file()
        && std::fs::canonicalize(target).map_err(io_error)?
            == std::fs::canonicalize(source).map_err(io_error)?;
    if !info.is_file() || same {
        return Err(profile_error("target must be a separate regular file"));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rates_and_profiles() {
        assert_eq!(normalize_avc_rate(30000, 1001), (30000, 1001));
        assert_eq!(normalize_avc_rate(90000, 3600), (25, 1));
        let mut profile = AvcProfile {
            rows: 480,
            columns: 640,
            frames: 2,
            rate_numerator: 25,
            rate_denominator: 1,
            colour: [2; 3],
        };
        assert!(profile.validate().is_ok());
        assert_eq!(profile.frame_bytes(), 460_800);
        assert_eq!(profile.frame_time(), 40.0);
        profile.rows = 481;
        assert!(profile.validate().is_err());
    }
    #[test]
    fn bounded_writer_refuses_overrun() {
        let mut out = Vec::new();
        let mut w = BoundedAvcWriter {
            dst: &mut out,
            remaining: 3,
            err: None,
        };
        assert!(w.write(&[1, 2]).is_ok());
        assert!(w.write(&[3, 4]).is_err());
        assert!(w.write(&[5]).is_err());
        assert_eq!(w.err.map(|e| e.kind), Some(Kind::MediaTooLarge));
        assert_eq!(out, [1, 2]);
    }
}
