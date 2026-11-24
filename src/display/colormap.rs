use super::{Lut, PaletteColorLut};
use crate::fmath;
impl PaletteColorLut {
    pub fn at(&self, value: i64) -> (u8, u8, u8) {
        match self.tables() {
            Some([r, g, b]) => (
                r.entry_byte(r.lookup(value)),
                g.entry_byte(g.lookup(value)),
                b.entry_byte(b.lookup(value)),
            ),
            None => (0, 0, 0),
        }
    }
}
fn ramp(at: impl Fn(i64) -> u8) -> Lut {
    Lut {
        first_mapped: 0,
        bits_per_entry: 8,
        entries: (0..256).map(|i| u16::from(at(i))).collect(),
        adjustments: Vec::new(),
    }
}
pub fn hot_iron_lut() -> PaletteColorLut {
    PaletteColorLut {
        red: Some(ramp(|i| if i < 85 { (i * 3) as u8 } else { 255 })),
        green: Some(ramp(|i| match i {
            ..85 => 0,
            85..170 => ((i - 85) * 3) as u8,
            _ => 255,
        })),
        blue: Some(ramp(|i| if i < 170 { 0 } else { ((i - 170) * 3) as u8 })),
    }
}
pub fn hot_metal_blue_lut() -> PaletteColorLut {
    PaletteColorLut {
        red: Some(ramp(|i| match i {
            ..64 => 0,
            64..192 => ((i - 64) * 255 / 128) as u8,
            _ => 255,
        })),
        green: Some(ramp(|i| {
            if i < 192 {
                0
            } else {
                ((i - 192) * 255 / 63) as u8
            }
        })),
        blue: Some(ramp(|i| match i {
            ..64 => (i * 255 / 63) as u8,
            64..128 => (255 - (i - 64) * 255 / 63) as u8,
            _ => 0,
        })),
    }
}
pub fn cool_warm_lut() -> PaletteColorLut {
    let cool = srgb8_to_lab(59, 76, 192);
    let neutral = srgb8_to_lab(221, 221, 221);
    let warm = srgb8_to_lab(180, 4, 38);
    let mut channels = [[0u8; 256]; 3];
    for i in 0..256 {
        let t = i as f64 / 255.0;
        let lab = if t <= 0.5 {
            lerp_lab(cool, neutral, t / 0.5)
        } else {
            lerp_lab(neutral, warm, (t - 0.5) / 0.5)
        };
        for (channel, value) in channels.iter_mut().zip(lab_to_srgb8(lab)) {
            channel[i] = value;
        }
    }
    let [red, green, blue] = channels.map(|values| ramp(|i| values[i as usize]));
    PaletteColorLut {
        red: Some(red),
        green: Some(green),
        blue: Some(blue),
    }
}
fn lerp_lab(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}
const WHITE_X: f64 = 0.95047;
const WHITE_Y: f64 = 1.0;
const WHITE_Z: f64 = 1.08883;
const LAB_EPSILON: f64 = 216.0 / 24389.0;
const LAB_KAPPA: f64 = 24389.0 / 27.0;
fn srgb8_to_lab(r: u8, g: u8, b: u8) -> [f64; 3] {
    let lin = |c: u8| {
        let v = f64::from(c) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            fmath::pow((v + 0.055) / 1.055, 2.4)
        }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / WHITE_X;
    let y = (0.2126729 * r + 0.7151522 * g + 0.0721750 * b) / WHITE_Y;
    let z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / WHITE_Z;
    let f = |t: f64| {
        if t > LAB_EPSILON {
            fmath::cbrt(t)
        } else {
            (LAB_KAPPA * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}
fn lab_to_srgb8(lab: [f64; 3]) -> [u8; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let inv = |f: f64| {
        let f3 = f * f * f;
        if f3 > LAB_EPSILON {
            f3
        } else {
            (116.0 * f - 16.0) / LAB_KAPPA
        }
    };
    let (x, y, z) = (inv(fx) * WHITE_X, inv(fy) * WHITE_Y, inv(fz) * WHITE_Z);
    let r = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let g = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let b = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
    const INV_GAMMA: f64 = 5.0 / 12.0;
    let enc = |v: f64| {
        let mut v = fmath::max(0.0, fmath::min(1.0, v));
        if v <= 0.0031308 {
            v *= 12.92;
        } else {
            v = 1.055 * fmath::pow(v, INV_GAMMA) - 0.055;
        }
        fmath::f64_to_u8((v * 255.0).round())
    };
    [enc(r), enc(g), enc(b)]
}
