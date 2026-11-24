#![no_main]
extern crate dicom_rust_fuzz_harness as _;
use dicom_rust::pixel::Metadata;
use dicom_rust::pixel::guard::{Family, check_stream, jpeg2000_siz};
use libfuzzer_sys::fuzz_target;
const JP2: [u8; 12] = [0, 0, 0, 12, b'j', b'P', b' ', b' ', 0x0D, 0x0A, 0x87, 0x0A];
fuzz_target!(|data: &[u8]| {
    let _ = jpeg2000_siz(data);
    let mut raw = vec![0xFF, 0x4F, 0xFF, 0x51];
    raw.extend_from_slice(data);
    let _ = jpeg2000_siz(&raw);
    let mut boxed = JP2.to_vec();
    boxed.extend_from_slice(data);
    let _ = jpeg2000_siz(&boxed);
    let m = Metadata {
        rows: 300,
        columns: 200,
        samples_per_pixel: 3,
        bits_allocated: 8,
        bits_stored: 8,
        number_of_frames: 1,
        ..Metadata::default()
    };
    let _ = check_stream(Family::Jpeg2000, &raw, &m, 0);
});
