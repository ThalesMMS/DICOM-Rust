use super::bitio::BitReader;
use super::codestream::{
    CodestreamHeader, FLAG_SKIP_LF_SM, FrameHeader, MAX_XYB_CODESTREAM, MAX_XYB_DIMENSION,
};
use super::reconstruct::read_vardct_sections;
use super::render::idct8x8;
use super::vardct::{COLOUR_TILE_BLOCKS, VarDctState};
use super::{Error, Image, corrupt, fault, limit, unsupported};
use crate::fmath;
use std::sync::OnceLock;
const XYB_SCALE_UNIT: f64 = 65536.0;
const XYB_BIAS: f64 = 0.0037930732552754493;
fn max_nan(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        fmath::max(a, b)
    }
}
fn min_nan(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        fmath::min(a, b)
    }
}
pub(crate) fn decode_xyb(
    hdr: &CodestreamHeader,
    br: &mut BitReader,
    codestream: &[u8],
    f: &FrameHeader,
    offsets: &[usize],
    sizes: &[usize],
) -> Result<Image, Error> {
    let m = &hdr.meta;
    if m.width > MAX_XYB_DIMENSION
        || m.height > MAX_XYB_DIMENSION
        || codestream.len() > MAX_XYB_CODESTREAM
    {
        return Err(limit());
    }
    if !m.colour.is_srgb
        || m.depth.float_sample
        || m.depth.bits_per_sample != 8
        || m.orientation != 1
        || !m.extra_channels.is_empty()
        || m.custom_opsin
        || m.custom_tone
        || m.have_animation
        || m.have_preview
    {
        return Err(unsupported(
            "XYB requires static 8-bit sRGB with the default colour transform",
        ));
    }
    if f.num_passes != 1
        || f.flags & !FLAG_SKIP_LF_SM != 0
        || f.blend_mode != 0
        || f.gab
        || f.epf_iters != 0
    {
        return Err(unsupported(
            "XYB requires one replacement pass without restoration filters",
        ));
    }
    br.guarded = true;
    let s = read_vardct_sections(br, codestream, f, offsets, sizes, true)?;
    let lf = s.xyb_dc()?;
    s.render_xyb(&lf)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn dct8_weights() -> &'static [[f64; 64]; 3] {
    static W: OnceLock<[[f64; 64]; 3]> = OnceLock::new();
    W.get_or_init(|| {
        const BANDS: [[f64; 6]; 3] = [
            [3150.0, 0.0, -0.4, -0.4, -0.4, -2.0],
            [560.0, 0.0, -0.3, -0.3, -0.3, -0.3],
            [512.0, -2.0, -1.0, 0.0, -1.0, -2.0],
        ];
        const DIVISOR: f64 = 9.899501936611665;
        let mut weights = [[0.0; 64]; 3];
        for (c, ratios) in BANDS.iter().enumerate() {
            let mut radial = [0.0f64; 6];
            radial[0] = ratios[0];
            for (i, &ratio) in ratios[1..].iter().enumerate() {
                let factor = if ratio < 0.0 {
                    1.0 / (1.0 - ratio)
                } else {
                    1.0 + ratio
                };
                radial[i + 1] = radial[i] * factor;
            }
            for (k, w) in weights[c].iter_mut().enumerate() {
                let distance = fmath::hypot((k % 8) as f64, (k / 8) as f64) * 5.0 / DIVISOR;
                let i = distance as usize;
                let weight = radial[i] * fmath::pow(radial[i + 1] / radial[i], distance - i as f64);
                *w = 1.0 / weight;
            }
        }
        weights
    })
}
impl VarDctState {
    fn xyb_dc(&self) -> Result<[Vec<f64>; 3], Error> {
        let mut steps = [1.0 / 4096.0, 1.0 / 512.0, 1.0 / 256.0];
        let mut dc: [Vec<f64>; 3] = Default::default();
        let planes = steps
            .iter_mut()
            .zip(&mut dc)
            .zip(&self.lf_dequant)
            .zip(&self.dc);
        for (((step, out), &dequant), quantized) in planes {
            if !self.lf_dequant_default {
                *step = f64::from(dequant) / 128.0;
                if *step < 1e-8 {
                    return Err(corrupt("invalid XYB LF dequantization"));
                }
            }
            *step *= XYB_SCALE_UNIT / (f64::from(self.global_scale) * f64::from(self.quant_lf));
            *out = vec![0.0; quantized.len()];
        }
        let corr_x =
            f64::from(self.base_corr_x) + f64::from(self.ytox_dc) / f64::from(self.colour_factor);
        let corr_b =
            f64::from(self.base_corr_b) + f64::from(self.ytob_dc) / f64::from(self.colour_factor);
        for (g, &precision) in self
            .lf_precision
            .as_deref()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let (x0, y0, w, h) = self.lf_group_rect(g);
            let precision_scale = fmath::ldexp(1.0, i64::from(precision).wrapping_neg());
            let [q0, q1, q2] = &self.dc;
            let [d0, d1, d2] = &mut dc;
            for y in y0..y0.saturating_add(h) {
                let start = y.saturating_mul(self.f.xsize_blocks).saturating_add(x0);
                let span = start..start.saturating_add(w);
                let (Some(q0), Some(q1), Some(q2), Some(d0), Some(d1), Some(d2)) = (
                    q0.get(span.clone()),
                    q1.get(span.clone()),
                    q2.get(span.clone()),
                    d0.get_mut(span.clone()),
                    d1.get_mut(span.clone()),
                    d2.get_mut(span),
                ) else {
                    return Err(fault());
                };
                let quantized = q0.iter().zip(q1).zip(q2);
                for (((&x, &yq), &b), ((o0, o1), o2)) in
                    quantized.zip(d0.iter_mut().zip(d1.iter_mut()).zip(d2.iter_mut()))
                {
                    let yv = f64::from(yq) * steps[1] * precision_scale;
                    *o0 = f64::from(x) * steps[0] * precision_scale + corr_x * yv;
                    *o1 = yv;
                    *o2 = f64::from(b) * steps[2] * precision_scale + corr_b * yv;
                }
            }
        }
        if self.f.flags & FLAG_SKIP_LF_SM == 0 {
            return Ok(smooth_lf(
                dc,
                self.f.xsize_blocks,
                self.f.ysize_blocks,
                steps,
            ));
        }
        Ok(dc)
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn render_xyb(&self, dc: &[Vec<f64>; 3]) -> Result<Image, Error> {
        let f = &self.f;
        let blocks = f.xsize_blocks.saturating_mul(f.ysize_blocks);
        let tiles = self
            .tiles_x
            .saturating_mul(f.ysize_blocks.div_ceil(COLOUR_TILE_BLOCKS));
        if self.qf.len() < blocks
            || dc.iter().any(|p| p.len() < blocks)
            || self
                .coeffs
                .iter()
                .any(|p| p.len() < blocks.saturating_mul(64))
            || self.ytox_map.len() < tiles
            || self.ytob_map.len() < tiles
            || self.tiles_x < f.xsize_blocks.div_ceil(COLOUR_TILE_BLOCKS)
            || f.xsize > f.xsize_blocks.saturating_mul(8)
            || f.ysize > f.ysize_blocks.saturating_mul(8)
        {
            return Err(fault());
        }
        let mut img = Image {
            width: f.xsize,
            height: f.ysize,
            channels: 3,
            bits_per_sample: 8,
            samples: vec![0; f.xsize.saturating_mul(f.ysize).saturating_mul(3)],
        };
        let weights = dct8_weights();
        let x_scale = fmath::pow(0.8, f64::from(f.xyb_quant_scale[0]) - 2.0);
        let b_scale = fmath::pow(0.8, f64::from(f.xyb_quant_scale[1]) - 2.0);
        let bias_root = fmath::cbrt(XYB_BIAS);
        let mut coeff = [[0.0f64; 64]; 3];
        let mut pixels = [[0.0f64; 64]; 3];
        for by in 0..f.ysize_blocks {
            for bx in 0..f.xsize_blocks {
                let i = by * f.xsize_blocks + bx;
                let tile = by / COLOUR_TILE_BLOCKS * self.tiles_x + bx / COLOUR_TILE_BLOCKS;
                let colour_factor = f64::from(self.colour_factor);
                let corr_x =
                    f64::from(self.base_corr_x) + f64::from(self.ytox_map[tile]) / colour_factor;
                let corr_b =
                    f64::from(self.base_corr_b) + f64::from(self.ytob_map[tile]) / colour_factor;
                let scale = XYB_SCALE_UNIT / (f64::from(self.global_scale) * f64::from(self.qf[i]));
                for c in 0..3 {
                    coeff[c][0] = 8.0 * dc[c][i];
                }
                for k in 1..64 {
                    let x = xyb_ac(self.coeffs[0][64 * i + k], 0) * weights[0][k] * scale * x_scale;
                    let y = xyb_ac(self.coeffs[1][64 * i + k], 1) * weights[1][k] * scale;
                    let b = xyb_ac(self.coeffs[2][64 * i + k], 2) * weights[2][k] * scale * b_scale;
                    let natural = (k % 8) * 8 + k / 8;
                    coeff[0][natural] = 8.0 * (x + corr_x * y);
                    coeff[1][natural] = 8.0 * y;
                    coeff[2][natural] = 8.0 * (b + corr_b * y);
                }
                for c in 0..3 {
                    idct8x8(&coeff[c], &mut pixels[c]);
                }
                for dy in 0..8usize.min(f.ysize - by * 8) {
                    for dx in 0..8usize.min(f.xsize - bx * 8) {
                        let j = dy * 8 + dx;
                        let part = (by * 8 + dy) * f.xsize + bx * 8 + dx;
                        let rgb = xyb_to_srgb(pixels[0][j], pixels[1][j], pixels[2][j], bias_root);
                        img.samples[part * 3..part * 3 + 3].copy_from_slice(&rgb);
                    }
                }
            }
        }
        Ok(img)
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn smooth_lf(planes: [Vec<f64>; 3], width: usize, height: usize, steps: [f64; 3]) -> [Vec<f64>; 3] {
    if width <= 2 || height <= 2 {
        return planes;
    }
    const SIDE_WEIGHT: f64 = 0.20345139757231578;
    const CORNER_WEIGHT: f64 = 0.0334829185968739;
    const CENTER_WEIGHT: f64 = 0.05226273532324128;
    if planes
        .iter()
        .any(|p| p.len() != width.saturating_mul(height))
    {
        return planes;
    }
    let mut result = planes.clone();
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let i = y * width + x;
            let mut delta = [0.0f64; 3];
            let mut gap = 0.5;
            for (c, p) in planes.iter().enumerate() {
                let side = p[i - 1] + p[i + 1] + p[i - width] + p[i + width];
                let corner =
                    p[i - width - 1] + p[i - width + 1] + p[i + width - 1] + p[i + width + 1];
                delta[c] =
                    CENTER_WEIGHT * p[i] + SIDE_WEIGHT * side + CORNER_WEIGHT * corner - p[i];
                gap = max_nan(gap, delta[c].abs() / steps[c]);
            }
            let strength = max_nan(0.0, 3.0 - 4.0 * gap);
            for c in 0..3 {
                result[c][i] += strength * delta[c];
            }
        }
    }
    result
}
const UNIT_QUANT: [f64; 3] = [0.945349926692846, 0.9299455010825141, 0.9500648966626563];
fn xyb_ac(coefficient: i32, channel: usize) -> f64 {
    let value = f64::from(coefficient);
    match coefficient {
        0 => 0.0,
        -1 | 1 => value * UNIT_QUANT.get(channel).copied().unwrap_or(1.0),
        _ => value - 0.145 / value,
    }
}
const DEFAULT_INVERSE_OPSIN: [[f64; 3]; 3] = [
    [11.031566901960783, -9.866943921568629, -0.16462299647058826],
    [-3.254147380392157, 4.418770392156863, -0.16462299647058826],
    [-3.6588512862745097, 2.7129230470588235, 1.9459282392156863],
];
fn xyb_to_srgb(x: f64, y: f64, b: f64, bias_root: f64) -> [i32; 3] {
    let gamma = [y + x + bias_root, y - x + bias_root, b + bias_root];
    let opsin = gamma.map(|g| g * g * g - XYB_BIAS);
    let mut out = [0i32; 3];
    for (o, row) in out.iter_mut().zip(&DEFAULT_INVERSE_OPSIN) {
        let mut v = min_nan(
            1.0,
            max_nan(
                0.0,
                row[0] * opsin[0] + row[1] * opsin[1] + row[2] * opsin[2],
            ),
        );
        if v <= 0.0031308 {
            v *= 12.92;
        } else {
            v = 1.055 * fmath::pow(v, 1.0 / 2.4) - 0.055;
        }
        *o = fmath::f64_to_i32((v * 255.0).round());
    }
    out
}
