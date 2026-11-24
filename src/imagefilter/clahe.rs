use super::volume::{Ctx, ErrorKind, FilterError, ctx_err};
use crate::display::Gray16Image;
use crate::display::GrayImage;
use crate::fmath;
use crate::textfmt::float_v;
pub const DEFAULT_CLAHE_TILES: i64 = 8;
pub const DEFAULT_CLAHE_CLIP_LIMIT: f64 = 2.5;
pub const DEFAULT_CLAHE_BINS: i64 = 1024;
pub const DEFAULT_CLAHE_BIT_DEPTH: i64 = 16;
fn invalid(detail: impl AsRef<str>) -> FilterError {
    FilterError::new(
        ErrorKind::InvalidCLAHEOptions,
        format!("imagefilter: invalid CLAHE options: {}", detail.as_ref()),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CLAHEOptions {
    pub tiles_x: i64,
    pub tiles_y: i64,
    pub clip_limit: f64,
    pub bins: i64,
    pub bit_depth: i64,
    pub workers: i64,
}
#[derive(Clone, Copy)]
struct ClaheConfig {
    tiles_x: usize,
    tiles_y: usize,
    clip_limit: f64,
    bins: usize,
    max_value: usize,
}
impl CLAHEOptions {
    fn resolve(&self, width: i64, height: i64) -> Result<ClaheConfig, FilterError> {
        let bit_depth = if self.bit_depth == 0 {
            DEFAULT_CLAHE_BIT_DEPTH
        } else {
            self.bit_depth
        };
        if !(1..=16).contains(&bit_depth) {
            return Err(invalid(format!(
                "BitDepth={} outside 1..16",
                self.bit_depth
            )));
        }
        let max_value = (1i64 << bit_depth) - 1;
        let tiles_x = if self.tiles_x == 0 {
            DEFAULT_CLAHE_TILES
        } else {
            self.tiles_x
        };
        let tiles_y = if self.tiles_y == 0 {
            DEFAULT_CLAHE_TILES
        } else {
            self.tiles_y
        };
        if tiles_x < 0 || tiles_y < 0 {
            return Err(invalid(format!("tiles {}×{}", self.tiles_x, self.tiles_y)));
        }
        let mut clip_limit = self.clip_limit;
        if clip_limit == 0.0 {
            clip_limit = DEFAULT_CLAHE_CLIP_LIMIT;
        } else if clip_limit.is_nan() || clip_limit < 1.0 {
            return Err(invalid(format!(
                "ClipLimit={} (want 0, >= 1 or +Inf)",
                float_v(self.clip_limit)
            )));
        }
        let bins = if self.bins == 0 {
            DEFAULT_CLAHE_BINS.min(max_value + 1)
        } else {
            self.bins
        };
        if bins < 2 || bins > max_value + 1 {
            return Err(invalid(format!(
                "Bins={} outside 2..{}",
                self.bins,
                max_value + 1
            )));
        }
        Ok(ClaheConfig {
            tiles_x: tiles_x.min(width) as usize,
            tiles_y: tiles_y.min(height) as usize,
            clip_limit,
            bins: bins as usize,
            max_value: max_value as usize,
        })
    }
}
pub fn clahe(
    ctx: Ctx<'_>,
    dst: &mut [u16],
    src: &[u16],
    width: i64,
    height: i64,
    opts: &CLAHEOptions,
) -> Result<(), FilterError> {
    clahe_impl(ctx, dst, src, width, height, opts)
}
#[doc(hidden)]
pub fn clahe_in_place(
    ctx: Ctx<'_>,
    buf: &mut [u16],
    width: i64,
    height: i64,
    opts: &CLAHEOptions,
) -> Result<(), FilterError> {
    let src = buf.to_vec();
    if width <= 0 || height <= 0 {
        return Err(invalid(format!("size {width}×{height}")));
    }
    let n = width.wrapping_mul(height);
    if n / width != height || src.len() as i64 != n {
        return Err(invalid(format!(
            "buffers of {} and {} values for {width}×{height}",
            buf.len(),
            src.len()
        )));
    }
    let cfg = opts.resolve(width, height)?;
    ctx_err(ctx)?;
    let model = build_model(ctx, &src, width as usize, height as usize, cfg)?;
    if model.constant {
        return ctx_err(ctx);
    }
    model.apply(ctx, buf, &src)
}
fn clahe_impl(
    ctx: Ctx<'_>,
    dst: &mut [u16],
    src: &[u16],
    width: i64,
    height: i64,
    opts: &CLAHEOptions,
) -> Result<(), FilterError> {
    if width <= 0 || height <= 0 {
        return Err(invalid(format!("size {width}×{height}")));
    }
    let n = width.wrapping_mul(height);
    if n / width != height || src.len() as i64 != n || dst.len() as i64 != n {
        return Err(invalid(format!(
            "buffers of {} and {} values for {width}×{height}",
            dst.len(),
            src.len()
        )));
    }
    let cfg = opts.resolve(width, height)?;
    ctx_err(ctx)?;
    let model = build_model(ctx, src, width as usize, height as usize, cfg)?;
    if model.constant {
        dst.copy_from_slice(src);
        return ctx_err(ctx);
    }
    model.apply(ctx, dst, src)
}
pub fn clahe_gray16(
    ctx: Ctx<'_>,
    img: Option<&Gray16Image>,
    opts: &CLAHEOptions,
) -> Result<Gray16Image, FilterError> {
    let Some(img) = img else {
        return Err(invalid("nil image"));
    };
    let (w, h) = (img.width, img.height);
    let mut values = vec![0u16; w * h];
    for y in 0..h {
        ctx_err(ctx)?;
        for x in 0..w {
            let i = 2 * (y * w + x);
            values[y * w + x] = u16::from(img.pixels[i]) << 8 | u16::from(img.pixels[i + 1]);
        }
    }
    ctx_err(ctx)?;
    clahe_in_place(ctx, &mut values, w as i64, h as i64, opts)?;
    let mut pixels = vec![0u8; 2 * w * h];
    for (i, v) in values.iter().enumerate() {
        pixels[2 * i] = (v >> 8) as u8;
        pixels[2 * i + 1] = *v as u8;
    }
    ctx_err(ctx)?;
    Ok(Gray16Image {
        width: w,
        height: h,
        pixels,
    })
}
pub fn clahe_gray(
    ctx: Ctx<'_>,
    img: Option<&GrayImage>,
    opts: &CLAHEOptions,
) -> Result<GrayImage, FilterError> {
    let Some(img) = img else {
        return Err(invalid("nil image"));
    };
    if opts.bit_depth != 0 && opts.bit_depth != 8 {
        return Err(invalid(format!(
            "BitDepth={} for an 8-bit image",
            opts.bit_depth
        )));
    }
    let mut o = *opts;
    o.bit_depth = 8;
    let (w, h) = (img.width, img.height);
    let mut values: Vec<u16> = img.pixels[..w * h].iter().map(|p| u16::from(*p)).collect();
    clahe_in_place(ctx, &mut values, w as i64, h as i64, &o)?;
    Ok(GrayImage {
        width: w,
        height: h,
        pixels: values.iter().map(|v| *v as u8).collect(),
    })
}
struct ClaheModel {
    width: usize,
    height: usize,
    cfg: ClaheConfig,
    col_start: Vec<usize>,
    row_start: Vec<usize>,
    maps: Vec<Vec<f32>>,
    value_bin: Vec<u16>,
    value_frac: Vec<f32>,
    constant: bool,
}
fn build_model(
    ctx: Ctx<'_>,
    src: &[u16],
    width: usize,
    height: usize,
    cfg: ClaheConfig,
) -> Result<ClaheModel, FilterError> {
    let mut m = ClaheModel {
        width,
        height,
        cfg,
        col_start: tile_starts(width, cfg.tiles_x),
        row_start: tile_starts(height, cfg.tiles_y),
        maps: vec![Vec::new(); cfg.tiles_x * cfg.tiles_y],
        value_bin: Vec::new(),
        value_frac: Vec::new(),
        constant: false,
    };
    let levels = cfg.max_value + 1;
    m.value_bin = vec![0; levels];
    m.value_frac = vec![0.0; levels];
    for v in 0..levels {
        let pos = (v as f64 + 0.5) * cfg.bins as f64 / levels as f64;
        let mut bin = pos as usize;
        if bin >= cfg.bins {
            bin = cfg.bins - 1;
        }
        m.value_bin[v] = bin as u16;
        m.value_frac[v] = (pos - bin as f64) as f32;
    }
    let tiles = m.maps.len();
    let mut ranges = vec![(0usize, 0usize); tiles];
    let mut hist = vec![0u32; cfg.bins];
    for t in 0..tiles {
        ctx_err(ctx)?;
        let (tx, ty) = (t % cfg.tiles_x, t / cfg.tiles_x);
        let (x0, x1) = (m.col_start[tx], m.col_start[tx + 1]);
        let (y0, y1) = (m.row_start[ty], m.row_start[ty + 1]);
        hist.iter_mut().for_each(|h| *h = 0);
        let (mut lo, mut hi) = (cfg.max_value, 0usize);
        for y in y0..y1 {
            for v in &src[y * width + x0..y * width + x1] {
                let value = (*v as usize).min(cfg.max_value);
                lo = lo.min(value);
                hi = hi.max(value);
                hist[m.value_bin[value] as usize] += 1;
            }
        }
        ranges[t] = (lo, hi);
        let pixels = (x1 - x0) * (y1 - y0);
        clip_histogram(&mut hist, clip_count(cfg.clip_limit, pixels, cfg.bins));
        m.maps[t] = cumulative_map(&hist, pixels, cfg.max_value);
    }
    ctx_err(ctx)?;
    let (mut lo, mut hi) = ranges[0];
    for r in &ranges[1..] {
        lo = lo.min(r.0);
        hi = hi.max(r.1);
    }
    m.constant = lo == hi;
    Ok(m)
}
fn tile_starts(n: usize, count: usize) -> Vec<usize> {
    (0..=count).map(|i| i * n / count).collect()
}
fn clip_count(clip_limit: f64, pixels: usize, bins: usize) -> u32 {
    let floor = (pixels + bins - 1) / bins;
    if clip_limit == f64::INFINITY {
        return pixels as u32;
    }
    let mut limit = fmath::f64_to_i64(clip_limit * pixels as f64 / bins as f64);
    if limit < floor as i64 {
        limit = floor as i64;
    }
    if limit > pixels as i64 {
        limit = pixels as i64;
    }
    limit as u32
}
fn clip_histogram(hist: &mut [u32], limit: u32) {
    let mut excess: u64 = 0;
    for h in hist.iter_mut() {
        if *h > limit {
            excess += u64::from(*h - limit);
            *h = limit;
        }
    }
    while excess > 0 {
        let below = hist.iter().filter(|h| **h < limit).count() as u64;
        if below == 0 {
            return;
        }
        let increment = excess / below;
        if increment == 0 {
            let step = below / excess;
            let mut seen: u64 = 0;
            for h in hist.iter_mut() {
                if *h >= limit {
                    continue;
                }
                if seen.is_multiple_of(step) && excess > 0 {
                    *h += 1;
                    excess -= 1;
                }
                seen += 1;
            }
            continue;
        }
        for h in hist.iter_mut() {
            if *h >= limit {
                continue;
            }
            let add = increment.min(u64::from(limit - *h));
            *h += add as u32;
            excess -= add;
        }
    }
}
fn cumulative_map(hist: &[u32], pixels: usize, max_value: usize) -> Vec<f32> {
    let mut out = vec![0f32; hist.len() + 1];
    let scale = max_value as f64 / pixels as f64;
    let mut sum: u64 = 0;
    for (i, h) in hist.iter().enumerate() {
        sum += u64::from(*h);
        out[i + 1] = fmath::min(sum as f64 * scale, max_value as f64) as f32;
    }
    out
}
fn axis_weights(starts: &[usize]) -> (Vec<usize>, Vec<usize>, Vec<f32>) {
    let tiles = starts.len() - 1;
    let size = starts[tiles];
    let centres: Vec<f64> = (0..tiles)
        .map(|i| (starts[i] + starts[i + 1]) as f64 - 1.0)
        .map(|s| s / 2.0)
        .collect();
    let (mut lo, mut hi, mut w) = (vec![0; size], vec![0; size], vec![0f32; size]);
    let mut t = 0;
    for p in 0..size {
        let pos = p as f64;
        while t + 1 < tiles && pos >= centres[t + 1] {
            t += 1;
        }
        if pos <= centres[0] {
            lo[p] = 0;
            hi[p] = 0;
        } else if t + 1 >= tiles {
            lo[p] = tiles - 1;
            hi[p] = tiles - 1;
        } else {
            lo[p] = t;
            hi[p] = t + 1;
            w[p] = ((pos - centres[t]) / (centres[t + 1] - centres[t])) as f32;
        }
    }
    (lo, hi, w)
}
impl ClaheModel {
    fn apply(&self, ctx: Ctx<'_>, dst: &mut [u16], src: &[u16]) -> Result<(), FilterError> {
        let (col_lo, col_hi, col_w) = axis_weights(&self.col_start);
        let (row_lo, row_hi, row_w) = axis_weights(&self.row_start);
        let mut runs = Vec::new();
        let mut x = 0;
        while x < self.width {
            let mut end = x + 1;
            while end < self.width && col_lo[end] == col_lo[x] && col_hi[end] == col_hi[x] {
                end += 1;
            }
            runs.push((x, end, col_lo[x], col_hi[x]));
            x = end;
        }
        const ROWS_PER_CHUNK: usize = 16;
        let chunks = self.height.div_ceil(ROWS_PER_CHUNK);
        let max_value = self.cfg.max_value as f32;
        let tiles_x = self.cfg.tiles_x;
        for chunk in 0..chunks {
            ctx_err(ctx)?;
            let y_end = ((chunk + 1) * ROWS_PER_CHUNK).min(self.height);
            for y in chunk * ROWS_PER_CHUNK..y_end {
                let wy = row_w[y];
                let (top, bottom) = (row_lo[y] * tiles_x, row_hi[y] * tiles_x);
                let input = &src[y * self.width..(y + 1) * self.width];
                let out = &mut dst[y * self.width..(y + 1) * self.width];
                for &(x0, x1, rlo, rhi) in &runs {
                    let (m00, m01) = (&self.maps[top + rlo], &self.maps[top + rhi]);
                    let (m10, m11) = (&self.maps[bottom + rlo], &self.maps[bottom + rhi]);
                    for x in x0..x1 {
                        let value = (input[x] as usize).min(self.cfg.max_value);
                        let b = self.value_bin[value] as usize;
                        let f = self.value_frac[value];
                        let v00 = m00[b] + f * (m00[b + 1] - m00[b]);
                        let v01 = m01[b] + f * (m01[b + 1] - m01[b]);
                        let v10 = m10[b] + f * (m10[b + 1] - m10[b]);
                        let v11 = m11[b] + f * (m11[b + 1] - m11[b]);
                        let wx = col_w[x];
                        let top_value = v00 + wx * (v01 - v00);
                        let bottom_value = v10 + wx * (v11 - v10);
                        let mut v = top_value + wy * (bottom_value - top_value);
                        if v < 0.0 {
                            v = 0.0;
                        } else if v > max_value {
                            v = max_value;
                        }
                        out[x] = (v + 0.5) as u16;
                    }
                }
            }
        }
        ctx_err(ctx)
    }
}
