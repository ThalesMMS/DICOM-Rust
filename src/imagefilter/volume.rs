use crate::render::parallel::Cancellation;
use crate::textfmt::float_v;
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    InvalidVolume,
    CannyResourceLimit,
    Canceled,
    DeadlineExceeded,
    Other,
    InvalidCLAHEOptions,
    InvalidHistogramMatch,
    PSFDeconvolutionInput,
    PSFDeconvolutionLimit,
    PSFDeconvolutionNumeric,
    InvalidHough2D,
    HoughResourceLimit,
    LocalChainBudget,
    LocalChainSuperseded,
    LocalChainClosed,
    RankResourceLimit,
    Phase2DInput,
    Phase2DLimit,
    Phase2DNumeric,
    PoissonDenoisingInput,
    PoissonDenoisingLimit,
    PoissonDenoisingNumeric,
    RegionalBudget,
    TensorDiffusionInput,
    TensorDiffusionLimit,
    TensorDiffusionNumeric,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterError {
    kind: ErrorKind,
    message: String,
}
impl FilterError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> FilterError {
        FilterError {
            kind,
            message: message.into(),
        }
    }
    pub(crate) fn invalid(detail: impl AsRef<str>) -> FilterError {
        FilterError::new(
            ErrorKind::InvalidVolume,
            format!("imagefilter: invalid volume: {}", detail.as_ref()),
        )
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
impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for FilterError {}
pub type Ctx<'a> = Option<&'a dyn Cancellation>;
pub(crate) fn ctx_err(ctx: Ctx<'_>) -> Result<(), FilterError> {
    match ctx.and_then(|c| c.err()) {
        None => Ok(()),
        Some(e) => {
            let kind = if e.is(crate::render::ErrorKind::DeadlineExceeded) {
                ErrorKind::DeadlineExceeded
            } else {
                ErrorKind::Canceled
            };
            Err(FilterError::new(kind, e.message()))
        }
    }
}
pub(crate) fn dims_v(dims: [i64; 3]) -> String {
    format!("[{} {} {}]", dims[0], dims[1], dims[2])
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Volume {
    pub dims: [i64; 3],
    pub spacing_mm: [f64; 3],
    pub data: Vec<f32>,
}
impl Volume {
    pub fn new(dims: [i64; 3], spacing_mm: [f64; 3]) -> Volume {
        let Some(n) = voxel_count(dims) else {
            panic!("imagefilter: invalid volume dimensions {}", dims_v(dims));
        };
        Volume {
            dims,
            spacing_mm,
            data: vec![0.0; n as usize],
        }
    }
    pub fn index(&self, x: i64, y: i64, z: i64) -> i64 {
        z.wrapping_mul(self.dims[1])
            .wrapping_add(y)
            .wrapping_mul(self.dims[0])
            .wrapping_add(x)
    }
    pub fn at(&self, x: i64, y: i64, z: i64) -> f32 {
        self.data[self.index(x, y, z) as usize]
    }
    pub fn validate(&self) -> Result<(), FilterError> {
        validate_shape(self)?;
        if let Some(i) = self.data.iter().position(|x| !x.is_finite()) {
            return Err(FilterError::invalid(format!(
                "non-finite sample at index {i}"
            )));
        }
        Ok(())
    }
}
pub(crate) fn validate_shape(v: &Volume) -> Result<(), FilterError> {
    match voxel_count(v.dims) {
        Some(n) if n as usize == v.data.len() => {}
        _ => {
            return Err(FilterError::invalid(format!(
                "{} samples for dimensions {}",
                v.data.len(),
                dims_v(v.dims)
            )));
        }
    }
    for (axis, s) in v.spacing_mm.iter().enumerate() {
        if v.dims[axis] > 1 && !(*s > 0.0 && *s <= f64::MAX) {
            return Err(FilterError::invalid(format!(
                "spacing {} mm along axis {axis}",
                float_v(*s)
            )));
        }
    }
    Ok(())
}
pub(crate) fn voxel_count(dims: [i64; 3]) -> Option<i64> {
    let mut n: i64 = 1;
    for d in dims {
        if d <= 0 || n > i64::MAX / d {
            return None;
        }
        n *= d;
    }
    Some(n)
}
pub trait ModalitySink {
    fn write(&mut self, p: &[u8]) -> Result<usize, FilterError>;
}
pub trait ModalityF32Writer {
    fn write_modality_f32_context(
        &self,
        ctx: Ctx<'_>,
        dst: &mut dyn ModalitySink,
    ) -> Result<(), FilterError>;
}
pub fn volume_from_modality_f32(
    ctx: Ctx<'_>,
    src: Option<&dyn ModalityF32Writer>,
    dims: [i64; 3],
    spacing_mm: [f64; 3],
) -> Result<Volume, FilterError> {
    let Some(src) = src else {
        return Err(FilterError::invalid("nil source"));
    };
    let Some(n) = voxel_count(dims) else {
        return Err(FilterError::invalid(format!("dimensions {}", dims_v(dims))));
    };
    let mut w = F32Decoder {
        dst: vec![0.0; n as usize],
        n: 0,
        partial: [0; 4],
        pending: 0,
    };
    src.write_modality_f32_context(ctx, &mut w)?;
    if w.n != n as usize || w.pending != 0 {
        return Err(FilterError::invalid(format!(
            "source wrote {} samples for dimensions {}",
            w.n,
            dims_v(dims)
        )));
    }
    Ok(Volume {
        dims,
        spacing_mm,
        data: w.dst,
    })
}
struct F32Decoder {
    dst: Vec<f32>,
    n: usize,
    partial: [u8; 4],
    pending: usize,
}
impl F32Decoder {
    fn put(&mut self, bits: u32) -> Result<(), FilterError> {
        if self.n >= self.dst.len() {
            return Err(FilterError::invalid(format!(
                "source wrote more than {} samples",
                self.dst.len()
            )));
        }
        self.dst[self.n] = f32::from_bits(bits);
        self.n += 1;
        Ok(())
    }
}
impl ModalitySink for F32Decoder {
    fn write(&mut self, p: &[u8]) -> Result<usize, FilterError> {
        let written = p.len();
        let mut p = p;
        while !p.is_empty() {
            if self.pending > 0 || p.len() < 4 {
                let c = (4 - self.pending).min(p.len());
                self.partial[self.pending..self.pending + c].copy_from_slice(&p[..c]);
                self.pending += c;
                p = &p[c..];
                if self.pending < 4 {
                    break;
                }
                self.put(u32::from_le_bytes(self.partial))?;
                self.pending = 0;
                continue;
            }
            self.put(u32::from_le_bytes([p[0], p[1], p[2], p[3]]))?;
            p = &p[4..];
        }
        Ok(written)
    }
}
pub fn volume_from_mask(
    dims: [i64; 3],
    spacing_mm: [f64; 3],
    voxels: &[bool],
) -> Result<Volume, FilterError> {
    match voxel_count(dims) {
        Some(n) if n as usize == voxels.len() => {}
        _ => {
            return Err(FilterError::invalid(format!(
                "mask of {} voxels for dimensions {}",
                voxels.len(),
                dims_v(dims)
            )));
        }
    }
    Ok(Volume {
        dims,
        spacing_mm,
        data: voxels.iter().map(|v| if *v { 1.0 } else { 0.0 }).collect(),
    })
}
