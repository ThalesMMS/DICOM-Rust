use super::{
    Error, Frame, Kind, TAG_DIFFUSION_B_VALUE, TAG_DIFFUSION_GRADIENT_DIR_SEQ,
    TAG_MR_DIFFUSION_SEQUENCE, numbers, standard_frame,
};
use crate::object::{Object, Vr};
use crate::optimize::Context;
use crate::siemens::{self, EmbeddedDatasetLimits, EmbeddedError, EmbeddedKind, LookupOptions};
use std::fmt;
pub const SOURCE_SIEMENS_CSA_EMBEDDED: &str = "siemens-csa-embedded";
#[derive(Clone, Copy, Debug, Default)]
pub struct SiemensEmbeddedOptions {
    pub lookup: LookupOptions,
    pub limits: EmbeddedDatasetLimits,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SiemensEmbeddedError {
    Diffusion(Error),
    Header(siemens::Error),
    Embedded(EmbeddedError),
}
impl SiemensEmbeddedError {
    pub fn is_canceled(&self) -> bool {
        matches!(self, SiemensEmbeddedError::Embedded(e) if e.kind == EmbeddedKind::Canceled)
    }
}
impl fmt::Display for SiemensEmbeddedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SiemensEmbeddedError::Diffusion(e) => e.fmt(f),
            SiemensEmbeddedError::Header(e) => e.fmt(f),
            SiemensEmbeddedError::Embedded(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for SiemensEmbeddedError {}
fn invalid(detail: &str) -> SiemensEmbeddedError {
    SiemensEmbeddedError::Diffusion(Error::new(Kind::Invalid, detail))
}
fn rejected(cause: Error) -> SiemensEmbeddedError {
    SiemensEmbeddedError::Diffusion(Error::with_text(
        cause.kind,
        "dicom/mrdiffusion: embedded diffusion attribute rejected",
    ))
}
fn canceled(ctx: &Context<'_>) -> Result<(), SiemensEmbeddedError> {
    ctx.err().map_err(|_| {
        SiemensEmbeddedError::Embedded(EmbeddedError {
            kind: EmbeddedKind::Canceled,
            element_index: 0,
            value_index: 0,
            stage: "",
            cause: None,
        })
    })
}
fn embedded_standard_frame(dataset: &Object) -> Result<Frame, SiemensEmbeddedError> {
    if let Some(gradients) = dataset.get_sequence(TAG_DIFFUSION_GRADIENT_DIR_SEQ)
        && gradients.len() != 1
    {
        return Err(invalid("embedded gradient item count"));
    }
    standard_frame(dataset, SOURCE_SIEMENS_CSA_EMBEDDED).map_err(rejected)
}
#[doc(hidden)]
pub fn read_siemens_embedded(
    ctx: Context<'_>,
    dataset: &Object,
    frame_index: i64,
    opts: SiemensEmbeddedOptions,
) -> Result<Frame, SiemensEmbeddedError> {
    canceled(&ctx)?;
    if frame_index < 0 {
        return Err(invalid("negative frame index"));
    }
    let header =
        siemens::image_header(dataset, opts.lookup).map_err(SiemensEmbeddedError::Header)?;
    canceled(&ctx)?;
    let embedded = siemens::embedded_dataset(&header.header, ctx, "MRDiffusion", opts.limits)
        .map_err(SiemensEmbeddedError::Embedded)?;
    let (items, source, conflict) =
        embedded.functional_group_items(frame_index, TAG_MR_DIFFUSION_SEQUENCE);
    if conflict.is_some() {
        return Err(invalid("embedded functional groups conflict"));
    }
    let mut frame = if !items.is_empty() {
        if items.len() != 1 {
            return Err(invalid("embedded macro item count"));
        }
        let mut frame = embedded_standard_frame(&items[0])?;
        frame.functional_group = source;
        frame
    } else if let Some(items) = embedded.get_sequence(TAG_MR_DIFFUSION_SEQUENCE) {
        if items.len() != 1 {
            return Err(invalid("embedded macro item count"));
        }
        embedded_standard_frame(&items[0])?
    } else {
        if !embedded.has(TAG_DIFFUSION_B_VALUE) {
            return Err(SiemensEmbeddedError::Diffusion(Error::bare(Kind::NotFound)));
        }
        let values = numbers(&embedded, TAG_DIFFUSION_B_VALUE, Vr::FD).map_err(rejected)?;
        if values.is_empty() {
            return Err(SiemensEmbeddedError::Diffusion(Error::bare(Kind::NotFound)));
        }
        embedded_standard_frame(&embedded)?
    };
    canceled(&ctx)?;
    frame.frame_index = frame_index;
    frame.source = SOURCE_SIEMENS_CSA_EMBEDDED;
    frame.tag = header.tag;
    Ok(frame)
}
