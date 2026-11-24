mod export;
mod files;
mod images;
mod presented;
mod readers;
mod ssim;
use crate::fmath;
use crate::net::ul::{Ctx, Kind as CtxKind};
pub use export::{
    ExportedNumerical, ExportedStatistics, PresentedArtifact, PresentedExportMetadata,
    PresentedWriters, export_presented,
};
pub use files::{Domain, FilesOptions, files};
pub use images::{
    PresentedImage, PresentedImagePolicy, PresentedRgba8Pair, compare_presented_images,
    compare_presented_rgba8,
};
pub use presented::{
    ComparisonRegion, PresentedFrame, PresentedOptions, PresentedResult, PresentedStatistics, Rect,
    SsimOptions, compare_presented, default_presented_options,
};
pub use readers::{
    PresentationProfile, PresentedFrameRecord, PresentedReader, PresentedReaderOptions,
    compare_presented_readers,
};
use std::fmt;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Mismatch(Option<&'static str>),
    Configuration(Option<&'static str>),
    Limit,
    Context(CtxKind),
    Qualification(String),
    MismatchDetail(String),
    Other(String),
    Wrapped(String, Box<Error>),
}
impl Error {
    pub fn is_mismatch(&self) -> bool {
        match self {
            Error::Mismatch(_) | Error::MismatchDetail(_) => true,
            Error::Wrapped(_, inner) => inner.is_mismatch(),
            _ => false,
        }
    }
    pub fn context_kind(&self) -> Option<CtxKind> {
        match self {
            Error::Context(kind) => Some(*kind),
            Error::Wrapped(_, inner) => inner.context_kind(),
            _ => None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (base, detail) = match self {
            Error::Mismatch(d) => ("dicom/compare: images differ in geometry or color model", d),
            Error::Configuration(d) => (
                "dicom/compare: invalid presented comparison configuration",
                d,
            ),
            Error::Limit => {
                return f.write_str("dicom/compare: presented comparison budget exceeded");
            }
            Error::Context(CtxKind::Canceled) => return f.write_str("context canceled"),
            Error::Context(_) => return f.write_str("context deadline exceeded"),
            Error::Qualification(text) | Error::Other(text) => return f.write_str(text),
            Error::MismatchDetail(detail) => {
                return write!(
                    f,
                    "dicom/compare: images differ in geometry or color model: {detail}"
                );
            }
            Error::Wrapped(context, inner) => return write!(f, "{context}: {inner}"),
        };
        f.write_str(base)?;
        if let Some(d) = detail {
            write!(f, ": {d}")?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {}
pub(crate) fn ctx_err(ctx: &Ctx) -> Result<(), Error> {
    match ctx.err() {
        Some(kind) => Err(Error::Context(kind)),
        None => Ok(()),
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    pub frames: i64,
    pub samples: i64,
    pub max_absolute_error: f64,
    pub mean_absolute_error: f64,
    pub root_mean_square_error: f64,
    pub peak_signal_to_noise_ratio: f64,
    pub signal_to_noise_ratio: f64,
}
impl Metrics {
    pub fn identical(&self) -> bool {
        self.max_absolute_error == 0.0
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Accumulator {
    samples: i64,
    sum_abs: f64,
    sum_square: f64,
    sum_reference: f64,
    max_abs: f64,
    peak: f64,
}
impl Accumulator {
    pub fn add(&mut self, reference: f64, test: f64) {
        let diff = (reference - test).abs();
        self.samples = self.samples.saturating_add(1);
        self.sum_abs += diff;
        self.sum_square += diff * diff;
        self.sum_reference += reference * reference;
        self.max_abs = fmath::max(self.max_abs, diff);
        self.peak = fmath::max(self.peak, reference * reference);
    }
    pub fn metrics(&self) -> Metrics {
        let mut m = Metrics {
            samples: self.samples,
            max_absolute_error: self.max_abs,
            ..Metrics::default()
        };
        if self.samples == 0 {
            return m;
        }
        let n = self.samples as f64;
        let mse = self.sum_square / n;
        m.mean_absolute_error = self.sum_abs / n;
        m.root_mean_square_error = mse.sqrt();
        if self.sum_square == 0.0 {
            m.peak_signal_to_noise_ratio = f64::INFINITY;
            m.signal_to_noise_ratio = f64::INFINITY;
            return m;
        }
        m.peak_signal_to_noise_ratio = 10.0 * fmath::log10(self.peak / mse);
        m.signal_to_noise_ratio = 10.0 * fmath::log10(self.sum_reference / self.sum_square);
        m
    }
}
