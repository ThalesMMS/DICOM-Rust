#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::vr_transfer::{VrLightingMaterial, finite};
use crate::fmath;
pub const MAX_VR_DIRECTIONAL_LIGHTS: usize = 3;
pub const VR_SPECULAR_TABLE_SAMPLES: usize = 4097;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VrToneMapping(pub u32);
impl VrToneMapping {
    #[doc(hidden)]
    pub const CLAMP: VrToneMapping = VrToneMapping(0);
    pub const REINHARD: VrToneMapping = VrToneMapping(1);
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrDirectionalLight {
    pub direction_camera: [f64; 3],
    pub intensity: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VrIlluminationState {
    pub light_count: u32,
    pub lights: [VrDirectionalLight; MAX_VR_DIRECTIONAL_LIGHTS],
    pub tone_mapping: VrToneMapping,
    pub specular_lookup: bool,
}
pub fn vr_vascular_illumination() -> VrIlluminationState {
    let unit = |x: f64, y: f64, z: f64| {
        let n = (x * x + y * y + z * z).sqrt();
        [x / n, y / n, z / n]
    };
    VrIlluminationState {
        light_count: 3,
        tone_mapping: VrToneMapping::REINHARD,
        lights: [
            VrDirectionalLight {
                direction_camera: unit(-0.5, 0.7, 1.0),
                intensity: 0.7,
            },
            VrDirectionalLight {
                direction_camera: unit(0.7, -0.2, 1.0),
                intensity: 0.2,
            },
            VrDirectionalLight {
                direction_camera: unit(0.0, 0.3, -1.0),
                intensity: 0.1,
            },
        ],
        specular_lookup: false,
    }
}
pub fn vr_specular_table(power: f64) -> Result<Vec<f32>, RenderError> {
    if !finite(power) || !(0.0..=128.0).contains(&power) || (power > 0.0 && power < 1.0) {
        return Err(RenderError::new(
            ErrorKind::InvalidViewState,
            "render: invalid view state: specular table power must be zero or 1..128",
        ));
    }
    Ok((0..VR_SPECULAR_TABLE_SAMPLES)
        .map(|i| fmath::pow(i as f64 / (VR_SPECULAR_TABLE_SAMPLES - 1) as f64, power) as f32)
        .collect())
}
pub(crate) fn validate_vr_illumination(
    s: &VrIlluminationState,
    material: &VrLightingMaterial,
) -> Result<(), String> {
    if s.light_count as usize > MAX_VR_DIRECTIONAL_LIGHTS
        || s.tone_mapping.0 > VrToneMapping::REINHARD.0
    {
        return Err("invalid illumination count/tone mapping".into());
    }
    let mut total = 0.0;
    for (i, l) in s.lights.iter().enumerate() {
        if i >= s.light_count as usize {
            if *l != VrDirectionalLight::default() {
                return Err("inactive light carries state".into());
            }
            continue;
        }
        let d = l.direction_camera;
        let n = fmath::hypot(fmath::hypot(d[0], d[1]), d[2]);
        if !finite(n)
            || (n - 1.0).abs() > 1e-6
            || !finite(l.intensity)
            || l.intensity <= 0.0
            || l.intensity > 4.0
        {
            return Err("invalid directional light".into());
        }
        total += l.intensity;
    }
    if total > 4.0
        || !material.valid()
        || material.ambient > 16.0
        || material.diffuse > 16.0
        || material.specular > 16.0
    {
        return Err("illumination material/intensity exceeds profile".into());
    }
    if s.specular_lookup
        && (!finite(material.specular_power)
            || material.specular_power > 128.0
            || (material.specular_power > 0.0 && material.specular_power < 1.0))
    {
        return Err("unsupported specular lookup power".into());
    }
    Ok(())
}
