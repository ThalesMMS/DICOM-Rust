use super::*;
fn mono16(rows: u16, columns: u16, frames: i64) -> Metadata {
    Metadata {
        rows,
        columns,
        samples_per_pixel: 1,
        bits_allocated: 16,
        bits_stored: 12,
        high_bit: 11,
        number_of_frames: frames,
        photometric_interpretation: "MONOCHROME2".into(),
        ..Metadata::default()
    }
}
fn mono8(rows: u16, columns: u16) -> Metadata {
    Metadata {
        bits_allocated: 8,
        bits_stored: 8,
        high_bit: 7,
        ..mono16(rows, columns, 1)
    }
}
#[test]
fn attribute_refusals_have_their_own_messages() {
    let l = GuardLimits::DEFAULT;
    let cases = [
        mono16(0, 256, 1),
        mono16(16_385, 256, 1),
        mono16(256, 256, 20_000),
        Metadata {
            samples_per_pixel: 2,
            ..mono16(256, 256, 1)
        },
        Metadata {
            bits_stored: 17,
            ..mono16(256, 256, 1)
        },
    ];
    let messages: Vec<String> = cases
        .iter()
        .map(|m| match check_attributes(m, &l) {
            Err(e) => e.to_string(),
            Ok(()) => String::from("accepted"),
        })
        .collect();
    assert_eq!(
        messages,
        [
            "the image has no rows (Rows is 0)",
            "the image is 16385 pixels high; the limit is 16384",
            "the image has 20000 frames; the limit is 10000",
            "Samples per Pixel 2 is not supported (1, 3, or 4 for ARGB)",
            "Bits Stored 17 must be between 1 and Bits Allocated (16)",
        ]
    );
    assert!(check_attributes(&mono16(512, 512, 100), &l).is_ok());
    let twelve = Metadata {
        bits_allocated: 12,
        bits_stored: 12,
        ..mono16(8, 8, 1)
    };
    assert!(matches!(
        check_attributes(&twelve, &l),
        Err(GuardError::UnsupportedBitsAllocated(12))
    ));
    let wide = Metadata {
        samples_per_pixel: 3,
        ..mono16(8192, 8192, 1)
    };
    assert!(matches!(
        check_attributes(&wide, &l),
        Err(GuardError::TooManySamples { .. })
    ));
    assert!(matches!(
        check_series_bytes(estimated_decoded_bytes(&mono16(1024, 1024, 1100)), &l),
        Err(GuardError::SeriesTooLarge { .. })
    ));
}
#[test]
fn native_length_allows_one_byte_either_way() {
    let m = mono16(16, 16, 2);
    let expected = 16 * 16 * 2 * 2;
    let verdicts: Vec<bool> = (-2..=2)
        .map(|d| check_native_length(&m, expected + d, false).is_ok())
        .collect();
    assert_eq!(verdicts, [false, true, true, true, false]);
    assert!(check_native_length(&m, expected + 64, true).is_ok());
    assert!(check_native_length(&m, expected - 2, true).is_err());
    let bits = Metadata {
        bits_allocated: 1,
        bits_stored: 1,
        high_bit: 0,
        ..mono16(3, 3, 1)
    };
    assert!(check_native_length(&bits, 2, false).is_ok());
    assert!(check_native_length(&bits, 4, false).is_err());
}
fn jpeg(marker: u8, precision: u8, height: u16, width: u16, components: u8) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, marker];
    out.extend_from_slice(&(8 + 3 * u16::from(components)).to_be_bytes());
    out.push(precision);
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&width.to_be_bytes());
    out.push(components);
    for c in 0..components {
        out.extend_from_slice(&[c + 1, 0x11, 0]);
    }
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
    out
}
fn j2k(width: u32, height: u32, components: u16, precision: u8, wrapped: bool) -> Vec<u8> {
    let mut cs = vec![0xFF, 0x4F, 0xFF, 0x51];
    cs.extend_from_slice(&(38 + 3 * components).to_be_bytes());
    cs.extend_from_slice(&[0, 0]);
    for v in [width, height, 0, 0, width, height, 0, 0] {
        cs.extend_from_slice(&v.to_be_bytes());
    }
    cs.extend_from_slice(&components.to_be_bytes());
    for _ in 0..components {
        cs.extend_from_slice(&[precision - 1, 1, 1]);
    }
    cs.extend_from_slice(&[0xFF, 0xD9]);
    if !wrapped {
        return cs;
    }
    let mut out = JP2_SIGNATURE.to_vec();
    out.extend_from_slice(&20u32.to_be_bytes());
    out.extend_from_slice(b"ftypjp2 \0\0\0\0jp2 ");
    out.extend_from_slice(&(8 + cs.len() as u32).to_be_bytes());
    out.extend_from_slice(b"jp2c");
    out.extend_from_slice(&cs);
    out
}
#[test]
fn stream_larger_than_the_attributes_is_refused() {
    let m = mono8(256, 256);
    let streams = [
        (Family::Jpeg, jpeg(0xC0, 8, 512, 512, 1)),
        (Family::JpegLs, jpeg(0xF7, 8, 512, 512, 1)),
        (Family::Jpeg2000, j2k(512, 512, 1, 8, false)),
        (Family::Jpeg2000, j2k(512, 512, 1, 8, true)),
    ];
    for (family, data) in &streams {
        let Err(err) = check_stream(*family, data, &m, 0) else {
            panic!("{family:?}: accepted");
        };
        assert_eq!(
            err.to_string(),
            "the image data does not match its description in the file (the compressed stream is larger)"
        );
        assert!(
            err.details().contains("stream 512x512"),
            "{}",
            err.details()
        );
    }
    let small = check_stream(Family::Jpeg, &jpeg(0xC0, 8, 128, 256, 1), &m, 0);
    assert!(matches!(
        small,
        Err(GuardError::Mismatch {
            larger: Larger::Attributes,
            ..
        })
    ));
    assert!(check_stream(Family::Jpeg, &jpeg(0xC0, 8, 256, 256, 1), &m, 0).is_ok());
    assert!(check_stream(Family::JpegLs, &jpeg(0xF7, 8, 256, 256, 1), &m, 0).is_ok());
    assert!(check_stream(Family::Jpeg2000, &j2k(256, 256, 1, 8, true), &m, 0).is_ok());
    assert!(check_stream(Family::Jpeg, &jpeg(0xC3, 16, 256, 256, 1), &m, 0).is_err());
    let m16 = mono16(256, 256, 1);
    assert!(check_stream(Family::Jpeg, &jpeg(0xC3, 16, 256, 256, 1), &m16, 0).is_ok());
    assert!(check_stream(Family::Jpeg, &jpeg(0xC1, 12, 256, 256, 1), &m16, 0).is_ok());
    assert!(check_stream(Family::Jpeg, &jpeg(0xC1, 10, 256, 256, 1), &m16, 0).is_err());
}
#[test]
fn six_hundred_segments_without_a_frame_header_are_refused() {
    let mut data = vec![0xFF, 0xD8];
    for _ in 0..600 {
        data.extend_from_slice(&[0xFF, 0xE1, 0x00, 0x02]);
    }
    assert_eq!(
        jpeg_frame_header(&data),
        Err(String::from("no frame header within 512 marker segments"))
    );
    let past = [0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF];
    assert!(jpeg_frame_header(&past).is_err());
}
#[test]
fn truncated_and_mutated_headers_never_panic() {
    let samples = [
        jpeg(0xC0, 8, 256, 256, 3),
        jpeg(0xF7, 16, 512, 512, 1),
        j2k(300, 200, 3, 12, false),
        j2k(300, 200, 3, 12, true),
    ];
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let m = mono16(200, 300, 1);
    for data in &samples {
        for cut in 0..=data.len() {
            let _ = jpeg_frame_header(&data[..cut]);
            let _ = jpeg2000_siz(&data[..cut]);
        }
        for _ in 0..2000 {
            let mut d = data.clone();
            for _ in 0..3 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let at = (state % d.len() as u64) as usize;
                d[at] = (state >> 32) as u8;
            }
            let cut = ((state >> 20) % (d.len() as u64 + 1)) as usize;
            let _ = jpeg_frame_header(&d[..cut]);
            let _ = jpeg2000_siz(&d[..cut]);
            let _ = check_stream(Family::Jpeg, &d[..cut], &m, 0);
            let _ = check_stream(Family::Jpeg2000, &d[..cut], &m, 0);
        }
    }
}
#[test]
fn settings_are_clamped_between_a_tenth_and_four_times() {
    let d = GuardLimits::DEFAULT;
    let huge = GuardLimits {
        max_side: d.max_side * 100,
        max_samples: d.max_samples * 100,
        max_frames: d.max_frames * 100,
        max_frame_bytes: d.max_frame_bytes * 100,
        max_series_bytes: d.max_series_bytes * 100,
    }
    .clamped();
    assert_eq!(
        huge,
        GuardLimits {
            max_side: 65_536,
            max_samples: 268_435_456,
            max_frames: 40_000,
            max_frame_bytes: 2 << 30,
            max_series_bytes: 8 << 30,
        }
    );
    let zero = GuardLimits {
        max_side: 0,
        max_samples: 0,
        max_frames: 0,
        max_frame_bytes: 0,
        max_series_bytes: 0,
    }
    .clamped();
    assert_eq!(
        zero,
        GuardLimits {
            max_side: 1638,
            max_samples: 6_710_886,
            max_frames: 1000,
            max_frame_bytes: (512 << 20) / 10,
            max_series_bytes: (2 << 30) / 10,
        }
    );
    assert_eq!(GuardLimits::DEFAULT.clamped(), GuardLimits::DEFAULT);
}
#[test]
fn families() {
    assert_eq!(family("1.2.840.10008.1.2.4.50"), Some(Family::Jpeg));
    assert_eq!(family("1.2.840.10008.1.2.4.70\0"), Some(Family::Jpeg));
    assert_eq!(family("1.2.840.10008.1.2.4.80"), Some(Family::JpegLs));
    assert_eq!(family("1.2.840.10008.1.2.4.202"), Some(Family::Jpeg2000));
    assert_eq!(family("1.2.840.10008.1.2.5"), None);
    assert_eq!(family("1.2.840.10008.1.2.4.110"), None);
}
