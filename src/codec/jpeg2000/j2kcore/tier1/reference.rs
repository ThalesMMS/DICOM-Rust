use self::mq::{MqContext, MqDecoder};
use super::super::codestream::{BLOCK_HT, BLOCK_RESET, BLOCK_SEG_SYM, BLOCK_VSC};
use super::super::geometry::{BAND_HH, BAND_HL};
use super::super::packet::{CodeBlockData, raw_pass};
use super::super::{CoreError, Result, errorf};
use crate::codec::jpeg2000::htj2k;
use std::sync::OnceLock;
pub(super) mod mq {
    #[derive(Clone, Copy)]
    struct QeEntry {
        qe: u32,
        nmps: u8,
        nlps: u8,
        switch_mps: bool,
    }
    const fn e(qe: u32, nmps: u8, nlps: u8, switch_mps: bool) -> QeEntry {
        QeEntry {
            qe,
            nmps,
            nlps,
            switch_mps,
        }
    }
    static QE_TABLE: [QeEntry; 47] = [
        e(0x5601, 1, 1, true),
        e(0x3401, 2, 6, false),
        e(0x1801, 3, 9, false),
        e(0x0AC1, 4, 12, false),
        e(0x0521, 5, 29, false),
        e(0x0221, 38, 33, false),
        e(0x5601, 7, 6, true),
        e(0x5401, 8, 14, false),
        e(0x4801, 9, 14, false),
        e(0x3801, 10, 14, false),
        e(0x3001, 11, 17, false),
        e(0x2401, 12, 18, false),
        e(0x1C01, 13, 20, false),
        e(0x1601, 29, 21, false),
        e(0x5601, 15, 14, true),
        e(0x5401, 16, 14, false),
        e(0x5101, 17, 15, false),
        e(0x4801, 18, 16, false),
        e(0x3801, 19, 17, false),
        e(0x3401, 20, 18, false),
        e(0x3001, 21, 19, false),
        e(0x2801, 22, 19, false),
        e(0x2401, 23, 20, false),
        e(0x2201, 24, 21, false),
        e(0x1C01, 25, 22, false),
        e(0x1801, 26, 23, false),
        e(0x1601, 27, 24, false),
        e(0x1401, 28, 25, false),
        e(0x1201, 29, 26, false),
        e(0x1101, 30, 27, false),
        e(0x0AC1, 31, 28, false),
        e(0x09C1, 32, 29, false),
        e(0x08A1, 33, 30, false),
        e(0x0521, 34, 31, false),
        e(0x0441, 35, 32, false),
        e(0x02A1, 36, 33, false),
        e(0x0221, 37, 34, false),
        e(0x0141, 38, 35, false),
        e(0x0111, 39, 36, false),
        e(0x0085, 40, 37, false),
        e(0x0049, 41, 38, false),
        e(0x0025, 42, 39, false),
        e(0x0015, 43, 40, false),
        e(0x0009, 44, 41, false),
        e(0x0005, 45, 42, false),
        e(0x0001, 45, 43, false),
        e(0x5601, 46, 46, false),
    ];
    #[derive(Clone, Copy, Default)]
    pub(crate) struct MqContext {
        pub index: u8,
        pub mps: u8,
    }
    pub(crate) struct MqDecoder {
        data: Vec<u8>,
        pub bp: usize,
        c: u32,
        a: u32,
        ct: i64,
        pub contexts: Vec<MqContext>,
    }
    impl MqDecoder {
        pub fn new(data: &[u8], contexts: usize) -> MqDecoder {
            let mut d = MqDecoder {
                data: data.to_vec(),
                bp: 0,
                c: 0,
                a: 0,
                ct: 0,
                contexts: vec![MqContext::default(); contexts],
            };
            d.init();
            d
        }
        pub fn start(&mut self, data: &[u8]) {
            self.data.clear();
            self.data.extend_from_slice(data);
            self.init();
        }
        pub fn set_context(&mut self, cx: usize, index: u8, mps: u8) {
            if cx < self.contexts.len() {
                self.contexts[cx] = MqContext { index, mps };
            }
        }
        fn byte_at(&self, i: usize) -> u32 {
            match self.data.get(i) {
                Some(&b) => b as u32,
                None => 0xFF,
            }
        }
        fn init(&mut self) {
            self.bp = 0;
            self.c = self.byte_at(0) << 16;
            self.byte_in();
            self.c <<= 7;
            self.ct -= 7;
            self.a = 0x8000;
        }
        fn byte_in(&mut self) {
            if self.byte_at(self.bp) == 0xFF {
                if self.byte_at(self.bp + 1) > 0x8F {
                    self.c = self.c.wrapping_add(0xFF00);
                    self.ct = 8;
                    return;
                }
                self.bp += 1;
                self.c = self.c.wrapping_add(self.byte_at(self.bp) << 9);
                self.ct = 7;
                return;
            }
            self.bp += 1;
            self.c = self.c.wrapping_add(self.byte_at(self.bp) << 8);
            self.ct = 8;
        }
        pub fn decode(&mut self, cx: usize) -> i32 {
            let ctx = self.contexts[cx];
            let entry = QE_TABLE[ctx.index as usize];
            let qe = entry.qe;
            self.a = self.a.wrapping_sub(qe);
            if (self.c >> 16) < qe {
                let bit = self.lps_exchange(cx, entry, qe);
                self.renorm();
                return bit;
            }
            self.c = self.c.wrapping_sub(qe << 16);
            if self.a & 0x8000 != 0 {
                return ctx.mps as i32;
            }
            let bit = self.mps_exchange(cx, entry);
            self.renorm();
            bit
        }
        fn mps_exchange(&mut self, cx: usize, entry: QeEntry) -> i32 {
            let ctx = &mut self.contexts[cx];
            if self.a < entry.qe {
                let bit = 1 - ctx.mps as i32;
                if entry.switch_mps {
                    ctx.mps = 1 - ctx.mps;
                }
                ctx.index = entry.nlps;
                return bit;
            }
            ctx.index = entry.nmps;
            ctx.mps as i32
        }
        fn lps_exchange(&mut self, cx: usize, entry: QeEntry, qe: u32) -> i32 {
            let ctx = &mut self.contexts[cx];
            if self.a < qe {
                self.a = qe;
                ctx.index = entry.nmps;
                return ctx.mps as i32;
            }
            self.a = qe;
            let bit = 1 - ctx.mps as i32;
            if entry.switch_mps {
                ctx.mps = 1 - ctx.mps;
            }
            ctx.index = entry.nlps;
            bit
        }
        fn renorm(&mut self) {
            loop {
                if self.ct == 0 {
                    self.byte_in();
                }
                self.a <<= 1;
                self.c <<= 1;
                self.ct -= 1;
                if self.a & 0x8000 != 0 {
                    return;
                }
            }
        }
    }
}
const CX_ZERO: usize = 0;
const CX_SIGN_FIRST: usize = 9;
const CX_REFINE_NONE: usize = 14;
const CX_REFINE_NEAR: usize = 15;
const CX_REFINE_MORE: usize = 16;
const CX_RUN_LEN: usize = 17;
const CX_UNIFORM: usize = 18;
const NUM_CONTEXTS: usize = 19;
const FLAG_SIGNIFICANT: u16 = 1;
const FLAG_VISITED: u16 = 1 << 1;
const FLAG_REFINED: u16 = 1 << 2;
const FLAG_NEGATIVE: u16 = 1 << 3;
const FLAG_LEFT: u16 = 1 << 4;
const FLAG_RIGHT: u16 = 1 << 5;
const FLAG_UP: u16 = 1 << 6;
const FLAG_DOWN: u16 = 1 << 7;
const FLAG_NW: u16 = 1 << 8;
const FLAG_NE: u16 = 1 << 9;
const FLAG_SW: u16 = 1 << 10;
const FLAG_SE: u16 = 1 << 11;
const FLAG_NEIGHBORS: u16 =
    FLAG_LEFT | FLAG_RIGHT | FLAG_UP | FLAG_DOWN | FLAG_NW | FLAG_NE | FLAG_SW | FLAG_SE;
const NEIGHBOR_SHIFT: u16 = 4;
const BELOW_STRIPE: u16 = FLAG_DOWN | FLAG_SW | FLAG_SE;
const NEIGHBOR_WRITES: [(isize, isize, u16); 8] = [
    (-1, 0, FLAG_RIGHT),
    (1, 0, FLAG_LEFT),
    (0, -1, FLAG_DOWN),
    (0, 1, FLAG_UP),
    (-1, -1, FLAG_SE),
    (1, -1, FLAG_SW),
    (-1, 1, FLAG_NE),
    (1, 1, FLAG_NW),
];
const ORIENT_LL_LH: usize = 0;
const ORIENT_HL: usize = 1;
const ORIENT_HH: usize = 2;
const PASS_SIGNIFICANCE: i32 = 0;
const PASS_REFINEMENT: i32 = 1;
const PASS_CLEANUP: i32 = 2;
fn orientation_of(band_type: i64) -> usize {
    match band_type {
        BAND_HL => ORIENT_HL,
        BAND_HH => ORIENT_HH,
        _ => ORIENT_LL_LH,
    }
}
fn zero_coding_tables() -> &'static [[u8; 256]; 3] {
    static TABLES: OnceLock<[[u8; 256]; 3]> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut tables = [[0u8; 256]; 3];
        for (orientation, table) in tables.iter_mut().enumerate() {
            for (mask, entry) in table.iter_mut().enumerate() {
                let bits = |positions: &[usize]| {
                    positions.iter().filter(|&&p| mask & (1 << p) != 0).count() as i32
                };
                let h = bits(&[0, 1]);
                let v = bits(&[2, 3]);
                let d = bits(&[4, 5, 6, 7]);
                *entry = zero_coding_context(orientation, h, v, d) as u8;
            }
        }
        tables
    })
}
fn zero_coding_context(orientation: usize, mut h: i32, mut v: i32, d: i32) -> i32 {
    if orientation == ORIENT_HL {
        std::mem::swap(&mut h, &mut v);
    }
    if orientation == ORIENT_HH {
        let hv = h + v;
        return if d >= 3 {
            8
        } else if d == 2 {
            if hv >= 1 { 7 } else { 6 }
        } else if d == 1 {
            if hv >= 2 { 5 } else { 3 + hv }
        } else if hv >= 2 {
            2
        } else {
            hv
        };
    }
    if h == 2 {
        8
    } else if h == 1 {
        if v >= 1 {
            7
        } else if d >= 1 {
            6
        } else {
            5
        }
    } else if v == 2 {
        4
    } else if v == 1 {
        3
    } else if d >= 2 {
        2
    } else {
        d
    }
}
#[derive(Default)]
struct RawDecoder {
    data: Vec<u8>,
    pos: usize,
    c: u32,
    ct: u32,
}
impl RawDecoder {
    fn start(&mut self, data: &[u8]) {
        self.data.clear();
        self.data.extend_from_slice(data);
        self.pos = 0;
        self.c = 0;
        self.ct = 0;
    }
    fn bit(&mut self) -> i32 {
        if self.ct == 0 {
            let next = self.data.get(self.pos).map_or(0xFF, |&b| b as u32);
            if self.c != 0xFF {
                self.c = next;
                self.ct = 8;
                self.pos += 1;
            } else if next > 0x8F {
                self.c = 0xFF;
                self.ct = 8;
            } else {
                self.c = next;
                self.ct = 7;
                self.pos += 1;
            }
        }
        self.ct -= 1;
        ((self.c >> self.ct) & 1) as i32
    }
}
struct BlockDecoder {
    w: usize,
    h: usize,
    stride: usize,
    flags: Vec<u16>,
    data: Vec<i32>,
    mq: MqDecoder,
    raw: RawDecoder,
    zc: &'static [u8; 256],
    vsc: bool,
}
impl BlockDecoder {
    fn new(w: usize, h: usize, band_type: i64) -> BlockDecoder {
        let stride = w + 2;
        let mut d = BlockDecoder {
            w,
            h,
            stride,
            flags: vec![0; stride * (h + 2)],
            data: vec![0; w * h],
            mq: MqDecoder::new(&[], NUM_CONTEXTS),
            raw: RawDecoder::default(),
            zc: &zero_coding_tables()[orientation_of(band_type)],
            vsc: false,
        };
        d.reset_contexts();
        d
    }
    fn reset_contexts(&mut self) {
        for cx in self.mq.contexts.iter_mut() {
            *cx = MqContext::default();
        }
        self.mq.set_context(CX_UNIFORM, 46, 0);
        self.mq.set_context(CX_RUN_LEN, 3, 0);
        self.mq.set_context(CX_ZERO, 4, 0);
    }
    fn context_flags(&self, i: usize, y: usize) -> u16 {
        if self.vsc && y % 4 == 3 {
            return self.flags[i] & !BELOW_STRIPE;
        }
        self.flags[i]
    }
    fn index(&self, x: usize, y: usize) -> usize {
        (y + 1) * self.stride + x + 1
    }
    fn significance_context(&self, flags: u16) -> usize {
        self.zc[((flags >> NEIGHBOR_SHIFT) & 0xFF) as usize] as usize
    }
    fn contribution(&self, i: usize) -> i32 {
        let f = self.flags[i];
        if f & FLAG_SIGNIFICANT == 0 {
            0
        } else if f & FLAG_NEGATIVE != 0 {
            -1
        } else {
            1
        }
    }
    fn sign_context(&self, i: usize, y: usize) -> (usize, i32) {
        let h = self.contribution(i - 1) + self.contribution(i + 1);
        let mut v = self.contribution(i - self.stride);
        if !self.vsc || y % 4 != 3 {
            v += self.contribution(i + self.stride);
        }
        let (h, v) = (h.clamp(-1, 1), v.clamp(-1, 1));
        match (h, v) {
            (0, 0) => (CX_SIGN_FIRST, 0),
            (0, 1) => (CX_SIGN_FIRST + 1, 0),
            (0, _) => (CX_SIGN_FIRST + 1, 1),
            (1, 0) => (CX_SIGN_FIRST + 3, 0),
            (1, 1) => (CX_SIGN_FIRST + 4, 0),
            (1, _) => (CX_SIGN_FIRST + 2, 0),
            (_, 0) => (CX_SIGN_FIRST + 3, 1),
            (_, 1) => (CX_SIGN_FIRST + 2, 1),
            _ => (CX_SIGN_FIRST + 4, 1),
        }
    }
    fn decode_sign(&mut self, i: usize, y: usize) {
        let (cx, xor) = self.sign_context(i, y);
        if self.mq.decode(cx) ^ xor == 1 {
            self.flags[i] |= FLAG_NEGATIVE;
        }
    }
    fn set_significant(&mut self, x: usize, y: usize, i: usize, plane: i64) {
        self.flags[i] |= FLAG_SIGNIFICANT;
        self.data[y * self.w + x] = 3 << plane;
        for (dx, dy, bit) in NEIGHBOR_WRITES {
            let j = (i as isize + dy * self.stride as isize + dx) as usize;
            self.flags[j] |= bit;
        }
    }
    fn significance_pass(&mut self, plane: i64) {
        let mut y0 = 0;
        while y0 < self.h {
            for x in 0..self.w {
                let mut i = self.index(x, y0);
                let mut y = y0;
                while y < y0 + 4 && y < self.h {
                    let f = self.context_flags(i, y);
                    if !(f & FLAG_SIGNIFICANT != 0 || f & FLAG_NEIGHBORS == 0) {
                        let cx = self.significance_context(f);
                        if self.mq.decode(cx) == 1 {
                            self.set_significant(x, y, i, plane);
                            self.decode_sign(i, y);
                        }
                        self.flags[i] |= FLAG_VISITED;
                    }
                    y += 1;
                    i += self.stride;
                }
            }
            y0 += 4;
        }
    }
    fn raw_significance_pass(&mut self, plane: i64) {
        let mut y0 = 0;
        while y0 < self.h {
            for x in 0..self.w {
                let mut i = self.index(x, y0);
                let mut y = y0;
                while y < y0 + 4 && y < self.h {
                    let f = self.context_flags(i, y);
                    if !(f & FLAG_SIGNIFICANT != 0 || f & FLAG_NEIGHBORS == 0) {
                        if self.raw.bit() == 1 {
                            self.set_significant(x, y, i, plane);
                            if self.raw.bit() == 1 {
                                self.flags[i] |= FLAG_NEGATIVE;
                            }
                        }
                        self.flags[i] |= FLAG_VISITED;
                    }
                    y += 1;
                    i += self.stride;
                }
            }
            y0 += 4;
        }
    }
    fn raw_refinement_pass(&mut self, plane: i64) {
        let mut y0 = 0;
        while y0 < self.h {
            for x in 0..self.w {
                let mut i = self.index(x, y0);
                let mut y = y0;
                while y < y0 + 4 && y < self.h {
                    let f = self.flags[i];
                    if !(f & FLAG_SIGNIFICANT == 0 || f & FLAG_VISITED != 0) {
                        let k = y * self.w + x;
                        if self.raw.bit() == 1 {
                            self.data[k] = self.data[k].wrapping_add(1 << plane);
                        } else {
                            self.data[k] = self.data[k].wrapping_sub(1 << plane);
                        }
                        self.flags[i] |= FLAG_REFINED;
                    }
                    y += 1;
                    i += self.stride;
                }
            }
            y0 += 4;
        }
    }
    fn refinement_pass(&mut self, plane: i64) {
        let mut y0 = 0;
        while y0 < self.h {
            for x in 0..self.w {
                let mut i = self.index(x, y0);
                let mut y = y0;
                while y < y0 + 4 && y < self.h {
                    let f = self.context_flags(i, y);
                    if !(f & FLAG_SIGNIFICANT == 0 || f & FLAG_VISITED != 0) {
                        let mut cx = CX_REFINE_MORE;
                        if f & FLAG_REFINED == 0 {
                            cx = if f & FLAG_NEIGHBORS == 0 {
                                CX_REFINE_NONE
                            } else {
                                CX_REFINE_NEAR
                            };
                        }
                        let k = y * self.w + x;
                        if self.mq.decode(cx) == 1 {
                            self.data[k] = self.data[k].wrapping_add(1 << plane);
                        } else {
                            self.data[k] = self.data[k].wrapping_sub(1 << plane);
                        }
                        self.flags[i] |= FLAG_REFINED;
                    }
                    y += 1;
                    i += self.stride;
                }
            }
            y0 += 4;
        }
    }
    fn cleanup_pass(&mut self, plane: i64) {
        let mut y0 = 0;
        while y0 < self.h {
            for x in 0..self.w {
                let mut y = y0;
                while y < y0 + 4 && y < self.h {
                    let mut i = self.index(x, y);
                    if y == y0 && y0 + 4 <= self.h && self.column_is_clean(i, y0) {
                        if self.mq.decode(CX_RUN_LEN) == 0 {
                            self.clear_visited(i, 4);
                            y = y0 + 4;
                            continue;
                        }
                        let hi = self.mq.decode(CX_UNIFORM);
                        let lo = self.mq.decode(CX_UNIFORM);
                        let first = ((hi << 1) | lo) as usize;
                        self.clear_visited(i, first);
                        y = y0 + first;
                        i += first * self.stride;
                        self.set_significant(x, y, i, plane);
                        self.decode_sign(i, y);
                        self.flags[i] &= !FLAG_VISITED;
                        y += 1;
                        continue;
                    }
                    let f = self.context_flags(i, y);
                    if f & (FLAG_SIGNIFICANT | FLAG_VISITED) == 0 {
                        let cx = self.significance_context(f);
                        if self.mq.decode(cx) == 1 {
                            self.set_significant(x, y, i, plane);
                            self.decode_sign(i, y);
                        }
                    }
                    self.flags[i] &= !FLAG_VISITED;
                    y += 1;
                }
            }
            y0 += 4;
        }
    }
    fn column_is_clean(&self, i: usize, y0: usize) -> bool {
        const BUSY: u16 = FLAG_SIGNIFICANT | FLAG_VISITED | FLAG_NEIGHBORS;
        self.flags[i] & BUSY == 0
            && self.flags[i + self.stride] & BUSY == 0
            && self.flags[i + 2 * self.stride] & BUSY == 0
            && self.context_flags(i + 3 * self.stride, y0 + 3) & BUSY == 0
    }
    fn clear_visited(&mut self, i: usize, n: usize) {
        for k in 0..n {
            self.flags[i + k * self.stride] &= !FLAG_VISITED;
        }
    }
    fn apply_signs(&mut self) {
        for y in 0..self.h {
            for x in 0..self.w {
                if self.flags[self.index(x, y)] & FLAG_NEGATIVE != 0 {
                    let k = y * self.w + x;
                    self.data[k] = self.data[k].wrapping_neg();
                }
            }
        }
    }
    fn decode_ht(&mut self, block: &CodeBlockData, plane: i64, magnitude_bits: i64) -> Result<()> {
        if block.passes != 1 || block.segments.len() != 1 {
            return Err(CoreError::Unsupported(
                "HT refinement or placeholder passes".into(),
            ));
        }
        let bound = (magnitude_bits - plane + 1).min(31);
        if let Err(e) = htj2k::decode_cleanup(
            &block.segments[0].data,
            self.w as i64,
            self.h as i64,
            &mut self.data,
            bound,
        ) {
            return Err(errorf(format_args!("HT code-block: {e}")));
        }
        let limit = 1i64 << 30;
        for v in self.data.iter_mut() {
            if *v == 0 {
                continue;
            }
            let mu = (*v as i64).abs();
            let mut halves = (2 * mu + 1) << plane;
            if halves >= limit {
                return Err(errorf("HT code-block value past what an int32 holds"));
            }
            if *v < 0 {
                halves = -halves;
            }
            *v = halves as i32;
        }
        Ok(())
    }
}
pub(in super::super) fn decode_block(
    block: &CodeBlockData,
    magnitude_bits: i64,
) -> Result<Vec<i32>> {
    let (w, h) = (block.block.width(), block.block.height());
    if w <= 0 || h <= 0 {
        return Err(errorf(format_args!("a code-block of {w}x{h} samples")));
    }
    if w > 1 << 12 || h > 1 << 12 || w * h > 1 << 22 {
        return Err(errorf(format_args!(
            "a code-block of {w}x{h} samples is larger than any legal one"
        )));
    }
    let mut d = BlockDecoder::new(w as usize, h as usize, block.band_type);
    d.vsc = block.style & BLOCK_VSC != 0;
    let mut plane = magnitude_bits - 1 - block.zero_bit_planes;
    if plane < 0 {
        return Ok(d.data);
    }
    if plane > 29 {
        return Err(errorf(format_args!(
            "a code-block claims bit plane {plane}, past what an int32 holds"
        )));
    }
    if block.style & BLOCK_HT != 0 {
        d.decode_ht(block, plane, magnitude_bits)?;
        return Ok(d.data);
    }
    let limit = 3 * (plane + 1) - 2;
    if block.passes > limit {
        return Err(errorf(format_args!(
            "a code-block claims {} coding passes, of the {limit} its {} bit planes hold",
            block.passes,
            plane + 1
        )));
    }
    let mut segment: i64 = -1;
    let mut left = 0i64;
    let mut segment_raw = false;
    let mut pass_type = PASS_CLEANUP;
    let mut pass = 0i64;
    while pass < block.passes && plane >= 0 {
        let raw = raw_pass(pass, block.style);
        if left == 0 {
            segment += 1;
            if segment >= block.segments.len() as i64 {
                break;
            }
            let seg = &block.segments[segment as usize];
            left = seg.passes;
            segment_raw = raw;
            if raw {
                d.raw.start(&seg.data);
            } else {
                d.mq.start(&seg.data);
            }
        }
        if raw != segment_raw {
            return Err(errorf(
                "a codeword segment mixes raw and arithmetic coding passes",
            ));
        }
        if block.style & BLOCK_RESET != 0 && pass > 0 {
            d.reset_contexts();
        }
        match pass_type {
            PASS_SIGNIFICANCE if raw => d.raw_significance_pass(plane),
            PASS_SIGNIFICANCE => d.significance_pass(plane),
            PASS_REFINEMENT if raw => d.raw_refinement_pass(plane),
            PASS_REFINEMENT => d.refinement_pass(plane),
            _ => {
                d.cleanup_pass(plane);
                if block.style & BLOCK_SEG_SYM != 0 {
                    for _ in 0..4 {
                        d.mq.decode(CX_UNIFORM);
                    }
                }
            }
        }
        left -= 1;
        if pass_type == PASS_CLEANUP {
            plane -= 1;
            pass_type = PASS_SIGNIFICANCE;
        } else {
            pass_type += 1;
        }
        pass += 1;
    }
    d.apply_signs();
    Ok(d.data)
}
