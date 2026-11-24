use crate::pixel::frame::FrameImage;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TileFormat {
    #[default]
    Gray,
    Gray16,
    Rgba,
    Nrgba,
    Rgba64,
    Nrgba64,
}
impl TileFormat {
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            TileFormat::Gray => 1,
            TileFormat::Gray16 => 2,
            TileFormat::Rgba | TileFormat::Nrgba => 4,
            TileFormat::Rgba64 | TileFormat::Nrgba64 => 8,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            TileFormat::Gray => "Gray",
            TileFormat::Gray16 => "Gray16",
            TileFormat::Rgba => "RGBA",
            TileFormat::Nrgba => "NRGBA",
            TileFormat::Rgba64 => "RGBA64",
            TileFormat::Nrgba64 => "NRGBA64",
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TileImage {
    pub format: TileFormat,
    pub width: usize,
    pub height: usize,
    pub pix: Vec<u8>,
}
fn be16(b: &[u8]) -> u32 {
    u32::from(u16::from_be_bytes([b[0], b[1]]))
}
impl TileImage {
    pub fn new(format: TileFormat, width: usize, height: usize) -> TileImage {
        TileImage {
            format,
            width,
            height,
            pix: vec![0; width * height * format.bytes_per_pixel()],
        }
    }
    pub fn from_frame(image: FrameImage) -> TileImage {
        match image {
            FrameImage::Gray(g) => TileImage {
                format: TileFormat::Gray,
                width: g.width,
                height: g.height,
                pix: g.pixels,
            },
            FrameImage::Rgba(c) => TileImage {
                format: TileFormat::Rgba,
                width: c.width,
                height: c.height,
                pix: c.pixels,
            },
        }
    }
    fn rgba64_at(&self, i: usize) -> [u32; 4] {
        let p = &self.pix[i * self.format.bytes_per_pixel()..];
        match self.format {
            TileFormat::Gray => {
                let y = u32::from(p[0]) * 0x101;
                [y, y, y, 0xffff]
            }
            TileFormat::Gray16 => {
                let y = be16(p);
                [y, y, y, 0xffff]
            }
            TileFormat::Rgba => std::array::from_fn(|c| u32::from(p[c]) * 0x101),
            TileFormat::Nrgba => {
                let a = u32::from(p[3]);
                let ch = |v: u8| u32::from(v) * 0x101 * a / 0xff;
                [ch(p[0]), ch(p[1]), ch(p[2]), a * 0x101]
            }
            TileFormat::Rgba64 => std::array::from_fn(|c| be16(&p[2 * c..])),
            TileFormat::Nrgba64 => {
                let a = be16(&p[6..]);
                let ch = |c: usize| be16(&p[2 * c..]) * a / 0xffff;
                [ch(0), ch(1), ch(2), a]
            }
        }
    }
    pub(crate) fn to_nrgba(&self) -> TileImage {
        if self.format == TileFormat::Nrgba {
            return self.clone();
        }
        let mut out = TileImage::new(TileFormat::Nrgba, self.width, self.height);
        for i in 0..self.width * self.height {
            let [r, g, b, a] = self.rgba64_at(i);
            let px = match a {
                0 | 0xffff => [r >> 8, g >> 8, b >> 8, a >> 8],
                _ => [
                    (r * 0xffff / a) >> 8,
                    (g * 0xffff / a) >> 8,
                    (b * 0xffff / a) >> 8,
                    a >> 8,
                ],
            };
            for (slot, v) in out.pix[i * 4..i * 4 + 4].iter_mut().zip(px) {
                *slot = v as u8;
            }
        }
        out
    }
    pub(crate) fn to_rgba64(&self) -> TileImage {
        let mut out = TileImage::new(TileFormat::Rgba64, self.width, self.height);
        for i in 0..self.width * self.height {
            for (c, v) in self.rgba64_at(i).into_iter().enumerate() {
                out.pix[i * 8 + 2 * c..i * 8 + 2 * c + 2]
                    .copy_from_slice(&(v as u16).to_be_bytes());
            }
        }
        out
    }
}
