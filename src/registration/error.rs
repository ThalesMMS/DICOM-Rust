use super::transform::{Error, ErrorKind};
use crate::render::error::{ErrorKind as RenderKind, RenderError};
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sentinel {
    InvalidParameters,
    InvalidMatrix,
    SingularMatrix,
    UnsupportedTransform,
    Image,
    Resample,
    TooFewSamples,
    MultiMetric,
    SamplingPercentage,
    NoSamplePoints,
    NoMass,
    PrincipalAxes,
    AmbiguousPrincipalAxes,
    Motion,
    MattesWorkspaceLimit,
    NeighborhoodCorrelation,
    NeighborhoodCorrelationLimit,
    Landmarks,
    LandmarkWarpInput,
    LandmarkWarpLimit,
    LandmarkWarpSystem,
    LandmarkWarpDerivative,
    GaussianPointSet,
    PointSetWorkLimit,
    Demons,
    Ffd,
    FfdResourceLimit,
    MaskedFft,
    MaskedFftNoOverlap,
    MaskedFftVariance,
    SyN,
    SyNResourceLimit,
    SyNFolding,
    TransformFile,
    TransformFileBudget,
    OptimizeOptions,
    OptimizeSearchBudget,
    OptimizeNoFiniteCandidate,
    OptimizeLbfgsbResourceLimit,
    Canceled,
    DeadlineExceeded,
    Spatial(crate::spatialreg::Kind),
    Hdf5Format,
    Hdf5Unsupported,
    Hdf5NotFound,
    Hdf5WriteBudget,
}
impl std::hash::Hash for Sentinel {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        if let Sentinel::Spatial(k) = self {
            (*k as u8).hash(state);
        }
    }
}
impl Sentinel {
    pub fn text(self) -> &'static str {
        use Sentinel::*;
        match self {
            InvalidParameters => ErrorKind::InvalidParameters.text(),
            InvalidMatrix => ErrorKind::InvalidMatrix.text(),
            SingularMatrix => ErrorKind::SingularMatrix.text(),
            UnsupportedTransform => ErrorKind::UnsupportedTransform.text(),
            Image => "registration: invalid image",
            Resample => "registration: invalid resampling",
            TooFewSamples => "registration: too few samples overlap",
            MultiMetric => "registration: invalid multi-metric composition",
            SamplingPercentage => "registration: sampling percentage outside (0, 1]",
            NoSamplePoints => "registration: no sample points for parameter scales",
            NoMass => "registration: image has no intensity mass",
            PrincipalAxes => "registration: invalid principal-axis moments",
            AmbiguousPrincipalAxes => "registration: ambiguous principal axes",
            Motion => "registration: invalid motion correction",
            MattesWorkspaceLimit => "registration: Mattes workspace limit exceeded",
            NeighborhoodCorrelation => "registration: invalid neighborhood correlation",
            NeighborhoodCorrelationLimit => "registration: neighborhood correlation resource limit",
            Landmarks => "registration: invalid landmarks",
            LandmarkWarpInput => "registration: invalid landmark warp input",
            LandmarkWarpLimit => "registration: landmark warp resource limit",
            LandmarkWarpSystem => "registration: degenerate or ill-conditioned landmark warp",
            LandmarkWarpDerivative => {
                "registration: radial warp derivative undefined at weighted center"
            }
            GaussianPointSet => "registration: invalid Gaussian pointset metric",
            PointSetWorkLimit => "registration: Gaussian pointset work limit exceeded",
            Demons => "registration: invalid demons registration",
            Ffd => "registration: invalid free-form deformation",
            FfdResourceLimit => "registration: bounded FFD resource limit",
            MaskedFft => "registration: masked FFT admission",
            MaskedFftNoOverlap => "registration: masked FFT insufficient overlap",
            MaskedFftVariance => "registration: masked FFT zero or unresolved variance",
            SyN => "registration: invalid symmetric normalization",
            SyNResourceLimit => "registration: symmetric normalization resource limit",
            SyNFolding => "registration: symmetric normalization folding or unsupported gradient",
            TransformFile => "dicom/registration: transform file",
            TransformFileBudget => "dicom/registration: transform file exceeds byte budget",
            OptimizeOptions => "optimize: invalid options",
            OptimizeSearchBudget => "optimize: search budget exceeded",
            OptimizeNoFiniteCandidate => "optimize: no finite search candidate",
            OptimizeLbfgsbResourceLimit => "optimize: L-BFGS-B resource limit",
            Canceled => "context canceled",
            DeadlineExceeded => "context deadline exceeded",
            Spatial(k) => k.text(),
            Hdf5Format => "hdf5: invalid file",
            Hdf5Unsupported => "hdf5: unsupported feature",
            Hdf5NotFound => "hdf5: no such object",
            Hdf5WriteBudget => "hdf5: encoded file exceeds byte budget",
        }
    }
    fn from_kind(kind: ErrorKind) -> Sentinel {
        match kind {
            ErrorKind::InvalidParameters => Sentinel::InvalidParameters,
            ErrorKind::InvalidMatrix => Sentinel::InvalidMatrix,
            ErrorKind::SingularMatrix => Sentinel::SingularMatrix,
            ErrorKind::UnsupportedTransform => Sentinel::UnsupportedTransform,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegError {
    sentinels: Vec<Sentinel>,
    text: String,
}
impl RegError {
    pub fn bare(s: Sentinel) -> RegError {
        RegError {
            sentinels: vec![s],
            text: s.text().to_string(),
        }
    }
    pub fn new(s: Sentinel, detail: impl AsRef<str>) -> RegError {
        RegError {
            sentinels: vec![s],
            text: format!("{}: {}", s.text(), detail.as_ref()),
        }
    }
    pub fn with_text(sentinels: Vec<Sentinel>, text: impl Into<String>) -> RegError {
        RegError {
            sentinels,
            text: text.into(),
        }
    }
    pub fn wrap(s: Sentinel, inner: &RegError) -> RegError {
        let mut sentinels = vec![s];
        sentinels.extend(inner.sentinels.iter().copied());
        RegError {
            sentinels,
            text: format!("{}: {}", s.text(), inner.text),
        }
    }
    pub(crate) fn wrap_text(s: Sentinel, inner: &dyn fmt::Display) -> RegError {
        RegError::new(s, inner.to_string())
    }
    pub fn prefixed(mut self, prefix: impl AsRef<str>) -> RegError {
        self.text = format!("{}: {}", prefix.as_ref(), self.text);
        self
    }
    pub fn opaque(&self) -> String {
        self.text.clone()
    }
    pub fn is(&self, s: Sentinel) -> bool {
        self.sentinels.contains(&s)
    }
    pub fn sentinels(&self) -> &[Sentinel] {
        &self.sentinels
    }
    pub fn canceled() -> RegError {
        RegError::bare(Sentinel::Canceled)
    }
}
impl fmt::Display for RegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for RegError {}
impl From<Error> for RegError {
    fn from(e: Error) -> RegError {
        RegError {
            sentinels: vec![Sentinel::from_kind(e.kind)],
            text: e.to_string(),
        }
    }
}
impl From<RenderError> for RegError {
    fn from(e: RenderError) -> RegError {
        let sentinels = match e.kind() {
            RenderKind::Canceled => vec![Sentinel::Canceled],
            RenderKind::DeadlineExceeded => vec![Sentinel::DeadlineExceeded],
            _ => Vec::new(),
        };
        RegError {
            sentinels,
            text: e.to_string(),
        }
    }
}
impl From<crate::optimize::Error> for RegError {
    fn from(e: crate::optimize::Error) -> RegError {
        use crate::optimize::Error as E;
        let sentinels = match &e {
            E::Options(_) => vec![Sentinel::OptimizeOptions],
            E::SearchBudget => vec![Sentinel::OptimizeSearchBudget],
            E::NoFiniteCandidate => vec![Sentinel::OptimizeNoFiniteCandidate],
            E::LbfgsbResourceLimit => vec![Sentinel::OptimizeLbfgsbResourceLimit],
            E::Canceled => vec![Sentinel::Canceled],
            E::Callback(inner) => match inner.downcast_ref::<RegError>() {
                Some(r) => return r.clone(),
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        RegError {
            sentinels,
            text: e.to_string(),
        }
    }
}
impl From<RegError> for crate::optimize::Error {
    fn from(e: RegError) -> crate::optimize::Error {
        if e.sentinels == [Sentinel::Canceled] {
            return crate::optimize::Error::Canceled;
        }
        crate::optimize::Error::Callback(Box::new(e))
    }
}
impl From<crate::imagefilter::FilterError> for RegError {
    fn from(e: crate::imagefilter::FilterError) -> RegError {
        let sentinels = if e.is(crate::imagefilter::ErrorKind::Canceled) {
            vec![Sentinel::Canceled]
        } else {
            Vec::new()
        };
        RegError {
            sentinels,
            text: e.to_string(),
        }
    }
}
impl From<crate::spatialreg::Error> for RegError {
    fn from(e: crate::spatialreg::Error) -> RegError {
        use crate::spatialreg::Kind;
        let sentinels = e
            .kinds()
            .iter()
            .map(|&k| match k {
                Kind::Canceled => Sentinel::Canceled,
                k => Sentinel::Spatial(k),
            })
            .collect();
        RegError {
            sentinels,
            text: e.to_string(),
        }
    }
}
