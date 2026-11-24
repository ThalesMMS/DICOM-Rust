#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| dicom_rust_fuzz_harness::dataset_explicit_be(data));
