use super::{Error, Kind};
use crate::fmath;
use crate::textfmt::float_v;
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdcOptions {
    pub mask: Option<Vec<bool>>,
    pub min_signal: f64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdcMap {
    pub values: Vec<f64>,
    pub fitted: i64,
    pub negative: i64,
}
fn adc_err(detail: impl AsRef<str>) -> Error {
    Error::new(Kind::Adc, detail)
}
pub fn fit_adc(b_values: &[f64], signals: &[Vec<f32>], opts: &AdcOptions) -> Result<AdcMap, Error> {
    if b_values.len() != signals.len() {
        return Err(adc_err(format!(
            "{} b-values for {} images",
            b_values.len(),
            signals.len()
        )));
    }
    if signals.len() < 2 {
        return Err(adc_err(format!("{} images", signals.len())));
    }
    let voxels = signals[0].len();
    let mut mean_b = 0.0f64;
    for (i, &b) in b_values.iter().enumerate() {
        if b.is_nan() || b.is_infinite() || b < 0.0 {
            return Err(adc_err(format!("b-value {}", float_v(b))));
        }
        if signals[i].len() != voxels {
            return Err(adc_err(format!(
                "image {i} has {} voxels, want {voxels}",
                signals[i].len()
            )));
        }
        mean_b += b;
    }
    mean_b /= b_values.len() as f64;
    let mut variance_b = 0.0f64;
    for &b in b_values {
        variance_b += (b - mean_b) * (b - mean_b);
    }
    if variance_b < 1e-9 {
        return Err(adc_err("the b-values are not distinct"));
    }
    if let Some(mask) = &opts.mask
        && mask.len() != voxels
    {
        return Err(adc_err(format!(
            "mask has {} voxels, want {voxels}",
            mask.len()
        )));
    }
    let min_signal = fmath::max(opts.min_signal, 0.0);
    let mut out = AdcMap {
        values: vec![0.0; voxels],
        fitted: 0,
        negative: 0,
    };
    let mut logs = vec![0.0f64; signals.len()];
    'voxel: for v in 0..voxels {
        if let Some(mask) = &opts.mask
            && !mask[v]
        {
            continue;
        }
        let mut mean_log = 0.0f64;
        for (i, image) in signals.iter().enumerate() {
            let s = image[v] as f64;
            if s.partial_cmp(&min_signal) != Some(std::cmp::Ordering::Greater) || s.is_infinite() {
                continue 'voxel;
            }
            logs[i] = fmath::log(s);
            mean_log += logs[i];
        }
        mean_log /= logs.len() as f64;
        let mut covariance = 0.0f64;
        for (i, &b) in b_values.iter().enumerate() {
            covariance += (b - mean_b) * (logs[i] - mean_log);
        }
        let adc = -covariance / variance_b;
        out.fitted += 1;
        if adc < 0.0 {
            out.negative += 1;
            continue;
        }
        out.values[v] = adc;
    }
    Ok(out)
}
