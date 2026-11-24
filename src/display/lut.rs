use super::DisplayError;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LutAdjustment {
    OneEntryPerWord,
    DataLength,
    Packed,
    BitsInferred,
    EightBitValues,
    SixteenBitValues,
    Masked,
}
impl LutAdjustment {
    pub fn description(self) -> &'static str {
        match self {
            LutAdjustment::OneEntryPerWord => "8-bit entries stored one per 16-bit word",
            LutAdjustment::DataLength => {
                "LUT Data length does not match the number of entries; the entries present were used"
            }
            LutAdjustment::Packed => {
                "8-bit entries packed two per word under another bits per entry; read as 8 bits"
            }
            LutAdjustment::BitsInferred => {
                "bits per entry outside 8-16; inferred from the largest entry"
            }
            LutAdjustment::EightBitValues => {
                "16 bits per entry declared for 8-bit values; read as 8 bits"
            }
            LutAdjustment::SixteenBitValues => {
                "8 bits per entry declared for 16-bit values; read as 16 bits"
            }
            LutAdjustment::Masked => {
                "entries above the bits per entry were masked to their low bits"
            }
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LutOptions {
    pub legacy: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lut {
    pub first_mapped: i64,
    pub bits_per_entry: i64,
    pub entries: Vec<u16>,
    pub adjustments: Vec<LutAdjustment>,
}
impl Lut {
    pub fn new(descriptor: &[i64], entries: Vec<u16>) -> Result<Lut, DisplayError> {
        if descriptor.len() != 3 {
            return Err(DisplayError::InvalidLutDescriptor(format!(
                "got {} descriptor values, want 3",
                descriptor.len()
            )));
        }
        let mut number_of_entries = descriptor[0];
        if number_of_entries == 0 {
            number_of_entries = 1 << 16;
        }
        if number_of_entries < 0 {
            return Err(DisplayError::InvalidLutDescriptor(format!(
                "number of entries {number_of_entries} is negative"
            )));
        }
        let bits_per_entry = descriptor[2];
        if !(8..=16).contains(&bits_per_entry) {
            return Err(DisplayError::InvalidLutDescriptor(format!(
                "bits per entry {bits_per_entry}, want 8 to 16"
            )));
        }
        if entries.len() as i64 != number_of_entries {
            return Err(DisplayError::InvalidLutData(format!(
                "got {} entries, want {number_of_entries}",
                entries.len()
            )));
        }
        let largest = max_entry(&entries);
        if i64::from(largest) > (1 << bits_per_entry) - 1 {
            return Err(DisplayError::InvalidLutData(format!(
                "entry {largest} exceeds {bits_per_entry} bits per entry"
            )));
        }
        Ok(Lut {
            first_mapped: descriptor[1],
            bits_per_entry,
            entries,
            adjustments: Vec::new(),
        })
    }
    pub fn from_data(
        raw: &[u8],
        big_endian: bool,
        number_of_entries: i64,
        first_mapped: i64,
        bits_per_entry: i64,
        options: LutOptions,
    ) -> Result<Lut, DisplayError> {
        let count = if number_of_entries == 0 {
            1 << 16
        } else {
            number_of_entries
        };
        let (entries, bits_per_entry, adjustments) =
            decode_lut_data(raw, big_endian, count, bits_per_entry, options)?;
        Ok(Lut {
            first_mapped,
            bits_per_entry,
            entries,
            adjustments,
        })
    }
    pub fn lookup(&self, value: i64) -> u16 {
        if self.entries.is_empty() {
            return 0;
        }
        let index = value.wrapping_sub(self.first_mapped).max(0);
        let last = self.entries.len() as i64 - 1;
        self.entries[index.min(last) as usize]
    }
    fn bits_or_default(&self) -> i64 {
        if (1..=16).contains(&self.bits_per_entry) {
            self.bits_per_entry
        } else {
            16
        }
    }
    pub(crate) fn output_max(&self) -> u16 {
        ((1u32 << self.bits_or_default()) - 1) as u16
    }
    pub(crate) fn entry_byte(&self, entry: u16) -> u8 {
        let mut entry = entry.min(self.output_max());
        let shift = self.bits_or_default() - 8;
        if shift > 0 {
            entry >>= shift;
        }
        entry as u8
    }
    pub(crate) fn entry16(&self, entry: u16) -> u16 {
        let top = u32::from(self.output_max());
        let value = u32::from(entry).min(top);
        ((value * 65535 + top / 2) / top) as u16
    }
}
pub fn encode_lut_data(entries: &[u16], bits_per_entry: i64) -> Vec<u8> {
    if bits_per_entry == 8 {
        let mut out = vec![0; entries.len() + entries.len() % 2];
        for (o, &e) in out.iter_mut().zip(entries) {
            *o = e as u8;
        }
        return out;
    }
    entries.iter().flat_map(|e| e.to_le_bytes()).collect()
}
fn legacy_error(data: bool, detail: String) -> DisplayError {
    DisplayError::LegacyLut { data, detail }
}
fn decode_lut_data(
    raw: &[u8],
    big_endian: bool,
    count: i64,
    bits_per_entry: i64,
    options: LutOptions,
) -> Result<(Vec<u16>, i64, Vec<LutAdjustment>), DisplayError> {
    if raw.is_empty() {
        return Err(DisplayError::InvalidLutData("no LUT data".into()));
    }
    let mut adjustments = Vec::new();
    if !(8..=16).contains(&bits_per_entry) && !options.legacy {
        return Err(legacy_error(
            false,
            format!("bits per entry {bits_per_entry}, want 8 to 16"),
        ));
    }
    let words: Vec<u16> = raw
        .chunks(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair.get(1).copied().unwrap_or(0));
            if big_endian {
                u16::from_be_bytes([a, b])
            } else {
                u16::from_le_bytes([a, b])
            }
        })
        .collect();
    let len = words.len() as i64;
    if len == (count + 1) / 2 && (count > 1 || bits_per_entry == 8) {
        if bits_per_entry != 8 {
            if !options.legacy {
                return Err(legacy_error(
                    true,
                    format!(
                        "{len} words hold {count} packed 8-bit entries, but bits per entry is {bits_per_entry}"
                    ),
                ));
            }
            adjustments.push(LutAdjustment::Packed);
        }
        let entries = (0..count as usize)
            .map(|i| (words[i / 2] >> (8 * (i % 2))) & 0xFF)
            .collect();
        return Ok((entries, 8, adjustments));
    }
    if len != count {
        if !options.legacy {
            return Err(legacy_error(
                true,
                format!("{len} data words for {count} entries of {bits_per_entry} bits"),
            ));
        }
        adjustments.push(LutAdjustment::DataLength);
    }
    word_lut_depth(words, bits_per_entry, adjustments, options, true)
}
pub(super) fn word_lut_depth(
    mut entries: Vec<u16>,
    mut bits_per_entry: i64,
    mut adjustments: Vec<LutAdjustment>,
    options: LutOptions,
    one_entry_per_word: bool,
) -> Result<(Vec<u16>, i64, Vec<LutAdjustment>), DisplayError> {
    let largest = max_entry(&entries);
    let eight_bit = eight_bit_values(&entries);
    if !(8..=16).contains(&bits_per_entry) {
        if !options.legacy {
            return Err(legacy_error(
                false,
                format!("bits per entry {bits_per_entry}, want 8 to 16"),
            ));
        }
        let len = 16 - largest.leading_zeros() as i64;
        bits_per_entry = len.clamp(8, 16);
        adjustments.push(LutAdjustment::BitsInferred);
    } else if options.legacy && bits_per_entry == 16 && eight_bit {
        adjustments.push(LutAdjustment::EightBitValues);
        return Ok((mask_entries(&entries, 8), 8, adjustments));
    } else if options.legacy && bits_per_entry == 8 && !eight_bit {
        bits_per_entry = 16;
        adjustments.push(LutAdjustment::SixteenBitValues);
    } else if bits_per_entry == 8 && one_entry_per_word {
        adjustments.push(LutAdjustment::OneEntryPerWord);
    }
    let limit = ((1u32 << bits_per_entry) - 1) as u16;
    if largest > limit {
        if !options.legacy {
            return Err(legacy_error(
                true,
                format!("entry {largest} exceeds {bits_per_entry} bits per entry"),
            ));
        }
        adjustments.push(LutAdjustment::Masked);
        entries = mask_entries(&entries, bits_per_entry);
    }
    Ok((entries, bits_per_entry, adjustments))
}
fn max_entry(entries: &[u16]) -> u16 {
    entries.iter().copied().max().unwrap_or(0)
}
fn eight_bit_values(entries: &[u16]) -> bool {
    entries.iter().all(|&e| {
        let high = e >> 8;
        high == 0 || high == e & 0xFF
    })
}
fn mask_entries(entries: &[u16], bits_per_entry: i64) -> Vec<u16> {
    let mask = ((1u32 << bits_per_entry) - 1) as u16;
    entries.iter().map(|&e| e & mask).collect()
}
