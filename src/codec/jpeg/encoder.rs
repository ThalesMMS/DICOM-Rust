#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
use super::extended::{ZIGZAG, cos_table};
use super::extended_encoder::{
    BitWriter, HuffmanCodes, HuffmanSpec, LUMINANCE_QUANTIZATION, append_segment,
    for_each_ac_symbol, magnitude_category, optimal_huffman_table,
};
use super::stdjpeg;
use super::{Error, ErrorKind, MAX_QUALITY, MIN_QUALITY, Metadata, UID, normalized};
use crate::fmath;
use std::f64::consts::SQRT_2;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Subsampling(pub u8);
impl Subsampling {
    #[doc(hidden)]
    pub const S420: Subsampling = Subsampling(0);
    #[doc(hidden)]
    pub const S422: Subsampling = Subsampling(1);
    pub const S444: Subsampling = Subsampling(2);
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EncoderOptions {
    pub quality: i64,
    pub subsampling: Subsampling,
    pub optimize_huffman: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub photometric_interpretation: String,
    pub planar_configuration: Option<u16>,
    pub compatibility_profile: &'static str,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderCapabilities {
    pub transfer_syntax_uid: &'static str,
    pub bits_allocated: Vec<u16>,
    pub samples_per_pixel: Vec<u16>,
    pub photometric_interpretations: Vec<&'static str>,
    pub output_photometric_interpretations: Vec<&'static str>,
    pub backend: &'static str,
    pub compatibility_profiles: Vec<&'static str>,
}
#[derive(Debug, Clone)]
pub struct Encoder {
    quality: i64,
    subsampling: Subsampling,
    optimize_huffman: bool,
}
pub(super) fn unsupported(field: &str) -> Error {
    Error::new(ErrorKind::UnsupportedEncoderMetadata, field)
}
const CHROMINANCE_QUANTIZATION: [i64; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99, 18, 21, 26, 66, 99, 99, 99, 99, 24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99,
];
pub(super) fn baseline_quantization(base: &[i64; 64], quality: i64) -> [i64; 64] {
    let scale = if quality < 50 {
        5000 / quality
    } else {
        200 - 2 * quality
    };
    let mut table = [0i64; 64];
    for (t, &value) in table.iter_mut().zip(base.iter()) {
        *t = ((value * scale + 50) / 100).clamp(1, 255);
    }
    table
}
impl Encoder {
    pub fn new(quality: i64) -> Result<Encoder, Error> {
        Encoder::with_options(EncoderOptions {
            quality,
            ..Default::default()
        })
    }
    pub fn with_options(options: EncoderOptions) -> Result<Encoder, Error> {
        if options.quality < MIN_QUALITY || options.quality > MAX_QUALITY {
            return Err(Error::bare(ErrorKind::InvalidQuality));
        }
        if options.subsampling.0 > Subsampling::S444.0 {
            return Err(Error::bare(ErrorKind::InvalidSubsampling));
        }
        Ok(Encoder {
            quality: options.quality,
            subsampling: options.subsampling,
            optimize_huffman: options.optimize_huffman || options.subsampling != Subsampling::S420,
        })
    }
    pub fn quality(&self) -> i64 {
        self.quality
    }
    pub fn subsampling(&self) -> Subsampling {
        self.subsampling
    }
    pub fn optimizes_huffman(&self) -> bool {
        self.optimize_huffman
    }
    pub fn capabilities(&self) -> EncoderCapabilities {
        EncoderCapabilities {
            transfer_syntax_uid: UID,
            bits_allocated: vec![8],
            samples_per_pixel: vec![1, 3],
            photometric_interpretations: vec!["MONOCHROME1", "MONOCHROME2", "RGB"],
            output_photometric_interpretations: vec!["YBR_FULL_422"],
            backend: if self.optimize_huffman {
                "pure-rust"
            } else {
                "stdjpeg"
            },
            compatibility_profiles: Vec::new(),
        }
    }
    pub fn encode_frame(&self, frame: &[u8], m: &Metadata) -> Result<EncodedFrame, Error> {
        if self.quality < MIN_QUALITY || self.quality > MAX_QUALITY {
            return Err(Error::bare(ErrorKind::InvalidQuality));
        }
        if self.subsampling.0 > Subsampling::S444.0 {
            return Err(Error::bare(ErrorKind::InvalidSubsampling));
        }
        let frame_length = validate_input(m)?;
        if frame.len() as u64 != frame_length {
            return Err(Error::new(
                ErrorKind::PixelDataSizeMismatch,
                "native frame length",
            ));
        }
        if self.optimize_huffman {
            return self.encode_baseline(frame, m);
        }
        let (width, height) = (m.columns as usize, m.rows as usize);
        if m.samples_per_pixel == 1 {
            let data = stdjpeg::encode(stdjpeg::Source::Gray(frame), width, height, self.quality);
            return Ok(EncodedFrame {
                data,
                ..Default::default()
            });
        }
        let data = stdjpeg::encode(stdjpeg::Source::Rgb(frame), width, height, self.quality);
        Ok(EncodedFrame {
            data,
            photometric_interpretation: "YBR_FULL_422".to_string(),
            planar_configuration: Some(0),
            compatibility_profile: "",
        })
    }
    fn encode_baseline(&self, frame: &[u8], m: &Metadata) -> Result<EncodedFrame, Error> {
        let (width, height) = (m.columns as usize, m.rows as usize);
        let components = m.samples_per_pixel as usize;
        let (mut h, mut v) = (1usize, 1usize);
        if components == 3 {
            match self.subsampling {
                Subsampling::S420 => (h, v) = (2, 2),
                Subsampling::S422 => h = 2,
                _ => {}
            }
        }
        let mcus_x = width.div_ceil(8 * h);
        let mcus_y = height.div_ceil(8 * v);
        let mut blocks_per_mcu = h * v;
        if components == 3 {
            blocks_per_mcu += 2;
        }
        let quant = [
            baseline_quantization(&LUMINANCE_QUANTIZATION, self.quality),
            baseline_quantization(&CHROMINANCE_QUANTIZATION, self.quality),
        ];
        let mut inverse = [[0f64; 64]; 2];
        for t in 0..2 {
            for k in 0..64 {
                inverse[t][k] = 1.0 / quant[t][k] as f64;
            }
        }
        let coefficient_count = mcus_x as u64 * mcus_y as u64 * blocks_per_mcu as u64 * 64;
        if coefficient_count > (i64::MAX as u64) / 2 {
            return Err(Error::new(
                ErrorKind::EncoderOutputInvalid,
                "JPEG coefficient buffer size",
            ));
        }
        let mut blocks = vec![0i16; coefficient_count as usize];
        let mut dc_frequency = [[0usize; 256]; 2];
        let mut ac_frequency = [[0usize; 256]; 2];
        let mut predictor = [0i64; 3];
        let mut planes = [[0f64; 256]; 3];
        let mut index = 0;
        for my in 0..mcus_y {
            for mx in 0..mcus_x {
                if components == 3 {
                    color_mcu(frame, width, height, mx, my, h, v, &mut planes);
                }
                for c in 0..components {
                    let (bh, bv, table) = if c > 0 { (1, 1, 1) } else { (h, v, 0) };
                    for by in 0..bv {
                        for bx in 0..bh {
                            let mut block = [0f64; 64];
                            if components == 1 {
                                block = sample_block(frame, width, height, mx, my);
                            } else {
                                let stride = 8 * bh;
                                for y in 0..8 {
                                    let start = (by * 8 + y) * stride + bx * 8;
                                    block[y * 8..y * 8 + 8]
                                        .copy_from_slice(&planes[c][start..start + 8]);
                                }
                            }
                            let coefficients = forward_dct(&block);
                            let zigzag = &mut blocks[index..index + 64];
                            index += 64;
                            for (zig, &natural) in ZIGZAG.iter().enumerate() {
                                zigzag[zig] = fmath::f64_to_i32(
                                    (coefficients[natural] * inverse[table][natural]).round(),
                                ) as i16;
                            }
                            let delta = zigzag[0] as i64 - predictor[c];
                            predictor[c] = zigzag[0] as i64;
                            dc_frequency[table][magnitude_category(delta)] += 1;
                            for_each_ac_symbol(zigzag, |symbol, _| {
                                ac_frequency[table][symbol as usize] += 1
                            });
                        }
                    }
                }
            }
        }
        let tables = if components == 3 { 2 } else { 1 };
        let mut dc: Vec<HuffmanSpec> = Vec::new();
        let mut ac: Vec<HuffmanSpec> = Vec::new();
        let mut dc_codes: Vec<HuffmanCodes> = Vec::new();
        let mut ac_codes: Vec<HuffmanCodes> = Vec::new();
        for i in 0..tables {
            dc.push(optimal_huffman_table(&dc_frequency[i]));
            ac.push(optimal_huffman_table(&ac_frequency[i]));
            dc_codes.push(dc[i].codes());
            ac_codes.push(ac[i].codes());
        }
        let mut out = Vec::with_capacity(frame.len() / 4 + 512);
        out.extend_from_slice(&[0xff, 0xd8]);
        let mut dqt = Vec::new();
        let mut dht = Vec::new();
        for i in 0..tables {
            dqt.push(i as u8);
            for &natural in ZIGZAG.iter() {
                dqt.push(quant[i][natural] as u8);
            }
            dht.push(i as u8);
            dht.extend_from_slice(&dc[i].counts[1..]);
            dht.extend_from_slice(&dc[i].symbols);
            dht.push(0x10 | i as u8);
            dht.extend_from_slice(&ac[i].counts[1..]);
            dht.extend_from_slice(&ac[i].symbols);
        }
        append_segment(&mut out, 0xdb, &dqt);
        let mut sof = vec![
            8,
            (height >> 8) as u8,
            height as u8,
            (width >> 8) as u8,
            width as u8,
            components as u8,
            1,
            ((h << 4) | v) as u8,
            0,
        ];
        let mut sos = vec![components as u8, 1, 0];
        if components == 3 {
            sof.extend_from_slice(&[2, 0x11, 1, 3, 0x11, 1]);
            sos.extend_from_slice(&[2, 0x11, 3, 0x11]);
        }
        sos.extend_from_slice(&[0, 63, 0]);
        append_segment(&mut out, 0xc0, &sof);
        append_segment(&mut out, 0xc4, &dht);
        append_segment(&mut out, 0xda, &sos);
        let mut writer = BitWriter::new(out);
        let mut predictor = [0i64; 3];
        let mut index = 0;
        for _ in 0..mcus_x * mcus_y {
            for (c, predictor) in predictor.iter_mut().enumerate().take(components) {
                let (count, table) = if c > 0 { (1, 1) } else { (h * v, 0) };
                for _ in 0..count {
                    let zigzag = &blocks[index..index + 64];
                    index += 64;
                    let delta = zigzag[0] as i64 - *predictor;
                    *predictor = zigzag[0] as i64;
                    let category = magnitude_category(delta);
                    writer.write(
                        dc_codes[table].code[category],
                        dc_codes[table].length[category],
                    );
                    writer.write_additional_bits(delta, category);
                    for_each_ac_symbol(zigzag, |symbol, value| {
                        writer.write(
                            ac_codes[table].code[symbol as usize],
                            ac_codes[table].length[symbol as usize],
                        );
                        writer.write_additional_bits(value, (symbol & 15) as usize);
                    });
                }
            }
        }
        let mut out = writer.flush();
        out.extend_from_slice(&[0xff, 0xd9]);
        let mut encoded = EncodedFrame {
            data: out,
            ..Default::default()
        };
        if components == 3 {
            encoded.photometric_interpretation = "YBR_FULL_422".to_string();
            encoded.planar_configuration = Some(0);
        }
        Ok(encoded)
    }
}
fn validate_input(m: &Metadata) -> Result<u64, Error> {
    if m.rows == 0 {
        return Err(unsupported("Rows"));
    }
    if m.columns == 0 {
        return Err(unsupported("Columns"));
    }
    if m.bits_allocated != 8 {
        return Err(unsupported("BitsAllocated"));
    }
    if m.bits_stored != 8 {
        return Err(unsupported("BitsStored"));
    }
    if m.high_bit != 7 {
        return Err(unsupported("HighBit"));
    }
    if m.pixel_representation != 0 {
        return Err(unsupported("PixelRepresentation"));
    }
    if m.number_of_frames < 1 {
        return Err(unsupported("NumberOfFrames"));
    }
    let photometric = normalized(&m.photometric_interpretation);
    match m.samples_per_pixel {
        1 => {
            if photometric != "MONOCHROME1" && photometric != "MONOCHROME2" {
                return Err(unsupported("PhotometricInterpretation"));
            }
            if m.planar_configuration_present {
                return Err(unsupported("PlanarConfiguration"));
            }
        }
        3 => {
            if photometric != "RGB" {
                return Err(unsupported("PhotometricInterpretation"));
            }
            if !m.planar_configuration_present || m.planar_configuration != 0 {
                return Err(unsupported("PlanarConfiguration"));
            }
        }
        _ => return Err(unsupported("SamplesPerPixel")),
    }
    Ok(m.rows as u64 * m.columns as u64 * m.samples_per_pixel as u64)
}
fn sample_block(frame: &[u8], width: usize, height: usize, mx: usize, my: usize) -> [f64; 64] {
    let mut block = [0f64; 64];
    for y in 0..8 {
        for x in 0..8 {
            let offset = (my * 8 + y).min(height - 1) * width + (mx * 8 + x).min(width - 1);
            block[y * 8 + x] = (frame[offset] as i64 - 128) as f64;
        }
    }
    block
}
#[allow(clippy::too_many_arguments)]
fn color_mcu(
    frame: &[u8],
    width: usize,
    height: usize,
    mx: usize,
    my: usize,
    h: usize,
    v: usize,
    planes: &mut [[f64; 256]; 3],
) {
    *planes = [[0.0; 256]; 3];
    let stride = 8 * h;
    for y in 0..8 * v {
        let row = (my * 8 * v + y).min(height - 1) * width;
        for x in 0..stride {
            let offset = (row + (mx * stride + x).min(width - 1)) * 3;
            let (yy, cb, cr) =
                stdjpeg::image::rgb_to_ycbcr(frame[offset], frame[offset + 1], frame[offset + 2]);
            planes[0][y * stride + x] = (yy as i64 - 128) as f64;
            let chroma = (y / v) * 8 + x / h;
            planes[1][chroma] += cb as f64;
            planes[2][chroma] += cr as f64;
        }
    }
    let n = (h * v) as i64;
    for plane in planes.iter_mut().skip(1) {
        for value in plane.iter_mut().take(64) {
            *value = ((*value as i64 + n / 2) / n - 128) as f64;
        }
    }
}
fn forward_dct(block: &[f64; 64]) -> [f64; 64] {
    let mut rows = [0f64; 64];
    let mut out = [0f64; 64];
    for y in 0..8 {
        let mut s = [0f64; 8];
        s.copy_from_slice(&block[y * 8..y * 8 + 8]);
        rows[y * 8..y * 8 + 8].copy_from_slice(&dct_1d(&s));
    }
    for u in 0..8 {
        let column = [
            rows[u],
            rows[8 + u],
            rows[16 + u],
            rows[24 + u],
            rows[32 + u],
            rows[40 + u],
            rows[48 + u],
            rows[56 + u],
        ];
        let transformed = dct_1d(&column);
        for (v, value) in transformed.iter().enumerate() {
            out[v * 8 + u] = 0.25 * value;
        }
    }
    out
}
fn dct_1d(s: &[f64; 8]) -> [f64; 8] {
    let cos = cos_table();
    let (a, b, c, d) = (s[0] + s[7], s[1] + s[6], s[2] + s[5], s[3] + s[4]);
    let (e, f, g, h) = (s[0] - s[7], s[1] - s[6], s[2] - s[5], s[3] - s[4]);
    let (c1, c2, c3, c5, c6, c7) = (
        cos[1][0], cos[2][0], cos[3][0], cos[5][0], cos[6][0], cos[7][0],
    );
    [
        (a + b + c + d) / SQRT_2,
        e * c1 + f * c3 + g * c5 + h * c7,
        (a - d) * c2 + (b - c) * c6,
        e * c3 - f * c7 - g * c1 - h * c5,
        (a - b - c + d) / SQRT_2,
        e * c5 - f * c1 + g * c7 + h * c3,
        (a - d) * c6 - (b - c) * c2,
        e * c7 - f * c5 + g * c3 - h * c1,
    ]
}
