#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::Error;
use super::bitstream::{BitReader, BitWriter, FillError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
pub(crate) const MIN_C: i64 = -128;
pub(crate) const MAX_C: i64 = 127;
pub(crate) const BASIC_T1: i64 = 3;
pub(crate) const BASIC_T2: i64 = 7;
pub(crate) const BASIC_T3: i64 = 21;
pub(crate) const BASIC_RESET: i64 = 64;
pub(crate) const RUN_J: [u32; 32] = [
    0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 9, 10, 11, 12, 13,
    14, 15,
];
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct Preset {
    pub(crate) near: i64,
    pub(crate) maxval: i64,
    pub(crate) t1: i64,
    pub(crate) t2: i64,
    pub(crate) t3: i64,
    pub(crate) reset: i64,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct ContextState {
    pub(crate) a: i64,
    pub(crate) b: i64,
    pub(crate) c: i64,
    pub(crate) n: i64,
    pub(crate) k: i64,
}
pub(crate) struct LocoState {
    pub(crate) maxval: i64,
    pub(crate) range: i64,
    pub(crate) qbpp: i64,
    pub(crate) limit: i64,
    pub(crate) reset: i64,
    pub(crate) near: i64,
    pub(crate) ctx: [ContextState; 367],
    pub(crate) nn: [i64; 2],
    pub(crate) run_idx: usize,
    qtab: Arc<[i8]>,
}
const MAX_QUANTIZE_TABLES: usize = 32;
const QUANTIZE_TABLE_OFFSET: i64 = (1 << 16) - 1;
fn quantize_tables() -> &'static Mutex<HashMap<Preset, Arc<[i8]>>> {
    static TABLES: OnceLock<Mutex<HashMap<Preset, Arc<[i8]>>>> = OnceLock::new();
    TABLES.get_or_init(|| Mutex::new(HashMap::new()))
}
#[allow(clippy::arithmetic_side_effects)]
fn quantize_slow(d: i64, p: &Preset) -> i64 {
    if d <= -p.t3 {
        -4
    } else if d <= -p.t2 {
        -3
    } else if d <= -p.t1 {
        -2
    } else if d < -p.near {
        -1
    } else if d <= p.near {
        0
    } else if d < p.t1 {
        1
    } else if d < p.t2 {
        2
    } else if d < p.t3 {
        3
    } else {
        4
    }
}
#[allow(clippy::arithmetic_side_effects)]
fn quantize_table(p: &Preset) -> Arc<[i8]> {
    let key = Preset {
        near: p.near,
        t1: p.t1,
        t2: p.t2,
        t3: p.t3,
        ..Default::default()
    };
    if let Some(t) = quantize_tables()
        .lock()
        .ok()
        .and_then(|m| m.get(&key).cloned())
    {
        return t;
    }
    let table: Arc<[i8]> = (0..2 * QUANTIZE_TABLE_OFFSET + 1)
        .map(|i| quantize_slow(i - QUANTIZE_TABLE_OFFSET, &key) as i8)
        .collect();
    if let Ok(mut m) = quantize_tables().lock()
        && m.len() < MAX_QUANTIZE_TABLES
    {
        m.insert(key, table.clone());
    }
    table
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn ceil_log2(v: i64) -> i64 {
    if v <= 1 {
        return 0;
    }
    let mut n = 0;
    let mut x = v - 1;
    while x > 0 {
        x >>= 1;
        n += 1;
    }
    n
}
#[allow(clippy::arithmetic_side_effects)]
fn bit_len(v: i64) -> i64 {
    64 - (v as u64).leading_zeros() as i64
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn golomb_k(a: i64, n: i64) -> i64 {
    let mut k = bit_len(a) - bit_len(n);
    k &= !(k >> 63);
    k - ((n << k).wrapping_sub(a) >> 63)
}
impl LocoState {
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn new(p: &Preset) -> Box<LocoState> {
        let range = (p.maxval + 2 * p.near) / (2 * p.near + 1) + 1;
        let qbpp = ceil_log2(range);
        let bpp = 2.max(ceil_log2(p.maxval + 1));
        let limit = 2 * (bpp + 8.max(bpp));
        let init_a = 2.max((range + 32) / 64);
        let init = ContextState {
            a: init_a,
            n: 1,
            k: golomb_k(init_a, 1),
            ..Default::default()
        };
        Box::new(LocoState {
            maxval: p.maxval,
            range,
            qbpp,
            limit,
            reset: p.reset,
            near: p.near,
            ctx: [init; 367],
            nn: [0; 2],
            run_idx: 0,
            qtab: quantize_table(p),
        })
    }
    #[inline]
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn quantize(&self, d: i64) -> i64 {
        self.qtab[(d + QUANTIZE_TABLE_OFFSET) as usize] as i64
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn mod_range(&self, mut err: i64) -> i64 {
        err += self.range & (err >> 63);
        err -= self.range & (((self.range + 1) / 2 - 1 - err) >> 63);
        err
    }
    #[allow(clippy::indexing_slicing)]
    fn special_mapping(&self, q: usize, k: i64) -> bool {
        self.near == 0 && k == 0 && self.ctx[q].b.wrapping_mul(2) <= self.ctx[q].n.wrapping_neg()
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn map_regular(&self, err: i64, q: usize, k: i64) -> i64 {
        let mut mapped = (err << 1) ^ (err >> 63);
        if self.special_mapping(q, k) {
            mapped += 1 + 2 * (err >> 63);
        }
        mapped
    }
    pub(crate) fn unmap_regular(&self, mapped: i64, q: usize, k: i64) -> i64 {
        let mut flip = mapped & 1;
        if self.special_mapping(q, k) {
            flip ^= 1;
        }
        (mapped >> 1) ^ flip.wrapping_neg()
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn update_regular(&mut self, q: usize, err: i64) {
        let near = self.near;
        let reset = self.reset;
        let cx = &mut self.ctx[q];
        cx.b = cx.b.wrapping_add(err.wrapping_mul(2 * near + 1));
        cx.a = cx.a.wrapping_add(err.wrapping_abs());
        if cx.n == reset {
            cx.a >>= 1;
            if cx.b >= 0 {
                cx.b >>= 1;
            } else {
                cx.b = 1i64.wrapping_sub(cx.b).wrapping_shr(1).wrapping_neg();
            }
            cx.n >>= 1;
        }
        cx.n = cx.n.saturating_add(1);
        let (b, n, c) = (cx.b, cx.n, cx.c);
        if b.wrapping_add(n).wrapping_sub(1) < 0 {
            cx.b = b.wrapping_add(n).max(1i64.saturating_sub(n));
            cx.c = c.saturating_sub(1).max(MIN_C);
        } else if b > 0 {
            cx.b = b.wrapping_sub(n).min(0);
            cx.c = c.saturating_add(1).min(MAX_C);
        }
        while cx.k < 62 && cx.n << cx.k < cx.a {
            cx.k = cx.k.saturating_add(1);
        }
        while cx.k > 0 && cx.n << cx.k.saturating_sub(1) >= cx.a {
            cx.k = cx.k.saturating_sub(1);
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn reconstruct(&self, prediction: i64, err: i64) -> i64 {
        let step = 2 * self.near + 1;
        let mut rx = prediction.wrapping_add(err.wrapping_mul(step));
        if rx < -self.near {
            rx = rx.wrapping_add(self.range * step);
        }
        if rx > self.maxval + self.near {
            rx = rx.wrapping_sub(self.range * step);
        }
        rx.clamp(0, self.maxval)
    }
}
#[inline]
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn context_index(q1: i64, q2: i64, q3: i64) -> (usize, i64) {
    let q = 81 * q1 + 9 * q2 + q3;
    let negative = q >> 63;
    (((q ^ negative) - negative) as usize, negative | 1)
}
#[inline]
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn predict(ra: i64, rb: i64, rc: i64) -> i64 {
    (ra + rb - rc).max(ra.min(rb)).min(ra.max(rb))
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn default_near_thresholds(maxval: i64, near: i64) -> (i64, i64, i64) {
    let threshold = |value: i64, lower: i64| {
        if value < lower || value > maxval {
            lower
        } else {
            value
        }
    };
    if maxval >= 128 {
        let factor = (maxval.min(4095) + 128) / 256;
        let t1 = threshold(factor * (BASIC_T1 - 2) + 2 + 3 * near, near + 1);
        let t2 = threshold(factor * (BASIC_T2 - 3) + 3 + 5 * near, t1);
        let t3 = threshold(factor * (BASIC_T3 - 4) + 4 + 7 * near, t2);
        return (t1, t2, t3);
    }
    let factor = 256 / (maxval + 1);
    let t1 = threshold(2.max(BASIC_T1 / factor + 3 * near), near + 1);
    let t2 = threshold(3.max(BASIC_T2 / factor + 5 * near), t1);
    let t3 = threshold(4.max(BASIC_T3 / factor + 7 * near), t2);
    (t1, t2, t3)
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn write_limited_golomb(w: &mut BitWriter, mapped: i64, k: i64, limit: i64, qbpp: i64) {
    let high = mapped >> k;
    let max_high = limit - qbpp - 1;
    if high < max_high {
        let n = high + 1 + k;
        if n <= 32 {
            let k = k as u32;
            let low = (mapped as u32) & 1u32.wrapping_shl(k).wrapping_sub(1);
            w.write_bits(1u32.wrapping_shl(k) | low, n as u32);
            return;
        }
        w.write_zeros(high as u32);
        w.write_bit(1);
        if k > 0 {
            w.write_bits((mapped as u32) & ((1u64 << k) - 1) as u32, k as u32);
        }
        return;
    }
    w.write_zeros(max_high as u32);
    w.write_bit(1);
    w.write_bits((mapped - 1) as u32, qbpp as u32);
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn read_limited_golomb(
    r: &mut BitReader,
    k: i64,
    limit: i64,
    qbpp: i64,
) -> Result<i64, Error> {
    let max_high = limit - qbpp - 1;
    if r.count < 32 {
        let _ = r.fill();
    }
    let run = r.leading_zeros() as i64;
    if run < max_high && run + 1 + k <= r.count as i64 {
        r.count -= (run + 1) as u32;
        let rem = (r.val >> (r.count as i64 - k)) & ((1u64 << k) - 1);
        r.consume(k as u32);
        return Ok((run << k) | rem as i64);
    }
    let mut zeros = 0i64;
    loop {
        if r.count == 0 {
            let res = r.fill();
            if r.count == 0 {
                return Err(res.err().unwrap_or(FillError::Eof).into());
            }
        }
        let run = r.leading_zeros() as i64;
        if zeros + run >= max_high {
            r.consume((max_high - zeros) as u32);
            break;
        }
        if run < r.count as i64 {
            r.consume((run + 1) as u32);
            let high = zeros + run;
            if k == 0 {
                return Ok(high);
            }
            let rem = r.read_bits(k as u32)?;
            return Ok((high << k) | rem as i64);
        }
        zeros += run;
        r.consume(run as u32);
    }
    let bit = r.read_bit()?;
    if bit != 1 {
        return Err(Error::invalid("limited Golomb terminator"));
    }
    let val = r.read_bits(qbpp as u32)?;
    Ok(val as i64 + 1)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn neighbors(recon: &[i64], width: usize, x: usize, y: usize) -> (i64, i64, i64, i64) {
    let (mut rb, mut rc, mut rd) = (0, 0, 0);
    if y > 0 {
        rb = recon[(y - 1) * width + x];
        rd = if x + 1 < width {
            recon[(y - 1) * width + x + 1]
        } else {
            rb
        };
    }
    if x > 0 {
        let ra = recon[y * width + x - 1];
        if y > 0 {
            rc = recon[(y - 1) * width + x - 1];
        }
        return (ra, rb, rc, rd);
    }
    if y > 1 {
        rc = recon[(y - 2) * width];
    }
    (rb, rb, rc, rd)
}
