use super::codestream::{
    BLOCK_HT, BLOCK_RESET, BLOCK_SEG_SYM, BLOCK_VSC, QUANT_DERIVED, Quantization,
};
use super::geometry::{BAND_HH, BAND_HL, Band};
use super::mq::{Contexts, Mq, SEGMENT_SENTINEL};
use super::packet::{CodeBlockData, raw_pass};
use super::{Result, errorf};
use crate::codec::jpeg2000::htj2k;
use std::sync::OnceLock;
#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;
const CX_ZERO: usize = 0;
const CX_SIGN_FIRST: usize = 9;
const CX_REFINE_NONE: usize = 14;
const CX_REFINE_NEAR: usize = 15;
const CX_REFINE_MORE: usize = 16;
const CX_RUN_LEN: usize = 17;
const CX_UNIFORM: usize = 18;
const _: () = assert!(CX_REFINE_NEAR == CX_REFINE_NONE + 1 && CX_REFINE_MORE == CX_REFINE_NONE + 2);
const SIGMA_THIS: u32 = 1 << 4;
const SIGMA_NEIGHBORS: u32 = 0x1EF;
const SIGMA_WINDOW: u32 = 0x3FFFF;
const CHI_ABOVE_BIT: u32 = 18;
const CHI_THIS_BIT: u32 = 19;
const CHI_SOUTH_BIT: u32 = 22;
const MU_THIS: u32 = 1 << 20;
const PI_THIS: u32 = 1 << 21;
const ROWS: u32 = 0x249;
const PI_ROWS: u32 = PI_THIS * ROWS;
#[allow(clippy::arithmetic_side_effects)]
fn rows_mask(count: usize) -> u32 {
    ROWS >> (3 * (4 - count))
}
#[allow(clippy::arithmetic_side_effects)]
fn below(shift: u32) -> u32 {
    !((2 << shift) - 1)
}
#[inline(always)]
fn significance_candidates(word: u32) -> u32 {
    let sigma = word & SIGMA_WINDOW;
    let any = sigma | sigma >> 1 | sigma >> 2;
    let sides = sigma | sigma >> 2;
    let near = any | sides >> 3 | any >> 6;
    near & !(sigma >> 4) & ROWS
}
#[inline(always)]
fn refinement_candidates(word: u32) -> u32 {
    (word >> 4) & !(word >> 21) & ROWS
}
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
fn zero_coding_tables() -> &'static [[u8; 512]; 3] {
    static TABLES: OnceLock<[[u8; 512]; 3]> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut tables = [[0u8; 512]; 3];
        for (orientation, table) in tables.iter_mut().enumerate() {
            for (mask, entry) in table.iter_mut().enumerate() {
                let bits = |positions: &[usize]| {
                    positions.iter().filter(|&&p| mask & (1 << p) != 0).count() as i32
                };
                let h = bits(&[3, 5]);
                let v = bits(&[1, 7]);
                let d = bits(&[0, 2, 6, 8]);
                *entry = zero_coding_context(orientation, h, v, d) as u8;
            }
        }
        tables
    })
}
#[allow(clippy::arithmetic_side_effects)]
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
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
static SIGN_CODING: [u8; 256] = {
    const fn contribution(index: usize, significant: usize, negative: usize) -> i32 {
        if index & (1 << significant) == 0 {
            0
        } else if index & (1 << negative) != 0 {
            -1
        } else {
            1
        }
    }
    const fn clamp(v: i32) -> i32 {
        if v > 1 {
            1
        } else if v < -1 {
            -1
        } else {
            v
        }
    }
    let mut table = [0u8; 256];
    let mut index = 0;
    while index < 256 {
        let h = clamp(contribution(index, 3, 0) + contribution(index, 5, 4));
        let v = clamp(contribution(index, 1, 2) + contribution(index, 7, 6));
        let (cx, flip): (usize, u8) = match (h, v) {
            (0, 0) => (0, 0),
            (0, 1) => (1, 0),
            (0, _) => (1, 1),
            (1, 0) => (3, 0),
            (1, 1) => (4, 0),
            (1, _) => (2, 0),
            (_, 0) => (3, 1),
            (_, 1) => (2, 1),
            _ => (4, 1),
        };
        table[index] = (CX_SIGN_FIRST + cx) as u8 | flip << 7;
        index += 1;
    }
    table
};
pub(super) fn subband_index(resolution: i64, band_type: i64) -> i64 {
    if resolution == 0 {
        return 0;
    }
    resolution
        .saturating_sub(1)
        .saturating_mul(3)
        .saturating_add(band_type)
}
pub(super) fn magnitude_bits(
    q: &Quantization,
    band: &Band,
    resolution: i64,
    levels: i64,
) -> Result<i64> {
    let exponent = if q.style == QUANT_DERIVED {
        let Some(&first) = q.exponents.first() else {
            return Err(errorf("a derived quantization with no exponent"));
        };
        first.saturating_add(band.level).saturating_sub(levels)
    } else {
        let index = subband_index(resolution, band.kind);
        let Some(&e) = usize::try_from(index).ok().and_then(|i| q.exponents.get(i)) else {
            return Err(errorf(format_args!(
                "subband {index} has no quantization exponent, of {} given",
                q.exponents.len()
            )));
        };
        e
    };
    let bits = q.guard_bits.saturating_add(exponent).saturating_sub(1);
    if !(0..=31).contains(&bits) {
        return Err(errorf(format_args!(
            "a subband claiming {bits} magnitude bits"
        )));
    }
    Ok(bits)
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
    #[allow(clippy::arithmetic_side_effects)]
    fn bit(&mut self) -> u32 {
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
        (self.c >> self.ct) & 1
    }
}
#[inline(always)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_sign(
    mq: &mut Mq,
    contexts: &mut Contexts,
    segment: &[u8],
    flags: &[u32],
    i: usize,
    word: u32,
    shift: u32,
) -> u32 {
    let own = word >> shift;
    let west = flags[i - 1] >> (CHI_THIS_BIT + shift);
    let east = flags[i + 1] >> (CHI_THIS_BIT + shift);
    let north = if shift == 0 {
        own >> CHI_ABOVE_BIT
    } else {
        own >> (CHI_THIS_BIT - 3)
    };
    let index = (own & 0xAA)
        | (west & 1)
        | (north & 1) << 2
        | (east & 1) << 4
        | ((own >> CHI_SOUTH_BIT) & 1) << 6;
    let entry = SIGN_CODING[index as usize];
    mq.decode(contexts, segment, (entry & 0x7F) as usize) ^ (entry >> 7) as u32
}
#[inline(always)]
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn set_significant(
    flags: &mut [u32],
    i: usize,
    word: &mut u32,
    shift: u32,
    sign: u32,
    stride: usize,
    vsc: bool,
) {
    flags[i - 1] |= (1 << 5) << shift;
    *word |= (sign << CHI_THIS_BIT | SIGMA_THIS) << shift;
    flags[i + 1] |= (1 << 3) << shift;
    if shift == 0 && !vsc {
        let north = i - stride;
        flags[north - 1] |= 1 << 17;
        flags[north] |= sign << 31 | 1 << 16;
        flags[north + 1] |= 1 << 15;
    }
    if shift == 9 {
        let south = i + stride;
        flags[south - 1] |= 1 << 2;
        flags[south] |= sign << CHI_ABOVE_BIT | 1 << 1;
        flags[south + 1] |= 1;
    }
}
struct BlockDecoder {
    w: usize,
    h: usize,
    stride: usize,
    flags: Vec<u32>,
    data: Vec<i32>,
    mq: Mq,
    contexts: Contexts,
    segment: Vec<u8>,
    raw: RawDecoder,
    zc: &'static [u8; 512],
    vsc: bool,
}
impl BlockDecoder {
    fn new(w: usize, h: usize, band_type: i64) -> BlockDecoder {
        let stride = w.saturating_add(2);
        let mut d = BlockDecoder {
            w,
            h,
            stride,
            flags: vec![0; stride.saturating_mul(h.div_ceil(4).saturating_add(2))],
            data: vec![0; w.saturating_mul(h)],
            mq: Mq::new(),
            contexts: Contexts::new(),
            segment: SEGMENT_SENTINEL.to_vec(),
            raw: RawDecoder::default(),
            zc: {
                let [ll_lh, hl, hh] = zero_coding_tables();
                match orientation_of(band_type) {
                    ORIENT_HL => hl,
                    ORIENT_HH => hh,
                    _ => ll_lh,
                }
            },
            vsc: false,
        };
        d.reset_contexts();
        d
    }
    fn reset_contexts(&mut self) {
        self.contexts = Contexts::new();
        self.contexts.set(CX_UNIFORM, 46, 0);
        self.contexts.set(CX_RUN_LEN, 3, 0);
        self.contexts.set(CX_ZERO, 4, 0);
    }
    fn start_mq(&mut self, data: &[u8]) {
        self.segment.clear();
        self.segment.extend_from_slice(data);
        self.segment.extend_from_slice(&SEGMENT_SENTINEL);
        self.mq.start(&self.segment);
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn significance_pass(&mut self, plane: u32) {
        let (w, h, stride, vsc, zc) = (self.w, self.h, self.stride, self.vsc, self.zc);
        let mut mq = self.mq;
        let mut contexts = self.contexts;
        let segment = self.segment.as_slice();
        let flags = self.flags.as_mut_slice();
        let data = self.data.as_mut_slice();
        let value = 3i32 << plane;
        let mut y0 = 0;
        let mut first = stride + 1;
        while y0 < h {
            let in_block = rows_mask((h - y0).min(4));
            for x in 0..w {
                let i = first + x;
                let mut word = flags[i];
                if word & SIGMA_WINDOW == 0 {
                    continue;
                }
                let mut pending = significance_candidates(word) & in_block;
                if pending == 0 {
                    continue;
                }
                while pending != 0 {
                    let shift = pending.trailing_zeros();
                    pending &= pending - 1;
                    let cx = zc[((word >> shift) & SIGMA_NEIGHBORS) as usize] as usize;
                    if mq.decode(&mut contexts, segment, cx) != 0 {
                        let sign =
                            decode_sign(&mut mq, &mut contexts, segment, flags, i, word, shift);
                        set_significant(flags, i, &mut word, shift, sign, stride, vsc);
                        data[(y0 + (shift / 3) as usize) * w + x] = value;
                        pending = significance_candidates(word) & in_block & below(shift);
                    }
                    word |= PI_THIS << shift;
                }
                flags[i] = word;
            }
            y0 += 4;
            first += stride;
        }
        self.mq = mq;
        self.contexts = contexts;
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn raw_significance_pass(&mut self, plane: u32) {
        let (w, h, stride, vsc) = (self.w, self.h, self.stride, self.vsc);
        let flags = self.flags.as_mut_slice();
        let data = self.data.as_mut_slice();
        let raw = &mut self.raw;
        let value = 3i32 << plane;
        let mut y0 = 0;
        let mut first = stride + 1;
        while y0 < h {
            let in_block = rows_mask((h - y0).min(4));
            for x in 0..w {
                let i = first + x;
                if flags[i] & SIGMA_WINDOW == 0 {
                    continue;
                }
                let mut word = flags[i];
                let mut pending = significance_candidates(word) & in_block;
                if pending == 0 {
                    continue;
                }
                while pending != 0 {
                    let shift = pending.trailing_zeros();
                    pending &= pending - 1;
                    if raw.bit() == 1 {
                        let sign = raw.bit();
                        set_significant(flags, i, &mut word, shift, sign, stride, vsc);
                        data[(y0 + (shift / 3) as usize) * w + x] = value;
                        pending = significance_candidates(word) & in_block & below(shift);
                    }
                    word |= PI_THIS << shift;
                }
                flags[i] = word;
            }
            y0 += 4;
            first += stride;
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn raw_refinement_pass(&mut self, plane: u32) {
        let (w, h, stride) = (self.w, self.h, self.stride);
        let flags = self.flags.as_mut_slice();
        let data = self.data.as_mut_slice();
        let raw = &mut self.raw;
        let half = 1i32 << plane;
        let mut y0 = 0;
        let mut first = stride + 1;
        while y0 < h {
            for x in 0..w {
                let i = first + x;
                let mut word = flags[i];
                let mut pending = refinement_candidates(word);
                if pending == 0 {
                    continue;
                }
                while pending != 0 {
                    let shift = pending.trailing_zeros();
                    pending &= pending - 1;
                    let k = (y0 + (shift / 3) as usize) * w + x;
                    data[k] = if raw.bit() == 1 {
                        data[k].wrapping_add(half)
                    } else {
                        data[k].wrapping_sub(half)
                    };
                    word |= MU_THIS << shift;
                }
                flags[i] = word;
            }
            y0 += 4;
            first += stride;
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn refinement_pass(&mut self, plane: u32) {
        let (w, h, stride) = (self.w, self.h, self.stride);
        let mut mq = self.mq;
        let mut contexts = self.contexts;
        let segment = self.segment.as_slice();
        let flags = self.flags.as_mut_slice();
        let data = self.data.as_mut_slice();
        let half = 1i32 << plane;
        let mut y0 = 0;
        let mut first = stride + 1;
        while y0 < h {
            for x in 0..w {
                let i = first + x;
                let mut word = flags[i];
                let mut pending = refinement_candidates(word);
                if pending == 0 {
                    continue;
                }
                while pending != 0 {
                    let shift = pending.trailing_zeros();
                    pending &= pending - 1;
                    let f = word >> shift;
                    let near = (f & SIGMA_NEIGHBORS != 0) as usize;
                    let more = ((f / MU_THIS) & 1) as usize;
                    let cx = CX_REFINE_NONE + near + more * (2 - near);
                    let bit = mq.decode_even(&mut contexts, segment, cx) as i32;
                    let k = (y0 + (shift / 3) as usize) * w + x;
                    data[k] = data[k]
                        .wrapping_sub(half)
                        .wrapping_add(bit.wrapping_mul(half).wrapping_mul(2));
                    word |= MU_THIS << shift;
                }
                flags[i] = word;
            }
            y0 += 4;
            first += stride;
        }
        self.mq = mq;
        self.contexts = contexts;
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn cleanup_pass(&mut self, plane: u32) {
        let (w, h, stride, vsc, zc) = (self.w, self.h, self.stride, self.vsc, self.zc);
        let mut mq = self.mq;
        let mut contexts = self.contexts;
        let segment = self.segment.as_slice();
        let flags = self.flags.as_mut_slice();
        let data = self.data.as_mut_slice();
        let value = 3i32 << plane;
        let mut y0 = 0;
        let mut first = stride + 1;
        while y0 < h {
            let rows = (h - y0).min(4);
            let in_block = rows_mask(rows);
            for x in 0..w {
                let i = first + x;
                let mut word = flags[i];
                let mut pending;
                if rows == 4 && word & (SIGMA_WINDOW | PI_ROWS) == 0 {
                    if mq.decode(&mut contexts, segment, CX_RUN_LEN) == 0 {
                        continue;
                    }
                    let hi = mq.decode(&mut contexts, segment, CX_UNIFORM);
                    let lo = mq.decode(&mut contexts, segment, CX_UNIFORM);
                    let row = (hi << 1 | lo) as usize;
                    let shift = 3 * row as u32;
                    let sign = decode_sign(&mut mq, &mut contexts, segment, flags, i, word, shift);
                    set_significant(flags, i, &mut word, shift, sign, stride, vsc);
                    data[(y0 + row) * w + x] = value;
                    pending = in_block & below(shift);
                } else {
                    pending = !(word >> 4 | word >> 21) & in_block;
                }
                while pending != 0 {
                    let shift = pending.trailing_zeros();
                    pending &= pending - 1;
                    let cx = zc[((word >> shift) & SIGMA_NEIGHBORS) as usize] as usize;
                    if mq.decode(&mut contexts, segment, cx) != 0 {
                        let sign =
                            decode_sign(&mut mq, &mut contexts, segment, flags, i, word, shift);
                        set_significant(flags, i, &mut word, shift, sign, stride, vsc);
                        data[(y0 + (shift / 3) as usize) * w + x] = value;
                    }
                }
                flags[i] = word & !PI_ROWS;
            }
            y0 += 4;
            first += stride;
        }
        self.mq = mq;
        self.contexts = contexts;
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn apply_signs(&mut self) {
        let (w, h, stride) = (self.w, self.h, self.stride);
        let mut y0 = 0;
        let mut first = stride + 1;
        while y0 < h {
            for x in 0..w {
                let mut negative = (self.flags[first + x] >> CHI_THIS_BIT) & ROWS;
                while negative != 0 {
                    let row = (negative.trailing_zeros() / 3) as usize;
                    negative &= negative - 1;
                    let k = (y0 + row) * w + x;
                    self.data[k] = self.data[k].wrapping_neg();
                }
            }
            y0 += 4;
            first += stride;
        }
    }
    fn decode_ht(&mut self, block: &CodeBlockData, plane: i64, magnitude_bits: i64) -> Result<()> {
        let Some(set) = last_ht_set(block) else {
            return Err(errorf("an HT code-block without a cleanup pass"));
        };
        let top = plane;
        let plane = top.saturating_sub(set.index);
        if plane < 0 || (plane == 0 && set.refinement_passes > 0) {
            return Err(errorf(format_args!(
                "an HT code-block claims an HT set below bit-plane zero ({} sets from bit-plane {top})",
                set.index.saturating_add(1),
            )));
        }
        let bound = magnitude_bits
            .saturating_sub(plane)
            .saturating_add(1)
            .min(31);
        if let Err(e) = htj2k::decode_cleanup(
            set.cleanup,
            self.w as i64,
            self.h as i64,
            &mut self.data,
            bound,
        ) {
            return Err(errorf(format_args!("HT code-block: {e}")));
        }
        if set.refinement_passes > 0 {
            if let Err(e) = htj2k::refine(
                set.refinement,
                self.w,
                self.h,
                &mut self.data,
                set.refinement_passes,
                self.vsc,
                plane as u32,
            ) {
                return Err(errorf(format_args!("HT code-block: {e}")));
            }
            return Ok(());
        }
        to_halves(&mut self.data, plane)
    }
}
#[allow(clippy::arithmetic_side_effects)]
fn to_halves(data: &mut [i32], plane: i64) -> Result<()> {
    let limit = 1i64 << 30;
    for v in data.iter_mut() {
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
struct HtSet<'a> {
    index: i64,
    cleanup: &'a [u8],
    refinement: &'a [u8],
    refinement_passes: u32,
}
fn last_ht_set(block: &CodeBlockData) -> Option<HtSet<'_>> {
    let mut set: Option<HtSet<'_>> = None;
    let mut pass = 0i64;
    for segment in &block.segments {
        let first = pass;
        pass = pass.saturating_add(segment.passes);
        if segment.placeholder {
            continue;
        }
        let index = first / 3;
        if first % 3 == 0 {
            if segment.data.is_empty() && set.is_some() {
                continue;
            }
            set = Some(HtSet {
                index,
                cleanup: &segment.data,
                refinement: &[],
                refinement_passes: 0,
            });
        } else if first % 3 == 1
            && let Some(s) = set.as_mut()
            && s.index == index
        {
            s.refinement = &segment.data;
            s.refinement_passes = pass.saturating_sub(first).clamp(0, 2) as u32;
        }
    }
    set
}
pub(super) fn decode_block(block: &CodeBlockData, magnitude_bits: i64) -> Result<Vec<i32>> {
    let (w, h) = (block.block.width(), block.block.height());
    if w <= 0 || h <= 0 {
        return Err(errorf(format_args!("a code-block of {w}x{h} samples")));
    }
    if w > 1 << 12 || h > 1 << 12 || w.saturating_mul(h) > 1 << 22 {
        return Err(errorf(format_args!(
            "a code-block of {w}x{h} samples is larger than any legal one"
        )));
    }
    let mut d = BlockDecoder::new(w as usize, h as usize, block.band_type);
    d.vsc = block.style & BLOCK_VSC != 0;
    let mut plane = magnitude_bits
        .saturating_sub(1)
        .saturating_sub(block.zero_bit_planes);
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
    let planes = plane.saturating_add(1);
    let limit = planes.saturating_mul(3).saturating_sub(2);
    if block.passes > limit {
        return Err(errorf(format_args!(
            "a code-block claims {} coding passes, of the {limit} its {planes} bit planes hold",
            block.passes,
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
            segment = segment.saturating_add(1);
            let Some(seg) = usize::try_from(segment)
                .ok()
                .and_then(|s| block.segments.get(s))
            else {
                break;
            };
            left = seg.passes;
            segment_raw = raw;
            if raw {
                d.raw.start(&seg.data);
            } else {
                d.start_mq(&seg.data);
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
            PASS_SIGNIFICANCE if raw => d.raw_significance_pass(plane as u32),
            PASS_SIGNIFICANCE => d.significance_pass(plane as u32),
            PASS_REFINEMENT if raw => d.raw_refinement_pass(plane as u32),
            PASS_REFINEMENT => d.refinement_pass(plane as u32),
            _ => {
                d.cleanup_pass(plane as u32);
                if block.style & BLOCK_SEG_SYM != 0 {
                    for _ in 0..4 {
                        d.mq.decode(&mut d.contexts, &d.segment, CX_UNIFORM);
                    }
                }
            }
        }
        left = left.saturating_sub(1);
        if pass_type == PASS_CLEANUP {
            plane = plane.saturating_sub(1);
            pass_type = PASS_SIGNIFICANCE;
        } else {
            pass_type = pass_type.saturating_add(1);
        }
        pass = pass.saturating_add(1);
    }
    d.apply_signs();
    Ok(d.data)
}
