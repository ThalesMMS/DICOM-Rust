#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::vec3::Vec3;
use super::volume::Volume;
use super::vr_transfer::finite;
use crate::textfmt::float_v;
pub const MAX_VR_GRADIENT_OPACITY_POINTS: usize = 32;
pub const ERR_INVALID_VR_GRADIENT_OPACITY: &str = "render: invalid VR gradient opacity function";
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrGradientOpacityPoint {
    pub magnitude: f64,
    pub opacity: f64,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VrGradientOpacity(pub Vec<VrGradientOpacityPoint>);
impl VrGradientOpacity {
    pub fn validate(&self) -> Result<(), RenderError> {
        let fail = |detail: String| {
            Err(RenderError::new(
                ErrorKind::InvalidVrGradientOpacity,
                format!("{ERR_INVALID_VR_GRADIENT_OPACITY}: {detail}"),
            ))
        };
        let f = &self.0;
        if f.len() > MAX_VR_GRADIENT_OPACITY_POINTS {
            return fail(format!(
                "{} points, at most {}",
                f.len(),
                MAX_VR_GRADIENT_OPACITY_POINTS
            ));
        }
        for (i, point) in f.iter().enumerate() {
            if !finite(point.magnitude)
                || point.magnitude < 0.0
                || !finite(point.opacity)
                || point.opacity < 0.0
                || point.opacity > 1.0
            {
                return fail(format!(
                    "point {i} {{Magnitude:{} Opacity:{}}}",
                    float_v(point.magnitude),
                    float_v(point.opacity)
                ));
            }
            if i > 0 && point.magnitude <= f[i - 1].magnitude {
                return fail(format!("magnitudes not increasing at point {i}"));
            }
        }
        Ok(())
    }
    pub fn at(&self, magnitude: f64) -> f64 {
        let f = &self.0;
        if f.is_empty() {
            return 1.0;
        }
        if magnitude <= f[0].magnitude || magnitude.is_nan() {
            return f[0].opacity;
        }
        for i in 1..f.len() {
            if magnitude <= f[i].magnitude {
                let previous = f[i - 1];
                let t = (magnitude - previous.magnitude) / (f[i].magnitude - previous.magnitude);
                return previous.opacity + t * (f[i].opacity - previous.opacity);
            }
        }
        f[f.len() - 1].opacity
    }
    pub fn ramp(low: f64, high: f64) -> VrGradientOpacity {
        VrGradientOpacity(vec![
            VrGradientOpacityPoint {
                magnitude: low,
                opacity: 0.0,
            },
            VrGradientOpacityPoint {
                magnitude: high,
                opacity: 1.0,
            },
        ])
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
impl Volume {
    pub(crate) fn physical_gradient_magnitude(&self, index_gradient: Vec3) -> f64 {
        self.physical_gradient(index_gradient).length()
    }
    pub(crate) fn physical_gradient(&self, index_gradient: Vec3) -> Vec3 {
        let mut physical = Vec3::ZERO;
        if self.col_spacing > 0.0 {
            physical = physical.add(self.axis_x.scale(0.5 * index_gradient.x / self.col_spacing));
        }
        if self.row_spacing > 0.0 {
            physical = physical.add(self.axis_y.scale(0.5 * index_gradient.y / self.row_spacing));
        }
        if self.slice_spacing > 0.0 {
            physical = physical.add(
                self.normal
                    .scale(0.5 * index_gradient.z / self.slice_spacing),
            );
        }
        physical
    }
}
