use super::bitio::{BitReader, bits_off, ceil_log2, val};
use super::{Error, corrupt};
pub(crate) const ANS_LOG_TAB_SIZE: u32 = 12;
pub(crate) const ANS_TAB_SIZE: u32 = 1 << ANS_LOG_TAB_SIZE;
pub(crate) const ANS_SIGNATURE: u32 = 0x13 << 16;
const MAX_CLUSTERS: u32 = 256;
const LZ77_WINDOW: u32 = 1 << 20;
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct HybridConfig {
    pub split_exp: u32,
    pub msb: u32,
    pub lsb: u32,
}
pub(crate) fn read_hybrid_config(br: &mut BitReader, log_alpha: u32) -> HybridConfig {
    let mut c = HybridConfig {
        split_exp: br.u(ceil_log2(u64::from(log_alpha).saturating_add(1))),
        ..Default::default()
    };
    if c.split_exp != log_alpha {
        c.msb = br.u(ceil_log2(u64::from(c.split_exp).saturating_add(1)));
        if c.msb > c.split_exp {
            br.fail(corrupt("hybrid config msb"));
            return c;
        }
        c.lsb = br.u(ceil_log2(
            u64::from(c.split_exp.saturating_sub(c.msb)).saturating_add(1),
        ));
    }
    if c.split_exp > 31 || c.msb.saturating_add(c.lsb) > c.split_exp {
        br.fail(corrupt("hybrid config"));
    }
    c
}
impl HybridConfig {
    pub(crate) fn value(&self, br: &mut BitReader, token: u32) -> u32 {
        let split = 1u32.checked_shl(self.split_exp).unwrap_or(0);
        if token < split {
            return token;
        }
        let ml = self.msb.wrapping_add(self.lsb);
        let n = self
            .split_exp
            .wrapping_sub(ml)
            .wrapping_add(token.wrapping_sub(split).checked_shr(ml).unwrap_or(0));
        if n > 32 {
            br.fail(corrupt("hybrid integer too large"));
            return 0;
        }
        let lmask = 1u32.checked_shl(self.lsb).unwrap_or(0).wrapping_sub(1);
        let low = token & lmask;
        let token = token.checked_shr(self.lsb).unwrap_or(0);
        let mbit = 1u32.checked_shl(self.msb).unwrap_or(0);
        let high = mbit | (token & mbit.wrapping_sub(1));
        let v = u64::from(high).checked_shl(n).unwrap_or(0) | br.read_bits(n);
        (v.checked_shl(self.lsb).unwrap_or(0) | u64::from(low)) as u32
    }
}
#[derive(Clone, Copy, Default)]
struct AliasEntry {
    cutoff: u32,
    right: u32,
    offsets1: u32,
    freq0: u32,
    freq1: u32,
}
#[derive(Clone)]
pub(crate) struct PrefixDecoder {
    single: Option<usize>,
    fast: [u16; 256],
    fast_len: [u8; 256],
    count: [u16; 16],
    symbols: Vec<u16>,
}
impl Default for PrefixDecoder {
    fn default() -> Self {
        PrefixDecoder {
            single: Some(0),
            fast: [0; 256],
            fast_len: [0; 256],
            count: [0; 16],
            symbols: Vec::new(),
        }
    }
}
#[derive(Default)]
pub(crate) struct EntropyCode {
    lz77_enabled: bool,
    lz77_min_symbol: u32,
    lz77_min_length: u32,
    lz77_config: HybridConfig,
    context_map: Vec<u8>,
    use_prefix: bool,
    log_alpha_size: u32,
    configs: Vec<HybridConfig>,
    prefix: Vec<PrefixDecoder>,
    alias: Vec<Vec<AliasEntry>>,
}
pub(crate) fn read_entropy_code(
    br: &mut BitReader,
    num_contexts: usize,
) -> Result<EntropyCode, Error> {
    read_entropy_code_lz(br, num_contexts, false)
}
fn read_entropy_code_lz(
    br: &mut BitReader,
    mut num_contexts: usize,
    disallow_lz77: bool,
) -> Result<EntropyCode, Error> {
    let mut code = EntropyCode {
        lz77_enabled: br.bool(),
        ..Default::default()
    };
    if code.lz77_enabled {
        if disallow_lz77 {
            return Err(corrupt("LZ77 in a two-entry context map"));
        }
        code.lz77_min_symbol = br.u32(val(224), val(512), val(4096), bits_off(15, 8));
        code.lz77_min_length = br.u32(val(3), val(4), bits_off(2, 5), bits_off(8, 9));
        code.lz77_config = read_hybrid_config(br, 8);
        num_contexts = num_contexts.saturating_add(1);
    }
    let mut num_clusters = 1;
    code.context_map = vec![0u8; num_contexts];
    if num_contexts > 1 {
        num_clusters = read_context_map(br, &mut code.context_map)?;
    }
    code.use_prefix = br.bool();
    code.log_alpha_size = if code.use_prefix {
        15
    } else {
        br.u(2).saturating_add(5)
    };
    code.configs = (0..num_clusters)
        .map(|_| read_hybrid_config(br, code.log_alpha_size))
        .collect();
    br.check()?;
    if code.use_prefix {
        let mut sizes = vec![1usize; num_clusters];
        for size in sizes.iter_mut() {
            if br.bool() {
                let n = br.u(4);
                if n > 14 {
                    return Err(corrupt("prefix alphabet size"));
                }
                *size = (1usize << n)
                    .saturating_add(br.u(n) as usize)
                    .saturating_add(1);
                if *size > 1 << 15 {
                    return Err(corrupt("prefix alphabet size"));
                }
            }
        }
        code.prefix = vec![PrefixDecoder::default(); num_clusters];
        for (pd, &size) in code.prefix.iter_mut().zip(&sizes) {
            if size == 1 {
                continue;
            }
            read_prefix_code(br, size, pd)?;
        }
    } else {
        code.alias = Vec::with_capacity(num_clusters);
        for _ in 0..num_clusters {
            let dist = read_ans_distribution(br, code.log_alpha_size)?;
            code.alias
                .push(build_alias_table(&dist, code.log_alpha_size)?);
        }
    }
    br.check()?;
    Ok(code)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn read_context_map(br: &mut BitReader, m: &mut [u8]) -> Result<usize, Error> {
    if br.bool() {
        let bits_per = br.u(2);
        for v in m.iter_mut() {
            *v = br.u(bits_per) as u8;
        }
    } else {
        let use_mtf = br.bool();
        let inner = read_entropy_code_lz(br, 1, m.len() <= 2)?;
        let mut sr = SymbolReader::new(&inner, br, 0);
        for v in m.iter_mut() {
            let x = sr.read_uint(br, 0);
            if x >= MAX_CLUSTERS {
                return Err(corrupt("context map cluster out of range"));
            }
            *v = x as u8;
        }
        if !sr.final_state_ok() {
            return Err(corrupt("context map ANS final state"));
        }
        if use_mtf {
            let mut mtf = [0u8; 256];
            for (i, e) in mtf.iter_mut().enumerate() {
                *e = i as u8;
            }
            for v in m.iter_mut() {
                let idx = *v as usize;
                let value = mtf[idx];
                *v = value;
                mtf.copy_within(0..idx, 1);
                mtf[0] = value;
            }
        }
    }
    br.check()?;
    let max = m.iter().copied().max().unwrap_or(0) as usize;
    let mut seen = vec![false; max + 1];
    let mut n = 0;
    for &v in m.iter() {
        if !seen[v as usize] {
            seen[v as usize] = true;
            n += 1;
        }
    }
    if n != max + 1 {
        return Err(corrupt("incomplete context map"));
    }
    Ok(max + 1)
}
#[allow(clippy::arithmetic_side_effects)]
fn read_var_len_uint8(br: &mut BitReader) -> usize {
    if !br.bool() {
        return 0;
    }
    let n = br.u(3);
    (1usize << n) + br.u(n) as usize
}
const LOG_COUNT_CODES: [(u8, u8); 14] = [
    (17, 5),
    (11, 4),
    (15, 4),
    (3, 4),
    (9, 4),
    (7, 4),
    (4, 3),
    (2, 3),
    (5, 3),
    (6, 3),
    (0, 3),
    (33, 6),
    (1, 7),
    (65, 7),
];
#[allow(clippy::arithmetic_side_effects)]
fn log_count_lookup(idx: u64) -> (u32, u32) {
    let mut out = (0, 0);
    for (sym, &(bits, length)) in LOG_COUNT_CODES.iter().enumerate() {
        if (idx as u8) & ((1u8 << length) - 1) == bits {
            out = (u32::from(length), sym as u32);
        }
    }
    out
}
pub(crate) fn popcount_precision(logcount: u32, shift: u32) -> u32 {
    let mut r = logcount as i32;
    let alt = (shift as i32).wrapping_sub((ANS_LOG_TAB_SIZE.wrapping_sub(logcount) >> 1) as i32);
    if alt < r {
        r = alt;
    }
    if r < 0 { 0 } else { r as u32 }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn read_ans_distribution(br: &mut BitReader, log_alpha: u32) -> Result<Vec<i32>, Error> {
    if br.bool() {
        let n = br.u(1) as usize + 1;
        let mut syms = [0usize; 2];
        let mut max_sym = 0;
        for s in syms.iter_mut().take(n) {
            *s = read_var_len_uint8(br);
            max_sym = max_sym.max(*s);
        }
        let mut counts = vec![0i32; max_sym + 1];
        if n == 1 {
            counts[syms[0]] = ANS_TAB_SIZE as i32;
        } else {
            if syms[0] == syms[1] {
                return Err(corrupt("duplicate ANS symbol"));
            }
            counts[syms[0]] = br.u(ANS_LOG_TAB_SIZE) as i32;
            counts[syms[1]] = ANS_TAB_SIZE as i32 - counts[syms[0]];
        }
        check_alphabet(&counts, log_alpha)?;
        return Ok(counts);
    }
    if br.bool() {
        let size = read_var_len_uint8(br) + 1;
        let counts: Vec<i32> = (0..size)
            .map(|i| {
                (ANS_TAB_SIZE as usize / size + usize::from(i < ANS_TAB_SIZE as usize % size))
                    as i32
            })
            .collect();
        check_alphabet(&counts, log_alpha)?;
        return Ok(counts);
    }
    let mut log = 0;
    while log < 3 {
        if br.u(1) == 0 {
            break;
        }
        log += 1;
    }
    let shift = (br.u(log) | 1 << log) - 1;
    if shift > ANS_LOG_TAB_SIZE + 1 {
        return Err(corrupt("ANS histogram shift"));
    }
    let length = read_var_len_uint8(br) + 3;
    let mut counts = vec![0i32; length];
    let mut logcounts = vec![0u32; length];
    let mut same = vec![0usize; length];
    let (mut omit_log, mut omit_pos): (i64, i64) = (-1, -1);
    let mut i = 0;
    while i < length {
        let idx = br.peek_bits(7);
        let (len, sym) = log_count_lookup(idx);
        br.skip_bits(len);
        logcounts[i] = sym;
        if sym == ANS_LOG_TAB_SIZE + 1 {
            let rle = read_var_len_uint8(br);
            same[i] = rle + 5;
            i += rle + 4;
            continue;
        }
        if i64::from(sym) > omit_log {
            omit_log = i64::from(sym);
            omit_pos = i as i64;
        }
        i += 1;
    }
    br.check()?;
    if omit_pos < 0
        || ((omit_pos + 1) < length as i64
            && logcounts[(omit_pos + 1) as usize] == ANS_LOG_TAB_SIZE + 1)
    {
        return Err(corrupt("ANS histogram"));
    }
    let omit_pos = omit_pos as usize;
    let mut total: i32 = 0;
    let mut num_same = 0usize;
    let mut prev: i32 = 0;
    for i in 0..length {
        if same[i] != 0 {
            num_same = same[i] - 1;
            prev = if i > 0 { counts[i - 1] } else { 0 };
        }
        if num_same > 0 {
            counts[i] = prev;
            num_same -= 1;
        } else {
            let code = logcounts[i];
            if i == omit_pos || code == 0 {
                continue;
            } else if code == 1 {
                counts[i] = 1;
            } else {
                let bc = popcount_precision(code - 1, shift);
                counts[i] = (1i32 << (code - 1)).wrapping_add((br.u(bc) as i32) << (code - 1 - bc));
            }
        }
        total = total.wrapping_add(counts[i]);
    }
    counts[omit_pos] = (ANS_TAB_SIZE as i32).wrapping_sub(total);
    if counts[omit_pos] <= 0 {
        return Err(corrupt("ANS histogram total"));
    }
    check_alphabet(&counts, log_alpha)?;
    Ok(counts)
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn check_alphabet(counts: &[i32], log_alpha: u32) -> Result<(), Error> {
    let mut n = counts.len();
    while n > 0 && counts[n - 1] == 0 {
        n -= 1;
    }
    if n > 1 << log_alpha {
        return Err(corrupt("ANS alphabet larger than the table"));
    }
    Ok(())
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn build_alias_table(dist: &[i32], log_alpha: u32) -> Result<Vec<AliasEntry>, Error> {
    let mut dist = dist;
    while let Some((&0, rest)) = dist.split_last() {
        dist = rest;
    }
    let full = [ANS_TAB_SIZE as i32];
    if dist.is_empty() {
        dist = &full;
    }
    let table_size = 1usize << log_alpha;
    let entry_size = ANS_TAB_SIZE >> log_alpha;
    if dist.len() > table_size {
        return Err(corrupt("ANS alphabet larger than the table"));
    }
    let mut table = vec![AliasEntry::default(); table_size];
    let mut sum: i32 = 0;
    let mut single = None;
    for (s, &v) in dist.iter().enumerate() {
        sum = sum.wrapping_add(v);
        if v == ANS_TAB_SIZE as i32 {
            single = Some(s);
        }
        if v < 0 {
            return Err(corrupt("negative ANS count"));
        }
    }
    if sum != ANS_TAB_SIZE as i32 {
        return Err(corrupt("ANS histogram does not sum to 4096"));
    }
    if let Some(single) = single {
        for (i, e) in table.iter_mut().enumerate() {
            *e = AliasEntry {
                cutoff: 0,
                right: single as u32,
                offsets1: entry_size * i as u32,
                freq0: 0,
                freq1: ANS_TAB_SIZE,
            };
        }
        return Ok(table);
    }
    let mut cutoffs = vec![0u32; table_size];
    let mut under = Vec::new();
    let mut over = Vec::new();
    for (i, &d) in dist.iter().enumerate() {
        cutoffs[i] = d as u32;
        if cutoffs[i] > entry_size {
            over.push(i);
        } else if cutoffs[i] < entry_size {
            under.push(i);
        }
    }
    under.extend(dist.len()..table_size);
    while let Some(o) = over.pop() {
        let Some(u) = under.pop() else {
            return Err(corrupt("ANS alias construction"));
        };
        let by = entry_size - cutoffs[u];
        cutoffs[o] -= by;
        table[u].right = o as u32;
        table[u].offsets1 = cutoffs[o];
        if cutoffs[o] < entry_size {
            under.push(o);
        } else if cutoffs[o] > entry_size {
            over.push(o);
        }
    }
    for (i, e) in table.iter_mut().enumerate() {
        if cutoffs[i] == entry_size {
            e.right = i as u32;
            e.offsets1 = 0;
            e.cutoff = 0;
        } else {
            e.offsets1 = e.offsets1.wrapping_sub(cutoffs[i]);
            e.cutoff = cutoffs[i];
        }
        if i < dist.len() {
            e.freq0 = dist[i] as u32;
        }
        let r = e.right as usize;
        if r < dist.len() {
            e.freq1 = dist[r] as u32;
        }
    }
    Ok(table)
}
pub(crate) fn alias_slots(dist: &[i32], log_alpha: u32) -> Vec<(u32, u32)> {
    let table = build_alias_table(dist, log_alpha).unwrap_or_default();
    let log_entry = ANS_LOG_TAB_SIZE.saturating_sub(log_alpha);
    let mask = 1u32.checked_shl(log_entry).unwrap_or(0).wrapping_sub(1);
    (0..ANS_TAB_SIZE)
        .map(|slot| {
            let idx = slot.checked_shr(log_entry).unwrap_or(0);
            let pos = slot & mask;
            match table.get(idx as usize) {
                Some(en) if pos >= en.cutoff => (en.right, en.offsets1.wrapping_add(pos)),
                _ => (idx, pos),
            }
        })
        .collect()
}
const CODE_LENGTH_ORDER: [usize; 18] =
    [1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const CLCL_LENGTH: [u32; 16] = [2, 2, 2, 3, 2, 2, 2, 4, 2, 2, 2, 3, 2, 2, 2, 4];
const CLCL_VALUE: [u8; 16] = [0, 4, 3, 2, 0, 4, 3, 1, 0, 4, 3, 2, 0, 4, 3, 5];
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn read_prefix_code(
    br: &mut BitReader,
    alphabet: usize,
    pd: &mut PrefixDecoder,
) -> Result<(), Error> {
    let mut lengths = vec![0u8; alphabet];
    let hskip = br.u(2);
    if hskip == 1 {
        let mut max_bits = 0;
        let mut c = alphabet - 1;
        while c > 0 {
            max_bits += 1;
            c >>= 1;
        }
        let nsym = br.u(2) as usize + 1;
        let mut syms = [0usize; 4];
        for i in 0..nsym {
            syms[i] = br.u(max_bits) as usize % alphabet;
            if syms[..i].contains(&syms[i]) {
                return Err(corrupt("duplicate simple prefix symbol"));
            }
        }
        match nsym {
            1 => {
                *pd = PrefixDecoder {
                    single: Some(syms[0]),
                    ..Default::default()
                };
                return br.check();
            }
            2 => {
                lengths[syms[0]] = 1;
                lengths[syms[1]] = 1;
            }
            3 => {
                lengths[syms[0]] = 1;
                lengths[syms[1]] = 2;
                lengths[syms[2]] = 2;
            }
            _ => {
                if br.u(1) == 0 {
                    for &s in &syms {
                        lengths[s] = 2;
                    }
                } else {
                    lengths[syms[0]] = 1;
                    lengths[syms[1]] = 2;
                    lengths[syms[2]] = 3;
                    lengths[syms[3]] = 3;
                }
            }
        }
        return pd.build(&lengths);
    }
    let mut cl = [0u8; 18];
    let mut space: i32 = 32;
    let mut num = 0;
    for &order in &CODE_LENGTH_ORDER[hskip as usize..] {
        let p = br.peek_bits(4) as usize;
        br.skip_bits(CLCL_LENGTH[p]);
        let v = CLCL_VALUE[p];
        cl[order] = v;
        if v != 0 {
            space -= 32 >> v;
            num += 1;
            if space <= 0 {
                break;
            }
        }
    }
    if num != 1 && space != 0 {
        return Err(corrupt("prefix code length code"));
    }
    let mut cl_code = PrefixDecoder::default();
    cl_code.build(&cl)?;
    let mut symbol = 0;
    let mut prev_len: u8 = 8;
    let mut repeat: usize = 0;
    let mut repeat_len: u8 = 0;
    let mut space: i64 = 32768;
    while symbol < alphabet && space > 0 {
        br.check()?;
        let c = cl_code.decode(br);
        if c < 16 {
            repeat = 0;
            lengths[symbol] = c as u8;
            symbol += 1;
            if c != 0 {
                prev_len = c as u8;
                space -= 32768 >> c;
            }
            continue;
        }
        let mut extra = 2;
        let mut new_len = prev_len;
        if c == 17 {
            extra = 3;
            new_len = 0;
        }
        if repeat_len != new_len {
            repeat = 0;
            repeat_len = new_len;
        }
        let old = repeat;
        if repeat > 0 {
            repeat = (repeat - 2) << extra;
        }
        repeat += br.u(extra) as usize + 3;
        let delta = repeat - old;
        if symbol + delta > alphabet {
            return Err(corrupt("prefix code repeat overflow"));
        }
        lengths[symbol..symbol + delta].fill(repeat_len);
        symbol += delta;
        if repeat_len != 0 {
            space -= (delta as i64) << (15 - repeat_len);
        }
    }
    if space != 0 {
        return Err(corrupt("incomplete prefix code"));
    }
    pd.build(&lengths)
}
impl PrefixDecoder {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn build(&mut self, lengths: &[u8]) -> Result<(), Error> {
        *self = PrefixDecoder {
            single: None,
            ..Default::default()
        };
        let (mut used, mut last) = (0usize, 0usize);
        for (s, &l) in lengths.iter().enumerate() {
            if l > 15 {
                return Err(corrupt("prefix code length"));
            }
            if l != 0 {
                self.count[l as usize] += 1;
                used += 1;
                last = s;
            }
        }
        if used == 0 {
            return Err(corrupt("empty prefix code"));
        }
        if used == 1 {
            self.single = Some(last);
            return Ok(());
        }
        let mut left: i64 = 1;
        for &count in &self.count[1..16] {
            left = (left << 1) - i64::from(count);
            if left < 0 {
                return Err(corrupt("over-subscribed prefix code"));
            }
        }
        if left != 0 {
            return Err(corrupt("incomplete prefix code"));
        }
        let mut offs = [0usize; 17];
        for l in 1..16 {
            offs[l + 1] = offs[l] + self.count[l] as usize;
        }
        self.symbols = vec![0u16; used];
        let mut next = offs;
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                self.symbols[next[l as usize]] = s as u16;
                next[l as usize] += 1;
            }
        }
        let mut code = 0usize;
        let mut idx = 0usize;
        for l in 1..=8usize {
            for _ in 0..self.count[l] {
                let mut rev = 0usize;
                for b in 0..l {
                    if (code >> (l - 1 - b)) & 1 == 1 {
                        rev |= 1 << b;
                    }
                }
                let mut fill = rev;
                while fill < 256 {
                    self.fast[fill] = self.symbols[idx];
                    self.fast_len[fill] = l as u8;
                    fill += 1 << l;
                }
                code += 1;
                idx += 1;
            }
            code <<= 1;
        }
        Ok(())
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn decode(&self, br: &mut BitReader) -> usize {
        if let Some(s) = self.single {
            return s;
        }
        let p = br.peek_bits(8) as usize;
        let l = self.fast_len[p];
        if l != 0 {
            br.skip_bits(u32::from(l));
            return self.fast[p] as usize;
        }
        let (mut code, mut first, mut index) = (0i64, 0i64, 0usize);
        for l in 1..16 {
            code |= br.read_bits(1) as i64;
            let count = i64::from(self.count[l]);
            if code - count < first {
                return self.symbols[index + (code - first) as usize] as usize;
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        br.fail(corrupt("invalid prefix code"));
        0
    }
}
const SPECIAL_DISTANCES: [[i8; 2]; 120] = [
    [0, 1],
    [1, 0],
    [1, 1],
    [-1, 1],
    [0, 2],
    [2, 0],
    [1, 2],
    [-1, 2],
    [2, 1],
    [-2, 1],
    [2, 2],
    [-2, 2],
    [0, 3],
    [3, 0],
    [1, 3],
    [-1, 3],
    [3, 1],
    [-3, 1],
    [2, 3],
    [-2, 3],
    [3, 2],
    [-3, 2],
    [0, 4],
    [4, 0],
    [1, 4],
    [-1, 4],
    [4, 1],
    [-4, 1],
    [3, 3],
    [-3, 3],
    [2, 4],
    [-2, 4],
    [4, 2],
    [-4, 2],
    [0, 5],
    [3, 4],
    [-3, 4],
    [4, 3],
    [-4, 3],
    [5, 0],
    [1, 5],
    [-1, 5],
    [5, 1],
    [-5, 1],
    [2, 5],
    [-2, 5],
    [5, 2],
    [-5, 2],
    [4, 4],
    [-4, 4],
    [3, 5],
    [-3, 5],
    [5, 3],
    [-5, 3],
    [0, 6],
    [6, 0],
    [1, 6],
    [-1, 6],
    [6, 1],
    [-6, 1],
    [2, 6],
    [-2, 6],
    [6, 2],
    [-6, 2],
    [4, 5],
    [-4, 5],
    [5, 4],
    [-5, 4],
    [3, 6],
    [-3, 6],
    [6, 3],
    [-6, 3],
    [0, 7],
    [7, 0],
    [1, 7],
    [-1, 7],
    [5, 5],
    [-5, 5],
    [7, 1],
    [-7, 1],
    [4, 6],
    [-4, 6],
    [6, 4],
    [-6, 4],
    [2, 7],
    [-2, 7],
    [7, 2],
    [-7, 2],
    [3, 7],
    [-3, 7],
    [7, 3],
    [-7, 3],
    [5, 6],
    [-5, 6],
    [6, 5],
    [-6, 5],
    [8, 0],
    [4, 7],
    [-4, 7],
    [7, 4],
    [-7, 4],
    [8, 1],
    [8, 2],
    [6, 6],
    [-6, 6],
    [8, 3],
    [5, 7],
    [-5, 7],
    [7, 5],
    [-7, 5],
    [8, 4],
    [6, 7],
    [-6, 7],
    [7, 6],
    [-7, 6],
    [8, 5],
    [7, 7],
    [-7, 7],
    [8, 6],
    [8, 7],
];
pub(crate) struct SymbolReader<'c> {
    code: &'c EntropyCode,
    state: u32,
    window: Vec<u32>,
    num_to_copy: u32,
    copy_pos: u32,
    num_decoded: u32,
    special: Vec<u32>,
    lz_cluster: usize,
}
impl<'c> SymbolReader<'c> {
    pub(crate) fn new(code: &'c EntropyCode, br: &mut BitReader, dist_multiplier: usize) -> Self {
        let mut sr = SymbolReader {
            code,
            state: 0,
            window: Vec::new(),
            num_to_copy: 0,
            copy_pos: 0,
            num_decoded: 0,
            special: Vec::new(),
            lz_cluster: 0,
        };
        if !code.use_prefix {
            sr.state = br.read_bits(32) as u32;
        }
        if code.lz77_enabled {
            sr.window = vec![0; LZ77_WINDOW as usize];
            sr.lz_cluster = code.context_map.last().copied().unwrap_or(0) as usize;
            if dist_multiplier != 0 {
                sr.special = SPECIAL_DISTANCES
                    .iter()
                    .map(|&[dx, dy]| {
                        let v = (dist_multiplier as i64)
                            .saturating_mul(i64::from(dy))
                            .saturating_add(i64::from(dx));
                        v.max(1) as u32
                    })
                    .collect();
            }
        }
        sr
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn read_symbol(&mut self, br: &mut BitReader, cluster: usize) -> u32 {
        if self.code.use_prefix {
            return self.code.prefix[cluster].decode(br) as u32;
        }
        let table = &self.code.alias[cluster];
        let res = self.state & (ANS_TAB_SIZE - 1);
        let log_entry = ANS_LOG_TAB_SIZE - self.code.log_alpha_size;
        let i = res >> log_entry;
        let pos = res & ((1 << log_entry) - 1);
        let e = &table[i as usize];
        let (sym, offset, freq) = if pos >= e.cutoff {
            (e.right, e.offsets1.wrapping_add(pos), e.freq1)
        } else {
            (i, pos, e.freq0)
        };
        self.state = freq
            .wrapping_mul(self.state >> ANS_LOG_TAB_SIZE)
            .wrapping_add(offset);
        if self.state < 1 << 16 {
            self.state = (self.state << 16) | br.read_bits(16) as u32;
        }
        sym
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn copy_one(&mut self) -> u32 {
        let v = self.window[(self.copy_pos & (LZ77_WINDOW - 1)) as usize];
        self.copy_pos = self.copy_pos.wrapping_add(1);
        self.num_to_copy -= 1;
        self.window[(self.num_decoded & (LZ77_WINDOW - 1)) as usize] = v;
        self.num_decoded = self.num_decoded.wrapping_add(1);
        v
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn read_uint(&mut self, br: &mut BitReader, ctx: usize) -> u32 {
        if self.num_to_copy > 0 {
            return self.copy_one();
        }
        if ctx >= self.code.context_map.len() {
            if br.err.is_none() {
                br.err = Some(corrupt(format!("entropy context {ctx} out of range")));
            }
            return 0;
        }
        let cluster = self.code.context_map[ctx] as usize;
        let token = self.read_symbol(br, cluster);
        if self.code.lz77_enabled && token >= self.code.lz77_min_symbol {
            self.num_to_copy = self
                .code
                .lz77_config
                .value(br, token - self.code.lz77_min_symbol)
                .wrapping_add(self.code.lz77_min_length);
            let dtoken = self.read_symbol(br, self.lz_cluster);
            let mut distance = self.code.configs[self.lz_cluster].value(br, dtoken);
            if (distance as usize) < self.special.len() {
                distance = self.special[distance as usize];
            } else {
                distance = distance
                    .wrapping_add(1)
                    .wrapping_sub(self.special.len() as u32);
            }
            distance = distance.min(self.num_decoded).min(LZ77_WINDOW);
            self.copy_pos = self.num_decoded - distance;
            if distance == 0 {
                let n = self.num_to_copy.min(LZ77_WINDOW) as usize;
                self.window[..n].fill(0);
            }
            if self.num_to_copy == 0 {
                return 0;
            }
            return self.copy_one();
        }
        let v = self.code.configs[cluster].value(br, token);
        if !self.window.is_empty() {
            self.window[(self.num_decoded & (LZ77_WINDOW - 1)) as usize] = v;
            self.num_decoded = self.num_decoded.wrapping_add(1);
        }
        v
    }
    pub(crate) fn final_state_ok(&self) -> bool {
        self.code.use_prefix || self.state == ANS_SIGNATURE
    }
}
pub(crate) fn read_permutation(
    br: &mut BitReader,
    sr: &mut SymbolReader,
    size: usize,
    skip: usize,
) -> Result<Vec<usize>, Error> {
    let end = (sr.read_uint(br, coeff_order_context(size as u32)) as usize).saturating_add(skip);
    if end > size {
        return Err(corrupt("permutation end"));
    }
    let mut lehmer = vec![0usize; size];
    let mut last = 0u32;
    for (i, l) in lehmer.iter_mut().enumerate().take(end).skip(skip) {
        let v = sr.read_uint(br, coeff_order_context(last));
        last = v;
        if v as usize >= size.saturating_sub(i) {
            return Err(corrupt("lehmer code"));
        }
        *l = v as usize;
    }
    br.check()?;
    Ok(decode_lehmer(&lehmer))
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
fn decode_lehmer(lehmer: &[usize]) -> Vec<usize> {
    let n = lehmer.len();
    let mut tree = vec![0usize; n + 1];
    for i in 1..=n {
        tree[i] += 1;
        let j = i + (i & i.wrapping_neg());
        if j <= n {
            tree[j] += tree[i];
        }
    }
    let mut top = 1;
    while top * 2 <= n {
        top *= 2;
    }
    let mut perm = vec![0usize; n];
    for (i, &k) in lehmer.iter().enumerate() {
        let (mut pos, mut rem) = (0usize, k + 1);
        let mut step = top;
        while step > 0 {
            let next = pos + step;
            if next <= n && tree[next] < rem {
                pos = next;
                rem -= tree[next];
            }
            step >>= 1;
        }
        perm[i] = pos;
        let mut j = pos + 1;
        while j <= n {
            tree[j] -= 1;
            j += j & j.wrapping_neg();
        }
    }
    perm
}
pub(crate) fn coeff_order_context(x: u32) -> usize {
    ceil_log2(u64::from(x).saturating_add(1)).min(7) as usize
}
