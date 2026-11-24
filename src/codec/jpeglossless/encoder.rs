#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
use super::decoder::predict_neighbors;
use super::{Error, ErrorKind, Metadata, UID_PROCESS14, UID_PROCESS14_SV1, normalized_photometric};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encoder {
    uid: &'static str,
    selection_value: u8,
}
impl Default for Encoder {
    fn default() -> Self {
        Encoder::sv1()
    }
}
impl Encoder {
    pub fn sv1() -> Encoder {
        Encoder {
            uid: UID_PROCESS14_SV1,
            selection_value: 1,
        }
    }
    pub fn process14(selection_value: i64) -> Result<Encoder, Error> {
        if !(1..=7).contains(&selection_value) {
            return Err(Error::encoder_metadata("SelectionValue"));
        }
        Ok(Encoder {
            uid: UID_PROCESS14,
            selection_value: selection_value as u8,
        })
    }
    pub fn transfer_syntax(&self) -> &'static str {
        self.uid
    }
    pub fn encode_frame(&self, frame: &[u8], metadata: &Metadata) -> Result<Vec<u8>, Error> {
        validate_encoder_metadata(metadata)?;
        let width = metadata.columns as usize;
        let height = metadata.rows as usize;
        let components = metadata.samples_per_pixel as usize;
        let bytes_per_sample = (metadata.bits_allocated / 8) as usize;
        if frame.len() as u64
            != width as u64 * height as u64 * components as u64 * bytes_per_sample as u64
        {
            return Err(Error::new(
                ErrorKind::PixelDataSizeMismatch,
                "native frame length",
            ));
        }
        let precision = metadata.bits_stored as u32;
        let mask = (1i32 << precision) - 1;
        let predictor = self.selection_value as usize;
        let default_prediction = 1i32 << (precision - 1);
        let mut differences: Vec<Vec<u32>> = Vec::with_capacity(components);
        let mut frequencies = vec![[0usize; 17]; components];
        for (c, frequency) in frequencies.iter_mut().enumerate() {
            let mut diffs = vec![0u32; width * height];
            let mut above_row = if predictor != 1 {
                vec![0i32; width]
            } else {
                Vec::new()
            };
            let mut above_first = 0i32;
            for y in 0..height {
                let (mut left, mut above_left) = (0i32, 0i32);
                if predictor == 1 {
                    left = if y != 0 {
                        above_first
                    } else {
                        default_prediction
                    };
                }
                let mut offset = (y * width * components + c) * bytes_per_sample;
                for x in 0..width {
                    let mut value = if bytes_per_sample == 2 {
                        u16::from_le_bytes([frame[offset], frame[offset + 1]]) as i32
                    } else {
                        frame[offset] as i32
                    };
                    value &= mask;
                    let above = if predictor != 1 {
                        above_row[x]
                    } else {
                        above_first
                    };
                    let mut prediction = left;
                    if predictor != 1 {
                        prediction = if y == 0 && x == 0 {
                            default_prediction
                        } else if y == 0 {
                            left
                        } else if x == 0 {
                            above
                        } else {
                            predict_neighbors(left, above, above_left, predictor)
                        };
                    }
                    let difference = value.wrapping_sub(prediction) as u16 as i16 as i32;
                    let category = difference_category(difference);
                    diffs[y * width + x] = (category as u32) << 16 | difference as u16 as u32;
                    frequency[category] += 1;
                    if predictor != 1 {
                        above_row[x] = value;
                    }
                    if x == 0 {
                        above_first = value;
                    }
                    left = value;
                    above_left = above;
                    offset += components * bytes_per_sample;
                }
            }
            differences.push(diffs);
        }
        let tables: Vec<HuffmanSpec> = frequencies.iter().map(optimal_huffman_spec).collect();
        let mut out = Vec::with_capacity(frame.len() / 2 + 1024);
        out.extend_from_slice(&[0xff, 0xd8]);
        let mut sof = vec![
            precision as u8,
            (height >> 8) as u8,
            height as u8,
            (width >> 8) as u8,
            width as u8,
            components as u8,
        ];
        for c in 0..components {
            sof.extend_from_slice(&[(c + 1) as u8, 0x11, 0]);
        }
        append_segment(&mut out, 0xc3, &sof);
        let mut dht = Vec::new();
        for (c, table) in tables.iter().enumerate() {
            dht.push(c as u8);
            dht.extend_from_slice(&table.counts[1..]);
            dht.extend_from_slice(&table.symbols);
        }
        append_segment(&mut out, 0xc4, &dht);
        let mut sos = vec![components as u8];
        for c in 0..components {
            sos.extend_from_slice(&[(c + 1) as u8, (c << 4) as u8]);
        }
        sos.extend_from_slice(&[predictor as u8, 0, 0]);
        append_segment(&mut out, 0xda, &sos);
        let codes: Vec<HuffmanCodes> = tables.iter().map(HuffmanSpec::codes).collect();
        let mut writer = EntropyWriter {
            out,
            acc: 0,
            count: 0,
        };
        for i in 0..width * height {
            for (diffs, codes) in differences.iter().zip(&codes) {
                let packed = diffs[i];
                let difference = packed as u16 as i16 as i32;
                let category = (packed >> 16) as usize;
                let mut code = codes.code[category];
                let mut length = codes.length[category];
                if category > 0 && category < 16 {
                    let mut bits = difference;
                    if bits < 0 {
                        bits -= 1;
                    }
                    code = (code << category) | (bits as u32 & ((1u32 << category) - 1));
                    length += category as u32;
                }
                writer.write(code, length);
            }
        }
        let mut out = writer.flush();
        out.extend_from_slice(&[0xff, 0xd9]);
        Ok(out)
    }
}
fn validate_encoder_metadata(metadata: &Metadata) -> Result<(), Error> {
    if metadata.rows == 0 {
        return Err(Error::encoder_metadata("Rows"));
    }
    if metadata.columns == 0 {
        return Err(Error::encoder_metadata("Columns"));
    }
    if metadata.bits_allocated != 8 && metadata.bits_allocated != 16 {
        return Err(Error::encoder_metadata("BitsAllocated"));
    }
    if metadata.bits_stored < 2 || metadata.bits_stored > metadata.bits_allocated {
        return Err(Error::encoder_metadata("BitsStored"));
    }
    if metadata.high_bit != metadata.bits_stored - 1 {
        return Err(Error::encoder_metadata("HighBit"));
    }
    if metadata.pixel_representation > 1 {
        return Err(Error::encoder_metadata("PixelRepresentation"));
    }
    if metadata.number_of_frames < 1 {
        return Err(Error::encoder_metadata("NumberOfFrames"));
    }
    let photometric = normalized_photometric(metadata);
    match metadata.samples_per_pixel {
        1 => {
            if photometric != "MONOCHROME1" && photometric != "MONOCHROME2" {
                return Err(Error::encoder_metadata("PhotometricInterpretation"));
            }
        }
        3 => {
            if photometric != "RGB" && photometric != "YBR_FULL" {
                return Err(Error::encoder_metadata("PhotometricInterpretation"));
            }
            if metadata.pixel_representation != 0 {
                return Err(Error::encoder_metadata("PixelRepresentation"));
            }
            if metadata.planar_configuration_present && metadata.planar_configuration != 0 {
                return Err(Error::encoder_metadata("PlanarConfiguration"));
            }
        }
        _ => return Err(Error::encoder_metadata("SamplesPerPixel")),
    }
    let worst =
        metadata.rows as u64 * metadata.columns as u64 * metadata.samples_per_pixel as u64 * 8
            + 4096;
    if worst > (1u64 << 32) - 2 {
        return Err(Error::new(
            ErrorKind::EncoderOutputInvalid,
            "encoded frame length",
        ));
    }
    Ok(())
}
fn difference_category(difference: i32) -> usize {
    if difference == -32768 {
        return 16;
    }
    (32 - difference.unsigned_abs().leading_zeros()) as usize
}
fn append_segment(out: &mut Vec<u8>, marker: u8, payload: &[u8]) {
    let length = payload.len() + 2;
    out.extend_from_slice(&[0xff, marker, (length >> 8) as u8, length as u8]);
    out.extend_from_slice(payload);
}
struct HuffmanSpec {
    counts: [u8; 17],
    symbols: Vec<u8>,
}
struct HuffmanCodes {
    code: [u32; 17],
    length: [u32; 17],
}
fn optimal_huffman_spec(frequencies: &[usize; 17]) -> HuffmanSpec {
    const SYMBOLS: usize = 17;
    let mut freq = [0usize; SYMBOLS + 1];
    freq[..SYMBOLS].copy_from_slice(frequencies);
    freq[SYMBOLS] = 1;
    let mut codesize = [0usize; SYMBOLS + 1];
    let mut others = [-1i64; SYMBOLS + 1];
    loop {
        let mut v1: i64 = -1;
        for (i, &f) in freq.iter().enumerate() {
            if f == 0 {
                continue;
            }
            if v1 < 0 || f <= freq[v1 as usize] {
                v1 = i as i64;
            }
        }
        let mut v2: i64 = -1;
        for (i, &f) in freq.iter().enumerate() {
            if f == 0 || i as i64 == v1 {
                continue;
            }
            if v2 < 0 || f <= freq[v2 as usize] {
                v2 = i as i64;
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
        others[v1] = v2 as i64;
        codesize[v2] += 1;
        while others[v2] >= 0 {
            v2 = others[v2] as usize;
            codesize[v2] += 1;
        }
    }
    let mut bits = [0i64; 33];
    for &size in &codesize {
        if size > 0 {
            bits[size] += 1;
        }
    }
    for i in (17..=32).rev() {
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
    }
    for i in (1..=16).rev() {
        if bits[i] > 0 {
            bits[i] -= 1;
            break;
        }
    }
    let mut counts = [0u8; 17];
    for i in 1..=16 {
        counts[i] = bits[i] as u8;
    }
    let mut order: Vec<usize> = (0..SYMBOLS).filter(|&s| codesize[s] > 0).collect();
    order.sort_by_key(|&s| codesize[s]);
    HuffmanSpec {
        counts,
        symbols: order.into_iter().map(|s| s as u8).collect(),
    }
}
impl HuffmanSpec {
    fn codes(&self) -> HuffmanCodes {
        let mut out = HuffmanCodes {
            code: [0; 17],
            length: [0; 17],
        };
        let mut code = 0u32;
        let mut k = 0;
        for length in 1..=16u32 {
            for _ in 0..self.counts[length as usize] {
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
struct EntropyWriter {
    out: Vec<u8>,
    acc: u64,
    count: u32,
}
impl EntropyWriter {
    fn write(&mut self, value: u32, length: u32) {
        if length == 0 {
            return;
        }
        let mask = ((1u64 << length) - 1) as u32;
        self.acc = (self.acc << length) | (value & mask) as u64;
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
    fn flush(mut self) -> Vec<u8> {
        if self.count > 0 {
            let fill = 8 - self.count;
            self.write((1 << fill) - 1, fill);
        }
        self.out
    }
}
