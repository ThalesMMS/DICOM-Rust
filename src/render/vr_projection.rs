#![forbid(unsafe_code)]
use super::vr_transfer::{VrMode, VrPreset, finite};
pub const ERR_INVALID_VR_VALUE_RANGE: &str = "render: invalid VR projection value range";
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrValueRange {
    pub low: f64,
    pub high: f64,
    pub enabled: bool,
}
impl VrValueRange {
    pub fn valid(&self) -> bool {
        !self.enabled || (finite(self.low) && finite(self.high) && self.low <= self.high)
    }
    pub(crate) fn contains(&self, value: f64) -> bool {
        !self.enabled || (value >= self.low && value <= self.high)
    }
}
impl VrPreset {
    pub fn uses_value_range(&self) -> bool {
        self.projection_range.enabled
            && (self.mode == VrMode::AVERAGE || self.mode == VrMode::RAY_SUM)
    }
    pub fn cpu_only_projection(&self) -> bool {
        self.mode == VrMode::RAY_SUM || self.uses_value_range()
    }
}
