#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::dct::{Block, idct};
use super::image::{Gray, Image, Quad, Ratio, YCbCr, ycbcr_to_rgb};
use std::fmt;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    Format(&'static str),
    Unsupported(&'static str),
    UnexpectedEof,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Format(s) => write!(f, "invalid JPEG format: {s}"),
            Error::Unsupported(s) => write!(f, "unsupported JPEG feature: {s}"),
            Error::UnexpectedEof => f.write_str("unexpected EOF"),
        }
    }
}
const ERR_SUBSAMPLING: Error = Error::Unsupported("luma/chroma subsampling ratio");
const ERR_NO_IMAGE: Error = Error::Format("missing SOF marker");
const ERR_MISSING_FF00: Error = Error::Format("missing 0xff00 sequence");
const ERR_SHORT_HUFFMAN: Error = Error::Format("short Huffman data");
const BLOCK_SIZE: usize = 64;
const MAX_TC: u8 = 1;
const MAX_TH: u8 = 3;
const MAX_TQ: u8 = 3;
const LUT_SIZE: u32 = 8;
const MAX_CODE_LENGTH: usize = 16;
const SOF0: u8 = 0xc0;
const SOF1: u8 = 0xc1;
const SOF2: u8 = 0xc2;
const DHT: u8 = 0xc4;
const RST0: u8 = 0xd0;
const RST7: u8 = 0xd7;
const SOI: u8 = 0xd8;
const EOI: u8 = 0xd9;
const SOS: u8 = 0xda;
const DQT: u8 = 0xdb;
const DRI: u8 = 0xdd;
const COM: u8 = 0xfe;
const APP0: u8 = 0xe0;
const APP14: u8 = 0xee;
const APP15: u8 = 0xef;
pub(crate) const UNZIG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
#[derive(Debug, Clone, Copy, Default)]
struct Component {
    h: usize,
    v: usize,
    c: u8,
    tq: u8,
}
#[derive(Debug, Clone, Copy, Default)]
struct Bits {
    a: u32,
    m: u32,
    n: i32,
}
#[derive(Clone)]
struct Huffman {
    n_codes: i32,
    lut: [u16; 256],
    vals: [u8; 256],
    min_codes: [i32; MAX_CODE_LENGTH],
    max_codes: [i32; MAX_CODE_LENGTH],
    vals_indices: [i32; MAX_CODE_LENGTH],
}
impl Default for Huffman {
    fn default() -> Self {
        Huffman {
            n_codes: 0,
            lut: [0; 256],
            vals: [0; 256],
            min_codes: [0; MAX_CODE_LENGTH],
            max_codes: [0; MAX_CODE_LENGTH],
            vals_indices: [0; MAX_CODE_LENGTH],
        }
    }
}
struct Decoder<'a> {
    src: &'a [u8],
    src_pos: usize,
    bits: Bits,
    buf: Box<[u8; 4096]>,
    bi: usize,
    bj: usize,
    n_unreadable: usize,
    width: usize,
    height: usize,
    img1: Option<Gray>,
    img3: Option<YCbCr>,
    black: Option<(Vec<u8>, usize)>,
    ri: usize,
    n_comp: usize,
    baseline: bool,
    progressive: bool,
    jfif: bool,
    adobe_valid: bool,
    adobe_transform: u8,
    eob_run: u16,
    comp: [Component; 4],
    prog_coeffs: [Option<Vec<Block>>; 4],
    huff: Box<[[Huffman; 4]; 2]>,
    quant: [Block; 4],
    tmp: [u8; 2 * BLOCK_SIZE],
}
type Result<T> = std::result::Result<T, Error>;
pub(crate) fn decode(stream: &[u8]) -> Result<Image> {
    let mut d = Decoder::new(stream);
    d.decode(false)
        .and_then(|img| img.ok_or(Error::Format("missing SOF marker")))
}
pub(crate) fn decode_config(stream: &[u8]) -> Result<(usize, usize)> {
    let mut d = Decoder::new(stream);
    d.decode(true)?;
    match d.n_comp {
        1 | 3 | 4 => Ok((d.width, d.height)),
        _ => Err(Error::Format("missing SOF marker")),
    }
}
impl<'a> Decoder<'a> {
    fn new(src: &'a [u8]) -> Decoder<'a> {
        Decoder {
            src,
            src_pos: 0,
            bits: Bits::default(),
            buf: Box::new([0; 4096]),
            bi: 0,
            bj: 0,
            n_unreadable: 0,
            width: 0,
            height: 0,
            img1: None,
            img3: None,
            black: None,
            ri: 0,
            n_comp: 0,
            baseline: false,
            progressive: false,
            jfif: false,
            adobe_valid: false,
            adobe_transform: 0,
            eob_run: 0,
            comp: [Component::default(); 4],
            prog_coeffs: [None, None, None, None],
            huff: Box::default(),
            quant: [[0; 64]; 4],
            tmp: [0; 2 * BLOCK_SIZE],
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn fill(&mut self) -> Result<()> {
        assert_eq!(
            self.bi, self.bj,
            "jpeg: fill called when unread bytes exist"
        );
        if self.bj > 2 {
            self.buf[0] = self.buf[self.bj - 2];
            self.buf[1] = self.buf[self.bj - 1];
            self.bi = 2;
            self.bj = 2;
        }
        let n = (self.buf.len() - self.bj).min(self.src.len() - self.src_pos);
        self.buf[self.bj..self.bj + n].copy_from_slice(&self.src[self.src_pos..self.src_pos + n]);
        self.src_pos += n;
        self.bj += n;
        if n > 0 {
            Ok(())
        } else {
            Err(Error::UnexpectedEof)
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn unread_byte_stuffed_byte(&mut self) {
        self.bi -= self.n_unreadable;
        self.n_unreadable = 0;
        if self.bits.n >= 8 {
            self.bits.a >>= 8;
            self.bits.n -= 8;
            self.bits.m >>= 8;
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn read_byte(&mut self) -> Result<u8> {
        while self.bi == self.bj {
            self.fill()?;
        }
        let x = self.buf[self.bi];
        self.bi += 1;
        self.n_unreadable = 0;
        Ok(x)
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn read_byte_stuffed_byte(&mut self) -> Result<u8> {
        if self.bi + 2 <= self.bj {
            let x = self.buf[self.bi];
            self.bi += 1;
            self.n_unreadable = 1;
            if x != 0xff {
                return Ok(x);
            }
            if self.buf[self.bi] != 0x00 {
                return Err(ERR_MISSING_FF00);
            }
            self.bi += 1;
            self.n_unreadable = 2;
            return Ok(0xff);
        }
        self.n_unreadable = 0;
        let x = self.read_byte()?;
        self.n_unreadable = 1;
        if x != 0xff {
            return Ok(x);
        }
        let x = self.read_byte()?;
        self.n_unreadable = 2;
        if x != 0x00 {
            return Err(ERR_MISSING_FF00);
        }
        Ok(0xff)
    }
    fn unread_overshoot(&mut self) {
        if self.n_unreadable != 0 {
            if self.bits.n >= 8 {
                self.unread_byte_stuffed_byte();
            }
            self.n_unreadable = 0;
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn read_tmp(&mut self, from: usize, to: usize) -> Result<()> {
        self.unread_overshoot();
        let mut pos = from;
        loop {
            let n = (to - pos).min(self.bj - self.bi);
            self.tmp[pos..pos + n].copy_from_slice(&self.buf[self.bi..self.bi + n]);
            pos += n;
            self.bi += n;
            if pos == to {
                break;
            }
            self.fill()?;
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn read_vals(&mut self, tc: usize, th: usize, len: usize) -> Result<()> {
        self.unread_overshoot();
        let mut pos = 0;
        loop {
            let n = (len - pos).min(self.bj - self.bi);
            self.huff[tc][th].vals[pos..pos + n].copy_from_slice(&self.buf[self.bi..self.bi + n]);
            pos += n;
            self.bi += n;
            if pos == len {
                break;
            }
            self.fill()?;
        }
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn ignore(&mut self, mut n: usize) -> Result<()> {
        self.unread_overshoot();
        loop {
            let m = (self.bj - self.bi).min(n);
            self.bi += m;
            n -= m;
            if n == 0 {
                break;
            }
            self.fill()?;
        }
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn ensure_n_bits(&mut self, n: i32) -> Result<()> {
        loop {
            let c = match self.read_byte_stuffed_byte() {
                Ok(c) => c,
                Err(Error::UnexpectedEof) => return Err(ERR_SHORT_HUFFMAN),
                Err(e) => return Err(e),
            };
            self.bits.a = (self.bits.a << 8) | c as u32;
            self.bits.n += 8;
            if self.bits.m == 0 {
                self.bits.m = 1 << 7;
            } else {
                self.bits.m <<= 8;
            }
            if self.bits.n >= n {
                break;
            }
        }
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn receive_extend(&mut self, t: u8) -> Result<i32> {
        if self.bits.n < t as i32 {
            self.ensure_n_bits(t as i32)?;
        }
        self.bits.n -= t as i32;
        self.bits.m >>= t;
        let s = 1i32 << t;
        let mut x = ((self.bits.a >> self.bits.n as u32) as i32) & (s - 1);
        if x < s >> 1 {
            x += (-1i32 << t) + 1;
        }
        Ok(x)
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn process_dht(&mut self, mut n: i64) -> Result<()> {
        while n > 0 {
            if n < 17 {
                return Err(Error::Format("DHT has wrong length"));
            }
            self.read_tmp(0, 17)?;
            let tc = self.tmp[0] >> 4;
            if tc > MAX_TC {
                return Err(Error::Format("bad Tc value"));
            }
            let th = self.tmp[0] & 0x0f;
            if th > MAX_TH || (self.baseline && th > 1) {
                return Err(Error::Format("bad Th value"));
            }
            let (tc, th) = (tc as usize, th as usize);
            let mut n_codes = [0i32; MAX_CODE_LENGTH];
            let mut total = 0i32;
            for (i, count) in n_codes.iter_mut().enumerate() {
                *count = self.tmp[i + 1] as i32;
                total += *count;
            }
            self.huff[tc][th].n_codes = total;
            if total == 0 {
                return Err(Error::Format("Huffman table has zero length"));
            }
            if total > 256 {
                return Err(Error::Format("Huffman table has excessive length"));
            }
            n -= total as i64 + 17;
            if n < 0 {
                return Err(Error::Format("DHT has wrong length"));
            }
            self.read_vals(tc, th, total as usize)?;
            let h = &mut self.huff[tc][th];
            h.lut = [0; 256];
            let mut x = 0usize;
            let mut code = 0u32;
            for i in 0..LUT_SIZE {
                code <<= 1;
                for _ in 0..n_codes[i as usize] {
                    let base = (code << (7 - i)) as u8;
                    let lut_value = ((h.vals[x] as u16) << 8) | (2 + i) as u16;
                    for k in 0..(1u32 << (7 - i)) {
                        h.lut[(base | k as u8) as usize] = lut_value;
                    }
                    code += 1;
                    x += 1;
                }
            }
            let mut c = 0i32;
            let mut index = 0i32;
            for (i, &count) in n_codes.iter().enumerate() {
                if count == 0 {
                    h.min_codes[i] = -1;
                    h.max_codes[i] = -1;
                    h.vals_indices[i] = -1;
                } else {
                    h.min_codes[i] = c;
                    h.max_codes[i] = c + count - 1;
                    h.vals_indices[i] = index;
                    c += count;
                    index += count;
                }
                c <<= 1;
            }
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn decode_huffman(&mut self, tc: usize, th: usize) -> Result<u8> {
        if self.huff[tc][th].n_codes == 0 {
            return Err(Error::Format("uninitialized Huffman table"));
        }
        let mut slow = false;
        if self.bits.n < 8
            && let Err(e) = self.ensure_n_bits(8)
        {
            if e != ERR_MISSING_FF00 && e != ERR_SHORT_HUFFMAN {
                return Err(e);
            }
            if self.n_unreadable != 0 {
                self.unread_byte_stuffed_byte();
            }
            slow = true;
        }
        if !slow {
            let v = self.huff[tc][th].lut
                [((self.bits.a >> (self.bits.n - LUT_SIZE as i32) as u32) & 0xff) as usize];
            if v != 0 {
                let n = (v & 0xff) - 1;
                self.bits.n -= n as i32;
                self.bits.m >>= n;
                return Ok((v >> 8) as u8);
            }
        }
        let mut code = 0i32;
        for i in 0..MAX_CODE_LENGTH {
            if self.bits.n == 0 {
                self.ensure_n_bits(1)?;
            }
            if self.bits.a & self.bits.m != 0 {
                code |= 1;
            }
            self.bits.n -= 1;
            self.bits.m >>= 1;
            let h = &self.huff[tc][th];
            if code <= h.max_codes[i] {
                return Ok(h.vals[(h.vals_indices[i] + code - h.min_codes[i]) as usize]);
            }
            code <<= 1;
        }
        Err(Error::Format("bad Huffman code"))
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn decode_bit(&mut self) -> Result<bool> {
        if self.bits.n == 0 {
            self.ensure_n_bits(1)?;
        }
        let ret = self.bits.a & self.bits.m != 0;
        self.bits.n -= 1;
        self.bits.m >>= 1;
        Ok(ret)
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn decode_bits(&mut self, n: i32) -> Result<u32> {
        if self.bits.n < n {
            self.ensure_n_bits(n)?;
        }
        let mut ret = self.bits.a >> (self.bits.n - n) as u32;
        ret &= (1u32 << n as u32) - 1;
        self.bits.n -= n;
        self.bits.m >>= n as u32;
        Ok(ret)
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn process_sof(&mut self, n: i64) -> Result<()> {
        if self.n_comp != 0 {
            return Err(Error::Format("multiple SOF markers"));
        }
        self.n_comp = match n {
            9 => 1,
            15 => 3,
            18 => 4,
            _ => return Err(Error::Unsupported("number of components")),
        };
        self.read_tmp(0, n as usize)?;
        if self.tmp[0] != 8 {
            return Err(Error::Unsupported("precision"));
        }
        self.height = ((self.tmp[1] as usize) << 8) + self.tmp[2] as usize;
        self.width = ((self.tmp[3] as usize) << 8) + self.tmp[4] as usize;
        if self.tmp[5] as usize != self.n_comp {
            return Err(Error::Format("SOF has wrong length"));
        }
        for i in 0..self.n_comp {
            self.comp[i].c = self.tmp[6 + 3 * i];
            for j in 0..i {
                if self.comp[i].c == self.comp[j].c {
                    return Err(Error::Format("repeated component identifier"));
                }
            }
            self.comp[i].tq = self.tmp[8 + 3 * i];
            if self.comp[i].tq > MAX_TQ {
                return Err(Error::Format("bad Tq value"));
            }
            let hv = self.tmp[7 + 3 * i];
            let (mut h, mut v) = ((hv >> 4) as usize, (hv & 0x0f) as usize);
            if !(1..=4).contains(&h) || !(1..=4).contains(&v) {
                return Err(Error::Format("luma/chroma subsampling ratio"));
            }
            if h == 3 || v == 3 {
                return Err(ERR_SUBSAMPLING);
            }
            match self.n_comp {
                1 => {
                    h = 1;
                    v = 1;
                }
                3 => match i {
                    0 => {
                        if v == 4 {
                            return Err(ERR_SUBSAMPLING);
                        }
                    }
                    1 => {
                        if !self.comp[0].h.is_multiple_of(h) || !self.comp[0].v.is_multiple_of(v) {
                            return Err(ERR_SUBSAMPLING);
                        }
                    }
                    _ => {
                        if self.comp[1].h != h || self.comp[1].v != v {
                            return Err(ERR_SUBSAMPLING);
                        }
                    }
                },
                _ => match i {
                    0 => {
                        if hv != 0x11 && hv != 0x22 {
                            return Err(ERR_SUBSAMPLING);
                        }
                    }
                    1 | 2 => {
                        if hv != 0x11 {
                            return Err(ERR_SUBSAMPLING);
                        }
                    }
                    _ => {
                        if self.comp[0].h != h || self.comp[0].v != v {
                            return Err(ERR_SUBSAMPLING);
                        }
                    }
                },
            }
            self.comp[i].h = h;
            self.comp[i].v = v;
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn process_dqt(&mut self, mut n: i64) -> Result<()> {
        while n > 0 {
            n -= 1;
            let x = self.read_byte()?;
            let tq = (x & 0x0f) as usize;
            if tq > MAX_TQ as usize {
                return Err(Error::Format("bad Tq value"));
            }
            match x >> 4 {
                0 => {
                    if n < BLOCK_SIZE as i64 {
                        break;
                    }
                    n -= BLOCK_SIZE as i64;
                    self.read_tmp(0, BLOCK_SIZE)?;
                    for i in 0..BLOCK_SIZE {
                        self.quant[tq][i] = self.tmp[i] as i32;
                    }
                }
                1 => {
                    if n < 2 * BLOCK_SIZE as i64 {
                        break;
                    }
                    n -= 2 * BLOCK_SIZE as i64;
                    self.read_tmp(0, 2 * BLOCK_SIZE)?;
                    for i in 0..BLOCK_SIZE {
                        self.quant[tq][i] =
                            ((self.tmp[2 * i] as i32) << 8) | self.tmp[2 * i + 1] as i32;
                    }
                }
                _ => return Err(Error::Format("bad Pq value")),
            }
        }
        if n != 0 {
            return Err(Error::Format("DQT has wrong length"));
        }
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn process_dri(&mut self, n: i64) -> Result<()> {
        if n != 2 {
            return Err(Error::Format("DRI has wrong length"));
        }
        self.read_tmp(0, 2)?;
        self.ri = ((self.tmp[0] as usize) << 8) + self.tmp[1] as usize;
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn process_app0(&mut self, mut n: i64) -> Result<()> {
        if n < 5 {
            return self.ignore(n as usize);
        }
        self.read_tmp(0, 5)?;
        n -= 5;
        self.jfif = &self.tmp[..5] == b"JFIF\x00";
        if n > 0 {
            return self.ignore(n as usize);
        }
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn process_app14(&mut self, mut n: i64) -> Result<()> {
        if n < 12 {
            return self.ignore(n as usize);
        }
        self.read_tmp(0, 12)?;
        n -= 12;
        if &self.tmp[..5] == b"Adobe" {
            self.adobe_valid = true;
            self.adobe_transform = self.tmp[11];
        }
        if n > 0 {
            return self.ignore(n as usize);
        }
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn decode(&mut self, config_only: bool) -> Result<Option<Image>> {
        self.read_tmp(0, 2)?;
        if self.tmp[0] != 0xff || self.tmp[1] != SOI {
            return Err(Error::Format("missing SOI marker"));
        }
        loop {
            self.read_tmp(0, 2)?;
            while self.tmp[0] != 0xff {
                self.tmp[0] = self.tmp[1];
                self.tmp[1] = self.read_byte()?;
            }
            let mut marker = self.tmp[1];
            if marker == 0 {
                continue;
            }
            while marker == 0xff {
                marker = self.read_byte()?;
            }
            if marker == EOI {
                break;
            }
            if (RST0..=RST7).contains(&marker) {
                continue;
            }
            self.read_tmp(0, 2)?;
            let n = ((self.tmp[0] as i64) << 8) + self.tmp[1] as i64 - 2;
            if n < 0 {
                return Err(Error::Format("short segment length"));
            }
            match marker {
                SOF0 | SOF1 | SOF2 => {
                    self.baseline = marker == SOF0;
                    self.progressive = marker == SOF2;
                    let r = self.process_sof(n);
                    if config_only && self.jfif {
                        return r.map(|_| None);
                    }
                    r?;
                }
                DHT => {
                    if config_only {
                        self.ignore(n as usize)?;
                    } else {
                        self.process_dht(n)?;
                    }
                }
                DQT => {
                    if config_only {
                        self.ignore(n as usize)?;
                    } else {
                        self.process_dqt(n)?;
                    }
                }
                SOS => {
                    if config_only {
                        return Ok(None);
                    }
                    self.process_sos(n)?;
                }
                DRI => {
                    if config_only {
                        self.ignore(n as usize)?;
                    } else {
                        self.process_dri(n)?;
                    }
                }
                APP0 => self.process_app0(n)?,
                APP14 => self.process_app14(n)?,
                _ => {
                    if (APP0..=APP15).contains(&marker) || marker == COM {
                        self.ignore(n as usize)?;
                    } else if marker < 0xc0 {
                        return Err(Error::Format("unknown marker"));
                    } else {
                        return Err(Error::Unsupported("unknown marker"));
                    }
                }
            }
        }
        if self.progressive {
            self.reconstruct_progressive_image()?;
        }
        if let Some(img) = self.img1.take() {
            return Ok(Some(Image::Gray(img)));
        }
        if let Some(img3) = self.img3.take() {
            if let Some((black, stride)) = self.black.take() {
                return self.apply_black(img3, &black, stride).map(Some);
            } else if self.is_rgb() {
                return Ok(Some(self.convert_to_rgb(&img3)));
            }
            return Ok(Some(Image::YCbCr(img3)));
        }
        Err(Error::Format("missing SOS marker"))
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn apply_black(&self, img3: YCbCr, black: &[u8], black_stride: usize) -> Result<Image> {
        if !self.adobe_valid {
            return Err(Error::Unsupported(
                "unknown color model: 4-component JPEG doesn't have Adobe APP14 metadata",
            ));
        }
        let (w, h) = (img3.width, img3.height);
        if self.adobe_transform != 0 {
            let mut img = Quad {
                pix: vec![0; 4 * w * h],
                stride: 4 * w,
                width: w,
                height: h,
            };
            if matches!(
                img3.ratio,
                Ratio::R444 | Ratio::R422 | Ratio::R420 | Ratio::R440
            ) {
                for y in 0..h {
                    for x in 0..w {
                        let yi = img3.y_offset(x, y);
                        let ci = img3.c_offset(x, y);
                        let (r, g, b) = ycbcr_to_rgb(img3.y[yi], img3.cb[ci], img3.cr[ci]);
                        let o = y * img.stride + 4 * x;
                        img.pix[o] = r;
                        img.pix[o + 1] = g;
                        img.pix[o + 2] = b;
                        img.pix[o + 3] = 255;
                    }
                }
            }
            for y in 0..h {
                for x in 0..w {
                    img.pix[y * img.stride + 4 * x + 3] = 255 - black[y * black_stride + x];
                }
            }
            return Ok(Image::Cmyk(img));
        }
        let mut img = Quad {
            pix: vec![0; 4 * w * h],
            stride: 4 * w,
            width: w,
            height: h,
        };
        let planes: [(&[u8], usize); 4] = [
            (&img3.y, img3.y_stride),
            (&img3.cb, img3.c_stride),
            (&img3.cr, img3.c_stride),
            (black, black_stride),
        ];
        for (t, (src, stride)) in planes.iter().enumerate() {
            let subsample = self.comp[t].h != self.comp[0].h || self.comp[t].v != self.comp[0].v;
            for y in 0..h {
                let sy = if subsample { y / 2 } else { y };
                for x in 0..w {
                    let sx = if subsample { x / 2 } else { x };
                    img.pix[y * img.stride + 4 * x + t] = 255 - src[sy * stride + sx];
                }
            }
        }
        Ok(Image::Cmyk(img))
    }
    fn is_rgb(&self) -> bool {
        if self.jfif {
            return false;
        }
        if self.adobe_valid && self.adobe_transform == 0 {
            return true;
        }
        self.comp[0].c == b'R' && self.comp[1].c == b'G' && self.comp[2].c == b'B'
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn convert_to_rgb(&self, img3: &YCbCr) -> Image {
        let c_scale = self.comp[0].h / self.comp[1].h;
        let (w, h) = (img3.width, img3.height);
        let mut img = Quad {
            pix: vec![0; 4 * w * h],
            stride: 4 * w,
            width: w,
            height: h,
        };
        for y in 0..h {
            let po = y * img.stride;
            let yo = img3.y_offset(0, y);
            let co = img3.c_offset(0, y);
            for i in 0..w {
                img.pix[po + 4 * i] = img3.y[yo + i];
                img.pix[po + 4 * i + 1] = img3.cb[co + i / c_scale];
                img.pix[po + 4 * i + 2] = img3.cr[co + i / c_scale];
                img.pix[po + 4 * i + 3] = 255;
            }
        }
        Image::Rgba(img)
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn make_img(&mut self, mxx: usize, myy: usize) -> Result<()> {
        if self.n_comp == 1 {
            let stride = 8 * mxx;
            let (w, h) = if self.width == 0 || self.height == 0 {
                (0, 0)
            } else {
                (self.width, self.height)
            };
            self.img1 = Some(Gray {
                pix: vec![0; stride * 8 * myy],
                stride,
                width: w,
                height: h,
            });
            return Ok(());
        }
        let h0 = self.comp[0].h;
        let v0 = self.comp[0].v;
        let h_ratio = h0 / self.comp[1].h;
        let v_ratio = v0 / self.comp[1].v;
        let ratio = match (h_ratio << 4) | v_ratio {
            0x11 => Ratio::R444,
            0x12 => Ratio::R440,
            0x21 => Ratio::R422,
            0x22 => Ratio::R420,
            0x41 => Ratio::R411,
            0x42 => Ratio::R410,
            _ => return Err(ERR_SUBSAMPLING),
        };
        self.img3 = Some(YCbCr::new(
            8 * h0 * mxx,
            8 * v0 * myy,
            ratio,
            self.width,
            self.height,
        ));
        if self.n_comp == 4 {
            let (h3, v3) = (self.comp[3].h, self.comp[3].v);
            self.black = Some((vec![0; 8 * h3 * mxx * 8 * v3 * myy], 8 * h3 * mxx));
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn process_sos(&mut self, n: i64) -> Result<()> {
        if self.n_comp == 0 {
            return Err(Error::Format("missing SOF marker"));
        }
        if n < 6 || 4 + 2 * (self.n_comp as i64) < n || n % 2 != 0 {
            return Err(Error::Format("SOS has wrong length"));
        }
        self.read_tmp(0, n as usize)?;
        let n_comp = self.tmp[0] as usize;
        if n as usize != 4 + 2 * n_comp {
            return Err(Error::Format(
                "SOS length inconsistent with number of components",
            ));
        }
        #[derive(Clone, Copy, Default)]
        struct ScanComp {
            comp_index: usize,
            td: u8,
            ta: u8,
        }
        let mut scan = [ScanComp::default(); 4];
        let mut total_hv = 0;
        for i in 0..n_comp {
            let cs = self.tmp[1 + 2 * i];
            let mut comp_index: i32 = -1;
            for (j, comp) in self.comp[..self.n_comp].iter().enumerate() {
                if cs == comp.c {
                    comp_index = j as i32;
                }
            }
            if comp_index < 0 {
                return Err(Error::Format("unknown component selector"));
            }
            scan[i].comp_index = comp_index as usize;
            for j in 0..i {
                if scan[i].comp_index == scan[j].comp_index {
                    return Err(Error::Format("repeated component selector"));
                }
            }
            total_hv += self.comp[comp_index as usize].h * self.comp[comp_index as usize].v;
            scan[i].td = self.tmp[2 + 2 * i] >> 4;
            let t = scan[i].td;
            if t > MAX_TH || (self.baseline && t > 1) {
                return Err(Error::Format("bad Td value"));
            }
            scan[i].ta = self.tmp[2 + 2 * i] & 0x0f;
            let t = scan[i].ta;
            if t > MAX_TH || (self.baseline && t > 1) {
                return Err(Error::Format("bad Ta value"));
            }
        }
        if self.n_comp > 1 && total_hv > 10 {
            return Err(Error::Format("total sampling factors too large"));
        }
        let (mut zig_start, mut zig_end, mut ah, mut al) =
            (0i32, BLOCK_SIZE as i32 - 1, 0u32, 0u32);
        if self.progressive {
            zig_start = self.tmp[1 + 2 * n_comp] as i32;
            zig_end = self.tmp[2 + 2 * n_comp] as i32;
            ah = (self.tmp[3 + 2 * n_comp] >> 4) as u32;
            al = (self.tmp[3 + 2 * n_comp] & 0x0f) as u32;
            if (zig_start == 0 && zig_end != 0)
                || zig_start > zig_end
                || BLOCK_SIZE as i32 <= zig_end
            {
                return Err(Error::Format("bad spectral selection bounds"));
            }
            if zig_start != 0 && n_comp != 1 {
                return Err(Error::Format(
                    "progressive AC coefficients for more than one component",
                ));
            }
            if ah != 0 && ah != al + 1 {
                return Err(Error::Format("bad successive approximation values"));
            }
        }
        let (h0, v0) = (self.comp[0].h, self.comp[0].v);
        let mxx = self.width.div_ceil(8 * h0);
        let myy = self.height.div_ceil(8 * v0);
        if self.img1.is_none() && self.img3.is_none() {
            self.make_img(mxx, myy)?;
        }
        if self.progressive {
            for s in &scan[..n_comp] {
                let ci = s.comp_index;
                if self.prog_coeffs[ci].is_none() {
                    self.prog_coeffs[ci] =
                        Some(vec![[0; 64]; mxx * myy * self.comp[ci].h * self.comp[ci].v]);
                }
            }
        }
        self.bits = Bits::default();
        let mut mcu = 0usize;
        let mut expected_rst = RST0;
        let mut dc = [0i32; 4];
        let mut block_count = 0usize;
        for my in 0..myy {
            for mx in 0..mxx {
                for s in &scan[..n_comp] {
                    let ci = s.comp_index;
                    let hi = self.comp[ci].h;
                    let vi = self.comp[ci].v;
                    for j in 0..hi * vi {
                        let (bx, by);
                        if n_comp != 1 {
                            bx = hi * mx + j % hi;
                            by = vi * my + j / hi;
                        } else {
                            let q = mxx * hi;
                            bx = block_count % q;
                            by = block_count / q;
                            block_count += 1;
                            if bx * 8 >= self.width || by * 8 >= self.height {
                                continue;
                            }
                        }
                        let mut b: Block = if self.progressive {
                            self.prog_coeffs[ci].as_ref().ok_or(ERR_NO_IMAGE)?[by * mxx * hi + bx]
                        } else {
                            [0; 64]
                        };
                        if ah != 0 {
                            self.refine(&mut b, s.ta as usize, zig_start, zig_end, 1 << al)?;
                        } else {
                            let mut zig = zig_start;
                            if zig == 0 {
                                zig += 1;
                                let value = self.decode_huffman(0, s.td as usize)?;
                                if value > 16 {
                                    return Err(Error::Unsupported("excessive DC component"));
                                }
                                let dc_delta = self.receive_extend(value)?;
                                dc[ci] = dc[ci].wrapping_add(dc_delta);
                                b[0] = dc[ci].wrapping_shl(al);
                            }
                            if zig <= zig_end && self.eob_run > 0 {
                                self.eob_run -= 1;
                            } else {
                                let ta = s.ta as usize;
                                while zig <= zig_end {
                                    let value = self.decode_huffman(1, ta)?;
                                    let val0 = value >> 4;
                                    let val1 = value & 0x0f;
                                    if val1 != 0 {
                                        zig += val0 as i32;
                                        if zig > zig_end {
                                            break;
                                        }
                                        let ac = self.receive_extend(val1)?;
                                        b[UNZIG[zig as usize]] = ac.wrapping_shl(al);
                                    } else {
                                        if val0 != 0x0f {
                                            self.eob_run = 1u16 << val0;
                                            if val0 != 0 {
                                                let bits = self.decode_bits(val0 as i32)?;
                                                self.eob_run |= bits as u16;
                                            }
                                            self.eob_run = self.eob_run.wrapping_sub(1);
                                            break;
                                        }
                                        zig += 0x0f;
                                    }
                                    zig += 1;
                                }
                            }
                        }
                        if self.progressive {
                            self.prog_coeffs[ci].as_mut().ok_or(ERR_NO_IMAGE)?
                                [by * mxx * hi + bx] = b;
                            continue;
                        }
                        self.reconstruct_block(&mut b, bx, by, ci)?;
                    }
                }
                mcu += 1;
                if self.ri > 0 && mcu.is_multiple_of(self.ri) && mcu < mxx * myy {
                    self.read_tmp(0, 2)?;
                    if self.tmp[0] != 0xff || self.tmp[1] != expected_rst {
                        self.find_rst(expected_rst)?;
                    }
                    expected_rst += 1;
                    if expected_rst == RST7 + 1 {
                        expected_rst = RST0;
                    }
                    self.bits = Bits::default();
                    dc = [0; 4];
                    self.eob_run = 0;
                }
            }
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn refine(
        &mut self,
        b: &mut Block,
        ta: usize,
        zig_start: i32,
        zig_end: i32,
        delta: i32,
    ) -> Result<()> {
        if zig_start == 0 {
            if zig_end != 0 {
                return Err(Error::Format("bad spectral selection bounds"));
            }
            if self.decode_bit()? {
                b[0] |= delta;
            }
            return Ok(());
        }
        let mut zig = zig_start;
        if self.eob_run == 0 {
            while zig <= zig_end {
                let mut z = 0i32;
                let value = self.decode_huffman(1, ta)?;
                let val0 = value >> 4;
                let val1 = value & 0x0f;
                match val1 {
                    0 => {
                        if val0 != 0x0f {
                            self.eob_run = 1u16 << val0;
                            if val0 != 0 {
                                let bits = self.decode_bits(val0 as i32)?;
                                self.eob_run |= bits as u16;
                            }
                            break;
                        }
                    }
                    1 => {
                        z = delta;
                        if !self.decode_bit()? {
                            z = -z;
                        }
                    }
                    _ => return Err(Error::Format("unexpected Huffman code")),
                }
                zig = self.refine_non_zeroes(b, zig, zig_end, val0 as i32, delta)?;
                if zig > zig_end {
                    return Err(Error::Format("too many coefficients"));
                }
                if z != 0 {
                    b[UNZIG[zig as usize]] = z;
                }
                zig += 1;
            }
        }
        if self.eob_run > 0 {
            self.eob_run -= 1;
            self.refine_non_zeroes(b, zig, zig_end, -1, delta)?;
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn refine_non_zeroes(
        &mut self,
        b: &mut Block,
        mut zig: i32,
        zig_end: i32,
        mut nz: i32,
        delta: i32,
    ) -> Result<i32> {
        while zig <= zig_end {
            let u = UNZIG[zig as usize];
            if b[u] == 0 {
                if nz == 0 {
                    break;
                }
                nz -= 1;
                zig += 1;
                continue;
            }
            if !self.decode_bit()? {
                zig += 1;
                continue;
            }
            if b[u] >= 0 {
                b[u] = b[u].wrapping_add(delta);
            } else {
                b[u] = b[u].wrapping_sub(delta);
            }
            zig += 1;
        }
        Ok(zig)
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn reconstruct_progressive_image(&mut self) -> Result<()> {
        let h0 = self.comp[0].h;
        let mxx = self.width.div_ceil(8 * h0);
        for i in 0..self.n_comp {
            let Some(mut coeffs) = self.prog_coeffs[i].take() else {
                continue;
            };
            let v = 8 * self.comp[0].v / self.comp[i].v;
            let h = 8 * self.comp[0].h / self.comp[i].h;
            let stride = mxx * self.comp[i].h;
            let mut by = 0;
            while by * v < self.height {
                let mut bx = 0;
                while bx * h < self.width {
                    self.reconstruct_block(&mut coeffs[by * stride + bx], bx, by, i)?;
                    bx += 1;
                }
                by += 1;
            }
            self.prog_coeffs[i] = Some(coeffs);
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn reconstruct_block(&mut self, b: &mut Block, bx: usize, by: usize, ci: usize) -> Result<()> {
        let qt = self.quant[self.comp[ci].tq as usize];
        for zig in 0..BLOCK_SIZE {
            b[UNZIG[zig]] = b[UNZIG[zig]].wrapping_mul(qt[zig]);
        }
        idct(b);
        let (dst, start, stride): (&mut [u8], usize, usize) = if self.n_comp == 1 {
            let img = self.img1.as_mut().ok_or(ERR_NO_IMAGE)?;
            (&mut img.pix, 8 * (by * img.stride + bx), img.stride)
        } else {
            let img = self.img3.as_mut().ok_or(ERR_NO_IMAGE)?;
            match ci {
                0 => (&mut img.y, 8 * (by * img.y_stride + bx), img.y_stride),
                1 => (&mut img.cb, 8 * (by * img.c_stride + bx), img.c_stride),
                2 => (&mut img.cr, 8 * (by * img.c_stride + bx), img.c_stride),
                3 => {
                    let (pix, stride) = self.black.as_mut().ok_or(ERR_NO_IMAGE)?;
                    (pix, 8 * (by * *stride + bx), *stride)
                }
                _ => return Err(Error::Unsupported("too many components")),
            }
        };
        let dst = &mut dst[start..];
        for y in 0..8 {
            for x in 0..8 {
                let mut c = b[y * 8 + x];
                if c < -128 {
                    c = 0;
                } else if c > 127 {
                    c = 255;
                } else {
                    c += 128;
                }
                dst[y * stride + x] = c as u8;
            }
        }
        Ok(())
    }
    fn find_rst(&mut self, expected_rst: u8) -> Result<()> {
        loop {
            let mut i = 0;
            if self.tmp[0] == 0xff {
                if self.tmp[1] == expected_rst {
                    return Ok(());
                } else if self.tmp[1] == 0xff {
                    i = 1;
                } else if self.tmp[1] != 0x00 {
                    return Err(Error::Format("bad RST marker"));
                }
            } else if self.tmp[1] == 0xff {
                self.tmp[0] = 0xff;
                i = 1;
            }
            self.read_tmp(i, 2)?;
        }
    }
}
