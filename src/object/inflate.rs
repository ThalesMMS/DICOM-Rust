#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::error::{Error, Kind};
use super::parser::source::ByteRead;
const MAX_CODE_LEN: usize = 16;
const MAX_NUM_LIT: usize = 286;
const MAX_NUM_DIST: usize = 30;
const NUM_CODES: usize = 19;
const MAX_MATCH_OFFSET: usize = 1 << 15;
const END_BLOCK_MARKER: usize = 256;
const CHUNK_BITS: u32 = 9;
const NUM_CHUNKS: usize = 1 << CHUNK_BITS;
const COUNT_MASK: u32 = 15;
const VALUE_SHIFT: u32 = 4;
const CODE_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
fn corrupt(offset: i64) -> Error {
    Error::new_kind(
        Kind::DeflateCorrupt,
        format!("flate: corrupt input before offset {offset}"),
    )
}
#[derive(Clone)]
struct Huffman {
    min: usize,
    chunks: Vec<u32>,
    links: Vec<Vec<u32>>,
    link_mask: u32,
}
impl Huffman {
    fn new() -> Huffman {
        Huffman {
            min: 0,
            chunks: vec![0; NUM_CHUNKS],
            links: Vec::new(),
            link_mask: 0,
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn init(&mut self, lengths: &[usize]) -> bool {
        if self.min != 0 {
            *self = Huffman::new();
        }
        let mut count = [0usize; MAX_CODE_LEN];
        let (mut min, mut max) = (0usize, 0usize);
        for &n in lengths {
            if n == 0 {
                continue;
            }
            if min == 0 || n < min {
                min = n;
            }
            if n > max {
                max = n;
            }
            count[n] += 1;
        }
        if max == 0 {
            return true;
        }
        let mut code = 0usize;
        let mut nextcode = [0usize; MAX_CODE_LEN];
        for i in min..=max {
            code <<= 1;
            nextcode[i] = code;
            code += count[i];
        }
        if code != 1 << max && !(code == 1 && max == 1) {
            return false;
        }
        self.min = min;
        if max > CHUNK_BITS as usize {
            let num_links = 1usize << (max - CHUNK_BITS as usize);
            self.link_mask = (num_links - 1) as u32;
            let link = nextcode[CHUNK_BITS as usize + 1] >> 1;
            self.links = vec![Vec::new(); NUM_CHUNKS - link];
            for j in link..NUM_CHUNKS {
                let reverse = ((j as u16).reverse_bits() as usize) >> (16 - CHUNK_BITS);
                let off = j - link;
                self.chunks[reverse] = (off << VALUE_SHIFT | (CHUNK_BITS as usize + 1)) as u32;
                self.links[off] = vec![0; num_links];
            }
        }
        for (i, &n) in lengths.iter().enumerate() {
            if n == 0 {
                continue;
            }
            let code = nextcode[n];
            nextcode[n] += 1;
            let chunk = (i << VALUE_SHIFT | n) as u32;
            let reverse = ((code as u16).reverse_bits() as usize) >> (16 - n);
            if n <= CHUNK_BITS as usize {
                let mut off = reverse;
                while off < self.chunks.len() {
                    self.chunks[off] = chunk;
                    off += 1 << n;
                }
            } else {
                let j = reverse & (NUM_CHUNKS - 1);
                let value = (self.chunks[j] >> VALUE_SHIFT) as usize;
                let linktab = &mut self.links[value];
                let mut off = reverse >> CHUNK_BITS;
                while off < linktab.len() {
                    linktab[off] = chunk;
                    off += 1 << (n - CHUNK_BITS as usize);
                }
            }
        }
        true
    }
}
fn fixed_huffman() -> Huffman {
    let mut bits = [0usize; 288];
    for (i, b) in bits.iter_mut().enumerate() {
        *b = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let mut h = Huffman::new();
    h.init(&bits);
    h
}
struct ByteSource<'a> {
    data: &'a [u8],
    pos: usize,
}
impl ByteSource<'_> {
    fn read_byte(&mut self) -> Option<u8> {
        let b = self.data.get(self.pos).copied()?;
        self.pos = self.pos.saturating_add(1);
        Some(b)
    }
    fn read_full(&mut self, buf: &mut [u8]) -> (usize, Option<Error>) {
        let rest = self.data.get(self.pos..).unwrap_or_default();
        let n = buf.len().min(rest.len());
        if let (Some(to), Some(from)) = (buf.get_mut(..n), rest.get(..n)) {
            to.copy_from_slice(from);
        }
        self.pos = self.pos.saturating_add(n);
        if n == buf.len() {
            (n, None)
        } else if n == 0 {
            (n, Some(Error::eof()))
        } else {
            (n, Some(Error::unexpected_eof()))
        }
    }
}
fn no_eof(err: Error) -> Error {
    if err.is_exactly(Kind::Eof) {
        Error::unexpected_eof()
    } else {
        err
    }
}
struct Decompressor<'a> {
    r: ByteSource<'a>,
    roffset: i64,
    b: u32,
    nb: u32,
    h1: Huffman,
    h2: Huffman,
    fixed: Huffman,
    bits: Vec<usize>,
    codebits: [usize; NUM_CODES],
    hist: Vec<u8>,
    wr_pos: usize,
    final_block: bool,
    out: Vec<u8>,
    max_out: usize,
    over: bool,
}
impl Decompressor<'_> {
    #[allow(clippy::arithmetic_side_effects)]
    fn more_bits(&mut self) -> Result<(), Error> {
        let Some(c) = self.r.read_byte() else {
            return Err(Error::unexpected_eof());
        };
        self.roffset += 1;
        self.b |= (c as u32) << self.nb;
        self.nb += 8;
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn huff_sym(&mut self, which: u8) -> Result<usize, Error> {
        let h = match which {
            0 => &self.fixed,
            1 => &self.h1,
            _ => &self.h2,
        };
        let mut n = h.min as u32;
        let (mut nb, mut b) = (self.nb, self.b);
        loop {
            while nb < n {
                let Some(c) = self.r.read_byte() else {
                    self.b = b;
                    self.nb = nb;
                    return Err(Error::unexpected_eof());
                };
                self.roffset += 1;
                b |= (c as u32) << (nb & 31);
                nb += 8;
            }
            let mut chunk = h.chunks[(b as usize) & (NUM_CHUNKS - 1)];
            n = chunk & COUNT_MASK;
            if n > CHUNK_BITS {
                chunk = h.links[(chunk >> VALUE_SHIFT) as usize]
                    [((b >> CHUNK_BITS) & h.link_mask) as usize];
                n = chunk & COUNT_MASK;
            }
            if n <= nb {
                if n == 0 {
                    self.b = b;
                    self.nb = nb;
                    return Err(corrupt(self.roffset));
                }
                self.b = b >> (n & 31);
                self.nb = nb - n;
                return Ok((chunk >> VALUE_SHIFT) as usize);
            }
        }
    }
    fn hist_size(&self, full: bool) -> usize {
        if full { self.hist.len() } else { self.wr_pos }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn write_byte(&mut self, c: u8, full: &mut bool) {
        self.hist[self.wr_pos] = c;
        self.wr_pos += 1;
        if self.out.len() < self.max_out {
            self.out.push(c);
        } else {
            self.over = true;
        }
        if self.wr_pos == self.hist.len() {
            self.wr_pos = 0;
            *full = true;
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn write_copy(&mut self, dist: usize, length: usize, full: &mut bool) {
        for _ in 0..length {
            let src = (self.wr_pos + self.hist.len() - dist) % self.hist.len();
            let c = self.hist[src];
            self.write_byte(c, full);
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn read_huffman(&mut self) -> Result<(), Error> {
        while self.nb < 5 + 5 + 4 {
            self.more_bits()?;
        }
        let nlit = (self.b & 0x1F) as usize + 257;
        if nlit > MAX_NUM_LIT {
            return Err(corrupt(self.roffset));
        }
        self.b >>= 5;
        let ndist = (self.b & 0x1F) as usize + 1;
        if ndist > MAX_NUM_DIST {
            return Err(corrupt(self.roffset));
        }
        self.b >>= 5;
        let nclen = (self.b & 0xF) as usize + 4;
        self.b >>= 4;
        self.nb -= 5 + 5 + 4;
        for &code in CODE_ORDER.iter().take(nclen) {
            while self.nb < 3 {
                self.more_bits()?;
            }
            self.codebits[code] = (self.b & 0x7) as usize;
            self.b >>= 3;
            self.nb -= 3;
        }
        for &code in CODE_ORDER.iter().skip(nclen) {
            self.codebits[code] = 0;
        }
        let codebits = self.codebits;
        if !self.h1.init(&codebits) {
            return Err(corrupt(self.roffset));
        }
        let n = nlit + ndist;
        let mut i = 0;
        while i < n {
            let x = self.huff_sym(1)?;
            if x < 16 {
                self.bits[i] = x;
                i += 1;
                continue;
            }
            let (mut rep, nb, b) = match x {
                16 => {
                    if i == 0 {
                        return Err(corrupt(self.roffset));
                    }
                    (3usize, 2u32, self.bits[i - 1])
                }
                17 => (3, 3, 0),
                18 => (11, 7, 0),
                _ => return Err(Error::msg("flate: internal error: unexpected length code")),
            };
            while self.nb < nb {
                self.more_bits()?;
            }
            rep += (self.b & ((1u32 << nb) - 1)) as usize;
            self.b >>= nb;
            self.nb -= nb;
            if i + rep > n {
                return Err(corrupt(self.roffset));
            }
            for _ in 0..rep {
                self.bits[i] = b;
                i += 1;
            }
        }
        let lit = self.bits[..nlit].to_vec();
        let dist = self.bits[nlit..nlit + ndist].to_vec();
        if !self.h1.init(&lit) || !self.h2.init(&dist) {
            return Err(corrupt(self.roffset));
        }
        if self.h1.min < self.bits[END_BLOCK_MARKER] {
            self.h1.min = self.bits[END_BLOCK_MARKER];
        }
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn huffman_block(&mut self, fixed: bool, full: &mut bool) -> Result<(), Error> {
        let (hl, hd) = if fixed { (0u8, None) } else { (1u8, Some(2u8)) };
        loop {
            let v = self.huff_sym(hl)?;
            let (mut length, n): (usize, u32) = match v {
                0..=255 => {
                    self.write_byte(v as u8, full);
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
                _ => return Err(corrupt(self.roffset)),
            };
            if n > 0 {
                while self.nb < n {
                    self.more_bits()?;
                }
                length += (self.b & ((1u32 << n) - 1)) as usize;
                self.b >>= n;
                self.nb -= n;
            }
            let mut dist = match hd {
                None => {
                    while self.nb < 5 {
                        self.more_bits()?;
                    }
                    let d = (((self.b & 0x1F) << 3) as u8).reverse_bits() as usize;
                    self.b >>= 5;
                    self.nb -= 5;
                    d
                }
                Some(which) => self.huff_sym(which)?,
            };
            if dist < 4 {
                dist += 1;
            } else if dist < MAX_NUM_DIST {
                let nb = ((dist - 2) >> 1) as u32;
                let mut extra = (dist & 1) << nb;
                while self.nb < nb {
                    self.more_bits()?;
                }
                extra |= (self.b & ((1u32 << nb) - 1)) as usize;
                self.b >>= nb;
                self.nb -= nb;
                dist = (1 << (nb + 1)) + 1 + extra;
            } else {
                return Err(corrupt(self.roffset));
            }
            if dist > self.hist_size(*full) {
                return Err(corrupt(self.roffset));
            }
            self.write_copy(dist, length, full);
        }
    }
    fn data_block(&mut self, full: &mut bool) -> Result<(), Error> {
        self.nb = 0;
        self.b = 0;
        let mut buf = [0u8; 4];
        let (nr, err) = self.r.read_full(&mut buf);
        self.roffset = self.roffset.saturating_add(nr as i64);
        if let Some(err) = err {
            return Err(no_eof(err));
        }
        let [n0, n1, c0, c1] = buf;
        let n = usize::from(u16::from_le_bytes([n0, n1]));
        let nn = u16::from_le_bytes([c0, c1]);
        if nn != !(n as u16) {
            return Err(corrupt(self.roffset));
        }
        let mut data = vec![0u8; n];
        let (cnt, err) = self.r.read_full(&mut data);
        self.roffset = self.roffset.saturating_add(cnt as i64);
        for &c in data.get(..cnt).unwrap_or_default() {
            self.write_byte(c, full);
        }
        if let Some(err) = err {
            return Err(no_eof(err));
        }
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn run(&mut self) -> Error {
        let mut full = false;
        loop {
            let step = (|| -> Result<(), Error> {
                while self.nb < 1 + 2 {
                    self.more_bits()?;
                }
                self.final_block = self.b & 1 == 1;
                self.b >>= 1;
                let typ = self.b & 3;
                self.b >>= 2;
                self.nb -= 1 + 2;
                match typ {
                    0 => self.data_block(&mut full),
                    1 => self.huffman_block(true, &mut full),
                    2 => {
                        self.read_huffman()?;
                        self.huffman_block(false, &mut full)
                    }
                    _ => Err(corrupt(self.roffset)),
                }
            })();
            if self.over {
                return Error::msg(format!(
                    "dicom: maximum inflated size exceeded: limit {} bytes",
                    self.max_out
                ));
            }
            if let Err(err) = step {
                return err;
            }
            if self.final_block {
                return Error::eof();
            }
        }
    }
}
pub(crate) fn inflate_raw(input: &[u8]) -> (Vec<u8>, Error, usize) {
    inflate_raw_limited(input, DEFAULT_MAX_INFLATED_BYTES)
}
pub const DEFAULT_MAX_INFLATED_BYTES: u64 = crate::limits::Limits::DEFAULT.max_inflated_bytes;
pub(crate) fn inflate_raw_limited(input: &[u8], max_out: u64) -> (Vec<u8>, Error, usize) {
    let max_out = match usize::try_from(max_out) {
        Ok(0) | Err(_) => usize::MAX,
        Ok(n) => n,
    };
    let mut d = Decompressor {
        r: ByteSource {
            data: input,
            pos: 0,
        },
        roffset: 0,
        b: 0,
        nb: 0,
        h1: Huffman::new(),
        h2: Huffman::new(),
        fixed: fixed_huffman(),
        bits: vec![0; MAX_NUM_LIT + MAX_NUM_DIST],
        codebits: [0; NUM_CODES],
        hist: vec![0; MAX_MATCH_OFFSET],
        wr_pos: 0,
        final_block: false,
        out: Vec::new(),
        max_out,
        over: false,
    };
    let err = d.run();
    let consumed = d.r.pos;
    (d.out, err, consumed)
}
#[allow(clippy::arithmetic_side_effects)]
fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}
#[doc(hidden)]
pub fn inflate_zlib(input: &[u8]) -> (Vec<u8>, Error) {
    inflate_zlib_limited(input, DEFAULT_MAX_INFLATED_BYTES)
}
pub(crate) fn inflate_zlib_limited(input: &[u8], max_out: u64) -> (Vec<u8>, Error) {
    if input.len() < 2 {
        return (Vec::new(), Error::unexpected_eof());
    }
    let body = input.get(2..).unwrap_or_default();
    let (out, err, consumed) = inflate_raw_limited(body, max_out);
    if !err.is_exactly(Kind::Eof) {
        return (out, err);
    }
    let rest = body.get(consumed..).unwrap_or_default();
    let Some(&checksum) = rest.first_chunk::<4>() else {
        return (out, Error::unexpected_eof());
    };
    let checksum = u32::from_be_bytes(checksum);
    if checksum != adler32(&out) {
        return (out, Error::msg("zlib: invalid checksum"));
    }
    (out, Error::eof())
}
pub(crate) fn is_zlib_header(head: &[u8]) -> bool {
    let &[cmf, flg, ..] = head else {
        return false;
    };
    cmf & 0x0F == 8
        && cmf >> 4 <= 7
        && flg & 0x20 == 0
        && ((cmf as u16) << 8 | flg as u16).is_multiple_of(31)
}
pub(crate) struct Inflated {
    data: Vec<u8>,
    pos: usize,
    err: Error,
}
impl Inflated {
    pub fn new(data: Vec<u8>, err: Error) -> Inflated {
        Inflated { data, pos: 0, err }
    }
}
impl ByteRead for Inflated {
    fn read(&mut self, p: &mut [u8]) -> (usize, Option<Error>) {
        if self.pos >= self.data.len() {
            return (0, Some(self.err.clone()));
        }
        let rest = self.data.get(self.pos..).unwrap_or_default();
        let n = p.len().min(rest.len());
        if let (Some(to), Some(from)) = (p.get_mut(..n), rest.get(..n)) {
            to.copy_from_slice(from);
        }
        self.pos = self.pos.saturating_add(n);
        if self.pos == self.data.len() && n > 0 {
            return (n, Some(self.err.clone()));
        }
        (n, None)
    }
}
