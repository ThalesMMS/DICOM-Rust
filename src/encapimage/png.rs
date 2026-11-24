#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Bad;
type Res<T> = Result<T, Bad>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Model {
    Gray,
    Gray16,
    Colour8,
    Colour16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Config {
    pub width: usize,
    pub height: usize,
    pub model: Model,
}
pub(crate) struct Image {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[u16; 3]>,
    pub opaque: bool,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Cb {
    G1,
    G2,
    G4,
    G8,
    Ga8,
    Tc8,
    P1,
    P2,
    P4,
    P8,
    Tca8,
    G16,
    Ga16,
    Tc16,
    Tca16,
}
impl Cb {
    fn paletted(self) -> bool {
        matches!(self, Cb::P1 | Cb::P2 | Cb::P4 | Cb::P8)
    }
    fn true_colour(self) -> bool {
        matches!(self, Cb::Tc8 | Cb::Tc16)
    }
    fn bits_per_pixel(self) -> usize {
        match self {
            Cb::G1 | Cb::P1 => 1,
            Cb::G2 | Cb::P2 => 2,
            Cb::G4 | Cb::P4 => 4,
            Cb::G8 | Cb::P8 => 8,
            Cb::Ga8 | Cb::G16 => 16,
            Cb::Tc8 => 24,
            Cb::Tca8 | Cb::Ga16 => 32,
            Cb::Tc16 => 48,
            Cb::Tca16 => 64,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Start,
    SeenIhdr,
    SeenPlte,
    SeenTrns,
    SeenIdat,
    SeenIend,
}
const INTERLACING: [(usize, usize, usize, usize); 7] = [
    (8, 8, 0, 0),
    (8, 8, 4, 0),
    (4, 8, 0, 4),
    (4, 4, 2, 0),
    (2, 4, 0, 2),
    (2, 2, 1, 0),
    (1, 2, 0, 1),
];
const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";
const BUFFER_SIZE: usize = 4096;
fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut crc = !crc;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
struct Decoder<'a> {
    data: &'a [u8],
    pos: usize,
    crc: u32,
    width: usize,
    height: usize,
    depth: u8,
    palette: [[u8; 4]; 256],
    palette_len: usize,
    cb: Option<Cb>,
    stage: Stage,
    idat_length: u32,
    interlaced: bool,
    use_transparent: bool,
    transparent: [u8; 6],
    image: Option<Image>,
    buffered: &'a [u8],
}
impl<'a> Decoder<'a> {
    fn new(data: &'a [u8]) -> Decoder<'a> {
        Decoder {
            data,
            pos: 0,
            crc: 0,
            width: 0,
            height: 0,
            depth: 0,
            palette: [[0, 0, 0, 0xff]; 256],
            palette_len: 0,
            cb: None,
            stage: Stage::Start,
            idat_length: 0,
            interlaced: false,
            use_transparent: false,
            transparent: [0; 6],
            image: None,
            buffered: &[],
        }
    }
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.data.len());
        let Some(end) = end else {
            self.pos = self.data.len();
            return Err(Bad);
        };
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }
    fn take_payload(&mut self, n: usize) -> Res<&'a [u8]> {
        let out = self.take(n)?;
        self.crc = crc32_update(self.crc, out);
        Ok(out)
    }
    fn verify_checksum(&mut self) -> Res<()> {
        if be32(self.take(4)?) != self.crc {
            return Err(Bad);
        }
        Ok(())
    }
    fn check_header(&mut self) -> Res<()> {
        if self.take(PNG_HEADER.len())? != PNG_HEADER {
            return Err(Bad);
        }
        Ok(())
    }
    fn parse_ihdr(&mut self, length: u32) -> Res<()> {
        if length != 13 {
            return Err(Bad);
        }
        let h = self.take_payload(13)?;
        if h[10] != 0 || h[11] != 0 || h[12] > 1 {
            return Err(Bad);
        }
        self.interlaced = h[12] == 1;
        let (w, h32) = (be32(h) as i32, be32(&h[4..]) as i32);
        if w <= 0 || h32 <= 0 {
            return Err(Bad);
        }
        let pixels = i64::from(w) * i64::from(h32);
        if pixels.checked_mul(8).is_none() {
            return Err(Bad);
        }
        self.depth = h[8];
        self.cb = Some(match (h[8], h[9]) {
            (1, 0) => Cb::G1,
            (1, 3) => Cb::P1,
            (2, 0) => Cb::G2,
            (2, 3) => Cb::P2,
            (4, 0) => Cb::G4,
            (4, 3) => Cb::P4,
            (8, 0) => Cb::G8,
            (8, 2) => Cb::Tc8,
            (8, 3) => Cb::P8,
            (8, 4) => Cb::Ga8,
            (8, 6) => Cb::Tca8,
            (16, 0) => Cb::G16,
            (16, 2) => Cb::Tc16,
            (16, 4) => Cb::Ga16,
            (16, 6) => Cb::Tca16,
            _ => return Err(Bad),
        });
        (self.width, self.height) = (w as usize, h32 as usize);
        self.verify_checksum()
    }
    fn parse_plte(&mut self, length: u32) -> Res<()> {
        let np = (length / 3) as usize;
        if !length.is_multiple_of(3) || np == 0 || np > 256 || np > 1usize << self.depth.min(16) {
            return Err(Bad);
        }
        let entries = self.take_payload(3 * np)?;
        match self.cb {
            Some(cb) if cb.paletted() => {
                for (slot, rgb) in self.palette.iter_mut().zip(entries.as_chunks::<3>().0) {
                    *slot = [rgb[0], rgb[1], rgb[2], 0xff];
                }
                self.palette_len = np;
            }
            Some(Cb::Tc8 | Cb::Tca8 | Cb::Tc16 | Cb::Tca16) => {}
            _ => return Err(Bad),
        }
        self.verify_checksum()
    }
    fn parse_trns(&mut self, length: u32) -> Res<()> {
        let Some(cb) = self.cb else {
            return Err(Bad);
        };
        match cb {
            Cb::G1 | Cb::G2 | Cb::G4 | Cb::G8 | Cb::G16 => {
                if length != 2 {
                    return Err(Bad);
                }
                let v = self.take_payload(2)?;
                self.transparent[..2].copy_from_slice(v);
                let scale = match cb {
                    Cb::G1 => 0xff,
                    Cb::G2 => 0x55,
                    Cb::G4 => 0x11,
                    _ => 1,
                };
                self.transparent[1] = self.transparent[1].wrapping_mul(scale);
                self.use_transparent = true;
            }
            Cb::Tc8 | Cb::Tc16 => {
                if length != 6 {
                    return Err(Bad);
                }
                let v = self.take_payload(6)?;
                self.transparent.copy_from_slice(v);
                self.use_transparent = true;
            }
            Cb::P1 | Cb::P2 | Cb::P4 | Cb::P8 => {
                if length > 256 {
                    return Err(Bad);
                }
                let alphas = self.take_payload(length as usize)?;
                self.palette_len = self.palette_len.max(alphas.len());
                for (slot, &alpha) in self.palette.iter_mut().zip(alphas) {
                    slot[3] = alpha;
                }
            }
            _ => return Err(Bad),
        }
        self.verify_checksum()
    }
    fn read_idat(&mut self, max: usize) -> Res<&'a [u8]> {
        while self.idat_length == 0 {
            self.verify_checksum()?;
            let head = self.take(8)?;
            self.idat_length = be32(head);
            if &head[4..] != b"IDAT" {
                return Err(Bad);
            }
            self.crc = crc32_update(0, b"IDAT");
        }
        let n = max
            .min(self.idat_length as usize)
            .min(self.data.len() - self.pos);
        if n == 0 {
            return Err(Bad);
        }
        let out = self.take_payload(n)?;
        self.idat_length -= n as u32;
        Ok(out)
    }
    fn read_byte(&mut self) -> Res<u8> {
        if self.buffered.is_empty() {
            self.buffered = self.read_idat(BUFFER_SIZE)?;
        }
        let b = self.buffered[0];
        self.buffered = &self.buffered[1..];
        Ok(b)
    }
    fn read_full(&mut self, n: usize, out: &mut Vec<u8>) -> Res<()> {
        let mut need = n;
        while need > 0 {
            if !self.buffered.is_empty() {
                let k = need.min(self.buffered.len());
                out.extend_from_slice(&self.buffered[..k]);
                self.buffered = &self.buffered[k..];
                need -= k;
            } else if need >= BUFFER_SIZE {
                let direct = self.read_idat(need)?;
                out.extend_from_slice(direct);
                need -= direct.len();
            } else {
                self.buffered = self.read_idat(BUFFER_SIZE)?;
            }
        }
        Ok(())
    }
    fn parse_idat(&mut self, length: u32) -> Res<()> {
        self.idat_length = length;
        self.buffered = &[];
        self.image = Some(self.decode()?);
        self.verify_checksum()
    }
    fn passes(&self) -> Vec<(usize, usize, usize)> {
        if !self.interlaced {
            return vec![(0, self.width, self.height)];
        }
        INTERLACING
            .iter()
            .enumerate()
            .map(|(pass, &(xf, yf, xo, yo))| {
                let width = (self.width + xf - 1).saturating_sub(xo) / xf;
                let height = (self.height + yf - 1).saturating_sub(yo) / yf;
                (pass, width, height)
            })
            .filter(|&(_, width, height)| width != 0 && height != 0)
            .collect()
    }
    fn decode(&mut self) -> Res<Image> {
        let Some(cb) = self.cb else {
            return Err(Bad);
        };
        let bits_per_pixel = cb.bits_per_pixel();
        let passes = self.passes();
        let row_size = |width: usize| 1 + (bits_per_pixel * width).div_ceil(8);
        let needed: usize = passes
            .iter()
            .map(|&(_, width, height)| row_size(width) * height)
            .sum();
        let mut head = Vec::with_capacity(4);
        self.read_full(2, &mut head)?;
        let h = u16::from_be_bytes([head[0], head[1]]);
        if head[0] & 0x0f != 8 || head[0] >> 4 > 7 || h % 31 != 0 {
            return Err(Bad);
        }
        if head[1] & 0x20 != 0 {
            head.clear();
            self.read_full(4, &mut head)?;
            if be32(&head) != 1 {
                return Err(Bad);
            }
        }
        let raw = Inflater::new(self, needed).run()?;
        if raw.len() != needed {
            return Err(Bad);
        }
        let mut checksum = Vec::with_capacity(4);
        self.read_full(4, &mut checksum)?;
        if be32(&checksum) != adler32(&raw) {
            return Err(Bad);
        }
        if self.idat_length != 0 {
            return Err(Bad);
        }
        let mut image = Image {
            width: self.width,
            height: self.height,
            pixels: vec![[0; 3]; self.width * self.height],
            opaque: true,
        };
        let bytes_per_pixel = bits_per_pixel.div_ceil(8);
        let mut rows = raw.as_slice();
        for (pass, width, height) in passes {
            let (xf, yf, xo, yo) = if self.interlaced {
                INTERLACING[pass]
            } else {
                (1, 1, 0, 0)
            };
            let size = row_size(width);
            let mut previous = vec![0u8; size - 1];
            let mut current = vec![0u8; size - 1];
            for y in 0..height {
                let (row, rest) = rows.split_at(size);
                rows = rest;
                current.copy_from_slice(&row[1..]);
                unfilter(row[0], &mut current, &previous, bytes_per_pixel)?;
                for x in 0..width {
                    let (rgb, opaque) = self.colour(cb, &current, x);
                    image.pixels[(y * yf + yo) * self.width + x * xf + xo] = rgb;
                    image.opaque &= opaque;
                }
                std::mem::swap(&mut previous, &mut current);
            }
        }
        Ok(image)
    }
    fn colour(&self, cb: Cb, row: &[u8], x: usize) -> ([u16; 3], bool) {
        let wide = |c: u8| u16::from(c) * 0x101;
        let word = |i: usize| u16::from(row[i]) << 8 | u16::from(row[i + 1]);
        let packed = |bits: usize| {
            let shift = 8 - bits - (x * bits) % 8;
            (row[x * bits / 8] >> shift) & ((1u8 << bits) - 1)
        };
        let grey8 = |y: u8| {
            let opaque = !(self.use_transparent && y == self.transparent[1]);
            ([wide(y); 3], opaque)
        };
        let indexed = |index: u8| {
            let [r, g, b, a] = self.palette[usize::from(index)];
            ([wide(r), wide(g), wide(b)], a == 0xff)
        };
        match cb {
            Cb::G1 => grey8(packed(1) * 0xff),
            Cb::G2 => grey8(packed(2) * 0x55),
            Cb::G4 => grey8(packed(4) * 0x11),
            Cb::G8 => grey8(row[x]),
            Cb::Ga8 => ([wide(row[2 * x]); 3], row[2 * x + 1] == 0xff),
            Cb::Tc8 => {
                let (r, g, b) = (row[3 * x], row[3 * x + 1], row[3 * x + 2]);
                let t = &self.transparent;
                let opaque = !(self.use_transparent && r == t[1] && g == t[3] && b == t[5]);
                ([wide(r), wide(g), wide(b)], opaque)
            }
            Cb::P1 => indexed(packed(1)),
            Cb::P2 => indexed(packed(2)),
            Cb::P4 => indexed(packed(4)),
            Cb::P8 => indexed(row[x]),
            Cb::Tca8 => (
                [wide(row[4 * x]), wide(row[4 * x + 1]), wide(row[4 * x + 2])],
                row[4 * x + 3] == 0xff,
            ),
            Cb::G16 => {
                let y = word(2 * x);
                let t = u16::from(self.transparent[0]) << 8 | u16::from(self.transparent[1]);
                ([y; 3], !(self.use_transparent && y == t))
            }
            Cb::Ga16 => ([word(4 * x); 3], word(4 * x + 2) == 0xffff),
            Cb::Tc16 => {
                let rgb = [word(6 * x), word(6 * x + 2), word(6 * x + 4)];
                let t = &self.transparent;
                let key = [
                    u16::from(t[0]) << 8 | u16::from(t[1]),
                    u16::from(t[2]) << 8 | u16::from(t[3]),
                    u16::from(t[4]) << 8 | u16::from(t[5]),
                ];
                (rgb, !(self.use_transparent && rgb == key))
            }
            Cb::Tca16 => (
                [word(8 * x), word(8 * x + 2), word(8 * x + 4)],
                word(8 * x + 6) == 0xffff,
            ),
        }
    }
    fn parse_chunk(&mut self, config_only: bool) -> Res<()> {
        let head = self.take(8)?;
        let length = be32(head);
        self.crc = crc32_update(0, &head[4..]);
        match &head[4..] {
            b"IHDR" => {
                if self.stage != Stage::Start {
                    return Err(Bad);
                }
                self.stage = Stage::SeenIhdr;
                return self.parse_ihdr(length);
            }
            b"PLTE" => {
                if self.stage != Stage::SeenIhdr {
                    return Err(Bad);
                }
                self.stage = Stage::SeenPlte;
                return self.parse_plte(length);
            }
            b"tRNS" => {
                let admitted = match self.cb {
                    Some(cb) if cb.paletted() => self.stage == Stage::SeenPlte,
                    Some(cb) if cb.true_colour() => {
                        self.stage == Stage::SeenIhdr || self.stage == Stage::SeenPlte
                    }
                    _ => self.stage == Stage::SeenIhdr,
                };
                if !admitted {
                    return Err(Bad);
                }
                self.stage = Stage::SeenTrns;
                return self.parse_trns(length);
            }
            b"IDAT" => {
                let paletted = self.cb.is_some_and(Cb::paletted);
                if self.stage < Stage::SeenIhdr
                    || self.stage > Stage::SeenIdat
                    || (self.stage == Stage::SeenIhdr && paletted)
                {
                    return Err(Bad);
                }
                if self.stage != Stage::SeenIdat {
                    self.stage = Stage::SeenIdat;
                    if config_only {
                        return Ok(());
                    }
                    return self.parse_idat(length);
                }
            }
            b"IEND" => {
                if self.stage != Stage::SeenIdat {
                    return Err(Bad);
                }
                self.stage = Stage::SeenIend;
                if length != 0 {
                    return Err(Bad);
                }
                return self.verify_checksum();
            }
            _ => {}
        }
        if length > 0x7fff_ffff {
            return Err(Bad);
        }
        self.take_payload(length as usize)?;
        self.verify_checksum()
    }
}
fn unfilter(filter: u8, row: &mut [u8], previous: &[u8], bytes_per_pixel: usize) -> Res<()> {
    let bpp = bytes_per_pixel;
    match filter {
        0 => {}
        1 => {
            for i in bpp..row.len() {
                row[i] = row[i].wrapping_add(row[i - bpp]);
            }
        }
        2 => {
            for (c, &p) in row.iter_mut().zip(previous) {
                *c = c.wrapping_add(p);
            }
        }
        3 => {
            for i in 0..bpp.min(row.len()) {
                row[i] = row[i].wrapping_add(previous[i] / 2);
            }
            for i in bpp..row.len() {
                let average = (u16::from(row[i - bpp]) + u16::from(previous[i])) / 2;
                row[i] = row[i].wrapping_add(average as u8);
            }
        }
        4 => {
            for i in 0..row.len() {
                let a = if i >= bpp { i32::from(row[i - bpp]) } else { 0 };
                let b = i32::from(previous[i]);
                let c = if i >= bpp {
                    i32::from(previous[i - bpp])
                } else {
                    0
                };
                let (pa, pb) = ((b - c).abs(), (a - c).abs());
                let pc = (b - c + a - c).abs();
                let predictor = if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                };
                row[i] = row[i].wrapping_add(predictor as u8);
            }
        }
        _ => return Err(Bad),
    }
    Ok(())
}
pub(crate) fn decode_config(data: &[u8]) -> Res<Config> {
    let mut d = Decoder::new(data);
    d.check_header()?;
    loop {
        d.parse_chunk(true)?;
        let paletted = d.cb.is_some_and(Cb::paletted);
        if (paletted && d.stage >= Stage::SeenTrns) || (!paletted && d.stage >= Stage::SeenIhdr) {
            break;
        }
    }
    let model = match d.cb {
        Some(Cb::G1 | Cb::G2 | Cb::G4 | Cb::G8) => Model::Gray,
        Some(Cb::G16) => Model::Gray16,
        Some(Cb::Ga16 | Cb::Tc16 | Cb::Tca16) => Model::Colour16,
        _ => Model::Colour8,
    };
    Ok(Config {
        width: d.width,
        height: d.height,
        model,
    })
}
pub(crate) fn decode(data: &[u8]) -> Res<Image> {
    let mut d = Decoder::new(data);
    d.check_header()?;
    while d.stage != Stage::SeenIend {
        d.parse_chunk(false)?;
    }
    d.image.take().ok_or(Bad)
}
const MAX_CODE_LEN: usize = 16;
const MAX_NUM_LIT: usize = 286;
const MAX_NUM_DIST: usize = 30;
const NUM_CODES: usize = 19;
const WINDOW: usize = 1 << 15;
const END_BLOCK_MARKER: usize = 256;
const CHUNK_BITS: u32 = 9;
const NUM_CHUNKS: usize = 1 << CHUNK_BITS;
const COUNT_MASK: u32 = 15;
const VALUE_SHIFT: u32 = 4;
const CODE_ORDER: [usize; NUM_CODES] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
#[derive(Clone, Default)]
struct Huffman {
    min: usize,
    chunks: Vec<u32>,
    links: Vec<Vec<u32>>,
    link_mask: u32,
}
impl Huffman {
    fn init(&mut self, lengths: &[usize]) -> bool {
        *self = Huffman {
            chunks: vec![0; NUM_CHUNKS],
            ..Huffman::default()
        };
        let mut count = [0usize; MAX_CODE_LEN];
        let (mut min, mut max) = (0usize, 0usize);
        for &n in lengths.iter().filter(|&&n| n != 0) {
            if min == 0 || n < min {
                min = n;
            }
            max = max.max(n);
            count[n] += 1;
        }
        if max == 0 {
            return true;
        }
        let mut code = 0usize;
        let mut next_code = [0usize; MAX_CODE_LEN];
        for i in min..=max {
            code <<= 1;
            next_code[i] = code;
            code += count[i];
        }
        if code != 1 << max && !(code == 1 && max == 1) {
            return false;
        }
        self.min = min;
        let chunk_bits = CHUNK_BITS as usize;
        if max > chunk_bits {
            let num_links = 1usize << (max - chunk_bits);
            self.link_mask = (num_links - 1) as u32;
            let link = next_code[chunk_bits + 1] >> 1;
            self.links = vec![vec![0; num_links]; NUM_CHUNKS - link];
            for j in link..NUM_CHUNKS {
                let reverse = usize::from((j as u16).reverse_bits()) >> (16 - chunk_bits);
                self.chunks[reverse] = ((j - link) << VALUE_SHIFT | (chunk_bits + 1)) as u32;
            }
        }
        for (i, &n) in lengths.iter().enumerate().filter(|&(_, &n)| n != 0) {
            let code = next_code[n];
            next_code[n] += 1;
            let chunk = (i << VALUE_SHIFT | n) as u32;
            let reverse = usize::from((code as u16).reverse_bits()) >> (16 - n);
            if n <= chunk_bits {
                for slot in self.chunks[reverse..].iter_mut().step_by(1 << n) {
                    *slot = chunk;
                }
            } else {
                let value = (self.chunks[reverse & (NUM_CHUNKS - 1)] >> VALUE_SHIFT) as usize;
                let table = &mut self.links[value];
                for slot in table[reverse >> chunk_bits..]
                    .iter_mut()
                    .step_by(1 << (n - chunk_bits))
                {
                    *slot = chunk;
                }
            }
        }
        true
    }
}
struct Inflater<'d, 'a> {
    source: &'d mut Decoder<'a>,
    b: u32,
    nb: u32,
    literals: Huffman,
    distances: Huffman,
    out: Vec<u8>,
    limit: usize,
}
impl<'d, 'a> Inflater<'d, 'a> {
    fn new(source: &'d mut Decoder<'a>, needed: usize) -> Inflater<'d, 'a> {
        Inflater {
            source,
            b: 0,
            nb: 0,
            literals: Huffman::default(),
            distances: Huffman::default(),
            out: Vec::new(),
            limit: needed,
        }
    }
    fn more_bits(&mut self) -> Res<()> {
        let c = self.source.read_byte()?;
        self.b |= u32::from(c) << (self.nb & 31);
        self.nb += 8;
        Ok(())
    }
    fn bits(&mut self, n: u32) -> Res<u32> {
        while self.nb < n {
            self.more_bits()?;
        }
        let v = self.b & ((1u32 << n) - 1);
        self.b >>= n;
        self.nb -= n;
        Ok(v)
    }
    fn symbol(&mut self, distances: bool) -> Res<usize> {
        let mut n = if distances {
            self.distances.min
        } else {
            self.literals.min
        } as u32;
        loop {
            while self.nb < n {
                self.more_bits()?;
            }
            let h = if distances {
                &self.distances
            } else {
                &self.literals
            };
            let mut chunk = h.chunks[(self.b as usize) & (NUM_CHUNKS - 1)];
            n = chunk & COUNT_MASK;
            if n > CHUNK_BITS {
                chunk = h.links[(chunk >> VALUE_SHIFT) as usize]
                    [((self.b >> CHUNK_BITS) & h.link_mask) as usize];
                n = chunk & COUNT_MASK;
            }
            if n <= self.nb {
                if n == 0 {
                    return Err(Bad);
                }
                self.b >>= n & 31;
                self.nb -= n;
                return Ok((chunk >> VALUE_SHIFT) as usize);
            }
        }
    }
    fn push(&mut self, c: u8) -> Res<()> {
        if self.out.len() >= self.limit {
            return Err(Bad);
        }
        self.out.push(c);
        Ok(())
    }
    fn read_huffman(&mut self) -> Res<()> {
        while self.nb < 5 + 5 + 4 {
            self.more_bits()?;
        }
        let nlit = (self.b & 0x1f) as usize + 257;
        let ndist = ((self.b >> 5) & 0x1f) as usize + 1;
        let nclen = ((self.b >> 10) & 0xf) as usize + 4;
        if nlit > MAX_NUM_LIT || ndist > MAX_NUM_DIST {
            return Err(Bad);
        }
        self.b >>= 14;
        self.nb -= 14;
        let mut code_bits = [0usize; NUM_CODES];
        for &code in CODE_ORDER.iter().take(nclen) {
            code_bits[code] = self.bits(3)? as usize;
        }
        if !self.literals.init(&code_bits) {
            return Err(Bad);
        }
        let n = nlit + ndist;
        let mut lengths = vec![0usize; n];
        let mut i = 0;
        while i < n {
            let x = self.symbol(false)?;
            if x < 16 {
                lengths[i] = x;
                i += 1;
                continue;
            }
            let (base, extra, value) = match x {
                16 => {
                    if i == 0 {
                        return Err(Bad);
                    }
                    (3usize, 2u32, lengths[i - 1])
                }
                17 => (3, 3, 0),
                _ => (11, 7, 0),
            };
            let repeat = base + self.bits(extra)? as usize;
            if i + repeat > n {
                return Err(Bad);
            }
            lengths[i..i + repeat].fill(value);
            i += repeat;
        }
        if !self.literals.init(&lengths[..nlit]) || !self.distances.init(&lengths[nlit..]) {
            return Err(Bad);
        }
        if self.literals.min < lengths[END_BLOCK_MARKER] {
            self.literals.min = lengths[END_BLOCK_MARKER];
        }
        Ok(())
    }
    fn huffman_block(&mut self, fixed: bool) -> Res<()> {
        loop {
            let v = self.symbol(false)?;
            let (base, extra): (usize, u32) = match v {
                0..=255 => {
                    self.push(v as u8)?;
                    continue;
                }
                256 => return Ok(()),
                257..=264 => (v - (257 - 3), 0),
                265..=268 => (v * 2 - (265 * 2 - 11), 1),
                269..=272 => (v * 4 - (269 * 4 - 19), 2),
                273..=276 => (v * 8 - (273 * 8 - 35), 3),
                277..=280 => (v * 16 - (277 * 16 - 67), 4),
                281..=284 => (v * 32 - (281 * 32 - 131), 5),
                285 => (258, 0),
                _ => return Err(Bad),
            };
            let length = base + self.bits(extra)? as usize;
            let code = if fixed {
                usize::from(((self.bits(5)? << 3) as u8).reverse_bits())
            } else {
                self.symbol(true)?
            };
            let distance = match code {
                0..=3 => code + 1,
                4..MAX_NUM_DIST => {
                    let nb = ((code - 2) >> 1) as u32;
                    let extra = (code & 1) << nb | self.bits(nb)? as usize;
                    (1 << (nb + 1)) + 1 + extra
                }
                _ => return Err(Bad),
            };
            if distance > self.out.len().min(WINDOW) {
                return Err(Bad);
            }
            for _ in 0..length {
                let c = self.out[self.out.len() - distance];
                self.push(c)?;
            }
        }
    }
    fn stored_block(&mut self) -> Res<()> {
        (self.b, self.nb) = (0, 0);
        let mut head = Vec::with_capacity(4);
        self.source.read_full(4, &mut head)?;
        let n = u16::from_le_bytes([head[0], head[1]]);
        if u16::from_le_bytes([head[2], head[3]]) != !n {
            return Err(Bad);
        }
        let mut left = usize::from(n);
        while left > 0 {
            let piece = left.min(WINDOW - self.out.len() % WINDOW);
            if self.out.len() + piece > self.limit {
                return Err(Bad);
            }
            self.source.read_full(piece, &mut self.out)?;
            left -= piece;
        }
        Ok(())
    }
    fn run(mut self) -> Res<Vec<u8>> {
        let fixed: Vec<usize> = (0..288)
            .map(|i| match i {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            })
            .collect();
        self.limit += 1;
        loop {
            let last = self.bits(1)? == 1;
            match self.bits(2)? {
                0 => self.stored_block()?,
                1 => {
                    self.literals.init(&fixed);
                    self.huffman_block(true)?;
                }
                2 => {
                    self.read_huffman()?;
                    self.huffman_block(false)?;
                }
                _ => return Err(Bad),
            }
            if last {
                return Ok(self.out);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(kind: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut out = (payload.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        let crc = crc32_update(crc32_update(0, kind), payload);
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }
    fn stored(raw: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01, 0x01];
        out.extend_from_slice(&(raw.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
        out.extend_from_slice(raw);
        out.extend_from_slice(&adler32(raw).to_be_bytes());
        out
    }
    fn png(ihdr: [u8; 13], idat: &[u8]) -> Vec<u8> {
        let mut out = PNG_HEADER.to_vec();
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"IDAT", idat));
        out.extend(chunk(b"IEND", b""));
        out
    }
    #[test]
    fn grey_rows_and_filters() {
        let ihdr = [0, 0, 0, 2, 0, 0, 0, 2, 8, 0, 0, 0, 0];
        let file = png(ihdr, &stored(&[0, 10, 20, 2, 5, 5]));
        assert_eq!(
            decode_config(&file),
            Ok(Config {
                width: 2,
                height: 2,
                model: Model::Gray
            })
        );
        let image = decode(&file).ok().map(|i| (i.pixels, i.opaque));
        let grey = |y: u16| [y * 0x101; 3];
        assert_eq!(
            image,
            Some((vec![grey(10), grey(20), grey(15), grey(25)], true))
        );
    }
    #[test]
    fn refusals() {
        let ihdr = [0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0];
        assert!(decode(&png(ihdr, &stored(&[0, 7]))).is_ok());
        assert!(decode(&png(ihdr, &stored(&[0, 7, 7]))).is_err());
        assert!(decode(&png(ihdr, &stored(&[0]))).is_err());
        assert!(decode(&png(ihdr, &stored(&[5, 7]))).is_err());
        let mut broken = png(ihdr, &stored(&[0, 7]));
        let last = broken.len() - 13;
        broken[last] ^= 1;
        assert!(decode(&broken).is_err());
        assert!(decode_config(&broken).is_ok());
    }
}
