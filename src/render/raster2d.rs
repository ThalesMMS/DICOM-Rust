#![forbid(unsafe_code)]
use super::error::{ErrorKind, RenderError};
use super::frame::{Frame, normalized_photometric};
use super::parallel::{Cancellation, ctx_err};
use super::window::{WindowLevel, normalize_window};
use crate::display::{VoiByteMapper, VoiLut};
use std::collections::HashMap;
type Ctx<'a> = Option<&'a dyn Cancellation>;
pub const ERR_RASTER_REGION_2D: &str = "render: invalid or incomplete regional 2D raster";
pub const ERR_GRAY_2D_PROFILE: &str = "render: unsupported resident grayscale 2D profile";
pub const MAX_REDUCTION_LEVEL: i64 = 16;
fn region_error() -> RenderError {
    RenderError::new(ErrorKind::RasterRegion2d, ERR_RASTER_REGION_2D)
}
fn profile_error() -> RenderError {
    RenderError::new(ErrorKind::Gray2dProfile, ERR_GRAY_2D_PROFILE)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}
impl Rect {
    pub const fn new(min_x: i64, min_y: i64, max_x: i64, max_y: i64) -> Rect {
        Rect {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }
    pub fn empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    pub fn size(&self) -> (i64, i64) {
        (self.max_x - self.min_x, self.max_y - self.min_y)
    }
    pub(crate) fn in_rect(&self, s: &Rect) -> bool {
        if self.empty() {
            return true;
        }
        s.min_x <= self.min_x
            && self.max_x <= s.max_x
            && s.min_y <= self.min_y
            && self.max_y <= s.max_y
    }
    pub fn contains(&self, x: i64, y: i64) -> bool {
        self.min_x <= x && x < self.max_x && self.min_y <= y && y < self.max_y
    }
}
fn floor_div(a: i64, b: i64) -> i64 {
    if a >= 0 {
        return a / b;
    }
    -((-a + b - 1) / b)
}
pub fn reduced_region(region: Rect, level: i64) -> Rect {
    if level <= 0 {
        return region;
    }
    let s = 1i64 << level;
    let ceil_div = |a: i64| -floor_div(-a, s);
    Rect::new(
        floor_div(region.min_x, s),
        floor_div(region.min_y, s),
        ceil_div(region.max_x),
        ceil_div(region.max_y),
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterImageKind {
    Gray,
    Gray16,
    Nrgba,
    Rgba,
}
#[derive(Debug, Clone, PartialEq)]
pub struct RasterImage {
    pub kind: RasterImageKind,
    pub rect: Rect,
    pub stride: usize,
    pub pix: Vec<u8>,
}
impl RasterImage {
    fn sample(&self, x: i64, y: i64) -> [u8; 4] {
        let row = (y - self.rect.min_y) as usize * self.stride;
        let col = (x - self.rect.min_x) as usize;
        match self.kind {
            RasterImageKind::Gray => {
                let v = self.pix[row + col];
                [v, v, v, 255]
            }
            RasterImageKind::Gray16 => {
                let v = self.pix[row + 2 * col];
                [v, v, v, 255]
            }
            RasterImageKind::Nrgba => {
                let o = row + 4 * col;
                [
                    self.pix[o],
                    self.pix[o + 1],
                    self.pix[o + 2],
                    self.pix[o + 3],
                ]
            }
            RasterImageKind::Rgba => {
                let o = row + 4 * col;
                let c = [
                    self.pix[o],
                    self.pix[o + 1],
                    self.pix[o + 2],
                    self.pix[o + 3],
                ];
                if c[3] == 255 {
                    return c;
                }
                nrgba_of_rgba(c)
            }
        }
    }
}
fn nrgba_of_rgba(c: [u8; 4]) -> [u8; 4] {
    let wide = |v: u8| {
        let v = u32::from(v);
        v | v << 8
    };
    let (r, g, b, a) = (wide(c[0]), wide(c[1]), wide(c[2]), wide(c[3]));
    if a == 0xffff {
        return [(r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8, 0xff];
    }
    if a == 0 {
        return [0, 0, 0, 0];
    }
    let r = r.wrapping_mul(0xffff) / a;
    let g = g.wrapping_mul(0xffff) / a;
    let b = b.wrapping_mul(0xffff) / a;
    [
        (r >> 8) as u8,
        (g >> 8) as u8,
        (b >> 8) as u8,
        (a >> 8) as u8,
    ]
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RasterRegion2d {
    pub image: Option<RasterImage>,
    pub grid: Rect,
    pub full_size: (i64, i64),
    pub reduction_level: i64,
}
pub const RASTER_NEAREST_2D: u8 = 0;
pub const RASTER_BILINEAR_2D: u8 = 1;
pub(crate) type SourceColorFn<'a> = &'a (dyn Fn(i64, i64, [u8; 4]) -> [u8; 4] + Sync);
#[derive(Clone, Copy, Default)]
pub struct RasterViewport2dRequest<'a> {
    pub width: i64,
    pub height: i64,
    pub affine: [f64; 6],
    pub sampling: u8,
    pub background: [u8; 4],
    pub max_bytes: u64,
    pub source_color: Option<SourceColorFn<'a>>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct NrgbaRaster {
    pub width: usize,
    pub height: usize,
    pub pix: Vec<u8>,
}
pub fn raster_viewport_2d(
    ctx: Ctx<'_>,
    source: &RasterRegion2d,
    request: &RasterViewport2dRequest<'_>,
) -> Result<NrgbaRaster, RenderError> {
    raster(ctx, source, request).map(|(out, _)| out)
}
pub fn raster_viewport_2d_presentation(
    ctx: Ctx<'_>,
    source: &RasterRegion2d,
    request: &RasterViewport2dRequest<'_>,
) -> Result<(NrgbaRaster, bool), RenderError> {
    raster(ctx, source, request)
}
fn raster(
    ctx: Ctx<'_>,
    source: &RasterRegion2d,
    request: &RasterViewport2dRequest<'_>,
) -> Result<(NrgbaRaster, bool), RenderError> {
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    let (fx, fy) = source.full_size;
    if fx < 1
        || fy < 1
        || fx > 65535
        || fy > 65535
        || source.reduction_level < 0
        || source.reduction_level > MAX_REDUCTION_LEVEL
        || request.width < 1
        || request.height < 1
        || request.width > 16384
        || request.height > 16384
        || request.sampling > RASTER_BILINEAR_2D
    {
        return Err(region_error());
    }
    let lattice = reduced_region(Rect::new(0, 0, fx, fy), source.reduction_level);
    if !source.grid.empty() {
        match &source.image {
            Some(img) if img.rect.size() == source.grid.size() && source.grid.in_rect(&lattice) => {
            }
            _ => return Err(region_error()),
        }
    } else if source.image.is_some() {
        return Err(region_error());
    }
    let budget = if request.max_bytes == 0 {
        64 << 20
    } else {
        request.max_bytes
    };
    let pixels = request.width as u64 * request.height as u64;
    if pixels > budget / 4 {
        return Err(region_error());
    }
    if request.affine.iter().any(|v| !v.is_finite()) {
        return Err(region_error());
    }
    let [a, b, c, d, tx, ty] = request.affine;
    let determinant = a * d - b * c;
    if determinant == 0.0 || !determinant.is_finite() {
        return Err(region_error());
    }
    let step = (1u64 << source.reduction_level) as f64;
    let fetch = |x: i64, y: i64| -> Option<[u8; 4]> {
        let x = lattice.min_x.max((lattice.max_x - 1).min(x));
        let y = lattice.min_y.max((lattice.max_y - 1).min(y));
        let img = source.image.as_ref()?;
        if !source.grid.contains(x, y) {
            return None;
        }
        let mut value = img.sample(
            img.rect.min_x + x - source.grid.min_x,
            img.rect.min_y + y - source.grid.min_y,
        );
        if let Some(f) = request.source_color {
            let sx = (fx - 1).min(((x as f64 + 0.5) * step) as i64);
            let sy = (fy - 1).min(((y as f64 + 0.5) * step) as i64);
            value = f(sx, sy, value);
        }
        Some(value)
    };
    let (w, h) = (request.width as usize, request.height as usize);
    let mut pix = vec![0u8; w * h * 4];
    let mut opaque = true;
    for y in 0..h {
        if let Some(e) = ctx_err(ctx) {
            return Err(e);
        }
        for x in 0..w {
            let px = x as f64 + 0.5 - tx;
            let py = y as f64 + 0.5 - ty;
            let sx = (d * px - b * py) / determinant;
            let sy = (-c * px + a * py) / determinant;
            if !sx.is_finite() || !sy.is_finite() {
                return Err(region_error());
            }
            let mut value = request.background;
            if sx >= 0.0 && sy >= 0.0 && sx < fx as f64 && sy < fy as f64 {
                if request.sampling == RASTER_NEAREST_2D {
                    value = fetch(
                        crate::fmath::f64_to_i64((sx / step).floor()),
                        crate::fmath::f64_to_i64((sy / step).floor()),
                    )
                    .ok_or_else(region_error)?;
                } else {
                    let u = sx / step - 0.5;
                    let v = sy / step - 0.5;
                    let ix = crate::fmath::f64_to_i64(u.floor());
                    let iy = crate::fmath::f64_to_i64(v.floor());
                    let fx = u - ix as f64;
                    let fy = v - iy as f64;
                    let weights = [
                        (1.0 - fx) * (1.0 - fy),
                        fx * (1.0 - fy),
                        (1.0 - fx) * fy,
                        fx * fy,
                    ];
                    let points = [(ix, iy), (ix + 1, iy), (ix, iy + 1), (ix + 1, iy + 1)];
                    let (mut alpha, mut red, mut green, mut blue) = (0.0, 0.0, 0.0, 0.0);
                    for (i, p) in points.iter().enumerate() {
                        if weights[i] == 0.0 {
                            continue;
                        }
                        let sample = fetch(p.0, p.1).ok_or_else(region_error)?;
                        let weighted = weights[i] * f64::from(sample[3]);
                        alpha += weighted;
                        red += weighted * f64::from(sample[0]);
                        green += weighted * f64::from(sample[1]);
                        blue += weighted * f64::from(sample[2]);
                    }
                    value = [0; 4];
                    if alpha > 0.0 {
                        value = [
                            (red / alpha).round() as u8,
                            (green / alpha).round() as u8,
                            (blue / alpha).round() as u8,
                            alpha.round() as u8,
                        ];
                    }
                }
            }
            opaque = opaque && value[3] == 255;
            pix[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&value);
        }
    }
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    Ok((
        NrgbaRaster {
            width: w,
            height: h,
            pix,
        },
        opaque,
    ))
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gray2dSource {
    pub width: i64,
    pub height: i64,
    pub indices: Vec<u32>,
    pub levels: Vec<f32>,
    pub invert: bool,
    pub default_window: WindowLevel,
}
pub fn check_gray_2d_profile(f: Option<&Frame>, max_bytes: u64) -> Result<(), RenderError> {
    let Some(f) = f else {
        return Err(profile_error());
    };
    let m = &f.metadata;
    if f.encapsulated
        || f.preserve_modality_precision
        || m.samples_per_pixel != 1
        || (m.bits_allocated != 8 && m.bits_allocated != 16)
    {
        return Err(profile_error());
    }
    let photo = normalized_photometric(&m.photometric_interpretation);
    if photo != "MONOCHROME1" && photo != "MONOCHROME2" {
        return Err(profile_error());
    }
    let n = u64::from(m.rows) * u64::from(m.columns);
    if n == 0 || max_bytes == 0 || n > max_bytes / 8 || n > 16 << 20 {
        return Err(profile_error());
    }
    Ok(())
}
pub fn prepare_gray_2d(
    ctx: Ctx<'_>,
    f: Option<&Frame>,
    max_bytes: u64,
) -> Result<Gray2dSource, RenderError> {
    if let Some(e) = ctx_err(ctx) {
        return Err(e);
    }
    check_gray_2d_profile(f, max_bytes)?;
    let Some(f) = f else {
        return Err(profile_error());
    };
    let n = u64::from(f.metadata.rows) * u64::from(f.metadata.columns);
    let photo = normalized_photometric(&f.metadata.photometric_interpretation);
    let decoded = super::render_cache::decode_slice_frame(f)?;
    let grayscale = decoded.grayscale.as_deref().map_or(&[][..], |g| &g[..]);
    if grayscale.len() as u64 != n {
        return Err(profile_error());
    }
    let mut s = Gray2dSource {
        width: decoded.cols,
        height: decoded.rows,
        indices: vec![0; n as usize],
        invert: photo == "MONOCHROME1",
        default_window: f.default_window.clone(),
        ..Default::default()
    };
    let mut ids: HashMap<u32, u32> = HashMap::new();
    for (i, v) in grayscale.iter().enumerate() {
        if i % 4096 == 0
            && let Some(e) = ctx_err(ctx)
        {
            return Err(e);
        }
        let bits = v.to_bits();
        let id = match ids.get(&bits) {
            Some(id) => *id,
            None => {
                if s.levels.len() == 65536 {
                    return Err(profile_error());
                }
                let id = s.levels.len() as u32;
                ids.insert(bits, id);
                s.levels.push(*v);
                id
            }
        };
        s.indices[i] = id;
    }
    Ok(s)
}
impl Gray2dSource {
    pub fn gray_table(&self, ctx: Ctx<'_>, w: &WindowLevel) -> Result<Vec<u32>, RenderError> {
        let w = normalize_window(w, &self.default_window);
        let mapper = VoiByteMapper::new(VoiLut {
            center: w.center,
            width: w.width,
            function: w.function,
            lut: w.lut.clone(),
            explanation: String::new(),
        });
        let mut out = Vec::with_capacity(self.levels.len());
        for (i, v) in self.levels.iter().enumerate() {
            if i % 256 == 0
                && let Some(e) = ctx_err(ctx)
            {
                return Err(e);
            }
            let mut g = mapper.byte(f64::from(*v));
            if self.invert {
                g = 255 - g;
            }
            out.push(u32::from(g));
        }
        Ok(out)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gray2dAxis {
    pub first: u32,
    pub second: u32,
    pub weight: u32,
    pub inside: u32,
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Gray2dPlan {
    pub width: i64,
    pub height: i64,
    pub source_width: i64,
    pub source_height: i64,
    pub x: Vec<Gray2dAxis>,
    pub y: Vec<Gray2dAxis>,
}
pub fn plan_gray_2d_viewport(
    size: (i64, i64),
    r: &RasterViewport2dRequest<'_>,
) -> Result<Gray2dPlan, RenderError> {
    let a = r.affine;
    if size.0 < 1
        || size.1 < 1
        || size.0 > 65535
        || size.1 > 65535
        || r.width < 1
        || r.height < 1
        || r.width > 16384
        || r.height > 16384
        || r.width as u64 * r.height as u64 > r.max_bytes / 4
        || a[1] != 0.0
        || a[2] != 0.0
        || a[0] == 0.0
        || a[3] == 0.0
        || r.sampling > RASTER_BILINEAR_2D
        || r.source_color.is_some()
    {
        return Err(profile_error());
    }
    if a.iter().any(|v| !v.is_finite()) {
        return Err(profile_error());
    }
    let axis = |n: i64, source: i64, scale: f64, translation: f64| -> Vec<Gray2dAxis> {
        (0..n)
            .map(|i| {
                let q = (i as f64 + 0.5 - translation) / scale;
                if q < 0.0 || q >= source as f64 {
                    return Gray2dAxis::default();
                }
                let mut entry = Gray2dAxis {
                    inside: 1,
                    ..Default::default()
                };
                if r.sampling == RASTER_NEAREST_2D {
                    entry.first = q.floor() as u32;
                    entry.second = entry.first;
                } else {
                    let u = q - 0.5;
                    let b = crate::fmath::f64_to_i64(u.floor());
                    entry.first = 0.max((source - 1).min(b)) as u32;
                    entry.second = 0.max((source - 1).min(b + 1)) as u32;
                    entry.weight = ((u - b as f64) as f32).to_bits();
                }
                entry
            })
            .collect()
    };
    Ok(Gray2dPlan {
        width: r.width,
        height: r.height,
        source_width: size.0,
        source_height: size.1,
        x: axis(r.width, size.0, a[0], a[4]),
        y: axis(r.height, size.1, a[3], a[5]),
    })
}
impl Gray2dPlan {
    pub fn parameters(&self, background: u32) -> Vec<u32> {
        let mut out = vec![
            self.width as u32,
            self.height as u32,
            self.source_width as u32,
            background,
        ];
        for axis in [&self.x, &self.y] {
            for v in axis {
                out.extend([v.first, v.second, v.weight, v.inside]);
            }
        }
        out
    }
}
pub const GRAY_2D_SHADER: &str = "
@group(0) @binding(0) var<storage,read> samples:array<u32>;
@group(0) @binding(1) var<storage,read> gray:array<u32>;
@group(0) @binding(2) var<storage,read> p:array<u32>;
@group(0) @binding(3) var<storage,read_write> output:array<u32>;
fn value(x:u32,y:u32)->f32{return f32(gray[samples[y*p[2]+x]]);}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id:vec3<u32>){
 if(id.x>=p[0] || id.y>=p[1]){return;}
 let xi=4u+id.x*4u;let yi=4u+p[0]*4u+id.y*4u;
 let outputIndex=id.y*p[0]+id.x;
 if(p[xi+3u]==0u || p[yi+3u]==0u){output[outputIndex]=p[3];return;}
 let x=bitcast<f32>(p[xi+2u]);let y=bitcast<f32>(p[yi+2u]);
 let a=value(p[xi],p[yi]);let b=value(p[xi+1u],p[yi]);
 let c=value(p[xi],p[yi+1u]);let d=value(p[xi+1u],p[yi+1u]);
 let v=u32(clamp(floor(mix(mix(a,b,x),mix(c,d,x),y)+0.5),0.0,255.0));
 output[outputIndex]=v|(v<<8u)|(v<<16u)|0xff000000u;
}";
