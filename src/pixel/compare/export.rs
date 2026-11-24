use super::images::PresentedImagePolicy;
use super::presented::{
    ComparisonRegion, PresentedOptions, PresentedResult, PresentedStatistics, Rect, finite,
    presented_budget,
};
use super::readers::{
    PresentationProfile, PresentedFrameRecord, clone_palette, clone_presentation_lut, clone_profile,
};
use super::{Error, ctx_err};
use crate::dicomjson::jsonwire::{JVal, write_compact};
use crate::display::{Lut, ModalityLut, PaletteColorLut, VoiFunction};
use crate::fmath;
use crate::net::ul::Ctx;
use crate::numtext::format_float;
use crate::pixel::Metadata;
use crate::render::png_encode::encode_nrgba64_no_compression;
use crate::render::qualification::RgbaFrameErrorMetricsV1;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
pub struct PresentedWriters<'a> {
    pub difference: &'a mut dyn Write,
    pub ssim: &'a mut dyn Write,
    pub validity: &'a mut dyn Write,
    pub difference_png: &'a mut dyn Write,
    pub ssim_png: &'a mut dyn Write,
    pub metadata: &'a mut dyn Write,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresentedArtifact {
    pub bytes: i64,
    pub sha256: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportedNumerical {
    pub samples: i64,
    pub max_absolute_error: f64,
    pub mean_absolute_error: f64,
    pub root_mean_square_error: f64,
    pub psnr: String,
    pub snr: String,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportedStatistics {
    pub name: String,
    pub bounds: Rect,
    pub numerical: ExportedNumerical,
    pub ssim_samples: i64,
    pub mean_ssim: f64,
    pub minimum_ssim: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentedExportMetadata {
    pub schema: String,
    pub width: i64,
    pub height: i64,
    pub components: i64,
    pub depth: i64,
    pub encoding: String,
    pub options: PresentedOptions,
    pub raw_rgba: Option<RgbaFrameErrorMetricsV1>,
    pub image_policy: Option<PresentedImagePolicy>,
    pub float_format: String,
    pub validity_format: String,
    pub difference_visual: String,
    pub ssim_visual: String,
    pub summary: ExportedStatistics,
    pub regions: Vec<ExportedStatistics>,
    pub frame: Option<Box<PresentedFrameRecord>>,
    pub artifacts: BTreeMap<String, PresentedArtifact>,
}
fn gfloat(f: f64) -> String {
    format_float(f, 'g', -1)
}
fn export_statistics(s: &PresentedStatistics) -> ExportedStatistics {
    ExportedStatistics {
        name: s.name.clone(),
        bounds: s.bounds,
        numerical: ExportedNumerical {
            samples: s.numerical.samples,
            max_absolute_error: s.numerical.max_absolute_error,
            mean_absolute_error: s.numerical.mean_absolute_error,
            root_mean_square_error: s.numerical.root_mean_square_error,
            psnr: gfloat(s.numerical.peak_signal_to_noise_ratio),
            snr: gfloat(s.numerical.signal_to_noise_ratio),
        },
        ssim_samples: s.ssim_samples,
        mean_ssim: s.mean_ssim,
        minimum_ssim: s.minimum_ssim,
    }
}
struct ArtifactWriter<'a, 'b> {
    ctx: &'a Ctx,
    remaining: &'a mut i64,
    dst: &'a mut (dyn Write + 'b),
    hash: Sha256,
    written: i64,
}
impl<'a, 'b> ArtifactWriter<'a, 'b> {
    fn new(
        ctx: &'a Ctx,
        remaining: &'a mut i64,
        dst: &'a mut (dyn Write + 'b),
    ) -> ArtifactWriter<'a, 'b> {
        ArtifactWriter {
            ctx,
            remaining,
            dst,
            hash: Sha256::new(),
            written: 0,
        }
    }
    fn write(&mut self, p: &[u8]) -> Result<(), Error> {
        ctx_err(self.ctx)?;
        if p.len() as i64 > *self.remaining {
            return Err(Error::Limit);
        }
        self.dst
            .write_all(p)
            .map_err(|e| Error::Other(e.to_string()))?;
        self.hash.update(p);
        self.written = self.written.saturating_add(p.len() as i64);
        *self.remaining = (*self.remaining).saturating_sub(p.len() as i64);
        Ok(())
    }
    fn artifact(self) -> PresentedArtifact {
        PresentedArtifact {
            bytes: self.written,
            sha256: self
                .hash
                .finalize()
                .iter()
                .fold(String::new(), |mut out, b| {
                    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{b:02x}"));
                    out
                }),
        }
    }
}
fn check_frame(frame: &PresentedFrameRecord, r: &PresentedResult) -> Result<(), Error> {
    if !std::ptr::eq(&frame.result, r)
        || (frame.mode != "common" && frame.mode != "per-image")
        || frame.reference_metadata.photometric_interpretation.len() > 64
        || frame.candidate_metadata.photometric_interpretation.len() > 64
    {
        return Err(Error::Mismatch(None));
    }
    let gray = r.components == 1;
    clone_profile(&frame.reference_profile, gray)?;
    clone_profile(&frame.candidate_profile, gray)?;
    if gray && (frame.reference_palette.is_some() || frame.candidate_palette.is_some()) {
        return Err(Error::Mismatch(None));
    }
    let modality_set = |m: &ModalityLut| m.lut.is_some() || m.slope != 0.0 || m.intercept != 0.0;
    if r.components == 3
        && (modality_set(&frame.reference_modality) || modality_set(&frame.candidate_modality))
    {
        return Err(Error::Mismatch(None));
    }
    clone_palette(frame.reference_palette.as_ref())?;
    clone_palette(frame.candidate_palette.as_ref())?;
    for m in [&frame.reference_modality, &frame.candidate_modality] {
        if !finite(m.slope) || !finite(m.intercept) {
            return Err(Error::Mismatch(None));
        }
        clone_presentation_lut(m.lut.as_ref())?;
    }
    Ok(())
}
fn visual(v: f64, ssim: bool, signed: bool, scale: f64) -> u16 {
    let v = if ssim {
        (v + 1.0) / 2.0
    } else if signed {
        (v / scale + 1.0) / 2.0
    } else {
        v / scale
    };
    let v = fmath::max(0.0, fmath::min(1.0, v));
    fmath::f64_to_i64((v * 65535.0).round()) as u16
}
pub fn export_presented(
    ctx: &Ctx,
    r: &PresentedResult,
    frame: Option<&PresentedFrameRecord>,
    w: &mut PresentedWriters<'_>,
    max_output_bytes: i64,
) -> Result<PresentedExportMetadata, Error> {
    ctx_err(ctx)?;
    if max_output_bytes <= 0 {
        return Err(Error::Configuration(None));
    }
    if r.depth != 8 && r.depth != 16 {
        return Err(Error::Mismatch(None));
    }
    if (r.components == 1 && r.encoding != "gray-pvalue")
        || (r.components == 3 && r.encoding != "srgb-code" && r.encoding != "linear-rgb")
    {
        return Err(Error::Mismatch(None));
    }
    let bytes = presented_budget(r.width, r.height, r.components, &r.options, 80)?;
    if bytes > r.options.max_working_bytes.saturating_sub(8 << 20) {
        return Err(Error::Limit);
    }
    if r.logical_working_bytes < 0 {
        return Err(Error::Configuration(None));
    }
    let pixels = r.width.saturating_mul(r.height);
    let base = presented_budget(r.width, r.height, r.components, &r.options, 0)?;
    let occupied = r.logical_working_bytes.max(base);
    let extra = pixels.saturating_mul(80).saturating_add(8 << 20);
    if occupied > r.options.max_working_bytes.saturating_sub(extra) {
        return Err(Error::Limit);
    }
    let n = pixels as usize;
    let c = r.components as usize;
    let nc = n.saturating_mul(c);
    if r.difference.len() != nc || r.ssim.len() != nc || r.valid.len() != n {
        return Err(Error::Mismatch(None));
    }
    for (i, (&d, &s)) in r.difference.iter().zip(&r.ssim).enumerate() {
        if i % 4096 == 0 {
            ctx_err(ctx)?;
        }
        if !finite(d) || !finite(s) || d.abs() > 1.0 || !(-1.0..=1.0).contains(&s) {
            return Err(Error::Mismatch(None));
        }
    }
    if r.regions.len() > 256
        || r.summary.name.len() > 256
        || r.regions.iter().any(|region| region.name.len() > 256)
    {
        return Err(Error::Configuration(None));
    }
    if let Some(frame) = frame {
        check_frame(frame, r)?;
    }
    let mut metadata = PresentedExportMetadata {
        schema: "presented-artifacts-v1".into(),
        width: r.width,
        height: r.height,
        components: r.components,
        depth: r.depth,
        encoding: r.encoding.clone(),
        options: r.options.clone(),
        raw_rgba: r.raw_rgba.clone(),
        image_policy: r.image_policy.clone(),
        float_format: "IEEE754-float64-little-endian-row-major-interleaved".into(),
        validity_format: "uint8: bit0=sample-included bit1=ssim-centre-valid".into(),
        difference_visual: format!(
            "{}-fixed-scale-{}",
            r.options.difference,
            gfloat(r.options.scale)
        ),
        ssim_visual: "fixed [-1,1]".into(),
        summary: export_statistics(&r.summary),
        regions: r.regions.iter().map(export_statistics).collect(),
        frame: frame.map(|f| {
            Box::new(PresentedFrameRecord {
                result: PresentedResult::default(),
                ..f.clone()
            })
        }),
        artifacts: BTreeMap::new(),
    };
    let mut remaining = max_output_bytes;
    let mask = &r.options.mask;
    let included = |p: usize| mask.is_empty() || mask.get(p).copied().unwrap_or(false);
    for (name, values, dst) in [
        ("difference.f64le", &r.difference, &mut *w.difference),
        ("ssim.f64le", &r.ssim, &mut *w.ssim),
    ] {
        let mut out = ArtifactWriter::new(ctx, &mut remaining, dst);
        for v in values {
            out.write(&v.to_bits().to_le_bytes())?;
        }
        metadata.artifacts.insert(name.into(), out.artifact());
    }
    let mut out = ArtifactWriter::new(ctx, &mut remaining, &mut *w.validity);
    for (p, &valid) in r.valid.iter().enumerate() {
        let mut v = u8::from(included(p));
        if valid {
            v |= 2;
        }
        out.write(&[v])?;
    }
    metadata
        .artifacts
        .insert("validity.u8".into(), out.artifact());
    let signed = r.options.difference == "signed";
    let width = r.width as usize;
    for (name, values, ssim, dst) in [
        (
            "difference.png",
            &r.difference,
            false,
            &mut *w.difference_png,
        ),
        ("ssim.png", &r.ssim, true, &mut *w.ssim_png),
    ] {
        let mut pix = vec![0u16; n.saturating_mul(4)];
        let mut pixels = pix.as_chunks_mut::<4>().0.iter_mut();
        for y in 0..r.height as usize {
            ctx_err(ctx)?;
            for x in 0..width {
                let Some(out) = pixels.next() else {
                    break;
                };
                let p = y.saturating_mul(width).saturating_add(x);
                let shown = if ssim {
                    r.valid.get(p).copied().unwrap_or(false)
                } else {
                    included(p)
                };
                if !shown {
                    continue;
                }
                let mut channels = [0u16; 3];
                let first = p.saturating_mul(c);
                for (slot, &v) in channels.iter_mut().zip(values.iter().skip(first).take(c)) {
                    *slot = visual(v, ssim, signed, r.options.scale);
                }
                if c == 1 {
                    channels = [channels[0]; 3];
                }
                let [red, green, blue] = channels;
                *out = [red, green, blue, 0xffff];
            }
        }
        let png =
            encode_nrgba64_no_compression(&pix, width, r.height as usize).map_err(Error::Other)?;
        let mut out = ArtifactWriter::new(ctx, &mut remaining, dst);
        write_png(&mut out, &png)?;
        metadata.artifacts.insert(name.into(), out.artifact());
    }
    let mut json = metadata.to_json()?;
    json.push('\n');
    let mut out = ArtifactWriter::new(ctx, &mut remaining, &mut *w.metadata);
    out.write(json.as_bytes())?;
    ctx_err(ctx)?;
    Ok(metadata)
}
fn write_png(out: &mut ArtifactWriter<'_, '_>, png: &[u8]) -> Result<(), Error> {
    let Some((signature, mut rest)) = png.split_at_checked(8) else {
        return Ok(());
    };
    out.write(signature)?;
    while let Some((head, body)) = rest.split_first_chunk::<8>() {
        let [l0, l1, l2, l3, ..] = *head;
        let len = u32::from_be_bytes([l0, l1, l2, l3]) as usize;
        let Some((data, after)) = body.split_at_checked(len) else {
            break;
        };
        let Some((crc, after)) = after.split_at_checked(4) else {
            break;
        };
        out.write(head)?;
        out.write(data)?;
        out.write(crc)?;
        rest = after;
    }
    Ok(())
}
fn obj(fields: Vec<(&str, JVal)>) -> JVal {
    JVal::Obj(
        fields
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}
fn s(v: &str) -> JVal {
    JVal::Str(v.to_string())
}
fn rect(r: &Rect) -> JVal {
    let point = |x: i64, y: i64| obj(vec![("X", JVal::I64(x)), ("Y", JVal::I64(y))]);
    obj(vec![("Min", point(r.x0, r.y0)), ("Max", point(r.x1, r.y1))])
}
fn floats(values: &[f64]) -> JVal {
    JVal::Arr(values.iter().map(|v| JVal::F64(*v)).collect())
}
fn options(o: &PresentedOptions) -> JVal {
    let ssim = &o.ssim;
    let regions = if o.regions.is_empty() {
        JVal::Null
    } else {
        JVal::Arr(
            o.regions
                .iter()
                .map(|ComparisonRegion { name, bounds }| {
                    obj(vec![("name", s(name)), ("bounds", rect(bounds))])
                })
                .collect(),
        )
    };
    obj(vec![
        (
            "ssim",
            obj(vec![
                ("window", JVal::I64(ssim.window)),
                ("kernel", s(&ssim.kernel)),
                ("sigma", JVal::F64(ssim.sigma)),
                ("K1", JVal::F64(ssim.k1)),
                ("K2", JVal::F64(ssim.k2)),
                ("range", JVal::F64(ssim.range)),
                ("border", s(&ssim.border)),
            ]),
        ),
        ("difference", s(&o.difference)),
        ("scale", JVal::F64(o.scale)),
        ("max_pixels", JVal::I64(o.max_pixels)),
        ("max_working_bytes", JVal::I64(o.max_working_bytes)),
        ("regions", regions),
    ])
}
fn raw_rgba(m: &RgbaFrameErrorMetricsV1) -> JVal {
    let bytes = |v: &[u8; 4]| JVal::Arr(v.iter().map(|b| JVal::U64(u64::from(*b))).collect());
    obj(vec![
        ("schema", s(&m.schema)),
        ("width", JVal::U64(u64::from(m.width))),
        ("height", JVal::U64(u64::from(m.height))),
        ("pixel_count", JVal::U64(m.pixel_count)),
        (
            "thresholds",
            obj(vec![
                (
                    "absolute_component",
                    bytes(&m.thresholds.absolute_component),
                ),
                (
                    "alpha_silhouette_cutoff",
                    JVal::U64(u64::from(m.thresholds.alpha_silhouette_cutoff)),
                ),
            ]),
        ),
        ("mean_absolute_error", floats(&m.mean_absolute_error)),
        ("root_mean_square_error", floats(&m.root_mean_square_error)),
        ("p99_absolute_error", floats(&m.p99_absolute_error)),
        ("maximum_absolute_error", bytes(&m.maximum_absolute_error)),
        (
            "components_over_threshold",
            JVal::Arr(
                m.components_over_threshold
                    .iter()
                    .map(|v| JVal::U64(*v))
                    .collect(),
            ),
        ),
        (
            "pixels_with_any_component_over_limit",
            JVal::U64(m.pixels_with_any_component_over_limit),
        ),
        (
            "alpha_silhouette_mismatch_pixels",
            JVal::U64(m.alpha_silhouette_mismatch_pixels),
        ),
    ])
}
fn image_policy(p: &PresentedImagePolicy) -> JVal {
    obj(vec![
        ("depth", JVal::I64(p.depth)),
        ("encoding", s(&p.encoding)),
        ("alpha", s(&p.alpha)),
        ("background", floats(&p.background)),
    ])
}
fn statistics(st: &ExportedStatistics) -> JVal {
    let n = &st.numerical;
    obj(vec![
        ("Name", s(&st.name)),
        ("Bounds", rect(&st.bounds)),
        (
            "Numerical",
            obj(vec![
                ("samples", JVal::I64(n.samples)),
                ("MaxAbsoluteError", JVal::F64(n.max_absolute_error)),
                ("MeanAbsoluteError", JVal::F64(n.mean_absolute_error)),
                ("RootMeanSquareError", JVal::F64(n.root_mean_square_error)),
                ("PSNR", s(&n.psnr)),
                ("SNR", s(&n.snr)),
            ]),
        ),
        ("SSIMSamples", JVal::I64(st.ssim_samples)),
        ("MeanSSIM", JVal::F64(st.mean_ssim)),
        ("MinimumSSIM", JVal::F64(st.minimum_ssim)),
    ])
}
fn metadata_json(m: &Metadata) -> JVal {
    obj(vec![
        ("Rows", JVal::U64(u64::from(m.rows))),
        ("Columns", JVal::U64(u64::from(m.columns))),
        ("SamplesPerPixel", JVal::U64(u64::from(m.samples_per_pixel))),
        ("BitsAllocated", JVal::U64(u64::from(m.bits_allocated))),
        ("BitsStored", JVal::U64(u64::from(m.bits_stored))),
        ("HighBit", JVal::U64(u64::from(m.high_bit))),
        (
            "PixelRepresentation",
            JVal::U64(u64::from(m.pixel_representation)),
        ),
        (
            "PlanarConfiguration",
            JVal::U64(u64::from(m.planar_configuration)),
        ),
        (
            "PlanarConfigurationPresent",
            JVal::Bool(m.planar_configuration_present),
        ),
        ("NumberOfFrames", JVal::I64(m.number_of_frames)),
        (
            "PhotometricInterpretation",
            s(&m.photometric_interpretation),
        ),
    ])
}
fn lut(l: Option<&Lut>) -> JVal {
    let Some(l) = l else {
        return JVal::Null;
    };
    let adjustments = if l.adjustments.is_empty() {
        JVal::Null
    } else {
        JVal::Arr(l.adjustments.iter().map(|a| s(a.description())).collect())
    };
    obj(vec![
        ("FirstMapped", JVal::I64(l.first_mapped)),
        ("BitsPerEntry", JVal::I64(l.bits_per_entry)),
        (
            "Entries",
            JVal::Arr(l.entries.iter().map(|e| JVal::U64(u64::from(*e))).collect()),
        ),
        ("Adjustments", adjustments),
    ])
}
fn voi_function(f: VoiFunction) -> i64 {
    match f {
        VoiFunction::Linear => 0,
        VoiFunction::LinearExact => 1,
        VoiFunction::Sigmoid => 2,
    }
}
fn profile(p: &PresentationProfile) -> JVal {
    let mut fields = vec![(
        "voi",
        obj(vec![
            ("Center", JVal::F64(p.voi.center)),
            ("Width", JVal::F64(p.voi.width)),
            ("Function", JVal::I64(voi_function(p.voi.function))),
            ("LUT", lut(p.voi.lut.as_ref())),
            ("Explanation", s(&p.voi.explanation)),
        ]),
    )];
    if p.lut.is_some() {
        fields.push(("presentation_lut", lut(p.lut.as_ref())));
    }
    fields.push(("invert", JVal::Bool(p.invert)));
    obj(fields)
}
fn modality(m: &ModalityLut) -> JVal {
    obj(vec![
        ("Slope", JVal::F64(m.slope)),
        ("Intercept", JVal::F64(m.intercept)),
        ("LUT", lut(m.lut.as_ref())),
    ])
}
fn palette(p: Option<&PaletteColorLut>) -> JVal {
    match p {
        None => JVal::Null,
        Some(p) => obj(vec![
            ("Red", lut(p.red.as_ref())),
            ("Green", lut(p.green.as_ref())),
            ("Blue", lut(p.blue.as_ref())),
        ]),
    }
}
fn frame_record(f: &PresentedFrameRecord) -> JVal {
    let meta = metadata_json;
    obj(vec![
        ("index", JVal::I64(f.index)),
        ("mode", s(&f.mode)),
        ("ReferenceMetadata", meta(&f.reference_metadata)),
        ("CandidateMetadata", meta(&f.candidate_metadata)),
        ("ReferenceProfile", profile(&f.reference_profile)),
        ("CandidateProfile", profile(&f.candidate_profile)),
        ("ReferenceModality", modality(&f.reference_modality)),
        ("CandidateModality", modality(&f.candidate_modality)),
        ("ReferencePalette", palette(f.reference_palette.as_ref())),
        ("CandidatePalette", palette(f.candidate_palette.as_ref())),
        ("result", JVal::Null),
    ])
}
fn unsupported(v: &JVal) -> Option<f64> {
    match v {
        JVal::F64(f) if !f.is_finite() => Some(*f),
        JVal::Obj(fields) => fields.iter().find_map(|(_, v)| unsupported(v)),
        JVal::Arr(items) => items.iter().find_map(unsupported),
        _ => None,
    }
}
impl PresentedExportMetadata {
    pub fn to_json(&self) -> Result<String, Error> {
        let mut fields = vec![
            ("schema", s(&self.schema)),
            ("Width", JVal::I64(self.width)),
            ("Height", JVal::I64(self.height)),
            ("Components", JVal::I64(self.components)),
            ("Depth", JVal::I64(self.depth)),
            ("Encoding", s(&self.encoding)),
            ("Options", options(&self.options)),
        ];
        if let Some(m) = &self.raw_rgba {
            fields.push(("raw_rgba8_before_background", raw_rgba(m)));
        }
        fields.push((
            "ImagePolicy",
            self.image_policy.as_ref().map_or(JVal::Null, image_policy),
        ));
        fields.push(("FloatFormat", s(&self.float_format)));
        fields.push(("ValidityFormat", s(&self.validity_format)));
        fields.push(("DifferenceVisual", s(&self.difference_visual)));
        fields.push(("SSIMVisual", s(&self.ssim_visual)));
        fields.push(("Summary", statistics(&self.summary)));
        fields.push((
            "Regions",
            if self.regions.is_empty() {
                JVal::Null
            } else {
                JVal::Arr(self.regions.iter().map(statistics).collect())
            },
        ));
        if let Some(frame) = &self.frame {
            fields.push(("frame", frame_record(frame)));
        }
        fields.push((
            "Artifacts",
            JVal::Obj(
                self.artifacts
                    .iter()
                    .map(|(k, a)| {
                        (
                            k.clone(),
                            obj(vec![
                                ("bytes", JVal::I64(a.bytes)),
                                ("sha256", s(&a.sha256)),
                            ]),
                        )
                    })
                    .collect(),
            ),
        ));
        let value = obj(fields);
        if let Some(f) = unsupported(&value) {
            return Err(Error::Other(format!(
                "json: unsupported value: {}",
                gfloat(f)
            )));
        }
        let mut out = String::new();
        write_compact(&value, &mut out);
        Ok(out)
    }
}
