use super::gzip::Terminal;
const FILE_MAGIC: u64 = 0x425a;
const BLOCK_MAGIC: u64 = 0x3141_5926_5359;
const FINAL_MAGIC: u64 = 0x1772_4538_5090;
const INVALID_NODE: u16 = 0xffff;
fn structural(text: &str) -> Terminal {
    Terminal::Other(format!("bzip2 data invalid: {text}"))
}
struct BitReader {
    data: Vec<u8>,
    pos: usize,
    n: u64,
    bits: u32,
    err: Option<Terminal>,
}
impl BitReader {
    fn read_byte(&mut self) -> Option<u8> {
        let byte = self.data.get(self.pos).copied();
        if byte.is_some() {
            self.pos += 1;
        }
        byte
    }
    fn read_bits64(&mut self, bits: u32) -> u64 {
        while bits > self.bits {
            let Some(byte) = self.read_byte() else {
                self.err = Some(Terminal::UnexpectedEof);
                return 0;
            };
            self.n <<= 8;
            self.n |= byte as u64;
            self.bits += 8;
        }
        let n = (self.n >> (self.bits - bits)) & ((1u64 << bits) - 1);
        self.bits -= bits;
        n
    }
    fn read_bits(&mut self, bits: u32) -> i64 {
        self.read_bits64(bits) as i64
    }
    fn read_bit(&mut self) -> bool {
        self.read_bits(1) != 0
    }
}
#[derive(Clone, Copy, Default)]
struct HuffmanNode {
    left: u16,
    right: u16,
    left_value: u16,
    right_value: u16,
}
#[derive(Clone, Default)]
struct HuffmanTree {
    nodes: Vec<HuffmanNode>,
    next_node: usize,
}
#[derive(Clone, Copy)]
struct HuffmanCode {
    code: u32,
    value: u16,
}
impl HuffmanTree {
    fn decode(&self, br: &mut BitReader) -> u16 {
        let mut node_index = 0u16;
        loop {
            let node = &self.nodes[node_index as usize];
            let bit = if br.bits > 0 {
                br.bits -= 1;
                ((br.n >> (br.bits & 63)) & 1) as u16
            } else {
                br.read_bits(1) as u16
            };
            node_index = if bit == 1 { node.left } else { node.right };
            if node_index == INVALID_NODE {
                return if bit == 1 {
                    node.left_value
                } else {
                    node.right_value
                };
            }
        }
    }
    fn new(lengths: &[u8]) -> Result<HuffmanTree, Terminal> {
        let mut pairs: Vec<(u16, u8)> = lengths
            .iter()
            .enumerate()
            .map(|(i, &length)| (i as u16, length))
            .collect();
        pairs.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        let mut code = 0u32;
        let mut length = 32u8;
        let mut codes = vec![HuffmanCode { code: 0, value: 0 }; lengths.len()];
        for i in (0..pairs.len()).rev() {
            if length > pairs[i].1 {
                length = pairs[i].1;
            }
            codes[i] = HuffmanCode {
                code,
                value: pairs[i].0,
            };
            code = code.wrapping_add(1u32 << (32 - length as u32));
        }
        codes.sort_by_key(|code| code.code);
        let mut tree = HuffmanTree {
            nodes: vec![HuffmanNode::default(); codes.len()],
            next_node: 0,
        };
        tree.build(&codes, 0)?;
        Ok(tree)
    }
    fn build(&mut self, codes: &[HuffmanCode], level: u32) -> Result<u16, Terminal> {
        let test = 1u32 << (31 - level);
        let first_right = codes
            .iter()
            .position(|code| code.code & test != 0)
            .unwrap_or(codes.len());
        let (left, right) = codes.split_at(first_right);
        if left.is_empty() || right.is_empty() {
            if codes.len() < 2 {
                return Err(structural("empty Huffman tree"));
            }
            if level == 31 {
                return Err(structural("equal symbols in Huffman tree"));
            }
            if left.is_empty() {
                return self.build(right, level + 1);
            }
            return self.build(left, level + 1);
        }
        let node_index = self.next_node;
        self.next_node += 1;
        if left.len() == 1 {
            self.nodes[node_index].left = INVALID_NODE;
            self.nodes[node_index].left_value = left[0].value;
        } else {
            let child = self.build(left, level + 1)?;
            self.nodes[node_index].left = child;
        }
        if right.len() == 1 {
            self.nodes[node_index].right = INVALID_NODE;
            self.nodes[node_index].right_value = right[0].value;
        } else {
            let child = self.build(right, level + 1)?;
            self.nodes[node_index].right = child;
        }
        Ok(node_index as u16)
    }
}
fn mtf_decode(m: &mut [u8], n: usize) -> u8 {
    let b = m[n];
    m.copy_within(0..n, 1);
    m[0] = b;
    b
}
fn crc_table() -> &'static [u32; 256] {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut crc = (i as u32) << 24;
            for _ in 0..8 {
                crc = if crc & 0x8000_0000 != 0 {
                    (crc << 1) ^ 0x04C1_1DB7
                } else {
                    crc << 1
                };
            }
            *slot = crc;
        }
        table
    })
}
fn update_crc(val: u32, b: &[u8]) -> u32 {
    let table = crc_table();
    let mut crc = !val;
    for &v in b {
        crc = table[((crc >> 24) as u8 ^ v) as usize] ^ (crc << 8);
    }
    !crc
}
pub struct Reader {
    br: BitReader,
    file_crc: u32,
    block_crc: u32,
    want_block_crc: u32,
    setup_done: bool,
    eof: bool,
    block_size: usize,
    c: [u32; 256],
    tt: Vec<u32>,
    t_pos: u32,
    pre_rle_len: usize,
    pre_rle_used: usize,
    last_byte: i32,
    byte_repeats: u32,
    repeats: u32,
}
impl Reader {
    pub fn new(data: Vec<u8>) -> Reader {
        Reader {
            br: BitReader {
                data,
                pos: 0,
                n: 0,
                bits: 0,
                err: None,
            },
            file_crc: 0,
            block_crc: 0,
            want_block_crc: 0,
            setup_done: false,
            eof: false,
            block_size: 0,
            c: [0; 256],
            tt: Vec::new(),
            t_pos: 0,
            pre_rle_len: 0,
            pre_rle_used: 0,
            last_byte: 0,
            byte_repeats: 0,
            repeats: 0,
        }
    }
    fn setup(&mut self, need_magic: bool) -> Result<(), Terminal> {
        if need_magic {
            let magic = self.br.read_bits(16);
            if magic as u64 != FILE_MAGIC {
                return Err(structural("bad magic value"));
            }
        }
        let t = self.br.read_bits(8);
        if t != b'h' as i64 {
            return Err(structural("non-Huffman entropy encoding"));
        }
        let level = self.br.read_bits(8);
        if !(b'1' as i64..=b'9' as i64).contains(&level) {
            return Err(structural("invalid compression level"));
        }
        self.file_crc = 0;
        self.block_size = 100 * 1000 * (level - b'0' as i64) as usize;
        if self.block_size > self.tt.len() {
            self.tt = vec![0; self.block_size];
        }
        Ok(())
    }
    pub fn read(&mut self, buf: &mut [u8]) -> (usize, Option<Terminal>) {
        if self.eof {
            return (0, Some(Terminal::Eof));
        }
        if !self.setup_done {
            let mut err = self.setup(true).err();
            if let Some(br_err) = &self.br.err {
                err = Some(br_err.clone());
            }
            if err.is_some() {
                return (0, err);
            }
            self.setup_done = true;
        }
        let (n, mut err) = self.read_inner(buf);
        if let Some(br_err) = &self.br.err {
            err = Some(br_err.clone());
        }
        (n, err)
    }
    fn read_from_block(&mut self, buf: &mut [u8]) -> usize {
        let mut n = 0;
        while (self.repeats > 0 || self.pre_rle_used < self.pre_rle_len) && n < buf.len() {
            if self.repeats > 0 {
                buf[n] = self.last_byte as u8;
                n += 1;
                self.repeats -= 1;
                if self.repeats == 0 {
                    self.last_byte = -1;
                }
                continue;
            }
            self.t_pos = self.tt[self.t_pos as usize];
            let b = self.t_pos as u8;
            self.t_pos >>= 8;
            self.pre_rle_used += 1;
            if self.byte_repeats == 3 {
                self.repeats = b as u32;
                self.byte_repeats = 0;
                continue;
            }
            if self.last_byte == b as i32 {
                self.byte_repeats += 1;
            } else {
                self.byte_repeats = 0;
            }
            self.last_byte = b as i32;
            buf[n] = b;
            n += 1;
        }
        n
    }
    fn read_inner(&mut self, buf: &mut [u8]) -> (usize, Option<Terminal>) {
        loop {
            let n = self.read_from_block(buf);
            if n > 0 || buf.is_empty() {
                self.block_crc = update_crc(self.block_crc, &buf[..n]);
                return (n, None);
            }
            if self.block_crc != self.want_block_crc {
                self.br.err = Some(structural("block checksum mismatch"));
                return (0, self.br.err.clone());
            }
            match self.br.read_bits64(48) {
                BLOCK_MAGIC => {
                    if let Err(err) = self.read_block() {
                        return (0, Some(err));
                    }
                }
                FINAL_MAGIC => {
                    let want_file_crc = self.br.read_bits64(32) as u32;
                    if self.br.err.is_some() {
                        return (0, self.br.err.clone());
                    }
                    if self.file_crc != want_file_crc {
                        self.br.err = Some(structural("file checksum mismatch"));
                        return (0, self.br.err.clone());
                    }
                    if !self.br.bits.is_multiple_of(8) {
                        self.br.read_bits(self.br.bits % 8);
                    }
                    let Some(b) = self.br.read_byte() else {
                        self.br.err = Some(Terminal::Eof);
                        self.eof = true;
                        return (0, Some(Terminal::Eof));
                    };
                    let Some(z) = self.br.read_byte() else {
                        self.br.err = Some(Terminal::UnexpectedEof);
                        return (0, self.br.err.clone());
                    };
                    if b != b'B' || z != b'Z' {
                        return (0, Some(structural("bad magic value in continuation file")));
                    }
                    if let Err(err) = self.setup(false) {
                        return (0, Some(err));
                    }
                }
                _ => return (0, Some(structural("bad magic value found"))),
            }
        }
    }
    fn read_block(&mut self) -> Result<(), Terminal> {
        self.want_block_crc = self.br.read_bits64(32) as u32;
        self.block_crc = 0;
        self.file_crc = self.file_crc.rotate_left(1) ^ self.want_block_crc;
        if self.br.read_bits(1) != 0 {
            return Err(structural("deprecated randomized files"));
        }
        let orig_ptr = self.br.read_bits(24) as usize;
        let symbol_range_used_bitmap = self.br.read_bits(16);
        let mut symbol_present = [false; 256];
        let mut num_symbols = 0usize;
        for sym_range in 0..16usize {
            if symbol_range_used_bitmap & (1 << (15 - sym_range)) != 0 {
                let bits = self.br.read_bits(16);
                for symbol in 0..16usize {
                    if bits & (1 << (15 - symbol)) != 0 {
                        symbol_present[16 * sym_range + symbol] = true;
                        num_symbols += 1;
                    }
                }
            }
        }
        if num_symbols == 0 {
            return Err(structural("no symbols in input"));
        }
        let num_huffman_trees = self.br.read_bits(3) as usize;
        if !(2..=6).contains(&num_huffman_trees) {
            return Err(structural("invalid number of Huffman trees"));
        }
        let num_selectors = self.br.read_bits(15) as usize;
        let mut tree_indexes = vec![0u8; num_selectors];
        let mut mtf_tree: Vec<u8> = (0..num_huffman_trees as u8).collect();
        for slot in tree_indexes.iter_mut() {
            let mut c = 0usize;
            while self.br.read_bits(1) != 0 {
                c += 1;
            }
            if c >= num_huffman_trees {
                return Err(structural("tree index too large"));
            }
            *slot = mtf_decode(&mut mtf_tree, c);
        }
        let mut mtf: Vec<u8> = (0..256usize)
            .filter(|&i| symbol_present[i])
            .map(|i| i as u8)
            .collect();
        num_symbols += 2;
        let mut huffman_trees = Vec::with_capacity(num_huffman_trees);
        let mut lengths = vec![0u8; num_symbols];
        for _ in 0..num_huffman_trees {
            let mut length = self.br.read_bits(5);
            for slot in lengths.iter_mut() {
                loop {
                    if !(1..=20).contains(&length) {
                        return Err(structural("Huffman length out of range"));
                    }
                    if !self.br.read_bit() {
                        break;
                    }
                    if self.br.read_bit() {
                        length -= 1;
                    } else {
                        length += 1;
                    }
                }
                *slot = length as u8;
            }
            huffman_trees.push(HuffmanTree::new(&lengths)?);
        }
        let mut selector_index = 1usize;
        if tree_indexes.is_empty() {
            return Err(structural("no tree selectors given"));
        }
        if tree_indexes[0] as usize >= huffman_trees.len() {
            return Err(structural("tree selector out of range"));
        }
        let mut current = tree_indexes[0] as usize;
        let mut buf_index = 0usize;
        let mut repeat = 0usize;
        let mut repeat_power = 0usize;
        self.c = [0; 256];
        let mut decoded = 0;
        loop {
            if decoded == 50 {
                if selector_index >= num_selectors {
                    return Err(structural(
                        "insufficient selector indices for number of symbols",
                    ));
                }
                if tree_indexes[selector_index] as usize >= huffman_trees.len() {
                    return Err(structural("tree selector out of range"));
                }
                current = tree_indexes[selector_index] as usize;
                selector_index += 1;
                decoded = 0;
            }
            let v = huffman_trees[current].decode(&mut self.br) as usize;
            decoded += 1;
            if v < 2 {
                if repeat == 0 {
                    repeat_power = 1;
                }
                repeat += repeat_power << v;
                repeat_power <<= 1;
                if repeat > 2 * 1024 * 1024 {
                    return Err(structural("repeat count too large"));
                }
                continue;
            }
            if repeat > 0 {
                if repeat > self.block_size - buf_index {
                    return Err(structural("repeats past end of block"));
                }
                let b = mtf[0];
                for _ in 0..repeat {
                    self.tt[buf_index] = b as u32;
                    self.c[b as usize] += 1;
                    buf_index += 1;
                }
                repeat = 0;
            }
            if v == num_symbols - 1 {
                break;
            }
            let b = mtf_decode(&mut mtf, v - 1);
            if buf_index >= self.block_size {
                return Err(structural("data exceeds block size"));
            }
            self.tt[buf_index] = b as u32;
            self.c[b as usize] += 1;
            buf_index += 1;
        }
        if orig_ptr >= buf_index {
            return Err(structural("origPtr out of bounds"));
        }
        self.pre_rle_len = buf_index;
        self.pre_rle_used = 0;
        self.t_pos = inverse_bwt(&mut self.tt[..buf_index], orig_ptr, &mut self.c);
        self.last_byte = -1;
        self.byte_repeats = 0;
        self.repeats = 0;
        Ok(())
    }
}
fn inverse_bwt(tt: &mut [u32], orig_ptr: usize, c: &mut [u32; 256]) -> u32 {
    let mut sum = 0u32;
    for slot in c.iter_mut() {
        sum += *slot;
        *slot = sum - *slot;
    }
    for i in 0..tt.len() {
        let b = (tt[i] & 0xff) as usize;
        tt[c[b] as usize] |= (i as u32) << 8;
        c[b] += 1;
    }
    tt[orig_ptr] >> 8
}
