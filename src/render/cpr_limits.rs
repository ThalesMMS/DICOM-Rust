#![forbid(unsafe_code)]
use std::fmt;
pub const MAX_CPR_CONTROL_POINTS: u64 = 65_536;
pub const MAX_CPR_PATH_LENGTH_MM: u64 = 1_000_000_000;
pub const MAX_CPR_PATH_SAMPLES: u64 = 1_000_000;
pub const MAX_CPR_OUTPUT_DIMENSION: u64 = 8_192;
pub const MAX_CPR_OUTPUT_PIXELS: u64 = 32 * 1024 * 1024;
pub const MAX_CPR_SLAB_SAMPLES: u64 = 4_097;
pub const MAX_CPR_SAMPLE_OPERATIONS: u64 = 128 * 1024 * 1024;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CprError {
    Input {
        field: String,
        reason: String,
    },
    Limit {
        resource: String,
        value: u64,
        limit: u64,
    },
}
impl CprError {
    pub(crate) fn input(field: impl Into<String>, reason: impl Into<String>) -> CprError {
        CprError::Input {
            field: field.into(),
            reason: reason.into(),
        }
    }
    pub(crate) fn limit(resource: impl Into<String>, value: u64, limit: u64) -> CprError {
        CprError::Limit {
            resource: resource.into(),
            value,
            limit,
        }
    }
    pub fn is_input(&self) -> bool {
        matches!(self, CprError::Input { .. })
    }
    pub fn is_limit(&self) -> bool {
        matches!(self, CprError::Limit { .. })
    }
}
impl fmt::Display for CprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CprError::Input { field, reason } if field.is_empty() => {
                write!(f, "render: invalid CPR input: {reason}")
            }
            CprError::Input { field, reason } => {
                write!(f, "render: invalid CPR input: {field}: {reason}")
            }
            CprError::Limit {
                resource,
                value,
                limit,
            } => write!(
                f,
                "render: CPR resource limit exceeded: {resource}={value}, limit={limit}"
            ),
        }
    }
}
impl std::error::Error for CprError {}
pub(crate) fn checked_cpr_product(
    resource: &str,
    limit: u64,
    values: &[u64],
) -> Result<u64, CprError> {
    let mut product: u64 = 1;
    for value in values {
        if *value != 0 && product > limit / value {
            return Err(CprError::limit(resource, limit.wrapping_add(1), limit));
        }
        product = product.wrapping_mul(*value);
        if product > limit {
            return Err(CprError::limit(resource, product, limit));
        }
    }
    Ok(product)
}
