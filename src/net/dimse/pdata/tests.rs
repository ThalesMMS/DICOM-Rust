#![forbid(unsafe_code)]
use super::*;
use crate::object::Tag;
use crate::object::transfer::EXPLICIT_VR_LITTLE_ENDIAN;
fn data_set() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0x08, 0x00, 0x60, 0x00, b'C', b'S', 2, 0]);
    out.extend_from_slice(b"CT");
    out.extend_from_slice(&[0x10, 0x00, 0x10, 0x00, b'P', b'N', 8, 0]);
    out.extend_from_slice(b"Doe^Jane");
    out
}
fn reader(bytes: Vec<u8>) -> PDataReader<'static> {
    PDataReader::with_prefix(None, 1, bytes, true)
}
#[test]
fn a_data_set_is_read_and_the_reader_given_back() {
    let bytes = data_set();
    let total = bytes.len();
    let mut r = reader(bytes);
    let obj = read_pdata_data_set(
        &mut r,
        EXPLICIT_VR_LITTLE_ENDIAN,
        ReadFileOptions::default(),
    );
    let obj = obj.unwrap_or_else(|e| panic!("read: {e}"));
    assert_eq!(
        obj.get_string(Tag::new(0x0008, 0x0060)).as_deref(),
        Some("CT")
    );
    assert_eq!(
        obj.get_string(Tag::new(0x0010, 0x0010)).as_deref(),
        Some("Doe^Jane")
    );
    assert_eq!(r.received_bytes(), total);
    let mut rest = [0u8; 4];
    assert_eq!(r.read_bytes(&mut rest).ok(), Some(0));
}
#[test]
fn a_truncated_data_set_fails_and_the_reader_is_given_back() {
    let mut bytes = data_set();
    bytes.truncate(bytes.len() - 3);
    let total = bytes.len();
    let mut r = reader(bytes);
    let got = read_pdata_data_set(
        &mut r,
        EXPLICIT_VR_LITTLE_ENDIAN,
        ReadFileOptions::default(),
    );
    assert!(got.is_err());
    assert_eq!(r.received_bytes(), total);
}
#[test]
fn the_wire_limited_read_fits_and_overflows() {
    let bytes = data_set();
    let total = bytes.len() as i64;
    let mut r = reader(bytes.clone());
    let opts = ReadFileOptions::default();
    let obj = read_pdata_data_set_wire_limited(&mut r, EXPLICIT_VR_LITTLE_ENDIAN, opts, total);
    let obj = obj.unwrap_or_else(|e| panic!("exact fit: {e}"));
    assert_eq!(
        obj.get_string(Tag::new(0x0010, 0x0010)).as_deref(),
        Some("Doe^Jane")
    );
    let mut r = reader(bytes);
    let got = read_pdata_data_set_wire_limited(&mut r, EXPLICIT_VR_LITTLE_ENDIAN, opts, total - 4);
    match got {
        Err(e) => assert!(e.to_string().contains(INTERLEAVED_WIRE_LIMIT_TEXT), "{e}"),
        Ok(_) => panic!("a data set over the wire budget was accepted"),
    }
}
