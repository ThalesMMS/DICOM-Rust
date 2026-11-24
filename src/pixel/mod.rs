#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    warn(clippy::indexing_slicing, clippy::arithmetic_side_effects)
)]
pub mod codec_router;
#[cfg(feature = "render")]
pub mod compare;
pub mod encapsulated;
pub mod extract;
pub mod extract_all;
pub mod frame;
pub mod frame_reader;
pub mod guard;
pub mod hologic;
pub mod memory_admission;
pub mod native_rewrite;
pub mod palette_quantize;
pub mod pixeldata;
pub mod redact;
pub mod render_single;
pub mod samples;
mod stream_sink;
pub mod transcode;
pub mod validation_rule;
pub use crate::codec::rle::Metadata;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SampleFormat {
    pub bits_allocated: i64,
    pub bits_stored: i64,
    pub high_bit: i64,
    pub signed: bool,
    pub big_endian: bool,
}
pub fn sample_format(metadata: &Metadata, big_endian: bool) -> SampleFormat {
    SampleFormat {
        bits_allocated: i64::from(metadata.bits_allocated),
        bits_stored: i64::from(metadata.bits_stored),
        high_bit: i64::from(metadata.high_bit),
        signed: metadata.pixel_representation != 0,
        big_endian,
    }
}
