use super::images::{PresentedImage, PresentedImagePolicy, image_plane};
use super::presented::{
    PresentedFrame, PresentedOptions, PresentedResult, compare_presented, finite, presented_budget,
};
use super::{Error, ctx_err};
use crate::display::{
    self, ColorFrame, Gray16View, GrayView, Lut, LutOptions, ModalityLut, PaletteColorLut,
    PixelFormat, Presentation, StoredSampleReader, VoiByteMapper, VoiFunction, VoiLut,
};
use crate::net::ul::Ctx;
use crate::object::{File, Tag};
use crate::pixel::Metadata;
use crate::pixel::frame_reader::FrameReader;
use crate::pixel::pixeldata::frame_size;
pub trait PresentedReader {
    fn metadata(&self) -> Metadata;
    fn file(&self) -> &File;
    fn frame(&self, ctx: &Ctx, index: i64) -> Result<(Vec<u8>, Metadata), Error>;
}
impl PresentedReader for FrameReader<'_> {
    fn metadata(&self) -> Metadata {
        FrameReader::metadata(self).clone()
    }
    fn file(&self) -> &File {
        FrameReader::file(self)
    }
    fn frame(&self, ctx: &Ctx, index: i64) -> Result<(Vec<u8>, Metadata), Error> {
        ctx_err(ctx)?;
        FrameReader::frame(self, index).map_err(|e| Error::Other(e.to_string()))
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentationProfile {
    pub voi: VoiLut,
    pub lut: Option<Lut>,
    pub invert: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentedReaderOptions {
    pub comparison: PresentedOptions,
    pub image: PresentedImagePolicy,
    pub mode: String,
    pub reference: PresentationProfile,
    pub candidate: PresentationProfile,
    pub max_frames: i64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentedFrameRecord {
    pub index: i64,
    pub mode: String,
    pub reference_metadata: Metadata,
    pub candidate_metadata: Metadata,
    pub reference_profile: PresentationProfile,
    pub candidate_profile: PresentationProfile,
    pub reference_modality: ModalityLut,
    pub candidate_modality: ModalityLut,
    pub reference_palette: Option<PaletteColorLut>,
    pub candidate_palette: Option<PaletteColorLut>,
    pub result: PresentedResult,
}
fn other(err: impl std::fmt::Display) -> Error {
    Error::Other(err.to_string())
}
pub(super) fn clone_presentation_lut(l: Option<&Lut>) -> Result<Option<Lut>, Error> {
    let Some(l) = l else {
        return Ok(None);
    };
    if l.first_mapped < -2_147_483_648
        || l.first_mapped > 2_147_483_647
        || l.entries.is_empty()
        || l.entries.len() > 65536
        || l.adjustments.len() > 32
        || l.adjustments.iter().any(|a| a.description().len() > 256)
    {
        return Err(Error::Configuration(None));
    }
    let mut out = Lut::new(
        &[l.entries.len() as i64, l.first_mapped, l.bits_per_entry],
        l.entries.clone(),
    )
    .map_err(other)?;
    out.adjustments = l.adjustments.clone();
    Ok(Some(out))
}
pub(super) fn clone_profile(
    p: &PresentationProfile,
    gray: bool,
) -> Result<PresentationProfile, Error> {
    if !gray {
        if p.voi.width != 0.0
            || p.voi.center != 0.0
            || p.voi.lut.is_some()
            || p.lut.is_some()
            || p.invert
            || p.voi.function != VoiFunction::Linear
            || !p.voi.explanation.is_empty()
        {
            return Err(Error::Configuration(None));
        }
        return Ok(p.clone());
    }
    if !finite(p.voi.center) || !finite(p.voi.width) || p.voi.explanation.len() > 1024 {
        return Err(Error::Configuration(None));
    }
    if p.voi.lut.is_none()
        && (p.voi.width <= 0.0 || (p.voi.function == VoiFunction::Linear && p.voi.width < 1.0))
    {
        return Err(Error::Configuration(Some("explicit VOI required")));
    }
    let mut out = p.clone();
    out.voi.lut = clone_presentation_lut(p.voi.lut.as_ref())?;
    out.lut = clone_presentation_lut(p.lut.as_ref())?;
    Ok(out)
}
pub(super) fn clone_palette(p: Option<&PaletteColorLut>) -> Result<Option<PaletteColorLut>, Error> {
    let Some(p) = p else {
        return Ok(None);
    };
    let out = PaletteColorLut {
        red: clone_presentation_lut(p.red.as_ref())?,
        green: clone_presentation_lut(p.green.as_ref())?,
        blue: clone_presentation_lut(p.blue.as_ref())?,
    };
    if out.red.is_none() || out.green.is_none() || out.blue.is_none() {
        return Err(Error::Mismatch(None));
    }
    Ok(Some(out))
}
fn palette_photometric(m: &Metadata) -> bool {
    let photo = m.photometric_interpretation.trim().to_uppercase();
    photo == "PALETTE COLOR" || photo == "ARGB"
}
fn reader_palette(r: &dyn PresentedReader, m: &Metadata) -> Result<Option<PaletteColorLut>, Error> {
    if !palette_photometric(m) {
        return Ok(None);
    }
    let p =
        display::palette_from_object(&r.file().dataset, LutOptions::default()).map_err(other)?;
    clone_palette(p.as_ref())
}
fn is_gray(m: &Metadata) -> bool {
    let s = m.photometric_interpretation.trim().to_uppercase();
    m.samples_per_pixel == 1 && (s == "MONOCHROME1" || s == "MONOCHROME2")
}
pub fn compare_presented_readers(
    ctx: &Ctx,
    a: &dyn PresentedReader,
    b: &dyn PresentedReader,
    o: &PresentedReaderOptions,
    consume: &mut dyn FnMut(&Ctx, PresentedFrameRecord) -> Result<(), Error>,
) -> (i64, Result<(), Error>) {
    let mut completed = 0;
    let result = compare_frames(ctx, a, b, o, consume, &mut completed);
    (completed, result)
}
fn compare_frames(
    ctx: &Ctx,
    a: &dyn PresentedReader,
    b: &dyn PresentedReader,
    o: &PresentedReaderOptions,
    consume: &mut dyn FnMut(&Ctx, PresentedFrameRecord) -> Result<(), Error>,
    completed: &mut i64,
) -> Result<(), Error> {
    ctx_err(ctx)?;
    let (am, bm) = (a.metadata(), b.metadata());
    let gray = is_gray(&am);
    if am.rows != bm.rows
        || am.columns != bm.columns
        || gray != is_gray(&bm)
        || am.number_of_frames != bm.number_of_frames
        || am.number_of_frames <= 0
    {
        return Err(Error::Mismatch(None));
    }
    if o.max_frames <= 0 {
        return Err(Error::Configuration(None));
    }
    if am.number_of_frames > o.max_frames {
        return Err(Error::Limit);
    }
    o.image.validate()?;
    if gray != (o.image.encoding == "gray-pvalue") {
        return Err(Error::Mismatch(None));
    }
    if o.mode != "common" && o.mode != "per-image" {
        return Err(Error::Configuration(None));
    }
    let c = if gray { 1 } else { 3 };
    let mut budget = presented_budget(
        i64::from(am.columns),
        i64::from(am.rows),
        c,
        &o.comparison,
        128,
    )?;
    if budget > o.comparison.max_working_bytes.saturating_sub(4 << 20) {
        return Err(Error::Limit);
    }
    budget = budget.saturating_add(4 << 20);
    let ar = clone_profile(&o.reference, gray)?;
    let br = if o.mode == "per-image" {
        clone_profile(&o.candidate, gray)?
    } else {
        ar.clone()
    };
    for r in [a, b] {
        if r.file().dataset.get(Tag::new(0x0028, 0x2000)).is_some() {
            return Err(Error::MismatchDetail(
                "ICC-managed presentation must be supplied as a produced image".into(),
            ));
        }
    }
    for i in 0..am.number_of_frames {
        ctx_err(ctx)?;
        let (av, ad, ma) = reader_plane(ctx, a, i, &am, &ar, &o.image, &o.comparison)?;
        let (bv, bd, mb) = reader_plane(ctx, b, i, &bm, &br, &o.image, &o.comparison)?;
        let mut result = compare_presented(ctx, &av, &bv, &o.comparison)?;
        result.image_policy = Some(o.image.clone());
        result.logical_working_bytes = budget;
        let mut record = PresentedFrameRecord {
            index: i,
            mode: o.mode.clone(),
            reference_palette: None,
            candidate_palette: None,
            reference_profile: ar.clone(),
            candidate_profile: br.clone(),
            reference_modality: ma,
            candidate_modality: mb,
            result,
            reference_metadata: ad,
            candidate_metadata: bd,
        };
        record.reference_palette = reader_palette(a, &record.reference_metadata)?;
        record.candidate_palette = reader_palette(b, &record.candidate_metadata)?;
        ctx_err(ctx)?;
        consume(ctx, record)?;
        *completed = (*completed).saturating_add(1);
    }
    ctx_err(ctx)
}
fn reader_plane(
    ctx: &Ctx,
    r: &dyn PresentedReader,
    index: i64,
    expected: &Metadata,
    p: &PresentationProfile,
    policy: &PresentedImagePolicy,
    o: &PresentedOptions,
) -> Result<(PresentedFrame, Metadata, ModalityLut), Error> {
    let (raw, m) = r.frame(ctx, index)?;
    ctx_err(ctx)?;
    let size = frame_size(&m);
    if m.rows != expected.rows
        || m.columns != expected.columns
        || is_gray(&m) != is_gray(expected)
        || m.samples_per_pixel == 0
        || m.samples_per_pixel > 4
        || m.bits_allocated > 32
        || size <= 0
        || raw.len() as i64 != size
    {
        return Err(Error::Mismatch(None));
    }
    let (rows, columns) = (usize::from(m.rows), usize::from(m.columns));
    let n = rows.saturating_mul(columns);
    if raw.len() as i64 > (n as i64).saturating_mul(16) {
        return Err(Error::Limit);
    }
    let format = PixelFormat {
        bits_allocated: i64::from(m.bits_allocated),
        bits_stored: i64::from(m.bits_stored),
        high_bit: i64::from(m.high_bit),
        signed: m.pixel_representation == 1,
        big_endian: false,
    };
    if !is_gray(&m) {
        let mut cf = ColorFrame {
            rows: i64::from(m.rows),
            columns: i64::from(m.columns),
            pixels: &raw,
            photometric: m.photometric_interpretation.clone(),
            samples_per_pixel: i64::from(m.samples_per_pixel),
            planar_configuration: i64::from(m.planar_configuration),
            format,
            palette: None,
        };
        let photo = m.photometric_interpretation.trim();
        if photo.eq_ignore_ascii_case("PALETTE COLOR") || photo.eq_ignore_ascii_case("ARGB") {
            cf.palette = display::palette_from_object(&r.file().dataset, LutOptions::default())
                .map_err(other)?;
        }
        let f = if policy.depth == 8 {
            let im = display::render_color(&cf).map_err(other)?;
            image_plane(
                ctx,
                &PresentedImage::Rgba {
                    width: im.width as i64,
                    height: im.height as i64,
                    pix: &im.pixels,
                },
                policy,
                o,
            )?
        } else {
            let im = display::render_color16(&cf).map_err(other)?;
            image_plane(
                ctx,
                &PresentedImage::Rgba64 {
                    width: im.width as i64,
                    height: im.height as i64,
                    pix: &im.pixels,
                },
                policy,
                o,
            )?
        };
        return Ok((f, m, ModalityLut::default()));
    }
    let (mut modality, err) =
        display::modality_lut_at(&r.file().dataset, index, LutOptions::default());
    if let Some(err) = err {
        return Err(other(err));
    }
    if !finite(modality.slope) || !finite(modality.intercept) {
        return Err(Error::Mismatch(None));
    }
    modality.lut = clone_presentation_lut(modality.lut.as_ref())?;
    let reader = StoredSampleReader::new(
        format.bits_allocated,
        format.bits_stored,
        format.high_bit,
        m.pixel_representation != 0,
        false,
    )
    .map_err(|e| other(format!("dicom: invalid stored sample format: {e:?}: {e}")))?;
    let width = reader.width();
    if width == 0 || n > raw.len().checked_div(width).unwrap_or(0) {
        return Err(Error::Mismatch(None));
    }
    let mapper = VoiByteMapper::new(p.voi.clone());
    let mut f = PresentedFrame {
        width: columns as i64,
        height: rows as i64,
        components: 1,
        depth: policy.depth,
        encoding: "gray-pvalue".into(),
        values: vec![0.0; n],
    };
    let deep = policy.depth == 16;
    let max = if deep { 65535.0 } else { 255.0 };
    let bytes = if deep { 2 } else { 1 };
    let mut row = vec![0u8; columns.saturating_mul(bytes)];
    let presentation = Presentation {
        lut: p.lut.clone(),
        invert: p.invert,
        ..Presentation::default()
    };
    for y in 0..rows {
        ctx_err(ctx)?;
        for x in 0..columns {
            let i = y.saturating_mul(columns).saturating_add(x);
            let Some(sample) = raw.get(i.saturating_mul(width)..) else {
                return Err(Error::Mismatch(None));
            };
            let stored = reader.value(sample);
            let value = modality.apply(stored);
            if !finite(value) {
                return Err(Error::Mismatch(None));
            }
            let q = (mapper.unit(value) * max).round() as u16;
            if deep {
                if let Some(d) = row.as_chunks_mut::<2>().0.get_mut(x) {
                    *d = q.to_be_bytes();
                }
            } else if let Some(d) = row.get_mut(x) {
                *d = q as u8;
            }
        }
        let stride = row.len();
        if deep {
            presentation.apply_gray16(&mut Gray16View {
                pixels: &mut row,
                stride,
                width: columns,
                height: 1,
            });
        } else {
            presentation.apply(&mut GrayView {
                pixels: &mut row,
                stride,
                width: columns,
                height: 1,
            });
        }
        let start = y.saturating_mul(columns);
        let out = f
            .values
            .get_mut(start..start.saturating_add(columns))
            .unwrap_or_default();
        if deep {
            for (v, &pair) in out.iter_mut().zip(row.as_chunks::<2>().0) {
                *v = f64::from(u16::from_be_bytes(pair)) / max;
            }
        } else {
            for (v, &b) in out.iter_mut().zip(&row) {
                *v = f64::from(b) / max;
            }
        }
    }
    ctx_err(ctx)?;
    Ok((f, m, modality))
}
