use super::recursive_gaussian::{Complex128, RecursiveKernel};
use super::volume::FilterError;
pub const VESSELNESS_SLAB_MARGIN_SIGMAS: f64 = 6.0;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RecursiveGaussianPole {
    pub pole: Complex128,
    pub causal: Complex128,
    pub anticausal: Complex128,
    pub causal_boundary: Complex128,
    pub anticausal_boundary: Complex128,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RecursiveGaussianKernel {
    pub poles: [RecursiveGaussianPole; 2],
}
pub fn plan_recursive_gaussian(
    sigma_samples: f64,
    order: i64,
    scale: f64,
) -> Result<RecursiveGaussianKernel, FilterError> {
    if !(sigma_samples > 0.0)
        || sigma_samples.is_infinite()
        || scale.is_nan()
        || scale.is_infinite()
    {
        return Err(FilterError::invalid("recursive Gaussian parameters"));
    }
    let k = RecursiveKernel::new(sigma_samples, order, scale).map_err(FilterError::invalid)?;
    let mut result = RecursiveGaussianKernel::default();
    for i in 0..2 {
        let p = RecursiveGaussianPole {
            pole: k.z[i],
            causal: k.c[i],
            anticausal: k.d[i],
            causal_boundary: k.c_init[i],
            anticausal_boundary: k.d_init[i],
        };
        let values = [
            p.pole,
            p.causal,
            p.anticausal,
            p.causal_boundary,
            p.anticausal_boundary,
        ];
        if values.iter().any(|v| !v.finite()) {
            return Err(FilterError::invalid(
                "non-finite recursive Gaussian coefficients",
            ));
        }
        result.poles[i] = p;
    }
    Ok(result)
}
