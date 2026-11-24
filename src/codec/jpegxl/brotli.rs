use super::brotli_tables::{CONTEXT_LOOKUP, TRANSFORMS};
use std::fmt;
static DICTIONARY: &[u8; 122_784] = include_bytes!("brotli_dictionary.bin");
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Invalid(String),
    TooLarge,
}
impl Error {
    pub fn is_invalid(&self) -> bool {
        matches!(self, Error::Invalid(_))
    }
    pub fn is_too_large(&self) -> bool {
        matches!(self, Error::TooLarge)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid(detail) => write!(f, "brotli: invalid stream: {detail}"),
            Error::TooLarge => f.write_str("brotli: decoded size exceeds limit"),
        }
    }
}
impl std::error::Error for Error {}
fn invalid(detail: impl Into<String>) -> Error {
    Error::Invalid(detail.into())
}
const ND_BITS: [u32; 25] = [
    0, 0, 0, 0, 10, 10, 11, 11, 10, 10, 10, 10, 10, 9, 9, 8, 7, 7, 8, 7, 7, 6, 6, 5, 5,
];
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const D_OFFSET: [usize; 25] = {
    let mut o = [0usize; 25];
    let mut i = 4;
    while i < 24 {
        o[i + 1] = o[i] + (i << ND_BITS[i]);
        i += 1;
    }
    o
};
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bits: u64,
    n: u32,
    err: Option<Error>,
}
impl BitReader<'_> {
    fn fill(&mut self, n: u32) {
        while self.n < n {
            match self.data.get(self.pos) {
                Some(&b) => {
                    self.bits |= u64::from(b) << self.n;
                    self.pos = self.pos.saturating_add(1);
                }
                None => {
                    if self.err.is_none() {
                        self.err = Some(invalid("truncated"));
                    }
                }
            }
            self.n = self.n.saturating_add(8);
        }
    }
    fn read(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.fill(n);
        let v = (self.bits & (1u64 << n).wrapping_sub(1)) as u32;
        self.bits >>= n;
        self.n = self.n.saturating_sub(n);
        v
    }
    fn peek4(&mut self) -> u32 {
        self.fill(4);
        (self.bits & 15) as u32
    }
    fn align_zero(&mut self) -> Result<(), Error> {
        let k = self.n % 8;
        if self.read(k) != 0 {
            return Err(invalid("non-zero padding"));
        }
        Ok(())
    }
    fn check(&self) -> Result<(), Error> {
        match &self.err {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}
#[derive(Default, Clone)]
struct PrefixCode {
    count: [u16; 16],
    symbols: Vec<u16>,
    single: Option<usize>,
}
impl PrefixCode {
    fn new(lengths: &[u8]) -> Result<PrefixCode, Error> {
        let mut pc = PrefixCode::default();
        let mut used = 0usize;
        let mut last = 0usize;
        for (s, &l) in lengths.iter().enumerate() {
            if l > 15 {
                return Err(invalid(format!("code length {l}")));
            }
            if l != 0 {
                if let Some(c) = pc.count.get_mut(l as usize) {
                    *c = c.saturating_add(1);
                }
                used = used.saturating_add(1);
                last = s;
            }
        }
        if used == 0 {
            return Err(invalid("empty prefix code"));
        }
        if used == 1 {
            pc.single = Some(last);
            return Ok(pc);
        }
        let mut left: i64 = 1;
        for &count in pc.count.iter().skip(1) {
            left <<= 1;
            left = left.saturating_sub(i64::from(count));
            if left < 0 {
                return Err(invalid("over-subscribed prefix code"));
            }
        }
        if left != 0 {
            return Err(invalid("incomplete prefix code"));
        }
        let mut offs = [0usize; 16];
        let mut total = 0usize;
        for (o, &count) in offs.iter_mut().zip(&pc.count).skip(1) {
            *o = total;
            total = total.saturating_add(count as usize);
        }
        pc.symbols = vec![0; used];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0
                && let Some(o) = offs.get_mut(l as usize)
            {
                if let Some(slot) = pc.symbols.get_mut(*o) {
                    *slot = s as u16;
                }
                *o = o.saturating_add(1);
            }
        }
        Ok(pc)
    }
    fn decode(&self, br: &mut BitReader<'_>) -> usize {
        if let Some(single) = self.single {
            return single;
        }
        let (mut code, mut first, mut index) = (0i64, 0i64, 0i64);
        for &count in self.count.iter().skip(1) {
            code |= i64::from(br.read(1));
            let count = i64::from(count);
            if code.saturating_sub(count) < first {
                let k = index.saturating_add(code.saturating_sub(first)) as usize;
                return self.symbols.get(k).map_or(0, |&s| usize::from(s));
            }
            index = index.saturating_add(count);
            first = first.saturating_add(count);
            first <<= 1;
            code <<= 1;
        }
        if br.err.is_none() {
            br.err = Some(invalid("bad prefix code"));
        }
        0
    }
}
const CODE_LENGTH_ORDER: [usize; 18] =
    [1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const CLCL_LENGTH: [u32; 16] = [2, 2, 2, 3, 2, 2, 2, 4, 2, 2, 2, 3, 2, 2, 2, 4];
const CLCL_VALUE: [u8; 16] = [0, 4, 3, 2, 0, 4, 3, 1, 0, 4, 3, 2, 0, 4, 3, 5];
fn read_prefix_code(br: &mut BitReader<'_>, alphabet: usize) -> Result<PrefixCode, Error> {
    let mut lengths = vec![0u8; alphabet];
    let hskip = br.read(2);
    if hskip == 1 {
        let max_bits = alphabet
            .saturating_sub(1)
            .checked_ilog2()
            .map_or(0, |b| b.saturating_add(1));
        let nsym = (br.read(2) as usize).saturating_add(1);
        let mut syms = [0usize; 4];
        for i in 0..nsym {
            let s = br.read(max_bits) as usize;
            if s >= alphabet {
                return Err(invalid("simple code symbol out of range"));
            }
            if syms.get(..i).is_some_and(|before| before.contains(&s)) {
                return Err(invalid("duplicate simple code symbol"));
            }
            if let Some(slot) = syms.get_mut(i) {
                *slot = s;
            }
        }
        let mut set = |s: usize, length: u8| {
            if let Some(l) = lengths.get_mut(s) {
                *l = length;
            }
        };
        let [s0, s1, s2, s3] = syms;
        match nsym {
            1 => set(s0, 1),
            2 => {
                set(s0, 1);
                set(s1, 1);
            }
            3 => {
                set(s0, 1);
                set(s1, 2);
                set(s2, 2);
            }
            _ => {
                if br.read(1) == 0 {
                    for s in syms {
                        set(s, 2);
                    }
                } else {
                    set(s0, 1);
                    set(s1, 2);
                    set(s2, 3);
                    set(s3, 3);
                }
            }
        }
        br.check()?;
        return PrefixCode::new(&lengths);
    }
    let mut cl_lengths = [0u8; 18];
    let mut space: i64 = 32;
    let mut num_codes = 0u32;
    for &order in CODE_LENGTH_ORDER.get(hskip as usize..).unwrap_or(&[]) {
        let p = br.peek4() as usize;
        br.read(CLCL_LENGTH.get(p).copied().unwrap_or(0));
        let v = CLCL_VALUE.get(p).copied().unwrap_or(0);
        if let Some(l) = cl_lengths.get_mut(order) {
            *l = v;
        }
        if v != 0 {
            space = space.saturating_sub(32 >> v);
            num_codes = num_codes.saturating_add(1);
            if space <= 0 {
                break;
            }
        }
    }
    if num_codes != 1 && space != 0 {
        return Err(invalid("bad code length code"));
    }
    let cl_code = PrefixCode::new(&cl_lengths)?;
    let mut symbol = 0usize;
    let mut prev_len = 8u8;
    let mut repeat: i64 = 0;
    let mut repeat_len = 0u8;
    space = 32768;
    while symbol < alphabet && space > 0 {
        br.check()?;
        let cl = cl_code.decode(br);
        if cl < 16 {
            repeat = 0;
            if let Some(l) = lengths.get_mut(symbol) {
                *l = cl as u8;
            }
            symbol = symbol.saturating_add(1);
            if cl != 0 {
                prev_len = cl as u8;
                space = space.saturating_sub(32768 >> cl);
            }
            continue;
        }
        let mut extra = 2u32;
        let mut new_len = prev_len;
        if cl == 17 {
            extra = 3;
            new_len = 0;
        }
        if repeat_len != new_len {
            repeat = 0;
            repeat_len = new_len;
        }
        let old = repeat;
        if repeat > 0 {
            repeat = repeat.saturating_sub(2);
            repeat <<= extra;
        }
        repeat = repeat
            .saturating_add(i64::from(br.read(extra)))
            .saturating_add(3);
        let delta = repeat.saturating_sub(old);
        let end = (symbol as i64).saturating_add(delta);
        let Some(run) = lengths
            .get_mut(symbol..end.max(0) as usize)
            .filter(|_| end <= alphabet as i64)
        else {
            return Err(invalid("code length repeat overflows alphabet"));
        };
        run.fill(repeat_len);
        let delta = delta as usize;
        symbol = symbol.saturating_add(delta);
        if repeat_len != 0 {
            space = space.saturating_sub((delta as i64) << 15u8.saturating_sub(repeat_len));
        }
    }
    if space != 0 {
        return Err(invalid("incomplete code lengths"));
    }
    br.check()?;
    PrefixCode::new(&lengths)
}
const BLOCK_LENGTH_BASE: [u32; 26] = [
    1, 5, 9, 13, 17, 25, 33, 41, 49, 65, 81, 97, 113, 145, 177, 209, 241, 305, 369, 497, 753, 1265,
    2289, 4337, 8433, 16625,
];
const BLOCK_LENGTH_EXTRA: [u32; 26] = [
    2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 6, 6, 7, 8, 9, 10, 11, 12, 13, 24,
];
const INSERT_BASE: [u32; 24] = [
    0, 1, 2, 3, 4, 5, 6, 8, 10, 14, 18, 26, 34, 50, 66, 98, 130, 194, 322, 578, 1090, 2114, 6210,
    22594,
];
const INSERT_EXTRA: [u32; 24] = [
    0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 7, 8, 9, 10, 12, 14, 24,
];
const COPY_BASE: [u32; 24] = [
    2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 14, 18, 22, 30, 38, 54, 70, 102, 134, 198, 326, 582, 1094, 2118,
];
const COPY_EXTRA: [u32; 24] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 7, 8, 9, 10, 24,
];
const CELL_INSERT: [usize; 11] = [0, 0, 0, 0, 8, 8, 0, 16, 8, 16, 16];
const CELL_COPY: [usize; 11] = [0, 8, 0, 8, 0, 8, 16, 0, 16, 8, 16];
fn read_var_len_uint8(br: &mut BitReader<'_>) -> usize {
    if br.read(1) == 0 {
        return 0;
    }
    let n = br.read(3);
    if n == 0 {
        return 1;
    }
    (1usize << n).saturating_add(br.read(n) as usize)
}
#[derive(Default)]
struct BlockCategory {
    num_types: usize,
    type_code: PrefixCode,
    len_code: PrefixCode,
    current: usize,
    previous: usize,
    remaining: u32,
    has_switches: bool,
}
impl BlockCategory {
    fn init(&mut self, br: &mut BitReader<'_>) -> Result<(), Error> {
        self.num_types = read_var_len_uint8(br).saturating_add(1);
        self.current = 0;
        self.previous = 1;
        self.remaining = 1 << 24;
        self.has_switches = self.num_types >= 2;
        if !self.has_switches {
            return br.check();
        }
        self.type_code = read_prefix_code(br, self.num_types.saturating_add(2))?;
        self.len_code = read_prefix_code(br, 26)?;
        self.remaining = read_block_length(br, &self.len_code);
        br.check()
    }
    fn next(&mut self, br: &mut BitReader<'_>) {
        if self.remaining == 0 && self.has_switches {
            let code = self.type_code.decode(br);
            let mut t = match code {
                0 => self.previous,
                1 => self.current.saturating_add(1),
                _ => code.saturating_sub(2),
            };
            if t >= self.num_types {
                t = t.saturating_sub(self.num_types);
            }
            self.previous = self.current;
            self.current = t;
            self.remaining = read_block_length(br, &self.len_code);
        }
        self.remaining = self.remaining.wrapping_sub(1);
    }
}
fn read_block_length(br: &mut BitReader<'_>, code: &PrefixCode) -> u32 {
    let s = code.decode(br);
    let (Some(&base), Some(&extra)) = (BLOCK_LENGTH_BASE.get(s), BLOCK_LENGTH_EXTRA.get(s)) else {
        return 0;
    };
    base.saturating_add(br.read(extra))
}
fn read_context_map(
    br: &mut BitReader<'_>,
    size: usize,
    num_trees: usize,
) -> Result<Vec<u8>, Error> {
    let mut m = vec![0u8; size];
    if num_trees < 2 {
        br.check()?;
        return Ok(m);
    }
    let mut rle_max = 0usize;
    if br.read(1) == 1 {
        rle_max = (br.read(4) as usize).saturating_add(1);
    }
    let code = read_prefix_code(br, num_trees.saturating_add(rle_max))?;
    let mut i = 0usize;
    while i < size {
        br.check()?;
        let s = code.decode(br);
        if s == 0 {
            if let Some(v) = m.get_mut(i) {
                *v = 0;
            }
            i = i.saturating_add(1);
        } else if s <= rle_max {
            let run = (1usize << s).saturating_add(br.read(s as u32) as usize);
            if i.saturating_add(run) > size {
                return Err(invalid("context map run overflows"));
            }
            i = i.saturating_add(run);
        } else {
            if let Some(v) = m.get_mut(i) {
                *v = s.saturating_sub(rle_max) as u8;
            }
            i = i.saturating_add(1);
        }
    }
    if br.read(1) == 1 {
        let mut mtf: [u8; 256] = std::array::from_fn(|i| i as u8);
        for v in m.iter_mut() {
            let idx = usize::from(*v);
            let val = mtf.get(idx).copied().unwrap_or(0);
            *v = val;
            mtf.copy_within(0..idx, 1);
            mtf[0] = val;
        }
    }
    br.check()?;
    Ok(m)
}
pub fn decode(src: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    let mut br = BitReader {
        data: src,
        pos: 0,
        bits: 0,
        n: 0,
        err: None,
    };
    let wbits: u32 = if br.read(1) == 0 {
        16
    } else {
        let n = br.read(3);
        if n != 0 {
            n.saturating_add(17)
        } else {
            let n = br.read(3);
            if n == 1 {
                return Err(invalid("large-window streams are unsupported"));
            } else if n != 0 {
                n.saturating_add(8)
            } else {
                17
            }
        }
    };
    let max_backward = (1usize << wbits).saturating_sub(16);
    let mut out = Vec::with_capacity(limit.min(1 << 16));
    let mut dist: [i64; 4] = [16, 15, 11, 4];
    loop {
        br.check()?;
        let is_last = br.read(1) == 1;
        if is_last && br.read(1) == 1 {
            break;
        }
        let nibbles = br.read(2);
        if nibbles == 3 {
            if br.read(1) != 0 {
                return Err(invalid("reserved bit set"));
            }
            let skip_bytes = br.read(2);
            let mut skip = 0usize;
            for i in 0..skip_bytes {
                let b = br.read(8) as usize;
                if i.saturating_add(1) == skip_bytes && skip_bytes > 1 && b == 0 {
                    return Err(invalid("non-minimal metadata length"));
                }
                skip |= b << i.saturating_mul(8);
            }
            if skip_bytes > 0 {
                skip = skip.saturating_add(1);
            }
            br.align_zero()?;
            if br.n != 0 || br.pos.saturating_add(skip) > src.len() {
                return Err(invalid("truncated metadata block"));
            }
            br.pos = br.pos.saturating_add(skip);
            if is_last {
                break;
            }
            continue;
        }
        let nn = nibbles.saturating_add(4);
        let mut mlen = 0usize;
        for i in 0..nn {
            let v = br.read(4) as usize;
            if i.saturating_add(1) == nn && nn > 4 && v == 0 {
                return Err(invalid("non-minimal meta-block length"));
            }
            mlen |= v << i.saturating_mul(4);
        }
        mlen = mlen.saturating_add(1);
        if out.len().saturating_add(mlen) > limit {
            return Err(Error::TooLarge);
        }
        if !is_last && br.read(1) == 1 {
            br.align_zero()?;
            let stored = br
                .pos
                .checked_add(mlen)
                .and_then(|end| src.get(br.pos..end));
            let Some(stored) = stored.filter(|_| br.n == 0) else {
                return Err(invalid("truncated uncompressed meta-block"));
            };
            out.extend_from_slice(stored);
            br.pos = br.pos.saturating_add(mlen);
            continue;
        }
        decode_compressed(&mut br, &mut out, mlen, max_backward, &mut dist)?;
        if is_last {
            break;
        }
    }
    br.align_zero()?;
    br.check()?;
    if br.pos != src.len() {
        return Err(invalid("trailing data after stream"));
    }
    Ok(out)
}
fn decode_compressed(
    br: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    mlen: usize,
    max_backward: usize,
    dist: &mut [i64; 4],
) -> Result<(), Error> {
    let mut lit = BlockCategory::default();
    let mut cmd = BlockCategory::default();
    let mut dst = BlockCategory::default();
    lit.init(br)?;
    cmd.init(br)?;
    dst.init(br)?;
    let npostfix = br.read(2);
    let ndirect = (br.read(4) as usize) << npostfix;
    let modes: Vec<usize> = (0..lit.num_types).map(|_| br.read(2) as usize).collect();
    let n_trees_l = read_var_len_uint8(br).saturating_add(1);
    let cmap_l = read_context_map(br, lit.num_types.saturating_mul(64), n_trees_l)?;
    let n_trees_d = read_var_len_uint8(br).saturating_add(1);
    let cmap_d = read_context_map(br, dst.num_types.saturating_mul(4), n_trees_d)?;
    let mut lit_codes = Vec::with_capacity(n_trees_l);
    for _ in 0..n_trees_l {
        lit_codes.push(read_prefix_code(br, 256)?);
    }
    let mut cmd_codes = Vec::with_capacity(cmd.num_types);
    for _ in 0..cmd.num_types {
        cmd_codes.push(read_prefix_code(br, 704)?);
    }
    let dist_alphabet = ndirect.saturating_add(16).saturating_add(48 << npostfix);
    let mut dist_codes = Vec::with_capacity(n_trees_d);
    for _ in 0..n_trees_d {
        dist_codes.push(read_prefix_code(br, dist_alphabet)?);
    }
    let end = out.len().saturating_add(mlen);
    let postfix_mask = (1i64 << npostfix).wrapping_sub(1);
    let ndirect_i = ndirect as i64;
    let corrupt = || invalid("code outside its table");
    while out.len() < end {
        br.check()?;
        cmd.next(br);
        let sym = cmd_codes.get(cmd.current).ok_or_else(corrupt)?.decode(br);
        let cell = sym >> 6;
        let (Some(&cell_insert), Some(&cell_copy)) = (CELL_INSERT.get(cell), CELL_COPY.get(cell))
        else {
            return Err(corrupt());
        };
        let ins_code = cell_insert.saturating_add((sym >> 3) & 7);
        let cpy_code = cell_copy.saturating_add(sym & 7);
        let (Some(&ins_base), Some(&ins_extra), Some(&cpy_base), Some(&cpy_extra)) = (
            INSERT_BASE.get(ins_code),
            INSERT_EXTRA.get(ins_code),
            COPY_BASE.get(cpy_code),
            COPY_EXTRA.get(cpy_code),
        ) else {
            return Err(corrupt());
        };
        let ins_len = ins_base.saturating_add(br.read(ins_extra)) as usize;
        let cpy_len = cpy_base.saturating_add(br.read(cpy_extra)) as usize;
        for _ in 0..ins_len {
            if out.len() >= end {
                return Err(invalid("insert overflows meta-block"));
            }
            lit.next(br);
            let p1 = out.last().map_or(0, |&b| usize::from(b));
            let p2 = out
                .len()
                .checked_sub(2)
                .and_then(|k| out.get(k))
                .map_or(0, |&b| usize::from(b));
            let mode = modes.get(lit.current).copied().unwrap_or(0);
            let lookup = |k: usize| CONTEXT_LOOKUP.get(k).copied().unwrap_or(0);
            let base = mode.saturating_mul(512);
            let ctx = lookup(base.saturating_add(p1))
                | lookup(base.saturating_add(256).saturating_add(p2));
            let slot = lit
                .current
                .saturating_mul(64)
                .saturating_add(usize::from(ctx));
            let tree = usize::from(*cmap_l.get(slot).ok_or_else(corrupt)?);
            out.push(lit_codes.get(tree).ok_or_else(corrupt)?.decode(br) as u8);
        }
        if out.len() >= end {
            break;
        }
        let distance: i64;
        let implicit = sym < 128;
        let mut push = !implicit;
        if implicit {
            distance = dist[3];
        } else {
            dst.next(br);
            let dctx = if cpy_len <= 4 {
                cpy_len.saturating_sub(2)
            } else {
                3
            };
            let slot = dst.current.saturating_mul(4).saturating_add(dctx);
            let tree = usize::from(*cmap_d.get(slot).ok_or_else(corrupt)?);
            let dcode = dist_codes.get(tree).ok_or_else(corrupt)?.decode(br) as i64;
            if dcode < 16 {
                push = dcode != 0;
                distance = special_distance(dcode, dist);
                if distance <= 0 {
                    return Err(invalid("invalid distance"));
                }
            } else if dcode < ndirect_i.saturating_add(16) {
                distance = dcode.saturating_sub(15);
            } else {
                let d = dcode.saturating_sub(ndirect_i).saturating_sub(16);
                let nbits = ((d >> npostfix.saturating_add(1)) as u32).saturating_add(1);
                let hcode = d >> npostfix;
                let lcode = d & postfix_mask;
                let offset = ((hcode & 1).saturating_add(2) << nbits).saturating_sub(4);
                distance = (offset.saturating_add(i64::from(br.read(nbits))) << npostfix)
                    .saturating_add(lcode)
                    .saturating_add(ndirect_i)
                    .saturating_add(1);
            }
        }
        let max_dist = max_backward.min(out.len()) as i64;
        if distance > max_dist {
            if !(4..=24).contains(&cpy_len) {
                return Err(invalid("dictionary word length"));
            }
            let word_id = distance.saturating_sub(max_dist).saturating_sub(1) as usize;
            let (Some(&nb), Some(&offset)) = (ND_BITS.get(cpy_len), D_OFFSET.get(cpy_len)) else {
                return Err(invalid("dictionary word length"));
            };
            let idx = word_id & (1usize << nb).wrapping_sub(1);
            let tid = word_id >> nb;
            let Some(&transform) = TRANSFORMS.get(tid) else {
                return Err(invalid("dictionary transform"));
            };
            let start = idx.saturating_mul(cpy_len).saturating_add(offset);
            let Some(word) = DICTIONARY.get(start..start.saturating_add(cpy_len)) else {
                return Err(invalid("dictionary word"));
            };
            apply_transform(out, word, transform);
            if out.len() > end {
                return Err(invalid("dictionary word overflows meta-block"));
            }
            continue;
        }
        if push {
            *dist = [dist[1], dist[2], dist[3], distance];
        }
        if out.len().saturating_add(cpy_len) > end {
            return Err(invalid("copy overflows meta-block"));
        }
        let distance = distance as usize;
        let Some(start) = out.len().checked_sub(distance).filter(|_| distance > 0) else {
            return Err(invalid("invalid distance"));
        };
        if distance >= cpy_len {
            out.extend_from_within(start..start.saturating_add(cpy_len));
        } else {
            for i in 0..cpy_len {
                let b = out.get(start.saturating_add(i)).copied().unwrap_or(0);
                out.push(b);
            }
        }
    }
    br.check()
}
fn special_distance(code: i64, dist: &[i64; 4]) -> i64 {
    let (last, second) = (dist[3], dist[2]);
    match code {
        0 => return last,
        1 => return second,
        2 => return dist[1],
        3 => return dist[0],
        _ => {}
    }
    const DELTAS: [i64; 6] = [-1, 1, -2, 2, -3, 3];
    let delta = |k: i64| DELTAS.get(k as usize).copied().unwrap_or(0);
    if code < 10 {
        return last.saturating_add(delta(code.saturating_sub(4)));
    }
    second.saturating_add(delta(code.saturating_sub(10)))
}
fn apply_transform(out: &mut Vec<u8>, word: &[u8], (prefix, kind, suffix): (&[u8], u8, &[u8])) {
    out.extend_from_slice(prefix);
    let kind = usize::from(kind);
    let mut w = word;
    if (1..=9).contains(&kind) {
        w = w.get(..w.len().saturating_sub(kind)).unwrap_or(&[]);
    } else if (12..=20).contains(&kind) {
        let skip = kind.saturating_sub(11);
        w = if skip >= w.len() {
            &[]
        } else {
            w.get(skip..).unwrap_or(&[])
        };
    }
    let start = out.len();
    out.extend_from_slice(w);
    if let Some(word) = out.get_mut(start..) {
        match kind {
            10 => uppercase(word, true),
            11 => uppercase(word, false),
            _ => {}
        }
    }
    out.extend_from_slice(suffix);
}
fn uppercase(b: &mut [u8], first_only: bool) {
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let (k, flip, step) = if c < 0xC0 {
            (i, if c.is_ascii_lowercase() { 32 } else { 0 }, 1)
        } else if c < 0xE0 {
            (i.saturating_add(1), 32, 2)
        } else {
            (i.saturating_add(2), 5, 3)
        };
        if let Some(t) = b.get_mut(k) {
            *t ^= flip;
        }
        if first_only {
            return;
        }
        i = i.saturating_add(step);
    }
}
pub fn encode_stored(mut data: &[u8]) -> Vec<u8> {
    struct Bits {
        out: Vec<u8>,
        acc: u64,
        n: u32,
    }
    impl Bits {
        fn put(&mut self, v: u64, bits: u32) {
            self.acc |= v << self.n;
            self.n = self.n.saturating_add(bits);
            while self.n >= 8 {
                self.out.push(self.acc as u8);
                self.acc >>= 8;
                self.n = self.n.saturating_sub(8);
            }
        }
        fn flush_byte(&mut self) {
            if self.n > 0 {
                self.out.push(self.acc as u8);
                self.acc = 0;
                self.n = 0;
            }
        }
    }
    let mut w = Bits {
        out: Vec::with_capacity(
            (data.len() / 65536)
                .saturating_mul(4)
                .saturating_add(data.len())
                .saturating_add(8),
        ),
        acc: 0,
        n: 0,
    };
    w.put(0, 1);
    while !data.is_empty() {
        let (chunk, rest) = data.split_at(data.len().min(65536));
        w.put(0, 1);
        w.put(0, 2);
        w.put(chunk.len().saturating_sub(1) as u64, 16);
        w.put(1, 1);
        w.flush_byte();
        w.out.extend_from_slice(chunk);
        data = rest;
    }
    w.put(1, 1);
    w.put(1, 1);
    w.flush_byte();
    w.out
}
