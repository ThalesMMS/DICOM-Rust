#[rustfmt::skip]
mod tables;
use tables::{FN, KN, RNG_COOKED, WN};
const RNG_LEN: usize = 607;
const RNG_TAP: usize = 273;
const INT32_MAX: i64 = (1 << 31) - 1;
const RNG_MASK: u64 = (1 << 63) - 1;
const RN: f64 = 3.442619855899;
const INV_RN: f64 = 0.29047645161474317;
#[derive(Clone)]
pub struct Rand {
    tap: usize,
    feed: usize,
    vec: Box<[i64; RNG_LEN]>,
}
impl std::fmt::Debug for Rand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rand")
            .field("tap", &self.tap)
            .field("feed", &self.feed)
            .finish_non_exhaustive()
    }
}
fn seedrand(x: i32) -> i32 {
    const A: i32 = 48271;
    const Q: i32 = 44488;
    const R: i32 = 3399;
    let hi = x / Q;
    let lo = x % Q;
    let mut x = A.wrapping_mul(lo).wrapping_sub(R.wrapping_mul(hi));
    if x < 0 {
        x = x.wrapping_add(INT32_MAX as i32);
    }
    x
}
impl Rand {
    pub fn new(seed: i64) -> Self {
        let mut seed = seed % INT32_MAX;
        if seed < 0 {
            seed += INT32_MAX;
        }
        if seed == 0 {
            seed = 89482311;
        }
        let mut vec = Box::new([0i64; RNG_LEN]);
        let mut x = seed as i32;
        for i in -20..RNG_LEN as i32 {
            x = seedrand(x);
            if i >= 0 {
                let mut u = i64::from(x) << 40;
                x = seedrand(x);
                u ^= i64::from(x) << 20;
                x = seedrand(x);
                u ^= i64::from(x);
                u ^= RNG_COOKED[i as usize];
                vec[i as usize] = u;
            }
        }
        Rand {
            tap: 0,
            feed: RNG_LEN - RNG_TAP,
            vec,
        }
    }
    fn uint64(&mut self) -> u64 {
        self.tap = if self.tap == 0 {
            RNG_LEN - 1
        } else {
            self.tap - 1
        };
        self.feed = if self.feed == 0 {
            RNG_LEN - 1
        } else {
            self.feed - 1
        };
        let x = self.vec[self.feed].wrapping_add(self.vec[self.tap]);
        self.vec[self.feed] = x;
        x as u64
    }
    pub fn int63(&mut self) -> i64 {
        (self.uint64() & RNG_MASK) as i64
    }
    pub fn uint32(&mut self) -> u32 {
        (self.int63() >> 31) as u32
    }
    pub fn int31(&mut self) -> i32 {
        (self.int63() >> 32) as i32
    }
    pub fn int63n(&mut self, n: i64) -> Option<i64> {
        if n <= 0 {
            return None;
        }
        if n & (n - 1) == 0 {
            return Some(self.int63() & (n - 1));
        }
        let max = ((1u64 << 63) - 1 - (1u64 << 63) % n as u64) as i64;
        let mut v = self.int63();
        while v > max {
            v = self.int63();
        }
        Some(v % n)
    }
    pub fn int31n(&mut self, n: i32) -> Option<i32> {
        if n <= 0 {
            return None;
        }
        if n & (n - 1) == 0 {
            return Some(self.int31() & (n - 1));
        }
        let max = ((1u32 << 31) - 1 - (1u32 << 31) % n as u32) as i32;
        let mut v = self.int31();
        while v > max {
            v = self.int31();
        }
        Some(v % n)
    }
    pub fn intn(&mut self, n: i64) -> Option<i64> {
        if n <= 0 {
            return None;
        }
        if n <= i64::from(i32::MAX) {
            return self.int31n(n as i32).map(i64::from);
        }
        self.int63n(n)
    }
    pub fn float64(&mut self) -> f64 {
        loop {
            let f = self.int63() as f64 / 9_223_372_036_854_775_808.0;
            if f != 1.0 {
                return f;
            }
        }
    }
    pub fn norm_float64(&mut self) -> f64 {
        loop {
            let j = self.uint32() as i32;
            let i = (j & 0x7F) as usize;
            let mut x = f64::from(j) * f64::from(WN[i]);
            if (j.wrapping_abs() as u32) < KN[i] {
                return x;
            }
            if i == 0 {
                loop {
                    x = -super::log(self.float64()) * INV_RN;
                    let y = -super::log(self.float64());
                    if y + y >= x * x {
                        break;
                    }
                }
                if j > 0 {
                    return RN + x;
                }
                return -RN - x;
            }
            if FN[i] + (self.float64() as f32) * (FN[i - 1] - FN[i])
                < super::exp(-0.5 * x * x) as f32
            {
                return x;
            }
        }
    }
}
