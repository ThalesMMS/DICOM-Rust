use super::presented::{
    PresentedFrame, PresentedOptions, PresentedResult, compare_presented, finite, presented_budget,
};
use super::{Error, ctx_err};
use crate::fmath;
use crate::net::ul::Ctx;
use crate::render::qualification::{RgbaFrameThresholdsV1, compare_rgba8_frames};
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresentedImagePolicy {
    pub depth: i64,
    pub encoding: String,
    pub alpha: String,
    pub background: [f64; 3],
}
impl PresentedImagePolicy {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if (self.depth != 8 && self.depth != 16)
            || !matches!(
                self.encoding.as_str(),
                "gray-pvalue" | "srgb-code" | "linear-rgb"
            )
            || !matches!(self.alpha.as_str(), "opaque" | "ignore" | "composite")
            || self
                .background
                .iter()
                .any(|v| !finite(*v) || *v < 0.0 || *v > 1.0)
        {
            return Err(Error::Configuration(None));
        }
        let [b0, b1, b2] = self.background;
        if self.encoding == "gray-pvalue" && (b0 != b1 || b0 != b2) {
            return Err(Error::Configuration(None));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentedImage<'a> {
    Gray {
        width: i64,
        height: i64,
        pix: &'a [u8],
    },
    Gray16 {
        width: i64,
        height: i64,
        pix: &'a [u8],
    },
    Rgba {
        width: i64,
        height: i64,
        pix: &'a [u8],
    },
    Nrgba {
        width: i64,
        height: i64,
        pix: &'a [u8],
    },
    Rgba64 {
        width: i64,
        height: i64,
        pix: &'a [u8],
    },
    Nrgba64 {
        width: i64,
        height: i64,
        pix: &'a [u8],
    },
    Other {
        width: i64,
        height: i64,
    },
}
impl PresentedImage<'_> {
    fn size(&self) -> (i64, i64) {
        match *self {
            PresentedImage::Gray { width, height, .. }
            | PresentedImage::Gray16 { width, height, .. }
            | PresentedImage::Rgba { width, height, .. }
            | PresentedImage::Nrgba { width, height, .. }
            | PresentedImage::Rgba64 { width, height, .. }
            | PresentedImage::Nrgba64 { width, height, .. }
            | PresentedImage::Other { width, height } => (width, height),
        }
    }
    fn supported(&self, depth: i64) -> bool {
        match self {
            PresentedImage::Gray { .. }
            | PresentedImage::Rgba { .. }
            | PresentedImage::Nrgba { .. } => depth == 8,
            PresentedImage::Gray16 { .. }
            | PresentedImage::Rgba64 { .. }
            | PresentedImage::Nrgba64 { .. } => depth == 16,
            PresentedImage::Other { .. } => false,
        }
    }
    fn nrgba64(&self, i: usize) -> [u32; 4] {
        let at = |p: &[u8], k: usize| p.get(k).copied().map_or(0, u32::from);
        let be = |p: &[u8], k: usize| at(p, k) << 8 | at(p, k.saturating_add(1));
        let wide = |v: u32| v << 8 | v;
        let quad = |p: &[u8], k: usize| [0, 1, 2, 3].map(|j| at(p, k.saturating_add(j)));
        let be4 = |p: &[u8], k: usize| [0, 2, 4, 6].map(|j| be(p, k.saturating_add(j)));
        let scale = |v: u32, a: u32| v.saturating_mul(a) / 0xffff;
        let premultiplied = match *self {
            PresentedImage::Gray { pix, .. } => {
                let y = wide(at(pix, i));
                [y, y, y, 0xffff]
            }
            PresentedImage::Gray16 { pix, .. } => {
                let y = be(pix, i.saturating_mul(2));
                [y, y, y, 0xffff]
            }
            PresentedImage::Rgba { pix, .. } => quad(pix, i.saturating_mul(4)).map(wide),
            PresentedImage::Nrgba { pix, .. } => {
                let [r, g, b, a] = quad(pix, i.saturating_mul(4)).map(wide);
                [scale(r, a), scale(g, a), scale(b, a), a]
            }
            PresentedImage::Rgba64 { pix, .. } => be4(pix, i.saturating_mul(8)),
            PresentedImage::Nrgba64 { pix, .. } => {
                return be4(pix, i.saturating_mul(8));
            }
            PresentedImage::Other { .. } => [0, 0, 0, 0],
        };
        let [r, g, b, a] = premultiplied;
        let unscale = |v: u32| v.saturating_mul(0xffff).checked_div(a).unwrap_or(0);
        match a {
            0xffff => [r, g, b, 0xffff],
            0 => [0, 0, 0, 0],
            _ => [unscale(r), unscale(g), unscale(b), a],
        }
    }
}
fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        return v / 12.92;
    }
    fmath::pow((v + 0.055) / 1.055, 2.4)
}
pub(super) fn image_plane(
    ctx: &Ctx,
    im: &PresentedImage,
    p: &PresentedImagePolicy,
    o: &PresentedOptions,
) -> Result<PresentedFrame, Error> {
    p.validate()?;
    let (width, height) = im.size();
    let mut c = 3;
    if p.encoding == "gray-pvalue" {
        c = 1;
        if !matches!(
            im,
            PresentedImage::Gray { .. } | PresentedImage::Gray16 { .. }
        ) {
            return Err(Error::Mismatch(Some(
                "grayscale requires a Gray/Gray16 image",
            )));
        }
    }
    presented_budget(width, height, c, o, c.saturating_mul(16))?;
    let (w, h, cu) = (width as usize, height as usize, c as usize);
    let mut f = PresentedFrame {
        width,
        height,
        components: c,
        depth: p.depth,
        encoding: p.encoding.clone(),
        values: vec![0.0; w.saturating_mul(h).saturating_mul(cu)],
    };
    let linear = p.encoding == "linear-rgb";
    let mut background = p.background;
    if linear {
        background = background.map(srgb_to_linear);
    }
    for y in 0..h {
        ctx_err(ctx)?;
        for x in 0..w {
            if x % 1024 == 0 {
                ctx_err(ctx)?;
            }
            let pixel = y.saturating_mul(w).saturating_add(x);
            let [r, g, b, a] = im.nrgba64(pixel);
            let mut values = [r as f64 / 65535.0, g as f64 / 65535.0, b as f64 / 65535.0];
            let alpha = a as f64 / 65535.0;
            if p.alpha == "opaque" && a != 65535 {
                return Err(Error::Mismatch(Some("nonopaque pixel")));
            }
            let first = pixel.saturating_mul(cu);
            let out = f
                .values
                .get_mut(first..first.saturating_add(cu))
                .unwrap_or_default();
            for ((o, v), &bg) in out.iter_mut().zip(values.iter_mut()).zip(&background) {
                if linear {
                    *v = srgb_to_linear(*v);
                }
                if p.alpha == "composite" {
                    *v = alpha * *v + (1.0 - alpha) * bg;
                }
                *o = *v;
            }
        }
    }
    Ok(f)
}
pub fn compare_presented_images(
    ctx: &Ctx,
    reference: &PresentedImage,
    candidate: &PresentedImage,
    p: &PresentedImagePolicy,
    o: &PresentedOptions,
) -> Result<PresentedResult, Error> {
    ctx_err(ctx)?;
    if !reference.supported(p.depth)
        || !candidate.supported(p.depth)
        || reference.size() != candidate.size()
    {
        return Err(Error::Mismatch(None));
    }
    let a = image_plane(ctx, reference, p, o)?;
    let b = image_plane(ctx, candidate, p, o)?;
    let mut r = compare_presented(ctx, &a, &b, o)?;
    r.image_policy = Some(p.clone());
    r.logical_working_bytes = r
        .logical_working_bytes
        .saturating_add((a.values.len().saturating_add(b.values.len()) as i64).saturating_mul(8));
    Ok(r)
}
#[derive(Debug, Clone, Copy)]
pub struct PresentedRgba8Pair<'a> {
    pub reference: &'a [u8],
    pub candidate: &'a [u8],
    pub width: i64,
    pub height: i64,
}
pub fn compare_presented_rgba8(
    ctx: &Ctx,
    frames: PresentedRgba8Pair<'_>,
    p: &PresentedImagePolicy,
    o: &PresentedOptions,
    t: RgbaFrameThresholdsV1,
) -> Result<PresentedResult, Error> {
    let PresentedRgba8Pair {
        reference,
        candidate,
        width,
        height,
    } = frames;
    ctx_err(ctx)?;
    if width <= 0
        || height <= 0
        || width > i64::from(u32::MAX)
        || height > i64::from(u32::MAX)
        || width
            .checked_mul(height)
            .is_none_or(|n| n > o.max_pixels || n > i64::MAX / 4)
    {
        return Err(Error::Limit);
    }
    let want = width.saturating_mul(height).saturating_mul(4) as usize;
    if reference.len() != want
        || candidate.len() != want
        || p.depth != 8
        || p.encoding == "gray-pvalue"
    {
        return Err(Error::Mismatch(None));
    }
    let raw = compare_rgba8_frames(reference, candidate, width as u32, height as u32, t)
        .map_err(|e| Error::Qualification(e.to_string()))?;
    ctx_err(ctx)?;
    let a = PresentedImage::Nrgba {
        width,
        height,
        pix: reference,
    };
    let b = PresentedImage::Nrgba {
        width,
        height,
        pix: candidate,
    };
    let mut r = compare_presented_images(ctx, &a, &b, p, o)?;
    r.raw_rgba = Some(raw);
    Ok(r)
}
