use super::SampleFormat;
use crate::display::StoredSampleReader;
use crate::net::ul::{Ctx, Kind as CtxKind};
use crate::object::{Object, Tag, Vr};
use std::fmt;
pub const DEFAULT_MAX_STORED_SAMPLES: i64 = 1 << 28;
const SAMPLE_CHECK_INTERVAL: usize = 1 << 16;
const TAG_PIXEL_PADDING_VALUE: Tag = Tag::new(0x0028, 0x0120);
const TAG_PIXEL_PADDING_RANGE_LIMIT: Tag = Tag::new(0x0028, 0x0121);
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SampleError {
    InvalidFormat {
        field: &'static str,
        cause: String,
    },
    TypeTooNarrow {
        type_name: &'static str,
        bits_stored: i64,
        signed: bool,
    },
    BufferTooShort {
        slots: usize,
        samples: usize,
    },
    DataLength {
        bytes: usize,
        width: usize,
    },
    BudgetExceeded {
        samples: usize,
        limit: i64,
    },
    InvalidPixelPadding(String),
    Context(CtxKind),
    NonFiniteValue,
    InvalidFormatBare,
    DestinationTooShort,
}
fn signedness(signed: bool) -> &'static str {
    if signed { "signed" } else { "unsigned" }
}
impl fmt::Display for SampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SampleError::InvalidFormat { field, cause } => {
                write!(f, "dicom: invalid stored sample format: {field}: {cause}")
            }
            SampleError::TypeTooNarrow {
                type_name,
                bits_stored,
                signed,
            } => write!(
                f,
                "dicom: destination type cannot hold every stored sample value: {type_name} for {bits_stored}-bit {} samples",
                signedness(*signed)
            ),
            SampleError::BufferTooShort { slots, samples } => write!(
                f,
                "dicom: destination shorter than the stored samples: {slots} slots for {samples} samples"
            ),
            SampleError::DataLength { bytes, width } => write!(
                f,
                "dicom: stored sample bytes are not a whole number of samples: {bytes} bytes in {width}-byte containers"
            ),
            SampleError::BudgetExceeded { samples, limit } => write!(
                f,
                "dicom: stored samples exceed the caller budget: {samples} samples, limit {limit}"
            ),
            SampleError::InvalidPixelPadding(detail) => {
                write!(f, "dicom: invalid pixel padding: {detail}")
            }
            SampleError::NonFiniteValue => f.write_str("dicom: stored sample value must be finite"),
            SampleError::InvalidFormatBare => f.write_str("dicom: invalid stored sample format"),
            SampleError::DestinationTooShort => {
                f.write_str("dicom: destination shorter than the stored samples")
            }
            SampleError::Context(CtxKind::Canceled) => f.write_str("context canceled"),
            SampleError::Context(_) => f.write_str("context deadline exceeded"),
        }
    }
}
impl std::error::Error for SampleError {}
fn context(ctx: &Ctx) -> Result<(), SampleError> {
    ctx.err().map_or(Ok(()), |k| Err(SampleError::Context(k)))
}
impl SampleFormat {
    pub fn reader(&self) -> Result<StoredSampleReader, SampleError> {
        StoredSampleReader::new(
            self.bits_allocated,
            self.bits_stored,
            self.high_bit,
            self.signed,
            self.big_endian,
        )
        .map_err(|e| SampleError::InvalidFormat {
            field: match format!("{e:?}").as_str() {
                "BitsAllocated" => "BitsAllocated",
                "BitsStored" => "BitsStored",
                _ => "HighBit",
            },
            cause: e.to_string(),
        })
    }
    pub fn validate(&self) -> Result<(), SampleError> {
        self.reader().map(|_| ())
    }
    pub fn range(&self) -> Result<(i64, i64), SampleError> {
        self.reader().map(|r| r.range())
    }
    pub fn writer(&self, value: f64) -> Result<StoredSampleWriter, SampleError> {
        let layout = self.reader()?;
        if !value.is_finite() {
            return Err(SampleError::NonFiniteValue);
        }
        let (minimum, maximum) = layout.range();
        let value = (maximum as f64).min((minimum as f64).max(value.round()));
        Ok(StoredSampleWriter {
            layout: Some(layout),
            value: crate::fmath::f64_to_i64(value),
        })
    }
}
#[derive(Clone, Debug, Default)]
pub struct StoredSampleWriter {
    layout: Option<StoredSampleReader>,
    value: i64,
}
impl StoredSampleWriter {
    pub fn write(&self, dst: &mut [u8]) -> Result<(), SampleError> {
        let layout = self
            .layout
            .as_ref()
            .filter(|l| l.width() != 0)
            .ok_or(SampleError::InvalidFormatBare)?;
        if dst.len() < layout.width() {
            return Err(SampleError::DestinationTooShort);
        }
        layout.put(dst, self.value);
        Ok(())
    }
}
pub trait StoredInteger: Copy + Default {
    const NAME: &'static str;
    const MIN: i128;
    const MAX: i128;
    fn from_stored(v: i64) -> Self;
}
macro_rules! stored_integer {
    ($($t:ty => $name:literal),*) => {$(
        impl StoredInteger for $t {
            const NAME: &'static str = $name;
            const MIN: i128 = <$t>::MIN as i128;
            const MAX: i128 = <$t>::MAX as i128;
            fn from_stored(v: i64) -> Self {
                v as $t
            }
        }
    )*};
}
stored_integer!(i8 => "int8", i16 => "int16", i32 => "int32", i64 => "int64",
    u8 => "uint8", u16 => "uint16", u32 => "uint32", u64 => "uint64");
fn checked_reader<T: StoredInteger>(f: &SampleFormat) -> Result<StoredSampleReader, SampleError> {
    let r = f.reader()?;
    let (lo, hi) = r.range();
    if i128::from(lo) < T::MIN || i128::from(hi) > T::MAX {
        return Err(SampleError::TypeTooNarrow {
            type_name: T::NAME,
            bits_stored: f.bits_stored,
            signed: f.signed,
        });
    }
    Ok(r)
}
fn sample_count(src: &[u8], width: usize) -> Result<usize, SampleError> {
    if !src.len().is_multiple_of(width) {
        return Err(SampleError::DataLength {
            bytes: src.len(),
            width,
        });
    }
    Ok(src.len().checked_div(width).unwrap_or(0))
}
pub fn read_stored_samples<T: StoredInteger>(
    ctx: &Ctx,
    dst: &mut [T],
    src: &[u8],
    format: &SampleFormat,
) -> Result<usize, SampleError> {
    let r = checked_reader::<T>(format)?;
    let width = r.width();
    let n = sample_count(src, width)?;
    if dst.len() < n {
        return Err(SampleError::BufferTooShort {
            slots: dst.len(),
            samples: n,
        });
    }
    for (block, out) in src
        .chunks(SAMPLE_CHECK_INTERVAL.saturating_mul(width).max(1))
        .zip(dst.chunks_mut(SAMPLE_CHECK_INTERVAL))
    {
        context(ctx)?;
        for (sample, slot) in block.chunks_exact(width.max(1)).zip(out.iter_mut()) {
            *slot = T::from_stored(r.value(sample));
        }
    }
    Ok(n)
}
pub fn stored_samples<T: StoredInteger>(
    ctx: &Ctx,
    src: &[u8],
    format: &SampleFormat,
    max_samples: i64,
) -> Result<Vec<T>, SampleError> {
    let r = checked_reader::<T>(format)?;
    let n = sample_count(src, r.width())?;
    let limit = if max_samples <= 0 {
        DEFAULT_MAX_STORED_SAMPLES
    } else {
        max_samples
    };
    if n as i64 > limit {
        return Err(SampleError::BudgetExceeded { samples: n, limit });
    }
    let mut out = vec![T::default(); n];
    read_stored_samples(ctx, &mut out, src, format)?;
    Ok(out)
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PixelPadding {
    pub value: i64,
    pub range_limit: i64,
    pub has_range: bool,
}
impl PixelPadding {
    pub fn contains(&self, stored: i64) -> bool {
        if !self.has_range {
            return stored == self.value;
        }
        stored >= self.value.min(self.range_limit) && stored <= self.value.max(self.range_limit)
    }
}
pub fn pixel_padding_from_object(
    obj: &Object,
    format: &SampleFormat,
) -> Result<Option<PixelPadding>, SampleError> {
    let (lo, hi) = format.range()?;
    let Some(value) = padding_value(obj, TAG_PIXEL_PADDING_VALUE, format.signed)? else {
        return Ok(None);
    };
    let mut padding = PixelPadding {
        value,
        ..PixelPadding::default()
    };
    if let Some(limit) = padding_value(obj, TAG_PIXEL_PADDING_RANGE_LIMIT, format.signed)? {
        padding.range_limit = limit;
        padding.has_range = true;
    }
    if [padding.value, padding.range_limit]
        .iter()
        .any(|&v| v < lo || v > hi)
    {
        return Err(SampleError::InvalidPixelPadding(format!(
            "value outside the {}-bit {} range",
            format.bits_stored,
            signedness(format.signed)
        )));
    }
    Ok(Some(padding))
}
fn padding_value(obj: &Object, tag: Tag, signed: bool) -> Result<Option<i64>, SampleError> {
    let Some(elem) = obj.get(tag) else {
        return Ok(None);
    };
    let invalid = |detail: String| Err(SampleError::InvalidPixelPadding(detail));
    match elem.vr() {
        Vr::US if signed => return invalid(format!("{tag} is US for signed samples")),
        Vr::SS if !signed => return invalid(format!("{tag} is SS for unsigned samples")),
        Vr::US | Vr::SS | Vr::UN => {}
        vr => return invalid(format!("{tag} has VR {}", vr.as_str())),
    }
    if let Some(&[b0, b1]) = elem.raw() {
        let pair = [b0, b1];
        let v = if obj.value_big_endian() {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        };
        return Ok(Some(if signed {
            i64::from(v as i16)
        } else {
            i64::from(v)
        }));
    }
    invalid(format!("{tag} must hold one 16-bit value"))
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StoredSampleStatistics {
    pub samples: i64,
    pub padding: i64,
    pub minimum: i64,
    pub maximum: i64,
    pub mean: f64,
}
pub fn compute_stored_sample_statistics(
    ctx: &Ctx,
    src: &[u8],
    format: &SampleFormat,
    padding: Option<&PixelPadding>,
) -> Result<StoredSampleStatistics, SampleError> {
    let r = format.reader()?;
    let width = r.width();
    sample_count(src, width)?;
    let mut stats = StoredSampleStatistics {
        minimum: i64::MAX,
        maximum: i64::MIN,
        ..StoredSampleStatistics::default()
    };
    let mut sum: i128 = 0;
    for block in src.chunks(SAMPLE_CHECK_INTERVAL.saturating_mul(width).max(1)) {
        context(ctx)?;
        for sample in block.chunks_exact(width.max(1)) {
            let v = r.value(sample);
            if padding.is_some_and(|p| p.contains(v)) {
                stats.padding = stats.padding.saturating_add(1);
                continue;
            }
            stats.samples = stats.samples.saturating_add(1);
            stats.minimum = stats.minimum.min(v);
            stats.maximum = stats.maximum.max(v);
            sum = sum.saturating_add(i128::from(v));
        }
    }
    if stats.samples == 0 {
        stats.minimum = 0;
        stats.maximum = 0;
        return Ok(stats);
    }
    stats.mean = int128_float(sum) / stats.samples as f64;
    Ok(stats)
}
fn int128_float(sum: i128) -> f64 {
    const TWO64: f64 = 18_446_744_073_709_551_616.0;
    let magnitude = sum.unsigned_abs();
    let value = ((magnitude >> 64) as u64) as f64 * TWO64 + (magnitude as u64) as f64;
    if sum < 0 { -value } else { value }
}
