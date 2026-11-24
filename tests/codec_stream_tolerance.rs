use dicom_rust::codec::jpeg2000::encoder as j2k_encoder;
use dicom_rust::codec::jpeg2000::j2kcore;
use dicom_rust::codec::jpeglossless::Metadata;
use dicom_rust::codec::jpegls;
fn metadata(rows: u16, columns: u16) -> Metadata {
    Metadata {
        rows,
        columns,
        samples_per_pixel: 1,
        bits_allocated: 8,
        bits_stored: 8,
        high_bit: 7,
        pixel_representation: 0,
        planar_configuration: 0,
        planar_configuration_present: false,
        number_of_frames: 1,
        photometric_interpretation: "MONOCHROME2".into(),
    }
}
fn pixels(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 37 % 251) as u8).collect()
}
fn position(stream: &[u8], marker: [u8; 2]) -> usize {
    stream
        .windows(2)
        .position(|w| w == marker)
        .expect("marker in the stream")
}
fn decode_j2k(stream: &[u8]) -> Result<Vec<i32>, String> {
    let codestream = j2kcore::parse_codestream(stream).map_err(|e| e.to_string())?;
    let image = j2kcore::decode(
        &codestream,
        j2kcore::Limits::defaults(),
        j2kcore::DecodeOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    Ok(image.components.into_iter().next().unwrap_or_default())
}
#[test]
fn jpeg2000_reserved_and_profile_markers_are_stepped_over() {
    let (rows, columns) = (16u16, 16u16);
    let source = pixels(usize::from(rows) * usize::from(columns));
    let encoder = j2k_encoder::Encoder::new(&j2k_encoder::Options {
        transfer_syntax_uid: "1.2.840.10008.1.2.4.90".into(),
        ..Default::default()
    })
    .expect("encoder");
    let plain = encoder
        .encode_frame(&source, &metadata(rows, columns))
        .expect("encode")
        .data;
    let want: Vec<i32> = source.iter().map(|v| i32::from(*v)).collect();
    assert_eq!(decode_j2k(&plain).as_deref(), Ok(want.as_slice()));
    let sot = position(&plain, [0xFF, 0x90]);
    let mut main = plain[..sot].to_vec();
    main.extend_from_slice(&[0xFF, 0x30, 0xFF, 0x59, 0x00, 0x04, 0x00, 0x00, 0xFF, 0x3F]);
    main.extend_from_slice(&plain[sot..]);
    assert_eq!(decode_j2k(&main).as_deref(), Ok(want.as_slice()));
    let sod = position(&plain, [0xFF, 0x93]);
    let mut tile = plain[..sod].to_vec();
    tile.extend_from_slice(&[0xFF, 0x31]);
    tile.extend_from_slice(&plain[sod..]);
    let psot = u32::from_be_bytes([tile[sot + 6], tile[sot + 7], tile[sot + 8], tile[sot + 9]]);
    if psot != 0 {
        tile[sot + 6..sot + 10].copy_from_slice(&(psot + 2).to_be_bytes());
    }
    assert_eq!(decode_j2k(&tile).as_deref(), Ok(want.as_slice()));
    let mut unknown = plain[..sot].to_vec();
    unknown.extend_from_slice(&[0xFF, 0x7A, 0x00, 0x02]);
    unknown.extend_from_slice(&plain[sot..]);
    assert_eq!(
        decode_j2k(&unknown),
        Err(format!("jpeg2000: unexpected marker FF7A at offset {sot}"))
    );
}
#[test]
fn jpegls_end_of_scan_padding() {
    let (rows, columns) = (8u16, 8u16);
    let m = metadata(rows, columns);
    let source = pixels(usize::from(rows) * usize::from(columns));
    let plain = jpegls::Encoder::new()
        .encode_frame(&source, &m)
        .expect("encode");
    assert!(plain.ends_with(&[0xFF, 0xD9]), "the stream ends with EOI");
    let entropy_end = plain.len() - 2;
    assert_ne!(plain[entropy_end - 1], 0xFF, "the fixture ends on a marker");
    let decode = |stream: &[u8]| {
        jpegls::Codec::lossless()
            .decode_frames(&[stream], &m, Default::default())
            .map(|frames| frames.data)
            .map_err(|e| e.to_string())
    };
    let with = |extra: &[u8]| {
        let mut stream = plain[..entropy_end].to_vec();
        stream.extend_from_slice(extra);
        stream.extend_from_slice(&[0xFF, 0xD9]);
        stream
    };
    assert_eq!(decode(&plain), Ok(vec![source.clone()]));
    assert_eq!(decode(&with(&[0x00])), Ok(vec![source.clone()]));
    for extra in [&[0x00, 0x00][..], &[0x01][..]] {
        let refused = decode(&with(extra)).expect_err("extra data after the last sample");
        assert!(refused.contains("trailing entropy data"), "{refused}");
    }
}
#[test]
fn jpegxl_lossless_streams_carry_no_restoration_filters() {
    use dicom_rust::codec::jpegxl;
    let image = jpegxl::Image {
        width: 9,
        height: 7,
        channels: 1,
        bits_per_sample: 12,
        samples: (0..63).map(|i| i * 61 % 4096).collect(),
    };
    let stream = jpegxl::encode_lossless(&image, Default::default()).expect("encode");
    assert_eq!(jpegxl::decode(&stream).expect("decode"), image);
}
