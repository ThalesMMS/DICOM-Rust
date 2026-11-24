#![forbid(unsafe_code)]
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    Other,
    InvalidGrid,
    NonCorrectableGeometry,
    InvalidViewState,
    InvalidVolumeSnapshot,
    VolumeStoreClosed,
    VolumeInUse,
    Canceled,
    DeadlineExceeded,
    VolumeNotFound,
    MprInvalidInput,
    MprResourceLimit,
    ColorbarInvalidRange,
    ColorbarNonAffine,
    ColorbarNoBands,
    InvalidVrGradientOpacity,
    InvalidVrTransfer2d,
    InvalidVrLabelMap,
    FusionMapping,
    FusionNoOverlap,
    MapperOutsideDomain,
    MprPresentationUnsupported,
    NotGrayscaleFrame,
    ModalityPrecisionRequired,
    SourceConvolutionKernel,
    SourceConvolutionFrame,
    SourceConvolutionLimit,
    VolumeLeaseReleased,
    VolumeBudgetExceeded,
    InvalidFrameOutput,
    RasterRegion2d,
    Gray2dProfile,
    InvalidVolumeBricks,
    VolumeBrickBudget,
}
#[derive(Debug, Clone, PartialEq)]
pub struct RenderError {
    kind: ErrorKind,
    message: String,
    guardrail: Option<(super::geometry::GeometryIssue, i64)>,
}
impl RenderError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> RenderError {
        RenderError {
            kind,
            message: message.into(),
            guardrail: None,
        }
    }
    pub(crate) fn guardrail(
        reason: super::geometry::GeometryIssue,
        frame_index: i64,
    ) -> RenderError {
        let base = "render: non-correctable volume geometry";
        let message = if frame_index > 0 {
            format!("{base}: {} at frame {frame_index}", reason.as_str())
        } else {
            format!("{base}: {}", reason.as_str())
        };
        RenderError {
            kind: ErrorKind::NonCorrectableGeometry,
            message,
            guardrail: Some((reason, frame_index)),
        }
    }
    pub fn geometry_fallback_reason(&self) -> Option<super::geometry::GeometryIssue> {
        self.guardrail.map(|g| g.0)
    }
    pub fn other(message: impl Into<String>) -> RenderError {
        RenderError::new(ErrorKind::Other, message)
    }
    pub fn is(&self, kind: ErrorKind) -> bool {
        self.kind == kind
    }
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
    pub fn message(&self) -> &str {
        &self.message
    }
}
impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RenderError {}
