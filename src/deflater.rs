#![forbid(unsafe_code)]
const NO_COMPRESSION: i32 = 0;
const BEST_SPEED: i32 = 1;
pub const DEFAULT_COMPRESSION: i32 = -1;
const HUFFMAN_ONLY: i32 = -2;
const LOG_WINDOW_SIZE: usize = 15;
const WINDOW_SIZE: usize = 1 << LOG_WINDOW_SIZE;
const WINDOW_MASK: usize = WINDOW_SIZE - 1;
const BASE_MATCH_LENGTH: i64 = 3;
const MIN_MATCH_LENGTH: i64 = 4;
const MAX_MATCH_LENGTH: i64 = 258;
const BASE_MATCH_OFFSET: i64 = 1;
const MAX_MATCH_OFFSET: i64 = 1 << 15;
const MAX_FLATE_BLOCK_TOKENS: usize = 1 << 14;
const MAX_STORE_BLOCK_SIZE: usize = 65535;
const HASH_BITS: u32 = 17;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: u32 = (1 << HASH_BITS) - 1;
const MAX_HASH_OFFSET: i64 = 1 << 24;
const SKIP_NEVER: i64 = i32::MAX as i64;
const HASHMUL: u32 = 0x1e35a7bd;
const MAX_NUM_LIT: usize = 286;
#[derive(Clone, Copy, Default)]
struct CompressionLevel {
    #[allow(dead_code)]
    level: i64,
    good: i64,
    lazy: i64,
    nice: i64,
    chain: i64,
    fast_skip_hashing: i64,
}
const fn lvl(
    level: i64,
    good: i64,
    lazy: i64,
    nice: i64,
    chain: i64,
    skip: i64,
) -> CompressionLevel {
    CompressionLevel {
        level,
        good,
        lazy,
        nice,
        chain,
        fast_skip_hashing: skip,
    }
}
const LEVELS: [CompressionLevel; 10] = [
    lvl(0, 0, 0, 0, 0, 0),
    lvl(1, 0, 0, 0, 0, 0),
    lvl(2, 4, 0, 16, 8, 5),
    lvl(3, 4, 0, 32, 32, 6),
    lvl(4, 4, 4, 16, 16, SKIP_NEVER),
    lvl(5, 8, 16, 32, 32, SKIP_NEVER),
    lvl(6, 8, 16, 128, 128, SKIP_NEVER),
    lvl(7, 8, 32, 128, 256, SKIP_NEVER),
    lvl(8, 32, 128, 258, 1024, SKIP_NEVER),
    lvl(9, 32, 258, 258, 4096, SKIP_NEVER),
];
type Token = u32;
const LENGTH_SHIFT: u32 = 22;
const OFFSET_MASK: u32 = (1 << LENGTH_SHIFT) - 1;
const MATCH_TYPE: u32 = 1 << 30;
#[rustfmt::skip]
const LENGTH_CODES: [u32; 256] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 8, 9, 9, 10, 10, 11, 11,
    12, 12, 12, 12, 13, 13, 13, 13, 14, 14, 14, 14, 15, 15, 15, 15,
    16, 16, 16, 16, 16, 16, 16, 16, 17, 17, 17, 17, 17, 17, 17, 17,
    18, 18, 18, 18, 18, 18, 18, 18, 19, 19, 19, 19, 19, 19, 19, 19,
    20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20,
    21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21,
    22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22,
    23, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23, 23,
    24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24,
    24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24,
    25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25,
    25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25, 25,
    26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26,
    26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26, 26,
    27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27,
    27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 27, 28,
];
#[rustfmt::skip]
const OFFSET_CODES: [u32; 256] = [
    0, 1, 2, 3, 4, 4, 5, 5, 6, 6, 6, 6, 7, 7, 7, 7,
    8, 8, 8, 8, 8, 8, 8, 8, 9, 9, 9, 9, 9, 9, 9, 9,
    10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10, 10,
    11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11, 11,
    12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12,
    12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12, 12,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13, 13,
    14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14,
    14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14,
    14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14,
    14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14, 14,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15,
];
fn literal_token(literal: u32) -> Token {
    literal
}
fn match_token(xlength: u32, xoffset: u32) -> Token {
    MATCH_TYPE
        .wrapping_add(xlength << LENGTH_SHIFT)
        .wrapping_add(xoffset)
}
fn token_length(t: Token) -> u32 {
    t.wrapping_sub(MATCH_TYPE) >> LENGTH_SHIFT
}
fn token_offset(t: Token) -> u32 {
    t & OFFSET_MASK
}
fn length_code(len: u32) -> u32 {
    LENGTH_CODES[len as usize]
}
fn offset_code(off: u32) -> u32 {
    if (off as usize) < OFFSET_CODES.len() {
        return OFFSET_CODES[off as usize];
    }
    if ((off >> 7) as usize) < OFFSET_CODES.len() {
        return OFFSET_CODES[(off >> 7) as usize] + 14;
    }
    OFFSET_CODES[(off >> 14) as usize] + 28
}
#[derive(Clone, Copy, Default)]
struct HCode {
    code: u16,
    len: u16,
}
#[derive(Clone, Copy, Default)]
struct LiteralNode {
    literal: u16,
    freq: i32,
}
#[derive(Clone, Copy, Default)]
struct LevelInfo {
    level: i32,
    last_freq: i32,
    next_char_freq: i32,
    next_pair_freq: i32,
    needed: i32,
}
const MAX_NODE: LiteralNode = LiteralNode {
    literal: u16::MAX,
    freq: i32::MAX,
};
const MAX_BITS_LIMIT: usize = 16;
#[derive(Clone)]
struct HuffmanEncoder {
    codes: Vec<HCode>,
    freqcache: Vec<LiteralNode>,
    bit_count: [i32; 17],
}
fn reverse_bits(number: u16, bit_length: u8) -> u16 {
    (number << (16 - u32::from(bit_length))).reverse_bits()
}
impl HuffmanEncoder {
    fn new(size: usize) -> Self {
        HuffmanEncoder {
            codes: vec![HCode::default(); size],
            freqcache: Vec::new(),
            bit_count: [0; 17],
        }
    }
    fn fixed_literal() -> Self {
        let mut h = HuffmanEncoder::new(MAX_NUM_LIT);
        for ch in 0..MAX_NUM_LIT as u16 {
            let (bits, size) = match ch {
                0..144 => (ch + 48, 8),
                144..256 => (ch + 400 - 144, 9),
                256..280 => (ch - 256, 7),
                _ => (ch + 192 - 280, 8),
            };
            h.codes[ch as usize] = HCode {
                code: reverse_bits(bits, size as u8),
                len: size,
            };
        }
        h
    }
    fn fixed_offset() -> Self {
        let mut h = HuffmanEncoder::new(30);
        for (ch, code) in h.codes.iter_mut().enumerate() {
            *code = HCode {
                code: reverse_bits(ch as u16, 5),
                len: 5,
            };
        }
        h
    }
    fn huff_offset() -> Self {
        let mut freq = vec![0i32; OFFSET_CODE_COUNT];
        freq[0] = 1;
        let mut h = HuffmanEncoder::new(OFFSET_CODE_COUNT);
        h.generate(&freq, 15);
        h
    }
    fn bit_counts(&mut self, n: usize, max_bits: i32) -> Vec<i32> {
        assert!(max_bits < MAX_BITS_LIMIT as i32, "flate: maxBits too large");
        let list = &mut self.freqcache;
        let n32 = n as i32;
        list[n] = MAX_NODE;
        let max_bits = max_bits.min(n32 - 1);
        let mb = max_bits as usize;
        let mut levels = [LevelInfo::default(); MAX_BITS_LIMIT];
        let mut leaf_counts = [[0i32; MAX_BITS_LIMIT]; MAX_BITS_LIMIT];
        for level in 1..=mb {
            levels[level] = LevelInfo {
                level: level as i32,
                last_freq: list[1].freq,
                next_char_freq: list[2].freq,
                next_pair_freq: list[0].freq.wrapping_add(list[1].freq),
                needed: 0,
            };
            leaf_counts[level][level] = 2;
            if level == 1 {
                levels[level].next_pair_freq = i32::MAX;
            }
        }
        levels[mb].needed = 2 * n32 - 4;
        let mut level = mb;
        loop {
            if levels[level].next_pair_freq == i32::MAX && levels[level].next_char_freq == i32::MAX
            {
                levels[level].needed = 0;
                levels[level + 1].next_pair_freq = i32::MAX;
                level += 1;
                continue;
            }
            let prev_freq = levels[level].last_freq;
            if levels[level].next_char_freq < levels[level].next_pair_freq {
                let n = leaf_counts[level][level] + 1;
                levels[level].last_freq = levels[level].next_char_freq;
                leaf_counts[level][level] = n;
                levels[level].next_char_freq = list[n as usize].freq;
            } else {
                levels[level].last_freq = levels[level].next_pair_freq;
                let (lower, upper) = leaf_counts.split_at_mut(level);
                upper[0][..level].copy_from_slice(&lower[level - 1][..level]);
                let below = levels[level].level as usize - 1;
                levels[below].needed = 2;
            }
            levels[level].needed -= 1;
            if levels[level].needed == 0 {
                if levels[level].level == max_bits {
                    break;
                }
                levels[level + 1].next_pair_freq = prev_freq.wrapping_add(levels[level].last_freq);
                level += 1;
            } else {
                while levels[level - 1].needed > 0 {
                    level -= 1;
                }
            }
        }
        assert!(
            leaf_counts[mb][mb] == n32,
            "leafCounts[maxBits][maxBits] != n"
        );
        let counts = &leaf_counts[mb];
        for (bits, level) in (1usize..).zip((1..=mb).rev()) {
            self.bit_count[bits] = counts[level] - counts[level - 1];
        }
        self.bit_count[..=mb].to_vec()
    }
    fn assign_encoding_and_size(&mut self, bit_count: &[i32], count: usize) {
        let mut code: u16 = 0;
        let mut end = count;
        for (n, &bits) in bit_count.iter().enumerate() {
            code <<= 1;
            if n == 0 || bits == 0 {
                continue;
            }
            let start = end - bits as usize;
            let chunk = &mut self.freqcache[start..end];
            chunk.sort_unstable_by_key(|node| node.literal);
            for node in chunk.iter() {
                self.codes[node.literal as usize] = HCode {
                    code: reverse_bits(code, n as u8),
                    len: n as u16,
                };
                code = code.wrapping_add(1);
            }
            end = start;
        }
    }
    fn generate(&mut self, freq: &[i32], max_bits: i32) {
        if self.freqcache.is_empty() {
            self.freqcache = vec![LiteralNode::default(); MAX_NUM_LIT + 1];
        }
        let mut count = 0usize;
        for (i, &f) in freq.iter().enumerate() {
            if f != 0 {
                self.freqcache[count] = LiteralNode {
                    literal: i as u16,
                    freq: f,
                };
                count += 1;
            } else {
                self.codes[i].len = 0;
            }
        }
        if count <= 2 {
            for i in 0..count {
                let literal = self.freqcache[i].literal as usize;
                self.codes[literal] = HCode {
                    code: i as u16,
                    len: 1,
                };
            }
            return;
        }
        self.freqcache[..count]
            .sort_unstable_by(|a, b| a.freq.cmp(&b.freq).then(a.literal.cmp(&b.literal)));
        let bit_count = self.bit_counts(count, max_bits);
        self.assign_encoding_and_size(&bit_count, count);
    }
}
fn bit_length(codes: &[HCode], freq: &[i32]) -> i64 {
    let mut total = 0i64;
    for (i, &f) in freq.iter().enumerate() {
        if f != 0 {
            total += i64::from(f) * i64::from(codes[i].len);
        }
    }
    total
}
const OFFSET_CODE_COUNT: usize = 30;
const END_BLOCK_MARKER: u32 = 256;
const LENGTH_CODES_START: usize = 257;
const CODEGEN_CODE_COUNT: usize = 19;
const BAD_CODE: u8 = 255;
const LENGTH_EXTRA_BITS: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const LENGTH_BASE: [u32; 29] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128,
    160, 192, 224, 255,
];
const OFFSET_EXTRA_BITS: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const OFFSET_BASE: [u32; 30] = [
    0x000000, 0x000001, 0x000002, 0x000003, 0x000004, 0x000006, 0x000008, 0x00000c, 0x000010,
    0x000018, 0x000020, 0x000030, 0x000040, 0x000060, 0x000080, 0x0000c0, 0x000100, 0x000180,
    0x000200, 0x000300, 0x000400, 0x000600, 0x000800, 0x000c00, 0x001000, 0x001800, 0x002000,
    0x003000, 0x004000, 0x006000,
];
const CODEGEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];
#[derive(Clone, Copy, PartialEq, Eq)]
enum Enc {
    Fixed,
    Dynamic,
}
struct Statics {
    fixed_literal: HuffmanEncoder,
    fixed_offset: HuffmanEncoder,
    huff_offset: HuffmanEncoder,
}
fn statics() -> &'static Statics {
    static S: std::sync::OnceLock<Statics> = std::sync::OnceLock::new();
    S.get_or_init(|| Statics {
        fixed_literal: HuffmanEncoder::fixed_literal(),
        fixed_offset: HuffmanEncoder::fixed_offset(),
        huff_offset: HuffmanEncoder::huff_offset(),
    })
}
struct HuffmanBitWriter {
    out: Vec<u8>,
    bits: u64,
    nbits: u32,
    codegen_freq: [i32; CODEGEN_CODE_COUNT],
    literal_freq: Vec<i32>,
    offset_freq: Vec<i32>,
    codegen: Vec<u8>,
    literal_encoding: HuffmanEncoder,
    offset_encoding: HuffmanEncoder,
    codegen_encoding: HuffmanEncoder,
}
fn with_end(tokens: &[Token]) -> impl Iterator<Item = Token> + '_ {
    tokens
        .iter()
        .copied()
        .chain(std::iter::once(END_BLOCK_MARKER))
}
fn generate_codegen(
    codegen: &mut [u8],
    codegen_freq: &mut [i32; CODEGEN_CODE_COUNT],
    num_literals: usize,
    num_offsets: usize,
    lit: &[HCode],
    off: &[HCode],
) {
    *codegen_freq = [0; CODEGEN_CODE_COUNT];
    for i in 0..num_literals {
        codegen[i] = lit[i].len as u8;
    }
    for i in 0..num_offsets {
        codegen[num_literals + i] = off[i].len as u8;
    }
    codegen[num_literals + num_offsets] = BAD_CODE;
    let mut size = codegen[0];
    let mut count: i32 = 1;
    let mut out_index = 0usize;
    let mut in_index = 1usize;
    while size != BAD_CODE {
        let next_size = codegen[in_index];
        if next_size == size {
            count += 1;
            in_index += 1;
            continue;
        }
        if size != 0 {
            codegen[out_index] = size;
            out_index += 1;
            codegen_freq[size as usize] += 1;
            count -= 1;
            while count >= 3 {
                let n = count.min(6);
                codegen[out_index] = 16;
                out_index += 1;
                codegen[out_index] = (n - 3) as u8;
                out_index += 1;
                codegen_freq[16] += 1;
                count -= n;
            }
        } else {
            while count >= 11 {
                let n = count.min(138);
                codegen[out_index] = 18;
                out_index += 1;
                codegen[out_index] = (n - 11) as u8;
                out_index += 1;
                codegen_freq[18] += 1;
                count -= n;
            }
            if count >= 3 {
                codegen[out_index] = 17;
                out_index += 1;
                codegen[out_index] = (count - 3) as u8;
                out_index += 1;
                codegen_freq[17] += 1;
                count = 0;
            }
        }
        count -= 1;
        while count >= 0 {
            codegen[out_index] = size;
            out_index += 1;
            codegen_freq[size as usize] += 1;
            count -= 1;
        }
        size = next_size;
        count = 1;
        in_index += 1;
    }
    codegen[out_index] = BAD_CODE;
}
impl HuffmanBitWriter {
    fn new() -> Self {
        HuffmanBitWriter {
            out: Vec::new(),
            bits: 0,
            nbits: 0,
            codegen_freq: [0; CODEGEN_CODE_COUNT],
            literal_freq: vec![0; MAX_NUM_LIT],
            offset_freq: vec![0; OFFSET_CODE_COUNT],
            codegen: vec![0; MAX_NUM_LIT + OFFSET_CODE_COUNT + 1],
            literal_encoding: HuffmanEncoder::new(MAX_NUM_LIT),
            offset_encoding: HuffmanEncoder::new(OFFSET_CODE_COUNT),
            codegen_encoding: HuffmanEncoder::new(CODEGEN_CODE_COUNT),
        }
    }
    fn flush(&mut self) {
        while self.nbits != 0 {
            self.out.push(self.bits as u8);
            self.bits >>= 8;
            self.nbits = self.nbits.saturating_sub(8);
        }
        self.bits = 0;
    }
    fn emit48(&mut self) {
        if self.nbits >= 48 {
            let bits = self.bits;
            self.bits >>= 48;
            self.nbits -= 48;
            self.out.extend_from_slice(&bits.to_le_bytes()[..6]);
        }
    }
    fn write_bits(&mut self, b: i32, nb: u32) {
        self.bits |= (i64::from(b) as u64) << self.nbits;
        self.nbits += nb;
        self.emit48();
    }
    fn write_code(&mut self, c: HCode) {
        self.bits |= u64::from(c.code) << self.nbits;
        self.nbits += u32::from(c.len);
        self.emit48();
    }
    fn write_bytes(&mut self, bytes: &[u8]) {
        while self.nbits != 0 {
            self.out.push(self.bits as u8);
            self.bits >>= 8;
            self.nbits -= 8;
        }
        self.out.extend_from_slice(bytes);
    }
    fn dynamic_size(&self, lit: &[HCode], off: &[HCode], extra_bits: i64) -> (i64, usize) {
        let mut num_codegens = self.codegen_freq.len();
        while num_codegens > 4 && self.codegen_freq[CODEGEN_ORDER[num_codegens - 1]] == 0 {
            num_codegens -= 1;
        }
        let header = 3
            + 5
            + 5
            + 4
            + 3 * num_codegens as i64
            + bit_length(&self.codegen_encoding.codes, &self.codegen_freq)
            + i64::from(self.codegen_freq[16]) * 2
            + i64::from(self.codegen_freq[17]) * 3
            + i64::from(self.codegen_freq[18]) * 7;
        let size = header
            + bit_length(lit, &self.literal_freq)
            + bit_length(off, &self.offset_freq)
            + extra_bits;
        (size, num_codegens)
    }
    fn fixed_size(&self, extra_bits: i64) -> i64 {
        let s = statics();
        3 + bit_length(&s.fixed_literal.codes, &self.literal_freq)
            + bit_length(&s.fixed_offset.codes, &self.offset_freq)
            + extra_bits
    }
    fn stored_size(input: Option<&[u8]>) -> (i64, bool) {
        match input {
            Some(input) if input.len() <= MAX_STORE_BLOCK_SIZE => {
                ((input.len() as i64 + 5) * 8, true)
            }
            _ => (0, false),
        }
    }
    fn write_dynamic_header(
        &mut self,
        num_literals: usize,
        num_offsets: usize,
        num_codegens: usize,
        eof: bool,
    ) {
        self.write_bits(if eof { 5 } else { 4 }, 3);
        self.write_bits(num_literals as i32 - 257, 5);
        self.write_bits(num_offsets as i32 - 1, 5);
        self.write_bits(num_codegens as i32 - 4, 4);
        for &order in CODEGEN_ORDER.iter().take(num_codegens) {
            let value = self.codegen_encoding.codes[order].len;
            self.write_bits(i32::from(value), 3);
        }
        let mut i = 0;
        loop {
            let code_word = self.codegen[i];
            i += 1;
            if code_word == BAD_CODE {
                break;
            }
            let c = self.codegen_encoding.codes[code_word as usize];
            self.write_code(c);
            let extra = match code_word {
                16 => 2,
                17 => 3,
                18 => 7,
                _ => continue,
            };
            let v = self.codegen[i];
            self.write_bits(i32::from(v), extra);
            i += 1;
        }
    }
    fn write_stored_header(&mut self, length: usize, eof: bool) {
        self.write_bits(i32::from(eof), 3);
        self.flush();
        self.write_bits(length as i32, 16);
        self.write_bits(i32::from(!(length as u16)), 16);
    }
    fn write_fixed_header(&mut self, eof: bool) {
        self.write_bits(if eof { 3 } else { 2 }, 3);
    }
    fn index_tokens(&mut self, tokens: &[Token]) -> (usize, usize) {
        self.literal_freq.fill(0);
        self.offset_freq.fill(0);
        for t in with_end(tokens) {
            if t < MATCH_TYPE {
                self.literal_freq[t as usize] += 1;
                continue;
            }
            self.literal_freq[LENGTH_CODES_START + length_code(token_length(t)) as usize] += 1;
            self.offset_freq[offset_code(token_offset(t)) as usize] += 1;
        }
        let mut num_literals = self.literal_freq.len();
        while self.literal_freq[num_literals - 1] == 0 {
            num_literals -= 1;
        }
        let mut num_offsets = self.offset_freq.len();
        while num_offsets > 0 && self.offset_freq[num_offsets - 1] == 0 {
            num_offsets -= 1;
        }
        if num_offsets == 0 {
            self.offset_freq[0] = 1;
            num_offsets = 1;
        }
        self.literal_encoding.generate(&self.literal_freq, 15);
        self.offset_encoding.generate(&self.offset_freq, 15);
        (num_literals, num_offsets)
    }
    fn codegen_for(&mut self, num_literals: usize, num_offsets: usize, huff_offset: bool) {
        let off = if huff_offset {
            &statics().huff_offset.codes
        } else {
            &self.offset_encoding.codes
        };
        generate_codegen(
            &mut self.codegen,
            &mut self.codegen_freq,
            num_literals,
            num_offsets,
            &self.literal_encoding.codes,
            off,
        );
        let freq = self.codegen_freq;
        self.codegen_encoding.generate(&freq, 7);
    }
    fn write_block(&mut self, tokens: &[Token], eof: bool, input: Option<&[u8]>) {
        let (num_literals, num_offsets) = self.index_tokens(tokens);
        let mut extra_bits = 0i64;
        let (stored_size, storable) = Self::stored_size(input);
        if storable {
            for lc in LENGTH_CODES_START + 8..num_literals {
                extra_bits += i64::from(self.literal_freq[lc])
                    * i64::from(LENGTH_EXTRA_BITS[lc - LENGTH_CODES_START]);
            }
            for oc in 4..num_offsets {
                extra_bits += i64::from(self.offset_freq[oc]) * i64::from(OFFSET_EXTRA_BITS[oc]);
            }
        }
        let mut enc = Enc::Fixed;
        let mut size = self.fixed_size(extra_bits);
        self.codegen_for(num_literals, num_offsets, false);
        let (dynamic_size, num_codegens) = self.dynamic_size(
            &self.literal_encoding.codes,
            &self.offset_encoding.codes,
            extra_bits,
        );
        if dynamic_size < size {
            size = dynamic_size;
            enc = Enc::Dynamic;
        }
        if storable && stored_size < size {
            let input = input.unwrap_or_default();
            self.write_stored_header(input.len(), eof);
            self.write_bytes(input);
            return;
        }
        if enc == Enc::Fixed {
            self.write_fixed_header(eof);
            let s = statics();
            self.write_tokens(tokens, &s.fixed_literal.codes, &s.fixed_offset.codes);
        } else {
            self.write_dynamic_header(num_literals, num_offsets, num_codegens, eof);
            let lit = self.literal_encoding.codes.clone();
            let off = self.offset_encoding.codes.clone();
            self.write_tokens(tokens, &lit, &off);
        }
    }
    fn write_block_dynamic(&mut self, tokens: &[Token], eof: bool, input: Option<&[u8]>) {
        let (num_literals, num_offsets) = self.index_tokens(tokens);
        self.codegen_for(num_literals, num_offsets, false);
        let (size, num_codegens) =
            self.dynamic_size(&self.literal_encoding.codes, &self.offset_encoding.codes, 0);
        let (ssize, storable) = Self::stored_size(input);
        if storable && ssize < size + (size >> 4) {
            let input = input.unwrap_or_default();
            self.write_stored_header(input.len(), eof);
            self.write_bytes(input);
            return;
        }
        self.write_dynamic_header(num_literals, num_offsets, num_codegens, eof);
        let lit = self.literal_encoding.codes.clone();
        let off = self.offset_encoding.codes.clone();
        self.write_tokens(tokens, &lit, &off);
    }
    fn write_tokens(&mut self, tokens: &[Token], le: &[HCode], oe: &[HCode]) {
        for t in with_end(tokens) {
            if t < MATCH_TYPE {
                self.write_code(le[t as usize]);
                continue;
            }
            let length = token_length(t);
            let lc = length_code(length) as usize;
            self.write_code(le[lc + LENGTH_CODES_START]);
            let extra_length_bits = u32::from(LENGTH_EXTRA_BITS[lc]);
            if extra_length_bits > 0 {
                self.write_bits((length - LENGTH_BASE[lc]) as i32, extra_length_bits);
            }
            let offset = token_offset(t);
            let oc = offset_code(offset) as usize;
            self.write_code(oe[oc]);
            let extra_offset_bits = u32::from(OFFSET_EXTRA_BITS[oc]);
            if extra_offset_bits > 0 {
                self.write_bits((offset - OFFSET_BASE[oc]) as i32, extra_offset_bits);
            }
        }
    }
    fn write_block_huff(&mut self, eof: bool, input: &[u8]) {
        self.literal_freq.fill(0);
        for &b in input {
            self.literal_freq[b as usize] += 1;
        }
        self.literal_freq[END_BLOCK_MARKER as usize] = 1;
        let num_literals = END_BLOCK_MARKER as usize + 1;
        self.offset_freq[0] = 1;
        let num_offsets = 1;
        self.literal_encoding.generate(&self.literal_freq, 15);
        self.codegen_for(num_literals, num_offsets, true);
        let (size, num_codegens) = self.dynamic_size(
            &self.literal_encoding.codes,
            &statics().huff_offset.codes,
            0,
        );
        let (ssize, storable) = Self::stored_size(Some(input));
        if storable && ssize < size + (size >> 4) {
            self.write_stored_header(input.len(), eof);
            self.write_bytes(input);
            return;
        }
        self.write_dynamic_header(num_literals, num_offsets, num_codegens, eof);
        let encoding: Vec<HCode> = self.literal_encoding.codes[..257].to_vec();
        for &t in input {
            self.write_code(encoding[t as usize]);
        }
        self.write_code(encoding[END_BLOCK_MARKER as usize]);
    }
}
const TABLE_BITS: u32 = 14;
const TABLE_SIZE: usize = 1 << TABLE_BITS;
const TABLE_MASK: u32 = (TABLE_SIZE - 1) as u32;
const TABLE_SHIFT: u32 = 32 - TABLE_BITS;
const BUFFER_RESET: i32 = i32::MAX - (MAX_STORE_BLOCK_SIZE as i32) * 2;
const INPUT_MARGIN: i32 = 16 - 1;
const MIN_NON_LITERAL_BLOCK_SIZE: usize = 1 + 1 + INPUT_MARGIN as usize;
fn load32(b: &[u8], i: i32) -> u32 {
    let i = i as usize;
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn load64(b: &[u8], i: i32) -> u64 {
    let i = i as usize;
    let mut v = [0u8; 8];
    v.copy_from_slice(&b[i..i + 8]);
    u64::from_le_bytes(v)
}
fn fast_hash(u: u32) -> u32 {
    u.wrapping_mul(0x1e35a7bd) >> TABLE_SHIFT
}
#[derive(Clone, Copy, Default)]
struct TableEntry {
    val: u32,
    offset: i32,
}
struct DeflateFast {
    table: Vec<TableEntry>,
    prev: Vec<u8>,
    cur: i32,
}
fn emit_literal(dst: &mut Vec<Token>, lit: &[u8]) {
    dst.extend(lit.iter().map(|&v| literal_token(u32::from(v))));
}
impl DeflateFast {
    fn new() -> Self {
        DeflateFast {
            table: vec![TableEntry::default(); TABLE_SIZE],
            prev: Vec::with_capacity(MAX_STORE_BLOCK_SIZE),
            cur: MAX_STORE_BLOCK_SIZE as i32,
        }
    }
    fn encode(&mut self, dst: &mut Vec<Token>, src: &[u8]) {
        if self.cur >= BUFFER_RESET {
            self.shift_offsets();
        }
        if src.len() < MIN_NON_LITERAL_BLOCK_SIZE {
            self.cur = self.cur.wrapping_add(MAX_STORE_BLOCK_SIZE as i32);
            self.prev.clear();
            emit_literal(dst, src);
            return;
        }
        let s_limit = src.len() as i32 - INPUT_MARGIN;
        let mut next_emit: i32 = 0;
        let mut s: i32 = 0;
        let mut cv = load32(src, s);
        let mut next_hash = fast_hash(cv);
        'outer: loop {
            let mut skip: i32 = 32;
            let mut next_s = s;
            let mut candidate;
            loop {
                s = next_s;
                let between = skip >> 5;
                next_s = s + between;
                skip += between;
                if next_s > s_limit {
                    break 'outer;
                }
                candidate = self.table[(next_hash & TABLE_MASK) as usize];
                let now = load32(src, next_s);
                self.table[(next_hash & TABLE_MASK) as usize] = TableEntry {
                    offset: s.wrapping_add(self.cur),
                    val: cv,
                };
                next_hash = fast_hash(now);
                let offset = s.wrapping_sub(candidate.offset.wrapping_sub(self.cur));
                if offset > MAX_MATCH_OFFSET as i32 || cv != candidate.val {
                    cv = now;
                    continue;
                }
                break;
            }
            emit_literal(dst, &src[next_emit as usize..s as usize]);
            loop {
                s += 4;
                let t = candidate.offset.wrapping_sub(self.cur) + 4;
                let l = self.match_len(s, t, src);
                dst.push(match_token(
                    (l + 4 - BASE_MATCH_LENGTH as i32) as u32,
                    (s - t - BASE_MATCH_OFFSET as i32) as u32,
                ));
                s += l;
                next_emit = s;
                if s >= s_limit {
                    break 'outer;
                }
                let mut x = load64(src, s - 1);
                let prev_hash = fast_hash(x as u32);
                self.table[(prev_hash & TABLE_MASK) as usize] = TableEntry {
                    offset: self.cur.wrapping_add(s) - 1,
                    val: x as u32,
                };
                x >>= 8;
                let curr_hash = fast_hash(x as u32);
                candidate = self.table[(curr_hash & TABLE_MASK) as usize];
                self.table[(curr_hash & TABLE_MASK) as usize] = TableEntry {
                    offset: self.cur.wrapping_add(s),
                    val: x as u32,
                };
                let offset = s.wrapping_sub(candidate.offset.wrapping_sub(self.cur));
                if offset > MAX_MATCH_OFFSET as i32 || x as u32 != candidate.val {
                    cv = (x >> 8) as u32;
                    next_hash = fast_hash(cv);
                    s += 1;
                    break;
                }
            }
        }
        if (next_emit as usize) < src.len() {
            emit_literal(dst, &src[next_emit as usize..]);
        }
        self.cur = self.cur.wrapping_add(src.len() as i32);
        self.prev.clear();
        self.prev.extend_from_slice(src);
    }
    fn match_len(&self, s: i32, t: i32, src: &[u8]) -> i32 {
        let s1 = (s as usize + MAX_MATCH_LENGTH as usize - 4).min(src.len());
        let s = s as usize;
        if t >= 0 {
            let t = t as usize;
            let a = &src[s..s1];
            let b = &src[t..t + a.len()];
            return a.iter().zip(b).take_while(|(x, y)| x == y).count() as i32;
        }
        let tp = self.prev.len() as i32 + t;
        if tp < 0 {
            return 0;
        }
        let a = &src[s..s1];
        let mut b = &self.prev[tp as usize..];
        if b.len() > a.len() {
            b = &b[..a.len()];
        }
        let a = &a[..b.len()];
        if let Some(i) = a.iter().zip(b).position(|(x, y)| x != y) {
            return i as i32;
        }
        let n = b.len();
        if s + n == s1 {
            return n as i32;
        }
        let a = &src[s + n..s1];
        let b = &src[..a.len()];
        match a.iter().zip(b).position(|(x, y)| x != y) {
            Some(i) => (i + n) as i32,
            None => (a.len() + n) as i32,
        }
    }
    fn shift_offsets(&mut self) {
        if self.prev.is_empty() {
            self.table.fill(TableEntry::default());
            self.cur = MAX_MATCH_OFFSET as i32 + 1;
            return;
        }
        for e in self.table.iter_mut() {
            let v = e
                .offset
                .wrapping_sub(self.cur)
                .wrapping_add(MAX_MATCH_OFFSET as i32 + 1);
            e.offset = v.max(0);
        }
        self.cur = MAX_MATCH_OFFSET as i32 + 1;
    }
    fn reset(&mut self) {
        self.prev.clear();
        self.cur = self.cur.wrapping_add(MAX_MATCH_OFFSET as i32);
        if self.cur >= BUFFER_RESET {
            self.shift_offsets();
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Store,
    Huff,
    Speed,
    Deflate,
}
pub struct Writer {
    cl: CompressionLevel,
    mode: Mode,
    w: HuffmanBitWriter,
    best_speed: Option<Box<DeflateFast>>,
    index: i64,
    window: Vec<u8>,
    window_end: i64,
    block_start: i64,
    byte_available: bool,
    sync: bool,
    tokens: Vec<Token>,
    length: i64,
    offset: i64,
    max_insert_index: i64,
    closed: bool,
    chain_head: i64,
    hash_head: Vec<u32>,
    hash_prev: Vec<u32>,
    hash_offset: i64,
}
fn hash4(b: &[u8]) -> u32 {
    (u32::from_be_bytes([b[0], b[1], b[2], b[3]]).wrapping_mul(HASHMUL)) >> (32 - HASH_BITS)
}
fn match_len(a: &[u8], b: &[u8], max: usize) -> i64 {
    let a = &a[..max];
    let b = &b[..max];
    a.iter().zip(b).take_while(|(x, y)| x == y).count() as i64
}
impl Writer {
    pub fn new(level: i32) -> Result<Writer, String> {
        let mut d = Writer {
            cl: CompressionLevel::default(),
            mode: Mode::Store,
            w: HuffmanBitWriter::new(),
            best_speed: None,
            index: 0,
            window: Vec::new(),
            window_end: 0,
            block_start: 0,
            byte_available: false,
            sync: false,
            tokens: Vec::new(),
            length: 0,
            offset: 0,
            max_insert_index: 0,
            closed: false,
            chain_head: 0,
            hash_head: Vec::new(),
            hash_prev: Vec::new(),
            hash_offset: 0,
        };
        match level {
            NO_COMPRESSION => {
                d.window = vec![0; MAX_STORE_BLOCK_SIZE];
                d.mode = Mode::Store;
            }
            HUFFMAN_ONLY => {
                d.window = vec![0; MAX_STORE_BLOCK_SIZE];
                d.mode = Mode::Huff;
            }
            BEST_SPEED => {
                d.cl = LEVELS[1];
                d.window = vec![0; MAX_STORE_BLOCK_SIZE];
                d.mode = Mode::Speed;
                d.best_speed = Some(Box::new(DeflateFast::new()));
            }
            DEFAULT_COMPRESSION | 2..=9 => {
                let level = if level == DEFAULT_COMPRESSION {
                    6
                } else {
                    level
                };
                d.cl = LEVELS[level as usize];
                d.window = vec![0; 2 * WINDOW_SIZE];
                d.hash_offset = 1;
                d.tokens = Vec::with_capacity(MAX_FLATE_BLOCK_TOKENS + 1);
                d.length = MIN_MATCH_LENGTH - 1;
                d.chain_head = -1;
                d.hash_head = vec![0; HASH_SIZE];
                d.hash_prev = vec![0; WINDOW_SIZE];
                d.mode = Mode::Deflate;
            }
            _ => {
                return Err(format!(
                    "flate: invalid compression level {level}: want value in range [-2, 9]"
                ));
            }
        }
        Ok(d)
    }
    fn step(&mut self) {
        match self.mode {
            Mode::Store => self.store(),
            Mode::Huff => self.store_huff(),
            Mode::Speed => self.enc_speed(),
            Mode::Deflate => self.deflate(),
        }
    }
    fn fill(&mut self, b: &[u8]) -> usize {
        if self.mode == Mode::Deflate {
            self.fill_deflate(b)
        } else {
            self.fill_store(b)
        }
    }
    fn fill_deflate(&mut self, b: &[u8]) -> usize {
        let ws = WINDOW_SIZE as i64;
        if self.index >= 2 * ws - (MIN_MATCH_LENGTH + MAX_MATCH_LENGTH) {
            self.window.copy_within(WINDOW_SIZE..2 * WINDOW_SIZE, 0);
            self.index -= ws;
            self.window_end -= ws;
            if self.block_start >= ws {
                self.block_start -= ws;
            } else {
                self.block_start = i64::from(i32::MAX);
            }
            self.hash_offset += ws;
            if self.hash_offset > MAX_HASH_OFFSET {
                let delta = self.hash_offset - 1;
                self.hash_offset -= delta;
                self.chain_head -= delta;
                for v in self.hash_prev.iter_mut().chain(self.hash_head.iter_mut()) {
                    *v = if i64::from(*v) > delta {
                        (i64::from(*v) - delta) as u32
                    } else {
                        0
                    };
                }
            }
        }
        self.fill_store(b)
    }
    fn fill_store(&mut self, b: &[u8]) -> usize {
        let end = self.window_end as usize;
        let n = b.len().min(self.window.len() - end);
        self.window[end..end + n].copy_from_slice(&b[..n]);
        self.window_end += n as i64;
        n
    }
    fn write_block(&mut self, index: i64) {
        if index > 0 {
            let window = if self.block_start <= index {
                Some(&self.window[self.block_start as usize..index as usize])
            } else {
                None
            };
            self.block_start = index;
            self.w.write_block(&self.tokens, false, window);
        }
    }
    fn write_stored_block(&mut self, end: usize) {
        self.w.write_stored_header(end, false);
        self.w.write_bytes(&self.window[..end]);
    }
    fn find_match(
        &self,
        pos: i64,
        prev_head: i64,
        prev_length: i64,
        lookahead: i64,
    ) -> (i64, i64, bool) {
        let min_match_look = MAX_MATCH_LENGTH.min(lookahead);
        let win = &self.window[..(pos + min_match_look) as usize];
        let nice = (win.len() as i64 - pos).min(self.cl.nice);
        let mut tries = self.cl.chain;
        let mut length = prev_length;
        let mut offset = 0;
        let mut ok = false;
        if length >= self.cl.good {
            tries >>= 2;
        }
        let p = pos as usize;
        let mut w_end = win[p + length as usize];
        let w_pos = &win[p..];
        let min_index = pos - WINDOW_SIZE as i64;
        let mut i = prev_head;
        while tries > 0 {
            if w_end == win[(i + length) as usize] {
                let n = match_len(&win[i as usize..], w_pos, min_match_look as usize);
                if n > length && (n > MIN_MATCH_LENGTH || pos - i <= 4096) {
                    length = n;
                    offset = pos - i;
                    ok = true;
                    if n >= nice {
                        break;
                    }
                    w_end = win[p + n as usize];
                }
            }
            if i == min_index {
                break;
            }
            i = i64::from(self.hash_prev[i as usize & WINDOW_MASK]) - self.hash_offset;
            if i < min_index || i < 0 {
                break;
            }
            tries -= 1;
        }
        (length, offset, ok)
    }
    fn enc_speed(&mut self) {
        let end = self.window_end as usize;
        if end < MAX_STORE_BLOCK_SIZE {
            if !self.sync {
                return;
            }
            if end < 128 {
                match end {
                    0 => return,
                    1..=16 => self.write_stored_block(end),
                    _ => self.w.write_block_huff(false, &self.window[..end]),
                }
                self.window_end = 0;
                if let Some(bs) = self.best_speed.as_mut() {
                    bs.reset();
                }
                return;
            }
        }
        self.tokens.clear();
        if let Some(bs) = self.best_speed.as_mut() {
            bs.encode(&mut self.tokens, &self.window[..end]);
        }
        if self.tokens.len() > end - (end >> 4) {
            self.w.write_block_huff(false, &self.window[..end]);
        } else {
            self.w
                .write_block_dynamic(&self.tokens, false, Some(&self.window[..end]));
        }
        self.window_end = 0;
    }
    fn insert_hash(&mut self, index: i64) {
        let i = index as usize;
        let hash = hash4(&self.window[i..i + MIN_MATCH_LENGTH as usize]);
        let hh = (hash & HASH_MASK) as usize;
        self.hash_prev[i & WINDOW_MASK] = self.hash_head[hh];
        self.hash_head[hh] = (index + self.hash_offset) as u32;
    }
    fn deflate(&mut self) {
        let ws = WINDOW_SIZE as i64;
        if self.window_end - self.index < MIN_MATCH_LENGTH + MAX_MATCH_LENGTH && !self.sync {
            return;
        }
        self.max_insert_index = self.window_end - (MIN_MATCH_LENGTH - 1);
        let skip_never = self.cl.fast_skip_hashing == SKIP_NEVER;
        loop {
            assert!(self.index <= self.window_end, "index > windowEnd");
            let lookahead = self.window_end - self.index;
            if lookahead < MIN_MATCH_LENGTH + MAX_MATCH_LENGTH {
                if !self.sync {
                    break;
                }
                if lookahead == 0 {
                    if self.byte_available {
                        let lit = self.window[self.index as usize - 1];
                        self.tokens.push(literal_token(u32::from(lit)));
                        self.byte_available = false;
                    }
                    if !self.tokens.is_empty() {
                        self.write_block(self.index);
                        self.tokens.clear();
                    }
                    break;
                }
            }
            if self.index < self.max_insert_index {
                let i = self.index as usize;
                let hash = hash4(&self.window[i..i + MIN_MATCH_LENGTH as usize]);
                let hh = (hash & HASH_MASK) as usize;
                self.chain_head = i64::from(self.hash_head[hh]);
                self.hash_prev[i & WINDOW_MASK] = self.chain_head as u32;
                self.hash_head[hh] = (self.index + self.hash_offset) as u32;
            }
            let prev_length = self.length;
            let prev_offset = self.offset;
            self.length = MIN_MATCH_LENGTH - 1;
            self.offset = 0;
            let min_index = (self.index - ws).max(0);
            if self.chain_head - self.hash_offset >= min_index
                && (!skip_never && lookahead > MIN_MATCH_LENGTH - 1
                    || skip_never && lookahead > prev_length && prev_length < self.cl.lazy)
            {
                let (new_length, new_offset, ok) = self.find_match(
                    self.index,
                    self.chain_head - self.hash_offset,
                    MIN_MATCH_LENGTH - 1,
                    lookahead,
                );
                if ok {
                    self.length = new_length;
                    self.offset = new_offset;
                }
            }
            if !skip_never && self.length >= MIN_MATCH_LENGTH
                || skip_never && prev_length >= MIN_MATCH_LENGTH && self.length <= prev_length
            {
                let token = if !skip_never {
                    match_token(
                        (self.length - BASE_MATCH_LENGTH) as u32,
                        (self.offset - BASE_MATCH_OFFSET) as u32,
                    )
                } else {
                    match_token(
                        (prev_length - BASE_MATCH_LENGTH) as u32,
                        (prev_offset - BASE_MATCH_OFFSET) as u32,
                    )
                };
                self.tokens.push(token);
                if self.length <= self.cl.fast_skip_hashing {
                    let new_index = if !skip_never {
                        self.index + self.length
                    } else {
                        self.index + prev_length - 1
                    };
                    let mut index = self.index + 1;
                    while index < new_index {
                        if index < self.max_insert_index {
                            self.insert_hash(index);
                        }
                        index += 1;
                    }
                    self.index = index;
                    if skip_never {
                        self.byte_available = false;
                        self.length = MIN_MATCH_LENGTH - 1;
                    }
                } else {
                    self.index += self.length;
                }
                if self.tokens.len() == MAX_FLATE_BLOCK_TOKENS {
                    self.write_block(self.index);
                    self.tokens.clear();
                }
            } else {
                if !skip_never || self.byte_available {
                    let i = if !skip_never {
                        self.index
                    } else {
                        self.index - 1
                    };
                    self.tokens
                        .push(literal_token(u32::from(self.window[i as usize])));
                    if self.tokens.len() == MAX_FLATE_BLOCK_TOKENS {
                        self.write_block(i + 1);
                        self.tokens.clear();
                    }
                }
                self.index += 1;
                if skip_never {
                    self.byte_available = true;
                }
            }
        }
    }
    fn store(&mut self) {
        let end = self.window_end as usize;
        if end > 0 && (end == MAX_STORE_BLOCK_SIZE || self.sync) {
            self.write_stored_block(end);
            self.window_end = 0;
        }
    }
    fn store_huff(&mut self) {
        let end = self.window_end as usize;
        if end < self.window.len() && !self.sync || end == 0 {
            return;
        }
        self.w.write_block_huff(false, &self.window[..end]);
        self.window_end = 0;
    }
    pub fn write(&mut self, mut b: &[u8]) -> Result<usize, String> {
        if self.closed {
            return Err("flate: closed writer".to_string());
        }
        let n = b.len();
        while !b.is_empty() {
            self.step();
            let k = self.fill(b);
            b = &b[k..];
        }
        Ok(n)
    }
    pub fn flush(&mut self) -> Result<(), String> {
        if self.closed {
            return Err("flate: closed writer".to_string());
        }
        self.sync = true;
        self.step();
        self.w.write_stored_header(0, false);
        self.w.flush();
        self.sync = false;
        Ok(())
    }
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.sync = true;
        self.step();
        self.w.write_stored_header(0, true);
        self.w.flush();
        self.closed = true;
    }
    pub fn output(&self) -> &[u8] {
        &self.w.out
    }
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.w.out)
    }
}
pub fn compress_chunks(data: &[u8], ends: &[usize], level: i32) -> Result<Vec<u8>, String> {
    let mut w = Writer::new(level)?;
    let mut start = 0;
    for &end in ends {
        if end > start && end <= data.len() {
            w.write(&data[start..end])?;
            start = end;
        }
    }
    if start < data.len() {
        w.write(&data[start..])?;
    }
    w.close();
    Ok(w.take_output())
}
