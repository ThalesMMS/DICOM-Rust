#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
use super::Error;
#[allow(clippy::arithmetic_side_effects)]
fn low_mask(n: u32) -> u64 {
    if n >= 64 { u64::MAX } else { (1u64 << n) - 1 }
}
#[derive(Default)]
pub(crate) struct BitWriter {
    pub(crate) buf: Vec<u8>,
    val: u64,
    count: u32,
}
fn has_ff_byte(v: u32) -> bool {
    let v = !v;
    (v.wrapping_sub(0x0101_0101) & !v & 0x8080_8080) != 0
}
impl BitWriter {
    pub(crate) fn with_capacity(capacity: usize) -> BitWriter {
        BitWriter {
            buf: Vec::with_capacity(capacity),
            ..Default::default()
        }
    }
    pub(crate) fn write_bit(&mut self, bit: u32) {
        self.write_bits(bit & 1, 1);
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn write_bits(&mut self, value: u32, n: u32) {
        self.val = (self.val << n) | (value as u64 & low_mask(n));
        self.count += n;
        while self.count >= 32 {
            let word = (self.val >> (self.count - 32)) as u32;
            if !has_ff_byte(word) {
                self.buf.extend_from_slice(&word.to_be_bytes());
                self.count -= 32;
                self.val &= low_mask(self.count);
                continue;
            }
            self.emit_byte();
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    fn emit_byte(&mut self) {
        self.count -= 8;
        let b = (self.val >> self.count) as u8;
        self.val &= low_mask(self.count);
        self.buf.push(b);
        if b == 0xff {
            self.count += 1;
        }
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn write_zeros(&mut self, mut n: u32) {
        while n > 32 {
            self.write_bits(0, 32);
            n -= 32;
        }
        self.write_bits(0, n);
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn pad_to_byte(&mut self) {
        while self.count >= 8 {
            self.emit_byte();
        }
        if self.count == 0 {
            return;
        }
        self.write_zeros(8 - self.count);
        while self.count >= 8 {
            self.emit_byte();
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) enum FillError {
    Eof,
    Marker,
}
impl From<FillError> for Error {
    fn from(e: FillError) -> Error {
        match e {
            FillError::Eof => Error::unexpected_eof(),
            FillError::Marker => Error::invalid("unexpected marker in entropy data"),
        }
    }
}
pub(crate) struct BitReader<'a> {
    buf: &'a [u8],
    pub(crate) pos: usize,
    pub(crate) val: u64,
    pub(crate) count: u32,
    pub(crate) last_was_ff: bool,
}
impl<'a> BitReader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> BitReader<'a> {
        BitReader {
            buf,
            pos: 0,
            val: 0,
            count: 0,
            last_was_ff: false,
        }
    }
    pub(crate) fn remaining_bytes(&self) -> bool {
        self.pos < self.buf.len()
    }
    #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
    pub(crate) fn fill(&mut self) -> Result<(), FillError> {
        while self.count <= 56 {
            if self.pos >= self.buf.len() {
                return Err(FillError::Eof);
            }
            let b = self.buf[self.pos];
            if self.last_was_ff {
                if b & 0x80 != 0 {
                    return Err(FillError::Marker);
                }
                self.val = (self.val << 7) | b as u64;
                self.count += 7;
                self.last_was_ff = false;
            } else {
                self.val = (self.val << 8) | b as u64;
                self.count += 8;
                self.last_was_ff = b == 0xff;
            }
            self.pos += 1;
        }
        Ok(())
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn consume(&mut self, n: u32) {
        self.count -= n;
        self.val &= low_mask(self.count);
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn read_bit(&mut self) -> Result<u32, FillError> {
        if self.count == 0 {
            let r = self.fill();
            if self.count == 0 {
                return Err(r.err().unwrap_or(FillError::Eof));
            }
        }
        self.count -= 1;
        let bit = (self.val >> self.count) as u32 & 1;
        self.val &= low_mask(self.count);
        Ok(bit)
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn read_bits(&mut self, n: u32) -> Result<u32, FillError> {
        if self.count < n {
            let r = self.fill();
            if self.count < n {
                self.val = 0;
                self.count = 0;
                return Err(r.err().unwrap_or(FillError::Eof));
            }
        }
        let mut value = (self.val >> (self.count - n)) as u32;
        if n < 32 {
            value &= (1u32 << n) - 1;
        }
        self.consume(n);
        Ok(value)
    }
    #[allow(clippy::arithmetic_side_effects)]
    pub(crate) fn leading_zeros(&self) -> u32 {
        self.count - (64 - self.val.leading_zeros())
    }
}
