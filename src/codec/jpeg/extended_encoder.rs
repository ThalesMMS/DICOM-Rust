#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
use super::encoder::{EncodedFrame, EncoderCapabilities, baseline_quantization, unsupported};
use super::extended::{ZIGZAG, cos_table};
use super::{Error, ErrorKind, MAX_QUALITY, MIN_QUALITY, Metadata, UID_EXTENDED, normalized};
use crate::fmath;
use std::f64::consts::SQRT_2;
pub(super) const LUMINANCE_QUANTIZATION: [i64; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81, 104, 113,
    92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
];
const DEFAULT_WORKING_BYTES: i64 = 256 << 20;
const DEFAULT_ENCODED_BYTES: i64 = 128 << 20;
pub const PROFILE_COLOR: &str = "jpeg-extended-color";
pub const PROFILE_REDUCED_PRECISION: &str = "jpeg-extended-reduced-precision";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtendedEncoderOptions {
    pub quality: i64,
    pub allow_color_compatibility: bool,
    pub allow_legacy_reduced_precision: bool,
    pub max_working_bytes: i64,
    pub max_encoded_frame_bytes: i64,
}
#[derive(Debug, Clone)]
pub struct ExtendedEncoder {
    quality: i64,
    options: ExtendedEncoderOptions,
}
impl ExtendedEncoder {
    pub fn new(quality: i64) -> Result<ExtendedEncoder, Error> {
        ExtendedEncoder::with_options(ExtendedEncoderOptions {
            quality,
            allow_legacy_reduced_precision: true,
            ..Default::default()
        })
    }
    pub fn with_options(mut options: ExtendedEncoderOptions) -> Result<ExtendedEncoder, Error> {
        if options.quality < MIN_QUALITY || options.quality > MAX_QUALITY {
            return Err(Error::bare(ErrorKind::InvalidQuality));
        }
        if options.max_working_bytes < 0 || options.max_encoded_frame_bytes < 0 {
            return Err(Error::bare(ErrorKind::InvalidExtendedEncoderLimits));
        }
        if options.max_working_bytes == 0 {
            options.max_working_bytes = DEFAULT_WORKING_BYTES;
        }
        if options.max_encoded_frame_bytes == 0 {
            options.max_encoded_frame_bytes = DEFAULT_ENCODED_BYTES;
        }
        Ok(ExtendedEncoder {
            quality: options.quality,
            options,
        })
    }
    pub fn quality(&self) -> i64 {
        self.quality
    }
    pub fn capabilities(&self) -> EncoderCapabilities {
        let mut caps = EncoderCapabilities {
            transfer_syntax_uid: UID_EXTENDED,
            bits_allocated: vec![8, 16],
            samples_per_pixel: vec![1],
            photometric_interpretations: vec!["MONOCHROME1", "MONOCHROME2"],
            output_photometric_interpretations: Vec::new(),
            backend: "pure-rust",
            compatibility_profiles: Vec::new(),
        };
        if self.options.allow_color_compatibility {
            caps.samples_per_pixel.push(3);
            caps.photometric_interpretations.extend(["RGB", "YBR_FULL"]);
            caps.compatibility_profiles.push(PROFILE_COLOR);
        }
        if self.options.allow_legacy_reduced_precision {
            caps.compatibility_profiles.push(PROFILE_REDUCED_PRECISION);
        }
        caps
    }
    fn validate_input(&self, m: &Metadata) -> Result<(&'static str, usize), Error> {
        if m.rows == 0 {
            return Err(unsupported("Rows"));
        }
        if m.columns == 0 {
            return Err(unsupported("Columns"));
        }
        if m.number_of_frames < 1 {
            return Err(unsupported("NumberOfFrames"));
        }
        if m.pixel_representation != 0 {
            return Err(unsupported("PixelRepresentation"));
        }
        let mut precision = 12;
        if m.bits_allocated == 8 {
            precision = 8;
            if m.bits_stored != 8 {
                return Err(unsupported("BitsStored"));
            }
        } else if m.bits_allocated != 16 {
            return Err(unsupported("BitsAllocated"));
        } else if m.bits_stored < 9 || m.bits_stored > 12 {
            return Err(unsupported("BitsStored"));
        }
        if m.high_bit != m.bits_stored.wrapping_sub(1) {
            return Err(unsupported("HighBit"));
        }
        let pi = normalized(&m.photometric_interpretation);
        if m.samples_per_pixel == 1 {
            if pi != "MONOCHROME1" && pi != "MONOCHROME2" {
                return Err(unsupported("PhotometricInterpretation"));
            }
            if m.planar_configuration_present {
                return Err(unsupported("PlanarConfiguration"));
            }
            if precision == 12 && m.bits_stored != 12 {
                if !self.options.allow_legacy_reduced_precision {
                    return Err(unsupported("BitsStored"));
                }
                return Ok((PROFILE_REDUCED_PRECISION, precision));
            }
            return Ok(("", precision));
        }
        if m.samples_per_pixel != 3 {
            return Err(unsupported("SamplesPerPixel"));
        }
        if !self.options.allow_color_compatibility {
            return Err(unsupported("ColorCompatibility"));
        }
        if pi != "RGB" && !(precision == 12 && pi == "YBR_FULL") {
            return Err(unsupported("PhotometricInterpretation"));
        }
        if m.bits_stored as usize != precision {
            return Err(unsupported("BitsStored"));
        }
        if !m.planar_configuration_present || m.planar_configuration != 0 {
            return Err(unsupported("PlanarConfiguration"));
        }
        Ok((PROFILE_COLOR, precision))
    }
    pub fn encode_frame(&self, frame: &[u8], m: &Metadata) -> Result<EncodedFrame, Error> {
        if self.quality < MIN_QUALITY || self.quality > MAX_QUALITY {
            return Err(Error::bare(ErrorKind::InvalidQuality));
        }
        let (profile, precision) = self.validate_input(m)?;
        let bytes_per_sample = super::bytes_per_sample(m) as usize;
        let frame_length =
            m.rows as u64 * m.columns as u64 * m.samples_per_pixel as u64 * bytes_per_sample as u64;
        if frame.len() as u64 != frame_length {
            return Err(Error::new(
                ErrorKind::PixelDataSizeMismatch,
                "native frame length",
            ));
        }
        let (width, height, components) = (
            m.columns as usize,
            m.rows as usize,
            m.samples_per_pixel as usize,
        );
        let (blocks_x, blocks_y) = (width.div_ceil(8), height.div_ceil(8));
        let block_count = blocks_x as u64 * blocks_y as u64 * components as u64;
        let encoded_capacity = block_count * 512 + 4096;
        let working_bytes = frame_length + block_count * 128 + encoded_capacity + 65536;
        let max_int = i64::MAX as u64;
        if encoded_capacity > self.options.max_encoded_frame_bytes as u64
            || encoded_capacity > max_int
        {
            return Err(Error::new(
                ErrorKind::ExtendedEncoderResourceLimit,
                "encoded_frame_bytes",
            ));
        }
        if working_bytes > self.options.max_working_bytes as u64 || block_count * 64 > max_int {
            return Err(Error::new(
                ErrorKind::ExtendedEncoderResourceLimit,
                "working_bytes",
            ));
        }
        let mut blocks = vec![0i16; block_count as usize * 64];
        let quant = if precision == 8 {
            baseline_quantization(&LUMINANCE_QUANTIZATION, self.quality)
        } else {
            quantization_table(self.quality)
        };
        let mask: u16 = 1u16
            .checked_shl(m.bits_stored as u32)
            .unwrap_or(0)
            .wrapping_sub(1);
        let center = 1i64 << (precision - 1);
        let mut dc_frequencies = [0usize; 256];
        let mut ac_frequencies = [0usize; 256];
        let mut predictors = [0i64; 3];
        for by in 0..blocks_y {
            for bx in 0..blocks_x {
                for (c, predictor) in predictors.iter_mut().enumerate().take(components) {
                    let mut block = [0f64; 64];
                    for y in 0..8 {
                        let row = (by * 8 + y).min(height - 1) * width;
                        for x in 0..8 {
                            let offset = ((row + (bx * 8 + x).min(width - 1)) * components + c)
                                * bytes_per_sample;
                            let value = if bytes_per_sample == 2 {
                                u16::from_le_bytes([frame[offset], frame[offset + 1]])
                            } else {
                                frame[offset] as u16
                            };
                            block[y * 8 + x] = ((value & mask) as i64 - center) as f64;
                        }
                    }
                    let coefficients = forward_dct(&block);
                    let index = ((by * blocks_x + bx) * components + c) * 64;
                    let zigzag = &mut blocks[index..index + 64];
                    for zig in 0..64 {
                        let natural = ZIGZAG[zig];
                        zigzag[zig] = fmath::f64_to_i32(
                            (coefficients[natural] / quant[natural] as f64).round(),
                        ) as i16;
                    }
                    let difference = zigzag[0] as i64 - *predictor;
                    *predictor = zigzag[0] as i64;
                    dc_frequencies[magnitude_category(difference)] += 1;
                    for_each_ac_symbol(zigzag, |symbol, _| ac_frequencies[symbol as usize] += 1);
                }
            }
        }
        let dc_table = optimal_huffman_table(&dc_frequencies);
        let ac_table = optimal_huffman_table(&ac_frequencies);
        let mut out = Vec::with_capacity(encoded_capacity as usize);
        out.extend_from_slice(&[0xff, 0xd8]);
        let mut dqt = vec![0u8];
        if quant.iter().any(|&v| v > 255) {
            dqt[0] = 0x10;
        }
        for &natural in ZIGZAG.iter() {
            let value = quant[natural];
            if dqt[0] == 0 {
                dqt.push(value as u8);
            } else {
                dqt.extend_from_slice(&(value as u16).to_be_bytes());
            }
        }
        append_segment(&mut out, 0xdb, &dqt);
        let mut sof = vec![
            precision as u8,
            (height >> 8) as u8,
            height as u8,
            (width >> 8) as u8,
            width as u8,
            components as u8,
        ];
        let mut sos = vec![components as u8];
        let mut ids = [1u8, 2, 3];
        if components == 3 && normalized(&m.photometric_interpretation) == "RGB" {
            ids = *b"RGB";
        }
        for &id in &ids[..components] {
            sof.extend_from_slice(&[id, 0x11, 0]);
            sos.extend_from_slice(&[id, 0]);
        }
        sos.extend_from_slice(&[0, 63, 0]);
        append_segment(&mut out, 0xc1, &sof);
        let mut dht = vec![0u8];
        dht.extend_from_slice(&dc_table.counts[1..]);
        dht.extend_from_slice(&dc_table.symbols);
        dht.push(0x10);
        dht.extend_from_slice(&ac_table.counts[1..]);
        dht.extend_from_slice(&ac_table.symbols);
        append_segment(&mut out, 0xc4, &dht);
        append_segment(&mut out, 0xda, &sos);
        let (dc_codes, ac_codes) = (dc_table.codes(), ac_table.codes());
        let mut writer = BitWriter::new(out);
        let mut predictors = [0i64; 3];
        for i in 0..block_count as usize {
            let c = i % components;
            let zigzag = &blocks[i * 64..i * 64 + 64];
            let difference = zigzag[0] as i64 - predictors[c];
            predictors[c] = zigzag[0] as i64;
            let category = magnitude_category(difference);
            writer.write(dc_codes.code[category], dc_codes.length[category]);
            writer.write_additional_bits(difference, category);
            for_each_ac_symbol(zigzag, |symbol, value| {
                writer.write(
                    ac_codes.code[symbol as usize],
                    ac_codes.length[symbol as usize],
                );
                writer.write_additional_bits(value, (symbol & 0x0f) as usize);
            });
        }
        let mut out = writer.flush();
        out.extend_from_slice(&[0xff, 0xd9]);
        if out.len() as u64 > encoded_capacity {
            return Err(Error::bare(ErrorKind::EncoderOutputInvalid));
        }
        let mut result = EncodedFrame {
            data: out,
            compatibility_profile: profile,
            ..Default::default()
        };
        if components == 3 {
            result.planar_configuration = Some(0);
            result.photometric_interpretation = normalized(&m.photometric_interpretation);
        }
        Ok(result)
    }
}
fn quantization_table(quality: i64) -> [i64; 64] {
    let scale = if quality < 50 {
        5000 / quality
    } else {
        200 - 2 * quality
    };
    let mut table = [0i64; 64];
    for (t, &base) in table.iter_mut().zip(LUMINANCE_QUANTIZATION.iter()) {
        *t = ((base * scale + 50) / 100).clamp(1, 32767);
    }
    table
}
fn forward_dct(block: &[f64; 64]) -> [f64; 64] {
    let cos = cos_table();
    let mut rows = [0f64; 64];
    let mut out = [0f64; 64];
    for y in 0..8 {
        for u in 0..8 {
            let mut sum = 0.0;
            for x in 0..8 {
                sum += block[y * 8 + x] * cos[u][x];
            }
            rows[y * 8 + u] = sum;
        }
    }
    for v in 0..8 {
        for u in 0..8 {
            let mut sum = 0.0;
            for y in 0..8 {
                sum += rows[y * 8 + u] * cos[v][y];
            }
            let mut scale = 0.25;
            if u == 0 {
                scale /= SQRT_2;
            }
            if v == 0 {
                scale /= SQRT_2;
            }
            out[v * 8 + u] = sum * scale;
        }
    }
    out
}
pub(super) fn for_each_ac_symbol(zigzag: &[i16], mut emit: impl FnMut(u8, i64)) {
    let mut run = 0;
    for &v in &zigzag[1..64] {
        let value = v as i64;
        if value == 0 {
            run += 1;
            continue;
        }
        while run > 15 {
            emit(0xf0, 0);
            run -= 16;
        }
        emit(((run << 4) | magnitude_category(value)) as u8, value);
        run = 0;
    }
    if run > 0 {
        emit(0x00, 0);
    }
}
pub(super) fn magnitude_category(value: i64) -> usize {
    (64 - value.unsigned_abs().leading_zeros()) as usize
}
pub(super) fn append_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    let len = payload.len() + 2;
    out.extend_from_slice(&[0xff, marker, (len >> 8) as u8, len as u8]);
    out.extend_from_slice(payload);
}
#[derive(Debug, Clone, Default)]
pub(super) struct HuffmanSpec {
    pub counts: [u8; 17],
    pub symbols: Vec<u8>,
}
pub(super) struct HuffmanCodes {
    pub code: [u32; 256],
    pub length: [usize; 256],
}
pub(super) fn optimal_huffman_table(frequencies: &[usize; 256]) -> HuffmanSpec {
    const RESERVED: usize = 256;
    let mut freq = [0usize; 257];
    freq[..256].copy_from_slice(frequencies);
    freq[RESERVED] = 1;
    let mut codesize = [0usize; 257];
    let mut others = [-1isize; 257];
    loop {
        let mut v1: isize = -1;
        let mut v2: isize = -1;
        for (v, &f) in freq.iter().enumerate() {
            if f > 0 && (v1 < 0 || f <= freq[v1 as usize]) {
                v1 = v as isize;
            }
        }
        for (v, &f) in freq.iter().enumerate() {
            if f > 0 && v as isize != v1 && (v2 < 0 || f <= freq[v2 as usize]) {
                v2 = v as isize;
            }
        }
        if v2 < 0 {
            break;
        }
        let (mut v1, mut v2) = (v1 as usize, v2 as usize);
        freq[v1] += freq[v2];
        freq[v2] = 0;
        codesize[v1] += 1;
        while others[v1] >= 0 {
            v1 = others[v1] as usize;
            codesize[v1] += 1;
        }
        others[v1] = v2 as isize;
        codesize[v2] += 1;
        while others[v2] >= 0 {
            v2 = others[v2] as usize;
            codesize[v2] += 1;
        }
    }
    let mut bits = [0usize; 257];
    for &size in &codesize {
        if size > 0 {
            bits[size] += 1;
        }
    }
    let mut i = bits.len() - 1;
    while i > 16 {
        while bits[i] > 0 {
            let mut j = i - 2;
            while bits[j] == 0 {
                j -= 1;
            }
            bits[i] -= 2;
            bits[i - 1] += 1;
            bits[j + 1] += 2;
            bits[j] -= 1;
        }
        i -= 1;
    }
    for i in (1..=16).rev() {
        if bits[i] > 0 {
            bits[i] -= 1;
            break;
        }
    }
    let mut spec = HuffmanSpec::default();
    for i in 1..=16 {
        spec.counts[i] = bits[i] as u8;
    }
    for size in 1..bits.len() {
        for (symbol, &cs) in codesize.iter().enumerate().take(RESERVED) {
            if cs == size {
                spec.symbols.push(symbol as u8);
            }
        }
    }
    spec
}
impl HuffmanSpec {
    pub fn codes(&self) -> HuffmanCodes {
        let mut out = HuffmanCodes {
            code: [0; 256],
            length: [0; 256],
        };
        let mut code = 0u32;
        let mut k = 0;
        for length in 1..=16 {
            for _ in 0..self.counts[length] {
                let symbol = self.symbols[k] as usize;
                out.code[symbol] = code;
                out.length[symbol] = length;
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        out
    }
}
pub(super) struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    count: usize,
}
impl BitWriter {
    pub fn new(out: Vec<u8>) -> BitWriter {
        BitWriter {
            out,
            acc: 0,
            count: 0,
        }
    }
    pub fn write(&mut self, value: u32, length: usize) {
        if length == 0 {
            return;
        }
        self.acc = self.acc.wrapping_shl(length as u32) | (value & ((1u32 << length) - 1)) as u64;
        self.count += length;
        while self.count >= 8 {
            let b = (self.acc >> (self.count - 8)) as u8;
            self.out.push(b);
            if b == 0xff {
                self.out.push(0);
            }
            self.count -= 8;
        }
    }
    pub(crate) fn write_additional_bits(&mut self, mut value: i64, category: usize) {
        if category == 0 {
            return;
        }
        if value < 0 {
            value -= 1;
        }
        self.write((value as u32) & ((1u32 << category) - 1), category);
    }
    pub fn flush(mut self) -> Vec<u8> {
        if self.count > 0 {
            let n = 8 - self.count;
            self.write((1 << n) - 1, n);
        }
        self.out
    }
}
