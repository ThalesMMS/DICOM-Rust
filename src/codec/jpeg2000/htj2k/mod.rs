mod refine;
mod vlc_codebook;
pub(crate) use refine::refine;
use std::fmt;
use std::sync::OnceLock;
use vlc_codebook::{VLC_INITIAL, VLC_NON_INITIAL};
const MALFORMED: &str = "htj2k: malformed HT cleanup segment";
const UNSUPPORTED: &str = "htj2k: unsupported HT code-block";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);
impl Error {
    fn malformed(detail: impl fmt::Display) -> Error {
        Error(format!("{MALFORMED}: {detail}"))
    }
    fn unsupported(detail: impl fmt::Display) -> Error {
        Error(format!("{UNSUPPORTED}: {detail}"))
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub(crate) const MAX_SCUP: usize = 4079;
pub const MAX_MAGNITUDE: i32 = (1 << 30) - 1;
#[derive(Clone, Copy)]
pub(crate) struct VlcRecord {
    context: u8,
    rho: u8,
    u_off: u8,
    ek: u8,
    e1: u8,
    codeword: u8,
    length: u8,
}
impl VlcRecord {
    const fn new(
        context: u8,
        rho: u8,
        u_off: u8,
        ek: u8,
        e1: u8,
        codeword: u8,
        length: u8,
    ) -> VlcRecord {
        VlcRecord {
            context,
            rho,
            u_off,
            ek,
            e1,
            codeword,
            length,
        }
    }
}
#[derive(Clone, Copy, Default)]
struct VlcCode {
    codeword: u8,
    length: u8,
    ek: u8,
    e1: u8,
    ok: bool,
}
#[derive(Clone, Copy, Default)]
struct VlcSymbol {
    rho: u8,
    u_off: u8,
    ek: u8,
    e1: u8,
    length: u8,
}
struct Tables {
    encode: Box<[[[[VlcCode; 16]; 16]; 8]; 2]>,
    decode: Box<[[[VlcSymbol; 128]; 8]; 2]>,
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut encode = Box::new([[[[VlcCode::default(); 16]; 16]; 8]; 2]);
        let mut decode = Box::new([[[VlcSymbol::default(); 128]; 8]; 2]);
        for (row, records) in [&VLC_INITIAL[..], &VLC_NON_INITIAL[..]]
            .into_iter()
            .enumerate()
        {
            for r in records {
                let mask = ((1u32 << r.length) - 1) as u8;
                for bits in 0..128u8 {
                    if bits & mask == r.codeword {
                        decode[row][r.context as usize][bits as usize] = VlcSymbol {
                            rho: r.rho,
                            u_off: r.u_off,
                            ek: r.ek,
                            e1: r.e1,
                            length: r.length,
                        };
                    }
                }
                for emb in 0..16u8 {
                    if emb & !r.rho != 0 || (emb != 0) != (r.u_off != 0) || r.ek & emb != r.e1 {
                        continue;
                    }
                    let current =
                        &mut encode[row][r.context as usize][r.rho as usize][emb as usize];
                    if !current.ok || cheaper(r, current) {
                        *current = VlcCode {
                            codeword: r.codeword,
                            length: r.length,
                            ek: r.ek,
                            e1: r.e1,
                            ok: true,
                        };
                    }
                }
            }
        }
        Tables { encode, decode }
    })
}
#[allow(clippy::arithmetic_side_effects)]
fn cheaper(r: &VlcRecord, current: &VlcCode) -> bool {
    let cost = r.length as i32 - popcount4(r.ek);
    let current_cost = current.length as i32 - popcount4(current.ek);
    if cost != current_cost {
        return cost < current_cost;
    }
    r.length < current.length
}
#[allow(clippy::arithmetic_side_effects)]
fn popcount4(v: u8) -> i32 {
    ((v & 1) + ((v >> 1) & 1) + ((v >> 2) & 1) + ((v >> 3) & 1)) as i32
}
const MEL_EXPONENT: [u8; 13] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 5];
#[allow(clippy::arithmetic_side_effects)]
fn exponent(mu: u32) -> u8 {
    if mu == 0 {
        return 0;
    }
    let mut v = 2 * mu as u64 - 1;
    let mut n = 0u8;
    while v != 0 {
        n += 1;
        v >>= 1;
    }
    n
}
fn magnitude(s: i32) -> u32 {
    if s < 0 {
        s.wrapping_neg() as u32
    } else {
        s as u32
    }
}
#[derive(Clone, Copy, Default)]
struct Quad {
    rho: u8,
    e: [u8; 4],
    v: [u32; 4],
    emax: u8,
    u: u32,
    bound: u8,
    code: VlcCode,
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn quad_context(quads: &[Quad], above: &[u8], c: usize, initial: bool) -> u8 {
    if initial {
        if c == 0 {
            return 0;
        }
        let left = quads[c - 1].rho;
        return (left >> 1) | (left & 1);
    }
    let sig = |i: usize| u8::from(above[i] != 0);
    let mut context = sig(2 * c) | sig(2 * c + 1) | ((sig(2 * c + 2) | sig(2 * c + 3)) << 2);
    if c > 0 {
        let left = quads[c - 1].rho;
        if left & 0xC != 0 {
            context |= 2;
        }
    }
    context
}
struct MagSgnReader<'a> {
    data: &'a [u8],
    pos: usize,
    acc: u64,
    bits: u32,
    unstuff: bool,
}
impl MagSgnReader<'_> {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn get(&mut self, n: u32) -> u32 {
        while self.bits < n {
            let mut b = 0xFFu8;
            if self.pos < self.data.len() {
                b = self.data[self.pos];
                self.pos += 1;
            }
            let width = if self.unstuff { 7 } else { 8 };
            self.acc |= ((b & ((1u16 << width) - 1) as u8) as u64) << self.bits;
            self.bits += width;
            self.unstuff = b == 0xFF;
        }
        let v = (self.acc & ((1u64 << n) - 1)) as u32;
        self.acc >>= n;
        self.bits -= n;
        v
    }
}
struct MelReader<'a> {
    data: &'a [u8],
    pos: usize,
    cur: u8,
    left: u32,
    last_ff: bool,
    state: usize,
    run: i64,
    pending1: bool,
}
impl MelReader<'_> {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn bit(&mut self) -> u8 {
        if self.left == 0 {
            let mut b = 0xFFu8;
            if self.pos < self.data.len() {
                b = self.data[self.pos];
                self.pos += 1;
                if self.pos == self.data.len() {
                    b |= 0x0F;
                }
            }
            self.left = if self.last_ff { 7 } else { 8 };
            self.cur = b;
            self.last_ff = b == 0xFF;
        }
        self.left -= 1;
        (self.cur >> self.left) & 1
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn event(&mut self) -> bool {
        loop {
            if self.run > 0 {
                self.run -= 1;
                return false;
            }
            if self.pending1 {
                self.pending1 = false;
                return true;
            }
            let e = MEL_EXPONENT[self.state] as i64;
            if self.bit() == 1 {
                self.run = 1 << e;
                if self.state < 12 {
                    self.state += 1;
                }
                continue;
            }
            let mut n = 0i64;
            for _ in 0..e {
                n = (n << 1) | self.bit() as i64;
            }
            if self.state > 0 {
                self.state -= 1;
            }
            self.run = n;
            self.pending1 = true;
        }
    }
}
struct VlcReader<'a> {
    data: &'a [u8],
    pos: isize,
    acc: u64,
    bits: u32,
    unstuff: bool,
}
impl VlcReader<'_> {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn new(segment: &[u8]) -> VlcReader<'_> {
        let n = segment.len();
        let b = segment[n - 2];
        let mut r = VlcReader {
            data: segment,
            pos: n as isize - 3,
            acc: (b >> 4) as u64,
            bits: 4,
            unstuff: false,
        };
        if r.acc & 7 == 7 {
            r.acc &= 7;
            r.bits = 3;
        }
        r.unstuff = b > 0x8F;
        r
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn fill(&mut self, n: u32) {
        while self.bits < n {
            let mut b = 0u8;
            if self.pos >= 0 {
                b = self.data[self.pos as usize];
                self.pos -= 1;
            }
            let width = if self.unstuff && b & 0x7F == 0x7F {
                7
            } else {
                8
            };
            self.acc |= ((b & ((1u16 << width) - 1) as u8) as u64) << self.bits;
            self.bits += width;
            self.unstuff = b > 0x8F;
        }
    }
    fn peek7(&mut self) -> u8 {
        self.fill(7);
        (self.acc & 0x7F) as u8
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn get(&mut self, n: u32) -> u32 {
        self.fill(n);
        let v = (self.acc & ((1u64 << n) - 1)) as u32;
        self.acc >>= n;
        self.bits -= n;
        v
    }
    fn u_prefix(&mut self) -> u32 {
        if self.get(1) == 1 {
            1
        } else if self.get(1) == 1 {
            2
        } else if self.get(1) == 1 {
            3
        } else {
            5
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn u_suffix(&mut self, prefix: u32) -> u32 {
        match prefix {
            1 | 2 => return prefix,
            3 => return 3 + self.get(1),
            _ => {}
        }
        let s = self.get(5);
        if s < 28 {
            return 5 + s;
        }
        33 + (s - 28) + 4 * self.get(4)
    }
}
#[allow(clippy::arithmetic_side_effects)]
fn read_u_values(
    vlc: &mut VlcReader<'_>,
    mel: &mut MelReader<'_>,
    a: bool,
    b: bool,
    initial: bool,
) -> (u32, u32) {
    if initial && a && b {
        if mel.event() {
            let pa = vlc.u_prefix();
            let pb = vlc.u_prefix();
            let ua = vlc.u_suffix(pa) + 2;
            let ub = vlc.u_suffix(pb) + 2;
            return (ua, ub);
        }
        let pa = vlc.u_prefix();
        if pa > 2 {
            let ub = vlc.get(1) + 1;
            return (vlc.u_suffix(pa), ub);
        }
        let pb = vlc.u_prefix();
        let ua = vlc.u_suffix(pa);
        return (ua, vlc.u_suffix(pb));
    }
    let (mut pa, mut pb, mut ua, mut ub) = (0, 0, 0, 0);
    if a {
        pa = vlc.u_prefix();
    }
    if b {
        pb = vlc.u_prefix();
    }
    if a {
        ua = vlc.u_suffix(pa);
    }
    if b {
        ub = vlc.u_suffix(pb);
    }
    (ua, ub)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn decode_cleanup(
    segment: &[u8],
    width: i64,
    height: i64,
    out: &mut [i32],
    max_bound: i64,
) -> Result<(), Error> {
    if width <= 0
        || height <= 0
        || width > 1024
        || height > 1024
        || width * height > 4096
        || out.len() as i64 != width * height
    {
        return Err(Error::unsupported(format_args!(
            "{width}x{height} code-block"
        )));
    }
    let max_bound = if max_bound <= 0 || max_bound > 31 {
        31
    } else {
        max_bound
    };
    let lcup = segment.len();
    if lcup < 2 {
        return Err(Error::malformed(format_args!("{lcup}-byte segment")));
    }
    let scup = ((segment[lcup - 1] as usize) << 4) | (segment[lcup - 2] & 0x0F) as usize;
    if scup < 2 || scup > lcup || scup > MAX_SCUP {
        return Err(Error::malformed(format_args!(
            "Scup {scup} in a {lcup}-byte segment"
        )));
    }
    out.fill(0);
    let (width, height) = (width as usize, height as usize);
    let pcup = lcup - scup;
    let mut ms = MagSgnReader {
        data: &segment[..pcup],
        pos: 0,
        acc: 0,
        bits: 0,
        unstuff: false,
    };
    let mut mel = MelReader {
        data: &segment[pcup..lcup - 1],
        pos: 0,
        cur: 0,
        left: 0,
        last_ff: false,
        state: 0,
        run: 0,
        pending1: false,
    };
    let mut vlc = VlcReader::new(&segment[pcup..]);
    let tables = tables();
    let quad_cols = width.div_ceil(2);
    let quad_rows = height.div_ceil(2);
    let mut above = vec![0u8; 2 * quad_cols + 3];
    let mut quads = vec![Quad::default(); quad_cols];
    let mut u_off = vec![false; quad_cols];
    for qy in 0..quad_rows {
        let initial = qy == 0;
        let row = usize::from(!initial);
        let mut qx = 0;
        while qx < quad_cols {
            let pair = if qx + 1 < quad_cols { 2 } else { 1 };
            for k in 0..pair {
                let c = qx + k;
                quads[c] = Quad::default();
                u_off[c] = false;
                let context = quad_context(&quads, &above, c, initial);
                if context == 0 && !mel.event() {
                    continue;
                }
                let symbol = tables.decode[row][context as usize][vlc.peek7() as usize];
                if symbol.length == 0 {
                    return Err(Error::malformed("invalid codeword"));
                }
                vlc.get(symbol.length as u32);
                let q = &mut quads[c];
                q.rho = symbol.rho;
                q.code = VlcCode {
                    ek: symbol.ek,
                    e1: symbol.e1,
                    ..VlcCode::default()
                };
                u_off[c] = symbol.u_off != 0 && symbol.rho != 0;
            }
            let (ua, ub) = read_u_values(
                &mut vlc,
                &mut mel,
                u_off[qx],
                pair == 2 && u_off[qx + 1],
                initial,
            );
            for k in 0..pair {
                let c = qx + k;
                let q = quads[c];
                if q.rho == 0 {
                    continue;
                }
                let mut kappa = 1u32;
                if !initial && q.rho & q.rho.wrapping_sub(1) != 0 {
                    let m = above[2 * c]
                        .max(above[2 * c + 1])
                        .max(above[2 * c + 2])
                        .max(above[2 * c + 3]);
                    if m > 2 {
                        kappa = m as u32 - 1;
                    }
                }
                let u = if k == 1 { ub } else { ua };
                let bound = kappa.wrapping_add(u);
                if bound > max_bound as u32 {
                    return Err(Error::malformed(format_args!(
                        "exponent bound {bound} above {max_bound}"
                    )));
                }
                for i in 0..4 {
                    if (q.rho >> i) & 1 == 0 {
                        continue;
                    }
                    let x = 2 * c + (i >> 1);
                    let y = 2 * qy + (i & 1);
                    if x >= width || y >= height {
                        return Err(Error::malformed(
                            "significant sample outside the code-block",
                        ));
                    }
                    let m = bound - ((q.code.ek >> i) & 1) as u32;
                    let v = ms.get(m) | ((((q.code.e1 >> i) & 1) as u32) << m);
                    if v >> 1 >= MAX_MAGNITUDE as u32 {
                        return Err(Error::malformed("magnitude above 2^30"));
                    }
                    let mut mu = (v >> 1) as i32 + 1;
                    if v & 1 != 0 {
                        mu = -mu;
                    }
                    out[y * width + x] = mu;
                }
            }
            qx += 2;
        }
        let y = 2 * qy + 1;
        for x in -1..=(2 * quad_cols as i64 + 1) {
            let mut e = 0u8;
            if x >= 0 && (x as usize) < width && y < height {
                e = exponent(magnitude(out[y * width + x as usize]));
            }
            above[(x + 1) as usize] = e;
        }
    }
    Ok(())
}
struct MagSgnWriter {
    buf: Vec<u8>,
    tmp: u32,
    used: u32,
    max: u32,
}
impl MagSgnWriter {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn put(&mut self, mut bits: u32, mut n: u32) {
        while n > 0 {
            let take = (self.max - self.used).min(n);
            self.tmp |= (bits & ((1u64 << take) - 1) as u32) << self.used;
            self.used += take;
            bits = if take >= 32 { 0 } else { bits >> take };
            n -= take;
            if self.used == self.max {
                let b = self.tmp as u8;
                self.buf.push(b);
                self.max = if b == 0xFF { 7 } else { 8 };
                self.tmp = 0;
                self.used = 0;
            }
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.tmp |= (0xFFu32 << self.used) & ((1u32 << self.max) - 1);
            self.buf.push(self.tmp as u8);
        }
        while self.buf.last() == Some(&0xFF) {
            self.buf.pop();
        }
        self.buf
    }
}
struct MelWriter {
    buf: Vec<u8>,
    tmp: u8,
    remaining: u32,
    state: usize,
    run: i64,
}
impl MelWriter {
    fn new() -> MelWriter {
        MelWriter {
            buf: Vec::new(),
            tmp: 0,
            remaining: 8,
            state: 0,
            run: 0,
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn bit(&mut self, b: u8) {
        self.tmp = (self.tmp << 1) | b;
        self.remaining -= 1;
        if self.remaining == 0 {
            self.buf.push(self.tmp);
            self.remaining = if self.tmp == 0xFF { 7 } else { 8 };
            self.tmp = 0;
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn event(&mut self, one: bool) {
        let e = MEL_EXPONENT[self.state] as i64;
        if !one {
            self.run += 1;
            if self.run == 1 << e {
                self.bit(1);
                self.run = 0;
                if self.state < 12 {
                    self.state += 1;
                }
            }
            return;
        }
        self.bit(0);
        let mut i = e - 1;
        while i >= 0 {
            self.bit(((self.run >> i) & 1) as u8);
            i -= 1;
        }
        self.run = 0;
        if self.state > 0 {
            self.state -= 1;
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.run > 0 {
            self.bit(1);
        }
        let full = if self.buf.last() == Some(&0xFF) { 7 } else { 8 };
        if self.remaining != full {
            self.buf.push(self.tmp << self.remaining);
        } else if self.buf.last() == Some(&0xFF) {
            self.buf.push(0);
        }
        self.buf
    }
}
struct VlcWriter {
    bytes: Vec<u8>,
    tmp: u32,
    used: u32,
    stuffed: bool,
}
impl VlcWriter {
    fn new() -> VlcWriter {
        VlcWriter {
            bytes: vec![0xFF],
            tmp: 0x0F,
            used: 4,
            stuffed: true,
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn put(&mut self, mut bits: u32, mut n: u32) {
        while n > 0 {
            self.tmp |= (bits & 1) << self.used;
            bits >>= 1;
            self.used += 1;
            n -= 1;
            if self.used == 7 && self.stuffed && self.tmp == 0x7F {
                self.emit();
                continue;
            }
            if self.used == 8 {
                self.emit();
            }
        }
    }
    fn emit(&mut self) {
        let b = self.tmp as u8;
        self.bytes.push(b);
        self.stuffed = b > 0x8F;
        self.tmp = 0;
        self.used = 0;
    }
    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.emit();
        }
        self.bytes.reverse();
        self.bytes
    }
    fn put_u_prefix(&mut self, u: u32) {
        if u == 1 {
            self.put(1, 1);
        } else if u == 2 {
            self.put(2, 2);
        } else if u <= 4 {
            self.put(4, 3);
        } else {
            self.put(0, 3);
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn put_u_suffix(&mut self, u: u32) {
        if u <= 2 {
        } else if u <= 4 {
            self.put(u - 3, 1);
        } else if u <= 32 {
            self.put(u - 5, 5);
        } else {
            self.put(28 + ((u - 33) & 3), 5);
            self.put((u - 33) >> 2, 4);
        }
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn put_u_values(
    vlc: &mut VlcWriter,
    mel: &mut MelWriter,
    a: &Quad,
    b: Option<&Quad>,
    initial: bool,
) -> Result<(), Error> {
    let ua = if a.rho != 0 { a.u } else { 0 };
    let ub = match b {
        Some(b) if b.rho != 0 => b.u,
        _ => 0,
    };
    if ua > 96 || ub > 96 {
        return Err(Error::unsupported("u value above 96"));
    }
    if initial && ua > 0 && ub > 0 {
        let both = ua > 2 && ub > 2;
        mel.event(both);
        if both {
            vlc.put_u_prefix(ua - 2);
            vlc.put_u_prefix(ub - 2);
            vlc.put_u_suffix(ua - 2);
            vlc.put_u_suffix(ub - 2);
            return Ok(());
        }
        vlc.put_u_prefix(ua);
        if ua > 2 {
            vlc.put(ub - 1, 1);
            vlc.put_u_suffix(ua);
            return Ok(());
        }
        vlc.put_u_prefix(ub);
        vlc.put_u_suffix(ua);
        vlc.put_u_suffix(ub);
        return Ok(());
    }
    if ua > 0 {
        vlc.put_u_prefix(ua);
    }
    if ub > 0 {
        vlc.put_u_prefix(ub);
    }
    if ua > 0 {
        vlc.put_u_suffix(ua);
    }
    if ub > 0 {
        vlc.put_u_suffix(ub);
    }
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn encode_cleanup(samples: &[i32], width: i64, height: i64) -> Result<Vec<u8>, Error> {
    if width <= 0
        || height <= 0
        || width > 1024
        || height > 1024
        || width * height > 4096
        || samples.len() as i64 != width * height
    {
        return Err(Error::unsupported(format_args!(
            "{width}x{height} code-block with {} samples",
            samples.len()
        )));
    }
    let mut nonzero = false;
    for &s in samples {
        if !(-MAX_MAGNITUDE..=MAX_MAGNITUDE).contains(&s) {
            return Err(Error::unsupported("sample magnitude above 2^30"));
        }
        nonzero = nonzero || s != 0;
    }
    if !nonzero {
        return Err(Error::unsupported(
            "a code-block without significant samples is not coded",
        ));
    }
    let quad_cols = ((width + 1) / 2) as usize;
    let quad_rows = (height + 1) / 2;
    let at = |x: i64, y: i64| -> i32 {
        if x < 0 || y < 0 || x >= width || y >= height {
            return 0;
        }
        samples[(y * width + x) as usize]
    };
    let mut above = vec![0u8; 2 * quad_cols + 3];
    let mut quads = vec![Quad::default(); quad_cols];
    let mut ms = MagSgnWriter {
        buf: Vec::new(),
        tmp: 0,
        used: 0,
        max: 8,
    };
    let mut mel = MelWriter::new();
    let mut vlc = VlcWriter::new();
    let tables = tables();
    for qy in 0..quad_rows {
        let initial = qy == 0;
        let row = usize::from(!initial);
        for (qx, q) in quads.iter_mut().enumerate() {
            *q = Quad::default();
            for i in 0..4 {
                let s = at(2 * qx as i64 + (i >> 1), 2 * qy + (i & 1));
                if s == 0 {
                    continue;
                }
                let (mu, sign) = if s < 0 {
                    (s.wrapping_neg() as u32, 1)
                } else {
                    (s as u32, 0)
                };
                q.rho |= 1 << i;
                q.e[i as usize] = exponent(mu);
                q.v[i as usize] = 2 * (mu - 1) + sign;
                if q.e[i as usize] > q.emax {
                    q.emax = q.e[i as usize];
                }
            }
        }
        let mut qx = 0;
        while qx < quad_cols {
            let pair = if qx + 1 < quad_cols { 2 } else { 1 };
            for k in 0..pair {
                let c = qx + k;
                let context = quad_context(&quads, &above, c, initial);
                let q = &mut quads[c];
                let mut kappa = 1u8;
                if !initial && q.rho & q.rho.wrapping_sub(1) != 0 {
                    let m = above[2 * c]
                        .max(above[2 * c + 1])
                        .max(above[2 * c + 2])
                        .max(above[2 * c + 3]);
                    if m > 2 {
                        kappa = m - 1;
                    }
                }
                q.bound = kappa.max(q.emax);
                q.u = (q.bound - kappa) as u32;
                let mut emb = 0u8;
                if q.u > 0 {
                    for i in 0..4 {
                        if (q.rho >> i) & 1 != 0 && q.e[i] == q.bound {
                            emb |= 1 << i;
                        }
                    }
                }
                if context == 0 {
                    mel.event(q.rho != 0);
                    if q.rho == 0 {
                        continue;
                    }
                }
                q.code = tables.encode[row][context as usize][q.rho as usize][emb as usize];
                if !q.code.ok {
                    return Err(Error::malformed(format_args!(
                        "no codeword for context {context} rho {:#x} emb {:#x}",
                        q.rho, emb
                    )));
                }
                vlc.put(q.code.codeword as u32, q.code.length as u32);
            }
            let b = if pair == 2 {
                Some(&quads[qx + 1])
            } else {
                None
            };
            put_u_values(&mut vlc, &mut mel, &quads[qx], b, initial)?;
            for q in &quads[qx..qx + pair] {
                for i in 0..4 {
                    if (q.rho >> i) & 1 == 0 {
                        continue;
                    }
                    let m = q.bound as u32 - ((q.code.ek >> i) & 1) as u32;
                    ms.put(q.v[i], m);
                }
            }
            qx += 2;
        }
        for x in -1..=(2 * quad_cols as i64 + 1) {
            above[(x + 1) as usize] = exponent(magnitude(at(x, 2 * qy + 1)));
        }
    }
    let magsgn = ms.finish();
    let mel_bytes = mel.finish();
    let vlc_bytes = vlc.finish();
    let scup = mel_bytes.len() + vlc_bytes.len();
    if scup > MAX_SCUP {
        return Err(Error::unsupported(format_args!(
            "MEL and VLC need {scup} bytes, more than {MAX_SCUP}"
        )));
    }
    let mut segment = Vec::with_capacity(magsgn.len() + scup);
    segment.extend_from_slice(&magsgn);
    segment.extend_from_slice(&mel_bytes);
    segment.extend_from_slice(&vlc_bytes);
    let n = segment.len();
    segment[n - 1] = (scup >> 4) as u8;
    segment[n - 2] = (segment[n - 2] & 0xF0) | (scup & 0x0F) as u8;
    Ok(segment)
}
