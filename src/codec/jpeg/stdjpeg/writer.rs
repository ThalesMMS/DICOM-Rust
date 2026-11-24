#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
use super::dct::{Block, fdct};
use super::image::rgb_to_ycbcr;
use super::reader::UNZIG;
fn div(a: i32, b: i32) -> i32 {
    if a >= 0 {
        (a + (b >> 1)) / b
    } else {
        -((-a + (b >> 1)) / b)
    }
}
fn bit_count(a: u32) -> u32 {
    32 - a.leading_zeros()
}
const UNSCALED_QUANT: [[u8; 64]; 2] = [
    [
        16, 11, 12, 14, 12, 10, 16, 14, 13, 14, 18, 17, 16, 19, 24, 40, 26, 24, 22, 22, 24, 49, 35,
        37, 29, 40, 58, 51, 61, 60, 57, 51, 56, 55, 64, 72, 92, 78, 64, 68, 87, 69, 55, 56, 80,
        109, 81, 87, 95, 98, 103, 104, 103, 62, 77, 113, 121, 112, 100, 120, 92, 101, 103, 99,
    ],
    [
        17, 18, 18, 24, 21, 24, 47, 26, 26, 47, 99, 66, 56, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99,
        99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
        99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    ],
];
struct HuffmanSpec {
    count: [u8; 16],
    value: &'static [u8],
}
const LUMA_AC: [u8; 162] = [
    0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61, 0x07,
    0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52, 0xd1, 0xf0,
    0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25, 0x26, 0x27, 0x28,
    0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
    0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
    0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89,
    0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7,
    0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3, 0xc4, 0xc5,
    0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xe1, 0xe2,
    0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
const CHROMA_AC: [u8; 162] = [
    0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61, 0x71,
    0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33, 0x52, 0xf0,
    0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18, 0x19, 0x1a, 0x26,
    0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48,
    0x49, 0x4a, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
    0x69, 0x6a, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87,
    0x88, 0x89, 0x8a, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0xa2, 0xa3, 0xa4, 0xa5,
    0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xc2, 0xc3,
    0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda,
    0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8,
    0xf9, 0xfa,
];
const DC_VALUES: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
const SPECS: [HuffmanSpec; 4] = [
    HuffmanSpec {
        count: [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        value: &DC_VALUES,
    },
    HuffmanSpec {
        count: [0, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 125],
        value: &LUMA_AC,
    },
    HuffmanSpec {
        count: [0, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0],
        value: &DC_VALUES,
    },
    HuffmanSpec {
        count: [0, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 119],
        value: &CHROMA_AC,
    },
];
fn lut(s: &HuffmanSpec) -> Vec<u32> {
    let max_value = s.value.iter().copied().max().unwrap_or(0) as usize;
    let mut h = vec![0u32; max_value + 1];
    let mut code = 0u32;
    let mut k = 0;
    for (i, &count) in s.count.iter().enumerate() {
        let n_bits = ((i + 1) as u32) << 24;
        for _ in 0..count {
            h[s.value[k] as usize] = n_bits | code;
            code += 1;
            k += 1;
        }
        code <<= 1;
    }
    h
}
pub(crate) enum Source<'a> {
    Gray(&'a [u8]),
    Rgb(&'a [u8]),
}
struct Encoder {
    out: Vec<u8>,
    bits: u32,
    n_bits: u32,
    quant: [[u8; 64]; 2],
    luts: [Vec<u32>; 4],
}
impl Encoder {
    fn emit(&mut self, bits: u32, n_bits: u32) {
        let n_bits = n_bits + self.n_bits;
        let mut bits = bits.checked_shl(32 - n_bits).unwrap_or(0);
        bits |= self.bits;
        let mut n = n_bits;
        while n >= 8 {
            let b = (bits >> 24) as u8;
            self.out.push(b);
            if b == 0xff {
                self.out.push(0);
            }
            bits <<= 8;
            n -= 8;
        }
        self.bits = bits;
        self.n_bits = n;
    }
    fn emit_huff(&mut self, h: usize, value: i32) {
        let x = self.luts[h][value as usize];
        self.emit(x & ((1 << 24) - 1), x >> 24);
    }
    fn emit_huff_rle(&mut self, h: usize, run_length: i32, value: i32) {
        let (mut a, mut b) = (value, value);
        if a < 0 {
            a = -value;
            b = value - 1;
        }
        let n_bits = if a < 0x100 {
            bit_count(a as u32)
        } else {
            8 + bit_count((a >> 8) as u32)
        };
        self.emit_huff(h, (run_length << 4) | n_bits as i32);
        if n_bits > 0 {
            self.emit((b as u32) & ((1 << n_bits) - 1), n_bits);
        }
    }
    fn write_marker_header(&mut self, marker: u8, len: usize) {
        self.out
            .extend_from_slice(&[0xff, marker, (len >> 8) as u8, (len & 0xff) as u8]);
    }
    fn write_block(&mut self, b: &mut Block, q: usize, prev_dc: i32) -> i32 {
        fdct(b);
        let dc = div(b[0], 8 * self.quant[q][0] as i32);
        self.emit_huff_rle(2 * q, 0, dc - prev_dc);
        let h = 2 * q + 1;
        let mut run_length = 0;
        for zig in 1..64 {
            let ac = div(b[UNZIG[zig]], 8 * self.quant[q][zig] as i32);
            if ac == 0 {
                run_length += 1;
            } else {
                while run_length > 15 {
                    self.emit_huff(h, 0xf0);
                    run_length -= 16;
                }
                self.emit_huff_rle(h, run_length, ac);
                run_length = 0;
            }
        }
        if run_length > 0 {
            self.emit_huff(h, 0x00);
        }
        dc
    }
}
fn scale(dst: &mut Block, src: &[Block; 4]) {
    for (i, s) in src.iter().enumerate() {
        let dst_off = ((i & 2) << 4) | ((i & 1) << 2);
        for y in 0..4 {
            for x in 0..4 {
                let j = 16 * y + 2 * x;
                let sum = s[j] + s[j + 1] + s[j + 8] + s[j + 9];
                dst[8 * y + x + dst_off] = (sum + 2) >> 2;
            }
        }
    }
}
pub(crate) fn encode(source: Source<'_>, width: usize, height: usize, quality: i64) -> Vec<u8> {
    let quality = quality.clamp(1, 100);
    let scale_factor = if quality < 50 {
        5000 / quality
    } else {
        200 - quality * 2
    };
    let mut quant = [[0u8; 64]; 2];
    for (i, table) in quant.iter_mut().enumerate() {
        for (j, q) in table.iter_mut().enumerate() {
            let x = (UNSCALED_QUANT[i][j] as i64 * scale_factor + 50) / 100;
            *q = x.clamp(1, 255) as u8;
        }
    }
    let mut e = Encoder {
        out: Vec::new(),
        bits: 0,
        n_bits: 0,
        quant,
        luts: [
            lut(&SPECS[0]),
            lut(&SPECS[1]),
            lut(&SPECS[2]),
            lut(&SPECS[3]),
        ],
    };
    let n_component = match source {
        Source::Gray(_) => 1,
        Source::Rgb(_) => 3,
    };
    e.out.extend_from_slice(&[0xff, 0xd8]);
    e.write_marker_header(0xdb, 2 + 2 * 65);
    for i in 0..2 {
        e.out.push(i as u8);
        let table = e.quant[i];
        e.out.extend_from_slice(&table);
    }
    e.write_marker_header(0xc0, 8 + 3 * n_component);
    e.out.extend_from_slice(&[
        8,
        (height >> 8) as u8,
        (height & 0xff) as u8,
        (width >> 8) as u8,
        (width & 0xff) as u8,
        n_component as u8,
    ]);
    if n_component == 1 {
        e.out.extend_from_slice(&[1, 0x11, 0x00]);
    } else {
        for i in 0..3 {
            e.out
                .extend_from_slice(&[(i + 1) as u8, [0x22, 0x11, 0x11][i], [0, 1, 1][i]]);
        }
    }
    let specs = if n_component == 1 {
        &SPECS[..2]
    } else {
        &SPECS[..]
    };
    let len = 2 + specs.iter().map(|s| 17 + s.value.len()).sum::<usize>();
    e.write_marker_header(0xc4, len);
    for (i, s) in specs.iter().enumerate() {
        e.out.push([0x00, 0x10, 0x01, 0x11][i]);
        e.out.extend_from_slice(&s.count);
        e.out.extend_from_slice(s.value);
    }
    match source {
        Source::Gray(pix) => {
            e.out
                .extend_from_slice(&[0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
            let mut b: Block = [0; 64];
            let mut prev_dc = 0;
            let (xmax, ymax) = (width.saturating_sub(1), height.saturating_sub(1));
            for y in (0..height).step_by(8) {
                for x in (0..width).step_by(8) {
                    for j in 0..8 {
                        for i in 0..8 {
                            b[8 * j + i] =
                                pix[(y + j).min(ymax) * width + (x + i).min(xmax)] as i32;
                        }
                    }
                    prev_dc = e.write_block(&mut b, 0, prev_dc);
                }
            }
        }
        Source::Rgb(pix) => {
            e.out.extend_from_slice(&[
                0xff, 0xda, 0x00, 0x0c, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3f, 0x00,
            ]);
            let mut b: Block = [0; 64];
            let mut cb: [Block; 4] = [[0; 64]; 4];
            let mut cr: [Block; 4] = [[0; 64]; 4];
            let (mut prev_y, mut prev_cb, mut prev_cr) = (0, 0, 0);
            let (xmax, ymax) = (width.saturating_sub(1), height.saturating_sub(1));
            for y in (0..height).step_by(16) {
                for x in (0..width).step_by(16) {
                    for i in 0..4 {
                        let x_off = (i & 1) * 8;
                        let y_off = (i & 2) * 4;
                        let (px, py) = (x + x_off, y + y_off);
                        for j in 0..8 {
                            let sj = (py + j).min(ymax);
                            for k in 0..8 {
                                let sx = (px + k).min(xmax);
                                let o = (sj * width + sx) * 3;
                                let (yy, ccb, ccr) = rgb_to_ycbcr(pix[o], pix[o + 1], pix[o + 2]);
                                b[8 * j + k] = yy as i32;
                                cb[i][8 * j + k] = ccb as i32;
                                cr[i][8 * j + k] = ccr as i32;
                            }
                        }
                        prev_y = e.write_block(&mut b, 0, prev_y);
                    }
                    scale(&mut b, &cb);
                    prev_cb = e.write_block(&mut b, 1, prev_cb);
                    scale(&mut b, &cr);
                    prev_cr = e.write_block(&mut b, 1, prev_cr);
                }
            }
        }
    }
    e.emit(0x7f, 7);
    e.out.extend_from_slice(&[0xff, 0xd9]);
    e.out
}
