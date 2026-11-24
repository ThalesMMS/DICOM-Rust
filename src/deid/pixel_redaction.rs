use super::PixelRegion;
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelRedactionAttestation {
    pub regions_digest: String,
}
pub fn attest_pixel_redaction(regions: &[PixelRegion], fill: i64) -> PixelRedactionAttestation {
    PixelRedactionAttestation {
        regions_digest: pixel_redaction_digest(regions, fill),
    }
}
pub fn pixel_redaction_digest(regions: &[PixelRegion], fill: i64) -> String {
    let mut sorted = regions.to_vec();
    sorted.sort_by(|a, b| {
        for d in [
            a.frame.wrapping_sub(b.frame),
            a.y.wrapping_sub(b.y),
            a.x.wrapping_sub(b.x),
            a.height.wrapping_sub(b.height),
            a.width.wrapping_sub(b.width),
        ] {
            if d != 0 {
                return d.cmp(&0);
            }
        }
        std::cmp::Ordering::Equal
    });
    let mut digest = Sha256::new();
    digest.update(b"dcm-rust/deid pixel redaction v1");
    let mut put = |value: i64| digest.update((value as u64).to_be_bytes());
    put(fill);
    put(sorted.len() as i64);
    for r in &sorted {
        put(r.frame);
        put(r.x);
        put(r.y);
        put(r.width);
        put(r.height);
    }
    hex_lower(&digest.finalize())
}
pub const ERR_UNSUPPORTED_PIXEL_REDACTION: &str = "deid: unsupported pixel redaction";
fn unsupported(detail: impl std::fmt::Display) -> super::Error {
    super::Error::new(super::Kind::UnsupportedPixelRedaction, detail)
}
fn region_repr(r: &PixelRegion) -> String {
    format!(
        "PixelRegion {{ frame: {}, x: {}, y: {}, width: {}, height: {} }}",
        r.frame, r.x, r.y, r.width, r.height
    )
}
pub fn mask_pixel_regions(
    obj: &mut crate::object::Object,
    regions: &[PixelRegion],
    fill: &[u8],
) -> Result<Vec<PixelRegion>, super::Error> {
    use crate::object::{Element, ElementHeader, Tag, Value};
    let native = crate::pixel::pixeldata::extract_native_frames(obj).map_err(unsupported)?;
    let Some(elem) = obj.get(Tag::PIXEL_DATA) else {
        return Err(unsupported("missing pixel data"));
    };
    let vr = elem.vr();
    let m = &native.metadata;
    if m.planar_configuration_present && m.planar_configuration > 1 {
        return Err(unsupported(format!(
            "PlanarConfiguration={}",
            m.planar_configuration
        )));
    }
    let photometric = crate::textfmt::to_upper(m.photometric_interpretation.trim());
    if matches!(
        photometric.as_str(),
        "YBR_FULL_422" | "YBR_PARTIAL_422" | "YBR_PARTIAL_420"
    ) {
        return Err(unsupported(format!(
            "subsampled PhotometricInterpretation={photometric} is not supported"
        )));
    }
    if m.bits_allocated < 8 {
        return Err(unsupported(format!(
            "BitsAllocated={} packed pixels are not supported",
            m.bits_allocated
        )));
    }
    let rows = i64::from(m.rows);
    let columns = i64::from(m.columns);
    let samples = usize::from(m.samples_per_pixel);
    let bytes_per_sample = usize::from(m.bits_allocated).div_ceil(8);
    let bytes_per_pixel = samples * bytes_per_sample;
    if bytes_per_pixel == 0 {
        return Err(unsupported("invalid pixel format"));
    }
    let fill_pixel = if fill.is_empty() {
        vec![0u8; bytes_per_pixel]
    } else {
        fill.to_vec()
    };
    if fill_pixel.len() != bytes_per_pixel {
        return Err(unsupported(format!(
            "fill has {} byte(s), want {bytes_per_pixel}",
            fill_pixel.len()
        )));
    }
    let row_stride = columns as usize * bytes_per_pixel;
    let plane_stride = (rows * columns) as usize * bytes_per_sample;
    let planar = m.planar_configuration_present && m.planar_configuration == 1;
    let mut frames = native.data.clone();
    let mut applied = Vec::with_capacity(regions.len());
    for r in regions {
        if r.width <= 0 || r.height <= 0 {
            return Err(unsupported(format!("invalid region {}", region_repr(r))));
        }
        if r.frame < 0 || r.frame >= frames.len() as i64 {
            return Err(unsupported(format!(
                "frame {} outside {} frame(s)",
                r.frame,
                frames.len()
            )));
        }
        if r.x < 0
            || r.y < 0
            || r.x >= columns
            || r.y >= rows
            || r.width > columns - r.x
            || r.height > rows - r.y
        {
            return Err(unsupported(format!(
                "region {} outside {columns}x{rows} pixels",
                region_repr(r)
            )));
        }
        let frame = &mut frames[r.frame as usize];
        for y in r.y as usize..(r.y + r.height) as usize {
            for x in r.x as usize..(r.x + r.width) as usize {
                if planar {
                    let pixel = y * columns as usize + x;
                    for s in 0..samples {
                        let offset = s * plane_stride + pixel * bytes_per_sample;
                        let f = s * bytes_per_sample;
                        frame[offset..offset + bytes_per_sample]
                            .copy_from_slice(&fill_pixel[f..f + bytes_per_sample]);
                    }
                    continue;
                }
                let offset = y * row_stride + x * bytes_per_pixel;
                frame[offset..offset + bytes_per_pixel].copy_from_slice(&fill_pixel);
            }
        }
        applied.push(*r);
    }
    let masked = frames.concat();
    obj.put(Element {
        header: ElementHeader {
            tag: Tag::PIXEL_DATA,
            vr,
            length: masked.len() as u32,
            length_set: true,
        },
        value: Value::Raw(masked),
    });
    Ok(applied)
}
fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
#[derive(Clone, Default)]
pub struct PixelRedaction {
    pub regions: Vec<PixelRegion>,
    pub fill: i64,
    pub attestation: Option<PixelRedactionAttestation>,
    pub decoder: Option<crate::pixel::transcode::DecoderRegistry>,
    pub encoders: Option<crate::pixel::transcode::EncoderRegistry>,
    pub output: crate::pixel::redact::RedactionOutput,
    pub limits: crate::pixel::transcode::TranscodeLimits,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PixelRedactionReport {
    pub frames: i64,
    pub frames_covered: i64,
    pub regions: i64,
    pub pixels: i64,
    pub attested: bool,
    pub annotation_cleared: bool,
    pub applied: bool,
    pub source_transfer_syntax_uid: String,
    pub output_transfer_syntax_uid: String,
    pub reencoded: bool,
    pub decoded_to_native: bool,
}
impl PixelRedactionReport {
    pub fn json(&self) -> String {
        let mut f = vec![
            format!("\"frames\":{}", self.frames),
            format!("\"frames_covered\":{}", self.frames_covered),
            format!("\"regions\":{}", self.regions),
        ];
        if self.pixels != 0 {
            f.push(format!("\"pixels\":{}", self.pixels));
        }
        f.push(format!("\"attested\":{}", self.attested));
        f.push(format!(
            "\"annotation_cleared\":{}",
            self.annotation_cleared
        ));
        f.push(format!("\"applied\":{}", self.applied));
        if !self.source_transfer_syntax_uid.is_empty() {
            f.push(format!(
                "\"source_transfer_syntax_uid\":{}",
                super::json_string(&self.source_transfer_syntax_uid)
            ));
        }
        if !self.output_transfer_syntax_uid.is_empty() {
            f.push(format!(
                "\"output_transfer_syntax_uid\":{}",
                super::json_string(&self.output_transfer_syntax_uid)
            ));
        }
        if self.reencoded {
            f.push("\"reencoded\":true".into());
        }
        if self.decoded_to_native {
            f.push("\"decoded_to_native\":true".into());
        }
        format!("{{{}}}", f.join(","))
    }
    pub(crate) fn record(&mut self, outcome: &crate::pixel::redact::RedactionReport) {
        self.applied = true;
        self.pixels = outcome.pixels_redacted;
        self.source_transfer_syntax_uid = outcome.source_transfer_syntax_uid.clone();
        self.output_transfer_syntax_uid = outcome.output_transfer_syntax_uid.clone();
        self.reencoded = outcome.reencoded;
        self.decoded_to_native = outcome.decoded_to_native;
    }
}
impl PixelRedaction {
    pub(crate) fn redaction_regions(&self) -> Vec<crate::pixel::redact::RedactionRegion> {
        self.regions
            .iter()
            .map(|r| crate::pixel::redact::RedactionRegion {
                frame: r.frame,
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            })
            .collect()
    }
    pub(crate) fn pixeldata_options(
        &self,
        max_regions: i64,
    ) -> crate::pixel::redact::RedactionOptions {
        crate::pixel::redact::RedactionOptions {
            decoder: self.decoder,
            encoders: self.encoders.clone(),
            fill: self.fill,
            output: self.output,
            limits: self.limits,
            max_regions,
        }
    }
}
pub(crate) fn validate_pixel_redaction_options(
    redaction: Option<&PixelRedaction>,
    max_pixel_masks: i64,
) -> Result<(), super::Error> {
    use super::{Error, Kind};
    let Some(r) = redaction else {
        return Ok(());
    };
    if r.regions.is_empty() {
        return Err(Error::bare(Kind::InvalidPixelRedaction));
    }
    if r.regions.len() as i64 > max_pixel_masks {
        return Err(Error::bare(Kind::ProfileResourceLimit));
    }
    if r.output.0 > crate::pixel::redact::RedactionOutput::NATIVE.0 {
        return Err(Error::bare(Kind::InvalidPixelRedaction));
    }
    if let Some(a) = &r.attestation
        && a.regions_digest != pixel_redaction_digest(&r.regions, r.fill)
    {
        return Err(Error::bare(Kind::StalePixelRedactionAttestation));
    }
    Ok(())
}
pub(crate) fn plan_pixel_redaction(
    redaction: &PixelRedaction,
    source: &crate::object::Object,
    max_pixel_masks: i64,
) -> Result<PixelRedactionReport, super::Error> {
    use super::{Error, Kind};
    let metadata = crate::pixel::pixeldata::extract_metadata(source).map_err(|_| {
        Error::new(
            Kind::InvalidPixelRedaction,
            "image pixel description is incomplete",
        )
    })?;
    let coverage = crate::pixel::redact::validate_redaction_regions(
        &metadata,
        &redaction.redaction_regions(),
        max_pixel_masks,
    )
    .map_err(|e| Error::new(Kind::InvalidPixelRedaction, e))?;
    let attested = redaction.attestation.is_some();
    Ok(PixelRedactionReport {
        frames: coverage.frames,
        frames_covered: coverage.frames_covered,
        regions: coverage.regions,
        attested,
        annotation_cleared: attested && coverage.frames_covered == coverage.frames,
        ..Default::default()
    })
}
pub(crate) fn redact_planned(
    redaction: &PixelRedaction,
    planned: &mut crate::object::Object,
    max_pixel_masks: i64,
) -> Result<Option<crate::pixel::redact::RedactionReport>, super::Error> {
    use super::{Error, Kind};
    use crate::object::{Tag, Value};
    let Some(element) = planned.get(Tag::PIXEL_DATA) else {
        return Err(Error::new(Kind::InvalidPixelRedaction, "no Pixel Data"));
    };
    match &element.value {
        Value::Nil => return Ok(None),
        Value::Raw(_) if !planned.value_big_endian() => {}
        _ => return Err(Error::bare(Kind::PixelRedactionRequiresFile)),
    }
    let (redacted, _, outcome) = crate::pixel::redact::redact_data_set(
        planned,
        crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN.uid,
        &redaction.redaction_regions(),
        &redaction.pixeldata_options(max_pixel_masks),
    )
    .map_err(|e| Error::new(Kind::InvalidPixelRedaction, e))?;
    if let Some(pixels) = redacted.get(Tag::PIXEL_DATA) {
        planned.put(pixels.clone());
    }
    Ok(Some(outcome))
}
pub(crate) fn redact_file_for_profile(
    file: &mut crate::object::File,
    redaction: &PixelRedaction,
    max_pixel_masks: i64,
) -> Result<crate::pixel::redact::RedactionReport, super::Error> {
    use super::{Error, Kind};
    use crate::object::transfer;
    let mut uid = transfer::normalize_uid(file.transfer_syntax.uid).to_string();
    if uid.is_empty()
        && let Some(value) = file
            .meta
            .get_string(crate::object::Tag::new(0x0002, 0x0010))
    {
        uid = transfer::normalize_uid(&value).to_string();
    }
    if uid.is_empty() {
        return Err(Error::with_text(
            Kind::Object,
            "dicom: missing TransferSyntaxUID in file meta information",
        ));
    }
    let Some(syntax) = transfer::lookup(&uid) else {
        return Err(Error::with_text(
            Kind::Object,
            "dicom: unknown transfer syntax",
        ));
    };
    let (redacted, output, outcome) = crate::pixel::redact::redact_data_set(
        &file.dataset,
        syntax.uid,
        &redaction.redaction_regions(),
        &redaction.pixeldata_options(max_pixel_masks),
    )
    .map_err(|e| {
        if e.kind == crate::pixel::redact::RedactErrorKind::Invalid {
            Error::new(Kind::InvalidPixelRedaction, e)
        } else {
            Error::new(Kind::PixelRedactionFailed, e)
        }
    })?;
    file.dataset = redacted;
    if let Some(s) = transfer::lookup(&output) {
        file.transfer_syntax = s;
    }
    Ok(outcome)
}
