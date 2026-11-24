#![forbid(unsafe_code)]
use super::error::RenderError;
use super::parallel::{Cancellation, ctx_err};
use super::source_convolution::PyramidImage;
use crate::codec::jpeg::stdjpeg;
use crate::roi::pdqsort_by::sort_slice;
use std::collections::HashMap;
use std::io::{Seek, SeekFrom, Write};
pub const MAX_GIF_COLORS: usize = 256;
const GIF_PALETTE_SAMPLES: i64 = 1 << 19;
const GIF_MAX_DIMENSION: i64 = (1 << 16) - 1;
pub const ERR_GIF_FRAME_SIZE: &str = "render: GIF frame size differs from the movie";
pub const ERR_GIF_CLOSED: &str = "render: GIF writer is closed";
fn err(text: impl Into<String>) -> RenderError {
    RenderError::other(text)
}
pub type PaletteColor = [u8; 3];
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paletted {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    pub palette: Vec<PaletteColor>,
}
pub(crate) fn to_opaque_rgba(img: &PyramidImage) -> Rgba {
    let (width, height) = img.size();
    let pixels = match img {
        PyramidImage::Rgba { pixels, .. } => pixels.clone(),
        PyramidImage::Gray { pixels, .. } => {
            pixels.iter().flat_map(|v| [*v, *v, *v, 255]).collect()
        }
        PyramidImage::Gray16 { pixels, .. } => pixels
            .iter()
            .flat_map(|v| {
                let g = (v >> 8) as u8;
                [g, g, g, 255]
            })
            .collect(),
        PyramidImage::Nrgba { pixels, .. } => pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|s| {
                let sa = u32::from(s[3]) * 0x101;
                let c = |v: u8| ((u32::from(v) * sa / 0xff) >> 8) as u8;
                [c(s[0]), c(s[1]), c(s[2]), (sa >> 8) as u8]
            })
            .collect(),
    };
    Rgba {
        width,
        height,
        pixels,
    }
}
#[derive(Debug, Clone, Copy)]
struct ColorCount {
    c: [u8; 3],
    n: u32,
}
#[derive(Debug, Clone, Copy, Default)]
struct CutBox {
    lo: usize,
    hi: usize,
    score: f64,
    axis: usize,
    mean: [f64; 3],
}
fn make_cut_box(entries: &[ColorCount], lo: usize, hi: usize) -> CutBox {
    let mut n = 0.0;
    let (mut sum, mut sum2) = ([0f64; 3], [0f64; 3]);
    for e in &entries[lo..hi] {
        let w = f64::from(e.n);
        n += w;
        for c in 0..3 {
            let v = f64::from(e.c[c]);
            sum[c] += w * v;
            sum2[c] += w * v * v;
        }
    }
    let mut b = CutBox {
        lo,
        hi,
        ..Default::default()
    };
    let mut best_var = -1.0;
    for c in 0..3 {
        b.mean[c] = sum[c] / n;
        let mut sse = sum2[c] - sum[c] * sum[c] / n;
        if sse < 0.0 {
            sse = 0.0;
        }
        b.score += sse;
        if sse > best_var {
            best_var = sse;
            b.axis = c;
        }
    }
    b
}
fn sample_histogram(images: &[&Rgba]) -> Vec<ColorCount> {
    let pixels: i64 = images
        .iter()
        .map(|i| i.width as i64 * i.height as i64)
        .sum();
    let mut step = 1i64;
    while pixels / (step * step) > GIF_PALETTE_SAMPLES {
        step += 1;
    }
    let mut counts: HashMap<u32, u32> = HashMap::new();
    for img in images {
        let mut y = 0;
        while y < img.height as i64 {
            let row = &img.pixels[y as usize * img.width * 4..];
            let mut x = (y / step * 7) % step;
            while x < img.width as i64 {
                let i = x as usize * 4;
                let key =
                    u32::from(row[i]) << 16 | u32::from(row[i + 1]) << 8 | u32::from(row[i + 2]);
                *counts.entry(key).or_insert(0) += 1;
                x += step;
            }
            y += step;
        }
    }
    let mut entries: Vec<ColorCount> = counts
        .into_iter()
        .map(|(key, n)| ColorCount {
            c: [(key >> 16) as u8, (key >> 8) as u8, key as u8],
            n,
        })
        .collect();
    entries.sort_by_key(|e| u32::from(e.c[0]) << 16 | u32::from(e.c[1]) << 8 | u32::from(e.c[2]));
    entries
}
fn round_byte(v: f64) -> u8 {
    if v <= 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        (v + 0.5) as u8
    }
}
fn median_cut_rgba(images: &[&Rgba], max_colors: usize) -> Vec<PaletteColor> {
    let mut entries = sample_histogram(images);
    if entries.is_empty() {
        return vec![[0, 0, 0]];
    }
    if entries.len() <= max_colors {
        return entries.iter().map(|e| e.c).collect();
    }
    let mut boxes = vec![make_cut_box(&entries, 0, entries.len())];
    while boxes.len() < max_colors {
        let mut best: Option<usize> = None;
        for (i, b) in boxes.iter().enumerate() {
            if b.hi - b.lo >= 2 && b.score > 0.0 && best.is_none_or(|k| b.score > boxes[k].score) {
                best = Some(i);
            }
        }
        let Some(best) = best else { break };
        let b = boxes[best];
        let axis = b.axis;
        let span = &mut entries[b.lo..b.hi];
        sort_slice(span, &|p: &ColorCount, q: &ColorCount| {
            p.c[axis] < q.c[axis]
        });
        let total: u64 = span.iter().map(|e| u64::from(e.n)).sum();
        let mut acc = 0u64;
        let mut cut = 1;
        for (i, e) in span.iter().enumerate() {
            acc += u64::from(e.n);
            if acc * 2 >= total {
                cut = i + 1;
                break;
            }
        }
        if cut >= span.len() {
            cut = span.len() - 1;
        }
        let mid = b.lo + cut;
        boxes[best] = make_cut_box(&entries, b.lo, mid);
        boxes.push(make_cut_box(&entries, mid, b.hi));
    }
    boxes
        .iter()
        .map(|b| {
            [
                round_byte(b.mean[0]),
                round_byte(b.mean[1]),
                round_byte(b.mean[2]),
            ]
        })
        .collect()
}
pub fn median_cut_palette(images: &[PyramidImage], max_colors: i64) -> Vec<PaletteColor> {
    let rgba: Vec<Rgba> = images
        .iter()
        .filter(|i| {
            let (w, h) = i.size();
            w > 0 && h > 0
        })
        .map(to_opaque_rgba)
        .collect();
    let refs: Vec<&Rgba> = rgba.iter().collect();
    let max_colors = max_colors.clamp(1, MAX_GIF_COLORS as i64) as usize;
    median_cut_rgba(&refs, max_colors)
}
struct PaletteSearch {
    rgb: Vec<[i32; 3]>,
    index: Vec<u8>,
}
impl PaletteSearch {
    fn new(p: &[PaletteColor]) -> PaletteSearch {
        let values: Vec<[i32; 3]> = p
            .iter()
            .map(|c| [i32::from(c[0]), i32::from(c[1]), i32::from(c[2])])
            .collect();
        let mut order: Vec<usize> = (0..p.len()).collect();
        order.sort_by_key(|i| values[*i][1]);
        PaletteSearch {
            rgb: order.iter().map(|i| values[*i]).collect(),
            index: order.iter().map(|i| *i as u8).collect(),
        }
    }
    fn nearest(&self, r: i32, g: i32, b: i32) -> usize {
        let start = self.rgb.partition_point(|c| c[1] < g);
        let (mut best, mut best_dist) = (0usize, 1i32 << 30);
        let mut visit = |k: usize| {
            let c = self.rgb[k];
            let dg = c[1] - g;
            if dg * dg >= best_dist {
                return false;
            }
            let (dr, db) = (c[0] - r, c[2] - b);
            let d = dr * dr + dg * dg + db * db;
            if d < best_dist {
                best = k;
                best_dist = d;
            }
            true
        };
        let n = self.rgb.len() as i64;
        let (mut up, mut down) = (start as i64, start as i64 - 1);
        while up < n || down >= 0 {
            if up < n {
                if visit(up as usize) {
                    up += 1;
                } else {
                    up = n;
                }
            }
            if down >= 0 {
                if visit(down as usize) {
                    down -= 1;
                } else {
                    down = -1;
                }
            }
        }
        best
    }
}
fn quantize_rgba(
    ctx: Option<&dyn Cancellation>,
    src: &Rgba,
    p: &[PaletteColor],
    dither: bool,
) -> Result<Paletted, RenderError> {
    let (w, h) = (src.width, src.height);
    let mut out = vec![0u8; w * h];
    let search = PaletteSearch::new(p);
    if !dither {
        for y in 0..h {
            if let Some(e) = ctx_err(ctx) {
                return Err(e);
            }
            for x in 0..w {
                let i = (y * w + x) * 4;
                let k = search.nearest(
                    i32::from(src.pixels[i]),
                    i32::from(src.pixels[i + 1]),
                    i32::from(src.pixels[i + 2]),
                );
                out[y * w + x] = search.index[k];
            }
        }
    } else {
        let mut cur = vec![[0f32; 3]; w + 2];
        let mut next = vec![[0f32; 3]; w + 2];
        for y in 0..h {
            if let Some(e) = ctx_err(ctx) {
                return Err(e);
            }
            let ltr = y % 2 == 0;
            for n in 0..w {
                let (x, dir): (usize, i64) = if ltr { (n, 1) } else { (w - 1 - n, -1) };
                let i = (y * w + x) * 4;
                let mut want = [0i32; 3];
                for c in 0..3 {
                    let v = f32::from(src.pixels[i + c]) + cur[x + 1][c];
                    want[c] = ((v + 0.5) as i32).clamp(0, 255);
                }
                let k = search.nearest(want[0], want[1], want[2]);
                out[y * w + x] = search.index[k];
                let got = search.rgb[k];
                let fwd = (x as i64 + 1 + dir) as usize;
                let back = (x as i64 + 1 - dir) as usize;
                for c in 0..3 {
                    let e = (want[c] - got[c]) as f32;
                    cur[fwd][c] += e * 7.0 / 16.0;
                    next[back][c] += e * 3.0 / 16.0;
                    next[x + 1][c] += e * 5.0 / 16.0;
                    next[fwd][c] += e * 1.0 / 16.0;
                }
            }
            std::mem::swap(&mut cur, &mut next);
            next.iter_mut().for_each(|v| *v = [0.0; 3]);
        }
    }
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    Ok(Paletted {
        width: w,
        height: h,
        pixels: out,
        palette: p.to_vec(),
    })
}
pub fn quantize_floyd_steinberg(img: &PyramidImage, p: &[PaletteColor]) -> Paletted {
    quantize_rgba(None, &to_opaque_rgba(img), p, true).unwrap_or(Paletted {
        width: 0,
        height: 0,
        pixels: Vec::new(),
        palette: p.to_vec(),
    })
}
const LZW_MAX_CODE: u32 = (1 << 12) - 1;
const LZW_INVALID_CODE: u32 = u32::MAX;
const LZW_TABLE_SIZE: usize = 4 * (1 << 12);
const LZW_TABLE_MASK: u32 = (LZW_TABLE_SIZE - 1) as u32;
struct LzwWriter<'a> {
    out: &'a mut Vec<u8>,
    lit_width: u32,
    n_bits: u32,
    width: u32,
    bits: u32,
    hi: u32,
    overflow: u32,
    saved_code: u32,
    table: Vec<u32>,
}
impl<'a> LzwWriter<'a> {
    fn new(out: &'a mut Vec<u8>, lit_width: u32) -> LzwWriter<'a> {
        LzwWriter {
            out,
            lit_width,
            n_bits: 0,
            width: 1 + lit_width,
            bits: 0,
            hi: (1 << lit_width) + 1,
            overflow: 1 << (lit_width + 1),
            saved_code: LZW_INVALID_CODE,
            table: vec![0; LZW_TABLE_SIZE],
        }
    }
    fn write_code(&mut self, c: u32) {
        self.bits |= c << self.n_bits;
        self.n_bits += self.width;
        while self.n_bits >= 8 {
            self.out.push(self.bits as u8);
            self.bits >>= 8;
            self.n_bits -= 8;
        }
    }
    fn inc_hi(&mut self) -> bool {
        self.hi += 1;
        if self.hi == self.overflow {
            self.width += 1;
            self.overflow <<= 1;
        }
        if self.hi == LZW_MAX_CODE {
            let clear = 1u32 << self.lit_width;
            self.write_code(clear);
            self.width = self.lit_width + 1;
            self.hi = clear + 1;
            self.overflow = clear << 1;
            self.table.iter_mut().for_each(|t| *t = 0);
            return false;
        }
        true
    }
    fn write(&mut self, p: &[u8]) {
        if p.is_empty() {
            return;
        }
        let mut p = p;
        let mut code = self.saved_code;
        if code == LZW_INVALID_CODE {
            self.write_code(1 << self.lit_width);
            code = u32::from(p[0]);
            p = &p[1..];
        }
        'bytes: for &x in p {
            let literal = u32::from(x);
            let key = code << 8 | literal;
            let mut hash = (key >> 12 ^ key) & LZW_TABLE_MASK;
            let (mut h, mut t) = (hash, self.table[hash as usize]);
            while t != 0 {
                if key == t >> 12 {
                    code = t & LZW_MAX_CODE;
                    continue 'bytes;
                }
                h = (h + 1) & LZW_TABLE_MASK;
                t = self.table[h as usize];
            }
            self.write_code(code);
            code = literal;
            if !self.inc_hi() {
                continue;
            }
            loop {
                if self.table[hash as usize] == 0 {
                    self.table[hash as usize] = (key << 12) | self.hi;
                    break;
                }
                hash = (hash + 1) & LZW_TABLE_MASK;
            }
        }
        self.saved_code = code;
    }
    fn close(mut self) {
        if self.saved_code != LZW_INVALID_CODE {
            let code = self.saved_code;
            self.write_code(code);
            self.inc_hi();
        } else {
            self.write_code(1 << self.lit_width);
        }
        let eof = (1u32 << self.lit_width) + 1;
        self.write_code(eof);
        if self.n_bits > 0 {
            self.out.push(self.bits as u8);
        }
    }
}
fn gif_image_data(out: &mut Vec<u8>, rows: &[&[u8]], lit_width: u32) {
    let mut stream = Vec::new();
    let mut lzw = LzwWriter::new(&mut stream, lit_width);
    for row in rows {
        lzw.write(row);
    }
    lzw.close();
    for block in stream.chunks(255) {
        out.push(block.len() as u8);
        out.extend_from_slice(block);
    }
    out.push(0);
}
fn gif_log2(x: usize) -> i64 {
    for (i, v) in [2usize, 4, 8, 16, 32, 64, 128, 256].iter().enumerate() {
        if x <= *v {
            return i as i64;
        }
    }
    -1
}
fn color_table(p: &[PaletteColor], size: i64) -> Vec<u8> {
    let n = 1usize << (size + 1);
    let mut out = vec![0u8; 3 * n.max(p.len())];
    for (i, c) in p.iter().enumerate() {
        out[3 * i..3 * i + 3].copy_from_slice(c);
    }
    out.truncate(3 * n);
    out
}
fn gif_encode_all(
    out: &mut Vec<u8>,
    frames: &[Paletted],
    delays: &[i64],
    loop_count: i64,
    global: Option<(&[PaletteColor], usize, usize)>,
) -> Result<(), RenderError> {
    let base = out.len();
    let mut flushed = base;
    out.extend_from_slice(b"GIF89a");
    let (width, height) = match global {
        Some((_, w, h)) => (w, h),
        None => (frames[0].width, frames[0].height),
    };
    out.extend_from_slice(&(width as u16).to_le_bytes());
    out.extend_from_slice(&(height as u16).to_le_bytes());
    let mut global_table = Vec::new();
    match global {
        Some((p, _, _)) if !p.is_empty() => {
            let size = gif_log2(p.len());
            out.extend_from_slice(&[0x80 | size as u8, 0, 0]);
            global_table = color_table(p, size);
            out.extend_from_slice(&global_table);
        }
        _ => out.extend_from_slice(&[0, 0, 0]),
    }
    if frames.len() > 1 && loop_count >= 0 {
        out.extend_from_slice(&[0x21, 0xff, 0x0b]);
        out.extend_from_slice(b"NETSCAPE2.0");
        out.extend_from_slice(&[0x03, 0x01]);
        out.extend_from_slice(&(loop_count as u16).to_le_bytes());
        out.push(0);
    }
    for (pm, delay) in frames.iter().zip(delays) {
        let fail = if pm.palette.is_empty() {
            Some("gif: cannot encode image block with empty palette")
        } else if pm.width >= 1 << 16 || pm.height >= 1 << 16 {
            Some("gif: image block is too large to encode")
        } else if pm.width > width || pm.height > height {
            Some("gif: image block is out of bounds")
        } else {
            None
        };
        if let Some(msg) = fail {
            out.truncate(flushed);
            return Err(err(msg));
        }
        if *delay > 0 {
            out.extend_from_slice(&[0x21, 0xf9, 0x04, 0x00]);
            out.extend_from_slice(&(*delay as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
        }
        out.push(0x2c);
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(pm.width as u16).to_le_bytes());
        out.extend_from_slice(&(pm.height as u16).to_le_bytes());
        let padded = gif_log2(pm.palette.len());
        let shares_global = global.is_some();
        if shares_global {
            out.push(0);
        } else {
            let local = color_table(&pm.palette, padded);
            let n = 3 * pm.palette.len();
            if local.len() <= global_table.len() && global_table[..n] == local[..n] {
                out.push(0);
            } else {
                out.push(0x80 | padded as u8);
                out.extend_from_slice(&local);
            }
        }
        let lit_width = (padded + 1).max(2) as u32;
        out.push(lit_width as u8);
        let rows: Vec<&[u8]> = pm.pixels.chunks(pm.width.max(1)).collect();
        gif_image_data(out, &rows, lit_width);
        flushed = out.len();
    }
    out.push(0x3b);
    Ok(())
}
pub const GIF_PALETTE_GLOBAL: i64 = 0;
pub const GIF_PALETTE_PER_FRAME: i64 = 1;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GifOptions {
    pub delay_cs: i64,
    pub loop_count: i64,
    pub colors: i64,
    pub palette: i64,
    pub no_dither: bool,
}
pub fn encode_adaptive_gif(
    frames: &[Option<PyramidImage>],
    opts: GifOptions,
) -> Result<Vec<u8>, RenderError> {
    let mut out = Vec::new();
    encode_adaptive_gif_into(&mut out, frames, opts)?;
    Ok(out)
}
#[doc(hidden)]
pub fn encode_adaptive_gif_into(
    out: &mut Vec<u8>,
    frames: &[Option<PyramidImage>],
    opts: GifOptions,
) -> Result<(), RenderError> {
    if frames.is_empty() {
        return Err(err("render: no frames to encode"));
    }
    let colors = if opts.colors == 0 {
        MAX_GIF_COLORS as i64
    } else {
        opts.colors
    };
    if colors < 2 || colors > MAX_GIF_COLORS as i64 {
        return Err(err(format!(
            "render: GIF palette size {} outside 2..{MAX_GIF_COLORS}",
            opts.colors
        )));
    }
    let delay = opts.delay_cs.max(1);
    let mut rgba = Vec::with_capacity(frames.len());
    for (i, f) in frames.iter().enumerate() {
        match f {
            Some(img) if img.size().0 > 0 && img.size().1 > 0 => rgba.push(to_opaque_rgba(img)),
            _ => return Err(err(format!("render: empty GIF frame {}", i + 1))),
        }
    }
    let colors = colors as usize;
    let global = (opts.palette != GIF_PALETTE_PER_FRAME)
        .then(|| median_cut_rgba(&rgba.iter().collect::<Vec<_>>(), colors));
    let mut paletted = Vec::with_capacity(rgba.len());
    for frame in &rgba {
        let p = match &global {
            Some(g) => g.clone(),
            None => median_cut_rgba(&[frame], colors),
        };
        paletted.push(quantize_rgba(None, frame, &p, !opts.no_dither)?);
    }
    let delays = vec![delay; paletted.len()];
    let size = global.as_ref().map(|g| {
        let w = rgba.iter().map(|f| f.width).max().unwrap_or(0);
        let h = rgba.iter().map(|f| f.height).max().unwrap_or(0);
        (g.as_slice(), w, h)
    });
    gif_encode_all(out, &paletted, &delays, opts.loop_count, size)
}
pub fn encode_gif(frames: &[Option<PyramidImage>], delay_cs: i64) -> Result<Vec<u8>, RenderError> {
    let mut out = Vec::new();
    encode_gif_into(&mut out, frames, delay_cs)?;
    Ok(out)
}
#[doc(hidden)]
pub fn encode_gif_into(
    out: &mut Vec<u8>,
    frames: &[Option<PyramidImage>],
    delay_cs: i64,
) -> Result<(), RenderError> {
    encode_adaptive_gif_into(
        out,
        frames,
        GifOptions {
            delay_cs,
            ..Default::default()
        },
    )
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GifStreamOptions {
    pub palette: Vec<PaletteColor>,
    pub delay_cs: i64,
    pub loop_count: i64,
}
pub struct GifWriter<W: Write> {
    w: BlockWriter<W>,
    opts: GifStreamOptions,
    lit_width: u32,
    width: usize,
    height: usize,
    frames: i64,
    err: Option<RenderError>,
    closed: bool,
}
struct BlockWriter<W: Write> {
    wr: W,
    buf: Vec<u8>,
    err: Option<std::io::ErrorKind>,
}
const GO_BUFIO_SIZE: usize = 4096;
impl<W: Write> BlockWriter<W> {
    fn new(wr: W) -> Self {
        BlockWriter {
            wr,
            buf: Vec::with_capacity(GO_BUFIO_SIZE),
            err: None,
        }
    }
    fn check(&self) -> std::io::Result<()> {
        match self.err {
            Some(k) => Err(std::io::Error::from(k)),
            None => Ok(()),
        }
    }
    fn available(&self) -> usize {
        GO_BUFIO_SIZE - self.buf.len()
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.check()?;
        if self.buf.is_empty() {
            return Ok(());
        }
        if let Err(e) = self.wr.write_all(&self.buf) {
            self.err = Some(e.kind());
            return Err(e);
        }
        self.buf.clear();
        Ok(())
    }
    fn write(&mut self, mut p: &[u8]) -> std::io::Result<()> {
        while p.len() > self.available() {
            self.check()?;
            if self.buf.is_empty() {
                if let Err(e) = self.wr.write_all(p) {
                    self.err = Some(e.kind());
                    return Err(e);
                }
                return Ok(());
            }
            let n = self.available();
            self.buf.extend_from_slice(&p[..n]);
            p = &p[n..];
            self.flush()?;
        }
        self.check()?;
        self.buf.extend_from_slice(p);
        Ok(())
    }
    fn write_byte(&mut self, c: u8) -> std::io::Result<()> {
        self.check()?;
        if self.available() == 0 {
            self.flush()?;
        }
        self.buf.push(c);
        Ok(())
    }
}
fn io_err(e: std::io::Error) -> RenderError {
    err(e.to_string())
}
impl<W: Write> GifWriter<W> {
    pub fn new(w: W, opts: GifStreamOptions) -> Result<GifWriter<W>, RenderError> {
        let mut opts = opts;
        let n = opts.palette.len();
        if !(2..=MAX_GIF_COLORS).contains(&n) {
            return Err(err(format!(
                "render: GIF palette has {n} colours, want 2..{MAX_GIF_COLORS}"
            )));
        }
        opts.delay_cs = opts.delay_cs.clamp(1, (1 << 16) - 1);
        if opts.loop_count < -1 || opts.loop_count > (1 << 16) - 1 {
            return Err(err(format!(
                "render: GIF loop count {} out of range",
                opts.loop_count
            )));
        }
        let lit_width = (usize::BITS - (n - 1).leading_zeros()).max(2);
        Ok(GifWriter {
            w: BlockWriter::new(w),
            opts,
            lit_width,
            width: 0,
            height: 0,
            frames: 0,
            err: None,
            closed: false,
        })
    }
    pub fn frames(&self) -> i64 {
        self.frames
    }
    pub fn write_frame(&mut self, img: Option<&Paletted>) -> Result<(), RenderError> {
        let delay = self.opts.delay_cs;
        self.write_frame_delay(img, delay)
    }
    pub fn write_frame_delay(
        &mut self,
        img: Option<&Paletted>,
        delay_cs: i64,
    ) -> Result<(), RenderError> {
        if self.closed {
            return Err(err(ERR_GIF_CLOSED));
        }
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        let Some(img) = img.filter(|i| i.width > 0 && i.height > 0) else {
            return Err(err(format!("render: empty GIF frame {}", self.frames + 1)));
        };
        if self.frames == 0 {
            if img.width as i64 > GIF_MAX_DIMENSION || img.height as i64 > GIF_MAX_DIMENSION {
                return Err(err(format!(
                    "render: GIF frame {}x{} exceeds {GIF_MAX_DIMENSION} pixels per side",
                    img.width, img.height
                )));
            }
            self.width = img.width;
            self.height = img.height;
        } else if img.width != self.width || img.height != self.height {
            return Err(err(format!(
                "{ERR_GIF_FRAME_SIZE}: frame {} is {}x{}, movie is {}x{}",
                self.frames + 1,
                img.width,
                img.height,
                self.width,
                self.height
            )));
        }
        let limit = (self.opts.palette.len() - 1) as u8;
        if let Some(index) = img.pixels.iter().find(|i| **i > limit) {
            return Err(err(format!(
                "render: GIF frame {} uses colour index {index} outside the {}-colour palette",
                self.frames + 1,
                self.opts.palette.len()
            )));
        }
        let mut b = Vec::new();
        if self.frames == 0 {
            b.extend_from_slice(b"GIF89a");
            b.extend_from_slice(&(self.width as u16).to_le_bytes());
            b.extend_from_slice(&(self.height as u16).to_le_bytes());
            b.extend_from_slice(&[0x80 | 0x70 | (self.lit_width - 1) as u8, 0, 0]);
            for i in 0..1usize << self.lit_width {
                let c = self.opts.palette.get(i).copied().unwrap_or([0, 0, 0]);
                b.extend_from_slice(&c);
            }
            if self.opts.loop_count >= 0 {
                b.extend_from_slice(&[0x21, 0xff, 0x0b]);
                b.extend_from_slice(b"NETSCAPE2.0");
                b.extend_from_slice(&[0x03, 0x01]);
                b.extend_from_slice(&(self.opts.loop_count as u16).to_le_bytes());
                b.push(0);
            }
        }
        if !b.is_empty()
            && let Err(e) = self.w.write(&b)
        {
            let e = io_err(e);
            self.err = Some(e.clone());
            return Err(e);
        }
        let delay = delay_cs.clamp(1, (1 << 16) - 1);
        let mut b = vec![0x21, 0xf9, 0x04, 0x00];
        b.extend_from_slice(&(delay as u16).to_le_bytes());
        b.extend_from_slice(&[0, 0, 0x2c, 0, 0, 0, 0]);
        b.extend_from_slice(&(self.width as u16).to_le_bytes());
        b.extend_from_slice(&(self.height as u16).to_le_bytes());
        b.extend_from_slice(&[0x00, self.lit_width as u8]);
        let rows: Vec<&[u8]> = img.pixels.chunks(self.width).collect();
        let mut stream = Vec::new();
        let mut lzw = LzwWriter::new(&mut stream, self.lit_width);
        for row in &rows {
            lzw.write(row);
        }
        lzw.close();
        let mut res = self.w.write(&b);
        for block in stream.chunks(255) {
            if res.is_err() {
                break;
            }
            res = self
                .w
                .write_byte(block.len() as u8)
                .and_then(|_| self.w.write(block));
        }
        if res.is_ok() {
            res = self.w.write_byte(0);
        }
        if let Err(e) = res {
            let e = io_err(e);
            self.err = Some(e.clone());
            return Err(e);
        }
        self.frames += 1;
        Ok(())
    }
    pub fn close(&mut self) -> Result<(), RenderError> {
        if self.closed {
            return Err(err(ERR_GIF_CLOSED));
        }
        self.closed = true;
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if self.frames == 0 {
            return Err(err("render: no frames to encode"));
        }
        self.w.write_byte(0x3b).map_err(io_err)?;
        self.w.flush().map_err(io_err)
    }
    pub fn into_inner(self) -> W {
        self.w.wr
    }
    pub fn get_ref(&self) -> &W {
        &self.w.wr
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdaptiveGifStreamOptions {
    pub gif: GifOptions,
    pub max_frame_pixels: i64,
    pub max_workspace_bytes: i64,
    pub sample_frames: i64,
    pub sample_pixels: i64,
}
pub fn encode_adaptive_gif_producer(
    ctx: Option<&dyn Cancellation>,
    total: i64,
    at: Option<&dyn Fn(i64) -> Result<Option<PyramidImage>, RenderError>>,
    opts: AdaptiveGifStreamOptions,
    mut progress: Option<&mut dyn FnMut(i64, i64)>,
) -> Result<Vec<u8>, RenderError> {
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    let Some(at) = at.filter(|_| (1..=1 << 30).contains(&total)) else {
        return Err(err("render: invalid GIF producer"));
    };
    let mut opts = opts;
    if opts.gif.palette != GIF_PALETTE_GLOBAL {
        return Err(err(
            "render: streaming adaptive GIF requires global palette",
        ));
    }
    let colors = if opts.gif.colors == 0 {
        256
    } else {
        opts.gif.colors
    };
    if !(2..=256).contains(&colors) {
        return Err(err("render: invalid GIF palette size"));
    }
    if opts.max_frame_pixels == 0 {
        opts.max_frame_pixels = 16 << 20;
    }
    if opts.max_workspace_bytes == 0 {
        opts.max_workspace_bytes = 384 << 20;
    }
    if opts.sample_frames == 0 {
        opts.sample_frames = 32;
    }
    if opts.sample_pixels == 0 {
        opts.sample_pixels = GIF_PALETTE_SAMPLES;
    }
    if opts.max_frame_pixels < 1
        || opts.max_workspace_bytes < 65 << 20
        || opts.sample_frames < 1
        || opts.sample_frames > 32
        || opts.sample_pixels < 1
        || opts.sample_pixels > GIF_PALETTE_SAMPLES
    {
        return Err(err("render: invalid GIF memory/sample bounds"));
    }
    let mut size: Option<(i64, i64)> = None;
    let mut frame = |index: i64| -> Result<PyramidImage, RenderError> {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        let img = at(index).map_err(|e| {
            RenderError::new(
                e.kind(),
                format!("render: produce GIF frame {}: {e}", index + 1),
            )
        })?;
        let Some(img) = img.filter(|i| i.size().0 > 0 && i.size().1 > 0) else {
            return Err(err(format!("render: empty GIF frame {}", index + 1)));
        };
        let (w, h) = (img.size().0 as i64, img.size().1 as i64);
        if w > GIF_MAX_DIMENSION || h > GIF_MAX_DIMENSION || w > opts.max_frame_pixels / h {
            return Err(err("render: GIF frame exceeds pixel budget"));
        }
        if (64 << 20) + w * h * 16 + (w + 2) * 24 + (1 << 20) > opts.max_workspace_bytes {
            return Err(err("render: GIF workspace exceeds budget"));
        }
        match size {
            None => size = Some((w, h)),
            Some(s) if s != (w, h) => return Err(err(ERR_GIF_FRAME_SIZE)),
            _ => {}
        }
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        Ok(img)
    };
    let n = total.min(opts.sample_frames).min(opts.sample_pixels);
    let mut samples: Vec<u8> = Vec::new();
    for k in 0..n {
        let index = if n > 1 { k * (total - 1) / (n - 1) } else { 0 };
        let img = frame(index)?;
        let rgba = to_opaque_rgba_16(&img);
        let pixels = (rgba.width * rgba.height) as i64;
        let quota = (opts.sample_pixels / n).min(pixels);
        for j in 0..quota {
            if j % 4096 == 0
                && let Some(e) = ctx_err(ctx)
            {
                return Err(e);
            }
            let pos = j * pixels / quota;
            let x = (pos % rgba.width as i64) as usize;
            let y = (pos / rgba.width as i64) as usize;
            let i = (y * rgba.width + x) * 4;
            samples.extend_from_slice(&[
                rgba.pixels[i],
                rgba.pixels[i + 1],
                rgba.pixels[i + 2],
                255,
            ]);
        }
    }
    let used = samples.len() / 4;
    let sample_image = Rgba {
        width: used,
        height: 1,
        pixels: samples,
    };
    let mut palette = median_cut_rgba(&[&sample_image], colors as usize);
    if palette.len() == 1 {
        palette.push(palette[0]);
    }
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    let mut writer = GifWriter::new(
        Vec::new(),
        GifStreamOptions {
            palette: palette.clone(),
            delay_cs: opts.gif.delay_cs,
            loop_count: opts.gif.loop_count,
        },
    )?;
    for i in 0..total {
        let img = frame(i)?;
        let paletted = quantize_rgba(ctx, &to_opaque_rgba(&img), &palette, !opts.gif.no_dither)?;
        writer.write_frame(Some(&paletted))?;
        if let Some(p) = progress.as_mut() {
            p(i + 1, total);
        }
    }
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    writer.close()?;
    Ok(writer.into_inner())
}
fn to_opaque_rgba_16(img: &PyramidImage) -> Rgba {
    match img {
        PyramidImage::Nrgba {
            width,
            height,
            pixels,
        } => Rgba {
            width: *width,
            height: *height,
            pixels: pixels
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let a = u32::from(p[3]);
                    let c = |v: u8| {
                        let v = u32::from(v);
                        ((((v | v << 8) * a) / 0xff) >> 8) as u8
                    };
                    [c(p[0]), c(p[1]), c(p[2]), p[3]]
                })
                .collect(),
        },
        other => to_opaque_rgba(other),
    }
}
pub const MAX_AVI_BYTES: i64 = 1 << 30;
pub const DEFAULT_AVI_JPEG_QUALITY: i64 = 90;
pub const MAX_AVI_FPS: i64 = 240;
const AVI_HEADER_BYTES: i64 = 224;
const AVI_INDEX_ENTRY_BYTES: i64 = 16;
const AVI_CHUNK_HEADER: i64 = 8;
const AVI_MAX_DIMENSION: usize = 65535;
pub const ERR_AVI_TOO_LARGE: &str = "render: AVI output would exceed the AVI 1.0 size bound";
pub const ERR_AVI_FRAME_SIZE: &str = "render: AVI frame dimensions differ from the first frame";
pub const ERR_AVI_CLOSED: &str = "render: AVI writer is closed";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AviOptions {
    pub fps: i64,
    pub quality: i64,
    pub max_bytes: i64,
}
impl AviOptions {
    fn normalized(mut self) -> Result<AviOptions, RenderError> {
        if self.fps < 1 || self.fps > MAX_AVI_FPS {
            return Err(err(format!(
                "render: AVI frame rate {} outside 1..{MAX_AVI_FPS}",
                self.fps
            )));
        }
        if self.quality == 0 {
            self.quality = DEFAULT_AVI_JPEG_QUALITY;
        }
        if !(1..=100).contains(&self.quality) {
            return Err(err(format!(
                "render: AVI JPEG quality {} outside 1..100",
                self.quality
            )));
        }
        if self.max_bytes <= 0 || self.max_bytes > MAX_AVI_BYTES {
            self.max_bytes = MAX_AVI_BYTES;
        }
        Ok(self)
    }
}
fn pad_even(n: i64) -> i64 {
    n + (n & 1)
}
#[derive(Debug, Clone, Default)]
struct AviLayout {
    width: usize,
    height: usize,
    fps: i64,
    frame_sizes: Vec<u32>,
    movi_bytes: i64,
}
impl AviLayout {
    fn add(&mut self, size: usize) {
        self.frame_sizes.push(size as u32);
        self.movi_bytes += AVI_CHUNK_HEADER + pad_even(size as i64);
    }
    fn projected_size(&self, size: usize) -> i64 {
        let frames = self.frame_sizes.len() as i64 + 1;
        AVI_HEADER_BYTES
            + self.movi_bytes
            + AVI_CHUNK_HEADER
            + pad_even(size as i64)
            + AVI_CHUNK_HEADER
            + frames * AVI_INDEX_ENTRY_BYTES
    }
    fn file_size(&self) -> i64 {
        AVI_HEADER_BYTES
            + self.movi_bytes
            + AVI_CHUNK_HEADER
            + self.frame_sizes.len() as i64 * AVI_INDEX_ENTRY_BYTES
    }
    fn header(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(AVI_HEADER_BYTES as usize);
        let u32le = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_le_bytes());
        let u16le = |b: &mut Vec<u8>, v: u16| b.extend_from_slice(&v.to_le_bytes());
        let frames = self.frame_sizes.len() as u32;
        let max_frame = self.frame_sizes.iter().copied().max().unwrap_or(0);
        let suggested = pad_even(i64::from(max_frame)) + AVI_CHUNK_HEADER;
        let micros = crate::fmath::f64_to_i64((1e6 / self.fps as f64).round()) as u32;
        let max_bytes_per_sec = (suggested as u64 * self.fps as u64).min(u64::from(u32::MAX));
        let (w, h) = (self.width as u32, self.height as u32);
        b.extend_from_slice(b"RIFF");
        u32le(&mut b, (self.file_size() - 8) as u32);
        b.extend_from_slice(b"AVI LIST");
        u32le(&mut b, 4 + (8 + 56) + (8 + 4 + (8 + 56) + (8 + 40)));
        b.extend_from_slice(b"hdrlavih");
        for v in [
            56,
            micros,
            max_bytes_per_sec as u32,
            0,
            0x10 | 0x100,
            frames,
            0,
            1,
            suggested as u32,
            w,
            h,
            0,
            0,
            0,
            0,
        ] {
            u32le(&mut b, v);
        }
        b.extend_from_slice(b"LIST");
        u32le(&mut b, 4 + (8 + 56) + (8 + 40));
        b.extend_from_slice(b"strlstrh");
        u32le(&mut b, 56);
        b.extend_from_slice(b"vidsMJPG");
        u32le(&mut b, 0);
        u16le(&mut b, 0);
        u16le(&mut b, 0);
        for v in [
            0,
            1,
            self.fps as u32,
            0,
            frames,
            suggested as u32,
            u32::MAX,
            0,
        ] {
            u32le(&mut b, v);
        }
        for v in [0u16, 0, w as u16, h as u16] {
            u16le(&mut b, v);
        }
        b.extend_from_slice(b"strf");
        u32le(&mut b, 40);
        u32le(&mut b, 40);
        u32le(&mut b, w);
        u32le(&mut b, h);
        u16le(&mut b, 1);
        u16le(&mut b, 24);
        b.extend_from_slice(b"MJPG");
        u32le(&mut b, (self.width * self.height * 3) as u32);
        for _ in 0..4 {
            u32le(&mut b, 0);
        }
        b.extend_from_slice(b"LIST");
        u32le(&mut b, (4 + self.movi_bytes) as u32);
        b.extend_from_slice(b"movi");
        b
    }
    fn index(&self) -> Vec<u8> {
        let mut b = b"idx1".to_vec();
        b.extend_from_slice(
            &((self.frame_sizes.len() as i64 * AVI_INDEX_ENTRY_BYTES) as u32).to_le_bytes(),
        );
        let mut offset = 4u32;
        for size in &self.frame_sizes {
            b.extend_from_slice(b"00dc");
            b.extend_from_slice(&0x10u32.to_le_bytes());
            b.extend_from_slice(&offset.to_le_bytes());
            b.extend_from_slice(&size.to_le_bytes());
            offset = offset.wrapping_add(8 + pad_even(i64::from(*size)) as u32);
        }
        b
    }
}
fn avi_chunk(data: &[u8]) -> Vec<u8> {
    let mut b = b"00dc".to_vec();
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(data);
    if data.len() & 1 == 1 {
        b.push(0);
    }
    b
}
fn encode_mjpeg_frame(img: &PyramidImage, quality: i64) -> Vec<u8> {
    let (w, h) = img.size();
    let rgb: Vec<u8> = match img {
        PyramidImage::Nrgba { .. } => to_opaque_rgba_16(img),
        other => to_opaque_rgba(other),
    }
    .pixels
    .as_chunks::<4>()
    .0
    .iter()
    .flat_map(|p| [p[0], p[1], p[2]])
    .collect();
    stdjpeg::encode(stdjpeg::Source::Rgb(&rgb), w, h, quality)
}
pub fn encode_mjpeg_avi(
    frames: &[Option<PyramidImage>],
    opts: AviOptions,
) -> Result<Vec<u8>, RenderError> {
    if frames.is_empty() {
        return Err(err("render: no frames to encode"));
    }
    let opts = opts.normalized()?;
    let mut layout = AviLayout {
        fps: opts.fps,
        ..Default::default()
    };
    let mut encoded = Vec::with_capacity(frames.len());
    for (i, f) in frames.iter().enumerate() {
        let Some(img) = f.as_ref().filter(|i| i.size().0 > 0 && i.size().1 > 0) else {
            return Err(err(format!("render: empty AVI frame {}", i + 1)));
        };
        let (w, h) = img.size();
        if i == 0 {
            if w > AVI_MAX_DIMENSION || h > AVI_MAX_DIMENSION {
                return Err(err(format!(
                    "render: AVI frame {w}x{h} exceeds {AVI_MAX_DIMENSION} pixels per side"
                )));
            }
            layout.width = w;
            layout.height = h;
        } else if w != layout.width || h != layout.height {
            return Err(err(format!(
                "{ERR_AVI_FRAME_SIZE}: frame {} is {w}x{h}, movie is {}x{}",
                i + 1,
                layout.width,
                layout.height
            )));
        }
        let data = encode_mjpeg_frame(img, opts.quality);
        if layout.projected_size(data.len()) > opts.max_bytes {
            return Err(err(format!(
                "{ERR_AVI_TOO_LARGE} ({} bytes at frame {})",
                opts.max_bytes,
                i + 1
            )));
        }
        layout.add(data.len());
        encoded.push(data);
    }
    let mut out = layout.header();
    for data in &encoded {
        out.extend_from_slice(&avi_chunk(data));
    }
    out.extend_from_slice(&layout.index());
    Ok(out)
}
pub struct AviWriter<W: Write + Seek> {
    w: W,
    opts: AviOptions,
    layout: AviLayout,
    err: Option<RenderError>,
    closed: bool,
    begun: bool,
}
impl<W: Write + Seek> AviWriter<W> {
    pub fn new(w: W, opts: AviOptions) -> Result<AviWriter<W>, RenderError> {
        let opts = opts.normalized()?;
        Ok(AviWriter {
            w,
            layout: AviLayout {
                fps: opts.fps,
                ..Default::default()
            },
            opts,
            err: None,
            closed: false,
            begun: false,
        })
    }
    pub fn frames(&self) -> i64 {
        self.layout.frame_sizes.len() as i64
    }
    pub fn write_frame(&mut self, img: Option<&PyramidImage>) -> Result<(), RenderError> {
        if self.closed {
            return Err(err(ERR_AVI_CLOSED));
        }
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        let Some(img) = img.filter(|i| i.size().0 > 0 && i.size().1 > 0) else {
            return Err(err(format!(
                "render: empty AVI frame {}",
                self.frames() + 1
            )));
        };
        let (w, h) = img.size();
        if !self.begun {
            if w > AVI_MAX_DIMENSION || h > AVI_MAX_DIMENSION {
                return Err(err(format!(
                    "render: AVI frame {w}x{h} exceeds {AVI_MAX_DIMENSION} pixels per side"
                )));
            }
            self.layout.width = w;
            self.layout.height = h;
        } else if w != self.layout.width || h != self.layout.height {
            return Err(err(format!(
                "{ERR_AVI_FRAME_SIZE}: frame {} is {w}x{h}, movie is {}x{}",
                self.frames() + 1,
                self.layout.width,
                self.layout.height
            )));
        }
        let data = encode_mjpeg_frame(img, self.opts.quality);
        if self.layout.projected_size(data.len()) > self.opts.max_bytes {
            return Err(err(format!(
                "{ERR_AVI_TOO_LARGE} ({} bytes; frame {} not written)",
                self.opts.max_bytes,
                self.frames() + 1
            )));
        }
        if !self.begun {
            if let Err(e) = self.w.write_all(&self.layout.header()) {
                let e = io_err(e);
                self.err = Some(e.clone());
                return Err(e);
            }
            self.begun = true;
        }
        if let Err(e) = self.w.write_all(&avi_chunk(&data)) {
            let e = io_err(e);
            self.err = Some(e.clone());
            return Err(e);
        }
        self.layout.add(data.len());
        Ok(())
    }
    pub fn close(&mut self) -> Result<(), RenderError> {
        if self.closed {
            return Err(err(ERR_AVI_CLOSED));
        }
        self.closed = true;
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        if self.frames() == 0 {
            return Err(err("render: no frames to encode"));
        }
        self.w.write_all(&self.layout.index()).map_err(io_err)?;
        let end = self.w.stream_position().map_err(io_err)?;
        let start = end as i64 - self.layout.file_size();
        self.w.seek(SeekFrom::Start(start as u64)).map_err(io_err)?;
        self.w.write_all(&self.layout.header()).map_err(io_err)?;
        self.w.seek(SeekFrom::Start(end)).map_err(io_err)?;
        Ok(())
    }
    pub fn into_inner(self) -> W {
        self.w
    }
}
