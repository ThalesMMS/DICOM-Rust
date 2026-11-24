use super::gsdf::{CalibrationError, DeviceType, DisplayCalibration, DisplayFunction, DisplayLut};
use super::gsdf::{GSDF_MAX_JND, GSDF_MIN_JND, gsdf_jnd_index};
use super::voi::clamp;
use crate::fmath;
const CIE_EPSILON: f64 = 0.008856;
const CIE_KAPPA: f64 = 903.3;
pub fn cielab_lightness(relative_luminance: f64) -> f64 {
    if relative_luminance > CIE_EPSILON {
        return 116.0 * fmath::cbrt(relative_luminance) - 16.0;
    }
    CIE_KAPPA * relative_luminance
}
pub fn cielab_relative_luminance(lightness: f64) -> f64 {
    let linear = lightness / CIE_KAPPA;
    if linear <= CIE_EPSILON {
        return linear;
    }
    let c = (lightness + 16.0) / 116.0;
    c * c * c
}
impl DisplayCalibration {
    pub fn cielab_lut(&self, input_bits: i64) -> Result<DisplayLut, CalibrationError> {
        if !(1..=16).contains(&input_bits) {
            return Err(CalibrationError::InvalidCurve(format!(
                "input bits {input_bits} outside 1..16"
            )));
        }
        let luminances = self.luminances()?;
        let curve = self.curve.as_ref().expect("luminances validated the curve");
        let (low, high) = self.luminance_range()?;
        let l_min = cielab_lightness(low / high);
        let count = 1usize << input_bits;
        let mut lut = DisplayLut {
            function: DisplayFunction::Cielab,
            input_bits,
            max_ddl: curve.max_ddl,
            entries: vec![0; count],
            min_luminance: low,
            max_luminance: high,
            min_jnd: clamp(gsdf_jnd_index(low), GSDF_MIN_JND, GSDF_MAX_JND),
            max_jnd: clamp(gsdf_jnd_index(high), GSDF_MIN_JND, GSDF_MAX_JND),
            min_lightness: l_min,
            max_lightness: 100.0,
        };
        let (mut first, mut last) = (0usize, luminances.len() - 1);
        if curve.device_type == DeviceType::Printer {
            if self.max_density > 0.0 {
                while first < last && luminances[first] < low {
                    first += 1;
                }
            }
            if self.min_density > 0.0 {
                while last > first && luminances[last] > high {
                    last -= 1;
                }
            }
        }
        let mut ddl = first;
        for (p, entry) in lut.entries.iter_mut().enumerate() {
            let target = high
                * cielab_relative_luminance(
                    l_min + p as f64 * (100.0 - l_min) / (count - 1) as f64,
                );
            while ddl < last && luminances[ddl] < target {
                ddl += 1;
            }
            if ddl > first
                && (luminances[ddl - 1] - target).abs() < (luminances[ddl] - target).abs()
            {
                ddl -= 1;
            }
            *entry = ddl as u16;
        }
        Ok(lut)
    }
}
