use super::{Error, corrupt, truncated};
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    nbuf: u32,
    pub(crate) err: Option<Error>,
    total: u64,
    pub(crate) guarded: bool,
}
#[derive(Clone, Copy)]
pub(crate) struct U32Dist {
    pub bits: u32,
    pub offset: u32,
}
pub(crate) const fn val(v: u32) -> U32Dist {
    U32Dist { bits: 0, offset: v }
}
pub(crate) const fn bits_off(n: u32, off: u32) -> U32Dist {
    U32Dist {
        bits: n,
        offset: off,
    }
}
pub(crate) const fn bits_n(n: u32) -> U32Dist {
    U32Dist { bits: n, offset: 0 }
}
impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            pos: 0,
            buf: 0,
            nbuf: 0,
            err: None,
            total: 0,
            guarded: false,
        }
    }
    pub(crate) fn check(&self) -> Result<(), Error> {
        match &self.err {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
    fn refill(&mut self) {
        while self.nbuf <= 56
            && let Some(&b) = self.data.get(self.pos)
        {
            self.buf |= u64::from(b) << self.nbuf;
            self.pos = self.pos.saturating_add(1);
            self.nbuf = self.nbuf.saturating_add(8);
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn read_bits(&mut self, n: u32) -> u64 {
        if n == 0 {
            return 0;
        }
        if n > 32 {
            let lo = self.read_bits(32);
            return lo | self.read_bits(n - 32) << 32;
        }
        if self.nbuf < n {
            self.refill();
            if self.nbuf < n {
                self.fail(truncated());
                self.total += u64::from(n);
                self.buf = 0;
                self.nbuf = 0;
                return 0;
            }
        }
        let v = self.buf & ((1u64 << n) - 1);
        self.buf >>= n;
        self.nbuf -= n;
        self.total += u64::from(n);
        v
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn peek_bits(&mut self, n: u32) -> u64 {
        if self.nbuf < n {
            self.refill();
        }
        self.buf & ((1u64 << n) - 1)
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn skip_bits(&mut self, n: u32) {
        if self.nbuf < n {
            self.refill();
            if self.nbuf < n {
                self.fail(truncated());
                self.total += u64::from(n);
                self.buf = 0;
                self.nbuf = 0;
                return;
            }
        }
        self.buf >>= n;
        self.nbuf -= n;
        self.total += u64::from(n);
    }
    pub(crate) fn u(&mut self, n: u32) -> u32 {
        self.read_bits(n) as u32
    }
    pub(crate) fn bool(&mut self) -> bool {
        self.read_bits(1) == 1
    }
    pub(crate) fn fail(&mut self, err: Error) {
        if self.err.is_none() {
            self.err = Some(err);
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn skip_large(&mut self, mut n: u64) {
        while n > 0 {
            let k = n.min(32) as u32;
            self.read_bits(k);
            n -= u64::from(k);
            if self.err.is_some() {
                return;
            }
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn zero_pad_to_byte(&mut self) {
        let r = (self.total % 8) as u32;
        if r != 0 && self.read_bits(8 - r) != 0 {
            self.fail(corrupt("non-zero padding bits"));
        }
    }
    pub(crate) fn data_len(&self) -> usize {
        self.data.len()
    }
    pub(crate) fn byte_pos(&self) -> usize {
        (self.total / 8) as usize
    }
    pub(crate) fn u32(&mut self, d0: U32Dist, d1: U32Dist, d2: U32Dist, d3: U32Dist) -> u32 {
        let d = match self.u(2) {
            0 => d0,
            1 => d1,
            2 => d2,
            _ => d3,
        };
        d.offset.wrapping_add(self.u(d.bits))
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn u64(&mut self) -> u64 {
        match self.u(2) {
            0 => return 0,
            1 => return 1 + u64::from(self.u(4)),
            2 => return 17 + u64::from(self.u(8)),
            _ => {}
        }
        let mut v = u64::from(self.u(12));
        let mut shift = 12;
        while self.bool() {
            if shift == 60 {
                v |= u64::from(self.u(4)) << shift;
                break;
            }
            v |= u64::from(self.u(8)) << shift;
            shift += 8;
        }
        v
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn f16(&mut self) -> f32 {
        let b = self.u(16);
        let sign = b >> 15;
        let exp = (b >> 10) & 31;
        let mant = b & 1023;
        if exp == 31 {
            self.fail(corrupt("non-finite F16 value"));
            return 0.0;
        }
        let f = if exp == 0 {
            f64::from(mant) / 1024.0 * (2f64).powi(-14)
        } else {
            (1.0 + f64::from(mant) / 1024.0) * (2f64).powi(exp as i32 - 15)
        };
        (if sign == 1 { -f } else { f }) as f32
    }
    pub(crate) fn enum_value(&mut self) -> u32 {
        let v = self.u32(val(0), val(1), bits_off(4, 2), bits_off(6, 18));
        if v >= 64 {
            self.fail(corrupt("enum value out of range"));
        }
        v
    }
    pub(crate) fn extensions(&mut self) {
        let mut ext = self.u64();
        let mut total: u64 = 0;
        while ext != 0 {
            let i = ext.trailing_zeros();
            ext &= !(1u64 << i);
            let n = self.u64();
            match total.checked_add(n) {
                Some(t) => total = t,
                None => {
                    self.fail(corrupt("extension length overflow"));
                    return;
                }
            }
        }
        self.skip_large(total);
    }
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn unpack_signed(v: u32) -> i32 {
    if v & 1 == 1 {
        -((v >> 1) as i32) - 1
    } else {
        (v >> 1) as i32
    }
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn unpack_signed64(v: u64) -> i64 {
    if v & 1 == 1 {
        -((v >> 1) as i64) - 1
    } else {
        (v >> 1) as i64
    }
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn ceil_log2(x: u64) -> u32 {
    if x <= 1 {
        0
    } else {
        64 - (x - 1).leading_zeros()
    }
}
pub(crate) fn floor_log2(x: u64) -> u32 {
    64u32.wrapping_sub(x.leading_zeros()).wrapping_sub(1)
}
pub(crate) fn div_ceil(a: usize, b: usize) -> usize {
    a.div_ceil(b)
}
#[derive(Default)]
pub(crate) struct BitWriter {
    pub(crate) buf: Vec<u8>,
    acc: u64,
    n: u32,
}
impl BitWriter {
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn write(&mut self, mut n: u32, mut v: u64) {
        while n > 32 {
            self.write(32, v & 0xFFFF_FFFF);
            v >>= 32;
            n -= 32;
        }
        if n == 0 {
            return;
        }
        self.acc |= (v & ((1u64 << n) - 1)) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.buf.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    pub(crate) fn bool(&mut self, b: bool) {
        self.write(1, u64::from(b));
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn zero_pad(&mut self) {
        if self.n > 0 {
            self.write(8 - self.n, 0);
        }
    }
    pub(crate) fn into_bytes(mut self) -> Vec<u8> {
        self.zero_pad();
        self.buf
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn bit_len(&self) -> u64 {
        self.buf.len() as u64 * 8 + u64::from(self.n)
    }
    pub(crate) fn append_writer(&mut self, o: &BitWriter) {
        for &b in &o.buf {
            self.write(8, u64::from(b));
        }
        self.write(o.n, o.acc);
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn u32(&mut self, v: u32, d0: U32Dist, d1: U32Dist, d2: U32Dist, d3: U32Dist) {
        for (sel, d) in [d0, d1, d2, d3].into_iter().enumerate() {
            if v < d.offset {
                continue;
            }
            let rest = u64::from(v - d.offset);
            if d.bits == 0 && rest != 0 {
                continue;
            }
            if d.bits < 32 && rest >= 1u64 << d.bits {
                continue;
            }
            self.write(2, sel as u64);
            self.write(d.bits, rest);
            return;
        }
        debug_assert!(false, "jpegxl: value not representable in U32 field");
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn u64(&mut self, mut v: u64) {
        match v {
            0 => self.write(2, 0),
            1..=16 => {
                self.write(2, 1);
                self.write(4, v - 1);
            }
            17..=272 => {
                self.write(2, 2);
                self.write(8, v - 17);
            }
            _ => {
                self.write(2, 3);
                self.write(12, v & 0xFFF);
                v >>= 12;
                let mut shift = 12;
                while v > 0 {
                    self.write(1, 1);
                    if shift == 60 {
                        self.write(4, v);
                        return;
                    }
                    self.write(8, v & 0xFF);
                    v >>= 8;
                    shift += 8;
                }
                self.write(1, 0);
            }
        }
    }
    pub(crate) fn f16(&mut self, f: f32) {
        self.write(16, u64::from(float16_bits(f)));
    }
    pub(crate) fn enum_value(&mut self, v: u32) {
        self.u32(v, val(0), val(1), bits_off(4, 2), bits_off(6, 18));
    }
}
#[allow(clippy::arithmetic_side_effects)]
fn float16_bits(f: f32) -> u16 {
    let b = f.to_bits();
    let sign = ((b >> 31) as u16) << 15;
    let exp = ((b >> 23) & 0xFF) as i32 - 127;
    let mant = b & 0x7F_FFFF;
    if f == 0.0 || exp < -24 {
        return sign;
    }
    if exp < -14 {
        let m = (mant | 0x80_0000) >> (-exp - 14 + 13);
        return sign | m as u16;
    }
    if exp > 15 {
        return sign | 0x7BFF;
    }
    sign | ((exp + 15) as u16) << 10 | (mant >> 13) as u16
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn pack_signed(v: i32) -> u32 {
    if v < 0 {
        ((-(i64::from(v) + 1)) as u32)
            .wrapping_mul(2)
            .wrapping_add(1)
    } else {
        (v as u32).wrapping_mul(2)
    }
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn floor_log2_u32(v: u32) -> u32 {
    (32 - v.leading_zeros()).wrapping_sub(1)
}
