#![no_main]
extern crate dicom_rust_fuzz_harness as _;
use dicom_rust::pixel::Metadata;
use dicom_rust::pixel::guard::{Family, check_stream, jpeg_frame_header};
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let mut stream = vec![0xFF, 0xD8, 0xFF, 0xF7];
    stream.extend_from_slice(data);
    let _ = jpeg_frame_header(&stream);
    let _ = jpeg_frame_header(data);
    let m = Metadata {
        rows: 512,
        columns: 512,
        samples_per_pixel: 1,
        bits_allocated: 16,
        bits_stored: 16,
        number_of_frames: 1,
        ..Metadata::default()
    };
    let _ = check_stream(Family::JpegLs, &stream, &m, 0);
});
