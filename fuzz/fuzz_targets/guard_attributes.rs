#![no_main]
extern crate dicom_rust_fuzz_harness as _;
use dicom_rust::pixel::Metadata;
use dicom_rust::pixel::guard::{
    GuardLimits, check_attributes, check_native_length, check_series_bytes, estimated_decoded_bytes,
};
use libfuzzer_sys::fuzz_target;
fn u16_at(d: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([
        d.get(i).copied().unwrap_or(0),
        d.get(i + 1).copied().unwrap_or(0),
    ])
}
fn i64_at(d: &[u8], i: usize) -> i64 {
    let mut b = [0u8; 8];
    for (k, v) in b.iter_mut().enumerate() {
        *v = d.get(i + k).copied().unwrap_or(0);
    }
    i64::from_le_bytes(b)
}
fuzz_target!(|d: &[u8]| {
    let m = Metadata {
        rows: u16_at(d, 0),
        columns: u16_at(d, 2),
        samples_per_pixel: u16_at(d, 4),
        bits_allocated: u16_at(d, 6),
        bits_stored: u16_at(d, 8),
        high_bit: u16_at(d, 10),
        number_of_frames: i64_at(d, 12),
        photometric_interpretation: if d.get(20).is_some_and(|b| b & 1 == 1) {
            "YBR_FULL_422".into()
        } else {
            "MONOCHROME2".into()
        },
        ..Metadata::default()
    };
    let limits = GuardLimits {
        max_side: i64_at(d, 21),
        max_samples: i64_at(d, 29),
        max_frames: i64_at(d, 37),
        max_frame_bytes: i64_at(d, 45),
        max_series_bytes: i64_at(d, 53),
    }
    .clamped();
    let _ = check_attributes(&m, &limits);
    let _ = check_series_bytes(estimated_decoded_bytes(&m), &limits);
    let _ = check_native_length(&m, i64_at(d, 61), d.get(69).is_some_and(|b| b & 1 == 1));
});
