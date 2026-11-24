use super::codestream::COLOUR_SPACE_GREY;
use super::jbrd::{JpegComponent, JpegData, JpegQuantTable};
use super::reconstruct::read_jpeg_compatible_frame;
use super::{Error, Image, unsupported};
use crate::fmath;
use std::sync::OnceLock;
pub(crate) fn decode_jpeg_compatible(codestream: &[u8]) -> Result<Image, Error> {
    let (hdr, s) = read_jpeg_compatible_frame(codestream)?;
    if s.f.upsampling != 1 {
        return Err(unsupported("upsampled VarDCT frame"));
    }
    let grey = hdr.meta.colour.colour_space == COLOUR_SPACE_GREY;
    let n = if grey { 1 } else { 3 };
    let mut j = JpegData {
        components: (0..n as u32)
            .map(|i| JpegComponent {
                id: i.saturating_add(1),
                quant_idx: i,
                ..Default::default()
            })
            .collect(),
        quant: vec![JpegQuantTable::default(); n],
        ..Default::default()
    };
    let is_ycbcr = !grey && s.f.do_ycbcr;
    s.fill_jpeg(&mut j, grey, is_ycbcr)?;
    Ok(render_jpeg(&j, is_ycbcr, hdr.meta.depth.bits_per_sample))
}
#[allow(clippy::arithmetic_side_effects)]
fn idct_basis() -> &'static [[f64; 8]; 8] {
    static BASIS: OnceLock<[[f64; 8]; 8]> = OnceLock::new();
    BASIS.get_or_init(|| {
        let mut t = [[0.0; 8]; 8];
        for (u, row) in t.iter_mut().enumerate() {
            let cu = if u == 0 {
                std::f64::consts::SQRT_2 / 4.0
            } else {
                0.5
            };
            for (x, v) in row.iter_mut().enumerate() {
                *v = cu * fmath::cos((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0);
            }
        }
        t
    })
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn idct8x8(coef: &[f64; 64], out: &mut [f64; 64]) {
    let basis = idct_basis();
    let mut tmp = [0.0f64; 64];
    for v in 0..8 {
        for x in 0..8 {
            let mut sum = 0.0;
            for u in 0..8 {
                sum += basis[u][x] * coef[v * 8 + u];
            }
            tmp[v * 8 + x] = sum;
        }
    }
    for x in 0..8 {
        for y in 0..8 {
            let mut sum = 0.0;
            for v in 0..8 {
                sum += basis[v][y] * tmp[v * 8 + x];
            }
            out[y * 8 + x] = sum;
        }
    }
}
struct Plane {
    w: usize,
    h: usize,
    v: Vec<f64>,
}
impl Plane {
    fn at(&self, x: i64, y: i64) -> f64 {
        if self.w == 0 || self.h == 0 {
            return 0.0;
        }
        let x = x.clamp(0, (self.w as i64).saturating_sub(1)) as usize;
        let y = y.clamp(0, (self.h as i64).saturating_sub(1)) as usize;
        self.v
            .get(y.saturating_mul(self.w).saturating_add(x))
            .copied()
            .unwrap_or(0.0)
    }
    fn sample(&self, x: usize, y: usize, hs: u32, vs: u32) -> f64 {
        if hs == 0 && vs == 0 {
            return self.at(x as i64, y as i64);
        }
        let fx = (x as f64 + 0.5) / f64::from(1u32 << hs) - 0.5;
        let fy = (y as f64 + 0.5) / f64::from(1u32 << vs) - 0.5;
        let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
        let (ax, ay) = (fx - x0 as f64, fy - y0 as f64);
        let (x1, y1) = (x0.saturating_add(1), y0.saturating_add(1));
        let top = self.at(x0, y0) * (1.0 - ax) + self.at(x1, y0) * ax;
        let bottom = self.at(x0, y1) * (1.0 - ax) + self.at(x1, y1) * ax;
        top * (1.0 - ay) + bottom * ay
    }
}
fn render_jpeg(j: &JpegData, is_ycbcr: bool, bits: u32) -> Image {
    let max_h = j.components.iter().map(|c| c.h_samp).fold(1, usize::max);
    let max_v = j.components.iter().map(|c| c.v_samp).fold(1, usize::max);
    let log2_ratio = |r: usize| u32::from(r >= 2);
    let mut planes = Vec::with_capacity(j.components.len());
    let mut shifts = Vec::with_capacity(j.components.len());
    let none = JpegQuantTable::default();
    for c in &j.components {
        let q = &j.quant.get(c.quant_idx as usize).unwrap_or(&none).values;
        let mut p = Plane {
            w: c.width_blocks.saturating_mul(8),
            h: c.height_blocks.saturating_mul(8),
            v: Vec::new(),
        };
        p.v = vec![0.0; p.w.saturating_mul(p.h)];
        let mut coef = [0.0f64; 64];
        let mut out = [0.0f64; 64];
        let mut blocks = c.coeffs.as_chunks::<64>().0.iter();
        for band in
            p.v.chunks_exact_mut(p.w.saturating_mul(8).max(1))
                .take(c.height_blocks)
        {
            for bx in 0..c.width_blocks {
                let Some(blk) = blocks.next() else {
                    break;
                };
                for ((d, &b), &q) in coef.iter_mut().zip(blk).zip(q) {
                    *d = f64::from(i32::from(b).wrapping_mul(q));
                }
                idct8x8(&coef, &mut out);
                for (row, src) in band
                    .chunks_exact_mut(p.w.max(1))
                    .zip(out.as_chunks::<8>().0)
                {
                    let x0 = bx.saturating_mul(8);
                    if let Some(dst) = row.get_mut(x0..x0.saturating_add(8)) {
                        for (d, &s) in dst.iter_mut().zip(src) {
                            *d = s + 128.0;
                        }
                    }
                }
            }
        }
        planes.push(p);
        shifts.push((
            log2_ratio(max_h.checked_div(c.h_samp).unwrap_or(max_h)),
            log2_ratio(max_v.checked_div(c.v_samp).unwrap_or(max_v)),
        ));
    }
    let n = j.components.len();
    let mut img = Image {
        width: j.width,
        height: j.height,
        channels: n,
        bits_per_sample: bits,
        samples: vec![0; j.width.saturating_mul(j.height).saturating_mul(n)],
    };
    let max_val = 1i64.checked_shl(bits).map_or(-1, |v| v.wrapping_sub(1)) as f64;
    let scale = max_val / 255.0;
    let put = |v: f64| fmath::f64_to_i32(fmath::max(0.0, fmath::min(max_val, v * scale)).round());
    let mut pixels = img.samples.chunks_exact_mut(n.max(1));
    for y in 0..j.height {
        for x in 0..j.width {
            let Some(pixel) = pixels.next() else {
                return img;
            };
            let sample = |c: usize| match (planes.get(c), shifts.get(c)) {
                (Some(p), Some(&(hs, vs))) => p.sample(x, y, hs, vs),
                _ => 0.0,
            };
            if let [s] = pixel {
                *s = put(sample(0));
                continue;
            }
            let (mut a, mut b, mut c) = (sample(0), sample(1), sample(2));
            if is_ycbcr {
                let (cb, cr) = (b - 128.0, c - 128.0);
                (a, b, c) = (
                    a + 1.402 * cr,
                    a - 0.344136 * cb - 0.714136 * cr,
                    a + 1.772 * cb,
                );
            }
            if let [sa, sb, sc, ..] = pixel {
                (*sa, *sb, *sc) = (put(a), put(b), put(c));
            }
        }
    }
    img
}
