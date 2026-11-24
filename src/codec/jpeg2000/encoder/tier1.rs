#[derive(Clone, Copy)]
struct MqState {
    qe: u32,
    nmps: u8,
    nlps: u8,
    switch_mps: bool,
}
const fn s(qe: u32, nmps: u8, nlps: u8, switch_mps: bool) -> MqState {
    MqState {
        qe,
        nmps,
        nlps,
        switch_mps,
    }
}
static MQ_STATES: [MqState; 47] = [
    s(0x5601, 1, 1, true),
    s(0x3401, 2, 6, false),
    s(0x1801, 3, 9, false),
    s(0x0AC1, 4, 12, false),
    s(0x0521, 5, 29, false),
    s(0x0221, 38, 33, false),
    s(0x5601, 7, 6, true),
    s(0x5401, 8, 14, false),
    s(0x4801, 9, 14, false),
    s(0x3801, 10, 14, false),
    s(0x3001, 11, 17, false),
    s(0x2401, 12, 18, false),
    s(0x1C01, 13, 20, false),
    s(0x1601, 29, 21, false),
    s(0x5601, 15, 14, true),
    s(0x5401, 16, 14, false),
    s(0x5101, 17, 15, false),
    s(0x4801, 18, 16, false),
    s(0x3801, 19, 17, false),
    s(0x3401, 20, 18, false),
    s(0x3001, 21, 19, false),
    s(0x2801, 22, 19, false),
    s(0x2401, 23, 20, false),
    s(0x2201, 24, 21, false),
    s(0x1C01, 25, 22, false),
    s(0x1801, 26, 23, false),
    s(0x1601, 27, 24, false),
    s(0x1401, 28, 25, false),
    s(0x1201, 29, 26, false),
    s(0x1101, 30, 27, false),
    s(0x0AC1, 31, 28, false),
    s(0x09C1, 32, 29, false),
    s(0x08A1, 33, 30, false),
    s(0x0521, 34, 31, false),
    s(0x0441, 35, 32, false),
    s(0x02A1, 36, 33, false),
    s(0x0221, 37, 34, false),
    s(0x0141, 38, 35, false),
    s(0x0111, 39, 36, false),
    s(0x0085, 40, 37, false),
    s(0x0049, 41, 38, false),
    s(0x0025, 42, 39, false),
    s(0x0015, 43, 40, false),
    s(0x0009, 44, 41, false),
    s(0x0005, 45, 42, false),
    s(0x0001, 45, 43, false),
    s(0x5601, 46, 46, false),
];
const CX_SIGN: usize = 9;
const CX_REFINE: usize = 14;
const CX_RUN_LEN: usize = 17;
const CX_UNIFORM: usize = 18;
const NUM_CONTEXT: usize = 19;
#[derive(Default)]
struct MqEncoder {
    a: u32,
    c: u32,
    ct: u32,
    out: Vec<u8>,
    index: [u8; NUM_CONTEXT],
    mps: [u8; NUM_CONTEXT],
}
impl MqEncoder {
    fn reset(&mut self) {
        self.a = 0x8000;
        self.c = 0;
        self.ct = 12;
        self.out.clear();
        self.out.push(0);
        self.index = [0; NUM_CONTEXT];
        self.mps = [0; NUM_CONTEXT];
        self.index[CX_UNIFORM] = 46;
        self.index[CX_RUN_LEN] = 3;
        self.index[0] = 4;
    }
    fn encode(&mut self, cx: usize, d: u8) {
        let st = MQ_STATES[self.index[cx] as usize];
        self.a = self.a.wrapping_sub(st.qe);
        if d == self.mps[cx] {
            if self.a & 0x8000 != 0 {
                self.c = self.c.wrapping_add(st.qe);
                return;
            }
            if self.a < st.qe {
                self.a = st.qe;
            } else {
                self.c = self.c.wrapping_add(st.qe);
            }
            self.index[cx] = st.nmps;
        } else {
            if self.a < st.qe {
                self.c = self.c.wrapping_add(st.qe);
            } else {
                self.a = st.qe;
            }
            if st.switch_mps {
                self.mps[cx] = 1 - self.mps[cx];
            }
            self.index[cx] = st.nlps;
        }
        while self.a & 0x8000 == 0 {
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.ct == 0 {
                self.byte_out();
            }
        }
    }
    fn byte_out(&mut self) {
        let last = self.out.len() - 1;
        if self.out[last] == 0xFF {
            self.out.push((self.c >> 20) as u8);
            self.c &= 0xFFFFF;
            self.ct = 7;
            return;
        }
        if self.c < 0x8000000 {
            self.out.push((self.c >> 19) as u8);
            self.c &= 0x7FFFF;
            self.ct = 8;
            return;
        }
        self.out[last] = self.out[last].wrapping_add(1);
        if self.out[last] == 0xFF {
            self.c &= 0x7FFFFFF;
            self.out.push((self.c >> 20) as u8);
            self.c &= 0xFFFFF;
            self.ct = 7;
            return;
        }
        self.out.push((self.c >> 19) as u8);
        self.c &= 0x7FFFF;
        self.ct = 8;
    }
    fn flush(&mut self) -> Vec<u8> {
        let temp = self.c.wrapping_add(self.a);
        self.c |= 0xFFFF;
        if self.c >= temp {
            self.c -= 0x8000;
        }
        self.c <<= self.ct;
        self.byte_out();
        self.c <<= self.ct;
        self.byte_out();
        let mut out = &self.out[1..];
        if let Some(&0xFF) = out.last() {
            out = &out[..out.len() - 1];
        }
        out.to_vec()
    }
}
const FLAG_SIG: u8 = 1;
const FLAG_NEG: u8 = 1 << 1;
const FLAG_VISITED: u8 = 1 << 2;
const FLAG_REFINED: u8 = 1 << 3;
pub(super) const ORIENT_LL: i64 = 0;
pub(super) const ORIENT_HL: i64 = 1;
pub(super) const ORIENT_LH: i64 = 2;
pub(super) const ORIENT_HH: i64 = 3;
fn zero_context(orientation: i64, mut h: i32, mut v: i32, d: i32) -> usize {
    if orientation == ORIENT_HL {
        std::mem::swap(&mut h, &mut v);
    }
    let cx = if orientation == ORIENT_HH {
        let hv = h + v;
        if d >= 3 {
            8
        } else if d == 2 {
            if hv >= 1 { 7 } else { 6 }
        } else if d == 1 {
            if hv >= 2 { 5 } else { 3 + hv }
        } else if hv >= 2 {
            2
        } else {
            hv
        }
    } else if h == 2 {
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
    };
    cx as usize
}
#[derive(Default)]
pub(super) struct BlockCoder {
    w: usize,
    h: usize,
    stride: usize,
    orientation: i64,
    mag: Vec<u32>,
    flags: Vec<u8>,
    mq: MqEncoder,
}
impl BlockCoder {
    pub(crate) fn encode_block(
        &mut self,
        coeffs: &[i32],
        w: usize,
        h: usize,
        orientation: i64,
    ) -> (Vec<u8>, i64, i64) {
        self.w = w;
        self.h = h;
        self.stride = w + 2;
        self.orientation = orientation;
        let n = (w + 2) * (h + 2);
        self.flags.clear();
        self.flags.resize(n, 0);
        self.mag.clear();
        self.mag.resize(n, 0);
        let mut max_mag = 0u32;
        for y in 0..h {
            for x in 0..w {
                let v = coeffs[y * w + x];
                let i = (y + 1) * self.stride + x + 1;
                if v < 0 {
                    self.mag[i] = v.wrapping_neg() as u32;
                    self.flags[i] = FLAG_NEG;
                } else {
                    self.mag[i] = v as u32;
                }
                max_mag = max_mag.max(self.mag[i]);
            }
        }
        let mut planes = 0i64;
        while max_mag > 0 {
            planes += 1;
            max_mag >>= 1;
        }
        if planes == 0 {
            return (Vec::new(), 0, 0);
        }
        self.mq.reset();
        let mut p = planes - 1;
        while p >= 0 {
            if p != planes - 1 {
                self.significance_pass(p as u32);
                self.refinement_pass(p as u32);
            }
            self.cleanup_pass(p as u32);
            for f in self.flags.iter_mut() {
                *f &= !FLAG_VISITED;
            }
            p -= 1;
        }
        let data = self.mq.flush();
        (data, 3 * planes - 2, planes)
    }
    fn neighbors(&self, i: usize) -> (i32, i32, i32) {
        let f = &self.flags;
        let s = self.stride;
        let sig = |j: usize| (f[j] & FLAG_SIG) as i32;
        let h = sig(i - 1) + sig(i + 1);
        let v = sig(i - s) + sig(i + s);
        let d = sig(i - s - 1) + sig(i - s + 1) + sig(i + s - 1) + sig(i + s + 1);
        (h, v, d)
    }
    fn contribution(&self, i: usize) -> i32 {
        if self.flags[i] & FLAG_SIG == 0 {
            0
        } else if self.flags[i] & FLAG_NEG != 0 {
            -1
        } else {
            1
        }
    }
    fn code_sign(&mut self, i: usize) {
        let s = self.stride;
        let h = (self.contribution(i - 1) + self.contribution(i + 1)).clamp(-1, 1);
        let v = (self.contribution(i - s) + self.contribution(i + s)).clamp(-1, 1);
        let mut xor = 0u8;
        let cx: usize = match h {
            0 => {
                if v < 0 {
                    xor = 1;
                }
                if v != 0 { CX_SIGN + 1 } else { CX_SIGN }
            }
            1 => {
                if v < 0 {
                    CX_SIGN + 2
                } else {
                    (CX_SIGN as i32 + 3 + v) as usize
                }
            }
            _ => {
                xor = 1;
                match v {
                    0 => CX_SIGN + 3,
                    1 => CX_SIGN + 2,
                    _ => CX_SIGN + 4,
                }
            }
        };
        let sign = u8::from(self.flags[i] & FLAG_NEG != 0);
        self.mq.encode(cx, sign ^ xor);
    }
    fn bit(&self, i: usize, p: u32) -> u8 {
        ((self.mag[i] >> p) & 1) as u8
    }
    fn significance_pass(&mut self, p: u32) {
        let mut y0 = 0;
        while y0 < self.h {
            for x in 0..self.w {
                let mut y = y0;
                while y < y0 + 4 && y < self.h {
                    let i = (y + 1) * self.stride + x + 1;
                    y += 1;
                    if self.flags[i] & FLAG_SIG != 0 {
                        continue;
                    }
                    let (h, v, d) = self.neighbors(i);
                    if h + v + d == 0 {
                        continue;
                    }
                    let bit = self.bit(i, p);
                    self.mq.encode(zero_context(self.orientation, h, v, d), bit);
                    self.flags[i] |= FLAG_VISITED;
                    if bit != 0 {
                        self.code_sign(i);
                        self.flags[i] |= FLAG_SIG;
                    }
                }
            }
            y0 += 4;
        }
    }
    fn refinement_pass(&mut self, p: u32) {
        let mut y0 = 0;
        while y0 < self.h {
            for x in 0..self.w {
                let mut y = y0;
                while y < y0 + 4 && y < self.h {
                    let i = (y + 1) * self.stride + x + 1;
                    y += 1;
                    if self.flags[i] & (FLAG_SIG | FLAG_VISITED) != FLAG_SIG {
                        continue;
                    }
                    let mut cx = CX_REFINE + 2;
                    if self.flags[i] & FLAG_REFINED == 0 {
                        cx = CX_REFINE;
                        let (h, v, d) = self.neighbors(i);
                        if h + v + d > 0 {
                            cx = CX_REFINE + 1;
                        }
                    }
                    let bit = self.bit(i, p);
                    self.mq.encode(cx, bit);
                    self.flags[i] |= FLAG_REFINED;
                }
            }
            y0 += 4;
        }
    }
    fn cleanup_pass(&mut self, p: u32) {
        let mut y0 = 0;
        while y0 < self.h {
            for x in 0..self.w {
                let mut y = y0;
                if y0 + 4 <= self.h && self.quiet_column(x, y0) {
                    let mut first: i64 = -1;
                    for k in 0..4 {
                        if self.bit((y0 + k + 1) * self.stride + x + 1, p) != 0 {
                            first = k as i64;
                            break;
                        }
                    }
                    if first < 0 {
                        self.mq.encode(CX_RUN_LEN, 0);
                        continue;
                    }
                    self.mq.encode(CX_RUN_LEN, 1);
                    self.mq.encode(CX_UNIFORM, (first >> 1) as u8);
                    self.mq.encode(CX_UNIFORM, (first & 1) as u8);
                    let first = first as usize;
                    let i = (y0 + first + 1) * self.stride + x + 1;
                    self.code_sign(i);
                    self.flags[i] |= FLAG_SIG;
                    y = y0 + first + 1;
                }
                while y < y0 + 4 && y < self.h {
                    let i = (y + 1) * self.stride + x + 1;
                    y += 1;
                    if self.flags[i] & (FLAG_SIG | FLAG_VISITED) != 0 {
                        continue;
                    }
                    let (h, v, d) = self.neighbors(i);
                    let bit = self.bit(i, p);
                    self.mq.encode(zero_context(self.orientation, h, v, d), bit);
                    if bit != 0 {
                        self.code_sign(i);
                        self.flags[i] |= FLAG_SIG;
                    }
                }
            }
            y0 += 4;
        }
    }
    fn quiet_column(&self, x: usize, y0: usize) -> bool {
        for k in 0..4 {
            let i = (y0 + k + 1) * self.stride + x + 1;
            if self.flags[i] & (FLAG_SIG | FLAG_VISITED) != 0 {
                return false;
            }
            let (h, v, d) = self.neighbors(i);
            if h + v + d != 0 {
                return false;
            }
        }
        true
    }
}
