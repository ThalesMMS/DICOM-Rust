const QE_TABLE: [(u16, u8, u8, bool); 47] = [
    (0x5601, 1, 1, true),
    (0x3401, 2, 6, false),
    (0x1801, 3, 9, false),
    (0x0AC1, 4, 12, false),
    (0x0521, 5, 29, false),
    (0x0221, 38, 33, false),
    (0x5601, 7, 6, true),
    (0x5401, 8, 14, false),
    (0x4801, 9, 14, false),
    (0x3801, 10, 14, false),
    (0x3001, 11, 17, false),
    (0x2401, 12, 18, false),
    (0x1C01, 13, 20, false),
    (0x1601, 29, 21, false),
    (0x5601, 15, 14, true),
    (0x5401, 16, 14, false),
    (0x5101, 17, 15, false),
    (0x4801, 18, 16, false),
    (0x3801, 19, 17, false),
    (0x3401, 20, 18, false),
    (0x3001, 21, 19, false),
    (0x2801, 22, 19, false),
    (0x2401, 23, 20, false),
    (0x2201, 24, 21, false),
    (0x1C01, 25, 22, false),
    (0x1801, 26, 23, false),
    (0x1601, 27, 24, false),
    (0x1401, 28, 25, false),
    (0x1201, 29, 26, false),
    (0x1101, 30, 27, false),
    (0x0AC1, 31, 28, false),
    (0x09C1, 32, 29, false),
    (0x08A1, 33, 30, false),
    (0x0521, 34, 31, false),
    (0x0441, 35, 32, false),
    (0x02A1, 36, 33, false),
    (0x0221, 37, 34, false),
    (0x0141, 38, 35, false),
    (0x0111, 39, 36, false),
    (0x0085, 40, 37, false),
    (0x0049, 41, 38, false),
    (0x0025, 42, 39, false),
    (0x0015, 43, 40, false),
    (0x0009, 44, 41, false),
    (0x0005, 45, 42, false),
    (0x0001, 45, 43, false),
    (0x5601, 46, 46, false),
];
const NEXT_SHIFT: [u32; 2] = [17, 24];
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
static STATES: [u32; 128] = {
    let mut states = [0x5601u32; 128];
    let mut i = 0;
    while i < QE_TABLE.len() {
        let (qe, nmps, nlps, switch) = QE_TABLE[i];
        let mut mps = 0u32;
        while mps < 2 {
            let after_mps = 2 * nmps as u32 + mps;
            let after_lps = 2 * nlps as u32 + (mps ^ switch as u32);
            states[2 * i + mps as usize] =
                qe as u32 | mps << 16 | after_mps << NEXT_SHIFT[0] | after_lps << NEXT_SHIFT[1];
            mps += 1;
        }
        i += 1;
    }
    states
};
#[inline(always)]
#[allow(clippy::indexing_slicing)]
fn next_state(state: u32, lps: u32) -> u32 {
    STATES[((state >> NEXT_SHIFT[(lps & 1) as usize]) & 127) as usize]
}
pub(super) const SEGMENT_SENTINEL: [u8; 2] = [0xFF, 0xFF];
const CONTEXTS: usize = 32;
#[derive(Clone, Copy)]
pub(super) struct Contexts([u32; CONTEXTS]);
impl Contexts {
    pub fn new() -> Contexts {
        Contexts([STATES[0]; CONTEXTS])
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub fn set(&mut self, cx: usize, index: u8, mps: u8) {
        let row = (2 * index as usize + mps as usize) % STATES.len();
        self.0[cx % CONTEXTS] = STATES[row];
    }
}
#[derive(Clone, Copy)]
pub(super) struct Mq {
    a: u32,
    c: u64,
    available: u32,
    bp: usize,
}
const C_SHIFT: u32 = 32;
impl Mq {
    pub fn new() -> Mq {
        Mq {
            a: 0x8000,
            c: 0,
            available: 0,
            bp: 0,
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub fn start(&mut self, buf: &[u8]) {
        self.bp = 0;
        self.c = (buf[0] as u64) << (16 + C_SHIFT);
        self.available = 0;
        self.byte_in(buf);
        self.c <<= 7;
        self.available -= 7;
        self.a = 0x8000;
    }
    #[inline(always)]
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn byte_in(&mut self, buf: &[u8]) {
        if buf[self.bp] == 0xFF {
            let next = buf[self.bp + 1] as u64;
            if next > 0x8F {
                self.c = self.c.wrapping_add(0xFF00 << C_SHIFT);
                self.available = 8;
            } else {
                self.bp += 1;
                self.c = self.c.wrapping_add(next << (9 + C_SHIFT));
                self.available = 7;
            }
        } else {
            self.bp += 1;
            self.c = self.c.wrapping_add((buf[self.bp] as u64) << (8 + C_SHIFT));
            self.available = 8;
        }
        while self.available <= C_SHIFT && buf[self.bp] != 0xFF {
            self.bp += 1;
            self.c |= (buf[self.bp] as u64) << (8 + C_SHIFT - self.available);
            self.available += 8;
        }
    }
    #[cold]
    #[inline(never)]
    #[allow(clippy::arithmetic_side_effects)]
    fn renormalize_reading(mut self, buf: &[u8], mut a: u32, mut shifts: u32) -> Mq {
        loop {
            if self.available == 0 {
                self.byte_in(buf);
            }
            let step = shifts.min(self.available);
            a <<= step;
            self.c <<= step;
            self.available -= step;
            shifts -= step;
            if shifts == 0 {
                break;
            }
        }
        self.a = a;
        self
    }
    #[inline(always)]
    #[allow(clippy::arithmetic_side_effects)]
    fn renormalize(&mut self, buf: &[u8], a: u32) {
        let shifts = (a | 1).leading_zeros() - 16;
        if shifts > self.available {
            *self = self.renormalize_reading(buf, a, shifts);
        } else {
            self.a = a << shifts;
            self.c <<= shifts;
            self.available -= shifts;
        }
    }
    #[inline(always)]
    #[allow(clippy::indexing_slicing)]
    pub fn decode(&mut self, contexts: &mut Contexts, buf: &[u8], cx: usize) -> u32 {
        let slot = cx % CONTEXTS;
        let state = contexts.0[slot];
        let mps = (state >> 16) & 1;
        let qe = state & 0xFFFF;
        let a = self.a.wrapping_sub(qe);
        if ((self.c >> (16 + C_SHIFT)) as u32) < qe {
            let exchange = (a < qe) as u32;
            contexts.0[slot] = next_state(state, exchange ^ 1);
            self.renormalize(buf, qe);
            return mps ^ exchange ^ 1;
        }
        self.c = self.c.wrapping_sub((qe as u64) << (16 + C_SHIFT));
        if a & 0x8000 != 0 {
            self.a = a;
            return mps;
        }
        let exchange = (a < qe) as u32;
        contexts.0[slot] = next_state(state, exchange);
        self.renormalize(buf, a);
        mps ^ exchange
    }
    #[inline(always)]
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn decode_even(&mut self, contexts: &mut Contexts, buf: &[u8], cx: usize) -> u32 {
        let slot = cx % CONTEXTS;
        let state = contexts.0[slot];
        let qe = state & 0xFFFF;
        let a = self.a.wrapping_sub(qe);
        let below = |x: u32, y: u32| ((x.wrapping_sub(y) as i32) >> 31) as u32;
        let lps = below((self.c >> (16 + C_SHIFT)) as u32, qe);
        let exchange = below(a, qe);
        let renormalize = lps | below(a, 0x8000);
        let lps_decoded = (lps ^ exchange) & 1;
        let next = next_state(state, lps_decoded);
        contexts.0[slot] = (next & renormalize) | (state & !renormalize);
        self.c = self
            .c
            .wrapping_sub(((qe as u64) << (16 + C_SHIFT)) & !(lps as i32 as i64 as u64));
        let a = (qe & lps) | (a & !lps);
        let shifts = (a | 1).leading_zeros() - 16;
        if shifts > self.available {
            *self = self.renormalize_reading(buf, a, shifts);
        } else {
            self.a = a << shifts;
            self.c <<= shifts;
            self.available -= shifts;
        }
        ((state >> 16) & 1) ^ lps_decoded
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row(state: usize, mps: usize) -> u32 {
        STATES[2 * state + mps]
    }
    #[test]
    fn the_state_rows_follow_table_c2() {
        assert_eq!(next_state(row(0, 0), 1), row(1, 1));
        assert_eq!(next_state(row(0, 1), 1), row(1, 0));
        assert_eq!(next_state(row(0, 0), 0), row(1, 0));
        assert_eq!(next_state(row(6, 1), 1), row(6, 0));
        assert_eq!(next_state(row(46, 0), 0), row(46, 0));
        assert_eq!(next_state(row(46, 1), 1), row(46, 1));
        assert_eq!(row(5, 0) & 0xFFFF, 0x0221);
        assert_eq!(next_state(row(5, 1), 0), row(38, 1));
        assert_eq!(next_state(row(5, 1), 1), row(33, 1));
        assert_eq!((row(5, 1) >> 16) & 1, 1);
    }
}
