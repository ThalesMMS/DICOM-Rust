#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StoredSampleReader {
    width: usize,
    shift: u32,
    mask: u64,
    sign_bit: u64,
    big_endian: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormatError {
    BitsAllocated,
    BitsStored,
    HighBit,
}
impl std::fmt::Display for SampleFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SampleFormatError::BitsAllocated => "BitsAllocated must be 8, 16 or 32",
            SampleFormatError::BitsStored => "BitsStored must be between 1 and BitsAllocated",
            SampleFormatError::HighBit => {
                "HighBit must be between BitsStored-1 and BitsAllocated-1"
            }
        })
    }
}
impl std::error::Error for SampleFormatError {}
impl StoredSampleReader {
    pub fn new(
        bits_allocated: i64,
        bits_stored: i64,
        high_bit: i64,
        signed: bool,
        big_endian: bool,
    ) -> Result<Self, SampleFormatError> {
        if !matches!(bits_allocated, 8 | 16 | 32) {
            return Err(SampleFormatError::BitsAllocated);
        }
        if bits_stored < 1 || bits_stored > bits_allocated {
            return Err(SampleFormatError::BitsStored);
        }
        if high_bit < bits_stored - 1 || high_bit >= bits_allocated {
            return Err(SampleFormatError::HighBit);
        }
        Ok(Self::build(
            bits_allocated,
            bits_stored,
            high_bit,
            signed,
            big_endian,
        ))
    }
    pub fn legacy(
        bits_allocated: i64,
        mut bits_stored: i64,
        mut high_bit: i64,
        signed: bool,
        big_endian: bool,
    ) -> Self {
        if !matches!(bits_allocated, 8 | 16 | 32) {
            return Self::default();
        }
        if bits_stored <= 0 || bits_stored > bits_allocated {
            bits_stored = bits_allocated;
        }
        if high_bit + 1 < bits_stored || high_bit >= bits_allocated {
            high_bit = bits_stored - 1;
        }
        Self::build(bits_allocated, bits_stored, high_bit, signed, big_endian)
    }
    fn build(
        bits_allocated: i64,
        bits_stored: i64,
        high_bit: i64,
        signed: bool,
        big_endian: bool,
    ) -> Self {
        StoredSampleReader {
            width: (bits_allocated / 8) as usize,
            shift: (high_bit + 1 - bits_stored) as u32,
            mask: (1u64 << bits_stored) - 1,
            sign_bit: if signed { 1u64 << (bits_stored - 1) } else { 0 },
            big_endian,
        }
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn signed(&self) -> bool {
        self.sign_bit != 0
    }
    pub(crate) fn values16(&self, data: &[[u8; 2]], out: &mut [i32]) {
        debug_assert_eq!(self.width, 2);
        let (shift, mask, sign) = (self.shift, self.mask as u32, self.sign_bit as u32);
        let field =
            |word: u16| ((((u32::from(word)) >> shift) & mask) ^ sign).wrapping_sub(sign) as i32;
        if self.big_endian {
            for (o, b) in out.iter_mut().zip(data) {
                *o = field(u16::from_be_bytes(*b));
            }
        } else {
            for (o, b) in out.iter_mut().zip(data) {
                *o = field(u16::from_le_bytes(*b));
            }
        }
    }
    pub fn value(&self, data: &[u8]) -> i64 {
        let raw: u64 = match self.width {
            1 => u64::from(data[0]),
            2 => {
                let b = [data[0], data[1]];
                u64::from(if self.big_endian {
                    u16::from_be_bytes(b)
                } else {
                    u16::from_le_bytes(b)
                })
            }
            4 => {
                let b = [data[0], data[1], data[2], data[3]];
                u64::from(if self.big_endian {
                    u32::from_be_bytes(b)
                } else {
                    u32::from_le_bytes(b)
                })
            }
            _ => return 0,
        };
        let raw = (raw >> self.shift) & self.mask;
        if raw & self.sign_bit != 0 {
            (raw | !self.mask) as i64
        } else {
            raw as i64
        }
    }
    pub fn put(&self, data: &mut [u8], value: i64) {
        let field_mask = self.mask << self.shift;
        let field = ((value as u64) & self.mask) << self.shift;
        match self.width {
            1 => data[0] = (data[0] & !(field_mask as u8)) | field as u8,
            2 => {
                let b = [data[0], data[1]];
                let word = if self.big_endian {
                    u16::from_be_bytes(b)
                } else {
                    u16::from_le_bytes(b)
                };
                let word = (word & !(field_mask as u16)) | field as u16;
                let out = if self.big_endian {
                    word.to_be_bytes()
                } else {
                    word.to_le_bytes()
                };
                data[..2].copy_from_slice(&out);
            }
            4 => {
                let b = [data[0], data[1], data[2], data[3]];
                let word = if self.big_endian {
                    u32::from_be_bytes(b)
                } else {
                    u32::from_le_bytes(b)
                };
                let word = (word & !(field_mask as u32)) | field as u32;
                let out = if self.big_endian {
                    word.to_be_bytes()
                } else {
                    word.to_le_bytes()
                };
                data[..4].copy_from_slice(&out);
            }
            _ => {}
        }
    }
    pub fn range(&self) -> (i64, i64) {
        if self.width == 0 {
            return (0, 0);
        }
        if self.sign_bit != 0 {
            (-(self.sign_bit as i64), self.sign_bit as i64 - 1)
        } else {
            (0, self.mask as i64)
        }
    }
}
