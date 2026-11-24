use super::super::Metadata;
use super::super::pixeldata::{NativeFrames, frame_size, normalized_photometric};
use super::{
    Error, ErrorKind, TAG_ENCAPSULATED_TOTAL_LENGTH, TAG_EXTENDED_OFFSET_TABLE,
    TAG_EXTENDED_OFFSET_TABLE_LENGTHS, TAG_PHOTOMETRIC, TAG_PLANAR_CONFIGURATION,
    TAG_SOP_INSTANCE_UID, TranscodeLimits, TranscodeOptions, TranscodeReport, UidPolicy,
    clone_detached, fragments_element, object_from, output_clone_limits, raw_element,
    remove_elements, replace_element, strings_element, uint16_element,
};
#[cfg(feature = "jpeg")]
use crate::codec::jpeg;
#[cfg(feature = "jpeg2000")]
use crate::codec::jpeg2000;
#[cfg(feature = "jpeg-lossless")]
use crate::codec::jpeglossless;
#[cfg(feature = "jpeg-ls")]
use crate::codec::jpegls;
#[cfg(feature = "jpeg-xl")]
use crate::codec::jpegxl;
use crate::codec::rle;
use crate::derivation::{self, Derivation};
use crate::object::{Element, Object, Tag, Vr};
use crate::textfmt;
use crate::transfer::{self, Syntax};
const ERR_CAPABILITIES_INVALID: &str = "dicom: invalid pixel data encoder capabilities";
const ERR_OUTPUT_INVALID: &str = "dicom: pixel data encoder returned invalid output";
const ERR_UNSUPPORTED_METADATA: &str = "dicom: pixel metadata is unsupported by encoder";
const TAG_LOSSY: Tag = Tag::new(0x0028, 0x2110);
const TAG_LOSSY_RATIO: Tag = Tag::new(0x0028, 0x2112);
const TAG_LOSSY_METHOD: Tag = Tag::new(0x0028, 0x2114);
const TAG_IMAGE_TYPE: Tag = Tag::new(0x0008, 0x0008);
const DEFLATED_IMAGE_FRAME: &str = "1.2.840.10008.1.2.8.1";
#[cfg(feature = "jpeg2000")]
const UID_JPEG2000_PART2_LOSSLESS: &str = "1.2.840.10008.1.2.4.92";
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum FrameEncoder {
    Rle,
    #[cfg(feature = "jpeg")]
    JpegBaseline(jpeg::Encoder),
    #[cfg(feature = "jpeg")]
    JpegExtended(jpeg::ExtendedEncoder),
    #[cfg(feature = "jpeg-lossless")]
    JpegLossless(jpeglossless::Encoder),
    #[cfg(feature = "jpeg-ls")]
    JpegLs(jpegls::Encoder),
    Deflate(i32),
    #[cfg(feature = "jpeg2000")]
    Jpeg2000(jpeg2000::encoder::Encoder),
    #[cfg(feature = "jpeg2000")]
    Jpeg2000Part2(jpeg2000::encoder::Part2Encoder),
    #[cfg(feature = "jpeg-xl")]
    JpegXlLossless,
    #[cfg(feature = "jpeg-xl")]
    JpegXlLossy(jpegxl::dicom::LossyEncoder),
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncoderCapabilities {
    pub transfer_syntax_uid: String,
    pub bits_allocated: Vec<u16>,
    pub pixel_representations: Vec<u16>,
    pub samples_per_pixel: Vec<u16>,
    pub photometric_interpretations: Vec<String>,
    pub output_photometric_interpretations: Vec<String>,
    pub output_planar_configurations: Vec<u16>,
    pub lossless: bool,
    pub lossy_method: String,
    pub supports_multi_frame: bool,
    pub backend: String,
    pub compatibility_profiles: Vec<String>,
}
fn strs(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}
impl FrameEncoder {
    pub fn capabilities(&self) -> EncoderCapabilities {
        match self {
            FrameEncoder::Rle => {
                let c = rle::capabilities();
                EncoderCapabilities {
                    transfer_syntax_uid: c.transfer_syntax_uid.into(),
                    bits_allocated: c.bits_allocated,
                    pixel_representations: c.pixel_representations,
                    samples_per_pixel: c.samples_per_pixel,
                    photometric_interpretations: strs(&c.photometric_interpretations),
                    lossless: true,
                    supports_multi_frame: true,
                    backend: c.backend.into(),
                    ..EncoderCapabilities::default()
                }
            }
            #[cfg(feature = "jpeg")]
            FrameEncoder::JpegBaseline(e) => {
                let c = e.capabilities();
                EncoderCapabilities {
                    transfer_syntax_uid: c.transfer_syntax_uid.into(),
                    bits_allocated: c.bits_allocated,
                    pixel_representations: vec![0],
                    samples_per_pixel: c.samples_per_pixel,
                    photometric_interpretations: strs(&c.photometric_interpretations),
                    output_photometric_interpretations: strs(&c.output_photometric_interpretations),
                    output_planar_configurations: vec![0],
                    lossless: false,
                    lossy_method: "ISO_10918_1".into(),
                    supports_multi_frame: true,
                    backend: c.backend.into(),
                    compatibility_profiles: Vec::new(),
                }
            }
            #[cfg(feature = "jpeg")]
            FrameEncoder::JpegExtended(e) => {
                let c = e.capabilities();
                EncoderCapabilities {
                    transfer_syntax_uid: c.transfer_syntax_uid.into(),
                    bits_allocated: c.bits_allocated,
                    pixel_representations: vec![0],
                    samples_per_pixel: c.samples_per_pixel,
                    photometric_interpretations: strs(&c.photometric_interpretations),
                    lossless: false,
                    lossy_method: "ISO_10918_1".into(),
                    supports_multi_frame: true,
                    backend: c.backend.into(),
                    compatibility_profiles: strs(&c.compatibility_profiles),
                    ..EncoderCapabilities::default()
                }
            }
            #[cfg(feature = "jpeg-lossless")]
            FrameEncoder::JpegLossless(e) => EncoderCapabilities {
                transfer_syntax_uid: e.transfer_syntax().into(),
                bits_allocated: vec![8, 16],
                pixel_representations: vec![0, 1],
                samples_per_pixel: vec![1, 3],
                photometric_interpretations: strs(&[
                    "MONOCHROME1",
                    "MONOCHROME2",
                    "RGB",
                    "YBR_FULL",
                ]),
                lossless: true,
                supports_multi_frame: true,
                backend: "pure-rust".into(),
                ..EncoderCapabilities::default()
            },
            #[cfg(feature = "jpeg-ls")]
            FrameEncoder::JpegLs(e) => {
                let c = e.capabilities();
                EncoderCapabilities {
                    transfer_syntax_uid: c.transfer_syntax_uid.into(),
                    bits_allocated: c.bits_allocated,
                    pixel_representations: c.pixel_representations,
                    samples_per_pixel: c.samples_per_pixel,
                    photometric_interpretations: strs(&c.photometric_interpretations),
                    lossless: c.lossless,
                    lossy_method: c.lossy_method.into(),
                    supports_multi_frame: true,
                    backend: c.backend.into(),
                    ..EncoderCapabilities::default()
                }
            }
            FrameEncoder::Deflate(_) => EncoderCapabilities {
                transfer_syntax_uid: DEFLATED_IMAGE_FRAME.into(),
                bits_allocated: vec![1, 8, 16, 24, 32, 40, 48, 56, 64],
                pixel_representations: vec![0, 1],
                samples_per_pixel: vec![1, 3],
                photometric_interpretations: strs(&DEFLATE_PHOTOMETRICS),
                lossless: true,
                supports_multi_frame: true,
                backend: "pure-rust".into(),
                ..EncoderCapabilities::default()
            },
            #[cfg(feature = "jpeg2000")]
            FrameEncoder::Jpeg2000(e) => j2k_capabilities(e.capabilities()),
            #[cfg(feature = "jpeg2000")]
            FrameEncoder::Jpeg2000Part2(e) => j2k_capabilities(e.capabilities()),
            #[cfg(feature = "jpeg-xl")]
            FrameEncoder::JpegXlLossless => EncoderCapabilities {
                transfer_syntax_uid: jpegxl::dicom::UID_LOSSLESS.into(),
                bits_allocated: vec![8, 16],
                pixel_representations: vec![0, 1],
                samples_per_pixel: vec![1, 3],
                photometric_interpretations: strs(&["MONOCHROME1", "MONOCHROME2", "RGB"]),
                lossless: true,
                supports_multi_frame: true,
                backend: jpegxl::dicom::BACKEND.into(),
                ..EncoderCapabilities::default()
            },
            #[cfg(feature = "jpeg-xl")]
            FrameEncoder::JpegXlLossy(_) => EncoderCapabilities {
                transfer_syntax_uid: jpegxl::dicom::UID_JPEG_XL.into(),
                bits_allocated: vec![8],
                pixel_representations: vec![0],
                samples_per_pixel: vec![1, 3],
                photometric_interpretations: strs(&["MONOCHROME1", "MONOCHROME2", "RGB"]),
                lossy_method: jpegxl::dicom::LOSSY_METHOD.into(),
                supports_multi_frame: true,
                backend: jpegxl::dicom::BACKEND.into(),
                ..EncoderCapabilities::default()
            },
        }
    }
    fn encode(&self, frame: &[u8], m: &Metadata) -> Result<Encoded, ()> {
        #[cfg(any(
            feature = "jpeg",
            feature = "jpeg-lossless",
            feature = "jpeg-ls",
            feature = "jpeg2000"
        ))]
        let cm = codec_metadata(m);
        let plain = |data: Vec<u8>| Encoded {
            data,
            ..Encoded::default()
        };
        match self {
            FrameEncoder::Rle => rle::encode_frame(frame, m).map(plain).map_err(|_| ()),
            #[cfg(feature = "jpeg")]
            FrameEncoder::JpegBaseline(e) => e
                .encode_frame(frame, &cm)
                .map(|f| Encoded {
                    data: f.data,
                    photometric: f.photometric_interpretation,
                    planar: f.planar_configuration,
                    profile: f.compatibility_profile.to_string(),
                })
                .map_err(|_| ()),
            #[cfg(feature = "jpeg")]
            FrameEncoder::JpegExtended(e) => e
                .encode_frame(frame, &cm)
                .map(|f| Encoded {
                    data: f.data,
                    photometric: f.photometric_interpretation,
                    planar: f.planar_configuration,
                    profile: f.compatibility_profile.to_string(),
                })
                .map_err(|_| ()),
            #[cfg(feature = "jpeg-lossless")]
            FrameEncoder::JpegLossless(e) => e.encode_frame(frame, &cm).map(plain).map_err(|_| ()),
            #[cfg(feature = "jpeg-ls")]
            FrameEncoder::JpegLs(e) => e.encode_frame(frame, &cm).map(plain).map_err(|_| ()),
            FrameEncoder::Deflate(level) => deflate_frame(frame, m, *level).map(plain),
            #[cfg(feature = "jpeg2000")]
            FrameEncoder::Jpeg2000(e) => e
                .encode_frame(frame, &cm)
                .map(|f| Encoded {
                    data: f.data,
                    photometric: f.photometric_interpretation,
                    planar: f.planar_configuration,
                    profile: String::new(),
                })
                .map_err(|_| ()),
            #[cfg(feature = "jpeg-xl")]
            FrameEncoder::JpegXlLossless => jpegxl::dicom::encode_lossless_frame(frame, m)
                .map(plain)
                .map_err(|_| ()),
            #[cfg(feature = "jpeg-xl")]
            FrameEncoder::JpegXlLossy(e) => e.encode_frame(frame, m).map(plain).map_err(|_| ()),
            #[cfg(feature = "jpeg2000")]
            FrameEncoder::Jpeg2000Part2(_) => Err(()),
        }
    }
}
#[cfg(feature = "jpeg2000")]
fn j2k_capabilities(c: jpeg2000::encoder::EncoderCapabilities) -> EncoderCapabilities {
    EncoderCapabilities {
        transfer_syntax_uid: c.transfer_syntax_uid,
        bits_allocated: c.bits_allocated,
        pixel_representations: c.pixel_representations,
        samples_per_pixel: c.samples_per_pixel,
        photometric_interpretations: c.photometric_interpretations,
        output_photometric_interpretations: c.output_photometric_interpretations,
        output_planar_configurations: c.output_planar_configurations,
        lossless: c.lossless,
        lossy_method: c.lossy_method,
        supports_multi_frame: c.supports_multi_frame,
        backend: c.backend,
        compatibility_profiles: Vec::new(),
    }
}
const DEFLATE_PHOTOMETRICS: [&str; 6] = [
    "MONOCHROME1",
    "MONOCHROME2",
    "PALETTE COLOR",
    "RGB",
    "YBR_FULL",
    "YBR_FULL_422",
];
fn deflate_frame(frame: &[u8], m: &Metadata, level: i32) -> Result<Vec<u8>, ()> {
    let photometric = textfmt::to_upper(m.photometric_interpretation.trim());
    if !DEFLATE_PHOTOMETRICS.contains(&photometric.as_str()) {
        return Err(());
    }
    if m.rows == 0 || m.columns == 0 || m.samples_per_pixel == 0 {
        return Err(());
    }
    let ba = m.bits_allocated;
    if !(ba == 1 || (8..=64).contains(&ba) && ba.is_multiple_of(8)) {
        return Err(());
    }
    if !matches!(m.samples_per_pixel, 1 | 3 | 4) || m.number_of_frames < 1 {
        return Err(());
    }
    let size = frame_size(m);
    if size <= 0 || frame.len() as i64 != size || frame.is_empty() {
        return Err(());
    }
    let chunk = 256 << 10;
    let ends: Vec<usize> = (1..)
        .map(|i: usize| i.saturating_mul(chunk))
        .take_while(|e| *e < frame.len())
        .collect();
    let mut out = crate::deflater::compress_chunks(frame, &ends, level).map_err(|_| ())?;
    if out.len() % 2 != 0 {
        out.push(0);
    }
    if out.len() as u64 > 0xFFFF_FFFE {
        return Err(());
    }
    Ok(out)
}
#[cfg(any(
    feature = "jpeg",
    feature = "jpeg-lossless",
    feature = "jpeg-ls",
    feature = "jpeg2000"
))]
fn codec_metadata(m: &Metadata) -> crate::codec::common::Metadata {
    crate::codec::common::Metadata {
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Encoded {
    pub(super) data: Vec<u8>,
    pub(super) photometric: String,
    pub(super) planar: Option<u16>,
    pub(super) profile: String,
}
#[derive(Debug, Default)]
pub struct EncoderCounter {
    calls: std::sync::atomic::AtomicU64,
    nanos: std::sync::atomic::AtomicU64,
}
impl EncoderCounter {
    pub fn calls(&self) -> u64 {
        self.calls.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn duration(&self) -> std::time::Duration {
        std::time::Duration::from_nanos(self.nanos.load(std::sync::atomic::Ordering::Relaxed))
    }
    fn add(&self, started: std::time::Instant) {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.nanos
            .fetch_add(nanos, std::sync::atomic::Ordering::Relaxed);
    }
}
#[derive(Debug, Clone, Default)]
pub struct EncoderRegistry {
    encoders: Vec<(String, FrameEncoder, EncoderCapabilities)>,
    counter: Option<std::sync::Arc<EncoderCounter>>,
}
impl EncoderRegistry {
    pub fn new() -> EncoderRegistry {
        EncoderRegistry::default()
    }
    pub fn register(&mut self, uid: &str, encoder: FrameEncoder) -> Result<(), Error> {
        let uid = transfer::normalize_uid(uid).to_string();
        if uid.is_empty() {
            return Err(Error::new(
                ErrorKind::Other,
                "dicom: pixel data encoder transfer syntax UID is empty",
            ));
        }
        let mut caps = encoder.capabilities();
        caps.transfer_syntax_uid = transfer::normalize_uid(&caps.transfer_syntax_uid).to_string();
        if caps.transfer_syntax_uid != uid || !valid_capabilities(&caps) {
            return Err(Error::new(
                ErrorKind::EncoderCapabilitiesInvalid,
                ERR_CAPABILITIES_INVALID,
            ));
        }
        if self.encoders.iter().any(|(u, _, _)| *u == uid) {
            return Err(Error::new(
                ErrorKind::Other,
                "dicom: pixel data encoder transfer syntax is already registered",
            ));
        }
        self.encoders.push((uid, encoder, caps));
        Ok(())
    }
    pub fn with_counter(mut self, counter: std::sync::Arc<EncoderCounter>) -> EncoderRegistry {
        self.counter = Some(counter);
        self
    }
    pub fn register_own(&mut self, encoder: FrameEncoder) -> Result<(), Error> {
        let uid = encoder.capabilities().transfer_syntax_uid;
        self.register(&uid, encoder)
    }
    fn get(&self, uid: &str) -> Option<&(String, FrameEncoder, EncoderCapabilities)> {
        let uid = transfer::normalize_uid(uid);
        self.encoders.iter().find(|(u, _, _)| u == uid)
    }
    pub fn has(&self, uid: &str) -> bool {
        self.get(uid).is_some()
    }
    pub fn capabilities_of(&self, uid: &str) -> Option<EncoderCapabilities> {
        self.get(uid).map(|(_, _, c)| c.clone())
    }
    pub fn registered_uids(&self) -> Vec<String> {
        let mut uids: Vec<String> = self.encoders.iter().map(|(u, _, _)| u.clone()).collect();
        uids.sort();
        uids
    }
}
fn valid_lossy_method(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && value.len() <= 16
        && value
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == ' ' || c == '_')
}
fn valid_capabilities(c: &EncoderCapabilities) -> bool {
    let mut seen: Vec<&str> = Vec::new();
    for p in &c.compatibility_profiles {
        if !matches!(
            p.as_str(),
            "jpeg-extended-color" | "jpeg-extended-reduced-precision"
        ) || seen.contains(&p.as_str())
            || c.transfer_syntax_uid != "1.2.840.10008.1.2.4.51"
        {
            return false;
        }
        seen.push(p);
    }
    if c.transfer_syntax_uid.is_empty()
        || c.bits_allocated.is_empty()
        || c.pixel_representations.is_empty()
        || c.samples_per_pixel.is_empty()
        || c.photometric_interpretations.is_empty()
    {
        return false;
    }
    if !c.lossless && !valid_lossy_method(&c.lossy_method) {
        return false;
    }
    c.bits_allocated
        .iter()
        .all(|b| *b != 0 && (*b == 1 || b % 8 == 0))
        && c.pixel_representations.iter().all(|r| *r <= 1)
        && c.samples_per_pixel.iter().all(|s| *s != 0)
        && c.photometric_interpretations
            .iter()
            .all(|p| !p.trim().is_empty())
        && c.output_photometric_interpretations
            .iter()
            .all(|p| !p.trim().is_empty())
        && c.output_planar_configurations.iter().all(|p| *p <= 1)
}
pub(crate) fn require_lossless_encoder(
    registry: Option<&EncoderRegistry>,
    uid: &str,
) -> Result<(), Error> {
    let Some(registry) = registry else {
        return Err(Error::new(
            ErrorKind::EncoderUnavailable,
            "dicom: pixel data encoder registry is nil",
        ));
    };
    let Some((_, _, caps)) = registry.get(uid) else {
        return Err(encoder_availability(
            "dicom: no pixel data encoder registered for transfer syntax",
            uid,
            Some(Vec::new()),
        ));
    };
    if !caps.lossless {
        return Err(Error::new(
            ErrorKind::LossyDisallowed,
            super::ERR_LOSSY_DISALLOWED,
        ));
    }
    Ok(())
}
fn encoder_availability(cause: &str, uid: &str, registered: Option<Vec<String>>) -> Error {
    let mut text = cause.to_string();
    if !uid.is_empty() {
        text.push_str(&format!(": transfer syntax {}", textfmt::quote(uid)));
        if let Some(s) = transfer::default_registry().get(uid)
            && !s.name.is_empty()
        {
            text.push_str(&format!(" ({})", s.name));
        }
    }
    if let Some(registered) = registered {
        if registered.is_empty() {
            text.push_str("; registered encoders: none");
        } else {
            text.push_str(&format!("; registered encoders: {}", registered.join(", ")));
        }
    }
    Error::new(ErrorKind::EncoderUnavailable, text)
}
fn unsupported_metadata(field: &str) -> Error {
    Error::new(
        ErrorKind::UnsupportedEncoderMetadata,
        format!("{ERR_UNSUPPORTED_METADATA}: {field}"),
    )
}
fn validate_encoder_metadata(c: &EncoderCapabilities, m: &Metadata) -> Result<(), Error> {
    if m.number_of_frames > 1 && !c.supports_multi_frame {
        return Err(unsupported_metadata("NumberOfFrames"));
    }
    if !c.bits_allocated.contains(&m.bits_allocated) {
        return Err(unsupported_metadata("BitsAllocated"));
    }
    if !c.pixel_representations.contains(&m.pixel_representation) {
        return Err(unsupported_metadata("PixelRepresentation"));
    }
    if !c.samples_per_pixel.contains(&m.samples_per_pixel) {
        return Err(unsupported_metadata("SamplesPerPixel"));
    }
    let photometric = normalized_photometric(&m.photometric_interpretation);
    if !c
        .photometric_interpretations
        .iter()
        .any(|p| normalized_photometric(p) == photometric)
    {
        return Err(unsupported_metadata("PhotometricInterpretation"));
    }
    Ok(())
}
pub(crate) struct Resolved<'r> {
    pub(super) encoder: &'r FrameEncoder,
    counter: Option<&'r std::sync::Arc<EncoderCounter>>,
    pub(crate) caps: EncoderCapabilities,
    target: String,
}
pub(crate) fn resolve<'r>(
    target: &Syntax,
    m: &Metadata,
    options: &'r TranscodeOptions,
) -> Result<Resolved<'r>, Error> {
    if target.media_payload
        || super::super::pixeldata::is_jpip_referenced_transfer_syntax(&target.uid)
    {
        return Err(Error::unsupported(
            "target is not a still-image encoder syntax",
        ));
    }
    let Some(registry) = &options.encoders else {
        return Err(encoder_availability(
            "dicom: pixel data encoder registry is nil",
            &target.uid,
            None,
        ));
    };
    let Some((_, encoder, caps)) = registry.get(&target.uid) else {
        return Err(encoder_availability(
            "dicom: no pixel data encoder registered for transfer syntax",
            transfer::normalize_uid(&target.uid),
            Some(registry.registered_uids()),
        ));
    };
    if transfer::normalize_uid(&caps.transfer_syntax_uid) != target.uid || !valid_capabilities(caps)
    {
        return Err(Error::new(
            ErrorKind::EncoderCapabilitiesInvalid,
            ERR_CAPABILITIES_INVALID,
        ));
    }
    if !caps.lossless && !options.allow_lossy {
        return Err(Error::new(
            ErrorKind::LossyDisallowed,
            super::ERR_LOSSY_DISALLOWED,
        ));
    }
    if !caps.lossless && options.uid_policy == UidPolicy::Preserve {
        return Err(Error::unsupported(
            "a new lossy generation requires a new UID",
        ));
    }
    validate_encoder_metadata(caps, m)?;
    Ok(Resolved {
        encoder,
        counter: registry.counter.as_ref(),
        caps: caps.clone(),
        target: target.uid.to_string(),
    })
}
fn output_invalid(detail: &str) -> Error {
    Error::new(
        ErrorKind::EncoderOutputInvalid,
        format!("{ERR_OUTPUT_INVALID}: {detail}"),
    )
}
fn validate_delta(
    m: &Metadata,
    c: &EncoderCapabilities,
    mut encoded: Encoded,
) -> Result<Encoded, Error> {
    if !encoded.profile.is_empty() && !c.compatibility_profiles.contains(&encoded.profile) {
        return Err(Error::new(
            ErrorKind::EncoderOutputInvalid,
            ERR_OUTPUT_INVALID,
        ));
    }
    let photometric = normalized_photometric(&encoded.photometric);
    if !photometric.is_empty() {
        if photometric != normalized_photometric(&m.photometric_interpretation)
            && !c
                .output_photometric_interpretations
                .iter()
                .any(|p| normalized_photometric(p) == photometric)
        {
            return Err(output_invalid(
                "PhotometricInterpretation transform is undeclared",
            ));
        }
        match m.samples_per_pixel {
            1 => {
                if !matches!(
                    photometric.as_str(),
                    "MONOCHROME1" | "MONOCHROME2" | "PALETTE COLOR"
                ) {
                    return Err(output_invalid("PhotometricInterpretation"));
                }
            }
            3 => {
                if !matches!(
                    photometric.as_str(),
                    "RGB" | "YBR_FULL" | "YBR_FULL_422" | "YBR_RCT" | "YBR_ICT"
                ) {
                    return Err(output_invalid("PhotometricInterpretation"));
                }
            }
            _ => return Err(output_invalid("SamplesPerPixel")),
        }
        encoded.photometric = photometric;
    }
    if let Some(planar) = encoded.planar {
        if m.samples_per_pixel == 1 || planar > 1 {
            return Err(output_invalid("PlanarConfiguration"));
        }
        if !m.planar_configuration_present
            || planar != m.planar_configuration && !c.output_planar_configurations.contains(&planar)
        {
            return Err(output_invalid(
                "PlanarConfiguration transform is undeclared",
            ));
        }
    }
    Ok(encoded)
}
#[derive(Default)]
pub(super) struct EncodeState {
    pub(super) total: i64,
    pub(super) photometric: String,
    pub(super) planar: Option<u16>,
    pub(super) profile: String,
}
impl EncodeState {
    pub(super) fn encode(
        &mut self,
        r: &Resolved<'_>,
        m: &Metadata,
        max_output: i64,
        i: usize,
        frame: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let encoded = encode_frame(r, m, max_output, frame)?;
        self.accept(r, m, max_output, i, encoded)
    }
    pub(super) fn accept(
        &mut self,
        r: &Resolved<'_>,
        m: &Metadata,
        max_output: i64,
        i: usize,
        encoded: Encoded,
    ) -> Result<Vec<u8>, Error> {
        let encoded = validate_delta(m, &r.caps, encoded)?;
        if encoded.data.len() as i64 > max_output.saturating_sub(self.total) {
            return Err(Error::limit("output_bytes"));
        }
        self.total = self.total.saturating_add(encoded.data.len() as i64);
        if i == 0 {
            self.profile = encoded.profile.clone();
            self.photometric = encoded.photometric.clone();
            self.planar = encoded.planar;
        } else if self.photometric != encoded.photometric
            || self.planar != encoded.planar
            || self.profile != encoded.profile
        {
            return Err(output_invalid("inconsistent encoder metadata"));
        }
        Ok(encoded.data)
    }
}
pub(super) fn encode_frame(
    r: &Resolved<'_>,
    m: &Metadata,
    max_frame: i64,
    frame: &[u8],
) -> Result<Encoded, Error> {
    let started = std::time::Instant::now();
    let encoded = r.encoder.encode(frame, m);
    if let Some(counter) = r.counter {
        counter.add(started);
    }
    let encoded = encoded
        .and_then(|e| {
            if !e.profile.is_empty() && !r.caps.compatibility_profiles.contains(&e.profile) {
                Err(())
            } else {
                Ok(e)
            }
        })
        .map_err(|()| {
            Error::new(
                ErrorKind::EncoderFailed,
                format!(
                    "dicom: pixel data encoder failed for transfer syntax {}",
                    textfmt::quote(&r.target)
                ),
            )
        })?;
    if encoded.data.is_empty() {
        return Err(Error::new(
            ErrorKind::EncoderOutputInvalid,
            ERR_OUTPUT_INVALID,
        ));
    }
    let encoded = validate_delta(m, &r.caps, encoded)?;
    if encoded.data.len() as i64 > max_frame {
        return Err(Error::limit("encoded_frame_bytes"));
    }
    Ok(encoded)
}
pub(crate) fn transcode_with_encoder(
    dataset: &Object,
    m: &Metadata,
    target: &Syntax,
    options: &TranscodeOptions,
    limits: &TranscodeLimits,
    mut report: TranscodeReport,
) -> Result<(Object, TranscodeReport), Error> {
    let resolved = resolve(target, m, options)?;
    let frames = super::super::pixeldata::extract_native_frames(dataset)?;
    if super::output::jpeg2000_part2(&target.uid) {
        return transcode_collection(dataset, &frames, &resolved, target, options, limits, report);
    }
    let verify = !resolved.caps.lossless && options.max_pixel_error.is_some();
    if verify {
        super::verify::check_codec_availability(options.decoder, &target.uid)?;
    }
    let mut state = EncodeState::default();
    let mut fragments = Vec::with_capacity(frames.data.len());
    let mut verification = None;
    for (i, frame) in frames.data.iter().enumerate() {
        let encoded = state.encode(&resolved, m, limits.max_output_bytes, i, frame)?;
        if verify {
            let delta = (state.photometric.clone(), state.planar);
            let header = object_from(
                encoded_metadata_elements(dataset, Some(&delta)),
                dataset.value_big_endian(),
            );
            let measured = super::verify::verify_transcoded_frame(
                dataset, &header, &encoded, frame, target, options, limits, i, false,
            )?;
            super::verify::add_lossy(&mut verification, measured);
        }
        fragments.push(encoded);
    }
    if verification.is_some() {
        report.verification = super::Verification::PixelError;
    }
    report.lossy_verification = verification;
    let delta = (state.photometric.clone(), state.planar);
    let (draft, encoded_bytes) =
        rewrite_encoded(dataset, m, fragments, false, Some(&delta), limits)?;
    let mut result = clone_detached(&draft, &output_clone_limits(*limits))?;
    report.encoded_bytes = encoded_bytes;
    report.lossy = !resolved.caps.lossless;
    if !state.profile.is_empty() {
        report.encode_compatibility_profiles = vec![state.profile.clone()];
    }
    if report.lossy {
        apply_lossy_metadata(
            &mut result,
            super::super::pixeldata::total_size(m),
            encoded_bytes,
            &resolved.caps.lossy_method,
            &options.uid_root,
        )?;
    }
    Ok((result, report))
}
#[cfg(not(feature = "jpeg2000"))]
fn transcode_collection(
    _dataset: &Object,
    _frames: &NativeFrames,
    _resolved: &Resolved<'_>,
    _target: &Syntax,
    _options: &TranscodeOptions,
    _limits: &TranscodeLimits,
    _report: TranscodeReport,
) -> Result<(Object, TranscodeReport), Error> {
    Err(Error::unsupported(
        "JPEG 2000 Part 2 requires a collection encoder",
    ))
}
#[cfg(feature = "jpeg2000")]
fn transcode_collection(
    dataset: &Object,
    frames: &NativeFrames,
    resolved: &Resolved<'_>,
    target: &Syntax,
    options: &TranscodeOptions,
    limits: &TranscodeLimits,
    mut report: TranscodeReport,
) -> Result<(Object, TranscodeReport), Error> {
    let FrameEncoder::Jpeg2000Part2(encoder) = resolved.encoder else {
        return Err(Error::unsupported(
            "JPEG 2000 Part 2 requires a collection encoder",
        ));
    };
    if !resolved.caps.lossless && resolved.target == UID_JPEG2000_PART2_LOSSLESS {
        return Err(Error::new(
            ErrorKind::EncoderCapabilitiesInvalid,
            ERR_CAPABILITIES_INVALID,
        ));
    }
    let m = &frames.metadata;
    let n = frames.data.len() as i64;
    if !(2..=jpeg2000::j2kpart2::MAX_FRAMES).contains(&n)
        || i64::from(m.rows)
            .saturating_mul(i64::from(m.columns))
            .saturating_mul(n)
            > jpeg2000::j2kpart2::MAX_SAMPLES
    {
        return Err(Error::limit("collection_samples"));
    }
    let slices: Vec<&[u8]> = frames.data.iter().map(Vec::as_slice).collect();
    let started = std::time::Instant::now();
    let encoded = encoder.encode_frame_collection(&slices, &codec_metadata(m));
    if let Some(counter) = resolved.counter {
        counter.add(started);
    }
    let encoded = encoded.map_err(|_| {
        Error::new(
            ErrorKind::EncoderFailed,
            format!(
                "dicom: pixel data encoder failed for transfer syntax {}",
                textfmt::quote(&resolved.target)
            ),
        )
    })?;
    let padded = (encoded.len() as i64).saturating_add((encoded.len() & 1) as i64);
    if padded > limits.max_output_bytes {
        return Err(Error::limit("encoded_collection_bytes"));
    }
    let valid = jpeg2000::j2kpart2::inspect(&encoded).is_ok_and(|layout| {
        layout.profile.frames == m.number_of_frames
            && layout.rows == i64::from(m.rows)
            && layout.columns == i64::from(m.columns)
            && layout.profile.bits == i64::from(m.bits_stored)
            && layout.profile.signed == (m.pixel_representation == 1)
            && layout.profile.reversible == resolved.caps.lossless
    });
    if !valid {
        return Err(output_invalid("invalid Annex J collection"));
    }
    let (draft, encoded_bytes) = rewrite_encoded(dataset, m, vec![encoded], true, None, limits)?;
    let mut result = clone_detached(&draft, &output_clone_limits(*limits))?;
    report.encoded_bytes = encoded_bytes;
    report.lossy = !resolved.caps.lossless;
    if report.lossy
        && let Some(max_error) = options.max_pixel_error
    {
        report.lossy_verification = Some(verify_lossy_collection(
            frames, &result, target, options, limits, max_error,
        )?);
        report.verification = super::Verification::PixelError;
    }
    if report.lossy {
        apply_lossy_metadata(
            &mut result,
            super::super::pixeldata::total_size(m),
            encoded_bytes,
            &resolved.caps.lossy_method,
            &options.uid_root,
        )?;
    }
    Ok((result, report))
}
#[cfg(feature = "jpeg2000")]
fn verify_lossy_collection(
    source: &NativeFrames,
    output: &Object,
    target: &Syntax,
    options: &TranscodeOptions,
    limits: &TranscodeLimits,
    max_error: f64,
) -> Result<super::verify::LossyVerificationReport, Error> {
    super::verify::check_codec_availability(options.decoder, &target.uid)?;
    let (native, _) = super::decompress::decompress_for_transcode(
        output,
        target,
        options.decoder,
        super::decompress::DecompressLimits {
            max_frames: limits.max_frames,
            max_pixels: limits.max_pixels as i64,
            max_input_bytes: limits.max_output_bytes,
            max_native_bytes: limits.max_output_bytes,
            max_expansion_ratio: limits.max_expansion_ratio,
        },
    )?;
    let decoded = super::super::pixeldata::extract_native_frames(&native)?;
    if decoded.data.len() != source.data.len() {
        return Err(super::verify::comparison_unsupported());
    }
    let mut aggregate = None;
    for (i, (frame, decoded_frame)) in source.data.iter().zip(&decoded.data).enumerate() {
        let mut measured = super::verify::compare_lossy_frame(
            frame,
            &source.metadata,
            decoded_frame,
            &decoded.metadata,
            options.average_block_size,
        )?;
        measured.limit = max_error;
        if measured.maximum_error > measured.limit {
            return Err(super::verify::lossy_error(
                (i as i64).saturating_add(1),
                &measured,
            ));
        }
        super::verify::add_lossy(&mut aggregate, Some(measured));
    }
    aggregate.ok_or_else(super::verify::comparison_unsupported)
}
pub(crate) fn encapsulate_frames(
    decompressed: &Object,
    m: &Metadata,
    frames: Vec<Vec<u8>>,
    limits: &TranscodeLimits,
    mut report: TranscodeReport,
) -> Result<(Object, TranscodeReport), Error> {
    let (draft, encoded_bytes) = rewrite_encoded(decompressed, m, frames, false, None, limits)?;
    let result = clone_detached(&draft, &output_clone_limits(*limits))?;
    report.encoded_bytes = encoded_bytes;
    Ok((result, report))
}
pub(crate) fn encoded_metadata_elements(
    source: &Object,
    delta: Option<&(String, Option<u16>)>,
) -> Vec<Element> {
    let mut elements = source.elements().to_vec();
    remove_elements(
        &mut elements,
        &[
            TAG_EXTENDED_OFFSET_TABLE,
            TAG_EXTENDED_OFFSET_TABLE_LENGTHS,
            TAG_ENCAPSULATED_TOTAL_LENGTH,
        ],
    );
    if let Some((photometric, planar)) = delta {
        if !photometric.trim().is_empty() {
            replace_element(
                &mut elements,
                strings_element(TAG_PHOTOMETRIC, Vr::CS, vec![photometric.clone()]),
            );
        }
        if let Some(planar) = planar {
            replace_element(
                &mut elements,
                uint16_element(TAG_PLANAR_CONFIGURATION, *planar),
            );
        }
    }
    elements
}
pub(super) fn rewrite_encoded(
    source: &Object,
    m: &Metadata,
    fragments: Vec<Vec<u8>>,
    collection: bool,
    delta: Option<&(String, Option<u16>)>,
    limits: &TranscodeLimits,
) -> Result<(Object, i64), Error> {
    let n = fragments.len() as i64;
    let expected = if collection { 1 } else { m.number_of_frames };
    if n != expected || n > limits.max_frames || n > limits.max_fragments {
        return Err(Error::limit("fragments"));
    }
    let mut lengths = Vec::with_capacity(fragments.len());
    let mut total: u64 = 0;
    for f in &fragments {
        let len = f.len() as u64;
        total = total
            .checked_add(len)
            .ok_or_else(|| Error::limit("output_bytes"))?;
        if total > limits.max_output_bytes as u64 {
            return Err(Error::limit("output_bytes"));
        }
        lengths.push(len);
    }
    let (bot, eot) = if collection {
        (Vec::new(), None)
    } else {
        frame_offset_tables(&lengths)?
    };
    let mut elements = encoded_metadata_elements(source, delta);
    replace_element(&mut elements, fragments_element(bot, fragments));
    if let Some((eot, eot_lengths)) = eot {
        replace_element(
            &mut elements,
            raw_element(TAG_EXTENDED_OFFSET_TABLE, Vr::OV, eot),
        );
        replace_element(
            &mut elements,
            raw_element(TAG_EXTENDED_OFFSET_TABLE_LENGTHS, Vr::OV, eot_lengths),
        );
    }
    Ok((object_from(elements, false), total as i64))
}
type ExtendedTables = Option<(Vec<u8>, Vec<u8>)>;
fn frame_offset_tables(lengths: &[u64]) -> Result<(Vec<u8>, ExtendedTables), Error> {
    if lengths.is_empty() {
        return Ok((Vec::new(), None));
    }
    let mut offsets = Vec::with_capacity(lengths.len());
    let mut next: u64 = 0;
    let mut use_eot = false;
    for &len in lengths {
        offsets.push(next);
        if next > u64::from(u32::MAX) {
            use_eot = true;
        }
        let padded = len.saturating_add(len % 2);
        if next > u64::MAX.saturating_sub(8).saturating_sub(padded) {
            return Err(Error::limit("offset_table"));
        }
        next = next.saturating_add(padded.saturating_add(8));
    }
    if !use_eot {
        let bot = offsets
            .iter()
            .flat_map(|o| (*o as u32).to_le_bytes())
            .collect();
        return Ok((bot, None));
    }
    let eot = offsets.iter().flat_map(|o| o.to_le_bytes()).collect();
    let eot_lengths = lengths.iter().flat_map(|l| l.to_le_bytes()).collect();
    Ok((Vec::new(), Some((eot, eot_lengths))))
}
fn history(obj: &Object, tag: Tag, vr: Vr) -> Result<Vec<String>, Error> {
    let Some(e) = obj.get(tag) else {
        return Ok(Vec::new());
    };
    if e.vr() != vr {
        return Err(output_invalid("lossy history VR"));
    }
    Ok(derivation::string_values(e))
}
fn valid_lossy_ratio(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || value.len() > 16 || value.contains('\\') {
        return false;
    }
    let b = trimmed.as_bytes();
    let mut i = 0usize;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
        if i == b.len() {
            return false;
        }
    }
    let mut digits = 0usize;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i = i.saturating_add(1);
        digits = digits.saturating_add(1);
    }
    if b.get(i) == Some(&b'.') {
        i = i.saturating_add(1);
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
            digits = digits.saturating_add(1);
        }
    }
    if digits == 0 {
        return false;
    }
    if matches!(b.get(i), Some(b'E' | b'e')) {
        i = i.saturating_add(1);
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i = i.saturating_add(1);
        }
        let mut exp = 0usize;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i = i.saturating_add(1);
            exp = exp.saturating_add(1);
        }
        if exp == 0 {
            return false;
        }
    }
    if i != b.len() {
        return false;
    }
    trimmed
        .parse::<f64>()
        .is_ok_and(|r| r > 0.0 && r.is_finite())
}
pub(crate) fn apply_lossy_metadata(
    obj: &mut Object,
    native: i64,
    encoded: i64,
    method: &str,
    root: &str,
) -> Result<(), Error> {
    if native <= 0 || encoded <= 0 || method.trim().is_empty() {
        return Err(output_invalid("invalid lossy encoder result"));
    }
    let mut ratios = history(obj, TAG_LOSSY_RATIO, Vr::DS)?;
    let mut methods = history(obj, TAG_LOSSY_METHOD, Vr::CS)?;
    if ratios.len() != methods.len() {
        return Err(output_invalid("lossy history multiplicity"));
    }
    if !valid_lossy_method(method) {
        return Err(output_invalid("lossy method"));
    }
    if methods.iter().any(|m| !valid_lossy_method(m)) {
        return Err(output_invalid("lossy history method"));
    }
    if ratios.iter().any(|r| !valid_lossy_ratio(r)) {
        return Err(output_invalid("lossy history ratio"));
    }
    obj.put(strings_element(TAG_LOSSY, Vr::CS, vec!["01".into()]));
    let ratio = native as f64 / encoded as f64;
    ratios.push(crate::numtext::format_float(ratio, 'g', 8));
    obj.put(strings_element(TAG_LOSSY_RATIO, Vr::DS, ratios));
    methods.push(method.trim().to_string());
    obj.put(strings_element(TAG_LOSSY_METHOD, Vr::CS, methods));
    let mut image_types = obj
        .get(TAG_IMAGE_TYPE)
        .map(derivation::string_values)
        .unwrap_or_default();
    if image_types.is_empty() {
        image_types = vec!["DERIVED".into(), "PRIMARY".into()];
    } else if let Some(first) = image_types.first_mut() {
        *first = "DERIVED".into();
    }
    obj.put(strings_element(TAG_IMAGE_TYPE, Vr::CS, image_types));
    let mut step = Derivation {
        description: derivation::lossy_description(method.trim(), ratio),
        codes: vec![derivation::codes::LossyCompression()],
        sources: Vec::new(),
    };
    if let Ok(source) = derivation::source_of(
        Some(&*obj),
        Some(derivation::codes::UncompressedPredecessor()),
    ) {
        step.sources.push(source);
    }
    derivation::apply_to(Some(obj), &step)
        .map_err(|e| output_invalid(&format!("derivation: {e}")))?;
    let uid = new_uid(root).map_err(|_| {
        Error::stage(
            "uid",
            Error::new(ErrorKind::Transaction, super::ERR_TRANSACTION),
        )
    })?;
    obj.put(strings_element(TAG_SOP_INSTANCE_UID, Vr::UI, vec![uid]));
    Ok(())
}
pub(crate) fn validate_uid_root(root: &str) -> Result<(), Error> {
    if root.is_empty() {
        return Ok(());
    }
    if !crate::object::is_valid_uid(root.as_bytes())
        || root.len() > 64 - 1 - 20
        || root == "1.2.840.10008"
        || root.starts_with("1.2.840.10008.")
        || root == "2.25"
        || root.starts_with("2.25.")
    {
        return Err(Error::unsupported("dicom: invalid UID root"));
    }
    Ok(())
}
pub(crate) fn new_uid(root: &str) -> Result<String, Error> {
    validate_uid_root(root)?;
    if root.is_empty() {
        return Ok(crate::deid::uid::new_uid());
    }
    let digits = 63usize.saturating_sub(root.len()).min(39);
    let mut bytes = [0u8; 40];
    let id = uuid::Uuid::new_v4();
    let id2 = uuid::Uuid::new_v4();
    bytes[..16].copy_from_slice(id.as_bytes());
    bytes[16..32].copy_from_slice(id2.as_bytes());
    let mut out = String::with_capacity(root.len().saturating_add(1).saturating_add(digits));
    out.push_str(root);
    out.push('.');
    for i in 0..digits {
        let b = bytes
            .get(i % 32)
            .map_or(0, |&v| v as usize)
            .saturating_add(i);
        let digit = if i == 0 {
            (b % 9).saturating_add(1)
        } else {
            b % 10
        };
        out.push(char::from(b'0'.saturating_add(digit as u8)));
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offset_tables_basic() {
        let (bot, eot) = frame_offset_tables(&[3, 4]).unwrap_or_default();
        assert_eq!(bot, vec![0, 0, 0, 0, 12, 0, 0, 0]);
        assert!(eot.is_none());
    }
    #[test]
    fn lossy_ratio_rules() {
        assert!(valid_lossy_ratio("12.5"));
        assert!(valid_lossy_ratio("1E3"));
        assert!(!valid_lossy_ratio("0"));
        assert!(!valid_lossy_ratio("1e"));
        assert!(!valid_lossy_ratio("a"));
    }
    #[test]
    fn rooted_uid_length() {
        let uid = new_uid("1.2.3").unwrap_or_default();
        assert_eq!(uid.len(), 6 + 39);
        assert!(validate_uid_root("2.25.1").is_err());
    }
}
