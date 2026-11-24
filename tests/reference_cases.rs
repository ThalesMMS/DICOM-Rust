#![allow(clippy::too_many_arguments, clippy::type_complexity)]
#[path = "reference_cases/charset.rs"]
mod charset;
#[path = "reference_cases/charset_encode.rs"]
mod charset_encode;
#[path = "reference_cases/deflate.rs"]
mod deflate;
#[path = "reference_cases/deid.rs"]
mod deid;
#[path = "reference_cases/dicomdir_refs.rs"]
mod dicomdir_refs;
#[path = "reference_cases/display_frame.rs"]
mod display_frame;
#[cfg(feature = "dsig")]
#[path = "reference_cases/dsig_sign.rs"]
mod dsig_sign;
#[cfg(feature = "dsig")]
#[path = "reference_cases/dsig_verify.rs"]
mod dsig_verify;
#[cfg(feature = "jpeg")]
#[path = "reference_cases/jpeg.rs"]
mod jpeg;
#[cfg(feature = "jpeg2000")]
#[path = "reference_cases/jpeg2000.rs"]
mod jpeg2000;
#[cfg(feature = "jpeg-lossless")]
#[path = "reference_cases/jpeg_lossless.rs"]
mod jpeg_lossless;
#[cfg(feature = "jpeg-ls")]
#[path = "reference_cases/jpegls.rs"]
mod jpegls;
#[cfg(feature = "jpeg-xl")]
#[path = "reference_cases/jpegxl_lossless.rs"]
mod jpegxl_lossless;
#[path = "reference_cases/json_stream.rs"]
mod json_stream;
#[path = "reference_cases/person_name.rs"]
mod person_name;
#[path = "reference_cases/read_private.rs"]
mod read_private;
#[path = "reference_cases/rle.rs"]
mod rle;
#[cfg(feature = "codes")]
#[path = "reference_cases/sr_extract.rs"]
mod sr_extract;
#[path = "reference_cases/support.rs"]
mod support;
#[cfg(feature = "codecs")]
#[path = "reference_cases/transcode.rs"]
mod transcode;
#[path = "reference_cases/transfer_syntax.rs"]
mod transfer_syntax;
#[path = "reference_cases/write.rs"]
mod write;
#[test]
fn read_private_cases() {
    support::run("read_private", read_private::call);
}
#[test]
fn write_cases() {
    support::run("write", write::call);
}
#[test]
fn deflate_cases() {
    support::run("deflate", deflate::call);
}
#[test]
fn charset_cases() {
    support::run("charset", charset::call);
}
#[test]
fn charset_encode_cases() {
    support::run("charset_encode", charset_encode::call);
}
#[test]
fn rle_cases() {
    support::run("rle", rle::call);
}
#[cfg(feature = "jpeg")]
#[test]
fn jpeg_cases() {
    support::run("jpeg", jpeg::call);
}
#[cfg(feature = "jpeg-ls")]
#[test]
fn jpegls_cases() {
    support::run("jpegls", jpegls::call);
}
#[cfg(feature = "jpeg-lossless")]
#[test]
fn jpeg_lossless_cases() {
    support::run("jpeg_lossless", jpeg_lossless::call);
}
#[cfg(feature = "jpeg2000")]
#[test]
fn jpeg2000_cases() {
    support::run("jpeg2000", jpeg2000::call);
}
#[cfg(feature = "jpeg-xl")]
#[test]
fn jpegxl_lossless_cases() {
    support::run("jpegxl_lossless", jpegxl_lossless::call);
}
#[cfg(feature = "codecs")]
#[test]
fn transcode_cases() {
    support::run("transcode", transcode::call);
}
#[test]
fn dicomdir_refs_cases() {
    support::run("dicomdir_refs", dicomdir_refs::call);
}
#[test]
fn json_stream_cases() {
    support::run("json_stream", json_stream::call);
}
#[test]
fn deid_cases() {
    support::run("deid", deid::call);
}
#[cfg(feature = "dsig")]
#[test]
fn dsig_verify_cases() {
    support::run("dsig_verify", dsig_verify::call);
}
#[cfg(feature = "dsig")]
#[test]
fn dsig_sign_cases() {
    support::run("dsig_sign", dsig_sign::call);
}
#[cfg(feature = "codes")]
#[test]
fn sr_extract_cases() {
    support::run("sr_extract", sr_extract::call);
}
#[test]
fn display_frame_cases() {
    support::run("display_frame", display_frame::call);
}
#[test]
fn person_name_cases() {
    support::run("person_name", person_name::call);
}
#[test]
fn transfer_syntax_cases() {
    support::run("transfer_syntax", transfer_syntax::call);
}
