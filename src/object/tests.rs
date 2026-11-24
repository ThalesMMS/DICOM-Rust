#![forbid(unsafe_code)]
use super::*;
fn le_elem(group: u16, element: u16, vr: &str, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&group.to_le_bytes());
    out.extend_from_slice(&element.to_le_bytes());
    out.extend_from_slice(vr.as_bytes());
    if matches!(vr, "OB" | "OW" | "SQ" | "UN" | "UT") {
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    } else {
        out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    }
    out.extend_from_slice(value);
    out
}
fn part10(ts: &str, dataset: &[u8]) -> Vec<u8> {
    let mut uid = ts.as_bytes().to_vec();
    if uid.len() % 2 == 1 {
        uid.push(0);
    }
    let body = [le_elem(2, 1, "OB", &[0, 1]), le_elem(2, 0x10, "UI", &uid)].concat();
    let mut out = vec![0u8; 128];
    out.extend_from_slice(b"DICM");
    out.extend_from_slice(&le_elem(2, 0, "UL", &(body.len() as u32).to_le_bytes()));
    out.extend_from_slice(&body);
    out.extend_from_slice(dataset);
    out
}
fn item(content: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFE, 0xFF, 0x00, 0xE0];
    out.extend_from_slice(&(content.len() as u32).to_le_bytes());
    out.extend_from_slice(content);
    out
}
#[test]
fn reads_explicit_little_endian_with_sequence() {
    let ds = [
        le_elem(0x0008, 0x0060, "CS", b"OT"),
        le_elem(
            0x0008,
            0x1140,
            "SQ",
            &item(&le_elem(0x0008, 0x1150, "UI", b"1.2\0")),
        ),
        le_elem(0x0010, 0x0010, "PN", b"Doe^"),
    ]
    .concat();
    let file = read_file(&part10("1.2.840.10008.1.2.1", &ds)).expect("valid file");
    assert_eq!(file.transfer_syntax.uid, "1.2.840.10008.1.2.1");
    assert_eq!(file.meta.elements().len(), 3);
    let elements = file.dataset.elements();
    assert_eq!(elements.len(), 3);
    match &elements[1].value {
        Value::Sequence(items) => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].elements[0].raw(), Some(&b"1.2\0"[..]));
        }
        other => panic!("unexpected {other:?}"),
    }
}
#[test]
fn reports_missing_preamble_and_unknown_syntax() {
    let err = read_file(b"short").expect_err("too short").1;
    assert_eq!(err.to_string(), "unexpected EOF");
    let err = read_file(&[0u8; 132]).expect_err("no DICM").1;
    assert_eq!(
        err.to_string(),
        "dicom: missing Part 10 preamble or DICM marker"
    );
    let err = read_file(&part10("1.2.3", &[])).expect_err("unknown").1;
    assert_eq!(err.to_string(), "dicom: unknown transfer syntax: \"1.2.3\"");
}
#[test]
fn odd_length_is_rejected_unless_accepted() {
    let data = part10(
        "1.2.840.10008.1.2.1",
        &le_elem(0x0010, 0x0020, "LO", b"ABC"),
    );
    let err = read_file(&data).expect_err("odd length").1;
    assert!(
        err.to_string()
            .ends_with("dicom: element length must be even: got 3"),
        "{err}"
    );
    let opts = ReadFileOptions {
        accept_odd_length: true,
        ..Default::default()
    };
    let file = read_file_with_options(&data, opts).expect("accepted");
    assert_eq!(file.dataset.elements()[0].raw(), Some(&b"ABC"[..]));
}
#[test]
fn truncated_value_reports_offset() {
    let mut data = part10(
        "1.2.840.10008.1.2.1",
        &le_elem(0x0010, 0x0020, "LO", b"ABCD"),
    );
    data.truncate(data.len() - 2);
    let err = read_file(&data).expect_err("truncated").1;
    let offset = data.len() - 2;
    assert_eq!(
        err.to_string(),
        format!(
            "dicom: data set error: dicom: read value at offset {offset} for tag (0010,0020) with VR LO with length 4: unexpected EOF"
        )
    );
}
#[test]
fn partial_read_keeps_completed_elements() {
    let mut data = part10(
        "1.2.840.10008.1.2.1",
        &[
            le_elem(0x0008, 0x0060, "CS", b"OT"),
            le_elem(0x0010, 0x0020, "LO", b"ABCD"),
        ]
        .concat(),
    );
    data.truncate(data.len() - 1);
    let opts = ReadFileOptions {
        allow_partial_read: true,
        ..Default::default()
    };
    let (file, err) = read_file_with_options(&data, opts).expect_err("partial");
    let file = file.expect("partial file");
    assert!(err.is(Kind::PartialRead));
    assert_eq!(file.dataset.elements().len(), 1);
    assert_eq!(file.partial_read.as_ref().map(|p| p.elements), Some(1));
}
#[test]
fn implicit_vr_uses_dictionary_and_relabels_xs() {
    let mut ds = Vec::new();
    for (tag, value) in [
        ((0x0028u16, 0x0103u16), 1u16.to_le_bytes()),
        ((0x0028, 0x0106), 0xFFFBu16.to_le_bytes()),
    ] {
        ds.extend_from_slice(&tag.0.to_le_bytes());
        ds.extend_from_slice(&tag.1.to_le_bytes());
        ds.extend_from_slice(&2u32.to_le_bytes());
        ds.extend_from_slice(&value);
    }
    let file = read_file(&part10("1.2.840.10008.1.2", &ds)).expect("implicit");
    let vrs: Vec<Vr> = file.dataset.elements().iter().map(|e| e.vr()).collect();
    assert_eq!(vrs, vec![Vr::US, Vr::SS]);
}
#[test]
fn inflates_go_compatible_errors() {
    let (out, err, _) = inflate::inflate_raw(&[0x03, 0x00]);
    assert!(out.is_empty());
    assert!(err.is_exactly(Kind::Eof));
    let (_, err, _) = inflate::inflate_raw(&[0xFF]);
    assert_eq!(err.to_string(), "flate: corrupt input before offset 1");
}
