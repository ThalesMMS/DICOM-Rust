#![forbid(unsafe_code)]
use crate::fmath;
const D50_WHITE_X: f64 = 0.9642956764295676;
const D50_WHITE_Z: f64 = 0.8251046025104602;
const DELTA: f64 = 0.20689655172413793;
const DELTA3: f64 = 0.008856451679035631;
const THREE_DELTA2: f64 = 0.12841854934601665;
const FOUR_29: f64 = 0.13793103448275862;
const INV_GAMMA: f64 = 0.4166666666666667;
pub fn rgb_to_cielab(rgb: [i64; 3]) -> [u16; 3] {
    let r = srgb_to_linear(clamp_byte(rgb[0]) as f64 / 255.0);
    let g = srgb_to_linear(clamp_byte(rgb[1]) as f64 / 255.0);
    let b = srgb_to_linear(clamp_byte(rgb[2]) as f64 / 255.0);
    let x50 = 0.43606574687427 * r + 0.38515150959016 * g + 0.14307841996514 * b;
    let y50 = 0.22249317711057 * r + 0.71688701309448 * g + 0.06061980979495 * b;
    let z50 = 0.01392392146317 * r + 0.09708132423141 * g + 0.71409935681588 * b;
    let fx = lab_f(x50 / D50_WHITE_X);
    let fy = lab_f(y50);
    let fz = lab_f(z50 / D50_WHITE_Z);
    [
        scale_lab_word(116.0 * fy - 16.0, 0.0, 100.0),
        scale_lab_word(500.0 * (fx - fy), -128.0, 127.0),
        scale_lab_word(200.0 * (fy - fz), -128.0, 127.0),
    ]
}
pub fn cielab_to_rgb(encoded: [u16; 3]) -> [i64; 3] {
    let l = f64::from(encoded[0]) * 100.0 / 65535.0;
    let a = f64::from(encoded[1]) * 255.0 / 65535.0 - 128.0;
    let b = f64::from(encoded[2]) * 255.0 / 65535.0 - 128.0;
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let x50 = D50_WHITE_X * lab_f_inverse(fx);
    let y50 = lab_f_inverse(fy);
    let z50 = D50_WHITE_Z * lab_f_inverse(fz);
    let r = 3.13413585290012 * x50 - 1.61738599801804 * y50 - 0.49066221791110 * z50;
    let g = -0.97879547655578 * x50 + 1.91625437739599 * y50 + 0.03344287339037 * z50;
    let bl = 0.07195539255795 * x50 - 0.22897675981518 * y50 + 1.40538603511312 * z50;
    [
        fmath::f64_to_i64((255.0 * linear_to_srgb(r)).round()),
        fmath::f64_to_i64((255.0 * linear_to_srgb(g)).round()),
        fmath::f64_to_i64((255.0 * linear_to_srgb(bl)).round()),
    ]
}
fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        return value / 12.92;
    }
    fmath::pow((value + 0.055) / 1.055, 2.4)
}
fn linear_to_srgb(value: f64) -> f64 {
    let value = fmath::max(0.0, fmath::min(1.0, value));
    if value <= 0.0031308 {
        return 12.92 * value;
    }
    1.055 * fmath::pow(value, INV_GAMMA) - 0.055
}
fn lab_f(value: f64) -> f64 {
    if value > DELTA3 {
        return fmath::cbrt(value);
    }
    value / THREE_DELTA2 + FOUR_29
}
fn lab_f_inverse(value: f64) -> f64 {
    if value > DELTA {
        return value * value * value;
    }
    THREE_DELTA2 * (value - FOUR_29)
}
fn scale_lab_word(value: f64, min_value: f64, max_value: f64) -> u16 {
    let value = fmath::max(min_value, fmath::min(max_value, value));
    ((value - min_value) * 65535.0 / (max_value - min_value)).round() as u16
}
fn clamp_byte(value: i64) -> i64 {
    value.clamp(0, 255)
}
