use super::super::codestream::{
    BLOCK_BYPASS, BLOCK_RESET, BLOCK_SEG_SYM, BLOCK_TERM_ALL, BLOCK_VSC,
};
use super::super::geometry::CodeBlock;
use super::super::packet::{CodeBlockData, Segment, raw_pass};
use super::{decode_block, reference};
struct Random(u64);
impl Random {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        let kind = self.below(4);
        (0..n)
            .map(|_| match kind {
                0 => (self.next() >> 8) as u8,
                1 => {
                    if self.below(4) == 0 {
                        (self.next() >> 8) as u8
                    } else {
                        0
                    }
                }
                2 => {
                    if self.below(3) == 0 {
                        0xFF
                    } else {
                        (self.next() >> 8) as u8
                    }
                }
                _ => (self.next() >> 8) as u8 & 0x7F,
            })
            .collect()
    }
}
fn random_block(r: &mut Random) -> (CodeBlockData, i64) {
    let (w, h) = match r.below(8) {
        0 => (64, 64),
        1 => (1 + r.below(64) as i64, 1 + r.below(64) as i64),
        2 => (1 + r.below(3) as i64, 1 + r.below(40) as i64),
        3 => (1 + r.below(40) as i64, 1 + r.below(3) as i64),
        _ => (1 + r.below(17) as i64, 1 + r.below(17) as i64),
    };
    let mut style = 0u8;
    if r.below(2) == 0 {
        for bit in [
            BLOCK_BYPASS,
            BLOCK_RESET,
            BLOCK_TERM_ALL,
            BLOCK_VSC,
            BLOCK_SEG_SYM,
        ] {
            if r.below(3) == 0 {
                style |= bit;
            }
        }
    }
    let most = if r.below(6) == 0 { 31 } else { 14 };
    let magnitude_bits = 1 + r.below(most) as i64;
    let zero_bit_planes = r.below(magnitude_bits as u32 + 1) as i64;
    let zero_bit_planes = zero_bit_planes - r.below(2) as i64;
    let planes = magnitude_bits - zero_bit_planes;
    let limit = (3 * planes - 2).max(1);
    let passes = match r.below(10) {
        0 => limit + 1 + r.below(3) as i64,
        1 => 0,
        _ => 1 + r.below(limit as u32) as i64,
    };
    let mut runs: Vec<i64> = Vec::new();
    if r.below(5) == 0 {
        let mut left = passes;
        while left > 0 {
            let take = r.below(4) as i64;
            let take = take.min(left);
            runs.push(take);
            left -= take.max(1);
        }
        if r.below(4) == 0 {
            runs.push(1 + r.below(3) as i64);
        }
    } else if style & BLOCK_TERM_ALL != 0 {
        runs = vec![1; passes as usize];
    } else if style & BLOCK_BYPASS != 0 {
        let mut pass = 0;
        while pass < passes {
            let raw = raw_pass(pass, style);
            let mut end = pass + 1;
            while end < passes && raw_pass(end, style) == raw {
                end += 1;
            }
            runs.push(end - pass);
            pass = end;
        }
    } else {
        runs = vec![passes];
    }
    if r.below(12) == 0 {
        runs.pop();
    }
    let segments = runs
        .into_iter()
        .map(|passes| {
            let length = match r.below(6) {
                0 => 0,
                1 => r.below(4) as usize,
                _ => {
                    let per_pass = r.below(40) as usize + 1;
                    per_pass * passes.max(1) as usize
                }
            };
            Segment {
                data: r.bytes(length),
                passes,
                placeholder: false,
            }
        })
        .collect();
    let block = CodeBlockData {
        block: CodeBlock {
            x0: 0,
            y0: 0,
            x1: w,
            y1: h,
            index: 0,
        },
        band_type: r.below(4) as i64,
        resolution: 1,
        component: 0,
        zero_bit_planes,
        passes,
        segments,
        style,
    };
    (block, magnitude_bits)
}
#[test]
fn the_fast_decoder_gives_the_plain_decoders_coefficients() {
    let mut r = Random(0x5EED_0001);
    let (mut decoded, mut refused, mut significant) = (0u32, 0u32, 0u64);
    for case in 0..30_000 {
        let (block, magnitude_bits) = random_block(&mut r);
        let expected = reference::decode_block(&block, magnitude_bits);
        let got = decode_block(&block, magnitude_bits);
        assert_eq!(
            got,
            expected,
            "case {case}: {}x{} style {:#04x} band {} Mb {magnitude_bits} zero planes {} passes {} segments {:?}",
            block.block.width(),
            block.block.height(),
            block.style,
            block.band_type,
            block.zero_bit_planes,
            block.passes,
            block
                .segments
                .iter()
                .map(|s| (s.passes, s.data.len()))
                .collect::<Vec<_>>(),
        );
        match got {
            Ok(values) => {
                decoded += 1;
                significant += values.iter().filter(|&&v| v != 0).count() as u64;
            }
            Err(_) => refused += 1,
        }
    }
    assert!(decoded > 20_000, "{decoded} decoded");
    assert!(refused > 500, "{refused} refused");
    assert!(significant > 1_000_000, "{significant} significant");
}
#[test]
fn the_mq_decoder_gives_the_plain_decoders_decisions() {
    use super::super::mq::{Contexts, Mq, SEGMENT_SENTINEL};
    let mut r = Random(0x5EED_0002);
    for case in 0..2_000 {
        let length = r.below(60) as usize;
        let data = r.bytes(length);
        let mut plain = reference::mq::MqDecoder::new(&data, 19);
        plain.set_context(18, 46, 0);
        plain.set_context(17, 3, 0);
        plain.set_context(0, 4, 0);
        plain.start(&data);
        let mut buf = data.clone();
        buf.extend_from_slice(&SEGMENT_SENTINEL);
        let mut fast = Mq::new();
        let mut contexts = Contexts::new();
        contexts.set(18, 46, 0);
        contexts.set(17, 3, 0);
        contexts.set(0, 4, 0);
        fast.start(&buf);
        for n in 0..(8 * length + 64) {
            let cx = r.below(19) as usize;
            let got = if r.below(2) == 0 {
                fast.decode(&mut contexts, &buf, cx)
            } else {
                fast.decode_even(&mut contexts, &buf, cx)
            };
            assert_eq!(got, plain.decode(cx) as u32, "case {case}, decision {n}");
        }
    }
}
