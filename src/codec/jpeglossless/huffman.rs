#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::Error;
pub(crate) struct HuffTable {
    mincode: [i64; 17],
    maxcode: [i64; 17],
    valptr: [i64; 17],
    values: Vec<u8>,
    look_length: [u8; 256],
    look_value: [u8; 256],
    diff_length: [u8; 256],
    diff_value: [i32; 256],
}
impl HuffTable {
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn build(counts: &[usize; 17], values: Vec<u8>) -> HuffTable {
        let mut sizes = Vec::new();
        for (l, &count) in counts.iter().enumerate().skip(1) {
            for _ in 0..count {
                sizes.push(l);
            }
        }
        let mut codes = vec![0i64; sizes.len()];
        if !sizes.is_empty() {
            let mut code = 0i64;
            let mut si = sizes[0];
            let mut k = 0;
            while k < sizes.len() {
                while k < sizes.len() && sizes[k] == si {
                    codes[k] = code;
                    code += 1;
                    k += 1;
                }
                code <<= 1;
                si += 1;
            }
        }
        let mut t = HuffTable {
            mincode: [0; 17],
            maxcode: [0; 17],
            valptr: [0; 17],
            values,
            look_length: [0; 256],
            look_value: [0; 256],
            diff_length: [0; 256],
            diff_value: [0; 256],
        };
        let mut k = 0usize;
        for l in 1..=16usize {
            if counts[l] == 0 {
                t.maxcode[l] = -1;
                continue;
            }
            t.valptr[l] = k as i64;
            t.mincode[l] = codes[k];
            k += counts[l];
            t.maxcode[l] = codes[k - 1];
            if l <= 8 {
                for i in t.valptr[l] as usize..k {
                    let first = (codes[i] << (8 - l)) as usize;
                    for prefix in first..first + (1 << (8 - l)) {
                        let value = t.values[i];
                        t.look_length[prefix] = l as u8;
                        t.look_value[prefix] = value;
                        let size = value as usize;
                        if size == 0 || size == 16 {
                            t.diff_length[prefix] = l as u8;
                            if size == 16 {
                                t.diff_value[prefix] = 32768;
                            }
                        } else if l + size <= 8 {
                            t.diff_length[prefix] = (l + size) as u8;
                            let magnitude = (prefix >> (8 - l - size)) & ((1 << size) - 1);
                            t.diff_value[prefix] = extend_difference(magnitude as i32, size as u32);
                        }
                    }
                }
            }
        }
        t
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn decode_symbol(&self, br: &mut BitReader) -> Result<u8, Error> {
        if br.nbits < 8 {
            br.refill();
        }
        if br.nbits >= 8 {
            let prefix = (br.bits >> (br.nbits - 8)) as u8 as usize;
            let length = self.look_length[prefix];
            if length != 0 {
                br.nbits -= length as u32;
                return Ok(self.look_value[prefix]);
            }
        }
        let mut code = 0i64;
        for l in 1..=16usize {
            let b = br.read_bit()?;
            code = (code << 1) | b as i64;
            if self.maxcode[l] >= 0 && code <= self.maxcode[l] {
                let idx = self.valptr[l] + (code - self.mincode[l]);
                if idx < 0 || idx >= self.values.len() as i64 {
                    return Err(Error::invalid("Huffman value index out of range"));
                }
                return Ok(self.values[idx as usize]);
            }
        }
        Err(Error::invalid("undecodable Huffman code"))
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn decode_difference(&self, br: &mut BitReader) -> Result<i32, Error> {
        if br.nbits < 24 {
            br.refill();
        }
        if br.nbits >= 8 {
            let prefix = (br.bits >> (br.nbits - 8)) as u8 as usize;
            let length = self.diff_length[prefix];
            if length != 0 {
                br.nbits -= length as u32;
                return Ok(self.diff_value[prefix]);
            }
            let length = self.look_length[prefix] as u32;
            if length != 0 {
                let size = self.look_value[prefix] as u32;
                if size == 0 || size == 16 {
                    br.nbits -= length;
                    if size == 16 {
                        return Ok(32768);
                    }
                    return Ok(0);
                }
                if size < 16 && br.nbits >= length + size {
                    br.nbits -= length + size;
                    let value = ((br.bits >> br.nbits) & ((1u64 << size) - 1)) as i32;
                    return Ok(extend_difference(value, size));
                }
            }
        }
        let symbol = self.decode_symbol(br)?;
        receive_extend(br, symbol as i64)
    }
}
pub(crate) fn receive_extend(reader: &mut BitReader, size: i64) -> Result<i32, Error> {
    if size == 0 {
        return Ok(0);
    }
    if size == 16 {
        return Ok(32768);
    }
    if !(0..=16).contains(&size) {
        return Err(Error::invalid(format!("difference category={size}")));
    }
    let value = reader.read_bits(size as u32)?;
    Ok(extend_difference(value as i32, size as u32))
}
#[allow(clippy::arithmetic_side_effects)]
pub(crate) fn extend_difference(mut value: i32, size: u32) -> i32 {
    if value < 1 << (size - 1) {
        value += (-1i32 << size) + 1;
    }
    value
}
pub(crate) struct BitReader<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) pos: usize,
    bits: u64,
    nbits: u32,
    lenient_fill: bool,
    pub(crate) fill_quirk: bool,
}
impl<'a> BitReader<'a> {
    pub(crate) fn new(data: &'a [u8], pos: usize, lenient_fill: bool) -> Self {
        BitReader {
            data,
            pos,
            bits: 0,
            nbits: 0,
            lenient_fill,
            fill_quirk: false,
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn read_bit(&mut self) -> Result<u32, Error> {
        if self.nbits == 0 {
            self.refill();
            if self.nbits == 0 {
                return Err(self.boundary_error());
            }
        }
        self.nbits -= 1;
        Ok(((self.bits >> self.nbits) & 1) as u32)
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn read_bits(&mut self, n: u32) -> Result<u32, Error> {
        if self.nbits < n {
            self.refill();
        }
        if self.nbits < n {
            return Err(self.boundary_error());
        }
        self.nbits -= n;
        Ok(((self.bits >> self.nbits) & ((1u64 << n) - 1)) as u32)
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn refill(&mut self) {
        while self.nbits <= 56 && self.pos < self.data.len() {
            let b = self.data[self.pos];
            if b == 0xff {
                if self.pos + 1 >= self.data.len() || self.data[self.pos + 1] != 0 {
                    return;
                }
                self.pos += 1;
            }
            self.pos += 1;
            self.bits = (self.bits << 8) | b as u64;
            self.nbits += 8;
        }
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    fn boundary_error(&self) -> Error {
        if self.pos >= self.data.len() {
            return Error::invalid("unexpected end of entropy data");
        }
        if self.pos + 1 >= self.data.len() {
            return Error::invalid("dangling 0xFF in entropy data");
        }
        Error::invalid(format!(
            "marker 0xFF{:02X} inside entropy data",
            self.data[self.pos + 1]
        ))
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn align_to_restart(&mut self, want: u8) -> Result<(), Error> {
        self.validate_fill_bits()?;
        self.nbits = 0;
        if self.pos >= self.data.len() || self.data[self.pos] != 0xff {
            return Err(Error::invalid(format!(
                "expected restart marker 0xFF{want:02X}"
            )));
        }
        let marker = skip_marker_fill(self.data, self.pos);
        if marker >= self.data.len() || self.data[marker] != want {
            return Err(Error::invalid(format!(
                "invalid restart marker, want 0xFF{want:02X}"
            )));
        }
        self.pos = marker + 1;
        Ok(())
    }
    #[allow(clippy::indexing_slicing)]
    pub(crate) fn finish_entropy(&mut self) -> Result<usize, Error> {
        self.validate_fill_bits()?;
        self.nbits = 0;
        if self.pos >= self.data.len() || self.data[self.pos] != 0xff {
            return Err(Error::invalid("trailing entropy data or missing marker"));
        }
        Ok(self.pos)
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn validate_fill_bits(&mut self) -> Result<(), Error> {
        if self.nbits == 0 {
            return Ok(());
        }
        if self.nbits >= 8 {
            return Err(Error::invalid("trailing entropy data before marker"));
        }
        let mask = (1u64 << self.nbits) - 1;
        if self.bits & mask != mask {
            if self.lenient_fill {
                self.fill_quirk = true;
                return Ok(());
            }
            return Err(Error::invalid("invalid entropy fill bits"));
        }
        Ok(())
    }
}
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
pub(crate) fn skip_marker_fill(data: &[u8], mut pos: usize) -> usize {
    while pos < data.len() && data[pos] == 0xff {
        pos += 1;
    }
    pos
}
