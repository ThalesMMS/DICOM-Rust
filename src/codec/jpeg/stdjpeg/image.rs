#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ratio {
    R444,
    R422,
    R420,
    R440,
    R411,
    R410,
}
#[derive(Debug, Clone, Default)]
pub(crate) struct Gray {
    pub pix: Vec<u8>,
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}
#[derive(Debug, Clone)]
pub(crate) struct YCbCr {
    pub y: Vec<u8>,
    pub cb: Vec<u8>,
    pub cr: Vec<u8>,
    pub y_stride: usize,
    pub c_stride: usize,
    pub ratio: Ratio,
    pub width: usize,
    pub height: usize,
}
#[derive(Debug, Clone, Default)]
pub(crate) struct Quad {
    pub pix: Vec<u8>,
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}
#[derive(Debug, Clone)]
pub(crate) enum Image {
    Gray(Gray),
    YCbCr(YCbCr),
    Rgba(Quad),
    Cmyk(Quad),
}
impl YCbCr {
    #[allow(clippy::arithmetic_side_effects)]
    pub fn new(w: usize, h: usize, ratio: Ratio, width: usize, height: usize) -> YCbCr {
        let (cw, ch) = match ratio {
            Ratio::R422 => (w.div_ceil(2), h),
            Ratio::R420 => (w.div_ceil(2), h.div_ceil(2)),
            Ratio::R440 => (w, h.div_ceil(2)),
            Ratio::R411 => (w.div_ceil(4), h),
            Ratio::R410 => (w.div_ceil(4), h.div_ceil(2)),
            Ratio::R444 => (w, h),
        };
        if width == 0 || height == 0 {
            return YCbCr {
                y: Vec::new(),
                cb: Vec::new(),
                cr: Vec::new(),
                y_stride: 0,
                c_stride: 0,
                ratio,
                width: 0,
                height: 0,
            };
        }
        YCbCr {
            y: vec![0; w * h],
            cb: vec![0; cw * ch],
            cr: vec![0; cw * ch],
            y_stride: w,
            c_stride: cw,
            ratio,
            width,
            height,
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn y_offset(&self, x: usize, y: usize) -> usize {
        y * self.y_stride + x
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn c_offset(&self, x: usize, y: usize) -> usize {
        match self.ratio {
            Ratio::R422 => y * self.c_stride + x / 2,
            Ratio::R420 => (y / 2) * self.c_stride + x / 2,
            Ratio::R440 => (y / 2) * self.c_stride + x,
            Ratio::R411 => y * self.c_stride + x / 4,
            Ratio::R410 => (y / 2) * self.c_stride + x / 4,
            Ratio::R444 => y * self.c_stride + x,
        }
    }
}
impl Image {
    pub fn bounds(&self) -> (usize, usize) {
        match self {
            Image::Gray(g) => (g.width, g.height),
            Image::YCbCr(m) => (m.width, m.height),
            Image::Rgba(m) | Image::Cmyk(m) => (m.width, m.height),
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn rgba16(&self, x: usize, y: usize) -> (u32, u32, u32) {
        match self {
            Image::Gray(g) => {
                let v = g.pix[y * g.stride + x] as u32;
                let v = v | (v << 8);
                (v, v, v)
            }
            Image::YCbCr(m) => {
                let yi = m.y_offset(x, y);
                let ci = m.c_offset(x, y);
                ycbcr_rgba16(m.y[yi], m.cb[ci], m.cr[ci])
            }
            Image::Rgba(m) => {
                let o = y * m.stride + 4 * x;
                let e = |v: u8| (v as u32) | ((v as u32) << 8);
                (e(m.pix[o]), e(m.pix[o + 1]), e(m.pix[o + 2]))
            }
            Image::Cmyk(m) => {
                let o = y * m.stride + 4 * x;
                cmyk_rgba16(m.pix[o], m.pix[o + 1], m.pix[o + 2], m.pix[o + 3])
            }
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn gray_at(&self, x: usize, y: usize) -> u8 {
        if let Image::Gray(g) = self {
            return g.pix[y * g.stride + x];
        }
        let (r, g, b) = self.rgba16(x, y);
        ((19595 * r + 38470 * g + 7471 * b + (1 << 15)) >> 24) as u8
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn rgb_at(&self, x: usize, y: usize) -> (u8, u8, u8) {
        if let Image::Rgba(m) = self {
            let o = y * m.stride + 4 * x;
            return (m.pix[o], m.pix[o + 1], m.pix[o + 2]);
        }
        let (r, g, b) = self.rgba16(x, y);
        ((r >> 8) as u8, (g >> 8) as u8, (b >> 8) as u8)
    }
}
#[inline]
fn clamp_shift(v: i32, shift: u32) -> i32 {
    if (v as u32) & 0xff00_0000 == 0 {
        v >> shift
    } else {
        !(v >> 31)
    }
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn ycbcr_to_rgb(y: u8, cb: u8, cr: u8) -> (u8, u8, u8) {
    let yy1 = y as i32 * 0x10101;
    let cb1 = cb as i32 - 128;
    let cr1 = cr as i32 - 128;
    let r = clamp_shift(yy1 + 91881 * cr1, 16);
    let g = clamp_shift(yy1 - 22554 * cb1 - 46802 * cr1, 16);
    let b = clamp_shift(yy1 + 116130 * cb1, 16);
    (r as u8, g as u8, b as u8)
}
#[allow(clippy::arithmetic_side_effects)]
fn ycbcr_rgba16(y: u8, cb: u8, cr: u8) -> (u32, u32, u32) {
    let yy1 = y as i32 * 0x10101;
    let cb1 = cb as i32 - 128;
    let cr1 = cr as i32 - 128;
    let f = |v: i32| -> u32 {
        if (v as u32) & 0xff00_0000 == 0 {
            (v >> 8) as u32
        } else {
            (!(v >> 31) & 0xffff) as u32
        }
    };
    (
        f(yy1 + 91881 * cr1),
        f(yy1 - 22554 * cb1 - 46802 * cr1),
        f(yy1 + 116130 * cb1),
    )
}
#[allow(clippy::arithmetic_side_effects)]
fn cmyk_rgba16(c: u8, m: u8, y: u8, k: u8) -> (u32, u32, u32) {
    let w = 0xffff - k as u32 * 0x101;
    (
        (0xffff - c as u32 * 0x101) * w / 0xffff,
        (0xffff - m as u32 * 0x101) * w / 0xffff,
        (0xffff - y as u32 * 0x101) * w / 0xffff,
    )
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn rgb_to_ycbcr(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let r1 = r as i32;
    let g1 = g as i32;
    let b1 = b as i32;
    let yy = (19595 * r1 + 38470 * g1 + 7471 * b1 + (1 << 15)) >> 16;
    let cb = clamp_shift(-11056 * r1 - 21712 * g1 + 32768 * b1 + (257 << 15), 16);
    let cr = clamp_shift(32768 * r1 - 27440 * g1 - 5328 * b1 + (257 << 15), 16);
    (yy as u8, cb as u8, cr as u8)
}
